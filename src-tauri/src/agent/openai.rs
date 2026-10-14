use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::thread::spawn;
use std::time::Duration;

use serde_json::{Value, json};

use super::secrets::{self, Secret};
use super::{Event, POLL, Running, Send, Status, Turn, keep, web};
use crate::{extract, workspace};

const OPENROUTER: &str = "https://openrouter.ai/api/v1";
const MAX_STEPS: usize = 24;
const MAX_CALLS: usize = 32;
const MAX_OUTPUT: usize = 60_000;
const MAX_PIECE: usize = MAX_OUTPUT / 4 - 100;
const MAX_HISTORY: usize = 400_000;
const MAX_HITS: usize = 100;
const MAX_MODELS_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Default)]
struct Call {
    id: String,
    name: String,
    arguments: String,
}

#[derive(Default)]
struct Reply {
    text: String,
    calls: Vec<Call>,
    done: bool,
}

pub struct Hosted {
    pub id: &'static str,
    address: Option<&'static str>,
    key_check: Option<&'static str>,
    key_missing: Option<&'static str>,
}

pub static HOSTED: [Hosted; 2] = [
    Hosted {
        id: "openrouter",
        address: Some(OPENROUTER),
        key_check: Some("/key"),
        key_missing: Some("Paste your OpenRouter API key first."),
    },
    Hosted {
        id: "custom",
        address: None,
        key_check: None,
        key_missing: None,
    },
];

pub fn hosted(provider: &str) -> Result<&'static Hosted, String> {
    HOSTED
        .iter()
        .find(|hosted| hosted.id == provider)
        .ok_or_else(|| format!("“{provider}” is not a provider znote knows."))
}

fn typed(url: Option<&str>) -> Result<String, String> {
    let url = url
        .unwrap_or_default()
        .trim()
        .trim_end_matches('/')
        .trim_end_matches("/chat/completions");
    let host = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .unwrap_or_default();
    if host.is_empty()
        || url.len() > 512
        || url.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err("Type the address of the server, for example http://localhost:11434/v1".into());
    }
    Ok(url.into())
}

impl Hosted {
    fn secret(&self, url: Option<&str>, key: &str) -> Result<Secret, String> {
        let key = key.trim();
        if key.len() > 512 || !key.chars().all(|c| c.is_ascii_graphic()) {
            return Err("That does not look like an API key. Copy it again.".into());
        }
        if let Some(missing) = self.key_missing.filter(|_| key.is_empty()) {
            return Err(missing.into());
        }
        let url = match self.address {
            Some(address) => address.into(),
            None => typed(url)?,
        };
        Ok(Secret {
            url,
            key: key.into(),
        })
    }

    pub fn connect(&self, url: Option<&str>, key: &str) -> Result<Vec<String>, String> {
        let secret = self.secret(url, key)?;
        if let Some(path) = self.key_check {
            request("GET", &secret, path)?.call().map_err(refused)?;
        }
        let models = models(&secret)?;
        secrets::write(self.id, &secret)?;
        Ok(models)
    }

    pub fn models(&self) -> Result<Vec<String>, String> {
        match secrets::read(self.id)? {
            Some(secret) => models(&secret),
            None => Ok(Vec::new()),
        }
    }

    pub fn status(&self) -> Result<Status, String> {
        let secret = secrets::read(self.id)?;
        Ok(Status {
            installed: true,
            connected: secret.is_some(),
            url: secret.map(|secret| secret.url),
        })
    }

    pub fn disconnect(&self) -> Result<Status, String> {
        secrets::clear(self.id)?;
        Ok(Status {
            installed: true,
            connected: false,
            url: None,
        })
    }

    pub fn run(
        &self,
        running: &Running,
        root: &Path,
        folder: &Path,
        turn: &Turn,
        send: Send,
    ) -> Result<(), String> {
        let secret = secrets::read(self.id)?.ok_or("Connect the assistant in Settings first.")?;
        run(running, root, folder, turn, &secret, send)
    }
}

pub fn connection() -> Result<ureq::AgentBuilder, String> {
    let tls = native_tls::TlsConnector::new()
        .map_err(|error| format!("A secure connection could not be set up: {error}"))?;
    Ok(ureq::AgentBuilder::new()
        .tls_connector(Arc::new(tls))
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(600))
        .user_agent(concat!("znote/", env!("CARGO_PKG_VERSION"))))
}

fn request(method: &str, secret: &Secret, path: &str) -> Result<ureq::Request, String> {
    let request = connection()?
        .build()
        .request(method, &format!("{}{path}", secret.url))
        .set("X-Title", "znote");
    Ok(if secret.key.is_empty() {
        request
    } else {
        request.set("Authorization", &format!("Bearer {}", secret.key))
    })
}

pub fn refused(error: ureq::Error) -> String {
    match error {
        ureq::Error::Status(code, response) => {
            let body: Value = response
                .into_string()
                .ok()
                .and_then(|body| serde_json::from_str(&body).ok())
                .unwrap_or_default();
            let said = body["error"]["message"]
                .as_str()
                .or(body["error"].as_str())
                .unwrap_or_default();
            let reason = match code {
                401 | 403 => "The API key was not accepted.",
                402 => "The account has no credit left.",
                404 => "The server does not know this address or model.",
                429 => "Too many requests. Wait a moment and try again.",
                _ => "The server refused the request.",
            };
            format!("{reason} {said}").trim_end().to_string()
        }
        ureq::Error::Transport(error) => format!("The server could not be reached: {error}"),
    }
}

fn models(secret: &Secret) -> Result<Vec<String>, String> {
    let response = request("GET", secret, "/models")?.call().map_err(refused)?;
    let body: Value = serde_json::from_reader(response.into_reader().take(MAX_MODELS_BYTES))
        .map_err(|_| "The server did not send a list of models. Check the address.".to_string())?;
    let mut ids: Vec<String> = body["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|model| model["id"].as_str().map(str::to_string))
        .collect();
    ids.sort();
    Ok(ids)
}

fn tools(read_only: bool, web: bool) -> Value {
    let tool = |name: &str, description: &str, fields: &[(&str, &str)]| {
        let properties: serde_json::Map<String, Value> = fields
            .iter()
            .map(|(field, about)| {
                (
                    field.to_string(),
                    json!({ "type": "string", "description": about }),
                )
            })
            .collect();
        let required: Vec<&str> = fields.iter().map(|(field, _)| *field).collect();
        json!({ "type": "function", "function": { "name": name, "description": description, "parameters": { "type": "object", "properties": properties, "required": required } } })
    };
    let path = ("path", "Project/Notebook/Page, without .typ");
    let mut tools = vec![
        tool("list_pages", "List every page in the workspace.", &[]),
        tool("read_page", "Read the Typst source of a page.", &[path]),
        tool(
            "search_pages",
            "Find lines that contain a text, in every page.",
            &[("query", "The text to look for")],
        ),
        tool(
            "read_file",
            "Read the text of a file the user attached to this chat: plain text, PDF, Word, PowerPoint, Excel or OpenDocument.",
            &[
                ("name", "The file name"),
                ("start", "The character to start at. Use 0 first."),
            ],
        ),
    ];
    if web {
        tools.push(tool("fetch_url", "Read the text of a public web page. Use it only for an address the user gave or asked for.", &[("url", "The https:// address")]));
    }
    if !read_only {
        tools.push(tool("write_page", "Write a whole page. Makes the page, its notebook and its project when they are missing.", &[path, ("text", "The full Typst source")]));
        tools.push(tool(
            "edit_page",
            "Replace one piece of text in a page. The old text must appear exactly once.",
            &[
                path,
                ("old", "Text to replace"),
                ("new", "Text to put there"),
            ],
        ));
    }
    Value::Array(tools)
}

fn shown(tool: &str) -> &'static str {
    match tool {
        "read_page" | "read_file" => "Read",
        "write_page" | "edit_page" => "Edit",
        _ => "Search",
    }
}

fn absorb(reply: &mut Reply, line: &str) -> Result<Vec<Event>, String> {
    let Some(data) = line.strip_prefix("data:").map(str::trim) else {
        return Ok(Vec::new());
    };
    if data == "[DONE]" {
        reply.done = true;
        return Ok(Vec::new());
    }
    let Ok(chunk) = serde_json::from_str::<Value>(data) else {
        return Ok(Vec::new());
    };
    if let Some(message) = chunk["error"]["message"].as_str() {
        return Err(message.to_string());
    }
    let delta = &chunk["choices"][0]["delta"];
    let filled = |value: &Value| {
        value
            .as_str()
            .filter(|text| !text.is_empty())
            .map(str::to_string)
    };
    let mut events = Vec::new();
    if let Some(text) = filled(&delta["reasoning"]).or_else(|| filled(&delta["reasoning_content"]))
    {
        events.push(Event::Thought { text });
    }
    if let Some(text) = filled(&delta["content"]) {
        reply.text.push_str(&text);
        events.push(Event::Text { text });
    }
    for part in delta["tool_calls"].as_array().into_iter().flatten() {
        let index = part["index"].as_u64().unwrap_or(0) as usize;
        if index >= MAX_CALLS {
            continue;
        }
        if reply.calls.len() <= index {
            reply.calls.resize_with(index + 1, Call::default);
        }
        let call = &mut reply.calls[index];
        if let Some(id) = filled(&part["id"]) {
            call.id = id;
        }
        if let Some(name) = filled(&part["function"]["name"]).filter(|_| call.name.is_empty()) {
            events.push(Event::Step {
                tool: shown(&name).into(),
                page: String::new(),
            });
            call.name = name;
        }
        call.arguments
            .push_str(part["function"]["arguments"].as_str().unwrap_or_default());
    }
    Ok(events)
}

fn message(reply: &Reply) -> Value {
    let calls: Vec<Value> = reply
        .calls
        .iter()
        .enumerate()
        .map(|(index, call)| {
            let id = if call.id.is_empty() { format!("call_{index}") } else { call.id.clone() };
            let arguments = if call.arguments.trim().is_empty() { "{}" } else { &call.arguments };
            json!({ "id": id, "type": "function", "function": { "name": call.name, "arguments": arguments } })
        })
        .collect();
    if calls.is_empty() {
        return json!({ "role": "assistant", "content": reply.text });
    }
    json!({ "role": "assistant", "content": reply.text, "tool_calls": calls })
}

fn stream(running: &Running, secret: &Secret, body: String, send: Send) -> Result<Reply, String> {
    let request =
        request("POST", secret, "/chat/completions")?.set("Content-Type", "application/json");
    let (lines, incoming) = channel::<Result<String, String>>();
    spawn(move || match request.send_string(&body) {
        Ok(response) => {
            for line in BufReader::new(response.into_reader()).lines() {
                if lines
                    .send(line.map_err(|error| format!("The answer was cut off: {error}")))
                    .is_err()
                {
                    break;
                }
            }
        }
        Err(error) => {
            let _ = lines.send(Err(refused(error)));
        }
    });
    let mut reply = Reply::default();
    while !reply.done && !running.stopped() {
        match incoming.recv_timeout(POLL) {
            Ok(line) => {
                for event in absorb(&mut reply, &line?)? {
                    send(event)?;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(reply)
}

fn page(input: &Value) -> String {
    let path = input["path"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .trim_start_matches('/');
    path.strip_suffix(".typ").unwrap_or(path).to_string()
}

fn search(root: &Path, query: &str) -> Result<String, String> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Err("Give a text to look for.".into());
    }
    let hits: Vec<String> = workspace::pages(root)
        .into_iter()
        .flat_map(|page| {
            let text = workspace::read_text(root, &page).unwrap_or_default();
            let found: Vec<String> = text
                .lines()
                .enumerate()
                .filter(|(_, line)| line.to_lowercase().contains(&query))
                .map(|(number, line)| format!("{page}.typ:{}: {}", number + 1, line.trim()))
                .collect();
            found
        })
        .take(MAX_HITS)
        .collect();
    Ok(if hits.is_empty() {
        "Nothing found.".into()
    } else {
        hits.join("\n")
    })
}

struct Tools<'a> {
    root: &'a Path,
    folder: &'a Path,
    turn: &'a Turn,
    send: Send<'a>,
}

impl Tools<'_> {
    fn call(&self, name: &str, input: &Value) -> Result<String, String> {
        let Self {
            root,
            folder,
            turn,
            send,
        } = self;
        let text = |field: &str| {
            input[field]
                .as_str()
                .ok_or_else(|| format!("The tool {name} needs “{field}”."))
        };
        let step = |tool: &str, page: &str| {
            send(Event::Step {
                tool: tool.into(),
                page: page.into(),
            })
        };
        match name {
            "list_pages" => {
                let pages = workspace::pages(root);
                Ok(if pages.is_empty() { "The workspace has no pages yet.".into() } else { pages.join("\n") })
            }
            "read_page" => {
                let page = page(input);
                step("Read", &page)?;
                let source = workspace::read_text(root, &page)?;
                send(Event::Read { page })?;
                Ok(source)
            }
            "search_pages" => search(root, text("query")?),
            "read_file" => {
                let file = Path::new(text("name")?).file_name().ok_or("Give the name of an attached file.")?;
                let start = input["start"].as_str().and_then(|start| start.trim().parse().ok()).or(input["start"].as_u64()).unwrap_or(0) as usize;
                let read = extract::text(&folder.join(file))?;
                let piece: String = read.chars().skip(start).take(MAX_PIECE).collect();
                let end = start + MAX_PIECE;
                Ok(if read.chars().nth(end).is_some() { format!("{piece}\n… there is more. Call read_file again with start {end}.") } else { piece })
            }
            "fetch_url" if !turn.web => Err("This chat may not use the internet. The user can allow it from the paperclip menu.".into()),
            "fetch_url" => web::fetch(text("url")?),
            "write_page" | "edit_page" if turn.read_only => Err("You may only read pages in this chat. Tell the user what you would change.".into()),
            "write_page" => {
                let page = page(input);
                let source = text("text")?;
                step("Write", &page)?;
                workspace::ensure(root, &page)?;
                let before = workspace::read_text(root, &page)?;
                workspace::write_text(root, &page, source)?;
                send(Event::Edited { page, before: Some(before) })?;
                Ok("Saved.".into())
            }
            "edit_page" => {
                let page = page(input);
                let (old, new) = (text("old")?, text("new")?);
                step("Edit", &page)?;
                let before = workspace::read_text(root, &page)?;
                match before.matches(old).count() {
                    0 => Err("The old text is not on the page. Read the page again and copy it exactly.".into()),
                    1 => {
                        workspace::write_text(root, &page, &before.replacen(old, new, 1))?;
                        send(Event::Edited { page, before: Some(before) })?;
                        Ok("Saved.".into())
                    }
                    _ => Err("The old text appears more than once. Include more of the text around it.".into()),
                }
            }
            _ => Err(format!("There is no tool named {name}.")),
        }
    }

    fn answer(&self, call: &Call) -> String {
        let input: Value = serde_json::from_str(&call.arguments).unwrap_or_default();
        let output = self
            .call(&call.name, &input)
            .unwrap_or_else(|error| format!("Error: {error}"));
        if output.len() <= MAX_OUTPUT {
            return output;
        }
        let cut: String = output.chars().take(MAX_OUTPUT / 4).collect();
        format!("{cut}\n… the rest was left out because it is too long.")
    }
}

fn trim(history: &mut Vec<Value>) {
    let size = |history: &[Value]| {
        history
            .iter()
            .map(|message| message.to_string().len())
            .sum::<usize>()
    };
    while history.len() > 1 && (size(history) > MAX_HISTORY || history[0]["role"] != "user") {
        history.remove(0);
    }
}

fn body(turn: &Turn, model: &str, history: &[Value]) -> Value {
    let mut messages = vec![json!({ "role": "system", "content": turn.system })];
    messages.extend_from_slice(history);
    let mut body = json!({ "model": model, "messages": messages, "stream": true, "tools": tools(turn.read_only, turn.web) });
    if let Some(effort) = turn.effort.as_deref() {
        body["reasoning_effort"] = json!(if matches!(effort, "low" | "medium") {
            effort
        } else {
            "high"
        });
    }
    body
}

pub fn run(
    running: &Running,
    root: &Path,
    folder: &Path,
    turn: &Turn,
    secret: &Secret,
    send: Send,
) -> Result<(), String> {
    let model = turn
        .model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty() && model.len() <= 200);
    let model = model.ok_or("Choose a model for the assistant first.")?;
    let tools = Tools {
        root,
        folder,
        turn,
        send,
    };
    let file = folder.join("history.json");
    let mut history: Vec<Value> = fs::read(&file)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    history.push(json!({ "role": "user", "content": turn.prompt }));
    let mut failure = None;
    for step in 0.. {
        if step == MAX_STEPS {
            failure = Some("The assistant took too many steps and was stopped.".to_string());
            break;
        }
        trim(&mut history);
        let reply = match stream(
            running,
            secret,
            body(turn, model, &history).to_string(),
            send,
        ) {
            Ok(reply) => reply,
            Err(error) => {
                failure = Some(error);
                break;
            }
        };
        if running.stopped() {
            if !reply.text.is_empty() {
                history.push(json!({ "role": "assistant", "content": reply.text }));
            }
            break;
        }
        let said = message(&reply);
        let ids = said["tool_calls"].clone();
        history.push(said);
        if reply.calls.is_empty() {
            break;
        }
        for (index, call) in reply.calls.iter().enumerate() {
            let output = tools.answer(call);
            history.push(
                json!({ "role": "tool", "tool_call_id": ids[index]["id"], "content": output }),
            );
        }
    }
    let saved = serde_json::to_vec(&history).map_err(|error| error.to_string())?;
    keep(&file, &saved).map_err(|error| format!("The chat could not be saved: {error}"))?;
    send(Event::Done {
        session: String::new(),
        failure,
    })
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::net::TcpListener;
    use std::sync::Mutex;

    use super::super::Agent;
    use super::*;

    fn scratch(test: &str) -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!("znote-openai-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("chat")).unwrap();
        for path in ["Work", "Work/Notes", "Work/Notes/Monday"] {
            workspace::create(&base, path).unwrap();
        }
        workspace::write_text(&base, "Work/Notes/Monday", "= Monday\n- milk\n").unwrap();
        base
    }

    fn turn() -> Turn {
        Turn {
            chat: "chat".into(),
            prompt: "Add eggs".into(),
            system: "write Typst".into(),
            session: None,
            provider: "custom".into(),
            model: Some("llama".into()),
            effort: None,
            read_only: false,
            fast: false,
            web: false,
        }
    }

    fn serve(answers: Vec<String>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let asked = Arc::new(Mutex::new(Vec::new()));
        let seen = asked.clone();
        spawn(move || {
            for answer in answers {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if let Some(size) = line.to_lowercase().strip_prefix("content-length:") {
                        length = size.trim().parse().unwrap();
                    }
                    if line.trim().is_empty() {
                        break;
                    }
                }
                let mut asked = vec![0; length];
                reader.read_exact(&mut asked).unwrap();
                seen.lock().unwrap().push(String::from_utf8(asked).unwrap());
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{answer}").unwrap();
            }
        });
        (url, asked)
    }

    #[test]
    fn a_streamed_answer_is_put_together_from_its_pieces() {
        let mut reply = Reply::default();
        let lines = [
            ": OPENROUTER PROCESSING",
            r#"data: {"choices":[{"delta":{"reasoning":"The user wants eggs."}}]}"#,
            r#"data: {"choices":[{"delta":{"content":"On "}}]}"#,
            r#"data: {"choices":[{"delta":{"content":"it."}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_a","function":{"name":"edit_page","arguments":"{\"path\":"}}]}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"Work/Notes/Monday\"}"}}]}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":999999999,"function":{"name":"x"}}]}}]}"#,
            "data: not json",
            "data: [DONE]",
        ];
        let events: Vec<Event> = lines
            .iter()
            .flat_map(|line| absorb(&mut reply, line).unwrap())
            .collect();
        let expected = [
            Event::Thought {
                text: "The user wants eggs.".into(),
            },
            Event::Text { text: "On ".into() },
            Event::Text { text: "it.".into() },
            Event::Step {
                tool: "Edit".into(),
                page: String::new(),
            },
        ];
        assert_eq!(events, expected);
        assert!(reply.done);
        assert_eq!(reply.text, "On it.");
        assert_eq!(reply.calls.len(), 1);
        assert_eq!(
            message(&reply)["tool_calls"][0]["function"]["arguments"],
            r#"{"path":"Work/Notes/Monday"}"#
        );
        assert_eq!(
            absorb(&mut reply, r#"data: {"error":{"message":"No credit"}}"#),
            Err("No credit".into())
        );
    }

    #[test]
    fn only_a_real_server_address_and_a_plain_key_are_kept() {
        let secret =
            |provider: &str, url: Option<&str>, key: &str| hosted(provider)?.secret(url, key);
        assert_eq!(
            secret("openrouter", None, " sk-or-1 \n").unwrap(),
            Secret {
                url: OPENROUTER.into(),
                key: "sk-or-1".into()
            }
        );
        assert!(secret("openrouter", None, "").is_err());
        let local = secret(
            "custom",
            Some(" http://localhost:11434/v1/chat/completions/ "),
            "",
        )
        .unwrap();
        assert_eq!(
            local,
            Secret {
                url: "http://localhost:11434/v1".into(),
                key: String::new()
            }
        );
        for bad in ["", "localhost:11434", "ftp://host", "http://", "http://a b"] {
            assert!(secret("custom", Some(bad), "").is_err(), "{bad:?}");
        }
        assert!(secret("custom", Some("http://localhost"), "two words").is_err());
        assert!(secret("claude", None, "key").is_err());
    }

    #[test]
    fn tools_stay_inside_the_workspace_and_a_read_only_chat_changes_nothing() {
        let root = scratch("tools");
        let folder = root.join("chat");
        let seen = Mutex::new(Vec::new());
        let send = |event: Event| {
            seen.lock().unwrap().push(event);
            Ok(())
        };
        let call = |name: &str, input: Value, read_only: bool| {
            Tools {
                root: &root,
                folder: &folder,
                turn: &Turn {
                    read_only,
                    ..turn()
                },
                send: &send,
            }
            .call(name, &input)
        };
        assert_eq!(
            call("list_pages", json!({}), false).unwrap(),
            "Work/Notes/Monday"
        );
        assert_eq!(
            call(
                "read_page",
                json!({ "path": "/Work/Notes/Monday.typ" }),
                false
            )
            .unwrap(),
            "= Monday\n- milk\n"
        );
        assert_eq!(
            call("search_pages", json!({ "query": "MILK" }), false).unwrap(),
            "Work/Notes/Monday.typ:2: - milk"
        );
        assert!(call("read_page", json!({ "path": "../../etc/passwd" }), false).is_err());
        assert!(
            call(
                "write_page",
                json!({ "path": "../Outside/Notes/Page", "text": "x" }),
                false
            )
            .is_err()
        );
        assert!(
            call(
                "edit_page",
                json!({ "path": "Work/Notes/Monday", "old": "- milk", "new": "- eggs" }),
                true
            )
            .is_err()
        );
        assert!(
            call(
                "edit_page",
                json!({ "path": "Work/Notes/Monday", "old": "- tea", "new": "- eggs" }),
                false
            )
            .is_err()
        );
        call(
            "edit_page",
            json!({ "path": "Work/Notes/Monday", "old": "- milk", "new": "- eggs" }),
            false,
        )
        .unwrap();
        call(
            "write_page",
            json!({ "path": "Home/Lists/Shopping", "text": "= Shopping\n" }),
            false,
        )
        .unwrap();
        assert_eq!(
            workspace::read_text(&root, "Work/Notes/Monday").unwrap(),
            "= Monday\n- eggs\n"
        );
        assert_eq!(
            workspace::read_text(&root, "Home/Lists/Shopping").unwrap(),
            "= Shopping\n"
        );
        fs::write(folder.join("1-notes.txt"), "attached").unwrap();
        assert_eq!(
            call(
                "read_file",
                json!({ "name": "/somewhere/else/1-notes.txt" }),
                false
            )
            .unwrap(),
            "attached"
        );
        assert!(
            call(
                "read_file",
                json!({ "name": "../../Work/Notes/Monday.typ" }),
                false
            )
            .is_err()
        );
        fs::write(folder.join("2-long.txt"), "é".repeat(MAX_PIECE + 5)).unwrap();
        let first = call("read_file", json!({ "name": "2-long.txt" }), false).unwrap();
        assert!(first.len() <= MAX_OUTPUT && first.ends_with(&format!("start {MAX_PIECE}.")));
        assert_eq!(
            call(
                "read_file",
                json!({ "name": "2-long.txt", "start": MAX_PIECE.to_string() }),
                false
            )
            .unwrap(),
            "ééééé"
        );
        assert!(
            call("fetch_url", json!({ "url": "https://example.com" }), false)
                .unwrap_err()
                .starts_with("This chat may not use the internet")
        );
        let named = |web: bool| tools(true, web).to_string().contains("fetch_url");
        assert!(named(true) && !named(false));
        let edits: Vec<Event> = seen
            .lock()
            .unwrap()
            .iter()
            .filter(|event| matches!(event, Event::Edited { .. }))
            .cloned()
            .collect();
        let expected = [
            Event::Edited {
                page: "Work/Notes/Monday".into(),
                before: Some("= Monday\n- milk\n".into()),
            },
            Event::Edited {
                page: "Home/Lists/Shopping".into(),
                before: Some(String::new()),
            },
        ];
        assert_eq!(edits, expected);
    }

    #[test]
    fn an_old_chat_is_shortened_from_the_front_and_still_starts_with_the_user() {
        let long = "x".repeat(MAX_HISTORY / 2);
        let mut history = vec![
            json!({ "role": "user", "content": long }),
            json!({ "role": "assistant", "content": long }),
            json!({ "role": "tool", "content": "left over" }),
            json!({ "role": "user", "content": "latest" }),
        ];
        trim(&mut history);
        assert_eq!(history, [json!({ "role": "user", "content": "latest" })]);
    }

    #[test]
    fn a_turn_calls_a_tool_sends_the_result_back_and_remembers_the_chat() {
        let root = scratch("turn");
        let folder = root.join("chat");
        let first = concat!(
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_a","function":{"name":"edit_page","arguments":"{\"path\":\"Work/Notes/Monday\",\"old\":\"- milk\",\"new\":\"- milk\\n- eggs\"}"}}]}}]}"#,
            "\n\n",
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":1,"id":"call_b","function":{"name":"list_pages","arguments":""}}]}}]}"#,
            "\n\ndata: [DONE]\n\n"
        );
        let second =
            "data: {\"choices\":[{\"delta\":{\"content\":\"Added eggs.\"}}]}\n\ndata: [DONE]\n\n";
        let (url, asked) = serve(vec![first.into(), second.into()]);
        let agent = Agent::default();
        let running = agent.begin("chat").unwrap();
        let seen = Mutex::new(Vec::new());
        let send = |event: Event| {
            seen.lock().unwrap().push(event);
            Ok(())
        };
        run(
            &running,
            &root,
            &folder,
            &turn(),
            &Secret {
                url,
                key: "secret-key".into(),
            },
            &send,
        )
        .unwrap();

        assert_eq!(
            workspace::read_text(&root, "Work/Notes/Monday").unwrap(),
            "= Monday\n- milk\n- eggs\n"
        );
        let step = |page: &str| Event::Step {
            tool: "Edit".into(),
            page: page.into(),
        };
        let expected = [
            step(""),
            Event::Step {
                tool: "Search".into(),
                page: String::new(),
            },
            step("Work/Notes/Monday"),
            Event::Edited {
                page: "Work/Notes/Monday".into(),
                before: Some("= Monday\n- milk\n".into()),
            },
            Event::Text {
                text: "Added eggs.".into(),
            },
            Event::Done {
                session: String::new(),
                failure: None,
            },
        ];
        assert_eq!(*seen.lock().unwrap(), expected);

        let asked = asked.lock().unwrap();
        let again: Value = serde_json::from_str(&asked[1]).unwrap();
        assert_eq!(again["model"], "llama");
        assert_eq!(again["messages"][0]["content"], "write Typst");
        assert_eq!(
            again["messages"][3],
            json!({ "role": "tool", "tool_call_id": "call_a", "content": "Saved." })
        );
        assert_eq!(again["messages"][4]["tool_call_id"], "call_b");
        let history: Vec<Value> =
            serde_json::from_slice(&fs::read(folder.join("history.json")).unwrap()).unwrap();
        assert_eq!(history.len(), 5);
        assert_eq!(history[4]["content"], "Added eggs.");
        assert!(
            !fs::read_to_string(folder.join("history.json"))
                .unwrap()
                .contains("secret-key")
        );
    }

    #[test]
    fn a_refused_request_ends_the_turn_with_a_reason_the_user_can_read() {
        let root = scratch("refused");
        let agent = Agent::default();
        let running = agent.begin("chat").unwrap();
        let seen = Mutex::new(Vec::new());
        let send = |event: Event| {
            seen.lock().unwrap().push(event);
            Ok(())
        };
        let nowhere = Secret {
            url: "http://127.0.0.1:1/v1".into(),
            key: String::new(),
        };
        run(
            &running,
            &root,
            &root.join("chat"),
            &turn(),
            &nowhere,
            &send,
        )
        .unwrap();
        match seen.lock().unwrap().last() {
            Some(Event::Done {
                failure: Some(reason),
                ..
            }) => assert!(
                reason.starts_with("The server could not be reached"),
                "{reason}"
            ),
            other => panic!("{other:?}"),
        }
        assert!(
            run(
                &running,
                &root,
                &root.join("chat"),
                &Turn {
                    model: None,
                    ..turn()
                },
                &nowhere,
                &send
            )
            .is_err()
        );
    }
}

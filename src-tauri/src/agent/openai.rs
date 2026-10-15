use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::net::ToSocketAddrs;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::thread::spawn;
use std::time::Duration;

use serde_json::{Value, json};

use super::secrets::{self, Secret};
use super::{
    Check, Event, POLL, Running, Send, Status, Turn, attachment_file, in_scope, keep, level_of,
    seen, web,
};
use crate::knowledge::{Gate, LIGHT};
use crate::policy::{self, Collection, Policy};
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

fn nearby(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://") else {
        return true;
    };
    let authority = rest.split('/').next().unwrap_or_default();
    let ported = authority
        .rsplit_once(':')
        .is_some_and(|(_, port)| port.parse::<u16>().is_ok() && !authority.ends_with(']'));
    let address = if ported {
        authority.to_string()
    } else {
        format!("{authority}:80")
    };
    address.to_socket_addrs().is_ok_and(|found| {
        let found: Vec<_> = found.collect();
        !found.is_empty() && found.iter().all(|address| !web::public(address.ip()))
    })
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
    if !nearby(url) {
        return Err("Plain http:// is only used for a server on this computer or your own network. Use https:// for this address.".into());
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
        gate: &Gate,
        (send, check): (Send, Check),
    ) -> Result<(), String> {
        let secret = secrets::read(self.id)?.ok_or("Connect the assistant in Settings first.")?;
        let routing = match self.address {
            Some(OPENROUTER) => routing(policy::get()),
            _ => Value::Null,
        };
        run(
            running,
            root,
            folder,
            turn,
            (&secret, &routing),
            gate,
            (send, check),
        )
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

fn tools(read_only: bool, web: bool, knows: bool) -> Value {
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
        tool("list_pages", "List the pages this chat may use.", &[]),
        tool("read_page", "Read the Typst source of a page.", &[path]),
        tool(
            "search_pages",
            "Find lines that contain a text, in the pages this chat may use.",
            &[("query", "The text to look for")],
        ),
        tool(
            "read_file",
            "Read the text of a file the user attached to this chat: plain text, PDF, Word, PowerPoint, Excel or OpenDocument.",
            &[
                ("name", "The file as the user named it, such as 1.pdf"),
                ("start", "The character to start at. Use 0 first."),
            ],
        ),
    ];
    if knows {
        tools.push(tool(
            "search_knowledge",
            "Search the user's knowledge base of notes and files. Gives at most 10 pieces of text.",
            &[("query", "What to look for")],
        ));
        tools.push(tool(
            "knowledge_entity",
            "Tell what the knowledge base knows about a token such as ⟦CUSTOMER_3⟧, and which pages mention it.",
            &[("token", "The token, written exactly")],
        ));
    }
    if web {
        tools.push(tool("fetch_url", "Read the text of a public web page. Use it only for an address the user gave or asked for.", &[("url", "The https:// address")]));
    }
    if !read_only {
        tools.push(tool("write_page", "Write a whole page. Makes the page, its notebook and its project when they are missing. Answers with the compile errors, if any.", &[path, ("text", "The full Typst source")]));
        tools.push(tool(
            "edit_page",
            "Replace one piece of text in a page. The old text must appear exactly once. Answers with the compile errors, if any.",
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
        "search_knowledge" | "knowledge_entity" => "Knowledge",
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

// With hiding on the pages are searched as the assistant sees them, hidden, so a guess at part of a
// name ("Jan de V") finds nothing and cannot tell the assistant what a token stands for.
fn search(
    root: &Path,
    query: &str,
    gate: &Gate,
    seen: impl Fn(&str) -> bool,
) -> Result<String, String> {
    if query.trim().is_empty() {
        return Err("Give a text to look for.".into());
    }
    let wanted = query.to_lowercase();
    let mut hits = Vec::new();
    for page in workspace::pages(root).into_iter().filter(|page| seen(page)) {
        let real = workspace::read_text(root, &page)?;
        // Hiding keeps every line where it was, so a hidden line that matches gives the real line,
        // which `answer` hides again with the rest of the output.
        let shown = match gate.on() {
            true => gate.veil_at(&real, gate.page_level(&page))?.text,
            false => real.clone(),
        };
        hits.extend(
            shown
                .lines()
                .zip(real.lines())
                .enumerate()
                .filter(|(_, (line, _))| line.to_lowercase().contains(&wanted))
                .map(|(at, (_, line))| format!("{page}.typ:{}: {}", at + 1, line.trim())),
        );
        if hits.len() >= MAX_HITS {
            hits.truncate(MAX_HITS);
            break;
        }
    }
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
    gate: &'a Gate<'a>,
    send: Send<'a>,
    check: Check<'a>,
}

// A page of a long text, ended before a token rather than inside one.
fn piece(text: &str, start: usize) -> String {
    let mut piece: String = text.chars().skip(start).take(MAX_PIECE).collect();
    let end = start + piece.chars().count();
    if text.chars().nth(end).is_none() {
        return piece;
    }
    whole_tokens(&mut piece);
    let end = start + piece.chars().count();
    format!("{piece}\n… there is more. Call read_file again with start {end}.")
}

pub(super) fn whole_tokens(text: &mut String) {
    if let Some(open) = text.rfind('⟦').filter(|&open| !text[open..].contains('⟧')) {
        text.truncate(open);
    }
}

fn cut(output: String) -> String {
    if output.len() <= MAX_OUTPUT {
        return output;
    }
    let mut cut: String = output.chars().take(MAX_OUTPUT / 4).collect();
    whole_tokens(&mut cut);
    format!("{cut}\n… the rest was left out because it is too long.")
}

// The page is checked as the preview compiles it, so the assistant sees the errors the user would.
fn saved(problems: Vec<String>) -> String {
    if problems.is_empty() {
        return "Saved.".into();
    }
    format!(
        "Saved, but the page does not compile. Fix these errors:\n{}",
        problems.join("\n")
    )
}

// Errors already on the page come after it, marked so they are not copied into an edit.
fn read(source: String, problems: Vec<String>) -> String {
    if problems.is_empty() {
        return source;
    }
    format!(
        "{source}\n\n[znote: the page above does not compile. These lines are not part of it:]\n{}",
        problems.join("\n")
    )
}

impl Tools<'_> {
    fn call(&self, name: &str, input: &Value) -> Result<String, String> {
        let Self {
            root,
            folder,
            turn,
            gate,
            send,
            check,
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
        let seen = |page: &str| in_scope(folder, turn.page.as_deref(), page);
        let hidden = |page: &str| -> Result<(), String> {
            if seen(page) {
                Ok(())
            } else {
                Err(format!(
                    "{page} is not part of this chat. Ask the user to add it with @."
                ))
            }
        };
        match name {
            "list_pages" => {
                let pages: Vec<String> = workspace::pages(root).into_iter().filter(|page| seen(page)).collect();
                Ok(if pages.is_empty() { "This chat has no pages yet. The user can add one with @.".into() } else { pages.join("\n") })
            }
            "read_page" => {
                let page = page(input);
                hidden(&page)?;
                step("Read", &page)?;
                let source = workspace::read_text(root, &page)?;
                send(Event::Read { page: page.clone() })?;
                Ok(read(source, check(&page)))
            }
            "search_pages" => search(root, text("query")?, gate, seen),
            // The whole file: `answer` hides it first and only then takes the piece asked for.
            "read_file" => {
                let file = attachment_file(folder, text("name")?).ok_or("There is no attached file by that name. Use the name the user gave, such as 1.pdf.")?;
                extract::text(&folder.join(file))
            }
            "search_knowledge" => gate.search(text("query")?),
            "knowledge_entity" => gate.entity(text("token")?),
            "fetch_url" if !turn.web => Err("This chat may not use the internet. The user can allow it from the paperclip menu.".into()),
            "fetch_url" => web::fetch(text("url")?),
            "write_page" | "edit_page" if turn.read_only => Err("You may only read pages in this chat. Tell the user what you would change.".into()),
            "write_page" => {
                let page = page(input);
                if workspace::page_exists(root, &page) {
                    hidden(&page)?;
                }
                let source = &gate.unveil_typst(text("text")?);
                step("Write", &page)?;
                workspace::ensure(root, &page)?;
                let before = workspace::read_text(root, &page)?;
                workspace::write_text(root, &page, source)?;
                send(Event::Edited { page: page.clone(), before: Some(before) })?;
                Ok(saved(check(&page)))
            }
            "edit_page" => {
                let page = page(input);
                hidden(&page)?;
                let (old, new) = (text("old")?, text("new")?);
                step("Edit", &page)?;
                let before = workspace::read_text(root, &page)?;
                // The edit is made on the page as the assistant read it, hidden, so the old text
                // must match what it saw and not a guess at the names behind the tokens.
                let shown = match gate.on() {
                    true => gate.veil_at(&before, gate.page_level(&page))?.text,
                    false => before.clone(),
                };
                match shown.matches(old).count() {
                    0 => Err("The old text is not on the page. Read the page again and copy it exactly.".into()),
                    1 => {
                        workspace::write_text(root, &page, &gate.unveil_typst(&shown.replacen(old, new, 1)))?;
                        send(Event::Edited { page: page.clone(), before: Some(before) })?;
                        Ok(saved(check(&page)))
                    }
                    _ => Err("The old text appears more than once. Include more of the text around it.".into()),
                }
            }
            _ => Err(format!("There is no tool named {name}.")),
        }
    }

    fn answer(&self, call: &Call) -> String {
        let mut input: Value = serde_json::from_str(&call.arguments).unwrap_or_default();
        let local = match call.name.as_str() {
            "read_page" | "write_page" | "edit_page" => "path",
            "search_knowledge" => "query",
            _ => "",
        };
        if let Some(text) = input[local].as_str() {
            let real = match local {
                "query" => self.gate.searchable(text),
                _ => Ok(self.gate.unveil(text).0),
            };
            input[local] = Value::String(real.unwrap_or_default());
        }
        let output = self
            .call(&call.name, &input)
            .unwrap_or_else(|error| format!("Error: {error}"));
        // Text is cut only once it is hidden, so a name is never split into pieces hiding misses.
        let paged = |output: String| match call.name.as_str() {
            "read_file" if !output.starts_with("Error: ") => {
                let start = input["start"]
                    .as_str()
                    .and_then(|start| start.trim().parse().ok())
                    .or(input["start"].as_u64())
                    .unwrap_or(0);
                piece(&output, start as usize)
            }
            _ => cut(output),
        };
        if !self.gate.on() {
            return paged(output);
        }
        let level = match call.name.as_str() {
            "read_file" => input["name"]
                .as_str()
                .and_then(|name| attachment_file(self.folder, name))
                .map_or(LIGHT, |file| level_of(self.folder, &file)),
            // A compile error can quote the page, so it is hidden as the page is.
            "read_page" | "write_page" | "edit_page" => self.gate.page_level(&page(&input)),
            "search_pages" | "search_knowledge" | "knowledge_entity" => self.gate.strictest(),
            _ => LIGHT,
        };
        match self.gate.veil_at(&output, level) {
            Ok(veiled) => {
                let veiled = paged(veiled.text);
                if matches!(
                    call.name.as_str(),
                    "read_page"
                        | "read_file"
                        | "search_pages"
                        | "search_knowledge"
                        | "knowledge_entity"
                        | "fetch_url"
                ) {
                    let about = ["path", "name", "query", "token", "url"]
                        .iter()
                        .find_map(|field| input[field].as_str())
                        .unwrap_or_default();
                    let _ = (self.send)(seen(&format!("{} {about}", call.name), &veiled));
                }
                veiled
            }
            Err(error) => format!("Error: {error}"),
        }
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

fn routing(policy: &Policy) -> Value {
    let collection = match policy.data_collection {
        Collection::Allow => "allow",
        Collection::Deny => "deny",
    };
    json!({ "data_collection": collection, "zdr": policy.zero_retention })
}

fn body(turn: &Turn, model: &str, history: &[Value], knows: bool, routing: &Value) -> Value {
    let mut messages = vec![json!({ "role": "system", "content": turn.system })];
    messages.extend_from_slice(history);
    let mut body = json!({ "model": model, "messages": messages, "stream": true, "tools": tools(turn.read_only, turn.web, knows) });
    if !routing.is_null() {
        body["provider"] = routing.clone();
    }
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
    (secret, routing): (&Secret, &Value),
    gate: &Gate,
    (send, check): (Send, Check),
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
        gate,
        send,
        check,
    };
    let file = folder.join("history.json");
    let mut history: Vec<Value> = fs::read(&file)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let prompt = gate.veil(&turn.prompt)?;
    if gate.on() {
        send(Event::Veiled {
            kinds: prompt.kinds,
        })?;
        send(seen("prompt", &prompt.text))?;
        send(seen("system", &turn.system))?;
    }
    history.push(json!({ "role": "user", "content": prompt.text }));
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
            body(turn, model, &history, gate.knows(), routing).to_string(),
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
    use crate::knowledge::{Knowledge, Options};
    use crate::privacy::glossary;

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
            veil: true,
            pages: Vec::new(),
            page: Some("Work/Notes/Monday".into()),
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
        for near in [
            "http://127.0.0.1:11434/v1",
            "http://192.168.1.20/v1",
            "http://[::1]:8080/v1",
            "https://1.1.1.1/v1",
        ] {
            assert!(secret("custom", Some(near), "").is_ok(), "{near}");
        }
        assert!(
            secret("custom", Some("http://1.1.1.1/v1"), "")
                .is_err_and(|error| error.contains("Use https://"))
        );
        assert!(secret("claude", None, "key").is_err());
    }

    #[test]
    fn with_hiding_on_a_guess_at_a_name_finds_nothing_and_a_literal_token_stays() {
        let root = scratch("guess");
        let folder = root.join("chat");
        workspace::write_text(
            &root,
            "Work/Notes/Monday",
            "= Monday\n- call Acme Corporation\n- see ⟦PERSON_1⟧\n",
        )
        .unwrap();
        let ttl = "@prefix skos: <http://www.w3.org/2004/02/skos/core#> .
<urn:c> skos:prefLabel \"Customer\" ; skos:topConceptOf <urn:s> .
<urn:a> skos:prefLabel \"Acme Corporation\" ; skos:broader <urn:c> .";
        let glossary = glossary::parse(ttl.as_bytes(), oxrdfio::RdfFormat::Turtle).unwrap();
        let knowledge = Knowledge::in_memory(
            &glossary,
            Options {
                veil: true,
                ..Options::default()
            },
        );
        let gate = knowledge.gate().unwrap();
        let send = |_| Ok(());
        let turn = turn();
        let tools = Tools {
            root: &root,
            folder: &folder,
            turn: &turn,
            gate: &gate,
            send: &send,
            check: &|_| Vec::new(),
        };
        let answer = |name: &str, arguments: Value| {
            tools.answer(&Call {
                id: "call".into(),
                name: name.into(),
                arguments: arguments.to_string(),
            })
        };
        assert_eq!(
            answer("search_pages", json!({ "query": "Acme Corp" })),
            "Nothing found."
        );
        assert_eq!(
            answer("search_pages", json!({ "query": "⟦CUSTOMER_1⟧" })),
            "Work/Notes/Monday.typ:2: - call ⟦CUSTOMER_1⟧"
        );
        assert!(
            answer(
                "edit_page",
                json!({ "path": "Work/Notes/Monday", "old": "Acme", "new": "x" })
            )
            .starts_with("Error: The old text is not")
        );
        assert_eq!(
            answer(
                "edit_page",
                json!({ "path": "Work/Notes/Monday", "old": "- call", "new": "- mail" })
            ),
            "Saved."
        );
        assert_eq!(
            workspace::read_text(&root, "Work/Notes/Monday").unwrap(),
            "= Monday\n- mail Acme Corporation\n- see ⟦PERSON_1⟧\n"
        );
    }

    #[test]
    fn a_chat_sees_only_the_open_page_the_pages_it_was_given_and_the_pages_it_wrote() {
        let root = scratch("scope");
        let folder = root.join("chat");
        workspace::ensure(&root, "Home/Lists/Shopping").unwrap();
        workspace::write_text(&root, "Home/Lists/Shopping", "- milk\n").unwrap();
        let knowledge = Knowledge::default();
        let gate = knowledge.gate().unwrap();
        let send = |_| Ok(());
        let isolated = Turn {
            page: None,
            ..turn()
        };
        let call = |name: &str, input: Value| {
            Tools {
                root: &root,
                folder: &folder,
                turn: &isolated,
                gate: &gate,
                send: &send,
                check: &|_| Vec::new(),
            }
            .call(name, &input)
        };
        assert!(
            call("list_pages", json!({}))
                .unwrap()
                .starts_with("This chat has no pages")
        );
        assert!(
            call("read_page", json!({ "path": "Work/Notes/Monday" }))
                .unwrap_err()
                .contains("not part of this chat")
        );
        assert_eq!(
            call("search_pages", json!({ "query": "milk" })).unwrap(),
            "Nothing found."
        );
        let edit = json!({ "path": "Work/Notes/Monday", "old": "- milk", "new": "- eggs" });
        assert!(call("edit_page", edit.clone()).is_err());
        assert!(
            call(
                "write_page",
                json!({ "path": "Work/Notes/Monday", "text": "gone" })
            )
            .is_err()
        );
        assert_eq!(
            workspace::read_text(&root, "Work/Notes/Monday").unwrap(),
            "= Monday\n- milk\n"
        );
        call(
            "write_page",
            json!({ "path": "Work/Notes/New", "text": "= New\n" }),
        )
        .unwrap();

        super::super::add_to_scope(&folder, &["Home".into(), "Work/Notes/New".into()]).unwrap();
        assert_eq!(
            call("list_pages", json!({})).unwrap(),
            "Home/Lists/Shopping\nWork/Notes/New"
        );
        assert_eq!(
            call("search_pages", json!({ "query": "milk" })).unwrap(),
            "Home/Lists/Shopping.typ:1: - milk"
        );
        assert!(call("read_page", json!({ "path": "Work/Notes/Monday" })).is_err());
        assert!(call("read_page", json!({ "path": "Work/Notes/New" })).is_ok());
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
        let knowledge = Knowledge::default();
        let gate = knowledge.gate().unwrap();
        let call = |name: &str, input: Value, read_only: bool| {
            Tools {
                root: &root,
                folder: &folder,
                turn: &Turn {
                    read_only,
                    ..turn()
                },
                gate: &gate,
                send: &send,
                check: &|_| Vec::new(),
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
        let long = "é".repeat(MAX_PIECE + 5);
        let first = piece(&long, 0);
        assert!(first.len() <= MAX_OUTPUT && first.ends_with(&format!("start {MAX_PIECE}.")));
        assert_eq!(piece(&long, MAX_PIECE), "ééééé");
        let split = format!("{}⟦PERSON_1⟧ rest", "a".repeat(MAX_PIECE - 3));
        let first = piece(&split, 0);
        let start = MAX_PIECE - 3;
        assert!(first.ends_with(&format!(
            "{}\n… there is more. Call read_file again with start {start}.",
            "a".repeat(3)
        )));
        assert_eq!(piece(&split, start), "⟦PERSON_1⟧ rest");
        assert!(
            call("fetch_url", json!({ "url": "https://example.com" }), false)
                .unwrap_err()
                .starts_with("This chat may not use the internet")
        );
        assert_eq!(
            call("read_file", json!({ "name": "1.txt" }), false).unwrap(),
            "attached"
        );
        assert!(call("read_file", json!({ "name": "3.pdf" }), false).is_err());
        let named = |web: bool| tools(true, web, false).to_string().contains("fetch_url");
        assert!(named(true) && !named(false));
        assert!(
            tools(true, false, true)
                .to_string()
                .contains("search_knowledge")
        );
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
        let knowledge = Knowledge::default();
        run(
            &running,
            &root,
            &folder,
            &turn(),
            (
                &Secret {
                    url,
                    key: "secret-key".into(),
                },
                &routing(policy::get()),
            ),
            &knowledge.gate().unwrap(),
            (&send, &|page: &str| {
                vec![format!("line 3: unknown variable: eggs in {page}")]
            }),
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
        assert_eq!(
            again["provider"],
            json!({ "data_collection": "deny", "zdr": false })
        );
        assert_eq!(again["messages"][0]["content"], "write Typst");
        assert_eq!(
            again["messages"][3],
            json!({ "role": "tool", "tool_call_id": "call_a", "content": "Saved, but the page does not compile. Fix these errors:\nline 3: unknown variable: eggs in Work/Notes/Monday" })
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
        let knowledge = Knowledge::default();
        let gate = knowledge.gate().unwrap();
        run(
            &running,
            &root,
            &root.join("chat"),
            &turn(),
            (&nowhere, &Value::Null),
            &gate,
            (&send, &|_| Vec::new()),
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
                (&nowhere, &Value::Null),
                &gate,
                (&send, &|_| Vec::new())
            )
            .is_err()
        );
    }

    #[test]
    fn with_veiling_on_the_provider_sees_tokens_only_and_edits_land_with_the_real_name() {
        let root = scratch("veiled");
        let folder = root.join("chat");
        workspace::write_text(
            &root,
            "Work/Notes/Monday",
            "= Monday\n- call Acme Corporation\n",
        )
        .unwrap();
        let ttl = "@prefix skos: <http://www.w3.org/2004/02/skos/core#> .
<urn:c> skos:prefLabel \"Customer\" ; skos:topConceptOf <urn:s> .
<urn:a> skos:prefLabel \"Acme Corporation\" ; skos:altLabel \"Acme\" ; skos:broader <urn:c> .";
        let glossary = glossary::parse(ttl.as_bytes(), oxrdfio::RdfFormat::Turtle).unwrap();
        let knowledge = Knowledge::in_memory(
            &glossary,
            Options {
                veil: true,
                ..Options::default()
            },
        );
        let gate = knowledge.gate().unwrap();
        let call = |id: &str, name: &str, arguments: Value| {
            let delta = json!({ "choices": [{ "delta": { "tool_calls": [{ "index": 0, "id": id, "function": { "name": name, "arguments": arguments.to_string() } }] } }] });
            format!("data: {delta}\n\ndata: [DONE]\n\n")
        };
        let answers = vec![
            call("call_a", "read_page", json!({ "path": "Work/Notes/Monday" })),
            call("call_b", "edit_page", json!({ "path": "Work/Notes/Monday", "old": "- call ⟦CUSTOMER_1⟧", "new": "- call ⟦CUSTOMER_1⟧ and ⟦CUSTOMER_1~1⟧ again at ⟦EMAIL_1⟧" })),
            call("call_c", "search_pages", json!({ "query": "⟦CUSTOMER_1~1⟧" })),
            "data: {\"choices\":[{\"delta\":{\"reasoning\":\"Tell ⟦CUSTOMER_1⟧ it is done.\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"Done with ⟦CUST\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"OMER_1⟧ and ⟦NOPE_2⟧.\"}}]}\n\ndata: [DONE]\n\n".into(),
        ];
        let (url, asked) = serve(answers);
        let agent = Agent::default();
        let running = agent.begin("chat").unwrap();
        let seen = Mutex::new(Vec::new());
        let raw = |event: Event| {
            seen.lock().unwrap().push(event);
            Ok(())
        };
        let streams = Mutex::new(Default::default());
        let send = |event| super::super::unveiled(&gate, &streams, &raw, event);
        let veiled_turn = Turn {
            prompt: "Tell Acme Corporation about Acme, mail jan@acme.example".into(),
            ..turn()
        };
        run(
            &running,
            &root,
            &folder,
            &veiled_turn,
            (
                &Secret {
                    url,
                    key: String::new(),
                },
                &Value::Null,
            ),
            &gate,
            // The error quotes the real name, as Typst would, and must reach the provider hidden.
            (&send, &|_| vec!["line 2: unknown variable: Acme".into()]),
        )
        .unwrap();

        assert_eq!(
            workspace::read_text(&root, "Work/Notes/Monday").unwrap(),
            "= Monday\n- call Acme Corporation and Acme again at jan\\@acme.example\n"
        );
        let asked = asked.lock().unwrap();
        assert_eq!(asked.len(), 4);
        for body in asked.iter() {
            assert!(!body.to_lowercase().contains("acme"), "{body}");
        }
        let first: Value = serde_json::from_str(&asked[0]).unwrap();
        assert_eq!(
            first["messages"][1]["content"],
            "Tell ⟦CUSTOMER_1⟧ about ⟦CUSTOMER_1~1⟧, mail ⟦EMAIL_1⟧"
        );
        let read: Value = serde_json::from_str(&asked[1]).unwrap();
        assert_eq!(
            read["messages"][3]["content"],
            "= Monday\n- call ⟦CUSTOMER_1⟧\n\n\n[znote: the page above does not compile. These lines are not part of it:]\nline 2: unknown variable: ⟦CUSTOMER_1~1⟧"
        );
        let last: Value = serde_json::from_str(&asked[3]).unwrap();
        assert!(
            last["messages"]
                .to_string()
                .contains("Work/Notes/Monday.typ:2: - call ⟦CUSTOMER_1⟧ and ⟦CUSTOMER_1~1⟧ again")
        );
        let history = fs::read_to_string(folder.join("history.json")).unwrap();
        assert!(!history.to_lowercase().contains("acme"));
        let events = seen.lock().unwrap();
        let said: String = events
            .iter()
            .filter_map(|event| match event {
                Event::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(said, "Done with Acme Corporation and ⟦NOPE_2⟧.");
        let thought: String = events
            .iter()
            .filter_map(|event| match event {
                Event::Thought { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(thought, "Tell ⟦CUSTOMER_1⟧ it is done.");
        assert!(events.iter().any(|event| matches!(event, Event::Restored { texts } if texts[0].text == "Acme Corporation" && texts[0].kind == "CUSTOMER")));
        assert!(
            events
                .iter()
                .any(|event| matches!(event, Event::Veiled { kinds } if kinds["CUSTOMER"] == 2))
        );
        assert!(events.iter().any(|event| matches!(event, Event::Edited { before: Some(before), .. } if before.contains("Acme Corporation"))));
        assert!(events.iter().any(|event| matches!(event, Event::Saw { label, text } if label == "read_page Work/Notes/Monday" && text.contains("⟦CUSTOMER_1⟧"))));
    }

    #[test]
    fn a_long_tool_answer_is_cut_before_a_token_and_not_inside_it() {
        let kept = "a".repeat(MAX_OUTPUT / 4 - 3);
        let long = format!("{kept}⟦PERSON_1⟧{}", "b".repeat(MAX_OUTPUT));
        assert_eq!(
            cut(long),
            format!("{kept}\n… the rest was left out because it is too long.")
        );
        assert_eq!(cut("short".into()), "short");
    }
}

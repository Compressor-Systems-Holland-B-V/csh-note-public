use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Mutex;
use std::thread::{sleep, spawn};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::workspace::Workspace;

const MODELS: [&str; 4] = ["fable", "opus", "sonnet", "haiku"];
const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];
const SIGN_INS: [&str; 2] = ["console", "sso"];
const MAX_ATTACHMENT_BYTES: u64 = 32 * 1024 * 1024;
const POLL: Duration = Duration::from_millis(150);

#[derive(Default)]
pub struct Agent {
    child: Mutex<Option<Child>>,
    input: Mutex<Option<ChildStdin>>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Status {
    installed: bool,
    connected: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Event {
    Text { text: String },
    Step { tool: String, page: String },
    Edited { page: String },
    Read { page: String },
    Done { session: String, failure: Option<String> },
}

#[derive(Deserialize)]
pub struct Turn {
    chat: String,
    prompt: String,
    system: String,
    session: Option<String>,
    model: Option<String>,
    effort: Option<String>,
}

#[derive(Serialize)]
pub struct Attachment {
    name: String,
    path: String,
}

fn label(root: &Path, file: &str) -> String {
    let path = Path::new(file);
    let shown = match path.strip_prefix(root) {
        Ok(inside) => inside.with_extension(""),
        Err(_) => path.file_name().map(PathBuf::from).unwrap_or_default(),
    };
    shown.to_string_lossy().replace('\\', "/")
}

fn parse(root: &Path, line: &str) -> Option<Event> {
    let event: Value = serde_json::from_str(line).ok()?;
    match event["type"].as_str()? {
        "stream_event" => {
            let delta = &event["event"]["delta"];
            let block = &event["event"]["content_block"];
            match (delta["type"].as_str(), block["type"].as_str()) {
                (Some("text_delta"), _) => Some(Event::Text { text: delta["text"].as_str().unwrap_or_default().into() }),
                (_, Some("thinking")) => Some(Event::Step { tool: "Thinking".into(), page: String::new() }),
                (_, Some("tool_use")) => {
                    Some(Event::Step { tool: block["name"].as_str().unwrap_or_default().into(), page: String::new() })
                }
                _ => None,
            }
        }
        "assistant" => event["message"]["content"].as_array()?.iter().find_map(|block| {
            let file = block["input"]["file_path"].as_str()?;
            Some(Event::Step { tool: block["name"].as_str()?.into(), page: label(root, file) })
        }),
        "user" => {
            let result = &event["tool_use_result"];
            if let Some(file) = result["filePath"].as_str() {
                return Some(Event::Edited { page: label(root, file) });
            }
            result["file"]["filePath"].as_str().map(|file| Event::Read { page: label(root, file) })
        }
        "result" => {
            let failed = event["is_error"].as_bool().unwrap_or(true);
            let message = event["result"].as_str().filter(|message| !message.is_empty());
            Some(Event::Done {
                session: event["session_id"].as_str().unwrap_or_default().into(),
                failure: failed.then(|| message.unwrap_or("The assistant stopped before it finished.").to_string()),
            })
        }
        _ => None,
    }
}

fn plain(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn arguments(turn: &Turn, attachments: &Path) -> Result<Vec<String>, String> {
    let mut arguments: Vec<String> = [
        "-p",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--restricted",
        "--safe-mode",
        "--strict-mcp-config",
        "--tools",
        "Read,Edit,Write,Glob,Grep",
        "--permission-mode",
        "dontAsk",
        "--permission-prompts",
        "none",
        "--allowedTools",
        "Read",
        "Glob",
        "Grep",
        "Edit(/**/*.typ)",
        "--system-prompt",
        &turn.system,
        "--add-dir",
        &attachments.to_string_lossy(),
    ]
    .map(str::to_string)
    .to_vec();
    let mut option = |flag: &str, value: &Option<String>, allowed: &dyn Fn(&str) -> bool| {
        let Some(value) = value else { return Ok(()) };
        if !allowed(value) {
            return Err(format!("“{value}” cannot be used here."));
        }
        arguments.extend([flag.to_string(), value.clone()]);
        Ok(())
    };
    option("--model", &turn.model, &|model| MODELS.contains(&model))?;
    option("--effort", &turn.effort, &|effort| EFFORTS.contains(&effort))?;
    option("--resume", &turn.session, &plain)?;
    Ok(arguments)
}

fn binary(app: &AppHandle) -> Option<PathBuf> {
    let name = if cfg!(windows) { "claude.exe" } else { "claude" };
    let installed = app.path().home_dir().ok().map(|home| home.join(".local").join("bin"));
    let listed = std::env::var_os("PATH").map(|path| std::env::split_paths(&path).collect::<Vec<_>>());
    installed.into_iter().chain(listed.into_iter().flatten()).map(|folder| folder.join(name)).find(|file| file.is_file())
}

fn claude(app: &AppHandle) -> Result<Command, String> {
    let file = binary(app).ok_or_else(|| "Claude Code is not installed yet.".to_string())?;
    let account = app.path().app_data_dir().map_err(|error| error.to_string())?.join("claude");
    fs::create_dir_all(&account).map_err(|error| format!("The assistant’s folder could not be created: {error}"))?;
    let mut command = quiet(Command::new(file));
    command.env("CLAUDE_CONFIG_DIR", account);
    Ok(command)
}

fn quiet(mut command: Command) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    command
}

fn installer() -> Command {
    let mut command;
    if cfg!(windows) {
        command = Command::new("powershell");
        command.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", "irm https://claude.ai/install.ps1 | iex"]);
    } else {
        command = Command::new("sh");
        command.args(["-c", "curl -fsSL https://claude.ai/install.sh | bash"]);
    }
    quiet(command)
}

fn start(agent: &Agent, command: &mut Command) -> Result<(), String> {
    let mut slot = agent.child.lock().map_err(|error| error.to_string())?;
    if slot.is_some() {
        return Err("The assistant is still busy. Wait for it to finish, or stop it.".into());
    }
    *slot = Some(command.spawn().map_err(|error| format!("Claude Code could not be started: {error}"))?);
    Ok(())
}

fn finish(agent: &Agent) -> Result<bool, String> {
    loop {
        let mut slot = agent.child.lock().map_err(|error| error.to_string())?;
        let Some(child) = slot.as_mut() else { return Ok(false) };
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            *slot = None;
            return Ok(status.success());
        }
        drop(slot);
        sleep(POLL);
    }
}

fn sign_in_link(line: &str) -> Option<&str> {
    line.split_whitespace().find(|word| word.starts_with("https://"))
}

fn sign_in_code(code: &str) -> Result<&str, String> {
    let code = code.trim();
    if code.is_empty() || code.len() > 1024 || !code.chars().all(|c| c.is_ascii_graphic()) {
        return Err("That does not look like a sign-in code. Copy it again from the browser.".into());
    }
    Ok(code)
}

fn status(app: &AppHandle) -> Status {
    let answer = claude(app).ok().and_then(|mut command| {
        command.args(["auth", "status", "--json"]).stdout(Stdio::piped()).output().ok()
    });
    let connected = answer
        .as_ref()
        .and_then(|output| serde_json::from_slice::<Value>(&output.stdout).ok())
        .is_some_and(|status| status["loggedIn"] == true);
    Status { installed: answer.is_some(), connected }
}

fn chat_folder(data: &Path, chat: &str) -> Result<PathBuf, String> {
    if !plain(chat) {
        return Err("This chat cannot hold attached files.".into());
    }
    Ok(data.join("assistant").join(chat))
}

fn attachments(app: &AppHandle, chat: &str) -> Result<PathBuf, String> {
    chat_folder(&app.path().app_data_dir().map_err(|error| error.to_string())?, chat)
}

fn attach(folder: &Path, file: &Path) -> Result<Attachment, String> {
    let name = file.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let unreadable = |error| format!("“{name}” could not be attached: {error}");
    if fs::metadata(file).map_err(unreadable)?.len() > MAX_ATTACHMENT_BYTES {
        return Err(format!("“{name}” is too large to attach. The limit is 32 MB."));
    }
    fs::create_dir_all(folder).map_err(unreadable)?;
    let copy = folder.join(format!("{}-{name}", fs::read_dir(folder).map_err(unreadable)?.count() + 1));
    fs::copy(file, &copy).map_err(unreadable)?;
    Ok(Attachment { name, path: copy.to_string_lossy().into_owned() })
}

#[tauri::command(async)]
pub fn agent_status(app: AppHandle) -> Result<Status, String> {
    Ok(status(&app))
}

#[tauri::command(async)]
pub fn agent_install(app: AppHandle) -> Result<(), String> {
    let agent = app.state::<Agent>();
    start(&agent, &mut installer())?;
    if finish(&agent)? && binary(&app).is_some() {
        return Ok(());
    }
    Err("Claude Code could not be installed. Check your internet connection and try again.".into())
}

#[tauri::command(async)]
pub fn agent_connect(app: AppHandle, method: Option<String>, link: Channel<String>) -> Result<Status, String> {
    let agent = app.state::<Agent>();
    let mut command = claude(&app)?;
    command.args(["auth", "login"]);
    if let Some(method) = method.filter(|method| SIGN_INS.contains(&method.as_str())) {
        command.arg(format!("--{method}"));
    }
    command.stdin(Stdio::piped()).stdout(Stdio::piped());
    start(&agent, &mut command)?;
    let pipes = agent
        .child
        .lock()
        .map_err(|error| error.to_string())?
        .as_mut()
        .and_then(|child| child.stdin.take().zip(child.stdout.take()));
    if let Some((input, output)) = pipes {
        *agent.input.lock().map_err(|error| error.to_string())? = Some(input);
        spawn(move || {
            for line in BufReader::new(output).lines().map_while(Result::ok) {
                if let Some(found) = sign_in_link(&line) {
                    let _ = link.send(found.to_string());
                }
            }
        });
    }
    finish(&agent)?;
    *agent.input.lock().map_err(|error| error.to_string())? = None;
    Ok(status(&app))
}

#[tauri::command]
pub fn agent_code(agent: State<Agent>, code: String) -> Result<(), String> {
    let code = sign_in_code(&code)?;
    let mut input = agent.input.lock().map_err(|error| error.to_string())?;
    let pipe = input.as_mut().ok_or_else(|| "Press Connect first, then paste the code.".to_string())?;
    writeln!(pipe, "{code}").and_then(|()| pipe.flush()).map_err(|error| format!("The code could not be passed on: {error}"))
}

#[tauri::command(async)]
pub fn agent_disconnect(app: AppHandle) -> Result<Status, String> {
    claude(&app)?.args(["auth", "logout"]).status().map_err(|error| format!("Could not disconnect: {error}"))?;
    Ok(status(&app))
}

#[tauri::command]
pub fn agent_cancel(agent: State<Agent>) -> Result<(), String> {
    if let Some(mut child) = agent.child.lock().map_err(|error| error.to_string())?.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    *agent.input.lock().map_err(|error| error.to_string())? = None;
    Ok(())
}

#[tauri::command(async)]
pub fn agent_run(app: AppHandle, turn: Turn, events: Channel<Event>) -> Result<(), String> {
    let agent = app.state::<Agent>();
    let root = app.state::<Workspace>().root()?;
    let folder = attachments(&app, &turn.chat)?;
    fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    let mut command = claude(&app)?;
    command.args(arguments(&turn, &folder)?).current_dir(&root).stdin(Stdio::piped()).stdout(Stdio::piped());
    start(&agent, &mut command)?;
    let pipes = agent
        .child
        .lock()
        .map_err(|error| error.to_string())?
        .as_mut()
        .and_then(|child| child.stdin.take().zip(child.stdout.take()));
    let Some((mut input, output)) = pipes else { return Ok(()) };
    let _ = input.write_all(turn.prompt.as_bytes());
    drop(input);
    let mut done = false;
    for event in BufReader::new(output).lines().map_while(Result::ok).filter_map(|line| parse(&root, &line)) {
        done |= matches!(event, Event::Done { .. });
        events.send(event).map_err(|error| error.to_string())?;
    }
    let stopped = agent.child.lock().map_err(|error| error.to_string())?.is_none();
    finish(&agent)?;
    if done || stopped {
        return Ok(());
    }
    Err("Claude Code stopped without an answer. Check that you are connected and online, then try again.".into())
}

#[tauri::command(async)]
pub fn agent_attach(app: AppHandle, chat: String) -> Result<Vec<Attachment>, String> {
    let folder = attachments(&app, &chat)?;
    let files = app.dialog().file().blocking_pick_files().unwrap_or_default();
    files
        .into_iter()
        .map(|file| attach(&folder, &file.into_path().map_err(|error| error.to_string())?))
        .collect()
}

#[tauri::command]
pub fn agent_forget(app: AppHandle, chat: String) -> Result<(), String> {
    let folder = attachments(&app, &chat)?;
    if folder.exists() {
        fs::remove_dir_all(&folder).map_err(|error| format!("The attached files could not be cleared: {error}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECORDED: &str = r#"{"type":"system","subtype":"init","cwd":"/w","session_id":"8ec00c1e-4fd4-499d-b38c-1b893b2fac63","tools":["Edit","Glob","Grep","Read","Write"]}
{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}}
{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","resetsAt":1791893400}}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_01","name":"Read","input":{"file_path":"/w/Work/Notes/Monday.typ"}}]}}
{"type":"user","message":{"role":"user","content":[{"tool_use_id":"toolu_01","type":"tool_result","content":"1\t= Monday\n2\t"}]},"tool_use_result":{"type":"text","file":{"filePath":"/w/Work/Notes/Monday.typ","content":"= Monday\n","numLines":2}}}
{"type":"system","subtype":"permission_denied","tool_name":"Read","message":"/tmp/outside.txt is outside /w"}
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"/tmp/outside.txt is outside /w","is_error":true,"tool_use_id":"toolu_02"}]},"tool_use_result":"Error: /tmp/outside.txt is outside /w"}
{"type":"stream_event","event":{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_03","name":"Edit","input":{}}}}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_03","name":"Edit","input":{"replace_all":false,"file_path":"/w/Work/Notes/Monday.typ","old_string":"= Monday\n","new_string":"= Monday\n- milk\n"}}]}}
{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}}
{"type":"user","message":{"role":"user","content":[{"tool_use_id":"toolu_03","type":"tool_result","content":"The file has been updated successfully."}]},"tool_use_result":{"filePath":"/w/Work/Notes/Monday.typ","oldString":"= Monday\n","newString":"= Monday\n- milk\n"}}
{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"I"}},"session_id":"8ec00c1e-4fd4-499d-b38c-1b893b2fac63"}
{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" added \"milk\"."}}}
{"type":"stream_event","event":{"type":"message_stop"}}
{"type":"result","stop_reason":"end_turn","session_id":"8ec00c1e-4fd4-499d-b38c-1b893b2fac63","is_error":false,"subtype":"success","result":"I added \"milk\"."}
not json at all"#;

    fn turn() -> Turn {
        Turn { chat: "chat-1".into(), prompt: "hello".into(), system: "write Typst".into(), session: None, model: None, effort: None }
    }

    #[test]
    fn a_recorded_stream_becomes_text_file_notes_and_one_ending() {
        let events: Vec<Event> = RECORDED.lines().filter_map(|line| parse(Path::new("/w"), line)).collect();
        let step = |tool: &str, page: &str| Event::Step { tool: tool.into(), page: page.into() };
        let expected = [
            step("Thinking", ""),
            step("Read", "Work/Notes/Monday"),
            Event::Read { page: "Work/Notes/Monday".into() },
            step("Edit", ""),
            step("Edit", "Work/Notes/Monday"),
            Event::Edited { page: "Work/Notes/Monday".into() },
            Event::Text { text: "I".into() },
            Event::Text { text: " added \"milk\".".into() },
            Event::Done { session: "8ec00c1e-4fd4-499d-b38c-1b893b2fac63".into(), failure: None },
        ];
        assert_eq!(events, expected);
    }

    #[test]
    fn the_sign_in_link_is_found_and_a_code_is_one_plain_word() {
        let printed = "If the browser didn't open, visit: https://claude.com/cai/oauth/authorize?code=true&client_id=9d1c";
        assert_eq!(sign_in_link(printed), Some("https://claude.com/cai/oauth/authorize?code=true&client_id=9d1c"));
        assert_eq!(sign_in_link("Opening browser to sign in…"), None);
        assert_eq!(sign_in_link("Paste code here if prompted > "), None);
        assert_eq!(sign_in_code("  abc_123-DEF#state9 \n"), Ok("abc_123-DEF#state9"));
        for bad in ["", "   ", "two words", "line\nbreak", "tab\there", "ünïcode"] {
            assert!(sign_in_code(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_failed_turn_carries_the_message_claude_code_gave() {
        let limit = r#"{"type":"result","is_error":true,"session_id":"s","result":"You've hit your usage limit"}"#;
        let silent = r#"{"type":"result","subtype":"error_max_turns","is_error":true,"session_id":"s"}"#;
        let failure = |line| match parse(Path::new("/w"), line) {
            Some(Event::Done { failure, .. }) => failure,
            other => panic!("{other:?}"),
        };
        assert_eq!(failure(limit).as_deref(), Some("You've hit your usage limit"));
        assert_eq!(failure(silent).as_deref(), Some("The assistant stopped before it finished."));
    }

    #[test]
    fn a_file_outside_the_workspace_is_shown_by_name_only() {
        assert_eq!(label(Path::new("/w"), "/data/assistant/1-report.pdf"), "1-report.pdf");
        assert_eq!(label(Path::new("/w"), "/w/Work/Notes/v1.2.typ"), "Work/Notes/v1.2");
    }

    #[test]
    fn the_assistant_gets_file_tools_only_and_no_way_to_ask_for_more() {
        let arguments = arguments(&turn(), Path::new("/data/assistant")).unwrap();
        let after = |flag: &str| arguments[arguments.iter().position(|argument| argument == flag).unwrap() + 1].clone();
        assert_eq!(after("--tools"), "Read,Edit,Write,Glob,Grep");
        assert_eq!(after("--permission-mode"), "dontAsk");
        assert_eq!(after("--permission-prompts"), "none");
        assert!(arguments.contains(&"--restricted".to_string()));
        assert!(arguments.contains(&"Edit(/**/*.typ)".to_string()));
        assert!(!arguments.iter().any(|argument| argument == "--resume" || argument == "--model"));
    }

    #[test]
    fn only_known_models_efforts_and_session_ids_reach_the_command_line() {
        let folder = Path::new("/data/assistant");
        let chosen = Turn {
            session: Some("8ec00c1e-4fd4".into()),
            model: Some("opus".into()),
            effort: Some("high".into()),
            ..turn()
        };
        let arguments = arguments(&chosen, folder).unwrap();
        assert_eq!(arguments[arguments.len() - 6..], ["--model", "opus", "--effort", "high", "--resume", "8ec00c1e-4fd4"]);

        assert!(super::arguments(&Turn { model: Some("--dangerously-skip-permissions".into()), ..turn() }, folder).is_err());
        assert!(super::arguments(&Turn { effort: Some("extreme".into()), ..turn() }, folder).is_err());
        assert!(super::arguments(&Turn { session: Some("x --add-dir /".into()), ..turn() }, folder).is_err());
    }

    #[test]
    fn each_chat_has_its_own_folder_and_an_odd_chat_name_gets_none() {
        let data = Path::new("/data");
        assert_eq!(chat_folder(data, "8ec00c1e-4fd4").unwrap(), Path::new("/data/assistant/8ec00c1e-4fd4"));
        for bad in ["", "..", "../claude", "a/b", "a\\b", "C:"] {
            assert!(chat_folder(data, bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn an_attached_file_is_copied_and_never_replaces_an_earlier_one() {
        let base = std::env::temp_dir().join(format!("znote-attach-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let file = base.join("notes.txt");
        fs::write(&file, "first").unwrap();
        let folder = base.join("assistant");
        let first = attach(&folder, &file).unwrap();
        fs::write(&file, "second").unwrap();
        let second = attach(&folder, &file).unwrap();
        assert_eq!(first.name, "notes.txt");
        assert_ne!(first.path, second.path);
        assert_eq!(fs::read_to_string(first.path).unwrap(), "first");
        assert_eq!(fs::read_to_string(second.path).unwrap(), "second");
        assert!(attach(&folder, &base.join("missing.txt")).is_err());
    }
}

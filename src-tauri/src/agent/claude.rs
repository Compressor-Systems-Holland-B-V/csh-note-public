use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::spawn;

use serde_json::Value;
use tauri::{AppHandle, Manager};

use super::mirror::Mirror;
use super::{
    Agent, Event, Running, Send, Status, Turn, keep, label, last_line, mirror_root, plain, seen,
};
use crate::knowledge::Gate;

pub const ID: &str = "claude";

const MODELS: [&str; 4] = ["fable", "opus", "sonnet", "haiku"];
const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];
const SIGN_INS: [&str; 2] = ["console", "sso"];
const WEB: [&str; 2] = ["WebFetch", "WebSearch"];
const BLOCKED: &str = "On a work computer the installer may be blocked. Ask your IT team to install Claude Code, then press Connect again.";

fn parse(root: &Path, line: &str) -> Vec<Event> {
    let Ok(event) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };
    let one = |event: Option<Event>| event.into_iter().collect();
    match event["type"].as_str().unwrap_or_default() {
        "stream_event" => {
            let delta = &event["event"]["delta"];
            let block = &event["event"]["content_block"];
            one(match (delta["type"].as_str(), block["type"].as_str()) {
                (Some("text_delta"), _) => Some(Event::Text {
                    text: delta["text"].as_str().unwrap_or_default().into(),
                }),
                (_, Some("thinking")) => Some(Event::Step {
                    tool: "Thinking".into(),
                    page: String::new(),
                }),
                (_, Some("tool_use")) => Some(Event::Step {
                    tool: block["name"].as_str().unwrap_or_default().into(),
                    page: String::new(),
                }),
                _ => None,
            })
        }
        "assistant" => event["message"]["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|block| {
                let file = block["input"]["file_path"].as_str()?;
                Some(Event::Step {
                    tool: block["name"].as_str()?.into(),
                    page: label(root, file),
                })
            })
            .collect(),
        "user" => {
            let result = &event["tool_use_result"];
            if let Some(file) = result["filePath"].as_str() {
                let before = result["originalFile"].as_str().map(str::to_string);
                return vec![Event::Edited {
                    page: label(root, file),
                    before,
                }];
            }
            one(result["file"]["filePath"].as_str().map(|file| Event::Read {
                page: label(root, file),
            }))
        }
        "result" => {
            let failed = event["is_error"].as_bool().unwrap_or(true);
            let message = event["result"]
                .as_str()
                .filter(|message| !message.is_empty());
            vec![Event::Done {
                session: event["session_id"].as_str().unwrap_or_default().into(),
                failure: failed.then(|| {
                    message
                        .unwrap_or("The assistant stopped before it finished.")
                        .to_string()
                }),
            }]
        }
        _ => Vec::new(),
    }
}

fn arguments(turn: &Turn, attachments: &Path, system: &Path) -> Result<Vec<String>, String> {
    let files = if turn.read_only {
        "Read,Glob,Grep"
    } else {
        "Read,Edit,Write,Glob,Grep"
    };
    let tools = if turn.web {
        format!("{files},{}", WEB.join(","))
    } else {
        files.to_string()
    };
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
        &tools,
        "--permission-mode",
        "dontAsk",
        "--permission-prompts",
        "none",
        "--allowedTools",
        "Read",
        "Glob",
        "Grep",
    ]
    .map(str::to_string)
    .to_vec();
    if !turn.read_only {
        arguments.push("Edit(/**/*.typ)".into());
    }
    if turn.web {
        arguments.extend(WEB.map(str::to_string));
    }
    arguments.extend(
        [
            "--system-prompt-file",
            &system.to_string_lossy(),
            "--add-dir",
            &attachments.to_string_lossy(),
        ]
        .map(str::to_string),
    );
    if turn.fast {
        arguments.extend(["--settings", r#"{"fastMode":true}"#].map(str::to_string));
    }
    let mut option = |flag: &str, value: &Option<String>, allowed: &dyn Fn(&str) -> bool| {
        let Some(value) = value else { return Ok(()) };
        if !allowed(value) {
            return Err(format!("“{value}” cannot be used here."));
        }
        arguments.extend([flag.to_string(), value.clone()]);
        Ok(())
    };
    option("--model", &turn.model, &|model| MODELS.contains(&model))?;
    option("--effort", &turn.effort, &|effort| {
        EFFORTS.contains(&effort)
    })?;
    option("--resume", &turn.session, &plain)?;
    Ok(arguments)
}

fn binary(app: &AppHandle) -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "claude.exe"
    } else {
        "claude"
    };
    let installed = app
        .path()
        .home_dir()
        .ok()
        .map(|home| home.join(".local").join("bin"));
    let listed =
        std::env::var_os("PATH").map(|path| std::env::split_paths(&path).collect::<Vec<_>>());
    installed
        .into_iter()
        .chain(listed.into_iter().flatten())
        .map(|folder| folder.join(name))
        .find(|file| file.is_file())
}

fn claude(app: &AppHandle) -> Result<Command, String> {
    let file = binary(app).ok_or_else(|| "Claude Code is not installed yet.".to_string())?;
    let account = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("claude");
    fs::create_dir_all(&account)
        .map_err(|error| format!("The assistant’s folder could not be created: {error}"))?;
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
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

fn installer() -> Command {
    let mut command;
    if cfg!(windows) {
        command = Command::new("powershell");
        command.args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "irm https://claude.ai/install.ps1 | iex",
        ]);
    } else {
        command = Command::new("sh");
        command.args(["-c", "curl -fsSL https://claude.ai/install.sh | bash"]);
    }
    quiet(command)
}

fn complaints(errors: Option<impl Read + std::marker::Send + 'static>) -> impl FnOnce() -> String {
    let reader = errors.map(|mut errors| {
        spawn(move || {
            let mut said = Vec::new();
            let _ = errors.read_to_end(&mut said);
            String::from_utf8_lossy(&said).into_owned()
        })
    });
    move || {
        reader
            .and_then(|reader| reader.join().ok())
            .unwrap_or_default()
    }
}

fn install_failure(said: &str) -> String {
    match last_line(said) {
        Some(reason) => format!("Claude Code could not be installed: {reason}\n{BLOCKED}"),
        None => format!(
            "Claude Code could not be installed. Check your internet connection and try again.\n{BLOCKED}"
        ),
    }
}

fn run_failure(said: &str) -> String {
    match last_line(said) {
        Some(reason) => format!("Claude Code stopped without an answer: {reason}"),
        None => "Claude Code stopped without an answer. Check that you are connected and online, then try again.".into(),
    }
}

fn sign_in_link(line: &str) -> Option<&str> {
    line.split_whitespace()
        .find(|word| word.starts_with("https://"))
}

fn sign_in_code(code: &str) -> Result<&str, String> {
    let code = code.trim();
    if code.is_empty() || code.len() > 1024 || !code.chars().all(|c| c.is_ascii_graphic()) {
        return Err(
            "That does not look like a sign-in code. Copy it again from the browser.".into(),
        );
    }
    Ok(code)
}

pub fn status(app: &AppHandle) -> Status {
    let answer = claude(app).ok().and_then(|mut command| {
        command
            .args(["auth", "status", "--json"])
            .stdout(Stdio::piped())
            .output()
            .ok()
    });
    let connected = answer
        .as_ref()
        .and_then(|output| serde_json::from_slice::<Value>(&output.stdout).ok())
        .is_some_and(|status| status["loggedIn"] == true);
    Status {
        installed: answer.is_some(),
        connected,
        url: None,
    }
}

pub fn install(app: &AppHandle, running: &Running) -> Result<(), String> {
    let mut child = installer()
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("The installer could not be started: {error}\n{BLOCKED}"))?;
    let said = complaints(child.stderr.take());
    running.hold(child)?;
    if running.wait()? && binary(app).is_some() {
        return Ok(());
    }
    if running.stopped() {
        return Err("The download was cancelled.".into());
    }
    Err(install_failure(&said()))
}

pub fn connect(
    app: &AppHandle,
    agent: &Agent,
    running: &Running,
    method: Option<String>,
    show: &(dyn Fn(String) + Sync),
) -> Result<Status, String> {
    let mut command = claude(app)?;
    command.args(["auth", "login"]);
    if let Some(method) = method.filter(|method| SIGN_INS.contains(&method.as_str())) {
        command.arg(format!("--{method}"));
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Claude Code could not be started: {error}"))?;
    let output = child.stdout.take();
    *agent.input.lock().map_err(|error| error.to_string())? = child.stdin.take();
    running.hold(child)?;
    std::thread::scope(|scope| {
        scope.spawn(|| {
            for line in output
                .into_iter()
                .flat_map(|output| BufReader::new(output).lines().map_while(Result::ok))
            {
                if let Some(found) = sign_in_link(&line) {
                    show(found.to_string());
                }
            }
        });
        running.wait()
    })?;
    *agent.input.lock().map_err(|error| error.to_string())? = None;
    Ok(status(app))
}

pub fn code(agent: &Agent, code: &str) -> Result<(), String> {
    let code = sign_in_code(code)?;
    let mut input = agent.input.lock().map_err(|error| error.to_string())?;
    let pipe = input
        .as_mut()
        .ok_or_else(|| "Press Connect first, then paste the code.".to_string())?;
    writeln!(pipe, "{code}")
        .and_then(|()| pipe.flush())
        .map_err(|error| format!("The code could not be passed on: {error}"))
}

pub fn disconnect(app: &AppHandle) -> Result<Status, String> {
    claude(app)?
        .args(["auth", "logout"])
        .status()
        .map_err(|error| format!("Could not disconnect: {error}"))?;
    Ok(status(app))
}

fn relay(
    lines: impl Iterator<Item = String>,
    place: &Path,
    root: &Path,
    mut mirror: Option<&mut Mirror>,
    gate: &Gate,
    send: Send,
) -> Result<Option<Event>, String> {
    let mut done = None;
    for event in lines.flat_map(|line| parse(place, &line)) {
        let event = match (mirror.as_deref_mut(), event) {
            (Some(mirror), Event::Edited { page, .. }) => match mirror.sync(root, &page, gate)? {
                Some(event) => event,
                None => continue,
            },
            (Some(mirror), Event::Read { page }) => {
                send(seen(&page, &mirror.text(&page)))?;
                Event::Read { page }
            }
            (_, event @ Event::Done { .. }) => {
                done = Some(event);
                continue;
            }
            (_, event) => event,
        };
        send(event)?;
    }
    Ok(done)
}

pub fn run(
    app: &AppHandle,
    running: &Running,
    root: &Path,
    folder: &Path,
    turn: &Turn,
    gate: &Gate,
    send: Send,
) -> Result<(), String> {
    let system = folder.join("system.txt");
    keep(&system, turn.system.as_bytes())
        .map_err(|error| format!("The assistant could not be prepared: {error}"))?;
    let mut mirror = match gate.on() {
        true => Some(Mirror::build(
            mirror_root(app, root)?,
            root,
            folder,
            &turn.chat,
            gate,
        )?),
        false => None,
    };
    let (place, attachments, prompt) = match &mirror {
        Some(mirror) => {
            let veiled = gate.veil(&mirror.rewrite(&turn.prompt))?;
            send(Event::Veiled {
                kinds: veiled.kinds,
            })?;
            send(seen("prompt", &veiled.text))?;
            (mirror.root.clone(), mirror.attachments.clone(), veiled.text)
        }
        None => (
            root.to_path_buf(),
            folder.to_path_buf(),
            turn.prompt.clone(),
        ),
    };
    let mut command = claude(app)?;
    command
        .args(arguments(turn, &attachments, &system)?)
        .current_dir(&place)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| format!("Claude Code could not be started: {error}"))?;
    let pipes = child.stdin.take().zip(child.stdout.take());
    let said = complaints(child.stderr.take());
    running.hold(child)?;
    let (mut input, output) = pipes.ok_or("Claude Code could not be reached.")?;
    let _ = input.write_all(prompt.as_bytes());
    drop(input);
    let lines = BufReader::new(output).lines().map_while(Result::ok);
    let done = relay(lines, &place, root, mirror.as_mut(), gate, send)?;
    running.wait()?;
    if let Some(mirror) = mirror.as_mut() {
        for event in mirror.finish(root, gate)? {
            send(event)?;
        }
    }
    if let Some(done) = done {
        return send(done);
    }
    if running.stopped() {
        return Ok(());
    }
    Err(run_failure(&said()))
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
{"type":"user","message":{"role":"user","content":[{"tool_use_id":"toolu_03","type":"tool_result","content":"The file has been updated successfully."}]},"tool_use_result":{"filePath":"/w/Work/Notes/Monday.typ","oldString":"= Monday\n","newString":"= Monday\n- milk\n","originalFile":"= Monday\n"}}
{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"I"}},"session_id":"8ec00c1e-4fd4-499d-b38c-1b893b2fac63"}
{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" added \"milk\"."}}}
{"type":"stream_event","event":{"type":"message_stop"}}
{"type":"result","stop_reason":"end_turn","session_id":"8ec00c1e-4fd4-499d-b38c-1b893b2fac63","is_error":false,"subtype":"success","result":"I added \"milk\"."}
not json at all"#;

    fn turn() -> Turn {
        Turn {
            chat: "chat-1".into(),
            prompt: "hello".into(),
            system: "write Typst".into(),
            session: None,
            provider: ID.into(),
            model: None,
            effort: None,
            read_only: false,
            fast: false,
            web: false,
        }
    }

    fn arguments(turn: &Turn) -> Result<Vec<String>, String> {
        super::arguments(
            turn,
            Path::new("/data/assistant"),
            Path::new("/data/assistant/system.txt"),
        )
    }

    #[test]
    fn a_recorded_stream_becomes_text_file_notes_and_one_ending() {
        let events: Vec<Event> = RECORDED
            .lines()
            .flat_map(|line| parse(Path::new("/w"), line))
            .collect();
        let step = |tool: &str, page: &str| Event::Step {
            tool: tool.into(),
            page: page.into(),
        };
        let expected = [
            step("Thinking", ""),
            step("Read", "Work/Notes/Monday"),
            Event::Read {
                page: "Work/Notes/Monday".into(),
            },
            step("Edit", ""),
            step("Edit", "Work/Notes/Monday"),
            Event::Edited {
                page: "Work/Notes/Monday".into(),
                before: Some("= Monday\n".into()),
            },
            Event::Text { text: "I".into() },
            Event::Text {
                text: " added \"milk\".".into(),
            },
            Event::Done {
                session: "8ec00c1e-4fd4-499d-b38c-1b893b2fac63".into(),
                failure: None,
            },
        ];
        assert_eq!(events, expected);
    }

    #[test]
    fn with_veiling_on_an_edit_in_the_mirror_lands_unveiled_and_a_conflicting_one_waits() {
        use crate::knowledge::{Knowledge, Options};
        use crate::privacy::glossary;
        use crate::workspace;
        use std::sync::Mutex;

        let base = std::env::temp_dir().join(format!("znote-claude-mirror-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let (real, place, chat) = (base.join("real"), base.join("mirror"), base.join("chat"));
        fs::create_dir_all(&chat).unwrap();
        fs::create_dir_all(&real).unwrap();
        workspace::ensure(&real, "Work/Notes/Monday").unwrap();
        workspace::write_text(&real, "Work/Notes/Monday", "= Monday\nAcme\n").unwrap();
        let ttl = "@prefix skos: <http://www.w3.org/2004/02/skos/core#> .
<urn:c> skos:prefLabel \"Customer\" ; skos:topConceptOf <urn:s> .
<urn:a> skos:prefLabel \"Acme\" ; skos:broader <urn:c> .";
        let glossary = glossary::parse(ttl.as_bytes(), oxrdfio::RdfFormat::Turtle).unwrap();
        let knowledge = Knowledge::in_memory(
            &glossary,
            Options {
                veil: true,
                ..Options::default()
            },
        );
        let gate = knowledge.gate().unwrap();
        let mut mirror = Mirror::build(place.clone(), &real, &chat, "chat-1", &gate).unwrap();
        assert_eq!(mirror.text("Work/Notes/Monday"), "= Monday\n⟦CUSTOMER_1⟧\n");
        let recorded = |edited: &str| {
            fs::write(place.join("Work/Notes/Monday.typ"), edited).unwrap();
            RECORDED
                .replace("/w/", &format!("{}/", place.to_string_lossy()))
                .replace("= Monday\\n- milk\\n", &edited.replace('\n', "\\n"))
        };
        let seen = Mutex::new(Vec::new());
        let send = |event: Event| {
            seen.lock().unwrap().push(event);
            Ok(())
        };
        let lines = recorded("= Monday\n⟦CUSTOMER_1⟧ and milk\n");
        let done = relay(
            lines.lines().map(str::to_string),
            &place,
            &real,
            Some(&mut mirror),
            &gate,
            &send,
        )
        .unwrap();
        assert!(matches!(done, Some(Event::Done { failure: None, .. })));
        assert_eq!(
            workspace::read_text(&real, "Work/Notes/Monday").unwrap(),
            "= Monday\nAcme and milk\n"
        );
        let events = seen.lock().unwrap().clone();
        assert!(events.contains(&Event::Edited {
            page: "Work/Notes/Monday".into(),
            before: Some("= Monday\nAcme\n".into())
        }));
        assert!(events.iter().any(|event| matches!(event, Event::Saw { label, text } if label == "Work/Notes/Monday" && text.contains("⟦CUSTOMER_1⟧"))));

        workspace::write_text(&real, "Work/Notes/Monday", "the user typed this\n").unwrap();
        seen.lock().unwrap().clear();
        let lines = recorded("= Monday\n⟦CUSTOMER_1⟧ only\n");
        relay(
            lines.lines().map(str::to_string),
            &place,
            &real,
            Some(&mut mirror),
            &gate,
            &send,
        )
        .unwrap();
        assert_eq!(
            workspace::read_text(&real, "Work/Notes/Monday").unwrap(),
            "the user typed this\n"
        );
        assert!(seen.lock().unwrap().contains(&Event::Conflict {
            page: "Work/Notes/Monday".into(),
            theirs: "= Monday\nAcme only\n".into()
        }));
    }

    #[test]
    fn every_file_the_assistant_opens_in_one_message_is_reported() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Reading both."},{"type":"tool_use","name":"Read","input":{"file_path":"/w/A/B/One.typ"}},{"type":"tool_use","name":"Read","input":{"file_path":"/w/A/B/Two.typ"}}]}}"#;
        let pages: Vec<Event> = parse(Path::new("/w"), line);
        let step = |page: &str| Event::Step {
            tool: "Read".into(),
            page: page.into(),
        };
        assert_eq!(pages, [step("A/B/One"), step("A/B/Two")]);
    }

    #[test]
    fn the_sign_in_link_is_found_and_a_code_is_one_plain_word() {
        let printed = "If the browser didn't open, visit: https://claude.com/cai/oauth/authorize?code=true&client_id=9d1c";
        assert_eq!(
            sign_in_link(printed),
            Some("https://claude.com/cai/oauth/authorize?code=true&client_id=9d1c")
        );
        assert_eq!(sign_in_link("Opening browser to sign in…"), None);
        assert_eq!(sign_in_link("Paste code here if prompted > "), None);
        assert_eq!(
            sign_in_code("  abc_123-DEF#state9 \n"),
            Ok("abc_123-DEF#state9")
        );
        for bad in [
            "",
            "   ",
            "two words",
            "line\nbreak",
            "tab\there",
            "ünïcode",
        ] {
            assert!(sign_in_code(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_failed_turn_carries_the_message_claude_code_gave() {
        let limit = r#"{"type":"result","is_error":true,"session_id":"s","result":"You've hit your usage limit"}"#;
        let silent =
            r#"{"type":"result","subtype":"error_max_turns","is_error":true,"session_id":"s"}"#;
        let failure = |line| match parse(Path::new("/w"), line).pop() {
            Some(Event::Done { failure, .. }) => failure,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            failure(limit).as_deref(),
            Some("You've hit your usage limit")
        );
        assert_eq!(
            failure(silent).as_deref(),
            Some("The assistant stopped before it finished.")
        );
    }

    #[test]
    fn what_claude_code_or_its_installer_complained_about_reaches_the_user() {
        assert!(
            run_failure("warning: x\nError: Invalid API key\n\n")
                .ends_with("without an answer: Error: Invalid API key")
        );
        assert!(run_failure("").contains("connected and online"));
        assert!(
            install_failure("irm : Access is denied\n")
                .starts_with("Claude Code could not be installed: irm : Access is denied")
        );
        assert!(install_failure("").contains("IT team"));
    }

    #[test]
    fn the_assistant_gets_file_tools_only_and_no_way_to_ask_for_more() {
        let arguments = arguments(&turn()).unwrap();
        let after = |flag: &str| {
            arguments[arguments
                .iter()
                .position(|argument| argument == flag)
                .unwrap()
                + 1]
            .clone()
        };
        assert_eq!(after("--tools"), "Read,Edit,Write,Glob,Grep");
        assert_eq!(after("--permission-mode"), "dontAsk");
        assert_eq!(after("--permission-prompts"), "none");
        assert_eq!(after("--system-prompt-file"), "/data/assistant/system.txt");
        assert!(arguments.contains(&"--restricted".to_string()));
        assert!(arguments.contains(&"Edit(/**/*.typ)".to_string()));
        assert!(!arguments.iter().any(|argument| {
            ["--resume", "--model", "--settings", "write Typst"].contains(&argument.as_str())
        }));
    }

    #[test]
    fn a_read_only_turn_has_no_tool_that_can_change_a_page() {
        let arguments = arguments(&Turn {
            read_only: true,
            fast: true,
            ..turn()
        })
        .unwrap();
        assert!(arguments.contains(&"Read,Glob,Grep".to_string()));
        assert!(
            !arguments
                .iter()
                .any(|argument| argument.contains("Edit") || argument.contains("Write"))
        );
        assert!(arguments.contains(&r#"{"fastMode":true}"#.to_string()));
    }

    #[test]
    fn the_web_tools_are_offered_only_when_the_chat_allows_the_internet() {
        assert!(
            !arguments(&turn())
                .unwrap()
                .iter()
                .any(|argument| argument.contains("Web"))
        );
        let arguments = arguments(&Turn {
            read_only: true,
            web: true,
            ..turn()
        })
        .unwrap();
        assert!(arguments.contains(&"Read,Glob,Grep,WebFetch,WebSearch".to_string()));
        let allowed = arguments
            .iter()
            .position(|argument| argument == "--allowedTools")
            .unwrap();
        assert_eq!(
            arguments[allowed + 1..allowed + 6],
            ["Read", "Glob", "Grep", "WebFetch", "WebSearch"]
        );
    }

    #[test]
    fn only_known_models_efforts_and_session_ids_reach_the_command_line() {
        let chosen = Turn {
            session: Some("8ec00c1e-4fd4".into()),
            model: Some("opus".into()),
            effort: Some("high".into()),
            ..turn()
        };
        let arguments = arguments(&chosen).unwrap();
        assert_eq!(
            arguments[arguments.len() - 6..],
            [
                "--model",
                "opus",
                "--effort",
                "high",
                "--resume",
                "8ec00c1e-4fd4"
            ]
        );

        assert!(
            super::arguments(
                &Turn {
                    model: Some("--dangerously-skip-permissions".into()),
                    ..turn()
                },
                Path::new("/d"),
                Path::new("/d/s")
            )
            .is_err()
        );
        assert!(
            super::arguments(
                &Turn {
                    effort: Some("extreme".into()),
                    ..turn()
                },
                Path::new("/d"),
                Path::new("/d/s")
            )
            .is_err()
        );
        assert!(
            super::arguments(
                &Turn {
                    session: Some("x --add-dir /".into()),
                    ..turn()
                },
                Path::new("/d"),
                Path::new("/d/s")
            )
            .is_err()
        );
    }
}

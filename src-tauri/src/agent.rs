mod claude;
mod mirror;
mod openai;
mod secrets;
mod web;

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::sleep;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

pub use openai::{connection, refused};

use crate::knowledge::{Gate, Knowledge, LIGHT, STRICT};
use crate::policy;
use crate::privacy::{Restored, Stream};
use crate::project;
use crate::workspace::Workspace;
use crate::{compile, extract, workspace};

const MAX_ATTACHMENT_BYTES: u64 = 32 * 1024 * 1024;
const MAX_CHAT_BYTES: usize = 16 * 1024 * 1024;
const POLL: Duration = Duration::from_millis(150);
const SETUP: &str = "setup";
const MARK: &str = "veil.txt";
const LEVELS: &str = "levels.json";
const PAGES: &str = "pages.txt";
const OFF: &str = "off";
const MAX_SEEN: usize = 20_000;
const TOKENS: &str = "Some names, numbers and other details in this conversation are hidden behind tokens such as ⟦CUSTOMER_3⟧ or ⟦PERSON_1~2⟧. Each token stands for one piece of hidden text, and tokens with the same number stand for the same thing. Keep every token exactly as it is written, brackets included, when you quote, edit or write text. Never make up new tokens and never guess what a token hides.";

struct Job {
    child: Option<Child>,
    stopped: Arc<AtomicBool>,
}

impl Job {
    fn stop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[derive(Default)]
pub struct Agent {
    jobs: Mutex<HashMap<String, Job>>,
    input: Mutex<Option<ChildStdin>>,
}

pub struct Running<'a> {
    agent: &'a Agent,
    key: String,
    stopped: Arc<AtomicBool>,
}

impl Agent {
    fn jobs(&self) -> Result<MutexGuard<'_, HashMap<String, Job>>, String> {
        self.jobs.lock().map_err(|error| error.to_string())
    }

    fn begin(&self, key: &str) -> Result<Running<'_>, String> {
        let mut jobs = self.jobs()?;
        if jobs.contains_key(key) {
            return Err("The assistant is still busy. Wait for it to finish, or stop it.".into());
        }
        let stopped = Arc::new(AtomicBool::new(false));
        jobs.insert(
            key.to_string(),
            Job {
                child: None,
                stopped: stopped.clone(),
            },
        );
        Ok(Running {
            agent: self,
            key: key.to_string(),
            stopped,
        })
    }

    fn cancel(&self, key: &str) -> Result<(), String> {
        if let Some(job) = self.jobs()?.get_mut(key) {
            job.stop();
        }
        Ok(())
    }

    pub fn cancel_all(&self) {
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.values_mut().for_each(Job::stop);
        }
    }
}

impl Running<'_> {
    fn hold(&self, child: Child) -> Result<(), String> {
        let mut jobs = self.agent.jobs()?;
        match jobs.get_mut(&self.key) {
            Some(job) if !self.stopped() => job.child = Some(child),
            _ => Job {
                child: Some(child),
                stopped: self.stopped.clone(),
            }
            .stop(),
        }
        Ok(())
    }

    fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }

    fn wait(&self) -> Result<bool, String> {
        loop {
            let mut jobs = self.agent.jobs()?;
            let Some(child) = jobs.get_mut(&self.key).and_then(|job| job.child.as_mut()) else {
                return Ok(false);
            };
            if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
                return Ok(status.success());
            }
            drop(jobs);
            sleep(POLL);
        }
    }
}

impl Drop for Running<'_> {
    fn drop(&mut self) {
        if let Ok(mut jobs) = self.agent.jobs.lock()
            && let Some(mut job) = jobs.remove(&self.key)
        {
            job.stop();
        }
    }
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Status {
    installed: bool,
    connected: bool,
    url: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Event {
    Text {
        text: String,
    },
    Thought {
        text: String,
    },
    Step {
        tool: String,
        page: String,
    },
    Edited {
        page: String,
        before: Option<String>,
    },
    Read {
        page: String,
    },
    Veiled {
        kinds: std::collections::BTreeMap<String, usize>,
    },
    Restored {
        texts: Vec<Restored>,
    },
    Saw {
        label: String,
        text: String,
    },
    Conflict {
        page: String,
        theirs: String,
    },
    Done {
        session: String,
        failure: Option<String>,
    },
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    chat: String,
    prompt: String,
    system: String,
    session: Option<String>,
    provider: String,
    model: Option<String>,
    effort: Option<String>,
    read_only: bool,
    fast: bool,
    web: bool,
    veil: bool,
    // The pages and folders the user pointed this turn to, and the page open in the editor.
    #[serde(default)]
    pages: Vec<String>,
    #[serde(default)]
    page: Option<String>,
}

type Send<'a> = &'a dyn Fn(Event) -> Result<(), String>;
// The compile errors on a page, read from the workspace, as `described` writes them.
type Check<'a> = &'a dyn Fn(&str) -> Vec<String>;

#[derive(Serialize)]
pub struct Attachment {
    name: String,
    id: String,
    path: String,
    text: Option<String>,
    level: u8,
}

pub fn seen(label: &str, text: &str) -> Event {
    let mut shown: String = text.chars().take(MAX_SEEN).collect();
    if shown.len() < text.len() {
        shown.push_str("\n…");
    }
    Event::Saw {
        label: label.into(),
        text: shown,
    }
}

// Only the answer is unveiled. Thinking is shown with its tokens, exactly as the model wrote it.
fn unveiled(gate: &Gate, stream: &Mutex<Stream>, send: Send, event: Event) -> Result<(), String> {
    if !gate.on() {
        return send(event);
    }
    let mut stream = stream.lock().map_err(|error| error.to_string())?;
    let pass = |released: String| -> Result<(), String> {
        if released.is_empty() {
            return Ok(());
        }
        let (real, texts) = gate.unveil(&released);
        send(Event::Text { text: real })?;
        if !texts.is_empty() {
            send(Event::Restored { texts })?;
        }
        Ok(())
    };
    match event {
        Event::Text { text: piece } => pass(stream.push(&piece)),
        thought @ Event::Thought { .. } => send(thought),
        other => {
            pass(stream.finish())?;
            send(other)
        }
    }
}

// A chat sees only the page open in the editor, the pages and folders the user pointed it to
// and the pages it wrote itself. The last two are kept in the chat folder for later turns.
pub fn in_scope(folder: &Path, open: Option<&str>, page: &str) -> bool {
    open == Some(page)
        || fs::read_to_string(folder.join(PAGES)).is_ok_and(|kept| {
            kept.lines()
                .any(|given| page == given || page.starts_with(&format!("{given}/")))
        })
}

fn add_to_scope(folder: &Path, pages: &[String]) -> Result<(), String> {
    let mut kept = fs::read_to_string(folder.join(PAGES)).unwrap_or_default();
    let before = kept.len();
    for page in pages.iter().map(|page| page.trim_matches('/')) {
        if !page.is_empty() && !page.contains('\n') && !kept.lines().any(|line| line == page) {
            kept.push_str(page);
            kept.push('\n');
        }
    }
    if kept.len() == before {
        return Ok(());
    }
    keep(&folder.join(PAGES), kept.as_bytes())
        .map_err(|error| format!("The chat could not be prepared: {error}"))
}

fn marked(folder: &Path, gate: &Gate, resumed: bool) -> Result<(), String> {
    let wanted = gate.id().unwrap_or_else(|| OFF.into());
    let file = folder.join(MARK);
    let mark = match fs::read_to_string(&file) {
        Ok(mark) => mark.trim().to_string(),
        Err(_) if resumed || folder.join("history.json").exists() => OFF.into(),
        Err(_) => {
            keep(&file, wanted.as_bytes())
                .map_err(|error| format!("The chat could not be prepared: {error}"))?;
            return Ok(());
        }
    };
    match (mark == wanted, mark == OFF, wanted == OFF) {
        (true, _, _) => Ok(()),
        (_, true, _) => Err("This chat started without hiding names. Start a new chat to hide them.".into()),
        (_, _, true) => Err("This chat hid names from the assistant. Turn hiding back on in Settings, or start a new chat.".into()),
        _ => Err("This chat hid names with another knowledge file. Switch back to it, or start a new chat.".into()),
    }
}

fn prepared(knowledge: &Knowledge, gate: &Gate, turn: &Turn) -> Result<Turn, String> {
    let mut turn = turn.clone();
    let asked = gate.unveil(&turn.prompt).0;
    let notes = knowledge.notes(&asked)?;
    if !notes.is_empty() {
        turn.prompt = format!("{notes}\n\n{}", turn.prompt);
    }
    if gate.on() {
        // The system prompt carries the templates of imported add-ons, which can name a company.
        turn.system = format!("{}\n\n{TOKENS}", gate.veil(&turn.system)?.text);
    }
    Ok(turn)
}

fn label(root: &Path, file: &str) -> String {
    let path = Path::new(file);
    let shown = match path.strip_prefix(root) {
        Ok(inside) => inside.with_extension(""),
        Err(_) => path.file_name().map(PathBuf::from).unwrap_or_default(),
    };
    shown.to_string_lossy().replace('\\', "/")
}

fn plain(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn last_line(output: &str) -> Option<&str> {
    output
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
}

pub fn keep(file: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut temp = file.to_path_buf();
    temp.as_mut_os_string().push(".tmp");
    fs::File::create(&temp).and_then(|mut handle| {
        handle.write_all(bytes)?;
        handle.sync_all()
    })?;
    fs::rename(&temp, file)
}

fn chat_folder(chats: &Path, chat: &str) -> Result<PathBuf, String> {
    if !plain(chat) {
        return Err("This chat cannot hold attached files.".into());
    }
    Ok(chats.join(chat))
}

fn mirrors(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?
        .join("mirrors"))
}

pub fn clear_mirrors(app: &AppHandle) {
    if let Ok(mirrors) = mirrors(app) {
        let _ = fs::remove_dir_all(mirrors);
    }
}

// Each chat gets its own mirror, holding only the pages that chat may see.
pub fn mirror_root(app: &AppHandle, root: &Path, chat: &str) -> Result<PathBuf, String> {
    Ok(mirrors(app)?
        .join(format!(
            "{:032x}",
            typst::utils::hash128(&root.to_string_lossy())
        ))
        .join(chat))
}

pub fn chat_path(app: &AppHandle, chat: &str) -> Result<PathBuf, String> {
    chat_folder(&project::chats(app)?, chat)
}

fn attach(folder: &Path, file: &Path) -> Result<Attachment, String> {
    let name = file
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let unreadable = |error| format!("“{name}” could not be attached: {error}");
    if fs::metadata(file).map_err(unreadable)?.len() > MAX_ATTACHMENT_BYTES {
        return Err(format!(
            "“{name}” is too large to attach. The limit is 32 MB."
        ));
    }
    fs::create_dir_all(folder).map_err(unreadable)?;
    let mut copy = folder.join(format!(
        "{}-{name}",
        fs::read_dir(folder).map_err(unreadable)?.count() + 1
    ));
    if extract::OFFICE.contains(&kind(&name).as_str()) {
        copy.as_mut_os_string().push(".txt");
        keep(&copy, extract::text(file)?.as_bytes()).map_err(unreadable)?;
    } else {
        fs::copy(file, &copy).map_err(unreadable)?;
    }
    let id = copy
        .file_name()
        .and_then(|copied| attachment_id(&copied.to_string_lossy()))
        .unwrap_or_default();
    Ok(Attachment {
        name,
        id,
        path: copy.to_string_lossy().into_owned(),
        text: None,
        level: LIGHT,
    })
}

// The number and the name the user gave a file kept in a chat folder as "3-Report.docx.txt".
fn original(file: &str) -> Option<(u32, &str)> {
    let (number, rest) = file.split_once('-')?;
    let number = number.parse().ok()?;
    let name = rest
        .strip_suffix(".txt")
        .filter(|original| extract::OFFICE.contains(&kind(original).as_str()))
        .unwrap_or(rest);
    Some((number, name))
}

fn kind(name: &str) -> String {
    Path::new(name)
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase()
}

// What the assistant calls an attached file: its number and kind, such as "3.pdf". A file name can
// hold a customer or a project, so the real one never reaches a provider.
pub fn attachment_id(file: &str) -> Option<String> {
    let (number, name) = original(file)?;
    let kind = kind(name);
    let kind = if kind.is_empty() || !kind.chars().all(|c| c.is_ascii_alphanumeric()) {
        "file".to_string()
    } else {
        kind
    };
    Some(format!("{number}.{kind}"))
}

// The file kept in the chat folder for an id the assistant gives, such as "3.pdf" or "3".
pub fn attachment_file(folder: &Path, id: &str) -> Option<String> {
    let id = Path::new(id).file_name()?.to_string_lossy().into_owned();
    let digits: String = id.chars().take_while(char::is_ascii_digit).collect();
    let number: u32 = digits.parse().ok()?;
    fs::read_dir(folder)
        .ok()?
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .find(|file| original(file).is_some_and(|(found, _)| found == number))
}

fn attached(folder: &Path) -> Vec<Attachment> {
    let levels = levels(folder);
    let level = |file: &str| {
        levels
            .as_ref()
            .map_or(STRICT, |levels| levels.get(file).copied().unwrap_or(LIGHT))
    };
    let mut found: Vec<(u32, Attachment)> = fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| {
            let file = entry.file_name().to_string_lossy().into_owned();
            let (number, name) = original(&file)?;
            Some((
                number,
                Attachment {
                    name: name.into(),
                    id: attachment_id(&file)?,
                    path: entry.path().to_string_lossy().into_owned(),
                    text: None,
                    level: level(&file),
                },
            ))
        })
        .collect();
    found.sort_by_key(|(number, _)| *number);
    found.into_iter().map(|(_, file)| file).collect()
}

fn levels(folder: &Path) -> Option<HashMap<String, u8>> {
    match fs::read(folder.join(LEVELS)) {
        Ok(bytes) => serde_json::from_slice(&bytes).ok(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(HashMap::new()),
        Err(_) => None,
    }
}

pub fn level_of(folder: &Path, file: &str) -> u8 {
    levels(folder).map_or(STRICT, |levels| levels.get(file).copied().unwrap_or(LIGHT))
}

fn set_level(folder: &Path, path: &str, level: u8) -> Result<(), String> {
    let file = Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| folder.join(name).is_file())
        .ok_or("This file is not attached to the chat.")?;
    let mut levels = levels(folder).unwrap_or_default();
    levels.insert(file, level.min(STRICT));
    let bytes = serde_json::to_vec(&levels).map_err(|error| error.to_string())?;
    keep(&folder.join(LEVELS), &bytes)
        .map_err(|error| format!("The level could not be saved: {error}"))
}

fn saved_chats(chats: &Path) -> Vec<Value> {
    fs::read_dir(chats)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| fs::read(entry.path().join("chat.json")).ok())
        .filter_map(|bytes| serde_json::from_slice(&bytes).ok())
        .collect()
}

fn save_chat(chats: &Path, chat: &Value) -> Result<(), String> {
    let unsaved = |error: &dyn std::fmt::Display| format!("This chat could not be saved: {error}");
    let folder = chat_folder(chats, chat["id"].as_str().unwrap_or_default())
        .map_err(|error| unsaved(&error))?;
    let bytes = serde_json::to_vec(chat).map_err(|error| unsaved(&error))?;
    if bytes.len() > MAX_CHAT_BYTES {
        return Err(unsaved(&"it has grown too long. Start a new chat."));
    }
    fs::create_dir_all(&folder)
        .and_then(|()| keep(&folder.join("chat.json"), &bytes))
        .map_err(|error| unsaved(&error))
}

enum Provider {
    Claude,
    Hosted(&'static openai::Hosted),
}

impl Provider {
    fn find(id: &str) -> Result<Self, String> {
        if !policy::get().allows(id) {
            return Err("Your organisation does not allow this assistant.".into());
        }
        if id == claude::ID {
            return Ok(Self::Claude);
        }
        openai::hosted(id).map(Self::Hosted)
    }

    fn status(&self, app: &AppHandle) -> Result<Status, String> {
        match self {
            Self::Claude => Ok(claude::status(app)),
            Self::Hosted(hosted) => hosted.status(),
        }
    }

    fn disconnect(&self, app: &AppHandle) -> Result<Status, String> {
        match self {
            Self::Claude => claude::disconnect(app),
            Self::Hosted(hosted) => hosted.disconnect(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn run(
        &self,
        app: &AppHandle,
        running: &Running,
        root: &Path,
        folder: &Path,
        turn: &Turn,
        gate: &Gate,
        (send, check): (Send, Check),
    ) -> Result<(), String> {
        match self {
            Self::Claude => claude::run(app, running, root, folder, turn, gate, (send, check)),
            Self::Hosted(hosted) => hosted.run(running, root, folder, turn, gate, (send, check)),
        }
    }
}

#[tauri::command]
pub async fn agent_status(app: AppHandle, provider: String) -> Result<Status, String> {
    crate::blocking(move || agent_status_now(app, provider)).await
}

fn agent_status_now(app: AppHandle, provider: String) -> Result<Status, String> {
    Provider::find(&provider)?.status(&app)
}

#[tauri::command]
pub async fn agent_install(app: AppHandle) -> Result<(), String> {
    crate::blocking(move || agent_install_now(app)).await
}

fn agent_install_now(app: AppHandle) -> Result<(), String> {
    claude::install(&app, &app.state::<Agent>().begin(SETUP)?)
}

#[tauri::command]
pub async fn agent_connect(
    app: AppHandle,
    method: Option<String>,
    link: Channel<String>,
) -> Result<Status, String> {
    crate::blocking(move || agent_connect_now(app, method, link)).await
}

fn agent_connect_now(
    app: AppHandle,
    method: Option<String>,
    link: Channel<String>,
) -> Result<Status, String> {
    let agent = app.state::<Agent>();
    claude::connect(&app, &agent, &agent.begin(SETUP)?, method, &|found| {
        let _ = link.send(found);
    })
}

#[tauri::command]
pub fn agent_code(agent: State<Agent>, code: String) -> Result<(), String> {
    claude::code(&agent, &code)
}

#[tauri::command]
pub async fn provider_connect(
    provider: String,
    url: Option<String>,
    key: String,
) -> Result<Vec<String>, String> {
    crate::blocking(move || provider_connect_now(provider, url, key)).await
}

fn provider_connect_now(
    provider: String,
    url: Option<String>,
    key: String,
) -> Result<Vec<String>, String> {
    openai::hosted(&provider)?.connect(url.as_deref(), &key)
}

#[tauri::command]
pub async fn provider_models(provider: String) -> Result<Vec<String>, String> {
    crate::blocking(move || provider_models_now(provider)).await
}

fn provider_models_now(provider: String) -> Result<Vec<String>, String> {
    openai::hosted(&provider)?.models()
}

#[tauri::command]
pub async fn agent_disconnect(app: AppHandle, provider: String) -> Result<Status, String> {
    crate::blocking(move || agent_disconnect_now(app, provider)).await
}

fn agent_disconnect_now(app: AppHandle, provider: String) -> Result<Status, String> {
    Provider::find(&provider)?.disconnect(&app)
}

#[tauri::command]
pub fn agent_cancel(agent: State<Agent>, key: Option<String>) -> Result<(), String> {
    if key.is_none() {
        *agent.input.lock().map_err(|error| error.to_string())? = None;
    }
    agent.cancel(key.as_deref().unwrap_or(SETUP))
}

#[tauri::command]
pub async fn agent_run(app: AppHandle, turn: Turn, events: Channel<Event>) -> Result<(), String> {
    crate::blocking(move || agent_run_now(app, turn, events)).await
}

fn agent_run_now(app: AppHandle, mut turn: Turn, events: Channel<Event>) -> Result<(), String> {
    let provider = Provider::find(&turn.provider)?;
    turn.web &= policy::get().web();
    let agent = app.state::<Agent>();
    let knowledge = app.state::<Knowledge>();
    let gate = knowledge.gate_if(turn.veil)?;
    let root = app.state::<Workspace>().root()?;
    let folder = chat_path(&app, &turn.chat)?;
    fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    marked(&folder, &gate, turn.session.is_some())?;
    let running = agent.begin(&turn.chat)?;
    add_to_scope(&folder, &turn.pages)?;
    let turn = prepared(&knowledge, &gate, &turn)?;
    let raw = |event| events.send(event).map_err(|error| error.to_string());
    let stream = Mutex::new(Stream::default());
    let send = |event: Event| {
        if let Event::Edited { page, .. } = &event {
            add_to_scope(&folder, std::slice::from_ref(page))?;
        }
        unveiled(&gate, &stream, &raw, event)
    };
    let check = |page: &str| {
        let Ok(text) = workspace::read_text(&root, page) else {
            return Vec::new();
        };
        let problems = compile::problems(&app, page, text.clone());
        let hidden = gate.on().then(|| {
            gate.veil_at(&text, gate.page_level(page))
                .map(|veiled| veiled.text)
                .unwrap_or_default()
        });
        described(&problems, hidden.as_deref())
    };
    let ran = provider.run(
        &app,
        &running,
        &root,
        &folder,
        &turn,
        &gate,
        (&send, &check),
    );
    project::changed(&app);
    ran
}

const MAX_SHOWN_LINE: usize = 300;

// What Typst quotes from the page in a message: `in backticks`, "in quotes", after the last ": "
// and the path a missing file was searched at.
fn unquoted(message: &str) -> String {
    let mut kept = String::new();
    let mut quote = None;
    for c in message.chars() {
        match quote {
            Some(end) if c == end => {
                kept.push('…');
                quote = None;
            }
            Some(_) => {}
            None if c == '`' || c == '"' => quote = Some(c),
            None => kept.push(c),
        }
    }
    if let Some(at) = kept.find(" (searched at ") {
        kept.truncate(at);
    }
    if let Some(at) = kept.rfind(": ") {
        kept.truncate(at);
    }
    kept.trim().to_string()
}

// Each compile error as a line for the assistant. With hiding on (`hidden` is the hidden page) the
// text Typst quotes from the page is left out, and the line the error is on is shown from the
// hidden page instead, so the page's text reaches the assistant only as hiding wrote it.
fn described(problems: &[compile::Problem], hidden: Option<&str>) -> Vec<String> {
    problems
        .iter()
        .map(|problem| {
            let Some(hidden) = hidden else {
                return match problem.line {
                    Some(line) => format!("line {line}: {}", problem.message),
                    None => problem.message.clone(),
                };
            };
            let mut said = problem.message.lines();
            let message = unquoted(said.next().unwrap_or_default());
            let hints: String = said
                .map(|hint| format!("\nHint: {}", unquoted(hint.trim_start_matches("Hint: "))))
                .collect();
            let Some(line) = problem.line else {
                return message + &hints;
            };
            let mut shown: String = hidden
                .lines()
                .nth(line - 1)
                .unwrap_or_default()
                .trim()
                .chars()
                .take(MAX_SHOWN_LINE)
                .collect();
            if shown.chars().count() == MAX_SHOWN_LINE {
                openai::whole_tokens(&mut shown);
                shown.push('…');
            }
            format!("line {line}: {message}, in: {shown}{hints}")
        })
        .collect()
}

#[tauri::command]
pub async fn agent_preview(app: AppHandle, turn: Turn) -> Result<Vec<Event>, String> {
    crate::blocking(move || agent_preview_now(app, turn)).await
}

fn agent_preview_now(app: AppHandle, turn: Turn) -> Result<Vec<Event>, String> {
    let knowledge = app.state::<Knowledge>();
    let gate = knowledge.gate_if(turn.veil)?;
    let turn = prepared(&knowledge, &gate, &turn)?;
    Ok(vec![
        seen("system", &turn.system),
        seen("prompt", &gate.veil(&turn.prompt)?.text),
    ])
}

#[tauri::command]
pub async fn agent_attach(app: AppHandle, chat: String) -> Result<Vec<Attachment>, String> {
    crate::blocking(move || agent_attach_now(app, chat)).await
}

fn agent_attach_now(app: AppHandle, chat: String) -> Result<Vec<Attachment>, String> {
    let folder = chat_path(&app, &chat)?;
    let knowledge = app.state::<Knowledge>();
    let gate = knowledge.gate()?;
    let files = app
        .dialog()
        .file()
        .blocking_pick_files()
        .unwrap_or_default();
    let attached = files
        .into_iter()
        .map(|file| {
            let mut attached = attach(
                &folder,
                &file.into_path().map_err(|error| error.to_string())?,
            )?;
            if gate.on() {
                attached.text = Some(extract::text(Path::new(&attached.path))?);
            }
            Ok(attached)
        })
        .collect();
    project::changed(&app);
    attached
}

#[tauri::command]
pub async fn agent_files(app: AppHandle, chat: String) -> Result<Vec<Attachment>, String> {
    crate::blocking(move || agent_files_now(app, chat)).await
}

fn agent_files_now(app: AppHandle, chat: String) -> Result<Vec<Attachment>, String> {
    let folder = chat_path(&app, &chat)?;
    let knowledge = app.state::<Knowledge>();
    let gate = knowledge.gate()?;
    attached(&folder)
        .into_iter()
        .map(|mut file| {
            if gate.on() {
                file.text = Some(extract::text(Path::new(&file.path))?);
            }
            Ok(file)
        })
        .collect()
}

#[tauri::command]
pub fn agent_in_scope(
    app: AppHandle,
    chat: String,
    path: String,
    open: Option<String>,
) -> Result<bool, String> {
    Ok(in_scope(
        &chat_path(&app, &chat)?,
        open.as_deref(),
        path.trim_matches('/'),
    ))
}

#[tauri::command]
pub fn agent_level(app: AppHandle, chat: String, path: String, level: u8) -> Result<(), String> {
    set_level(&chat_path(&app, &chat)?, &path, level)?;
    project::changed(&app);
    Ok(())
}

#[tauri::command]
pub fn agent_forget(app: AppHandle, chat: String) -> Result<(), String> {
    let folder = chat_path(&app, &chat)?;
    if folder.exists() {
        fs::remove_dir_all(&folder)
            .map_err(|error| format!("The chat could not be cleared: {error}"))?;
    }
    for workspace in fs::read_dir(mirrors(&app)?).into_iter().flatten().flatten() {
        let _ = fs::remove_dir_all(workspace.path().join(&chat));
    }
    project::changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn chats_list(app: AppHandle) -> Result<Vec<Value>, String> {
    crate::blocking(move || chats_list_now(app)).await
}

fn chats_list_now(app: AppHandle) -> Result<Vec<Value>, String> {
    Ok(project::chats(&app)
        .map(|chats| saved_chats(&chats))
        .unwrap_or_default())
}

#[tauri::command]
pub async fn chat_save(app: AppHandle, chat: Value) -> Result<(), String> {
    crate::blocking(move || chat_save_now(app, chat)).await
}

fn chat_save_now(app: AppHandle, chat: Value) -> Result<(), String> {
    save_chat(&project::chats(&app)?, &chat)?;
    project::changed(&app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(test: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("znote-agent-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        base
    }

    #[test]
    fn with_hiding_on_an_error_keeps_what_went_wrong_and_shows_its_line_hidden() {
        let problem = |line, message: &str| compile::Problem {
            line,
            message: message.into(),
        };
        let problems = [
            problem(
                Some(2),
                "unknown variable: jansen\nHint: did you mean `jansens`?",
            ),
            problem(Some(3), "module `acme` does not contain `offer`"),
            problem(Some(1), "file not found (searched at /acme-logo.png)"),
            problem(None, "unexpected argument: acme"),
        ];
        let hidden = "#image(\"⟦PATH_1⟧\")\n  #⟦PERSON_1⟧ called\n#⟦CUSTOMER_1⟧.offer\n";
        assert_eq!(
            described(&problems, Some(hidden)),
            [
                "line 2: unknown variable, in: #⟦PERSON_1⟧ called\nHint: did you mean …?",
                "line 3: module … does not contain …, in: #⟦CUSTOMER_1⟧.offer",
                "line 1: file not found, in: #image(\"⟦PATH_1⟧\")",
                "unexpected argument",
            ]
        );
        assert_eq!(
            described(&problems[..1], None),
            ["line 2: unknown variable: jansen\nHint: did you mean `jansens`?"]
        );
        let long = format!("{}⟦PERSON_1⟧", "x".repeat(MAX_SHOWN_LINE - 3));
        assert_eq!(
            described(&[problem(Some(1), "oops")], Some(&long)),
            [format!(
                "line 1: oops, in: {}…",
                "x".repeat(MAX_SHOWN_LINE - 3)
            )]
        );
    }

    #[test]
    fn the_system_prompt_is_hidden_like_the_rest() {
        let ttl = "@prefix skos: <http://www.w3.org/2004/02/skos/core#> .
<urn:c> skos:prefLabel \"Customer\" ; skos:topConceptOf <urn:s> .
<urn:a> skos:prefLabel \"Acme Corporation\" ; skos:broader <urn:c> .";
        let glossary =
            crate::privacy::glossary::parse(ttl.as_bytes(), oxrdfio::RdfFormat::Turtle).unwrap();
        let options = crate::knowledge::Options {
            veil: true,
            ..Default::default()
        };
        let knowledge = Knowledge::in_memory(&glossary, options);
        let turn: Turn = serde_json::from_value(serde_json::json!({
            "chat": "chat", "prompt": "Write a letter", "system": "Template: Acme Corporation letter",
            "provider": "custom", "readOnly": false, "fast": false, "web": false, "veil": true
        }))
        .unwrap();
        let prepared = prepared(&knowledge, &knowledge.gate().unwrap(), &turn).unwrap();
        assert!(prepared.system.starts_with("Template: ⟦CUSTOMER_1⟧ letter"));
        assert!(prepared.system.ends_with(TOKENS));
    }

    #[test]
    fn every_offered_provider_is_found_and_an_unknown_one_is_refused() {
        assert!(matches!(Provider::find("claude"), Ok(Provider::Claude)));
        for id in ["openrouter", "custom"] {
            assert!(
                matches!(Provider::find(id), Ok(Provider::Hosted(hosted)) if hosted.id == id),
                "{id}"
            );
        }
        for bad in ["", "Claude", "openai", "../claude"] {
            assert!(
                Provider::find(bad).is_err_and(|error| error.contains("does not allow")),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn a_file_outside_the_workspace_is_shown_by_name_only() {
        assert_eq!(
            label(Path::new("/w"), "/data/assistant/1-report.pdf"),
            "1-report.pdf"
        );
        assert_eq!(
            label(Path::new("/w"), "/w/Work/Notes/v1.2.typ"),
            "Work/Notes/v1.2"
        );
    }

    #[test]
    fn each_chat_has_its_own_folder_and_an_odd_chat_name_gets_none() {
        let data = Path::new("/data");
        assert_eq!(
            chat_folder(data, "8ec00c1e-4fd4").unwrap(),
            Path::new("/data/8ec00c1e-4fd4")
        );
        for bad in ["", "..", "../claude", "a/b", "a\\b", "C:"] {
            assert!(chat_folder(data, bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn an_attached_file_is_copied_and_never_replaces_an_earlier_one() {
        let base = scratch("attach");
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

    #[test]
    fn a_level_is_kept_per_attached_file_and_only_for_attached_files() {
        let base = scratch("level");
        let folder = base.join("chat");
        let file = base.join("notes.txt");
        fs::write(&file, "notes").unwrap();
        let first = attach(&folder, &file).unwrap();
        let second = attach(&folder, &file).unwrap();
        assert_eq!(first.level, LIGHT);
        set_level(&folder, &first.path, 9).unwrap();
        assert_eq!(level_of(&folder, "1-notes.txt"), STRICT);
        assert_eq!(
            level_of(
                &folder,
                Path::new(&second.path)
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
            ),
            LIGHT
        );
        assert!(set_level(&folder, &base.join("elsewhere.txt").to_string_lossy(), 0).is_err());
        fs::write(folder.join(LEVELS), "{ broken").unwrap();
        assert_eq!(level_of(&folder, "2-notes.txt"), STRICT);
        assert!(attached(&folder).iter().all(|file| file.level == STRICT));
    }

    #[test]
    fn the_files_of_a_chat_are_listed_in_order_with_their_names_and_levels() {
        let base = scratch("files");
        let folder = base.join("chat");
        fs::create_dir_all(&folder).unwrap();
        for file in [
            "2-Report.docx.txt",
            "10-plan.txt",
            "1-notes.txt",
            "chat.json",
            MARK,
        ] {
            fs::write(folder.join(file), "x").unwrap();
        }
        set_level(&folder, "10-plan.txt", STRICT).unwrap();
        let listed: Vec<(String, u8)> = attached(&folder)
            .into_iter()
            .map(|file| (file.name, file.level))
            .collect();
        assert_eq!(
            listed,
            [
                ("notes.txt".into(), LIGHT),
                ("Report.docx".into(), LIGHT),
                ("plan.txt".into(), STRICT)
            ]
        );
    }

    #[test]
    fn the_assistant_knows_an_attached_file_by_number_and_kind_only() {
        assert_eq!(
            attachment_id("1-Projectplan_Noorderlicht.pdf").as_deref(),
            Some("1.pdf")
        );
        assert_eq!(
            attachment_id("2-Acme Report.DOCX.txt").as_deref(),
            Some("2.docx")
        );
        assert_eq!(attachment_id("3-Makefile").as_deref(), Some("3.file"));
        assert_eq!(attachment_id("chat.json"), None);
        let folder = scratch("ids");
        for file in [
            "1-Projectplan_Noorderlicht.pdf",
            "12-notes.txt",
            "chat.json",
        ] {
            fs::write(folder.join(file), "x").unwrap();
        }
        assert_eq!(
            attachment_file(&folder, "1.pdf").as_deref(),
            Some("1-Projectplan_Noorderlicht.pdf")
        );
        assert_eq!(
            attachment_file(&folder, "12.txt").as_deref(),
            Some("12-notes.txt")
        );
        assert_eq!(
            attachment_file(&folder, "/old/path/12-notes.txt").as_deref(),
            Some("12-notes.txt")
        );
        assert_eq!(attachment_file(&folder, "2.pdf"), None);
        assert_eq!(attachment_file(&folder, "chat.json"), None);
    }

    #[test]
    fn an_office_file_is_attached_as_its_text_and_a_broken_one_is_refused() {
        let base = scratch("office");
        let file = base.join("Report.DOCX");
        let mut archive = zip::ZipWriter::new(fs::File::create(&file).unwrap());
        archive
            .start_file(
                "word/document.xml",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        archive.write_all(br#"<w:document xmlns:w="w"><w:p><w:r><w:t>Quarterly numbers</w:t></w:r></w:p></w:document>"#).unwrap();
        archive.finish().unwrap();
        let folder = base.join("assistant");
        let attached = attach(&folder, &file).unwrap();
        assert_eq!(attached.name, "Report.DOCX");
        assert!(
            attached.path.ends_with("1-Report.DOCX.txt"),
            "{}",
            attached.path
        );
        assert_eq!(
            fs::read_to_string(attached.path).unwrap(),
            "Quarterly numbers"
        );

        fs::write(base.join("broken.xlsx"), "not a zip").unwrap();
        assert!(
            attach(&folder, &base.join("broken.xlsx"))
                .is_err_and(|error| error.starts_with("This xlsx file cannot be read"))
        );
        assert_eq!(fs::read_dir(&folder).unwrap().count(), 1);
    }

    #[test]
    fn each_chat_is_saved_in_its_own_file_and_a_damaged_one_is_skipped() {
        let base = scratch("chats");
        save_chat(
            &base,
            &serde_json::json!({ "id": "chat-1", "title": "first" }),
        )
        .unwrap();
        save_chat(
            &base,
            &serde_json::json!({ "id": "chat-2", "title": "second" }),
        )
        .unwrap();
        save_chat(
            &base,
            &serde_json::json!({ "id": "chat-1", "title": "renamed" }),
        )
        .unwrap();
        assert!(save_chat(&base, &serde_json::json!({ "id": "../outside" })).is_err());
        fs::create_dir_all(base.join("chat-3")).unwrap();
        fs::write(base.join("chat-3/chat.json"), "{ not json").unwrap();
        let mut titles: Vec<String> = saved_chats(&base)
            .iter()
            .map(|chat| chat["title"].to_string())
            .collect();
        titles.sort();
        assert_eq!(titles, ["\"renamed\"", "\"second\""]);
        assert!(!base.join("chat-1/chat.json.tmp").exists());
    }

    #[test]
    fn a_job_frees_its_place_when_it_ends_however_it_ends() {
        let agent = Agent::default();
        let first = agent.begin("chat-1").unwrap();
        assert!(agent.begin("chat-1").is_err());
        let other = agent.begin("chat-2").unwrap();
        agent.cancel("chat-1").unwrap();
        assert!(first.stopped());
        assert!(!other.stopped());
        assert_eq!(first.wait(), Ok(false));
        drop(first);
        assert!(!agent.begin("chat-1").unwrap().stopped());
    }

    #[test]
    fn a_process_left_running_is_ended_with_its_job() {
        let agent = Agent::default();
        let running = agent.begin("chat-1").unwrap();
        let sleeper = if cfg!(windows) {
            ("timeout", "30")
        } else {
            ("sleep", "30")
        };
        let child = std::process::Command::new(sleeper.0)
            .arg(sleeper.1)
            .spawn()
            .unwrap();
        let id = child.id();
        running.hold(child).unwrap();
        drop(running);
        if cfg!(target_os = "linux") {
            assert!(!Path::new(&format!("/proc/{id}")).exists());
        }
        assert!(agent.jobs().unwrap().is_empty());
    }
}

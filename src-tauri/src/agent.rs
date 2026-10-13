mod claude;
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

use crate::extract;
use crate::workspace::Workspace;

const MAX_ATTACHMENT_BYTES: u64 = 32 * 1024 * 1024;
const MAX_CHAT_BYTES: usize = 16 * 1024 * 1024;
const POLL: Duration = Duration::from_millis(150);
const SETUP: &str = "setup";

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
        jobs.insert(key.to_string(), Job { child: None, stopped: stopped.clone() });
        Ok(Running { agent: self, key: key.to_string(), stopped })
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
            _ => Job { child: Some(child), stopped: self.stopped.clone() }.stop(),
        }
        Ok(())
    }

    fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }

    fn wait(&self) -> Result<bool, String> {
        loop {
            let mut jobs = self.agent.jobs()?;
            let Some(child) = jobs.get_mut(&self.key).and_then(|job| job.child.as_mut()) else { return Ok(false) };
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
    Text { text: String },
    Thought { text: String },
    Step { tool: String, page: String },
    Edited { page: String, before: Option<String> },
    Read { page: String },
    Done { session: String, failure: Option<String> },
}

#[derive(Deserialize)]
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
}

type Send<'a> = &'a dyn Fn(Event) -> Result<(), String>;

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

fn plain(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn last_line(output: &str) -> Option<&str> {
    output.lines().rev().map(str::trim).find(|line| !line.is_empty())
}

fn keep(file: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut temp = file.to_path_buf();
    temp.as_mut_os_string().push(".tmp");
    fs::File::create(&temp).and_then(|mut handle| {
        handle.write_all(bytes)?;
        handle.sync_all()
    })?;
    fs::rename(&temp, file)
}

fn chat_folder(data: &Path, chat: &str) -> Result<PathBuf, String> {
    if !plain(chat) {
        return Err("This chat cannot hold attached files.".into());
    }
    Ok(data.join("assistant").join(chat))
}

fn data(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|error| error.to_string())
}

fn attach(folder: &Path, file: &Path) -> Result<Attachment, String> {
    let name = file.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let unreadable = |error| format!("“{name}” could not be attached: {error}");
    if fs::metadata(file).map_err(unreadable)?.len() > MAX_ATTACHMENT_BYTES {
        return Err(format!("“{name}” is too large to attach. The limit is 32 MB."));
    }
    fs::create_dir_all(folder).map_err(unreadable)?;
    let mut copy = folder.join(format!("{}-{name}", fs::read_dir(folder).map_err(unreadable)?.count() + 1));
    let kind = file.extension().unwrap_or_default().to_string_lossy().to_lowercase();
    if extract::OFFICE.contains(&kind.as_str()) {
        copy.as_mut_os_string().push(".txt");
        keep(&copy, extract::text(file)?.as_bytes()).map_err(unreadable)?;
    } else {
        fs::copy(file, &copy).map_err(unreadable)?;
    }
    Ok(Attachment { name, path: copy.to_string_lossy().into_owned() })
}

fn saved_chats(data: &Path) -> Vec<Value> {
    fs::read_dir(data.join("assistant"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| fs::read(entry.path().join("chat.json")).ok())
        .filter_map(|bytes| serde_json::from_slice(&bytes).ok())
        .collect()
}

fn save_chat(data: &Path, chat: &Value) -> Result<(), String> {
    let unsaved = |error: &dyn std::fmt::Display| format!("This chat could not be saved: {error}");
    let folder = chat_folder(data, chat["id"].as_str().unwrap_or_default()).map_err(|error| unsaved(&error))?;
    let bytes = serde_json::to_vec(chat).map_err(|error| unsaved(&error))?;
    if bytes.len() > MAX_CHAT_BYTES {
        return Err(unsaved(&"it has grown too long. Start a new chat."));
    }
    fs::create_dir_all(&folder).and_then(|()| keep(&folder.join("chat.json"), &bytes)).map_err(|error| unsaved(&error))
}

#[tauri::command(async)]
pub fn agent_status(app: AppHandle, provider: String) -> Result<Status, String> {
    if provider == claude::ID {
        return Ok(claude::status(&app));
    }
    let secret = secrets::read(&provider)?;
    Ok(Status { installed: true, connected: secret.is_some(), url: secret.map(|secret| secret.url) })
}

#[tauri::command(async)]
pub fn agent_install(app: AppHandle) -> Result<(), String> {
    claude::install(&app, &app.state::<Agent>().begin(SETUP)?)
}

#[tauri::command(async)]
pub fn agent_connect(app: AppHandle, method: Option<String>, link: Channel<String>) -> Result<Status, String> {
    let agent = app.state::<Agent>();
    claude::connect(&app, &agent, &agent.begin(SETUP)?, method, &|found| {
        let _ = link.send(found);
    })
}

#[tauri::command]
pub fn agent_code(agent: State<Agent>, code: String) -> Result<(), String> {
    claude::code(&agent, &code)
}

#[tauri::command(async)]
pub fn provider_connect(provider: String, url: Option<String>, key: String) -> Result<Vec<String>, String> {
    let secret = openai::secret(&provider, url.as_deref(), &key)?;
    let models = openai::check(&provider, &secret)?;
    secrets::write(&provider, &secret)?;
    Ok(models)
}

#[tauri::command(async)]
pub fn provider_models(provider: String) -> Result<Vec<String>, String> {
    match secrets::read(&provider)? {
        Some(secret) => openai::models(&secret),
        None => Ok(Vec::new()),
    }
}

#[tauri::command(async)]
pub fn agent_disconnect(app: AppHandle, provider: String) -> Result<Status, String> {
    if provider == claude::ID {
        return claude::disconnect(&app);
    }
    secrets::clear(&provider)?;
    Ok(Status { installed: true, connected: false, url: None })
}

#[tauri::command]
pub fn agent_cancel(agent: State<Agent>, key: Option<String>) -> Result<(), String> {
    if key.is_none() {
        *agent.input.lock().map_err(|error| error.to_string())? = None;
    }
    agent.cancel(key.as_deref().unwrap_or(SETUP))
}

#[tauri::command(async)]
pub fn agent_run(app: AppHandle, turn: Turn, events: Channel<Event>) -> Result<(), String> {
    let agent = app.state::<Agent>();
    let root = app.state::<Workspace>().root()?;
    let folder = chat_folder(&data(&app)?, &turn.chat)?;
    fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    let running = agent.begin(&turn.chat)?;
    let send = |event| events.send(event).map_err(|error| error.to_string());
    if turn.provider == claude::ID {
        return claude::run(&app, &running, &root, &folder, &turn, &send);
    }
    let secret = secrets::read(&turn.provider)?.ok_or("Connect the assistant in Settings first.")?;
    openai::run(&running, &root, &folder, &turn, &secret, &send)
}

#[tauri::command(async)]
pub fn agent_attach(app: AppHandle, chat: String) -> Result<Vec<Attachment>, String> {
    let folder = chat_folder(&data(&app)?, &chat)?;
    let files = app.dialog().file().blocking_pick_files().unwrap_or_default();
    files
        .into_iter()
        .map(|file| attach(&folder, &file.into_path().map_err(|error| error.to_string())?))
        .collect()
}

#[tauri::command]
pub fn agent_forget(app: AppHandle, chat: String) -> Result<(), String> {
    let folder = chat_folder(&data(&app)?, &chat)?;
    if folder.exists() {
        fs::remove_dir_all(&folder).map_err(|error| format!("The chat could not be cleared: {error}"))?;
    }
    Ok(())
}

#[tauri::command(async)]
pub fn chats_list(app: AppHandle) -> Result<Vec<Value>, String> {
    Ok(saved_chats(&data(&app)?))
}

#[tauri::command(async)]
pub fn chat_save(app: AppHandle, chat: Value) -> Result<(), String> {
    save_chat(&data(&app)?, &chat)
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
    fn a_file_outside_the_workspace_is_shown_by_name_only() {
        assert_eq!(label(Path::new("/w"), "/data/assistant/1-report.pdf"), "1-report.pdf");
        assert_eq!(label(Path::new("/w"), "/w/Work/Notes/v1.2.typ"), "Work/Notes/v1.2");
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
    fn an_office_file_is_attached_as_its_text_and_a_broken_one_is_refused() {
        let base = scratch("office");
        let file = base.join("Report.DOCX");
        let mut archive = zip::ZipWriter::new(fs::File::create(&file).unwrap());
        archive.start_file("word/document.xml", zip::write::SimpleFileOptions::default()).unwrap();
        archive.write_all(br#"<w:document xmlns:w="w"><w:p><w:r><w:t>Quarterly numbers</w:t></w:r></w:p></w:document>"#).unwrap();
        archive.finish().unwrap();
        let folder = base.join("assistant");
        let attached = attach(&folder, &file).unwrap();
        assert_eq!(attached.name, "Report.DOCX");
        assert!(attached.path.ends_with("1-Report.DOCX.txt"), "{}", attached.path);
        assert_eq!(fs::read_to_string(attached.path).unwrap(), "Quarterly numbers");

        fs::write(base.join("broken.xlsx"), "not a zip").unwrap();
        assert!(attach(&folder, &base.join("broken.xlsx")).is_err_and(|error| error.starts_with("This xlsx file cannot be read")));
        assert_eq!(fs::read_dir(&folder).unwrap().count(), 1);
    }

    #[test]
    fn each_chat_is_saved_in_its_own_file_and_a_damaged_one_is_skipped() {
        let base = scratch("chats");
        save_chat(&base, &serde_json::json!({ "id": "chat-1", "title": "first" })).unwrap();
        save_chat(&base, &serde_json::json!({ "id": "chat-2", "title": "second" })).unwrap();
        save_chat(&base, &serde_json::json!({ "id": "chat-1", "title": "renamed" })).unwrap();
        assert!(save_chat(&base, &serde_json::json!({ "id": "../outside" })).is_err());
        fs::create_dir_all(base.join("assistant/chat-3")).unwrap();
        fs::write(base.join("assistant/chat-3/chat.json"), "{ not json").unwrap();
        let mut titles: Vec<String> = saved_chats(&base).iter().map(|chat| chat["title"].to_string()).collect();
        titles.sort();
        assert_eq!(titles, ["\"renamed\"", "\"second\""]);
        assert!(!base.join("assistant/chat-1/chat.json.tmp").exists());
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
        let sleeper = if cfg!(windows) { ("timeout", "30") } else { ("sleep", "30") };
        let child = std::process::Command::new(sleeper.0).arg(sleeper.1).spawn().unwrap();
        let id = child.id();
        running.hold(child).unwrap();
        drop(running);
        if cfg!(target_os = "linux") {
            assert!(!Path::new(&format!("/proc/{id}")).exists());
        }
        assert!(agent.jobs().unwrap().is_empty());
    }
}

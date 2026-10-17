use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Mutex, MutexGuard};
use std::thread::spawn;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;
use zsuite_sealed::{
    EXTENSION, Knowledge, LOCKED, Listed, check_name, check_password, checked, connect, create,
    export_file, file_of, fresh_id, import_file, index, sealed, sql,
};
use zsuite_veil::glossary;

use crate::agent;
use crate::models;
use crate::workspace::{self, Workspace};
use zsuite_agent::extract;

#[derive(Default)]
pub struct Pending {
    picked: Mutex<Option<PathBuf>>,
    queue: Mutex<Option<Sender<String>>>,
}

fn locked<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>, String> {
    mutex.lock().map_err(|error| error.to_string())
}

const SERVICE: &str = "znote knowledge";

const DEBOUNCE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
struct Shelf {
    active: Option<String>,
    files: Vec<Listed>,
}

fn folder(app: &AppHandle) -> Result<PathBuf, String> {
    let folder = app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?
        .join("knowledge");
    fs::create_dir_all(&folder)
        .map_err(|error| format!("The knowledge folder could not be made: {error}"))?;
    Ok(folder)
}

fn shelf(folder: &Path) -> Shelf {
    let mut shelf: Shelf = fs::read(folder.join("index.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    shelf
        .files
        .retain(|file| file_of(folder, &file.id).is_ok_and(|path| path.is_file()));
    if shelf
        .active
        .as_ref()
        .is_some_and(|active| !shelf.files.iter().any(|file| file.id == *active))
    {
        shelf.active = None;
    }
    shelf
}

fn put_shelf(folder: &Path, shelf: &Shelf) -> Result<(), String> {
    let bytes = serde_json::to_vec(shelf).map_err(|error| error.to_string())?;
    crate::keep(&folder.join("index.json"), &bytes)
        .map_err(|error| format!("The list of knowledge files could not be saved: {error}"))
}

fn remembered(id: &str) -> Option<String> {
    sealed::remembered(SERVICE, id)
}

fn remember(id: &str, password: Option<&str>) -> Result<(), String> {
    sealed::remember(SERVICE, id, password)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    files: Vec<Listed>,
    active: Option<String>,
    open: bool,
    remembered: bool,
    concepts: usize,
    glossary: bool,
    hidden: usize,
    names: bool,
    search: bool,
    projects: Vec<index::Counted>,
}

fn status(app: &AppHandle) -> Result<Status, String> {
    let knowledge = app.state::<Knowledge>();
    let shelf = shelf(&folder(app)?);
    let (concepts, glossary, hidden, projects) = knowledge
        .with(|open| {
            let masked = open
                .glossary
                .concepts
                .iter()
                .filter(|concept| concept.masked)
                .count();
            Ok((
                masked,
                open.imported,
                open.hidden(),
                index::counts(&open.db)?,
            ))
        })?
        .unwrap_or_default();
    let models = models::folder(app)?;
    Ok(Status {
        remembered: shelf.active.as_deref().and_then(remembered).is_some(),
        files: shelf.files,
        active: shelf.active,
        open: knowledge.is_open(),
        concepts,
        glossary,
        hidden,
        names: models::NAME_FINDER.present(&models),
        search: models::SMART_SEARCH.present(&models),
        projects,
    })
}

fn activate(app: &AppHandle, id: &str, db: Connection) -> Result<(), String> {
    let folder = folder(app)?;
    let mut shelf = shelf(&folder);
    shelf.active = Some(id.into());
    put_shelf(&folder, &shelf)?;
    app.state::<Knowledge>().use_file(id.into(), db)?;
    load_finder(app);
    Ok(())
}

fn load_finder(app: &AppHandle) {
    if let Ok(models) = models::folder(app)
        && models::NAME_FINDER.verified(&models)
    {
        app.state::<Knowledge>()
            .load_finder(&models::NAME_FINDER.path(&models));
    }
}

pub fn models_ready(app: &AppHandle, model: &str) {
    match model {
        "names" => load_finder(app),
        _ => load_embedder(app),
    }
}

fn load_embedder(app: &AppHandle) {
    if let Ok(models) = models::folder(app)
        && models::SMART_SEARCH.verified(&models)
    {
        app.state::<Knowledge>()
            .load_embedder(&models::SMART_SEARCH.path(&models));
    }
}

fn project_of(app: &AppHandle) -> String {
    crate::project::file(app).unwrap_or_default()
}

fn refresh_page(app: &AppHandle, page: &str, by_hand: bool) -> Result<(), String> {
    let knowledge = app.state::<Knowledge>();
    let options = knowledge.options();
    let root = app.state::<Workspace>().root()?;
    let project = project_of(app);
    let text = match workspace::read_text(&root, page) {
        Ok(text) => text,
        Err(error) => {
            if !workspace::page_exists(&root, page) {
                knowledge.with(|open| index::remove(open, index::PAGE, &project, page))?;
                return Ok(());
            }
            return Err(error);
        }
    };
    if options.veil {
        knowledge.veil(&text, knowledge.page_level(page))?;
    }
    if !options.knowledge {
        return Ok(());
    }
    knowledge.with_embedder(|open, embedder| {
        if by_hand || options.auto_add || index::known(&open.db, index::PAGE, &project, page)? {
            index::put(open, embedder, index::PAGE, &project, page, &text)?;
        }
        Ok(())
    })?;
    Ok(())
}

fn worker(app: AppHandle, pages: Receiver<String>) {
    while let Ok(first) = pages.recv() {
        let mut waiting = BTreeSet::from([first]);
        while let Ok(more) = pages.recv_timeout(DEBOUNCE) {
            waiting.insert(more);
        }
        for page in waiting {
            let _ = refresh_page(&app, &page, false);
        }
    }
}

pub fn saved(app: &AppHandle, page: String) {
    let knowledge = app.state::<Knowledge>();
    let options = knowledge.options();
    if !(options.veil || options.knowledge) || !knowledge.is_open() {
        return;
    }
    let pending = app.state::<Pending>();
    let Ok(mut queue) = pending.queue.lock() else {
        return;
    };
    let sender = queue.get_or_insert_with(|| {
        let (sender, pages) = channel();
        let app = app.clone();
        spawn(move || worker(app, pages));
        sender
    });
    let _ = sender.send(page);
}

fn every_page(app: &AppHandle) {
    if let Ok(root) = app.state::<Workspace>().root() {
        for page in workspace::pages(&root) {
            saved(app, page);
        }
    }
}

#[tauri::command]
pub async fn knowledge_sync(app: AppHandle) -> Result<(), String> {
    crate::blocking(move || knowledge_sync_now(app)).await
}

fn knowledge_sync_now(app: AppHandle) -> Result<(), String> {
    let options = app.state::<Knowledge>().options();
    if options.knowledge && options.auto_add {
        every_page(&app);
    }
    Ok(())
}

#[tauri::command]
pub async fn knowledge_add(app: AppHandle, path: String) -> Result<Status, String> {
    crate::blocking(move || knowledge_add_now(app, path)).await
}

fn knowledge_add_now(app: AppHandle, path: String) -> Result<Status, String> {
    let root = app.state::<Workspace>().root()?;
    let inside = format!("{path}/");
    for page in workspace::pages(&root)
        .into_iter()
        .filter(|page| *page == path || page.starts_with(&inside))
    {
        refresh_page(&app, &page, true)?;
    }
    status(&app)
}

#[tauri::command]
pub async fn knowledge_attach(app: AppHandle, chat: String, path: String) -> Result<(), String> {
    crate::blocking(move || knowledge_attach_now(app, chat, path)).await
}

fn knowledge_attach_now(app: AppHandle, chat: String, path: String) -> Result<(), String> {
    let folder = agent::chat_path(&app, &chat)?;
    let file = folder.join(
        Path::new(&path)
            .file_name()
            .ok_or("This file is not attached to the chat.")?,
    );
    let text = extract::text(&file)?;
    let name = file
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let knowledge = app.state::<Knowledge>();
    // Names found at the file's level go into the vault now, so they stay hidden when its text
    // comes back from a search, whatever the level of the page or chat asking.
    if knowledge.options().veil {
        knowledge.veil(&text, zsuite_agent::level_of(&folder, &name))?;
    }
    knowledge
        .with_embedder(|open, embedder| {
            index::put(
                open,
                embedder,
                index::ATTACHMENT,
                index::ATTACHMENTS,
                &name,
                &text,
            )
        })?
        .ok_or_else(|| LOCKED.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn knowledge_forget(app: AppHandle, project: String) -> Result<Status, String> {
    crate::blocking(move || knowledge_forget_now(app, project)).await
}

fn knowledge_forget_now(app: AppHandle, project: String) -> Result<Status, String> {
    app.state::<Knowledge>()
        .with(|open| index::forget(open, &project))?;
    status(&app)
}

#[tauri::command]
pub async fn knowledge_rebuild(app: AppHandle) -> Result<Status, String> {
    crate::blocking(move || knowledge_rebuild_now(app)).await
}

fn knowledge_rebuild_now(app: AppHandle) -> Result<Status, String> {
    load_embedder(&app);
    let knowledge = app.state::<Knowledge>();
    knowledge.with_embedder(index::refresh)?;
    every_page(&app);
    status(&app)
}

#[tauri::command]
pub async fn knowledge_status(app: AppHandle) -> Result<Status, String> {
    crate::blocking(move || knowledge_status_now(app)).await
}

fn knowledge_status_now(app: AppHandle) -> Result<Status, String> {
    let knowledge = app.state::<Knowledge>();
    let folder = folder(&app)?;
    let shelf = shelf(&folder);
    if !knowledge.is_open()
        && let Some(id) = shelf.active.as_deref()
        && let Some(password) = remembered(id)
        && let Ok(db) = connect(&file_of(&folder, id)?, &password, OpenFlags::default())
    {
        knowledge.use_file(id.into(), db)?;
        load_finder(&app);
    }
    status(&app)
}

#[tauri::command]
pub async fn knowledge_create(
    app: AppHandle,
    name: String,
    password: String,
    remember_it: bool,
) -> Result<Status, String> {
    crate::blocking(move || knowledge_create_now(app, name, password, remember_it)).await
}

fn knowledge_create_now(
    app: AppHandle,
    name: String,
    password: String,
    remember_it: bool,
) -> Result<Status, String> {
    let name = check_name(&name)?;
    check_password(&password)?;
    let folder = folder(&app)?;
    let temp = folder.join(format!("new-{}.tmp", fresh_id(&name)));
    let id = create(&temp, &name, &password)?;
    let file = file_of(&folder, &id)?;
    fs::rename(&temp, &file)
        .map_err(|error| format!("The knowledge file could not be made: {error}"))?;
    let mut shelf = shelf(&folder);
    shelf.files.push(Listed {
        id: id.clone(),
        name,
    });
    put_shelf(&folder, &shelf)?;
    remember(&id, remember_it.then_some(password.as_str()))?;
    activate(&app, &id, connect(&file, &password, OpenFlags::default())?)?;
    status(&app)
}

#[tauri::command]
pub async fn knowledge_pick(app: AppHandle) -> Result<Option<String>, String> {
    crate::blocking(move || knowledge_pick_now(app)).await
}

fn knowledge_pick_now(app: AppHandle) -> Result<Option<String>, String> {
    let Some(file) = app
        .dialog()
        .file()
        .add_filter("znote knowledge", &[EXTENSION])
        .blocking_pick_file()
    else {
        return Ok(None);
    };
    let file = file.into_path().map_err(|error| error.to_string())?;
    let name = file
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    *locked(&app.state::<Pending>().picked)? = Some(file);
    Ok(Some(name))
}

#[tauri::command]
pub async fn knowledge_import(
    app: AppHandle,
    password: String,
    remember_it: bool,
) -> Result<Status, String> {
    crate::blocking(move || knowledge_import_now(app, password, remember_it)).await
}

fn knowledge_import_now(
    app: AppHandle,
    password: String,
    remember_it: bool,
) -> Result<Status, String> {
    let source = locked(&app.state::<Pending>().picked)?
        .clone()
        .ok_or("Choose a knowledge file first.")?;
    let folder = folder(&app)?;
    let listed = import_file(&folder, &source, &password)?;
    let mut shelf = shelf(&folder);
    shelf.files.push(listed.clone());
    put_shelf(&folder, &shelf)?;
    remember(&listed.id, remember_it.then_some(password.as_str()))?;
    let file = file_of(&folder, &listed.id)?;
    activate(
        &app,
        &listed.id,
        connect(&file, &password, OpenFlags::default())?,
    )?;
    *locked(&app.state::<Pending>().picked)? = None;
    status(&app)
}

#[tauri::command]
pub async fn knowledge_unlock(
    app: AppHandle,
    id: String,
    password: String,
    remember_it: bool,
) -> Result<Status, String> {
    crate::blocking(move || knowledge_unlock_now(app, id, password, remember_it)).await
}

fn knowledge_unlock_now(
    app: AppHandle,
    id: String,
    password: String,
    remember_it: bool,
) -> Result<Status, String> {
    let file = file_of(&folder(&app)?, &id)?;
    let db = connect(&file, &password, OpenFlags::default())?;
    checked(&db)?;
    remember(&id, remember_it.then_some(password.as_str()))?;
    activate(&app, &id, db)?;
    status(&app)
}

#[tauri::command]
pub async fn knowledge_activate(app: AppHandle, id: Option<String>) -> Result<Status, String> {
    crate::blocking(move || knowledge_activate_now(app, id)).await
}

fn knowledge_activate_now(app: AppHandle, id: Option<String>) -> Result<Status, String> {
    let knowledge = app.state::<Knowledge>();
    let folder = folder(&app)?;
    knowledge.close()?;
    let mut shelf = shelf(&folder);
    shelf.active = None;
    put_shelf(&folder, &shelf)?;
    if let Some(id) = id {
        let file = file_of(&folder, &id)?;
        shelf.active = Some(id.clone());
        put_shelf(&folder, &shelf)?;
        if let Some(password) = remembered(&id)
            && let Ok(db) = connect(&file, &password, OpenFlags::default())
        {
            activate(&app, &id, db)?;
        }
    }
    status(&app)
}

#[tauri::command]
pub async fn knowledge_export(app: AppHandle, password: String) -> Result<bool, String> {
    crate::blocking(move || knowledge_export_now(app, password)).await
}

fn knowledge_export_now(app: AppHandle, password: String) -> Result<bool, String> {
    check_password(&password)?;
    let Some(target) = app
        .dialog()
        .file()
        .add_filter("znote knowledge", &[EXTENSION])
        .set_file_name(format!("knowledge.{EXTENSION}"))
        .blocking_save_file()
    else {
        return Ok(false);
    };
    let mut target = target.into_path().map_err(|error| error.to_string())?;
    if target
        .extension()
        .is_none_or(|extension| extension != EXTENSION)
    {
        target.as_mut_os_string().push(format!(".{EXTENSION}"));
    }
    app.state::<Knowledge>()
        .with(|open| export_file(&open.db, &target, &password))?
        .ok_or_else(|| LOCKED.to_string())?;
    Ok(true)
}

#[tauri::command]
pub async fn knowledge_password(
    app: AppHandle,
    current: String,
    password: String,
    remember_it: bool,
) -> Result<Status, String> {
    crate::blocking(move || knowledge_password_now(app, current, password, remember_it)).await
}

fn knowledge_password_now(
    app: AppHandle,
    current: String,
    password: String,
    remember_it: bool,
) -> Result<Status, String> {
    check_password(&password)?;
    let folder = folder(&app)?;
    let knowledge = app.state::<Knowledge>();
    let id = knowledge
        .with(|open| Ok(open.id.clone()))?
        .ok_or_else(|| LOCKED.to_string())?;
    connect(
        &file_of(&folder, &id)?,
        &current,
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|_| "The current password is not right.".to_string())?;
    knowledge.with(|open| open.db.pragma_update(None, "rekey", &password).map_err(sql))?;
    remember(&id, remember_it.then_some(password.as_str()))?;
    status(&app)
}

#[tauri::command]
pub async fn knowledge_remove(app: AppHandle, id: String) -> Result<Status, String> {
    crate::blocking(move || knowledge_remove_now(app, id)).await
}

fn knowledge_remove_now(app: AppHandle, id: String) -> Result<Status, String> {
    let folder = folder(&app)?;
    let file = file_of(&folder, &id)?;
    let knowledge = app.state::<Knowledge>();
    if knowledge.with(|open| Ok(open.id == id))?.unwrap_or(false) {
        knowledge.close()?;
    }
    fs::remove_file(&file)
        .map_err(|error| format!("The knowledge file could not be removed: {error}"))?;
    remember(&id, None)?;
    let mut shelf = shelf(&folder);
    shelf.files.retain(|listed| listed.id != id);
    put_shelf(&folder, &shelf)?;
    status(&app)
}

#[tauri::command]
pub async fn glossary_import(app: AppHandle) -> Result<Option<Status>, String> {
    crate::blocking(move || glossary_import_now(app)).await
}

fn glossary_import_now(app: AppHandle) -> Result<Option<Status>, String> {
    let Some(file) = app
        .dialog()
        .file()
        .add_filter("SKOS glossary", &["ttl", "rdf", "xml", "owl"])
        .blocking_pick_file()
    else {
        return Ok(None);
    };
    let file = file.into_path().map_err(|error| error.to_string())?;
    let format = glossary::format_of(&file.to_string_lossy())?;
    if fs::metadata(&file)
        .map_err(|error| error.to_string())?
        .len()
        > glossary::MAX_BYTES
    {
        return Err("The glossary is larger than 20 MB.".into());
    }
    let bytes =
        fs::read(&file).map_err(|error| format!("The glossary could not be read: {error}"))?;
    let parsed = glossary::parse(&bytes, format)?;
    app.state::<Knowledge>()
        .with(|open| open.import_glossary(&parsed))?
        .ok_or_else(|| LOCKED.to_string())?;
    Ok(Some(status(&app)?))
}

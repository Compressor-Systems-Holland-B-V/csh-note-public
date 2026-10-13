use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::spawn;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use typst::utils::hash128;
use typst_kit::fonts;

use crate::compile::{Compiler, UsedFont};
use crate::snapshot::{self, Home, Manifest};
use crate::theme;

const IDLE: Duration = Duration::from_secs(30);
const MAX_FONT_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
struct Record {
    file: String,
    size: u64,
    modified: u64,
    dirty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
struct Carried {
    family: String,
    file: Option<String>,
}

type FontUse = BTreeMap<String, Vec<Carried>>;

#[derive(Debug, Default, Clone)]
struct Note {
    look: Option<String>,
    settings: Value,
}

struct Open {
    file: PathBuf,
    home: PathBuf,
    dirty: AtomicBool,
}

#[derive(Default)]
pub struct Project {
    open: Mutex<Option<Arc<Open>>>,
    note: Mutex<Note>,
    wake: Mutex<Option<Sender<()>>>,
    packing: Mutex<()>,
    fonts: Mutex<FontUse>,
    warned: Mutex<BTreeSet<String>>,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Answer {
    Opened { file: String, folder: String, look: Option<String>, settings: Value, missing_fonts: Vec<String> },
    Asks { file: String, themes: Vec<String> },
}

enum Loaded {
    Ready(PathBuf, Manifest),
    Asks(Vec<String>),
}

fn unsaved(error: impl std::fmt::Display) -> String {
    format!("Your znote file could not be saved: {error}")
}

fn home(projects: &Path, file: &Path) -> PathBuf {
    projects.join(format!("{:032x}", hash128(&file.to_string_lossy())))
}

fn recorded(home: &Path) -> Record {
    fs::read(home.join("state.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

fn record(home: &Path, file: &Path, dirty: bool) -> Result<(), String> {
    let (size, modified) = snapshot::stamp(file);
    let record = Record { file: file.to_string_lossy().into_owned(), size, modified, dirty };
    let bytes = serde_json::to_vec(&record).map_err(unsaved)?;
    fs::write(home.join("state.json"), bytes).map_err(unsaved)
}

fn with(file: &Path, ending: &str) -> PathBuf {
    let mut other = file.to_path_buf();
    other.as_mut_os_string().push(ending);
    other
}

fn font_use(home: &Path) -> FontUse {
    fs::read(Home(home).font_use()).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

fn keep_font_use(home: &Path, used: &FontUse) {
    if let Ok(bytes) = serde_json::to_vec_pretty(used) {
        let _ = fs::write(Home(home).font_use(), bytes);
    }
}

fn prune(home: &Path) {
    let layout = Home(home);
    let mut used = font_use(home);
    let before = used.len();
    used.retain(|page, _| layout.pages().join(format!("{page}.typ")).is_file());
    if used.len() != before {
        keep_font_use(home, &used);
    }
    let wanted: BTreeSet<&str> = used.values().flatten().filter_map(|font| font.file.as_deref()).collect();
    for entry in fs::read_dir(layout.fonts()).into_iter().flatten().flatten() {
        if !wanted.contains(entry.file_name().to_string_lossy().as_ref()) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn swap(temp: &Path, target: &Path) -> Result<(), String> {
    let backup = with(target, ".bak");
    if target.is_file() {
        let _ = fs::remove_file(&backup);
        if fs::hard_link(target, &backup).is_err() {
            let _ = fs::rename(target, &backup);
        }
    }
    fs::rename(temp, target).map_err(unsaved)
}

fn store(home: &Path, installed: &Path, file: &Path, target: &Path, note: Note) -> Result<(), String> {
    prune(home);
    let known = recorded(home);
    let current = target == file && file.is_file() && snapshot::stamp(file) == (known.size, known.modified);
    let temp = with(target, ".tmp");
    let packed = snapshot::pack(home, installed, &temp, current.then_some(file), note.look, note.settings)
        .and_then(|packed| swap(&temp, target).map(|()| packed))
        .inspect_err(|_| {
            let _ = fs::remove_file(&temp);
        })?;
    if target == file {
        snapshot::remember(home, &packed);
    }
    Ok(())
}

fn beside(file: &Path) -> PathBuf {
    let name = file.file_stem().unwrap_or_default().to_string_lossy();
    (1..)
        .map(|copy| file.with_file_name(format!("{name} (recovered {copy}).znote")))
        .find(|candidate| !candidate.exists())
        .unwrap_or_else(|| file.to_path_buf())
}

fn kept_note(home: &Path) -> Note {
    let manifest = snapshot::read_manifest(&Home(home)).unwrap_or_default();
    Note { look: manifest.look, settings: manifest.settings }
}

fn load(projects: &Path, themes: &Path, file: &Path, accept: bool) -> Result<Loaded, String> {
    let home = home(projects, file);
    let known = recorded(&home);
    let kept = known.file == file.to_string_lossy() && Home(&home).pages().is_dir();
    let untouched = snapshot::stamp(file) == (known.size, known.modified);
    let mut ready = kept && untouched && !known.dirty;
    if kept && known.dirty {
        let target = if untouched || !file.exists() { file.to_path_buf() } else { beside(file) };
        store(&home, themes, file, &target, kept_note(&home))?;
        if target == file {
            record(&home, file, false)?;
            ready = true;
        }
    }
    let manifest = match snapshot::read_manifest(&Home(&home)).filter(|_| ready) {
        Some(manifest) => manifest,
        None if !file.is_file() => {
            return Err(format!("“{}” could not be found. It may have been moved or deleted.", file.display()));
        }
        None => {
            fs::create_dir_all(projects).map_err(|error| format!("This znote file could not be opened: {error}"))?;
            let manifest = snapshot::unpack(file, &home).inspect_err(|_| {
                let _ = fs::remove_dir_all(&home);
            })?;
            record(&home, file, false)?;
            manifest
        }
    };
    let (offered, absent) = snapshot::missing(&home, &manifest, &theme::installed(themes));
    if !absent.is_empty() {
        return Err(snapshot::absent(&absent));
    }
    if !offered.is_empty() && !accept {
        let name = |folder: &PathBuf| theme::named(folder).unwrap_or_else(|| folder.to_string_lossy().into_owned());
        return Ok(Loaded::Asks(offered.iter().map(name).collect()));
    }
    for folder in &offered {
        theme::adopt(themes, folder)?;
    }
    Ok(Loaded::Ready(home, manifest))
}

fn tidy(projects: &Path, current: &Path) {
    for entry in fs::read_dir(projects).into_iter().flatten().flatten() {
        let home = entry.path();
        if home != current && !recorded(&home).dirty {
            let _ = fs::remove_dir_all(home);
        }
    }
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)?.flatten() {
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        } else if kind.is_file() {
            fs::copy(entry.path(), to.join(entry.file_name()))?;
        }
    }
    Ok(())
}

fn create(projects: &Path, themes: &Path, file: &Path, from: Option<&Path>) -> Result<(), String> {
    let fresh = projects.join(".new");
    let _ = fs::remove_dir_all(&fresh);
    let pages = Home(&fresh).pages();
    let filled = match from {
        Some(folder) => copy_tree(folder, &pages),
        None => fs::create_dir_all(&pages),
    };
    let temp = with(file, ".tmp");
    let made = filled
        .map_err(unsaved)
        .and_then(|()| snapshot::pack(&fresh, themes, &temp, None, None, Value::Null))
        .and_then(|_| swap(&temp, file));
    let _ = fs::remove_dir_all(&fresh);
    let _ = fs::remove_dir_all(home(projects, file));
    made
}

fn duplicate(home: &Path, themes: &Path, copy: &Path, note: Note) -> Result<(), String> {
    let temp = with(copy, ".tmp");
    snapshot::pack(home, themes, &temp, None, note.look, note.settings)?;
    swap(&temp, copy)
}

fn carried(home: &Path, font: &UsedFont) -> Carried {
    let folder = Home(home).fonts();
    let name = font.file.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let size = fs::metadata(&font.file).map(|data| data.len()).unwrap_or(u64::MAX);
    let placed = folder.join(&name);
    let file = if font.file.starts_with(&folder) {
        Some(name)
    } else if font.restricted || size > MAX_FONT_BYTES {
        None
    } else if placed.is_file() && fs::metadata(&placed).is_ok_and(|data| data.len() == size) {
        Some(name)
    } else {
        fs::create_dir_all(&folder).and_then(|()| fs::copy(&font.file, &placed)).ok().map(|_| name)
    };
    Carried { family: font.family.clone(), file }
}

fn fallbacks(home: &Path) -> Vec<String> {
    let folder = Home(home).fonts();
    let left_out: BTreeSet<String> = font_use(home)
        .into_values()
        .flatten()
        .filter(|font| !font.file.as_ref().is_some_and(|file| folder.join(file).is_file()))
        .map(|font| font.family)
        .collect();
    if left_out.is_empty() {
        return Vec::new();
    }
    let here: BTreeSet<String> = fonts::system().map(|(_, info)| info.family.to_lowercase()).collect();
    left_out.into_iter().filter(|family| !here.contains(&family.to_lowercase())).collect()
}

fn projects(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|error| error.to_string())?.join("projects"))
}

fn current(app: &AppHandle) -> Option<Arc<Open>> {
    app.state::<Project>().open.lock().ok()?.clone()
}

pub fn fonts(app: &AppHandle) -> Option<PathBuf> {
    current(app).map(|open| Home(&open.home).fonts())
}

pub fn fonts_used(app: &AppHandle, page: &str, used: &[UsedFont]) {
    let (Some(open), Ok(themes)) = (current(app), theme::folder(app)) else { return };
    let mut carry: Vec<Carried> =
        used.iter().filter(|font| !font.file.starts_with(&themes)).map(|font| carried(&open.home, font)).collect();
    carry.sort();
    carry.dedup();
    let project = app.state::<Project>();
    let Ok(mut known) = project.fonts.lock() else { return };
    if known.get(page) == Some(&carry) || (carry.is_empty() && !known.contains_key(page)) {
        return;
    }
    let left_out: Vec<String> = carry.iter().filter(|font| font.file.is_none()).map(|font| font.family.clone()).collect();
    known.insert(page.to_string(), carry);
    keep_font_use(&open.home, &known);
    drop(known);
    if let Ok(mut warned) = project.warned.lock() {
        let news: Vec<String> = left_out.into_iter().filter(|family| warned.insert(family.clone())).collect();
        if !news.is_empty() {
            let _ = app.emit("project-fonts-left-out", news);
        }
    }
    changed(app);
}

pub fn changed(app: &AppHandle) {
    let Some(open) = current(app) else { return };
    if !open.dirty.swap(true, Ordering::Relaxed) {
        let _ = record(&open.home, &open.file, true);
    }
    if let Ok(wake) = app.state::<Project>().wake.lock()
        && let Some(wake) = wake.as_ref()
    {
        let _ = wake.send(());
    }
}

pub fn save(app: &AppHandle) {
    let project = app.state::<Project>();
    let Ok(_packing) = project.packing.lock() else { return };
    let Some(open) = current(app) else { return };
    if !open.dirty.swap(false, Ordering::Relaxed) {
        return;
    }
    let note = project.note.lock().map(|note| note.clone()).unwrap_or_default();
    let stored = theme::folder(app)
        .and_then(|themes| store(&open.home, &themes, &open.file, &open.file, note))
        .and_then(|()| record(&open.home, &open.file, open.dirty.load(Ordering::Relaxed)));
    match stored {
        Ok(()) => {
            let _ = app.emit("project-saved", ());
        }
        Err(error) => {
            open.dirty.store(true, Ordering::Relaxed);
            let _ = app.emit("project-save-failed", error);
        }
    }
}

fn watch(app: &AppHandle) -> Result<(), String> {
    let project = app.state::<Project>();
    let mut wake = project.wake.lock().map_err(|error| error.to_string())?;
    if wake.is_some() {
        return Ok(());
    }
    let (sender, changes) = channel();
    *wake = Some(sender);
    let app = app.clone();
    spawn(move || {
        while changes.recv().is_ok() {
            while changes.recv_timeout(IDLE).is_ok() {}
            save(&app);
        }
    });
    Ok(())
}

fn switch(app: &AppHandle, file: PathBuf, accept: bool) -> Result<Answer, String> {
    save(app);
    let projects = projects(app)?;
    let themes = theme::folder(app)?;
    let shown = file.to_string_lossy().into_owned();
    let (home, manifest) = match load(&projects, &themes, &file, accept)? {
        Loaded::Ready(home, manifest) => (home, manifest),
        Loaded::Asks(themes) => return Ok(Answer::Asks { file: shown, themes }),
    };
    tidy(&projects, &home);
    watch(app)?;
    let project = app.state::<Project>();
    *project.fonts.lock().map_err(|error| error.to_string())? = font_use(&home);
    *project.note.lock().map_err(|error| error.to_string())? =
        Note { look: manifest.look.clone(), settings: manifest.settings.clone() };
    let missing_fonts = fallbacks(&home);
    let folder = Home(&home).pages().to_string_lossy().into_owned();
    *project.open.lock().map_err(|error| error.to_string())? = Some(Arc::new(Open { file, home, dirty: AtomicBool::new(false) }));
    app.state::<Compiler>().forget_themes()?;
    Ok(Answer::Opened { file: shown, folder, look: manifest.look, settings: manifest.settings, missing_fonts })
}

fn named(mut file: PathBuf) -> PathBuf {
    if file.extension().is_none_or(|given| !given.eq_ignore_ascii_case("znote")) {
        file.as_mut_os_string().push(".znote");
    }
    file
}

pub fn asked(arguments: &[String], folder: &Path) -> Option<String> {
    let file = arguments.iter().skip(1).find(|argument| argument.to_lowercase().ends_with(".znote"))?;
    Some(folder.join(file).to_string_lossy().into_owned())
}

#[tauri::command]
pub fn project_asked() -> Option<String> {
    asked(&std::env::args().collect::<Vec<_>>(), &std::env::current_dir().unwrap_or_default())
}

#[tauri::command(async)]
pub fn project_open(app: AppHandle, file: Option<String>, accept: bool) -> Result<Option<Answer>, String> {
    let file = match file {
        Some(file) => PathBuf::from(file),
        None => {
            let Some(picked) = app.dialog().file().add_filter("znote file", &["znote"]).blocking_pick_file() else {
                return Ok(None);
            };
            picked.into_path().map_err(|error| error.to_string())?
        }
    };
    switch(&app, file, accept).map(Some)
}

#[tauri::command(async)]
pub fn project_new(app: AppHandle, from: Option<String>) -> Result<Option<Answer>, String> {
    let mut dialog = app.dialog().file().set_file_name("My notes.znote").add_filter("znote file", &["znote"]);
    if let Ok(documents) = app.path().document_dir() {
        dialog = dialog.set_directory(documents);
    }
    let Some(target) = dialog.blocking_save_file() else {
        return Ok(None);
    };
    let file = named(target.into_path().map_err(|error| error.to_string())?);
    save(&app);
    create(&projects(&app)?, &theme::folder(&app)?, &file, from.as_deref().map(Path::new))?;
    switch(&app, file, true).map(Some)
}

#[tauri::command]
pub fn project_note(app: AppHandle, project: State<Project>, look: Option<String>, settings: Value) -> Result<(), String> {
    let mut kept = project.note.lock().map_err(|error| error.to_string())?;
    if kept.look != look || kept.settings != settings {
        *kept = Note { look, settings };
        drop(kept);
        changed(&app);
    }
    Ok(())
}

#[tauri::command(async)]
pub fn save_snapshot(app: AppHandle, project: State<Project>) -> Result<Option<String>, String> {
    let open = current(&app).ok_or("Open a znote file first.")?;
    let name = format!("{} copy.znote", open.file.file_stem().unwrap_or_default().to_string_lossy());
    let dialog = app.dialog().file().set_file_name(name).add_filter("znote file", &["znote"]);
    let Some(target) = dialog.blocking_save_file() else {
        return Ok(None);
    };
    let copy = named(target.into_path().map_err(|error| error.to_string())?);
    if copy == open.file {
        save(&app);
        return Ok(Some(copy.to_string_lossy().into_owned()));
    }
    let _packing = project.packing.lock().map_err(|error| error.to_string())?;
    let note = project.note.lock().map_err(|error| error.to_string())?.clone();
    duplicate(&open.home, &theme::folder(&app)?, &copy, note)?;
    Ok(Some(copy.to_string_lossy().into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace;

    struct Desk {
        base: PathBuf,
        projects: PathBuf,
        themes: PathBuf,
        file: PathBuf,
    }

    fn desk(test: &str) -> Desk {
        let base = std::env::temp_dir().join(format!("znote-project-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("themes")).unwrap();
        Desk { projects: base.join("projects"), themes: base.join("themes"), file: base.join("Notes.znote"), base }
    }

    fn ready(projects: &Path, themes: &Path, file: &Path) -> PathBuf {
        match load(projects, themes, file, true).unwrap() {
            Loaded::Ready(home, _) => home,
            Loaded::Asks(themes) => panic!("asked about {themes:?}"),
        }
    }

    fn write(home: &Path, text: &str) {
        let root = Home(home).pages();
        workspace::ensure(&root, "Work/Notes/Monday").unwrap();
        workspace::write_text(&root, "Work/Notes/Monday", text).unwrap();
    }

    fn read(home: &Path) -> String {
        workspace::read_text(&Home(home).pages(), "Work/Notes/Monday").unwrap()
    }

    fn save(home: &Path, themes: &Path, file: &Path) {
        store(home, themes, file, file, Note::default()).unwrap();
        record(home, file, false).unwrap();
    }

    fn install(themes: &Path, id: &str) {
        let folder = themes.join(id).join("1.0.0");
        fs::create_dir_all(folder.join("typst")).unwrap();
        fs::write(folder.join("theme.json"), format!(r#"{{ "format": 1, "id": "{id}", "name": "House style", "version": "1.0.0" }}"#)).unwrap();
        fs::write(folder.join("typst/lib.typ"), "#let brand = [Acme]").unwrap();
    }

    #[test]
    fn a_znote_file_carries_the_notes_and_their_theme_to_a_computer_that_never_saw_them() {
        let desk = desk("portable");
        install(&desk.themes, "acme");
        create(&desk.projects, &desk.themes, &desk.file, None).unwrap();
        let home = ready(&desk.projects, &desk.themes, &desk.file);
        assert!(workspace::tree(&Home(&home).pages()).is_empty());
        write(&home, "#import \"@themes/acme:1.0.0\": brand\n= Monday\n");
        fs::write(Home(&home).pages().join("Work").join("logo.svg"), "<svg/>").unwrap();
        save(&home, &desk.themes, &desk.file);
        assert!(!with(&desk.file, ".tmp").exists());
        assert!(with(&desk.file, ".bak").is_file());

        let (there, their_themes) = (desk.base.join("there"), desk.base.join("their themes"));
        let moved = desk.base.join("Copied.znote");
        fs::copy(&desk.file, &moved).unwrap();
        match load(&there, &their_themes, &moved, false).unwrap() {
            Loaded::Asks(themes) => assert_eq!(themes, ["House style 1.0.0"]),
            Loaded::Ready(..) => panic!("the theme was installed without asking"),
        }
        assert!(!their_themes.join("acme").exists());
        let elsewhere = ready(&there, &their_themes, &moved);
        assert!(read(&elsewhere).ends_with("= Monday\n"));
        assert_eq!(fs::read_to_string(Home(&elsewhere).pages().join("Work").join("logo.svg")).unwrap(), "<svg/>");
        assert_eq!(fs::read_to_string(their_themes.join("acme/1.0.0/typst/lib.typ")).unwrap(), "#let brand = [Acme]");
        assert_eq!(theme::installed(&their_themes), [("acme".to_string(), "1.0.0".to_string())]);

        fs::remove_dir_all(&their_themes).unwrap();
        write(&elsewhere, "#import \"@themes/acme:1.0.0\": brand\n= Changed over there\n");
        save(&elsewhere, &their_themes, &moved);
        let third = ready(&desk.base.join("third"), &desk.base.join("third themes"), &moved);
        assert!(read(&third).ends_with("= Changed over there\n"));
        assert!(desk.base.join("third themes/acme/1.0.0/theme.json").is_file());
    }

    #[test]
    fn a_theme_in_a_file_is_checked_before_it_is_installed() {
        let desk = desk("bad-theme");
        install(&desk.themes, "acme");
        create(&desk.projects, &desk.themes, &desk.file, None).unwrap();
        let home = ready(&desk.projects, &desk.themes, &desk.file);
        write(&home, "#import \"@themes/acme:1.0.0\": brand\n");
        fs::write(desk.themes.join("acme/1.0.0/theme.json"), r#"{ "format": 1, "id": "acme", "name": "House style", "version": "1.0.0", "ui": { "light": { "--background": "url(https://example.com/track)" } } }"#).unwrap();
        save(&home, &desk.themes, &desk.file);
        let theirs = desk.base.join("their themes");
        assert!(load(&desk.base.join("there"), &theirs, &desk.file, true).is_err());
        assert!(!theirs.join("acme").exists());
    }

    #[test]
    fn a_znote_file_named_when_the_app_is_started_is_the_one_to_open() {
        let started = |arguments: &[&str]| asked(&arguments.iter().map(|argument| argument.to_string()).collect::<Vec<_>>(), Path::new("/home/me"));
        assert_eq!(started(&["znote.exe"]), None);
        assert_eq!(started(&["/tmp/odd.znote"]), None);
        assert_eq!(started(&["znote", "--flag", "Trip.ZNOTE"]).as_deref(), Some("/home/me/Trip.ZNOTE"));
        assert_eq!(started(&["znote", "/notes/My trip.znote", "other.znote"]).as_deref(), Some("/notes/My trip.znote"));
        assert_eq!(started(&["znote", "theme.znote-theme"]), None);
    }

    #[test]
    fn notes_folders_from_before_become_a_znote_file() {
        let desk = desk("import");
        let old = desk.base.join("old notes");
        fs::create_dir_all(&old).unwrap();
        workspace::ensure(&old, "Work/Notes/Monday").unwrap();
        workspace::write_text(&old, "Work/Notes/Monday", "= From the folder\n").unwrap();
        create(&desk.projects, &desk.themes, &desk.file, Some(&old)).unwrap();
        let home = ready(&desk.projects, &desk.themes, &desk.file);
        assert_eq!(read(&home), "= From the folder\n");
        assert_eq!(workspace::read_text(&old, "Work/Notes/Monday").unwrap(), "= From the folder\n");
    }

    #[test]
    fn a_copy_still_has_what_was_deleted_afterwards_and_a_new_file_keeps_what_it_replaces() {
        let desk = desk("copy");
        create(&desk.projects, &desk.themes, &desk.file, None).unwrap();
        let home = ready(&desk.projects, &desk.themes, &desk.file);
        write(&home, "= Kept in the copy\n");
        save(&home, &desk.themes, &desk.file);
        let copy = desk.base.join("Elsewhere").join("Notes copy.znote");
        fs::create_dir_all(copy.parent().unwrap()).unwrap();
        duplicate(&home, &desk.themes, &copy, Note::default()).unwrap();

        fs::remove_dir_all(Home(&home).pages().join("Work")).unwrap();
        save(&home, &desk.themes, &desk.file);
        assert!(workspace::tree(&Home(&ready(&desk.base.join("check"), &desk.themes, &desk.file)).pages()).is_empty());
        assert_eq!(read(&ready(&desk.projects, &desk.themes, &copy)), "= Kept in the copy\n");

        create(&desk.projects, &desk.themes, &copy, None).unwrap();
        assert!(workspace::tree(&Home(&ready(&desk.projects, &desk.themes, &copy)).pages()).is_empty());
        let replaced = desk.base.join("Replaced.znote");
        fs::copy(with(&copy, ".bak"), &replaced).unwrap();
        assert_eq!(read(&ready(&desk.projects, &desk.themes, &replaced)), "= Kept in the copy\n");
    }

    #[test]
    fn what_was_not_saved_before_a_crash_is_saved_on_the_next_start() {
        let desk = desk("crash");
        create(&desk.projects, &desk.themes, &desk.file, None).unwrap();
        let home = ready(&desk.projects, &desk.themes, &desk.file);
        write(&home, "= Typed just before the crash\n");
        record(&home, &desk.file, true).unwrap();

        let again = ready(&desk.projects, &desk.themes, &desk.file);
        assert_eq!(again, home);
        assert_eq!(read(&again), "= Typed just before the crash\n");
        assert!(!recorded(&home).dirty);
        let fresh = ready(&desk.base.join("another computer"), &desk.themes, &desk.file);
        assert_eq!(read(&fresh), "= Typed just before the crash\n");
    }

    #[test]
    fn when_the_file_and_the_unsaved_notes_both_changed_neither_is_lost() {
        let desk = desk("conflict");
        create(&desk.projects, &desk.themes, &desk.file, None).unwrap();
        let home = ready(&desk.projects, &desk.themes, &desk.file);
        write(&home, "= Mine, not saved\n");
        save(&home, &desk.themes, &desk.file);
        write(&home, "= Mine, changed again and not saved\n");
        record(&home, &desk.file, true).unwrap();

        let colleague = desk.base.join("colleague");
        let other = ready(&colleague, &desk.themes, &desk.file);
        write(&other, "= Theirs, sent to me, and quite a bit longer\n");
        save(&other, &desk.themes, &desk.file);

        let opened = ready(&desk.projects, &desk.themes, &desk.file);
        assert_eq!(read(&opened), "= Theirs, sent to me, and quite a bit longer\n");
        let rescued = ready(&desk.base.join("rescue"), &desk.themes, &desk.base.join("Notes (recovered 1).znote"));
        assert_eq!(read(&rescued), "= Mine, changed again and not saved\n");
    }

    #[test]
    fn a_file_replaced_from_outside_is_read_again_and_old_working_copies_are_cleared() {
        let desk = desk("replaced");
        let (first, second) = (desk.base.join("First.znote"), desk.base.join("Second.znote"));
        create(&desk.projects, &desk.themes, &first, None).unwrap();
        create(&desk.projects, &desk.themes, &second, None).unwrap();
        let one = ready(&desk.projects, &desk.themes, &first);
        let two = ready(&desk.projects, &desk.themes, &second);
        write(&two, "= Second\n");
        record(&two, &second, true).unwrap();
        tidy(&desk.projects, &one);
        assert!(one.is_dir() && two.is_dir());
        save(&two, &desk.themes, &second);
        tidy(&desk.projects, &one);
        assert!(one.is_dir() && !two.exists());

        let sender = ready(&desk.base.join("sender"), &desk.themes, &first);
        write(&sender, "= Sent to me, with more in it\n");
        save(&sender, &desk.themes, &first);
        let reopened = ready(&desk.projects, &desk.themes, &first);
        assert_eq!(read(&reopened), "= Sent to me, with more in it\n");
        write(&reopened, "= Mine on top of theirs\n");
        save(&reopened, &desk.themes, &first);
        assert_eq!(read(&ready(&desk.base.join("check"), &desk.themes, &first)), "= Mine on top of theirs\n");
        assert!(load(&desk.projects, &desk.themes, &desk.base.join("Missing.znote"), true).is_err_and(|error| error.contains("could not be found")));
    }

    #[test]
    fn fonts_a_page_uses_travel_in_the_file_unless_they_may_not_and_then_the_reader_is_told() {
        let desk = desk("fonts");
        create(&desk.projects, &desk.themes, &desk.file, None).unwrap();
        let home = ready(&desk.projects, &desk.themes, &desk.file);
        write(&home, "= Monday\n");
        let system = desk.base.join("system fonts");
        fs::create_dir_all(&system).unwrap();
        fs::write(system.join("Fancy.ttf"), "fancy bytes").unwrap();
        fs::write(system.join("Locked.ttf"), "locked bytes").unwrap();
        let font = |name: &str, family: &str, restricted| UsedFont { family: family.into(), file: system.join(name), restricted };

        let fancy = carried(&home, &font("Fancy.ttf", "Fancy Znote Test", false));
        let locked = carried(&home, &font("Locked.ttf", "Locked Znote Test", true));
        assert_eq!(fancy.file.as_deref(), Some("Fancy.ttf"));
        assert_eq!(locked.file, None);
        let gone: FontUse = [("Work/Old/Deleted".to_string(), vec![carried(&home, &font("Locked.ttf", "Unused", false))])].into();
        let used: FontUse = gone.into_iter().chain([("Work/Notes/Monday".to_string(), vec![fancy, locked])]).collect();
        keep_font_use(&home, &used);
        save(&home, &desk.themes, &desk.file);
        assert!(!Home(&home).fonts().join("Locked.ttf").exists());
        assert_eq!(font_use(&home).len(), 1);

        let there = ready(&desk.base.join("there"), &desk.themes, &desk.file);
        assert_eq!(fs::read_to_string(Home(&there).fonts().join("Fancy.ttf")).unwrap(), "fancy bytes");
        assert_eq!(fallbacks(&there), ["Locked Znote Test"]);
        let again = carried(&there, &UsedFont { family: "Fancy Znote Test".into(), file: Home(&there).fonts().join("Fancy.ttf"), restricted: false });
        assert_eq!(again.file.as_deref(), Some("Fancy.ttf"));
    }

    #[test]
    #[ignore = "writes about 500 MB to the temporary folder; run it by name to time a save"]
    fn saving_one_changed_line_in_a_large_file_stays_fast() {
        let desk = desk("large");
        create(&desk.projects, &desk.themes, &desk.file, None).unwrap();
        let home = ready(&desk.projects, &desk.themes, &desk.file);
        write(&home, "= Monday\n");
        let mut seed = 0x2545_F491_4F6C_DD1D_u64;
        let mut noise = vec![0_u8; 10 * 1024 * 1024];
        for picture in 0..50 {
            for chunk in noise.chunks_mut(8) {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                chunk.copy_from_slice(&seed.to_le_bytes()[..chunk.len()]);
            }
            fs::write(Home(&home).pages().join("Work").join(format!("photo-{picture}.png")), &noise).unwrap();
        }
        let started = std::time::Instant::now();
        save(&home, &desk.themes, &desk.file);
        let first = started.elapsed();
        write(&home, "= Monday\n- one more line\n");
        let started = std::time::Instant::now();
        save(&home, &desk.themes, &desk.file);
        let again = started.elapsed();
        let size = fs::metadata(&desk.file).unwrap().len() / (1024 * 1024);
        println!("{size} MB file: first save {first:?}, save after one changed line {again:?}");
        let _ = fs::remove_dir_all(&desk.base);
        assert!(again < Duration::from_secs(5), "{again:?}");
    }
}

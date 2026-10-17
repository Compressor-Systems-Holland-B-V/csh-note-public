use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use notify::event::ModifyKind;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
pub use zsuite_agent::pages::*;

pub fn delete(root: &Path, path: &str) -> Result<(), String> {
    trash::delete(resolve(root, path)?)
        .map_err(|error| format!("Could not delete “{path}”: {error}"))
}

fn structural(event: &Event) -> bool {
    let content = matches!(
        event.kind,
        EventKind::Access(_) | EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Metadata(_))
    );
    !content
        && !event
            .paths
            .iter()
            .all(|path| path.extension().is_some_and(|extension| extension == "tmp"))
}

fn watch(app: AppHandle, root: PathBuf) -> notify::Result<RecommendedWatcher> {
    let folder = root.clone();
    let shown = Mutex::new(tree(&root));
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
        let Ok(event) = event else { return };
        if matches!(event.kind, EventKind::Access(_)) {
            return;
        }
        for page in event.paths.iter().filter_map(|path| page_of(&folder, path)) {
            crate::knowledge::saved(&app, page.clone());
            let _ = app.emit("page-changed", page);
        }
        if !event
            .paths
            .iter()
            .all(|path| path.extension().is_some_and(|extension| extension == "tmp"))
        {
            crate::project::changed(&app);
        }
        if !structural(&event) {
            return;
        }
        let now = tree(&folder);
        if let Ok(mut shown) = shown.lock()
            && *shown != now
        {
            *shown = now;
            let _ = app.emit("workspace-changed", ());
        }
    })?;
    watcher.watch(&root, RecursiveMode::Recursive)?;
    Ok(watcher)
}

#[derive(Default)]
pub struct Workspace {
    root: Mutex<Option<PathBuf>>,
    watcher: Mutex<Option<RecommendedWatcher>>,
}

impl Workspace {
    pub fn root(&self) -> Result<PathBuf, String> {
        let root = self.root.lock().map_err(|error| error.to_string())?;
        root.clone()
            .ok_or_else(|| "Choose a workspace folder first.".to_string())
    }

    pub fn typst_source(&self, page: &str) -> Result<(PathBuf, String), String> {
        let root = self.root()?;
        page_file(&root, page)?;
        let (project, main) = page
            .split_once('/')
            .ok_or_else(|| format!("“{page}” is not a page."))?;
        Ok((root.join(project), format!("{main}.typ")))
    }
}

#[derive(Serialize)]
pub struct PickedFile {
    name: String,
    text: String,
}

#[tauri::command]
pub async fn pick_typst_file(app: AppHandle) -> Result<Option<PickedFile>, String> {
    crate::blocking(move || pick_typst_file_now(app)).await
}

fn pick_typst_file_now(app: AppHandle) -> Result<Option<PickedFile>, String> {
    let Some(file) = app
        .dialog()
        .file()
        .add_filter("Typst", &["typ"])
        .blocking_pick_file()
    else {
        return Ok(None);
    };
    let path = file.into_path().map_err(|error| error.to_string())?;
    let text = fs::read_to_string(&path)
        .map_err(|error| format!("The file could not be read: {error}"))?;
    let name = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    Ok(Some(PickedFile { name, text }))
}

#[tauri::command]
pub fn open_workspace(
    app: AppHandle,
    workspace: State<Workspace>,
    folder: String,
) -> Result<Vec<Project>, String> {
    let root = crate::project::pages(&app)?;
    if root != Path::new(&folder) {
        return Err("Open a znote file first.".into());
    }
    fs::create_dir_all(&root).map_err(|error| failed("open", &folder, error))?;
    let watcher = watch(app, root.clone()).map_err(|error| failed("watch", &folder, error))?;
    let projects = tree(&root);
    *workspace
        .watcher
        .lock()
        .map_err(|error| error.to_string())? = Some(watcher);
    *workspace.root.lock().map_err(|error| error.to_string())? = Some(root);
    Ok(projects)
}

#[tauri::command]
pub fn list_tree(workspace: State<Workspace>) -> Result<Vec<Project>, String> {
    Ok(tree(&workspace.root()?))
}

#[tauri::command]
pub fn create_entry(
    app: AppHandle,
    workspace: State<Workspace>,
    path: String,
) -> Result<(), String> {
    create(&workspace.root()?, &path)?;
    crate::project::changed(&app);
    Ok(())
}

#[tauri::command]
pub fn move_entry(
    app: AppHandle,
    workspace: State<Workspace>,
    from: String,
    to: String,
) -> Result<(), String> {
    rename(&workspace.root()?, &from, &to)?;
    crate::project::changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn delete_entry(app: AppHandle, path: String) -> Result<(), String> {
    crate::blocking(move || delete_entry_now(app, path)).await
}

fn delete_entry_now(app: AppHandle, path: String) -> Result<(), String> {
    delete(&app.state::<Workspace>().root()?, &path)?;
    crate::project::changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn search_pages(app: AppHandle, query: String) -> Result<Vec<Hit>, String> {
    let root = app.state::<Workspace>().root()?;
    crate::blocking(move || Ok(find(&root, &query, 200))).await
}

#[tauri::command]
pub fn read_page(workspace: State<Workspace>, path: String) -> Result<String, String> {
    read_text(&workspace.root()?, &path)
}

#[tauri::command]
pub fn write_page(
    app: AppHandle,
    workspace: State<Workspace>,
    path: String,
    text: String,
) -> Result<(), String> {
    write_text(&workspace.root()?, &path, &text)?;
    crate::project::changed(&app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saving_a_file_is_not_a_change_to_the_tree() {
        let event = |kind, path: &str| Event::new(kind).add_path(PathBuf::from(path));
        let renamed = EventKind::Modify(ModifyKind::Name(notify::event::RenameMode::Any));
        assert!(!structural(&event(
            EventKind::Modify(ModifyKind::Data(notify::event::DataChange::Any)),
            "/w/A/B/Page.typ"
        )));
        assert!(!structural(&event(
            EventKind::Create(notify::event::CreateKind::File),
            "/w/A/B/Page.typ.tmp"
        )));
        assert!(structural(&event(renamed, "/w/A/B/Page.typ")));
        assert!(structural(&event(
            EventKind::Remove(notify::event::RemoveKind::Any),
            "/w/A/B"
        )));
    }
}

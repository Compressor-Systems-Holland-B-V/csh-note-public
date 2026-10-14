use std::fmt::Display;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use notify::event::ModifyKind;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

#[derive(Debug, Serialize, PartialEq)]
pub struct Notebook {
    name: String,
    pages: Vec<String>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Project {
    name: String,
    notebooks: Vec<Notebook>,
}

fn failed(action: &str, path: &str, error: impl Display) -> String {
    format!("Could not {action} “{path}”: {error}")
}

fn taken(path: &str) -> String {
    format!("Something named “{path}” already exists.")
}

fn depth(path: &str) -> usize {
    path.split('/').count()
}

pub fn check_name(name: &str) -> Result<(), String> {
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_ascii_uppercase();
    let numbered = stem.len() == 4
        && (stem.starts_with("COM") || stem.starts_with("LPT"))
        && stem.ends_with(|last: char| ('1'..='9').contains(&last));
    let device = numbered || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL");
    let bad_character = name
        .chars()
        .any(|c| c.is_control() || r#"<>:"/\|?*"#.contains(c));
    let bad_edge = name.is_empty() || name.starts_with(['.', ' ']) || name.ends_with(['.', ' ']);
    if device || bad_character || bad_edge || name.len() > 200 {
        return Err(format!(
            "“{name}” cannot be used as a name. Avoid < > : \" / \\ | ? * and names that start or end with a dot or a space."
        ));
    }
    Ok(())
}

fn resolve(root: &Path, path: &str) -> Result<PathBuf, String> {
    let names: Vec<&str> = path.split('/').collect();
    if names.len() > 3 {
        return Err("A page lives in a notebook inside a project. Nothing can go deeper.".into());
    }
    let mut resolved = root.to_path_buf();
    for name in &names {
        check_name(name)?;
        resolved.push(name);
    }
    if names.len() == 3 {
        resolved.as_mut_os_string().push(".typ");
    }
    let anchor = if resolved.exists() {
        Some(resolved.as_path())
    } else {
        resolved.parent()
    };
    let inside = anchor.map(|anchor| {
        anchor
            .canonicalize()
            .and_then(|anchor| Ok(anchor.starts_with(root.canonicalize()?)))
    });
    if !matches!(inside, Some(Ok(true))) {
        return Err(format!("“{path}” could not be found in your workspace."));
    }
    Ok(resolved)
}

fn page_file(root: &Path, path: &str) -> Result<PathBuf, String> {
    if depth(path) != 3 {
        return Err(format!("“{path}” is not a page."));
    }
    resolve(root, path)
}

fn children(folder: &Path, pages: bool) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            entry
                .file_type()
                .is_ok_and(|kind| if pages { kind.is_file() } else { kind.is_dir() })
        })
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter_map(|name| {
            if pages {
                name.strip_suffix(".typ").map(str::to_string)
            } else {
                Some(name)
            }
        })
        .filter(|name| check_name(name).is_ok())
        .collect();
    names.sort_by_key(|name| name.to_lowercase());
    names
}

pub fn tree(root: &Path) -> Vec<Project> {
    children(root, false)
        .into_iter()
        .map(|name| {
            let folder = root.join(&name);
            let notebooks = children(&folder, false)
                .into_iter()
                .map(|name| {
                    let pages = children(&folder.join(&name), true);
                    Notebook { name, pages }
                })
                .collect();
            Project { name, notebooks }
        })
        .collect()
}

pub fn pages(root: &Path) -> Vec<String> {
    tree(root)
        .into_iter()
        .flat_map(|project| {
            project.notebooks.into_iter().flat_map(move |notebook| {
                let folder = format!("{}/{}", project.name, notebook.name);
                notebook
                    .pages
                    .into_iter()
                    .map(move |page| format!("{folder}/{page}"))
            })
        })
        .collect()
}

pub fn ensure(root: &Path, page: &str) -> Result<(), String> {
    let names: Vec<&str> = page.split('/').collect();
    if names.len() != 3 {
        return Err(format!("“{page}” is not a page."));
    }
    for depth in 1..=names.len() {
        let path = names[..depth].join("/");
        if !resolve(root, &path)?.exists() {
            create(root, &path)?;
        }
    }
    Ok(())
}

pub fn create(root: &Path, path: &str) -> Result<(), String> {
    let target = resolve(root, path)?;
    if target.exists() {
        return Err(taken(path));
    }
    let made = if depth(path) == 3 {
        File::create_new(&target).map(drop)
    } else {
        fs::create_dir(&target)
    };
    made.map_err(|error| failed("create", path, error))
}

pub fn rename(root: &Path, from: &str, to: &str) -> Result<(), String> {
    if depth(from) != depth(to) {
        return Err("A page can only move to a notebook, and a notebook to a project.".into());
    }
    let source = resolve(root, from)?;
    let target = resolve(root, to)?;
    if target.exists() {
        return Err(taken(to));
    }
    fs::rename(source, target).map_err(|error| failed("rename", from, error))
}

pub fn delete(root: &Path, path: &str) -> Result<(), String> {
    trash::delete(resolve(root, path)?).map_err(|error| failed("delete", path, error))
}

pub fn read_text(root: &Path, path: &str) -> Result<String, String> {
    fs::read_to_string(page_file(root, path)?).map_err(|error| failed("open", path, error))
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Hit {
    pub page: String,
    pub line: usize,
    pub text: String,
}

pub fn find(root: &Path, query: &str, most: usize) -> Vec<Hit> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    pages(root)
        .into_iter()
        .flat_map(|page| {
            let text = read_text(root, &page).unwrap_or_default();
            let named = page.to_lowercase().contains(&query);
            let lines: Vec<Hit> = text
                .lines()
                .enumerate()
                .filter(|(_, line)| line.to_lowercase().contains(&query))
                .map(|(number, line)| Hit {
                    page: page.clone(),
                    line: number + 1,
                    text: line.trim().to_string(),
                })
                .collect();
            let title = (named && lines.is_empty()).then(|| Hit {
                page: page.clone(),
                line: 0,
                text: String::new(),
            });
            title.into_iter().chain(lines)
        })
        .take(most)
        .collect()
}

pub fn page_exists(root: &Path, path: &str) -> bool {
    page_file(root, path).is_ok_and(|file| file.is_file())
}

pub fn write_text(root: &Path, path: &str, text: &str) -> Result<(), String> {
    let file = page_file(root, path)?;
    let temp = file.with_extension("typ.tmp");
    File::create(&temp)
        .and_then(|mut handle| {
            handle.write_all(text.as_bytes())?;
            handle.sync_all()
        })
        .and_then(|()| fs::rename(&temp, &file))
        .map_err(|error| failed("save", path, error))
}

fn page_of(root: &Path, file: &Path) -> Option<String> {
    if file.extension()? != "typ" {
        return None;
    }
    let relative = file.strip_prefix(root).ok()?.with_extension("");
    let names: Vec<&str> = relative
        .iter()
        .map(|name| name.to_str())
        .collect::<Option<_>>()?;
    (names.len() == 3).then(|| names.join("/"))
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

#[tauri::command(async)]
pub fn pick_typst_file(app: AppHandle) -> Result<Option<PickedFile>, String> {
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
    let root = PathBuf::from(&folder);
    if !root.is_absolute() {
        return Err(format!("“{folder}” is not a full folder path."));
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

#[tauri::command(async)]
pub fn delete_entry(app: AppHandle, path: String) -> Result<(), String> {
    delete(&app.state::<Workspace>().root()?, &path)?;
    crate::project::changed(&app);
    Ok(())
}

#[tauri::command(async)]
pub fn search_pages(workspace: State<Workspace>, query: String) -> Result<Vec<Hit>, String> {
    Ok(find(&workspace.root()?, &query, 200))
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

    fn workspace(test: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("znote-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn filled(test: &str) -> PathBuf {
        let root = workspace(test);
        for path in ["Work", "Work/Notes", "Work/Notes/Monday"] {
            create(&root, path).unwrap();
        }
        root
    }

    #[test]
    fn search_finds_lines_in_any_case_and_pages_by_name() {
        let root = filled("find");
        create(&root, "Work/Notes/Budget").unwrap();
        write_text(&root, "Work/Notes/Monday", "= Monday\nCall ACME about the budget\n").unwrap();
        let hits = find(&root, " budget ", 10);
        assert_eq!(
            hits,
            [
                Hit {
                    page: "Work/Notes/Budget".into(),
                    line: 0,
                    text: String::new()
                },
                Hit {
                    page: "Work/Notes/Monday".into(),
                    line: 2,
                    text: "Call ACME about the budget".into()
                },
            ]
        );
        assert_eq!(find(&root, "budget", 1).len(), 1);
        assert!(find(&root, "  ", 10).is_empty());
    }

    #[test]
    fn the_tree_has_three_sorted_levels_and_ignores_everything_else() {
        let root = filled("tree");
        for path in ["Work/Notes/alpha", "Work/Archive", "Home"] {
            create(&root, path).unwrap();
        }
        fs::create_dir_all(root.join("Work/Notes/deeper/still")).unwrap();
        fs::create_dir(root.join(".git")).unwrap();
        fs::write(root.join("Work/Notes/picture.png"), "").unwrap();
        fs::write(root.join("Work/loose.typ"), "").unwrap();
        fs::write(root.join("readme.txt"), "").unwrap();

        let notebook = |name: &str, pages: &[&str]| Notebook {
            name: name.into(),
            pages: pages.iter().map(|page| page.to_string()).collect(),
        };
        let expected = [
            Project {
                name: "Home".into(),
                notebooks: Vec::new(),
            },
            Project {
                name: "Work".into(),
                notebooks: vec![
                    notebook("Archive", &[]),
                    notebook("Notes", &["alpha", "Monday"]),
                ],
            },
        ];
        assert_eq!(tree(&root), expected);
    }

    #[test]
    fn nothing_reaches_outside_the_workspace() {
        let root = filled("escape");
        let outside = root.with_extension("outside");
        fs::write(&outside, "secret").unwrap();

        for path in [
            "..",
            "../x",
            "Work/../..",
            "Work/Notes/..",
            "Work/Notes/../../../x",
            "/etc",
            "Work//Monday",
            "",
        ] {
            assert!(create(&root, path).is_err(), "create {path}");
            assert!(delete(&root, path).is_err(), "delete {path}");
            assert!(
                rename(&root, "Work/Notes/Monday", path).is_err(),
                "rename to {path}"
            );
            assert!(
                rename(&root, path, "Work/Notes/Other").is_err(),
                "rename from {path}"
            );
            assert!(read_text(&root, path).is_err(), "read {path}");
            assert!(write_text(&root, path, "x").is_err(), "write {path}");
        }
        assert!(create(&root, "Work/Notes/Monday/deeper").is_err());
        assert!(read_text(&root, "Work/Notes").is_err());
        assert_eq!(fs::read_to_string(&outside).unwrap(), "secret");
        assert_eq!(read_text(&root, "Work/Notes/Monday").unwrap(), "");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_that_leaves_the_workspace_is_refused() {
        let root = filled("link");
        let outside = workspace("link-target");
        fs::write(outside.join("Page.typ"), "secret").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("Work/Linked")).unwrap();

        assert!(read_text(&root, "Work/Linked/Page").is_err());
        assert!(write_text(&root, "Work/Linked/Page", "changed").is_err());
        assert!(create(&root, "Work/Linked/New").is_err());
        assert_eq!(
            fs::read_to_string(outside.join("Page.typ")).unwrap(),
            "secret"
        );
        assert_eq!(tree(&root)[0].notebooks.len(), 1);
    }

    #[test]
    fn names_windows_cannot_store_are_refused() {
        let bad = [
            "",
            ".",
            "..",
            "CON",
            "con",
            "NUL.txt",
            "com1",
            "LPT9.typ",
            "a<b",
            "a>b",
            "a:b",
            "a\"b",
            "a/b",
            "a\\b",
            "a|b",
            "a?b",
            "a*b",
            "tab\there",
            "ends.",
            "ends ",
            " starts",
            ".hidden",
        ];
        for name in bad {
            assert!(check_name(name).is_err(), "{name:?} should be refused");
        }
        for name in [
            "Notes",
            "Q3 report (final)",
            "console",
            "COM10",
            "v1.2",
            "Überblick",
        ] {
            assert!(check_name(name).is_ok(), "{name:?} should be allowed");
        }
    }

    #[test]
    fn a_page_keeps_what_was_written_and_leaves_no_temporary_file() {
        let root = filled("write");
        write_text(&root, "Work/Notes/Monday", "= Héllo").unwrap();
        write_text(&root, "Work/Notes/Monday", "= Again").unwrap();
        assert_eq!(read_text(&root, "Work/Notes/Monday").unwrap(), "= Again");
        assert_eq!(fs::read_dir(root.join("Work/Notes")).unwrap().count(), 1);
    }

    #[test]
    fn renaming_and_moving_never_replace_what_is_already_there() {
        let root = filled("rename");
        create(&root, "Work/Other").unwrap();
        create(&root, "Work/Other/Monday").unwrap();
        write_text(&root, "Work/Notes/Monday", "mine").unwrap();

        assert!(create(&root, "Work/Notes/Monday").is_err());
        assert!(rename(&root, "Work/Notes/Monday", "Work/Other/Monday").is_err());
        assert!(rename(&root, "Work/Notes/Monday", "Work/Other").is_err());

        rename(&root, "Work/Notes/Monday", "Work/Other/Tuesday").unwrap();
        assert_eq!(read_text(&root, "Work/Other/Tuesday").unwrap(), "mine");
        rename(&root, "Work/Other", "Work/Renamed").unwrap();
        rename(&root, "Work", "Office").unwrap();
        assert_eq!(read_text(&root, "Office/Renamed/Tuesday").unwrap(), "mine");
    }

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

    #[test]
    fn only_page_files_three_levels_down_are_reported_as_pages() {
        let root = Path::new("/w");
        assert_eq!(
            page_of(root, Path::new("/w/Work/Notes/v1.2.typ")),
            Some("Work/Notes/v1.2".into())
        );
        assert_eq!(
            page_of(root, Path::new("/w/Work/Notes/Monday.typ.tmp")),
            None
        );
        assert_eq!(page_of(root, Path::new("/w/Work/helper.typ")), None);
        assert_eq!(page_of(root, Path::new("/w/Work/Notes/deep/x.typ")), None);
        assert_eq!(
            page_of(root, Path::new("/elsewhere/Work/Notes/x.typ")),
            None
        );
    }
}

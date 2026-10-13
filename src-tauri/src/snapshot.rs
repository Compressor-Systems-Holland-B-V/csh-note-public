use std::collections::BTreeSet;
use std::fmt::Display;
use std::fs::{self, File};
use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use typst::syntax::package::PackageVersion;
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

use crate::theme;
use crate::workspace::{Workspace, check_name};

const MAX_UNPACKED_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_ENTRIES: usize = 50_000;

#[derive(Debug, Serialize, Deserialize)]
struct Manifest {
    format: u32,
    look: Option<String>,
    themes: Vec<String>,
}

fn unsaved(error: impl Display) -> String {
    format!("Your notes could not be saved to a file: {error}")
}

fn unreadable(error: impl Display) -> String {
    format!("This znote file could not be read: {error}")
}

fn imports(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split("@themes/").skip(1).filter_map(|rest| {
        let spec = rest.split(|c: char| !(c.is_ascii_alphanumeric() || "-.:".contains(c))).next()?;
        let (id, version) = spec.split_once(':')?;
        (theme::valid_id(id) && version.parse::<PackageVersion>().is_ok()).then(|| spec.to_string())
    })
}

fn files(folder: &Path, found: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(folder)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let path = entry.path();
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if kind.is_dir() {
            files(&path, found)?;
        } else if kind.is_file() && path.extension().is_none_or(|extension| extension != "znote") {
            found.push(path);
        }
    }
    Ok(())
}

pub fn pack(root: &Path, look: Option<String>) -> Result<Vec<u8>, String> {
    let mut found = Vec::new();
    files(root, &mut found).map_err(unsaved)?;
    found.sort();
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let mut themes = BTreeSet::new();
    for file in found {
        let names: Option<Vec<&str>> =
            file.strip_prefix(root).map_err(unsaved)?.iter().map(|name| name.to_str()).collect();
        let names = names.ok_or_else(|| unsaved(format!("“{}” has a name that cannot be stored", file.display())))?;
        if names.len() < 2 {
            continue;
        }
        let data = fs::read(&file).map_err(unsaved)?;
        if file.extension().is_some_and(|extension| extension == "typ") {
            themes.extend(imports(&String::from_utf8_lossy(&data)));
        }
        writer.start_file(format!("pages/{}", names.join("/")), SimpleFileOptions::default()).map_err(unsaved)?;
        writer.write_all(&data).map_err(unsaved)?;
    }
    let manifest = Manifest { format: 1, look, themes: themes.into_iter().collect() };
    writer.start_file("znote.json", SimpleFileOptions::default()).map_err(unsaved)?;
    writer.write_all(&serde_json::to_vec_pretty(&manifest).map_err(unsaved)?).map_err(unsaved)?;
    Ok(writer.finish().map_err(unsaved)?.into_inner())
}

fn missing(manifest: &Manifest, installed: &[(String, String)]) -> Vec<String> {
    let mut needed: Vec<String> = manifest
        .themes
        .iter()
        .filter(|spec| !installed.iter().any(|(id, version)| **spec == format!("{id}:{version}")))
        .map(|spec| spec.replace(':', " "))
        .collect();
    if let Some(look) = &manifest.look {
        let listed = needed.iter().any(|theme| theme.split(' ').next() == Some(look));
        if !listed && !installed.iter().any(|(id, _)| id == look) {
            needed.push(look.clone());
        }
    }
    needed
}

fn projects(folder: &Path) -> io::Result<Vec<(String, PathBuf)>> {
    let mut found = Vec::new();
    for entry in fs::read_dir(folder)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type()?.is_dir() && check_name(&name).is_ok() {
            found.push((name, entry.path()));
        }
    }
    Ok(found)
}

pub fn open(
    file: &Path,
    themes: &Path,
    root: &Path,
    discard: impl Fn(&Path) -> Result<(), String>,
) -> Result<Option<String>, String> {
    let mut archive = ZipArchive::new(File::open(file).map_err(unreadable)?)
        .map_err(|_| "This file is not a znote file.".to_string())?;
    if archive.len() > MAX_ENTRIES {
        return Err("This znote file has too many files in it, so it was not opened.".into());
    }
    let mut json = Vec::new();
    archive
        .by_name("znote.json")
        .map_err(|_| "This file is not a znote file: it has no znote.json.".to_string())?
        .take(MAX_MANIFEST_BYTES)
        .read_to_end(&mut json)
        .map_err(unreadable)?;
    let manifest: Manifest = serde_json::from_slice(&json).map_err(unreadable)?;
    if manifest.format != 1 {
        return Err("This file was made by a newer version of znote.".into());
    }
    let needed = missing(&manifest, &theme::installed(themes));
    if !needed.is_empty() {
        return Err(format!(
            "This file cannot be opened because it needs themes that are not installed:\n• {}\nInstall them on the Themes page, then open the file again.",
            needed.join("\n• ")
        ));
    }

    let staging = root.join(format!(".incoming-{}", std::process::id()));
    let _ = fs::remove_dir_all(&staging);
    let replaced = theme::unpack(&mut archive, &staging, MAX_UNPACKED_BYTES, "znote file").and_then(|()| {
        let pages = staging.join("pages");
        fs::create_dir_all(&pages).map_err(unreadable)?;
        let incoming = projects(&pages).map_err(unreadable)?;
        for (_, old) in projects(root).map_err(unreadable)? {
            discard(&old)?;
        }
        for (name, project) in incoming {
            fs::rename(project, root.join(name)).map_err(unreadable)?;
        }
        Ok(())
    });
    let _ = fs::remove_dir_all(&staging);
    replaced?;
    Ok(manifest.look)
}

#[tauri::command(async)]
pub fn save_snapshot(app: AppHandle, workspace: State<Workspace>, look: Option<String>) -> Result<Option<String>, String> {
    let root = workspace.root()?;
    let name = root.file_name().unwrap_or_default().to_string_lossy();
    let dialog = app.dialog().file().set_file_name(format!("{name}.znote")).add_filter("znote file", &["znote"]);
    let Some(target) = dialog.blocking_save_file() else {
        return Ok(None);
    };
    let mut path = target.into_path().map_err(|error| error.to_string())?;
    if path.extension().is_none_or(|given| !given.eq_ignore_ascii_case("znote")) {
        path.as_mut_os_string().push(".znote");
    }
    let bytes = pack(&root, look)?;
    let mut temp = path.clone();
    temp.as_mut_os_string().push(".tmp");
    fs::write(&temp, bytes).and_then(|()| fs::rename(&temp, &path)).map_err(unsaved)?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

#[derive(Serialize)]
pub struct Opened {
    look: Option<String>,
}

#[tauri::command(async)]
pub fn open_snapshot(app: AppHandle, workspace: State<Workspace>) -> Result<Option<Opened>, String> {
    let Some(file) = app.dialog().file().add_filter("znote file", &["znote"]).blocking_pick_file() else {
        return Ok(None);
    };
    let file = file.into_path().map_err(|error| error.to_string())?;
    let recycle = |old: &Path| {
        trash::delete(old).map_err(|error| format!("Your current notes could not be moved to the Recycle Bin: {error}"))
    };
    let look = open(&file, &theme::folder(&app)?, &workspace.root()?, recycle)?;
    Ok(Some(Opened { look }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("znote-snapshot-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn put(file: PathBuf, text: &str) {
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }

    fn install(themes: &Path, id: &str, version: &str) {
        let json = format!(r#"{{ "format": 1, "id": "{id}", "name": "Theme", "version": "{version}" }}"#);
        put(themes.join(id).join(version).join("theme.json"), &json);
    }

    fn archive(file: &Path, entries: &[(&str, &str)]) {
        let mut writer = ZipWriter::new(File::create(file).unwrap());
        for (name, text) in entries {
            writer.start_file(*name, SimpleFileOptions::default()).unwrap();
            writer.write_all(text.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
    }

    fn erase(old: &Path) -> Result<(), String> {
        fs::remove_dir_all(old).map_err(|error| error.to_string())
    }

    #[test]
    fn opening_replaces_the_notes_only_once_every_theme_they_use_is_installed() {
        let folder = scratch("round");
        let root = folder.join("notes");
        put(root.join("Work/Notes/Monday.typ"), "// @themes/nothing here\n#import \"@themes/acme:0.1.0\": *\n= Hi");
        put(root.join("Work/Notes/Plain.typ"), "#import \"@preview/cetz:0.3.0\": *");
        put(root.join("Work/images/logo.svg"), "<svg/>");
        put(root.join("Work/earlier.znote"), "old");
        put(root.join("loose.txt"), "loose");
        put(root.join(".git/config"), "hidden");
        let file = folder.join("notes.znote");
        fs::write(&file, pack(&root, Some("paper".into())).unwrap()).unwrap();

        let (themes, target) = (folder.join("themes"), folder.join("other"));
        put(target.join("Home/Diary/Today.typ"), "mine");
        put(target.join("Work/Notes/Monday.typ"), "older");
        put(target.join("kept.txt"), "kept");
        put(target.join(".git/config"), "kept");

        let error = open(&file, &themes, &target, erase).unwrap_err();
        assert!(error.contains("not installed:\n• acme 0.1.0\n• paper\nInstall"), "{error}");
        install(&themes, "paper", "2.0.0");
        install(&themes, "acme", "0.2.0");
        assert!(open(&file, &themes, &target, erase).unwrap_err().contains("not installed:\n• acme 0.1.0\nInstall"));
        assert_eq!(fs::read_to_string(target.join("Home/Diary/Today.typ")).unwrap(), "mine");
        assert_eq!(fs::read_to_string(target.join("Work/Notes/Monday.typ")).unwrap(), "older");

        fs::remove_dir_all(themes.join("acme")).unwrap();
        install(&themes, "acme", "0.1.0");
        assert_eq!(open(&file, &themes, &target, erase).unwrap().as_deref(), Some("paper"));
        assert!(fs::read_to_string(target.join("Work/Notes/Monday.typ")).unwrap().ends_with("= Hi"));
        assert!(target.join("Work/images/logo.svg").is_file());
        assert!(!target.join("Home").exists());
        assert!(!target.join("Work/earlier.znote").exists());
        assert!(!target.join("loose.txt").exists());
        assert_eq!(fs::read_to_string(target.join("kept.txt")).unwrap(), "kept");
        assert_eq!(fs::read_to_string(target.join(".git/config")).unwrap(), "kept");
        assert_eq!(fs::read_dir(&target).unwrap().count(), 3);
    }

    #[test]
    fn a_file_that_is_not_ours_or_reaches_outside_its_folder_changes_nothing() {
        let folder = scratch("refused");
        let (file, themes, target) = (folder.join("bad.znote"), folder.join("themes"), folder.join("inner/notes"));
        let manifest = r#"{ "format": 1, "look": null, "themes": [] }"#;
        put(target.join("Home/Diary/Today.typ"), "mine");

        for name in ["../escaped.txt", "pages/../../../escaped.txt", "/escaped.txt", "pages\\..\\..\\escaped.txt"] {
            archive(&file, &[("znote.json", manifest), ("pages/Work/Notes/A.typ", ""), (name, "gotcha")]);
            let error = open(&file, &themes, &target, erase).unwrap_err();
            assert!(error.contains("outside its own folder"), "{name}: {error}");
            assert!(!folder.join("escaped.txt").exists() && !folder.join("inner/escaped.txt").exists());
        }

        archive(&file, &[("pages/Work/Notes/A.typ", "")]);
        assert!(open(&file, &themes, &target, erase).unwrap_err().contains("no znote.json"));
        archive(&file, &[("znote.json", r#"{ "format": 2, "look": null, "themes": [] }"#)]);
        assert!(open(&file, &themes, &target, erase).unwrap_err().contains("newer version"));
        fs::write(&file, "plain text").unwrap();
        assert!(open(&file, &themes, &target, erase).unwrap_err().contains("not a znote file"));
        assert_eq!(fs::read_to_string(target.join("Home/Diary/Today.typ")).unwrap(), "mine");
        assert_eq!(fs::read_dir(&target).unwrap().count(), 1);

        archive(&file, &[("znote.json", manifest), ("pages/CON/Notes/A.typ", ""), ("pages/.hidden/x", "")]);
        assert_eq!(open(&file, &themes, &target, erase).unwrap(), None);
        assert_eq!(fs::read_dir(&target).unwrap().count(), 0);
    }
}

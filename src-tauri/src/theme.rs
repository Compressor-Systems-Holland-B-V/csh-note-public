//! `.znx` add-ons: Typst templates and the Typst package they import. The look of the interface
//! comes in a `.zth` file, read by the `zsuite-theme` crate.

use std::fmt::Display;
use std::fs::{self, File};
use std::io::{Cursor, Read, Seek};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;
use typst_kit::downloader::{Downloader, SystemDownloader};
use zip::ZipArchive;
use zsuite_theme::archive::{
    MAX_ARCHIVE_BYTES, MAX_CATALOG_BYTES, MAX_ENTRIES, MAX_MANIFEST_BYTES, MAX_UNPACKED_BYTES,
    folders, is_label, version,
};
pub use zsuite_theme::archive::{unpack, valid_id};
use zsuite_theme::{FontFile, Look, Tokens};

use crate::compile::Compiler;

const FORMAT: u32 = 1;

/// The most of a package file that is read, the most of its outline the assistant is shown, and
/// the largest guide.md.
const MAX_SOURCE_BYTES: u64 = 256 * 1024;
const MAX_OUTLINE_BYTES: usize = 6000;
const MAX_GUIDE_BYTES: u64 = 16 * 1024;

const STORE: &str = "https://huggingface.co/datasets/zal-analytics-core/znote-data/resolve/main";
const LOOKS: &str = "https://huggingface.co/datasets/zal-analytics-core/zsuite-themes/resolve/main";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Document,
    Slides,
    Poster,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Template {
    label: String,
    kind: Kind,
    snippet: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Typst {
    #[serde(default)]
    templates: Vec<Template>,
}

// Unknown fields are refused, so a look put in a .znx by mistake is not silently dropped.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: u32,
    id: String,
    name: String,
    version: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    typst: Typst,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Part {
    Look,
    Templates,
}

/// A look or a set of templates, as the marketplace and the rest of znote see it.
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Theme {
    part: Part,
    id: String,
    name: String,
    version: String,
    description: String,
    category: String,
    tags: Vec<String>,
    logo: Option<String>,
    logo_dark: Option<String>,
    font_family: Option<String>,
    fonts: Vec<FontFile>,
    light: Tokens,
    dark: Tokens,
    templates: Vec<Template>,
    /// The top-level `#let` lines of the add-on's Typst package, for the assistant.
    outline: String,
    /// The add-on's guide.md: how to write pages with its templates, for the assistant.
    guide: String,
}

impl From<Look> for Theme {
    fn from(look: Look) -> Self {
        Theme {
            part: Part::Look,
            id: look.id,
            name: look.name,
            version: look.version,
            description: look.description,
            category: look.category,
            tags: look.tags,
            logo: look.logo,
            logo_dark: look.logo_dark,
            font_family: look.font_family,
            fonts: look.fonts,
            light: look.light,
            dark: look.dark,
            templates: Vec::new(),
            outline: String::new(),
            guide: String::new(),
        }
    }
}

fn broken(error: impl Display) -> String {
    format!("This add-on file could not be read: {error}")
}

fn failed(action: &str, error: impl Display) -> String {
    format!("Could not {action} the theme: {error}")
}

fn check(manifest: &Manifest) -> Result<(), String> {
    if manifest.format != FORMAT {
        return Err("This theme was made for a newer version of znote.".into());
    }
    zsuite_theme::check_about(
        &manifest.id,
        &manifest.version,
        &manifest.name,
        &manifest.description,
        &manifest.category,
        &manifest.tags,
    )?;
    for template in &manifest.typst.templates {
        if !is_label(&template.label, " -_.&") {
            return Err("A template name must be 1 to 60 letters, digits or spaces.".into());
        }
    }
    Ok(())
}

fn parse(json: &[u8]) -> Result<Manifest, String> {
    let manifest = serde_json::from_slice(json)
        .map_err(|error| format!("The theme’s theme.json is not valid: {error}"))?;
    check(&manifest)?;
    Ok(manifest)
}

/// An add-on as listed; `folder` is where it is installed, if it is.
fn listed(manifest: Manifest, folder: Option<&Path>) -> Theme {
    Theme {
        part: Part::Templates,
        id: manifest.id,
        name: manifest.name,
        version: manifest.version,
        description: manifest.description,
        category: manifest.category,
        tags: manifest.tags,
        logo: None,
        logo_dark: None,
        font_family: None,
        fonts: Vec::new(),
        light: Tokens::new(),
        dark: Tokens::new(),
        templates: manifest.typst.templates,
        outline: folder.map(outline_of).unwrap_or_default(),
        guide: folder
            .and_then(|folder| guide_of(folder).ok())
            .unwrap_or_default(),
    }
}

fn read(folder: &Path) -> Result<Manifest, String> {
    parse(&fs::read(folder.join("theme.json")).map_err(broken)?)
}

/// The guide.md every add-on must carry: Markdown that tells the assistant how to write pages
/// with its templates.
fn guide_of(folder: &Path) -> Result<String, String> {
    let refused = |why: &str| format!("This add-on {why}, so it was not installed.");
    let file = File::open(folder.join("guide.md"))
        .map_err(|_| refused("has no guide.md that explains its templates"))?;
    let mut guide = Vec::new();
    file.take(MAX_GUIDE_BYTES + 1)
        .read_to_end(&mut guide)
        .map_err(broken)?;
    if guide.len() as u64 > MAX_GUIDE_BYTES {
        return Err(refused("has a guide.md larger than 16 KB"));
    }
    let guide = String::from_utf8(guide).map_err(|_| refused("has a guide.md that is not text"))?;
    if guide.trim().is_empty() {
        return Err(refused("has an empty guide.md"));
    }
    Ok(guide)
}

fn inside(path: &Path) -> bool {
    path.components()
        .all(|part| matches!(part, Component::Normal(_)))
}

/// What the Typst package of an installed add-on offers a page: the top-level `#let` lines its
/// entrypoint exports, with the comments right above them.
fn outline_of(folder: &Path) -> String {
    let package = folder.join("typst");
    let entrypoint = fs::read_to_string(package.join("typst.toml"))
        .ok()
        .and_then(|manifest| {
            manifest.lines().find_map(|line| {
                let (key, value) = line.split_once('=')?;
                (key.trim() == "entrypoint").then(|| value.trim().trim_matches('"').to_string())
            })
        })
        .unwrap_or_else(|| "lib.typ".into());
    let mut found = String::new();
    outline_file(&package, Path::new(&entrypoint), None, &mut found, 4);
    found
}

/// Adds the `#let` lines of one package file, after those of the files it imports by a relative
/// path. `names` keeps only what an import names; paths stay inside the package.
fn outline_file(
    package: &Path,
    file: &Path,
    names: Option<&[&str]>,
    found: &mut String,
    depth: u8,
) {
    let mut source = Vec::new();
    if depth == 0
        || !inside(file)
        || File::open(package.join(file))
            .and_then(|opened| opened.take(MAX_SOURCE_BYTES).read_to_end(&mut source))
            .is_err()
    {
        return;
    }
    let source = String::from_utf8_lossy(&source);
    for line in source.lines() {
        let Some((path, list)) = line
            .strip_prefix("#import \"")
            .and_then(|rest| rest.split_once("\":"))
        else {
            continue;
        };
        let listed: Vec<&str> = list
            .split(',')
            .map(|name| name.split(" as ").next().unwrap_or("").trim())
            .collect();
        let wanted = names.or((listed != ["*"]).then_some(listed.as_slice()));
        let path = file.parent().unwrap_or(Path::new("")).join(path);
        outline_file(package, &path, wanted, found, depth - 1);
    }
    outline(&source, names, found);
}

fn outline(source: &str, names: Option<&[&str]>, found: &mut String) {
    let mut comments = String::new();
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        let rest = &source[offset..];
        offset += line.len();
        if line.starts_with("//") {
            comments.push_str(line.trim_end());
            comments.push('\n');
            continue;
        }
        let comment = std::mem::take(&mut comments);
        let Some(entry) = line
            .starts_with("#let ")
            .then(|| signature(&rest[5..]))
            .flatten()
        else {
            continue;
        };
        let name = entry.split(['(', ' ']).next().unwrap_or("");
        if name.starts_with('_')
            || names.is_some_and(|names| !names.contains(&name))
            || found.contains(&format!("#let {entry}\n"))
        {
            continue;
        }
        let entry = format!("{comment}#let {entry}\n");
        if found.len() + entry.len() > MAX_OUTLINE_BYTES {
            return;
        }
        found.push_str(&entry);
    }
}

/// The name and parameters of a `#let`, or the name and a short value; `None` when it never ends.
fn signature(rest: &str) -> Option<String> {
    let (mut depth, mut quoted, mut escaped) = (0, false, false);
    for (at, character) in rest.char_indices() {
        if quoted {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                quoted = false;
            }
            continue;
        }
        match character {
            '"' => quoted = true,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            '\n' if depth == 0 => return None,
            '=' if depth == 0 => {
                let name = rest[..at].split_whitespace().collect::<Vec<_>>().join(" ");
                if name.contains('(') {
                    return Some(name);
                }
                let value = rest[at + 1..].lines().next().unwrap_or("").trim();
                let short = if value.chars().count() > 80 {
                    format!("{}…", value.chars().take(79).collect::<String>())
                } else {
                    value.to_string()
                };
                return Some(format!("{name} = {short}"));
            }
            _ => {}
        }
    }
    None
}

/// Where looks are kept: next to the add-ons, so every place that knows one knows the other.
pub fn looks_of(themes: &Path) -> PathBuf {
    themes.with_file_name("looks")
}

pub fn install(themes: &Path, archive: &Path) -> Result<Theme, String> {
    let file = File::open(archive).map_err(broken)?;
    if file.metadata().map_err(broken)?.len() > MAX_ARCHIVE_BYTES {
        return Err("This add-on file is larger than 50 MB, so it was not imported.".into());
    }
    unpacked(themes, file, None)
}

fn unpacked(
    themes: &Path,
    archive: impl Read + Seek,
    wanted: Option<&str>,
) -> Result<Theme, String> {
    let mut archive =
        ZipArchive::new(archive).map_err(|_| "This file is not a znote add-on.".to_string())?;
    if archive.len() > MAX_ENTRIES {
        return Err("This theme has too many files, so it was not imported.".into());
    }
    let mut json = Vec::new();
    archive
        .by_name("theme.json")
        .map_err(|_| "This file is not a znote add-on: it has no theme.json.".to_string())?
        .take(MAX_MANIFEST_BYTES)
        .read_to_end(&mut json)
        .map_err(broken)?;
    let manifest = parse(&json)?;
    if wanted.is_some_and(|id| id != manifest.id) {
        return Err("The marketplace sent a different add-on than the one asked for, so it was not installed.".into());
    }

    let staging = themes.join(format!(".incoming-{}", manifest.id));
    let home = themes.join(&manifest.id);
    let folder = home.join(&manifest.version);
    let _ = fs::remove_dir_all(&staging);
    let moved = unpack(&mut archive, &staging, MAX_UNPACKED_BYTES, "theme")
        .and_then(|()| read(&staging))
        .and_then(|_| guide_of(&staging))
        .and_then(|_| {
            let _ = fs::remove_dir_all(&folder);
            fs::create_dir_all(&home)
                .and_then(|()| fs::rename(&staging, &folder))
                .map_err(|error| failed("save", error))
        });
    if moved.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    moved?;
    Ok(listed(manifest, Some(&folder)))
}

pub fn named(folder: &Path) -> Option<String> {
    read(folder)
        .ok()
        .map(|theme| (theme.name, theme.version))
        .or_else(|| {
            zsuite_theme::open(folder)
                .ok()
                .map(|look| (look.name, look.version))
        })
        .map(|(name, version)| format!("{name} {version}"))
}

pub fn adopt(themes: &Path, folder: &Path) -> Result<(), String> {
    let theme = read(folder)?;
    if !folder.ends_with(Path::new(&theme.id).join(&theme.version)) {
        return Err(format!(
            "The theme “{}” is not stored where it says it is, so it was not installed.",
            theme.name
        ));
    }
    let target = themes.join(&theme.id).join(&theme.version);
    if target.is_dir() {
        return Ok(());
    }
    let staging = themes.join(format!(".incoming-{}", theme.id));
    let _ = fs::remove_dir_all(&staging);
    let moved =
        zsuite_theme::archive::copy(folder, &staging, &mut MAX_UNPACKED_BYTES.clone(), &mut 0)
            .and_then(|()| read(&staging).and_then(|_| guide_of(&staging)).map(drop))
            .and_then(|()| {
                fs::create_dir_all(themes.join(&theme.id))
                    .and_then(|()| fs::rename(&staging, &target))
                    .map_err(|error| failed("save", error))
            });
    if moved.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    moved
}

/// A look carried in a project file.
pub fn adopt_look(themes: &Path, folder: &Path) -> Result<(), String> {
    zsuite_theme::adopt(&looks_of(themes), folder)
}

fn latest(found: Vec<Theme>) -> Vec<Theme> {
    let mut kept: Vec<Theme> = Vec::new();
    for theme in found {
        match kept
            .iter_mut()
            .find(|other| other.id == theme.id && other.part == theme.part)
        {
            Some(other) if version(&other.version) < version(&theme.version) => *other = theme,
            Some(_) => {}
            None => kept.push(theme),
        }
    }
    kept
}

pub fn list(themes: &Path) -> Vec<Theme> {
    let mut found: Vec<Theme> = folders(themes)
        .iter()
        .flat_map(|home| folders(home))
        .filter_map(|folder| {
            let theme = read(&folder).ok()?;
            folder
                .ends_with(Path::new(&theme.id).join(&theme.version))
                .then(|| listed(theme, Some(&folder)))
        })
        .collect();
    found.sort_by_key(|theme| theme.name.to_lowercase());
    found
}

pub fn installed(themes: &Path) -> Vec<(String, String)> {
    list(themes)
        .into_iter()
        .map(|theme| (theme.id, theme.version))
        .collect()
}

pub fn installed_looks(themes: &Path) -> Vec<(String, String)> {
    zsuite_theme::list(&looks_of(themes))
        .into_iter()
        .map(|look| (look.id, look.version))
        .collect()
}

pub fn remove(themes: &Path, id: &str) -> Result<(), String> {
    if !valid_id(id) {
        return Err(format!("“{id}” is not a theme."));
    }
    fs::remove_dir_all(themes.join(id)).map_err(|error| failed("remove", error))
}

pub fn catalog(json: &[u8]) -> Result<Vec<Theme>, String> {
    let entries: Vec<serde_json::Value> = serde_json::from_slice(json)
        .map_err(|_| "The marketplace sent a list that znote cannot read.".to_string())?;
    let mut found: Vec<Theme> = entries
        .into_iter()
        .filter_map(|entry| {
            let manifest = serde_json::from_value(entry).ok()?;
            check(&manifest).ok()?;
            Some(listed(manifest, None))
        })
        .collect();
    found.sort_by_key(|theme| theme.name.to_lowercase());
    Ok(found)
}

fn fetch(store: &str, path: &str, limit: u64) -> Result<Vec<u8>, String> {
    let unreachable = |_| {
        "The marketplace could not be reached. Check your internet connection and try again."
            .to_string()
    };
    let downloader = SystemDownloader::new(concat!("znote/", env!("CARGO_PKG_VERSION")));
    let (_, reader) = downloader
        .stream(&(), &format!("{store}/{path}"))
        .map_err(unreachable)?;
    let mut data = Vec::new();
    reader
        .take(limit + 1)
        .read_to_end(&mut data)
        .map_err(unreachable)?;
    if data.len() as u64 > limit {
        return Err("The marketplace sent more data than znote accepts.".into());
    }
    Ok(data)
}

pub fn folder(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("themes"))
}

fn everything(themes: &Path) -> Vec<Theme> {
    let mut found = latest(list(themes));
    let looks = zsuite_theme::list(&looks_of(themes));
    found.extend(latest(looks.into_iter().map(Theme::from).collect()));
    found
}

#[tauri::command]
pub fn list_themes(app: AppHandle) -> Result<Vec<Theme>, String> {
    Ok(everything(&folder(&app)?))
}

#[tauri::command]
pub async fn import_theme(app: AppHandle) -> Result<Option<Theme>, String> {
    crate::blocking(move || import_theme_now(app)).await
}

fn import_theme_now(app: AppHandle) -> Result<Option<Theme>, String> {
    let Some(file) = app
        .dialog()
        .file()
        .add_filter("znote add-on or look", &["znx", zsuite_theme::EXTENSION])
        .blocking_pick_file()
    else {
        return Ok(None);
    };
    let file = file.into_path().map_err(|error| error.to_string())?;
    let themes = folder(&app)?;
    let look = file
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(zsuite_theme::EXTENSION));
    let added = match look {
        true => zsuite_theme::install(&looks_of(&themes), &file)?.into(),
        false => install(&themes, &file)?,
    };
    app.state::<Compiler>().forget_themes()?;
    Ok(Some(added))
}

#[tauri::command]
pub async fn store_themes() -> Result<Vec<Theme>, String> {
    crate::blocking(store_themes_now).await
}

fn store_themes_now() -> Result<Vec<Theme>, String> {
    let mut found = catalog(&fetch(STORE, "index.json", MAX_CATALOG_BYTES)?)?;
    let looks = zsuite_theme::catalog(&fetch(LOOKS, "index.json", MAX_CATALOG_BYTES)?)?;
    found.extend(looks.into_iter().map(Theme::from));
    Ok(found)
}

#[tauri::command]
pub async fn install_store_theme(app: AppHandle, id: String, part: Part) -> Result<Theme, String> {
    crate::blocking(move || install_store_theme_now(app, id, part)).await
}

fn install_store_theme_now(app: AppHandle, id: String, part: Part) -> Result<Theme, String> {
    if !valid_id(&id) {
        return Err(format!("“{id}” is not a theme."));
    }
    let themes = folder(&app)?;
    let added = match part {
        Part::Look => {
            let file = format!("themes/{id}.{}", zsuite_theme::EXTENSION);
            let data = fetch(LOOKS, &file, MAX_ARCHIVE_BYTES)?;
            zsuite_theme::install_bytes(&looks_of(&themes), data, &id)?.into()
        }
        Part::Templates => {
            let data = fetch(STORE, &format!("themes/{id}.znx"), MAX_ARCHIVE_BYTES)?;
            unpacked(&themes, Cursor::new(data), Some(&id))?
        }
    };
    app.state::<Compiler>().forget_themes()?;
    Ok(added)
}

#[tauri::command]
pub async fn remove_theme(app: AppHandle, id: String, part: Part) -> Result<(), String> {
    crate::blocking(move || remove_theme_now(app, id, part)).await
}

fn remove_theme_now(app: AppHandle, id: String, part: Part) -> Result<(), String> {
    let themes = folder(&app)?;
    match part {
        Part::Look => zsuite_theme::remove(&looks_of(&themes), &id),
        Part::Templates => {
            remove(&themes, &id)?;
            app.state::<Compiler>().forget_themes()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("znote-znx-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn archive(folder: &Path, entries: &[(&str, &[u8])]) -> PathBuf {
        let file = folder.join("test.znx");
        let mut writer = ZipWriter::new(File::create(&file).unwrap());
        for (name, data) in entries {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap();
        file
    }

    fn manifest(rest: &str) -> Vec<u8> {
        format!(r#"{{ "format": 1, "id": "acme", "name": "Acme", "version": "0.1.0"{rest} }}"#)
            .into_bytes()
    }

    #[test]
    fn an_add_on_installs_lists_beside_the_looks_and_is_removed() {
        let folder = scratch("full");
        let themes = folder.join("themes");
        let json = br#"{
            "format": 1, "id": "acme", "name": "Acme", "version": "0.1.0",
            "typst": { "templates": [{ "label": "Report", "kind": "document", "snippet": "= Report\n" }] }
        }"#;
        let unguided = archive(&folder, &[("theme.json", json)]);
        assert!(
            install(&themes, &unguided)
                .unwrap_err()
                .contains("guide.md")
        );
        let file = archive(
            &folder,
            &[
                ("theme.json", json),
                ("guide.md", b"Use `report`.\n"),
                ("typst/typst.toml", b""),
            ],
        );
        let theme = install(&themes, &file).unwrap();
        assert_eq!(theme.guide, "Use `report`.\n");
        assert_eq!(
            (theme.part, theme.templates[0].kind.clone()),
            (Part::Templates, Kind::Document)
        );
        assert!(themes.join("acme/0.1.0/typst/typst.toml").is_file());
        assert_eq!(list(&themes), [theme]);

        let look = looks_of(&themes).join("acme/0.1.0");
        fs::create_dir_all(&look).unwrap();
        fs::write(
            look.join("theme.json"),
            r##"{ "format": 1, "id": "acme", "name": "Acme", "version": "0.1.0", "light": { "--primary": "#000" } }"##,
        )
        .unwrap();
        let parts: Vec<Part> = everything(&themes).iter().map(|theme| theme.part).collect();
        assert_eq!(parts, [Part::Templates, Part::Look]);

        install(&themes, &file).unwrap();
        assert_eq!(list(&themes).len(), 1);
        remove(&themes, "acme").unwrap();
        assert!(list(&themes).is_empty());
        assert!(remove(&themes, "../acme").is_err());
    }

    #[test]
    fn a_look_or_anything_else_unknown_in_an_add_on_is_refused() {
        assert!(parse(&manifest("")).is_ok());
        let looked = manifest(r##", "ui": { "light": { "--primary": "#000" } }"##);
        assert!(parse(&looked).unwrap_err().contains("unknown field"));
        assert!(parse(&manifest(r#", "typst": { "fonts": [] }"#)).is_err());
        assert!(
            parse(br#"{ "format": 2, "id": "acme", "name": "Acme", "version": "0.1.0" }"#)
                .unwrap_err()
                .contains("newer version")
        );
    }

    #[test]
    fn an_archive_that_writes_outside_its_folder_is_refused() {
        let folder = scratch("slip");
        let themes = folder.join("themes");
        for name in [
            "../escaped.txt",
            "/escaped.txt",
            "typst/../../escaped.txt",
            "typst\\..\\..\\escaped.txt",
        ] {
            let file = archive(&folder, &[("theme.json", &manifest("")), (name, b"gotcha")]);
            let error = install(&themes, &file).unwrap_err();
            assert!(error.contains("outside its own folder"), "{name}: {error}");
            assert!(list(&themes).is_empty());
            assert!(!folder.join("escaped.txt").exists());
            assert!(!themes.join("escaped.txt").exists());
        }
    }

    #[test]
    fn a_theme_that_is_too_large_is_refused() {
        let folder = scratch("large");
        let huge = folder.join("huge.znx");
        File::create(&huge)
            .unwrap()
            .set_len(MAX_ARCHIVE_BYTES + 1)
            .unwrap();
        assert!(
            install(&folder.join("themes"), &huge)
                .unwrap_err()
                .contains("larger than 50 MB")
        );
    }

    #[test]
    fn unsafe_names_are_refused() {
        let named = |id: &str, version: &str| {
            parse(
                format!(
                    r#"{{ "format": 1, "id": "{id}", "name": "Acme", "version": "{version}" }}"#
                )
                .as_bytes(),
            )
        };
        assert!(named("acme", "0.1.0").is_ok());
        assert!(named("../acme", "0.1.0").is_err());
        assert!(named("Acme", "0.1.0").is_err());
        assert!(named("acme", "../1").is_err());
        let label = r#", "typst": { "templates": [{ "label": "<b>", "kind": "document", "snippet": "" }] }"#;
        assert!(parse(&manifest(label)).is_err());
    }

    #[test]
    fn the_store_list_keeps_valid_themes_and_a_download_must_be_the_theme_asked_for() {
        let json = br##"[
            { "format": 1, "id": "paper", "name": "Paper", "version": "1.0.0", "description": "Warm and calm.",
              "category": "Business", "tags": ["sepia"], "typst": { "templates": [] } },
            { "format": 1, "id": "evil", "name": "Evil", "version": "1.0.0", "ui": { "light": { "--background": "#000" } } },
            { "format": 1, "id": "loud", "name": "Loud", "version": "1.0.0", "category": "<script>" },
            "nonsense"
        ]"##;
        let themes = catalog(json).unwrap();
        assert_eq!(themes.len(), 1);
        assert_eq!(
            (
                themes[0].id.as_str(),
                themes[0].category.as_str(),
                themes[0].part
            ),
            ("paper", "Business", Part::Templates)
        );
        assert!(catalog(b"{}").is_err());

        let folder = scratch("store");
        let themes = folder.join("themes");
        let data = fs::read(archive(
            &folder,
            &[("theme.json", &manifest("")), ("guide.md", b"Guide")],
        ))
        .unwrap();
        assert!(
            unpacked(&themes, Cursor::new(data.clone()), Some("paper"))
                .unwrap_err()
                .contains("different add-on")
        );
        assert!(list(&themes).is_empty());
        assert_eq!(
            unpacked(&themes, Cursor::new(data), Some("acme"))
                .unwrap()
                .id,
            "acme"
        );
    }

    #[test]
    fn the_outline_of_a_package_lists_its_lets_and_stays_inside_the_add_on() {
        let source = r##"#import "util.typ": helper
#let brand = rgb("#1e3a8a")
#let today = datetime.today().display("[day padding:none] = [year]")

// A boxed note.
#let callout(title: "Note", body) = block(inset: 10pt)[#title #body]

#let letter(
  to: "Recipient (name)",
  body,
) = {
  let inner = 1
  body
}
#let broken(
"##;
        let mut found = String::new();
        outline(source, None, &mut found);
        assert_eq!(
            found,
            "#let brand = rgb(\"#1e3a8a\")\n#let today = datetime.today().display(\"[day padding:none] = [year]\")\n\
             // A boxed note.\n#let callout(title: \"Note\", body)\n#let letter( to: \"Recipient (name)\", body, )\n"
        );

        let folder = scratch("outline");
        fs::create_dir_all(folder.join("typst")).unwrap();
        fs::write(folder.join("secret.typ"), "#let secret = 1\n").unwrap();
        fs::write(
            folder.join("typst/main.typ"),
            "#import \"parts/util.typ\": helper, value as renamed\n#import \"../secret.typ\": *\n#let shown = 1\n",
        )
        .unwrap();
        fs::create_dir_all(folder.join("typst/parts")).unwrap();
        fs::write(
            folder.join("typst/parts/util.typ"),
            "#import \"colours.typ\": *\n#let helper(x) = x\n#let value = 2\n#let other = 3\n#let _private = 4\n",
        )
        .unwrap();
        fs::write(
            folder.join("typst/parts/colours.typ"),
            "#let helper(x) = x\n#let navy = 1\n",
        )
        .unwrap();
        fs::write(
            folder.join("typst/typst.toml"),
            "entrypoint = \"main.typ\"\n",
        )
        .unwrap();
        assert_eq!(
            outline_of(&folder),
            "#let helper(x)\n#let value = 2\n#let shown = 1\n"
        );
        fs::write(
            folder.join("typst/typst.toml"),
            "entrypoint = \"../secret.typ\"\n",
        )
        .unwrap();
        assert_eq!(outline_of(&folder), "");
    }
}

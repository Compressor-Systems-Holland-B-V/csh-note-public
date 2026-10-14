use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Display;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use typst::syntax::package::PackageVersion;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::theme;
use crate::workspace::check_name;

const MAX_UNPACKED_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_ENTRIES: usize = 50_000;
const ALREADY_SMALL: [&str; 10] = [
    "png", "jpg", "jpeg", "gif", "webp", "pdf", "zip", "mp4", "mp3", "woff2",
];
const RESERVED: [&str; 5] = ["znote.json", "fonts.json", "pages", "themes", "fonts"];

pub type Packed = BTreeMap<String, (u64, u64)>;

type Sources = Vec<(String, PathBuf)>;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub format: u32,
    pub look: Option<String>,
    pub themes: Vec<String>,
    #[serde(default)]
    pub settings: Value,
    #[serde(flatten)]
    pub other: serde_json::Map<String, Value>,
}

fn unsaved(error: impl Display) -> String {
    format!("Your notes could not be saved to a file: {error}")
}

fn unreadable(error: impl Display) -> String {
    format!("This znote file could not be read: {error}")
}

fn imports(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split("@themes/").skip(1).filter_map(|rest| {
        let spec = rest
            .split(|c: char| !(c.is_ascii_alphanumeric() || "-.:".contains(c)))
            .next()?;
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
        } else if kind.is_file()
            && path
                .extension()
                .is_none_or(|extension| extension != "znote" && extension != "tmp")
        {
            found.push(path);
        }
    }
    Ok(())
}

fn inside(folder: &Path, prefix: &str) -> Result<Sources, String> {
    let mut found = Vec::new();
    if folder.is_dir() {
        files(folder, &mut found).map_err(unsaved)?;
    }
    found.sort();
    found
        .into_iter()
        .map(|file| {
            let names: Option<Vec<&str>> = file
                .strip_prefix(folder)
                .map_err(unsaved)?
                .iter()
                .map(|name| name.to_str())
                .collect();
            let names = names.ok_or_else(|| {
                unsaved(format!(
                    "“{}” has a name that cannot be stored",
                    file.display()
                ))
            })?;
            Ok((format!("{prefix}{}", names.join("/")), file.clone()))
        })
        .collect()
}

pub fn stamp(file: &Path) -> (u64, u64) {
    let modified = |data: &fs::Metadata| {
        Some(
            data.modified()
                .ok()?
                .duration_since(UNIX_EPOCH)
                .ok()?
                .as_millis() as u64,
        )
    };
    fs::metadata(file)
        .map(|data| (data.len(), modified(&data).unwrap_or_default()))
        .unwrap_or_default()
}

fn versions(folder: &Path) -> Vec<String> {
    let mut found: Vec<String> = fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| {
            name.parse::<PackageVersion>().is_ok() && folder.join(name).join("theme.json").is_file()
        })
        .collect();
    found.sort();
    found
}

fn needed(pages: &[(String, PathBuf)]) -> Vec<String> {
    let mut themes = BTreeSet::new();
    for (_, file) in pages
        .iter()
        .filter(|(_, file)| file.extension().is_some_and(|extension| extension == "typ"))
    {
        themes.extend(imports(&String::from_utf8_lossy(
            &fs::read(file).unwrap_or_default(),
        )));
    }
    themes.into_iter().collect()
}

fn already_small(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, extension)| {
        ALREADY_SMALL.contains(&extension.to_ascii_lowercase().as_str())
    })
}

fn options(name: &str, size: u64) -> SimpleFileOptions {
    let method = if already_small(name) {
        CompressionMethod::Stored
    } else {
        CompressionMethod::Deflated
    };
    SimpleFileOptions::default()
        .compression_method(method)
        .large_file(size >= u32::MAX as u64)
}

fn checked(target: &Path, expected: usize, fresh: &[String]) -> Result<(), String> {
    let broken = |error: &dyn Display| {
        unsaved(format!(
            "the new file did not read back correctly ({error})"
        ))
    };
    let mut archive = ZipArchive::new(File::open(target).map_err(|error| broken(&error))?)
        .map_err(|error| broken(&error))?;
    if archive.len() != expected {
        return Err(broken(&"an entry is missing"));
    }
    for name in fresh.iter().map(String::as_str).chain(["znote.json"]) {
        let mut entry = archive.by_name(name).map_err(|error| broken(&error))?;
        io::copy(&mut entry, &mut io::sink()).map_err(|error| broken(&error))?;
    }
    Ok(())
}

pub fn absent(themes: &[String]) -> String {
    format!(
        "This file cannot be opened because it needs themes that are not installed:\n• {}\nInstall them in the Marketplace, then open the file again.",
        themes.join("\n• ")
    )
}

fn folders(folder: &Path) -> io::Result<Vec<(String, PathBuf)>> {
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

fn contents(file: &Path) -> Result<(ZipArchive<File>, Manifest), String> {
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
    Ok((archive, manifest))
}

fn moved(from: PathBuf, to: PathBuf) -> Result<(), String> {
    if from.exists() {
        fs::rename(from, to).map_err(unreadable)?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct Home(pub PathBuf);

impl Home {
    pub fn pages(&self) -> PathBuf {
        self.0.join("files")
    }

    pub fn themes(&self) -> PathBuf {
        self.0.join("themes")
    }

    pub fn fonts(&self) -> PathBuf {
        self.0.join("fonts")
    }

    pub fn font_use(&self) -> PathBuf {
        self.0.join("fonts.json")
    }

    fn extra(&self) -> PathBuf {
        self.0.join("extra")
    }

    fn manifest(&self) -> PathBuf {
        self.0.join("znote.json")
    }

    fn packed(&self) -> PathBuf {
        self.0.join("packed.json")
    }

    fn theme_folder(&self, installed: Option<&Path>, spec: &str) -> Option<(String, PathBuf)> {
        let (id, wanted) = spec
            .split_once(':')
            .map_or((spec, None), |(id, version)| (id, Some(version)));
        if !theme::valid_id(id) {
            return None;
        }
        [Some(self.themes()), installed.map(Path::to_path_buf)]
            .into_iter()
            .flatten()
            .find_map(|themes| {
                let folder = themes.join(id);
                let version = match wanted {
                    Some(version) => folder
                        .join(version)
                        .join("theme.json")
                        .is_file()
                        .then(|| version.to_string()),
                    None => versions(&folder).pop(),
                }?;
                Some((format!("themes/{id}/{version}/"), folder.join(version)))
            })
    }

    fn entries(
        &self,
        installed: Option<&Path>,
        look: &Option<String>,
    ) -> Result<(Sources, Vec<String>), String> {
        let pages: Vec<(String, PathBuf)> = inside(&self.pages(), "pages/")?
            .into_iter()
            .filter(|(name, _)| name.matches('/').count() >= 2)
            .collect();
        let themes = needed(&pages);
        let mut found = pages;
        let mut carried = BTreeSet::new();
        for spec in themes.iter().chain(look) {
            if let Some((prefix, folder)) = self.theme_folder(installed, spec)
                && carried.insert(prefix.clone())
            {
                found.extend(inside(&folder, &prefix)?);
            }
        }
        found.extend(inside(&self.fonts(), "fonts/")?);
        if self.font_use().is_file() {
            found.push(("fonts.json".into(), self.font_use()));
        }
        let extra = inside(&self.extra(), "")?;
        found.extend(
            extra.into_iter().filter(|(name, _)| {
                !RESERVED.contains(&name.split('/').next().unwrap_or_default())
            }),
        );
        Ok((found, themes))
    }

    pub fn pack(
        &self,
        installed: &Path,
        target: &Path,
        previous: Option<&Path>,
        look: Option<String>,
        settings: Value,
    ) -> Result<Packed, String> {
        let before: Packed = fs::read(self.packed())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let mut old = previous.and_then(|file| ZipArchive::new(File::open(file).ok()?).ok());
        let (found, themes) = self.entries(Some(installed), &look)?;
        let mut writer = ZipWriter::new(File::create(target).map_err(unsaved)?);
        let mut packed = Packed::new();
        let mut fresh = Vec::new();
        for (name, source) in &found {
            let stamp = stamp(source);
            let same = before.get(name) == Some(&stamp);
            let kept = old
                .as_mut()
                .filter(|_| same)
                .and_then(|old| Some((old.index_for_name(name)?, old)));
            match kept.and_then(|(index, old)| old.by_index_raw(index).ok()) {
                Some(entry) => writer.raw_copy_file(entry).map_err(unsaved)?,
                None => {
                    writer
                        .start_file(name.as_str(), options(name, stamp.0))
                        .map_err(unsaved)?;
                    io::copy(&mut File::open(source).map_err(unsaved)?, &mut writer)
                        .map_err(unsaved)?;
                    fresh.push(name.clone());
                }
            }
            packed.insert(name.clone(), stamp);
        }
        let other = self
            .read_manifest()
            .map(|manifest| manifest.other)
            .unwrap_or_default();
        let manifest = Manifest {
            format: 1,
            look,
            themes,
            settings,
            other,
        };
        let json = serde_json::to_vec_pretty(&manifest).map_err(unsaved)?;
        writer
            .start_file("znote.json", SimpleFileOptions::default())
            .map_err(unsaved)?;
        io::Write::write_all(&mut writer, &json).map_err(unsaved)?;
        writer
            .finish()
            .and_then(|file| Ok(file.sync_all()?))
            .map_err(unsaved)?;
        checked(target, found.len() + 1, &fresh)?;
        fs::write(self.manifest(), json).map_err(unsaved)?;
        Ok(packed)
    }

    pub fn remember(&self, packed: &Packed) {
        if let Ok(bytes) = serde_json::to_vec(packed) {
            let _ = fs::write(self.packed(), bytes);
        }
    }

    pub fn read_manifest(&self) -> Option<Manifest> {
        serde_json::from_slice(&fs::read(self.manifest()).ok()?).ok()
    }

    pub fn missing(
        &self,
        manifest: &Manifest,
        installed: &[(String, String)],
    ) -> (Vec<PathBuf>, Vec<String>) {
        let mut offered = Vec::new();
        let mut absent = Vec::new();
        let has = |id: &str, version: Option<&str>| {
            installed
                .iter()
                .any(|(have, at)| have == id && version.is_none_or(|version| version == at))
        };
        let looked = manifest.look.iter().filter(|look| {
            !manifest
                .themes
                .iter()
                .any(|spec| spec.split(':').next() == Some(look))
        });
        for spec in manifest.themes.iter().chain(looked) {
            let (id, version) = spec
                .split_once(':')
                .map_or((spec.as_str(), None), |(id, version)| (id, Some(version)));
            if has(id, version) {
                continue;
            }
            match self.theme_folder(None, spec) {
                Some((_, folder)) => offered.push(folder),
                None => absent.push(spec.replace(':', " ")),
            }
        }
        (offered, absent)
    }

    pub fn unpack(&self, file: &Path) -> Result<Manifest, String> {
        let (mut archive, manifest) = contents(file)?;
        let _ = fs::remove_dir_all(&self.0);
        let staging = self.0.join(".incoming");
        theme::unpack(&mut archive, &staging, MAX_UNPACKED_BYTES, "znote file")?;
        fs::create_dir_all(self.pages())
            .and_then(|()| fs::create_dir_all(staging.join("pages")))
            .map_err(unreadable)?;
        for (name, project) in folders(&staging.join("pages")).map_err(unreadable)? {
            fs::rename(project, self.pages().join(name)).map_err(unreadable)?;
        }
        let _ = fs::remove_dir_all(staging.join("pages"));
        let _ = fs::remove_file(staging.join("znote.json"));
        moved(staging.join("themes"), self.themes())?;
        moved(staging.join("fonts"), self.fonts())?;
        moved(staging.join("fonts.json"), self.font_use())?;
        moved(staging, self.extra())?;
        fs::write(
            self.manifest(),
            serde_json::to_vec_pretty(&manifest).map_err(unreadable)?,
        )
        .map_err(unreadable)?;
        let (found, _) = self.entries(None, &manifest.look)?;
        self.remember(
            &found
                .iter()
                .map(|(name, source)| (name.clone(), stamp(source)))
                .collect(),
        );
        Ok(manifest)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("znote-snapshot-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn put(file: PathBuf, text: &str) {
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }

    fn install(themes: &Path, id: &str, version: &str) {
        let json =
            format!(r#"{{ "format": 1, "id": "{id}", "name": "Theme", "version": "{version}" }}"#);
        put(themes.join(id).join(version).join("theme.json"), &json);
        put(
            themes.join(id).join(version).join("typst/lib.typ"),
            "#let brand = [Acme]",
        );
    }

    fn archive(file: &Path, entries: &[(&str, &str)]) {
        let mut writer = ZipWriter::new(File::create(file).unwrap());
        for (name, text) in entries {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(text.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
    }

    fn names(file: &Path) -> Vec<String> {
        let archive = ZipArchive::new(File::open(file).unwrap()).unwrap();
        let mut names: Vec<String> = archive
            .file_names()
            .map(|name| name.unwrap().into_owned())
            .collect();
        names.sort();
        names
    }

    fn text(file: &Path, name: &str) -> String {
        let mut archive = ZipArchive::new(File::open(file).unwrap()).unwrap();
        let mut text = String::new();
        archive
            .by_name(name)
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        text
    }

    #[test]
    fn a_packed_file_carries_pages_themes_fonts_settings_and_what_a_newer_znote_added() {
        let folder = scratch("round");
        let (home, themes, file) = (
            Home(folder.join("home")),
            folder.join("themes"),
            folder.join("notes.znote"),
        );
        let pages = home.pages();
        put(
            pages.join("Work/Notes/Monday.typ"),
            "// @themes/nothing here\n#import \"@themes/acme:0.1.0\": *\n= Hi",
        );
        put(
            pages.join("Work/Notes/Plain.typ"),
            "#import \"@preview/cetz:0.3.0\": *",
        );
        put(pages.join("Work/images/logo.svg"), "<svg/>");
        put(pages.join("Work/earlier.znote"), "old");
        put(pages.join("Work/Notes/Monday.typ.tmp"), "half written");
        put(pages.join("loose.txt"), "loose");
        put(pages.join(".git/config"), "hidden");
        put(home.0.join("fonts/Inter.ttf"), "font bytes");
        put(
            home.0.join("fonts.json"),
            r#"{"Work/Notes/Monday":[{"family":"Inter","file":"Inter.ttf"}]}"#,
        );
        put(home.0.join("extra/history/1.json"), "from a newer znote");
        put(
            home.0.join("extra/pages/Sneaky/Notes/A.typ"),
            "must not shadow a page",
        );
        put(
            home.0.join("znote.json"),
            r#"{ "format": 1, "look": null, "themes": [], "revision": 7 }"#,
        );
        install(&themes, "acme", "0.1.0");
        install(&themes, "paper", "2.0.0");
        install(&themes, "unused", "1.0.0");

        let settings = serde_json::json!({ "theme": "dark" });
        home.pack(&themes, &file, None, Some("paper".into()), settings.clone())
            .unwrap();
        let expected = [
            "fonts.json",
            "fonts/Inter.ttf",
            "history/1.json",
            "pages/Work/Notes/Monday.typ",
            "pages/Work/Notes/Plain.typ",
            "pages/Work/images/logo.svg",
            "themes/acme/0.1.0/theme.json",
            "themes/acme/0.1.0/typst/lib.typ",
            "themes/paper/2.0.0/theme.json",
            "themes/paper/2.0.0/typst/lib.typ",
            "znote.json",
        ];
        assert_eq!(names(&file), expected);

        let there = Home(folder.join("another computer"));
        let manifest = there.unpack(&file).unwrap();
        assert_eq!(manifest.look.as_deref(), Some("paper"));
        assert_eq!(manifest.themes, ["acme:0.1.0"]);
        assert_eq!(manifest.settings, settings);
        assert_eq!(manifest.other["revision"], 7);
        assert!(
            fs::read_to_string(there.pages().join("Work/Notes/Monday.typ"))
                .unwrap()
                .ends_with("= Hi")
        );
        assert_eq!(
            fs::read_to_string(there.fonts().join("Inter.ttf")).unwrap(),
            "font bytes"
        );
        assert!(there.font_use().is_file());
        assert_eq!(
            fs::read_to_string(there.0.join("extra/history/1.json")).unwrap(),
            "from a newer znote"
        );
        assert!(there.themes().join("paper/2.0.0/typst/lib.typ").is_file());

        let (offered, absent) = there.missing(&manifest, &[]);
        assert_eq!(
            offered,
            [
                there.themes().join("acme/0.1.0"),
                there.themes().join("paper/2.0.0")
            ]
        );
        assert!(absent.is_empty());
        let installed = [
            ("acme".to_string(), "0.2.0".to_string()),
            ("paper".to_string(), "1.0.0".to_string()),
        ];
        assert_eq!(
            there.missing(&manifest, &installed).0,
            [there.themes().join("acme/0.1.0")]
        );
        fs::remove_dir_all(there.themes()).unwrap();
        assert_eq!(there.missing(&manifest, &[]).1, ["acme 0.1.0", "paper"]);
        assert!(
            super::absent(&["acme 0.1.0".into()]).contains("not installed:\n• acme 0.1.0\nInstall")
        );

        let again = folder.join("sent on.znote");
        there
            .pack(
                &folder.join("no themes here"),
                &again,
                None,
                manifest.look.clone(),
                manifest.settings.clone(),
            )
            .unwrap();
        assert_eq!(
            names(&again),
            expected
                .iter()
                .filter(|name| !name.starts_with("themes/"))
                .copied()
                .collect::<Vec<_>>()
        );
        assert_eq!(
            text(&again, "znote.json")
                .matches("\"revision\": 7")
                .count(),
            1
        );
    }

    #[test]
    fn saving_again_copies_what_did_not_change_and_rewrites_only_what_did() {
        let folder = scratch("again");
        let (home, themes, file, next) = (
            Home(folder.join("home")),
            folder.join("themes"),
            folder.join("a.znote"),
            folder.join("b.znote"),
        );
        let pages = home.pages();
        let (same, changed) = (
            pages.join("Work/Notes/Same.typ"),
            pages.join("Work/Notes/Changed.typ"),
        );
        put(same.clone(), "first");
        put(changed.clone(), "first");
        home.remember(&home.pack(&themes, &file, None, None, Value::Null).unwrap());

        let written = fs::metadata(&same).unwrap().modified().unwrap();
        fs::write(&same, "FIRST").unwrap();
        File::options()
            .write(true)
            .open(&same)
            .unwrap()
            .set_modified(written)
            .unwrap();
        fs::write(&changed, "second, and longer").unwrap();
        put(pages.join("Work/Notes/New.typ"), "new");
        home.pack(&themes, &next, Some(&file), None, Value::Null)
            .unwrap();
        assert_eq!(text(&next, "pages/Work/Notes/Same.typ"), "first");
        assert_eq!(
            text(&next, "pages/Work/Notes/Changed.typ"),
            "second, and longer"
        );
        assert_eq!(text(&next, "pages/Work/Notes/New.typ"), "new");

        home.pack(&themes, &next, None, None, Value::Null).unwrap();
        assert_eq!(text(&next, "pages/Work/Notes/Same.typ"), "FIRST");
        assert!(
            checked(&next, 99, &[])
                .unwrap_err()
                .contains("did not read back")
        );
        assert!(
            already_small("pages/Work/photo.PNG")
                && !already_small("pages/Work/Notes/page.typ")
                && !already_small("png")
        );
    }

    #[test]
    fn a_file_that_is_not_ours_or_reaches_outside_its_folder_changes_nothing() {
        let folder = scratch("refused");
        let (file, home) = (folder.join("bad.znote"), Home(folder.join("inner/home")));
        let manifest = r#"{ "format": 1, "look": null, "themes": [] }"#;

        for name in [
            "../escaped.txt",
            "pages/../../../escaped.txt",
            "/escaped.txt",
            "pages\\..\\..\\escaped.txt",
        ] {
            archive(
                &file,
                &[
                    ("znote.json", manifest),
                    ("pages/Work/Notes/A.typ", ""),
                    (name, "gotcha"),
                ],
            );
            let error = home.unpack(&file).unwrap_err();
            assert!(error.contains("outside its own folder"), "{name}: {error}");
            assert!(
                !folder.join("escaped.txt").exists() && !folder.join("inner/escaped.txt").exists()
            );
        }

        archive(&file, &[("pages/Work/Notes/A.typ", "")]);
        assert!(home.unpack(&file).unwrap_err().contains("no znote.json"));
        archive(
            &file,
            &[(
                "znote.json",
                r#"{ "format": 2, "look": null, "themes": [] }"#,
            )],
        );
        assert!(home.unpack(&file).unwrap_err().contains("newer version"));
        fs::write(&file, "plain text").unwrap();
        assert!(home.unpack(&file).unwrap_err().contains("not a znote file"));

        archive(
            &file,
            &[
                ("znote.json", manifest),
                ("pages/CON/Notes/A.typ", ""),
                ("pages/.hidden/x", ""),
                ("themes/../x/1.0.0/theme.json", "{}"),
            ],
        );
        assert!(home.unpack(&file).is_err());
        archive(
            &file,
            &[
                ("znote.json", manifest),
                ("pages/CON/Notes/A.typ", ""),
                ("pages/.hidden/x", ""),
                ("themes/Bad Id/1.0.0/theme.json", "{}"),
            ],
        );
        let read = home.unpack(&file).unwrap();
        assert_eq!(fs::read_dir(home.pages()).unwrap().count(), 0);
        let wants = Manifest {
            themes: vec!["Bad Id:1.0.0".into()],
            ..read
        };
        assert_eq!(
            home.missing(&wants, &[]),
            (Vec::new(), vec!["Bad Id 1.0.0".to_string()])
        );
    }
}

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Display;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use typst::syntax::package::PackageVersion;

use crate::sealed;
use crate::theme;
use crate::workspace::check_name;

const MAX_UNPACKED_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_ENTRIES: usize = 50_000;
const MANIFEST: &str = "znote.json";
const SCHEMA: &str =
    "CREATE TABLE IF NOT EXISTS entries (name TEXT PRIMARY KEY, data BLOB NOT NULL);";
const WRONG: &str = "The password is wrong, or this is not a znote file.";
const RESERVED: [&str; 7] = [
    "znote.json",
    "fonts.json",
    "pages",
    "themes",
    "looks",
    "fonts",
    "chats",
];

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
    found.sort_by_key(|name| name.parse::<PackageVersion>().ok());
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

fn checked(target: &Path, password: &str, expected: usize, fresh: &[String]) -> Result<(), String> {
    let broken = |error: &dyn Display| {
        unsaved(format!(
            "the new file did not read back correctly ({error})"
        ))
    };
    let db = sealed::open(target, password, OpenFlags::SQLITE_OPEN_READ_ONLY, WRONG)
        .map_err(|error| broken(&error))?;
    let count: i64 = db
        .query_row("SELECT count(*) FROM entries", [], |row| row.get(0))
        .map_err(|error| broken(&error))?;
    if count as usize != expected {
        return Err(broken(&"an entry is missing"));
    }
    let mut statement = db
        .prepare("SELECT data FROM entries WHERE name = ?1")
        .map_err(|error| broken(&error))?;
    for name in fresh.iter().map(String::as_str).chain([MANIFEST]) {
        statement
            .query_row([name], |row| row.get_ref(0).map(drop))
            .map_err(|error| broken(&error))?;
    }
    Ok(())
}

fn entry_path(name: &str) -> Option<Vec<&str>> {
    let names: Vec<&str> = name.split('/').collect();
    names
        .iter()
        .all(|name| {
            !matches!(*name, "" | "." | "..")
                && !name
                    .chars()
                    .any(|c| c.is_control() || c == '\\' || c == ':')
        })
        .then_some(names)
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

fn contents(file: &Path, password: &str) -> Result<(Connection, Manifest), String> {
    let db = sealed::open(file, password, OpenFlags::SQLITE_OPEN_READ_ONLY, WRONG)?;
    let not_ours = |_| "This file is not a znote file.".to_string();
    let (count, size): (i64, i64) = db
        .query_row(
            "SELECT count(*), coalesce(sum(length(data)), 0) FROM entries",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(not_ours)?;
    if count as usize > MAX_ENTRIES {
        return Err("This znote file has too many files in it, so it was not opened.".into());
    }
    if size as u64 > MAX_UNPACKED_BYTES {
        return Err("This znote file is too large once unpacked, so it was not imported.".into());
    }
    let json: Vec<u8> = db
        .query_row(
            "SELECT data FROM entries WHERE name = ?1 AND length(data) <= ?2",
            params![MANIFEST, MAX_MANIFEST_BYTES as i64],
            |row| row.get(0),
        )
        .optional()
        .map_err(not_ours)?
        .ok_or("This file is not a znote file: it has no znote.json.")?;
    let manifest: Manifest = serde_json::from_slice(&json).map_err(unreadable)?;
    if manifest.format != 2 {
        return Err("This file was made by a newer version of znote.".into());
    }
    Ok((db, manifest))
}

pub fn unlocks(file: &Path, password: &str) -> Result<(), String> {
    contents(file, password).map(drop)
}

fn written(staging: &Path, db: &Connection) -> Result<(), String> {
    let mut statement = db
        .prepare("SELECT name, data FROM entries WHERE name <> ?1")
        .map_err(unreadable)?;
    let mut rows = statement.query([MANIFEST]).map_err(unreadable)?;
    while let Some(row) = rows.next().map_err(unreadable)? {
        let name: String = row.get(0).map_err(unreadable)?;
        let names = entry_path(&name).ok_or_else(|| {
            format!("This znote file tries to put “{name}” outside its own folder, so it was not imported.")
        })?;
        if names.iter().any(|name| check_name(name).is_err()) {
            continue;
        }
        let target = staging.join(PathBuf::from_iter(&names));
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(unreadable)?;
        }
        let data = row
            .get_ref(1)
            .map_err(unreadable)?
            .as_blob()
            .map_err(unreadable)?;
        fs::write(target, data).map_err(unreadable)?;
    }
    Ok(())
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

    pub fn looks(&self) -> PathBuf {
        self.0.join("looks")
    }

    pub fn fonts(&self) -> PathBuf {
        self.0.join("fonts")
    }

    pub fn font_use(&self) -> PathBuf {
        self.0.join("fonts.json")
    }

    pub fn chats(&self) -> PathBuf {
        self.0.join("chats")
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
        let bases = [Some(self.themes()), installed.map(Path::to_path_buf)];
        Self::found(bases, "themes", spec)
    }

    fn look_folder(&self, installed: Option<&Path>, id: &str) -> Option<(String, PathBuf)> {
        let bases = [Some(self.looks()), installed.map(theme::looks_of)];
        Self::found(bases, "looks", id)
    }

    fn found(bases: [Option<PathBuf>; 2], kind: &str, spec: &str) -> Option<(String, PathBuf)> {
        let (id, wanted) = spec
            .split_once(':')
            .map_or((spec, None), |(id, version)| (id, Some(version)));
        if !theme::valid_id(id) {
            return None;
        }
        bases.into_iter().flatten().find_map(|themes| {
            let folder = themes.join(id);
            let version = match wanted {
                Some(version) => folder
                    .join(version)
                    .join("theme.json")
                    .is_file()
                    .then(|| version.to_string()),
                None => versions(&folder).pop(),
            }?;
            Some((format!("{kind}/{id}/{version}/"), folder.join(version)))
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
        let folders = themes
            .iter()
            .map(|spec| self.theme_folder(installed, spec))
            .chain(look.iter().map(|id| self.look_folder(installed, id)));
        for (prefix, folder) in folders.flatten() {
            if carried.insert(prefix.clone()) {
                found.extend(inside(&folder, &prefix)?);
            }
        }
        found.extend(inside(&self.fonts(), "fonts/")?);
        found.extend(inside(&self.chats(), "chats/")?);
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
        note: (Option<String>, Value),
        password: &str,
    ) -> Result<Packed, String> {
        let (look, settings) = note;
        let before: Packed = fs::read(self.packed())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let (found, themes) = self.entries(Some(installed), &look)?;
        let _ = fs::remove_file(target);
        if let Some(previous) = previous {
            fs::copy(previous, target).map_err(unsaved)?;
        }
        let mut db = sealed::open(target, password, OpenFlags::default(), WRONG)?;
        db.execute_batch(SCHEMA).map_err(unsaved)?;
        let transaction = db.transaction().map_err(unsaved)?;
        let mut stored = BTreeSet::new();
        {
            let mut statement = transaction
                .prepare("SELECT name FROM entries")
                .map_err(unsaved)?;
            let names = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(unsaved)?;
            for name in names {
                stored.insert(name.map_err(unsaved)?);
            }
        }
        let mut packed = Packed::new();
        let mut fresh = Vec::new();
        {
            let mut put = transaction
                .prepare("INSERT OR REPLACE INTO entries (name, data) VALUES (?1, ?2)")
                .map_err(unsaved)?;
            for (name, source) in &found {
                let stamp = stamp(source);
                if before.get(name) != Some(&stamp) || !stored.contains(name) {
                    put.execute(params![name, fs::read(source).map_err(unsaved)?])
                        .map_err(unsaved)?;
                    fresh.push(name.clone());
                }
                packed.insert(name.clone(), stamp);
            }
            let mut gone = transaction
                .prepare("DELETE FROM entries WHERE name = ?1")
                .map_err(unsaved)?;
            for name in stored
                .iter()
                .filter(|name| *name != MANIFEST && !packed.contains_key(*name))
            {
                gone.execute([name]).map_err(unsaved)?;
            }
            let other = self
                .read_manifest()
                .map(|manifest| manifest.other)
                .unwrap_or_default();
            let manifest = Manifest {
                format: 2,
                look,
                themes,
                settings,
                other,
            };
            let json = serde_json::to_vec_pretty(&manifest).map_err(unsaved)?;
            put.execute(params![MANIFEST, json]).map_err(unsaved)?;
            fs::write(self.manifest(), json).map_err(unsaved)?;
        }
        transaction.commit().map_err(unsaved)?;
        drop(db);
        checked(target, password, found.len() + 1, &fresh)?;
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

    /// The themes and look the file needs that are not installed: those it carries, to offer,
    /// and those it does not.
    pub fn missing(
        &self,
        manifest: &Manifest,
        installed: &[(String, String)],
        looks: &[(String, String)],
    ) -> (Vec<PathBuf>, Vec<String>) {
        let mut offered = Vec::new();
        let mut absent = Vec::new();
        let has = |installed: &[(String, String)], id: &str, version: Option<&str>| {
            installed
                .iter()
                .any(|(have, at)| have == id && version.is_none_or(|version| version == at))
        };
        let themes = manifest.themes.iter().map(|spec| (spec, installed, false));
        let look = manifest.look.iter().map(|id| (id, looks, true));
        for (spec, installed, look) in themes.chain(look) {
            let (id, version) = spec
                .split_once(':')
                .map_or((spec.as_str(), None), |(id, version)| (id, Some(version)));
            if has(installed, id, version) {
                continue;
            }
            let folder = match look {
                true => self.look_folder(None, spec),
                false => self.theme_folder(None, spec),
            };
            match folder {
                Some((_, folder)) if !offered.contains(&folder) => offered.push(folder),
                Some(_) => {}
                None => absent.push(spec.replace(':', " ")),
            }
        }
        (offered, absent)
    }

    pub fn unpack(&self, file: &Path, password: &str) -> Result<Manifest, String> {
        let (db, manifest) = contents(file, password)?;
        let _ = fs::remove_dir_all(&self.0);
        let staging = self.0.join(".incoming");
        written(&staging, &db)?;
        drop(db);
        fs::create_dir_all(self.pages())
            .and_then(|()| fs::create_dir_all(staging.join("pages")))
            .map_err(unreadable)?;
        for (name, project) in folders(&staging.join("pages")).map_err(unreadable)? {
            fs::rename(project, self.pages().join(name)).map_err(unreadable)?;
        }
        let _ = fs::remove_dir_all(staging.join("pages"));
        let _ = fs::remove_file(staging.join("znote.json"));
        moved(staging.join("themes"), self.themes())?;
        moved(staging.join("looks"), self.looks())?;
        moved(staging.join("fonts"), self.fonts())?;
        moved(staging.join("fonts.json"), self.font_use())?;
        moved(staging.join("chats"), self.chats())?;
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
    use super::*;

    const PASSWORD: &str = "a fine password";

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

    fn sealed_db(file: &Path) -> Connection {
        sealed::open(file, PASSWORD, OpenFlags::default(), WRONG).unwrap()
    }

    fn archive(file: &Path, entries: &[(&str, &str)]) {
        let _ = fs::remove_file(file);
        let db = sealed_db(file);
        db.execute_batch(SCHEMA).unwrap();
        for (name, text) in entries {
            db.execute(
                "INSERT INTO entries (name, data) VALUES (?1, ?2)",
                params![name, text.as_bytes()],
            )
            .unwrap();
        }
    }

    fn names(file: &Path) -> Vec<String> {
        let db = sealed_db(file);
        let mut statement = db
            .prepare("SELECT name FROM entries ORDER BY name")
            .unwrap();
        statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn text(file: &Path, name: &str) -> String {
        let data: Vec<u8> = sealed_db(file)
            .query_row("SELECT data FROM entries WHERE name = ?1", [name], |row| {
                row.get(0)
            })
            .unwrap();
        String::from_utf8(data).unwrap()
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
        put(home.0.join("chats/chat-1/chat.json"), "{}");
        put(
            home.0.join("extra/pages/Sneaky/Notes/A.typ"),
            "must not shadow a page",
        );
        put(
            home.0.join("znote.json"),
            r#"{ "format": 2, "look": null, "themes": [], "revision": 7 }"#,
        );
        install(&themes, "acme", "0.1.0");
        put(
            theme::looks_of(&themes).join("paper/2.0.0/theme.json"),
            r#"{ "format": 1, "id": "paper", "name": "Paper", "version": "2.0.0" }"#,
        );
        install(&themes, "unused", "1.0.0");

        let settings = serde_json::json!({ "theme": "dark" });
        home.pack(
            &themes,
            &file,
            None,
            (Some("paper".into()), settings.clone()),
            PASSWORD,
        )
        .unwrap();
        let expected = [
            "chats/chat-1/chat.json",
            "fonts.json",
            "fonts/Inter.ttf",
            "history/1.json",
            "looks/paper/2.0.0/theme.json",
            "pages/Work/Notes/Monday.typ",
            "pages/Work/Notes/Plain.typ",
            "pages/Work/images/logo.svg",
            "themes/acme/0.1.0/theme.json",
            "themes/acme/0.1.0/typst/lib.typ",
            "znote.json",
        ];
        assert_eq!(names(&file), expected);

        let there = Home(folder.join("another computer"));
        assert!(
            there
                .unpack(&file, "not the password")
                .unwrap_err()
                .contains("password is wrong")
        );
        assert!(
            !fs::read(&file)
                .unwrap()
                .windows(5)
                .any(|window| window == b"Hi\n= " || window == b"Work/")
        );
        let manifest = there.unpack(&file, PASSWORD).unwrap();
        assert!(there.chats().join("chat-1/chat.json").is_file());
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
        assert!(there.looks().join("paper/2.0.0/theme.json").is_file());

        let (offered, absent) = there.missing(&manifest, &[], &[]);
        assert_eq!(
            offered,
            [
                there.themes().join("acme/0.1.0"),
                there.looks().join("paper/2.0.0")
            ]
        );
        assert!(absent.is_empty());
        let installed = [("acme".to_string(), "0.2.0".to_string())];
        let looks = [("paper".to_string(), "1.0.0".to_string())];
        assert_eq!(
            there.missing(&manifest, &installed, &looks).0,
            [there.themes().join("acme/0.1.0")]
        );
        fs::remove_dir_all(there.themes()).unwrap();
        assert_eq!(there.missing(&manifest, &[], &[]).1, ["acme 0.1.0"]);
        assert!(
            super::absent(&["acme 0.1.0".into()]).contains("not installed:\n• acme 0.1.0\nInstall")
        );

        let again = folder.join("sent on.znote");
        there
            .pack(
                &folder.join("no themes here"),
                &again,
                None,
                (manifest.look.clone(), manifest.settings.clone()),
                PASSWORD,
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
        home.remember(
            &home
                .pack(&themes, &file, None, (None, Value::Null), PASSWORD)
                .unwrap(),
        );

        let written = fs::metadata(&same).unwrap().modified().unwrap();
        fs::write(&same, "FIRST").unwrap();
        fs::File::options()
            .write(true)
            .open(&same)
            .unwrap()
            .set_modified(written)
            .unwrap();
        fs::write(&changed, "second, and longer").unwrap();
        put(pages.join("Work/Notes/New.typ"), "new");
        put(pages.join("Work/Notes/Gone.typ"), "gone");
        fs::remove_file(pages.join("Work/Notes/Gone.typ")).unwrap();
        home.pack(&themes, &next, Some(&file), (None, Value::Null), PASSWORD)
            .unwrap();
        assert!(!names(&next).contains(&"pages/Work/Notes/Gone.typ".to_string()));
        assert_eq!(text(&next, "pages/Work/Notes/Same.typ"), "first");
        assert_eq!(
            text(&next, "pages/Work/Notes/Changed.typ"),
            "second, and longer"
        );
        assert_eq!(text(&next, "pages/Work/Notes/New.typ"), "new");

        home.pack(&themes, &next, None, (None, Value::Null), PASSWORD)
            .unwrap();
        assert_eq!(text(&next, "pages/Work/Notes/Same.typ"), "FIRST");
        assert!(
            checked(&next, PASSWORD, 99, &[])
                .unwrap_err()
                .contains("did not read back")
        );
    }

    #[test]
    fn a_file_that_is_not_ours_or_reaches_outside_its_folder_changes_nothing() {
        let folder = scratch("refused");
        let (file, home) = (folder.join("bad.znote"), Home(folder.join("inner/home")));
        let manifest = r#"{ "format": 2, "look": null, "themes": [] }"#;

        for name in [
            "../escaped.txt",
            "pages/../../../escaped.txt",
            "/escaped.txt",
            "pages\\..\\..\\escaped.txt",
            "pages//x",
        ] {
            archive(
                &file,
                &[
                    ("znote.json", manifest),
                    ("pages/Work/Notes/A.typ", ""),
                    (name, "gotcha"),
                ],
            );
            let error = home.unpack(&file, PASSWORD).unwrap_err();
            assert!(error.contains("outside its own folder"), "{name}: {error}");
            assert!(
                !folder.join("escaped.txt").exists() && !folder.join("inner/escaped.txt").exists()
            );
        }

        archive(&file, &[("pages/Work/Notes/A.typ", "")]);
        assert!(
            home.unpack(&file, PASSWORD)
                .unwrap_err()
                .contains("no znote.json")
        );
        archive(
            &file,
            &[(
                "znote.json",
                r#"{ "format": 3, "look": null, "themes": [] }"#,
            )],
        );
        assert!(
            home.unpack(&file, PASSWORD)
                .unwrap_err()
                .contains("newer version")
        );
        fs::write(&file, "plain text").unwrap();
        assert!(
            home.unpack(&file, PASSWORD)
                .unwrap_err()
                .contains("not a znote file")
        );

        archive(
            &file,
            &[
                ("znote.json", manifest),
                ("pages/CON/Notes/A.typ", ""),
                ("pages/.hidden/x", ""),
                ("themes/../x/1.0.0/theme.json", "{}"),
            ],
        );
        assert!(home.unpack(&file, PASSWORD).is_err());
        archive(
            &file,
            &[
                ("znote.json", manifest),
                ("pages/CON/Notes/A.typ", ""),
                ("pages/.hidden/x", ""),
                ("themes/Bad Id/1.0.0/theme.json", "{}"),
            ],
        );
        let read = home.unpack(&file, PASSWORD).unwrap();
        assert_eq!(fs::read_dir(home.pages()).unwrap().count(), 0);
        let wants = Manifest {
            themes: vec!["Bad Id:1.0.0".into()],
            ..read
        };
        assert_eq!(
            home.missing(&wants, &[], &[]),
            (Vec::new(), vec!["Bad Id 1.0.0".to_string()])
        );
    }

    #[test]
    fn the_newest_theme_version_is_found_by_number_not_by_letter() {
        let folder = scratch("versions");
        for version in ["1.9.0", "1.10.0", "1.2.0"] {
            install(&folder, "acme", version);
        }
        assert_eq!(versions(&folder.join("acme")), ["1.2.0", "1.9.0", "1.10.0"]);
    }
}

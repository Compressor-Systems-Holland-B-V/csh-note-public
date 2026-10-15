pub mod index;
pub mod search;

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Mutex, MutexGuard};
use std::thread::spawn;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use model2vec_rs::model::StaticModel;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

use crate::models;
use crate::policy::{self, Rule};
use crate::privacy::glossary::{self, Concept, Edge, Label};
use crate::privacy::names::{self, Finder};
use crate::privacy::{
    self, Detector, Entry, Found, Glossary, LITERAL, NAMES, Override, Restored, Vault, Veiled,
    folded,
};
use crate::sealed::{self, check_password};
use crate::workspace::{self, Workspace};
use crate::{agent, extract};

const VERSION: i64 = 1;
const EXTENSION: &str = "znote-knowledge";
const SERVICE: &str = "znote knowledge";
const MAX_NAME: usize = 80;
const DEBOUNCE: Duration = Duration::from_secs(2);
const LOCKED: &str = "Names are hidden from the assistant, but your knowledge file is locked. Open it in Settings first.";
const NO_FINDER: &str = "Names are hidden from the assistant, but the name finder could not be loaded. Download it again in Settings, or stop finding names.";
const NOT_READY: &str = "Names are hidden from the assistant, but the hiding settings have not been loaded yet. Try again in a moment.";

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS concepts (id INTEGER PRIMARY KEY, iri TEXT NOT NULL UNIQUE, kind TEXT NOT NULL, masked INTEGER NOT NULL, top INTEGER NOT NULL, notes TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS labels (concept INTEGER NOT NULL, position INTEGER NOT NULL, text TEXT NOT NULL, hidden INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS edges (source INTEGER NOT NULL, target INTEGER NOT NULL, relation TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS overrides (text TEXT PRIMARY KEY, kind TEXT);
CREATE TABLE IF NOT EXISTS vault (token TEXT PRIMARY KEY, kind TEXT NOT NULL, number INTEGER NOT NULL, variant INTEGER NOT NULL, entity TEXT NOT NULL, text TEXT NOT NULL UNIQUE);
CREATE TABLE IF NOT EXISTS veil_cache (hash TEXT PRIMARY KEY, hits TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS sources (id INTEGER PRIMARY KEY, kind TEXT NOT NULL, project TEXT NOT NULL, path TEXT NOT NULL, hash TEXT NOT NULL, time INTEGER NOT NULL, UNIQUE (kind, project, path));
CREATE TABLE IF NOT EXISTS chunks (id INTEGER PRIMARY KEY, source INTEGER NOT NULL, position INTEGER NOT NULL, text TEXT NOT NULL);
CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5 (text, tokenize = 'unicode61 remove_diacritics 2');
CREATE TABLE IF NOT EXISTS vectors (chunk INTEGER PRIMARY KEY, vector BLOB NOT NULL);
CREATE TABLE IF NOT EXISTS mentions (chunk INTEGER NOT NULL, concept INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS chunks_by_source ON chunks (source);
CREATE INDEX IF NOT EXISTS mentions_by_chunk ON mentions (chunk);
CREATE INDEX IF NOT EXISTS mentions_by_concept ON mentions (concept);
";

/// How much the name finder hides: 0 finds nothing, 1 people, 2 also organisations, places and
/// the glossary's own categories, 3 also products and projects. The glossary, overrides and
/// patterns hide at every level.
pub const LIGHT: u8 = 1;
pub const STRICT: u8 = 3;

fn lowest_allowed(level: u8) -> u8 {
    level.clamp(policy::get().minimum_level, STRICT)
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Options {
    pub veil: bool,
    pub amounts: bool,
    pub names: bool,
    pub knowledge: bool,
    pub auto_add: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Listed {
    id: String,
    name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
struct Shelf {
    active: Option<String>,
    files: Vec<Listed>,
}

pub struct Open {
    pub id: String,
    pub db: Connection,
    vault: Vault,
    detector: Detector,
    pub glossary: Glossary,
    concepts: HashMap<String, usize>,
    imported: bool,
}

#[derive(Default)]
pub struct Knowledge {
    open: Mutex<Option<Open>>,
    options: Mutex<Option<Options>>,
    pages: Mutex<Option<HashMap<String, u8>>>,
    finder: Mutex<Option<Finder>>,
    embedder: Mutex<Option<StaticModel>>,
    picked: Mutex<Option<PathBuf>>,
    queue: Mutex<Option<Sender<String>>>,
}

fn sql(error: rusqlite::Error) -> String {
    format!("The knowledge file could not be used: {error}")
}

fn locked<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>, String> {
    mutex.lock().map_err(|error| error.to_string())
}

pub fn hash(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn fresh_id(seed: &str) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_nanos())
        .unwrap_or_default();
    hash(&format!("{seed}{now}{}", std::process::id()))[..16].to_string()
}

fn plain_id(id: &str) -> bool {
    id.len() == 16 && id.chars().all(|c| c.is_ascii_hexdigit())
}

fn connect(file: &Path, password: &str, flags: OpenFlags) -> Result<Connection, String> {
    sealed::open(
        file,
        password,
        flags,
        "The password is wrong, or this is not a knowledge file.",
    )
}

fn checked(db: &Connection) -> Result<(String, String), String> {
    let damaged = |_| "This knowledge file is damaged.".to_string();
    let verdict: String = db
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(damaged)?;
    if verdict != "ok" {
        return Err("This knowledge file is damaged.".into());
    }
    let meta = |key: &str| -> Result<String, String> {
        db.query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .map_err(damaged)
    };
    if meta("version")?.parse::<i64>().ok() != Some(VERSION) {
        return Err("This knowledge file was made by another version of znote.".into());
    }
    let id = meta("id")?;
    if !plain_id(&id) {
        return Err("This knowledge file is damaged.".into());
    }
    Ok((id, meta("name")?))
}

fn create(file: &Path, name: &str, password: &str) -> Result<String, String> {
    let db = connect(file, password, OpenFlags::default())?;
    db.execute_batch(SCHEMA).map_err(sql)?;
    let id = fresh_id(name);
    for (key, value) in [
        ("version", VERSION.to_string()),
        ("id", id.clone()),
        ("name", name.into()),
    ] {
        db.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)",
            params![key, value],
        )
        .map_err(sql)?;
    }
    Ok(id)
}

fn store_glossary(db: &mut Connection, glossary: &Glossary) -> Result<(), String> {
    let transaction = db.transaction().map_err(sql)?;
    transaction
        .execute_batch(
            "DELETE FROM concepts; DELETE FROM labels; DELETE FROM edges; DELETE FROM mentions;",
        )
        .map_err(sql)?;
    for (id, concept) in glossary.concepts.iter().enumerate() {
        let notes = serde_json::to_string(&concept.notes).map_err(|error| error.to_string())?;
        transaction
            .execute(
                "INSERT INTO concepts (id, iri, kind, masked, top, notes) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id as i64, concept.iri, concept.kind, concept.masked, concept.top, notes],
            )
            .map_err(sql)?;
        for (position, label) in concept.labels.iter().enumerate() {
            transaction
                .execute(
                    "INSERT INTO labels (concept, position, text, hidden) VALUES (?1, ?2, ?3, ?4)",
                    params![id as i64, position as i64, label.text, label.hidden],
                )
                .map_err(sql)?;
        }
    }
    for edge in &glossary.edges {
        transaction
            .execute(
                "INSERT INTO edges (source, target, relation) VALUES (?1, ?2, ?3)",
                params![edge.from as i64, edge.to as i64, edge.relation],
            )
            .map_err(sql)?;
    }
    transaction.commit().map_err(sql)
}

fn read_glossary(db: &Connection) -> Result<Glossary, String> {
    let mut labels: HashMap<i64, Vec<Label>> = HashMap::new();
    let mut statement = db
        .prepare("SELECT concept, text, hidden FROM labels ORDER BY concept, position")
        .map_err(sql)?;
    for row in statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, bool>(2)?,
            ))
        })
        .map_err(sql)?
    {
        let (concept, text, hidden) = row.map_err(sql)?;
        labels
            .entry(concept)
            .or_default()
            .push(Label { text, hidden });
    }
    let mut statement = db
        .prepare("SELECT id, iri, kind, masked, top, notes FROM concepts ORDER BY id")
        .map_err(sql)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, bool>(3)?,
                row.get::<_, bool>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .map_err(sql)?;
    let mut position = HashMap::new();
    let mut concepts = Vec::new();
    for row in rows {
        let (id, iri, kind, masked, top, notes) = row.map_err(sql)?;
        position.insert(id, concepts.len());
        concepts.push(Concept {
            iri,
            kind,
            labels: labels.remove(&id).unwrap_or_default(),
            masked,
            top,
            notes: serde_json::from_str(&notes).unwrap_or_default(),
        });
    }
    let mut statement = db
        .prepare("SELECT source, target, relation FROM edges")
        .map_err(sql)?;
    let edges = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(sql)?
        .filter_map(Result::ok)
        .filter_map(|(source, target, relation)| {
            Some(Edge {
                from: *position.get(&source)?,
                to: *position.get(&target)?,
                relation,
            })
        })
        .collect();
    Ok(Glossary { concepts, edges })
}

fn read_overrides(db: &Connection) -> Result<Vec<Override>, String> {
    let mut statement = db
        .prepare("SELECT text, kind FROM overrides")
        .map_err(sql)?;
    let rows = statement
        .query_map([], |row| {
            Ok(Override {
                text: row.get(0)?,
                kind: row.get(1)?,
            })
        })
        .map_err(sql)?;
    rows.map(|row| row.map_err(sql)).collect()
}

fn read_vault(db: &Connection) -> Result<Vault, String> {
    let mut vault = Vault::default();
    let mut statement = db
        .prepare("SELECT token, kind, entity, text, number, variant FROM vault")
        .map_err(sql)?;
    let rows = statement
        .query_map([], |row| {
            Ok(Entry {
                token: row.get(0)?,
                kind: row.get(1)?,
                entity: row.get(2)?,
                text: row.get(3)?,
                number: row.get(4)?,
                variant: row.get(5)?,
            })
        })
        .map_err(sql)?;
    for row in rows {
        vault.add(&row.map_err(sql)?);
    }
    Ok(vault)
}

fn positions(glossary: &Glossary) -> HashMap<String, usize> {
    glossary
        .concepts
        .iter()
        .enumerate()
        .map(|(at, concept)| (concept.iri.clone(), at))
        .collect()
}

impl Open {
    fn load(id: String, db: Connection) -> Result<Self, String> {
        let stored = read_glossary(&db)?;
        let imported = !stored.concepts.is_empty();
        let glossary = if imported {
            stored
        } else {
            glossary::default()
        };
        let detector = Detector::new(&glossary, &read_overrides(&db)?)?;
        Ok(Self {
            id,
            vault: read_vault(&db)?,
            db,
            detector,
            concepts: positions(&glossary),
            glossary,
            imported,
        })
    }

    fn rebuild(&mut self) -> Result<(), String> {
        self.detector = Detector::new(&self.glossary, &read_overrides(&self.db)?)?;
        self.concepts = positions(&self.glossary);
        Ok(())
    }

    pub fn concept_at(&self, iri: &str) -> Option<usize> {
        self.concepts.get(iri).copied()
    }

    fn name_labels(&self) -> Vec<(String, String)> {
        let mut labels: Vec<(String, String)> = privacy::rules()
            .map(|rules| rules.labels.clone())
            .unwrap_or_default();
        for concept in self.glossary.concepts.iter().filter(|concept| concept.top) {
            if !labels.iter().any(|(_, kind)| *kind == concept.kind) {
                labels.push((concept.pref().to_lowercase(), concept.kind.clone()));
            }
        }
        labels
    }

    fn names(&mut self, text: &str, finder: &Finder, level: u8) -> Result<Vec<Found>, String> {
        if level == 0 {
            return Ok(Vec::new());
        }
        let labels = self.name_labels();
        let salt = hash(&format!("{labels:?}"));
        let mut lines = Vec::new();
        let mut at = 0;
        for line in text.split_inclusive('\n') {
            let bare = line.trim_end_matches(['\n', '\r']);
            if bare.chars().filter(|c| c.is_alphabetic()).count() >= 2 {
                lines.push((at, bare, hash(&format!("{salt}{bare}"))));
            }
            at += line.len();
        }
        let mut hits: HashMap<String, Vec<names::Hit>> = HashMap::new();
        {
            let mut statement = self
                .db
                .prepare_cached("SELECT hits FROM veil_cache WHERE hash = ?1")
                .map_err(sql)?;
            for (_, _, key) in &lines {
                let cached: Option<String> = statement
                    .query_row([key], |row| row.get(0))
                    .optional()
                    .map_err(sql)?;
                if let Some(found) = cached.and_then(|json| serde_json::from_str(&json).ok()) {
                    hits.insert(key.clone(), found);
                }
            }
        }
        let missing: Vec<&(usize, &str, String)> = lines
            .iter()
            .filter(|(_, _, key)| !hits.contains_key(key))
            .collect();
        if !missing.is_empty() {
            let texts: Vec<&str> = missing.iter().map(|(_, line, _)| *line).collect();
            let found = finder.find(&texts, &labels)?;
            let transaction = self.db.transaction().map_err(sql)?;
            for ((_, _, key), found) in missing.iter().zip(found) {
                let json = serde_json::to_string(&found).map_err(|error| error.to_string())?;
                transaction
                    .execute(
                        "INSERT OR REPLACE INTO veil_cache (hash, hits) VALUES (?1, ?2)",
                        params![key, json],
                    )
                    .map_err(sql)?;
                hits.insert(key.clone(), found);
            }
            transaction.commit().map_err(sql)?;
        }
        Ok(lines
            .iter()
            .flat_map(|(offset, line, key)| {
                hits.get(key)
                    .into_iter()
                    .flatten()
                    .filter(move |(_, _, kind)| shown(kind, level))
                    .filter(move |(start, end, _)| line.get(*start..*end).is_some())
                    .flat_map(move |(start, end, kind)| {
                        names::cleaned(line, *start, *end)
                            .into_iter()
                            .map(move |(start, end)| Found {
                                start: offset + start,
                                end: offset + end,
                                kind: kind.clone(),
                                entity: format!("{kind}:{}", folded(&line[start..end]).0),
                                pref: None,
                                rank: NAMES,
                            })
                    })
            })
            .collect())
    }

    fn veil(
        &mut self,
        text: &str,
        options: &Options,
        finder: Option<&Finder>,
        level: u8,
    ) -> Result<Veiled, String> {
        let names = match finder.filter(|_| options.names) {
            Some(finder) => self.names(text, finder, level)?,
            None => Vec::new(),
        };
        let pass = |vault: &mut Vault| {
            let found = [names.clone(), vault.known(text)].concat();
            vault.veil(text, &self.detector.find(text, options.amounts, found))
        };
        let (mut veiled, mut fresh) = pass(&mut self.vault);
        if !fresh.is_empty() {
            // A name first found further down is hidden at its earlier places too.
            let (again, more) = pass(&mut self.vault);
            veiled = again;
            fresh.extend(more);
        }
        if !fresh.is_empty()
            && let Err(error) = self.keep(&fresh)
        {
            self.vault = read_vault(&self.db)?;
            return Err(error);
        }
        Ok(veiled)
    }

    fn keep(&mut self, fresh: &[Entry]) -> Result<(), String> {
        let transaction = self.db.transaction().map_err(sql)?;
        for entry in fresh {
            transaction
                .execute(
                    // ponytail: a retagged text replaces its old row, so the old token restores only
                    // until the file is reopened; keep both rows if old chats must keep restoring.
                    "INSERT OR REPLACE INTO vault (token, kind, number, variant, entity, text) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![entry.token, entry.kind, entry.number, entry.variant, entry.entity, entry.text],
                )
                .map_err(sql)?;
        }
        transaction.commit().map_err(sql)
    }

    pub fn unveil(&self, text: &str) -> (String, Vec<Restored>) {
        self.vault.unveil(text)
    }

    pub fn unveil_typst(&self, text: &str) -> String {
        self.vault.unveil_typst(text)
    }

    pub fn real(&self, token: &str) -> Option<&(String, String)> {
        self.vault.real(token)
    }
}

// The finder is always asked for every kind, because with fewer to choose from it pins the ones
// left on ordinary words. The level only decides which of its answers are hidden. Broad kinds
// such as the glossary's customer and supplier catch many ordinary words, so only Strict hides them.
fn shown(kind: &str, level: u8) -> bool {
    privacy::rules().map_or(true, |rules| level >= rules.level_of(kind))
}

pub struct Gate<'a> {
    knowledge: &'a Knowledge,
    on: bool,
}

impl Gate<'_> {
    pub fn on(&self) -> bool {
        self.on
    }

    pub fn knows(&self) -> bool {
        self.knowledge.options().knowledge && self.knowledge.is_open()
    }

    pub fn search(&self, query: &str) -> Result<String, String> {
        self.knowledge.search_tool(query)
    }

    pub fn entity(&self, token: &str) -> Result<String, String> {
        self.knowledge.entity_tool(token)
    }

    pub fn id(&self) -> Option<String> {
        if !self.on {
            return None;
        }
        self.knowledge
            .open
            .lock()
            .ok()?
            .as_ref()
            .map(|open| open.id.clone())
    }

    // A prompt can carry notes from any page in the knowledge base, so it is hidden as strictly as
    // the strictest page.
    pub fn veil(&self, text: &str) -> Result<Veiled, String> {
        self.veil_at(
            text,
            if self.knows() {
                self.strictest()
            } else {
                LIGHT
            },
        )
    }

    pub fn page_level(&self, page: &str) -> u8 {
        self.knowledge.page_level(page)
    }

    pub fn searchable(&self, query: &str) -> Result<String, String> {
        if !self.on {
            return Ok(query.into());
        }
        let veiled = self.veil(query)?;
        let given = |token: &str| {
            self.knowledge
                .with(|open| Ok(open.real(token).is_some_and(|(_, kind)| kind == LITERAL)))
                .ok()
                .flatten()
                .unwrap_or(false)
        };
        let mut kept = String::with_capacity(query.len());
        let mut at = 0;
        for (start, end, token) in &veiled.marks {
            kept.push_str(&query[at..*start]);
            if given(token) {
                kept.push_str(&query[*start..*end]);
            }
            at = *end;
        }
        kept.push_str(&query[at..]);
        Ok(self.unveil(&kept).0)
    }

    pub fn strictest(&self) -> u8 {
        let most = match self.knowledge.pages.lock().as_deref() {
            Ok(Some(pages)) => pages.values().copied().max().unwrap_or(LIGHT),
            _ => STRICT,
        };
        lowest_allowed(most.max(LIGHT))
    }

    pub fn veil_at(&self, text: &str, level: u8) -> Result<Veiled, String> {
        if !self.on {
            return Ok(Veiled {
                text: text.into(),
                ..Veiled::default()
            });
        }
        self.knowledge.veil(text, level)
    }

    pub fn unveil(&self, text: &str) -> (String, Vec<Restored>) {
        if !self.on {
            return (text.into(), Vec::new());
        }
        match self.knowledge.open.lock() {
            Ok(open) => match open.as_ref() {
                Some(open) => open.unveil(text),
                None => (text.into(), Vec::new()),
            },
            Err(_) => (text.into(), Vec::new()),
        }
    }

    pub fn unveil_typst(&self, text: &str) -> String {
        if !self.on {
            return text.into();
        }
        match self.knowledge.open.lock() {
            Ok(open) => open
                .as_ref()
                .map_or_else(|| text.into(), |open| open.unveil_typst(text)),
            Err(_) => text.into(),
        }
    }
}

impl Knowledge {
    fn chosen(&self) -> Option<Options> {
        let mut options = (*self.options.lock().ok()?)?;
        let policy = policy::get();
        options.veil |= policy.hide_names == Rule::Required;
        options.names |= policy.find_names == Rule::Required;
        options.amounts |= policy.hide_amounts == Rule::Required;
        Some(options)
    }

    pub fn options(&self) -> Options {
        self.chosen().unwrap_or_default()
    }

    pub fn with<T>(
        &self,
        work: impl FnOnce(&mut Open) -> Result<T, String>,
    ) -> Result<Option<T>, String> {
        let mut open = locked(&self.open)?;
        open.as_mut().map(work).transpose()
    }

    pub fn page_level(&self, page: &str) -> u8 {
        let level = match self.pages.lock().as_deref() {
            Ok(Some(pages)) => pages.get(page).copied().unwrap_or(LIGHT),
            _ => STRICT,
        };
        lowest_allowed(level)
    }

    pub fn is_open(&self) -> bool {
        self.open.lock().is_ok_and(|open| open.is_some())
    }

    pub fn gate(&self) -> Result<Gate<'_>, String> {
        self.gated(self.options().veil)
    }

    pub fn gate_if(&self, wanted: bool) -> Result<Gate<'_>, String> {
        let options = self.chosen().ok_or(NOT_READY)?;
        let wanted = wanted || policy::get().hide_names == Rule::Required;
        if wanted && !options.veil {
            return Err(NOT_READY.into());
        }
        self.gated(wanted)
    }

    fn gated(&self, on: bool) -> Result<Gate<'_>, String> {
        if on && !self.is_open() {
            return Err(LOCKED.into());
        }
        Ok(Gate {
            knowledge: self,
            on,
        })
    }

    pub fn veil(&self, text: &str, level: u8) -> Result<Veiled, String> {
        let options = self.options();
        let finder = locked(&self.finder)?;
        let level = lowest_allowed(level);
        if options.names && level > 0 && finder.is_none() {
            return Err(NO_FINDER.into());
        }
        self.with(|open| open.veil(text, &options, finder.as_ref(), level))?
            .ok_or_else(|| LOCKED.to_string())
    }

    fn search(
        &self,
        work: impl FnOnce(&Open, Option<&StaticModel>) -> Result<String, String>,
    ) -> Result<String, String> {
        if !self.options().knowledge {
            return Ok(String::new());
        }
        let embedder = locked(&self.embedder)?;
        Ok(self
            .with(|open| work(open, embedder.as_ref()))?
            .unwrap_or_default())
    }

    pub fn notes(&self, prompt: &str) -> Result<String, String> {
        self.search(|open, embedder| search::notes(open, embedder, prompt))
    }

    pub fn search_tool(&self, query: &str) -> Result<String, String> {
        self.search(|open, embedder| search::found(open, embedder, query))
            .map(|found| {
                if found.is_empty() {
                    "The knowledge base is switched off.".into()
                } else {
                    found
                }
            })
    }

    pub fn entity_tool(&self, token: &str) -> Result<String, String> {
        self.with(|open| search::entity(open, token))?
            .ok_or_else(|| LOCKED.to_string())
    }

    fn close(&self) -> Result<(), String> {
        *locked(&self.open)? = None;
        Ok(())
    }

    fn use_file(&self, id: String, db: Connection) -> Result<(), String> {
        *locked(&self.open)? = Some(Open::load(id, db)?);
        Ok(())
    }

    #[cfg(test)]
    pub fn in_memory(glossary: &Glossary, options: Options) -> Self {
        let knowledge = Self::default();
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch(SCHEMA).unwrap();
        store_glossary(&mut db, glossary).unwrap();
        knowledge.use_file("0000000000000000".into(), db).unwrap();
        *knowledge.options.lock().unwrap() = Some(options);
        *knowledge.pages.lock().unwrap() = Some(HashMap::new());
        knowledge
    }
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

fn file_of(folder: &Path, id: &str) -> Result<PathBuf, String> {
    if !plain_id(id) {
        return Err("This knowledge file is not known.".into());
    }
    Ok(folder.join(format!("{id}.{EXTENSION}")))
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
    crate::agent::keep(&folder.join("index.json"), &bytes)
        .map_err(|error| format!("The list of knowledge files could not be saved: {error}"))
}

fn remembered(id: &str) -> Option<String> {
    sealed::remembered(SERVICE, id)
}

fn remember(id: &str, password: Option<&str>) -> Result<(), String> {
    sealed::remember(SERVICE, id, password)
}

fn check_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME || name.chars().any(char::is_control) {
        return Err("Give the knowledge file a short name.".into());
    }
    Ok(name.into())
}

pub fn import_file(folder: &Path, source: &Path, password: &str) -> Result<Listed, String> {
    let (id, name) = {
        let db = connect(source, password, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        checked(&db)?
    };
    let target = file_of(folder, &id)?;
    if target.exists() {
        return Err(
            "This knowledge file is already imported. Remove it first to import it again.".into(),
        );
    }
    let mut temp = target.clone();
    temp.as_mut_os_string().push(".tmp");
    fs::copy(source, &temp)
        .and_then(|_| fs::rename(&temp, &target))
        .map_err(|error| {
            let _ = fs::remove_file(&temp);
            format!("The knowledge file could not be copied: {error}")
        })?;
    Ok(Listed { id, name })
}

pub fn export_file(db: &Connection, target: &Path, password: &str) -> Result<(), String> {
    check_password(password)?;
    let mut temp = target.to_path_buf();
    temp.as_mut_os_string().push(".tmp");
    let _ = fs::remove_file(&temp);
    let exported = db
        .execute(
            "ATTACH DATABASE ?1 AS export KEY ?2",
            params![temp.to_string_lossy(), password],
        )
        .and_then(|_| db.query_row("SELECT sqlcipher_export('export')", [], |_| Ok(())))
        .and_then(|()| db.execute("DETACH DATABASE export", []))
        .map_err(sql)
        .and_then(|_| {
            checked(&connect(&temp, password, OpenFlags::SQLITE_OPEN_READ_ONLY)?).map(drop)
        })
        .and_then(|()| fs::rename(&temp, target).map_err(|error| error.to_string()));
    if exported.is_err() {
        let _ = fs::remove_file(&temp);
    }
    exported.map_err(|error| format!("The knowledge file could not be exported: {error}"))
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
                open.vault.len(),
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
    let knowledge = app.state::<Knowledge>();
    let Ok(mut finder) = knowledge.finder.lock() else {
        return;
    };
    if finder.is_none()
        && let Ok(models) = models::folder(app)
        && models::NAME_FINDER.verified(&models)
    {
        *finder = Finder::load(&models::NAME_FINDER.path(&models)).ok();
    }
}

pub fn models_ready(app: &AppHandle, model: &str) {
    match model {
        "names" => load_finder(app),
        _ => load_embedder(app),
    }
}

fn load_embedder(app: &AppHandle) {
    let knowledge = app.state::<Knowledge>();
    let Ok(mut embedder) = knowledge.embedder.lock() else {
        return;
    };
    if embedder.is_none()
        && let Ok(models) = models::folder(app)
        && models::SMART_SEARCH.verified(&models)
    {
        *embedder =
            StaticModel::from_pretrained(models::SMART_SEARCH.path(&models), None, None, None).ok();
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
    let embedder = locked(&knowledge.embedder)?;
    knowledge.with(|open| {
        if by_hand || options.auto_add || index::known(&open.db, index::PAGE, &project, page)? {
            index::put(open, embedder.as_ref(), index::PAGE, &project, page, &text)?;
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
    let Ok(mut queue) = knowledge.queue.lock() else {
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
        knowledge.veil(&text, agent::level_of(&folder, &name))?;
    }
    let embedder = locked(&knowledge.embedder)?;
    knowledge
        .with(|open| {
            index::put(
                open,
                embedder.as_ref(),
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
    {
        let embedder = locked(&knowledge.embedder)?;
        knowledge.with(|open| index::refresh(open, embedder.as_ref()))?;
    }
    every_page(&app);
    status(&app)
}

#[tauri::command]
pub fn privacy_pages(app: AppHandle, levels: HashMap<String, u8>) -> Result<(), String> {
    *locked(&app.state::<Knowledge>().pages)? = Some(levels);
    Ok(())
}

#[tauri::command]
pub async fn privacy_options(app: AppHandle, options: Options) -> Result<(), String> {
    crate::blocking(move || privacy_options_now(app, options)).await
}

fn privacy_options_now(app: AppHandle, options: Options) -> Result<(), String> {
    *locked(&app.state::<Knowledge>().options)? = Some(options);
    if options.names {
        load_finder(&app);
    }
    if options.knowledge {
        load_embedder(&app);
    }
    Ok(())
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
    *locked(&app.state::<Knowledge>().picked)? = Some(file);
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
    let source = locked(&app.state::<Knowledge>().picked)?
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
    *locked(&app.state::<Knowledge>().picked)? = None;
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
        .with(|open| {
            store_glossary(&mut open.db, &parsed)?;
            open.glossary = read_glossary(&open.db)?;
            open.imported = true;
            open.rebuild()?;
            index::concepts(open, None)
        })?
        .ok_or_else(|| LOCKED.to_string())?;
    Ok(Some(status(&app)?))
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Piece {
    text: String,
    token: Option<String>,
    kind: Option<String>,
}

fn pieces(text: &str, veiled: &Veiled, kinds: impl Fn(&str) -> Option<String>) -> Vec<Piece> {
    let mut pieces = Vec::new();
    let mut at = 0;
    for (start, end, token) in &veiled.marks {
        if *start > at {
            pieces.push(Piece {
                text: text[at..*start].into(),
                token: None,
                kind: None,
            });
        }
        pieces.push(Piece {
            text: text[*start..*end].into(),
            token: Some(token.clone()),
            kind: kinds(token),
        });
        at = *end;
    }
    if at < text.len() {
        pieces.push(Piece {
            text: text[at..].into(),
            token: None,
            kind: None,
        });
    }
    pieces
}

#[tauri::command]
pub async fn privacy_view(
    app: AppHandle,
    text: String,
    level: Option<u8>,
) -> Result<Vec<Piece>, String> {
    crate::blocking(move || privacy_view_now(app, text, level)).await
}

fn privacy_view_now(app: AppHandle, text: String, level: Option<u8>) -> Result<Vec<Piece>, String> {
    let knowledge = app.state::<Knowledge>();
    let veiled = knowledge.veil(&text, level.unwrap_or(LIGHT))?;
    knowledge
        .with(|open| {
            Ok(pieces(&text, &veiled, |token| {
                open.real(token).map(|(_, kind)| kind.clone())
            }))
        })?
        .ok_or_else(|| LOCKED.to_string())
}

#[tauri::command]
pub async fn privacy_override(
    app: AppHandle,
    text: String,
    kind: Option<String>,
) -> Result<(), String> {
    crate::blocking(move || privacy_override_now(app, text, kind)).await
}

fn privacy_override_now(app: AppHandle, text: String, kind: Option<String>) -> Result<(), String> {
    let text = text.trim().to_string();
    if text.is_empty() || text.contains('\n') || text.chars().count() > 200 {
        return Err("Choose a short piece of text on one line.".into());
    }
    let kind = kind.map(|kind| crate::privacy::kind_name(&kind));
    app.state::<Knowledge>()
        .with(|open| {
            open.db
                .execute(
                    "INSERT OR REPLACE INTO overrides (text, kind) VALUES (?1, ?2)",
                    params![text, kind],
                )
                .map_err(sql)?;
            open.rebuild()
        })?
        .ok_or_else(|| LOCKED.to_string())
}

#[tauri::command]
pub async fn privacy_overrides(app: AppHandle) -> Result<Vec<Override>, String> {
    crate::blocking(move || privacy_overrides_now(app)).await
}

fn privacy_overrides_now(app: AppHandle) -> Result<Vec<Override>, String> {
    app.state::<Knowledge>()
        .with(|open| read_overrides(&open.db))?
        .ok_or_else(|| LOCKED.to_string())
}

#[tauri::command]
pub async fn privacy_forget(app: AppHandle, text: String) -> Result<(), String> {
    crate::blocking(move || privacy_forget_now(app, text)).await
}

fn privacy_forget_now(app: AppHandle, text: String) -> Result<(), String> {
    app.state::<Knowledge>()
        .with(|open| {
            open.db
                .execute("DELETE FROM overrides WHERE text = ?1", [text.trim()])
                .map_err(sql)?;
            open.rebuild()
        })?
        .ok_or_else(|| LOCKED.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::config::DbConfig;

    fn scratch(test: &str) -> PathBuf {
        let base =
            std::env::temp_dir().join(format!("znote-knowledge-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        base
    }

    fn made(base: &Path, password: &str) -> (PathBuf, String) {
        let file = base.join("mine.znote-knowledge");
        let id = create(&file, "Mine", password).unwrap();
        (file, id)
    }

    #[test]
    fn a_file_with_a_trigger_is_refused() {
        let base = scratch("trigger");
        let (file, _) = made(&base, "a fine password");
        let db = connect(&file, "a fine password", OpenFlags::default()).unwrap();
        db.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, false)
            .unwrap();
        db.execute_batch("CREATE TRIGGER t AFTER INSERT ON vault BEGIN INSERT INTO overrides (text, kind) VALUES (NEW.text, NULL); END;")
            .unwrap();
        drop(db);
        assert!(
            connect(&file, "a fine password", OpenFlags::default())
                .is_err_and(|error| error.contains("holds more than znote puts in one"))
        );
    }

    #[test]
    fn a_wrong_password_is_refused() {
        let base = scratch("password");
        let (file, id) = made(&base, "correct horse");
        assert!(
            connect(&file, "wrong horse", OpenFlags::default())
                .unwrap_err()
                .contains("password is wrong")
        );
        let db = connect(&file, "correct horse", OpenFlags::default()).unwrap();
        assert_eq!(checked(&db).unwrap(), (id, "Mine".into()));
        let bytes = fs::read(&file).unwrap();
        assert!(!bytes.windows(4).any(|window| window == b"Mine"));
        assert!(check_password("short").is_err());
    }

    #[test]
    fn an_export_reopens_with_its_new_password_and_gives_the_same_tokens() {
        let base = scratch("export");
        let (file, id) = made(&base, "first password");
        let db = connect(&file, "first password", OpenFlags::default()).unwrap();
        let mut open = Open::load(id.clone(), db).unwrap();
        let options = Options {
            veil: true,
            ..Options::default()
        };
        let veiled = open
            .veil("mail jan@acme.example", &options, None, STRICT)
            .unwrap();
        let target = base.join("copy.znote-knowledge");
        export_file(&open.db, &target, "second password").unwrap();
        assert!(connect(&target, "first password", OpenFlags::default()).is_err());

        let elsewhere = base.join("elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        let listed = import_file(&elsewhere, &target, "second password").unwrap();
        assert_eq!(listed.id, id);
        let moved = file_of(&elsewhere, &id).unwrap();
        let mut again = Open::load(
            id,
            connect(&moved, "second password", OpenFlags::default()).unwrap(),
        )
        .unwrap();
        assert_eq!(again.unveil(&veiled.text).0, "mail jan@acme.example");
        assert_eq!(
            again
                .veil("mail jan@acme.example", &options, None, STRICT)
                .unwrap()
                .text,
            veiled.text
        );
        assert!(
            import_file(&elsewhere, &target, "second password")
                .unwrap_err()
                .contains("already imported")
        );
    }

    #[test]
    fn a_corrupt_or_foreign_file_is_refused_and_nothing_is_copied() {
        let base = scratch("corrupt");
        let (file, _) = made(&base, "a fine password");
        let mut bytes = fs::read(&file).unwrap();
        let middle = bytes.len() / 2;
        bytes[middle..middle + 64]
            .iter_mut()
            .for_each(|byte| *byte ^= 0xff);
        let broken = base.join("broken.znote-knowledge");
        fs::write(&broken, bytes).unwrap();
        let shelf = base.join("shelf");
        fs::create_dir_all(&shelf).unwrap();
        assert!(import_file(&shelf, &broken, "a fine password").is_err());
        fs::write(base.join("text.znote-knowledge"), "just text").unwrap();
        assert!(
            import_file(
                &shelf,
                &base.join("text.znote-knowledge"),
                "a fine password"
            )
            .is_err()
        );
        assert!(import_file(&shelf, &file, "the wrong one").is_err());
        assert_eq!(fs::read_dir(&shelf).unwrap().count(), 0);
    }

    #[test]
    fn a_glossary_and_overrides_are_kept_in_the_file() {
        let base = scratch("glossary");
        let (file, id) = made(&base, "a fine password");
        let mut db = connect(&file, "a fine password", OpenFlags::default()).unwrap();
        let ttl = "@prefix skos: <http://www.w3.org/2004/02/skos/core#> .
<urn:c> skos:prefLabel \"Customer\" ; skos:topConceptOf <urn:s> .
<urn:a> skos:prefLabel \"Acme\" ; skos:broader <urn:c> ; skos:definition \"Big one.\" .";
        let parsed = glossary::parse(ttl.as_bytes(), oxrdfio::RdfFormat::Turtle).unwrap();
        store_glossary(&mut db, &parsed).unwrap();
        db.execute(
            "INSERT INTO overrides (text, kind) VALUES ('Blue Harbour', 'PROJECT')",
            [],
        )
        .unwrap();
        let mut open = Open::load(id, db).unwrap();
        assert_eq!(open.glossary, parsed);
        let options = Options {
            veil: true,
            ..Options::default()
        };
        assert_eq!(
            open.veil("Acme in Blue Harbour", &options, None, STRICT)
                .unwrap()
                .text,
            "⟦CUSTOMER_1⟧ in ⟦PROJECT_1⟧"
        );
        let text = "Acme here";
        let veiled = open.veil(text, &options, None, STRICT).unwrap();
        let shown = pieces(text, &veiled, |token| {
            open.real(token).map(|(_, kind)| kind.clone())
        });
        assert_eq!(
            shown,
            [
                Piece {
                    text: "Acme".into(),
                    token: Some("⟦CUSTOMER_1⟧".into()),
                    kind: Some("CUSTOMER".into())
                },
                Piece {
                    text: " here".into(),
                    token: None,
                    kind: None
                }
            ]
        );
    }

    #[test]
    fn a_name_hidden_once_stays_hidden_after_the_file_is_reopened() {
        let base = scratch("known");
        let (file, id) = made(&base, "a fine password");
        let db = connect(&file, "a fine password", OpenFlags::default()).unwrap();
        db.execute(
            "INSERT INTO overrides (text, kind) VALUES ('Fatima El Amrani', 'PERSON')",
            [],
        )
        .unwrap();
        let mut open = Open::load(id.clone(), db).unwrap();
        let options = Options {
            veil: true,
            ..Options::default()
        };
        let table = open
            .veil("Fatima El Amrani, ontwikkelaar", &options, None, LIGHT)
            .unwrap();
        assert_eq!(table.text, "⟦PERSON_1⟧, ontwikkelaar");
        open.db.execute("DELETE FROM overrides", []).unwrap();
        let mut open = Open::load(id, open.db).unwrap();
        let risk = "met kennis van de oude koppeling: Fatima\nEl Amrani.";
        let veiled = open.veil(risk, &options, None, 0).unwrap();
        assert_eq!(
            veiled.text,
            "met kennis van de oude koppeling: ⟦PERSON_1~1⟧\n⟦PERSON_1~2⟧."
        );
        assert_eq!(open.unveil(&veiled.text).0, risk);
    }

    #[test]
    fn veiling_off_lets_text_through_and_on_without_a_file_refuses() {
        let knowledge = Knowledge::default();
        assert_eq!(knowledge.gate().unwrap().veil("Acme").unwrap().text, "Acme");
        assert_eq!(knowledge.gate_if(true).err().as_deref(), Some(NOT_READY));
        assert_eq!(knowledge.gate_if(false).err().as_deref(), Some(NOT_READY));
        *knowledge.options.lock().unwrap() = Some(Options {
            veil: true,
            ..Options::default()
        });
        assert_eq!(knowledge.gate_if(true).err().as_deref(), Some(LOCKED));
        assert_eq!(knowledge.gate().err().as_deref(), Some(LOCKED));
        let unveiled = knowledge.gate_if(false).unwrap();
        assert!(!unveiled.on());
        assert_eq!(unveiled.veil("Acme").unwrap().text, "Acme");
        let finderless = Knowledge::in_memory(
            &Glossary::default(),
            Options {
                veil: true,
                names: true,
                ..Options::default()
            },
        );
        let gate = finderless.gate().unwrap();
        assert_eq!(
            gate.veil_at("Acme", LIGHT).err().as_deref(),
            Some(NO_FINDER)
        );
        assert_eq!(gate.veil_at("Acme", 0).unwrap().text, "Acme");
    }

    #[test]
    fn a_page_is_strict_until_levels_arrive_then_light_without_one() {
        let knowledge = Knowledge::default();
        let gate = knowledge.gate().unwrap();
        assert_eq!(gate.page_level("Work/Notes/Monday"), STRICT);
        assert_eq!(gate.strictest(), STRICT);
        *knowledge.pages.lock().unwrap() = Some(HashMap::new());
        assert_eq!(gate.page_level("Work/Notes/Monday"), LIGHT);
        assert_eq!(gate.strictest(), LIGHT);
        *knowledge.pages.lock().unwrap() = Some(HashMap::from([
            ("Work/Notes/Monday".into(), 9),
            ("Work/Notes/Tuesday".into(), 0),
        ]));
        assert_eq!(gate.page_level("Work/Notes/Monday"), STRICT);
        assert_eq!(gate.page_level("Work/Notes/Tuesday"), 0);
        assert_eq!(gate.strictest(), STRICT);
    }

    #[test]
    fn a_knowledge_search_finds_by_token_but_not_by_a_name_guessed_in_clear() {
        let ttl = "@prefix skos: <http://www.w3.org/2004/02/skos/core#> .
<urn:c> skos:prefLabel \"Customer\" ; skos:topConceptOf <urn:s> .
<urn:a> skos:prefLabel \"Acme Corporation\" ; skos:broader <urn:c> .";
        let glossary = glossary::parse(ttl.as_bytes(), oxrdfio::RdfFormat::Turtle).unwrap();
        let knowledge = Knowledge::in_memory(
            &glossary,
            Options {
                veil: true,
                ..Options::default()
            },
        );
        let gate = knowledge.gate().unwrap();
        let token = gate.veil("Acme Corporation").unwrap().text;
        assert_eq!(
            gate.searchable(&format!("contract with {token}")).unwrap(),
            "contract with Acme Corporation"
        );
        assert_eq!(
            gate.searchable("contract with Acme Corporation").unwrap(),
            "contract with "
        );
        let off = Knowledge::default();
        assert_eq!(off.gate().unwrap().searchable("Acme").unwrap(), "Acme");
    }

    #[test]
    fn each_level_hides_more_kinds_of_name() {
        let kinds = [
            "PERSON",
            "ORGANISATION",
            "LOCATION",
            "CUSTOMER",
            "SUPPLIER",
            "PRODUCT",
            "PROJECT",
        ];
        let hidden = |level| kinds.iter().filter(|kind| shown(kind, level)).count();
        assert_eq!(
            [hidden(0), hidden(LIGHT), hidden(2), hidden(STRICT)],
            [0, 1, 3, 7]
        );
    }
}

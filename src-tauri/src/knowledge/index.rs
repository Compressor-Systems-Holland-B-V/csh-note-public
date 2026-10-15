use model2vec_rs::model::StaticModel;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;

use super::{Open, hash, sql};

pub const PAGE: &str = "page";
pub const ATTACHMENT: &str = "attachment";
pub const CONCEPT: &str = "concept";
pub const GLOSSARY: &str = "glossary";
pub const ATTACHMENTS: &str = "attachments";
const PIECE: usize = 1000;
const OVERLAP: usize = 150;

#[derive(Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Counted {
    pub project: String,
    pub sources: usize,
    pub chunks: usize,
}

pub fn counts(db: &Connection) -> Result<Vec<Counted>, String> {
    let mut statement = db
        .prepare(
            "SELECT project, count(DISTINCT sources.id), count(chunks.id) FROM sources
             LEFT JOIN chunks ON chunks.source = sources.id GROUP BY project ORDER BY project",
        )
        .map_err(sql)?;
    let rows = statement
        .query_map([], |row| {
            Ok(Counted {
                project: row.get(0)?,
                sources: row.get::<_, i64>(1)? as usize,
                chunks: row.get::<_, i64>(2)? as usize,
            })
        })
        .map_err(sql)?;
    rows.map(|row| row.map_err(sql)).collect()
}

fn heading(kind: &str, line: &str) -> bool {
    match kind {
        PAGE => line.starts_with('='),
        ATTACHMENT => line.starts_with('#'),
        _ => false,
    }
}

fn sections<'a>(text: &'a str, kind: &str) -> Vec<&'a str> {
    let mut found = Vec::new();
    let mut start = 0;
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        if at > start && heading(kind, line) {
            found.push(&text[start..at]);
            start = at;
        }
        at += line.len();
    }
    found.push(&text[start..]);
    found
}

fn pieces(section: &str) -> Vec<String> {
    let chars: Vec<char> = section.chars().collect();
    let mut found = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let mut end = (start + PIECE).min(chars.len());
        if end < chars.len()
            && let Some(space) = chars[start + PIECE / 2..end]
                .iter()
                .rposition(|c| c.is_whitespace())
        {
            end = start + PIECE / 2 + space;
        }
        found.push(chars[start..end].iter().collect::<String>());
        if end == chars.len() {
            break;
        }
        start = end.saturating_sub(OVERLAP).max(start + 1);
    }
    found
}

pub fn chunks(text: &str, kind: &str) -> Vec<String> {
    sections(text, kind)
        .into_iter()
        .flat_map(pieces)
        .map(|piece| piece.trim().to_string())
        .filter(|piece| !piece.is_empty())
        .collect()
}

pub fn vector(embedder: &StaticModel, text: &str) -> Vec<u8> {
    embedder
        .encode_single(text)
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

pub fn values(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|four| f32::from_le_bytes(*four))
        .collect()
}

pub fn mentioned(open: &Open, text: &str) -> Vec<usize> {
    let mut found: Vec<usize> = open
        .detector
        .terms(text)
        .into_iter()
        .filter_map(|hit| open.concept_at(hit.entity.strip_prefix("concept:")?))
        .collect();
    found.sort_unstable();
    found.dedup();
    found
}

fn clear(transaction: &Transaction, source: i64) -> Result<(), String> {
    for statement in [
        "DELETE FROM chunks_fts WHERE rowid IN (SELECT id FROM chunks WHERE source = ?1)",
        "DELETE FROM vectors WHERE chunk IN (SELECT id FROM chunks WHERE source = ?1)",
        "DELETE FROM mentions WHERE chunk IN (SELECT id FROM chunks WHERE source = ?1)",
        "DELETE FROM chunks WHERE source = ?1",
    ] {
        transaction.execute(statement, [source]).map_err(sql)?;
    }
    Ok(())
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|time| time.as_secs() as i64)
        .unwrap_or_default()
}

pub fn known(db: &Connection, kind: &str, project: &str, path: &str) -> Result<bool, String> {
    db.query_row(
        "SELECT 1 FROM sources WHERE kind = ?1 AND project = ?2 AND path = ?3",
        params![kind, project, path],
        |_| Ok(()),
    )
    .optional()
    .map(|found| found.is_some())
    .map_err(sql)
}

pub fn put(
    open: &mut Open,
    embedder: Option<&StaticModel>,
    kind: &str,
    project: &str,
    path: &str,
    text: &str,
) -> Result<bool, String> {
    let digest = hash(text);
    let existing: Option<(i64, String)> = open
        .db
        .query_row(
            "SELECT id, hash FROM sources WHERE kind = ?1 AND project = ?2 AND path = ?3",
            params![kind, project, path],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(sql)?;
    if existing.as_ref().is_some_and(|(_, known)| *known == digest) {
        return Ok(false);
    }
    let pieces: Vec<(String, Vec<usize>, Option<Vec<u8>>)> = chunks(text, kind)
        .into_iter()
        .map(|piece| {
            let concepts = mentioned(open, &piece);
            let vector = embedder.map(|embedder| vector(embedder, &piece));
            (piece, concepts, vector)
        })
        .collect();
    let transaction = open.db.transaction().map_err(sql)?;
    let source = match existing {
        Some((id, _)) => {
            clear(&transaction, id)?;
            transaction
                .prepare_cached("UPDATE sources SET hash = ?1, time = ?2 WHERE id = ?3")
                .and_then(|mut statement| statement.execute(params![digest, now(), id]))
                .map_err(sql)?;
            id
        }
        None => {
            transaction
                .prepare_cached("INSERT INTO sources (kind, project, path, hash, time) VALUES (?1, ?2, ?3, ?4, ?5)")
                .and_then(|mut statement| statement.execute(params![kind, project, path, digest, now()]))
                .map_err(sql)?;
            transaction.last_insert_rowid()
        }
    };
    for (position, (piece, concepts, vector)) in pieces.iter().enumerate() {
        transaction
            .prepare_cached("INSERT INTO chunks (source, position, text) VALUES (?1, ?2, ?3)")
            .and_then(|mut statement| statement.execute(params![source, position as i64, piece]))
            .map_err(sql)?;
        let chunk = transaction.last_insert_rowid();
        transaction
            .prepare_cached("INSERT INTO chunks_fts (rowid, text) VALUES (?1, ?2)")
            .and_then(|mut statement| statement.execute(params![chunk, piece]))
            .map_err(sql)?;
        if let Some(vector) = vector {
            transaction
                .prepare_cached("INSERT INTO vectors (chunk, vector) VALUES (?1, ?2)")
                .and_then(|mut statement| statement.execute(params![chunk, vector]))
                .map_err(sql)?;
        }
        for concept in concepts {
            transaction
                .prepare_cached("INSERT INTO mentions (chunk, concept) VALUES (?1, ?2)")
                .and_then(|mut statement| statement.execute(params![chunk, *concept as i64]))
                .map_err(sql)?;
        }
    }
    transaction.commit().map_err(sql)?;
    Ok(true)
}

pub fn remove(open: &mut Open, kind: &str, project: &str, path: &str) -> Result<(), String> {
    let transaction = open.db.transaction().map_err(sql)?;
    let sources: Vec<i64> = {
        let mut statement = transaction
            .prepare("SELECT id FROM sources WHERE kind = ?1 AND project = ?2 AND (path = ?3 OR ?3 = '')")
            .map_err(sql)?;
        let rows = statement
            .query_map(params![kind, project, path], |row| row.get(0))
            .map_err(sql)?;
        rows.filter_map(Result::ok).collect()
    };
    for source in sources {
        clear(&transaction, source)?;
        transaction
            .execute("DELETE FROM sources WHERE id = ?1", [source])
            .map_err(sql)?;
    }
    transaction.commit().map_err(sql)
}

pub fn forget(open: &mut Open, project: &str) -> Result<(), String> {
    for kind in [PAGE, ATTACHMENT, CONCEPT] {
        remove(open, kind, project, "")?;
    }
    Ok(())
}

pub fn concepts(open: &mut Open, embedder: Option<&StaticModel>) -> Result<(), String> {
    remove(open, CONCEPT, GLOSSARY, "")?;
    let notes: Vec<(String, String)> = open
        .glossary
        .concepts
        .iter()
        .filter(|concept| !concept.notes.is_empty())
        .map(|concept| {
            (
                concept.iri.clone(),
                format!("{}: {}", concept.pref(), concept.notes.join(" ")),
            )
        })
        .collect();
    for (iri, text) in notes {
        put(open, embedder, CONCEPT, GLOSSARY, &iri, &text)?;
    }
    Ok(())
}

pub fn refresh(open: &mut Open, embedder: Option<&StaticModel>) -> Result<(), String> {
    let chunks: Vec<(i64, String)> = {
        let mut statement = open
            .db
            .prepare("SELECT id, text FROM chunks")
            .map_err(sql)?;
        let rows = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(sql)?;
        rows.filter_map(Result::ok).collect()
    };
    let computed: Vec<(i64, Vec<usize>, Option<Vec<u8>>)> = chunks
        .iter()
        .map(|(id, text)| {
            (
                *id,
                mentioned(open, text),
                embedder.map(|embedder| vector(embedder, text)),
            )
        })
        .collect();
    let transaction = open.db.transaction().map_err(sql)?;
    transaction
        .execute_batch("DELETE FROM mentions; DELETE FROM vectors; UPDATE sources SET hash = '' WHERE kind = 'page';")
        .map_err(sql)?;
    for (chunk, concepts, vector) in computed {
        for concept in concepts {
            transaction
                .prepare_cached("INSERT INTO mentions (chunk, concept) VALUES (?1, ?2)")
                .and_then(|mut statement| statement.execute(params![chunk, concept as i64]))
                .map_err(sql)?;
        }
        if let Some(vector) = vector {
            transaction
                .prepare_cached("INSERT INTO vectors (chunk, vector) VALUES (?1, ?2)")
                .and_then(|mut statement| statement.execute(params![chunk, vector]))
                .map_err(sql)?;
        }
    }
    transaction.commit().map_err(sql)?;
    concepts(open, embedder)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typst_pages_split_at_headings_and_long_text_overlaps() {
        let page = "intro\n= One\nfirst\n== Two\nsecond\n";
        assert_eq!(
            chunks(page, PAGE),
            ["intro", "= One\nfirst", "== Two\nsecond"]
        );
        let long = "word ".repeat(500);
        let cut = chunks(&long, PAGE);
        assert_eq!(cut.len(), 3);
        assert!(cut.iter().all(|piece| piece.chars().count() <= PIECE));
        assert!(cut[0].ends_with("word") && cut[1].starts_with("word"));
        let total: usize = cut.iter().map(|piece| piece.len()).sum();
        assert!(total > long.trim().len());
    }

    #[test]
    fn attachments_split_at_the_sections_the_extractor_writes() {
        let text = "## Page 1\n\nhello\n\n## Page 2\n\nworld\nPage 3 is not a heading";
        assert_eq!(
            chunks(text, ATTACHMENT),
            [
                "## Page 1\n\nhello",
                "## Page 2\n\nworld\nPage 3 is not a heading"
            ]
        );
        assert_eq!(
            chunks("## Slide 3: Intro\nx", ATTACHMENT),
            ["## Slide 3: Intro\nx"]
        );
        assert!(chunks(" \n ", PAGE).is_empty());
    }

    #[test]
    fn vectors_survive_the_trip_through_bytes() {
        let bytes: Vec<u8> = [1.5f32, -2.0]
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        assert_eq!(values(&bytes), [1.5, -2.0]);
    }
}

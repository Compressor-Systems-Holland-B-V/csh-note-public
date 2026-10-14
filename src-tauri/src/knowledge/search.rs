use std::collections::{BTreeSet, HashMap};

use model2vec_rs::model::StaticModel;
use rusqlite::params;

use super::index::{self, values};
use super::{Open, sql};

const K: f64 = 60.0;
const CANDIDATES: usize = 50;
const SEEDS: usize = 5;
const MAX_RELATED: usize = 8;
pub const NOTES: usize = 5;
pub const MAX_NOTES: usize = 6000;
pub const MAX_HITS: usize = 10;

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub chunk: i64,
    pub path: String,
    pub text: String,
    pub concepts: Vec<usize>,
}

fn phrase(query: &str) -> Option<String> {
    let words: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.chars().count() >= 2)
        .take(32)
        .map(|word| format!("\"{word}\""))
        .collect();
    (!words.is_empty()).then(|| words.join(" OR "))
}

fn by_words(open: &Open, query: &str) -> Result<Vec<i64>, String> {
    let Some(phrase) = phrase(query) else {
        return Ok(Vec::new());
    };
    let mut statement = open
        .db
        .prepare_cached("SELECT rowid FROM chunks_fts WHERE chunks_fts MATCH ?1 ORDER BY bm25(chunks_fts) LIMIT ?2")
        .map_err(sql)?;
    let rows = statement
        .query_map(params![phrase, CANDIDATES as i64], |row| row.get(0))
        .map_err(sql)?;
    rows.map(|row| row.map_err(sql)).collect()
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let size = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>().sqrt();
    let product = size(a) * size(b);
    if product == 0.0 { 0.0 } else { dot / product }
}

fn by_meaning(
    open: &Open,
    embedder: Option<&StaticModel>,
    query: &str,
) -> Result<Vec<i64>, String> {
    let Some(embedder) = embedder else {
        return Ok(Vec::new());
    };
    let wanted = embedder.encode_single(query);
    let mut statement = open
        .db
        .prepare_cached("SELECT chunk, vector FROM vectors")
        .map_err(sql)?;
    let mut scored: Vec<(f32, i64)> = statement
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
        })
        .map_err(sql)?
        .filter_map(Result::ok)
        .map(|(chunk, bytes)| (cosine(&wanted, &values(&bytes)), chunk))
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    Ok(scored
        .into_iter()
        .take(CANDIDATES)
        .map(|(_, chunk)| chunk)
        .collect())
}

fn mentions(open: &Open, chunk: i64) -> Result<Vec<usize>, String> {
    let mut statement = open
        .db
        .prepare_cached("SELECT concept FROM mentions WHERE chunk = ?1")
        .map_err(sql)?;
    let rows = statement
        .query_map([chunk], |row| row.get::<_, i64>(0))
        .map_err(sql)?;
    Ok(rows
        .filter_map(Result::ok)
        .map(|concept| concept as usize)
        .collect())
}

fn mentioning(open: &Open, concept: usize) -> Result<Vec<i64>, String> {
    let mut statement = open
        .db
        .prepare_cached("SELECT chunk FROM mentions WHERE concept = ?1 LIMIT ?2")
        .map_err(sql)?;
    let rows = statement
        .query_map(params![concept as i64, CANDIDATES as i64], |row| row.get(0))
        .map_err(sql)?;
    Ok(rows.filter_map(Result::ok).collect())
}

pub fn neighbours(open: &Open, concepts: &BTreeSet<usize>) -> BTreeSet<usize> {
    let mut near = concepts.clone();
    for edge in &open.glossary.edges {
        if concepts.contains(&edge.from) {
            near.insert(edge.to);
        }
        if concepts.contains(&edge.to) {
            near.insert(edge.from);
        }
    }
    near
}

pub fn search(
    open: &Open,
    embedder: Option<&StaticModel>,
    query: &str,
    limit: usize,
) -> Result<Vec<Hit>, String> {
    let mut scores: HashMap<i64, f64> = HashMap::new();
    for ranked in [by_words(open, query)?, by_meaning(open, embedder, query)?] {
        for (rank, chunk) in ranked.into_iter().enumerate() {
            *scores.entry(chunk).or_default() += 1.0 / (K + rank as f64 + 1.0);
        }
    }
    let asked: BTreeSet<usize> = index::mentioned(open, query).into_iter().collect();
    for concept in &asked {
        for chunk in mentioning(open, *concept)? {
            scores.entry(chunk).or_default();
        }
    }
    let mut ranked: Vec<(i64, f64)> = scores.into_iter().collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut seeds = asked.clone();
    for (chunk, _) in ranked.iter().take(SEEDS) {
        seeds.extend(mentions(open, *chunk)?);
    }
    let near = neighbours(open, &seeds);
    for concept in near.difference(&seeds) {
        for chunk in mentioning(open, *concept)? {
            if !ranked.iter().any(|(known, _)| *known == chunk) {
                ranked.push((chunk, 0.0));
            }
        }
    }
    let mut boosted = Vec::with_capacity(ranked.len());
    for (chunk, score) in ranked {
        let concepts = mentions(open, chunk)?;
        let shared = concepts
            .iter()
            .filter(|concept| near.contains(concept))
            .count()
            .min(3);
        boosted.push((chunk, score + shared as f64 / (2.0 * (K + 1.0)), concepts));
    }
    boosted.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut hits = Vec::new();
    let mut statement = open
        .db
        .prepare_cached("SELECT chunks.text, sources.path, sources.kind FROM chunks JOIN sources ON sources.id = chunks.source WHERE chunks.id = ?1")
        .map_err(sql)?;
    for (chunk, _, concepts) in boosted.into_iter().take(limit) {
        let (text, path, kind): (String, String, String) = statement
            .query_row([chunk], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(sql)?;
        let path = if kind == index::CONCEPT {
            "glossary".to_string()
        } else {
            path
        };
        hits.push(Hit {
            chunk,
            path,
            text,
            concepts,
        });
    }
    Ok(hits)
}

fn category(open: &Open, kind: &str) -> String {
    open.glossary
        .concepts
        .iter()
        .find(|concept| concept.top && concept.kind == kind)
        .map(|concept| concept.pref().to_string())
        .unwrap_or_else(|| kind.to_lowercase())
}

pub fn card(open: &Open, concept: usize) -> Option<String> {
    let found = open.glossary.concepts.get(concept)?;
    let related: Vec<&str> = neighbours(open, &BTreeSet::from([concept]))
        .into_iter()
        .filter(|other| *other != concept)
        .filter_map(|other| open.glossary.concepts.get(other))
        .filter(|other| !other.top)
        .take(MAX_RELATED)
        .map(|other| other.pref())
        .collect();
    let mut card = format!("{}: {}.", found.pref(), category(open, &found.kind));
    if !related.is_empty() {
        card.push_str(&format!(" Related: {}.", related.join(", ")));
    }
    if !found.notes.is_empty() {
        card.push_str(&format!(" Note: {}", found.notes.join(" ")));
    }
    Some(card)
}

pub fn notes(open: &Open, embedder: Option<&StaticModel>, query: &str) -> Result<String, String> {
    let hits = search(open, embedder, query, NOTES)?;
    if hits.is_empty() {
        return Ok(String::new());
    }
    let mut block = String::from("Notes that may help:\n");
    let concepts: BTreeSet<usize> = hits
        .iter()
        .flat_map(|hit| hit.concepts.iter().copied())
        .collect();
    let mut parts: Vec<String> = hits
        .iter()
        .map(|hit| format!("From {}:\n{}", hit.path, hit.text))
        .collect();
    parts.extend(
        concepts
            .into_iter()
            .filter_map(|concept| card(open, concept)),
    );
    for part in parts {
        if block.chars().count() + part.chars().count() + 2 > MAX_NOTES {
            break;
        }
        block.push_str(&part);
        block.push_str("\n\n");
    }
    Ok(block.trim_end().to_string())
}

pub fn found(open: &Open, embedder: Option<&StaticModel>, query: &str) -> Result<String, String> {
    let hits = search(open, embedder, query, MAX_HITS)?;
    if hits.is_empty() {
        return Ok("Nothing found in the knowledge base.".into());
    }
    Ok(hits
        .iter()
        .map(|hit| format!("From {}:\n{}", hit.path, hit.text))
        .collect::<Vec<_>>()
        .join("\n\n"))
}

pub fn entity(open: &Open, token: &str) -> Result<String, String> {
    let Some((text, _)) = open.real(token.trim()).cloned() else {
        return Err("This token is not known. Copy it exactly as it was written.".into());
    };
    let lower = text.to_lowercase();
    let concept = open.glossary.concepts.iter().position(|concept| {
        concept
            .labels
            .iter()
            .any(|label| label.text.to_lowercase() == lower)
    });
    let mut paths: BTreeSet<String> = BTreeSet::new();
    let chunks = match concept {
        Some(concept) => mentioning(open, concept)?,
        None => by_words(open, &text)?,
    };
    let mut statement = open
        .db
        .prepare_cached("SELECT sources.path FROM chunks JOIN sources ON sources.id = chunks.source WHERE chunks.id = ?1 AND sources.kind <> 'concept'")
        .map_err(sql)?;
    for chunk in chunks {
        if let Ok(path) = statement.query_row([chunk], |row| row.get::<_, String>(0)) {
            paths.insert(path);
        }
    }
    let mut answer = concept
        .and_then(|concept| card(open, concept))
        .unwrap_or_else(|| format!("{text}: not in the glossary."));
    if !paths.is_empty() {
        answer.push_str(&format!(
            "\nMentioned in: {}",
            paths.into_iter().take(20).collect::<Vec<_>>().join(", ")
        ));
    }
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::super::{Knowledge, Options};
    use super::*;
    use crate::privacy::glossary;

    const GLOSSARY: &str = r#"
@prefix skos: <http://www.w3.org/2004/02/skos/core#> .
<urn:customer> skos:prefLabel "Customer" ; skos:topConceptOf <urn:s> .
<urn:project> skos:prefLabel "Project" ; skos:topConceptOf <urn:s> .
<urn:acme> skos:prefLabel "Acme Corporation" ; skos:altLabel "Acme" ; skos:broader <urn:customer> ;
  skos:related <urn:phoenix> ; skos:definition "Pays Zephyr Fund monthly." .
<urn:phoenix> skos:prefLabel "Phoenix" ; skos:broader <urn:project> .
<urn:other> skos:prefLabel "Globex" ; skos:broader <urn:customer> .
"#;

    #[test]
    fn a_fixed_corpus_returns_the_expected_chunk_first_and_says_no_glossary_term() {
        let glossary = glossary::parse(GLOSSARY.as_bytes(), oxrdfio::RdfFormat::Turtle).unwrap();
        let options = Options {
            veil: true,
            knowledge: true,
            ..Options::default()
        };
        let knowledge = Knowledge::in_memory(&glossary, options);
        knowledge
            .with(|open| {
                let pages = [
                    ("Work/Notes/Compressors", "= Maintenance\nThe compressor maintenance contract with Acme renews in March.\n= Lunch\nPizza on Friday."),
                    ("Work/Notes/Phoenix", "= Phoenix\nThe Phoenix rollout needs two more engineers."),
                    ("Work/Notes/Globex", "= Globex\nGlobex asked about the price of filters."),
                ];
                for (path, text) in pages {
                    index::put(open, None, index::PAGE, "/notes.znote", path, text)?;
                }
                index::concepts(open, None)?;
                let hits = search(open, None, "When does the maintenance contract renew?", 3)?;
                assert_eq!(hits[0].path, "Work/Notes/Compressors");
                assert!(hits[0].text.contains("renews in March"));
                let phoenix = search(open, None, "who works on Acme things", 5)?;
                assert!(phoenix.iter().any(|hit| hit.path == "Work/Notes/Phoenix"), "{phoenix:?}");
                assert!(!index::put(open, None, index::PAGE, "/notes.znote", pages[0].0, pages[0].1)?);
                Ok(())
            })
            .unwrap();
        let gate = knowledge.gate().unwrap();
        let block = knowledge
            .with(|open| notes(open, None, "maintenance contract with acme"))
            .unwrap()
            .unwrap();
        assert!(block.contains(
            "Acme Corporation: Customer. Related: Phoenix. Note: Pays Zephyr Fund monthly."
        ));
        let veiled = gate.veil(&block).unwrap().text;
        for term in ["Acme", "acme", "Phoenix", "Globex"] {
            assert!(!veiled.contains(term), "{term} in {veiled}");
        }
        assert!(veiled.starts_with("Notes that may help:\nFrom Work/Notes/Compressors:"));
        assert!(
            veiled.contains("⟦CUSTOMER_1⟧: Customer. Related: ⟦PROJECT_1⟧."),
            "{veiled}"
        );
        assert_eq!(gate.unveil(&veiled).0, block);

        let card = knowledge
            .with(|open| entity(open, "⟦CUSTOMER_1⟧"))
            .unwrap()
            .unwrap();
        assert!(
            card.contains("Mentioned in: Work/Notes/Compressors"),
            "{card}"
        );
        assert!(knowledge.with(|open| entity(open, "⟦CUSTOMER_9⟧")).is_err());
    }
}

use std::path::Path;

use ndarray::Array2;
use ort::session::Session;
use tokenizers::{InputSequence, Tokenizer};

pub const MODEL: &str = "model.onnx";
pub const TOKENIZER: &str = "tokenizer.json";
pub const KINDS: [(&str, &str); 5] = [
    ("person", "PERSON"),
    ("organisation", "ORGANISATION"),
    ("product", "PRODUCT"),
    ("project", "PROJECT"),
    ("location", "LOCATION"),
];

const THRESHOLD: f32 = 0.3;
const MAX_WIDTH: usize = 12;
const MAX_WORDS: usize = 200;
const BATCH: usize = 16;
const ENTITY: &str = "<<ENT>>";
const SEPARATOR: &str = "<<SEP>>";

pub type Hit = (usize, usize, String);

pub struct Finder {
    session: Session,
    tokenizer: Tokenizer,
}

fn broken(error: impl std::fmt::Display) -> String {
    format!("The name finder could not run: {error}")
}

fn sigmoid(value: f32) -> f32 {
    1.0 / (1.0 + (-value).exp())
}

pub fn words(line: &str) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut start = None;
    for (at, c) in line.char_indices().chain([(line.len(), ' ')]) {
        match (c.is_whitespace(), start) {
            (false, None) => start = Some(at),
            (true, Some(from)) => {
                found.push((from, at));
                start = None;
            }
            _ => {}
        }
    }
    found
}

fn trimmed(line: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    let piece = &line[start..end];
    let inner = piece.trim_matches(|c: char| !c.is_alphanumeric());
    if inner.is_empty() {
        return None;
    }
    let from = start + piece.find(inner)?;
    Some((from, from + inner.len()))
}

impl Finder {
    pub fn load(folder: &Path) -> Result<Self, String> {
        let session = Session::builder()
            .and_then(|builder| builder.commit_from_file(folder.join(MODEL)))
            .map_err(broken)?;
        let tokenizer = Tokenizer::from_file(folder.join(TOKENIZER)).map_err(broken)?;
        Ok(Self { session, tokenizer })
    }

    pub fn find(
        &self,
        lines: &[&str],
        labels: &[(String, String)],
    ) -> Result<Vec<Vec<Hit>>, String> {
        let mut found = vec![Vec::new(); lines.len()];
        let worded: Vec<usize> = (0..lines.len())
            .filter(|&at| !words(lines[at]).is_empty())
            .collect();
        for batch in worded.chunks(BATCH) {
            let texts: Vec<&str> = batch.iter().map(|&at| lines[at]).collect();
            for (&at, hits) in batch.iter().zip(self.batch(&texts, labels)?) {
                found[at] = hits;
            }
        }
        Ok(found)
    }

    fn batch(&self, lines: &[&str], labels: &[(String, String)]) -> Result<Vec<Vec<Hit>>, String> {
        let mut prompt: Vec<&str> = labels
            .iter()
            .flat_map(|(label, _)| [ENTITY, label.as_str()])
            .collect();
        prompt.push(SEPARATOR);
        let mut encoded = Vec::with_capacity(lines.len());
        for line in lines {
            let spans: Vec<(usize, usize)> = words(line).into_iter().take(MAX_WORDS).collect();
            let mut pieces = prompt.clone();
            pieces.extend(spans.iter().map(|&(start, end)| &line[start..end]));
            let encoding = self
                .tokenizer
                .encode(InputSequence::from(pieces), true)
                .map_err(broken)?;
            encoded.push((encoding, spans));
        }
        let width = encoded
            .iter()
            .map(|(encoding, _)| encoding.get_ids().len())
            .max()
            .unwrap_or(0);
        let count = encoded
            .iter()
            .map(|(_, spans)| spans.len())
            .max()
            .unwrap_or(0);
        if count == 0 {
            return Ok(vec![Vec::new(); lines.len()]);
        }
        let rows = encoded.len();
        let mut ids = Array2::<i64>::zeros((rows, width));
        let mut attention = Array2::<i64>::zeros((rows, width));
        let mut words_mask = Array2::<i64>::zeros((rows, width));
        let mut lengths = Array2::<i64>::zeros((rows, 1));
        for (row, (encoding, spans)) in encoded.iter().enumerate() {
            let mut last = None;
            for (column, (&id, word)) in encoding
                .get_ids()
                .iter()
                .zip(encoding.get_word_ids())
                .enumerate()
            {
                ids[[row, column]] = i64::from(id);
                attention[[row, column]] = 1;
                if let Some(word) = word.map(|word| word as usize)
                    && word >= prompt.len()
                    && last != Some(word)
                {
                    words_mask[[row, column]] = (word - prompt.len() + 1) as i64;
                }
                last = word.map(|word| word as usize);
            }
            lengths[[row, 0]] = spans.len() as i64;
        }
        let inputs = ort::inputs! {
            "input_ids" => ids,
            "attention_mask" => attention,
            "words_mask" => words_mask,
            "text_lengths" => lengths,
        }
        .map_err(broken)?;
        let outputs = self.session.run(inputs).map_err(broken)?;
        let logits = outputs["logits"]
            .try_extract_tensor::<f32>()
            .map_err(broken)?;
        if logits.shape() != [rows, count, labels.len(), 3] {
            return Err(broken(format!("unexpected output {:?}", logits.shape())));
        }
        let mut found = Vec::with_capacity(rows);
        for (row, (_, spans)) in encoded.iter().enumerate() {
            let line = lines[row];
            let mut hits: Vec<(f32, usize, usize, usize)> = Vec::new();
            for class in 0..labels.len() {
                let score = |word: usize, part: usize| {
                    logits
                        .get(&[row, word, class, part][..])
                        .copied()
                        .map(sigmoid)
                        .unwrap_or(0.0)
                };
                for start in (0..spans.len()).filter(|&start| score(start, 0) >= THRESHOLD) {
                    for end in start..spans.len().min(start + MAX_WIDTH) {
                        if score(end, 1) < THRESHOLD {
                            continue;
                        }
                        let inside: Vec<f32> = (start..=end).map(|word| score(word, 2)).collect();
                        if inside.iter().all(|&value| value >= THRESHOLD) {
                            hits.push((
                                inside.iter().sum::<f32>() / inside.len() as f32,
                                start,
                                end,
                                class,
                            ));
                        }
                    }
                }
            }
            hits.sort_by(|a, b| b.0.total_cmp(&a.0));
            let mut kept: Vec<(usize, usize, usize)> = Vec::new();
            for (_, start, end, class) in hits {
                if kept.iter().all(|&(from, to, _)| end < from || start > to) {
                    kept.push((start, end, class));
                }
            }
            found.push(
                kept.into_iter()
                    .filter_map(|(start, end, class)| {
                        let (from, to) = trimmed(line, spans[start].0, spans[end].1)?;
                        Some((from, to, labels[class].1.clone()))
                    })
                    .collect(),
            );
        }
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_split_on_white_space_and_hits_lose_their_punctuation() {
        let line = "  Met Jan, at (Acme)  ";
        let spans: Vec<&str> = words(line)
            .iter()
            .map(|&(start, end)| &line[start..end])
            .collect();
        assert_eq!(spans, ["Met", "Jan,", "at", "(Acme)"]);
        assert_eq!(
            trimmed(line, 6, 10).map(|(start, end)| &line[start..end]),
            Some("Jan")
        );
        assert_eq!(
            trimmed(line, 14, 20).map(|(start, end)| &line[start..end]),
            Some("Acme")
        );
        assert_eq!(trimmed("-- --", 0, 5), None);
    }

    #[test]
    #[ignore = "needs the downloaded model in ZNOTE_NAME_FINDER"]
    fn the_downloaded_model_finds_people_and_places() {
        let folder = std::env::var("ZNOTE_NAME_FINDER").unwrap();
        let finder = Finder::load(Path::new(&folder)).unwrap();
        let labels: Vec<(String, String)> = KINDS
            .iter()
            .map(|(label, kind)| (label.to_string(), kind.to_string()))
            .collect();
        let lines = [
            "Jan de Vries met Maria Lopez in Rotterdam.",
            "",
            "Nothing here.",
        ];
        let found = finder.find(&lines, &labels).unwrap();
        let named: Vec<(&str, &str)> = found[0]
            .iter()
            .map(|(start, end, kind)| (&lines[0][*start..*end], kind.as_str()))
            .collect();
        assert!(named.contains(&("Jan de Vries", "PERSON")), "{named:?}");
        assert!(named.contains(&("Rotterdam", "LOCATION")), "{named:?}");
        assert!(found[1].is_empty() && found[2].is_empty(), "{found:?}");
    }
}

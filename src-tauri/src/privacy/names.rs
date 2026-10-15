use std::path::Path;

use ndarray::Array2;
use ort::session::Session;
use tokenizers::{InputSequence, Tokenizer};

use super::rules::rules;

pub const MODEL: &str = "model.onnx";
pub const TOKENIZER: &str = "tokenizer.json";
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

// Narrows a hit of the finder to what can be a name: it is split where a sentence ends, loses the
// lower-case words at either end ("het Microsoft account", "340 uur") and is dropped when nothing
// is left or only a common word is. Scripts without capitals keep every word.
fn sentences<'a>(text: &'a str, ends: &[String]) -> Vec<(usize, &'a str)> {
    let mut found = Vec::new();
    let mut from = 0;
    let mut at = 0;
    while at < text.len() {
        match ends.iter().find(|end| text[at..].starts_with(end.as_str())) {
            Some(end) => {
                found.push((from, &text[from..at]));
                at += end.len();
                from = at;
            }
            None => at += text[at..].chars().next().map_or(1, char::len_utf8),
        }
    }
    found.push((from, &text[from..]));
    found
}

pub fn cleaned(line: &str, start: usize, end: usize) -> Vec<(usize, usize)> {
    let Ok(rules) = rules() else {
        return vec![(start, end)];
    };
    let named = |(from, to): (usize, usize)| {
        line[from..to]
            .chars()
            .find(|c| c.is_alphabetic())
            .is_some_and(|c| !c.is_lowercase())
    };
    let mut kept = Vec::new();
    for (offset, sentence) in sentences(&line[start..end], &rules.sentence_ends) {
        let at = start + offset;
        let spans: Vec<(usize, usize)> = words(sentence)
            .into_iter()
            .map(|(from, to)| (at + from, at + to))
            .collect();
        let (Some(first), Some(last)) = (
            spans.iter().position(|&span| named(span)),
            spans.iter().rposition(|&span| named(span)),
        ) else {
            continue;
        };
        let Some((from, to)) = trimmed(line, spans[first].0, spans[last].1) else {
            continue;
        };
        if first == last && rules.stopword(&line[from..to]) {
            continue;
        }
        kept.push((from, to));
    }
    kept
}

// The finder reads at most MAX_WORDS words at once, so a long line is read in windows that overlap
// by MAX_WIDTH words: a name late in a long paragraph is found, and one across a seam is whole in
// the next window.
fn windows(line: &str) -> Vec<(usize, usize)> {
    let spans = words(line);
    let mut found = Vec::new();
    let mut first = 0;
    while first < spans.len() {
        let last = (first + MAX_WORDS).min(spans.len());
        found.push((spans[first].0, spans[last - 1].1));
        if last == spans.len() {
            break;
        }
        first = last - MAX_WIDTH;
    }
    found
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
        let pieces: Vec<(usize, usize, &str)> = lines
            .iter()
            .enumerate()
            .flat_map(|(at, line)| {
                windows(line)
                    .into_iter()
                    .map(move |(from, to)| (at, from, &line[from..to]))
            })
            .collect();
        for batch in pieces.chunks(BATCH) {
            let texts: Vec<&str> = batch.iter().map(|&(_, _, text)| text).collect();
            for (&(at, from, _), hits) in batch.iter().zip(self.batch(&texts, labels)?) {
                found[at].extend(
                    hits.into_iter()
                        .map(|(start, end, kind)| (from + start, from + end, kind)),
                );
            }
        }
        for hits in &mut found {
            hits.sort_unstable();
            hits.dedup();
        }
        Ok(found)
    }

    fn batch(&self, lines: &[&str], labels: &[(String, String)]) -> Result<Vec<Vec<Hit>>, String> {
        let threshold = rules()?.threshold;
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
                for start in (0..spans.len()).filter(|&start| score(start, 0) >= threshold) {
                    for end in start..spans.len().min(start + MAX_WIDTH) {
                        if score(end, 1) < threshold {
                            continue;
                        }
                        let inside: Vec<f32> = (start..=end).map(|word| score(word, 2)).collect();
                        if inside.iter().all(|&value| value >= threshold) {
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
    fn a_long_line_is_read_in_overlapping_windows_to_its_end() {
        assert!(windows("  ").is_empty());
        assert_eq!(windows("one two"), [(0, 7)]);
        let line = (0..450)
            .map(|word| format!("w{word}"))
            .collect::<Vec<_>>()
            .join(" ");
        let found = windows(&line);
        assert_eq!(found.len(), 3);
        assert!(line[found[0].0..found[0].1].ends_with("w199"));
        assert!(line[found[1].0..found[1].1].starts_with(&format!("w{}", MAX_WORDS - MAX_WIDTH)));
        assert_eq!(found[2].1, line.len());
    }

    #[test]
    fn a_hit_keeps_only_what_can_be_a_name() {
        let named = |line: &str, piece: &str| -> Vec<String> {
            let start = line.find(piece).unwrap();
            cleaned(line, start, start + piece.len())
                .into_iter()
                .map(|(from, to)| line[from..to].to_string())
                .collect()
        };
        let line = "zijn. Als de proef in Rotterdam uitloopt, ten name van Brouwer & Smit ICT.";
        assert!(named(line, "Als de proef").is_empty());
        assert_eq!(named(line, "van Brouwer & Smit"), ["Brouwer & Smit"]);
        let line = "het adres in het beheernetwerk van Hendriks. Monteurs gebruiken het";
        for ordinary in ["adres", "beheernetwerk", "het adres"] {
            assert!(named(line, ordinary).is_empty(), "{ordinary}");
        }
        assert_eq!(named(line, "Hendriks. Monteurs"), ["Hendriks", "Monteurs"]);
        assert!(named("Inrichting, 340 uur à € 95", "340 uur").is_empty());
        assert!(named("De meetdata", "De").is_empty());
        assert_eq!(
            named("met het Microsoft account van", "het Microsoft account"),
            ["Microsoft"]
        );
        assert_eq!(
            named("Proef in regio Rotterdam", "regio Rotterdam"),
            ["Rotterdam"]
        );
        assert_eq!(
            named("Fatima El Amrani is de enige", "Fatima El Amrani"),
            ["Fatima El Amrani"]
        );
        assert_eq!(named("التقى أحمد بسارة", "أحمد"), ["أحمد"]);
    }

    #[test]
    #[ignore = "needs the downloaded model in ZNOTE_NAME_FINDER"]
    fn the_downloaded_model_finds_people_and_places() {
        let folder = std::env::var("ZNOTE_NAME_FINDER").unwrap();
        let finder = Finder::load(Path::new(&folder)).unwrap();
        let labels = rules().unwrap().labels.clone();
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

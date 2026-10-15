use std::collections::{BTreeMap, HashSet};
use std::ops::RangeInclusive;
use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;

const DETECTION: &str = include_str!("../../resources/detection.json");
const LANGUAGES: &str = include_str!(concat!(env!("OUT_DIR"), "/languages.json"));

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Language {
    #[serde(rename = "$schema", default)]
    _schema: Option<String>,
    pub code: String,
    #[serde(rename = "name")]
    _name: String,
    stopwords: Vec<String>,
    suffixes: Vec<String>,
    prefixes: Vec<String>,
    sentence_ends: Vec<String>,
    scale_words: Vec<String>,
    phone_trunks: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Check {
    Iban,
    Luhn,
    Ipv4,
    Ipv6,
    Phone,
    Amount,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PatternSource {
    kind: String,
    regex: String,
    check: Option<Check>,
    #[serde(default)]
    trim: String,
    #[serde(default)]
    amounts: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FinderSource {
    threshold: f32,
    labels: Vec<(String, String)>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Detection {
    #[serde(rename = "$schema", default)]
    _schema: Option<String>,
    ignorable: Vec<String>,
    spaces: Vec<char>,
    currency_codes: Vec<String>,
    currency_symbols: Vec<String>,
    number: String,
    patterns: Vec<PatternSource>,
    finder: FinderSource,
    levels: BTreeMap<String, u8>,
    default_level: u8,
}

pub struct Pattern {
    pub kind: String,
    pub regex: Regex,
    pub check: Option<Check>,
    pub trim: Vec<char>,
    pub amounts: bool,
}

pub struct Rules {
    pub languages: Vec<Language>,
    ignorable: Vec<RangeInclusive<u32>>,
    spaces: Vec<char>,
    stopwords: HashSet<String>,
    suffixes: Vec<String>,
    prefixes: Vec<String>,
    pub sentence_ends: Vec<String>,
    pub patterns: Vec<Pattern>,
    pub threshold: f32,
    pub labels: Vec<(String, String)>,
    levels: BTreeMap<String, u8>,
    default_level: u8,
}

fn code_point(hex: &str) -> Result<u32, String> {
    u32::from_str_radix(hex, 16).map_err(|_| format!("“{hex}” is not a code point"))
}

fn range(text: &str) -> Result<RangeInclusive<u32>, String> {
    match text.split_once('-') {
        Some((from, to)) => Ok(code_point(from)?..=code_point(to)?),
        None => code_point(text).map(|one| one..=one),
    }
}

fn any_of(words: impl Iterator<Item = String>) -> String {
    let mut words: Vec<String> = words.collect::<HashSet<_>>().into_iter().collect();
    words.sort_by(|a, b| b.chars().count().cmp(&a.chars().count()).then(a.cmp(b)));
    words
        .iter()
        .map(|word| regex::escape(word))
        .collect::<Vec<_>>()
        .join("|")
}

fn lowered(words: &[String]) -> Vec<String> {
    words
        .iter()
        .filter(|word| !word.is_empty())
        .map(|word| word.to_lowercase())
        .collect()
}

impl Rules {
    pub fn parse(detection: &str, languages: &str) -> Result<Self, String> {
        let detection: Detection =
            serde_json::from_str(detection).map_err(|error| format!("detection rules: {error}"))?;
        let languages: Vec<Language> =
            serde_json::from_str(languages).map_err(|error| format!("languages: {error}"))?;
        if languages.is_empty() {
            return Err("there is no language".into());
        }
        let every = |pick: fn(&Language) -> &Vec<String>| {
            languages
                .iter()
                .flat_map(move |language| pick(language).iter().cloned())
        };
        let scale = format!(
            "(?:[ ]?(?:{}))?",
            any_of(every(|language| &language.scale_words))
        );
        let filled = |source: &str| {
            source
                .replace(
                    "{symbols}",
                    &any_of(detection.currency_symbols.iter().cloned()),
                )
                .replace("{codes}", &any_of(detection.currency_codes.iter().cloned()))
                .replace("{number}", &detection.number)
                .replace("{scale}", &scale)
                .replace("{trunk}", &any_of(every(|language| &language.phone_trunks)))
        };
        let patterns = detection
            .patterns
            .into_iter()
            .map(|pattern| {
                Ok(Pattern {
                    regex: Regex::new(&filled(&pattern.regex))
                        .map_err(|error| format!("pattern {}: {error}", pattern.kind))?,
                    kind: pattern.kind,
                    check: pattern.check,
                    trim: pattern.trim.chars().collect(),
                    amounts: pattern.amounts,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        if detection.default_level > 3 || detection.levels.values().any(|level| *level > 3) {
            return Err("a level is above the strictest".into());
        }
        Ok(Self {
            ignorable: detection
                .ignorable
                .iter()
                .map(|text| range(text))
                .collect::<Result<_, _>>()?,
            spaces: detection.spaces,
            stopwords: languages
                .iter()
                .flat_map(|language| lowered(&language.stopwords))
                .collect(),
            suffixes: languages
                .iter()
                .flat_map(|language| lowered(&language.suffixes))
                .collect(),
            prefixes: languages
                .iter()
                .flat_map(|language| lowered(&language.prefixes))
                .collect(),
            sentence_ends: every(|language| &language.sentence_ends)
                .collect::<HashSet<_>>()
                .into_iter()
                .collect(),
            patterns,
            threshold: detection.finder.threshold,
            labels: detection.finder.labels,
            levels: detection.levels,
            default_level: detection.default_level,
            languages,
        })
    }

    pub fn ignorable(&self, c: char) -> bool {
        let c = u32::from(c);
        self.ignorable.iter().any(|range| range.contains(&c))
    }

    pub fn space(&self, c: char) -> bool {
        self.spaces.contains(&c)
    }

    pub fn stopword(&self, word: &str) -> bool {
        self.stopwords.contains(&word.to_lowercase())
    }

    pub fn suffixes(&self) -> &[String] {
        &self.suffixes
    }

    pub fn prefixes(&self) -> &[String] {
        &self.prefixes
    }

    pub fn level_of(&self, kind: &str) -> u8 {
        self.levels.get(kind).copied().unwrap_or(self.default_level)
    }
}

static RULES: LazyLock<Result<Rules, String>> =
    LazyLock::new(|| Rules::parse(DETECTION, LANGUAGES));

pub fn rules() -> Result<&'static Rules, String> {
    RULES
        .as_ref()
        .map_err(|error| format!("The hiding rules built into znote could not be read: {error}"))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::Path;

    use super::*;

    #[test]
    fn the_built_in_rules_and_every_language_are_read() {
        let rules = rules().unwrap();
        let codes: Vec<&str> = rules
            .languages
            .iter()
            .map(|language| language.code.as_str())
            .collect();
        assert_eq!(codes, ["en", "nl"]);
        assert!(rules.stopword("Het") && rules.stopword("THE") && !rules.stopword("Acme"));
        assert!(rules.ignorable('\u{200B}') && rules.ignorable('\u{AD}') && !rules.ignorable('a'));
        assert!(rules.space('~'));
        assert_eq!(rules.level_of("PERSON"), 1);
        assert_eq!(rules.level_of("CUSTOMER"), 3);
        assert_eq!(
            rules.labels[0],
            ("person".to_string(), "PERSON".to_string())
        );
    }

    #[test]
    fn a_broken_rule_file_is_refused() {
        let languages = include_str!(concat!(env!("OUT_DIR"), "/languages.json"));
        for broken in [
            DETECTION.replace("\"spaces\"", "\"spacing\""),
            DETECTION.replace("\"defaultLevel\": 3", "\"defaultLevel\": 9"),
            DETECTION.replace("[A-Za-z]{2,}", "[A-Za-z]{2,"),
        ] {
            assert!(Rules::parse(&broken, languages).is_err());
        }
        assert!(Rules::parse(DETECTION, "[]").is_err());
        assert!(Rules::parse(DETECTION, &languages.replace("\"stopwords\"", "\"stop\"")).is_err());
    }

    #[test]
    fn words_are_tried_longest_first_once_each_and_escaped() {
        let words = ["k", "million", "m", "k", "$"].map(String::from);
        assert_eq!(any_of(words.into_iter()), "million|\\$|k|m");
        assert_eq!(lowered(&["".into(), "Se".into()]), ["se"]);
    }

    #[test]
    fn code_points_are_read_alone_or_as_a_range_and_unknown_kinds_are_strictest() {
        assert_eq!(range("200B-200F").unwrap(), 0x200B..=0x200F);
        assert_eq!(range("E0000").unwrap(), 0xE0000..=0xE0000);
        assert!(range("zz").is_err() && range("200B-").is_err());
        let rules = rules().unwrap();
        assert!(rules.ignorable('\u{E0041}') && !rules.ignorable('\u{2010}'));
        assert_eq!(rules.level_of("SOMETHING_NEW"), 3);
        assert!(rules.suffixes().contains(&"sche".to_string()));
    }

    fn keys(value: &serde_json::Value) -> BTreeSet<String> {
        value.as_object().unwrap().keys().cloned().collect()
    }

    #[test]
    fn every_built_in_json_file_has_exactly_the_fields_its_schema_names() {
        let resources = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
        let read = |file: &Path| -> serde_json::Value {
            serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap()
        };
        let mut checked = vec![
            (
                "detection.schema.json".to_string(),
                "detection.json".to_string(),
            ),
            ("policy.schema.json".to_string(), "policy.json".to_string()),
        ];
        for entry in fs::read_dir(resources.join("languages")).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.ends_with(".schema.json") {
                checked.push((
                    "languages/language.schema.json".into(),
                    format!("languages/{name}"),
                ));
            }
        }
        assert_eq!(checked.len(), 4);
        for (schema, file) in checked {
            let schema = read(&resources.join(schema));
            let data = read(&resources.join(&file));
            let properties = keys(&schema["properties"]);
            let mut required: BTreeSet<String> = schema["required"]
                .as_array()
                .unwrap()
                .iter()
                .map(|key| key.as_str().unwrap().to_string())
                .collect();
            required.insert("$schema".into());
            assert_eq!(keys(&data), properties, "{file}");
            assert_eq!(required, properties, "{file}");
            if let Some(code) = data["code"].as_str() {
                assert_eq!(format!("languages/{code}.json"), file);
            }
        }
    }
}

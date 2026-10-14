pub mod glossary;
pub mod names;
mod patterns;

use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap, HashSet};

use aho_corasick::{AhoCorasick, MatchKind};
use serde::Serialize;
use typst::syntax::{LinkedNode, Side, SyntaxKind};

pub use glossary::Glossary;
use patterns::Patterns;

const OPEN: char = '⟦';
const CLOSE: char = '⟧';
const MAX_TOKEN: usize = 40;
const MAX_KIND: usize = 16;
const LITERAL: &str = "TOKEN";
const GLOSSARY: u8 = 1;
const PATTERN: u8 = 2;
pub const NAMES: u8 = 3;

pub fn kind_name(label: &str) -> String {
    let mut kind = String::new();
    for c in label.chars() {
        if c.is_ascii_alphanumeric() {
            kind.push(c.to_ascii_uppercase());
        } else if (c.is_whitespace() || c == '-' || c == '_') && !kind.ends_with('_') {
            kind.push('_');
        }
    }
    let kind: String = kind.trim_matches('_').chars().take(MAX_KIND).collect();
    let kind = kind.trim_end_matches('_');
    if kind.is_empty() || kind.starts_with(|c: char| c.is_ascii_digit()) {
        "TERM".into()
    } else {
        kind.into()
    }
}

fn word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

pub fn folded(text: &str) -> (String, Vec<usize>) {
    let mut out = String::with_capacity(text.len());
    let mut origin = Vec::with_capacity(text.len() + 1);
    for (at, c) in text.char_indices() {
        let mut lower = c.to_lowercase();
        let one = match (lower.next(), lower.next()) {
            (Some(single), None) => single,
            _ => c,
        };
        origin.extend(std::iter::repeat_n(at, one.len_utf8()));
        out.push(one);
    }
    origin.push(text.len());
    (out, origin)
}

fn tokens(text: &str) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut open: Option<(usize, usize)> = None;
    for (at, c) in text.char_indices() {
        match c {
            OPEN => open = Some((at, 0)),
            CLOSE => {
                if let Some((start, inside)) = open.take()
                    && (1..=MAX_TOKEN - 2).contains(&inside)
                {
                    found.push((start, at + CLOSE.len_utf8()));
                }
            }
            '\n' => open = None,
            _ => {
                if let Some((_, inside)) = open.as_mut() {
                    *inside += 1;
                }
            }
        }
    }
    found
}

#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub start: usize,
    pub end: usize,
    pub kind: String,
    pub entity: String,
    pub pref: Option<String>,
    pub rank: u8,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Override {
    pub text: String,
    pub kind: Option<String>,
}

struct Term {
    kind: String,
    entity: String,
    pref: Option<String>,
}

pub struct Detector {
    automaton: Option<AhoCorasick>,
    terms: Vec<Term>,
    exempt: HashSet<String>,
    patterns: Patterns,
}

impl Detector {
    pub fn new(glossary: &Glossary, overrides: &[Override]) -> Self {
        let mut labels = Vec::new();
        let mut terms = Vec::new();
        for concept in glossary.concepts.iter().filter(|concept| concept.masked) {
            for label in &concept.labels {
                labels.push(folded(&label.text).0);
                terms.push(Term {
                    kind: concept.kind.clone(),
                    entity: format!("concept:{}", concept.iri),
                    pref: Some(concept.pref().to_string()),
                });
            }
        }
        for added in overrides {
            if let Some(kind) = &added.kind {
                let lower = folded(&added.text).0;
                terms.push(Term {
                    kind: kind_name(kind),
                    entity: format!("override:{lower}"),
                    pref: None,
                });
                labels.push(lower);
            }
        }
        let automaton = AhoCorasick::builder()
            .match_kind(MatchKind::Standard)
            .build(&labels)
            .ok()
            .filter(|_| !labels.is_empty());
        let exempt = overrides
            .iter()
            .filter(|kept| kept.kind.is_none())
            .map(|kept| folded(&kept.text).0)
            .collect();
        Self {
            automaton,
            terms,
            exempt,
            patterns: Patterns::new(),
        }
    }

    pub fn terms(&self, text: &str) -> Vec<Found> {
        let Some(automaton) = &self.automaton else {
            return Vec::new();
        };
        let (lower, origin) = folded(text);
        automaton
            .find_overlapping_iter(&lower)
            .filter_map(|hit| {
                let (start, end) = (origin[hit.start()], origin[hit.end()]);
                let bounded = !text[..start].chars().next_back().is_some_and(word)
                    && !text[end..].chars().next().is_some_and(word)
                    && start < end;
                let term = &self.terms[hit.pattern().as_usize()];
                bounded.then(|| Found {
                    start,
                    end,
                    kind: term.kind.clone(),
                    entity: term.entity.clone(),
                    pref: term.pref.clone(),
                    rank: GLOSSARY,
                })
            })
            .collect()
    }

    pub fn find(&self, text: &str, amounts: bool, names: Vec<Found>) -> Vec<Found> {
        let literals = tokens(text).into_iter().map(|(start, end)| Found {
            start,
            end,
            kind: LITERAL.into(),
            entity: format!("{LITERAL}:{}", &text[start..end]),
            pref: None,
            rank: 0,
        });
        let patterns = self
            .patterns
            .find(text, amounts)
            .into_iter()
            .map(|(start, end, kind)| Found {
                start,
                end,
                kind: kind.into(),
                entity: format!("{kind}:{}", folded(&text[start..end]).0.replace('\\', "")),
                pref: None,
                rank: PATTERN,
            });
        let mut found: Vec<Found> = literals
            .chain(self.terms(text))
            .chain(patterns)
            .chain(names)
            .filter(|hit| hit.end <= text.len() && hit.start < hit.end)
            .filter(|hit| text.is_char_boundary(hit.start) && text.is_char_boundary(hit.end))
            .filter(|hit| !text[hit.start..hit.end].contains('\n'))
            .filter(|hit| {
                hit.rank == 0 || !self.exempt.contains(&folded(&text[hit.start..hit.end]).0)
            })
            .collect();
        found.sort_by_key(|hit| {
            (
                hit.rank != 0,
                Reverse(hit.end - hit.start),
                hit.rank,
                hit.start,
            )
        });
        let mut kept: Vec<Found> = Vec::new();
        for hit in found {
            if kept
                .iter()
                .all(|other| hit.end <= other.start || hit.start >= other.end)
            {
                kept.push(hit);
            }
        }
        kept.sort_by_key(|hit| hit.start);
        kept
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Entry {
    pub token: String,
    pub kind: String,
    pub entity: String,
    pub text: String,
    pub number: u32,
    pub variant: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Restored {
    pub text: String,
    pub kind: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Veiled {
    pub text: String,
    pub kinds: BTreeMap<String, usize>,
    pub marks: Vec<(usize, usize, String)>,
}

#[derive(Default)]
pub struct Vault {
    real: HashMap<String, (String, String)>,
    tokens: HashMap<String, String>,
    entities: HashMap<String, (String, u32, u32)>,
    numbers: HashMap<String, u32>,
}

impl Vault {
    pub fn add(&mut self, entry: &Entry) {
        self.real.insert(
            entry.token.clone(),
            (entry.text.clone(), entry.kind.clone()),
        );
        self.tokens.insert(entry.text.clone(), entry.token.clone());
        let number = self.numbers.entry(entry.kind.clone()).or_default();
        *number = (*number).max(entry.number);
        let next = if entry.entity.starts_with("concept:") {
            1
        } else {
            0
        };
        let known = self
            .entities
            .entry(entry.entity.clone())
            .or_insert_with(|| (entry.kind.clone(), entry.number, next));
        known.2 = known.2.max(entry.variant + 1);
    }

    pub fn len(&self) -> usize {
        self.real.len()
    }

    fn token(&mut self, found: &Found, text: &str) -> (String, Option<Entry>) {
        if let Some(token) = self.tokens.get(text) {
            return (token.clone(), None);
        }
        let (kind, number, next) = match self.entities.get(&found.entity) {
            Some(known) => known.clone(),
            None => {
                let number = self.numbers.get(&found.kind).copied().unwrap_or(0) + 1;
                (found.kind.clone(), number, u32::from(found.pref.is_some()))
            }
        };
        let variant = if found.pref.as_deref() == Some(text) {
            0
        } else {
            next
        };
        let token = if variant == 0 {
            format!("{OPEN}{kind}_{number}{CLOSE}")
        } else {
            format!("{OPEN}{kind}_{number}~{variant}{CLOSE}")
        };
        let entry = Entry {
            token: token.clone(),
            kind,
            entity: found.entity.clone(),
            text: text.into(),
            number,
            variant,
        };
        self.add(&entry);
        (token, Some(entry))
    }

    pub fn veil(&mut self, text: &str, found: &[Found]) -> (Veiled, Vec<Entry>) {
        let mut veiled = Veiled::default();
        let mut fresh = Vec::new();
        let mut at = 0;
        for hit in found {
            veiled.text.push_str(&text[at..hit.start]);
            let (token, entry) = self.token(hit, &text[hit.start..hit.end]);
            fresh.extend(entry);
            veiled.text.push_str(&token);
            *veiled.kinds.entry(hit.kind.clone()).or_default() += 1;
            veiled.marks.push((hit.start, hit.end, token));
            at = hit.end;
        }
        veiled.text.push_str(&text[at..]);
        (veiled, fresh)
    }

    pub fn real(&self, token: &str) -> Option<&(String, String)> {
        self.real.get(token)
    }

    pub fn unveil(&self, text: &str) -> (String, Vec<Restored>) {
        let mut out = String::with_capacity(text.len());
        let mut restored = Vec::new();
        let mut at = 0;
        for (start, end) in tokens(text) {
            let Some((real, kind)) = self.real.get(&text[start..end]) else {
                continue;
            };
            out.push_str(&text[at..start]);
            out.push_str(real);
            restored.push(Restored {
                text: real.clone(),
                kind: kind.clone(),
            });
            at = end;
        }
        out.push_str(&text[at..]);
        (out, restored)
    }
}

fn escaped(real: &str, special: impl Fn(char, Option<char>) -> bool) -> String {
    let mut out = String::with_capacity(real.len() + 4);
    let mut chars = real.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            out.push(c);
            out.push(chars.next().unwrap_or('\\'));
            continue;
        }
        if special(c, chars.peek().copied()) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn markup(c: char, next: Option<char>) -> bool {
    matches!(c, '#' | '*' | '_' | '$' | '`' | '@' | '<' | '[' | ']' | '~')
        || c == '/' && matches!(next, Some('/' | '*'))
}

fn place(root: &LinkedNode, at: usize) -> Option<SyntaxKind> {
    let mut node = root.leaf_at(at, Side::After);
    while let Some(here) = node {
        match here.kind() {
            kind @ (SyntaxKind::Markup | SyntaxKind::Str) => return Some(kind),
            SyntaxKind::Raw
            | SyntaxKind::Equation
            | SyntaxKind::Math
            | SyntaxKind::Code
            | SyntaxKind::CodeBlock
            | SyntaxKind::Link
            | SyntaxKind::Label
            | SyntaxKind::Ref
            | SyntaxKind::LineComment
            | SyntaxKind::BlockComment => return None,
            _ => node = here.parent().cloned(),
        }
    }
    None
}

impl Vault {
    /// Unveils Typst source: real text that lands in markup or in a string is escaped there, so an
    /// address like a@b.example does not turn into a reference. Text that is already escaped stays as
    /// it is, so a page read and written back comes out the same.
    pub fn unveil_typst(&self, text: &str) -> String {
        let known: Vec<(usize, usize, &str)> = tokens(text)
            .into_iter()
            .filter_map(|(start, end)| {
                Some((start, end, self.real.get(&text[start..end])?.0.as_str()))
            })
            .collect();
        let mut blank = text.to_string();
        for &(start, end, _) in &known {
            blank.replace_range(start..end, &"a".repeat(end - start));
        }
        let parsed = typst::syntax::parse(&blank);
        let root = LinkedNode::new(&parsed);
        let mut out = String::with_capacity(text.len());
        let mut at = 0;
        for (start, end, real) in known {
            out.push_str(&text[at..start]);
            let link = real.starts_with("http://") || real.starts_with("https://");
            match place(&root, start) {
                Some(SyntaxKind::Markup) if !link => out.push_str(&escaped(real, markup)),
                Some(SyntaxKind::Str) => out.push_str(&escaped(real, |c, _| c == '"')),
                _ => out.push_str(real),
            }
            at = end;
        }
        out.push_str(&text[at..]);
        out
    }
}

#[derive(Default)]
pub struct Stream {
    held: String,
}

impl Stream {
    pub fn push(&mut self, piece: &str) -> String {
        self.held.push_str(piece);
        let keep = self
            .held
            .rfind(OPEN)
            .filter(|&open| {
                let tail = &self.held[open..];
                !tail.contains([CLOSE, '\n']) && tail.chars().count() < MAX_TOKEN
            })
            .unwrap_or(self.held.len());
        let rest = self.held.split_off(keep);
        std::mem::replace(&mut self.held, rest)
    }

    pub fn finish(&mut self) -> String {
        std::mem::take(&mut self.held)
    }
}

#[cfg(test)]
mod tests {
    use super::glossary::{Concept, Label};
    use super::*;

    struct Random(u64);

    impl Random {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, limit: usize) -> usize {
            (self.next() % limit as u64) as usize
        }

        fn pick<'a>(&mut self, from: &[&'a str]) -> &'a str {
            from[self.below(from.len())]
        }
    }

    fn concept(iri: &str, kind: &str, labels: &[&str]) -> Concept {
        Concept {
            iri: iri.into(),
            kind: kind.into(),
            labels: labels
                .iter()
                .map(|text| Label {
                    text: text.to_string(),
                    hidden: false,
                })
                .collect(),
            masked: true,
            top: false,
            notes: Vec::new(),
        }
    }

    fn acme() -> Glossary {
        Glossary {
            concepts: vec![
                concept("urn:acme", "CUSTOMER", &["Acme Corporation", "Acme"]),
                concept("urn:jan", "PERSON", &["Jan de Vries"]),
                concept("urn:ijs", "PRODUCT", &["IJsbreker"]),
            ],
            edges: Vec::new(),
        }
    }

    fn veil(vault: &mut Vault, detector: &Detector, text: &str) -> Veiled {
        let found = detector.find(text, true, Vec::new());
        vault.veil(text, &found).0
    }

    #[test]
    fn a_term_and_its_spellings_become_one_customer_with_variants() {
        let detector = Detector::new(&acme(), &[]);
        let mut vault = Vault::default();
        let text = "ACME and Acme Corporation, not Acmeville. acme corporation pays.";
        let veiled = veil(&mut vault, &detector, text);
        assert_eq!(
            veiled.text,
            "⟦CUSTOMER_1~1⟧ and ⟦CUSTOMER_1⟧, not Acmeville. ⟦CUSTOMER_1~2⟧ pays."
        );
        assert_eq!(veiled.kinds["CUSTOMER"], 3);
        assert_eq!(vault.unveil(&veiled.text).0, text);
        let again = veil(
            &mut vault,
            &detector,
            "Acme, Jan de Vries and jan@acme.example",
        );
        assert_eq!(again.text, "⟦CUSTOMER_1~3⟧, ⟦PERSON_1⟧ and ⟦EMAIL_1⟧");
        let (_, restored) = vault.unveil("⟦PERSON_1⟧ wrote");
        assert_eq!(
            restored,
            [Restored {
                text: "Jan de Vries".into(),
                kind: "PERSON".into()
            }]
        );
    }

    #[test]
    fn tokens_are_never_reused_and_survive_a_reload_of_the_vault() {
        let detector = Detector::new(&acme(), &[]);
        let mut vault = Vault::default();
        let (_, fresh) = vault.veil(
            "Jan de Vries at Acme",
            &detector.find("Jan de Vries at Acme", true, Vec::new()),
        );
        let mut reloaded = Vault::default();
        fresh.iter().for_each(|entry| reloaded.add(entry));
        let text = "Acme Corporation and Jan de Vries and IJsbreker and Acme";
        let first = veil(&mut vault, &detector, text);
        let second = veil(&mut reloaded, &detector, text);
        assert_eq!(first, second);
        assert_eq!(
            first.text,
            "⟦CUSTOMER_1⟧ and ⟦PERSON_1⟧ and ⟦PRODUCT_1⟧ and ⟦CUSTOMER_1~1⟧"
        );
    }

    #[test]
    fn unknown_and_mangled_tokens_are_left_alone() {
        let detector = Detector::new(&acme(), &[]);
        let mut vault = Vault::default();
        veil(&mut vault, &detector, "Acme");
        for odd in [
            "⟦CUSTOMER_9⟧",
            "⟦CUSTOMER 1⟧",
            "⟦customer_1⟧",
            "[CUSTOMER_1]",
            "⟦CUSTOMER_1",
            "⟦⟧",
            "⟦CUSTOMER_\n1⟧",
        ] {
            assert_eq!(vault.unveil(odd).0, odd);
            assert!(vault.unveil(odd).1.is_empty());
        }
        assert_eq!(vault.unveil("⟦x⟦CUSTOMER_1~1⟧").0, "⟦xAcme");
    }

    #[test]
    fn a_page_that_already_holds_token_text_still_comes_back_exactly() {
        let detector = Detector::new(&acme(), &[]);
        let mut vault = Vault::default();
        let text = "Pasted ⟦CUSTOMER_1⟧ before Acme existed, and ⟦ open";
        let veiled = veil(&mut vault, &detector, text);
        assert!(!veiled.text.contains("⟦CUSTOMER_1⟧ before"));
        assert_eq!(vault.unveil(&veiled.text).0, text);
    }

    #[test]
    fn overrides_hide_more_or_keep_a_term_visible() {
        let overrides = [
            Override {
                text: "Acme".into(),
                kind: None,
            },
            Override {
                text: "Blue Harbour".into(),
                kind: Some("Project".into()),
            },
        ];
        let detector = Detector::new(&acme(), &overrides);
        let mut vault = Vault::default();
        let veiled = veil(
            &mut vault,
            &detector,
            "Acme on blue harbour with Acme Corporation",
        );
        assert_eq!(veiled.text, "Acme on ⟦PROJECT_1⟧ with ⟦CUSTOMER_1⟧");
    }

    #[test]
    fn veiling_keeps_every_line_where_it_was() {
        let detector = Detector::new(&acme(), &[]);
        let mut vault = Vault::default();
        let text = "= Acme\nJan de Vries\n\ncall +31 6 1234 5678\nIJsbreker\n";
        let veiled = veil(&mut vault, &detector, text);
        assert_eq!(veiled.text.lines().count(), text.lines().count());
        assert_eq!(
            veiled.text.matches('\n').count(),
            text.matches('\n').count()
        );
    }

    #[test]
    fn veil_then_unveil_gives_back_the_same_bytes_for_random_text_and_glossaries() {
        let mut random = Random(0x2545_f491_4f6c_dd1d);
        let words = [
            "Acme",
            "acme",
            "ACME",
            "Corp",
            "İstanbul",
            "straße",
            "ß",
            "Ǆ",
            "⟦",
            "⟧",
            "⟦X_1⟧",
            "~",
            "_",
            "\n",
            " ",
            "  ",
            ",",
            ".",
            "€",
            "12",
            "+31 6 1234 5678",
            "a@b.co",
            "https://x.example/a.",
            "NL91 ABNA 0417 1643 00",
            "\\$5",
            "$",
            "日本",
            "é",
            "e\u{301}",
            "10.0.0.1",
            "::1",
            "4111 1111 1111 1111",
            "Jan",
            "de",
            "Vries",
        ];
        for round in 0..300 {
            let glossary = Glossary {
                concepts: (0..1 + random.below(6))
                    .map(|index| {
                        let labels: Vec<String> = (0..1 + random.below(3))
                            .map(|_| {
                                (0..1 + random.below(3))
                                    .map(|_| random.pick(&words[..10]).to_string())
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            })
                            .filter(|label| !label.trim().is_empty())
                            .collect();
                        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
                        concept(&format!("urn:{index}"), "THING", &labels)
                    })
                    .collect(),
                edges: Vec::new(),
            };
            let detector = Detector::new(&glossary, &[]);
            let mut vault = Vault::default();
            for _ in 0..3 {
                let text: String = (0..random.below(40)).map(|_| random.pick(&words)).collect();
                let veiled = veil(&mut vault, &detector, &text);
                assert_eq!(
                    vault.unveil(&veiled.text).0,
                    text,
                    "round {round}: {:?}",
                    veiled.text
                );
                assert_eq!(
                    veiled.text.matches('\n').count(),
                    text.matches('\n').count()
                );
            }
        }
    }

    #[test]
    fn a_token_split_at_any_point_of_a_stream_unveils_exactly_once() {
        let detector = Detector::new(&acme(), &[]);
        let mut vault = Vault::default();
        let veiled = veil(&mut vault, &detector, "Acme Corporation and Acme");
        let reply = format!("Hi {} ⟦NOPE_1⟧ ⟦ok", veiled.text);
        let whole = vault.unveil(&reply).0;
        for first in 0..=reply.len() {
            for second in first..=reply.len() {
                if !(reply.is_char_boundary(first) && reply.is_char_boundary(second)) {
                    continue;
                }
                let mut stream = Stream::default();
                let mut shown = String::new();
                for piece in [&reply[..first], &reply[first..second], &reply[second..]] {
                    let released = stream.push(piece);
                    assert!(!released.ends_with("⟦CUSTOMER"), "{released}");
                    shown.push_str(&vault.unveil(&released).0);
                }
                shown.push_str(&vault.unveil(&stream.finish()).0);
                assert_eq!(shown, whole);
            }
        }
        assert_eq!(whole, "Hi Acme Corporation and Acme ⟦NOPE_1⟧ ⟦ok");
    }

    #[test]
    fn typst_gets_real_text_escaped_for_where_the_token_stands() {
        let detector = Detector::new(&acme(), &[]);
        let mut vault = Vault::default();
        let text = "jan_x@acme.example, \\$12 and https://acme.example/a_b";
        let veiled = veil(&mut vault, &detector, text);
        let [mail, amount, url] = ["jan_x@acme.example", "\\$12", "https://acme.example/a_b"]
            .map(|real| vault.tokens[real].clone());
        let page = format!(
            "= {mail}\n[{mail}] #link(\"mailto:{mail}\")[{amount}] `{mail}` $x$ {url} // {mail}\n"
        );
        assert_eq!(
            vault.unveil_typst(&page),
            "= jan\\_x\\@acme.example\n[jan\\_x\\@acme.example] #link(\"mailto:jan_x@acme.example\")[\\$12] `jan_x@acme.example` $x$ https://acme.example/a_b // jan_x@acme.example\n"
        );
        let escaped = vault.unveil_typst(&format!("Mail {mail}"));
        let again = veil(&mut vault, &detector, &escaped);
        assert_eq!(again.text, format!("Mail {}", mail.replace('⟧', "~1⟧")));
        assert_eq!(vault.unveil_typst(&again.text), escaped);
        assert_eq!(vault.unveil(&veiled.text).0, text);
    }

    #[test]
    fn a_long_bracket_that_never_closes_is_not_held_back_forever() {
        let mut stream = Stream::default();
        assert_eq!(stream.push("start ⟦"), "start ");
        let released = stream.push(&"x".repeat(MAX_TOKEN));
        assert!(released.starts_with('⟦'));
        assert_eq!(stream.finish(), "");
    }

    #[test]
    fn kinds_are_short_plain_upper_case_words() {
        assert_eq!(kind_name("Customer"), "CUSTOMER");
        assert_eq!(kind_name("Key account – EMEA"), "KEY_ACCOUNT_EMEA");
        assert_eq!(kind_name("Société"), "SOCIT");
        assert_eq!(
            kind_name("Very long category name here"),
            "VERY_LONG_CATEGO"
        );
        assert_eq!(kind_name("日本"), "TERM");
        assert_eq!(kind_name("2024 plan"), "TERM");
    }

    #[test]
    fn veiling_a_long_page_with_a_large_glossary_is_quick() {
        let glossary = Glossary {
            concepts: (0..10_000)
                .map(|index| {
                    concept(
                        &format!("urn:{index}"),
                        "CUSTOMER",
                        &[
                            format!("Customer number {index} Holding").as_str(),
                            format!("CN{index}").as_str(),
                        ],
                    )
                })
                .collect(),
            edges: Vec::new(),
        };
        let detector = Detector::new(&glossary, &[]);
        let page: String = (0..200)
            .map(|line| {
                format!("Line {line}: Customer number {line} Holding met CN{} about € 1.200 and mail x{line}@y.example.\n", line * 7)
            })
            .collect();
        let mut vault = Vault::default();
        veil(&mut vault, &detector, &page);
        let started = std::time::Instant::now();
        let mut fresh = Vault::default();
        let veiled = veil(&mut fresh, &detector, &page);
        let took = started.elapsed();
        assert_eq!(veiled.kinds["CUSTOMER"], 400);
        let limit = if cfg!(debug_assertions) { 250 } else { 5 };
        assert!(took.as_millis() < limit, "{took:?}");
    }
}

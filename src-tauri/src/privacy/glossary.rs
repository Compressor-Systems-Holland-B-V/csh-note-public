use std::collections::{HashMap, HashSet};

use oxrdf::Term;
use oxrdfio::{RdfFormat, RdfParser, RdfSyntaxError};

use super::kind_name;

pub const MAX_BYTES: u64 = 20 * 1024 * 1024;
const MAX_XML_BYTES: usize = 4 * 1024 * 1024;
const MAX_TRIPLES: usize = 200_000;
const MAX_LABEL: usize = 200;
const SKOS: &str = "http://www.w3.org/2004/02/skos/core#";
const ZNOTE: &str = "urn:znote:skos#";
const TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const DEFAULT: &str = include_str!("../../resources/glossary.ttl");
const LOOSE: &str = "TERM";

#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    pub text: String,
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Concept {
    pub iri: String,
    pub kind: String,
    pub labels: Vec<Label>,
    pub masked: bool,
    pub top: bool,
    pub notes: Vec<String>,
}

impl Concept {
    pub fn pref(&self) -> &str {
        self.labels
            .first()
            .map(|label| label.text.as_str())
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    pub from: usize,
    pub to: usize,
    pub relation: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Glossary {
    pub concepts: Vec<Concept>,
    pub edges: Vec<Edge>,
}

#[derive(Default)]
struct Node {
    concept: bool,
    declared_top: bool,
    pref: Vec<(String, String)>,
    alt: Vec<String>,
    hidden: Vec<String>,
    broader: Vec<String>,
    related: Vec<String>,
    notes: Vec<String>,
    unmasked: bool,
}

fn failed(syntax: RdfSyntaxError) -> String {
    match syntax.location() {
        Some(place) => format!(
            "The glossary could not be read at line {}: {syntax}",
            place.start.line + 1
        ),
        None => format!("The glossary could not be read: {syntax}"),
    }
}

fn label(text: &str) -> Result<String, String> {
    let text = text.trim();
    if text.chars().count() > MAX_LABEL {
        let start: String = text.chars().take(40).collect();
        return Err(format!(
            "The label “{start}…” is longer than {MAX_LABEL} characters."
        ));
    }
    if text.contains(['\n', '\r']) {
        return Err(format!("The label “{text}” has a line break in it."));
    }
    Ok(text.to_string())
}

pub fn format_of(name: &str) -> Result<RdfFormat, String> {
    let lower = name.to_lowercase();
    if lower.ends_with(".ttl") {
        Ok(RdfFormat::Turtle)
    } else if [".rdf", ".xml", ".owl"]
        .iter()
        .any(|end| lower.ends_with(end))
    {
        Ok(RdfFormat::RdfXml)
    } else {
        Err("Choose a SKOS glossary in Turtle (.ttl) or RDF/XML (.rdf).".into())
    }
}

pub fn parse(bytes: &[u8], format: RdfFormat) -> Result<Glossary, String> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err("The glossary is larger than 20 MB.".into());
    }
    if format == RdfFormat::RdfXml && bytes.len() > MAX_XML_BYTES {
        return Err("An RDF/XML glossary can be at most 4 MB. Save it as Turtle (.ttl) to use a larger one.".into());
    }
    let mut nodes: HashMap<String, Node> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut node = |name: &str, nodes: &mut HashMap<String, Node>| {
        if !nodes.contains_key(name) {
            order.push(name.to_string());
            nodes.insert(name.to_string(), Node::default());
        }
    };
    for (count, quad) in RdfParser::from_format(format).for_slice(bytes).enumerate() {
        if count >= MAX_TRIPLES {
            return Err("The glossary has more than 200 000 statements.".into());
        }
        let quad = quad.map_err(failed)?;
        let subject = quad.subject.to_string();
        let predicate = quad.predicate.as_str();
        let object = match &quad.object {
            Term::NamedNode(named) => named.to_string(),
            Term::BlankNode(blank) => blank.to_string(),
            Term::Literal(literal) => literal.value().to_string(),
            #[allow(unreachable_patterns)]
            _ => continue,
        };
        let language = match &quad.object {
            Term::Literal(literal) => literal.language().unwrap_or_default().to_string(),
            _ => String::new(),
        };
        let Some(local) = predicate
            .strip_prefix(SKOS)
            .or_else(|| predicate.strip_prefix(ZNOTE).map(|_| "mask"))
            .or_else(|| (predicate == TYPE).then_some("type"))
        else {
            continue;
        };
        node(&subject, &mut nodes);
        match local {
            "type" if object == format!("<{SKOS}Concept>") => {
                nodes.entry(subject).or_default().concept = true;
            }
            "prefLabel" => nodes
                .entry(subject)
                .or_default()
                .pref
                .push((label(&object)?, language)),
            "altLabel" => nodes.entry(subject).or_default().alt.push(label(&object)?),
            "hiddenLabel" => nodes
                .entry(subject)
                .or_default()
                .hidden
                .push(label(&object)?),
            "broader" => nodes.entry(subject).or_default().broader.push(object),
            "narrower" => {
                node(&object, &mut nodes);
                nodes.entry(object).or_default().broader.push(subject);
            }
            "related" => nodes.entry(subject).or_default().related.push(object),
            "topConceptOf" => {
                let found = nodes.entry(subject).or_default();
                found.declared_top = true;
                found.concept = true;
            }
            "hasTopConcept" => {
                node(&object, &mut nodes);
                let found = nodes.entry(object).or_default();
                found.declared_top = true;
                found.concept = true;
            }
            "definition" | "scopeNote" => nodes.entry(subject).or_default().notes.push(object),
            "mask" => {
                nodes.entry(subject).or_default().unmasked = matches!(object.trim(), "false" | "0");
            }
            _ => {}
        }
    }
    Ok(build(&nodes, &order))
}

fn preferred(node: &Node) -> Option<&str> {
    let languages = super::rules()
        .map(|rules| rules.languages.as_slice())
        .unwrap_or_default();
    languages
        .iter()
        .find_map(|known| {
            node.pref
                .iter()
                .find(|(_, language)| language.split('-').next() == Some(known.code.as_str()))
        })
        .or(node.pref.first())
        .map(|(text, _)| text.as_str())
}

fn build(nodes: &HashMap<String, Node>, order: &[String]) -> Glossary {
    let has_children: HashSet<&str> = nodes
        .values()
        .flat_map(|node| node.broader.iter().map(String::as_str))
        .collect();
    let named: Vec<&String> = order
        .iter()
        .filter(|name| {
            nodes.get(*name).is_some_and(|node| {
                (node.concept
                    || node.declared_top
                    || has_children.contains(name.as_str())
                    || !node.broader.is_empty())
                    && preferred(node).is_some()
            })
        })
        .collect();
    let is_top = |name: &str| {
        nodes.get(name).is_some_and(|node| {
            node.declared_top || node.broader.is_empty() && has_children.contains(name)
        })
    };
    let kind_of = |start: &str| {
        let mut seen = HashSet::new();
        let mut queue = vec![start.to_string()];
        while let Some(name) = queue.pop() {
            if !seen.insert(name.clone()) {
                continue;
            }
            let Some(node) = nodes.get(&name) else {
                continue;
            };
            if name != start && is_top(&name) {
                return preferred(node).map(kind_name);
            }
            queue.extend(node.broader.iter().rev().cloned());
        }
        None
    };
    let index: HashMap<&str, usize> = named
        .iter()
        .enumerate()
        .map(|(at, name)| (name.as_str(), at))
        .collect();
    let concepts = named
        .iter()
        .filter_map(|name| {
            let node = nodes.get(*name)?;
            let top = is_top(name);
            let pref = preferred(node)?.to_string();
            let mut labels = vec![Label {
                text: pref.clone(),
                hidden: false,
            }];
            let spellings = node
                .pref
                .iter()
                .map(|(text, _)| (text, false))
                .chain(node.alt.iter().map(|text| (text, false)))
                .chain(node.hidden.iter().map(|text| (text, true)));
            for (text, hidden) in spellings {
                if !text.is_empty() && !labels.iter().any(|known| known.text == *text) {
                    labels.push(Label {
                        text: text.clone(),
                        hidden,
                    });
                }
            }
            Some(Concept {
                iri: name.to_string(),
                kind: if top {
                    kind_name(&pref)
                } else {
                    kind_of(name).unwrap_or_else(|| LOOSE.into())
                },
                labels,
                masked: !top && !node.unmasked,
                top,
                notes: node.notes.clone(),
            })
        })
        .collect();
    let mut edges = Vec::new();
    for name in &named {
        let Some(node) = nodes.get(*name) else {
            continue;
        };
        let from = index[name.as_str()];
        for (targets, relation) in [(&node.broader, "broader"), (&node.related, "related")] {
            for target in targets {
                if let Some(&to) = index.get(target.as_str()) {
                    edges.push(Edge {
                        from,
                        to,
                        relation: relation.into(),
                    });
                }
            }
        }
    }
    Glossary { concepts, edges }
}

pub fn default() -> Glossary {
    parse(DEFAULT.as_bytes(), RdfFormat::Turtle).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
@prefix skos: <http://www.w3.org/2004/02/skos/core#> .
@prefix znote: <urn:znote:skos#> .
@prefix ex: <https://example.org/> .

ex:scheme a skos:ConceptScheme ; skos:hasTopConcept ex:customer .
ex:customer a skos:Concept ; skos:prefLabel "Kunde"@de, "Customer"@en .
ex:acme a skos:Concept ;
  skos:prefLabel "Acme Corporation"@en ;
  skos:altLabel "Acme" ;
  skos:hiddenLabel "ACM-01" ;
  skos:broader ex:emea ;
  skos:related ex:phoenix ;
  skos:definition "Pays for maintenance." .
ex:emea a skos:Concept ; skos:prefLabel "Customers in Europe" ; skos:broader ex:customer .
ex:phoenix a skos:Concept ; skos:prefLabel "Phoenix" ; znote:mask false .
ex:project skos:prefLabel "Project" ; skos:narrower ex:phoenix .
"#;

    fn sample() -> Glossary {
        parse(SAMPLE.as_bytes(), RdfFormat::Turtle).unwrap()
    }

    fn concept<'a>(glossary: &'a Glossary, pref: &str) -> &'a Concept {
        glossary
            .concepts
            .iter()
            .find(|concept| concept.pref() == pref)
            .unwrap()
    }

    #[test]
    fn kinds_come_from_the_top_concept_found_by_following_broader_up() {
        let glossary = sample();
        let acme = concept(&glossary, "Acme Corporation");
        assert_eq!(acme.kind, "CUSTOMER");
        assert!(acme.masked && !acme.top);
        assert_eq!(concept(&glossary, "Customers in Europe").kind, "CUSTOMER");
        let customer = concept(&glossary, "Customer");
        assert!(customer.top && !customer.masked);
        assert_eq!(customer.labels.len(), 2);
        assert_eq!(concept(&glossary, "Phoenix").kind, "PROJECT");
        assert!(concept(&glossary, "Project").top);
        assert_eq!(acme.notes, ["Pays for maintenance."]);
    }

    #[test]
    fn znote_mask_false_and_hidden_labels_are_kept_apart() {
        let glossary = sample();
        assert!(!concept(&glossary, "Phoenix").masked);
        let acme = concept(&glossary, "Acme Corporation");
        assert_eq!(
            acme.labels,
            [
                Label {
                    text: "Acme Corporation".into(),
                    hidden: false
                },
                Label {
                    text: "Acme".into(),
                    hidden: false
                },
                Label {
                    text: "ACM-01".into(),
                    hidden: true
                }
            ]
        );
        let relations: Vec<(&str, &str, &str)> = glossary
            .edges
            .iter()
            .map(|edge| {
                (
                    glossary.concepts[edge.from].pref(),
                    edge.relation.as_str(),
                    glossary.concepts[edge.to].pref(),
                )
            })
            .collect();
        assert!(relations.contains(&("Acme Corporation", "broader", "Customers in Europe")));
        assert!(relations.contains(&("Acme Corporation", "related", "Phoenix")));
        assert!(relations.contains(&("Phoenix", "broader", "Project")));
    }

    #[test]
    fn rdf_xml_is_read_as_well() {
        let xml = r#"<?xml version="1.0"?>
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:skos="http://www.w3.org/2004/02/skos/core#">
  <skos:Concept rdf:about="urn:x:person"><skos:prefLabel>Person</skos:prefLabel><skos:topConceptOf rdf:resource="urn:x:s"/></skos:Concept>
  <skos:Concept rdf:about="urn:x:jan"><skos:prefLabel>Jan de Vries</skos:prefLabel><skos:broader rdf:resource="urn:x:person"/></skos:Concept>
</rdf:RDF>"#;
        let glossary = parse(xml.as_bytes(), format_of("names.RDF").unwrap()).unwrap();
        assert_eq!(concept(&glossary, "Jan de Vries").kind, "PERSON");
    }

    #[test]
    fn the_size_limits_and_broken_input_are_refused_with_a_reason() {
        let long = format!(
            "@prefix skos: <http://www.w3.org/2004/02/skos/core#> . <urn:a> skos:prefLabel \"{}\" .",
            "x".repeat(MAX_LABEL + 1)
        );
        assert!(
            parse(long.as_bytes(), RdfFormat::Turtle)
                .unwrap_err()
                .contains("longer than 200")
        );
        let broken = "@prefix skos: <http://www.w3.org/2004/02/skos/core#> .\n\n<urn:a> skos:prefLabel \"open .";
        let error = parse(broken.as_bytes(), RdfFormat::Turtle).unwrap_err();
        assert!(
            error.starts_with("The glossary could not be read at line 3"),
            "{error}"
        );
        let many: String = (0..=MAX_TRIPLES)
            .map(|n| format!("<urn:a> <urn:p> \"{n}\" .\n"))
            .collect();
        assert!(
            parse(many.as_bytes(), RdfFormat::Turtle)
                .unwrap_err()
                .contains("200 000")
        );
        let huge = vec![b' '; MAX_BYTES as usize + 1];
        assert!(
            parse(&huge, RdfFormat::Turtle)
                .unwrap_err()
                .contains("20 MB")
        );
        assert!(
            parse(&vec![b' '; MAX_XML_BYTES + 1], RdfFormat::RdfXml)
                .unwrap_err()
                .contains("4 MB")
        );
        assert!(format_of("glossary.json").is_err());
    }

    #[test]
    fn the_default_glossary_has_categories_only() {
        let glossary = default();
        let mut kinds: Vec<&str> = glossary
            .concepts
            .iter()
            .map(|concept| concept.kind.as_str())
            .collect();
        kinds.sort();
        assert_eq!(
            kinds,
            [
                "CUSTOMER",
                "LOCATION",
                "ORGANISATION",
                "PERSON",
                "PRODUCT",
                "PROJECT",
                "SUPPLIER"
            ]
        );
        assert!(
            glossary
                .concepts
                .iter()
                .all(|concept| concept.top && !concept.masked)
        );
    }
}

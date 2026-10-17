use zsuite_veil::Glossary;
use zsuite_veil::glossary::{RdfFormat, parse};

const DEFAULT: &str = include_str!("../resources/glossary.ttl");

pub fn default_glossary() -> Glossary {
    parse(DEFAULT.as_bytes(), RdfFormat::Turtle).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_glossary_has_categories_only() {
        let glossary = default_glossary();
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

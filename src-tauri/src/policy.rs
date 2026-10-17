use std::sync::LazyLock;

pub use zsuite_veil::policy::Policy;

const BUILT_IN: &str = include_str!("../resources/policy.json");

static POLICY: LazyLock<Policy> =
    LazyLock::new(|| Policy::parse(BUILT_IN).unwrap_or_else(|_| Policy::strictest()));

pub fn get() -> &'static Policy {
    &POLICY
}

#[cfg(test)]
mod tests {
    use super::*;
    use zsuite_veil::policy::{Collection, Rule};

    #[test]
    fn the_built_in_policy_is_read_and_leaves_hiding_to_the_user() {
        let policy = Policy::parse(BUILT_IN).unwrap();
        assert_eq!(policy.hide_names, Rule::Optional);
        assert_eq!(policy.data_collection, Collection::Deny);
        assert!(policy.allows("claude") && policy.web());
        assert_eq!(get(), &policy);
    }

    #[test]
    fn the_built_in_policy_has_exactly_the_fields_its_schema_names() {
        let keys = |value: &serde_json::Value| -> std::collections::BTreeSet<String> {
            value.as_object().unwrap().keys().cloned().collect()
        };
        let schema: serde_json::Value = serde_json::from_str(include_str!(
            "../../vendor/zsuite/crates/zsuite-veil/resources/policy.schema.json"
        ))
        .unwrap();
        let data: serde_json::Value = serde_json::from_str(BUILT_IN).unwrap();
        let mut required: std::collections::BTreeSet<String> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|key| key.as_str().unwrap().to_string())
            .collect();
        required.insert("$schema".into());
        assert_eq!(keys(&data), keys(&schema["properties"]));
        assert_eq!(required, keys(&schema["properties"]));
    }
}

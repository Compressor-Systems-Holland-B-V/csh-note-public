use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

use crate::knowledge::STRICT;

const BUILT_IN: &str = include_str!("../resources/policy.json");

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Rule {
    Optional,
    Required,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Web {
    Allowed,
    Blocked,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Collection {
    Allow,
    Deny,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Policy {
    #[serde(rename = "$schema", default, skip_serializing)]
    schema: Option<String>,
    pub hide_names: Rule,
    pub find_names: Rule,
    pub hide_amounts: Rule,
    pub minimum_level: u8,
    pub web: Web,
    pub providers: Vec<String>,
    pub data_collection: Collection,
    pub zero_retention: bool,
}

impl Policy {
    fn strictest() -> Self {
        Self {
            schema: None,
            hide_names: Rule::Required,
            find_names: Rule::Optional,
            hide_amounts: Rule::Required,
            minimum_level: STRICT,
            web: Web::Blocked,
            providers: Vec::new(),
            data_collection: Collection::Deny,
            zero_retention: true,
        }
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let policy: Self = serde_json::from_str(text).map_err(|error| error.to_string())?;
        if policy.minimum_level > STRICT {
            return Err("minimumLevel is above the strictest level".into());
        }
        Ok(policy)
    }

    pub fn allows(&self, provider: &str) -> bool {
        self.providers.iter().any(|allowed| allowed == provider)
    }

    pub fn web(&self) -> bool {
        self.web == Web::Allowed
    }
}

static POLICY: LazyLock<Policy> =
    LazyLock::new(|| Policy::parse(BUILT_IN).unwrap_or_else(|_| Policy::strictest()));

pub fn get() -> &'static Policy {
    &POLICY
}

#[tauri::command]
pub fn privacy_policy() -> Policy {
    get().clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_policy_is_read_and_leaves_hiding_to_the_user() {
        let policy = Policy::parse(BUILT_IN).unwrap();
        assert_eq!(policy.hide_names, Rule::Optional);
        assert_eq!(policy.data_collection, Collection::Deny);
        assert!(policy.allows("claude") && policy.web());
        assert_eq!(get(), &policy);
    }

    #[test]
    fn a_broken_policy_is_refused_and_an_unreadable_one_hides_everything() {
        for broken in [
            "{}",
            r#"{"hideNames":"maybe"}"#,
            &BUILT_IN.replace("\"minimumLevel\": 0", "\"minimumLevel\": 7"),
            &BUILT_IN.replace("\"web\"", "\"wbe\""),
        ] {
            assert!(Policy::parse(broken).is_err(), "{broken}");
        }
        let fallback = Policy::strictest();
        assert_eq!(fallback.hide_names, Rule::Required);
        assert!(!fallback.web() && !fallback.allows("claude"));
    }
}

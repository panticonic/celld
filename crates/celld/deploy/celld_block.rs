//! The `"celld"` block of a deployment config: celld-only declarations that
//! Wrangler has no word for.
//!
//! ```jsonc
//! "celld": {
//!   "durable_objects": {
//!     "Kernel": { "residency": true, "secrets": ["API_TOKEN"] }
//!   }
//! }
//! ```
//!
//! Each Durable Object class carries a [`ClassCapabilities`]. Every capability
//! is an optional field of that one struct, so independent features (residency,
//! codegen confinement, ...) add a field without touching how the others are
//! read, and an unknown key is a hard error naming the keys that exist:
//! refusing is compat-safe, while guessing produces a deployment that silently
//! lacks a capability the author thought they had.

use anyhow::{anyhow, bail, Context};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// What one Durable Object class is allowed to do beyond the Workers contract.
/// Absent and `false` both mean "not granted".
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassCapabilities {
    /// `ctx.residency.hold()`: keep an object resident until it releases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub residency: Option<bool>,
    /// Codegen confinement is owned by its own feature; the key is accepted
    /// here so a config that sets both parses, and carried through unread.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codegen: Option<bool>,
    /// Names of secrets only this class sees (`docs/fork/class-secrets.md`).
    /// The values are not in the config or the manifest; `celld secret put`
    /// stores them, sealed, apart from every deployment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secrets: Option<Vec<String>>,
}

impl ClassCapabilities {
    pub fn residency(&self) -> bool {
        self.residency == Some(true)
    }
}

/// The parsed `"celld"` block.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CelldDeclaration {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub durable_objects: BTreeMap<String, ClassCapabilities>,
}

impl CelldDeclaration {
    pub fn is_empty(&self) -> bool {
        self.durable_objects.is_empty()
    }

    /// The secrets each class declares, by class, for the classes that declare
    /// any. A name appears under one class only.
    pub fn class_secrets(&self) -> BTreeMap<String, Vec<String>> {
        self.durable_objects
            .iter()
            .filter_map(|(class, caps)| {
                let names = caps.secrets.as_ref().filter(|names| !names.is_empty())?;
                Some((class.clone(), names.clone()))
            })
            .collect()
    }

    /// The classes declared `residency: true`, in name order.
    pub fn residency_classes(&self) -> Vec<String> {
        self.durable_objects
            .iter()
            .filter(|(_, caps)| caps.residency())
            .map(|(class, _)| class.clone())
            .collect()
    }
}

/// Read the config's `celld` block. `do_classes` are the user's Durable Object
/// classes, which are the only ones a block may name.
pub fn read(
    object: &Map<String, Value>,
    do_classes: &[String],
) -> anyhow::Result<CelldDeclaration> {
    let Some(value) = object.get("celld") else {
        return Ok(CelldDeclaration::default());
    };
    if !value.is_object() {
        bail!("config `celld` must be an object");
    }
    let declaration: CelldDeclaration = serde_json::from_value(value.clone())
        .map_err(|error| anyhow!("{error}"))
        .context(
            "config `celld` is invalid (known: `durable_objects`, whose classes take \
             `residency`, `codegen` and `secrets`)",
        )?;
    for (class, caps) in &declaration.durable_objects {
        if crate::deploy::is_reserved_class(class) {
            bail!("config `celld.durable_objects` names {class:?}, a class celld supplies itself");
        }
        if !do_classes.iter().any(|declared| declared == class) {
            bail!(
                "config `celld.durable_objects` names class {class:?}, which \
                 `durable_objects.bindings` does not declare"
            );
        }
        // Serde accepts any JSON for a `bool`-typed field only when it is a
        // boolean, so a string or number has already failed above.
        for name in caps.secrets.iter().flatten() {
            if !crate::secrets::valid_name(name) {
                bail!(
                    "config `celld.durable_objects.{class}.secrets` names {name:?}, which is \
                     not a valid binding name"
                );
            }
        }
    }
    // A secret is one class's. The same name under two classes would be two
    // values behind one stored object, and says nothing about which was meant.
    let mut owner: BTreeMap<&str, &str> = BTreeMap::new();
    for (class, caps) in &declaration.durable_objects {
        for name in caps.secrets.iter().flatten() {
            if let Some(first) = owner.insert(name, class) {
                if first == class.as_str() {
                    bail!(
                        "config `celld.durable_objects.{class}.secrets` lists {name:?} twice"
                    );
                }
                bail!(
                    "secret {name:?} is declared for both {first} and {class}; a secret belongs \
                     to one class"
                );
            }
        }
    }
    Ok(declaration)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(value: Value, classes: &[&str]) -> anyhow::Result<CelldDeclaration> {
        let classes: Vec<String> = classes.iter().map(|class| class.to_string()).collect();
        read(value.as_object().unwrap(), &classes)
    }

    #[test]
    fn absent_block_declares_nothing() {
        let declaration = parse(json!({}), &["Kernel"]).unwrap();
        assert!(declaration.is_empty());
        assert!(declaration.residency_classes().is_empty());
    }

    #[test]
    fn residency_and_codegen_coexist_on_one_class() {
        let declaration = parse(
            json!({ "celld": { "durable_objects": {
                "Kernel": { "residency": true, "codegen": true },
                "Plain": { "codegen": true },
                "Off": { "residency": false }
            } } }),
            &["Kernel", "Plain", "Off"],
        )
        .unwrap();
        assert_eq!(declaration.residency_classes(), vec!["Kernel".to_string()]);
        assert_eq!(declaration.durable_objects["Kernel"].codegen, Some(true));
        assert_eq!(declaration.durable_objects["Plain"].residency, None);
        // Round-trips through the manifest unchanged.
        let json = serde_json::to_value(&declaration).unwrap();
        assert_eq!(
            serde_json::from_value::<CelldDeclaration>(json).unwrap(),
            declaration
        );
    }

    #[test]
    fn class_secrets_are_declared_by_name_for_one_class() {
        let declaration = parse(
            json!({ "celld": { "durable_objects": {
                "Vault": { "secrets": ["API_TOKEN", "DB_URL"] },
                "Plain": {},
                "Empty": { "secrets": [] }
            } } }),
            &["Vault", "Plain", "Empty"],
        )
        .unwrap();
        assert_eq!(
            declaration.class_secrets(),
            BTreeMap::from([(
                "Vault".to_string(),
                vec!["API_TOKEN".to_string(), "DB_URL".to_string()]
            )])
        );
        // Names round-trip through the manifest; values are never in it.
        let json = serde_json::to_value(&declaration).unwrap();
        assert_eq!(json["durable_objects"]["Vault"]["secrets"][0], "API_TOKEN");
        assert_eq!(
            serde_json::from_value::<CelldDeclaration>(json).unwrap(),
            declaration
        );
    }

    #[test]
    fn a_secret_belongs_to_one_class_and_has_a_valid_name() {
        let both = parse(
            json!({ "celld": { "durable_objects": {
                "A": { "secrets": ["TOKEN"] }, "B": { "secrets": ["TOKEN"] }
            } } }),
            &["A", "B"],
        )
        .unwrap_err();
        assert!(format!("{both:#}").contains("belongs to one class"), "{both:#}");
        let twice = parse(
            json!({ "celld": { "durable_objects": { "A": { "secrets": ["TOKEN", "TOKEN"] } } } }),
            &["A"],
        )
        .unwrap_err();
        assert!(format!("{twice:#}").contains("twice"), "{twice:#}");
        let bad = parse(
            json!({ "celld": { "durable_objects": { "A": { "secrets": ["not a name"] } } } }),
            &["A"],
        )
        .unwrap_err();
        assert!(format!("{bad:#}").contains("valid binding name"), "{bad:#}");
        let wrong_type = parse(
            json!({ "celld": { "durable_objects": { "A": { "secrets": "TOKEN" } } } }),
            &["A"],
        );
        assert!(wrong_type.is_err());
    }

    #[test]
    fn an_unknown_capability_is_refused_by_name() {
        let error = parse(
            json!({ "celld": { "durable_objects": { "Kernel": { "residency": true, "teleport": true } } } }),
            &["Kernel"],
        )
        .unwrap_err();
        let text = format!("{error:#}");
        assert!(text.contains("teleport"), "{text}");
        assert!(text.contains("residency"), "{text}");
    }

    #[test]
    fn an_unknown_top_level_key_and_a_non_boolean_are_refused() {
        assert!(parse(json!({ "celld": { "workers": {} } }), &[]).is_err());
        assert!(parse(
            json!({ "celld": { "durable_objects": { "Kernel": { "residency": "yes" } } } }),
            &["Kernel"]
        )
        .is_err());
        assert!(parse(json!({ "celld": [] }), &[]).is_err());
    }

    #[test]
    fn a_class_the_config_does_not_declare_is_refused() {
        let error = parse(
            json!({ "celld": { "durable_objects": { "Ghost": { "residency": true } } } }),
            &["Kernel"],
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("Ghost"));
    }
}

use std::collections::BTreeSet;

use serde_json::{Map, Value};
use skotos_domain::{AttributeRef, Context, SingleContext};

/// Configuration controlling private attribute redaction prior to telemetry transmission.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RedactionConfig {
    /// When true, all attributes except `kind`, `key`, and `anonymous` are redacted.
    pub all_attributes_private: bool,
    /// Global private attribute references applied to every context kind.
    pub global_private_attributes: Vec<String>,
}

/// Redacts private attributes from a [`Context`] before event transmission.
///
/// Rules:
/// - `kind`, `key`, and `anonymous` are NEVER redacted.
/// - `_meta.privateAttributes` and `_meta.secondary` are stripped from output;
///   redacted attribute paths are recorded in `_meta.redactedAttributes` (sorted, deduped).
/// - Both literal top-level names and RFC 6901 JSON Pointer nested paths are supported.
pub fn redact_context(context: &Context, config: &RedactionConfig) -> Value {
    match context {
        Context::Single(single) => redact_single(single, config, true),
        Context::Multi(map) => {
            let mut out = Map::new();
            out.insert("kind".to_string(), Value::String("multi".to_string()));
            for (kind, single) in map {
                out.insert(kind.clone(), redact_single(single, config, false));
            }
            Value::Object(out)
        }
    }
}

fn redact_single(single: &SingleContext, config: &RedactionConfig, include_kind: bool) -> Value {
    let mut obj = Map::new();
    if include_kind {
        obj.insert("kind".to_string(), Value::String(single.kind.clone()));
    }
    obj.insert("key".to_string(), Value::String(single.key.clone()));
    if single.anonymous {
        obj.insert("anonymous".to_string(), Value::Bool(true));
    }

    let mut redacted: BTreeSet<String> = BTreeSet::new();

    if config.all_attributes_private {
        if single.name.is_some() {
            redacted.insert("name".to_string());
        }
        for k in single.attributes.keys() {
            redacted.insert(k.clone());
        }
    } else {
        if let Some(ref name) = single.name {
            obj.insert("name".to_string(), Value::String(name.clone()));
        }
        for (k, v) in &single.attributes {
            obj.insert(k.clone(), v.clone());
        }

        let mut refs: Vec<AttributeRef> = Vec::new();
        for raw in config
            .global_private_attributes
            .iter()
            .chain(single.meta.private_attributes.iter())
        {
            if let Ok(parsed) = AttributeRef::parse(raw) {
                refs.push(parsed);
            }
        }

        // Apply top-level redactions first, then nested JSON pointer redactions
        for attr_ref in &refs {
            let components = attr_ref.components();
            if components.is_empty() {
                continue;
            }
            let first = components[0].as_str();
            if matches!(first, "kind" | "key" | "anonymous" | "_meta") {
                continue;
            }

            if components.len() == 1 {
                if obj.remove(first).is_some() {
                    redacted.insert(attr_ref.raw().to_string());
                }
            } else if remove_nested(&mut obj, components) {
                redacted.insert(attr_ref.raw().to_string());
            }
        }
    }

    if !redacted.is_empty() {
        let mut meta = Map::new();
        meta.insert(
            "redactedAttributes".to_string(),
            Value::Array(redacted.into_iter().map(Value::String).collect()),
        );
        obj.insert("_meta".to_string(), Value::Object(meta));
    }

    Value::Object(obj)
}

fn remove_nested(root: &mut Map<String, Value>, components: &[String]) -> bool {
    let Some(first) = components.first() else {
        return false;
    };
    if components.len() == 1 {
        return root.remove(first).is_some();
    }
    let Some(Value::Object(child)) = root.get_mut(first) else {
        return false;
    };
    remove_nested(child, &components[1..])
}

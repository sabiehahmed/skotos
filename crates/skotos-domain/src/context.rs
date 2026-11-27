use std::collections::BTreeMap;

use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;

use crate::error::DomainError;

/// Validated attribute reference supporting both literal top-level names and
/// `/`-prefixed RFC 6901 JSON Pointer paths.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AttributeRef {
    raw: String,
    components: Vec<String>,
    is_pointer: bool,
}

impl AttributeRef {
    /// Parses an attribute reference string according to LaunchDarkly/Skotos rules:
    /// - Empty string, `"/"`, or trailing `"/"` is invalid.
    /// - Non-`/`-prefixed strings are treated as single literal top-level attribute names.
    /// - `/`-prefixed strings are parsed as RFC 6901 JSON Pointers (`~1` -> `/`, `~0` -> `~`).
    /// - References targeting `_meta` (or any nested path inside `_meta`) are rejected.
    pub fn parse(raw: &str) -> Result<Self, DomainError> {
        if raw.is_empty() {
            return Err(DomainError::InvalidAttributeRef {
                reference: raw.to_string(),
                reason: "attribute reference cannot be empty".to_string(),
            });
        }

        if !raw.starts_with('/') {
            if raw == "_meta" {
                return Err(DomainError::InvalidAttributeRef {
                    reference: raw.to_string(),
                    reason: "cannot reference reserved '_meta' attribute".to_string(),
                });
            }
            return Ok(Self {
                raw: raw.to_string(),
                components: vec![raw.to_string()],
                is_pointer: false,
            });
        }

        if raw == "/" || raw.ends_with('/') {
            return Err(DomainError::InvalidAttributeRef {
                reference: raw.to_string(),
                reason: "JSON pointer cannot be '/' or end with '/'".to_string(),
            });
        }

        let mut components = Vec::new();
        for segment in raw[1..].split('/') {
            if segment.is_empty() {
                return Err(DomainError::InvalidAttributeRef {
                    reference: raw.to_string(),
                    reason: "JSON pointer cannot contain empty path segments".to_string(),
                });
            }
            let unescaped = unescape_json_pointer_segment(segment, raw)?;
            components.push(unescaped);
        }

        if components.first().map(String::as_str) == Some("_meta") {
            return Err(DomainError::InvalidAttributeRef {
                reference: raw.to_string(),
                reason: "cannot reference reserved '_meta' attribute".to_string(),
            });
        }

        Ok(Self {
            raw: raw.to_string(),
            components,
            is_pointer: true,
        })
    }

    pub fn raw(&self) -> &str {
        &self.raw
    }

    pub fn components(&self) -> &[String] {
        &self.components
    }

    pub fn is_pointer(&self) -> bool {
        self.is_pointer
    }
}

fn unescape_json_pointer_segment(segment: &str, raw: &str) -> Result<String, DomainError> {
    let mut out = String::with_capacity(segment.len());
    let mut chars = segment.chars();
    while let Some(c) = chars.next() {
        if c == '~' {
            match chars.next() {
                Some('0') => out.push('~'),
                Some('1') => out.push('/'),
                _ => {
                    return Err(DomainError::InvalidAttributeRef {
                        reference: raw.to_string(),
                        reason: "invalid '~' escape sequence in JSON pointer".to_string(),
                    });
                }
            }
        } else {
            out.push(c);
        }
    }
    Ok(out)
}

/// Validates a context kind string (`[a-zA-Z0-9._-]+`, cannot be `"kind"` or `"multi"`).
pub fn validate_context_kind(kind: &str) -> Result<(), DomainError> {
    if kind.is_empty() {
        return Err(DomainError::InvalidContext(
            "context kind cannot be empty".to_string(),
        ));
    }
    if kind == "kind" || kind == "multi" {
        return Err(DomainError::InvalidContext(format!(
            "context kind cannot be reserved value '{kind}'"
        )));
    }
    if !kind
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return Err(DomainError::InvalidContext(format!(
            "context kind '{kind}' contains invalid characters"
        )));
    }
    Ok(())
}

/// Metadata attached to a single evaluation context (`_meta`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ContextMeta {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub private_attributes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub redacted_attributes: Vec<String>,
}

impl ContextMeta {
    pub fn is_empty(&self) -> bool {
        self.private_attributes.is_empty()
            && self.secondary.is_none()
            && self.redacted_attributes.is_empty()
    }
}

/// A single typed evaluation context (e.g., `kind: "user"`, `key: "usr_123"`).
#[derive(Debug, Clone, PartialEq)]
pub struct SingleContext {
    pub kind: String,
    pub key: String,
    pub name: Option<String>,
    pub anonymous: bool,
    pub attributes: BTreeMap<String, Value>,
    pub meta: ContextMeta,
}

impl SingleContext {
    pub fn new(kind: impl Into<String>, key: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            key: key.into(),
            name: None,
            anonymous: false,
            attributes: BTreeMap::new(),
            meta: ContextMeta::default(),
        }
    }

    pub fn validate(&self) -> Result<(), DomainError> {
        validate_context_kind(&self.kind)?;
        if self.key.is_empty() {
            return Err(DomainError::InvalidContext(format!(
                "context of kind '{}' has an empty key",
                self.kind
            )));
        }
        Ok(())
    }

    /// Resolves an `AttributeRef` against this single context.
    /// Built-in top-level attributes (`kind`, `key`, `name`, `anonymous`) are checked first;
    /// nested paths traverse custom JSON objects.
    pub fn get_attribute(&self, attr_ref: &AttributeRef) -> Option<Value> {
        let components = attr_ref.components();
        let first = components.first()?.as_str();

        if components.len() == 1 {
            return match first {
                "kind" => Some(Value::String(self.kind.clone())),
                "key" => Some(Value::String(self.key.clone())),
                "name" => self.name.as_ref().map(|n| Value::String(n.clone())),
                "anonymous" => Some(Value::Bool(self.anonymous)),
                "_meta" => None,
                other => self.attributes.get(other).cloned(),
            };
        }

        // Multi-segment JSON Pointer traversal only descends into custom JSON object attributes.
        let mut current = match first {
            "kind" | "key" | "name" | "anonymous" | "_meta" => return None,
            other => self.attributes.get(other)?,
        };

        for segment in &components[1..] {
            match current {
                Value::Object(map) => {
                    current = map.get(segment)?;
                }
                _ => return None,
            }
        }
        Some(current.clone())
    }

    fn serialize_fields<S: Serializer>(
        &self,
        serializer: S,
        include_kind: bool,
    ) -> Result<S::Ok, S::Error> {
        let extra_len = usize::from(include_kind)
            + 1 // key
            + usize::from(self.name.is_some())
            + usize::from(self.anonymous)
            + usize::from(!self.meta.is_empty())
            + self.attributes.len();
        let mut map = serializer.serialize_map(Some(extra_len))?;
        if include_kind {
            map.serialize_entry("kind", &self.kind)?;
        }
        map.serialize_entry("key", &self.key)?;
        if let Some(ref name) = self.name {
            map.serialize_entry("name", name)?;
        }
        if self.anonymous {
            map.serialize_entry("anonymous", &true)?;
        }
        for (k, v) in &self.attributes {
            map.serialize_entry(k, v)?;
        }
        if !self.meta.is_empty() {
            map.serialize_entry("_meta", &self.meta)?;
        }
        map.end()
    }
}

impl Serialize for SingleContext {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.serialize_fields(serializer, true)
    }
}

/// Wrapper used when serializing nested contexts inside a multi-context map (omits redundant `kind`).
struct NestedSingleContext<'a>(&'a SingleContext);

impl Serialize for NestedSingleContext<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize_fields(serializer, false)
    }
}

/// Evaluation context supporting both LaunchDarkly single-context and multi-context shapes.
#[derive(Debug, Clone, PartialEq)]
pub enum Context {
    Single(SingleContext),
    Multi(BTreeMap<String, SingleContext>),
}

#[cfg(feature = "openapi")]
impl utoipa::PartialSchema for Context {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        utoipa::openapi::ObjectBuilder::new()
            .schema_type(utoipa::openapi::schema::Type::Object)
            .additional_properties(Some(
                utoipa::openapi::schema::AdditionalProperties::FreeForm(true),
            ))
            .description(Some(
                "Evaluation context: either a single context ({\"kind\":\"user\",\"key\":\"...\",...}) or a multi-context ({\"kind\":\"multi\",\"user\":{...},\"organization\":{...}})",
            ))
            .into()
    }
}

#[cfg(feature = "openapi")]
impl utoipa::ToSchema for Context {}

impl Context {
    pub fn single(ctx: SingleContext) -> Self {
        Self::Single(ctx)
    }

    pub fn multi(contexts: BTreeMap<String, SingleContext>) -> Self {
        Self::Multi(contexts)
    }

    pub fn validate(&self) -> Result<(), DomainError> {
        match self {
            Self::Single(ctx) => ctx.validate(),
            Self::Multi(map) => {
                if map.is_empty() {
                    return Err(DomainError::InvalidContext(
                        "multi-context must contain at least one nested context".to_string(),
                    ));
                }
                for (kind_key, ctx) in map {
                    validate_context_kind(kind_key)?;
                    if ctx.kind != *kind_key {
                        return Err(DomainError::InvalidContext(format!(
                            "multi-context entry '{kind_key}' has mismatched kind '{}'",
                            ctx.kind
                        )));
                    }
                    ctx.validate()?;
                }
                Ok(())
            }
        }
    }

    /// Returns the nested or top-level `SingleContext` matching `kind`, if present.
    pub fn get_kind(&self, kind: &str) -> Option<&SingleContext> {
        match self {
            Self::Single(ctx) if ctx.kind == kind => Some(ctx),
            Self::Single(_) => None,
            Self::Multi(map) => map.get(kind),
        }
    }

    /// Returns all `(kind, SingleContext)` entries in canonical sorted order.
    pub fn kinds(&self) -> Vec<(&str, &SingleContext)> {
        match self {
            Self::Single(ctx) => vec![(ctx.kind.as_str(), ctx)],
            Self::Multi(map) => map.iter().map(|(k, v)| (k.as_str(), v)).collect(),
        }
    }
}

impl Serialize for Context {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Single(ctx) => ctx.serialize(serializer),
            Self::Multi(map) => {
                let mut m = serializer.serialize_map(Some(map.len() + 1))?;
                m.serialize_entry("kind", "multi")?;
                for (kind, ctx) in map {
                    m.serialize_entry(kind, &NestedSingleContext(ctx))?;
                }
                m.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Context {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ContextVisitor;

        impl<'de> Visitor<'de> for ContextVisitor {
            type Value = Context;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a Skotos single or multi evaluation context JSON object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut raw: BTreeMap<String, Value> = BTreeMap::new();
                while let Some((k, v)) = map.next_entry::<String, Value>()? {
                    raw.insert(k, v);
                }

                let kind_val = raw.remove("kind");
                let kind_str = match kind_val {
                    Some(Value::String(s)) => s,
                    Some(_) => {
                        return Err(de::Error::custom("context 'kind' must be a string"));
                    }
                    None => {
                        // Legacy single-user shorthand defaults to kind "user" only when "key" exists
                        if raw.contains_key("key") {
                            "user".to_string()
                        } else {
                            String::new()
                        }
                    }
                };

                if kind_str == "multi" {
                    let mut nested = BTreeMap::new();
                    for (entry_kind, entry_val) in raw {
                        let Value::Object(obj) = entry_val else {
                            return Err(de::Error::custom(format!(
                                "multi-context entry '{entry_kind}' must be a JSON object"
                            )));
                        };
                        let ctx = parse_single_from_map(entry_kind.clone(), obj)
                            .map_err(de::Error::custom)?;
                        nested.insert(entry_kind, ctx);
                    }
                    return Ok(Context::Multi(nested));
                }

                let map_obj: serde_json::Map<String, Value> = raw.into_iter().collect();
                let single = parse_single_from_map(kind_str, map_obj).map_err(de::Error::custom)?;
                Ok(Context::Single(single))
            }
        }

        deserializer.deserialize_map(ContextVisitor)
    }
}

fn parse_single_from_map(
    kind: String,
    mut obj: serde_json::Map<String, Value>,
) -> Result<SingleContext, String> {
    let key = match obj.remove("key") {
        Some(Value::String(s)) => s,
        Some(_) => return Err("context 'key' must be a string".to_string()),
        None => String::new(),
    };

    let name = match obj.remove("name") {
        Some(Value::String(s)) => Some(s),
        Some(Value::Null) | None => None,
        Some(_) => return Err("context 'name' must be a string".to_string()),
    };

    let anonymous = match obj.remove("anonymous") {
        Some(Value::Bool(b)) => b,
        Some(Value::Null) | None => false,
        Some(_) => return Err("context 'anonymous' must be a boolean".to_string()),
    };

    let meta = match obj.remove("_meta") {
        Some(val) if !val.is_null() => serde_json::from_value::<ContextMeta>(val)
            .map_err(|e| format!("invalid '_meta' object: {e}"))?,
        _ => ContextMeta::default(),
    };

    // Strip any redundant inner "kind" field inside multi-context objects
    obj.remove("kind");

    let attributes: BTreeMap<String, Value> = obj.into_iter().collect();

    Ok(SingleContext {
        kind,
        key,
        name,
        anonymous,
        attributes,
        meta,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_attribute_ref_literal_and_json_pointer() {
        let lit = AttributeRef::parse("email").unwrap();
        assert!(!lit.is_pointer());
        assert_eq!(lit.components(), &["email"]);

        let ptr = AttributeRef::parse("/address/country").unwrap();
        assert!(ptr.is_pointer());
        assert_eq!(ptr.components(), &["address", "country"]);

        // RFC 6901 escape order: ~01 -> ~1 (NOT /)
        let escaped = AttributeRef::parse("/a~1b/c~0d/e~01f").unwrap();
        assert_eq!(escaped.components(), &["a/b", "c~d", "e~1f"]);

        assert!(AttributeRef::parse("").is_err());
        assert!(AttributeRef::parse("/").is_err());
        assert!(AttributeRef::parse("/a/").is_err());
        assert!(AttributeRef::parse("/a//b").is_err());
        assert!(AttributeRef::parse("/a~2b").is_err());
        assert!(AttributeRef::parse("_meta").is_err());
        assert!(AttributeRef::parse("/_meta/secondary").is_err());
    }

    #[test]
    fn roundtrips_single_and_multi_context() {
        let raw = json!({
            "kind": "multi",
            "user": {
                "key": "usr_1",
                "name": "Ada",
                "email": "ada@example.com",
                "_meta": { "privateAttributes": ["email"] }
            },
            "organization": {
                "key": "org_42",
                "tier": "enterprise",
                "address": { "country": "US" }
            }
        });
        let ctx: Context = serde_json::from_value(raw.clone()).unwrap();
        ctx.validate().unwrap();
        let org = ctx.get_kind("organization").unwrap();
        assert_eq!(
            org.get_attribute(&AttributeRef::parse("/address/country").unwrap()),
            Some(json!("US"))
        );
        let reserialized = serde_json::to_value(&ctx).unwrap();
        assert_eq!(reserialized, raw);
    }
}

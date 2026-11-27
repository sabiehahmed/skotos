use std::collections::HashMap;

use serde_json::Value;
use skotos_domain::{
    Context, EvaluationDetail, EvaluationErrorKind, EvaluationReason, SingleContext,
};

use crate::client::SkotosClient;

/// OpenFeature-compliant evaluation context input for [`SkotosOpenFeatureProvider`].
#[derive(Debug, Clone, Default)]
pub struct OpenFeatureContext {
    pub targeting_key: Option<String>,
    pub attributes: HashMap<String, Value>,
}

impl OpenFeatureContext {
    pub fn with_targeting_key(key: impl Into<String>) -> Self {
        Self {
            targeting_key: Some(key.into()),
            attributes: HashMap::new(),
        }
    }

    pub fn with_attr(mut self, key: impl Into<String>, value: Value) -> Self {
        self.attributes.insert(key.into(), value);
        self
    }

    pub fn to_skotos_context(&self) -> Context {
        let kind = self
            .attributes
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("user")
            .to_string();
        let key = self
            .targeting_key
            .clone()
            .or_else(|| {
                self.attributes
                    .get("key")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_default();

        let mut single = SingleContext::new(kind, key);
        for (k, v) in &self.attributes {
            if k == "kind" || k == "key" || k == "targetingKey" {
                continue;
            }
            if k == "name" {
                if let Some(s) = v.as_str() {
                    single.name = Some(s.to_string());
                    continue;
                }
            }
            if k == "anonymous" {
                if let Some(b) = v.as_bool() {
                    single.anonymous = b;
                    continue;
                }
            }
            single.attributes.insert(k.clone(), v.clone());
        }
        Context::single(single)
    }
}

/// OpenFeature-standard resolution metadata returned by [`SkotosOpenFeatureProvider`].
#[derive(Debug, Clone, PartialEq)]
pub struct ResolutionDetails<T> {
    pub value: T,
    pub variant: Option<String>,
    pub reason: String,
    pub error_code: Option<String>,
}

/// Official OpenFeature Rust Provider wrapping [`SkotosClient`] (ADR-0004).
#[derive(Clone)]
pub struct SkotosOpenFeatureProvider {
    client: SkotosClient,
}

impl SkotosOpenFeatureProvider {
    pub fn new(client: SkotosClient) -> Self {
        Self { client }
    }

    pub fn metadata_name(&self) -> &'static str {
        "Skotos Rust Server OpenFeature Provider"
    }

    pub fn resolve_bool_value(
        &self,
        flag_key: &str,
        ctx: &OpenFeatureContext,
        default: bool,
    ) -> ResolutionDetails<bool> {
        let sk_ctx = ctx.to_skotos_context();
        let detail = self
            .client
            .bool_variation_detail(flag_key, &sk_ctx, default);
        let value = detail.value.as_bool().unwrap_or(default);
        to_resolution(value, detail)
    }

    pub fn resolve_string_value(
        &self,
        flag_key: &str,
        ctx: &OpenFeatureContext,
        default: &str,
    ) -> ResolutionDetails<String> {
        let sk_ctx = ctx.to_skotos_context();
        let detail = self
            .client
            .string_variation_detail(flag_key, &sk_ctx, default);
        let value = detail
            .value
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| default.to_string());
        to_resolution(value, detail)
    }

    pub fn resolve_float_value(
        &self,
        flag_key: &str,
        ctx: &OpenFeatureContext,
        default: f64,
    ) -> ResolutionDetails<f64> {
        let sk_ctx = ctx.to_skotos_context();
        let detail = self.client.f64_variation_detail(flag_key, &sk_ctx, default);
        let value = detail.value.as_f64().unwrap_or(default);
        to_resolution(value, detail)
    }

    pub fn resolve_int_value(
        &self,
        flag_key: &str,
        ctx: &OpenFeatureContext,
        default: i64,
    ) -> ResolutionDetails<i64> {
        let sk_ctx = ctx.to_skotos_context();
        let detail = self
            .client
            .f64_variation_detail(flag_key, &sk_ctx, default as f64);
        let value = detail
            .value
            .as_i64()
            .or_else(|| detail.value.as_f64().map(|f| f as i64))
            .unwrap_or(default);
        to_resolution(value, detail)
    }

    pub fn resolve_struct_value(
        &self,
        flag_key: &str,
        ctx: &OpenFeatureContext,
        default: Value,
    ) -> ResolutionDetails<Value> {
        let sk_ctx = ctx.to_skotos_context();
        let detail = self
            .client
            .json_variation_detail(flag_key, &sk_ctx, default);
        let value = detail.value.clone();
        to_resolution(value, detail)
    }
}

fn to_resolution<T>(value: T, detail: EvaluationDetail) -> ResolutionDetails<T> {
    let variant = detail.variation_index.map(|i| i.to_string());
    let (reason, error_code) = match detail.reason {
        EvaluationReason::Off => ("DISABLED".to_string(), None),
        EvaluationReason::TargetMatch => ("TARGETING_MATCH".to_string(), None),
        EvaluationReason::RuleMatch { .. } => ("TARGETING_MATCH".to_string(), None),
        EvaluationReason::Fallthrough { .. } => ("DEFAULT".to_string(), None),
        EvaluationReason::PrerequisiteFailed { .. } => ("DISABLED".to_string(), None),
        EvaluationReason::Error { error_kind } => {
            let code = match error_kind {
                EvaluationErrorKind::ClientNotReady => "PROVIDER_NOT_READY",
                EvaluationErrorKind::FlagNotFound => "FLAG_NOT_FOUND",
                EvaluationErrorKind::MalformedFlag => "PARSE_ERROR",
                EvaluationErrorKind::UserNotSpecified => "TARGETING_KEY_MISSING",
                EvaluationErrorKind::WrongType => "TYPE_MISMATCH",
                EvaluationErrorKind::Exception => "GENERAL",
            };
            ("ERROR".to_string(), Some(code.to_string()))
        }
    };
    ResolutionDetails {
        value,
        variant,
        reason,
        error_code,
    }
}

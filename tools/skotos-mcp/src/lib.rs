//! Skotos Model Context Protocol (MCP) Server — JSON-RPC 2.0 over stdio and HTTP.
//!
//! Enables AI coding assistants (Antigravity, Claude, Cursor) and autonomous agents to
//! inspect flags, simulate targeting, check usage/stale status, review approval requests,
//! toggle flags (subject to RBAC & approval workflows), and scan local repositories for
//! flag code references.

use std::path::Path;

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct McpConfig {
    pub api_url: String,
    pub api_token: Option<String>,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            api_url: std::env::var("SKOTOS_API_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8080".to_string())
                .trim_end_matches('/')
                .to_string(),
            api_token: std::env::var("SKOTOS_API_TOKEN").ok(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Value>,
}

#[derive(Clone)]
pub struct McpServer {
    cfg: McpConfig,
    http: reqwest::Client,
}

impl McpServer {
    pub fn new(cfg: McpConfig) -> Self {
        Self {
            cfg,
            http: reqwest::Client::new(),
        }
    }

    pub fn http_router(self) -> Router {
        Router::new()
            .route("/mcp", post(handle_http_mcp))
            .with_state(self)
    }

    pub fn tool_definitions() -> Value {
        json!([
            {
                "name": "list_flags",
                "description": "List feature flags in a Skotos project",
                "inputSchema": {
                    "type": "object",
                    "properties": { "projectKey": { "type": "string", "default": "default" } }
                }
            },
            {
                "name": "get_flag",
                "description": "Retrieve full configuration and targeting rules for a feature flag",
                "inputSchema": {
                    "type": "object",
                    "required": ["flagKey"],
                    "properties": {
                        "projectKey": { "type": "string", "default": "default" },
                        "flagKey": { "type": "string" }
                    }
                }
            },
            {
                "name": "toggle_flag",
                "description": "Turn flag targeting ON or OFF in an environment (respects RBAC and approval workflows)",
                "inputSchema": {
                    "type": "object",
                    "required": ["flagKey", "environmentKey", "on"],
                    "properties": {
                        "projectKey": { "type": "string", "default": "default" },
                        "flagKey": { "type": "string" },
                        "environmentKey": { "type": "string" },
                        "on": { "type": "boolean" },
                        "comment": { "type": "string" }
                    }
                }
            },
            {
                "name": "simulate_flag",
                "description": "Evaluate a flag against a targeting context without side effects",
                "inputSchema": {
                    "type": "object",
                    "required": ["flagKey", "environmentKey", "context"],
                    "properties": {
                        "projectKey": { "type": "string", "default": "default" },
                        "flagKey": { "type": "string" },
                        "environmentKey": { "type": "string" },
                        "context": { "type": "object" }
                    }
                }
            },
            {
                "name": "list_approvals",
                "description": "List pending or historical flag change approval requests",
                "inputSchema": {
                    "type": "object",
                    "properties": { "status": { "type": "string" } }
                }
            },
            {
                "name": "get_flag_usage",
                "description": "Check 30-day evaluation counts and stale status for a flag in an environment",
                "inputSchema": {
                    "type": "object",
                    "required": ["flagKey", "environmentKey"],
                    "properties": {
                        "projectKey": { "type": "string", "default": "default" },
                        "flagKey": { "type": "string" },
                        "environmentKey": { "type": "string" }
                    }
                }
            },
            {
                "name": "scan_code_refs",
                "description": "Scan a local directory for references to feature flag keys",
                "inputSchema": {
                    "type": "object",
                    "required": ["dir", "flagKeys"],
                    "properties": {
                        "dir": { "type": "string" },
                        "flagKeys": { "type": "array", "items": { "type": "string" } }
                    }
                }
            }
        ])
    }

    pub async fn handle_request(&self, req: JsonRpcRequest) -> Option<JsonRpcResponse> {
        let id = req.id.clone()?;
        let res = match req.method.as_str() {
            "initialize" => Ok(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {}, "resources": {} },
                "serverInfo": {
                    "name": "skotos-mcp",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": Self::tool_definitions() })),
            "resources/list" => Ok(json!({
                "resources": [{
                    "uri": "skotos://projects/default/flags",
                    "name": "Skotos Feature Flags",
                    "mimeType": "application/json"
                }]
            })),
            "tools/call" => {
                let name = req.params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = req
                    .params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                self.call_tool(name, &args).await.map(|payload| {
                    json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string_pretty(&payload).unwrap_or_default()
                        }],
                        "isError": false
                    })
                })
            }
            other => Err((-32601, format!("method not found: {other}"))),
        };

        Some(match res {
            Ok(result) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(result),
                error: None,
            },
            Err((code, message)) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: None,
                error: Some(json!({ "code": code, "message": message })),
            },
        })
    }

    async fn call_tool(&self, name: &str, args: &Value) -> Result<Value, (i32, String)> {
        if name == "scan_code_refs" {
            let dir = args
                .get("dir")
                .and_then(Value::as_str)
                .ok_or((-32602, "dir is required".to_string()))?;
            let keys: Vec<String> = args
                .get("flagKeys")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_str)
                        .map(String::from)
                        .collect()
                })
                .unwrap_or_default();
            let hunks =
                skotos_code_refs::scan_directory(Path::new(dir), &keys).map_err(|e| (-32000, e))?;
            let summary = skotos_code_refs::summarize_hunks(&hunks);
            return Ok(
                json!({ "totalReferences": hunks.len(), "byFlag": summary, "hunks": hunks }),
            );
        }

        let token = self
            .cfg
            .api_token
            .as_deref()
            .ok_or((-32001, "SKOTOS_API_TOKEN is not configured".to_string()))?;
        let auth = format!("Bearer {token}");
        let project = args
            .get("projectKey")
            .and_then(Value::as_str)
            .unwrap_or("default");

        let resp = match name {
            "list_flags" => self
                .http
                .get(format!(
                    "{}/api/v1/projects/{project}/flags",
                    self.cfg.api_url
                ))
                .header("authorization", auth)
                .send()
                .await
                .map_err(|e| (-32000, e.to_string()))?,
            "get_flag" => {
                let key = args
                    .get("flagKey")
                    .and_then(Value::as_str)
                    .ok_or((-32602, "flagKey required".to_string()))?;
                self.http
                    .get(format!(
                        "{}/api/v1/projects/{project}/flags/{key}",
                        self.cfg.api_url
                    ))
                    .header("authorization", auth)
                    .send()
                    .await
                    .map_err(|e| (-32000, e.to_string()))?
            }
            "toggle_flag" => {
                let key = args
                    .get("flagKey")
                    .and_then(Value::as_str)
                    .ok_or((-32602, "flagKey required".to_string()))?;
                let env = args
                    .get("environmentKey")
                    .and_then(Value::as_str)
                    .ok_or((-32602, "environmentKey required".to_string()))?;
                let on = args
                    .get("on")
                    .and_then(Value::as_bool)
                    .ok_or((-32602, "on boolean required".to_string()))?;
                let kind = if on { "turnFlagOn" } else { "turnFlagOff" };
                let comment = args
                    .get("comment")
                    .and_then(Value::as_str)
                    .unwrap_or("Updated via skotos-mcp");
                self.http
                    .patch(format!(
                        "{}/api/v1/projects/{project}/flags/{key}",
                        self.cfg.api_url
                    ))
                    .header("authorization", auth)
                    .json(&json!({
                        "environmentKey": env,
                        "comment": comment,
                        "instructions": [{ "kind": kind }],
                    }))
                    .send()
                    .await
                    .map_err(|e| (-32000, e.to_string()))?
            }
            "simulate_flag" => {
                let key = args
                    .get("flagKey")
                    .and_then(Value::as_str)
                    .ok_or((-32602, "flagKey required".to_string()))?;
                let env = args
                    .get("environmentKey")
                    .and_then(Value::as_str)
                    .ok_or((-32602, "environmentKey required".to_string()))?;
                let context = args
                    .get("context")
                    .cloned()
                    .ok_or((-32602, "context object required".to_string()))?;
                self.http
                    .post(format!(
                        "{}/api/v1/projects/{project}/flags/{key}/environments/{env}/simulate",
                        self.cfg.api_url
                    ))
                    .header("authorization", auth)
                    .json(&json!({ "context": context }))
                    .send()
                    .await
                    .map_err(|e| (-32000, e.to_string()))?
            }
            "list_approvals" => {
                let mut url = format!("{}/api/v1/approvals", self.cfg.api_url);
                if let Some(st) = args.get("status").and_then(Value::as_str) {
                    url.push_str(&format!("?status={st}"));
                }
                self.http
                    .get(url)
                    .header("authorization", auth)
                    .send()
                    .await
                    .map_err(|e| (-32000, e.to_string()))?
            }
            "get_flag_usage" => {
                let key = args
                    .get("flagKey")
                    .and_then(Value::as_str)
                    .ok_or((-32602, "flagKey required".to_string()))?;
                let env = args
                    .get("environmentKey")
                    .and_then(Value::as_str)
                    .ok_or((-32602, "environmentKey required".to_string()))?;
                self.http
                    .get(format!(
                        "{}/api/v1/projects/{project}/flags/{key}/environments/{env}/usage?days=30",
                        self.cfg.api_url
                    ))
                    .header("authorization", auth)
                    .send()
                    .await
                    .map_err(|e| (-32000, e.to_string()))?
            }
            other => return Err((-32602, format!("unknown tool: {other}"))),
        };

        let status = resp.status();
        let text = resp.text().await.map_err(|e| (-32000, e.to_string()))?;
        let val: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!({ "raw": text }));
        if !status.is_success() {
            return Err((-32000, format!("HTTP {status}: {val}")));
        }
        Ok(val)
    }
}

async fn handle_http_mcp(
    State(server): State<McpServer>,
    Json(req): Json<JsonRpcRequest>,
) -> Json<Value> {
    match server.handle_request(req).await {
        Some(resp) => Json(serde_json::to_value(resp).unwrap_or(Value::Null)),
        None => Json(json!({ "ok": true })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn handles_mcp_initialize_tools_list_and_scan_code_refs() {
        let srv = McpServer::new(McpConfig {
            api_url: "http://127.0.0.1:8080".to_string(),
            api_token: None,
        });

        let init = srv
            .handle_request(JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                id: Some(json!(1)),
                method: "initialize".to_string(),
                params: json!({}),
            })
            .await
            .unwrap();
        assert_eq!(init.result.unwrap()["serverInfo"]["name"], "skotos-mcp");

        let tools = srv
            .handle_request(JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                id: Some(json!(2)),
                method: "tools/list".to_string(),
                params: json!({}),
            })
            .await
            .unwrap();
        let arr = tools.result.unwrap()["tools"].as_array().unwrap().len();
        assert!(arr >= 7);

        let tmp = std::env::temp_dir().join("skotos-mcp-scan-test");
        let _ = std::fs::create_dir_all(&tmp);
        std::fs::write(tmp.join("app.ts"), "useFlag('mcp-demo-flag')").unwrap();
        let call = srv
            .handle_request(JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                id: Some(json!(3)),
                method: "tools/call".to_string(),
                params: json!({
                    "name": "scan_code_refs",
                    "arguments": {
                        "dir": tmp.to_string_lossy(),
                        "flagKeys": ["mcp-demo-flag"]
                    }
                }),
            })
            .await
            .unwrap();
        let text = call.result.unwrap()["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(text.contains("mcp-demo-flag"));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

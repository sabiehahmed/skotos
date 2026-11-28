//! Skotos CLI library powering the `skotos` developer and CI/CD binary.

use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct CliConfig {
    pub api_url: String,
    pub api_token: String,
}

impl CliConfig {
    pub fn from_env() -> Result<Self, String> {
        let api_url =
            std::env::var("SKOTOS_API_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string());
        let api_token = std::env::var("SKOTOS_API_TOKEN")
            .map_err(|_| "SKOTOS_API_TOKEN environment variable is required".to_string())?;
        Ok(Self {
            api_url: api_url.trim_end_matches('/').to_string(),
            api_token,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Help,
    ProjectsList,
    FlagsList {
        project: String,
    },
    FlagsGet {
        project: String,
        flag_key: String,
    },
    FlagsToggle {
        project: String,
        env: String,
        flag_key: String,
        on: bool,
        comment: Option<String>,
    },
    FlagsEval {
        project: String,
        env: String,
        flag_key: String,
        context_json: String,
    },
    ApprovalsList,
    ApprovalsApply {
        approval_id: String,
    },
}

pub fn parse_args(args: &[String]) -> Result<Command, String> {
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" || args[0] == "help" {
        return Ok(Command::Help);
    }
    match (
        args.first().map(String::as_str),
        args.get(1).map(String::as_str),
    ) {
        (Some("projects"), Some("list")) => Ok(Command::ProjectsList),
        (Some("flags"), Some("list")) => Ok(Command::FlagsList {
            project: flag_value(args, "--project").unwrap_or_else(|| "default".to_string()),
        }),
        (Some("flags"), Some("get")) => {
            let key = args
                .get(2)
                .ok_or_else(|| "usage: skotos flags get <flagKey> [--project default]".to_string())?
                .clone();
            Ok(Command::FlagsGet {
                project: flag_value(args, "--project").unwrap_or_else(|| "default".to_string()),
                flag_key: key,
            })
        }
        (Some("flags"), Some("toggle")) => {
            let key = args
                .get(2)
                .ok_or_else(|| {
                    "usage: skotos flags toggle <flagKey> --env <env> --on|--off".to_string()
                })?
                .clone();
            let env = flag_value(args, "--env").unwrap_or_else(|| "development".to_string());
            let on = if args.iter().any(|a| a == "--on") {
                true
            } else if args.iter().any(|a| a == "--off") {
                false
            } else {
                return Err("specify either --on or --off".to_string());
            };
            Ok(Command::FlagsToggle {
                project: flag_value(args, "--project").unwrap_or_else(|| "default".to_string()),
                env,
                flag_key: key,
                on,
                comment: flag_value(args, "--comment"),
            })
        }
        (Some("flags"), Some("eval")) => {
            let key = args
                .get(2)
                .ok_or_else(|| {
                    "usage: skotos flags eval <flagKey> --env <env> --context '<json>'".to_string()
                })?
                .clone();
            let env = flag_value(args, "--env").unwrap_or_else(|| "development".to_string());
            let ctx = flag_value(args, "--context")
                .unwrap_or_else(|| r#"{"kind":"user","key":"cli-user"}"#.to_string());
            Ok(Command::FlagsEval {
                project: flag_value(args, "--project").unwrap_or_else(|| "default".to_string()),
                env,
                flag_key: key,
                context_json: ctx,
            })
        }
        (Some("approvals"), Some("list")) => Ok(Command::ApprovalsList),
        (Some("approvals"), Some("apply")) => {
            let id = args
                .get(2)
                .ok_or_else(|| "usage: skotos approvals apply <approvalId>".to_string())?
                .clone();
            Ok(Command::ApprovalsApply { approval_id: id })
        }
        _ => Err(format!("unknown command: {}", args.join(" "))),
    }
}

fn flag_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}

pub async fn execute(cfg: &CliConfig, cmd: Command) -> Result<Value, String> {
    let http = reqwest::Client::new();
    let auth = format!("Bearer {}", cfg.api_token);

    let resp = match cmd {
        Command::Help => {
            return Ok(json!({
                "usage": "skotos <projects|flags|approvals> <subcommand> [options]"
            }));
        }
        Command::ProjectsList => http
            .get(format!("{}/api/v1/projects", cfg.api_url))
            .header("authorization", auth)
            .send()
            .await
            .map_err(|e| e.to_string())?,
        Command::FlagsList { project } => http
            .get(format!("{}/api/v1/projects/{project}/flags", cfg.api_url))
            .header("authorization", auth)
            .send()
            .await
            .map_err(|e| e.to_string())?,
        Command::FlagsGet { project, flag_key } => http
            .get(format!(
                "{}/api/v1/projects/{project}/flags/{flag_key}",
                cfg.api_url
            ))
            .header("authorization", auth)
            .send()
            .await
            .map_err(|e| e.to_string())?,
        Command::FlagsToggle {
            project,
            env,
            flag_key,
            on,
            comment,
        } => {
            let kind = if on { "turnFlagOn" } else { "turnFlagOff" };
            http.patch(format!(
                "{}/api/v1/projects/{project}/flags/{flag_key}",
                cfg.api_url
            ))
            .header("authorization", auth)
            .json(&json!({
                "environmentKey": env,
                "comment": comment.unwrap_or_else(|| format!("skotos-cli {kind}")),
                "instructions": [{ "kind": kind }],
            }))
            .send()
            .await
            .map_err(|e| e.to_string())?
        }
        Command::FlagsEval {
            project,
            env,
            flag_key,
            context_json,
        } => {
            let ctx: Value = serde_json::from_str(&context_json)
                .map_err(|e| format!("invalid context JSON: {e}"))?;
            http.post(format!(
                "{}/api/v1/projects/{project}/flags/{flag_key}/environments/{env}/simulate",
                cfg.api_url
            ))
            .header("authorization", auth)
            .json(&json!({ "context": ctx }))
            .send()
            .await
            .map_err(|e| e.to_string())?
        }
        Command::ApprovalsList => http
            .get(format!("{}/api/v1/approvals", cfg.api_url))
            .header("authorization", auth)
            .send()
            .await
            .map_err(|e| e.to_string())?,
        Command::ApprovalsApply { approval_id } => http
            .post(format!(
                "{}/api/v1/approvals/{approval_id}/apply",
                cfg.api_url
            ))
            .header("authorization", auth)
            .send()
            .await
            .map_err(|e| e.to_string())?,
    };

    let status = resp.status();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    let val: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!({ "raw": text }));
    if !status.is_success() {
        return Err(format!("HTTP {status}: {val}"));
    }
    Ok(val)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cli_subcommands() {
        let args: Vec<String> = [
            "flags",
            "toggle",
            "checkout-v2",
            "--project",
            "core",
            "--env",
            "production",
            "--on",
            "--comment",
            "Launch",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
        assert_eq!(
            parse_args(&args).unwrap(),
            Command::FlagsToggle {
                project: "core".to_string(),
                env: "production".to_string(),
                flag_key: "checkout-v2".to_string(),
                on: true,
                comment: Some("Launch".to_string()),
            }
        );
    }
}

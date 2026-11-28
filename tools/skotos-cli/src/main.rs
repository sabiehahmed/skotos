use std::process::ExitCode;

use skotos_cli::{execute, parse_args, CliConfig, Command};

const HELP: &str = "\
Skotos CLI — Feature Flag Management & Automation

Usage:
  skotos projects list
  skotos flags list [--project default]
  skotos flags get <flagKey> [--project default]
  skotos flags toggle <flagKey> --env <envKey> --on|--off [--project default] [--comment <text>]
  skotos flags eval <flagKey> --env <envKey> --context '<json>' [--project default]
  skotos approvals list
  skotos approvals apply <approvalId>

Environment:
  SKOTOS_API_URL     Control Plane base URL (default: http://127.0.0.1:8080)
  SKOTOS_API_TOKEN   API access token (sk_api_...)
";

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = match parse_args(&args) {
        Ok(Command::Help) => {
            print!("{HELP}");
            return ExitCode::SUCCESS;
        }
        Ok(c) => c,
        Err(err) => {
            eprintln!("skotos: {err}\n\n{HELP}");
            return ExitCode::from(2);
        }
    };

    let cfg = match CliConfig::from_env() {
        Ok(c) => c,
        Err(err) => {
            eprintln!("skotos: {err}");
            return ExitCode::from(1);
        }
    };

    match execute(&cfg, cmd).await {
        Ok(val) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&val).unwrap_or_else(|_| val.to_string())
            );
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("skotos error: {err}");
            ExitCode::from(1)
        }
    }
}

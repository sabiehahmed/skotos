use std::path::PathBuf;
use std::process::ExitCode;

use skotos_code_refs::{scan_directory, summarize_hunks, upload_code_refs};

const USAGE: &str = "\
skotos-code-refs — CI/CD Flag Reference Scanner & Extinction Guard

Usage:
  skotos-code-refs --dir <path> --flags <key1,key2,...> [--check-extinction <flagKey>]
                   [--upload --project <key> --repo <name> --branch <branch> --commit <sha>]
";

fn flag_val(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    let dir = PathBuf::from(flag_val(&args, "--dir").unwrap_or_else(|| ".".to_string()));
    let check_extinct = flag_val(&args, "--check-extinction");
    let mut flag_keys: Vec<String> = flag_val(&args, "--flags")
        .map(|raw| {
            raw.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();

    if let Some(ref ext) = check_extinct {
        if !flag_keys.contains(ext) {
            flag_keys.push(ext.clone());
        }
    }

    let hunks = match scan_directory(&dir, &flag_keys) {
        Ok(h) => h,
        Err(err) => {
            eprintln!("skotos-code-refs scan error: {err}");
            return ExitCode::from(1);
        }
    };

    let summary = summarize_hunks(&hunks);
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "scannedDir": dir.display().to_string(),
            "totalReferences": hunks.len(),
            "byFlag": summary,
            "hunks": hunks,
        }))
        .unwrap_or_default()
    );

    if args.iter().any(|a| a == "--upload") {
        let api_url =
            std::env::var("SKOTOS_API_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string());
        let Ok(api_token) = std::env::var("SKOTOS_API_TOKEN") else {
            eprintln!("skotos-code-refs: SKOTOS_API_TOKEN required for --upload");
            return ExitCode::from(1);
        };
        let project = flag_val(&args, "--project").unwrap_or_else(|| "default".to_string());
        let repo = flag_val(&args, "--repo").unwrap_or_else(|| "workspace".to_string());
        let branch = flag_val(&args, "--branch").unwrap_or_else(|| "main".to_string());
        let commit = flag_val(&args, "--commit").unwrap_or_default();
        if let Err(err) = upload_code_refs(
            &api_url, &api_token, &project, &repo, &branch, &commit, &hunks,
        )
        .await
        {
            eprintln!("skotos-code-refs upload error: {err}");
            return ExitCode::from(1);
        }
    }

    if let Some(ext_key) = check_extinct {
        let remaining = summary.get(&ext_key).copied().unwrap_or(0);
        if remaining > 0 {
            eprintln!(
                "EXTINCTION CHECK FAILED: flag '{ext_key}' still has {remaining} reference(s) in {}",
                dir.display()
            );
            return ExitCode::from(1);
        }
    }

    ExitCode::SUCCESS
}

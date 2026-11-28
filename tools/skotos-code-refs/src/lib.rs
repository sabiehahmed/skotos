//! High-speed repository code reference scanner & extinction checker for Skotos feature flags.
//!
//! Designed for GitHub Actions, GitLab CI, and local pre-cleanup checks:
//! - Walks a directory tree skipping `.git`, `node_modules`, `target`, `dist`, and binary files.
//! - Locates every reference to configured flag keys with exact file path, 1-indexed line number,
//!   and trimmed code line preview (`CodeRefHunk`).
//! - Supports `--check-extinction <flagKey>` to fail CI if a deprecated flag is still referenced.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::json;
use skotos_domain::CodeRefHunk;

const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    "coverage",
    ".next",
    ".venv",
    "playwright-report",
    "test-results",
];

const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// Recursively scans `root` for references to any key in `flag_keys`.
pub fn scan_directory(root: &Path, flag_keys: &[String]) -> Result<Vec<CodeRefHunk>, String> {
    if flag_keys.is_empty() {
        return Ok(Vec::new());
    }
    let mut hunks = Vec::new();
    walk_dir(root, root, flag_keys, &mut hunks)?;
    Ok(hunks)
}

fn walk_dir(
    root: &Path,
    dir: &Path,
    flag_keys: &[String],
    out: &mut Vec<CodeRefHunk>,
) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        if path.is_dir() {
            if SKIP_DIRS.contains(&name.as_ref()) {
                continue;
            }
            walk_dir(root, &path, flag_keys, out)?;
        } else if path.is_file() {
            if let Ok(meta) = entry.metadata() {
                if meta.len() > MAX_FILE_BYTES {
                    continue;
                }
            }
            let Ok(bytes) = fs::read(&path) else {
                continue;
            };
            // Skip binary files containing NUL bytes
            if bytes.contains(&0) {
                continue;
            }
            let Ok(text) = std::str::from_utf8(&bytes) else {
                continue;
            };
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");

            for (idx, line) in text.lines().enumerate() {
                for key in flag_keys {
                    if contains_flag_token(line, key) {
                        let preview: String = line.trim().chars().take(200).collect();
                        out.push(CodeRefHunk {
                            flag_key: key.clone(),
                            file_path: rel.clone(),
                            line_number: (idx + 1) as i32,
                            line_preview: preview,
                            aliases: vec![],
                        });
                    }
                }
            }
        }
    }
    Ok(())
}

/// Checks whether `line` contains `flag_key` delimited by non-identifier boundaries
/// (quotes, whitespace, punctuation) to avoid false substring matches.
fn contains_flag_token(line: &str, flag_key: &str) -> bool {
    if flag_key.is_empty() {
        return false;
    }
    let bytes = line.as_bytes();
    let klen = flag_key.len();
    let mut start = 0;
    while let Some(pos) = line[start..].find(flag_key) {
        let abs = start + pos;
        let left_ok = abs == 0 || !is_ident_char(bytes[abs - 1]);
        let right_idx = abs + klen;
        let right_ok = right_idx >= bytes.len() || !is_ident_char(bytes[right_idx]);
        if left_ok && right_ok {
            return true;
        }
        start = abs + 1;
    }
    false
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_'
}

/// Groups hunks by flag key and returns `(total_refs, by_flag_counts)`.
pub fn summarize_hunks(hunks: &[CodeRefHunk]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for h in hunks {
        *counts.entry(h.flag_key.clone()).or_insert(0) += 1;
    }
    counts
}

/// Uploads scanned code reference hunks to the Skotos Control Plane API.
pub async fn upload_code_refs(
    api_url: &str,
    api_token: &str,
    project_key: &str,
    repo_name: &str,
    branch: &str,
    commit_sha: &str,
    hunks: &[CodeRefHunk],
) -> Result<serde_json::Value, String> {
    let url = format!(
        "{}/api/v1/projects/{project_key}/code-refs",
        api_url.trim_end_matches('/')
    );
    let resp = reqwest::Client::new()
        .post(&url)
        .header("authorization", format!("Bearer {api_token}"))
        .json(&json!({
            "repoName": repo_name,
            "branch": branch,
            "commitSha": commit_sha,
            "hunks": hunks,
        }))
        .send()
        .await
        .map_err(|e| format!("upload failed: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("HTTP {status}: {text}"));
    }
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_flag_references_respecting_boundaries_and_skip_dirs() {
        let tmp = std::env::temp_dir().join(format!(
            "skotos-coderefs-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(tmp.join("src")).unwrap();
        fs::create_dir_all(tmp.join("node_modules/pkg")).unwrap();

        fs::write(
            tmp.join("src/checkout.ts"),
            "const enabled = client.boolVariation('express-checkout', ctx, false);\nconst other = 'express-checkout-v2';\n",
        )
        .unwrap();
        fs::write(
            tmp.join("node_modules/pkg/index.js"),
            "const ignored = 'express-checkout';\n",
        )
        .unwrap();

        let keys = vec!["express-checkout".to_string(), "retired-banner".to_string()];
        let hunks = scan_directory(&tmp, &keys).unwrap();
        let summary = summarize_hunks(&hunks);

        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].flag_key, "express-checkout");
        assert_eq!(hunks[0].file_path, "src/checkout.ts");
        assert_eq!(hunks[0].line_number, 1);
        assert_eq!(summary.get("express-checkout"), Some(&1));
        assert_eq!(summary.get("retired-banner"), None); // Extinct!

        let _ = fs::remove_dir_all(&tmp);
    }
}

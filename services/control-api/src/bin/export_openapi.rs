use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use control_api::ApiDoc;
use utoipa::OpenApi;

fn main() -> ExitCode {
    let out_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/openapi/control-api.json");

    if let Some(parent) = out_path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            eprintln!("export-openapi: failed to create {}: {e}", parent.display());
            return ExitCode::from(1);
        }
    }

    let doc = ApiDoc::openapi();
    let json = match serde_json::to_string_pretty(&doc) {
        Ok(s) => format!("{s}\n"),
        Err(e) => {
            eprintln!("export-openapi: failed to serialize OpenAPI spec: {e}");
            return ExitCode::from(1);
        }
    };

    if let Err(e) = fs::write(&out_path, json) {
        eprintln!(
            "export-openapi: failed to write {}: {e}",
            out_path.display()
        );
        return ExitCode::from(1);
    }

    println!("Exported OpenAPI 3.1 schema to {}", out_path.display());
    ExitCode::SUCCESS
}

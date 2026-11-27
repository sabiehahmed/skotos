use std::process::ExitCode;

fn main() -> ExitCode {
    eprintln!(
        "export-openapi: OpenAPI 3.1 schema generation (spec/openapi/control-api.json) is not yet \
         implemented (scheduled for Milestone 1C / M4)."
    );
    ExitCode::from(2)
}

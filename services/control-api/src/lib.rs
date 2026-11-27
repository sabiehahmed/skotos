//! Skotos Control Plane API service.
//!
//! Full Axum REST server, SQLx migration runner, and OpenAPI 3.1 exporter are
//! implemented in Milestone 1C (M4).

/// Canonical service name.
pub const SERVICE_NAME: &str = "control-api";

/// Usage summary printed by `control-api --help`.
pub const USAGE: &str = "\
Usage: control-api [OPTIONS]

Options:
  --migrate-only    Run embedded PostgreSQL migrations and exit (implemented in Milestone 1C / M4)
  -h, --help        Print this help message
";

/// Parsed CLI command for the `control-api` binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliCommand {
    /// Start the Control Plane HTTP server.
    Serve,
    /// Run embedded SQLx migrations and exit without starting HTTP.
    MigrateOnly,
    /// Print CLI usage and exit 0.
    Help,
}

/// Parses command-line arguments (excluding `argv[0]`) for `control-api`.
pub fn parse_cli_args<I, S>(args: I) -> Result<CliCommand, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut command = CliCommand::Serve;

    for arg in args {
        match arg.as_ref() {
            "--help" | "-h" => return Ok(CliCommand::Help),
            "--migrate-only" => command = CliCommand::MigrateOnly,
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    Ok(command)
}

/// Returns the four internal workspace crates linked into `control-api`.
pub fn linked_crates() -> [&'static str; 4] {
    [
        skotos_domain::CRATE_NAME,
        skotos_eval::CRATE_NAME,
        skotos_storage::CRATE_NAME,
        skotos_auth::CRATE_NAME,
    ]
}

#[cfg(test)]
mod tests {
    use super::{linked_crates, parse_cli_args, CliCommand, SERVICE_NAME};

    #[test]
    fn links_all_workspace_crates() {
        assert_eq!(SERVICE_NAME, "control-api");
        assert_eq!(
            linked_crates(),
            [
                "skotos-domain",
                "skotos-eval",
                "skotos-storage",
                "skotos-auth"
            ]
        );
    }

    #[test]
    fn parses_default_serve_command() {
        assert_eq!(parse_cli_args(Vec::<&str>::new()), Ok(CliCommand::Serve));
    }

    #[test]
    fn parses_migrate_only_flag() {
        assert_eq!(
            parse_cli_args(["--migrate-only"]),
            Ok(CliCommand::MigrateOnly)
        );
    }

    #[test]
    fn parses_help_flags() {
        assert_eq!(parse_cli_args(["--help"]), Ok(CliCommand::Help));
        assert_eq!(parse_cli_args(["-h"]), Ok(CliCommand::Help));
    }

    #[test]
    fn rejects_unknown_argument() {
        let err = parse_cli_args(["--invalid"]).expect_err("expected unknown argument error");
        assert!(err.contains("--invalid"));
    }
}

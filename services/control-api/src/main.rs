use std::process::ExitCode;

use control_api::{linked_crates, parse_cli_args, CliCommand, SERVICE_NAME, USAGE};

fn main() -> ExitCode {
    match parse_cli_args(std::env::args().skip(1)) {
        Ok(CliCommand::Help) => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(CliCommand::MigrateOnly) => {
            eprintln!(
                "{SERVICE_NAME}: embedded PostgreSQL migrations are not yet implemented \
                 (scheduled for Milestone 1C / M4)."
            );
            ExitCode::from(2)
        }
        Ok(CliCommand::Serve) => {
            let crates = linked_crates().join(", ");
            eprintln!(
                "{SERVICE_NAME}: HTTP control plane server is not yet implemented \
                 (scheduled for Milestone 1C / M4; linked crates: {crates})."
            );
            ExitCode::from(2)
        }
        Err(err) => {
            eprintln!("{SERVICE_NAME}: {err}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

mod output;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use crate::Result;
use crate::app::init;

use self::output::{CommandResponse, ErrorResponse};

#[derive(Debug, Parser)]
#[command(name = "commonplace", version, about)]
struct Cli {
    #[arg(long, global = true, default_value = ".commonplace")]
    store: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create or validate a knowledge base.
    Init,
}

pub fn main() -> ExitCode {
    let cli = Cli::parse();
    let operation = cli.command.operation();

    match execute(cli) {
        Ok(response) => match serde_json::to_string_pretty(&response) {
            Ok(json) => {
                println!("{json}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("failed to serialize command result: {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            let response = ErrorResponse::new(operation, &error);
            match serde_json::to_string_pretty(&response) {
                Ok(json) => eprintln!("{json}"),
                Err(serialization_error) => {
                    eprintln!("{error}; failed to serialize error: {serialization_error}");
                }
            }
            ExitCode::from(error.exit_code())
        }
    }
}

fn execute(cli: Cli) -> Result<CommandResponse> {
    match cli.command {
        Command::Init => {
            let result = init::initialize(&cli.store)?;
            Ok(CommandResponse::init(result))
        }
    }
}

impl Command {
    const fn operation(&self) -> &'static str {
        match self {
            Self::Init => "init",
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::Cli;
    use super::execute;

    #[test]
    fn init_command_is_idempotent() {
        let parent = tempfile::tempdir().expect("temporary parent");
        let store = parent.path().join("knowledge");
        let store_text = store.to_string_lossy().into_owned();

        let created = execute(
            Cli::try_parse_from(["commonplace", "--store", &store_text, "init"])
                .expect("parse init command"),
        )
        .expect("initialize command");
        assert_eq!(created.status, "complete");

        let existing = execute(
            Cli::try_parse_from(["commonplace", "--store", &store_text, "init"])
                .expect("parse repeated init command"),
        )
        .expect("repeat init command");
        assert_eq!(existing.status, "unchanged");
    }
}

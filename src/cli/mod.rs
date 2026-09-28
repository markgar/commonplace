mod ingest;
mod input;
mod output;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use crate::Result;
use crate::app::{get, init, schema};

use self::output::{CommandResponse, CommandResult, ErrorResponse};

#[derive(Debug, Parser)]
#[command(name = "commonplace", version, about)]
struct Cli {
    #[arg(long, global = true, default_value = ".commonplace")]
    store: PathBuf,

    /// Emit JSON (also the default).
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create or validate a knowledge base.
    Init,
    /// Ingest local UTF-8 files with independently atomic publication.
    Ingest(ingest::IngestArgs),
    /// Read a complete document, revision, or exact passage.
    Get { id: String },
    /// Apply or inspect the additive vocabulary.
    Schema {
        #[command(subcommand)]
        command: SchemaCommand,
    },
}

#[derive(Debug, Subcommand)]
enum SchemaCommand {
    /// Validate and add vocabulary terms and predicate endpoints.
    Apply {
        #[arg(required_unless_present = "describe", conflicts_with = "describe")]
        file: Option<PathBuf>,
        #[arg(long, conflicts_with = "describe")]
        check: bool,
        /// Describe the validated JSON input without opening a store.
        #[arg(long)]
        describe: bool,
    },
    /// Read the complete vocabulary.
    Show,
}

pub fn main() -> ExitCode {
    let cli = Cli::parse();
    let operation = cli.command.operation();

    match execute(cli) {
        Ok(response) => match serde_json::to_string_pretty(&response) {
            Ok(json) => {
                println!("{json}");
                ExitCode::from(response.exit_code())
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
        Command::Ingest(args) => ingest::execute(&cli.store, args),
        Command::Get { id } => Ok(CommandResponse::new(
            "get",
            "complete",
            CommandResult::Get(get::get(&cli.store, &id)?),
        )),
        Command::Init => {
            let result = init::initialize(&cli.store)?;
            Ok(CommandResponse::init(result))
        }
        Command::Schema {
            command: SchemaCommand::Show,
        } => Ok(CommandResponse::new(
            "schema.show",
            "complete",
            CommandResult::Vocabulary(schema::show(&cli.store)?),
        )),
        Command::Schema {
            command:
                SchemaCommand::Apply {
                    file,
                    check,
                    describe,
                },
        } => {
            if describe {
                return Ok(CommandResponse::new(
                    "schema.apply.describe",
                    "complete",
                    CommandResult::Description(input::describe()?),
                ));
            }
            let file = file.ok_or_else(|| {
                crate::CommonplaceError::InvalidInput("schema file is required".into())
            })?;
            let config = schema::OperationConfig::default();
            let input = input::read(&file, config.maximum_input_bytes)?;
            let result = schema::apply(&cli.store, input, check, &config)?;
            let status = if check {
                "checked"
            } else if result.changed {
                "complete"
            } else {
                "unchanged"
            };
            Ok(CommandResponse::new(
                "schema.apply",
                status,
                CommandResult::Apply(result),
            ))
        }
    }
}

impl Command {
    const fn operation(&self) -> &'static str {
        match self {
            Self::Init => "init",
            Self::Ingest(args) if args.describe => "ingest.describe",
            Self::Ingest(_) => "ingest",
            Self::Get { .. } => "get",
            Self::Schema {
                command: SchemaCommand::Show,
            } => "schema.show",
            Self::Schema {
                command: SchemaCommand::Apply { describe: true, .. },
            } => "schema.apply.describe",
            Self::Schema { .. } => "schema.apply",
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

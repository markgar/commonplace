mod ingest;
mod input;
mod output;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use crate::Result;
use crate::app::{get, graph, init, schema};

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
    /// Inspect, query, or explicitly rebuild the derived RDF graph.
    Graph {
        #[command(subcommand)]
        command: GraphCommand,
    },
    /// Ingest UTF-8 files or caller-prepared stdin/JSONL; each document succeeds or fails independently.
    Ingest(Box<ingest::IngestArgs>),
    /// Read document metadata, revision text, or an exact passage.
    Get { id: String },
    /// Apply or inspect your vocabulary (entity types, predicates, and identifiers).
    Schema {
        #[command(subcommand)]
        command: SchemaCommand,
    },
}

#[derive(Debug, Subcommand)]
enum GraphCommand {
    /// Describe how knowledge maps to RDF, not your vocabulary; no store required.
    Schema,
    /// Run a local, native read-only SPARQL SELECT.
    Query {
        query: String,
        /// Maximum retained rows (zero allowed); consumes at most one extra solution.
        #[arg(long, default_value_t = 1000)]
        row_limit: usize,
        /// Cooperative evaluation budget in milliseconds, not a hard timeout.
        #[arg(long, default_value_t = 5000)]
        timeout_ms: u64,
    },
    /// Rebuild from committed SQLite, repairing derived state only.
    Rebuild,
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
    /// Read your complete vocabulary, not the RDF mapping (see graph schema).
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
        Command::Graph { command } => match command {
            GraphCommand::Schema => Ok(CommandResponse::new(
                "graph.schema",
                "complete",
                CommandResult::GraphSchema(crate::graph::schema::describe()),
            )),
            GraphCommand::Query {
                query,
                row_limit,
                timeout_ms,
            } => Ok(CommandResponse::new(
                "graph.query",
                "complete",
                CommandResult::GraphQuery(graph::query(
                    &cli.store,
                    &query,
                    crate::graph::QueryConfig {
                        row_limit,
                        timeout: std::time::Duration::from_millis(timeout_ms),
                    },
                )?),
            )),
            GraphCommand::Rebuild => Ok(CommandResponse::new(
                "graph.rebuild",
                "complete",
                CommandResult::GraphRebuild(graph::rebuild(&cli.store)?),
            )),
        },
        Command::Ingest(args) => ingest::execute(&cli.store, *args),
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
            Self::Graph {
                command: GraphCommand::Schema,
            } => "graph.schema",
            Self::Graph {
                command: GraphCommand::Query { .. },
            } => "graph.query",
            Self::Graph {
                command: GraphCommand::Rebuild,
            } => "graph.rebuild",
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
    fn help_distinguishes_vocabulary_mapping_and_file_outcomes() {
        for (args, expected) in [
            (
                vec!["commonplace", "--help"],
                "your vocabulary (entity types, predicates, and identifiers)",
            ),
            (
                vec!["commonplace", "schema", "--help"],
                "not the RDF mapping",
            ),
            (
                vec!["commonplace", "graph", "schema", "--help"],
                "not your vocabulary",
            ),
            (
                vec!["commonplace", "ingest", "--help"],
                "each document succeeds or fails independently",
            ),
            (
                vec!["commonplace", "get", "--help"],
                "document metadata, revision text, or an exact passage",
            ),
        ] {
            let help = Cli::try_parse_from(args).expect_err("help exits before execution");
            assert_eq!(help.kind(), clap::error::ErrorKind::DisplayHelp);
            assert!(help.to_string().contains(expected), "{help}");
        }
    }

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

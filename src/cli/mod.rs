mod ingest;
mod input;
mod output;
mod record;
mod remove;
mod withdraw;

use std::io::Write;
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
    /// Retrieve current exact passages using lexical/vector search and local reranking.
    Search {
        /// Plain text, not FTS syntax (maximum 4096 UTF-8 bytes and 64 terms).
        query: String,
        /// Inclusive source time (RFC3339); excludes sources without a time.
        #[arg(long)]
        since: Option<String>,
        /// Exact source type; repeat to match any supplied type.
        #[arg(long = "source-type")]
        source_types: Vec<String>,
        /// Retained results, 0 through 50; required models are checked even at zero.
        #[arg(long, default_value_t = crate::domain::search::DEFAULT_RESULT_LIMIT)]
        limit: usize,
    },
    /// Atomically record entities, metadata, cited types and facts from JSON or JSONL.
    Record(record::RecordArgs),
    /// Permanently delete one source's stored evidence, retaining authored knowledge and source files.
    Remove(remove::RemoveArgs),
    /// Atomically withdraw active knowledge from JSON, preserving exact retained history.
    Withdraw(withdraw::WithdrawArgs),
    /// Read document metadata, revision text, or an exact passage, entity, or knowledge item.
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
    /// Permanently freeze this store's current vocabulary.
    Freeze,
}

pub fn main() -> ExitCode {
    let cli = Cli::parse();
    let operation = cli.command.operation();

    match execute(cli) {
        Ok(response)
            if matches!(
                &response.result,
                CommandResult::Record(_)
                    | CommandResult::Remove(_)
                    | CommandResult::Withdraw(_)
                    | CommandResult::Freeze(_)
            ) =>
        {
            match write_committed_response(&response, &mut std::io::stdout().lock()) {
                Ok(()) => ExitCode::from(response.exit_code()),
                Err(error) => {
                    let error = ErrorResponse::new(operation, &error);
                    let mut stderr = std::io::stderr().lock();
                    let _ = serde_json::to_writer_pretty(&mut stderr, &error);
                    let _ = stderr.write_all(b"\n").and_then(|()| stderr.flush());
                    ExitCode::FAILURE
                }
            }
        }
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

fn write_committed_response(response: &CommandResponse, output: &mut impl Write) -> Result<()> {
    let (receipt, guidance) = match &response.result {
        CommandResult::Record(record) => (&record.receipt, crate::app::record::RECOVERY_GUIDANCE),
        CommandResult::Remove(removed) => (&removed.receipt, crate::app::remove::RECOVERY_GUIDANCE),
        CommandResult::Withdraw(withdrawn) => {
            (&withdrawn.receipt, crate::app::withdraw::RECOVERY_GUIDANCE)
        }
        CommandResult::Freeze(frozen) => (&frozen.receipt, crate::app::schema::RECOVERY_GUIDANCE),
        _ => {
            return Err(crate::CommonplaceError::Storage(
                "expected committed record, remove, withdraw, or schema freeze response".into(),
            ));
        }
    };
    let write = (|| -> std::result::Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_vec_pretty(response)?;
        output.write_all(&json)?;
        output.write_all(b"\n")?;
        output.flush()?;
        Ok(())
    })();
    write.map_err(|error| {
        crate::CommonplaceError::Storage(format!(
            "{receipt}; response delivery failed: {error}. {guidance}"
        ))
    })
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
        Command::Search {
            query,
            since,
            source_types,
            limit,
        } => {
            let cache = std::env::var_os("COMMONPLACE_MODEL_CACHE").map(PathBuf::from);
            let mut embedding =
                crate::providers::embeddings::LocalEmbeddingModel::new(cache.clone(), 1);
            let mut reranker = crate::providers::reranker::LocalReranker::new(cache);
            Ok(CommandResponse::new(
                "search",
                "complete",
                CommandResult::Search(crate::app::search::search(
                    &cli.store,
                    &crate::domain::search::SearchRequest {
                        query,
                        since,
                        source_types,
                        limit,
                    },
                    &mut embedding,
                    &mut reranker,
                )?),
            ))
        }
        Command::Record(args) => record::execute(&cli.store, args),
        Command::Remove(args) => remove::execute(&cli.store, args),
        Command::Withdraw(args) => withdraw::execute(&cli.store, args),
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
            command: SchemaCommand::Freeze,
        } => {
            let result = schema::freeze(&cli.store, &schema::OperationConfig::default())?;
            let status = if result.created {
                "complete"
            } else {
                "unchanged"
            };
            Ok(CommandResponse::new(
                "schema.freeze",
                status,
                CommandResult::Freeze(result),
            ))
        }
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
            Self::Search { .. } => "search",
            Self::Get { .. } => "get",
            Self::Record(args) if args.describe => "record.describe",
            Self::Record(_) => "record",
            Self::Remove(args) if args.describe => "remove.describe",
            Self::Remove(_) => "remove",
            Self::Withdraw(args) if args.describe => "withdraw.describe",
            Self::Withdraw(_) => "withdraw",
            Self::Schema {
                command: SchemaCommand::Show,
            } => "schema.show",
            Self::Schema {
                command: SchemaCommand::Freeze,
            } => "schema.freeze",
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
    fn committed_output_write_and_flush_failures_report_recovery_guidance() {
        struct FailingOutput(bool);
        impl std::io::Write for FailingOutput {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                if self.0 {
                    Ok(bytes.len())
                } else {
                    Err(std::io::Error::other("write failed"))
                }
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Err(std::io::Error::other("flush failed"))
            }
        }
        let response = super::CommandResponse::new(
            "record",
            "complete",
            super::CommandResult::Record(crate::app::record::RecordResult {
                schema_version: 1,
                knowledge_version: 2,
                summary: Default::default(),
                items: vec![],
                receipt: "record committed at knowledge_version 2 (knowledge:1)".into(),
            }),
        );
        for flush in [false, true] {
            let error =
                super::write_committed_response(&response, &mut FailingOutput(flush)).unwrap_err();
            assert_eq!(error.code(), "internal_error");
            assert!(
                error
                    .to_string()
                    .contains("record committed at knowledge_version 2 (knowledge:1)")
            );
            assert!(error.to_string().contains("Do not retry record"));
            assert!(!error.to_string().contains("cleanup"));
        }
        let response = super::CommandResponse::new(
            "schema.freeze",
            "complete",
            super::CommandResult::Freeze(crate::app::schema::FreezeResult {
                schema_version: 3,
                frozen: true,
                created: true,
                receipt: "schema freeze is durable at schema_version 3".into(),
            }),
        );
        for flush in [false, true] {
            let error =
                super::write_committed_response(&response, &mut FailingOutput(flush)).unwrap_err();
            assert_eq!(error.code(), "internal_error");
            assert!(
                error
                    .to_string()
                    .contains("schema freeze is durable at schema_version 3")
            );
            assert!(error.to_string().contains("Rerun schema freeze"));
        }
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("store");
        let source = directory.path().join("empty.txt");
        std::fs::write(&source, "").unwrap();
        let run = |args: &[&str]| {
            execute(
                Cli::try_parse_from(
                    ["commonplace", "--store", root.to_str().unwrap()]
                        .into_iter()
                        .chain(args.iter().copied()),
                )
                .unwrap(),
            )
            .unwrap()
        };
        run(&["init"]);
        let ingested = run(&["ingest", source.to_str().unwrap()]);
        let super::CommandResult::Ingest(ingested) = ingested.result else {
            panic!("ingest");
        };
        let key = ingested.items[0].source_key.as_deref().unwrap();
        let response = run(&["remove", "--source-key", key]);
        for flush in [false, true] {
            let error =
                super::write_committed_response(&response, &mut FailingOutput(flush)).unwrap_err();
            assert_eq!(error.code(), "internal_error");
            assert!(
                error
                    .to_string()
                    .contains("remove committed at knowledge_version 1")
            );
            assert!(error.to_string().contains("removed doc:1"));
            assert!(error.to_string().contains(key));
            assert!(error.to_string().contains("Do not retry remove"));
            assert!(error.to_string().contains("not_found"));
            assert!(!error.to_string().contains("record"));
            assert!(!error.to_string().contains("rebuild"));
            assert_eq!(
                crate::app::get::get(&root, "doc:1").unwrap_err().code(),
                "not_found"
            );
            crate::graph::GraphRuntime::open(&root).unwrap();
        }
        let schema = directory.path().join("schema.json");
        std::fs::write(&schema, r#"{"entity_types":[{"name":"person"}]}"#).unwrap();
        run(&["schema", "apply", schema.to_str().unwrap()]);
        let input = directory.path().join("record.json");
        std::fs::write(&input, r#"{"items":[{"kind":"entity","ref":"p","name":"P"},{"kind":"type_membership","entity":{"ref":"p"},"entity_type":"person"}]}"#).unwrap();
        run(&["record", input.to_str().unwrap()]);
        let input = directory.path().join("withdraw.json");
        std::fs::write(&input, r#"{"knowledge_ids":["knowledge:1"]}"#).unwrap();
        let response = run(&["withdraw", input.to_str().unwrap()]);
        for flush in [false, true] {
            let error =
                super::write_committed_response(&response, &mut FailingOutput(flush)).unwrap_err();
            assert_eq!(error.code(), "internal_error");
            assert!(
                error
                    .to_string()
                    .contains("withdraw committed at knowledge_version 3 (knowledge:1)")
            );
            assert!(error.to_string().contains("Do not retry withdraw"));
            assert!(!error.to_string().contains("rebuild"));
            assert!(!error.to_string().contains("cleanup"));
            assert!(!error.to_string().contains("retry record"));
            let history =
                serde_json::to_value(crate::app::get::get(&root, "knowledge:1").unwrap()).unwrap();
            assert!(history["withdrawn_at"].is_string());
            crate::graph::GraphRuntime::open(&root).unwrap();
        }
    }

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
                vec!["commonplace", "schema", "freeze", "--help"],
                "Permanently freeze this store's current vocabulary",
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

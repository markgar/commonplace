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
    /// Knowledge-base path; overrides COMMONPLACE_STORE and user configuration.
    #[arg(long, global = true)]
    store: Option<PathBuf>,

    /// Strict offline pinned-model cache; overrides COMMONPLACE_MODEL_CACHE and user configuration.
    #[arg(long, global = true)]
    model_cache: Option<PathBuf>,

    /// Emit JSON (also the default).
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Inspect effective path configuration without opening the store or loading models.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
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
        #[arg(
            required_unless_present = "describe_scope",
            conflicts_with = "describe_scope"
        )]
        query: Option<String>,
        /// Describe the strict scope JSON input without opening a store or loading models.
        #[arg(long)]
        describe_scope: bool,
        /// Path to a JSON document scope file (not inline JSON); use - for stdin.
        #[arg(long, conflicts_with = "describe_scope")]
        scope: Option<PathBuf>,
        /// Group retained passages by source with full exact text; limit still counts passages.
        #[arg(long, conflicts_with = "describe_scope")]
        grouped: bool,
        /// Literal contiguous passage phrase; Unicode lowercase matching, no normalization.
        #[arg(long, conflicts_with = "describe_scope")]
        must_contain: Option<String>,
        /// Inclusive dated source cutoff (RFC3339), plus timeless context; excludes unknown dates and reports coverage.
        #[arg(long, conflicts_with = "describe_scope")]
        since: Option<String>,
        /// Inclusive RFC3339 upper event bound, plus timeless context; excludes unknown dates.
        #[arg(long, conflicts_with = "describe_scope")]
        until: Option<String>,
        /// Exact source type; repeat to match any supplied type.
        #[arg(long = "source-type", conflicts_with = "describe_scope")]
        source_types: Vec<String>,
        /// Retained results, 0 through 50; required models are checked even at zero.
        #[arg(long, default_value_t = crate::domain::search::DEFAULT_RESULT_LIMIT, conflicts_with = "describe_scope")]
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
    /// Discover or resolve authoritative entity metadata without writes or inference.
    Entity {
        #[command(subcommand)]
        command: EntityCommand,
    },
    /// Apply or inspect your vocabulary (entity types, predicates, and identifiers).
    Schema {
        #[command(subcommand)]
        command: SchemaCommand,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    /// Show discovered configuration and effective values with their sources.
    Show,
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
        /// Emit a reusable document scope from this SELECT column (variable name without ?).
        #[arg(long)]
        document_scope: Option<String>,
    },
    /// Rebuild from committed SQLite, repairing derived state only.
    Rebuild,
}

#[derive(Debug, Subcommand)]
enum EntityCommand {
    /// List metadata in canonical ID order, optionally filtering active types or missing identifiers.
    List {
        #[arg(long = "type")]
        entity_type: Option<String>,
        #[arg(long)]
        missing_identifier: Option<String>,
        /// Resume strictly after this canonical entity ID.
        #[arg(long)]
        after: Option<String>,
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    /// Resolve an exact canonical name/alias, scheme/value identifier, or entity ID; ambiguity is an error.
    Resolve {
        #[arg(long, required_unless_present_any = ["scheme", "id"], conflicts_with_all = ["scheme", "value", "id"])]
        name: Option<String>,
        #[arg(long, requires = "value", conflicts_with_all = ["name", "id"])]
        scheme: Option<String>,
        #[arg(long, requires = "scheme")]
        value: Option<String>,
        #[arg(long, conflicts_with_all = ["name", "scheme", "value"])]
        id: Option<String>,
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
    /// Read your complete vocabulary, not the RDF mapping (see graph schema).
    Show,
    /// Permanently freeze this store's current vocabulary.
    Freeze,
}

pub fn main() -> ExitCode {
    let cli = Cli::parse();
    let operation = cli.command.operation();
    ExitCode::from(deliver(
        operation,
        execute(cli),
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    ))
}

fn deliver(
    operation: &'static str,
    result: Result<CommandResponse>,
    output: &mut impl Write,
    errors: &mut impl Write,
) -> u8 {
    let error = match result {
        Ok(response) => match write_response(&response, output) {
            Ok(()) => return response.exit_code(),
            Err(error) => error,
        },
        Err(error) => error,
    };
    if write_json(&ErrorResponse::new(operation, &error), errors).is_err() {
        // With stderr unavailable there is no remaining diagnostic channel.
        return 1;
    }
    error.exit_code()
}

fn write_json(value: &impl serde::Serialize, output: &mut impl Write) -> Result<()> {
    let json = serde_json::to_vec_pretty(value)?;
    output.write_all(&json)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}

fn write_response(response: &CommandResponse, output: &mut impl Write) -> Result<()> {
    write_json(response, output).map_err(|error| {
        let context = delivery_context(response);
        crate::CommonplaceError::Storage(format!("{context}; response delivery failed: {error}"))
    })
}

fn delivery_context(response: &CommandResponse) -> String {
    match &response.result {
        CommandResult::Record(record) => {
            format!(
                "{}. {}",
                record.receipt,
                crate::app::record::RECOVERY_GUIDANCE
            )
        }
        CommandResult::Remove(removed) => {
            format!(
                "{}. {}",
                removed.receipt,
                crate::app::remove::RECOVERY_GUIDANCE
            )
        }
        CommandResult::Withdraw(withdrawn) => {
            format!(
                "{}. {}",
                withdrawn.receipt,
                crate::app::withdraw::RECOVERY_GUIDANCE
            )
        }
        CommandResult::Freeze(frozen) => {
            format!(
                "{}. {}",
                frozen.receipt,
                crate::app::schema::RECOVERY_GUIDANCE
            )
        }
        CommandResult::Init(initialized) => format!(
            "store {} at {}; rerun init to validate this store",
            if initialized.created {
                "created"
            } else {
                "validated"
            },
            initialized.path.display()
        ),
        CommandResult::Apply(applied) if !applied.checked => format!(
            "schema apply {} at schema_version {}; inspect schema show before retrying",
            if applied.changed {
                "committed"
            } else {
                "was unchanged"
            },
            applied.schema_version
        ),
        CommandResult::GraphRebuild(rebuilt) => format!(
            "graph rebuild completed at knowledge_version {}; inspect graph query before deciding whether recovery is needed",
            rebuilt.knowledge_version
        ),
        CommandResult::Ingest(ingested) => {
            let mut receipt = format!(
                "ingestion finished: {} added, {} updated, {} unchanged, {} failed. Successful items remain committed; do not blindly replay the batch; inspect canonical IDs/source keys before retrying individual failures",
                ingested.summary.added,
                ingested.summary.updated,
                ingested.summary.unchanged,
                ingested.summary.failed
            );
            for item in &ingested.items {
                use std::fmt::Write as _;
                let _ = write!(
                    receipt,
                    "; input {:?}, source_key {:?}: {}, document {}, revision {}, passages [{}]",
                    item.input,
                    item.source_key,
                    item.status,
                    item.document_id
                        .map_or_else(|| "none".into(), |id| id.to_string()),
                    item.revision_id
                        .map_or_else(|| "none".into(), |id| id.to_string()),
                    item.passage_ids
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                if let Some(error) = &item.error {
                    let _ = write!(
                        receipt,
                        ", {} / {}: {}",
                        error.stage, error.code, error.message
                    );
                }
            }
            receipt
        }
        _ => format!(
            "{} completed without mutation; rerun this read-only operation",
            response.operation
        ),
    }
}

fn execute(cli: Cli) -> Result<CommandResponse> {
    let config = crate::config::resolve(cli.store, cli.model_cache)?;
    execute_command(cli.command, config)
}

fn execute_command(
    command: Command,
    config: crate::config::ResolvedConfig,
) -> Result<CommandResponse> {
    match command {
        Command::Config {
            command: ConfigCommand::Show,
        } => Ok(CommandResponse::new(
            "config.show",
            "complete",
            CommandResult::Config(config.report),
        )),
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
                document_scope,
            } => {
                let result = graph::query(
                    &config.store,
                    &query,
                    crate::graph::QueryConfig {
                        row_limit,
                        timeout: std::time::Duration::from_millis(timeout_ms),
                    },
                )?;
                let result = if let Some(column) = document_scope {
                    CommandResult::DocumentScope(result.document_scope(&column)?)
                } else {
                    CommandResult::GraphQuery(result)
                };
                Ok(CommandResponse::new("graph.query", "complete", result))
            }
            GraphCommand::Rebuild => Ok(CommandResponse::new(
                "graph.rebuild",
                "complete",
                CommandResult::GraphRebuild(graph::rebuild(&config.store)?),
            )),
        },
        Command::Ingest(args) => ingest::execute(&config.store, config.model_cache, *args),
        Command::Search {
            query,
            must_contain,
            since,
            until,
            scope,
            grouped,
            describe_scope,
            source_types,
            limit,
        } => {
            if describe_scope {
                return Ok(CommandResponse::new(
                    "search.describe_scope",
                    "complete",
                    CommandResult::Description(input::describe_scope()?),
                ));
            }
            let request = crate::domain::search::SearchRequest {
                query: query.ok_or_else(|| {
                    crate::CommonplaceError::InvalidInput("search query is required".into())
                })?,
                must_contain,
                since,
                until,
                scope: scope.as_deref().map(input::read_scope).transpose()?,
                source_types,
                limit,
            };
            let mut embedding = crate::providers::embeddings::LocalEmbeddingModel::new(
                config.model_cache.clone(),
                1,
            );
            let mut reranker = crate::providers::reranker::LocalReranker::new(config.model_cache);
            let result =
                crate::app::search::search(&config.store, &request, &mut embedding, &mut reranker)?;
            Ok(CommandResponse::new(
                "search",
                "complete",
                if grouped {
                    CommandResult::GroupedSearch(result.grouped())
                } else {
                    CommandResult::Search(result)
                },
            ))
        }
        Command::Entity { command } => match command {
            EntityCommand::List {
                entity_type,
                missing_identifier,
                after,
                limit,
            } => Ok(CommandResponse::new(
                "entity.list",
                "complete",
                CommandResult::Entities(crate::app::entity::list(
                    &config.store,
                    entity_type.as_deref(),
                    missing_identifier.as_deref(),
                    after.as_deref(),
                    limit,
                )?),
            )),
            EntityCommand::Resolve {
                name,
                scheme,
                value,
                id,
            } => {
                let reference = if let Some(id) = id {
                    crate::domain::knowledge::EntityReference::Id { id }
                } else if let Some(name) = name {
                    crate::domain::knowledge::EntityReference::Name {
                        name: crate::domain::knowledge::IdentityText(name),
                    }
                } else {
                    crate::domain::knowledge::EntityReference::Identifier {
                        identifier: crate::domain::knowledge::Identifier {
                            scheme: crate::domain::schema::Name(scheme.ok_or_else(|| {
                                crate::CommonplaceError::InvalidInput(
                                    "identifier scheme is required".into(),
                                )
                            })?),
                            value: crate::domain::knowledge::IdentityText(value.ok_or_else(
                                || {
                                    crate::CommonplaceError::InvalidInput(
                                        "identifier value is required".into(),
                                    )
                                },
                            )?),
                        },
                    }
                };
                Ok(CommandResponse::new(
                    "entity.resolve",
                    "complete",
                    CommandResult::Entity(crate::app::entity::resolve(&config.store, &reference)?),
                ))
            }
        },
        Command::Record(args) => record::execute(&config.store, args),
        Command::Remove(args) => remove::execute(&config.store, args),
        Command::Withdraw(args) => withdraw::execute(&config.store, args),
        Command::Get { id } => Ok(CommandResponse::new(
            "get",
            "complete",
            CommandResult::Get(get::get(&config.store, &id)?),
        )),
        Command::Init => {
            let result = init::initialize(&config.store)?;
            Ok(CommandResponse::init(result))
        }
        Command::Schema {
            command: SchemaCommand::Show,
        } => Ok(CommandResponse::new(
            "schema.show",
            "complete",
            CommandResult::Vocabulary(schema::show(&config.store)?),
        )),
        Command::Schema {
            command: SchemaCommand::Freeze,
        } => {
            let result = schema::freeze(&config.store, &schema::OperationConfig::default())?;
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
            let operation_config = schema::OperationConfig::default();
            let input = input::read(&file, operation_config.maximum_input_bytes)?;
            let result = schema::apply(&config.store, input, check, &operation_config)?;
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
            Self::Config {
                command: ConfigCommand::Show,
            } => "config.show",
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
            Self::Search {
                describe_scope: true,
                ..
            } => "search.describe_scope",
            Self::Search { .. } => "search",
            Self::Entity {
                command: EntityCommand::List { .. },
            } => "entity.list",
            Self::Entity {
                command: EntityCommand::Resolve { .. },
            } => "entity.resolve",
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
    use super::execute_command;

    fn execute(cli: Cli) -> crate::Result<super::CommandResponse> {
        let store = cli.store.unwrap_or_else(|| ".commonplace".into());
        let model_cache = cli.model_cache;
        let config = crate::config::ResolvedConfig {
            store: store.clone(),
            model_cache: model_cache.clone(),
            report: crate::config::ConfigReport {
                config_file: crate::config::ConfigFileReport {
                    path: None,
                    status: crate::config::ConfigStatus::Unavailable,
                },
                store: crate::config::PathReport {
                    path: store,
                    source: crate::config::ValueSource::CommandLine,
                },
                model_cache: crate::config::OptionalPathReport {
                    path: model_cache,
                    source: crate::config::ValueSource::CommandLine,
                },
            },
        };
        execute_command(cli.command, config)
    }

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
            let error = super::write_response(&response, &mut FailingOutput(flush)).unwrap_err();
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
            let error = super::write_response(&response, &mut FailingOutput(flush)).unwrap_err();
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
        let ingested = run(&[
            "ingest",
            "--temporal-state",
            "unknown",
            source.to_str().unwrap(),
        ]);
        let super::CommandResult::Ingest(ingested) = ingested.result else {
            panic!("ingest");
        };
        let key = ingested.items[0].source_key.as_deref().unwrap();
        let response = run(&["remove", "--source-key", key]);
        for flush in [false, true] {
            let error = super::write_response(&response, &mut FailingOutput(flush)).unwrap_err();
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
            let error = super::write_response(&response, &mut FailingOutput(flush)).unwrap_err();
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
                vec!["commonplace", "config", "show", "--help"],
                "effective values with their sources",
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
    fn every_response_family_checks_write_partial_write_and_flush() {
        struct FailingOutput {
            remaining: usize,
        }
        impl std::io::Write for FailingOutput {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                if self.remaining == 0 {
                    return Err(std::io::ErrorKind::BrokenPipe.into());
                }
                let count = bytes.len().min(self.remaining);
                self.remaining -= count;
                Ok(count)
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Err(std::io::Error::other("flush failed"))
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("store");
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
        let input = directory.path().join("input.json");
        let input_path = input.to_str().unwrap();
        let mut responses = vec![run(&["init"]), run(&["init"])];
        std::fs::write(&input, r#"{"entity_types":[{"name":"person"}]}"#).unwrap();
        responses.push(run(&["schema", "apply", input_path, "--check"]));
        responses.push(run(&["schema", "apply", input_path]));
        responses.push(run(&["schema", "apply", input_path]));
        std::fs::write(&input, "{\"source_key\":\"empty\",\"text\":\"\",\"temporal_state\":\"unknown\"}\n{\"source_key\":\"change\",\"text\":\"\",\"temporal_state\":\"unknown\"}\n").unwrap();
        responses.push(run(&["ingest", "--jsonl", input_path]));
        std::fs::write(&input, "{\"source_key\":\"empty\",\"text\":\"\",\"temporal_state\":\"unknown\"}\n{\"source_key\":\"change\",\"text\":\"\",\"temporal_state\":\"unknown\",\"metadata\":{\"new\":true}}\n{\"source_key\":\"new\",\"text\":\"\",\"temporal_state\":\"unknown\"}\n{}\n").unwrap();
        let ingested = run(&["ingest", "--jsonl", input_path]);
        let receipt = super::delivery_context(&ingested);
        for required in [
            "1 added",
            "1 updated",
            "1 unchanged",
            "1 failed",
            "doc:1",
            "revision:1",
            "empty",
            "failed",
            "invalid_input",
            "do not blindly replay",
        ] {
            assert!(receipt.contains(required), "{receipt}");
        }
        assert!(!receipt.contains("DocumentId("), "{receipt}");
        responses.push(ingested);
        std::fs::write(&input, r#"{"items":[{"kind":"entity","ref":"p","name":"P"},{"kind":"type_membership","entity":{"ref":"p"},"entity_type":"person"}]}"#).unwrap();
        responses.push(run(&["record", input_path]));
        for args in [
            vec!["config", "show"],
            vec!["graph", "schema"],
            vec!["graph", "query", "SELECT ?s WHERE { ?s ?p ?o }"],
            vec![
                "graph",
                "query",
                "SELECT ?document WHERE { VALUES ?document { <urn:commonplace:doc:1> } }",
                "--document-scope",
                "document",
            ],
            vec!["get", "doc:1"],
            vec!["get", "entity:1"],
            vec!["get", "knowledge:1"],
            vec!["entity", "list"],
            vec!["entity", "resolve", "--name", "P"],
            vec!["schema", "show"],
            vec!["schema", "apply", "--describe"],
            vec!["ingest", "--describe"],
            vec!["record", "--describe"],
            vec!["withdraw", "--describe"],
            vec!["remove", "--describe"],
            vec!["search", "--describe-scope"],
        ] {
            responses.push(run(&args));
        }
        let search = || crate::domain::search::SearchResult {
            items: vec![],
            truncated: false,
            temporal_filter: None,
            scope: None,
        };
        responses.push(super::CommandResponse::new(
            "search",
            "complete",
            super::CommandResult::Search(search()),
        ));
        responses.push(super::CommandResponse::new(
            "search",
            "complete",
            super::CommandResult::GroupedSearch(search().grouped()),
        ));
        std::fs::write(&input, r#"{"knowledge_ids":["knowledge:1"]}"#).unwrap();
        responses.push(run(&["withdraw", input_path]));
        responses.push(run(&["remove", "--source-key", "empty"]));
        responses.push(run(&["graph", "rebuild"]));
        responses.push(run(&["schema", "freeze"]));
        for response in responses {
            let mutation = matches!(
                response.result,
                super::CommandResult::Init(_)
                    | super::CommandResult::Ingest(_)
                    | super::CommandResult::Record(_)
                    | super::CommandResult::Withdraw(_)
                    | super::CommandResult::Remove(_)
                    | super::CommandResult::GraphRebuild(_)
                    | super::CommandResult::Freeze(_)
            ) || matches!(&response.result, super::CommandResult::Apply(result) if !result.checked);
            for remaining in [0, 7, usize::MAX] {
                let error =
                    super::write_response(&response, &mut FailingOutput { remaining }).unwrap_err();
                assert_eq!(error.code(), "internal_error", "{response:?}");
                assert_eq!(error.exit_code(), 1);
                assert!(error.to_string().contains("response delivery failed"));
                assert!(!error.to_string().contains("cleanup"));
                if !mutation {
                    assert!(error.to_string().contains("without mutation"), "{error}");
                    assert!(!error.to_string().contains("committed"), "{error}");
                    assert!(!error.to_string().contains("rebuild"), "{error}");
                }
            }
        }
        assert!(run(&["get", "knowledge:1"]).status == "complete");
        let history =
            serde_json::to_value(crate::app::get::get(&root, "knowledge:1").unwrap()).unwrap();
        assert!(history["withdrawn_at"].is_string());
        assert_eq!(
            crate::app::get::get(&root, "doc:1").unwrap_err().code(),
            "not_found"
        );
        crate::graph::GraphRuntime::open(&root).unwrap();
        assert!(root.join("schema-freeze.json").is_file());
        for remaining in [0, 7, usize::MAX] {
            assert_eq!(
                super::deliver(
                    "get",
                    Err(crate::CommonplaceError::NotFound("missing".into())),
                    &mut Vec::new(),
                    &mut FailingOutput { remaining }
                ),
                1
            );
        }
        let mut stderr = Vec::new();
        assert_eq!(
            super::deliver(
                "init",
                Ok(run(&["init"])),
                &mut FailingOutput { remaining: 7 },
                &mut stderr
            ),
            1
        );
        let error: serde_json::Value = serde_json::from_slice(&stderr).unwrap();
        assert_eq!(error["error"]["code"], "internal_error");
        assert_eq!(error["operation"], "init");
    }

    #[test]
    fn serialization_failure_is_checked_before_writing() {
        struct CannotSerialize;
        impl serde::Serialize for CannotSerialize {
            fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("injected serialization failure"))
            }
        }
        let mut bytes = Vec::new();
        let error = super::write_json(&CannotSerialize, &mut bytes).unwrap_err();
        assert_eq!(error.code(), "internal_error");
        assert!(bytes.is_empty());
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

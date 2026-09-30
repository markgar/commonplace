use serde::Serialize;

use super::input::Description;
use crate::CommonplaceError;
use crate::app::init::InitResult;
use crate::app::schema::{ApplyResult, FreezeResult};
use crate::domain::schema::Vocabulary;

const CONTRACT_VERSION: &str = "1";

#[derive(Debug, Serialize)]
pub struct CommandResponse {
    pub operation: &'static str,
    pub contract_version: &'static str,
    pub status: &'static str,
    pub result: CommandResult,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum CommandResult {
    GraphQuery(crate::graph::SelectResult),
    GraphSchema(crate::graph::schema::GraphSchema),
    GraphRebuild(crate::app::graph::RebuildResult),
    Init(InitResult),
    Apply(ApplyResult),
    Freeze(FreezeResult),
    Vocabulary(Vocabulary),
    Description(Description),
    IngestDescription(super::ingest::IngestDescription),
    Ingest(crate::app::ingest::IngestResult),
    Get(crate::app::get::Record),
    Search(crate::domain::search::SearchResult),
    Record(crate::app::record::RecordResult),
    RecordDescription(super::record::Description),
    Remove(crate::app::remove::RemoveResult),
    RemoveDescription(Box<super::remove::Description>),
    Withdraw(crate::app::withdraw::WithdrawResult),
    WithdrawDescription(Box<super::withdraw::Description>),
}

impl CommandResponse {
    pub fn exit_code(&self) -> u8 {
        match &self.result {
            CommandResult::Ingest(result) => result.exit_code,
            _ => 0,
        }
    }
    pub fn new(operation: &'static str, status: &'static str, result: CommandResult) -> Self {
        Self {
            operation,
            contract_version: CONTRACT_VERSION,
            status,
            result,
        }
    }

    pub fn init(result: InitResult) -> Self {
        Self {
            operation: "init",
            contract_version: CONTRACT_VERSION,
            status: if result.created {
                "complete"
            } else {
                "unchanged"
            },
            result: CommandResult::Init(result),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    operation: &'static str,
    contract_version: &'static str,
    status: &'static str,
    error: ErrorBody,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    code: &'static str,
    message: String,
}

impl ErrorResponse {
    pub fn new(operation: &'static str, error: &CommonplaceError) -> Self {
        Self {
            operation,
            contract_version: CONTRACT_VERSION,
            status: "failed",
            error: ErrorBody {
                code: error.code(),
                message: error.to_string(),
            },
        }
    }
}

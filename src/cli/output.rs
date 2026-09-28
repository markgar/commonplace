use serde::Serialize;

use super::input::Description;
use crate::CommonplaceError;
use crate::app::init::InitResult;
use crate::app::schema::ApplyResult;
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
    Init(InitResult),
    Apply(ApplyResult),
    Vocabulary(Vocabulary),
    Description(Description),
}

impl CommandResponse {
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

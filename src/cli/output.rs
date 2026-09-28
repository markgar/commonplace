use serde::Serialize;

use crate::CommonplaceError;
use crate::app::init::InitResult;

const CONTRACT_VERSION: &str = "1";

#[derive(Debug, Serialize)]
pub struct CommandResponse {
    pub operation: &'static str,
    pub contract_version: &'static str,
    pub status: &'static str,
    pub result: InitResult,
}

impl CommandResponse {
    pub fn init(result: InitResult) -> Self {
        Self {
            operation: "init",
            contract_version: CONTRACT_VERSION,
            status: if result.created {
                "complete"
            } else {
                "unchanged"
            },
            result,
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

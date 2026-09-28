use thiserror::Error;

pub type Result<T> = std::result::Result<T, CommonplaceError>;

#[derive(Debug, Error)]
pub enum CommonplaceError {
    #[error("{0}")]
    Configuration(String),
    #[error("{0}")]
    InvalidInput(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    ModelUnavailable(String),
    #[error("{0}")]
    LimitExceeded(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Storage(String),
    #[error("{0}")]
    Graph(String),
    #[error("{0}")]
    PostCommitCleanup(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Serialization(#[from] serde_json::Error),
}

impl CommonplaceError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Configuration(_) => "configuration_error",
            Self::InvalidInput(_) => "invalid_input",
            Self::NotFound(_) => "not_found",
            Self::ModelUnavailable(_) => "model_unavailable",
            Self::LimitExceeded(_) => "limit_exceeded",
            Self::Conflict(_) => "conflict",
            Self::Storage(_) | Self::Io(_) | Self::Serialization(_) => "internal_error",
            Self::Graph(_) => "graph_unavailable",
            Self::PostCommitCleanup(_) => "post_commit_cleanup",
        }
    }

    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::Configuration(_)
            | Self::InvalidInput(_)
            | Self::LimitExceeded(_)
            | Self::NotFound(_) => 2,
            Self::Conflict(_) => 3,
            Self::Storage(_)
            | Self::Graph(_)
            | Self::PostCommitCleanup(_)
            | Self::Io(_)
            | Self::Serialization(_)
            | Self::ModelUnavailable(_) => 1,
        }
    }
}

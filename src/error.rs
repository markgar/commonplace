use thiserror::Error;

pub type Result<T> = std::result::Result<T, CommonplaceError>;

#[derive(Debug, Error)]
pub enum CommonplaceError {
    #[error("{0}")]
    InvalidInput(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Storage(String),
    #[error("{0}")]
    Graph(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Serialization(#[from] serde_json::Error),
}

impl CommonplaceError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput(_) => "invalid_input",
            Self::Conflict(_) => "conflict",
            Self::Storage(_) | Self::Io(_) | Self::Serialization(_) => "internal_error",
            Self::Graph(_) => "graph_unavailable",
        }
    }

    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::InvalidInput(_) => 2,
            Self::Conflict(_) => 3,
            Self::Storage(_) | Self::Graph(_) | Self::Io(_) | Self::Serialization(_) => 1,
        }
    }
}

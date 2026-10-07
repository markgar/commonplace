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
    pub(crate) fn validation(error: &jsonschema::ValidationError<'_>) -> Self {
        Self::InvalidInput(Self::validation_message(error))
    }
    pub(crate) fn context(self, context: impl std::fmt::Display) -> Self {
        let message = format!("{context}: {self}");
        match self {
            Self::Configuration(_) => Self::Configuration(message),
            Self::InvalidInput(_) => Self::InvalidInput(message),
            Self::NotFound(_) => Self::NotFound(message),
            Self::ModelUnavailable(_) => Self::ModelUnavailable(message),
            Self::LimitExceeded(_) => Self::LimitExceeded(message),
            Self::Conflict(_) => Self::Conflict(message),
            Self::Graph(_) => Self::Graph(message),
            Self::PostCommitCleanup(_) => Self::PostCommitCleanup(message),
            Self::Storage(_) | Self::Io(_) | Self::Serialization(_) => Self::Storage(message),
        }
    }
    fn validation_message(error: &jsonschema::ValidationError<'_>) -> String {
        use jsonschema::error::ValidationErrorKind as Kind;
        match &error.kind {
            Kind::AnyOf { context } | Kind::OneOfNotValid { context } => {
                // Prefer branches whose discriminator and required fields match the input.
                // The validator's error tree is the source of these constraints.
                let score = |branch: &Vec<jsonschema::ValidationError<'static>>| {
                    (
                        branch
                            .iter()
                            .filter(|error| matches!(error.kind, Kind::Constant { .. }))
                            .count(),
                        branch
                            .iter()
                            .filter(|error| matches!(error.kind, Kind::Required { .. }))
                            .count(),
                        branch
                            .iter()
                            .filter(|error| matches!(error.kind, Kind::AdditionalProperties { .. }))
                            .count(),
                    )
                };
                let minimum = context.iter().map(score).min();
                let branches = context
                    .iter()
                    .filter(|branch| Some(score(branch)) == minimum)
                    .map(|branch| {
                        branch
                            .iter()
                            .map(Self::validation_message)
                            .collect::<Vec<_>>()
                            .join("; ")
                    })
                    .collect::<Vec<_>>();
                if branches.is_empty() {
                    format!("{}: {}", error.instance_path, error.masked())
                } else {
                    format!(
                        "{}: accepted alternatives: {}",
                        error.instance_path,
                        branches.join(" OR ")
                    )
                }
            }
            _ => format!("{}: {}", error.instance_path, error.masked()),
        }
    }
    pub(crate) fn input_io(context: impl std::fmt::Display, error: std::io::Error) -> Self {
        use std::io::ErrorKind;
        let message = format!("{context}: {error}");
        match error.kind() {
            ErrorKind::NotFound
            | ErrorKind::NotADirectory
            | ErrorKind::IsADirectory
            | ErrorKind::PermissionDenied
            | ErrorKind::InvalidInput => Self::InvalidInput(message),
            _ => Self::Storage(message),
        }
    }

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

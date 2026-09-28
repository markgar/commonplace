mod projection;
mod query;
mod runtime;
pub mod schema;

pub use query::{QueryConfig, SelectResult};
pub(crate) use runtime::publish;
pub use runtime::{GraphRuntime, initialize, rebuild};

pub(crate) fn graph_error(error: impl std::fmt::Display) -> crate::CommonplaceError {
    crate::CommonplaceError::Graph(format!(
        "{error}; for a compatible store, run `commonplace graph rebuild`"
    ))
}

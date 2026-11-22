//! Error type shared by all crates.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum TvError {
    #[error("cancelled by user")]
    Cancelled,
    #[error("mesh would have {nodes} nodes (> max {max}); increase cell sizes / target error or max nodes")]
    MeshTooLarge { nodes: usize, max: usize },
    #[error("invalid parameter: {0}")]
    InvalidParam(String),
    #[error("solver failed: {0}")]
    Solver(String),
    #[error("{0}")]
    Synthesis(String),
}

pub type TvResult<T> = Result<T, TvError>;

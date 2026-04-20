use thiserror::Error;

#[derive(Debug, Error)]
pub enum HyperindexError {
    #[error("{0} is not implemented in the Phase 2 scaffold")]
    NotImplemented(&'static str),
    #[error("invalid config: {0}")]
    InvalidConfig(String),
    #[error("{0}")]
    Message(String),
}

pub type HyperindexResult<T> = Result<T, HyperindexError>;

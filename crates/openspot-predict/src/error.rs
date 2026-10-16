use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum PredictError {
    #[error("unknown material id '{0}'")]
    UnknownMaterial(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("prediction cancelled")]
    Cancelled,
}

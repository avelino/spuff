use thiserror::Error;

use crate::provider::ProviderError;

#[derive(Error, Debug)]
pub enum SpuffError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Provider error: {0}")]
    ProviderError(#[from] ProviderError),

    /// Legacy provider error for backward compatibility.
    /// New code should use `ProviderError` variants directly.
    #[error("Provider error: {0}")]
    Provider(String),

    #[error("SSH connection error: {0}")]
    Ssh(String),

    #[error("No active instance found")]
    NoActiveInstance,

    #[error("Cloud-init failed: {0}")]
    CloudInit(String),

    #[error("Build failed: {0}")]
    Build(String),

    #[error("API request failed: {0}")]
    Api(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Template error: {0}")]
    Template(#[from] tera::Error),

    #[error("Dialog error: {0}")]
    Dialog(#[from] dialoguer::Error),

    #[error("SSH protocol error: {0}")]
    SshProtocol(#[from] russh::Error),

    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, SpuffError>;

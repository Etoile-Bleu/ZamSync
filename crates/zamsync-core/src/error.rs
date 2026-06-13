#[derive(Debug, thiserror::Error)]
pub enum ZamError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Data corruption detected: {0}")]
    Corruption(String),
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Protocol error: {0}")]
    Protocol(String),
    #[error("Invalid configuration: {0}")]
    Config(String),
    #[error("Storage engine error: {0}")]
    Storage(String),
}

pub type ZamResult<T> = Result<T, ZamError>;

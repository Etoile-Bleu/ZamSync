use std::fmt;

/// Magic number for ZamSync WAL files: "ZAM!" in ASCII
pub const WAL_MAGIC: [u8; 4] = [0x5A, 0x41, 0x4D, 0x21];
pub const WAL_VERSION: u8 = 1;

/// Unique identifier for a node in the ZamSync network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NodeId(pub u32);

/// Monotonically increasing sequence number for events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SequenceNumber(pub u64);

impl SequenceNumber {
    pub const ZERO: Self = Self(0);
    
    pub fn next(&self) -> Self {
        Self(self.0 + 1)
    }
}

impl fmt::Display for SequenceNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Common error types for the ZamSync system.
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

/// Represents a validated chunk of data for transport.
#[derive(Debug, Clone)]
pub struct Chunk {
    pub seq: SequenceNumber,
    pub data: Vec<u8>,
    pub crc: u32,
}

pub mod access;
pub mod error;
pub mod event;
pub mod ports;
pub mod sync;
pub mod validation;

pub use access::AccessPolicy;
pub use error::{ZamError, ZamResult};
pub use event::{
    Chunk, Event, Hlc, NodeId, SequenceNumber, WAL_MAGIC, WAL_VERSION, WAL_VERSION_ENCRYPTED,
};
pub use sync::{PeerSyncState, ReplicationState, SyncMessage, VersionVector};
pub use validation::PayloadSchema;

/// Reserved event_type for conflict-resolution records emitted by `zamsync resolve`.
/// Application event types should avoid this value.
pub const EVENT_TYPE_CONFLICT_RESOLUTION: u32 = 0xffff;

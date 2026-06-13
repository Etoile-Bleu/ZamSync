use std::fmt;
use rkyv::{Archive, Deserialize, Serialize};

pub const WAL_MAGIC: [u8; 4] = [0x5A, 0x41, 0x4D, 0x21];
pub const WAL_VERSION: u8 = 1;

#[derive(Archive, Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[archive(check_bytes)]
pub struct Hlc {
    pub physical: u64,
    pub logical: u32,
}

impl Hlc {
    pub fn new(physical: u64, logical: u32) -> Self {
        Self { physical, logical }
    }

    pub fn tick(&mut self, now_ms: u64) {
        if now_ms > self.physical {
            self.physical = now_ms;
            self.logical = 0;
        } else {
            self.logical += 1;
        }
    }

    pub fn sync(&mut self, now_ms: u64, remote: &Hlc) {
        let max_phys = now_ms.max(self.physical).max(remote.physical);
        if max_phys == self.physical && max_phys == remote.physical {
            self.logical = self.logical.max(remote.logical) + 1;
        } else if max_phys == self.physical {
            self.logical += 1;
        } else if max_phys == remote.physical {
            self.physical = remote.physical;
            self.logical = remote.logical + 1;
        } else {
            self.physical = max_phys;
            self.logical = 0;
        }
    }
}

#[derive(Archive, Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[archive(check_bytes)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NodeId(pub u32);

#[derive(Archive, Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[archive(check_bytes)]
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

#[derive(Archive, Deserialize, Serialize, Debug, Clone)]
#[archive(check_bytes)]
pub struct Event {
    pub origin_node: NodeId,
    pub seq: SequenceNumber,
    pub hlc: Hlc,
    pub event_type: u32,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Chunk {
    pub seq: SequenceNumber,
    pub data: Vec<u8>,
    pub crc: u32,
}

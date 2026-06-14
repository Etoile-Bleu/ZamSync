use rkyv::{Archive, Deserialize, Serialize};
use std::fmt;

pub const WAL_MAGIC: [u8; 4] = [0x5A, 0x41, 0x4D, 0x21];
pub const WAL_VERSION: u8 = 1;
/// WAL records with this version byte have their payload encrypted with
/// ChaCha20-Poly1305. Format: [nonce: 12][ciphertext][tag: 16].
pub const WAL_VERSION_ENCRYPTED: u8 = 2;

#[derive(
    Archive,
    Deserialize,
    Serialize,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
)]
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

#[derive(
    Archive,
    Deserialize,
    Serialize,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
)]
#[archive(check_bytes)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NodeId(pub u32);

#[derive(
    Archive,
    Deserialize,
    Serialize,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hlc_tick_advances_physical_clock() {
        let mut h = Hlc::new(100, 5);
        h.tick(200);
        assert_eq!(h.physical, 200);
        assert_eq!(h.logical, 0);
    }

    #[test]
    fn test_hlc_tick_same_wall_increments_logical() {
        let mut h = Hlc::new(100, 0);
        h.tick(100);
        assert_eq!(h.physical, 100);
        assert_eq!(h.logical, 1);
        h.tick(100);
        assert_eq!(h.logical, 2);
    }

    #[test]
    fn test_hlc_tick_clock_rollback_does_not_regress() {
        // Wall clock rolls back (e.g. NTP correction) -- HLC must not go backward.
        let mut h = Hlc::new(1000, 0);
        h.tick(50);
        assert_eq!(h.physical, 1000, "physical must not decrease");
        assert_eq!(h.logical, 1, "logical increments when wall is behind physical");
    }

    #[test]
    fn test_hlc_tick_sequence_is_strictly_monotonic() {
        let mut h = Hlc::default();
        let mut prev = h;
        // Interleave advancing and stale wall-clock readings.
        for now in [100u64, 100, 100, 200, 200, 50, 300, 300, 300, 400] {
            h.tick(now);
            assert!(h > prev, "HLC must be strictly monotonic at wall={now}");
            prev = h;
        }
    }

    #[test]
    fn test_hlc_sync_remote_physical_ahead() {
        let mut local = Hlc::new(100, 0);
        let remote = Hlc::new(200, 5);
        local.sync(100, &remote);
        // max_phys = 200 (remote wins)
        assert_eq!(local.physical, 200);
        assert_eq!(local.logical, 6); // remote.logical + 1
    }

    #[test]
    fn test_hlc_sync_local_physical_ahead() {
        let mut local = Hlc::new(500, 3);
        let remote = Hlc::new(100, 99);
        local.sync(100, &remote);
        // max_phys = 500 (local wins)
        assert_eq!(local.physical, 500);
        assert_eq!(local.logical, 4); // self.logical + 1
    }

    #[test]
    fn test_hlc_sync_wall_clock_ahead_of_both() {
        let mut local = Hlc::new(100, 0);
        let remote = Hlc::new(100, 0);
        local.sync(500, &remote);
        // max_phys = 500 (wall clock is ahead of both local and remote)
        assert_eq!(local.physical, 500);
        assert_eq!(local.logical, 0); // fresh physical, logical resets
    }

    #[test]
    fn test_hlc_sync_tie_uses_max_logical() {
        let mut local = Hlc::new(100, 3);
        let remote = Hlc::new(100, 7);
        local.sync(100, &remote);
        // max_phys = 100 == local.physical == remote.physical
        assert_eq!(local.physical, 100);
        assert_eq!(local.logical, 8); // max(3, 7) + 1
    }

    #[test]
    fn test_hlc_sync_always_strictly_ahead_of_remote() {
        let mut h = Hlc::default();
        let remote = Hlc::new(999, 42);
        h.sync(1, &remote);
        assert!(h > remote, "local HLC must be strictly ahead of remote after sync");
    }

    #[test]
    fn test_hlc_total_order() {
        let a = Hlc::new(100, 5);
        let b = Hlc::new(100, 6);
        let c = Hlc::new(200, 0);
        assert!(a < b, "same physical, lower logical is smaller");
        assert!(b < c, "higher physical wins regardless of logical");
        assert!(a < c);
    }
}

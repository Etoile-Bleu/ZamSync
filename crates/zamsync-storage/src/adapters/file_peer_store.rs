use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zamsync_core::{NodeId, ReplicationState, VersionVector, ZamError, ZamResult};
use zamsync_core::ports::PeerStore;

pub struct FilePeerStore {
    path: PathBuf,
    self_id: NodeId,
}

impl FilePeerStore {
    pub fn open(path: impl AsRef<Path>, self_id: NodeId) -> ZamResult<Self> {
        Ok(Self {
            path: path.as_ref().to_path_buf(),
            self_id,
        })
    }
}

impl PeerStore for FilePeerStore {
    fn load(&self) -> ZamResult<ReplicationState> {
        if !self.path.exists() {
            return Ok(ReplicationState {
                self_id: self.self_id,
                local_vv: VersionVector::new(),
                peers: HashMap::new(),
            });
        }
        let mut file = File::open(&self.path)?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;
        rkyv::from_bytes(&buffer)
            .map_err(|e| ZamError::Storage(format!("Failed to load peer state: {}", e)))
    }

    fn save(&mut self, state: &ReplicationState) -> ZamResult<()> {
        let bytes = rkyv::to_bytes::<_, 1024>(state)
            .map_err(|e| ZamError::Serialization(e.to_string()))?;
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        Ok(())
    }
}

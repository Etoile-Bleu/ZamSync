use crate::encryption::EncryptionKey;
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use crc32fast::Hasher;
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Arc;
use tracing::warn;
use zamsync_core::{SequenceNumber, ZamError, ZamResult, WAL_MAGIC, WAL_VERSION, WAL_VERSION_ENCRYPTED};

/// WAL Header Size: 4 (Magic) + 1 (Ver) + 4 (CRC) + 8 (Seq) + 4 (Len) = 21 bytes.
pub const WAL_HEADER_SIZE: usize = 21;

#[derive(Debug)]
pub struct WalRecord {
    pub seq: SequenceNumber,
    pub payload: Vec<u8>,
}

pub struct WalWriter {
    file: BufWriter<File>,
    current_seq: SequenceNumber,
    encryption: Option<Arc<EncryptionKey>>,
}

impl WalWriter {
    pub fn open(path: impl AsRef<Path>, start_seq: SequenceNumber) -> ZamResult<Self> {
        Self::open_inner(path, start_seq, None)
    }

    pub fn open_encrypted(
        path: impl AsRef<Path>,
        start_seq: SequenceNumber,
        key: Arc<EncryptionKey>,
    ) -> ZamResult<Self> {
        Self::open_inner(path, start_seq, Some(key))
    }

    fn open_inner(
        path: impl AsRef<Path>,
        start_seq: SequenceNumber,
        encryption: Option<Arc<EncryptionKey>>,
    ) -> ZamResult<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self {
            file: BufWriter::new(file),
            current_seq: start_seq,
            encryption,
        })
    }

    pub fn next_seq(&self) -> SequenceNumber {
        self.current_seq
    }

    pub fn append(&mut self, payload: &[u8]) -> ZamResult<SequenceNumber> {
        let seq = self.current_seq;
        self.append_at_seq(seq, payload)?;
        Ok(seq)
    }

    pub fn append_at_seq(&mut self, seq: SequenceNumber, payload: &[u8]) -> ZamResult<()> {
        let (version, encoded) = match &self.encryption {
            Some(key) => (WAL_VERSION_ENCRYPTED, key.encrypt(payload)?),
            None => (WAL_VERSION, payload.to_vec()),
        };

        let len = encoded.len() as u32;

        let mut hasher = Hasher::new();
        hasher.update(&[version]);
        hasher.update(&seq.0.to_be_bytes());
        hasher.update(&len.to_be_bytes());
        hasher.update(&encoded);
        let crc = hasher.finalize();

        self.file.write_all(&WAL_MAGIC)?;
        self.file.write_u8(version)?;
        self.file.write_u32::<BigEndian>(crc)?;
        self.file.write_u64::<BigEndian>(seq.0)?;
        self.file.write_u32::<BigEndian>(len)?;
        self.file.write_all(&encoded)?;
        self.file.flush()?;

        if seq.next() > self.current_seq {
            self.current_seq = seq.next();
        }
        Ok(())
    }

    pub fn sync(&mut self) -> io::Result<()> {
        self.file.get_ref().sync_all()
    }
}

pub struct WalScanner {
    file: File,
    encryption: Option<Arc<EncryptionKey>>,
}

impl WalScanner {
    pub fn open(path: impl AsRef<Path>) -> ZamResult<Self> {
        Self::open_inner(path, None)
    }

    pub fn open_encrypted(
        path: impl AsRef<Path>,
        key: Arc<EncryptionKey>,
    ) -> ZamResult<Self> {
        Self::open_inner(path, Some(key))
    }

    fn open_inner(path: impl AsRef<Path>, encryption: Option<Arc<EncryptionKey>>) -> ZamResult<Self> {
        let file = File::open(path)?;
        Ok(Self { file, encryption })
    }

    pub fn scan(self) -> WalIterator {
        WalIterator {
            file: self.file,
            offset: 0,
            encryption: self.encryption,
        }
    }

    pub fn recover(path: impl AsRef<Path>) -> ZamResult<(Option<SequenceNumber>, u64)> {
        Self::recover_inner(path, None)
    }

    pub fn recover_encrypted(
        path: impl AsRef<Path>,
        key: Arc<EncryptionKey>,
    ) -> ZamResult<(Option<SequenceNumber>, u64)> {
        Self::recover_inner(path, Some(key))
    }

    fn recover_inner(
        path: impl AsRef<Path>,
        encryption: Option<Arc<EncryptionKey>>,
    ) -> ZamResult<(Option<SequenceNumber>, u64)> {
        if !path.as_ref().exists() {
            return Ok((None, 0));
        }
        let scanner = Self::open_inner(&path, encryption)?;
        let mut it = scanner.scan();
        let mut last_seq = None;
        let mut last_pos = 0;

        loop {
            match it.next() {
                Some(Ok(record)) => {
                    last_seq = Some(record.seq);
                    last_pos = it.offset;
                }
                None => break,
                Some(Err(e)) => {
                    warn!(pos = it.offset, error = %e, "WAL recovery stopped");
                    break;
                }
            }
        }

        Ok((last_seq, last_pos))
    }
}

pub struct WalIterator {
    file: File,
    pub offset: u64,
    encryption: Option<Arc<EncryptionKey>>,
}

impl Iterator for WalIterator {
    type Item = ZamResult<WalRecord>;

    fn next(&mut self) -> Option<Self::Item> {
        self.file.seek(SeekFrom::Start(self.offset)).ok()?;

        let mut header = [0u8; WAL_HEADER_SIZE];
        let bytes_read = self.file.read(&mut header).ok()?;

        if bytes_read == 0 {
            return None;
        }
        if bytes_read < WAL_HEADER_SIZE {
            return Some(Err(ZamError::Corruption("Partial header at EOF".into())));
        }

        let mut rdr = io::Cursor::new(&header);

        let mut magic = [0u8; 4];
        if let Err(e) = rdr.read_exact(&mut magic) {
            return Some(Err(e.into()));
        }
        if magic != WAL_MAGIC {
            return Some(Err(ZamError::Corruption(format!(
                "Invalid magic: {:?}", magic
            ))));
        }

        let version = match rdr.read_u8() {
            Ok(v) => v,
            Err(e) => return Some(Err(e.into())),
        };
        if version != WAL_VERSION && version != WAL_VERSION_ENCRYPTED {
            return Some(Err(ZamError::Corruption(format!(
                "Unsupported WAL version: {}", version
            ))));
        }

        let expected_crc = match rdr.read_u32::<BigEndian>() {
            Ok(v) => v,
            Err(e) => return Some(Err(e.into())),
        };
        let seq = match rdr.read_u64::<BigEndian>() {
            Ok(v) => v,
            Err(e) => return Some(Err(e.into())),
        };
        let seq = SequenceNumber(seq);
        let len = match rdr.read_u32::<BigEndian>() {
            Ok(v) => v,
            Err(e) => return Some(Err(e.into())),
        } as usize;

        let mut raw = vec![0u8; len];
        if let Err(e) = self.file.read_exact(&mut raw) {
            if e.kind() == io::ErrorKind::UnexpectedEof {
                return Some(Err(ZamError::Corruption("Partial payload at EOF".into())));
            }
            return Some(Err(e.into()));
        }

        // Verify CRC (covers encrypted bytes when version == 2)
        let mut hasher = Hasher::new();
        hasher.update(&[version]);
        hasher.update(&seq.0.to_be_bytes());
        hasher.update(&(len as u32).to_be_bytes());
        hasher.update(&raw);
        let actual_crc = hasher.finalize();
        if actual_crc != expected_crc {
            return Some(Err(ZamError::Corruption(format!(
                "CRC mismatch for seq {}: expected {}, got {}",
                seq, expected_crc, actual_crc
            ))));
        }

        // Decrypt if needed
        let payload = if version == WAL_VERSION_ENCRYPTED {
            match &self.encryption {
                Some(key) => match key.decrypt(&raw) {
                    Ok(p) => p,
                    Err(e) => return Some(Err(e)),
                },
                None => {
                    return Some(Err(ZamError::Config(
                        "WAL is encrypted but no key was provided -- use --key-file".into(),
                    )))
                }
            }
        } else {
            raw
        };

        self.offset += (WAL_HEADER_SIZE + len) as u64;
        Some(Ok(WalRecord { seq, payload }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_wal_roundtrip() -> ZamResult<()> {
        let dir = tempdir()?;
        let path = dir.path().join("test.wal");

        let mut writer = WalWriter::open(&path, SequenceNumber::ZERO)?;
        writer.append(b"hello")?;
        writer.append(b"world")?;
        writer.sync()?;

        let scanner = WalScanner::open(&path)?;
        let mut it = scanner.scan();

        let r1 = it.next().unwrap().unwrap();
        assert_eq!(r1.seq.0, 0);
        assert_eq!(r1.payload, b"hello");
        let r2 = it.next().unwrap().unwrap();
        assert_eq!(r2.seq.0, 1);
        assert_eq!(r2.payload, b"world");
        assert!(it.next().is_none());
        Ok(())
    }

    #[test]
    fn test_wal_encrypted_roundtrip() -> ZamResult<()> {
        let dir = tempdir()?;
        let path = dir.path().join("enc.wal");
        let key = Arc::new(EncryptionKey::generate()?);

        let mut writer = WalWriter::open_encrypted(&path, SequenceNumber::ZERO, Arc::clone(&key))?;
        writer.append(b"patient-data-secret")?;
        writer.append(b"another-record")?;
        writer.sync()?;

        // Reading WITHOUT key must fail
        let scanner = WalScanner::open(&path)?;
        let err = scanner.scan().next().unwrap().unwrap_err();
        assert!(
            matches!(err, ZamError::Config(_)),
            "expected Config error, got {err:?}"
        );

        // Reading WITH key must succeed
        let scanner = WalScanner::open_encrypted(&path, Arc::clone(&key))?;
        let mut it = scanner.scan();
        assert_eq!(it.next().unwrap().unwrap().payload, b"patient-data-secret");
        assert_eq!(it.next().unwrap().unwrap().payload, b"another-record");
        assert!(it.next().is_none());
        Ok(())
    }

    #[test]
    fn test_wal_recovery_partial_write() -> ZamResult<()> {
        let dir = tempdir()?;
        let path = dir.path().join("test.wal");

        {
            let mut writer = WalWriter::open(&path, SequenceNumber::ZERO)?;
            writer.append(b"valid")?;
            writer.sync()?;
        }

        {
            let mut f = OpenOptions::new().append(true).open(&path)?;
            f.write_all(&WAL_MAGIC)?;
            f.write_all(&[WAL_VERSION])?;
        }

        let (last_seq, pos) = WalScanner::recover(&path)?;
        assert_eq!(last_seq, Some(SequenceNumber(0)));
        let first_record_size = WAL_HEADER_SIZE + 5;
        assert_eq!(pos, first_record_size as u64);
        Ok(())
    }

    #[test]
    fn test_wal_corruption_detection() -> ZamResult<()> {
        let dir = tempdir()?;
        let path = dir.path().join("test.wal");

        let mut writer = WalWriter::open(&path, SequenceNumber::ZERO)?;
        writer.append(b"perfect")?;
        writer.sync()?;

        {
            let mut f = OpenOptions::new().write(true).open(&path)?;
            f.seek(SeekFrom::Start((WAL_HEADER_SIZE + 1) as u64))?;
            f.write_all(b"x")?;
        }

        let scanner = WalScanner::open(&path)?;
        let mut it = scanner.scan();
        match it.next().unwrap() {
            Err(ZamError::Corruption(msg)) => assert!(msg.contains("CRC mismatch")),
            other => panic!("Expected CRC mismatch error, got {:?}", other),
        }
        Ok(())
    }
}

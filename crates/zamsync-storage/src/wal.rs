use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use crc32fast::Hasher;
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;
use tracing::warn;
use zamsync_core::{SequenceNumber, ZamError, ZamResult, WAL_MAGIC, WAL_VERSION};

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
}

impl WalWriter {
    /// Opens or creates a WAL file.
    /// If it exists, it MUST be validated first to ensure no partial records at the end.
    pub fn open(path: impl AsRef<Path>, start_seq: SequenceNumber) -> ZamResult<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;

        Ok(Self {
            file: BufWriter::new(file),
            current_seq: start_seq,
        })
    }

    pub fn next_seq(&self) -> SequenceNumber {
        self.current_seq
    }

    pub fn append(&mut self, payload: &[u8]) -> ZamResult<SequenceNumber> {
        let seq = self.current_seq;
        let len = payload.len() as u32;

        // CRC covers: Version + Seq + Len + Payload
        let mut hasher = Hasher::new();
        hasher.update(&[WAL_VERSION]);
        hasher.update(&seq.0.to_be_bytes());
        hasher.update(&len.to_be_bytes());
        hasher.update(payload);
        let crc = hasher.finalize();

        // Write Header
        self.file.write_all(&WAL_MAGIC)?;
        self.file.write_u8(WAL_VERSION)?;
        self.file.write_u32::<BigEndian>(crc)?;
        self.file.write_u64::<BigEndian>(seq.0)?;
        self.file.write_u32::<BigEndian>(len)?;

        // Write Payload
        self.file.write_all(payload)?;

        // Flush to OS. Higher level should call sync() for durability.
        self.file.flush()?;

        self.current_seq = seq.next();
        Ok(seq)
    }

    pub fn sync(&mut self) -> io::Result<()> {
        self.file.get_ref().sync_all()
    }
}

pub struct WalScanner {
    file: File,
}

impl WalScanner {
    pub fn open(path: impl AsRef<Path>) -> ZamResult<Self> {
        let file = File::open(path)?;
        Ok(Self { file })
    }

    /// Scans the WAL and returns an iterator over valid records.
    /// It stops at the first corruption or partial write.
    pub fn scan(self) -> WalIterator {
        WalIterator {
            file: self.file,
            offset: 0,
        }
    }

    /// Returns the last valid sequence number and the position after it.
    pub fn recover(path: impl AsRef<Path>) -> ZamResult<(Option<SequenceNumber>, u64)> {
        if !path.as_ref().exists() {
            return Ok((None, 0));
        }
        let scanner = WalScanner::open(&path)?;
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
    offset: u64,
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

        // 1. Verify Magic
        let mut magic = [0u8; 4];
        if let Err(e) = rdr.read_exact(&mut magic) {
            return Some(Err(e.into()));
        }
        if magic != WAL_MAGIC {
            return Some(Err(ZamError::Corruption(format!(
                "Invalid magic: {:?}",
                magic
            ))));
        }

        // 2. Read Metadata
        let version = match rdr.read_u8() {
            Ok(v) => v,
            Err(e) => return Some(Err(e.into())),
        };
        if version != WAL_VERSION {
            return Some(Err(ZamError::Corruption(format!(
                "Unsupported version: {}",
                version
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

        // 3. Read Payload
        let mut payload = vec![0u8; len];
        if let Err(e) = self.file.read_exact(&mut payload) {
            if e.kind() == io::ErrorKind::UnexpectedEof {
                return Some(Err(ZamError::Corruption("Partial payload at EOF".into())));
            }
            return Some(Err(e.into()));
        }

        // 4. Verify CRC
        let mut hasher = Hasher::new();
        hasher.update(&[version]);
        hasher.update(&seq.0.to_be_bytes());
        hasher.update(&(len as u32).to_be_bytes());
        hasher.update(&payload);
        let actual_crc = hasher.finalize();

        if actual_crc != expected_crc {
            return Some(Err(ZamError::Corruption(format!(
                "CRC mismatch for sequence {}: expected {}, got {}",
                seq, expected_crc, actual_crc
            ))));
        }

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
    fn test_wal_recovery_partial_write() -> ZamResult<()> {
        let dir = tempdir()?;
        let path = dir.path().join("test.wal");

        {
            let mut writer = WalWriter::open(&path, SequenceNumber::ZERO)?;
            writer.append(b"valid")?;
            writer.sync()?;
        }

        // Manually append garbage (partial header)
        {
            let mut f = OpenOptions::new().append(true).open(&path)?;
            f.write_all(&WAL_MAGIC)?;
            f.write_all(&[WAL_VERSION])?;
            // Missing CRC, Seq, Len, Payload
        }

        let (last_seq, pos) = WalScanner::recover(&path)?;
        assert_eq!(last_seq, Some(SequenceNumber(0)));
        // Position should be exactly after the first record
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

        // Corrupt the payload
        {
            let mut f = OpenOptions::new().write(true).open(&path)?;
            f.seek(SeekFrom::Start((WAL_HEADER_SIZE + 1) as u64))?;
            f.write_all(b"x")?; // Change 'p' to 'x'
        }

        let scanner = WalScanner::open(&path)?;
        let mut it = scanner.scan();
        let res = it.next().unwrap();

        match res {
            Err(ZamError::Corruption(msg)) => assert!(msg.contains("CRC mismatch")),
            _ => panic!("Expected CRC mismatch error, got {:?}", res),
        }

        Ok(())
    }
}

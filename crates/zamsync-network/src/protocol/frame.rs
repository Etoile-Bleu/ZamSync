use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Read, Write};
use zamsync_core::{ZamError, ZamResult};

/// Maximum decompressed payload size accepted from a peer.
pub const MAX_FRAME_SIZE: u32 = 64 * 1024 * 1024;

/// Payloads below this size are sent uncompressed -- zstd overhead exceeds savings.
const COMPRESS_THRESHOLD: usize = 64;

const FLAG_RAW: u8 = 0x00;
const FLAG_ZSTD: u8 = 0x01;

/// Wire format:
///   [4 bytes] uint32 big-endian  -- total byte count that follows (flag + body)
///   [1 byte]  compression flag   -- 0x00 raw, 0x01 zstd
///   [N bytes] body               -- raw payload or zstd-compressed payload
pub fn write_frame(writer: &mut impl Write, payload: &[u8]) -> ZamResult<()> {
    if payload.len() as u64 >= MAX_FRAME_SIZE as u64 {
        return Err(ZamError::Protocol(format!(
            "frame payload too large: {} bytes (max {})",
            payload.len(),
            MAX_FRAME_SIZE - 1
        )));
    }

    let (flag, body): (u8, Vec<u8>) = if payload.len() >= COMPRESS_THRESHOLD {
        let compressed = zstd::encode_all(payload, 3)
            .map_err(|e| ZamError::Protocol(format!("zstd compress: {e}")))?;
        if compressed.len() < payload.len() {
            (FLAG_ZSTD, compressed)
        } else {
            (FLAG_RAW, payload.to_vec())
        }
    } else {
        (FLAG_RAW, payload.to_vec())
    };

    let total_len = 1u32 + body.len() as u32;
    writer.write_u32::<BigEndian>(total_len)?;
    writer.write_u8(flag)?;
    writer.write_all(&body)?;
    Ok(())
}

pub fn read_frame(reader: &mut impl Read) -> ZamResult<Vec<u8>> {
    let total_len = reader.read_u32::<BigEndian>()?;
    if total_len as u64 > MAX_FRAME_SIZE as u64 {
        return Err(ZamError::Protocol(format!(
            "received frame too large: {} bytes (max {})",
            total_len, MAX_FRAME_SIZE
        )));
    }

    if total_len == 0 {
        return Ok(vec![]);
    }

    let flag = reader.read_u8()?;
    let body_len = (total_len - 1) as usize;
    let mut body = vec![0u8; body_len];
    reader.read_exact(&mut body)?;

    match flag {
        FLAG_RAW => Ok(body),
        FLAG_ZSTD => zstd::decode_all(body.as_slice())
            .map_err(|e| ZamError::Protocol(format!("zstd decompress: {e}"))),
        other => Err(ZamError::Protocol(format!(
            "unknown frame flag: 0x{other:02x}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_frame_roundtrip_small() {
        let payload = b"hello world"; // < COMPRESS_THRESHOLD, sent raw
        let mut buf = Vec::new();
        write_frame(&mut buf, payload).unwrap();
        let decoded = read_frame(&mut Cursor::new(&buf)).unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn test_frame_roundtrip_empty() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &[]).unwrap();
        let decoded = read_frame(&mut Cursor::new(&buf)).unwrap();
        assert!(decoded.is_empty());
    }

    #[test]
    fn test_frame_compression_roundtrip() {
        // JSON-like payload that compresses well
        let payload: Vec<u8> = (0..512).map(|i| b"abcdefghij"[i % 10]).collect();
        let mut buf = Vec::new();
        write_frame(&mut buf, &payload).unwrap();

        // Wire bytes must be smaller than raw payload + overhead
        assert!(
            buf.len() < payload.len(),
            "compressed frame ({} bytes) should be smaller than raw payload ({} bytes)",
            buf.len(),
            payload.len()
        );

        let decoded = read_frame(&mut Cursor::new(&buf)).unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn test_frame_compression_flag_raw() {
        // Small payload -- flag byte must be FLAG_RAW
        let payload = b"hi";
        let mut buf = Vec::new();
        write_frame(&mut buf, payload).unwrap();
        // bytes 4..5 is the flag
        assert_eq!(buf[4], FLAG_RAW);
    }

    #[test]
    fn test_frame_compression_flag_zstd() {
        // Large repetitive payload -- flag byte must be FLAG_ZSTD
        let payload: Vec<u8> = vec![b'x'; 1024];
        let mut buf = Vec::new();
        write_frame(&mut buf, &payload).unwrap();
        assert_eq!(buf[4], FLAG_ZSTD);
    }
}

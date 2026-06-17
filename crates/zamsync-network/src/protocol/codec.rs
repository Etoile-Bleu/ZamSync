use super::frame;
use std::io::{Read, Write};
use zamsync_core::{SyncMessage, ZamError, ZamResult};

/// Encodes `msg` onto `writer` and returns the number of bytes written to the wire.
pub fn encode(msg: &SyncMessage, writer: &mut impl Write) -> ZamResult<usize> {
    let bytes =
        rkyv::to_bytes::<_, 1024>(msg).map_err(|e| ZamError::Serialization(e.to_string()))?;
    frame::write_frame(writer, &bytes)
}

pub fn decode(reader: &mut impl Read) -> ZamResult<SyncMessage> {
    let bytes = frame::read_frame(reader)?;
    rkyv::from_bytes::<SyncMessage>(&bytes).map_err(|e| ZamError::Serialization(format!("{}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use zamsync_core::{NodeId, VersionVector};

    #[test]
    fn test_handshake_roundtrip() {
        let msg = SyncMessage::Handshake {
            node_id: NodeId(42),
            vv: VersionVector::new(),
        };

        let mut buf = Vec::new();
        encode(&msg, &mut buf).unwrap();

        let mut cursor = Cursor::new(&buf);
        let decoded = decode(&mut cursor).unwrap();

        match decoded {
            SyncMessage::Handshake { node_id, .. } => assert_eq!(node_id.0, 42),
            _ => panic!("unexpected message type"),
        }
    }

    #[test]
    fn test_pull_request_roundtrip() {
        use zamsync_core::SequenceNumber;
        let msg = SyncMessage::PullRequest {
            origin_node: NodeId(1),
            start_seq: SequenceNumber(100),
            limit: 50,
        };

        let mut buf = Vec::new();
        encode(&msg, &mut buf).unwrap();

        let mut cursor = Cursor::new(&buf);
        let decoded = decode(&mut cursor).unwrap();

        match decoded {
            SyncMessage::PullRequest {
                origin_node,
                start_seq,
                limit,
            } => {
                assert_eq!(origin_node.0, 1);
                assert_eq!(start_seq.0, 100);
                assert_eq!(limit, 50);
            }
            _ => panic!("unexpected message type"),
        }
    }

    #[test]
    fn test_event_batch_roundtrip() {
        use zamsync_core::{Event, Hlc, SequenceNumber};
        let event = Event {
            origin_node: NodeId(3),
            seq: SequenceNumber(7),
            hlc: Hlc::new(9999, 0),
            event_type: 2,
            payload: b"payload".to_vec(),
        };
        let msg = SyncMessage::EventBatch {
            origin_node: NodeId(3),
            events: vec![event],
        };

        let mut buf = Vec::new();
        encode(&msg, &mut buf).unwrap();

        let mut cursor = Cursor::new(&buf);
        let decoded = decode(&mut cursor).unwrap();

        match decoded {
            SyncMessage::EventBatch {
                origin_node,
                events,
            } => {
                assert_eq!(origin_node.0, 3);
                assert_eq!(events.len(), 1);
                assert_eq!(events[0].payload, b"payload");
            }
            _ => panic!("unexpected message type"),
        }
    }
}

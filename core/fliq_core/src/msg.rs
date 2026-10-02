//! Control messages (CBOR over the control stream) and data frame headers (spec 6.4, 6.5).

use crate::consts::*;
use crate::error::{FliqError, Result};
use crate::noise::{SecureReader, SecureWriter};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{Read, Write};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct FileMeta {
    pub id: u32,
    pub name: String,
    pub size: u64,
    pub mtime: u64,
    pub chunk_count: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Ctrl {
    Hello {
        version: u64,
        ciphers: Vec<String>,
        device_name: String,
        os: String,
    },
    /// The server's role: "sender" or "receiver".
    Role {
        server_role: String,
    },
    Offer {
        files: Vec<FileMeta>,
    },
    Accept {
        files: Vec<u32>,
    },
    Reject {
        reason: String,
    },
    StreamsReady {
        count: u16,
    },
    /// Receiver's chunk bitmap for a file; the sender resends every chunk not set.
    Ack {
        id: u32,
        #[serde(with = "serde_bytes")]
        bitmap: Vec<u8>,
    },
    FileDone {
        id: u32,
        #[serde(with = "serde_bytes")]
        manifest_hash: Vec<u8>,
    },
    Verified {
        id: u32,
    },
    Complete,
    Close,
    Error {
        code: String,
        text: String,
    },
}

pub fn send_ctrl<W: Write>(w: &mut SecureWriter<W>, m: &Ctrl) -> Result<()> {
    let mut body = Vec::with_capacity(128);
    ciborium::into_writer(m, &mut body).map_err(|_| FliqError::Protocol("encode"))?;
    if body.len() > MAX_CONTROL_MSG {
        return Err(FliqError::Limit("Too many files in one transfer.".into()));
    }
    w.write(&(body.len() as u32).to_be_bytes())?;
    w.write(&body)?;
    w.flush()
}

pub fn recv_ctrl<R: Read>(r: &mut SecureReader<R>) -> Result<Ctrl> {
    let mut l = [0u8; 4];
    r.read_exact(&mut l)?;
    let n = u32::from_be_bytes(l) as usize;
    if n > MAX_CONTROL_MSG {
        return Err(FliqError::Protocol("oversized control message"));
    }
    let mut body = vec![0u8; n];
    r.read_exact(&mut body)?;
    decode_ctrl(&body)
}

/// Decode a control message body. Never panics; bounded by the caller's size cap.
pub fn decode_ctrl(body: &[u8]) -> Result<Ctrl> {
    let m: Ctrl = ciborium::from_reader(body).map_err(|_| FliqError::Protocol("malformed control message"))?;
    let ok = match &m {
        Ctrl::Hello { ciphers, device_name, os, .. } => {
            ciphers.len() <= 8 && ciphers.iter().all(|c| c.len() <= 32) && device_name.len() <= 256 && os.len() <= 64
        }
        Ctrl::Role { server_role } => server_role == "sender" || server_role == "receiver",
        Ctrl::Reject { reason } => reason.len() <= 512,
        Ctrl::Error { code, text } => code.len() <= 32 && text.len() <= 512,
        Ctrl::FileDone { manifest_hash, .. } => manifest_hash.len() == 32,
        Ctrl::StreamsReady { count } => *count <= MAX_DATA_STREAMS,
        _ => true,
    };
    if ok { Ok(m) } else { Err(FliqError::Protocol("invalid control message")) }
}

pub fn chunk_count(size: u64) -> u64 {
    size.div_ceil(CHUNK_SIZE)
}

pub fn chunk_len(size: u64, idx: u32) -> u64 {
    let start = idx as u64 * CHUNK_SIZE;
    (size - start).min(CHUNK_SIZE)
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_total: u64,
    pub max_files: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_total: DEFAULT_MAX_TOTAL_BYTES, max_files: DEFAULT_MAX_FILES }
    }
}

/// Receiver-side validation of an Offer (spec 6.6 limits).
pub fn validate_offer(files: &[FileMeta], lim: &Limits) -> Result<u64> {
    if files.is_empty() {
        return Err(FliqError::Protocol("empty offer"));
    }
    if files.len() > lim.max_files {
        return Err(FliqError::Limit(format!("Too many files: the limit is {} per transfer.", lim.max_files)));
    }
    let mut ids = HashSet::new();
    let mut total: u64 = 0;
    for f in files {
        if !ids.insert(f.id) || f.id == u32::MAX {
            return Err(FliqError::Protocol("duplicate file id"));
        }
        if f.name.is_empty() || f.name.len() > 4 * MAX_NAME_LEN {
            return Err(FliqError::Limit("A file name is too long.".into()));
        }
        let cc = chunk_count(f.size);
        if cc > u32::MAX as u64 || cc != f.chunk_count as u64 {
            return Err(FliqError::Protocol("chunk count mismatch"));
        }
        total = total.checked_add(f.size).ok_or(FliqError::Protocol("size overflow"))?;
    }
    if total > lim.max_total {
        return Err(FliqError::Limit(format!(
            "This transfer is larger than the limit of {:.2} GB.",
            lim.max_total as f64 / 1e9
        )));
    }
    Ok(total)
}

/// Data frame header: file_id, chunk_index, chunk_hash (BLAKE3), len. 44 bytes.
pub const DATA_HEADER_LEN: usize = 44;
/// file_id value that marks the clean end of a data stream.
pub const END_OF_STREAM: u32 = u32::MAX;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChunkHeader {
    pub file_id: u32,
    pub chunk_index: u32,
    pub chunk_hash: [u8; 32],
    pub len: u32,
}

impl ChunkHeader {
    pub fn encode(&self) -> [u8; DATA_HEADER_LEN] {
        let mut b = [0u8; DATA_HEADER_LEN];
        b[0..4].copy_from_slice(&self.file_id.to_be_bytes());
        b[4..8].copy_from_slice(&self.chunk_index.to_be_bytes());
        b[8..40].copy_from_slice(&self.chunk_hash);
        b[40..44].copy_from_slice(&self.len.to_be_bytes());
        b
    }
    pub fn decode(b: &[u8; DATA_HEADER_LEN]) -> Self {
        let mut h = [0u8; 32];
        h.copy_from_slice(&b[8..40]);
        ChunkHeader {
            file_id: u32::from_be_bytes(b[0..4].try_into().unwrap()),
            chunk_index: u32::from_be_bytes(b[4..8].try_into().unwrap()),
            chunk_hash: h,
            len: u32::from_be_bytes(b[40..44].try_into().unwrap()),
        }
    }
    pub fn end() -> Self {
        ChunkHeader { file_id: END_OF_STREAM, chunk_index: 0, chunk_hash: [0; 32], len: 0 }
    }
}

/// Manifest hash = BLAKE3(chunk_hash_0 || chunk_hash_1 || ...).
pub fn manifest_hash(chunk_hashes: &[[u8; 32]]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    for c in chunk_hashes {
        h.update(c);
    }
    *h.finalize().as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn chunk_math() {
        assert_eq!(chunk_count(0), 0);
        assert_eq!(chunk_count(1), 1);
        assert_eq!(chunk_count(CHUNK_SIZE), 1);
        assert_eq!(chunk_count(CHUNK_SIZE + 1), 2);
        assert_eq!(chunk_len(CHUNK_SIZE + 1, 0), CHUNK_SIZE);
        assert_eq!(chunk_len(CHUNK_SIZE + 1, 1), 1);
        assert_eq!(chunk_count(50 * 1024 * 1024 * 1024), 12_800);
    }

    #[test]
    fn header_roundtrip() {
        let h = ChunkHeader { file_id: 3, chunk_index: 9, chunk_hash: [5; 32], len: 77 };
        assert_eq!(ChunkHeader::decode(&h.encode()), h);
    }

    fn meta(id: u32, size: u64) -> FileMeta {
        FileMeta { id, name: "a".into(), size, mtime: 0, chunk_count: chunk_count(size) as u32 }
    }

    #[test]
    fn offer_limits() {
        let lim = Limits { max_total: 10 * CHUNK_SIZE, max_files: 3 };
        assert!(validate_offer(&[meta(1, 5)], &lim).is_ok());
        assert!(validate_offer(&[], &lim).is_err());
        assert!(validate_offer(&[meta(1, 5), meta(1, 5)], &lim).is_err());
        assert!(matches!(validate_offer(&[meta(1, 11 * CHUNK_SIZE)], &lim), Err(FliqError::Limit(_))));
        assert!(matches!(
            validate_offer(&[meta(1, 1), meta(2, 1), meta(3, 1), meta(4, 1)], &lim),
            Err(FliqError::Limit(_))
        ));
        let mut bad = meta(1, 5);
        bad.chunk_count = 7;
        assert!(validate_offer(&[bad], &lim).is_err());
        assert!(validate_offer(&[meta(1, u64::MAX), meta(2, 2)], &lim).is_err());
    }

    #[test]
    fn ctrl_roundtrip() {
        let m = Ctrl::FileDone { id: 1, manifest_hash: vec![1; 32] };
        let mut b = vec![];
        ciborium::into_writer(&m, &mut b).unwrap();
        assert_eq!(decode_ctrl(&b).unwrap(), m);
        let bad = Ctrl::FileDone { id: 1, manifest_hash: vec![1; 31] };
        let mut b = vec![];
        ciborium::into_writer(&bad, &mut b).unwrap();
        assert!(decode_ctrl(&b).is_err());
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(20_000))]
        #[test]
        fn decode_never_panics(data in proptest::collection::vec(any::<u8>(), 0..512)) {
            let _ = decode_ctrl(&data);
        }

        #[test]
        fn decode_mutated_valid_never_panics(idx in 0usize..64, val in any::<u8>()) {
            let m = Ctrl::Offer { files: vec![meta(1, 99), meta(2, 5)] };
            let mut b = vec![];
            ciborium::into_writer(&m, &mut b).unwrap();
            let i = idx % b.len();
            b[i] = val;
            let _ = decode_ctrl(&b);
        }
    }
}

#[cfg(test)]
mod frame_tests {
    use super::*;
    use crate::noise::{SecureReader, SecureWriter, tests::memory_pair};
    use std::io::Cursor;

    #[test]
    fn oversized_control_frame_rejected_before_allocation() {
        let (c, s) = memory_pair();
        let mut w = SecureWriter::new(Vec::new(), c.transport, None);
        w.write(&((MAX_CONTROL_MSG as u32) + 1).to_be_bytes()).unwrap();
        w.flush().unwrap();
        let mut r = SecureReader::new(Cursor::new(w.get_ref().clone()), s.transport, None);
        assert!(matches!(recv_ctrl(&mut r), Err(FliqError::Protocol(_))));
    }

    #[test]
    fn control_roundtrip_over_records() {
        let (c, s) = memory_pair();
        let mut w = SecureWriter::new(Vec::new(), c.transport, None);
        let m = Ctrl::Verified { id: 4 };
        send_ctrl(&mut w, &m).unwrap();
        let mut r = SecureReader::new(Cursor::new(w.get_ref().clone()), s.transport, None);
        assert_eq!(recv_ctrl(&mut r).unwrap(), m);
    }
}

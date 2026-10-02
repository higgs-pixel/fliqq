//! Noise_NKpsk0 handshake and the record layer (spec 6.2, 6.3).
//!
//! Wire format: every Noise message (handshake or transport) is prefixed with a 2-byte
//! big-endian length. Transport messages carry at most 65,519 plaintext bytes. Each
//! direction uses its own strictly increasing nonce counter, so a dropped, reordered,
//! replayed or modified record fails authentication.

use crate::consts::*;
use crate::error::{FliqError, Result};
use crate::kdf;
use crate::stats::{StageStats, Timer};
use snow::{Builder, StatelessTransportState};
use std::io::{self, BufReader, Read, Write};
use std::sync::Arc;
use zeroize::Zeroizing;

/// Flush the outgoing buffer once it reaches this size (bounds per-stream memory).
const WIRE_FLUSH_BYTES: usize = 1024 * 1024;
const READ_BUF_BYTES: usize = 256 * 1024;

pub fn io_err(e: io::Error) -> FliqError {
    use io::ErrorKind::*;
    match e.kind() {
        UnexpectedEof | ConnectionReset | ConnectionAborted | BrokenPipe | TimedOut | WouldBlock | NotConnected => {
            FliqError::ConnectionLost
        }
        _ => FliqError::Io(e),
    }
}

/// Generate the server's per-session static X25519 keypair. Private key is zeroized on drop.
pub fn generate_static_keypair() -> Result<(Zeroizing<Vec<u8>>, Vec<u8>)> {
    let params = NOISE_PARAMS.parse().map_err(|_| FliqError::Handshake)?;
    let kp = Builder::new(params).generate_keypair().map_err(|_| FliqError::Handshake)?;
    Ok((Zeroizing::new(kp.private), kp.public))
}

pub struct Handshaken {
    pub transport: Arc<StatelessTransportState>,
    pub hash: Zeroizing<Vec<u8>>,
}

fn prologue(sid: &[u8]) -> Vec<u8> {
    let mut p = NOISE_PROLOGUE_PREFIX.to_vec();
    p.extend_from_slice(sid);
    p
}

fn write_frame16<S: Write>(s: &mut S, msg: &[u8]) -> Result<()> {
    let mut buf = Vec::with_capacity(msg.len() + 2);
    buf.extend_from_slice(&(msg.len() as u16).to_be_bytes());
    buf.extend_from_slice(msg);
    s.write_all(&buf).map_err(io_err)?;
    s.flush().map_err(io_err)
}

fn read_frame16<S: Read>(s: &mut S, buf: &mut [u8]) -> Result<usize> {
    let mut l = [0u8; 2];
    s.read_exact(&mut l).map_err(io_err)?;
    let n = u16::from_be_bytes(l) as usize;
    if n > buf.len() {
        return Err(FliqError::Protocol("oversized handshake"));
    }
    s.read_exact(&mut buf[..n]).map_err(io_err)?;
    Ok(n)
}

/// Client side (the device that scanned the QR).
pub fn initiator<S: Read + Write>(s: &mut S, sid: &[u8], server_pk: &[u8], psk16: &[u8]) -> Result<Handshaken> {
    let psk = kdf::noise_psk(psk16, sid);
    let pro = prologue(sid);
    let params = NOISE_PARAMS.parse().map_err(|_| FliqError::Handshake)?;
    let mut hs = Builder::new(params)
        .remote_public_key(server_pk)
        .psk(0, psk.as_ref())
        .prologue(&pro)
        .build_initiator()
        .map_err(|_| FliqError::Handshake)?;
    let mut buf = vec![0u8; 1024];
    let n = hs.write_message(&[], &mut buf).map_err(|_| FliqError::Handshake)?;
    write_frame16(s, &buf[..n])?;
    let mut inb = vec![0u8; 1024];
    let n = read_frame16(s, &mut inb)?;
    hs.read_message(&inb[..n], &mut buf).map_err(|_| FliqError::Handshake)?;
    finish(hs)
}

/// Server side (the device that shows the QR).
pub fn responder<S: Read + Write>(s: &mut S, sid: &[u8], server_sk: &[u8], psk16: &[u8]) -> Result<Handshaken> {
    let psk = kdf::noise_psk(psk16, sid);
    let pro = prologue(sid);
    let params = NOISE_PARAMS.parse().map_err(|_| FliqError::Handshake)?;
    let mut hs = Builder::new(params)
        .local_private_key(server_sk)
        .psk(0, psk.as_ref())
        .prologue(&pro)
        .build_responder()
        .map_err(|_| FliqError::Handshake)?;
    let mut inb = vec![0u8; 1024];
    let n = read_frame16(s, &mut inb)?;
    let mut buf = vec![0u8; 1024];
    hs.read_message(&inb[..n], &mut buf).map_err(|_| FliqError::Handshake)?;
    let n = hs.write_message(&[], &mut buf).map_err(|_| FliqError::Handshake)?;
    write_frame16(s, &buf[..n])?;
    finish(hs)
}

fn finish(hs: snow::HandshakeState) -> Result<Handshaken> {
    let hash = Zeroizing::new(hs.get_handshake_hash().to_vec());
    let ts = hs.into_stateless_transport_mode().map_err(|_| FliqError::Handshake)?;
    Ok(Handshaken { transport: Arc::new(ts), hash })
}

pub struct SecureWriter<W: Write> {
    inner: W,
    ts: Arc<StatelessTransportState>,
    nonce: u64,
    wire: Vec<u8>,
    stats: Option<Arc<StageStats>>,
}

impl<W: Write> SecureWriter<W> {
    pub fn new(inner: W, ts: Arc<StatelessTransportState>, stats: Option<Arc<StageStats>>) -> Self {
        SecureWriter { inner, ts, nonce: 0, wire: Vec::new(), stats }
    }

    /// Encrypt `data` into buffered records. Call [`flush`] to put them on the wire.
    pub fn write(&mut self, mut data: &[u8]) -> Result<()> {
        while !data.is_empty() {
            let n = data.len().min(NOISE_MAX_PLAINTEXT);
            let start = self.wire.len();
            self.wire.resize(start + 2 + n + NOISE_TAG_LEN, 0);
            let t = Timer::start();
            let ct = self
                .ts
                .write_message(self.nonce, &data[..n], &mut self.wire[start + 2..])
                .map_err(|_| FliqError::Protocol("encrypt"))?;
            if let Some(s) = &self.stats {
                t.stop(&s.encrypt_ns);
            }
            self.nonce += 1;
            self.wire[start..start + 2].copy_from_slice(&(ct as u16).to_be_bytes());
            self.wire.truncate(start + 2 + ct);
            data = &data[n..];
            if self.wire.len() >= WIRE_FLUSH_BYTES {
                self.flush_wire()?;
            }
        }
        Ok(())
    }

    fn flush_wire(&mut self) -> Result<()> {
        let t = Timer::start();
        self.inner.write_all(&self.wire).map_err(io_err)?;
        if let Some(s) = &self.stats {
            t.stop(&s.net_send_ns);
        }
        self.wire.clear();
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        self.flush_wire()?;
        self.inner.flush().map_err(io_err)
    }

    pub fn set_stats(&mut self, stats: Option<Arc<StageStats>>) {
        self.stats = stats;
    }

    pub fn get_ref(&self) -> &W {
        &self.inner
    }
}

pub struct SecureReader<R: Read> {
    inner: BufReader<R>,
    ts: Arc<StatelessTransportState>,
    nonce: u64,
    ct: Vec<u8>,
    pt: Vec<u8>,
    pos: usize,
    len: usize,
    stats: Option<Arc<StageStats>>,
}

impl<R: Read> SecureReader<R> {
    pub fn new(inner: R, ts: Arc<StatelessTransportState>, stats: Option<Arc<StageStats>>) -> Self {
        SecureReader {
            inner: BufReader::with_capacity(READ_BUF_BYTES, inner),
            ts,
            nonce: 0,
            ct: vec![0; NOISE_MAX_MSG],
            pt: vec![0; NOISE_MAX_PLAINTEXT],
            pos: 0,
            len: 0,
            stats,
        }
    }

    pub fn set_stats(&mut self, stats: Option<Arc<StageStats>>) {
        self.stats = stats;
    }

    /// Read and decrypt exactly `out.len()` plaintext bytes.
    pub fn read_exact(&mut self, mut out: &mut [u8]) -> Result<()> {
        while !out.is_empty() {
            if self.pos < self.len {
                let n = out.len().min(self.len - self.pos);
                out[..n].copy_from_slice(&self.pt[self.pos..self.pos + n]);
                self.pos += n;
                out = &mut out[n..];
                continue;
            }
            let t = Timer::start();
            let mut l = [0u8; 2];
            self.inner.read_exact(&mut l).map_err(io_err)?;
            let ct_len = u16::from_be_bytes(l) as usize;
            if ct_len < NOISE_TAG_LEN {
                return Err(FliqError::Protocol("short record"));
            }
            self.inner.read_exact(&mut self.ct[..ct_len]).map_err(io_err)?;
            if let Some(s) = &self.stats {
                t.stop(&s.net_recv_ns);
            }
            let pt_len = ct_len - NOISE_TAG_LEN;
            let t = Timer::start();
            if out.len() >= pt_len {
                // Decrypt straight into the caller's buffer (no extra copy for bulk data).
                self.ts
                    .read_message(self.nonce, &self.ct[..ct_len], &mut out[..pt_len])
                    .map_err(|_| FliqError::Protocol("record authentication failed"))?;
                out = &mut out[pt_len..];
            } else {
                self.ts
                    .read_message(self.nonce, &self.ct[..ct_len], &mut self.pt[..pt_len])
                    .map_err(|_| FliqError::Protocol("record authentication failed"))?;
                self.pos = 0;
                self.len = pt_len;
            }
            if let Some(s) = &self.stats {
                t.stop(&s.decrypt_ns);
            }
            self.nonce += 1;
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Cursor;

    /// Run a full NKpsk0 handshake in memory; returns (client, server) transports.
    pub fn memory_pair() -> (Handshaken, Handshaken) {
        let (sk, pk) = generate_static_keypair().unwrap();
        let sid = [7u8; 16];
        let psk = [9u8; 16];
        let (mut a, mut b) = duplex();
        let t = std::thread::spawn(move || responder(&mut b, &sid, &sk, &psk).unwrap());
        let c = initiator(&mut a, &sid, &pk, &psk).unwrap();
        (c, t.join().unwrap())
    }

    /// A crude in-memory duplex built on a loopback TCP pair.
    pub fn duplex() -> (std::net::TcpStream, std::net::TcpStream) {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let a = std::net::TcpStream::connect(l.local_addr().unwrap()).unwrap();
        let (b, _) = l.accept().unwrap();
        (a, b)
    }

    /// Split a byte stream of records into individual records (with their length prefix).
    fn records(wire: &[u8]) -> Vec<Vec<u8>> {
        let mut out = vec![];
        let mut i = 0;
        while i < wire.len() {
            let n = u16::from_be_bytes([wire[i], wire[i + 1]]) as usize;
            out.push(wire[i..i + 2 + n].to_vec());
            i += 2 + n;
        }
        out
    }

    fn sealed(msgs: &[&[u8]]) -> (Vec<Vec<u8>>, Arc<StatelessTransportState>) {
        let (c, s) = memory_pair();
        let mut w = SecureWriter::new(Vec::new(), c.transport.clone(), None);
        for m in msgs {
            w.write(m).unwrap();
        }
        w.flush().unwrap();
        (records(w.get_ref()), s.transport)
    }

    fn open(recs: &[Vec<u8>], ts: Arc<StatelessTransportState>, n: usize) -> Result<Vec<u8>> {
        let wire: Vec<u8> = recs.concat();
        let mut r = SecureReader::new(Cursor::new(wire), ts, None);
        let mut out = vec![0u8; n];
        r.read_exact(&mut out)?;
        Ok(out)
    }

    #[test]
    fn roundtrip_large() {
        let data: Vec<u8> = (0..300_000u32).map(|i| i as u8).collect();
        let (recs, ts) = sealed(&[&data]);
        assert!(recs.len() >= 5);
        assert_eq!(open(&recs, ts, data.len()).unwrap(), data);
    }

    #[test]
    fn tampered_record_fails() {
        let (mut recs, ts) = sealed(&[b"hello", b"world"]);
        let last = recs[0].len() - 1;
        recs[0][last] ^= 1;
        assert!(open(&recs, ts, 10).is_err());
    }

    #[test]
    fn reordered_records_fail() {
        let (mut recs, ts) = sealed(&[b"hello", b"world"]);
        recs.swap(0, 1);
        assert!(open(&recs, ts, 10).is_err());
    }

    #[test]
    fn replayed_record_fails() {
        let (recs, ts) = sealed(&[b"hello", b"world"]);
        let replay = vec![recs[0].clone(), recs[0].clone()];
        assert!(open(&replay, ts, 10).is_err());
    }

    #[test]
    fn dropped_record_fails() {
        let (recs, ts) = sealed(&[b"aaaaa", b"bbbbb", b"ccccc"]);
        let dropped = vec![recs[0].clone(), recs[2].clone()];
        assert!(open(&dropped, ts, 10).is_err());
    }

    #[test]
    fn truncated_stream_fails() {
        let (recs, ts) = sealed(&[b"aaaaa", b"bbbbb"]);
        assert!(matches!(open(&recs[..1], ts, 10), Err(FliqError::ConnectionLost)));
    }

    #[test]
    fn wrong_psk_fails_handshake() {
        let (sk, pk) = generate_static_keypair().unwrap();
        let sid = [7u8; 16];
        let (mut a, mut b) = duplex();
        let t = std::thread::spawn(move || responder(&mut b, &sid, &sk, &[1u8; 16]).is_err());
        let client = initiator(&mut a, &sid, &pk, &[2u8; 16]);
        drop(a);
        assert!(t.join().unwrap());
        assert!(client.is_err());
    }

    #[test]
    fn wrong_server_key_fails_handshake() {
        let (sk, _pk) = generate_static_keypair().unwrap();
        let (_sk2, pk2) = generate_static_keypair().unwrap();
        let sid = [7u8; 16];
        let (mut a, mut b) = duplex();
        let t = std::thread::spawn(move || responder(&mut b, &sid, &sk, &[1u8; 16]).is_err());
        let client = initiator(&mut a, &sid, &pk2, &[1u8; 16]);
        drop(a);
        assert!(t.join().unwrap());
        assert!(client.is_err());
    }

    #[test]
    fn hashes_match_and_codes_agree() {
        let (c, s) = memory_pair();
        assert_eq!(*c.hash, *s.hash);
        assert_eq!(kdf::verification_code(&c.hash), kdf::verification_code(&s.hash));
    }
}

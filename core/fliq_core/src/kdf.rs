//! Key derivations named in the spec (HKDF-SHA256 only).

use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroizing;

fn expand(salt: Option<&[u8]>, ikm: &[u8], info: &[u8], out: &mut [u8]) {
    Hkdf::<Sha256>::new(salt, ikm).expand(info, out).expect("HKDF output length is valid");
}

/// The QR carries a 16-byte PSK (spec 5.2) but Noise requires a 32-byte PSK.
/// Expand it with HKDF-SHA256, salted with the session id. (Flagged to owner: spec gap.)
pub fn noise_psk(psk16: &[u8], sid: &[u8]) -> Zeroizing<[u8; 32]> {
    let mut out = Zeroizing::new([0u8; 32]);
    expand(Some(sid), psk16, b"Fliq/1 noise-psk", out.as_mut());
    out
}

/// 6-digit verification code: HKDF(handshake_hash, "verify"), first 4 bytes as u32, mod 1e6.
pub fn verification_code(handshake_hash: &[u8]) -> String {
    let mut b = [0u8; 4];
    expand(None, handshake_hash, b"verify", &mut b);
    format!("{:06}", u32::from_be_bytes(b) % 1_000_000)
}

/// Token that authorizes data streams: HKDF(control_hash, "data-token").
pub fn data_token(control_hash: &[u8]) -> Zeroizing<[u8; 32]> {
    let mut out = Zeroizing::new([0u8; 32]);
    expand(None, control_hash, b"data-token", out.as_mut());
    out
}

/// Constant-time byte comparison.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut d = 0u8;
    for (x, y) in a.iter().zip(b) {
        d |= x ^ y;
    }
    d == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn code_is_six_digits_and_deterministic() {
        let c = verification_code(b"hash");
        assert_eq!(c.len(), 6);
        assert!(c.chars().all(|c| c.is_ascii_digit()));
        assert_eq!(c, verification_code(b"hash"));
        assert_ne!(c, verification_code(b"hash2"));
    }
    #[test]
    fn ct_eq_works() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
    }
}

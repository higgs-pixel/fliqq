//! QR payload (spec section 5): `fliq://v1?d=<base64url(CBOR)>`.

use crate::consts::*;
use crate::error::{FliqError, Result};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;
use zeroize::Zeroize;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct WifiInfo {
    pub ssid: String,
    pub pass: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub band_hint: Option<String>,
}

impl Drop for WifiInfo {
    fn drop(&mut self) {
        self.pass.zeroize();
    }
}

/// What the server (the device showing the QR) will do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerMode {
    Send,
    Receive,
}

impl ServerMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ServerMode::Send => "send",
            ServerMode::Receive => "receive",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct QrPayload {
    pub v: u64,
    #[serde(with = "serde_bytes")]
    pub sid: Vec<u8>,
    #[serde(with = "serde_bytes")]
    pub pk: Vec<u8>,
    #[serde(with = "serde_bytes")]
    pub psk: Vec<u8>,
    pub ip: Vec<String>,
    pub port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wifi: Option<WifiInfo>,
    pub mode: String,
    pub exp: u64,
    pub name: String,
}

// Never print the PSK or Wi-Fi password (spec 6.7).
impl std::fmt::Debug for QrPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QrPayload")
            .field("v", &self.v)
            .field("ips", &self.ip.len())
            .field("port", &self.port)
            .field("mode", &self.mode)
            .field("exp", &self.exp)
            .finish_non_exhaustive()
    }
}

impl Drop for QrPayload {
    fn drop(&mut self) {
        self.psk.zeroize();
    }
}

pub fn now_unix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl QrPayload {
    pub fn server_mode(&self) -> Result<ServerMode> {
        match self.mode.as_str() {
            "send" => Ok(ServerMode::Send),
            "receive" => Ok(ServerMode::Receive),
            _ => Err(FliqError::QrInvalid),
        }
    }

    pub fn ipv4s(&self) -> Vec<Ipv4Addr> {
        self.ip.iter().filter_map(|s| s.parse().ok()).collect()
    }

    pub fn to_uri(&self) -> String {
        let mut cbor = Vec::with_capacity(256);
        ciborium::into_writer(self, &mut cbor).expect("CBOR encode to Vec cannot fail");
        let s = format!("{QR_PREFIX}{}", URL_SAFE_NO_PAD.encode(&cbor));
        cbor.zeroize();
        s
    }

    /// Parse and validate structure. Does NOT check expiry; call [`check_fresh`].
    pub fn from_uri(uri: &str) -> Result<Self> {
        let uri = uri.trim();
        if uri.len() > QR_MAX_URI_LEN {
            return Err(FliqError::QrInvalid);
        }
        let data = uri.strip_prefix(QR_PREFIX).ok_or(FliqError::QrInvalid)?;
        let mut cbor = URL_SAFE_NO_PAD.decode(data).map_err(|_| FliqError::QrInvalid)?;
        let parsed: std::result::Result<QrPayload, _> = ciborium::from_reader(cbor.as_slice());
        cbor.zeroize();
        let p = parsed.map_err(|_| FliqError::QrInvalid)?;
        p.validate()?;
        Ok(p)
    }

    pub fn validate(&self) -> Result<()> {
        let ok = self.v == PROTOCOL_VERSION
            && self.sid.len() == 16
            && self.pk.len() == 32
            && self.psk.len() == 16
            && !self.ip.is_empty()
            && self.ip.len() <= 16
            && self.ip.iter().all(|s| s.parse::<Ipv4Addr>().is_ok())
            && self.port != 0
            && self.server_mode().is_ok()
            && self.name.chars().count() <= MAX_DEVICE_NAME_LEN
            && self.wifi.as_ref().is_none_or(|w| !w.ssid.is_empty() && w.ssid.len() <= 32 && w.pass.len() <= 63);
        if ok { Ok(()) } else { Err(FliqError::QrInvalid) }
    }

    pub fn check_fresh(&self, now: u64) -> Result<()> {
        if now > self.exp { Err(FliqError::QrExpired) } else { Ok(()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> QrPayload {
        QrPayload {
            v: 1,
            sid: vec![1; 16],
            pk: vec![2; 32],
            psk: vec![3; 16],
            ip: vec!["192.168.1.20".into(), "10.0.0.5".into()],
            port: 50123,
            wifi: Some(WifiInfo {
                ssid: "DIRECT-fliq".into(),
                pass: "secretpass".into(),
                band_hint: Some("5GHz".into()),
            }),
            mode: "send".into(),
            exp: 2_000_000_000,
            name: "Desk PC".into(),
        }
    }

    #[test]
    fn roundtrip_and_size() {
        let q = sample();
        let uri = q.to_uri();
        assert!(uri.starts_with(QR_PREFIX));
        assert!(uri.len() < 700, "uri {} bytes", uri.len());
        let back = QrPayload::from_uri(&uri).unwrap();
        assert_eq!(back, q);
    }

    #[test]
    fn rejects_bad_inputs() {
        assert!(matches!(QrPayload::from_uri("https://x"), Err(FliqError::QrInvalid)));
        assert!(matches!(QrPayload::from_uri("fliq://v1?d=!!!"), Err(FliqError::QrInvalid)));
        let mut q = sample();
        q.v = 2;
        assert!(QrPayload::from_uri(&q.to_uri()).is_err());
        let mut q = sample();
        q.psk = vec![0; 15];
        assert!(QrPayload::from_uri(&q.to_uri()).is_err());
        let mut q = sample();
        q.ip = vec!["not-an-ip".into()];
        assert!(QrPayload::from_uri(&q.to_uri()).is_err());
        let long = format!("{QR_PREFIX}{}", "A".repeat(5000));
        assert!(QrPayload::from_uri(&long).is_err());
    }

    #[test]
    fn expiry() {
        let q = sample();
        assert!(q.check_fresh(q.exp).is_ok());
        assert!(matches!(q.check_fresh(q.exp + 1), Err(FliqError::QrExpired)));
    }

    #[test]
    fn debug_hides_secrets() {
        let s = format!("{:?}", sample());
        assert!(!s.contains("secretpass"));
    }
}

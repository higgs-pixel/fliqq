//! Errors with stable short codes and plain-language messages (spec section 14).
//! Messages never contain file names or secrets.

use std::io;

#[derive(Debug, thiserror::Error)]
pub enum FliqError {
    #[error("This code expired. Ask the other device to show a new one.")]
    QrExpired,
    #[error("That doesn't look like a Fliq code.")]
    QrInvalid,
    #[error("Can't reach the other device. Make sure both are on the same Wi-Fi, or choose 'Create offline network'.")]
    Unreachable,
    #[error("Couldn't secure the connection. Start again.")]
    Handshake,
    #[error("Too many failed connection attempts. Start again with a new code.")]
    Locked,
    #[error("Not enough space on this device (need {need} bytes, have {have} bytes).")]
    NoSpace { need: u64, have: u64 },
    #[error("{0}")]
    Limit(String),
    #[error("Connection lost.")]
    ConnectionLost,
    #[error("The file arrived damaged and was deleted. Try again.")]
    HashMismatch,
    #[error("The other device sent something unexpected ({0}).")]
    Protocol(&'static str),
    #[error("The other device declined the transfer.")]
    Rejected,
    #[error("Transfer cancelled.")]
    Cancelled,
    #[error("The other device reported an error ({0}).")]
    Remote(String),
    #[error("Storage or network error.")]
    Io(#[from] io::Error),
}

impl FliqError {
    pub fn code(&self) -> &'static str {
        match self {
            FliqError::QrExpired => "E_QR_EXPIRED",
            FliqError::QrInvalid => "E_QR_INVALID",
            FliqError::Unreachable => "E_UNREACHABLE",
            FliqError::Handshake => "E_HANDSHAKE",
            FliqError::Locked => "E_LOCKED",
            FliqError::NoSpace { .. } => "E_NO_SPACE",
            FliqError::Limit(_) => "E_LIMIT",
            FliqError::ConnectionLost => "E_CONN_LOST",
            FliqError::HashMismatch => "E_HASH_MISMATCH",
            FliqError::Protocol(_) => "E_PROTOCOL",
            FliqError::Rejected => "E_REJECTED",
            FliqError::Cancelled => "E_CANCELLED",
            FliqError::Remote(_) => "E_REMOTE",
            FliqError::Io(_) => "E_IO",
        }
    }
}

pub type Result<T> = std::result::Result<T, FliqError>;

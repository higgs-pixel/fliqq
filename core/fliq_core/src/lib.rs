//! Fliq core: handshake, protocol, crypto record layer and transfer engine.
//! No UI code; no platform APIs. File bytes only ever move inside this crate.

pub mod bitmap;
pub mod consts;
pub mod engine;
pub mod error;
pub mod fsutil;
pub mod kdf;
pub mod msg;
pub mod net;
pub mod noise;
pub mod qr;
pub mod sanitize;
pub mod service;
pub mod session;
pub mod stats;

//! The only surface the Flutter UI sees (spec 3, 7.4): control calls in, events out.
//! File bytes never cross this boundary.

use crate::frb_generated::StreamSink;
use fliq_core::service::{self, AppConfig, SessionHandle, UiEvent};
use flutter_rust_bridge::frb;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[frb(init)]
pub fn init_app() {
    flutter_rust_bridge::setup_default_user_utils();
}

/// User settings the UI keeps (spec 9 item 10).
#[derive(Clone, Debug)]
pub struct Settings {
    pub device_name: String,
    pub save_dir: String,
    /// Configured size limit in bytes (0 = default 50 GiB).
    pub max_total_bytes: i64,
    pub move_sources: bool,
}

fn config(s: &Settings) -> AppConfig {
    let mut c = AppConfig::new(&s.device_name, PathBuf::from(&s.save_dir));
    if s.max_total_bytes > 0 {
        c.max_total = s.max_total_bytes as u64;
    }
    c.move_sources = s.move_sources;
    c
}

#[derive(Clone, Debug)]
pub struct FileEntry {
    pub name: String,
    pub size: i64,
    pub path: Option<String>,
}

/// Which event this is. A flat struct + plain enum keeps the Dart side free of code
/// generators beyond flutter_rust_bridge itself (rich enums would need `freezed`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum EventKind {
    /// uri, addresses, port, expires_unix
    #[default]
    ShowQr,
    /// addresses (reachable), unreachable
    SelfCheck,
    /// code, peer_name
    Connected,
    /// code, peer_name, files, total, free_space, fat32_warning — call `decide`
    OfferReceived,
    /// files, total, move_sources
    Sending,
    /// done, total, mbps, eta_secs, streams, bottleneck
    Progress,
    Reconnecting,
    Reconnected,
    /// index, name
    FileVerified,
    /// name
    MoveFailed,
    /// done (= bytes), secs, mbps, files (with paths on the receiver), streams
    Finished,
    /// error_code, message
    Failed,
}

#[derive(Clone, Debug, Default)]
pub struct FliqEvent {
    pub kind: EventKind,
    pub uri: String,
    pub addresses: Vec<String>,
    pub unreachable: Vec<String>,
    pub port: u16,
    pub expires_unix: i64,
    pub code: String,
    pub peer_name: String,
    pub files: Vec<FileEntry>,
    pub total: i64,
    pub done: i64,
    pub free_space: i64,
    pub fat32_warning: bool,
    pub move_sources: bool,
    pub mbps: f64,
    pub eta_secs: Option<f64>,
    pub secs: f64,
    pub streams: u16,
    pub bottleneck: String,
    pub index: u32,
    pub name: String,
    pub error_code: String,
    pub message: String,
}

fn entries(v: Vec<service::NamedSize>) -> Vec<FileEntry> {
    v.into_iter()
        .map(|f| FileEntry {
            name: f.name,
            size: f.size as i64,
            path: None,
        })
        .collect()
}

impl From<UiEvent> for FliqEvent {
    fn from(e: UiEvent) -> Self {
        use EventKind as K;
        let d = FliqEvent::default();
        match e {
            UiEvent::ShowQr {
                uri,
                addresses,
                port,
                expires_unix,
            } => FliqEvent {
                kind: K::ShowQr,
                uri,
                addresses,
                port,
                expires_unix: expires_unix as i64,
                ..d
            },
            UiEvent::SelfCheck {
                reachable,
                unreachable,
            } => FliqEvent {
                kind: K::SelfCheck,
                addresses: reachable,
                unreachable,
                ..d
            },
            UiEvent::Connected { code, peer_name } => FliqEvent {
                kind: K::Connected,
                code,
                peer_name,
                ..d
            },
            UiEvent::OfferReceived {
                code,
                peer_name,
                files,
                total,
                free_space,
                fat32_warning,
            } => FliqEvent {
                kind: K::OfferReceived,
                code,
                peer_name,
                files: entries(files),
                total: total as i64,
                free_space: free_space as i64,
                fat32_warning,
                ..d
            },
            UiEvent::Sending {
                files,
                total,
                move_sources,
            } => FliqEvent {
                kind: K::Sending,
                files: entries(files),
                total: total as i64,
                move_sources,
                ..d
            },
            UiEvent::Progress {
                done,
                total,
                mbps,
                eta_secs,
                streams,
                bottleneck,
            } => FliqEvent {
                kind: K::Progress,
                done: done as i64,
                total: total as i64,
                mbps,
                eta_secs,
                streams,
                bottleneck,
                ..d
            },
            UiEvent::Reconnecting => FliqEvent {
                kind: K::Reconnecting,
                ..d
            },
            UiEvent::Reconnected => FliqEvent {
                kind: K::Reconnected,
                ..d
            },
            UiEvent::FileVerified { index, name, .. } => FliqEvent {
                kind: K::FileVerified,
                index,
                name,
                ..d
            },
            UiEvent::MoveFailed { name } => FliqEvent {
                kind: K::MoveFailed,
                name,
                ..d
            },
            UiEvent::Finished {
                bytes,
                secs,
                mbps,
                files,
                stream_count,
            } => FliqEvent {
                kind: K::Finished,
                done: bytes as i64,
                total: bytes as i64,
                secs,
                mbps,
                streams: stream_count,
                files: files
                    .into_iter()
                    .map(|f| FileEntry {
                        name: f.name,
                        size: f.size as i64,
                        path: f.path,
                    })
                    .collect(),
                ..d
            },
            UiEvent::Failed { code, message } => FliqEvent {
                kind: K::Failed,
                error_code: code,
                message,
                ..d
            },
        }
    }
}

/// One pairing + transfer. Create it, then call `host` or `join`; the returned stream
/// delivers every event until the session finishes or fails.
#[frb(opaque)]
pub struct FliqSession {
    handle: Mutex<Option<SessionHandle>>,
}

fn sink_fn(sink: StreamSink<FliqEvent>) -> service::Sink {
    let sink = Arc::new(sink);
    Arc::new(move |e: UiEvent| {
        let _ = sink.add(e.into());
    })
}

impl FliqSession {
    #[frb(sync)]
    pub fn new() -> FliqSession {
        FliqSession {
            handle: Mutex::new(None),
        }
    }

    /// Show a QR. Empty `files` = receive into the save folder; otherwise send them.
    pub fn host(
        &self,
        settings: Settings,
        files: Vec<String>,
        sink: StreamSink<FliqEvent>,
    ) -> Result<(), String> {
        let files = files.into_iter().map(PathBuf::from).collect();
        let h = service::host(config(&settings), files, sink_fn(sink))
            .map_err(|e| format!("{}|{}", e.code(), e))?;
        *self.handle.lock().unwrap() = Some(h);
        Ok(())
    }

    /// Use a code from the other device. `files` are sent if the other device receives.
    pub fn join(
        &self,
        settings: Settings,
        code: String,
        files: Vec<String>,
        sink: StreamSink<FliqEvent>,
    ) -> Result<(), String> {
        let files = files.into_iter().map(PathBuf::from).collect();
        let h = service::join(config(&settings), &code, files, sink_fn(sink))
            .map_err(|e| format!("{}|{}", e.code(), e))?;
        *self.handle.lock().unwrap() = Some(h);
        Ok(())
    }

    #[frb(sync)]
    pub fn decide(&self, accept: bool) {
        if let Some(h) = self.handle.lock().unwrap().as_ref() {
            h.decide(accept);
        }
    }

    #[frb(sync)]
    pub fn cancel(&self) {
        if let Some(h) = self.handle.lock().unwrap().as_ref() {
            h.cancel();
        }
    }
}

/// Values for the dashboard metrics strip (spec 10.9).
#[derive(Clone, Debug)]
pub struct EngineFacts {
    pub max_payload_bytes: i64,
    pub crypto_label: String,
    pub cipher: String,
    pub inflight_budget_bytes: i64,
}

#[frb(sync)]
pub fn engine_facts(settings: Settings) -> EngineFacts {
    let f = service::engine_facts(&config(&settings));
    EngineFacts {
        max_payload_bytes: f.max_payload_bytes as i64,
        crypto_label: f.crypto_label,
        cipher: f.cipher,
        inflight_budget_bytes: f.inflight_budget_bytes as i64,
    }
}

#[derive(Clone, Debug)]
pub struct FolderInfo {
    pub free_bytes: i64,
    pub is_fat: bool,
}

pub fn folder_info(path: String) -> Result<FolderInfo, String> {
    let (free, fat) =
        service::folder_info(std::path::Path::new(&path)).map_err(|e| e.to_string())?;
    Ok(FolderInfo {
        free_bytes: free as i64,
        is_fat: fat,
    })
}

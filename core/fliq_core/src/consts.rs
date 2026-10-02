//! Protocol and engine constants. Every limit the spec names lives here.

pub const PROTOCOL_VERSION: u64 = 1;
pub const QR_PREFIX: &str = "fliq://v1?d=";
/// Upper bound on the QR URI length we accept (encoded payload target is ~700 bytes).
pub const QR_MAX_URI_LEN: usize = 2048;
pub const QR_TTL_SECS: u64 = 120;

pub const NOISE_PARAMS: &str = "Noise_NKpsk0_25519_ChaChaPoly_BLAKE2s";
pub const NOISE_PROLOGUE_PREFIX: &[u8] = b"Fliq/1";
pub const NOISE_MAX_MSG: usize = 65_535;
pub const NOISE_TAG_LEN: usize = 16;
pub const NOISE_MAX_PLAINTEXT: usize = 65_519;
pub const CIPHER_NAME: &str = "ChaCha20-Poly1305";
/// What the UI shows as "Crypto Integrity" (spec 10.9). Update if the scheme changes.
pub const CRYPTO_LABEL: &str = "Noise + BLAKE3";

pub const CHUNK_SIZE: u64 = 4 * 1024 * 1024;
pub const MAX_FAILED_HANDSHAKES: u32 = 3;
pub const MAX_DATA_STREAMS: u16 = 16;
pub const DEFAULT_STREAMS: u16 = 4;

/// Pipeline in-flight buffer budget (spec 7.1). Shown in the UI as "In-Flight RAM Bound".
pub const DEFAULT_INFLIGHT_BUDGET_BYTES: usize = 64 * 1024 * 1024;
pub const MIN_INFLIGHT_BUDGET_BYTES: usize = 16 * 1024 * 1024;

pub const DEFAULT_MAX_TOTAL_BYTES: u64 = 50 * 1024 * 1024 * 1024;
pub const DEFAULT_MAX_FILES: usize = 1000;
pub const MAX_NAME_LEN: usize = 255;
pub const MAX_DEVICE_NAME_LEN: usize = 64;
pub const MAX_CONTROL_MSG: usize = 1024 * 1024;

pub const SOCKET_BUF_BYTES: usize = 4 * 1024 * 1024;
pub const CONNECT_TIMEOUT_MS: u64 = 3_000;
pub const HANDSHAKE_TIMEOUT_MS: u64 = 10_000;
pub const IO_TIMEOUT_MS: u64 = 30_000;
/// How long the session survives with no data progress before failing (spec 7.7).
pub const SESSION_LINGER_MS: u64 = 60_000;
/// Receiver asks for missing chunks after this long without progress once a file is "done".
pub const REPAIR_IDLE_MS: u64 = 1_500;

/// Suffix for in-progress receive files. Swept on receiver start (crash clean-up).
pub const PART_SUFFIX: &str = ".fliq.part";

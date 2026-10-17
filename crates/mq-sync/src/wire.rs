//! JSON shapes between devices and the relay. Binary fields travel as hex
//! (key ids) and base64 (bodies).

use serde::{Deserialize, Serialize};

/// Most records per push or pull page.
pub const PAGE: usize = 500;
/// Largest sealed body the relay accepts. The addon list carries every
/// manifest, which reaches a few hundred KiB for heavy setups.
pub const MAX_BODY: usize = 1024 * 1024;
/// Smallest possible sealed body: a 24-byte nonce and a 16-byte tag.
pub const MIN_BODY: usize = 40;
/// A record whose clock is further ahead of the relay than this is refused.
pub const MAX_CLOCK_AHEAD_MS: u64 = 10 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireRecord {
    pub coll: u8,
    /// 16 bytes as 32 hex digits.
    pub key_id: String,
    /// 32 hex digits, compared as text.
    pub hlc: String,
    pub epoch: u32,
    /// Base64 (standard, padded) sealed body.
    pub body: String,
    /// Assigned by the relay; ignored on push.
    #[serde(default)]
    pub seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushRequest {
    pub records: Vec<WireRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushResponse {
    pub accepted: u32,
    /// The relay already holds a newer or equal version.
    pub stale: u32,
    /// Relay seq of each stale register's current version.
    #[serde(default)]
    pub stale_at: Vec<u64>,
    pub rejected: Vec<Rejected>,
    pub server_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rejected {
    pub key_id: String,
    pub reason: String,
    #[serde(default)]
    pub code: Refusal,
    /// For a clock refusal: the hlc of the version the relay holds now.
    #[serde(default)]
    pub current: Option<String>,
}

/// Why the relay refused a record, so a device knows whether to retry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Refusal {
    /// The record's clock is too far ahead of the relay.
    ClockAhead,
    TooLarge,
    /// Written under a key epoch the account has not reached.
    Epoch,
    #[default]
    #[serde(other)]
    Invalid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullResponse {
    pub records: Vec<WireRecord>,
    /// Cursor for the next pull.
    pub last: u64,
    /// The account's newest seq. Below a device's cursor it means the relay
    /// lost records, and the device sends and reads everything again.
    #[serde(default)]
    pub head: u64,
    /// Changes when the relay is restored from a backup; a device that saw
    /// another value sends and reads everything again.
    #[serde(default)]
    pub generation: String,
    pub more: bool,
    pub server_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountResponse {
    pub account: String,
    /// Bearer token for this device. The relay keeps only its SHA-256.
    pub device_token: String,
}

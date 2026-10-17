//! JSON shapes between devices and the relay. Binary fields travel as hex
//! (key ids) and base64 (bodies).

use crate::roster::{Digest32, Entry, Id, TokenRequest};
use serde::{Deserialize, Serialize};

/// Most records per push or pull page.
pub const PAGE: usize = 500;
/// Most roster entries per page (each is at most 64 KiB).
pub const ROSTER_PAGE: usize = 64;
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
    /// The roster head the device holds; the relay refuses the whole push
    /// (409) when its own differs.
    #[serde(default)]
    pub head: Digest32,
}

/// Seq and hash of the last roster entry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RosterHead {
    pub seq: u64,
    pub hash: Digest32,
}

/// The body of a 409 to a push: the device's roster or keys are behind.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Behind {
    pub error: String,
    pub epoch: u32,
    pub roster: Option<RosterHead>,
}

/// Roster entries in seq order from `from`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RosterPage {
    pub from: u64,
    pub entries: Vec<Entry>,
    pub head: Option<RosterHead>,
    pub more: bool,
}

/// One entry to append (the relay checks every rule first).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppendRequest {
    pub entry: Entry,
}

/// Entries a device holds and the relay lost, continuing the relay's head.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayRequest {
    pub account: Id,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayInfo {
    /// Random, fixed for the relay's life; devices pin it to the URL.
    pub relay_id: Id,
    pub generation: String,
    /// When the current generation started (0 if the relay was never
    /// restored).
    pub generation_since_ms: u64,
    pub server_ms: u64,
}

/// A fresh token for a member, signed with its device key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenGrant {
    pub request: TokenRequest,
    pub sig: crate::roster::Hex<64>,
}

/// A new account: its genesis entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountRequest {
    pub entry: Entry,
}

/// What redeeming a kit gives: a pending token and the kit's member id.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Redeemed {
    pub account: Id,
    pub device_token: String,
    pub kit: Id,
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
    /// The account's key epoch and roster head, so a device sees at once
    /// that the roster moved (or forked) under it.
    #[serde(default)]
    pub epoch: u32,
    #[serde(default)]
    pub roster: Option<RosterHead>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountResponse {
    pub account: String,
    /// Bearer token for this device. The relay keeps only its SHA-256.
    pub device_token: String,
}

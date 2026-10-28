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
    /// The sign-in providers this relay brokers (`/v1/auth/{provider}`),
    /// none without an identity file.
    #[serde(default)]
    pub providers: Vec<String>,
}

/// A fresh token for a member, signed with its device key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenGrant {
    pub request: TokenRequest,
    pub sig: crate::roster::Hex<64>,
}

/// A new account: its genesis entry. On a relay with sign-in, a ticket
/// (or the invite) is needed; the ticket's identity is linked to it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountRequest {
    pub entry: Entry,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticket: Option<String>,
    /// Start over: the identity's link moves here, and the account it
    /// pointed to is scheduled for deletion.
    #[serde(default, rename = "move", skip_serializing_if = "std::ops::Not::not")]
    pub move_link: bool,
}

/// Deletes the account now: the active token, and this request signed by
/// the device key (`MemberSecret::sign_delete`), so a token alone deletes
/// nothing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteRequest {
    pub request: TokenRequest,
    pub sig: crate::roster::Hex<64>,
    /// An Apple revocation's receipt, when the account has an Apple link.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<String>,
}

/// Opens a sign-in or revoke flow at `/v1/auth/{provider}/start`.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRequest {
    /// Lowercase hex SHA-256 of the verifier the device keeps.
    pub verifier_hash: String,
    /// `signin` or `revoke` (Apple only).
    pub purpose: String,
    /// Where a brokered flow comes back to (on the relay's allowlist).
    #[serde(default, rename = "return", skip_serializing_if = "Option::is_none")]
    pub return_to: Option<String>,
    /// A native sheet signs in: the answer carries the nonce, no URL.
    #[serde(default)]
    pub native: bool,
    /// The address an `email` flow mails its code to. Never logged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

impl std::fmt::Debug for StartRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StartRequest")
            .field("purpose", &self.purpose)
            .field("return_to", &self.return_to)
            .field("native", &self.native)
            .field("email", &self.email.as_ref().map(|_| "<redacted>"))
            .finish_non_exhaustive()
    }
}

/// A deletion of the account that waits (D14): every device shows it with
/// Cancel. `provider` names the identity that asked from the web page
/// ("Someone signed in with your Google account..."); none for a Start over
/// or when that link is gone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledDeletion {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub due_ms: u64,
    #[serde(default)]
    pub provider: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartResponse {
    pub flow: String,
    /// When the flow stops being open (relay time).
    #[serde(default)]
    pub expires_ms: u64,
    /// Brokered: the provider page to open.
    #[serde(default)]
    pub url: Option<String>,
    /// Native: the relay's nonce (base64url); the sheet gets `nonce_for`.
    #[serde(default)]
    pub nonce: Option<String>,
}

/// Collects a brokered flow's result, once: the verifier and the one-time
/// code from the return fragment (or the code an email flow mailed).
#[derive(Clone, Serialize, Deserialize)]
pub struct TakeRequest {
    pub flow: String,
    pub verifier: String,
    pub code: String,
}

impl std::fmt::Debug for TakeRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TakeRequest")
            .field("flow", &self.flow)
            .finish_non_exhaustive()
    }
}

/// A native sheet's ID token for a flow.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeToken {
    pub flow: String,
    pub verifier: String,
    pub id_token: String,
    /// Apple: a fresh authorization code, for a revoke.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorization_code: Option<String>,
}

/// What a finished flow gives: a ticket for the identity (10 minutes, one
/// use) and the account it signs in to, when linked; a revoke also gives
/// its receipt.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Taken {
    pub ticket: String,
    #[serde(default)]
    pub account: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<String>,
}

/// An identity linked to the account. No subject, email or name: the
/// provider and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct IdentityLink {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub link: i64,
    pub provider: String,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub created_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkRequest {
    pub ticket: String,
    #[serde(default, rename = "move")]
    pub move_link: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UnlinkRequest {
    #[serde(default)]
    pub receipt: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CancelDeletion {
    /// Also remove the identity link that asked.
    #[serde(default)]
    pub unlink: bool,
}

/// The nonce a provider sheet gets for a relay's flow: lowercase hex of
/// SHA-256(relay nonce bytes || 0x00 || host), `host` being the relay host
/// the app is configured with. A relay that forwards another relay's flow
/// cannot make the claim match there.
pub fn nonce_for(relay_nonce: &[u8], host: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(
        [relay_nonce, &[0], host.as_bytes()].concat(),
    ))
}

/// An email address as an identity (plans/accounts-email.md E2): trimmed,
/// one `@`, a lowercased local part of RFC 5322 `atext` and single inner
/// dots, a lowercased ASCII domain name (IDNA for a Unicode one) with a dot
/// and no IP literal, at most 254 bytes. The same string is hashed and
/// mailed. `None` for anything else.
pub fn normalize_email(address: &str) -> Option<String> {
    const ATEXT: &str = "!#$%&'*+-/=?^_{|}~";
    let (local, domain) = address.trim().split_once('@')?;
    let local_ok = (1..=64).contains(&local.len())
        && local.split('.').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || ATEXT.contains(c))
        });
    // `Host::parse` decodes `%` and keeps a trailing dot: both refused first.
    if !local_ok || domain.contains(['%', '@']) || domain.ends_with('.') {
        return None;
    }
    let domain = match url::Host::parse(domain).ok()? {
        url::Host::Domain(d) => d,
        _ => return None,
    };
    let labels: Vec<&str> = domain.split('.').collect();
    let domain_ok = labels.len() >= 2
        && labels.iter().all(|l| {
            (1..=63).contains(&l.len())
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
        && !labels.last()?.chars().all(|c| c.is_ascii_digit());
    let address = format!("{}@{domain}", local.to_ascii_lowercase());
    (domain_ok && address.len() <= 254).then_some(address)
}

/// The mailbox a normalized address delivers to, for mail limits only
/// (E3): no `+tag`, and Gmail's dots and `googlemail.com` folded.
pub fn mailbox(address: &str) -> String {
    let (local, domain) = address.split_once('@').unwrap_or((address, ""));
    let local = local.split_once('+').map_or(local, |(base, _)| base);
    match domain {
        "gmail.com" | "googlemail.com" => format!("{}@gmail.com", local.replace('.', "")),
        _ => format!("{local}@{domain}"),
    }
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
    /// The key epoch and roster head the push was taken under; a device
    /// checks them before it counts its records as sent.
    pub epoch: u32,
    pub roster: Option<RosterHead>,
    /// A deletion of the account scheduled from the web page or a Start over.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletion: Option<ScheduledDeletion>,
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
    /// A deletion of the account scheduled from the web page or a Start over.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletion: Option<ScheduledDeletion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountResponse {
    pub account: String,
    /// Bearer token for this device. The relay keeps only its SHA-256.
    pub device_token: String,
}

#[cfg(test)]
mod tests {
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;

    /// The relay and every app compute the provider nonce the same way: the
    /// shared vectors (made with openssl, not this code) say how.
    #[test]
    fn the_host_bound_nonce_matches_the_shared_vectors() {
        let vectors: serde_json::Value =
            serde_json::from_str(include_str!("../../../apps/shared/passkey-vectors.json"))
                .unwrap();
        let cases = vectors["nonce"].as_array().unwrap();
        assert_eq!(cases.len(), 3);
        for case in cases {
            let relay = B64URL.decode(case["relayNonce"].as_str().unwrap()).unwrap();
            let host = case["host"].as_str().unwrap();
            assert_eq!(
                super::nonce_for(&relay, host),
                case["nonce"].as_str().unwrap(),
                "{host}"
            );
        }
    }

    #[test]
    fn email_addresses_normalize_and_fold_into_mailboxes() {
        use super::{mailbox, normalize_email};
        let n = |a: &str| normalize_email(a);
        assert_eq!(
            n("  Ann.Lee@Example.ORG ").as_deref(),
            Some("ann.lee@example.org")
        );
        assert_eq!(n("a@b\u{fc}cher.de").as_deref(), Some("a@xn--bcher-kva.de"));
        assert_eq!(
            n("o'brien+tv@mail.example").as_deref(),
            Some("o'brien+tv@mail.example")
        );
        for bad in [
            "",
            "a",
            "@x.org",
            "a@",
            "a@x",
            "a@@x.org",
            "a@1.2.3.4",
            "a@x.org.",
            "a@x%2eorg",
            "a.@x.org",
            "a@-x.org",
            "\u{e4}@x.org",
            "a@x.123",
        ] {
            assert_eq!(n(bad), None, "{bad}");
        }
        assert_eq!(mailbox("a.b+c@googlemail.com"), "ab@gmail.com");
        assert_eq!(mailbox("a.b+c@example.org"), "a.b@example.org");
    }
}

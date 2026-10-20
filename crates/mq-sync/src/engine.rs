//! One sync round: catch up on the account roster, push queued local
//! registers, then pull and apply what other devices wrote. Push problems
//! never stop the pull, so one bad record cannot cut a device off from its
//! other devices.
//!
//! State lives in the library's `sync_state`, outbox and roster store, so a
//! crash at any point only repeats work: pushes are upserts, applies are
//! idempotent, and taking a new key is repeatable from this device's own box
//! in the roster. The keystore record (`SyncConfig`) is the app's: a round
//! that changes it returns it and publishes nothing until the app stored it.

use crate::roster::{
    Bundle, Digest32, Entry, Hex, Id, Kind, MemberSecret, Role, Roster, RosterError, TokenRequest,
    open_name,
};
use crate::seal::{AccountKey, SealError};
use crate::wire::{
    AccountRequest, AccountResponse, AppendRequest, MAX_CLOCK_AHEAD_MS, PAGE, PullResponse,
    PushRequest, PushResponse, Refusal, RelayInfo, ReplayRequest, RosterHead, RosterPage,
    ScheduledDeletion, TokenGrant, WireRecord,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use mq_http::{HttpClient, HttpError, Priority, RequestOptions};
use mq_library::hlc::{Hlc, HlcClock, MAX_WALL_MS};
use mq_library::{Library, LibraryError, SignedEntry, SyncRecord};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

const PULL_SEQ: &str = "pull_seq";
const READER_SEEN: &str = "pull_reader";
const TARGET: &str = "sync_target";
const GENERATION: &str = "relay_generation";
/// `<seq>:<epoch>`: the cursor to return to (just before the first record
/// whose key was missing) once a key for that epoch arrives.
const EPOCH_PARK: &str = "epoch_park";
/// `approval:<member id>` → `<relay ms>:<device ms>`: when the pairing it
/// approved times out, by the relay's clock and by this device's.
pub(crate) const APPROVAL: &str = "approval:";
/// `cancel:<member id>`: an approval that timed out; resolved by a rotation.
pub(crate) const CANCEL: &str = "cancel:";
/// `relay:<relay id>` → the host that relay id was first seen at.
const RELAY_PIN: &str = "relay:";
/// Set when the relay showed a roster that conflicts with this device's;
/// every call fails with `Fork` until `use_server_roster` settles it. The
/// relay's history as read then is kept under `fork:<genesis>`.
const FORK: &str = "fork";

/// Shape version of register values this build writes; newer ones are
/// skipped until the app updates.
pub const SHAPES: u16 = 1;
/// What this build can read. Raising it makes every device pull everything
/// again once, to apply records an older build skipped.
const READER: &str = "4";
/// Sealed bytes sent in one push request.
const PUSH_BYTES: usize = 4 * 1024 * 1024;
/// After a relay restore, automatic rotations wait this long, so devices
/// holding entries the restore lost can send them back first.
const GRACE_MS: u64 = 60 * 60 * 1000;
/// How long an approved pairing may take before the approver cancels it.
pub const PAIRING_MS: u64 = 10 * 60 * 1000;
/// Tries at an append or push whose roster moved under it, per round.
const TRIES: usize = 3;

/// The keystore record of one device of one account. The app stores it
/// whole and passes it to every call; calls that change it return it.
/// Apps keep it as opaque JSON, `Record<string, unknown>` in the TypeScript
/// types: it holds a token and keys.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncConfig {
    pub server: String,
    pub device_token: String,
    /// This device's member seed (its signing and box keys).
    pub member: Hex<32>,
    pub keys: Bundle,
    /// The roster head this record was written at. A library database
    /// older than this syncs nothing until it has caught up to it.
    pub checkpoint: RosterHead,
    /// The relay's id, pinned to `server` at first contact.
    pub relay_id: Id,
    /// Raised on every change; the app keeps the highest it was given.
    pub revision: u64,
}

impl std::fmt::Debug for SyncConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyncConfig")
            .field("server", &self.server)
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}

impl SyncConfig {
    pub fn secret(&self) -> MemberSecret {
        MemberSecret::from_seed(self.member.0)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub pushed: u32,
    /// The relay already had these or newer.
    pub stale: u32,
    /// Registers the relay will never take (too large, malformed); dropped
    /// from the outbox so they do not block the rest.
    pub refused: u32,
    /// Registers the relay cannot take yet (stamped ahead of its clock);
    /// still queued, they go once the relay's clock reaches them.
    pub waiting: u32,
    pub pulled: u32,
    /// Pulled registers that changed local state.
    pub applied: u32,
    /// Pulled records that did not open with this account key.
    pub unreadable: u32,
    /// Pulled records written by a newer app version, kept for after the
    /// update.
    pub newer: u32,
    /// Pulled records under a key epoch this device does not hold yet; they
    /// are read again once it does.
    pub parked: u32,
    /// Pulled records sealed under an old key well after it was replaced.
    /// An honest relay refuses those, so the app should say the relay may
    /// be hiding roster changes, and suggest comparing fingerprints.
    pub old_epoch_writes: u32,
    /// This device's clock minus the relay's. Beyond 10 minutes either way
    /// one of them is wrong: the UI should say so (pushes get stamped at
    /// the relay's time, and last-writer-wins falls back to push order).
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub clock_gap_ms: i64,
    /// A rotation is due but waits this long after a relay restore; pushes
    /// wait with it.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub rotation_wait_ms: u64,
    /// Why queued changes were not sent this round, when the app should say
    /// so (none when only the record changed: then `again` is set).
    pub blocked: Option<Blocked>,
    /// The roster fingerprint, for people to compare between devices.
    pub fingerprint: String,
    /// A changed keystore record: store it, then sync again at once.
    #[cfg_attr(feature = "ts", ts(type = "Record<string, unknown> | null"))]
    pub record: Option<SyncConfig>,
    /// Sync again right away (the record changed, nothing was published).
    pub again: bool,
    /// The account's deletion waits for this time (D14): show it with
    /// Cancel on every device. From the round's last relay answer.
    pub deletion: Option<ScheduledDeletion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub enum Blocked {
    /// A member left: pushes wait for the key rotation (which itself waits
    /// out the grace period after a relay restore).
    RotationPending,
    /// The relay takes no pushes from this device's token yet.
    NotActive,
    /// The relay kept answering for another roster or key than this
    /// device's.
    RelayDisagrees,
}

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    /// Any HTTP failure but the relay's own `accountGone` answer.
    #[error(transparent)]
    Http(HttpError),
    #[error(transparent)]
    Library(#[from] LibraryError),
    #[error("the relay must use https unless it is on this network")]
    Insecure,
    #[error("pairing: {0}")]
    Pairing(String),
    #[error("roster: {0}")]
    Roster(String),
    #[error("the relay shows a roster that conflicts with this device's; compare devices")]
    Fork,
    #[error("this device was removed from the account")]
    Dropped,
    #[error("this device has not joined the account")]
    NotJoined,
    #[error("this is not the relay this device joined")]
    WrongRelay,
    #[error("the relay is behind this device; it needs a device that still holds the account")]
    RelayBehind,
    #[error("some changes are not sent yet; sync again or sign out anyway")]
    Unsent,
    #[error("this record is not this device's sync account")]
    WrongRecord,
    #[error("the relay's roster changed since it was shown; compare again")]
    RosterChanged,
    #[error("this account was deleted")]
    AccountGone,
    #[error("this identity already signs in to another account")]
    LinkedElsewhere,
    #[error("this identity made several accounts lately; try again later")]
    SignupLimit,
    #[error("the sign-in expired; sign in again to finish")]
    TicketExpired,
    #[error("the sign-in was cancelled")]
    Cancelled,
    #[error("the sign-in did not check")]
    SignInRefused,
    #[error("this sign-in was started elsewhere")]
    WrongFlow,
    /// A sign-in's return names another relay than the one it started at
    /// (unlike `WrongRelay`: not the relay this device joined).
    #[error("this sign-in came back from another relay")]
    OtherRelay,
    #[error("this receipt is not for this link")]
    ReceiptRefused,
    /// The relay's sign-in budget for this address is spent for the minute.
    #[error("too many sign-in requests; try again in a minute")]
    Busy,
}

/// The relay's coded answers map in one place. Only the relay's own 410
/// with code `accountGone` means the account is gone; any other 410 (a
/// proxy, a parked domain) is a plain failure: it never makes a device
/// forget its account.
impl From<HttpError> for SyncError {
    fn from(e: HttpError) -> Self {
        let HttpError::Status {
            status, ref body, ..
        } = e
        else {
            return SyncError::Http(e);
        };
        match (status, code_of(body).as_deref()) {
            (410, Some("accountGone")) => SyncError::AccountGone,
            (409, Some("linkedElsewhere")) => SyncError::LinkedElsewhere,
            (429, Some("signupLimit")) => SyncError::SignupLimit,
            (403, Some("ticketExpired")) => SyncError::TicketExpired,
            (403, Some("receiptRefused")) => SyncError::ReceiptRefused,
            (429, Some("busy")) => SyncError::Busy,
            _ => SyncError::Http(e),
        }
    }
}

/// The `code` of a relay's JSON error body.
pub(crate) fn code_of(body: &[u8]) -> Option<String> {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()?
        .get("code")?
        .as_str()
        .map(str::to_owned)
}

impl SyncError {
    /// No definite answer came (the network, a timeout, the relay's own
    /// trouble, its budget): the same call may succeed later. A body that
    /// did not decode or was too large came after a 2xx: the relay acted.
    pub fn transient(&self) -> bool {
        match self {
            SyncError::Http(HttpError::Status { status, .. }) => *status >= 500,
            SyncError::Http(HttpError::Network(_) | HttpError::Timeout(_)) | SyncError::Busy => {
                true
            }
            _ => false,
        }
    }

    /// A stable name for the errors an app acts on (rather than only shows).
    pub fn code(&self) -> Option<&'static str> {
        Some(match self {
            SyncError::Insecure => "insecure",
            SyncError::Fork => "fork",
            SyncError::Dropped => "dropped",
            SyncError::NotJoined => "notJoined",
            SyncError::WrongRelay => "wrongRelay",
            SyncError::RelayBehind => "relayBehind",
            SyncError::Unsent => "unsent",
            SyncError::WrongRecord => "wrongRecord",
            SyncError::RosterChanged => "rosterChanged",
            SyncError::AccountGone => "accountGone",
            SyncError::LinkedElsewhere => "linkedElsewhere",
            SyncError::SignupLimit => "signupLimit",
            SyncError::TicketExpired => "ticketExpired",
            SyncError::Cancelled => "cancelled",
            SyncError::SignInRefused => "signInRefused",
            SyncError::WrongFlow => "wrongFlow",
            SyncError::OtherRelay => "otherRelay",
            SyncError::ReceiptRefused => "receiptRefused",
            SyncError::Busy => "busy",
            _ => return None,
        })
    }
}

fn roster_error(e: RosterError) -> SyncError {
    SyncError::Roster(e.to_string())
}

/// The plaintext inside a sealed body. The key travels inside so the reader
/// can check it against the blind id the body was filed under.
#[derive(Serialize, Deserialize)]
struct Payload {
    k: String,
    v: serde_json::Value,
    /// Shape version of `v`.
    #[serde(default)]
    s: u16,
}

pub async fn sync_once(
    library: &Library,
    http: &HttpClient,
    config: &SyncConfig,
    clock: &HlcClock,
) -> Result<SyncReport, SyncError> {
    let mut report = SyncReport::default();
    let mut session = Session::open(library, http, config, clock).await?;
    session.upkeep(&mut report).await?;
    let pushed = match session.may_publish() {
        true => session.push(&mut report).await,
        false => Ok(()),
    };
    session.pull(&mut report).await?;
    pushed?;
    Ok(session.finish(report))
}

/// Only the push half, for when the app goes to the background.
pub async fn push_changes(
    library: &Library,
    http: &HttpClient,
    config: &SyncConfig,
    clock: &HlcClock,
) -> Result<SyncReport, SyncError> {
    let mut report = SyncReport::default();
    let mut session = Session::open(library, http, config, clock).await?;
    session.upkeep(&mut report).await?;
    if session.may_publish() {
        session.push(&mut report).await?;
    }
    Ok(session.finish(report))
}

/// Only the pull half, for when the relay says something changed.
pub async fn pull_changes(
    library: &Library,
    http: &HttpClient,
    config: &SyncConfig,
    clock: &HlcClock,
) -> Result<SyncReport, SyncError> {
    let mut report = SyncReport::default();
    let mut session = Session::open(library, http, config, clock).await?;
    session.pull(&mut report).await?;
    Ok(session.finish(report))
}

/// Creates an account on the relay with this device as its first member,
/// and returns the device's keystore record.
pub async fn create_account(
    http: &HttpClient,
    server: &str,
    invite: Option<&str>,
    name: &str,
) -> Result<SyncConfig, SyncError> {
    create_account_with(http, server, invite, name, None, false).await
}

/// `create_account` on a relay with sign-in: the sign-in's `ticket` links
/// its identity to the new account. `move_link` (Start over) takes the
/// identity from the account it signs in to now, which is then scheduled
/// for deletion. A retry after a lost answer finds the account made and
/// signs for its token; the ticket was used once, by the first.
pub async fn create_account_with(
    http: &HttpClient,
    server: &str,
    invite: Option<&str>,
    name: &str,
    ticket: Option<&str>,
    move_link: bool,
) -> Result<SyncConfig, SyncError> {
    require_secure(server)?;
    let info = relay_info(http, server).await?;
    let member = MemberSecret::generate();
    let (entry, keys) = Roster::genesis_entry(&member, Hex(random()), name, info.server_ms);
    let mut options = options(server, "");
    options.headers = invite
        .map(|i| vec![("x-invite".to_owned(), i.to_owned())])
        .unwrap_or_default();
    let request = AccountRequest {
        entry: entry.clone(),
        ticket: ticket.map(str::to_owned),
        move_link,
    };
    let send = || async {
        http.post_json(&format!("{server}/v1/accounts"), &request, &options)
            .await?
            .json::<AccountResponse>()
    };
    // No answer (or a failure after the relay stored it) may still have
    // made the account: send it once more; "exists" then means this one.
    let created = match send().await {
        Err(e) if !matches!(e, HttpError::Status { status: ..500, .. }) => send().await,
        first => first,
    };
    let device_token = match created {
        Ok(account) => account.device_token,
        // "This account already exists" carries no code; a coded 409
        // (linked elsewhere) is the answer itself.
        Err(HttpError::Status {
            status: 409,
            ref body,
            ..
        }) if code_of(body).is_none() => {
            let account = keys.account;
            signed_token(
                http,
                server,
                info.relay_id,
                account,
                &member,
                info.server_ms,
            )
            .await?
        }
        Err(e) => return Err(e.into()),
    };
    Ok(SyncConfig {
        server: server.to_owned(),
        device_token,
        member: Hex(member.seed()),
        keys,
        checkpoint: RosterHead {
            seq: 0,
            hash: entry.hash(),
        },
        relay_id: info.relay_id,
        revision: 1,
    })
}

pub(crate) async fn relay_info(http: &HttpClient, server: &str) -> Result<RelayInfo, SyncError> {
    Ok(http
        .get(&format!("{server}/v1/relay"), &options(server, ""))
        .await?
        .json()?)
}

/// Whether the relay answering at the record's URL is the one this device
/// joined: the record's relay id (once known), pinned to the host. One relay
/// id, one host: a relay answering with another relay's id cannot borrow
/// what this device signs or sends for that one.
pub(crate) fn pinned(
    library: &Library,
    config: &SyncConfig,
    info: &RelayInfo,
) -> Result<(), SyncError> {
    if config.relay_id != Id::default() && config.relay_id != info.relay_id {
        return Err(SyncError::WrongRelay);
    }
    let pin = format!("{RELAY_PIN}{}", hex::encode(info.relay_id.0));
    let host = host(&config.server);
    match library.sync_state(&pin)? {
        Some(pinned) if !pinned.is_empty() && pinned != host => Err(SyncError::WrongRelay),
        _ => Ok(library.set_sync_state(&pin, &host)?),
    }
}

/// The relay's host, as relay pins and token requests name it (its port may
/// change when a relay is moved or restored).
pub(crate) fn host(server: &str) -> String {
    url::Url::parse(server)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_default()
}

/// A fresh token for a device member, signed with its key: how a device
/// whose token the relay lost (or never sent back) gets back in.
async fn signed_token(
    http: &HttpClient,
    server: &str,
    relay_id: Id,
    account: Id,
    member: &MemberSecret,
    at_ms: u64,
) -> Result<String, SyncError> {
    let request = TokenRequest::new(relay_id, account, member.id(), at_ms, &host(server));
    let sig = member.sign_token(&request);
    let granted: AccountResponse = http
        .post_json(
            &format!("{server}/v1/tokens"),
            &TokenGrant { request, sig },
            &options(server, ""),
        )
        .await?
        .json()?;
    Ok(granted.device_token)
}

pub(crate) fn random<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).expect("os randomness");
    bytes
}

/// Bearer tokens never cross the internet in clear text.
pub(crate) fn require_secure(server: &str) -> Result<(), SyncError> {
    let url = url::Url::parse(server).map_err(|_| SyncError::Insecure)?;
    // Loopback, private ranges and link-local only: carrier NAT (100.64/10)
    // and names that may resolve anywhere still cross other networks.
    let local = match url.host() {
        Some(url::Host::Domain(name)) => name == "localhost",
        Some(url::Host::Ipv4(ip)) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        Some(url::Host::Ipv6(ip)) => {
            ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local()
        }
        None => false,
    };
    if url.scheme() == "https" || (url.scheme() == "http" && local) {
        Ok(())
    } else {
        Err(SyncError::Insecure)
    }
}

/// Cursors belong to one relay and one account: syncing somewhere else
/// starts over, queueing everything and pulling from the start. A new token
/// for the same account keeps them. The roster store stays (it belongs to
/// the account, not the relay).
fn prepare(library: &Library, info: &RelayInfo, account: &Id) -> Result<(), SyncError> {
    let target = format!(
        "{}|{}",
        hex::encode(info.relay_id.0),
        hex::encode(account.0)
    );
    if library.sync_state(TARGET)?.as_deref() != Some(target.as_str()) {
        for key in [PULL_SEQ, READER_SEEN, GENERATION, EPOCH_PARK] {
            library.set_sync_state(key, "")?;
        }
        library.set_sync_state(TARGET, &target)?;
    }
    // First contact (registers written before are not queued), or a relay
    // restored from a backup (a new generation) that lost what came after
    // it: send and read everything, starting with this round's push.
    if library.sync_state(GENERATION)?.as_deref() != Some(info.generation.as_str()) {
        library.queue_all()?;
        for key in [PULL_SEQ, EPOCH_PARK] {
            library.set_sync_state(key, "")?;
        }
        library.set_sync_state(GENERATION, &info.generation)?;
    }
    Ok(())
}

pub(crate) fn options(server: &str, device_token: &str) -> RequestOptions {
    let allow_private = url::Url::parse(server)
        .ok()
        .and_then(|u| u.host_str().map(mq_http::is_private_host))
        .unwrap_or(false);
    RequestOptions {
        priority: Priority::Addon,
        timeout: Duration::from_secs(30),
        max_bytes: 16 * 1024 * 1024,
        allow_private,
        headers: match device_token {
            "" => Vec::new(),
            token => vec![("authorization".into(), format!("Bearer {token}"))],
        },
        ..Default::default()
    }
}

fn genesis_key(bundle: &Bundle) -> String {
    hex::encode(bundle.genesis.0)
}

/// Reads every roster entry from the relay starting at seq `from`, with
/// `token` (any token of the account, even a pending or revoked one).
pub(crate) async fn fetch_roster(
    http: &HttpClient,
    server: &str,
    token: &str,
    mut from: u64,
) -> Result<(Vec<Entry>, Option<RosterHead>), SyncError> {
    let mut entries = Vec::new();
    loop {
        let page: RosterPage = http
            .get(
                &format!("{server}/v1/roster?from={from}"),
                &options(server, token),
            )
            .await?
            .json()?;
        // A relay that says "more" without serving any is not followed.
        let stalled = page.entries.is_empty();
        from += page.entries.len() as u64;
        entries.extend(page.entries);
        if !page.more || stalled {
            return Ok((entries, page.head));
        }
    }
}

/// A roster seq as an index into this device's entries. A seq past `usize` (a relay's number above 4 Gi on a
/// 32-bit device) is past every entry, so a lookup finds none; `as usize` would wrap it onto one it holds.
fn index(seq: u64) -> usize {
    usize::try_from(seq).unwrap_or(usize::MAX)
}

/// Builds a roster from entries, checking each.
pub(crate) fn rebuild(entries: &[Entry]) -> Result<Roster, RosterError> {
    let mut roster = Roster::default();
    for entry in entries {
        roster.apply(entry)?;
    }
    Ok(roster)
}

fn stored_entries(library: &Library, genesis: &str) -> Result<Vec<Entry>, SyncError> {
    Ok(library
        .roster_entries(genesis)?
        .into_iter()
        .map(|(body, sig)| Entry {
            body,
            sig: Hex(sig.try_into().unwrap_or([0; 64])),
        })
        .collect())
}

fn signed(entries: &[Entry]) -> Vec<SignedEntry> {
    entries
        .iter()
        .map(|e| (e.body.clone(), e.sig.0.to_vec()))
        .collect()
}

/// One device's view of its account during a call: its keystore record,
/// its verified roster and the relay.
pub(crate) struct Session<'a> {
    pub(crate) library: &'a Library,
    pub(crate) http: &'a HttpClient,
    clock: &'a HlcClock,
    /// What the app passed in.
    stored: SyncConfig,
    /// The record as this call leaves it.
    pub(crate) record: SyncConfig,
    pub(crate) member: MemberSecret,
    pub(crate) roster: Roster,
    /// The verified entries, in order.
    entries: Vec<Entry>,
    pub(crate) info: RelayInfo,
    /// The roster moved during the call, so a key may have arrived.
    caught_up: bool,
    /// Entries this session verifies go to the library's roster store (not
    /// while settling a fork, until the settling revoke landed).
    persist: bool,
}

impl<'a> Session<'a> {
    /// Checks the relay, catches up on the roster, confirms this device is a
    /// member and takes the newest key it holds a box for.
    pub(crate) async fn open(
        library: &'a Library,
        http: &'a HttpClient,
        config: &SyncConfig,
        clock: &'a HlcClock,
    ) -> Result<Session<'a>, SyncError> {
        require_secure(&config.server)?;
        if library.sync_state(FORK)?.is_some_and(|f| !f.is_empty()) {
            return Err(SyncError::Fork);
        }
        let genesis = genesis_key(&config.keys);
        let stored = stored_entries(library, &genesis)?;
        let (entries, roster) = match rebuild(&stored) {
            Ok(roster) => (stored, roster),
            // A damaged local store: start over from the relay, guarded by
            // the keystore checkpoint.
            Err(_) => {
                library.clear_roster(&genesis)?;
                (Vec::new(), Roster::default())
            }
        };
        let mut session = Session::connect(library, http, config, clock, entries, roster).await?;
        session.catch_up().await?;
        session.joined()?;
        session.adopt()?;
        Ok(session)
    }

    /// Checks the relay is the one `config` joined and makes a session on
    /// `entries` (verified as `roster`).
    async fn connect(
        library: &'a Library,
        http: &'a HttpClient,
        config: &SyncConfig,
        clock: &'a HlcClock,
        entries: Vec<Entry>,
        roster: Roster,
    ) -> Result<Session<'a>, SyncError> {
        require_secure(&config.server)?;
        let info = relay_info(http, &config.server).await?;
        let mut record = config.clone();
        if record.relay_id == Id::default() {
            record.relay_id = info.relay_id;
        }
        pinned(library, config, &info)?;
        prepare(library, &info, &config.keys.account)?;
        Ok(Session {
            library,
            http,
            clock,
            stored: config.clone(),
            member: config.secret(),
            record,
            roster,
            entries,
            info,
            caught_up: false,
            persist: true,
        })
    }

    fn head(&self) -> RosterHead {
        let (seq, hash) = self.roster.head().unwrap_or_default();
        RosterHead { seq, hash }
    }

    pub(crate) fn token(&self) -> &str {
        &self.record.device_token
    }

    pub(crate) fn server(&self) -> &str {
        &self.record.server
    }

    fn keep(&mut self, entry: Entry) -> Result<(), SyncError> {
        if self.persist {
            let seq = self.roster.head().map_or(0, |(seq, _)| seq);
            self.library.add_roster_entry(
                &genesis_key(&self.record.keys),
                seq,
                &entry.body,
                &entry.sig.0,
            )?;
        }
        self.entries.push(entry);
        Ok(())
    }

    /// Whether the relay's head conflicts with the entry this device holds
    /// at that seq.
    fn diverged(&self, relay: &RosterHead) -> bool {
        self.entries
            .get(index(relay.seq))
            .is_some_and(|held| held.hash() != relay.hash)
    }

    /// Records a fork so it outlives this call: the relay's history as it
    /// reads now goes next to this device's, and every call fails until the
    /// user settles it. Returns the error to fail with.
    async fn forked(&self) -> SyncError {
        let theirs = fetch_roster(self.http, self.server(), self.token(), 0)
            .await
            .map(|(entries, _)| entries)
            .unwrap_or_default();
        let key = format!("{FORK}:{}", genesis_key(&self.record.keys));
        let kept = self
            .library
            .replace_roster(&key, &signed(&theirs))
            .and_then(|()| self.library.set_sync_state(FORK, "1"));
        match kept {
            Ok(()) => SyncError::Fork,
            Err(e) => e.into(),
        }
    }

    /// Reads the roster entries after this device's head from the relay,
    /// checks each, and sends the relay any it lost. A deleted account
    /// (`accountGone` from either replay or the token request): this device
    /// forgets it as a removed one does, and the library stays.
    pub(crate) async fn catch_up(&mut self) -> Result<(), SyncError> {
        let caught = self.read_roster().await;
        if matches!(caught, Err(SyncError::AccountGone)) {
            self.library.forget_sync()?;
        }
        caught
    }

    async fn read_roster(&mut self) -> Result<(), SyncError> {
        let mut refreshed = false;
        let mut replays = 0;
        let before = self.roster.head();
        loop {
            let from = self.roster.head().map_or(0, |(seq, _)| seq + 1);
            let fetched = fetch_roster(self.http, self.server(), self.token(), from).await;
            let (entries, relay) = match fetched {
                // The relay does not know this token (restored from a
                // backup): give it back the roster this device holds, which
                // may include this device's own joining, then ask for a
                // fresh token as a member.
                // A deleted account (its tokens went with it) answers 410
                // `accountGone` there.
                Err(SyncError::Http(HttpError::Status { status: 401, .. })) if !refreshed => {
                    refreshed = true;
                    self.replay(0).await?;
                    self.fresh_token().await?;
                    continue;
                }
                fetched => fetched?,
            };
            let served = !entries.is_empty();
            for entry in entries {
                match self.roster.apply(&entry) {
                    Ok(()) => self.keep(entry)?,
                    // Not built on this device's head: another history.
                    Err(RosterError::NotNext) => return Err(self.forked().await),
                    Err(e) => {
                        return Err(SyncError::Roster(format!(
                            "the relay served an entry that does not check: {e}"
                        )));
                    }
                }
            }
            let Some((seq, _)) = self.roster.head() else {
                return Err(SyncError::Roster(
                    "the relay has no roster for this account".into(),
                ));
            };
            match relay {
                // A revoked token reads up to its removal and no head.
                None => break,
                Some(relay) if self.diverged(&relay) => return Err(self.forked().await),
                // Restored from a backup that lacked these: send them back.
                // A relay that keeps not taking them is behind for good.
                Some(relay) if relay.seq < seq => {
                    if replays == TRIES {
                        return Err(SyncError::RelayBehind);
                    }
                    replays += 1;
                    self.replay(relay.seq + 1).await?;
                }
                // Appended meanwhile: read again. A head the relay does not
                // serve entries towards is not followed.
                Some(relay) if relay.seq > seq && !served => {
                    return Err(SyncError::Roster(
                        "the relay names roster entries it does not serve".into(),
                    ));
                }
                Some(relay) if relay.seq > seq => {}
                Some(_) => break,
            }
        }
        let checkpoint = self.record.checkpoint;
        match self.entries.get(index(checkpoint.seq)) {
            Some(entry) if entry.hash() != checkpoint.hash => return Err(self.forked().await),
            Some(_) => {}
            None => return Err(SyncError::RelayBehind),
        }
        if self.roster.head() != before {
            self.caught_up = true;
        }
        Ok(())
    }

    /// Sends the relay this device's entries from seq `from`; it keeps the
    /// ones it already has and appends the rest (no token needed: members
    /// signed them).
    async fn replay(&self, from: u64) -> Result<(), SyncError> {
        let Some(entries) = self.entries.get(index(from)..).filter(|e| !e.is_empty()) else {
            return Ok(());
        };
        self.http
            .post_json(
                &format!("{}/v1/roster/replay", self.server()),
                &ReplayRequest {
                    account: self.roster.account(),
                    entries: entries.to_vec(),
                },
                &options(self.server(), ""),
            )
            .await?;
        Ok(())
    }

    /// A fresh token, signed with this device's key (the relay lost the old
    /// one, for example after a restore).
    async fn fresh_token(&mut self) -> Result<(), SyncError> {
        self.record.device_token = signed_token(
            self.http,
            self.server(),
            self.record.relay_id,
            self.record.keys.account,
            &self.member,
            self.info.server_ms,
        )
        .await?;
        Ok(())
    }

    /// Fails unless this device is a member; a device the roster dropped
    /// forgets its sync state (the library stays) and says so.
    fn joined(&self) -> Result<(), SyncError> {
        let id = self.member.id();
        if self
            .roster
            .members()
            .get(&id)
            .is_some_and(|m| m.role == Role::Device)
        {
            return Ok(());
        }
        if self.roster.admitted(&id) {
            self.library.forget_sync()?;
            return Err(SyncError::Dropped);
        }
        Err(SyncError::NotJoined)
    }

    /// Takes the current epoch's key (from this device's own box when the
    /// roster moved on) and the roster head as the record's checkpoint.
    fn adopt(&mut self) -> Result<(), SyncError> {
        self.record.keys = self
            .roster
            .adopt(&self.member, &self.record.keys)
            .map_err(roster_error)?;
        self.record.checkpoint = self.head();
        Ok(())
    }

    /// Publishing waits until the app stored the record this call changed,
    /// and while a rotation is due.
    pub(crate) fn may_publish(&self) -> bool {
        self.record.keys == self.stored.keys
            && self.record.checkpoint == self.stored.checkpoint
            && !self.roster.rotation_pending()
    }

    pub(crate) fn finish(&self, mut report: SyncReport) -> SyncReport {
        report.fingerprint = self.roster.fingerprint();
        if self.roster.rotation_pending() {
            report.blocked.get_or_insert(Blocked::RotationPending);
        }
        if let Some(record) = self.changed() {
            report.record = Some(record);
            report.again = true;
        }
        report
    }

    /// The record to store, if this call changed it.
    pub(crate) fn changed(&self) -> Option<SyncConfig> {
        let mut record = self.record.clone();
        record.revision = self.stored.revision;
        (record != self.stored).then(|| SyncConfig {
            revision: self.stored.revision + 1,
            ..record
        })
    }

    /// Appends an entry built on the current head; when the head moved,
    /// catches up and builds it again, at most a few times. An entry that
    /// already landed (its answer was lost) counts as appended.
    pub(crate) async fn propose(
        &mut self,
        build: impl Fn(&Self) -> Result<Entry, SyncError>,
    ) -> Result<(), SyncError> {
        for _ in 0..TRIES {
            let entry = build(self)?;
            let appended = self
                .http
                .post_json(
                    &format!("{}/v1/roster", self.server()),
                    &AppendRequest {
                        entry: entry.clone(),
                    },
                    &options(self.server(), self.token()),
                )
                .await;
            match appended {
                Ok(_) => {
                    self.roster.apply(&entry).map_err(roster_error)?;
                    self.keep(entry)?;
                    self.adopt()?;
                    return Ok(());
                }
                Err(HttpError::Status { status: 409, .. }) => {
                    self.catch_up().await?;
                    self.adopt()?;
                    if self.entries.iter().any(|e| e.hash() == entry.hash()) {
                        return Ok(());
                    }
                }
                Err(e) => return Err(e.into()),
            }
        }
        Err(SyncError::Roster(
            "the roster keeps changing; try again".into(),
        ))
    }

    /// Starts a new epoch dropping what `dropped` names on the roster the
    /// entry is built on (and the kits that must go with them). Asked again
    /// on every try, so members added meanwhile are judged too.
    pub(crate) async fn rotate(
        &mut self,
        dropped: impl Fn(&Roster) -> Vec<Id>,
    ) -> Result<(), SyncError> {
        self.rotate_on(None, dropped).await
    }

    /// As `rotate`, built only on the head hash `reviewed` when given: a roster that moved
    /// past what the user saw refuses with `RosterChanged` instead.
    pub(crate) async fn rotate_on(
        &mut self,
        reviewed: Option<Digest32>,
        dropped: impl Fn(&Roster) -> Vec<Id>,
    ) -> Result<(), SyncError> {
        let at_ms = self.clock.tick(self.clock.wall_now()).wall_ms;
        self.propose(|s| {
            if reviewed.is_some_and(|h| h != s.head().hash) {
                return Err(SyncError::RosterChanged);
            }
            let drop = with_kits(&s.roster, dropped(&s.roster));
            let (entry, _) = s
                .roster
                .revoke(&s.member, &s.record.keys, drop, at_ms)
                .map_err(roster_error)?;
            Ok(entry)
        })
        .await
    }

    /// Automatic roster work: cancel pairings that timed out, and rotate
    /// when a member left. Waits through the grace period after a restore.
    pub(crate) async fn upkeep(&mut self, report: &mut SyncReport) -> Result<(), SyncError> {
        let now = self.info.server_ms;
        let own = self.clock.wall_now();
        for (key, deadlines) in self.library.sync_states(APPROVAL)? {
            let member = &key[APPROVAL.len()..];
            let joined = id_of(member).is_some_and(|id| self.roster.members().contains_key(&id));
            // Late by either clock, so neither a relay nor a device whose
            // clock runs slow holds a cancellation back.
            let late = deadlines
                .split(':')
                .zip([now, own])
                .any(|(deadline, now)| deadline.parse::<u64>().is_ok_and(|d| d <= now));
            if joined {
                self.library.delete_sync_state(&key)?;
            } else if late {
                let cancel = format!("{CANCEL}{member}");
                self.library.move_sync_state(&key, &cancel, "1")?;
            }
        }
        let cancels: Vec<(String, Id)> = self
            .library
            .sync_states(CANCEL)?
            .into_iter()
            .filter_map(|(key, _)| id_of(&key[CANCEL.len()..]).map(|id| (key, id)))
            .collect();
        if !self.roster.rotation_pending() && cancels.is_empty() {
            return Ok(());
        }
        let grace_end = match self.info.generation_since_ms {
            0 => 0,
            since => since + GRACE_MS,
        };
        if now < grace_end {
            report.rotation_wait_ms = grace_end - now;
            return Ok(());
        }
        // An approval that timed out always rotates: the sealed payload,
        // with the current key, may have been collected without a join.
        let ids: Vec<Id> = cancels.iter().map(|(_, id)| *id).collect();
        self.rotate(|_| ids.clone()).await?;
        for (key, _) in cancels {
            self.library.delete_sync_state(&key)?;
        }
        Ok(())
    }

    async fn push(&mut self, report: &mut SyncReport) -> Result<(), SyncError> {
        let mut restamped = HashSet::new();
        let mut behind = 0;
        loop {
            let key = AccountKey::from_bundle(&self.record.keys);
            let page = self.library.outbox(PAGE)?;
            let Some(last_id) = page.last_id else {
                return Ok(());
            };
            // As many records as fit one request; the rest stay queued.
            let mut records = Vec::new();
            let mut sent: Vec<&SyncRecord> = Vec::new();
            let mut bytes = 0;
            let mut through = last_id;
            for (id, record) in &page.records {
                let wire = seal(&key, self.record.keys.epoch, record);
                if !records.is_empty() && bytes + wire.body.len() > PUSH_BYTES {
                    through = id - 1;
                    break;
                }
                bytes += wire.body.len();
                records.push(wire);
                sent.push(record);
            }
            if records.is_empty() {
                self.library.outbox_done(last_id)?;
                continue;
            }
            let pushed = self
                .http
                .post_json(
                    &format!("{}/v1/push", self.server()),
                    &PushRequest {
                        records,
                        head: self.head().hash,
                    },
                    &options(self.server(), self.token()),
                )
                .await;
            let answered = match pushed {
                Ok(response) => Some(response.json::<PushResponse>()?),
                Err(HttpError::Status { status: 409, .. }) => None,
                // Not a member here (yet): nothing is dropped.
                Err(HttpError::Status { status: 403, .. }) => {
                    report.blocked = Some(Blocked::NotActive);
                    return Ok(());
                }
                Err(e) => return Err(e.into()),
            };
            let pin = self.head();
            let epoch = self.record.keys.epoch;
            let response = match answered {
                Some(r) if r.roster == Some(pin) && r.epoch == epoch => r,
                // Taken under a roster that conflicts with this device's.
                Some(r) if r.roster.is_some_and(|relay| self.diverged(&relay)) => {
                    return Err(self.forked().await);
                }
                // The roster or key moved (or the relay answers for another):
                // catch up and, if this device's record did not change, try
                // again. Nothing counts as sent.
                _ => {
                    behind += 1;
                    self.catch_up().await?;
                    self.adopt()?;
                    if behind >= TRIES {
                        report.blocked = Some(Blocked::RelayDisagrees);
                        return Ok(());
                    }
                    if !self.may_publish() {
                        return Ok(());
                    }
                    continue;
                }
            };
            observe_server(self.clock, response.server_ms);
            report.clock_gap_ms = self.clock.wall_now() as i64 - response.server_ms as i64;
            report.deletion = response.deletion.clone();
            report.pushed += response.accepted;
            report.stale += response.stale;
            // The relay holds a newer version of these, which this device may
            // have passed already: read from just before the oldest of them.
            if let Some(oldest) = response.stale_at.iter().min() {
                lower_cursor(self.library, oldest.saturating_sub(1))?;
            }
            let mut waiting = false;
            for refusal in &response.rejected {
                let Some(record) = sent
                    .iter()
                    .find(|r| hex::encode(key.key_id(r.coll, &r.key)) == refusal.key_id)
                else {
                    continue;
                };
                let clock = self.clock;
                match refusal.code {
                    // Written while this device's clock ran ahead. Nobody else
                    // ever accepted that version, so it is stamped again at the
                    // relay's time, once per round.
                    // A version stamped by another device (an import, or one queued
                    // after moving to a new relay) is never re-stamped: it must not
                    // jump above a real edit. It waits, queued, until the relay's
                    // clock reaches it.
                    Refusal::ClockAhead
                        if Hlc::decode(&record.hlc).is_none_or(|h| h.device != clock.device()) =>
                    {
                        self.library.requeue(record.coll, &record.key)?;
                        report.waiting += 1;
                        waiting = true;
                    }
                    Refusal::ClockAhead if restamped.insert((record.coll, record.key.clone())) => {
                        // Just above what the relay holds for this register (and no
                        // earlier than its time), so the edit still wins without
                        // pushing every device's clock to the 10-minute edge.
                        let device = clock.device();
                        let floor = Hlc {
                            wall_ms: response.server_ms.min(MAX_WALL_MS),
                            counter: 0,
                            device,
                        };
                        let above = refusal
                            .current
                            .as_deref()
                            .and_then(Hlc::decode)
                            .map(|current| current.next_for(device));
                        let at = above.map_or(floor, |above| above.max(floor));
                        clock.observe(at, clock.wall_now());
                        self.library
                            .restamp(record.coll, &record.key, &at.encode())?;
                    }
                    Refusal::ClockAhead => {
                        // Still ahead after a restamp: keep it queued for a later round.
                        self.library.requeue(record.coll, &record.key)?;
                        report.waiting += 1;
                        waiting = true;
                    }
                    _ => {
                        tracing::warn!("relay refused a register for good: {}", refusal.reason);
                        report.refused += 1;
                    }
                }
            }
            self.library.outbox_done(through)?;
            // What waits is at the tail of the outbox now: stop, so a round
            // never loops on it.
            if waiting {
                return Ok(());
            }
        }
    }

    /// The account key for a record's epoch: the current one, or an older
    /// one opened through the roster's chain of previous keys.
    fn key_for(&self, epoch: u32, keys: &mut HashMap<u32, AccountKey>) -> Option<AccountKey> {
        if let Some(key) = keys.get(&epoch) {
            return Some(key.clone());
        }
        let bundle = &self.record.keys;
        let ak = match epoch {
            e if e == bundle.epoch => bundle.ak,
            e if e < bundle.epoch => self.roster.key_at(bundle, e).ok()?,
            _ => return None,
        };
        let key = AccountKey::new(&ak.0, &bundle.id_key.0);
        keys.insert(epoch, key.clone());
        Some(key)
    }

    async fn pull(&mut self, report: &mut SyncReport) -> Result<(), SyncError> {
        let reader = format!("{READER}.{SHAPES}");
        let mut after: u64 = match self.library.sync_state(READER_SEEN)? {
            Some(seen) if seen == reader => self
                .library
                .sync_state(PULL_SEQ)?
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
            _ => 0,
        };
        // Records parked for a key this device now holds are read again: the
        // cursor goes back first, so a round that fails right after still
        // reads them next time.
        if let Some((seq, epoch)) = parked(self.library)?
            && epoch <= self.record.keys.epoch
        {
            lower_cursor(self.library, seq)?;
            after = after.min(seq);
            self.library.set_sync_state(EPOCH_PARK, "")?;
        }
        let mut keys: HashMap<u32, AccountKey> = HashMap::new();
        let mut started_over = false;
        loop {
            let page: PullResponse = self
                .http
                .get(
                    &format!("{}/v1/pull?after={after}&limit={PAGE}", self.server()),
                    &options(self.server(), self.token()),
                )
                .await?
                .json()?;
            observe_server(self.clock, page.server_ms);
            report.clock_gap_ms = self.clock.wall_now() as i64 - page.server_ms as i64;
            report.deletion = page.deletion.clone();
            let known = self.library.sync_state(GENERATION)?.unwrap_or_default();
            let restored = page.head < after || (!known.is_empty() && known != page.generation);
            if restored && !started_over {
                // The relay lost records (restored from a backup): send it
                // everything this device holds and read it all again. Once per
                // round, so a relay that keeps saying so cannot loop us.
                started_over = true;
                self.library.queue_all()?;
                self.library.set_sync_state(GENERATION, &page.generation)?;
                self.library.set_sync_state(PULL_SEQ, "0")?;
                self.library.set_sync_state(EPOCH_PARK, "")?;
                after = 0;
                continue;
            }
            self.library.set_sync_state(GENERATION, &page.generation)?;
            // The relay's roster conflicts with this device's: a fork.
            if page.roster.is_some_and(|relay| self.diverged(&relay)) {
                return Err(self.forked().await);
            }
            let pin = self.head();
            let moved = page.roster.is_some_and(|relay| relay != pin);
            let mut records = Vec::with_capacity(page.records.len());
            for wire in &page.records {
                if wire.epoch > self.record.keys.epoch && !self.caught_up {
                    // A key this device does not hold yet: the roster moved.
                    self.caught_up = true;
                    self.catch_up().await?;
                    self.adopt()?;
                    keys.clear();
                }
                let Some(key) = self.key_for(wire.epoch, &mut keys) else {
                    park(self.library, wire.seq, wire.epoch)?;
                    report.parked += 1;
                    continue;
                };
                match open(&key, wire) {
                    Ok((_, shape)) if shape > SHAPES => report.newer += 1,
                    Ok((record, _)) => {
                        self.watch_old_epoch(wire, &record, report);
                        records.push(record);
                    }
                    // Under the current key while the roster moved: maybe a
                    // key for the same epoch number on another branch.
                    Err(_) if wire.epoch == self.record.keys.epoch && moved => {
                        park(self.library, wire.seq, wire.epoch + 1)?;
                        report.parked += 1;
                    }
                    Err(e) => {
                        tracing::warn!("sync record {} did not open: {e}", wire.seq);
                        report.unreadable += 1;
                    }
                }
            }
            // The relay refused anything more than 10 minutes ahead of its own
            // time when it was pushed; the clock's absolute ceiling covers a relay
            // that lies about both.
            if let Some(newest) = records.iter().filter_map(|r| Hlc::decode(&r.hlc)).max() {
                let capped = Hlc {
                    wall_ms: newest.wall_ms.min(page.server_ms + MAX_CLOCK_AHEAD_MS),
                    ..newest
                };
                self.clock.observe(capped, self.clock.wall_now());
            }
            report.pulled += page.records.len() as u32;
            report.applied += self.library.apply(&records)? as u32;
            self.library
                .set_sync_state(PULL_SEQ, &page.last.to_string())?;
            // A relay that says "more" without moving forward is not followed.
            let stalled = page.last <= after;
            after = page.last;
            if !page.more || stalled {
                self.library.set_sync_state(READER_SEEN, &reader)?;
                return Ok(());
            }
        }
    }

    /// A record under an old key, stamped well after the rotation that
    /// replaced that key. An honest relay never accepts one; a heuristic,
    /// since clocks can be skewed and stamps backdated.
    fn watch_old_epoch(&self, wire: &WireRecord, record: &SyncRecord, report: &mut SyncReport) {
        if wire.epoch >= self.record.keys.epoch {
            return;
        }
        let rotated = self.roster.rotated_at_ms(wire.epoch + 1);
        let stamped = Hlc::decode(&record.hlc).map(|h| h.wall_ms);
        if let (Some(rotated), Some(stamped)) = (rotated, stamped)
            && stamped > rotated + MAX_CLOCK_AHEAD_MS
        {
            report.old_epoch_writes += 1;
        }
    }
}

fn id_of(text: &str) -> Option<Id> {
    hex::decode(text)
        .ok()
        .and_then(|b| b.try_into().ok())
        .map(Hex)
}

fn lower_cursor(library: &Library, to: u64) -> Result<(), SyncError> {
    let cursor: u64 = library
        .sync_state(PULL_SEQ)?
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    if to < cursor {
        library.set_sync_state(PULL_SEQ, &to.to_string())?;
    }
    Ok(())
}

fn parked(library: &Library) -> Result<Option<(u64, u32)>, SyncError> {
    Ok(library.sync_state(EPOCH_PARK)?.and_then(|text| {
        let (seq, epoch) = text.split_once(':')?;
        Some((seq.parse().ok()?, epoch.parse().ok()?))
    }))
}

/// Remembers to read again from just before `seq` once a key for `epoch`
/// arrives, before the cursor moves past it.
fn park(library: &Library, seq: u64, epoch: u32) -> Result<(), SyncError> {
    let resume = seq.saturating_sub(1);
    let (seq, epoch) = match parked(library)? {
        Some((at, e)) => (at.min(resume), e.min(epoch)),
        None => (resume, epoch),
    };
    library.set_sync_state(EPOCH_PARK, &format!("{seq}:{epoch}"))?;
    Ok(())
}

/// Lifts a slow local clock to the relay's time (like NTP) so new writes
/// sort after what other devices already pushed; a TV box that booted with
/// its clock in 1970 needs exactly this. The clock's absolute ceiling keeps
/// a lying relay from breaking the encoding.
fn observe_server(clock: &HlcClock, server_ms: u64) {
    let now = clock.wall_now();
    clock.observe(
        Hlc {
            wall_ms: server_ms,
            counter: 0,
            device: 0,
        },
        now,
    );
}

fn seal(key: &AccountKey, epoch: u32, record: &SyncRecord) -> WireRecord {
    let key_id = key.key_id(record.coll, &record.key);
    let payload = serde_json::to_vec(&Payload {
        k: record.key.clone(),
        v: record.value.clone(),
        s: SHAPES,
    })
    .expect("json values serialize");
    WireRecord {
        coll: record.coll,
        key_id: hex::encode(key_id),
        hlc: record.hlc.clone(),
        epoch,
        body: B64.encode(key.seal(record.coll, &key_id, &record.hlc, epoch, &payload)),
        seq: 0,
    }
}

/// Opens a pulled record; returns it with the shape version it was written in.
fn open(key: &AccountKey, wire: &WireRecord) -> Result<(SyncRecord, u16), SealError> {
    let key_id: [u8; 16] = hex::decode(&wire.key_id)
        .ok()
        .and_then(|k| k.try_into().ok())
        .ok_or(SealError::Malformed)?;
    let body = B64.decode(&wire.body).map_err(|_| SealError::Malformed)?;
    let plain = key.open(wire.coll, &key_id, &wire.hlc, wire.epoch, &body)?;
    let payload: Payload = serde_json::from_slice(&plain).map_err(|_| SealError::Malformed)?;
    // The body must belong to the register it was filed under.
    if key.key_id(wire.coll, &payload.k) != key_id {
        return Err(SealError::Open);
    }
    let record = SyncRecord {
        coll: wire.coll,
        key: payload.k,
        value: payload.v,
        hlc: wire.hlc.clone(),
    };
    Ok((record, payload.s))
}

/// A member of the account as the devices screen shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct MemberInfo {
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub id: Id,
    pub kit: bool,
    pub name: Option<String>,
    /// Who vouched for it.
    #[cfg_attr(feature = "ts", ts(type = "string | null"))]
    pub sponsor: Option<Id>,
    /// By the adding device's clock.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub added_ms: u64,
    pub this_device: bool,
}

/// The account's members, after catching up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct Devices {
    pub members: Vec<MemberInfo>,
    pub fingerprint: String,
    pub epoch: u32,
    pub rotation_pending: bool,
    #[cfg_attr(feature = "ts", ts(type = "Record<string, unknown> | null"))]
    pub record: Option<SyncConfig>,
}

/// What a revoke on `roster` drops for `dropped`: those that are members,
/// and the kits that would be left without their device.
fn with_kits(roster: &Roster, dropped: Vec<Id>) -> Vec<Id> {
    let mut drop: Vec<Id> = dropped
        .into_iter()
        .filter(|id| roster.members().contains_key(id))
        .collect();
    for kit in roster.kits_of(&drop) {
        if !drop.contains(&kit) {
            drop.push(kit);
        }
    }
    drop
}

/// The members of `roster` as the devices screen shows them. Names are
/// sealed under `idKey`, which never changes, so any branch's open.
fn member_infos(roster: &Roster, keys: &Bundle, me: &Id) -> Vec<MemberInfo> {
    roster
        .members()
        .iter()
        .map(|(id, m)| MemberInfo {
            id: *id,
            kit: m.role == Role::Kit,
            name: m.name.as_deref().and_then(|n| open_name(keys, id, n)),
            sponsor: m.sponsor,
            added_ms: m.added_ms,
            this_device: id == me,
        })
        .collect()
}

pub async fn devices(
    library: &Library,
    http: &HttpClient,
    config: &SyncConfig,
    clock: &HlcClock,
) -> Result<Devices, SyncError> {
    let session = Session::open(library, http, config, clock).await?;
    Ok(Devices {
        members: member_infos(&session.roster, &session.record.keys, &session.member.id()),
        fingerprint: session.roster.fingerprint(),
        epoch: session.roster.epoch(),
        rotation_pending: session.roster.rotation_pending(),
        record: session.changed(),
    })
}

/// What reporting `lost` drops by default: the devices it vouched for
/// (transitively) for the user to untick, the kits that go with all of
/// them, and the kits each of those devices made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct LostPreview {
    #[cfg_attr(feature = "ts", ts(type = "Array<string>"))]
    pub devices: Vec<Id>,
    /// Every kit that goes when the lost device and all of `devices` go.
    #[cfg_attr(feature = "ts", ts(type = "Array<string>"))]
    pub kits: Vec<Id>,
    /// The kits each of `devices` made: they stay with a device the user
    /// unticks, and go with it otherwise.
    pub device_kits: Vec<DeviceKits>,
}

pub async fn lost_preview(
    library: &Library,
    http: &HttpClient,
    config: &SyncConfig,
    clock: &HlcClock,
    lost: Id,
) -> Result<LostPreview, SyncError> {
    let session = Session::open(library, http, config, clock).await?;
    let devices = session.roster.descendants(&lost, &session.member.id());
    let mut all = devices.clone();
    all.push(lost);
    let made_by = |device: &Id| {
        session
            .roster
            .members()
            .iter()
            .filter(|(_, m)| m.role == Role::Kit && m.sponsor.as_ref() == Some(device))
            .map(|(id, _)| *id)
            .collect()
    };
    Ok(LostPreview {
        kits: session.roster.kits_of(&all),
        device_kits: devices
            .iter()
            .map(|device| DeviceKits {
                device: *device,
                kits: made_by(device),
            })
            .collect(),
        devices,
    })
}

/// Removes devices (no cascade) and starts a new key epoch without them.
/// Returns the record to store.
pub async fn remove_devices(
    library: &Library,
    http: &HttpClient,
    config: &SyncConfig,
    clock: &HlcClock,
    ids: Vec<Id>,
) -> Result<SyncConfig, SyncError> {
    let mut session = Session::open(library, http, config, clock).await?;
    if ids.contains(&session.member.id()) {
        return Err(SyncError::Roster("sign out to remove this device".into()));
    }
    session.rotate(|_| ids.clone()).await?;
    Ok(session.changed().unwrap_or_else(|| session.record.clone()))
}

/// Reports `lost` stolen or lost: drops it, every device it vouched for
/// (transitively) except those in `keep`, and every kit any of them added.
pub async fn report_lost(
    library: &Library,
    http: &HttpClient,
    config: &SyncConfig,
    clock: &HlcClock,
    lost: Id,
    keep: Vec<Id>,
) -> Result<SyncConfig, SyncError> {
    let mut session = Session::open(library, http, config, clock).await?;
    let me = session.member.id();
    if lost == me {
        return Err(SyncError::Roster("this device cannot report itself".into()));
    }
    // On the roster each try is built on: a device the lost one vouched for
    // while this ran goes too.
    session
        .rotate(|roster| {
            let mut drop: Vec<Id> = roster
                .descendants(&lost, &me)
                .into_iter()
                .filter(|id| !keep.contains(id))
                .collect();
            drop.push(lost);
            drop
        })
        .await?;
    Ok(session.changed().unwrap_or_else(|| session.record.clone()))
}

/// Signs this device out: sends what is still queued (unless `force`),
/// leaves the roster and forgets sync state; the library stays.
pub async fn sign_out(
    library: &Library,
    http: &HttpClient,
    config: &SyncConfig,
    clock: &HlcClock,
    force: bool,
) -> Result<(), SyncError> {
    let mut config = config.clone();
    for _ in 0..5 {
        let report = sync_once(library, http, &config, clock).await?;
        if let Some(record) = report.record {
            config = record;
        }
        if library.outbox(1)?.last_id.is_none() {
            break;
        }
    }
    if !force && library.outbox(1)?.last_id.is_some() {
        return Err(SyncError::Unsent);
    }
    let mut session = Session::open(library, http, &config, clock).await?;
    let at_ms = clock.tick(clock.wall_now()).wall_ms;
    session
        .propose(|s| Ok(s.roster.leave(&s.member, at_ms)))
        .await?;
    library.forget_sync()?;
    Ok(())
}

/// Deletes the account on the relay for every device: its records, roster,
/// recovery data and tokens. Returns once the relay confirmed (an account
/// already gone counts, so a retry after a lost answer succeeds); this
/// device then forgets the account as after a sign-out, and the library
/// stays. The app deletes the keystore record.
///
/// The relay needs only an active token, not an agreed roster, so the
/// stored token goes first: a device stuck on a fork can still delete
/// (while the relay still knows that token). Only a token the relay does
/// not know (a restore lost it, or the account is gone) opens a session,
/// which signs for a new one or hears `accountGone`. Before any token goes
/// out, the relay must be the one this device joined (its id, pinned to the
/// host), as for every other call. The request is signed by this device's
/// key: a token alone deletes nothing. `receipt`: an Apple revoke's.
pub async fn delete_account(
    library: &Library,
    http: &HttpClient,
    config: &SyncConfig,
    clock: &HlcClock,
    receipt: Option<&str>,
) -> Result<(), SyncError> {
    require_secure(&config.server)?;
    let info = relay_info(http, &config.server).await?;
    pinned(library, config, &info)?;
    let member = config.secret();
    let delete = |token: String, at_ms: u64| {
        let request = TokenRequest::new(
            info.relay_id,
            config.keys.account,
            member.id(),
            at_ms,
            &host(&config.server),
        );
        let body = crate::wire::DeleteRequest {
            sig: member.sign_delete(&request),
            request,
            receipt: receipt.map(str::to_owned),
        };
        async move {
            http.request(
                mq_http::Method::DELETE,
                &format!("{}/v1/account", config.server),
                Some(mq_http::Body::Json(
                    serde_json::to_value(&body).expect("plain struct"),
                )),
                &options(&config.server, &token),
            )
            .await
            .map_err(SyncError::from)
        }
    };
    let mut deleted = delete(config.device_token.clone(), info.server_ms).await;
    if matches!(
        deleted,
        Err(SyncError::Http(HttpError::Status { status: 401, .. }))
    ) {
        deleted = match Session::open(library, http, config, clock).await {
            Ok(session) => delete(session.token().to_owned(), session.info.server_ms).await,
            Err(e) => Err(e),
        };
    }
    match deleted {
        Ok(_) | Err(SyncError::AccountGone) => {}
        Err(e) => return Err(e),
    }
    library.forget_sync()?;
    Ok(())
}

/// What taking the relay's roster drops besides the user's ticks (`also`):
/// every member this device's branch had dropped and every kit only the
/// relay's branch has (a thief's kit looks like any other). Never this
/// device.
fn fork_drops(local: &Roster, server: &Roster, me: &Id, also: &[Id]) -> Vec<Id> {
    let kits = server
        .members()
        .iter()
        .filter(|(id, m)| m.role == Role::Kit && !local.admitted(id))
        .map(|(id, _)| *id);
    let mut drop: Vec<Id> = local
        .gone()
        .into_iter()
        .chain(also.iter().copied())
        .chain(kits)
        .filter(|id| id != me)
        .collect();
    drop.sort();
    drop.dedup();
    drop
}

/// Both sides of a fork, for the user to choose from before
/// `use_server_roster`: this device's members, the relay's, and what taking
/// the relay's would drop on its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct ForkPreview {
    pub mine: Vec<MemberInfo>,
    pub server: Vec<MemberInfo>,
    /// Members of the relay's branch that go whatever the user ticks.
    #[cfg_attr(feature = "ts", ts(type = "Array<string>"))]
    pub drops: Vec<Id>,
    /// For each device of the relay's side that made kits, those kits: they
    /// go with it when the user ticks it.
    pub kits: Vec<DeviceKits>,
    /// This device is a member of the relay's side. When not, it cannot take
    /// that side (`useServerRoster` refuses): pair it again.
    pub on_server: bool,
    /// The hash of the relay's last entry as shown (it commits to the whole
    /// chain). `useServerRoster` takes it back and refuses with
    /// `rosterChanged` when the roster moved since.
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub head: Digest32,
}

/// A device and the recovery kits it made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
pub struct DeviceKits {
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub device: Id,
    #[cfg_attr(feature = "ts", ts(type = "Array<string>"))]
    pub kits: Vec<Id>,
}

/// Reads both sides of a fork and writes nothing: this device's branch from
/// its store, the relay's read and checked in memory with the record's token.
pub async fn fork_preview(
    library: &Library,
    http: &HttpClient,
    config: &SyncConfig,
) -> Result<ForkPreview, SyncError> {
    require_secure(&config.server)?;
    let local =
        rebuild(&stored_entries(library, &genesis_key(&config.keys))?).map_err(roster_error)?;
    let (entries, _) = fetch_roster(http, &config.server, &config.device_token, 0).await?;
    if entries.first().map(Entry::hash) != Some(config.keys.genesis) {
        return Err(SyncError::Roster(
            "the relay serves another account's roster".into(),
        ));
    }
    let server = rebuild(&entries).map_err(|e| {
        SyncError::Roster(format!(
            "the relay served an entry that does not check: {e}"
        ))
    })?;
    let me = config.secret().id();
    let mut kits: BTreeMap<Id, Vec<Id>> = BTreeMap::new();
    for (id, m) in server.members() {
        if let Some(maker) = m.sponsor.filter(|_| m.role == Role::Kit) {
            kits.entry(maker).or_default().push(*id);
        }
    }
    Ok(ForkPreview {
        mine: member_infos(&local, &config.keys, &me),
        server: member_infos(&server, &config.keys, &me),
        drops: with_kits(&server, fork_drops(&local, &server, &me, &[])),
        kits: kits
            .into_iter()
            .map(|(device, kits)| DeviceKits { device, kits })
            .collect(),
        on_server: server.members().contains_key(&me),
        head: server.head().unwrap_or_default().1,
    })
}

/// Leaves the account on this device only: forgets its keys, roster and
/// cursors without a round and without telling the relay. The way out for a
/// device the relay will not let leave (off its list, a lost token, a fork
/// it cannot settle); the relay lists it until another device removes it.
/// The library stays, and pairing again merges it.
///
/// Only with this device's own record: its member must be one this device
/// stored the admission of, which a caller without the record's member
/// secret cannot fake. Anything else is `WrongRecord` and wipes nothing (any
/// page that can reach the core could otherwise send one). Each admission
/// is read on its own, not through a rebuilt roster: a store whose history
/// no longer verifies is the case this way out exists for.
pub fn leave_here(library: &Library, config: &SyncConfig) -> Result<(), SyncError> {
    let me = config.secret().id();
    let admits_me = |entry: &Entry| match entry.parse().map(|b| b.kind) {
        Ok(Kind::Genesis(g)) => g.keys.id() == me,
        Ok(Kind::AddDevice(a)) => a.keys.id() == me,
        _ => false,
    };
    if !stored_entries(library, &genesis_key(&config.keys))?
        .iter()
        .any(admits_me)
    {
        return Err(SyncError::WrongRecord);
    }
    library.forget_sync()?;
    Ok(())
}

/// After a fork: takes the relay's roster instead of this device's, drops
/// again what this device's branch had dropped (plus `also_drop`, ticked by
/// the user, and every kit only the relay's branch has), and starts a new
/// key epoch so nothing sealed under the abandoned branch's key survives.
///
/// This device's own branch stays in its store (and the fork flag stays
/// set) until that revoke has landed, so a call that fails anywhere before
/// can simply be made again.
pub async fn use_server_roster(
    library: &Library,
    http: &HttpClient,
    config: &SyncConfig,
    clock: &HlcClock,
    also_drop: Vec<Id>,
    reviewed: Option<Digest32>,
) -> Result<SyncConfig, SyncError> {
    require_secure(&config.server)?;
    let genesis = genesis_key(&config.keys);
    let mine = stored_entries(library, &genesis)?;
    let local = rebuild(&mine).map_err(roster_error)?;
    let member = config.secret();
    let me = member.id();
    // The newest key this device holds on its own branch (the record may
    // lag behind its roster store).
    let held = local.adopt(&member, &config.keys).map_err(roster_error)?;
    // The relay's roster, read and checked in memory only.
    let detached = SyncConfig {
        checkpoint: RosterHead {
            seq: 0,
            hash: config.keys.genesis,
        },
        ..config.clone()
    };
    let mut session = Session::connect(
        library,
        http,
        &detached,
        clock,
        Vec::new(),
        Roster::default(),
    )
    .await?;
    session.persist = false;
    session.catch_up().await?;
    if !session.roster.members().contains_key(&me) {
        return Err(SyncError::Pairing(
            "this device is not on the server's roster; pair it again".into(),
        ));
    }
    // The key at the last epoch both branches share opens this device's box
    // on the server's branch.
    let common = mine
        .iter()
        .zip(&session.entries)
        .take_while(|(a, b)| a.hash() == b.hash())
        .count();
    let shared = rebuild(&mine[..common]).map_err(roster_error)?;
    let base = Bundle {
        epoch: shared.epoch(),
        ak: local.key_at(&held, shared.epoch()).map_err(roster_error)?,
        ..held.clone()
    };
    session.record.keys = session.roster.adopt(&member, &base).map_err(roster_error)?;
    session.record.checkpoint = session.head();
    session
        .rotate_on(reviewed, |server| {
            fork_drops(&local, server, &me, &also_drop)
        })
        .await?;
    // Landed: the store takes the relay's branch, then everything is sent
    // and read again under the new key. The fork flag goes last.
    library.replace_roster(&genesis, &signed(&session.entries))?;
    for key in [PULL_SEQ, EPOCH_PARK] {
        library.set_sync_state(key, "")?;
    }
    library.queue_all()?;
    library.clear_roster(&format!("{FORK}:{genesis}"))?;
    library.delete_sync_state(FORK)?;
    let mut record = session.record.clone();
    record.revision = config.revision + 1;
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seq_past_usize_is_past_every_entry() {
        assert_eq!(index(7), 7);
        // On a 32-bit device `as usize` turned 2^32 + 1 into entry 1.
        let past = (1u64 << 32) + 1;
        assert!(index(past) > 1);
        #[cfg(target_pointer_width = "32")]
        assert_eq!(index(past), usize::MAX);
    }

    #[test]
    fn only_an_answer_that_never_came_is_transient() {
        let status = |status| {
            SyncError::Http(HttpError::Status {
                status,
                body: Default::default(),
                retry_after: None,
            })
        };
        let again = [
            SyncError::Http(HttpError::Network("reset".into())),
            SyncError::Http(HttpError::Timeout(std::time::Duration::from_secs(1))),
            status(502),
            SyncError::Busy,
        ];
        assert!(again.iter().all(SyncError::transient));
        // After a 2xx the relay acted: a body that did not decode, or a refusal.
        let done = [
            SyncError::Http(HttpError::Decode("eof".into())),
            SyncError::Http(HttpError::TooLarge { limit: 1 }),
            status(404),
            SyncError::SignInRefused,
        ];
        assert!(!done.iter().any(SyncError::transient));
    }
}

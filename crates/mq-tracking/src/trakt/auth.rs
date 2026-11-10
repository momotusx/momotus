//! Sign-in with the device code, token refresh with acknowledged rotation,
//! and revoke. Refresh tokens are single use (research/trakt.md section 1),
//! so one refresh runs at a time and the rotated record is held until the
//! app says it stored it.

use super::api::{Answer, Api};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::Instant;

/// After a refresh Trakt did not answer, calls fail fast for this long.
const RETRY_AFTER: Duration = Duration::from_secs(30);

/// What the app keeps in its keystore, opaque to it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraktRecord {
    pub access_token: String,
    pub refresh_token: String,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
    pub scope: String,
    /// The Trakt user's stable id (`ids.uuid`) the tokens belong to.
    pub account: String,
    /// The user's name, to show.
    #[serde(default)]
    pub user: String,
    /// Raised on every rotation.
    pub revision: u64,
    /// Raised on every new connection.
    pub generation: u64,
    /// The sign-in this token chain came from (16 random bytes, hex), kept
    /// through rotations; one Momotus account shares it (plans/trakt-account.md
    /// 3.1). Empty in a record stored before chains existed.
    #[serde(default)]
    pub chain: String,
}

impl fmt::Debug for TraktRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TraktRecord")
            .field("account", &self.account)
            .field("chain", &self.chain)
            .field("revision", &self.revision)
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

/// The code a user types (or scans) at the page Trakt names in
/// `verification_url` (auth.trakt.tv/activate).
#[derive(Clone, PartialEq)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_url: String,
    pub expires_in: Duration,
    pub interval: Duration,
}

impl fmt::Debug for DeviceCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The device code polls for tokens; it never goes to a log.
        f.debug_struct("DeviceCode")
            .field("user_code", &self.user_code)
            .field("verification_url", &self.verification_url)
            .finish_non_exhaustive()
    }
}

/// One poll of a device code.
#[derive(Debug, Clone, PartialEq)]
pub enum Poll {
    Approved(Tokens),
    Pending,
    /// Polling too fast: wait 5 s longer from now on.
    SlowDown,
    Expired,
    Denied,
    /// Already used or unknown: start over.
    Invalid,
    Transient(String),
}

/// A token answer before the account is known.
#[derive(Clone, PartialEq)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
    pub scope: String,
}

impl fmt::Debug for Tokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Tokens(..)")
    }
}

/// The connection a call belongs to: work started under it never goes out
/// with another account's tokens or a later sign-in's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bound {
    pub account: String,
    pub generation: u64,
}

/// A sign-in in progress (`Auth::ticket`), for `Auth::adopt`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ticket {
    generation: u64,
    login: u64,
}

/// Why a call has no usable token.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    #[error("not signed in to Trakt")]
    Off,
    /// The tokens are dead; the user must sign in again.
    #[error("sign in to Trakt again: {0}")]
    Reconnect(String),
    #[error("Trakt did not answer: {0}")]
    Transient(String),
}

pub async fn start(api: &Api) -> Result<DeviceCode, String> {
    match api
        .auth("/oauth/device/code", &json!({"client_id": api.client_id()}))
        .await
    {
        Answer::Ok { body, .. } => {
            device_code(&body).ok_or_else(|| "Trakt sent an incomplete device code".to_owned())
        }
        Answer::Refused { status, error } => Err(refusal(status, error.as_deref())),
        Answer::Transient { reason, .. } => Err(reason),
    }
}

fn device_code(body: &Value) -> Option<DeviceCode> {
    let secs = |key: &str| Duration::from_secs(body[key].as_u64().unwrap_or(0));
    let text = |key: &str| body[key].as_str().map(str::to_owned);
    Some(DeviceCode {
        device_code: text("device_code")?,
        user_code: text("user_code")?,
        verification_url: text("verification_url")?,
        expires_in: secs("expires_in"),
        interval: secs("interval").max(Duration::from_secs(1)),
    })
}

pub async fn poll(api: &Api, code: &DeviceCode) -> Poll {
    let body = json!({"code": code.device_code, "client_id": api.client_id()});
    match api.auth("/oauth/device/token", &body).await {
        Answer::Ok { status: 200, body } => match tokens(&body) {
            Some(t) => Poll::Approved(t),
            None => Poll::Transient("Trakt sent incomplete tokens".into()),
        },
        Answer::Ok { .. } => Poll::Pending,
        Answer::Refused { status, .. } => match status {
            400 => Poll::Pending,
            404 | 409 => Poll::Invalid,
            410 => Poll::Expired,
            418 => Poll::Denied,
            _ => Poll::Transient(format!("status {status}")),
        },
        // 429 is "slow down" here, not an outage.
        Answer::Transient { reason, .. } if reason == "status 429" => Poll::SlowDown,
        Answer::Transient { reason, .. } => Poll::Transient(reason),
    }
}

/// Exchanges the refresh token. Terminal answers mean the chain is dead.
pub async fn refresh(
    api: &Api,
    redirect_uri: &str,
    refresh_token: &str,
) -> Result<Tokens, AuthError> {
    let body = json!({
        "refresh_token": refresh_token,
        "client_id": api.client_id(),
        "redirect_uri": redirect_uri,
        "grant_type": "refresh_token",
    });
    match api.auth("/oauth/token", &body).await {
        Answer::Ok { body, .. } => {
            tokens(&body).ok_or_else(|| AuthError::Transient("Trakt sent incomplete tokens".into()))
        }
        Answer::Refused { status, error } => {
            Err(AuthError::Reconnect(refusal(status, error.as_deref())))
        }
        Answer::Transient { reason, .. } => Err(AuthError::Transient(reason)),
    }
}

/// Best effort: the token is dropped locally whatever Trakt answers.
pub async fn revoke(api: &Api, access_token: &str) {
    let body = json!({"token": access_token, "client_id": api.client_id()});
    if let Answer::Transient { reason, .. } = api.auth("/oauth/revoke", &body).await {
        tracing::debug!("trakt revoke not confirmed: {reason}");
    }
}

fn tokens(body: &Value) -> Option<Tokens> {
    let created_s = body["created_at"]
        .as_u64()
        .unwrap_or_else(|| now_ms() / 1000);
    let expires_s = body["expires_in"].as_u64()?;
    Some(Tokens {
        access_token: body["access_token"].as_str()?.to_owned(),
        refresh_token: body["refresh_token"].as_str()?.to_owned(),
        created_at_ms: created_s.checked_mul(1000)?,
        expires_at_ms: created_s.checked_add(expires_s)?.checked_mul(1000)?,
        scope: body["scope"].as_str().unwrap_or_default().to_owned(),
    })
}

/// Only the status and Trakt's `error` field: an auth body can hold tokens.
fn refusal(status: u16, error: Option<&str>) -> String {
    match error {
        Some(error) => format!("status {status} ({error})"),
        None => format!("status {status}"),
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

#[derive(Default)]
struct State {
    record: Option<TraktRecord>,
    acked: bool,
    dead: Option<String>,
    generation: u64,
    /// Sign-ins begun; only the newest may adopt.
    logins: u64,
    /// After a refresh that got no answer, no new refresh before this.
    retry_at: Option<Instant>,
    /// When this device rotated the record or took it from the account, by
    /// its own clock (0: handed over by the app). The 60-second check reads
    /// this, never Trakt's `created_at` from another device.
    received_at_ms: u64,
    /// In a Momotus account the chain is shared: calls use the held token
    /// and only the account's round refreshes it (`refresh_now`), on one
    /// device (plans/trakt-account.md 3.4).
    shared: bool,
    /// Shared: Trakt refused this revision's refresh token. The held access
    /// token stays in use until a newer revision arrives or it is refused.
    spent: bool,
    /// Shared: Trakt refused the held access token. `Reconnect` until a
    /// newer revision arrives or the round's refresh works.
    refused: bool,
}

/// A new sign-in's chain id: 16 random bytes, hex.
fn new_chain() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("os randomness");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The tokens of one profile. The state sits behind a short lock that is
/// never held across a network call; refreshes run one at a time in their
/// own task, so a caller that goes away cannot lose a rotated token.
pub struct Auth {
    redirect_uri: String,
    state: Arc<Mutex<State>>,
    refreshing: Arc<tokio::sync::Mutex<()>>,
}

impl Auth {
    pub fn new(redirect_uri: &str) -> Self {
        Self {
            redirect_uri: redirect_uri.to_owned(),
            state: Arc::default(),
            refreshing: Arc::default(),
        }
    }

    /// A record from the app (at start, or after `adopt` and a rotation, as
    /// the acknowledgement), or `None` to sign out. Returns whether it was
    /// taken: an acknowledgement for an older revision or connection is not.
    pub async fn set(&self, record: Option<TraktRecord>) -> bool {
        let mut s = lock(&self.state);
        match (record, &s.record) {
            (None, _) => {
                *s = State {
                    generation: s.generation + 1,
                    // Every sign-in begun before is stale.
                    logins: s.logins + 1,
                    shared: s.shared,
                    ..State::default()
                };
                true
            }
            (Some(r), Some(held))
                if r.account == held.account && r.generation == held.generation =>
            {
                if r.revision != held.revision {
                    return false;
                }
                // A record without the chain it was given is not stored yet.
                s.acked = r.chain == held.chain;
                true
            }
            // A record from an older connection (a late acknowledgement after
            // a sign-out or a new sign-in) never comes back.
            (Some(r), _) if r.generation < s.generation => false,
            (Some(mut r), _) => {
                s.generation = r.generation;
                // A record stored before chains existed gets one, and is
                // offered to the app again to store with it.
                s.acked = !r.chain.is_empty();
                if r.chain.is_empty() {
                    r.chain = new_chain();
                }
                s.record = Some(r);
                s.dead = None;
                s.retry_at = None;
                s.spent = false;
                s.refused = false;
                s.received_at_ms = 0;
                // A record installed or replaced (even at the same
                // generation) makes every sign-in begun before it stale.
                s.logins += 1;
                true
            }
        }
    }

    /// A sign-in begins: its ticket, which a later sign-in, a sign-out or a
    /// record from the app makes stale.
    pub async fn ticket(&self) -> Ticket {
        let mut s = lock(&self.state);
        s.logins += 1;
        Ticket {
            generation: s.generation,
            login: s.logins,
        }
    }

    /// A new sign-in: a new connection generation, held until acknowledged.
    /// Taken only while `ticket` is the newest sign-in's and nothing else
    /// happened since it began; checked and written under one lock.
    pub async fn adopt(
        &self,
        tokens: Tokens,
        account: &str,
        user: &str,
        ticket: Ticket,
    ) -> Option<TraktRecord> {
        let mut s = lock(&self.state);
        if s.generation != ticket.generation || s.logins != ticket.login {
            return None;
        }
        s.generation += 1;
        let record = TraktRecord {
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            created_at_ms: tokens.created_at_ms,
            expires_at_ms: tokens.expires_at_ms,
            scope: tokens.scope,
            account: account.to_owned(),
            user: user.to_owned(),
            revision: 0,
            generation: s.generation,
            chain: new_chain(),
        };
        s.record = Some(record.clone());
        s.acked = false;
        s.dead = None;
        s.retry_at = None;
        s.spent = false;
        s.refused = false;
        Some(record)
    }

    /// Another chain's record, from the account's link, as a new connection:
    /// work started under the old one is fenced off. Held until the app
    /// stores it.
    pub async fn take(&self, record: TraktRecord) -> TraktRecord {
        let mut s = lock(&self.state);
        let generation = s.generation + 1;
        let record = TraktRecord {
            generation,
            ..record
        };
        *s = State {
            record: Some(record.clone()),
            generation,
            logins: s.logins + 1,
            received_at_ms: now_ms(),
            shared: s.shared,
            ..State::default()
        };
        record
    }

    /// A newer revision of the held chain (another device rotated it): the
    /// tokens are replaced and the connection goes on, so `Bound` and running
    /// sessions keep working. Taken for the same account and chain at a
    /// higher revision, or at the same one with other tokens.
    pub async fn advance(&self, record: &TraktRecord) -> bool {
        let mut s = lock(&self.state);
        let generation = s.generation;
        let Some(held) = &s.record else {
            return false;
        };
        let newer = record.revision > held.revision
            || (record.revision == held.revision && record.refresh_token != held.refresh_token);
        if !newer
            || held.chain.is_empty()
            || (&held.account, &held.chain) != (&record.account, &record.chain)
        {
            return false;
        }
        s.record = Some(TraktRecord {
            generation,
            ..record.clone()
        });
        s.acked = false;
        s.dead = None;
        s.retry_at = None;
        s.spent = false;
        s.refused = false;
        s.received_at_ms = now_ms();
        true
    }

    /// The record held now, acknowledged or not.
    pub async fn record(&self) -> Option<TraktRecord> {
        lock(&self.state).record.clone()
    }

    /// Whether the chain is the Momotus account's (see `State::shared`).
    pub async fn set_shared(&self, shared: bool) {
        lock(&self.state).shared = shared;
    }

    /// The account's refresh, on the one device that makes it: rotates when
    /// under the spread of the token's lifetime is left, or Trakt refused the
    /// held token, unless this revision's refresh was already refused.
    /// Returns the rotated record.
    pub async fn refresh_now(&self, api: &Api) -> Option<TraktRecord> {
        let bound = {
            let s = lock(&self.state);
            let record = s.record.as_ref()?;
            if s.dead.is_some() || s.spent || (fresh(record) && !s.refused) {
                return None;
            }
            Bound {
                account: record.account.clone(),
                generation: s.generation,
            }
        };
        self.rotate(api, None, &bound, true).await.ok()?;
        self.record().await
    }

    /// The record the app has not stored yet.
    pub async fn unacked(&self) -> Option<TraktRecord> {
        let s = lock(&self.state);
        s.record.clone().filter(|_| !s.acked)
    }

    pub async fn account(&self) -> Option<String> {
        lock(&self.state).record.as_ref().map(|r| r.account.clone())
    }

    /// The user's name, while a record is held.
    pub async fn user(&self) -> Option<String> {
        lock(&self.state).record.as_ref().map(|r| r.user.clone())
    }

    /// The access token held now, as it is: a sign-out revokes it without
    /// waiting for a refresh.
    pub async fn held_token(&self) -> Option<String> {
        lock(&self.state)
            .record
            .as_ref()
            .map(|r| r.access_token.clone())
    }

    /// Raised by every sign-in, sign-out and new record: work started under
    /// an older one drops its results.
    pub async fn generation(&self) -> u64 {
        lock(&self.state).generation
    }

    /// The connection held now, to bind work to.
    pub async fn bound(&self) -> Option<Bound> {
        let s = lock(&self.state);
        s.record.as_ref().map(|r| Bound {
            account: r.account.clone(),
            generation: s.generation,
        })
    }

    /// A token of `bound`'s connection to call with, refreshed first when
    /// under a quarter of its lifetime is left. Another connection's token is
    /// never given: the answer is `Off`.
    pub async fn token(&self, api: &Api, bound: &Bound) -> Result<String, AuthError> {
        {
            let s = lock(&self.state);
            let record = usable_for(&s, bound)?;
            // A shared chain is refreshed only by the account's round.
            if s.shared || fresh(record) {
                return Ok(record.access_token.clone());
            }
        }
        self.rotate(api, None, bound, false).await
    }

    /// After a 401 with `used`: refresh once, unless another caller already
    /// did (then the newer token is the answer). A second 401 for a token
    /// that came from a refresh is terminal.
    pub async fn after_401(
        &self,
        api: &Api,
        used: &str,
        bound: &Bound,
    ) -> Result<String, AuthError> {
        {
            let mut s = lock(&self.state);
            let record = usable_for(&s, bound)?;
            if record.access_token != used {
                return Ok(record.access_token.clone());
            }
            // A shared chain waits for a newer revision from the account,
            // or for the round's refresh.
            if s.shared {
                s.refused = true;
                return Err(AuthError::Reconnect("Trakt refused the token".into()));
            }
            if record.revision > 0 && s.received_at_ms + 60_000 > now_ms() {
                s.dead = Some("Trakt refused a fresh token".into());
                return Err(AuthError::Reconnect("Trakt refused a fresh token".into()));
            }
        }
        self.rotate(api, Some(used), bound, false).await
    }

    /// Trakt refused `used` right after it was given: the chain is dead and
    /// the user signs in again.
    pub async fn reject(&self, used: &str, bound: &Bound) {
        let mut s = lock(&self.state);
        let current = usable_for(&s, bound).is_ok_and(|r| r.access_token == used);
        if current {
            s.dead = Some("Trakt refused a fresh token".into());
        }
    }

    async fn rotate(
        &self,
        api: &Api,
        used: Option<&str>,
        bound: &Bound,
        forced: bool,
    ) -> Result<String, AuthError> {
        // One refresh at a time; the guard moves into the refresh task, so
        // it is held until the refresh finishes even if this caller goes.
        let guard = self.refreshing.clone().lock_owned().await;
        let (old, generation) = {
            let s = lock(&self.state);
            // The account's refresh goes on for a refused token.
            let record = if forced {
                held_for(&s, bound)?
            } else {
                usable_for(&s, bound)?
            };
            // Another caller may have rotated while this one waited.
            let rotated = match used {
                Some(used) => record.access_token != used,
                None => !forced && fresh(record),
            };
            if rotated {
                return Ok(record.access_token.clone());
            }
            if s.retry_at.is_some_and(|at| Instant::now() < at) {
                return Err(AuthError::Transient(
                    "waiting to retry the token refresh".into(),
                ));
            }
            (record.clone(), s.generation)
        };
        let (api, state, redirect) = (api.clone(), self.state.clone(), self.redirect_uri.clone());
        let task = tokio::spawn(async move {
            let _guard = guard;
            let result = refresh(&api, &redirect, &old.refresh_token).await;
            let mut s = lock(&state);
            // Under the same lock as the write: signed out, signed in again
            // or handed another account's record (even at this generation)
            // meanwhile, the result is moot.
            let same = s.record.as_ref().is_some_and(|r| r.account == old.account);
            if s.generation != generation || !same {
                return Err(AuthError::Off);
            }
            match result {
                Ok(t) => {
                    let record = TraktRecord {
                        access_token: t.access_token,
                        refresh_token: t.refresh_token,
                        created_at_ms: t.created_at_ms,
                        expires_at_ms: t.expires_at_ms,
                        scope: t.scope,
                        revision: old.revision + 1,
                        ..old
                    };
                    let token = record.access_token.clone();
                    s.record = Some(record);
                    s.acked = false;
                    s.retry_at = None;
                    s.refused = false;
                    s.received_at_ms = now_ms();
                    Ok(token)
                }
                // A shared chain's refresh token may have been spent by
                // another device: wait for its newer revision.
                Err(AuthError::Reconnect(reason)) if s.shared => {
                    s.spent = true;
                    Err(AuthError::Reconnect(reason))
                }
                Err(AuthError::Reconnect(reason)) => {
                    s.dead = Some(reason.clone());
                    Err(AuthError::Reconnect(reason))
                }
                Err(AuthError::Transient(reason)) => {
                    s.retry_at = Instant::now().checked_add(RETRY_AFTER);
                    Err(AuthError::Transient(reason))
                }
                Err(AuthError::Off) => Err(AuthError::Off),
            }
        });
        task.await
            .unwrap_or_else(|e| Err(AuthError::Transient(format!("refresh task: {e}"))))
    }

    /// Why calls cannot be made, if they cannot.
    pub async fn status(&self) -> Result<(), AuthError> {
        usable(&lock(&self.state)).map(|_| ())
    }
}

/// More than the spread of the token's lifetime is left.
fn fresh(record: &TraktRecord) -> bool {
    fresh_at(record, now_ms(), spread())
}

/// Refreshed once under `lifetime * (0.25 + 0.15 * u)` is left, `u` in
/// thousandths: Trakt documents 24-hour tokens, ours measured 7 days, so the
/// margin follows the lifetime, never a fixed number of hours.
fn fresh_at(record: &TraktRecord, now: u64, u_permille: u64) -> bool {
    let lifetime = record.expires_at_ms.saturating_sub(record.created_at_ms);
    let margin = lifetime.saturating_mul(250_000 + 150 * u_permille.min(1000)) / 1_000_000;
    now.saturating_add(margin) < record.expires_at_ms
}

/// `u` for `fresh_at`, drawn once per process.
fn spread() -> u64 {
    static U: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *U.get_or_init(|| {
        let mut bytes = [0u8; 2];
        getrandom::fill(&mut bytes).expect("os randomness");
        u64::from(u16::from_le_bytes(bytes)) % 1000
    })
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn usable(s: &State) -> Result<&TraktRecord, AuthError> {
    if s.refused {
        return Err(AuthError::Reconnect("Trakt refused the token".into()));
    }
    held(s)
}

fn held(s: &State) -> Result<&TraktRecord, AuthError> {
    if let Some(reason) = &s.dead {
        return Err(AuthError::Reconnect(reason.clone()));
    }
    s.record.as_ref().ok_or(AuthError::Off)
}

/// The record, when it is still `bound`'s connection.
fn usable_for<'a>(s: &'a State, bound: &Bound) -> Result<&'a TraktRecord, AuthError> {
    usable(s)?;
    held_for(s, bound)
}

/// `usable_for`, also while Trakt refuses the held token.
fn held_for<'a>(s: &'a State, bound: &Bound) -> Result<&'a TraktRecord, AuthError> {
    let record = held(s)?;
    if record.account == bound.account && s.generation == bound.generation {
        Ok(record)
    } else {
        Err(AuthError::Off)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lasting(lifetime_ms: u64) -> TraktRecord {
        TraktRecord {
            access_token: "a".into(),
            refresh_token: "r".into(),
            created_at_ms: 0,
            expires_at_ms: lifetime_ms,
            scope: String::new(),
            account: "u".into(),
            user: "u".into(),
            revision: 0,
            generation: 1,
            chain: "c".into(),
        }
    }

    /// For 24-hour and 7-day tokens alike: never refreshed with over 40% of
    /// the lifetime left, always with under 25%, and the margin moves with u.
    #[test]
    fn the_refresh_spread_holds_for_24_hour_and_7_day_tokens() {
        for lifetime in [86_400_000, 7 * 86_400_000] {
            let record = lasting(lifetime);
            let at = |left_percent: u64| lifetime - lifetime * left_percent / 100;
            for u in [0, 500, 999] {
                assert!(fresh_at(&record, at(41), u), "{lifetime} {u}");
                assert!(!fresh_at(&record, at(24), u), "{lifetime} {u}");
            }
            assert!(fresh_at(&record, at(30), 0));
            assert!(!fresh_at(&record, at(30), 999));
        }
    }
}

//! Sign-in with the device code, token refresh with acknowledged rotation,
//! and revoke. Refresh tokens are single use (research/trakt.md section 1),
//! so one refresh runs at a time and the rotated record is held until the
//! app says it stored it.

use super::api::{Answer, Api};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

/// What the app keeps in its keystore, opaque to it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraktRecord {
    pub access_token: String,
    pub refresh_token: String,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
    pub scope: String,
    /// The Trakt user slug the tokens belong to.
    pub account: String,
    /// Raised on every rotation.
    pub revision: u64,
    /// Raised on every new connection.
    pub generation: u64,
}

impl fmt::Debug for TraktRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TraktRecord")
            .field("account", &self.account)
            .field("revision", &self.revision)
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

/// The code a user types (or scans) at trakt.tv/activate.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_url: String,
    pub expires_in: Duration,
    pub interval: Duration,
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
        created_at_ms: created_s * 1000,
        expires_at_ms: (created_s + expires_s) * 1000,
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
}

/// The tokens of one profile, refreshed one at a time.
pub struct Auth {
    redirect_uri: String,
    state: Mutex<State>,
}

impl Auth {
    pub fn new(redirect_uri: &str) -> Self {
        Self {
            redirect_uri: redirect_uri.to_owned(),
            state: Mutex::default(),
        }
    }

    /// A record from the app (at start, or after `adopt` and a rotation, as
    /// the acknowledgement), or `None` to sign out. Returns whether it was
    /// taken: an acknowledgement for an older revision or connection is not.
    pub async fn set(&self, record: Option<TraktRecord>) -> bool {
        let mut s = self.state.lock().await;
        match (record, &s.record) {
            (None, _) => {
                s.generation += 1;
                *s = State {
                    generation: s.generation,
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
                s.acked = true;
                true
            }
            // A record from an older connection (a late acknowledgement after
            // a sign-out or a new sign-in) never comes back.
            (Some(r), _) if r.generation < s.generation => false,
            (Some(r), _) => {
                s.generation = r.generation;
                s.record = Some(r);
                s.acked = true;
                s.dead = None;
                true
            }
        }
    }

    /// A new sign-in: a new connection generation, held until acknowledged.
    pub async fn adopt(&self, tokens: Tokens, account: &str) -> TraktRecord {
        let mut s = self.state.lock().await;
        s.generation += 1;
        let record = TraktRecord {
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            created_at_ms: tokens.created_at_ms,
            expires_at_ms: tokens.expires_at_ms,
            scope: tokens.scope,
            account: account.to_owned(),
            revision: 0,
            generation: s.generation,
        };
        s.record = Some(record.clone());
        s.acked = false;
        s.dead = None;
        record
    }

    /// The record the app has not stored yet.
    pub async fn unacked(&self) -> Option<TraktRecord> {
        let s = self.state.lock().await;
        s.record.clone().filter(|_| !s.acked)
    }

    pub async fn account(&self) -> Option<String> {
        self.state
            .lock()
            .await
            .record
            .as_ref()
            .map(|r| r.account.clone())
    }

    /// A token to call with, refreshed first when under a quarter of its
    /// lifetime is left.
    pub async fn token(&self, api: &Api) -> Result<String, AuthError> {
        let mut s = self.state.lock().await;
        let record = usable(&s)?;
        let lifetime = record.expires_at_ms.saturating_sub(record.created_at_ms);
        if now_ms() + lifetime / 4 < record.expires_at_ms {
            return Ok(record.access_token.clone());
        }
        self.rotate(api, &mut s).await
    }

    /// After a 401 with `used`: refresh once, unless another caller already
    /// did (then the newer token is the answer). A second 401 for a token
    /// that came from a refresh is terminal.
    pub async fn after_401(&self, api: &Api, used: &str) -> Result<String, AuthError> {
        let mut s = self.state.lock().await;
        let record = usable(&s)?;
        if record.access_token != used {
            return Ok(record.access_token.clone());
        }
        if record.revision > 0 && record.created_at_ms + 60_000 > now_ms() {
            s.dead = Some("Trakt refused a fresh token".into());
            return Err(AuthError::Reconnect("Trakt refused a fresh token".into()));
        }
        self.rotate(api, &mut s).await
    }

    async fn rotate(&self, api: &Api, s: &mut State) -> Result<String, AuthError> {
        let Some(old) = s.record.clone() else {
            return Err(AuthError::Off);
        };
        match refresh(api, &self.redirect_uri, &old.refresh_token).await {
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
                Ok(token)
            }
            Err(AuthError::Reconnect(reason)) => {
                s.dead = Some(reason.clone());
                Err(AuthError::Reconnect(reason))
            }
            Err(e) => Err(e),
        }
    }

    /// Why calls cannot be made, if they cannot.
    pub async fn status(&self) -> Result<(), AuthError> {
        usable(&*self.state.lock().await).map(|_| ())
    }
}

fn usable(s: &State) -> Result<&TraktRecord, AuthError> {
    if let Some(reason) = &s.dead {
        return Err(AuthError::Reconnect(reason.clone()));
    }
    s.record.as_ref().ok_or(AuthError::Off)
}

//! Sign-in with a code shown on the device, for the services that offer it:
//! Real-Debrid (device flow, the open source client id), AllDebrid (PIN) and
//! TorBox (device flow). The user approves on a phone; the device polls.
//! Nothing here logs a device code, key or token.

use crate::realdebrid::Credentials;
use crate::{DebridError, StoreId, alldebrid, realdebrid, torbox};
use mq_http::HttpClient;
use std::fmt;
use std::time::Duration;
use tokio::time::Instant;

/// What the device shows while it waits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Code {
    /// What the user types.
    pub user_code: String,
    /// Where the user types it.
    pub url: String,
    /// What a QR code carries: a page and the user code, never a secret the
    /// device polls with.
    pub qr: String,
    /// Wait this long before every poll, the first included.
    pub interval: Duration,
    pub expires_in: Duration,
}

/// What a finished sign-in yields.
#[derive(Clone, PartialEq, Eq)]
pub enum Credential {
    /// The account's API key (AllDebrid, TorBox).
    Key(String),
    /// Real-Debrid's user-bound client and tokens.
    OAuth(Credentials),
}

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Credential::Key(_) => f.write_str("Key(<redacted>)"),
            Credential::OAuth(c) => f.debug_tuple("OAuth").field(c).finish(),
        }
    }
}

#[derive(Debug)]
pub enum LoginPoll {
    Pending,
    /// Asked to poll less often: add 5 s to the interval.
    SlowDown,
    Approved(Credential),
    Expired,
    Denied,
    /// TorBox: the key was already collected; only a new sign-in helps.
    Used,
}

enum Flow {
    RealDebrid(realdebrid::Device),
    AllDebrid(alldebrid::Pin),
    TorBox(torbox::Device),
}

pub struct Login {
    flow: Flow,
    code: Code,
    deadline: Instant,
}

impl Login {
    /// Starts a sign-in against the service's own host.
    pub async fn start(http: &HttpClient, store: StoreId) -> Result<Self, DebridError> {
        let base = match store {
            StoreId::RealDebrid => realdebrid::OAUTH_BASE,
            StoreId::AllDebrid => alldebrid::BASE,
            StoreId::TorBox => torbox::BASE,
            StoreId::Premiumize => return Err(no_code_sign_in()),
        };
        Self::start_at(http, store, base).await
    }

    /// Like [`Login::start`] against another base: Real-Debrid's OAuth base
    /// (`.../oauth/v2`), AllDebrid's and TorBox's API origin.
    pub async fn start_at(
        http: &HttpClient,
        store: StoreId,
        base: &str,
    ) -> Result<Self, DebridError> {
        let (flow, code) = match store {
            StoreId::RealDebrid => {
                let (device, code) = realdebrid::Device::start(http, base).await?;
                (Flow::RealDebrid(device), code)
            }
            StoreId::AllDebrid => {
                let (pin, code) = alldebrid::Pin::start(http, base).await?;
                (Flow::AllDebrid(pin), code)
            }
            StoreId::TorBox => {
                let (device, code) = torbox::Device::start(http, base).await?;
                (Flow::TorBox(device), code)
            }
            StoreId::Premiumize => return Err(no_code_sign_in()),
        };
        Ok(Self {
            deadline: Instant::now() + code.expires_in,
            flow,
            code,
        })
    }

    pub fn code(&self) -> &Code {
        &self.code
    }

    /// One poll; callers wait [`Code::interval`] before each. Past the
    /// lifetime it answers `Expired` without a request.
    pub async fn poll(&self) -> Result<LoginPoll, DebridError> {
        if Instant::now() >= self.deadline {
            return Ok(LoginPoll::Expired);
        }
        let (poll, secrets) = match &self.flow {
            Flow::RealDebrid(device) => (device.poll().await, device.secrets()),
            Flow::AllDebrid(pin) => (pin.poll().await, pin.secrets()),
            Flow::TorBox(device) => (device.poll().await, device.secrets()),
        };
        match poll {
            Err(DebridError::RateLimited { .. }) => Ok(LoginPoll::SlowDown),
            other => other.map_err(|e| e.without(&secrets)),
        }
    }
}

fn no_code_sign_in() -> DebridError {
    DebridError::provider("no_code_sign_in", "this service signs in with a key")
}

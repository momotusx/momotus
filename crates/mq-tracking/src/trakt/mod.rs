//! Trakt (plans/m7-trakt.md). P1: transport, answer classification, the
//! device-code sign-in, token refresh with acknowledged rotation, and
//! pacing per account.

pub mod api;
pub mod auth;

pub use api::{Answer, Api, Gate, Hosts};
pub use auth::{Auth, AuthError, DeviceCode, Poll, Tokens, TraktRecord};

/// The project's Trakt app. `None` until the owner registers it: every
/// Trakt call then answers "not configured". Forks register their own app.
pub const CLIENT_ID: Option<&str> = None;
/// The https redirect URI registered with the app. Sign-in uses the device
/// code and never visits it; refreshes send it and it must match exactly.
pub const REDIRECT_URI: Option<&str> = None;

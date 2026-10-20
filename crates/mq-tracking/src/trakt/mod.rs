//! Trakt (plans/m7-trakt.md). P1: transport, answer classification, the
//! device-code sign-in, token refresh with acknowledged rotation, and
//! pacing per account.

pub mod api;
pub mod auth;
pub mod call;
pub mod search;

pub use api::{Answer, Api, Gate, Hosts};
pub use auth::{Auth, AuthError, Bound, DeviceCode, Poll, Ticket, Tokens, TraktRecord};
pub use call::CallError;

/// The project's Trakt app, given at build time
/// (`MARQUEE_TRAKT_CLIENT_ID=... cargo build`) and never committed. Unset
/// or empty, every Trakt call answers "not configured". Forks register
/// their own app.
pub const CLIENT_ID: Option<&str> = given(option_env!("MARQUEE_TRAKT_CLIENT_ID"));
/// The https redirect URI registered with the app
/// (`MARQUEE_TRAKT_REDIRECT_URI`). Sign-in uses the device code and never
/// visits it; refreshes send it and it must match exactly.
pub const REDIRECT_URI: Option<&str> = given(option_env!("MARQUEE_TRAKT_REDIRECT_URI"));

const fn given(value: Option<&'static str>) -> Option<&'static str> {
    match value {
        Some(v) if !v.is_empty() => Some(v),
        _ => None,
    }
}

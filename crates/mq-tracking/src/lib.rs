//! Watch tracking services. Trakt first (plans/m7-trakt.md); every call
//! leaves the device, and tokens never leave the core's memory except to the
//! app's keystore.

pub mod trakt;

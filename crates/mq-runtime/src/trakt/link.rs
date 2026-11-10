//! Trakt for the whole Momotus account (plans/trakt-account.md): one synced
//! per-profile setting, `trakt.link`, carries the account's Trakt record
//! sealed for members, or a disconnect. A sync round settles it after the
//! pull, for the session profile only, never for kids or Incognito.
//!
//! The refresh follows section 3.4's fallback, which stays correct even if
//! Trakt revokes a whole chain when a spent refresh token comes back: the
//! chain is refreshed only in a sync round, right after its pull, and only
//! by the account's member that sorts first among those active lately. Every
//! other device uses the held access token and waits for the newer link.

use super::{ACCOUNT_SETTING, Conn, TRACKER, kids_now, lock};
use crate::Core;
use mq_library::hlc::now_ms;
use mq_tracking::trakt::TraktRecord;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::Ordering;

/// The synced per-profile setting.
pub(super) const LINK: &str = "trakt.link";
/// `trakt.active.<member>`: when that member last ran this profile's round.
const ACTIVE: &str = "trakt.active.";
/// The device value with every chain this device saw disconnected.
const OFFS: &str = "trakt.off";
const INFO: &[u8] = b"marquee/trakt/link/v1";
const DAY_MS: u64 = 24 * 60 * 60 * 1000;
/// A member that just became active refreshes only after this long, so the
/// others have heard of it first.
const SETTLED_MS: u64 = 10 * 60 * 1000;

/// The link's value. The chain id is no secret: it lets a device refuse a
/// disconnected chain without the account's keys.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Link {
    pub chain: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sealed: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub off: bool,
}

/// A member's activity for the refresher's choice.
#[derive(Serialize, Deserialize)]
struct Beat {
    since: u64,
    at: u64,
}

/// The profile's link, if any.
pub(super) fn read(core: &Core) -> Option<Link> {
    let settings = core.library().settings(core.profile()).ok()?;
    let (_, value) = settings.into_iter().rev().find(|(k, _)| k == LINK)?;
    serde_json::from_str(&value).ok()
}

/// Whether this device is in a Momotus account.
pub(super) fn in_account(core: &Core) -> bool {
    core.library().roster_genesis().is_ok_and(|g| g.is_some())
}

/// The chains this device saw disconnected (never shrinks; a few ids).
pub(super) fn offs(core: &Core) -> HashSet<String> {
    core.device_library()
        .device_value(OFFS)
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default()
}

fn remember_off(core: &Core, chain: &str) {
    let mut all = offs(core);
    if all.insert(chain.to_owned()) {
        let value = serde_json::to_string(&all).unwrap_or_default();
        if let Err(e) = core.device_library().set_device_value(OFFS, &value) {
            tracing::warn!("trakt: disconnect not kept: {e}");
        }
    }
}

fn aad(profile: &str) -> Vec<u8> {
    [
        &(profile.len() as u32).to_be_bytes()[..],
        profile.as_bytes(),
    ]
    .concat()
}

fn write(core: &Core, link: &Link) -> bool {
    let value = serde_json::to_string(link).unwrap_or_default();
    match core
        .library()
        .set_setting(core.profile(), LINK, &value, core.clock())
    {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!("trakt: link not written: {e}");
            false
        }
    }
}

fn write_record(core: &Core, bundle: &mq_sync::roster::Bundle, record: &TraktRecord) -> bool {
    let plain = serde_json::to_vec(record).unwrap_or_default();
    let sealed = mq_sync::roster::seal_for_members(bundle, INFO, &aad(core.profile()), &plain);
    write(
        core,
        &Link {
            chain: record.chain.clone(),
            sealed: Some(sealed),
            off: false,
        },
    )
}

/// A disconnect of `chain`: kept here and written for the account.
pub(super) fn write_off(core: &Core, chain: &str) -> bool {
    remember_off(core, chain);
    write(
        core,
        &Link {
            chain: chain.to_owned(),
            sealed: None,
            off: true,
        },
    )
}

fn open(bundle: &mq_sync::roster::Bundle, profile: &str, link: &Link) -> Option<TraktRecord> {
    let plain =
        mq_sync::roster::open_for_members(bundle, INFO, &aad(profile), link.sealed.as_deref()?)?;
    let record: TraktRecord = serde_json::from_slice(&plain).ok()?;
    (record.chain == link.chain).then_some(record)
}

/// Whether a record the app hands over is refused: its chain was
/// disconnected, or it is the shared chain of an account this device left.
pub(super) fn refused(core: &Core, record: &TraktRecord) -> bool {
    if record.chain.is_empty() {
        return false;
    }
    if offs(core).contains(&record.chain) {
        return true;
    }
    read(core).is_some_and(|l| l.chain == record.chain && (l.off || !in_account(core)))
}

impl Core {
    /// After a sync round, under the sync gate: the session profile adopts,
    /// writes or drops the account's link, the account's refresher refreshes,
    /// and what was written goes out at once.
    pub(crate) async fn trakt_link_round(&self, config: &mq_sync::SyncConfig) {
        let trakt = self.trakt();
        let Some((api, _)) = trakt.api() else { return };
        if self.incognito() || kids_now(self) != Some(false) {
            return;
        }
        let conn = self.trakt_conn();
        conn.auth.set_shared(true).await;
        let bundle = &config.keys;
        let mut wrote = false;
        let held = conn.auth.record().await;
        let held_chain = held.as_ref().map(|h| h.chain.clone());
        match read(self) {
            Some(link) if link.off || offs(self).contains(&link.chain) => {
                if link.off {
                    remember_off(self, &link.chain);
                }
                let fresh = held.filter(|h| {
                    h.chain != link.chain && conn.signed_in_here.load(Ordering::SeqCst)
                });
                if let Some(mine) = fresh {
                    // Connected again here after the disconnect: the account's link again.
                    wrote |= write_record(self, bundle, &mine);
                    conn.signed_in_here.store(false, Ordering::SeqCst);
                } else if !link.off {
                    // A rotation written over the disconnect: it stays one.
                    wrote |= write_off(self, &link.chain);
                }
                if held_chain.as_deref() == Some(link.chain.as_str()) {
                    self.trakt_drop_here(&conn);
                }
            }
            Some(link) => match open(bundle, self.profile(), &link) {
                Some(record) => {
                    if self.trakt_apply(&conn, held, record).await
                        && let Some(mine) = conn.auth.record().await
                    {
                        wrote |= write_record(self, bundle, &mine);
                    }
                }
                None => tracing::warn!("trakt: the account's link did not open"),
            },
            None => {
                if let Some(held) = held.filter(|h| !offs(self).contains(&h.chain)) {
                    wrote |= write_record(self, bundle, &held);
                    conn.signed_in_here.store(false, Ordering::SeqCst);
                }
            }
        }
        if let Some(record) = conn.auth.record().await {
            wrote |= beat(self, config, &record);
            let current = read(self).is_some_and(|l| !l.off && l.chain == record.chain);
            if current
                && refresher(self, config, &record)
                && let Some(rotated) = conn.auth.refresh_now(&api).await
            {
                wrote |= write_record(self, bundle, &rotated);
            }
        }
        self.trakt_refresh(&conn);
        if wrote
            && let Err(e) =
                mq_sync::push_changes(self.library(), self.http(), config, self.clock()).await
        {
            tracing::debug!("trakt: link not pushed yet: {e}");
        }
    }

    /// The link holds `record`; this device holds `held`. Returns whether the
    /// link takes this device's record instead.
    async fn trakt_apply(
        &self,
        conn: &Arc<Conn>,
        held: Option<TraktRecord>,
        record: TraktRecord,
    ) -> bool {
        match held {
            Some(h) if h.chain == record.chain => {
                if !conn.auth.advance(&record).await && h.revision > record.revision {
                    // This device rotated and the account has not heard yet:
                    // the link is the durable copy of the rotation.
                    return true;
                }
                false
            }
            // A sign-in made here wins over the link it found.
            Some(_) if conn.signed_in_here.swap(false, Ordering::SeqCst) => true,
            Some(h)
                if h.account != record.account
                    && !conn.deferred.swap(true, Ordering::SeqCst)
                    && conn.auth.status().await.is_ok()
                    && self
                        .library()
                        .tracker_pending(conn.key(&h.account))
                        .is_ok_and(|(n, _)| n > 0) =>
            {
                // Another Trakt account: one round under the old record
                // first, so its waiting writes can go out.
                false
            }
            _ => {
                conn.deferred.store(false, Ordering::SeqCst);
                let taken = conn.auth.take(record).await;
                if let Err(e) = self.trakt_connected(conn, &taken.account, taken.generation) {
                    tracing::debug!("trakt: the account's sign-in not connected: {e}");
                }
                conn.drop_stored.store(false, Ordering::SeqCst);
                false
            }
        }
    }

    /// The tokens leave this device for the profile, nothing is revoked and
    /// the account's link stays: the account's own pending writes and
    /// cursors go, this device's flag says it is signed out, and the `trakt`
    /// view asks the app to delete its stored record.
    pub(super) fn trakt_drop_here(&self, conn: &Conn) {
        if let Some(login) = lock(&conn.login).take() {
            login.cancel();
        }
        {
            let _fence = lock(&conn.fence);
            let _ = futures_util::FutureExt::now_or_never(conn.auth.set(None));
        }
        let (library, profile) = (self.library(), self.profile());
        if let Err(e) = library
            .set_tracker_account(profile, TRACKER, None, 0, now_ms() as i64)
            .and_then(|()| {
                library.set_setting(profile, &self.trakt_device_setting(), "null", self.clock())
            })
        {
            tracing::warn!("trakt: local drop not recorded: {e}");
        }
        conn.drop_stored.store(true, Ordering::SeqCst);
        self.trakt_refresh(conn);
    }

    /// This device is out of the account (removed, or the account deleted):
    /// the session profile's shared sign-in leaves it, with no revoke. Other
    /// profiles' copies are refused when the app hands them over.
    pub(crate) fn trakt_left_account(&self) {
        let conn = self.trakt_conn();
        let held = futures_util::FutureExt::now_or_never(conn.auth.record()).flatten();
        if held.is_some_and(|h| read(self).is_some_and(|l| l.chain == h.chain)) {
            self.trakt_drop_here(&conn);
        }
        let _ = futures_util::FutureExt::now_or_never(conn.auth.set_shared(false));
    }

    /// A disconnect in an account: the chain is marked off for every device,
    /// and the profile no longer says it uses Trakt.
    pub(super) fn trakt_off_for_account(&self, held: Option<&str>) -> Result<(), crate::CoreError> {
        let chain = held
            .map(str::to_owned)
            .or_else(|| read(self).filter(|l| !l.off).map(|l| l.chain));
        if let Some(chain) = chain.filter(|c| !c.is_empty()) {
            write_off(self, &chain);
        }
        self.library()
            .set_setting(self.profile(), ACCOUNT_SETTING, "null", self.clock())?;
        Ok(())
    }
}

/// Notes this member as active for the profile, at most every quarter of
/// the activity window.
fn beat(core: &Core, config: &mq_sync::SyncConfig, record: &TraktRecord) -> bool {
    let key = format!("{ACTIVE}{}", config.member_hex());
    let now = now_ms();
    let window = window(record);
    let old: Option<Beat> = core
        .library()
        .settings(core.profile())
        .ok()
        .and_then(|all| all.into_iter().rev().find(|(k, _)| *k == key))
        .and_then(|(_, v)| serde_json::from_str(&v).ok());
    if old
        .as_ref()
        .is_some_and(|b| now.saturating_sub(b.at) < window / 4)
    {
        return false;
    }
    let since = old
        .filter(|b| now.saturating_sub(b.at) < window)
        .map_or(now, |b| b.since);
    let value = serde_json::to_string(&Beat { since, at: now }).unwrap_or_default();
    core.library()
        .set_setting(core.profile(), &key, &value, core.clock())
        .is_ok()
}

/// How recent a member's round must be to count as active: a day, or a
/// quarter of the token's lifetime when that is shorter (Trakt documents
/// 24-hour tokens).
fn window(record: &TraktRecord) -> u64 {
    let lifetime = record.expires_at_ms.saturating_sub(record.created_at_ms);
    (lifetime / 4).clamp(4 * SETTLED_MS, DAY_MS)
}

/// Whether this device refreshes the account's chain: the member that sorts
/// first among the account's members active within the window, each settled
/// for ten minutes so the others have heard of it.
fn refresher(core: &Core, config: &mq_sync::SyncConfig, record: &TraktRecord) -> bool {
    let me = config.member_hex();
    let members = mq_sync::stored_devices(core.library()).ok().flatten();
    let now = now_ms();
    let window = window(record);
    let first = core
        .library()
        .settings(core.profile())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(k, v)| {
            let member = k.strip_prefix(ACTIVE)?.to_owned();
            let beat: Beat = serde_json::from_str(&v).ok()?;
            let known = member == me || members.as_ref().is_some_and(|m| m.contains(&member));
            let active = now.saturating_sub(beat.at) < window;
            let settled = now.saturating_sub(beat.since) >= SETTLED_MS;
            (known && active && settled).then_some(member)
        })
        .min();
    first.as_deref() == Some(me.as_str())
}

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

use super::{ACCOUNT_SETTING, Conn, OWNER, SYNC_MEMBER, TRACKER, kids_now, lock};
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
/// The account asked for a rotation: its time (ms), or 0 once made.
const ROTATE: &str = "trakt.rotate";
/// Rounds a link of another Trakt account waits for the old one's writes.
const DEFER_ROUNDS: u32 = 3;
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
                let connected_again = fresh.is_some();
                if let Some(mine) = fresh {
                    // Connected again here after the disconnect: the account's link again.
                    wrote |= write_record(self, bundle, &mine);
                    conn.signed_in_here.store(false, Ordering::SeqCst);
                } else if !link.off {
                    // A rotation written over the disconnect: it stays one.
                    wrote |= write_off(self, &link.chain);
                }
                // Disconnected for the account: no chain held here goes on,
                // whichever it is, but a sign-in made here since.
                if !connected_again && held_chain.is_some() {
                    self.trakt_drop_here(&conn);
                }
            }
            Some(link) => match open(bundle, self.profile(), &link) {
                Some(record) => {
                    conn.unopened.store(false, Ordering::SeqCst);
                    // Only a record that works replaces the account's.
                    if self.trakt_apply(&conn, held, record).await
                        && self.trakt_prove(&conn, &api).await
                        && let Some(mine) = conn.auth.record().await
                    {
                        wrote |= write_record(self, bundle, &mine);
                    }
                }
                None => {
                    // Said as off (Connect shown), never "connecting" forever.
                    conn.unopened.store(true, Ordering::SeqCst);
                    tracing::warn!("trakt: the account's link did not open");
                }
            },
            None => {
                // Only a sign-in that works is published: a stored one may
                // have died while the app was closed, and every device of
                // the account would take it.
                if let Some(held) = held.filter(|h| !offs(self).contains(&h.chain))
                    && (conn.signed_in_here.load(Ordering::SeqCst)
                        || self.trakt_prove(&conn, &api).await)
                {
                    wrote |= write_record(self, bundle, &held);
                    conn.signed_in_here.store(false, Ordering::SeqCst);
                }
            }
        }
        if let Some(record) = conn.auth.record().await {
            wrote |= beat(self, config, &record);
            let current = read(self).is_some_and(|l| !l.off && l.chain == record.chain);
            let asked = rotation_asked(self);
            // ponytail: the refresh runs under `sync_gate`, so a Trakt that
            // does not answer holds sync callers for the HTTP timeout; a
            // shorter timeout here if that ever shows.
            if current
                && refresher(self, config, &record)
                && let Some(rotated) = conn.auth.refresh_now(&api, asked).await
            {
                wrote |= write_record(self, bundle, &rotated);
                // The ask is answered; a later one is a newer write.
                if asked {
                    wrote |= self
                        .library()
                        .set_setting(self.profile(), ROTATE, "0", self.clock())
                        .is_ok();
                }
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
                    && conn.auth.status().await.is_ok()
                    && self
                        .library()
                        .tracker_pending(conn.key(&h.account))
                        .is_ok_and(|(n, _)| n > 0)
                    && conn.deferred.fetch_add(1, Ordering::SeqCst) < DEFER_ROUNDS =>
            {
                // Another Trakt account: the old one stays while its writes
                // wait and go out (they would be dropped with its rows), for
                // a few rounds at most.
                false
            }
            _ => {
                conn.deferred.store(0, Ordering::SeqCst);
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
    /// The account's chain is the link's, whatever this device holds (it may
    /// not have taken the link yet): both are kept off here, the link's is
    /// written.
    pub(super) fn trakt_off_for_account(&self, held: Option<&str>) -> Result<(), crate::CoreError> {
        let linked = read(self).filter(|l| !l.off).map(|l| l.chain);
        if let Some(held) = held.filter(|c| !c.is_empty()) {
            remember_off(self, held);
        }
        if let Some(chain) = linked.or(held.map(str::to_owned)).filter(|c| !c.is_empty()) {
            write_off(self, &chain);
        }
        self.library()
            .set_setting(self.profile(), ACCOUNT_SETTING, "null", self.clock())?;
        Ok(())
    }
}

impl Core {
    /// Asks the account's refresher to rotate the chain at its next round
    /// (a device was removed or reported lost): the copy of the refresh
    /// token that device keeps is then spent.
    pub fn trakt_rotate_soon(&self) {
        let value = now_ms().to_string();
        if let Err(e) = self
            .library()
            .set_setting(self.profile(), ROTATE, &value, self.clock())
        {
            tracing::warn!("trakt: rotation not asked: {e}");
        }
    }

    /// The session leaves this profile: this device stops being its
    /// refresher at once (its beat says it is not active), so the role moves
    /// to another device at their next round.
    pub(crate) fn trakt_leave_profile(&self) {
        let device = format!("{SYNC_MEMBER}{}", self.clock().device());
        let me = self
            .library()
            .settings(OWNER)
            .ok()
            .and_then(|all| {
                all.into_iter()
                    .find_map(|(k, v)| (k == device).then_some(v))
            })
            .and_then(|v| serde_json::from_str::<String>(&v).ok());
        let Some(me) = me else { return };
        let key = format!("{ACTIVE}{me}");
        let beats = self.library().settings(self.profile()).unwrap_or_default();
        let active = beats
            .iter()
            .any(|(k, v)| *k == key && serde_json::from_str::<Beat>(v).is_ok_and(|b| b.at > 0));
        if active {
            let gone = serde_json::to_string(&Beat { since: 0, at: 0 }).unwrap_or_default();
            if let Err(e) = self
                .library()
                .set_setting(self.profile(), &key, &gone, self.clock())
            {
                tracing::warn!("trakt: refresher role not handed on: {e}");
            }
        }
    }

    /// Whether the held record works: it answered or came from Trakt in this
    /// process, or answers a cheap call now.
    async fn trakt_prove(&self, conn: &Conn, api: &mq_tracking::trakt::Api) -> bool {
        if conn.auth.proven().await {
            return true;
        }
        let Some(bound) = conn.auth.bound().await else {
            return false;
        };
        let gate = self.trakt().gate(&bound.account);
        let _ =
            mq_tracking::trakt::call::get(api, &gate, &conn.auth, &bound, "/users/settings").await;
        conn.auth.proven().await
    }
}

/// Whether the account asked for a rotation the refresher has not made yet
/// (the setting holds the ask's time, or 0 once it was made).
fn rotation_asked(core: &Core) -> bool {
    let Ok(settings) = core.library().settings(core.profile()) else {
        return false;
    };
    settings
        .into_iter()
        .rev()
        .find(|(k, _)| k == ROTATE)
        .is_some_and(|(_, v)| v.parse::<u64>().is_ok_and(|at| at > 0))
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
    // Without the roster nobody is known to sort after this device: no refresh.
    let Some(members) = mq_sync::stored_devices(core.library()).ok().flatten() else {
        return false;
    };
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
            let known = member == me || members.contains(&member);
            let active = now.saturating_sub(beat.at) < window;
            let settled = now.saturating_sub(beat.since) >= SETTLED_MS;
            (known && active && settled).then_some(member)
        })
        .min();
    first.as_deref() == Some(me.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mq_sync::roster::{Hex, MemberSecret, Roster};

    /// A link sealed for one profile opens only as that profile, and only
    /// for the account whose keys sealed it.
    #[test]
    fn a_link_opens_only_for_its_profile_and_account() {
        let (_, bundle) = Roster::genesis_entry(&MemberSecret::generate(), Hex([1; 16]), "t", 1);
        let (_, other) = Roster::genesis_entry(&MemberSecret::generate(), Hex([2; 16]), "t", 1);
        let record: TraktRecord = serde_json::from_value(serde_json::json!({
            "accessToken": "a", "refreshToken": "r", "createdAtMs": 1, "expiresAtMs": 2,
            "scope": "public", "account": "u", "revision": 0, "generation": 1, "chain": "c"
        }))
        .unwrap();
        let plain = serde_json::to_vec(&record).unwrap();
        let link = Link {
            chain: "c".into(),
            sealed: Some(mq_sync::roster::seal_for_members(
                &bundle,
                INFO,
                &aad("p"),
                &plain,
            )),
            off: false,
        };
        assert_eq!(open(&bundle, "p", &link), Some(record));
        assert_eq!(open(&bundle, "q", &link), None);
        assert_eq!(open(&other, "p", &link), None);
        let moved = Link {
            chain: "d".into(),
            ..link
        };
        assert_eq!(
            open(&bundle, "p", &moved),
            None,
            "the clear chain is checked"
        );
    }
}

//! Profiles and their PINs (plans/m11-profiles.md): the operations under
//! the permission rules of sections 4 and 9, Argon2id hashing, and the
//! device's one attempt throttle.

use crate::Core;
use crate::views::profiles::{ProfileInfo, ProfilesSnapshot};
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::{Algorithm, Argon2, Params, Version};
use futures_util::StreamExt;
use futures_util::stream::FuturesUnordered;
use mq_library::{AllowedRow, Library, LibraryError, MAX_ALLOWED, OWNER};
use mq_protocol::{ResourcePath, ResourceResponse};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Profiles `createProfile` allows on this device (two offline devices can
/// sync to one more; all are shown).
pub const MAX_PROFILES: usize = 6;
const NAME_CHARS: usize = 30;

/// OWASP's minimum Argon2id setting: 19 MiB, two passes, one lane.
const M_KIB: u32 = 19_456;
const T: u32 = 2;
/// The most a stored hash may ask for; anything beyond fails closed, so a
/// planted record cannot make a check take minutes or gigabytes.
const MAX_M_KIB: u32 = 65_536;
const MAX_T: u32 = 4;

#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("only the owner profile can do that")]
    NotOwner,
    #[error("there are already {MAX_PROFILES} profiles")]
    TooMany,
    #[error("a profile name has 1 to {NAME_CHARS} characters")]
    BadName,
    #[error("an avatar is up to 64 letters, digits, ':', '-' or '_'")]
    BadAvatar,
    #[error("the owner profile cannot be removed")]
    OwnerProfile,
    #[error("a PIN is four digits that are not easy to guess")]
    WeakPin,
    #[error("wrong PIN")]
    WrongPin,
    #[error("too many wrong PINs: wait before trying again")]
    Throttled { retry_after_ms: u64 },
    #[error("no such profile")]
    Unknown,
    #[error("this profile is still arriving from another device")]
    Syncing,
    #[error("the stored PIN cannot be read: clear it, then set a new one")]
    Unreadable,
    /// A kids profile needs the owner's PIN (plans/m11-kids.md 9.3).
    #[error("kids profiles need the owner's PIN")]
    KidsNeedPin,
    #[error("that profile is not a kids profile")]
    NotKids,
    #[error("the title's details came under another id")]
    Mismatch,
    #[error("the title's details could not be read")]
    NoDetails,
    #[error("the list already has {MAX_ALLOWED} titles")]
    ListFull,
    #[error(transparent)]
    Library(#[from] LibraryError),
    #[error("internal error: {0}")]
    Internal(String),
}

impl ProfileError {
    /// The protocol's error code (plans/core-protocol.md).
    pub fn code(&self) -> Option<&'static str> {
        Some(match self {
            Self::NotOwner => "notOwner",
            Self::TooMany => "tooMany",
            Self::BadName => "badName",
            Self::BadAvatar => "badAvatar",
            Self::OwnerProfile => "ownerProfile",
            Self::WeakPin => "weakPin",
            Self::WrongPin => "wrongPin",
            Self::Throttled { .. } => "locked",
            Self::Unknown | Self::Syncing => "unknownProfile",
            // Removed while the request ran.
            Self::Library(LibraryError::ProfileGone(_)) => "unknownProfile",
            Self::Unreadable => "pinUnreadable",
            // The library's own checks, made in the write.
            Self::KidsNeedPin | Self::Library(LibraryError::KidsNeedPin) => "kidsNeedPin",
            Self::NotKids => "notKids",
            Self::Mismatch => "mismatch",
            Self::NoDetails => "noDetails",
            Self::ListFull | Self::Library(LibraryError::ListFull) => "listFull",
            Self::Library(_) | Self::Internal(_) => return None,
        })
    }
}

impl Core {
    /// The profile a session opens as: the one this device last switched
    /// to; with none stored, this handle's (the app's first-run hint). A
    /// stored or hinted profile that is gone, unknown or still syncing opens
    /// the owner; none is ever created.
    pub fn active(&self) -> Core {
        let library = self.library();
        let usable = |id: &str| matches!(library.profile(id), Ok(Some(p)) if p.ready());
        let chosen = match library.active_profile() {
            Ok(Some(stored)) => stored,
            _ => self.profile().to_owned(),
        };
        self.for_profile(if usable(&chosen) { &chosen } else { OWNER })
    }

    /// Remembers this handle's profile as the device's active one.
    pub fn make_active(&self) -> Result<(), ProfileError> {
        Ok(self.library().set_active_profile(self.profile())?)
    }

    /// Whether this handle's profile asks for a PIN (an unreadable hash
    /// counts: it fails closed).
    pub fn has_pin(&self) -> bool {
        // Open only when the profile is known to have none: a read error, an
        // unknown profile or a missing lock register all count as a PIN.
        !matches!(self.library().profile(self.profile()), Ok(Some(p)) if p.lock == Some(None))
    }

    /// Every profile, with this handle's as the active one.
    pub fn profiles_snapshot(&self, locked: bool) -> ProfilesSnapshot {
        let profiles = self
            .library()
            .profiles()
            .unwrap_or_default()
            .into_iter()
            .map(|p| ProfileInfo {
                has_pin: p.pin_hash().is_some(),
                is_owner: p.id == OWNER,
                syncing: !p.ready(),
                kids: p.kids(),
                id: p.id,
                name: p.name,
                avatar: p.avatar,
                position: p.position,
            })
            .collect::<Vec<_>>();
        let kids = profiles.iter().any(|p| p.id == self.profile() && p.kids);
        ProfilesSnapshot {
            profiles,
            active: self.profile().to_owned(),
            locked,
            max: MAX_PROFILES as u32,
            kids,
        }
    }

    /// The owner adds a profile (no PIN, the owner's addons copied once).
    /// Returns its id.
    pub fn create_profile(&self, name: &str, avatar: &str) -> Result<String, ProfileError> {
        self.create(name, avatar, false)
    }

    /// The owner adds a kids profile (plans/m11-kids.md 6.1): no PIN of its
    /// own, no addon copy (it reads the owner's list), an empty list. Only
    /// while the owner has a PIN this build can check: leaving it needs that
    /// PIN. Returns its id.
    pub fn create_kids_profile(&self, name: &str, avatar: &str) -> Result<String, ProfileError> {
        self.create(name, avatar, true)
    }

    fn create(&self, name: &str, avatar: &str, kids: bool) -> Result<String, ProfileError> {
        if self.profile() != OWNER {
            return Err(ProfileError::NotOwner);
        }
        let name = valid_name(name)?;
        valid_avatar(avatar)?;
        if kids {
            self.owner_pin_checkable()?;
        }
        if self.library().profiles()?.len() >= MAX_PROFILES {
            return Err(ProfileError::TooMany);
        }
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).expect("os randomness");
        let id: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        if kids {
            self.library()
                .create_kids_profile(&id, name, avatar, self.clock())?;
        } else {
            self.library()
                .create_profile(&id, name, avatar, self.clock())?;
        }
        Ok(id)
    }

    /// The owner has a PIN, and one this build can check.
    fn owner_pin_checkable(&self) -> Result<(), ProfileError> {
        let owner = self
            .library()
            .profile(OWNER)?
            .ok_or(ProfileError::Unknown)?;
        match owner.lock {
            None => Err(ProfileError::Syncing),
            Some(None) => Err(ProfileError::KidsNeedPin),
            Some(Some(stored)) => parse(&stored)
                .map(drop)
                .map_err(|_| ProfileError::Unreadable),
        }
    }

    /// Whether any kids profile is here.
    fn kids_exist(&self) -> Result<bool, ProfileError> {
        Ok(self.library().profiles()?.iter().any(|p| p.kids()))
    }

    /// A profile edits its own name, avatar and position; the owner edits
    /// any profile's.
    pub fn update_profile(
        &self,
        id: &str,
        name: Option<&str>,
        avatar: Option<&str>,
        position: Option<i64>,
    ) -> Result<(), ProfileError> {
        if id != self.profile() && self.profile() != OWNER {
            return Err(ProfileError::NotOwner);
        }
        let row = self.library().profile(id)?.ok_or(ProfileError::Unknown)?;
        let name = name.map(valid_name).transpose()?.unwrap_or(&row.name);
        avatar.map(valid_avatar).transpose()?;
        self.library().update_profile(
            id,
            name,
            avatar.unwrap_or(&row.avatar),
            position.unwrap_or(row.position),
            self.clock(),
        )?;
        Ok(())
    }

    /// The owner removes another profile for good, on every device.
    pub fn delete_profile(&self, id: &str) -> Result<(), ProfileError> {
        if self.profile() != OWNER {
            return Err(ProfileError::NotOwner);
        }
        if id == OWNER {
            return Err(ProfileError::OwnerProfile);
        }
        self.library().profile(id)?.ok_or(ProfileError::Unknown)?;
        self.library().delete_profile(id, self.clock())?;
        // Its Trakt rounds stop and its tokens leave memory.
        self.trakt().forget(id);
        Ok(())
    }

    /// Sets (`pin`) or clears (`None`) a PIN, by the table in section 9
    /// (N6): a profile sets its own when it has none, changes or clears its
    /// own with `current` (an unreadable one can only be cleared, without
    /// it), and the owner clears another's; nobody sets another's.
    pub async fn set_pin(
        &self,
        id: &str,
        pin: Option<&str>,
        current: Option<&str>,
    ) -> Result<(), ProfileError> {
        self.set_pin_with(id, pin, current, || {}).await
    }

    /// `set_pin`, with `between` run after its checks and before its write
    /// (tests make a kids profile there).
    pub(crate) async fn set_pin_with(
        &self,
        id: &str,
        pin: Option<&str>,
        current: Option<&str>,
        between: impl FnOnce(),
    ) -> Result<(), ProfileError> {
        let row = self.library().profile(id)?.ok_or(ProfileError::Unknown)?;
        if id != self.profile() {
            if self.profile() != OWNER || pin.is_some() {
                return Err(ProfileError::NotOwner);
            }
            self.library().set_pin_hash(id, None, self.clock())?;
            return self.forgive(id);
        }
        // Leaving a kids profile needs the owner's PIN: it stays while one
        // exists (plans/m11-kids.md 6.1).
        if id == OWNER && pin.is_none() && self.kids_exist()? {
            return Err(ProfileError::KidsNeedPin);
        }
        if pin.is_some_and(weak) {
            return Err(ProfileError::WeakPin);
        }
        match row.lock {
            None => return Err(ProfileError::Syncing),
            Some(None) => {}
            Some(Some(stored)) if parse(&stored).is_err() => {
                if pin.is_some() {
                    return Err(ProfileError::Unreadable);
                }
            }
            Some(Some(_)) => self.check_pin(id, current).await?,
        }
        let hash = match pin {
            Some(pin) => {
                let pin = pin.to_owned();
                Some(blocking(move || hash(&pin)).await?)
            }
            None => None,
        };
        between();
        if id == OWNER && hash.is_none() {
            // Checked again in the write: a kids profile created meanwhile
            // keeps the PIN.
            self.library().clear_owner_pin(self.clock())?;
        } else {
            self.library()
                .set_pin_hash(id, hash.as_deref(), self.clock())?;
        }
        self.forgive(id)
    }

    /// Clears the owner's PIN without it, from any session (section 4): the
    /// apps call it only after the device's own lock (an OS prompt), which
    /// the core cannot see, so it has the trust of removing a device.
    pub fn reset_owner_pin(&self) -> Result<(), ProfileError> {
        self.library().set_pin_hash(OWNER, None, self.clock())?;
        self.forgive(OWNER)
    }

    /// A PIN set, changed or cleared by someone allowed to: earlier wrong
    /// tries were against another PIN, so their count and wait go.
    fn forgive(&self, id: &str) -> Result<(), ProfileError> {
        let mut throttle = self.pins().lock().unwrap_or_else(|p| p.into_inner());
        throttle.until.remove(id);
        Ok(self.library().set_pin_failures(id, 0)?)
    }

    /// One PIN attempt for `profile` under the device's throttle. A profile
    /// without a PIN passes; a missing `pin` is wrong without counting.
    pub async fn check_pin(&self, profile: &str, pin: Option<&str>) -> Result<(), ProfileError> {
        let core = self.clone();
        let (profile, pin) = (profile.to_owned(), pin.map(str::to_owned));
        blocking(move || {
            attempt(
                core.library(),
                core.pins(),
                &profile,
                pin.as_deref(),
                Instant::now,
                verify,
            )
        })
        .await?
    }
}

/// The longest `allowTitle` waits for the owner's meta addons, all of them.
const ALLOW_WAIT: Duration = Duration::from_secs(5);
/// The kids exit count and the owner lock register it was counted against.
const KIDS_EXIT: &str = "kids_exit";
/// The exit's wait in the throttle: no profile id has a NUL in it, so the
/// purge of removed profiles' waits never takes it.
const KIDS_EXIT_WAIT: &str = "\u{0}kids_exit";
/// The exit count drops by one for every full hour without a try.
const DECAY: Duration = Duration::from_secs(60 * 60);

/// The kids exit count as stored (plans/m11-kids.md 3.6), never synced.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExitState {
    failures: u32,
    owner_lock_hlc: String,
}

impl Core {
    /// The owner allows `kind` `id` for kids profile `profile` (plans/m11-
    /// kids.md 6.1, 9.2). Stamped as the request arrives, so a Remove sent
    /// during a slow fetch wins. Every owner meta addon that serves the kind
    /// is asked at once; the verified answers that arrive within 5 s give
    /// the names, in addon order. `noDetails` when none answered, `mismatch`
    /// when answers came for another kind or id.
    pub async fn allow_title(
        &self,
        profile: &str,
        kind: &str,
        id: &str,
    ) -> Result<(), ProfileError> {
        let hlc = self.hlc();
        self.kids_target(profile)?;
        let library = self.library();
        let allowed_now =
            allowed_entry(library, profile, id)?.is_some_and(|v| v["allowed"] == true);
        let list = library.allowed_list(profile)?;
        if !allowed_now && list.counted.len() + list.over_count >= MAX_ALLOWED {
            return Err(ProfileError::ListFull);
        }
        let (name, names, poster) = self.title_details(kind, id).await?;
        let row = AllowedRow {
            item_id: id.to_owned(),
            kind: kind.to_owned(),
            name,
            names,
            poster,
            allowed: true,
            added_ms: mq_library::hlc::now_ms() as i64,
        };
        library.write_allowed(profile, &row, &hlc)?;
        Ok(())
    }

    /// The owner takes `id` off the list of kids profile `profile`.
    pub fn remove_title(&self, profile: &str, id: &str) -> Result<(), ProfileError> {
        let hlc = self.hlc();
        self.kids_target(profile)?;
        let library = self.library();
        // Written even when nothing is stored yet: an Allow still fetching
        // (stamped earlier) then lands below it and changes nothing.
        let stored = allowed_entry(library, profile, id)?.unwrap_or_default();
        let text = |field: &str| stored[field].as_str().unwrap_or_default().to_owned();
        let kind = Some(text("kind"))
            .filter(|k| !k.is_empty())
            .unwrap_or_else(|| "unknown".into());
        let row = AllowedRow {
            item_id: id.to_owned(),
            kind,
            name: text("name"),
            names: serde_json::from_value(stored["names"].clone()).unwrap_or_default(),
            poster: stored["poster"].as_str().map(str::to_owned),
            allowed: false,
            added_ms: stored["addedMs"].as_i64().unwrap_or_default(),
        };
        library.write_allowed(profile, &row, &hlc)?;
        Ok(())
    }

    /// The owner makes kids profile `profile` a standard one, with a copy of
    /// the owner's addons. There is no way back. The write reads the state
    /// again: made standard meanwhile (by sync) is `notKids`.
    pub fn make_standard(&self, profile: &str) -> Result<(), ProfileError> {
        self.kids_target(profile)?;
        self.library()
            .make_standard(profile, self.clock())?
            .then_some(())
            .ok_or(ProfileError::NotKids)
    }

    /// A kids profile the owner may edit or view.
    pub(crate) fn kids_target(&self, profile: &str) -> Result<(), ProfileError> {
        if self.profile() != OWNER {
            return Err(ProfileError::NotOwner);
        }
        let row = self
            .library()
            .profile(profile)?
            .ok_or(ProfileError::Unknown)?;
        if !row.ready() {
            return Err(ProfileError::Syncing);
        }
        if !row.kids() {
            return Err(ProfileError::NotKids);
        }
        Ok(())
    }

    /// Name, other names and poster of `kind` `id` from the owner's meta
    /// addons, asked at once.
    async fn title_details(
        &self,
        kind: &str,
        id: &str,
    ) -> Result<(String, Vec<String>, Option<String>), ProfileError> {
        let path = ResourcePath {
            resource: "meta".into(),
            r#type: kind.to_owned(),
            id: id.to_owned(),
            extra: vec![],
        };
        let addons: Vec<_> = self
            .installed_addons()
            .map_err(|e| ProfileError::Internal(e.to_string()))?
            .into_iter()
            .filter(|a| a.supports(&path))
            .collect();
        let deadline = tokio::time::Instant::now() + ALLOW_WAIT;
        let mut asked: FuturesUnordered<_> = addons
            .iter()
            .enumerate()
            .map(|(i, addon)| {
                let path = &path;
                async move { (i, self.addons().fetch(addon, path, ALLOW_WAIT).await) }
            })
            .collect();
        let mut verified: Vec<Option<(String, Option<String>)>> = vec![None; addons.len()];
        let mut answered = false;
        while let Ok(Some((i, fetched))) = tokio::time::timeout_at(deadline, asked.next()).await {
            let meta = fetched.ok().and_then(|f| {
                match serde_json::from_slice::<ResourceResponse>(&f.body) {
                    Ok(ResourceResponse::Meta { meta }) => Some(meta),
                    _ => None,
                }
            });
            let Some(meta) = meta else { continue };
            answered = true;
            let p = meta.preview;
            if p.id == id && p.r#type == kind {
                verified[i] = Some((p.name, p.poster.map(|u| u.to_string())));
            }
        }
        let mut verified = verified.into_iter().flatten();
        let Some((name, poster)) = verified.next() else {
            return Err(if answered {
                ProfileError::Mismatch
            } else {
                ProfileError::NoDetails
            });
        };
        let mut poster = poster;
        let mut names = Vec::new();
        for (other, other_poster) in verified {
            names.push(other);
            poster = poster.or(other_poster);
        }
        Ok((name, names, poster))
    }

    /// One try to leave a kids session with the owner's PIN (plans/m11-
    /// kids.md 7), under the exit's own throttle. With no owner PIN only the
    /// device's own lock (`device_auth`) lets it pass.
    pub async fn kids_exit_attempt(
        &self,
        pin: Option<&str>,
        device_auth: bool,
    ) -> Result<(), ProfileError> {
        let core = self.clone();
        let pin = pin.map(str::to_owned);
        blocking(move || {
            exit_attempt(
                core.library(),
                core.pins(),
                pin.as_deref(),
                device_auth,
                Instant::now,
                verify,
            )
        })
        .await?
    }
}

/// The stored allowlist register of `id`, as its sync value.
fn allowed_entry(
    library: &Library,
    profile: &str,
    id: &str,
) -> Result<Option<serde_json::Value>, ProfileError> {
    let key = format!("{profile}\u{1f}{id}");
    Ok(library
        .register(mq_library::coll::ALLOWED, &key)?
        .map(|r| r.value))
}

/// One exit try against the owner's PIN. Its count lives in `kids_exit`
/// (never the owner's own row) and its wait under its own throttle entry:
/// four free tries, then 1, 5 and 15 minutes, then an hour each, as P1. The
/// count clears when the owner's lock register changes (a new PIN, here or
/// synced), and drops by one for each full hour without a try, on the
/// monotonic clock: a restart starts that hour again.
fn exit_attempt(
    library: &Library,
    throttle: &Mutex<Throttle>,
    pin: Option<&str>,
    device_auth: bool,
    now: impl FnOnce() -> Instant,
    verify: impl FnOnce(&str, &str) -> Result<bool, Unreadable>,
) -> Result<(), ProfileError> {
    let mut throttle = throttle.lock().unwrap_or_else(|p| p.into_inner());
    let now = now();
    let owner = library.profile(OWNER)?.ok_or(ProfileError::Unknown)?;
    let stored = match owner.lock {
        None => return Err(ProfileError::Syncing),
        Some(None) if device_auth => return Ok(()),
        Some(None) => return Err(ProfileError::KidsNeedPin),
        Some(Some(stored)) => stored,
    };
    parse(&stored).map_err(|_| ProfileError::Unreadable)?;
    let lock_hlc = library
        .register(mq_library::coll::PROFILE_LOCK, OWNER)?
        .map(|r| r.hlc)
        .unwrap_or_default();
    let mut state: ExitState = library
        .device_value(KIDS_EXIT)?
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default();
    let save = |state: &ExitState| {
        let value = serde_json::to_string(state).expect("plain struct");
        library.set_device_value(KIDS_EXIT, &value)
    };
    if state.owner_lock_hlc != lock_hlc {
        // A new owner PIN: earlier tries were against another one.
        state = ExitState {
            failures: 0,
            owner_lock_hlc: lock_hlc,
        };
        throttle.until.remove(KIDS_EXIT_WAIT);
        save(&state)?;
    }
    let base = throttle.kids_since.unwrap_or(throttle.started);
    let hours = now.saturating_duration_since(base).as_secs() / DECAY.as_secs();
    if hours > 0 {
        state.failures = state.failures.saturating_sub(hours as u32);
        throttle.kids_since = Some(base + DECAY * hours as u32);
        throttle.until.remove(KIDS_EXIT_WAIT);
        save(&state)?;
    }
    if let Some(wait) = throttle.wait(KIDS_EXIT_WAIT, state.failures, now) {
        return Err(ProfileError::Throttled {
            retry_after_ms: wait.as_nanos().div_ceil(1_000_000) as u64,
        });
    }
    let pin = pin.ok_or(ProfileError::WrongPin)?;
    state.failures += 1;
    save(&state)?;
    throttle.kids_since = Some(now);
    if verify(&stored, pin) == Ok(true) {
        state.failures = 0;
        save(&state)?;
        throttle.until.remove(KIDS_EXIT_WAIT);
        return Ok(());
    }
    throttle
        .until
        .insert(KIDS_EXIT_WAIT.to_owned(), now + penalty(state.failures));
    Err(ProfileError::WrongPin)
}

/// Runs CPU-bound work (a hash takes tens of milliseconds) off the async
/// threads.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, ProfileError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| ProfileError::Internal(e.to_string()))
}

fn valid_avatar(avatar: &str) -> Result<(), ProfileError> {
    if mq_library::valid_avatar(avatar) {
        Ok(())
    } else {
        Err(ProfileError::BadAvatar)
    }
}

fn valid_name(name: &str) -> Result<&str, ProfileError> {
    let name = name.trim();
    match name.chars().count() {
        1..=NAME_CHARS => Ok(name),
        _ => Err(ProfileError::BadName),
    }
}

/// Four digits, refused when trivial: one digit repeated, a run up or
/// down (0123, 9876), or a pair repeated (1212). Anything that is not four
/// digits is refused too.
pub fn weak(pin: &str) -> bool {
    let d: Vec<i8> = pin.bytes().map(|b| b.wrapping_sub(b'0') as i8).collect();
    if d.len() != 4 || d.iter().any(|&x| !(0..=9).contains(&x)) {
        return true;
    }
    let steps: Vec<i8> = d.windows(2).map(|w| w[1] - w[0]).collect();
    let run = |step: i8| steps.iter().all(|&s| s == step);
    run(0) || run(1) || run(-1) || (d[0] == d[2] && d[1] == d[3])
}

/// A PHC string for `pin`: Argon2id, 16 random salt bytes.
fn hash(pin: &str) -> String {
    let params = Params::new(M_KIB, T, 1, None).expect("fixed valid parameters");
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).expect("os randomness");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_with_salt(pin.as_bytes(), &salt)
        .expect("a four digit PIN and a 16 byte salt hash")
        .to_string()
}

/// The stored hash cannot be checked: malformed, another algorithm or
/// version, or parameters past the bounds. It fails closed.
#[derive(Debug, PartialEq, Eq)]
pub struct Unreadable;

/// The stored PHC string, when it is one this build checks: Argon2id
/// version 19 with bounded parameters, a salt and a hash.
fn parse(stored: &str) -> Result<PasswordHash, Unreadable> {
    let parsed = PasswordHash::new(stored).map_err(|_| Unreadable)?;
    let params = Params::try_from(&parsed).map_err(|_| Unreadable)?;
    let bounded = parsed.algorithm == Algorithm::Argon2id.ident()
        && parsed.version == Some(Version::V0x13.into())
        && params.m_cost() <= MAX_M_KIB
        && params.t_cost() <= MAX_T
        && params.p_cost() == 1
        && parsed.salt.is_some()
        && parsed.hash.is_some();
    bounded.then_some(parsed).ok_or(Unreadable)
}

/// Whether `pin` matches the PHC string (the output compares in constant
/// time).
pub fn verify(stored: &str, pin: &str) -> Result<bool, Unreadable> {
    let parsed = parse(stored)?;
    match Argon2::default().verify_password(pin.as_bytes(), &parsed) {
        Ok(()) => Ok(true),
        Err(argon2::password_hash::Error::PasswordInvalid) => Ok(false),
        Err(_) => Err(Unreadable),
    }
}

/// The wait before the next try after `failures` wrong PINs in a row: four
/// free tries, then 1, 5 and 15 minutes, then an hour each.
fn penalty(failures: u32) -> Duration {
    Duration::from_secs(match failures {
        0..=3 => 0,
        4 => 60,
        5 => 5 * 60,
        6 => 15 * 60,
        _ => 60 * 60,
    })
}

/// The device's PIN throttle, shared by every session and profile handle
/// (one per `Core::open`). Failure counts are stored; waits run on the
/// monotonic clock. A restart begins the current wait again from the start,
/// so changing the date neither shortens nor stretches it.
pub(crate) struct Throttle {
    started: Instant,
    until: HashMap<String, Instant>,
    /// Where the kids exit's current decay hour began: its last try, or
    /// this process's start.
    kids_since: Option<Instant>,
}

impl Throttle {
    pub(crate) fn new(started: Instant) -> Self {
        Self {
            started,
            until: HashMap::new(),
            kids_since: None,
        }
    }

    fn wait(&self, profile: &str, failures: u32, now: Instant) -> Option<Duration> {
        let until = self
            .until
            .get(profile)
            .copied()
            .unwrap_or(self.started + penalty(failures));
        until.checked_duration_since(now).filter(|d| !d.is_zero())
    }
}

/// One attempt, serialized with every other on the device. The failure is
/// counted before the compare (a crash mid-check still counts it) and
/// cleared only by the right PIN.
fn attempt(
    library: &Library,
    throttle: &Mutex<Throttle>,
    profile: &str,
    pin: Option<&str>,
    now: impl FnOnce() -> Instant,
    verify: impl FnOnce(&str, &str) -> Result<bool, Unreadable>,
) -> Result<(), ProfileError> {
    let mut throttle = throttle.lock().unwrap_or_else(|p| p.into_inner());
    let now = now();
    let live = library.profiles()?;
    // Waits of profiles removed since (here or by sync) go with them; the
    // kids exit's is no profile's.
    throttle
        .until
        .retain(|id, _| id == KIDS_EXIT_WAIT || live.iter().any(|p| p.id == *id));
    let row = live
        .into_iter()
        .find(|p| p.id == profile)
        .ok_or(ProfileError::Unknown)?;
    let stored = match row.lock {
        None => return Err(ProfileError::Syncing),
        Some(None) => return Ok(()),
        Some(Some(stored)) => stored,
    };
    // No PIN can match a hash this build cannot check: say so, and spend no
    // try on it (it would only lengthen the waits).
    parse(&stored).map_err(|_| ProfileError::Unreadable)?;
    let failures = library.pin_failures(profile)?;
    if let Some(wait) = throttle.wait(profile, failures, now) {
        return Err(ProfileError::Throttled {
            retry_after_ms: wait.as_nanos().div_ceil(1_000_000) as u64,
        });
    }
    let pin = pin.ok_or(ProfileError::WrongPin)?;
    library.set_pin_failures(profile, failures + 1)?;
    if verify(&stored, pin) == Ok(true) {
        library.set_pin_failures(profile, 0)?;
        throttle.until.remove(profile);
        return Ok(());
    }
    throttle
        .until
        .insert(profile.to_owned(), now + penalty(failures + 1));
    Err(ProfileError::WrongPin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mq_library::Hlc;

    #[test]
    fn trivial_pins_are_refused() {
        for pin in [
            "0000", "1111", "9999", "0123", "1234", "6789", "9876", "3210", "1212", "0909", "123",
            "12345", "12a4", "", "١٢٣٤",
        ] {
            assert!(weak(pin), "{pin} should be refused");
        }
        for pin in ["1357", "2580", "0192", "7391", "1122", "8890"] {
            assert!(!weak(pin), "{pin} should be accepted");
        }
    }

    #[test]
    fn a_hash_checks_its_pin_and_an_unreadable_one_fails_closed() {
        let stored = hash("1357");
        assert!(
            stored.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"),
            "{stored}"
        );
        assert_eq!(verify(&stored, "1357"), Ok(true));
        assert_eq!(verify(&stored, "1358"), Ok(false));
        assert_ne!(hash("1357"), stored, "a fresh salt each time");
        let swap = |from: &str, to: &str| stored.replacen(from, to, 1);
        let without_hash = stored[..stored.rfind('$').unwrap()].to_owned();
        for bad in [
            String::new(),
            "1357".into(),
            "$argon2id$".into(),
            swap("m=19456", "m=1048576"),
            swap("t=2", "t=5"),
            swap("p=1", "p=2"),
            swap("argon2id", "argon2i"),
            swap("v=19", "v=16"),
            without_hash,
        ] {
            assert_eq!(verify(&bad, "1357"), Err(Unreadable), "{bad}");
        }
    }

    const PIN: &str = "1357";

    fn locked_profile() -> Library {
        let lib = Library::temporary().unwrap();
        let hlc = |wall| {
            Hlc {
                wall_ms: wall,
                counter: 0,
                device: 1,
            }
            .encode()
        };
        lib.create_profile("x", "x", "", &hlc(1)).unwrap();
        lib.set_pin_hash("x", Some(&hash(PIN)), &hlc(2)).unwrap();
        lib
    }

    /// One try at `at`, with a compare that knows the PIN.
    fn try_at(
        lib: &Library,
        throttle: &Mutex<Throttle>,
        at: Instant,
        pin: &str,
    ) -> Result<(), ProfileError> {
        attempt(
            lib,
            throttle,
            "x",
            Some(pin),
            || at,
            |_, pin| Ok(pin == PIN),
        )
    }

    fn wait_ms(result: Result<(), ProfileError>) -> u64 {
        match result {
            Err(ProfileError::Throttled { retry_after_ms }) => retry_after_ms,
            other => panic!("expected a wait, got {other:?}"),
        }
    }

    fn wrong(result: Result<(), ProfileError>) {
        assert!(matches!(result, Err(ProfileError::WrongPin)), "{result:?}");
    }

    #[test]
    fn four_free_tries_then_one_five_fifteen_minutes_then_an_hour_each() {
        let lib = locked_profile();
        let t0 = Instant::now();
        let throttle = Mutex::new(Throttle::new(t0));
        let s = Duration::from_secs;
        for _ in 0..4 {
            wrong(try_at(&lib, &throttle, t0, "0000"));
        }
        // Even the right PIN waits.
        assert_eq!(wait_ms(try_at(&lib, &throttle, t0, PIN)), 60_000);
        assert_eq!(wait_ms(try_at(&lib, &throttle, t0 + s(59), PIN)), 1_000);
        let mut at = t0 + s(60);
        for penalty in [5 * 60, 15 * 60, 60 * 60, 60 * 60] {
            wrong(try_at(&lib, &throttle, at, "0000"));
            assert_eq!(
                wait_ms(try_at(&lib, &throttle, at, PIN)),
                penalty * 1000,
                "after {} failures",
                lib.pin_failures("x").unwrap()
            );
            at += s(penalty);
        }
        assert_eq!(lib.pin_failures("x").unwrap(), 8);
        // The right PIN clears it all: four free tries again.
        try_at(&lib, &throttle, at, PIN).unwrap();
        assert_eq!(lib.pin_failures("x").unwrap(), 0);
        for _ in 0..4 {
            wrong(try_at(&lib, &throttle, at, "0000"));
        }
        wait_ms(try_at(&lib, &throttle, at, PIN));
    }

    #[test]
    fn a_restart_runs_the_current_wait_again_in_full() {
        let lib = locked_profile();
        let t0 = Instant::now();
        let throttle = Mutex::new(Throttle::new(t0));
        let s = Duration::from_secs;
        for _ in 0..4 {
            wrong(try_at(&lib, &throttle, t0, "0000"));
        }
        wrong(try_at(&lib, &throttle, t0 + s(60), "0000"));
        wrong(try_at(&lib, &throttle, t0 + s(360), "0000"));
        // The app restarts much later, whatever the date says: the count is
        // stored, and the wait for six failures starts again from the start.
        let t1 = t0 + s(86_400);
        let restarted = Mutex::new(Throttle::new(t1));
        assert_eq!(wait_ms(try_at(&lib, &restarted, t1, PIN)), 15 * 60_000);
        assert_eq!(
            wait_ms(try_at(&lib, &restarted, t1 + s(15 * 60 - 1), PIN)),
            1_000
        );
        try_at(&lib, &restarted, t1 + s(15 * 60), PIN).unwrap();
    }

    #[test]
    fn a_failure_is_counted_before_the_compare() {
        let lib = locked_profile();
        let t0 = Instant::now();
        let throttle = Mutex::new(Throttle::new(t0));
        let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            attempt(
                &lib,
                &throttle,
                "x",
                Some(PIN),
                || t0,
                |_, _| panic!("power cut"),
            )
        }));
        assert!(crashed.is_err());
        assert_eq!(lib.pin_failures("x").unwrap(), 1);
    }

    #[test]
    fn no_pin_unknown_and_syncing_profiles() {
        let lib = locked_profile();
        let t0 = Instant::now();
        let throttle = Mutex::new(Throttle::new(t0));
        let never = |_: &str, _: &str| -> Result<bool, Unreadable> { panic!("no compare") };
        // A missing PIN is wrong without spending a try.
        wrong(attempt(&lib, &throttle, "x", None, || t0, never));
        assert_eq!(lib.pin_failures("x").unwrap(), 0);
        let hlc = Hlc::zero(1).encode();
        lib.create_profile("open", "open", "", &hlc).unwrap();
        attempt(&lib, &throttle, "open", None, || t0, never).unwrap();
        let unknown = attempt(&lib, &throttle, "nobody", Some(PIN), || t0, never);
        assert!(matches!(unknown, Err(ProfileError::Unknown)));
        // A profile row whose lock register has not arrived.
        let synced = mq_library::SyncRecord {
            coll: mq_library::coll::PROFILES,
            key: "new".into(),
            value: serde_json::json!({"profile": "new", "name": "new"}),
            hlc,
        };
        lib.apply(&[synced]).unwrap();
        let syncing = attempt(&lib, &throttle, "new", Some(PIN), || t0, never);
        assert!(matches!(syncing, Err(ProfileError::Syncing)));
        // An unreadable hash takes no PIN at all, says so and costs no try.
        let later = Hlc {
            wall_ms: 10,
            counter: 0,
            device: 1,
        };
        lib.set_pin_hash("x", Some("$argon2id$garbage"), &later.encode())
            .unwrap();
        for _ in 0..6 {
            let unreadable = attempt(&lib, &throttle, "x", Some(PIN), || t0, never);
            let code = unreadable.unwrap_err().code();
            assert_eq!(code, Some("pinUnreadable"));
        }
        assert_eq!(lib.pin_failures("x").unwrap(), 0);
    }

    #[test]
    fn the_throttle_forgets_removed_profiles() {
        let lib = locked_profile();
        let hlc = |wall| {
            Hlc {
                wall_ms: wall,
                counter: 0,
                device: 1,
            }
            .encode()
        };
        lib.create_profile("y", "y", "", &hlc(3)).unwrap();
        lib.set_pin_hash("y", Some(&hash(PIN)), &hlc(4)).unwrap();
        let t0 = Instant::now();
        let throttle = Mutex::new(Throttle::new(t0));
        wrong(try_at(&lib, &throttle, t0, "0000"));
        assert!(throttle.lock().unwrap().until.contains_key("x"));
        lib.delete_profile("x", &hlc(5)).unwrap();
        let other = attempt(&lib, &throttle, "y", Some("0000"), || t0, |_, _| Ok(false));
        wrong(other);
        let kept: Vec<String> = throttle.lock().unwrap().until.keys().cloned().collect();
        assert_eq!(kept, ["y"]);
    }

    #[test]
    fn an_attempt_on_a_profile_removed_mid_check_reports_it_unknown() {
        let lib = locked_profile();
        let t0 = Instant::now();
        let throttle = Mutex::new(Throttle::new(t0));
        // The deletion lands while the hash is compared.
        let result = attempt(
            &lib,
            &throttle,
            "x",
            Some(PIN),
            || t0,
            |_, _| {
                let hlc = Hlc {
                    wall_ms: 9,
                    counter: 0,
                    device: 1,
                };
                lib.delete_profile("x", &hlc.encode()).unwrap();
                Ok(true)
            },
        );
        let err = result.unwrap_err();
        assert_eq!(err.code(), Some("unknownProfile"), "{err:?}");
        assert_eq!(lib.pin_failures("x").unwrap(), 0, "no row came back");
    }

    // The kids exit (plans/m11-kids.md 7).

    fn at(wall: u64) -> String {
        Hlc {
            wall_ms: wall,
            counter: 0,
            device: 1,
        }
        .encode()
    }

    /// The owner with PIN `PIN`, and a kids profile to leave.
    fn kids_device() -> Library {
        let lib = Library::temporary().unwrap();
        lib.ensure_profile(OWNER, "Main", &Hlc::zero(1).encode())
            .unwrap();
        lib.set_pin_hash(OWNER, Some(&hash(PIN)), &at(2)).unwrap();
        lib.create_kids_profile("kid", "Kid", "", &at(3)).unwrap();
        lib
    }

    fn exit_at(
        lib: &Library,
        throttle: &Mutex<Throttle>,
        now: Instant,
        pin: &str,
    ) -> Result<(), ProfileError> {
        exit_attempt(
            lib,
            throttle,
            Some(pin),
            false,
            || now,
            |_, pin| Ok(pin == PIN),
        )
    }

    fn exit_failures(lib: &Library) -> u32 {
        let value = lib.device_value(KIDS_EXIT).unwrap().unwrap_or_default();
        serde_json::from_str::<ExitState>(&value)
            .unwrap_or_default()
            .failures
    }

    #[test]
    fn the_kids_exit_waits_as_p1_on_its_own_count() {
        let lib = kids_device();
        let t0 = Instant::now();
        let throttle = Mutex::new(Throttle::new(t0));
        let s = Duration::from_secs;
        for _ in 0..4 {
            wrong(exit_at(&lib, &throttle, t0, "0000"));
        }
        assert_eq!(wait_ms(exit_at(&lib, &throttle, t0, PIN)), 60_000);
        let mut at = t0 + s(60);
        for penalty in [5 * 60, 15 * 60, 60 * 60] {
            wrong(exit_at(&lib, &throttle, at, "0000"));
            assert_eq!(wait_ms(exit_at(&lib, &throttle, at, PIN)), penalty * 1000);
            at += s(penalty);
        }
        assert_eq!(exit_failures(&lib), 7);
        // Never on the owner's own row: the parent still unlocks Main here.
        assert_eq!(lib.pin_failures(OWNER).unwrap(), 0);
        // A P1 attempt purges waits of removed profiles, never the exit's.
        let _ = attempt(&lib, &throttle, OWNER, None, || at, |_, _| Ok(false));
        assert!(throttle.lock().unwrap().until.contains_key(KIDS_EXIT_WAIT));
        // The right PIN clears it.
        exit_at(&lib, &throttle, at, PIN).unwrap();
        assert_eq!(exit_failures(&lib), 0);
    }

    #[test]
    fn a_new_owner_pin_clears_the_kids_exit() {
        let lib = kids_device();
        let t0 = Instant::now();
        let throttle = Mutex::new(Throttle::new(t0));
        for _ in 0..4 {
            wrong(exit_at(&lib, &throttle, t0, "0000"));
        }
        wait_ms(exit_at(&lib, &throttle, t0, PIN));
        // The owner sets a new PIN on another device; it syncs here.
        let lock = mq_library::SyncRecord {
            coll: mq_library::coll::PROFILE_LOCK,
            key: OWNER.into(),
            value: serde_json::json!({"profile": OWNER, "pinHash": hash(PIN)}),
            hlc: at(50),
        };
        lib.apply(&[lock]).unwrap();
        exit_at(&lib, &throttle, t0, PIN).unwrap();
        assert_eq!(exit_failures(&lib), 0);
    }

    #[test]
    fn the_kids_exit_count_drops_one_for_each_hour_without_a_try() {
        let lib = kids_device();
        let t0 = Instant::now();
        let throttle = Mutex::new(Throttle::new(t0));
        let s = Duration::from_secs;
        let mut last = t0;
        for (i, wait) in [0, 0, 0, 0, 60, 5 * 60, 15 * 60].into_iter().enumerate() {
            last += s(wait);
            wrong(exit_at(&lib, &throttle, last, "0000"));
            assert_eq!(exit_failures(&lib), i as u32 + 1);
        }
        // Three hours on the monotonic clock without a try: three fewer, so
        // the next wrong one is the fifth (5 minutes), not the eighth.
        let later = last + s(3 * 60 * 60);
        wrong(exit_at(&lib, &throttle, later, "0000"));
        assert_eq!(exit_failures(&lib), 5);
        assert_eq!(wait_ms(exit_at(&lib, &throttle, later, PIN)), 5 * 60_000);
    }

    #[test]
    fn a_restart_starts_the_decay_hour_again() {
        let lib = kids_device();
        let t0 = Instant::now();
        let throttle = Mutex::new(Throttle::new(t0));
        let s = Duration::from_secs;
        let mut last = t0;
        for wait in [0, 0, 0, 0, 60, 5 * 60, 15 * 60] {
            last += s(wait);
            wrong(exit_at(&lib, &throttle, last, "0000"));
        }
        // The app restarts at t1, whatever the date says: the hour starts
        // there, so 59 minutes later nothing has dropped yet.
        let t1 = last + s(30);
        let restarted = Mutex::new(Throttle::new(t1));
        assert_eq!(
            wait_ms(exit_at(&lib, &restarted, t1 + s(59 * 60), PIN)),
            60_000,
            "still seven: the hour's wait from the restart"
        );
        assert_eq!(exit_failures(&lib), 7);
        exit_at(&lib, &restarted, t1 + s(61 * 60), PIN).unwrap();
    }

    #[test]
    fn without_an_owner_pin_only_the_device_lock_lets_a_kid_out() {
        let lib = Library::temporary().unwrap();
        lib.ensure_profile(OWNER, "Main", &Hlc::zero(1).encode())
            .unwrap();
        let throttle = Mutex::new(Throttle::new(Instant::now()));
        let never = |_: &str, _: &str| -> Result<bool, Unreadable> { panic!("no compare") };
        let refused = exit_attempt(&lib, &throttle, Some(PIN), false, Instant::now, never);
        assert_eq!(refused.unwrap_err().code(), Some("kidsNeedPin"));
        exit_attempt(&lib, &throttle, None, true, Instant::now, never).unwrap();
    }

    /// The owner's PIN removal passed its own check (no kids then); a kids
    /// profile is made before its write: the removal is refused and the
    /// PIN stays.
    #[tokio::test]
    async fn a_kids_profile_made_while_the_owner_pin_goes_keeps_it() {
        let core = Core::temporary(OWNER).unwrap();
        core.set_pin(OWNER, Some(PIN), None).await.unwrap();
        let mut made = None;
        let removed = core
            .set_pin_with(OWNER, None, Some(PIN), || {
                made = Some(core.create_kids_profile("Kid", "fox"));
            })
            .await;
        assert!(made.unwrap().is_ok(), "the PIN was there when it was made");
        assert_eq!(removed.unwrap_err().code(), Some("kidsNeedPin"));
        assert!(core.has_pin());
    }
}

//! Adding a device and recovering an account, with no password and nothing
//! the relay can read or forge (plan: "Phase 2" and "Phase 3").
//!
//! Pairing: the new device makes its member keys and shows a QR with a relay
//! slot id, its public keys, its name and form. A device of the account first
//! asks the relay whether the slot was opened from its own network; if not,
//! only a live scan in its own scanner, behind the screen lock, approves
//! (plans/tv-qr-login.md Q2). Only then it seals the current key bundle, the
//! roster head and a sponsor statement for exactly those keys to the new
//! device's box key with HPKE (RFC 9180). The new device opens it, claims a
//! token that can only join, checks the roster, and signs its own `addDevice`.
//!
//! Recovery: a 160-bit kit derives a lookup id, a redeem secret, a tag key
//! and member keys. Making a kit appends it to the roster with a box of the
//! current bundle; later rotations box new keys to it like to any member.
//! Redeeming needs the redeem secret, which no device keeps; the recovering
//! device opens the kit's newest box, checks the id tag, and joins vouched
//! for by the kit.

use crate::engine::{
    APPROVAL, CANCEL, PAIRING_MS, Session, SyncConfig, SyncError, fetch_roster, options, rebuild,
    relay_info, require_secure,
};
use crate::roster::{
    Bundle, FORMS, Hex, Id, KitKind, MAX_KITS, MemberKeys, MemberSecret, NewKit, Role, Sponsor,
    open_form, open_name, seal_form, seal_name,
};
use crate::wire::{AccountResponse, AppendRequest, Redeemed, RosterHead};
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD as B64, URL_SAFE_NO_PAD as B64URL};
use hkdf::Hkdf;
use hpke::aead::ChaCha20Poly1305;
use hpke::kdf::HkdfSha256;
use hpke::kem::X25519HkdfSha256;
use hpke::{Deserializable, OpModeR, OpModeS, Serializable};
use mq_http::{HttpClient, HttpError};
use mq_library::Library;
use mq_library::hlc::{HlcClock, now_ms};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(crate) type Kem = X25519HkdfSha256;
pub(crate) type PrivateKey = <Kem as hpke::Kem>::PrivateKey;
pub(crate) type PublicKey = <Kem as hpke::Kem>::PublicKey;
/// v3: the payload lost its six digits, so an old device shown a new phone's
/// payload fails at once instead of waiting for digits (Q10).
const PAIR_INFO: &[u8] = b"marquee/pair/v3";
/// A phone built before v3 still signs a new device in (the rename's
/// migration, plans/rename-momotus.md 2): its payload carries six digits the
/// new device ignores, since that phone checked the screen its old way.
const PAIR_INFO_V2: &[u8] = b"marquee/pair/v2";
/// X25519 encapsulated key length, in front of the HPKE ciphertext.
const ENC: usize = 32;

fn bad(why: &str) -> SyncError {
    SyncError::Pairing(why.to_owned())
}

pub(crate) fn seal_to(
    public: &PublicKey,
    info: &[u8],
    aad: &[u8],
    plain: &[u8],
) -> Result<Vec<u8>, SyncError> {
    let (enc, mut context) =
        hpke::setup_sender::<ChaCha20Poly1305, HkdfSha256, Kem>(&OpModeS::Base, public, info)
            .map_err(|_| bad("the key is not usable"))?;
    let ciphertext = context
        .seal(plain, aad)
        .map_err(|_| bad("sealing failed"))?;
    let mut blob = enc.to_bytes().to_vec();
    blob.extend_from_slice(&ciphertext);
    Ok(blob)
}

pub(crate) fn open_with(
    secret: &PrivateKey,
    info: &[u8],
    aad: &[u8],
    blob: &[u8],
) -> Result<Vec<u8>, SyncError> {
    if blob.len() < ENC {
        return Err(bad("sealed data is too short"));
    }
    let (enc, ciphertext) = blob.split_at(ENC);
    let enc =
        <Kem as hpke::Kem>::EncappedKey::from_bytes(enc).map_err(|_| bad("bad sealed key"))?;
    let mut context = hpke::setup_receiver::<ChaCha20Poly1305, HkdfSha256, Kem>(
        &OpModeR::Base,
        secret,
        &enc,
        info,
    )
    .map_err(|_| bad("sealed data does not open"))?;
    context
        .open(ciphertext, aad)
        .map_err(|_| bad("sealed data does not open"))
}

/// Sealed inside a pairing blob, to the new device's box key.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PairPayload {
    server: String,
    relay_id: Id,
    keys: Bundle,
    head: RosterHead,
    sponsor: Sponsor,
    /// A v2 payload's six digits: read, never shown or sent.
    #[serde(default, skip_serializing)]
    code: Option<String>,
}

/// A pairing payload sealed to `secret` for `slot`: a v3 phone's, or a v2 one's (Q10).
fn open_payload(secret: &PrivateKey, slot: &str, blob: &[u8]) -> Result<PairPayload, SyncError> {
    let open = |info| open_with(secret, info, slot.as_bytes(), blob);
    let plain = open(PAIR_INFO).or_else(|_| open(PAIR_INFO_V2))?;
    serde_json::from_slice(&plain).map_err(|_| bad("pairing reply is malformed"))
}

/// A pairing a device of the account approved: `claim` it.
#[derive(Clone, PartialEq, Eq)]
pub struct Offer {
    payload: PairPayload,
}

impl std::fmt::Debug for Offer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Offer")
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Slot {
    pairing: String,
    take: String,
    expires_ms: u64,
    /// How long the slot lives, so the new device counts it on its own
    /// monotonic clock, not a wall clock that may be wrong (Q5).
    #[serde(default)]
    ttl_ms: Option<u64>,
    /// The relay's link host: the QR is then a link a phone camera opens.
    #[serde(default)]
    link_host: Option<String>,
}

/// A pairing in progress on the new device.
pub struct PairingRequest {
    /// Text for the QR code.
    pub qr: String,
    /// When the slot ends, by the relay's clock.
    pub expires_ms: u64,
    /// How long it lives, counted from `sent`.
    pub ttl: std::time::Duration,
    /// When this device asked for the slot: the relay's life for it starts
    /// no earlier, so a view counted from here never outlives it.
    pub sent: std::time::Instant,
    server: String,
    pairing: String,
    take: String,
    member: MemberSecret,
}

/// Opens a pairing slot on the relay, makes this device's member keys and
/// returns the QR to show (with no name or form in it).
pub async fn start_pairing(http: &HttpClient, server: &str) -> Result<PairingRequest, SyncError> {
    start_pairing_as(http, server, "", None).await
}

/// As `start_pairing`, the QR naming this device and its form, which the
/// approving phone shows (Q4).
pub async fn start_pairing_as(
    http: &HttpClient,
    server: &str,
    name: &str,
    form: Option<&str>,
) -> Result<PairingRequest, SyncError> {
    require_secure(server)?;
    let form = checked_form(form)?;
    let sent = std::time::Instant::now();
    let slot: Slot = http
        .post_json(
            &format!("{server}/v1/pairings"),
            &serde_json::json!({}),
            &options(server, ""),
        )
        .await?
        .json()?;
    let member = MemberSecret::generate();
    let name = clean_name(name);
    let shown = Shown {
        name: name.as_deref(),
        form,
    };
    // The relay's word for its link host goes into a link: a host name, or none (Q8).
    let link_host = slot.link_host.as_deref().filter(|h| bare_host(h));
    let qr = pairing_code(link_host, server, &slot.pairing, member.public(), shown);
    // A relay from before `ttlMs`: its own end against this clock, at best.
    let ttl = slot
        .ttl_ms
        .unwrap_or_else(|| slot.expires_ms.saturating_sub(now_ms()));
    Ok(PairingRequest {
        qr,
        expires_ms: slot.expires_ms,
        ttl: std::time::Duration::from_millis(ttl),
        sent,
        server: server.to_owned(),
        pairing: slot.pairing,
        take: slot.take,
        member,
    })
}

/// A lowercase domain name with a dot and no scheme, port, path, user or
/// empty label: never an IP literal or a one-label name such as `localhost`,
/// which would point a phone's link into its own network.
fn bare_host(host: &str) -> bool {
    let parsed = url::Url::parse(&format!("https://{host}/")).ok();
    host.contains('.')
        && host.split('.').all(|label| !label.is_empty())
        && parsed.is_some_and(|u| {
            u.domain() == Some(host)
                && u.port().is_none()
                && u.path() == "/"
                && u.username().is_empty()
                && u.query().is_none()
        })
}

/// Longest device name a QR carries, in characters.
const QR_NAME_CHARS: usize = 40;

/// A device name as a phone may show it: no control, bidi or zero-width
/// characters (a screen must not pass off a name like "Momotus (verified)"
/// written right to left), whitespace collapsed, at most `QR_NAME_CHARS`
/// characters; none when nothing is left (Q4).
fn clean_name(name: &str) -> Option<String> {
    let hidden = |c: char| {
        c.is_control()
            || matches!(
                c,
                '\u{00AD}'
                    | '\u{034F}'
                    | '\u{061C}'
                    | '\u{115F}'
                    | '\u{1160}'
                    | '\u{180E}'
                    | '\u{200B}'..='\u{200F}'
                    | '\u{202A}'..='\u{202E}'
                    | '\u{2060}'..='\u{2069}'
                    | '\u{2800}'
                    | '\u{3164}'
                    | '\u{FE00}'..='\u{FE0F}'
                    | '\u{FEFF}'
                    | '\u{FFA0}'
                    | '\u{E0000}'..='\u{E007F}'
                    | '\u{E0100}'..='\u{E01EF}'
            )
    };
    let kept: String = name
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|c| !hidden(*c))
        .collect();
    let words: Vec<&str> = kept.split_whitespace().collect();
    let joined: String = words.join(" ").chars().take(QR_NAME_CHARS).collect();
    Some(joined.trim_end().to_owned()).filter(|n| !n.is_empty())
}

/// What the QR says about the device that shows it, for the approving
/// phone's question only: the device's own claim, never a check (Q4).
#[derive(Clone, Copy, Default)]
struct Shown<'a> {
    name: Option<&'a str>,
    form: Option<&'a str>,
}

/// What a scanned or opened pairing code says, for the approving phone's
/// sheet: its sync server, and the name and form the screen gives itself,
/// cleaned (`readPairingCode`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PairingCode {
    pub server: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub form: Option<String>,
}

/// Reads a pairing code for the sheet: refuses anything that is not one.
pub fn read_pairing_code(qr: &str) -> Result<PairingCode, SyncError> {
    let fields = code_fields(qr)?;
    Ok(PairingCode {
        server: fields.get("s").ok_or_else(|| bad("not a pairing code"))?,
        name: fields.get("n").as_deref().and_then(clean_name),
        form: fields.get("f").filter(|f| FORMS.contains(&f.as_str())),
    })
}

#[derive(Deserialize)]
struct Taken {
    sealed: String,
}

impl PairingRequest {
    /// `None` while no device has approved yet.
    pub async fn poll(&self, http: &HttpClient) -> Result<Option<Offer>, SyncError> {
        let response = http
            .post_json(
                &format!("{}/v1/pairings/{}/take", self.server, self.pairing),
                &serde_json::json!({ "take": self.take, "claim": false }),
                &options(&self.server, ""),
            )
            .await?;
        if response.status == 204 {
            return Ok(None);
        }
        let taken: Taken = response.json()?;
        let blob = B64
            .decode(&taken.sealed)
            .map_err(|_| bad("pairing reply is not base64"))?;
        let payload = open_payload(self.member.box_secret(), &self.pairing, &blob)?;
        if payload.server != self.server {
            return Err(bad("the reply names another sync server"));
        }
        Ok(Some(Offer { payload }))
    }

    /// Claims a token that can only join, checks the roster up to the head
    /// the approver saw, and appends this device's own `addDevice`. Returns
    /// the keystore record.
    pub async fn claim(
        &self,
        http: &HttpClient,
        offer: Offer,
        name: &str,
    ) -> Result<SyncConfig, SyncError> {
        self.claim_as(http, offer, name, None)
            .await
            .map(|(record, _)| record)
    }

    /// As `claim`, with this device's form (one of `FORMS`); also the name
    /// of the device that approved it, as the account knows it.
    pub async fn claim_as(
        &self,
        http: &HttpClient,
        offer: Offer,
        name: &str,
        form: Option<&str>,
    ) -> Result<(SyncConfig, Option<String>), SyncError> {
        let form = checked_form(form)?;
        let payload = offer.payload;
        let info = relay_info(http, &self.server).await?;
        if info.relay_id != payload.relay_id {
            return Err(SyncError::WrongRelay);
        }
        let claimed: AccountResponse = http
            .post_json(
                &format!("{}/v1/pairings/{}/take", self.server, self.pairing),
                &serde_json::json!({ "take": self.take, "claim": true }),
                &options(&self.server, ""),
            )
            .await?
            .json()?;
        join(
            http,
            &self.server,
            &claimed.device_token,
            &self.member,
            JoinBy::Pairing(payload),
            (name, form),
            info.relay_id,
        )
        .await
    }
}

/// How a new device joins: the approver's word, or a kit it holds.
enum JoinBy {
    Pairing(PairPayload),
    Kit { kit: MemberSecret, tag: [u8; 32] },
}

/// Reads and checks the roster with a pending token, then appends `member`'s
/// own `addDevice` (its name and form), which makes the token active.
async fn join(
    http: &HttpClient,
    server: &str,
    token: &str,
    member: &MemberSecret,
    by: JoinBy,
    (name, form): (&str, Option<&str>),
    relay_id: Id,
) -> Result<(SyncConfig, Option<String>), SyncError> {
    let by_kit = matches!(by, JoinBy::Kit { .. });
    let (entries, _) = fetch_roster(http, server, token, 0).await?;
    let roster = rebuild(&entries).map_err(|e| SyncError::Roster(e.to_string()))?;
    let (keys, sponsor) = match by {
        JoinBy::Pairing(payload) => {
            let head = roster.head().unwrap_or_default();
            if head != (payload.head.seq, payload.head.hash) {
                return Err(bad("the account changed while pairing; pair again"));
            }
            let keys = roster
                .adopt(member, &payload.keys)
                .map_err(|e| SyncError::Roster(e.to_string()))?;
            (keys, payload.sponsor)
        }
        // Keys that do not carry this account's id tag: the lookup was
        // squatted in another account. Nothing is used.
        JoinBy::Kit { kit, tag } => {
            let keys = roster
                .open_for_kit(&kit, &tag)
                .map_err(|_| SyncError::KitUnknown)?;
            (keys, Sponsor::new(&kit, &roster, member.public()))
        }
    };
    // Who let this device in, by the name the account knows it by (a kit has none here).
    let approver = match by_kit {
        true => None,
        false => roster
            .members()
            .get(&sponsor.by)
            .and_then(|m| m.name.as_deref())
            .and_then(|sealed| open_name(&keys, &sponsor.by, sealed)),
    };
    let entry = roster.add_device_as(
        member,
        sponsor,
        seal_name(&keys, &member.id(), name),
        form.map(|f| seal_form(&keys, &member.id(), f)),
        now_ms(),
    );
    http.post_json(
        &format!("{server}/v1/roster"),
        &AppendRequest {
            entry: entry.clone(),
        },
        &options(server, token),
    )
    .await?;
    let seq = roster.head().map_or(0, |(seq, _)| seq + 1);
    let record = SyncConfig {
        server: server.to_owned(),
        device_token: token.to_owned(),
        member: Hex(member.seed()),
        keys,
        checkpoint: RosterHead {
            seq,
            hash: entry.hash(),
        },
        relay_id,
        revision: 1,
    };
    Ok((record, approver))
}

/// A pairing code's fields, after its form and version checked out.
struct Fields(Vec<(String, String)>);

impl Fields {
    fn get(&self, name: &str) -> Option<String> {
        self.0
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    }
}

/// The fields of any pairing code. v3 carries them in the fragment of an
/// https link (any host: the link host of whichever relay made it), or of
/// `momotus://pair`, which the link host's page hands to an app it cannot
/// open by link (an iPhone without associated domains); v2 in a `marquee:`
/// query.
fn code_fields(qr: &str) -> Result<Fields, SyncError> {
    let url = url::Url::parse(qr.trim()).map_err(|_| bad("not a pairing code"))?;
    let (fields, version) = match (url.scheme(), url.host_str(), url.path()) {
        ("https", _, "/pair") | ("momotus", Some("pair"), "") => {
            (url.fragment().unwrap_or_default(), "3")
        }
        ("marquee", None, "pair") => (url.query().unwrap_or_default(), "2"),
        _ => return Err(bad("not a pairing code")),
    };
    let fields = Fields(
        url::form_urlencoded::parse(fields.as_bytes())
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect(),
    );
    if fields.get("v").as_deref() != Some(version) {
        return Err(bad("not a pairing code"));
    }
    Ok(fields)
}

/// A pairing code shown by a new device: its slot and its keys. Refuses a
/// code for another sync server.
fn read_code(server: &str, qr: &str) -> Result<(String, MemberKeys), SyncError> {
    let fields = code_fields(qr)?;
    let pairing = fields
        .get("p")
        .ok_or_else(|| bad("pairing code has no slot"))?;
    if fields.get("s").as_deref() != Some(server) {
        return Err(bad("the code is for another sync server"));
    }
    let key = |name: &str| {
        fields
            .get(name)
            .and_then(|k| B64URL.decode(k).ok())
            .and_then(|k| <[u8; 32]>::try_from(k).ok())
            .map(Hex)
    };
    let (Some(boxk), Some(sign)) = (key("k"), key("g")) else {
        return Err(bad("pairing code has no valid keys"));
    };
    Ok((pairing, MemberKeys { sign, boxk }))
}

/// The text of a new device's QR: `https://<link host>/pair#v=3&...` when the
/// relay names a link host (a phone camera opens the app; the fragment never
/// reaches the host), else `marquee:pair?v=2&...`. The relay, the slot, the
/// device's public keys, and its name and form when given.
fn pairing_code(
    link_host: Option<&str>,
    server: &str,
    slot: &str,
    keys: &MemberKeys,
    shown: Shown,
) -> String {
    let mut fields = url::form_urlencoded::Serializer::new(String::new());
    fields
        .append_pair("v", if link_host.is_some() { "3" } else { "2" })
        .append_pair("s", server)
        .append_pair("p", slot)
        .append_pair("k", &B64URL.encode(keys.boxk.0))
        .append_pair("g", &B64URL.encode(keys.sign.0));
    if let Some(name) = shown.name {
        fields.append_pair("n", name);
    }
    if let Some(form) = shown.form {
        fields.append_pair("f", form);
    }
    let fields = fields.finish();
    match link_host {
        Some(host) => format!("https://{host}/pair#{fields}"),
        None => format!("marquee:pair?{fields}"),
    }
}

/// How the approving phone got the code (plans/tv-qr-login.md Q2): a live
/// scan in its own scanner, or a link (the system camera, the link host's
/// page, a message); and whether its screen lock was just passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Approval {
    pub scanned: bool,
    pub unlocked: bool,
}

/// On a device that already syncs, by a trusted caller (the CLI, test
/// peers): as `approve_pairing_with` for a scan behind the screen lock.
pub async fn approve_pairing(
    library: &Library,
    http: &HttpClient,
    config: &SyncConfig,
    clock: &HlcClock,
    qr: &str,
) -> Result<(String, SyncConfig), SyncError> {
    let trusted = Approval {
        scanned: true,
        unlocked: true,
    };
    approve_pairing_with(library, http, config, clock, qr, trusted).await
}

/// On a device that already syncs: vouches for the device whose QR it got
/// and hands it the account. The relay says first whether the new device
/// opened its code from this device's network. If not, only a live scan
/// approves (`ScanRequired` for a link), and only after the screen lock
/// (`UnlockRequired`), both before anything is kept or sent (Q2, Q6).
/// Returns the new device's member id (hex) and this device's record to
/// store. If the new device does not join in time, the next sync cancels
/// the approval with a key rotation.
pub async fn approve_pairing_with(
    library: &Library,
    http: &HttpClient,
    config: &SyncConfig,
    clock: &HlcClock,
    qr: &str,
    how: Approval,
) -> Result<(String, SyncConfig), SyncError> {
    let (pairing, keys) = read_code(&config.server, qr)?;
    let public =
        PublicKey::from_bytes(&keys.boxk.0).map_err(|_| bad("pairing code has no valid keys"))?;
    let session = Session::open(library, http, config, clock).await?;
    // Only a phone or a tablet approves (D6): a device that said it is a TV, a computer or the
    // web never vouches for another, whatever its app shows. One that said nothing (an older
    // core) still may.
    let me = session.member.id();
    let mine = session
        .roster
        .members()
        .get(&me)
        .and_then(|m| m.form.as_deref());
    let mine = mine.and_then(|sealed| open_form(&session.record.keys, &me, sealed));
    if matches!(mine.as_deref(), Some("tv" | "computer" | "web")) {
        return Err(SyncError::CannotApprove);
    }
    // A code naming a device the account ever admitted cannot join again; approving it would
    // only hand its key out, and They differ would then drop that device.
    if session.roster.admitted(&keys.id()) {
        return Err(bad("this code belongs to a device already in the account"));
    }
    // A relay from before the near check has no such route: never near, so
    // only a scan behind the screen lock approves there.
    let near = match http
        .get_json::<Near>(
            &format!("{}/v1/pairings/{pairing}/near", config.server),
            &options(&config.server, &session.record.device_token),
        )
        .await
    {
        Ok(answer) => answer.near,
        Err(HttpError::Status {
            status: 404 | 405, ..
        }) => false,
        Err(e) => return Err(e.into()),
    };
    if !near && !how.scanned {
        return Err(SyncError::ScanRequired);
    }
    if !near && !how.unlocked {
        return Err(SyncError::UnlockRequired);
    }
    let (seq, hash) = session.roster.head().unwrap_or_default();
    let payload = PairPayload {
        server: config.server.clone(),
        relay_id: session.record.relay_id,
        keys: session.record.keys.clone(),
        head: RosterHead { seq, hash },
        sponsor: Sponsor::new(&session.member, &session.roster, &keys),
        code: None,
    };
    let plain = serde_json::to_vec(&payload).expect("plain struct");
    let blob = seal_to(&public, PAIR_INFO, pairing.as_bytes(), &plain)?;
    // Kept before the payload goes out (and after a failure that may have
    // delivered it): if the device does not join in time, by the relay's
    // clock or this device's, the next sync cancels the approval with a
    // rotation.
    let approval = format!("{APPROVAL}{}", hex::encode(keys.id().0));
    let earlier = library.sync_state(&approval)?;
    let deadlines = format!(
        "{}:{}",
        session.info.server_ms + PAIRING_MS,
        clock.wall_now() + PAIRING_MS
    );
    library.set_sync_state(&approval, &deadlines)?;
    // Refused for sure: this payload never reached the slot, so only an
    // earlier attempt's approval stands.
    let undo = || match &earlier {
        Some(value) => library.set_sync_state(&approval, value),
        None => library.delete_sync_state(&approval),
    };
    let filled = http
        .request(
            mq_http::Method::PUT,
            &format!("{}/v1/pairings/{pairing}", config.server),
            Some(mq_http::Body::Json(
                serde_json::json!({ "sealed": B64.encode(blob) }),
            )),
            &options(&config.server, &session.record.device_token),
        )
        .await;
    match filled {
        Ok(_) => {}
        Err(mq_http::HttpError::Status { status: 409, .. }) => {
            undo()?;
            return Err(SyncError::CodeAnswered);
        }
        Err(mq_http::HttpError::Status { status: 410, .. }) => {
            undo()?;
            return Err(SyncError::CodeExpired);
        }
        Err(e) => return Err(e.into()),
    }
    let record = session.changed().unwrap_or_else(|| session.record.clone());
    Ok((hex::encode(keys.id().0), record))
}

#[derive(Deserialize)]
struct Near {
    near: bool,
}

/// A fresh recovery kit: 160 random bits as 32 base32 characters in groups
/// of four. Shown once; whoever holds it can join the account.
pub fn new_recovery_kit() -> String {
    let mut secret = [0u8; 20];
    getrandom::fill(&mut secret).expect("os randomness");
    base32(&secret)
        .as_bytes()
        .chunks(4)
        .map(|c| std::str::from_utf8(c).expect("ascii"))
        .collect::<Vec<_>>()
        .join("-")
}

/// What a kit derives: lookup id (hex), redeem secret, tag key and the
/// kit's member keys.
struct KitKeys {
    lookup: String,
    auth: [u8; 32],
    tag: [u8; 32],
    member: MemberSecret,
}

fn kit_keys(kit: &str) -> Result<KitKeys, SyncError> {
    let secret: [u8; 20] = unbase32(kit)
        .and_then(|s| s.try_into().ok())
        .ok_or_else(|| bad("not a recovery kit"))?;
    Ok(kit_keys_from(&secret))
}

/// A passkey kit's secret (plans/accounts.md 3.1): the first 20 bytes of
/// HKDF-SHA256 of the passkey's 32-byte PRF output; then every key comes as
/// from a recovery kit's.
pub fn passkey_secret(prf: &[u8; 32]) -> [u8; 20] {
    let mut secret = [0u8; 20];
    Hkdf::<Sha256>::new(None, prf)
        .expand(PASSKEY_KIT_INFO, &mut secret)
        .expect("valid length");
    secret
}

const PASSKEY_KIT_INFO: &[u8] = b"marquee/passkey-kit/v1";

/// What every platform's passkey sheet evaluates the PRF on (`eval.first`,
/// 3.1); `apps/shared/passkey-vectors.json` pins its encodings.
pub const PRF_INPUT: &str = "marquee/passkey/v1";

/// A PRF output as the apps send it: 32 bytes, base64url without padding.
/// A platform encoder that leaves the `=` on is taken too.
pub fn read_prf(text: &str) -> Option<[u8; 32]> {
    B64URL
        .decode(text.trim_end_matches('='))
        .ok()?
        .try_into()
        .ok()
}

fn kit_keys_from(secret: &[u8; 20]) -> KitKeys {
    let hk = Hkdf::<Sha256>::new(None, secret);
    let mut lookup = [0u8; 16];
    let mut auth = [0u8; 32];
    let mut tag = [0u8; 32];
    let mut seed = [0u8; 32];
    hk.expand(b"marquee/recovery/lookup/v1", &mut lookup)
        .expect("valid length");
    hk.expand(b"marquee/recovery/auth/v1", &mut auth)
        .expect("valid length");
    hk.expand(b"marquee/recovery/tag/v1", &mut tag)
        .expect("valid length");
    hk.expand(b"marquee/recovery/member/v1", &mut seed)
        .expect("valid length");
    KitKeys {
        lookup: hex::encode(lookup),
        auth,
        tag,
        member: MemberSecret::from_seed(seed),
    }
}

impl KitKeys {
    /// What the roster gets of this kit.
    fn new_kit(&self, kind: KitKind, label: Option<String>) -> NewKit {
        let lookup = hex::decode(&self.lookup).expect("hex we made");
        NewKit {
            keys: *self.member.public(),
            tag_key: self.tag,
            lookup_hash: Sha256::digest(lookup).into(),
            auth_hash: Sha256::digest(self.auth).into(),
            kind,
            label,
        }
    }
}

/// The approver saw other digits on the new device: the approval for `qr`
/// ends now instead of when it times out. The key rotates at once, dropping
/// the device if it joined meanwhile, and every device it vouched for since
/// (the user approved none of them), so the sealed payload it was handed
/// (the current key and a sponsor statement bound to this roster head) is
/// worth nothing: a join built on it no longer fits the roster. Returns
/// this device's record to store.
pub async fn cancel_pairing(
    library: &Library,
    http: &HttpClient,
    config: &SyncConfig,
    clock: &HlcClock,
    qr: &str,
) -> Result<SyncConfig, SyncError> {
    let (_, keys) = read_code(&config.server, qr)?;
    let id = keys.id();
    let mut session = Session::open(library, http, config, clock).await?;
    let me = session.member.id();
    // A user's own action: no grace period after a relay restore delays it.
    // Judged on the roster each try is built on, as a lost report is.
    session
        .rotate(|roster| {
            let mut drop = roster.descendants(&id, &me);
            drop.push(id);
            drop
        })
        .await?;
    for state in [APPROVAL, CANCEL] {
        library.delete_sync_state(&format!("{state}{}", hex::encode(id.0)))?;
    }
    Ok(session.changed().unwrap_or_else(|| session.record.clone()))
}

/// Makes a recovery kit and adds it to the account's roster with a box of
/// the current key. Returns the kit (show it once, then forget it), its id
/// in the roster (the `id` of its `syncDevices` entry), and this device's
/// record to store.
pub async fn make_kit(
    library: &Library,
    http: &HttpClient,
    config: &SyncConfig,
    clock: &HlcClock,
) -> Result<(String, Id, SyncConfig), SyncError> {
    let kit = new_recovery_kit();
    let keys = kit_keys(&kit)?;
    let new_kit = keys.new_kit(KitKind::Recovery, None);
    let (id, record) = add_kit(library, http, config, clock, new_kit).await?;
    Ok((kit, id, record))
}

/// Saves a passkey as a way into the account: a kit whose secret comes from
/// the passkey's 32-byte PRF output (3.1), labelled for the devices screen
/// ("Passkey, saved on Robin's iPhone"). Returns its id in the
/// roster and this device's record to store. A PRF output whose lookup
/// another account holds is `kitTaken`; a kit dropped before is never let
/// in again.
pub async fn save_passkey(
    library: &Library,
    http: &HttpClient,
    config: &SyncConfig,
    clock: &HlcClock,
    prf: &[u8; 32],
    label: &str,
) -> Result<(Id, SyncConfig), SyncError> {
    let new_kit = passkey_kit(prf, Some(label.to_owned()));
    add_kit(library, http, config, clock, new_kit).await
}

/// What the roster gets of the passkey kit a PRF output makes.
pub fn passkey_kit(prf: &[u8; 32], label: Option<String>) -> NewKit {
    kit_keys_from(&passkey_secret(prf)).new_kit(KitKind::Passkey, label)
}

async fn add_kit(
    library: &Library,
    http: &HttpClient,
    config: &SyncConfig,
    clock: &HlcClock,
    new_kit: NewKit,
) -> Result<(Id, SyncConfig), SyncError> {
    let mut session = Session::open(library, http, config, clock).await?;
    let at_ms = clock.tick(clock.wall_now()).wall_ms;
    let id = new_kit.keys.id();
    session
        .propose(|s| {
            // Judged on the roster each try is built on, before anything is
            // sent: a passkey that already opens the account (the save
            // probe, 3.1), one dropped before (never let in again: save a
            // new one), the cap.
            let roster = &s.roster;
            if roster.members().contains_key(&id) {
                return Err(SyncError::KitHere);
            }
            if roster.admitted(&id) {
                return Err(SyncError::KitDropped);
            }
            let kits = roster.members().values().filter(|m| m.role == Role::Kit);
            if kits.count() >= MAX_KITS {
                return Err(SyncError::TooManyKits);
            }
            roster
                .add_kit(&s.member, &s.record.keys, &new_kit, at_ms)
                .map_err(|e| SyncError::Roster(e.to_string()))
        })
        .await?;
    let record = session.changed().unwrap_or_else(|| session.record.clone());
    Ok((id, record))
}

/// A way back into an account.
pub enum Recovery {
    /// A recovery kit as typed.
    Kit(String),
    /// A passkey's 32-byte PRF output.
    Passkey([u8; 32]),
}

/// Which way a recovered device came in: only a kit's user is told to make
/// a new one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Via {
    Kit,
    Passkey,
}

/// Joins the account a recovery kit belongs to, as a new device named
/// `name`. Returns the new device's keystore record.
pub async fn recover(
    http: &HttpClient,
    server: &str,
    kit: &str,
    name: &str,
) -> Result<SyncConfig, SyncError> {
    recover_with(http, server, Recovery::Kit(kit.to_owned()), name, None)
        .await
        .map(|(record, _)| record)
}

/// Joins the account a kit or a passkey opens, as a new device named `name`
/// of `form`: the new device's keystore record, and which way it came. A
/// kit the relay does not know (another passkey's PRF, a typo) and keys
/// that are not that account's (a lookup squatted in another account) are
/// both `kitUnknown`.
pub async fn recover_with(
    http: &HttpClient,
    server: &str,
    way: Recovery,
    name: &str,
    form: Option<&str>,
) -> Result<(SyncConfig, Via), SyncError> {
    require_secure(server)?;
    let form = checked_form(form)?;
    let (keys, via) = match way {
        Recovery::Kit(kit) => (kit_keys(&kit)?, Via::Kit),
        Recovery::Passkey(prf) => (kit_keys_from(&passkey_secret(&prf)), Via::Passkey),
    };
    let info = relay_info(http, server).await?;
    let redeemed: Redeemed = http
        .post_json(
            &format!("{server}/v1/recovery/redeem"),
            &serde_json::json!({ "lookup": keys.lookup, "auth": hex::encode(keys.auth) }),
            &options(server, ""),
        )
        .await?
        .json()?;
    if redeemed.kit != keys.member.id() {
        return Err(SyncError::KitUnknown);
    }
    let (record, _) = join(
        http,
        server,
        &redeemed.device_token,
        &MemberSecret::generate(),
        JoinBy::Kit {
            kit: keys.member,
            tag: keys.tag,
        },
        (name, form),
        info.relay_id,
    )
    .await?;
    Ok((record, via))
}

/// A form as the caller gave it: one of `FORMS`, or none.
pub(crate) fn checked_form(form: Option<&str>) -> Result<Option<&str>, SyncError> {
    match form {
        Some(f) if !FORMS.contains(&f) => Err(bad("unknown device form")),
        form => Ok(form),
    }
}

const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// RFC 4648 base32 without padding.
fn base32(bytes: &[u8]) -> String {
    let mut out = String::new();
    let (mut buffer, mut bits) = (0u32, 0);
    for &b in bytes {
        buffer = (buffer << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buffer >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

/// Reads a kit as typed: case, spaces and dashes do not matter.
fn unbase32(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut buffer, mut bits) = (0u32, 0);
    for c in text.chars().filter(|c| c.is_ascii_alphanumeric()) {
        let v = ALPHABET
            .iter()
            .position(|&a| a == c.to_ascii_uppercase() as u8)? as u32;
        buffer = (buffer << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kits_derive_the_same_keys_however_they_are_typed() {
        let kit = new_recovery_kit();
        assert_eq!(kit.len(), 32 + 7, "{kit}");
        let a = kit_keys(&kit).unwrap();
        let b = kit_keys(&kit.to_lowercase().replace('-', " ")).unwrap();
        assert_eq!((a.lookup, a.auth), (b.lookup, b.auth));
        assert_eq!(a.member.public(), b.member.public());
        assert!(kit_keys("ABCD-EFGH").is_err());
        assert_eq!(base32(b"foobar"), "MZXW6YTBOI");
        assert_eq!(unbase32("MZXW6YTBOI").unwrap(), b"foobar");
    }

    /// The HKDF step every platform shares (`apps/shared/passkey-vectors.json`,
    /// computed apart from this code).
    #[test]
    fn passkey_kits_derive_as_the_shared_vectors_say() {
        let vectors: serde_json::Value =
            serde_json::from_str(include_str!("../../../apps/shared/passkey-vectors.json"))
                .unwrap();
        let cases = vectors["passkeyKit"].as_array().unwrap();
        assert_eq!(cases.len(), 3);
        for case in cases {
            let prf = read_prf(case["prf"].as_str().unwrap()).unwrap();
            let secret = passkey_secret(&prf);
            assert_eq!(hex::encode(secret), case["kitSecret"].as_str().unwrap());
            assert_eq!(
                kit_keys_from(&secret).lookup,
                case["lookup"].as_str().unwrap()
            );
        }
        assert!(read_prf("AAAA").is_none(), "only 32 bytes");
        // The input side: one text, its encodings and the salt WebAuthn makes of it.
        let input = &vectors["prfInput"];
        assert_eq!(input["text"], PRF_INPUT);
        assert_eq!(input["hex"], hex::encode(PRF_INPUT));
        assert_eq!(input["base64url"], B64URL.encode(PRF_INPUT));
        let salt = Sha256::digest([b"WebAuthn PRF\0".as_slice(), PRF_INPUT.as_bytes()].concat());
        assert_eq!(input["salt"], hex::encode(salt));
        // A platform encoder that pads is read too.
        let padded = format!("{}=", cases[0]["prf"].as_str().unwrap());
        assert_eq!(read_prf(&padded), Some([0; 32]));
    }

    /// The QR as a link a phone camera opens when the relay names a link
    /// host (v3, its fields in the fragment, also under `momotus://pair`,
    /// which the link host's page hands on), as `marquee:pair` when not
    /// (v2); all read, for this relay only. Name and form ride along.
    #[test]
    fn pairing_codes_read_in_every_form() {
        let keys = *MemberSecret::generate().public();
        let server = "https://sync.example.org";
        let shown = Shown {
            name: Some("Living room TV"),
            form: Some("tv"),
        };
        let v3 = pairing_code(Some("link.example.org"), server, "slot", &keys, shown);
        assert!(v3.starts_with("https://link.example.org/pair#v=3&"), "{v3}");
        assert!(v3.ends_with("&n=Living+room+TV&f=tv"), "{v3}");
        let app = v3.replace("https://link.example.org/pair#", "momotus://pair#");
        let v2 = pairing_code(None, server, "slot", &keys, Shown::default());
        assert!(v2.starts_with("marquee:pair?v=2&"), "{v2}");
        assert!(!v2.contains("&n="), "{v2}");
        for code in [&v3, &app, &v2] {
            let (slot, read) = read_code(server, code).unwrap();
            assert_eq!((slot.as_str(), read), ("slot", keys));
            assert!(
                read_code("https://other.example", code).is_err(),
                "another relay"
            );
        }
        for wrong in [
            v3.replace("v=3", "v=2"),
            v3.replace("/pair#", "/other#"),
            v3.replace("https:", "http:"),
            app.replace("momotus://pair#", "momotus://auth#"),
            app.replace("momotus://pair#", "momotus://pair/x#"),
            app.replace("momotus://pair#", "other://pair#"),
            v2.replace("v=2", "v=3"),
        ] {
            assert!(read_code(server, &wrong).is_err(), "{wrong}");
        }
    }

    /// A new device opens a v3 payload and an old phone's v2 one, its digits
    /// ignored; one sealed under another info or slot does not open (Q10).
    #[test]
    fn a_new_device_opens_an_old_phones_payload() {
        let tv = MemberSecret::generate();
        let public = PublicKey::from_bytes(&tv.public().boxk.0).unwrap();
        for (info, opens) in [
            (PAIR_INFO, true),
            (PAIR_INFO_V2, true),
            (b"marquee/pair/v1".as_slice(), false),
        ] {
            let blob = seal_to(&public, info, b"slot", b"{}").unwrap();
            let opened = open_payload(tv.box_secret(), "slot", &blob);
            // "{}" is no payload: what matters is whether it opened.
            let malformed =
                matches!(&opened, Err(SyncError::Pairing(why)) if why.contains("malformed"));
            assert_eq!(malformed, opens, "{info:?}");
        }
        let blob = seal_to(&public, PAIR_INFO_V2, b"other", b"{}").unwrap();
        assert!(
            !matches!(open_payload(tv.box_secret(), "slot", &blob), Err(SyncError::Pairing(why)) if why.contains("malformed"))
        );
    }

    /// A relay names its link host; only a bare host name goes into the link.
    #[test]
    fn only_a_bare_host_makes_a_link() {
        assert!(bare_host("momotus.normalisdead.mom"));
        for odd in [
            "evil.example/x?",
            "host:443",
            "Link.Example",
            "",
            "a..b",
            "user@host",
            "a.example?x",
            "10.0.0.1",
            "192.168.1.1",
            "localhost",
            "[::1]",
        ] {
            assert!(!bare_host(odd), "{odd}");
        }
    }

    /// The name a screen gives itself reaches the sheet as plain text: no
    /// control, bidi or zero-width characters, whitespace collapsed, 40
    /// characters at most; a form outside the five is none (Q4).
    #[test]
    fn a_screens_name_reaches_the_sheet_as_plain_text() {
        assert_eq!(
            clean_name("Living room TV").as_deref(),
            Some("Living room TV")
        );
        assert_eq!(
            clean_name("Momotus\u{202E}deifirev\u{202C} TV").as_deref(),
            Some("Momotusdeifirev TV")
        );
        assert_eq!(
            clean_name("Two\nlines\u{200B} here ").as_deref(),
            Some("Two lines here")
        );
        assert_eq!(clean_name(" \u{200D}\t").as_deref(), None);
        assert_eq!(
            clean_name("Den\u{061C}\u{3164}\u{2800}\u{FE0F}\u{E0041}\u{180E} TV").as_deref(),
            Some("Den TV")
        );
        let long = "Ç".repeat(41);
        assert_eq!(clean_name(&long).map(|n| n.chars().count()), Some(40));

        let keys = *MemberSecret::generate().public();
        let shown = Shown {
            name: Some("Den"),
            form: Some("tv"),
        };
        let qr = pairing_code(
            Some("link.example.org"),
            "https://sync.example.org",
            "slot",
            &keys,
            shown,
        );
        let read = read_pairing_code(&qr).unwrap();
        assert_eq!(
            (
                read.server.as_str(),
                read.name.as_deref(),
                read.form.as_deref()
            ),
            ("https://sync.example.org", Some("Den"), Some("tv"))
        );
        let odd = qr
            .replace("n=Den", "n=%E2%80%AEevil%0A")
            .replace("f=tv", "f=fridge");
        let read = read_pairing_code(&odd).unwrap();
        assert_eq!((read.name.as_deref(), read.form), (Some("evil"), None));
        assert!(read_pairing_code("https://link.example.org/other#v=3").is_err());
    }
}

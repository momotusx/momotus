//! Adding a device and recovering an account, with no password and nothing
//! the relay can read or forge (plan: "Phase 2" and "Phase 3").
//!
//! Pairing: the new device makes its member keys and shows a QR with a relay
//! slot id and its public keys; a device of the account seals the current
//! key bundle, the roster head, a sponsor statement for exactly those keys
//! and a six-digit code to the new device's box key with HPKE (RFC 9180),
//! and shows the code. The new device keeps nothing until the user confirms
//! both screens show the same code; then it claims a token that can only
//! join, checks the roster, and signs its own `addDevice`.
//!
//! Recovery: a 160-bit kit derives a lookup id, a redeem secret, a tag key
//! and member keys. Making a kit appends it to the roster with a box of the
//! current bundle; later rotations box new keys to it like to any member.
//! Redeeming needs the redeem secret, which no device keeps; the recovering
//! device opens the kit's newest box, checks the id tag, and joins vouched
//! for by the kit.

use crate::engine::{
    PAIRING_MS, Session, SyncConfig, SyncError, fetch_roster, options, rebuild, relay_info,
    require_secure,
};
use crate::roster::{Bundle, Hex, Id, MemberKeys, MemberSecret, NewKit, Sponsor, seal_name};
use crate::wire::{AccountResponse, AppendRequest, Redeemed, RosterHead};
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD as B64, URL_SAFE_NO_PAD as B64URL};
use hkdf::Hkdf;
use hpke::aead::ChaCha20Poly1305;
use hpke::kdf::HkdfSha256;
use hpke::kem::X25519HkdfSha256;
use hpke::{Deserializable, OpModeR, OpModeS, Serializable};
use mq_http::HttpClient;
use mq_library::Library;
use mq_library::hlc::{HlcClock, now_ms};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(crate) type Kem = X25519HkdfSha256;
pub(crate) type PrivateKey = <Kem as hpke::Kem>::PrivateKey;
pub(crate) type PublicKey = <Kem as hpke::Kem>::PublicKey;
const PAIR_INFO: &[u8] = b"marquee/pair/v2";
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
    code: String,
}

/// A pairing a device of the account approved. Show `code`; only when the
/// user confirms it matches the other device, `claim` it.
#[derive(Clone, PartialEq, Eq)]
pub struct Offer {
    pub code: String,
    payload: PairPayload,
}

impl std::fmt::Debug for Offer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Offer({})", self.code)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Slot {
    pairing: String,
    take: String,
    expires_ms: u64,
}

/// A pairing in progress on the new device.
pub struct PairingRequest {
    /// Text for the QR code.
    pub qr: String,
    pub expires_ms: u64,
    server: String,
    pairing: String,
    take: String,
    member: MemberSecret,
}

/// Opens a pairing slot on the relay, makes this device's member keys and
/// returns the QR to show.
pub async fn start_pairing(http: &HttpClient, server: &str) -> Result<PairingRequest, SyncError> {
    require_secure(server)?;
    let slot: Slot = http
        .post_json(
            &format!("{server}/v1/pairings"),
            &serde_json::json!({}),
            &options(server, ""),
        )
        .await?
        .json()?;
    let member = MemberSecret::generate();
    let keys = member.public();
    let mut qr = url::Url::parse("marquee:pair").expect("static url");
    qr.query_pairs_mut()
        .append_pair("v", "2")
        .append_pair("s", server)
        .append_pair("p", &slot.pairing)
        .append_pair("k", &B64URL.encode(keys.boxk.0))
        .append_pair("g", &B64URL.encode(keys.sign.0));
    Ok(PairingRequest {
        qr: qr.to_string(),
        expires_ms: slot.expires_ms,
        server: server.to_owned(),
        pairing: slot.pairing,
        take: slot.take,
        member,
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
        let plain = open_with(
            self.member.box_secret(),
            PAIR_INFO,
            self.pairing.as_bytes(),
            &blob,
        )?;
        let payload: PairPayload =
            serde_json::from_slice(&plain).map_err(|_| bad("pairing reply is malformed"))?;
        if payload.server != self.server {
            return Err(bad("the reply names another sync server"));
        }
        Ok(Some(Offer {
            code: payload.code.clone(),
            payload,
        }))
    }

    /// After the user confirmed the codes match: claims a token that can
    /// only join, checks the roster up to the head the approver saw, and
    /// appends this device's own `addDevice`. Returns the keystore record.
    pub async fn claim(
        &self,
        http: &HttpClient,
        offer: Offer,
        name: &str,
    ) -> Result<SyncConfig, SyncError> {
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
            name,
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
/// own `addDevice`, which makes the token active.
async fn join(
    http: &HttpClient,
    server: &str,
    token: &str,
    member: &MemberSecret,
    by: JoinBy,
    name: &str,
    relay_id: Id,
) -> Result<SyncConfig, SyncError> {
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
        JoinBy::Kit { kit, tag } => {
            let keys = roster
                .open_for_kit(&kit, &tag)
                .map_err(|_| bad("the recovered keys are not this account's; do not use them"))?;
            (keys, Sponsor::new(&kit, &roster, member.public()))
        }
    };
    let entry = roster.add_device(
        member,
        sponsor,
        seal_name(&keys, &member.id(), name),
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
    Ok(SyncConfig {
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
    })
}

/// On a device that already syncs: vouches for the device whose QR was
/// scanned and hands it the account. Returns the code to show (the new
/// device must show the same) and this device's record to store. If the
/// new device does not join in time, the next sync cancels the approval
/// with a key rotation.
pub async fn approve_pairing(
    library: &Library,
    http: &HttpClient,
    config: &SyncConfig,
    clock: &HlcClock,
    qr: &str,
) -> Result<(String, SyncConfig), SyncError> {
    let url = url::Url::parse(qr).map_err(|_| bad("not a pairing code"))?;
    let field = |name: &str| {
        url.query_pairs()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.into_owned())
    };
    if url.scheme() != "marquee" || url.path() != "pair" || field("v").as_deref() != Some("2") {
        return Err(bad("not a pairing code"));
    }
    let pairing = field("p").ok_or_else(|| bad("pairing code has no slot"))?;
    if field("s").as_deref() != Some(config.server.as_str()) {
        return Err(bad("the code is for another sync server"));
    }
    let key = |name: &str| {
        field(name)
            .and_then(|k| B64URL.decode(k).ok())
            .and_then(|k| <[u8; 32]>::try_from(k).ok())
            .map(Hex)
    };
    let (Some(boxk), Some(sign)) = (key("k"), key("g")) else {
        return Err(bad("pairing code has no valid keys"));
    };
    let keys = MemberKeys { sign, boxk };
    let public =
        PublicKey::from_bytes(&boxk.0).map_err(|_| bad("pairing code has no valid keys"))?;
    let session = Session::open(library, http, config, clock).await?;
    let code = six_digits();
    let (seq, hash) = session.roster.head().unwrap_or_default();
    let payload = PairPayload {
        server: config.server.clone(),
        relay_id: session.record.relay_id,
        keys: session.record.keys.clone(),
        head: RosterHead { seq, hash },
        sponsor: Sponsor::new(&session.member, &session.roster, &keys),
        code: code.clone(),
    };
    let plain = serde_json::to_vec(&payload).expect("plain struct");
    let blob = seal_to(&public, PAIR_INFO, pairing.as_bytes(), &plain)?;
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
            return Err(bad(
                "someone else already answered this code; reject it on the new device",
            ));
        }
        Err(mq_http::HttpError::Status { status: 410, .. }) => {
            return Err(bad(
                "this pairing code expired; start again on the new device",
            ));
        }
        Err(e) => return Err(e.into()),
    }
    // Until the device joins, the approval stands; past this, it is
    // cancelled by a rotation (kept across restarts).
    library.set_sync_state(
        &format!("approval:{}", hex::encode(keys.id().0)),
        &(session.info.server_ms + PAIRING_MS).to_string(),
    )?;
    let record = session.changed().unwrap_or_else(|| session.record.clone());
    Ok((code, record))
}

fn six_digits() -> String {
    let mut b = [0u8; 4];
    getrandom::fill(&mut b).expect("os randomness");
    format!("{:06}", u32::from_le_bytes(b) % 1_000_000)
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
    let secret = unbase32(kit)
        .filter(|s| s.len() == 20)
        .ok_or_else(|| bad("not a recovery kit"))?;
    let hk = Hkdf::<Sha256>::new(None, &secret);
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
    Ok(KitKeys {
        lookup: hex::encode(lookup),
        auth,
        tag,
        member: MemberSecret::from_seed(seed),
    })
}

/// Makes a recovery kit and adds it to the account's roster with a box of
/// the current key. Returns the kit (show it once, then forget it) and this
/// device's record to store.
pub async fn make_kit(
    library: &Library,
    http: &HttpClient,
    config: &SyncConfig,
    clock: &HlcClock,
) -> Result<(String, SyncConfig), SyncError> {
    let kit = new_recovery_kit();
    let keys = kit_keys(&kit)?;
    let lookup = hex::decode(&keys.lookup).expect("hex we made");
    let new_kit = NewKit {
        keys: *keys.member.public(),
        tag_key: keys.tag,
        lookup_hash: Sha256::digest(lookup).into(),
        auth_hash: Sha256::digest(keys.auth).into(),
    };
    let mut session = Session::open(library, http, config, clock).await?;
    let at_ms = clock.tick(clock.wall_now()).wall_ms;
    session
        .propose(|s| {
            s.roster
                .add_kit(&s.member, &s.record.keys, &new_kit, at_ms)
                .map_err(|e| SyncError::Roster(e.to_string()))
        })
        .await?;
    let record = session.changed().unwrap_or_else(|| session.record.clone());
    Ok((kit, record))
}

/// Joins the account a recovery kit belongs to, as a new device named
/// `name`. Returns the new device's keystore record.
pub async fn recover(
    http: &HttpClient,
    server: &str,
    kit: &str,
    name: &str,
) -> Result<SyncConfig, SyncError> {
    require_secure(server)?;
    let keys = kit_keys(kit)?;
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
        return Err(bad("the relay answered for another kit"));
    }
    join(
        http,
        server,
        &redeemed.device_token,
        &MemberSecret::generate(),
        JoinBy::Kit {
            kit: keys.member,
            tag: keys.tag,
        },
        name,
        info.relay_id,
    )
    .await
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
        assert_eq!(six_digits().len(), 6);
    }
}

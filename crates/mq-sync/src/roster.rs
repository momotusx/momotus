//! The account roster: who may hold the account key, kept as a hash-chained
//! log of entries that members sign. The relay stores and serves the log but
//! cannot extend it, so it cannot add a device, a kit or a key. Devices apply
//! every entry with the rules here; the relay runs the same rules as a first
//! filter.
//!
//! The key of an epoch reaches members in boxes (HPKE to each member's box
//! key) inside the signed `revoke` entry that starts the epoch. That entry
//! also carries the previous key sealed under the new one, so the newest key
//! opens every older epoch and history needs no resealing.

use crate::pair::{Kem, PrivateKey, PublicKey, open_with, seal_to};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use hpke::{Deserializable, Kem as _, Serializable};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// Every signature names what it is for, so a signature made for one use
/// can never pass as another.
const ROSTER: &[u8] = b"marquee/roster/v1\0";
const SPONSOR: &[u8] = b"marquee/sponsor/v1\0";
const TOKEN: &[u8] = b"marquee/token/v1\0";
const DELETE: &[u8] = b"marquee/delete/v1\0";
const NAME_INFO: &[u8] = b"marquee/name/v1";
const EPOCH_INFO: &[u8] = b"marquee/epoch/v1";
const CHAIN_INFO: &[u8] = b"marquee/chain/v1";
pub const MAX_DEVICES: usize = 32;
/// Apple, Google and a password manager's passkeys, a recovery kit, and
/// room for stale passkeys (D8).
pub const MAX_KITS: usize = 8;
/// What a device says it is (sealed, padded to one length so the relay
/// cannot tell them apart): only phones and tablets approve a new device.
pub const FORMS: [&str; 5] = ["phone", "tablet", "tv", "computer", "web"];
const FORM_LEN: usize = 8;
/// The largest entry body, checked before parsing: 32 boxes and a name fit
/// many times over.
pub const MAX_ENTRY: usize = 64 * 1024;
const NONCE: usize = 24;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RosterError {
    #[error("roster entry refused: {0}")]
    Invalid(&'static str),
    #[error("roster entry does not follow the head")]
    NotNext,
    #[error("a sealed key does not open")]
    Open,
}

use RosterError::Invalid;

/// Fixed-size bytes, written as lowercase hex (one spelling per value).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Hex<const N: usize>(pub [u8; N]);

pub type Id = Hex<16>;
pub type Digest32 = Hex<32>;

impl<const N: usize> Default for Hex<N> {
    fn default() -> Self {
        Self([0; N])
    }
}

impl<const N: usize> std::fmt::Debug for Hex<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&hex::encode(self.0))
    }
}

impl<const N: usize> Serialize for Hex<N> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(self.0))
    }
}

impl<'de, const N: usize> Deserialize<'de> for Hex<N> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        let bytes = (!text.bytes().any(|b| b.is_ascii_uppercase()))
            .then(|| hex::decode(&text).ok())
            .flatten()
            .and_then(|b| b.try_into().ok());
        bytes
            .map(Hex)
            .ok_or_else(|| serde::de::Error::custom(format!("expected {N} bytes of lowercase hex")))
    }
}

mod b64 {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD as B64;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&B64.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        B64.decode(String::deserialize(d)?)
            .map_err(|_| serde::de::Error::custom("expected base64"))
    }
}

fn digest(bytes: &[u8]) -> Digest32 {
    Hex(Sha256::digest(bytes).into())
}

fn random<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).expect("os randomness");
    bytes
}

/// A member's public keys, as the roster lists them. They never change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberKeys {
    pub sign: Hex<32>,
    #[serde(rename = "box")]
    pub boxk: Hex<32>,
}

impl MemberKeys {
    /// Derived from the signing key, never taken from a field.
    pub fn id(&self) -> Id {
        let hash = Sha256::digest(self.sign.0);
        Hex(hash[..16].try_into().expect("16 bytes"))
    }

    fn verifies(&self, purpose: &[u8], bytes: &[u8], sig: &Hex<64>) -> bool {
        VerifyingKey::from_bytes(&self.sign.0).is_ok_and(|key| {
            key.verify_strict(&[purpose, bytes].concat(), &Signature::from_bytes(&sig.0))
                .is_ok()
        })
    }

    fn box_key(&self) -> Result<PublicKey, RosterError> {
        PublicKey::from_bytes(&self.boxk.0).map_err(|_| Invalid("unusable box key"))
    }
}

/// A member's private keys, from one 32-byte seed: a device keeps the seed
/// in its keystore; a kit derives it from its secret.
pub struct MemberSecret {
    seed: [u8; 32],
    sign: SigningKey,
    boxk: PrivateKey,
    public: MemberKeys,
}

impl std::fmt::Debug for MemberSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MemberSecret({:?}, <redacted>)", self.public.id())
    }
}

impl MemberSecret {
    pub fn generate() -> Self {
        Self::from_seed(random())
    }

    pub fn from_seed(seed: [u8; 32]) -> Self {
        let hk = Hkdf::<Sha256>::new(None, &seed);
        let mut sign_seed = [0u8; 32];
        let mut box_seed = [0u8; 32];
        hk.expand(b"marquee/member/sign/v1", &mut sign_seed)
            .expect("valid length");
        hk.expand(b"marquee/member/box/v1", &mut box_seed)
            .expect("valid length");
        let sign = SigningKey::from_bytes(&sign_seed);
        let (boxk, box_public) = Kem::derive_keypair(&box_seed);
        let public = MemberKeys {
            sign: Hex(sign.verifying_key().to_bytes()),
            boxk: Hex(box_public.to_bytes().into()),
        };
        Self {
            seed,
            sign,
            boxk,
            public,
        }
    }

    pub fn seed(&self) -> [u8; 32] {
        self.seed
    }

    pub fn public(&self) -> &MemberKeys {
        &self.public
    }

    pub fn id(&self) -> Id {
        self.public.id()
    }

    fn sign(&self, purpose: &[u8], bytes: &[u8]) -> Hex<64> {
        Hex(self.sign.sign(&[purpose, bytes].concat()).to_bytes())
    }

    pub(crate) fn box_secret(&self) -> &PrivateKey {
        &self.boxk
    }

    /// Asks a relay for a fresh token as this member.
    pub fn sign_token(&self, request: &TokenRequest) -> Hex<64> {
        self.sign(TOKEN, &request.message())
    }

    /// Asks a relay to delete the account, as this member: the same fields
    /// as a token request, under a purpose of its own.
    pub fn sign_delete(&self, request: &TokenRequest) -> Hex<64> {
        self.sign(DELETE, &request.message())
    }
}

/// A member's request for a fresh token, signed with its device key. The
/// relay id keeps a relay from replaying it to another relay; the relay's
/// time and a nonce used once keep it from being replayed at all.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TokenRequest {
    pub relay_id: Id,
    pub account: Id,
    pub member: Id,
    pub at_ms: u64,
    pub nonce: Hex<16>,
    /// The host the device sent the request to; the relay takes only its
    /// own public host.
    pub host: String,
}

impl TokenRequest {
    pub fn new(relay_id: Id, account: Id, member: Id, at_ms: u64, host: &str) -> Self {
        Self {
            relay_id,
            account,
            member,
            at_ms,
            nonce: Hex(random()),
            host: host.to_owned(),
        }
    }

    /// Fixed-size fields first, so the one of variable length is unambiguous.
    fn message(&self) -> Vec<u8> {
        [
            &self.relay_id.0[..],
            &self.account.0,
            &self.member.0,
            &self.at_ms.to_be_bytes(),
            &self.nonce.0,
            self.host.as_bytes(),
        ]
        .concat()
    }

    /// Whether `sig` is this request signed by the member with `keys`.
    pub fn verifies(&self, keys: &MemberKeys, sig: &Hex<64>) -> bool {
        keys.id() == self.member && keys.verifies(TOKEN, &self.message(), sig)
    }

    /// Whether `sig` is this request signed as a deletion by the member.
    pub fn verifies_delete(&self, keys: &MemberKeys, sig: &Hex<64>) -> bool {
        keys.id() == self.member && keys.verifies(DELETE, &self.message(), sig)
    }
}

/// What a member's box holds: everything needed to read and write the
/// account's records at one epoch.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Bundle {
    pub v: u8,
    pub account: Id,
    pub genesis: Digest32,
    pub epoch: u32,
    pub ak: Hex<32>,
    pub id_key: Hex<32>,
}

impl std::fmt::Debug for Bundle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Bundle({:?}, epoch {}, <redacted>)",
            self.account, self.epoch
        )
    }
}

/// A signed roster entry: the body bytes exactly as signed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    #[serde(with = "b64")]
    pub body: Vec<u8>,
    pub sig: Hex<64>,
}

impl Entry {
    pub fn sign(author: &MemberSecret, body: &Body) -> Self {
        let bytes = serde_json::to_vec(body).expect("roster bodies serialize");
        Self {
            sig: author.sign(ROSTER, &bytes),
            body: bytes,
        }
    }

    pub fn hash(&self) -> Digest32 {
        digest(&self.body)
    }

    pub fn parse(&self) -> Result<Body, RosterError> {
        if self.body.len() > MAX_ENTRY {
            return Err(Invalid("entry too large"));
        }
        serde_json::from_slice(&self.body).map_err(|_| Invalid("malformed entry"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Body {
    pub v: u8,
    pub account: Id,
    pub seq: u64,
    /// The parent entry's hash; zero for genesis.
    pub prev: Digest32,
    pub author: Id,
    /// The author's clock; shown to people, never trusted for ordering.
    pub at_ms: u64,
    pub kind: Kind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Genesis(Genesis),
    AddDevice(AddDevice),
    AddKit(AddKit),
    Revoke(Revoke),
    Leave(Leave),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Genesis {
    pub keys: MemberKeys,
    /// The first device's name, sealed for members.
    pub name: String,
    /// Its form (`FORMS`), sealed for members.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form: Option<String>,
}

/// Authored by the new device itself, which proves it holds both keys.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AddDevice {
    pub keys: MemberKeys,
    /// The device's name, sealed for members.
    pub name: String,
    pub sponsor: Sponsor,
    /// Its form (`FORMS`), sealed for members.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form: Option<String>,
}

/// A recovery kit (its secret shown once) or a passkey kit (its secret from
/// a passkey's PRF output). The relay reads the kind: it checks how long
/// each may stay (3.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KitKind {
    #[default]
    Recovery,
    Passkey,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AddKit {
    pub keys: MemberKeys,
    /// `HMAC(kit tag key, idKey || genesis)`: a recovering device checks the
    /// keys it opens belong to this account.
    pub id_tag: Hex<32>,
    pub lookup_hash: Hex<32>,
    pub auth_hash: Hex<32>,
    #[serde(rename = "box")]
    pub boxed: SealedBox,
    #[serde(default)]
    pub kind: KitKind,
    /// What the kit is ("Passkey, saved on Robin's iPhone, 20 Oct"), sealed
    /// for members.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Starts a new epoch: drops members (possibly none, a plain rotation) and
/// boxes the new key to everyone left.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Revoke {
    pub dropped: Vec<Id>,
    pub new_epoch: u32,
    pub boxes: Vec<SealedBox>,
    #[serde(with = "b64")]
    pub prev_blob: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Leave {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SealedBox {
    pub to: Id,
    #[serde(with = "b64")]
    pub sealed: Vec<u8>,
}

/// A member's word that these keys may join as the entry right after
/// `parent`, at that epoch. Any roster change makes it useless, so it needs
/// no clock.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sponsor {
    pub by: Id,
    pub sig: Hex<64>,
}

impl Sponsor {
    pub fn new(by: &MemberSecret, roster: &Roster, keys: &MemberKeys) -> Self {
        let message = sponsor_message(&roster.account, &roster.parent(), roster.epoch, keys);
        Self {
            by: by.id(),
            sig: by.sign(SPONSOR, &message),
        }
    }
}

fn sponsor_message(account: &Id, parent: &Digest32, epoch: u32, keys: &MemberKeys) -> Vec<u8> {
    [
        &account.0[..],
        &parent.0,
        &epoch.to_be_bytes(),
        &keys.sign.0,
        &keys.boxk.0,
    ]
    .concat()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Device,
    Kit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub role: Role,
    pub keys: MemberKeys,
    /// Who vouched for it; never changes, so a lost device's descendants
    /// can always be found.
    pub sponsor: Option<Id>,
    pub added_seq: u64,
    pub added_ms: u64,
    /// A device's name, or a kit's label; sealed.
    pub name: Option<String>,
    /// A device's form, sealed.
    pub form: Option<String>,
    /// A passkey kit.
    pub passkey: bool,
}

/// Every id ever admitted, dropped or not: ids are never reused, and a lost
/// device's descendants are found even through members that are gone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Admission {
    role: Role,
    sponsor: Option<Id>,
    seq: u64,
    at_ms: u64,
    passkey: bool,
}

/// What a rotation brought: the previous key sealed under the new one, and
/// each member's box.
#[derive(Clone, Debug)]
struct Rotation {
    prev_blob: Vec<u8>,
    boxes: BTreeMap<Id, Vec<u8>>,
    at_ms: u64,
}

/// What a new kit puts in the roster, from the kit's derived keys.
pub struct NewKit {
    pub keys: MemberKeys,
    pub tag_key: [u8; 32],
    pub lookup_hash: [u8; 32],
    pub auth_hash: [u8; 32],
    pub kind: KitKind,
    /// In the clear here; sealed in the entry.
    pub label: Option<String>,
}

/// The state the entries so far describe.
#[derive(Clone, Debug, Default)]
pub struct Roster {
    account: Id,
    genesis: Digest32,
    head: Option<(u64, Digest32)>,
    epoch: u32,
    members: BTreeMap<Id, Member>,
    ancestry: BTreeMap<Id, Admission>,
    rotation_pending: bool,
    rotations: BTreeMap<u32, Rotation>,
    /// A kit's box from `addKit`, with that epoch, and its id tag.
    kit_boxes: BTreeMap<Id, (u32, Vec<u8>)>,
    kit_tags: BTreeMap<Id, Hex<32>>,
}

impl Roster {
    pub fn account(&self) -> Id {
        self.account
    }

    pub fn genesis(&self) -> Digest32 {
        self.genesis
    }

    /// `(seq, hash)` of the last applied entry.
    pub fn head(&self) -> Option<(u64, Digest32)> {
        self.head
    }

    pub fn epoch(&self) -> u32 {
        self.epoch
    }

    pub fn members(&self) -> &BTreeMap<Id, Member> {
        &self.members
    }

    /// A member left; the next device to see it starts a new epoch.
    pub fn rotation_pending(&self) -> bool {
        self.rotation_pending
    }

    /// The hash the next entry must name as `prev`.
    pub fn parent(&self) -> Digest32 {
        self.head.map(|(_, hash)| hash).unwrap_or_default()
    }

    /// 60 bits of the head hash as 12 characters, for people to compare two
    /// devices' views once both have caught up.
    pub fn fingerprint(&self) -> String {
        const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
        let bits = u64::from_be_bytes(self.parent().0[..8].try_into().expect("8 bytes"));
        (0..12)
            .map(|i| ALPHABET[((bits >> (59 - 5 * i)) & 31) as usize] as char)
            .collect()
    }

    /// Applies the next entry after checking every rule. No rule reads a
    /// clock, so replaying history to a restored relay passes them again.
    pub fn apply(&mut self, entry: &Entry) -> Result<(), RosterError> {
        let body = entry.parse()?;
        if body.v != 1 {
            return Err(Invalid("unknown version"));
        }
        let genesis = matches!(body.kind, Kind::Genesis(_));
        match self.head {
            None if body.seq != 0 || !genesis || body.prev != Digest32::default() => {
                return Err(RosterError::NotNext);
            }
            Some(_) if genesis => return Err(Invalid("an account has one genesis")),
            Some((seq, hash)) if body.seq != seq + 1 || body.prev != hash => {
                return Err(RosterError::NotNext);
            }
            Some(_) if body.account != self.account => return Err(Invalid("another account")),
            _ => {}
        }
        // The key that must have signed: a new device's own (proof of
        // possession), otherwise a current device's.
        let signer = match &body.kind {
            Kind::Genesis(g) => g.keys,
            Kind::AddDevice(a) => a.keys,
            _ => self.device(&body.author)?.keys,
        };
        if signer.id() != body.author || !signer.verifies(ROSTER, &entry.body, &entry.sig) {
            return Err(Invalid("bad signature"));
        }
        let mut next = self.clone();
        next.change(&body)?;
        next.head = Some((body.seq, entry.hash()));
        if body.seq == 0 {
            next.genesis = entry.hash();
        }
        *self = next;
        Ok(())
    }

    fn device(&self, id: &Id) -> Result<&Member, RosterError> {
        self.members
            .get(id)
            .filter(|m| m.role == Role::Device)
            .ok_or(Invalid("author is not a device of the account"))
    }

    fn count(&self, role: Role) -> usize {
        self.members.values().filter(|m| m.role == role).count()
    }

    fn admit(&mut self, keys: MemberKeys, member: Member) -> Result<(), RosterError> {
        let id = keys.id();
        if self.ancestry.contains_key(&id) {
            return Err(Invalid("that member id already existed"));
        }
        let limit = match member.role {
            Role::Device => MAX_DEVICES,
            Role::Kit => MAX_KITS,
        };
        if self.count(member.role) >= limit {
            return Err(Invalid("too many members"));
        }
        let admission = Admission {
            role: member.role,
            sponsor: member.sponsor,
            seq: member.added_seq,
            at_ms: member.added_ms,
            passkey: member.passkey,
        };
        self.ancestry.insert(id, admission);
        self.members.insert(id, member);
        Ok(())
    }

    fn change(&mut self, body: &Body) -> Result<(), RosterError> {
        let member = |role, keys, sponsor, name, form| Member {
            role,
            keys,
            sponsor,
            added_seq: body.seq,
            added_ms: body.at_ms,
            name,
            form,
            passkey: false,
        };
        match &body.kind {
            Kind::Genesis(g) => {
                self.account = body.account;
                let name = Some(g.name.clone());
                let form = g.form.clone();
                self.admit(g.keys, member(Role::Device, g.keys, None, name, form))
            }
            Kind::AddDevice(a) => {
                let sponsor = self
                    .members
                    .get(&a.sponsor.by)
                    .ok_or(Invalid("sponsor is not a member"))?;
                // Still the head the sponsor saw (`prev` was checked against
                // it), at the epoch it saw.
                let message = sponsor_message(&self.account, &body.prev, self.epoch, &a.keys);
                if !sponsor.keys.verifies(SPONSOR, &message, &a.sponsor.sig) {
                    return Err(Invalid("sponsor statement is not for this roster"));
                }
                let name = Some(a.name.clone());
                let form = a.form.clone();
                self.admit(
                    a.keys,
                    member(Role::Device, a.keys, Some(a.sponsor.by), name, form),
                )
            }
            Kind::AddKit(k) => {
                self.device(&body.author)?;
                if k.boxed.to != k.keys.id() {
                    return Err(Invalid("kit box is for someone else"));
                }
                let kit = Member {
                    passkey: k.kind == KitKind::Passkey,
                    ..member(Role::Kit, k.keys, Some(body.author), k.label.clone(), None)
                };
                self.admit(k.keys, kit)?;
                self.kit_boxes
                    .insert(k.keys.id(), (self.epoch, k.boxed.sealed.clone()));
                self.kit_tags.insert(k.keys.id(), k.id_tag);
                Ok(())
            }
            Kind::Revoke(r) => {
                let dropped: BTreeSet<Id> = r.dropped.iter().copied().collect();
                if dropped.len() != r.dropped.len() || dropped.contains(&body.author) {
                    return Err(Invalid("bad drop list"));
                }
                if !dropped.iter().all(|id| self.members.contains_key(id)) {
                    return Err(Invalid("dropping someone who is not a member"));
                }
                if self.epoch.checked_add(1) != Some(r.new_epoch) {
                    return Err(Invalid("epoch must move by one"));
                }
                let boxes: BTreeMap<Id, Vec<u8>> =
                    r.boxes.iter().map(|b| (b.to, b.sealed.clone())).collect();
                let left: BTreeSet<Id> = self
                    .members
                    .keys()
                    .filter(|id| !dropped.contains(id))
                    .copied()
                    .collect();
                if boxes.len() != r.boxes.len()
                    || boxes.keys().copied().collect::<BTreeSet<_>>() != left
                {
                    return Err(Invalid("boxes must cover exactly the members left"));
                }
                // A kit goes with the device that added it (dropped or gone),
                // a passkey kit unless a device that joined through it stays.
                let orphan = left
                    .iter()
                    .any(|id| self.members[id].role == Role::Kit && !self.kit_stays(id, &left));
                if orphan {
                    return Err(Invalid("a kit outlives the device that added it"));
                }
                for id in &dropped {
                    self.members.remove(id);
                    self.kit_boxes.remove(id);
                    self.kit_tags.remove(id);
                }
                self.epoch = r.new_epoch;
                self.rotation_pending = false;
                let rotation = Rotation {
                    prev_blob: r.prev_blob.clone(),
                    boxes,
                    at_ms: body.at_ms,
                };
                self.rotations.insert(r.new_epoch, rotation);
                Ok(())
            }
            Kind::Leave(_) => {
                self.members.remove(&body.author);
                self.rotation_pending = true;
                Ok(())
            }
        }
    }

    /// Reporting `lost`: the current devices it sponsored, transitively
    /// (through kits it added, and through members already gone), for the
    /// user to untick what they still hold. Neither `lost` nor `author` is
    /// listed; `lost` is always dropped.
    pub fn descendants(&self, lost: &Id, author: &Id) -> Vec<Id> {
        let mut found = BTreeSet::from([*lost]);
        loop {
            let more: Vec<Id> = self
                .ancestry
                .iter()
                .filter(|(id, a)| {
                    !found.contains(id) && a.sponsor.is_some_and(|s| found.contains(&s))
                })
                .map(|(id, _)| *id)
                .collect();
            if more.is_empty() {
                break;
            }
            found.extend(more);
        }
        found
            .into_iter()
            .filter(|id| {
                id != lost
                    && id != author
                    && self.members.get(id).is_some_and(|m| m.role == Role::Device)
            })
            .collect()
    }

    /// The kits that must go when `devices` are dropped: every kit whose
    /// device is among them or already gone (a kit shown on a stolen device
    /// may sit in its screenshots). A revoke that keeps one is refused.
    pub fn kits_of(&self, devices: &[Id]) -> Vec<Id> {
        let stays: BTreeSet<Id> = self
            .members
            .keys()
            .filter(|id| !devices.contains(id))
            .copied()
            .collect();
        self.members
            .iter()
            .filter(|(id, m)| m.role == Role::Kit && !self.kit_stays(id, &stays))
            .map(|(id, _)| *id)
            .collect()
    }

    /// Whether `kit` may stay when exactly `left` stay (3.2): a recovery kit
    /// while the device that added it stays; a passkey kit also while a
    /// device that joined through it stays (the new phone that joined with
    /// it keeps it when the old one signs out).
    fn kit_stays(&self, kit: &Id, left: &BTreeSet<Id>) -> bool {
        let Some(m) = self.members.get(kit) else {
            return false;
        };
        let joined_through = || {
            self.members.iter().any(|(id, d)| {
                d.role == Role::Device && d.sponsor == Some(*kit) && left.contains(id)
            })
        };
        m.sponsor.is_some_and(|s| left.contains(&s)) || (m.passkey && joined_through())
    }

    /// Every passkey kit of the account: Report lost drops them all,
    /// whoever added them (a stolen phone may hold any synced passkey).
    pub fn passkey_kits(&self) -> Vec<Id> {
        self.members
            .iter()
            .filter(|(_, m)| m.role == Role::Kit && m.passkey)
            .map(|(id, _)| *id)
            .collect()
    }

    /// The current devices that joined through a passkey kit (gone or not),
    /// newest first, never `author`: Report lost lists them ticked.
    pub fn passkey_joined(&self, author: &Id) -> Vec<Id> {
        let mut joined: Vec<(&Id, &Member)> = self
            .members
            .iter()
            .filter(|(id, m)| {
                *id != author && m.role == Role::Device && self.joined_with_passkey(id)
            })
            .collect();
        joined.sort_by_key(|(_, m)| std::cmp::Reverse(m.added_seq));
        joined.into_iter().map(|(id, _)| *id).collect()
    }

    /// The newest device a kit vouched for (gone or not), and when: "Last
    /// used" on its row, so a dead passkey is easy to tell at the cap.
    pub fn last_used(&self, kit: &Id) -> Option<(Id, u64)> {
        self.ancestry
            .iter()
            .filter(|(_, a)| a.role == Role::Device && a.sponsor == Some(*kit))
            .max_by_key(|(_, a)| a.seq)
            .map(|(id, a)| (*id, a.at_ms))
    }

    /// Whether `id` joined through a passkey kit.
    pub fn joined_with_passkey(&self, id: &Id) -> bool {
        self.ancestry
            .get(id)
            .and_then(|a| a.sponsor)
            .and_then(|s| self.ancestry.get(&s))
            .is_some_and(|a| a.role == Role::Kit && a.passkey)
    }

    fn body(&self, author: &MemberSecret, kind: Kind, at_ms: u64) -> Body {
        let (seq, prev) = self
            .head
            .map_or((0, Digest32::default()), |(seq, hash)| (seq + 1, hash));
        Body {
            v: 1,
            account: self.account,
            seq,
            prev,
            author: author.id(),
            at_ms,
            kind,
        }
    }

    /// The first entry of a new account, and the account's first bundle.
    pub fn genesis_entry(
        me: &MemberSecret,
        account: Id,
        name: &str,
        at_ms: u64,
    ) -> (Entry, Bundle) {
        Self::genesis_entry_as(me, account, name, None, at_ms)
    }

    /// As `genesis_entry`, with the first device's form (one of `FORMS`).
    pub fn genesis_entry_as(
        me: &MemberSecret,
        account: Id,
        name: &str,
        form: Option<&str>,
        at_ms: u64,
    ) -> (Entry, Bundle) {
        let mut bundle = Bundle {
            v: 1,
            account,
            genesis: Digest32::default(),
            epoch: 0,
            ak: Hex(random()),
            id_key: Hex(random()),
        };
        let body = Body {
            v: 1,
            account,
            seq: 0,
            prev: Digest32::default(),
            author: me.id(),
            at_ms,
            kind: Kind::Genesis(Genesis {
                keys: me.public,
                name: seal_name(&bundle, &me.id(), name),
                form: form.map(|f| seal_form(&bundle, &me.id(), f)),
            }),
        };
        let entry = Entry::sign(me, &body);
        bundle.genesis = entry.hash();
        (entry, bundle)
    }

    /// The new device's own entry, carrying the statement its sponsor made.
    pub fn add_device(
        &self,
        me: &MemberSecret,
        sponsor: Sponsor,
        name: String,
        at_ms: u64,
    ) -> Entry {
        self.add_device_as(me, sponsor, name, None, at_ms)
    }

    /// As `add_device`, with the device's sealed form (`seal_form`).
    pub fn add_device_as(
        &self,
        me: &MemberSecret,
        sponsor: Sponsor,
        name: String,
        form: Option<String>,
        at_ms: u64,
    ) -> Entry {
        let kind = Kind::AddDevice(AddDevice {
            keys: me.public,
            name,
            sponsor,
            form,
        });
        Entry::sign(me, &self.body(me, kind, at_ms))
    }

    /// Adds a kit, boxing the current bundle to it.
    pub fn add_kit(
        &self,
        me: &MemberSecret,
        bundle: &Bundle,
        kit: &NewKit,
        at_ms: u64,
    ) -> Result<Entry, RosterError> {
        self.current(bundle)?;
        let kind = Kind::AddKit(AddKit {
            keys: kit.keys,
            id_tag: id_tag(&kit.tag_key, bundle),
            lookup_hash: Hex(kit.lookup_hash),
            auth_hash: Hex(kit.auth_hash),
            boxed: seal_box(&kit.keys, bundle)?,
            kind: kit.kind,
            label: kit
                .label
                .as_deref()
                .map(|l| seal_name(bundle, &kit.keys.id(), l)),
        });
        Ok(Entry::sign(me, &self.body(me, kind, at_ms)))
    }

    /// Drops `dropped` (possibly none) and starts a new epoch with a fresh
    /// key boxed to everyone left. Returns the entry and the new bundle.
    pub fn revoke(
        &self,
        me: &MemberSecret,
        held: &Bundle,
        dropped: Vec<Id>,
        at_ms: u64,
    ) -> Result<(Entry, Bundle), RosterError> {
        self.current(held)?;
        let next = Bundle {
            epoch: held
                .epoch
                .checked_add(1)
                .ok_or(Invalid("the account has used every epoch"))?,
            ak: Hex(random()),
            ..held.clone()
        };
        let boxes = self
            .members
            .iter()
            .filter(|(id, _)| !dropped.contains(id))
            .map(|(_, m)| seal_box(&m.keys, &next))
            .collect::<Result<Vec<_>, _>>()?;
        let kind = Kind::Revoke(Revoke {
            dropped,
            new_epoch: next.epoch,
            boxes,
            prev_blob: chain_seal(&held.account, next.epoch, &next.ak, &held.ak),
        });
        Ok((Entry::sign(me, &self.body(me, kind, at_ms)), next))
    }

    pub fn leave(&self, me: &MemberSecret, at_ms: u64) -> Entry {
        Entry::sign(me, &self.body(me, Kind::Leave(Leave {}), at_ms))
    }

    /// The bundle for the current epoch, given the one a device holds:
    /// opened from its own box when the roster has moved on, and checked to
    /// continue the held bundle (same account, genesis and id key, and the
    /// chain of earlier keys leads back to the held key).
    pub fn adopt(&self, me: &MemberSecret, held: &Bundle) -> Result<Bundle, RosterError> {
        self.check(held, held.epoch, held.id_key)
            .map_err(|_| Invalid("the held keys are not for this roster"))?;
        if held.epoch > self.epoch {
            return Err(Invalid("the held keys are newer than the roster"));
        }
        if held.epoch == self.epoch {
            self.current(held)?;
            return Ok(held.clone());
        }
        let sealed = self
            .rotations
            .get(&self.epoch)
            .and_then(|r| r.boxes.get(&me.id()))
            .ok_or(Invalid("no key for this member"))?;
        self.continued(me, sealed, held)
    }

    /// Opens a box of the current epoch and checks it continues `base`.
    fn continued(
        &self,
        me: &MemberSecret,
        sealed: &[u8],
        base: &Bundle,
    ) -> Result<Bundle, RosterError> {
        let newest = open_box(me, &self.account, self.epoch, sealed)?;
        self.check(&newest, self.epoch, base.id_key)?;
        if self.key_at(&newest, base.epoch)? != base.ak {
            return Err(Invalid("the new key does not continue the held one"));
        }
        Ok(newest)
    }

    /// A kit's view of the current bundle, for recovery: the kit must be a
    /// member now, its first box (from `addKit`) must carry the account's id
    /// tag, and the newest key must continue that first one.
    pub fn open_for_kit(
        &self,
        kit: &MemberSecret,
        tag_key: &[u8; 32],
    ) -> Result<Bundle, RosterError> {
        let id = kit.id();
        let (tag, (first_epoch, first_box)) = match (
            self.members.get(&id),
            self.kit_tags.get(&id),
            self.kit_boxes.get(&id),
        ) {
            (Some(m), Some(tag), Some(first)) if m.role == Role::Kit => (*tag, first),
            _ => return Err(Invalid("this kit is not a member of the account")),
        };
        let first = open_box(kit, &self.account, *first_epoch, first_box)?;
        self.check(&first, *first_epoch, first.id_key)?;
        if !same(&id_tag(tag_key, &first).0, &tag.0) {
            return Err(Invalid("the keys are not this account's"));
        }
        if *first_epoch == self.epoch {
            self.current(&first)?;
            return Ok(first);
        }
        let sealed = self
            .rotations
            .get(&self.epoch)
            .and_then(|r| r.boxes.get(&id))
            .ok_or(Invalid("no key for this kit"))?;
        self.continued(kit, sealed, &first)
    }

    fn check(&self, bundle: &Bundle, epoch: u32, id_key: Hex<32>) -> Result<(), RosterError> {
        let same = bundle.v == 1
            && bundle.account == self.account
            && bundle.genesis == self.genesis
            && bundle.epoch == epoch
            && bundle.id_key == id_key;
        same.then_some(())
            .ok_or(Invalid("the boxed keys do not match the roster"))
    }

    /// A bundle this roster's current epoch can vouch for: this account,
    /// this epoch, and (after any rotation) the key the rotation was made
    /// with, which opens its chain link. A bundle at epoch 0 has no link: it
    /// is trusted on the word of the code-confirmed approver.
    fn current(&self, bundle: &Bundle) -> Result<(), RosterError> {
        self.check(bundle, self.epoch, bundle.id_key)?;
        if self.epoch > 0 {
            self.key_at(bundle, self.epoch - 1)?;
        }
        Ok(())
    }

    /// The key of an older epoch, walking back from `newest`.
    pub fn key_at(&self, newest: &Bundle, epoch: u32) -> Result<Hex<32>, RosterError> {
        self.check(newest, newest.epoch, newest.id_key)?;
        if epoch > newest.epoch || newest.epoch > self.epoch {
            return Err(Invalid("no such epoch"));
        }
        let mut ak = newest.ak;
        for e in (epoch..newest.epoch).rev() {
            let rotation = self
                .rotations
                .get(&(e + 1))
                .ok_or(Invalid("a rotation is missing"))?;
            ak = chain_open(&self.account, e + 1, &ak, &rotation.prev_blob)?;
        }
        Ok(ak)
    }

    /// When the rotation that started `epoch` was made, by its author's
    /// clock (for the old-epoch warning only).
    pub fn rotated_at_ms(&self, epoch: u32) -> Option<u64> {
        self.rotations.get(&epoch).map(|r| r.at_ms)
    }

    /// Whether `id` was ever admitted (it may be gone now).
    pub fn admitted(&self, id: &Id) -> bool {
        self.ancestry.contains_key(id)
    }

    /// Every id admitted once and not a member now.
    pub fn gone(&self) -> Vec<Id> {
        self.ancestry
            .keys()
            .filter(|id| !self.members.contains_key(id))
            .copied()
            .collect()
    }
}

/// Constant-time comparison.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// `HMAC(kit tag key, idKey || genesis)`.
pub fn id_tag(tag_key: &[u8; 32], bundle: &Bundle) -> Hex<32> {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(tag_key).expect("any key length");
    mac.update(&bundle.id_key.0);
    mac.update(&bundle.genesis.0);
    Hex(mac.finalize().into_bytes().into())
}

fn box_aad(account: &Id, epoch: u32, to: &Id) -> Vec<u8> {
    [&account.0[..], &epoch.to_be_bytes(), &to.0].concat()
}

pub fn seal_box(to: &MemberKeys, bundle: &Bundle) -> Result<SealedBox, RosterError> {
    let plain = serde_json::to_vec(bundle).expect("bundles serialize");
    let aad = box_aad(&bundle.account, bundle.epoch, &to.id());
    let sealed =
        seal_to(&to.box_key()?, EPOCH_INFO, &aad, &plain).map_err(|_| RosterError::Open)?;
    Ok(SealedBox {
        to: to.id(),
        sealed,
    })
}

fn open_box(
    me: &MemberSecret,
    account: &Id,
    epoch: u32,
    sealed: &[u8],
) -> Result<Bundle, RosterError> {
    let aad = box_aad(account, epoch, &me.id());
    let plain = open_with(&me.boxk, EPOCH_INFO, &aad, sealed).map_err(|_| RosterError::Open)?;
    serde_json::from_slice(&plain).map_err(|_| Invalid("malformed bundle"))
}

fn derived(ikm: &Hex<32>, info: &[u8]) -> XChaCha20Poly1305 {
    let mut key = [0u8; 32];
    Hkdf::<Sha256>::new(None, &ikm.0)
        .expand(info, &mut key)
        .expect("valid length");
    XChaCha20Poly1305::new((&key).into())
}

/// A device name sealed for members under a key from the id key, which never
/// changes: any member reads it at any epoch, the relay cannot.
pub fn seal_name(bundle: &Bundle, member: &Id, name: &str) -> String {
    let nonce: [u8; NONCE] = random();
    let aad = [&bundle.account.0[..], &member.0].concat();
    let sealed = derived(&bundle.id_key, NAME_INFO)
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: name.as_bytes(),
                aad: &aad,
            },
        )
        .expect("sealing never fails");
    B64.encode([&nonce[..], &sealed].concat())
}

/// A device's form (one of `FORMS`, checked by the caller), sealed as a
/// name padded to one length: the relay cannot tell a TV from a phone.
pub fn seal_form(bundle: &Bundle, member: &Id, form: &str) -> String {
    seal_name(bundle, member, &format!("{form:<FORM_LEN$}"))
}

/// A sealed form, opened: one of `FORMS`, else none.
pub fn open_form(bundle: &Bundle, member: &Id, sealed: &str) -> Option<String> {
    let form = open_name(bundle, member, sealed)?;
    let form = form.trim_end();
    FORMS.contains(&form).then(|| form.to_owned())
}

pub fn open_name(bundle: &Bundle, member: &Id, sealed: &str) -> Option<String> {
    let blob = B64.decode(sealed).ok()?;
    let (nonce, sealed) = blob.split_at_checked(NONCE)?;
    let aad = [&bundle.account.0[..], &member.0].concat();
    let plain = derived(&bundle.id_key, NAME_INFO)
        .decrypt(
            &XNonce::try_from(nonce).ok()?,
            Payload {
                msg: sealed,
                aad: &aad,
            },
        )
        .ok()?;
    String::from_utf8(plain).ok()
}

fn chain_seal(account: &Id, new_epoch: u32, new_ak: &Hex<32>, prev_ak: &Hex<32>) -> Vec<u8> {
    let nonce: [u8; NONCE] = random();
    let aad = [&account.0[..], &new_epoch.to_be_bytes()].concat();
    let sealed = derived(new_ak, CHAIN_INFO)
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: &prev_ak.0,
                aad: &aad,
            },
        )
        .expect("sealing never fails");
    [&nonce[..], &sealed].concat()
}

fn chain_open(account: &Id, epoch: u32, ak: &Hex<32>, blob: &[u8]) -> Result<Hex<32>, RosterError> {
    if blob.len() < NONCE {
        return Err(RosterError::Open);
    }
    let (nonce, sealed) = blob.split_at(NONCE);
    let aad = [&account.0[..], &epoch.to_be_bytes()].concat();
    let plain = derived(ak, CHAIN_INFO)
        .decrypt(
            &XNonce::try_from(nonce).map_err(|_| RosterError::Open)?,
            Payload {
                msg: sealed,
                aad: &aad,
            },
        )
        .map_err(|_| RosterError::Open)?;
    plain.try_into().map(Hex).map_err(|_| RosterError::Open)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account() -> (Roster, Bundle, MemberSecret) {
        let first = MemberSecret::generate();
        let (entry, bundle) = Roster::genesis_entry(&first, Hex(random()), "phone", 1);
        let mut roster = Roster::default();
        roster.apply(&entry).unwrap();
        (roster, bundle, first)
    }

    fn join(roster: &mut Roster, sponsor: &MemberSecret) -> MemberSecret {
        let new = MemberSecret::generate();
        let statement = Sponsor::new(sponsor, roster, new.public());
        let entry = roster.add_device(&new, statement, "tv".into(), 2);
        roster.apply(&entry).unwrap();
        new
    }

    pub(super) fn new_kit(kit: &MemberSecret, tag_key: [u8; 32]) -> NewKit {
        NewKit {
            keys: *kit.public(),
            tag_key,
            lookup_hash: random(),
            auth_hash: random(),
            kind: KitKind::Recovery,
            label: None,
        }
    }

    pub(super) fn passkey(kit: &MemberSecret) -> NewKit {
        NewKit {
            kind: KitKind::Passkey,
            label: Some("Passkey, saved on the phone".into()),
            ..new_kit(kit, random())
        }
    }

    /// Re-signs `entry` after `edit` changed its body: what a member (or a
    /// relay holding a member's key) could send.
    pub(super) fn tampered(
        entry: &Entry,
        author: &MemberSecret,
        edit: impl FnOnce(&mut Body),
    ) -> Entry {
        let mut body = entry.parse().unwrap();
        edit(&mut body);
        Entry::sign(author, &body)
    }

    #[test]
    fn a_revoke_hands_the_new_key_to_everyone_left_and_no_one_else() {
        let (mut roster, held, phone) = account();
        let tv = join(&mut roster, &phone);
        let lost = join(&mut roster, &phone);
        let (entry, next) = roster.revoke(&phone, &held, vec![lost.id()], 3).unwrap();
        roster.apply(&entry).unwrap();
        assert_eq!(roster.epoch(), 1);
        assert_eq!(roster.adopt(&tv, &held).unwrap(), next);
        assert!(
            roster.adopt(&lost, &held).is_err(),
            "no box for the lost one"
        );
        assert!(!roster.members().contains_key(&lost.id()));
        assert_ne!(next.ak, held.ak);
        assert_eq!(next.id_key, held.id_key, "the id key never changes");
    }

    #[test]
    fn a_device_offline_for_two_revokes_catches_up_with_one_box() {
        let (mut roster, held, phone) = account();
        let tv = join(&mut roster, &phone);
        let (one, two) = (join(&mut roster, &phone), join(&mut roster, &phone));
        let (entry, mid) = roster.revoke(&phone, &held, vec![one.id()], 3).unwrap();
        roster.apply(&entry).unwrap();
        let (entry, newest) = roster.revoke(&phone, &mid, vec![two.id()], 4).unwrap();
        roster.apply(&entry).unwrap();
        // The TV still holds epoch 0.
        assert_eq!(roster.adopt(&tv, &held).unwrap(), newest);
        assert_eq!(roster.key_at(&newest, 1).unwrap(), mid.ak);
        assert_eq!(roster.key_at(&newest, 0).unwrap(), held.ak);
    }

    #[test]
    fn a_signature_without_the_roster_purpose_does_not_verify() {
        let (roster, held, phone) = account();
        let (entry, _) = roster.revoke(&phone, &held, vec![], 3).unwrap();
        // The same bytes signed bare, as a challenge answer would be.
        let bare = Entry {
            sig: Hex(phone.sign.sign(&entry.body).to_bytes()),
            body: entry.body.clone(),
        };
        let mut copy = roster.clone();
        assert_eq!(copy.apply(&bare), Err(Invalid("bad signature")));
        copy.apply(&entry).unwrap();
    }

    #[test]
    fn ids_are_never_reused() {
        let (mut roster, held, phone) = account();
        let tv = MemberSecret::generate();
        let statement = Sponsor::new(&phone, &roster, tv.public());
        let entry = roster.add_device(&tv, statement, "tv".into(), 2);
        roster.apply(&entry).unwrap();
        // Joining again under the same keys, before and after a drop.
        let again = roster.add_device(
            &tv,
            Sponsor::new(&phone, &roster, tv.public()),
            "tv".into(),
            3,
        );
        assert!(roster.clone().apply(&again).is_err());
        let (entry, _) = roster.revoke(&phone, &held, vec![tv.id()], 4).unwrap();
        roster.apply(&entry).unwrap();
        let back = roster.add_device(
            &tv,
            Sponsor::new(&phone, &roster, tv.public()),
            "tv".into(),
            5,
        );
        assert_eq!(
            roster.apply(&back),
            Err(Invalid("that member id already existed"))
        );
    }

    #[test]
    fn boxes_must_cover_exactly_the_members_left() {
        let (mut roster, held, phone) = account();
        let tv = join(&mut roster, &phone);
        let lost = join(&mut roster, &phone);
        let (entry, next) = roster.revoke(&phone, &held, vec![lost.id()], 3).unwrap();
        let missing = tampered(&entry, &phone, |b| {
            if let Kind::Revoke(r) = &mut b.kind {
                r.boxes.retain(|x| x.to != tv.id());
            }
        });
        assert!(roster.clone().apply(&missing).is_err());
        let extra = tampered(&entry, &phone, |b| {
            if let Kind::Revoke(r) = &mut b.kind {
                r.boxes.push(seal_box(lost.public(), &next).unwrap());
            }
        });
        assert!(roster.clone().apply(&extra).is_err());
        let twice = tampered(&entry, &phone, |b| {
            if let Kind::Revoke(r) = &mut b.kind {
                let first = r.boxes[0].clone();
                r.boxes.push(first);
            }
        });
        assert!(roster.clone().apply(&twice).is_err());
        roster.apply(&entry).unwrap();
    }

    #[test]
    fn unknown_fields_are_refused() {
        let (roster, held, phone) = account();
        let (entry, _) = roster.revoke(&phone, &held, vec![], 3).unwrap();
        let mut json: serde_json::Value = serde_json::from_slice(&entry.body).unwrap();
        json["kind"]["revoke"]["keepFor"] = serde_json::json!(1);
        let bytes = serde_json::to_vec(&json).unwrap();
        let forged = Entry {
            sig: phone.sign(ROSTER, &bytes),
            body: bytes,
        };
        assert_eq!(
            roster.clone().apply(&forged),
            Err(Invalid("malformed entry"))
        );
    }

    #[test]
    fn a_sponsor_statement_works_only_on_the_head_it_was_made_for() {
        let (mut roster, held, phone) = account();
        let tv = MemberSecret::generate();
        let statement = Sponsor::new(&phone, &roster, tv.public());
        // Anything lands in between: the approval is spent.
        let (entry, _) = roster.revoke(&phone, &held, vec![], 3).unwrap();
        roster.apply(&entry).unwrap();
        let late = roster.add_device(&tv, statement, "tv".into(), 4);
        assert_eq!(
            roster.apply(&late),
            Err(Invalid("sponsor statement is not for this roster"))
        );
        // A statement for other keys does not cover these.
        let other = MemberSecret::generate();
        let statement = Sponsor::new(&phone, &roster, other.public());
        let swapped = roster.add_device(&tv, statement, "tv".into(), 5);
        assert!(roster.apply(&swapped).is_err());
    }

    #[test]
    fn a_box_with_another_id_key_is_refused_when_opened() {
        let (mut roster, held, phone) = account();
        let tv = join(&mut roster, &phone);
        // A revoker hands out keys that would split the blind ids.
        let (entry, _) = roster.revoke(&phone, &held, vec![], 3).unwrap();
        let forged = tampered(&entry, &phone, |b| {
            if let Kind::Revoke(r) = &mut b.kind {
                let split = Bundle {
                    epoch: 1,
                    id_key: Hex(random()),
                    ak: Hex(random()),
                    ..held.clone()
                };
                r.prev_blob = chain_seal(&held.account, 1, &split.ak, &held.ak);
                r.boxes = vec![
                    seal_box(phone.public(), &split).unwrap(),
                    seal_box(tv.public(), &split).unwrap(),
                ];
            }
        });
        roster.apply(&forged).unwrap();
        assert_eq!(
            roster.adopt(&tv, &held),
            Err(Invalid("the boxed keys do not match the roster"))
        );
    }

    #[test]
    fn a_new_key_that_does_not_continue_the_held_one_is_refused() {
        let (mut roster, held, phone) = account();
        let tv = join(&mut roster, &phone);
        let (entry, next) = roster.revoke(&phone, &held, vec![], 3).unwrap();
        let forged = tampered(&entry, &phone, |b| {
            if let Kind::Revoke(r) = &mut b.kind {
                r.prev_blob = chain_seal(&held.account, 1, &next.ak, &Hex(random()));
            }
        });
        roster.apply(&forged).unwrap();
        assert_eq!(
            roster.adopt(&tv, &held),
            Err(Invalid("the new key does not continue the held one"))
        );
    }

    #[test]
    fn a_kit_opens_the_newest_key_of_its_own_account_only() {
        let (mut roster, held, phone) = account();
        let kit = MemberSecret::generate();
        let tag_key: [u8; 32] = random();
        let entry = roster
            .add_kit(&phone, &held, &new_kit(&kit, tag_key), 2)
            .unwrap();
        roster.apply(&entry).unwrap();
        assert_eq!(roster.open_for_kit(&kit, &tag_key).unwrap(), held);
        assert!(
            roster.open_for_kit(&kit, &random()).is_err(),
            "wrong tag key"
        );
        let (entry, next) = roster.revoke(&phone, &held, vec![], 3).unwrap();
        roster.apply(&entry).unwrap();
        assert_eq!(roster.open_for_kit(&kit, &tag_key).unwrap(), next);
        let (entry, _) = roster.revoke(&phone, &next, vec![kit.id()], 4).unwrap();
        roster.apply(&entry).unwrap();
        assert!(roster.open_for_kit(&kit, &tag_key).is_err(), "dropped");
    }

    #[test]
    fn only_devices_that_stay_author_entries() {
        let (mut roster, held, phone) = account();
        let kit = MemberSecret::generate();
        let entry = roster
            .add_kit(&phone, &held, &new_kit(&kit, random()), 2)
            .unwrap();
        roster.apply(&entry).unwrap();
        let (by_kit, _) = roster.revoke(&kit, &held, vec![phone.id()], 3).unwrap();
        assert!(roster.clone().apply(&by_kit).is_err());
        let (itself, _) = roster.revoke(&phone, &held, vec![phone.id()], 3).unwrap();
        assert!(roster.clone().apply(&itself).is_err());
        let (stranger, _) = roster
            .revoke(&phone, &held, vec![MemberSecret::generate().id()], 3)
            .unwrap();
        assert!(roster.clone().apply(&stranger).is_err());
        let outsider = MemberSecret::generate();
        let (by_outsider, _) = roster.revoke(&outsider, &held, vec![], 3).unwrap();
        assert!(roster.apply(&by_outsider).is_err());
    }

    #[test]
    fn a_leave_makes_rotation_pending_until_a_revoke() {
        let (mut roster, held, phone) = account();
        let tv = join(&mut roster, &phone);
        roster.apply(&roster.leave(&tv, 3)).unwrap();
        assert!(roster.rotation_pending());
        assert!(!roster.members().contains_key(&tv.id()));
        let (entry, _) = roster.revoke(&phone, &held, vec![], 4).unwrap();
        roster.apply(&entry).unwrap();
        assert!(!roster.rotation_pending());
    }

    #[test]
    fn entries_follow_the_head_of_their_own_account() {
        let (mut roster, held, phone) = account();
        let (entry, _) = roster.revoke(&phone, &held, vec![], 3).unwrap();
        roster.apply(&entry).unwrap();
        assert_eq!(
            roster.apply(&entry),
            Err(RosterError::NotNext),
            "applied twice"
        );
        let (other, other_bundle, other_phone) = account();
        let (foreign, _) = other
            .revoke(&other_phone, &other_bundle, vec![], 3)
            .unwrap();
        assert!(roster.apply(&foreign).is_err());
        let fingerprint = roster.fingerprint();
        assert_eq!(fingerprint.len(), 12);
        assert_ne!(fingerprint, other.fingerprint());
    }
}

#[cfg(test)]
mod lost {
    use super::*;

    fn kit(roster: &mut Roster, by: &MemberSecret, held: &Bundle) -> MemberSecret {
        let kit = MemberSecret::generate();
        let entry = roster
            .add_kit(by, held, &tests::new_kit(&kit, random()), 9)
            .unwrap();
        roster.apply(&entry).unwrap();
        kit
    }

    fn sponsored(roster: &mut Roster, sponsor: &MemberSecret) -> MemberSecret {
        let new = MemberSecret::generate();
        let entry = roster.add_device(
            &new,
            Sponsor::new(sponsor, roster, new.public()),
            "d".into(),
            9,
        );
        roster.apply(&entry).unwrap();
        new
    }

    fn report(roster: &Roster, lost: &Id, author: &Id, keep: &[Id]) -> Vec<Id> {
        let mut devices: Vec<Id> = roster
            .descendants(lost, author)
            .into_iter()
            .filter(|id| !keep.contains(id))
            .collect();
        devices.push(*lost);
        let kits = roster.kits_of(&devices);
        devices.extend(kits);
        devices.sort();
        devices
    }

    #[test]
    fn a_thief_device_and_its_kit_go_with_the_stolen_phone() {
        let first = MemberSecret::generate();
        let (genesis, held) = Roster::genesis_entry(&first, Hex(random()), "phone", 1);
        let mut roster = Roster::default();
        roster.apply(&genesis).unwrap();
        let x = sponsored(&mut roster, &first);
        let y = sponsored(&mut roster, &x);
        let k = kit(&mut roster, &y, &held);
        let mut expected = vec![x.id(), y.id(), k.id()];
        expected.sort();
        assert_eq!(report(&roster, &x.id(), &first.id(), &[]), expected);
    }

    #[test]
    fn after_a_recovery_the_recovered_device_stays_and_the_kit_goes() {
        let x = MemberSecret::generate();
        let (genesis, held) = Roster::genesis_entry(&x, Hex(random()), "phone", 1);
        let mut roster = Roster::default();
        roster.apply(&genesis).unwrap();
        let tv = sponsored(&mut roster, &x);
        let k = kit(&mut roster, &x, &held);
        // Recovered on Y through K, which X made.
        let y = sponsored(&mut roster, &k);
        assert_eq!(roster.descendants(&x.id(), &y.id()), vec![tv.id()]);
        let dropped = report(&roster, &x.id(), &y.id(), &[tv.id()]);
        let mut expected = vec![x.id(), k.id()];
        expected.sort();
        assert_eq!(dropped, expected, "the TV unticked, Y never listed");
        let (entry, _) = roster.revoke(&y, &held, dropped, 10).unwrap();
        roster.apply(&entry).unwrap();
        assert!(roster.members().contains_key(&y.id()));
        assert!(roster.members().contains_key(&tv.id()));
    }
}

#[cfg(test)]
mod round4 {
    use super::tests::{new_kit, tampered};
    use super::*;

    fn account() -> (Roster, Bundle, MemberSecret) {
        let first = MemberSecret::generate();
        let (entry, bundle) = Roster::genesis_entry(&first, Hex(random()), "phone", 1);
        let mut roster = Roster::default();
        roster.apply(&entry).unwrap();
        (roster, bundle, first)
    }

    fn join(roster: &mut Roster, sponsor: &MemberSecret) -> MemberSecret {
        let new = MemberSecret::generate();
        let statement = Sponsor::new(sponsor, roster, new.public());
        let entry = roster.add_device(&new, statement, "d".into(), 2);
        roster.apply(&entry).unwrap();
        new
    }

    #[test]
    fn an_account_has_one_genesis_with_no_parent() {
        let (mut roster, _, _) = account();
        let outsider = MemberSecret::generate();
        let (entry, _) = Roster::genesis_entry(&outsider, roster.account(), "phone", 2);
        let mut body = entry.parse().unwrap();
        body.seq = 1;
        body.prev = roster.parent();
        let second = Entry::sign(&outsider, &body);
        assert_eq!(
            roster.apply(&second),
            Err(Invalid("an account has one genesis"))
        );
        let (entry, _) = Roster::genesis_entry(&outsider, Hex(random()), "phone", 1);
        let mut first = entry.parse().unwrap();
        first.prev = Hex([7; 32]);
        let with_parent = Entry::sign(&outsider, &first);
        assert_eq!(
            Roster::default().apply(&with_parent),
            Err(RosterError::NotNext)
        );
    }

    #[test]
    fn descendants_are_found_through_members_already_gone() {
        let (mut roster, held, phone) = account();
        let x = join(&mut roster, &phone);
        let y = join(&mut roster, &x);
        let z = join(&mut roster, &y);
        let (entry, _) = roster.revoke(&phone, &held, vec![y.id()], 3).unwrap();
        roster.apply(&entry).unwrap();
        assert_eq!(roster.descendants(&x.id(), &phone.id()), vec![z.id()]);
    }

    #[test]
    fn a_kit_refuses_a_newest_key_that_does_not_continue_its_first() {
        let (mut roster, held, phone) = account();
        let kit = MemberSecret::generate();
        let tag_key: [u8; 32] = random();
        let entry = roster
            .add_kit(&phone, &held, &new_kit(&kit, tag_key), 2)
            .unwrap();
        roster.apply(&entry).unwrap();
        let (entry, _) = roster.revoke(&phone, &held, vec![], 3).unwrap();
        let forged = tampered(&entry, &phone, |b| {
            if let Kind::Revoke(r) = &mut b.kind {
                let unrelated = Bundle {
                    epoch: 1,
                    ak: Hex(random()),
                    ..held.clone()
                };
                for x in r.boxes.iter_mut().filter(|x| x.to == kit.id()) {
                    *x = seal_box(kit.public(), &unrelated).unwrap();
                }
            }
        });
        roster.apply(&forged).unwrap();
        assert!(roster.open_for_kit(&kit, &tag_key).is_err());
    }

    #[test]
    fn helpers_refuse_epochs_and_versions_they_cannot_vouch_for() {
        let (mut roster, held, phone) = account();
        let (entry, next) = roster.revoke(&phone, &held, vec![], 3).unwrap();
        roster.apply(&entry).unwrap();
        assert!(roster.key_at(&next, 2).is_err(), "a future epoch");
        let v2 = Bundle {
            v: 2,
            ..next.clone()
        };
        assert!(roster.adopt(&phone, &v2).is_err());
        let wrong = Bundle {
            ak: Hex(random()),
            ..next.clone()
        };
        assert!(
            roster.adopt(&phone, &wrong).is_err(),
            "not the rotation's key"
        );
        assert_eq!(roster.adopt(&phone, &next).unwrap(), next);
    }

    #[test]
    fn a_kit_goes_with_the_device_that_added_it() {
        let (mut roster, held, phone) = account();
        let tv = join(&mut roster, &phone);
        let kit = MemberSecret::generate();
        let entry = roster
            .add_kit(&tv, &held, &new_kit(&kit, random()), 3)
            .unwrap();
        roster.apply(&entry).unwrap();
        let (keeps, _) = roster.revoke(&phone, &held, vec![tv.id()], 4).unwrap();
        assert_eq!(
            roster.clone().apply(&keeps),
            Err(Invalid("a kit outlives the device that added it"))
        );
        // After the TV leaves, the rotation must drop its kit as well.
        roster.apply(&roster.leave(&tv, 5)).unwrap();
        assert_eq!(roster.kits_of(&[]), vec![kit.id()]);
        let (without, _) = roster.revoke(&phone, &held, vec![], 6).unwrap();
        assert!(roster.clone().apply(&without).is_err());
        let (with, _) = roster
            .revoke(&phone, &held, roster.kits_of(&[]), 6)
            .unwrap();
        roster.apply(&with).unwrap();
    }

    #[test]
    fn an_oversized_entry_is_refused_before_parsing() {
        let (roster, _, phone) = account();
        let tv = MemberSecret::generate();
        let sponsor = Sponsor::new(&phone, &roster, tv.public());
        let entry = roster.add_device(&tv, sponsor, "x".repeat(MAX_ENTRY), 2);
        assert_eq!(
            roster.clone().apply(&entry),
            Err(Invalid("entry too large"))
        );
    }
}

#[cfg(test)]
mod round5 {
    use super::tests::{new_kit, tampered};
    use super::*;

    fn account() -> (Roster, Bundle, MemberSecret) {
        let first = MemberSecret::generate();
        let (entry, bundle) = Roster::genesis_entry(&first, Hex(random()), "phone", 1);
        let mut roster = Roster::default();
        roster.apply(&entry).unwrap();
        (roster, bundle, first)
    }

    #[test]
    fn builders_refuse_a_bundle_the_roster_cannot_vouch_for() {
        let (mut roster, held, phone) = account();
        let (entry, next) = roster.revoke(&phone, &held, vec![], 3).unwrap();
        roster.apply(&entry).unwrap();
        let kit = new_kit(&MemberSecret::generate(), random());
        assert!(roster.revoke(&phone, &held, vec![], 4).is_err(), "stale");
        assert!(roster.add_kit(&phone, &held, &kit, 4).is_err(), "stale");
        let (_, foreign, _) = account();
        assert!(roster.revoke(&phone, &foreign, vec![], 4).is_err());
        let wrong = Bundle {
            ak: Hex(random()),
            ..next.clone()
        };
        assert!(roster.revoke(&phone, &wrong, vec![], 4).is_err());
        assert!(roster.add_kit(&phone, &wrong, &kit, 4).is_err());
        assert!(roster.revoke(&phone, &next, vec![], 4).is_ok());
        assert_eq!(roster.key_at(&next, 1).unwrap(), next.ak, "same epoch");
    }

    #[test]
    fn a_kit_added_after_a_rotation_checks_its_first_link() {
        let (mut roster, held, phone) = account();
        let (entry, next) = roster.revoke(&phone, &held, vec![], 2).unwrap();
        roster.apply(&entry).unwrap();
        let kit = MemberSecret::generate();
        let tag_key: [u8; 32] = random();
        let entry = roster
            .add_kit(&phone, &next, &new_kit(&kit, tag_key), 3)
            .unwrap();
        // A member boxes the kit a key that is not the epoch's, with a tag
        // that matches it.
        let wrong = Bundle {
            ak: Hex(random()),
            ..next.clone()
        };
        let forged = tampered(&entry, &phone, |b| {
            if let Kind::AddKit(k) = &mut b.kind {
                k.boxed = seal_box(kit.public(), &wrong).unwrap();
                k.id_tag = id_tag(&tag_key, &wrong);
            }
        });
        let mut copy = roster.clone();
        copy.apply(&forged).unwrap();
        assert!(copy.open_for_kit(&kit, &tag_key).is_err());
        roster.apply(&entry).unwrap();
        assert_eq!(roster.open_for_kit(&kit, &tag_key).unwrap(), next);
    }

    #[test]
    fn names_open_for_members_only() {
        let (_, held, phone) = account();
        let sealed = seal_name(&held, &phone.id(), "Living room TV");
        assert_eq!(
            open_name(&held, &phone.id(), &sealed).as_deref(),
            Some("Living room TV")
        );
        let stranger = MemberSecret::generate().id();
        assert_eq!(open_name(&held, &stranger, &sealed), None);
        let (_, other, _) = account();
        assert_eq!(open_name(&other, &phone.id(), &sealed), None);
    }
}

/// Passkey kits (plans/accounts.md 3.2): how long each kind of kit may stay,
/// what Report lost takes, labels and forms.
#[cfg(test)]
mod passkeys {
    use super::tests::{new_kit, passkey};
    use super::*;

    fn account() -> (Roster, Bundle, MemberSecret) {
        let phone = MemberSecret::generate();
        let (genesis, held) =
            Roster::genesis_entry_as(&phone, Hex(random()), "phone", Some("phone"), 1);
        let mut roster = Roster::default();
        roster.apply(&genesis).unwrap();
        (roster, held, phone)
    }

    fn add(
        roster: &mut Roster,
        by: &MemberSecret,
        held: &Bundle,
        made: fn(&MemberSecret) -> NewKit,
    ) -> MemberSecret {
        let kit = MemberSecret::generate();
        let entry = roster.add_kit(by, held, &made(&kit), 3).unwrap();
        roster.apply(&entry).unwrap();
        kit
    }

    fn recovery(kit: &MemberSecret) -> NewKit {
        new_kit(kit, random())
    }

    /// A device that joined through `sponsor` (a device's approval, or a kit).
    fn joined(roster: &mut Roster, sponsor: &MemberSecret) -> MemberSecret {
        let new = MemberSecret::generate();
        let statement = Sponsor::new(sponsor, roster, new.public());
        let entry = roster.add_device(&new, statement, "d".into(), 4);
        roster.apply(&entry).unwrap();
        new
    }

    #[test]
    fn a_passkey_stays_while_a_device_that_joined_through_it_stays() {
        let (mut roster, held, phone) = account();
        let key = add(&mut roster, &phone, &held, passkey);
        let new_phone = joined(&mut roster, &key);
        roster.apply(&roster.leave(&phone, 5)).unwrap();
        assert!(roster.kits_of(&[]).is_empty(), "the new phone keeps it");
        let (rotation, _) = roster.revoke(&new_phone, &held, vec![], 6).unwrap();
        roster.apply(&rotation).unwrap();
        assert!(roster.members()[&key.id()].passkey);
        // A recovery kit goes with the device that made it, whoever joined.
        let (mut roster, held, phone) = account();
        let kit = add(&mut roster, &phone, &held, recovery);
        let laptop = joined(&mut roster, &kit);
        roster.apply(&roster.leave(&phone, 5)).unwrap();
        assert_eq!(roster.kits_of(&[]), vec![kit.id()]);
        let (keeps, _) = roster.revoke(&laptop, &held, vec![], 6).unwrap();
        assert_eq!(
            roster.clone().apply(&keeps),
            Err(Invalid("a kit outlives the device that added it"))
        );
    }

    #[test]
    fn a_passkey_goes_with_its_device_when_no_one_joined_through_it() {
        let (mut roster, held, phone) = account();
        let tv = joined(&mut roster, &phone);
        let key = add(&mut roster, &tv, &held, passkey);
        assert_eq!(roster.kits_of(&[tv.id()]), vec![key.id()]);
        let (keeps, _) = roster.revoke(&phone, &held, vec![tv.id()], 5).unwrap();
        assert_eq!(
            roster.clone().apply(&keeps),
            Err(Invalid("a kit outlives the device that added it"))
        );
        // Once a device joined through it, it stays when the TV goes.
        let laptop = joined(&mut roster, &key);
        assert!(roster.kits_of(&[tv.id()]).is_empty());
        let (drops_tv, _) = roster.revoke(&phone, &held, vec![tv.id()], 6).unwrap();
        roster.apply(&drops_tv).unwrap();
        assert!(roster.members().contains_key(&key.id()));
        // And it goes when that device goes too.
        assert_eq!(roster.kits_of(&[laptop.id()]), vec![key.id()]);
    }

    #[test]
    fn report_lost_takes_every_passkey_and_lists_who_joined_through_one() {
        let (mut roster, held, phone) = account();
        let tv = joined(&mut roster, &phone);
        let mine = add(&mut roster, &phone, &held, passkey);
        let theirs = add(&mut roster, &tv, &held, passkey);
        add(&mut roster, &phone, &held, recovery);
        let desk = joined(&mut roster, &mine);
        let laptop = joined(&mut roster, &theirs);
        let mut kits = roster.passkey_kits();
        kits.sort();
        let mut expected = vec![mine.id(), theirs.id()];
        expected.sort();
        assert_eq!(kits, expected, "passkeys only, whoever added them");
        assert_eq!(
            roster.passkey_joined(&phone.id()),
            vec![laptop.id(), desk.id()],
            "newest first"
        );
        assert_eq!(
            roster.passkey_joined(&desk.id()),
            vec![laptop.id()],
            "never the device reporting"
        );
        assert!(roster.joined_with_passkey(&desk.id()) && !roster.joined_with_passkey(&tv.id()));
        assert_eq!(roster.last_used(&mine.id()), Some((desk.id(), 4)));
    }

    #[test]
    fn a_dropped_passkey_is_never_let_in_again_and_kits_stop_at_eight() {
        let (mut roster, held, phone) = account();
        let key = MemberSecret::generate();
        let first = roster.add_kit(&phone, &held, &passkey(&key), 3).unwrap();
        roster.apply(&first).unwrap();
        let (drop, held) = roster.revoke(&phone, &held, vec![key.id()], 4).unwrap();
        roster.apply(&drop).unwrap();
        let again = roster.add_kit(&phone, &held, &passkey(&key), 5).unwrap();
        assert_eq!(
            roster.clone().apply(&again),
            Err(Invalid("that member id already existed"))
        );
        for _ in 0..MAX_KITS {
            add(&mut roster, &phone, &held, passkey);
        }
        let ninth = roster
            .add_kit(&phone, &held, &passkey(&MemberSecret::generate()), 6)
            .unwrap();
        assert_eq!(roster.apply(&ninth), Err(Invalid("too many members")));
    }

    #[test]
    fn labels_and_forms_are_sealed_and_forms_look_alike() {
        let (mut roster, held, phone) = account();
        let key = add(&mut roster, &phone, &held, passkey);
        let label = roster.members()[&key.id()].name.clone().unwrap();
        assert_eq!(
            open_name(&held, &key.id(), &label).as_deref(),
            Some("Passkey, saved on the phone")
        );
        let form = roster.members()[&phone.id()].form.clone().unwrap();
        assert_eq!(
            open_form(&held, &phone.id(), &form).as_deref(),
            Some("phone")
        );
        let tv = seal_form(&held, &phone.id(), "tv");
        let computer = seal_form(&held, &phone.id(), "computer");
        assert_eq!(tv.len(), computer.len(), "the relay cannot tell them apart");
        let odd = seal_name(&held, &phone.id(), "toaster");
        assert_eq!(open_form(&held, &phone.id(), &odd), None);
    }
}

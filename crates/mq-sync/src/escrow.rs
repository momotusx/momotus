//! Escrow (a relay run with `--escrow`): the secret of one recovery kit of
//! the account kept by the relay, so a completed sign-in alone brings a new
//! device in. The device puts the kit secret sealed to the relay's escrow
//! key (HPKE), the relay keeps it sealed under that key at rest, and gives
//! it back sealed to the taker's own key after a sign-in for the account;
//! the taker then joins with `pair::recover_with`. Neither way does the
//! secret read in clear between the device and the relay (TLS may end
//! earlier, at a proxy). Nothing in the roster or the engine changes: the
//! kit is an ordinary roster member, so rotations reach it.

use crate::engine::{SyncConfig, SyncError, options, require_secure};
use crate::identity::with_token;
use crate::pair::{Kem, PrivateKey, PublicKey, kit_keys, open_with, seal_to};
use crate::roster::{Hex, Id};
use crate::store::Store;
use crate::wire::{EscrowInfo, EscrowPut, EscrowStatus, EscrowTake, EscrowTaken};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use hpke::{Deserializable, Kem as _, Serializable};
use mq_http::{HttpClient, Method};
use mq_library::hlc::HlcClock;
use sha2::{Digest, Sha256};

const PUT_INFO: &[u8] = b"marquee/escrow/put/v1";
const TAKE_INFO: &[u8] = b"marquee/escrow/take/v1";
const KEYPAIR_INFO: &[u8] = b"marquee/escrow/keypair/v1";
const REST_INFO: &[u8] = b"marquee/escrow/rest/v1";
const NONCE: usize = 24;

/// The relay's escrow key (from its identity file) and what it derives:
/// the published HPKE key pair and the key that seals entries at rest.
pub struct EscrowKey {
    secret: PrivateKey,
    public: PublicKey,
    rest: [u8; 32],
    fingerprint: String,
}

impl EscrowKey {
    pub fn new(key: &[u8; 32]) -> Self {
        let hk = Hkdf::<Sha256>::new(None, key);
        let (mut ikm, mut rest) = ([0u8; 32], [0u8; 32]);
        hk.expand(KEYPAIR_INFO, &mut ikm).expect("valid length");
        hk.expand(REST_INFO, &mut rest).expect("valid length");
        let (secret, public) = Kem::derive_keypair(&ikm);
        let fingerprint =
            hex::encode(&Sha256::digest([b"marquee/escrow-key/v1\0", &key[..]].concat())[..16]);
        Self {
            secret,
            public,
            rest,
            fingerprint,
        }
    }

    /// Names the key without revealing it, for the database to check.
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub fn public_key(&self) -> Hex<32> {
        Hex(self.public.to_bytes().into())
    }

    /// A put's kit secret, for `account`, and the kit member it signs as.
    pub fn open_put(&self, account: &Id, sealed: &[u8]) -> Option<(String, Id)> {
        let kit = open_with(&self.secret, PUT_INFO, &account.0, sealed).ok()?;
        let kit = String::from_utf8(kit).ok()?;
        let id = kit_keys(&kit).ok()?.member.id();
        Some((kit, id))
    }

    /// `kit` sealed at rest for `account` and its member id: nonce, then
    /// XChaCha20-Poly1305.
    pub fn seal_at_rest(&self, account: &Id, kit_id: &Id, kit: &str) -> Vec<u8> {
        let nonce: [u8; NONCE] = crate::engine::random();
        let cipher = XChaCha20Poly1305::new((&self.rest).into());
        let payload = Payload {
            msg: kit.as_bytes(),
            aad: &[account.0, kit_id.0].concat(),
        };
        let mut out = nonce.to_vec();
        out.extend(
            cipher
                .encrypt(&XNonce::from(nonce), payload)
                .expect("sealing in memory"),
        );
        out
    }

    pub fn open_at_rest(&self, account: &Id, kit_id: &Id, blob: &[u8]) -> Option<String> {
        let (nonce, sealed) = blob.split_at_checked(NONCE)?;
        let cipher = XChaCha20Poly1305::new((&self.rest).into());
        let payload = Payload {
            msg: sealed,
            aad: &[account.0, kit_id.0].concat(),
        };
        String::from_utf8(
            cipher
                .decrypt(&XNonce::try_from(nonce).ok()?, payload)
                .ok()?,
        )
        .ok()
    }
}

/// The kit secret sealed to a taker's key.
pub fn seal_for_taker(to: &Hex<32>, kit: &str) -> Option<Vec<u8>> {
    let public = PublicKey::from_bytes(&to.0).ok()?;
    seal_to(&public, TAKE_INFO, &[], kit.as_bytes()).ok()
}

/// Puts `kit` (a recovery kit of this device's account, as `make_kit`
/// returned it) in escrow, replacing the kit held before. Only a device of
/// the account may; the relay checks the kit is a member of its roster.
/// The relay must be the one this device joined; a token the relay lost is
/// replaced, and then the record to store comes back.
pub async fn put<'s>(
    store: impl Into<Store<'s>>,
    http: &HttpClient,
    config: &SyncConfig,
    clock: &HlcClock,
    kit: &str,
) -> Result<Option<SyncConfig>, SyncError> {
    let store = store.into();
    require_secure(&config.server)?;
    let info: EscrowInfo = http
        .get(
            &format!("{}/v1/escrow", config.server),
            &options(&config.server, ""),
        )
        .await?
        .json()?;
    let public = PublicKey::from_bytes(&info.public_key.0)
        .map_err(|_| SyncError::Pairing("the relay's escrow key is not usable".into()))?;
    let sealed = seal_to(&public, PUT_INFO, &config.keys.account.0, kit.as_bytes())?;
    let body = serde_json::to_value(EscrowPut {
        sealed: B64.encode(sealed),
    })
    .expect("plain struct");
    let (_, changed) = with_token(
        store,
        http,
        config,
        clock,
        Method::POST,
        "/v1/escrow",
        Some(body),
    )
    .await?;
    Ok(changed)
}

/// What the relay holds in escrow for this device's account, and the
/// record to store if the token was replaced.
pub async fn status<'s>(
    store: impl Into<Store<'s>>,
    http: &HttpClient,
    config: &SyncConfig,
    clock: &HlcClock,
) -> Result<(EscrowStatus, Option<SyncConfig>), SyncError> {
    let store = store.into();
    let (answer, changed) = with_token(
        store,
        http,
        config,
        clock,
        Method::GET,
        "/v1/escrow/status",
        None,
    )
    .await?;
    Ok((answer.json()?, changed))
}

/// Keeps a sign-in able to open the account; an app calls it after its
/// sync rounds. When the relay holds no kit, or one that is no longer a kit
/// of the roster (a restore, a replay), or one put before the relay's last
/// rekey, this device makes a new kit and puts it in escrow. Then it removes
/// what should not stay: a rekeyed kit, since its secret may sit in a leaked
/// copy of the database, and every recovery kit this device made that is not
/// the escrowed one (two devices replacing it at once, or a kit made by a
/// check that never reached its put): on an escrow relay every recovery kit
/// is escrow's, and they count against the account's 8. A removal rotates
/// the key. Returns the record to store when anything changed it.
pub async fn ensure<'s>(
    store: impl Into<Store<'s>>,
    http: &HttpClient,
    config: &SyncConfig,
    clock: &HlcClock,
) -> Result<Option<SyncConfig>, SyncError> {
    let store = store.into();
    let (held, changed) = status(store, http, config, clock).await?;
    let mut touched = changed.is_some();
    let mut record = changed.unwrap_or_else(|| config.clone());
    let mut escrowed = held.kit_id.filter(|_| held.current);
    let mut going = Vec::new();
    if !held.current || held.stale {
        let (kit, _, made) = crate::pair::make_kit(store, http, &record, clock).await?;
        record = put(store, http, &made, clock, &kit).await?.unwrap_or(made);
        touched = true;
        going.extend(held.kit_id.filter(|_| held.stale && held.current));
        // Another device may have put after this one: the relay names the kit kept.
        let (now, again) = status(store, http, &record, clock).await?;
        record = again.unwrap_or(record);
        escrowed = now.kit_id.filter(|_| now.current);
    }
    let devices = crate::devices(store, http, &record, clock).await?;
    if let Some(changed) = devices.record {
        (record, touched) = (changed, true);
    }
    let me = record.secret().id();
    let strays = devices
        .members
        .iter()
        .filter(|m| m.kit && !m.passkey && m.sponsor == Some(me) && Some(m.id) != escrowed);
    going.extend(strays.map(|m| m.id));
    going.sort();
    going.dedup();
    going.retain(|id| Some(*id) != escrowed);
    if going.is_empty() {
        return Ok(touched.then_some(record));
    }
    Ok(Some(
        crate::remove_devices(store, http, &record, clock, going).await?,
    ))
}

/// Spends a completed sign-in's `ticket` (one that found an account) for
/// that account's escrowed kit; join with `pair::recover_with` and
/// `Recovery::Kit`. A ticket that found no account, or an account with no
/// kit in escrow, keeps the ticket and answers 404.
pub async fn take(http: &HttpClient, server: &str, ticket: &str) -> Result<String, SyncError> {
    require_secure(server)?;
    let (secret, public) = Kem::gen_keypair();
    let request = EscrowTake {
        ticket: ticket.to_owned(),
        to: Hex(public.to_bytes().into()),
    };
    let taken: EscrowTaken = http
        .post_json(
            &format!("{server}/v1/escrow/take"),
            &request,
            &options(server, ""),
        )
        .await?
        .json()?;
    let sealed = B64
        .decode(taken.sealed)
        .map_err(|_| SyncError::Pairing("escrow answer is malformed".into()))?;
    let kit = open_with(&secret, TAKE_INFO, &[], &sealed)?;
    String::from_utf8(kit).map_err(|_| SyncError::Pairing("escrow answer is malformed".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kit_goes_through_put_rest_and_take_only_with_its_keys() {
        let relay = EscrowKey::new(&[7; 32]);
        let account = Hex([1; 16]);
        let kit = crate::pair::new_recovery_kit();
        let public = PublicKey::from_bytes(&relay.public_key().0).unwrap();
        let sealed = seal_to(&public, PUT_INFO, &account.0, kit.as_bytes()).unwrap();
        let (opened, id) = relay.open_put(&account, &sealed).unwrap();
        assert_eq!(opened, kit);
        assert_eq!(id, kit_keys(&kit).unwrap().member.id());
        // Sealed for one account, it opens for no other; another key opens nothing.
        assert!(relay.open_put(&Hex([2; 16]), &sealed).is_none());
        assert!(
            EscrowKey::new(&[8; 32])
                .open_put(&account, &sealed)
                .is_none()
        );
        let rest = relay.seal_at_rest(&account, &id, &kit);
        assert_eq!(
            relay.open_at_rest(&account, &id, &rest).as_deref(),
            Some(kit.as_str())
        );
        assert!(relay.open_at_rest(&Hex([2; 16]), &id, &rest).is_none());
        assert!(relay.open_at_rest(&account, &Hex([3; 16]), &rest).is_none());
        assert!(
            EscrowKey::new(&[8; 32])
                .open_at_rest(&account, &id, &rest)
                .is_none()
        );
        let (secret, taker) = Kem::gen_keypair();
        let back = seal_for_taker(&Hex(taker.to_bytes().into()), &kit).unwrap();
        assert_eq!(
            open_with(&secret, TAKE_INFO, &[], &back).unwrap(),
            kit.as_bytes()
        );
        // A secret that is no kit is refused at put.
        let junk = seal_to(&public, PUT_INFO, &account.0, b"not a kit").unwrap();
        assert!(relay.open_put(&account, &junk).is_none());
        assert_ne!(relay.fingerprint(), EscrowKey::new(&[8; 32]).fingerprint());
        // deploy/relay/identity.py prints this form; its self-test pins the same value.
        assert_eq!(relay.fingerprint(), "91c252dc3ff49e79e90146f3cd754b40");
    }
}

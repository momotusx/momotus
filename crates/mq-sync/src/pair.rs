//! Adding a device and recovering an account, with no password and nothing
//! the relay can read or forge (plan: "Phase 2: pairing and recovery").
//!
//! Pairing: the new device opens a slot and shows a QR with the slot id and
//! its X25519 public key; a device of the account seals the key bundle and a
//! six-digit code to that key with HPKE (RFC 9180) and shows the code. The
//! new device collects the blob with a take secret that never left it, and
//! saves nothing until the user confirms both screens show the same code, so
//! someone else who saw the QR cannot slip in their own account.
//!
//! Recovery: a 160-bit kit derives a lookup id, a redeem secret and an
//! X25519 key pair. The relay keeps the bundle sealed to the kit's public
//! key, so any device can reseal a new bundle after a key rotation, and
//! hands it back (with a new device token) only for the redeem secret,
//! which no device keeps.

use crate::engine::{SyncConfig, SyncError, require_secure};
use crate::seal::KeyBundle;
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD as B64, URL_SAFE_NO_PAD as B64URL};
use hkdf::Hkdf;
use hpke::aead::ChaCha20Poly1305;
use hpke::kdf::HkdfSha256;
use hpke::kem::X25519HkdfSha256;
use hpke::{Deserializable, Kem as _, OpModeR, OpModeS, Serializable};
use mq_http::{HttpClient, Priority, RequestOptions};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::Duration;

type Kem = X25519HkdfSha256;
type PrivateKey = <Kem as hpke::Kem>::PrivateKey;
type PublicKey = <Kem as hpke::Kem>::PublicKey;
const PAIR_INFO: &[u8] = b"marquee/pair/v1";
const RECOVERY_INFO: &[u8] = b"marquee/recovery/v1";
/// X25519 encapsulated key length, in front of the HPKE ciphertext.
const ENC: usize = 32;

/// What a device needs to sync with an account; goes to the keystore.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Joined {
    pub server: String,
    pub device_token: String,
    pub keys: KeyBundle,
}

impl std::fmt::Debug for Joined {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Joined")
            .field("server", &self.server)
            .finish_non_exhaustive()
    }
}

impl Joined {
    pub fn config(&self) -> SyncConfig {
        SyncConfig {
            server: self.server.clone(),
            device_token: self.device_token.clone(),
            keys: self.keys.clone(),
        }
    }
}

/// A pairing a device of the account approved: save `joined` only after
/// the user confirms `code` matches the one on that device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub joined: Joined,
    pub code: String,
}

/// Sealed inside a pairing blob.
#[derive(Serialize, Deserialize)]
struct PairPayload {
    server: String,
    keys: KeyBundle,
    code: String,
}

fn options(server: &str, token: Option<&str>) -> RequestOptions {
    let allow_private = url::Url::parse(server)
        .ok()
        .and_then(|u| u.host_str().map(mq_http::is_private_host))
        .unwrap_or(false);
    RequestOptions {
        priority: Priority::Addon,
        timeout: Duration::from_secs(20),
        max_bytes: 1024 * 1024,
        allow_private,
        headers: token
            .map(|t| vec![("authorization".to_owned(), format!("Bearer {t}"))])
            .unwrap_or_default(),
    }
}

fn bad(why: &str) -> SyncError {
    SyncError::Pairing(why.to_owned())
}

fn seal_to(
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

fn open_with(
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
    secret: PrivateKey,
}

/// Opens a pairing slot on the relay and returns the QR to show.
pub async fn start_pairing(http: &HttpClient, server: &str) -> Result<PairingRequest, SyncError> {
    require_secure(server)?;
    let slot: Slot = http
        .post_json(
            &format!("{server}/v1/pairings"),
            &serde_json::json!({}),
            &options(server, None),
        )
        .await?
        .json()?;
    let (secret, public) = Kem::gen_keypair();
    let mut qr = url::Url::parse("marquee:pair").expect("static url");
    qr.query_pairs_mut()
        .append_pair("v", "1")
        .append_pair("s", server)
        .append_pair("p", &slot.pairing)
        .append_pair("k", &B64URL.encode(public.to_bytes()));
    Ok(PairingRequest {
        qr: qr.to_string(),
        expires_ms: slot.expires_ms,
        server: server.to_owned(),
        pairing: slot.pairing,
        take: slot.take,
        secret,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Taken {
    sealed: String,
    device_token: String,
}

impl PairingRequest {
    /// `None` while no device has approved yet.
    pub async fn poll(&self, http: &HttpClient) -> Result<Option<Offer>, SyncError> {
        let response = http
            .post_json(
                &format!("{}/v1/pairings/{}/take", self.server, self.pairing),
                &serde_json::json!({ "take": self.take }),
                &options(&self.server, None),
            )
            .await?;
        if response.status == 204 {
            return Ok(None);
        }
        let taken: Taken = response.json()?;
        let blob = B64
            .decode(&taken.sealed)
            .map_err(|_| bad("pairing reply is not base64"))?;
        let plain = open_with(&self.secret, PAIR_INFO, self.pairing.as_bytes(), &blob)?;
        let payload: PairPayload =
            serde_json::from_slice(&plain).map_err(|_| bad("pairing reply is malformed"))?;
        if payload.server != self.server {
            return Err(bad("the reply names another sync server"));
        }
        Ok(Some(Offer {
            joined: Joined {
                server: self.server.clone(),
                device_token: taken.device_token,
                keys: payload.keys,
            },
            code: payload.code,
        }))
    }
}

/// On a device that already syncs: hands the account to the device whose QR
/// was scanned. Returns the code to show, which the new device must match.
pub async fn approve_pairing(
    http: &HttpClient,
    config: &SyncConfig,
    qr: &str,
) -> Result<String, SyncError> {
    let url = url::Url::parse(qr).map_err(|_| bad("not a pairing code"))?;
    let field = |name: &str| {
        url.query_pairs()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.into_owned())
    };
    if url.scheme() != "marquee" || url.path() != "pair" || field("v").as_deref() != Some("1") {
        return Err(bad("not a pairing code"));
    }
    let pairing = field("p").ok_or_else(|| bad("pairing code has no slot"))?;
    if field("s").as_deref() != Some(config.server.as_str()) {
        return Err(bad("the code is for another sync server"));
    }
    let public = field("k")
        .and_then(|k| B64URL.decode(k).ok())
        .and_then(|k| PublicKey::from_bytes(&k).ok())
        .ok_or_else(|| bad("pairing code has no valid key"))?;
    let code = six_digits();
    let payload = PairPayload {
        server: config.server.clone(),
        keys: config.keys.clone(),
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
            &options(&config.server, Some(&config.device_token)),
        )
        .await;
    match filled {
        Ok(_) => Ok(code),
        Err(mq_http::HttpError::Status { status: 409, .. }) => {
            Err(bad("this pairing code was already used or has expired"))
        }
        Err(e) => Err(e.into()),
    }
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

/// What a kit derives: lookup id (hex), redeem secret and key pair.
struct KitKeys {
    lookup: String,
    auth: [u8; 32],
    secret: PrivateKey,
    public: PublicKey,
}

fn kit_keys(kit: &str) -> Result<KitKeys, SyncError> {
    let secret = unbase32(kit)
        .filter(|s| s.len() == 20)
        .ok_or_else(|| bad("not a recovery kit"))?;
    let hk = Hkdf::<Sha256>::new(None, &secret);
    let mut lookup = [0u8; 16];
    let mut auth = [0u8; 32];
    let mut ikm = [0u8; 32];
    hk.expand(b"marquee/recovery/lookup/v1", &mut lookup)
        .expect("valid length");
    hk.expand(b"marquee/recovery/auth/v1", &mut auth)
        .expect("valid length");
    hk.expand(b"marquee/recovery/key/v1", &mut ikm)
        .expect("valid length");
    let (secret, public) = Kem::derive_keypair(&ikm);
    Ok(KitKeys {
        lookup: hex::encode(lookup),
        auth,
        secret,
        public,
    })
}

/// Registers a new kit for the account with the bundle sealed to it.
pub async fn store_recovery(
    http: &HttpClient,
    config: &SyncConfig,
    kit: &str,
) -> Result<(), SyncError> {
    let keys = kit_keys(kit)?;
    let plain = serde_json::to_vec(&config.keys).expect("plain struct");
    let sealed = seal_to(&keys.public, RECOVERY_INFO, keys.lookup.as_bytes(), &plain)?;
    http.post_json(
        &format!("{}/v1/recovery", config.server),
        &serde_json::json!({
            "lookup": keys.lookup,
            "authHash": hex::encode(Sha256::digest(keys.auth)),
            "pk": B64.encode(keys.public.to_bytes()),
            "sealed": B64.encode(sealed),
        }),
        &options(&config.server, Some(&config.device_token)),
    )
    .await?;
    Ok(())
}

#[derive(Deserialize)]
struct KitList {
    kits: Vec<KitRef>,
}

#[derive(Deserialize)]
struct KitRef {
    lookup: String,
    pk: String,
}

/// Seals the current bundle to every kit of the account (after a key
/// rotation); needs no kit, only the public keys the relay lists.
pub async fn reseal_recovery(http: &HttpClient, config: &SyncConfig) -> Result<u32, SyncError> {
    let list: KitList = http
        .get(
            &format!("{}/v1/recovery", config.server),
            &options(&config.server, Some(&config.device_token)),
        )
        .await?
        .json()?;
    let plain = serde_json::to_vec(&config.keys).expect("plain struct");
    let mut resealed = 0;
    for kit in list.kits {
        let public = B64
            .decode(&kit.pk)
            .ok()
            .and_then(|k| PublicKey::from_bytes(&k).ok())
            .ok_or_else(|| bad("the relay listed a bad kit key"))?;
        let sealed = seal_to(&public, RECOVERY_INFO, kit.lookup.as_bytes(), &plain)?;
        http.request(
            mq_http::Method::PUT,
            &format!("{}/v1/recovery/reseal", config.server),
            Some(mq_http::Body::Json(serde_json::json!({
                "lookup": kit.lookup,
                "sealed": B64.encode(sealed),
            }))),
            &options(&config.server, Some(&config.device_token)),
        )
        .await?;
        resealed += 1;
    }
    Ok(resealed)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Redeemed {
    device_token: String,
    sealed: String,
}

/// Joins the account a recovery kit belongs to, as a new device.
pub async fn recover(http: &HttpClient, server: &str, kit: &str) -> Result<Joined, SyncError> {
    require_secure(server)?;
    let keys = kit_keys(kit)?;
    let redeemed: Redeemed = http
        .post_json(
            &format!("{server}/v1/recovery/redeem"),
            &serde_json::json!({ "lookup": keys.lookup, "auth": hex::encode(keys.auth) }),
            &options(server, None),
        )
        .await?
        .json()?;
    let blob = B64
        .decode(&redeemed.sealed)
        .map_err(|_| bad("recovery data is not base64"))?;
    let plain = open_with(&keys.secret, RECOVERY_INFO, keys.lookup.as_bytes(), &blob)?;
    let bundle: KeyBundle =
        serde_json::from_slice(&plain).map_err(|_| bad("recovery data is malformed"))?;
    Ok(Joined {
        server: server.to_owned(),
        device_token: redeemed.device_token,
        keys: bundle,
    })
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
        assert_eq!(a.public.to_bytes(), b.public.to_bytes());
        assert!(kit_keys("ABCD-EFGH").is_err());
        assert_eq!(base32(b"foobar"), "MZXW6YTBOI");
        assert_eq!(unbase32("MZXW6YTBOI").unwrap(), b"foobar");
        assert_eq!(six_digits().len(), 6);
    }
}

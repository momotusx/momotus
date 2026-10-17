//! Adding a device and recovering an account, with no password and nothing
//! the relay can read or forge (plan: "Phase 2: pairing and recovery").
//!
//! Pairing: the new device shows a QR with a one-time slot and its X25519
//! public key; a device that already syncs seals `{server, token, keys}` to
//! that key with HPKE (RFC 9180) and drops it in the slot. The key never
//! passes through the relay, so the relay cannot seal a substitute.
//!
//! Recovery: a 160-bit kit derives a lookup id and a wrap key; the relay
//! keeps the bundle sealed under the wrap key and hands it back, with a new
//! device token, to whoever presents the lookup id.

use crate::engine::{SyncConfig, SyncError, add_device, require_secure};
use crate::seal::KeyBundle;
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD as B64, URL_SAFE_NO_PAD as B64URL};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use hpke::aead::ChaCha20Poly1305;
use hpke::kdf::HkdfSha256;
use hpke::kem::X25519HkdfSha256;
use hpke::{Deserializable, Kem as _, OpModeR, OpModeS, Serializable};
use mq_http::{HttpClient, Priority, RequestOptions};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::time::Duration;

type Kem = X25519HkdfSha256;
const INFO: &[u8] = b"marquee/pair/v1";
/// X25519 encapsulated key length, in front of the HPKE ciphertext.
const ENC: usize = 32;

/// What a new device needs to join an account; goes to the keystore.
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

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Slot {
    pairing: String,
    expires_ms: u64,
}

#[derive(Serialize, Deserialize)]
struct Sealed {
    sealed: String,
}

/// A pairing in progress on the new device.
pub struct PairingRequest {
    /// Text for the QR code.
    pub qr: String,
    pub expires_ms: u64,
    server: String,
    pairing: String,
    secret: <Kem as hpke::Kem>::PrivateKey,
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
        secret,
    })
}

impl PairingRequest {
    /// `None` while the other device has not approved yet.
    pub async fn poll(&self, http: &HttpClient) -> Result<Option<Joined>, SyncError> {
        let response = http
            .get(
                &format!("{}/v1/pairings/{}", self.server, self.pairing),
                &options(&self.server, None),
            )
            .await?;
        if response.status == 204 {
            return Ok(None);
        }
        let sealed: Sealed = response.json()?;
        let blob = B64
            .decode(&sealed.sealed)
            .map_err(|_| bad("pairing reply is not base64"))?;
        if blob.len() < ENC {
            return Err(bad("pairing reply is too short"));
        }
        let (enc, ciphertext) = blob.split_at(ENC);
        let enc = <Kem as hpke::Kem>::EncappedKey::from_bytes(enc)
            .map_err(|_| bad("pairing reply has a bad key"))?;
        let mut context = hpke::setup_receiver::<ChaCha20Poly1305, HkdfSha256, Kem>(
            &OpModeR::Base,
            &self.secret,
            &enc,
            INFO,
        )
        .map_err(|_| bad("pairing reply does not open"))?;
        let plain = context
            .open(ciphertext, self.pairing.as_bytes())
            .map_err(|_| bad("pairing reply does not open"))?;
        serde_json::from_slice(&plain)
            .map(Some)
            .map_err(|_| bad("pairing reply is malformed"))
    }
}

/// On a device that already syncs: hands the account to the device whose QR
/// was scanned.
pub async fn approve_pairing(
    http: &HttpClient,
    config: &SyncConfig,
    qr: &str,
) -> Result<(), SyncError> {
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
        .and_then(|k| <Kem as hpke::Kem>::PublicKey::from_bytes(&k).ok())
        .ok_or_else(|| bad("pairing code has no valid key"))?;
    let device = add_device(http, &config.server, &config.device_token).await?;
    let joined = Joined {
        server: config.server.clone(),
        device_token: device.device_token,
        keys: config.keys.clone(),
    };
    let plain = serde_json::to_vec(&joined).expect("plain struct");
    let (enc, mut context) =
        hpke::setup_sender::<ChaCha20Poly1305, HkdfSha256, Kem>(&OpModeS::Base, &public, INFO)
            .map_err(|_| bad("pairing key is not usable"))?;
    let ciphertext = context
        .seal(&plain, pairing.as_bytes())
        .map_err(|_| bad("sealing failed"))?;
    let mut blob = enc.to_bytes().to_vec();
    blob.extend_from_slice(&ciphertext);
    http.request(
        mq_http::Method::PUT,
        &format!("{}/v1/pairings/{pairing}", config.server),
        Some(mq_http::Body::Json(
            serde_json::json!({ "sealed": B64.encode(blob) }),
        )),
        &options(&config.server, Some(&config.device_token)),
    )
    .await?;
    Ok(())
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

/// Lookup id (hex) and wrap key of a kit.
fn kit_keys(kit: &str) -> Result<(String, [u8; 32]), SyncError> {
    let secret = unbase32(kit)
        .filter(|s| s.len() == 20)
        .ok_or_else(|| bad("not a recovery kit"))?;
    let hk = Hkdf::<Sha256>::new(None, &secret);
    let mut lookup = [0u8; 16];
    let mut wrap = [0u8; 32];
    hk.expand(b"marquee/recovery/lookup/v1", &mut lookup)
        .expect("valid length");
    hk.expand(b"marquee/recovery/wrap/v1", &mut wrap)
        .expect("valid length");
    Ok((hex::encode(lookup), wrap))
}

/// Keeps the key bundle on the relay, sealed under the kit. Run again
/// whenever the bundle changes.
pub async fn store_recovery(
    http: &HttpClient,
    config: &SyncConfig,
    kit: &str,
) -> Result<(), SyncError> {
    let (lookup, wrap) = kit_keys(kit)?;
    let mut nonce = [0u8; 24];
    getrandom::fill(&mut nonce).expect("os randomness");
    let plain = serde_json::to_vec(&config.keys).expect("plain struct");
    let sealed = XChaCha20Poly1305::new_from_slice(&wrap)
        .expect("32-byte key")
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: &plain,
                aad: lookup.as_bytes(),
            },
        )
        .expect("bounded buffer");
    let mut blob = nonce.to_vec();
    blob.extend_from_slice(&sealed);
    http.request(
        mq_http::Method::PUT,
        &format!("{}/v1/recovery", config.server),
        Some(mq_http::Body::Json(
            serde_json::json!({ "lookup": lookup, "sealed": B64.encode(blob) }),
        )),
        &options(&config.server, Some(&config.device_token)),
    )
    .await?;
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Recovered {
    device_token: String,
    sealed: String,
}

/// Joins the account a recovery kit belongs to, as a new device.
pub async fn recover(http: &HttpClient, server: &str, kit: &str) -> Result<Joined, SyncError> {
    require_secure(server)?;
    let (lookup, wrap) = kit_keys(kit)?;
    let found: Recovered = http
        .post_json(
            &format!("{server}/v1/recovery/{lookup}"),
            &serde_json::json!({}),
            &options(server, None),
        )
        .await?
        .json()?;
    let blob = B64
        .decode(&found.sealed)
        .map_err(|_| bad("recovery data is not base64"))?;
    if blob.len() < 24 {
        return Err(bad("recovery data is too short"));
    }
    let (nonce, sealed) = blob.split_at(24);
    let plain = XChaCha20Poly1305::new_from_slice(&wrap)
        .expect("32-byte key")
        .decrypt(
            &XNonce::try_from(nonce).map_err(|_| bad("recovery data is malformed"))?,
            Payload {
                msg: sealed,
                aad: lookup.as_bytes(),
            },
        )
        .map_err(|_| bad("the kit does not open the recovery data"))?;
    let keys: KeyBundle =
        serde_json::from_slice(&plain).map_err(|_| bad("recovery data is malformed"))?;
    Ok(Joined {
        server: server.to_owned(),
        device_token: found.device_token,
        keys,
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
    fn kits_round_trip_however_they_are_typed() {
        let kit = new_recovery_kit();
        assert_eq!(kit.len(), 32 + 7, "{kit}");
        let (lookup, wrap) = kit_keys(&kit).unwrap();
        let typed = kit.to_lowercase().replace('-', " ");
        assert_eq!(kit_keys(&typed).unwrap(), (lookup, wrap));
        assert!(kit_keys("ABCD-EFGH").is_err());
        assert_eq!(base32(b"foobar"), "MZXW6YTBOI");
        assert_eq!(unbase32("MZXW6YTBOI").unwrap(), b"foobar");
    }
}

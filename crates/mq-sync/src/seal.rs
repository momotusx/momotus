//! Record sealing. The server stores `(coll, key id, hlc, epoch, body)` and
//! can read none of the keys or values:
//!
//! - `K_enc` and `K_id` come from the 32-byte account key through HKDF-SHA256.
//! - The key id is `HMAC-SHA256(K_id, coll || 0 || key)[..16]`, so the server
//!   can upsert per register without learning which title or setting it is.
//! - The body is `nonce(24) || XChaCha20-Poly1305(PADME(value))`, with the
//!   format tag, coll, key id, hlc and key epoch bound as AAD, so a body
//!   cannot be moved to another key or replayed under another hlc.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::Sha256;

const NONCE: usize = 24;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SealError {
    #[error("record does not open with this account key")]
    Open,
    #[error("record body is malformed")]
    Malformed,
}

/// Everything a device needs to read and write the account's records. QR
/// enrollment and the recovery kit carry this bundle, not a bare key.
///
/// `ak` encrypts records and changes when a device is revoked (with
/// `epoch`); `id_key` names registers and never changes, so a rotation does
/// not rename every record on the relay.
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyBundle {
    pub v: u8,
    pub epoch: u32,
    #[serde(with = "hex32")]
    pub ak: [u8; 32],
    #[serde(with = "hex32")]
    pub id_key: [u8; 32],
}

impl std::fmt::Debug for KeyBundle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "KeyBundle(epoch {}, <redacted>)", self.epoch)
    }
}

impl KeyBundle {
    pub fn generate() -> Self {
        Self {
            v: 1,
            epoch: 0,
            ak: AccountKey::generate(),
            id_key: AccountKey::generate(),
        }
    }
}

mod hex32 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 32], D::Error> {
        let text = String::deserialize(d)?;
        hex::decode(&text)
            .ok()
            .and_then(|b| b.try_into().ok())
            .ok_or_else(|| serde::de::Error::custom("expected 64 hex digits"))
    }
}

/// The account key, held in memory only; the platform keystore owns it.
#[derive(Clone)]
pub struct AccountKey {
    enc: [u8; 32],
    id: [u8; 32],
}

impl std::fmt::Debug for AccountKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AccountKey(<redacted>)")
    }
}

impl AccountKey {
    pub fn from_bundle(bundle: &KeyBundle) -> Self {
        Self {
            enc: expand(&bundle.ak, b"marquee/sync/enc/v1"),
            id: expand(&bundle.id_key, b"marquee/sync/id/v1"),
        }
    }

    /// A fresh random account key.
    pub fn generate() -> [u8; 32] {
        let mut ak = [0u8; 32];
        getrandom::fill(&mut ak).expect("os randomness");
        ak
    }

    pub fn key_id(&self, coll: u8, key: &str) -> [u8; 16] {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.id).expect("any key length");
        mac.update(&[coll, 0]);
        mac.update(key.as_bytes());
        let full = mac.finalize().into_bytes();
        let mut out = [0u8; 16];
        out.copy_from_slice(&full[..16]);
        out
    }

    pub fn seal(
        &self,
        coll: u8,
        key_id: &[u8; 16],
        hlc: &str,
        epoch: u32,
        value: &[u8],
    ) -> Vec<u8> {
        let mut nonce = [0u8; NONCE];
        getrandom::fill(&mut nonce).expect("os randomness");
        let aad = aad(coll, key_id, hlc, epoch);
        let sealed = self
            .cipher()
            .encrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: &pad(value),
                    aad: &aad,
                },
            )
            .expect("encryption of a bounded buffer cannot fail");
        let mut body = nonce.to_vec();
        body.extend_from_slice(&sealed);
        body
    }

    pub fn open(
        &self,
        coll: u8,
        key_id: &[u8; 16],
        hlc: &str,
        epoch: u32,
        body: &[u8],
    ) -> Result<Vec<u8>, SealError> {
        if body.len() < NONCE {
            return Err(SealError::Malformed);
        }
        let (nonce, sealed) = body.split_at(NONCE);
        let aad = aad(coll, key_id, hlc, epoch);
        let padded = self
            .cipher()
            .decrypt(
                &XNonce::try_from(nonce).map_err(|_| SealError::Malformed)?,
                Payload {
                    msg: sealed,
                    aad: &aad,
                },
            )
            .map_err(|_| SealError::Open)?;
        unpad(&padded)
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new_from_slice(&self.enc).expect("32-byte key")
    }
}

fn expand(ikm: &[u8; 32], info: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    Hkdf::<Sha256>::new(None, ikm)
        .expand(info, &mut out)
        .expect("32 bytes is a valid HKDF length");
    out
}

fn aad(coll: u8, key_id: &[u8; 16], hlc: &str, epoch: u32) -> Vec<u8> {
    let mut aad = b"mq1".to_vec();
    aad.push(coll);
    aad.extend_from_slice(key_id);
    aad.extend_from_slice(hlc.as_bytes());
    aad.extend_from_slice(&epoch.to_le_bytes());
    aad
}

/// PADME (Nikitin et al., PETS 2019): rounds a length up so at most
/// O(log log n) bits of it leak, with at most 12% overhead.
pub fn padme(len: usize) -> usize {
    if len < 2 {
        return len;
    }
    let e = (usize::BITS - 1 - len.leading_zeros()) as usize;
    let s = (usize::BITS - 1 - e.leading_zeros()) as usize + 1;
    let last_bits = e - s;
    let mask = (1usize << last_bits) - 1;
    (len + mask) & !mask
}

/// `len(u32 le) || value || zeros`, padded to the PADME size.
fn pad(value: &[u8]) -> Vec<u8> {
    let framed = 4 + value.len();
    let mut out = Vec::with_capacity(padme(framed));
    out.extend_from_slice(&(value.len() as u32).to_le_bytes());
    out.extend_from_slice(value);
    out.resize(padme(framed), 0);
    out
}

fn unpad(padded: &[u8]) -> Result<Vec<u8>, SealError> {
    let len = padded
        .get(..4)
        .map(|b| u32::from_le_bytes(b.try_into().expect("4 bytes")) as usize)
        .ok_or(SealError::Malformed)?;
    padded
        .get(4..4 + len)
        .map(<[u8]>::to_vec)
        .ok_or(SealError::Malformed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HLC: &str = "000190000000000000000000000000a1";

    fn bundle(n: u8) -> KeyBundle {
        KeyBundle {
            v: 1,
            epoch: 0,
            ak: [n; 32],
            id_key: [n; 32],
        }
    }

    fn key() -> AccountKey {
        AccountKey::from_bundle(&bundle(7))
    }

    #[test]
    fn padme_matches_the_paper() {
        // Worked by hand from the paper's algorithm: E = floor(log2 L),
        // S = floor(log2 E) + 1, round L up to a multiple of 2^(E - S).
        let cases = [
            (0, 0),
            (1, 1),
            (2, 2),
            (9, 10),
            (100, 104),
            (1000, 1024),
            (1_000_000, 1_015_808),
        ];
        for (len, padded) in cases {
            assert_eq!(padme(len), padded, "padme({len})");
        }
        for len in 2..5000 {
            let p = padme(len);
            assert!(
                p >= len && (p - len) as f64 <= len as f64 * 0.12 + 1.0,
                "{len} -> {p}"
            );
        }
    }

    #[test]
    fn round_trip_and_every_bound_field_matters() {
        let k = key();
        let id = k.key_id(3, "default/tt0944947:1:1");
        let body = k.seal(3, &id, HLC, 0, b"{\"positionMs\":1}");
        assert_eq!(
            k.open(3, &id, HLC, 0, &body).unwrap(),
            b"{\"positionMs\":1}"
        );

        let other_id = k.key_id(3, "default/tt0944947:1:2");
        assert_eq!(k.open(4, &id, HLC, 0, &body), Err(SealError::Open), "coll");
        assert_eq!(
            k.open(3, &other_id, HLC, 0, &body),
            Err(SealError::Open),
            "key id"
        );
        let later = "000190000000000100000000000000a1";
        assert_eq!(k.open(3, &id, later, 0, &body), Err(SealError::Open), "hlc");
        assert_eq!(k.open(3, &id, HLC, 1, &body), Err(SealError::Open), "epoch");
        let mut flipped = body.clone();
        *flipped.last_mut().unwrap() ^= 1;
        assert_eq!(
            k.open(3, &id, HLC, 0, &flipped),
            Err(SealError::Open),
            "body"
        );
        let stranger = AccountKey::from_bundle(&bundle(8));
        assert_eq!(
            stranger.open(3, &id, HLC, 0, &body),
            Err(SealError::Open),
            "account"
        );
    }

    #[test]
    fn key_ids_are_stable_and_separate_collections() {
        let k = key();
        assert_eq!(k.key_id(2, "p/tt1"), k.key_id(2, "p/tt1"));
        assert_ne!(k.key_id(2, "p/tt1"), k.key_id(3, "p/tt1"));
        assert_ne!(
            k.key_id(2, "p/tt1"),
            AccountKey::from_bundle(&bundle(9)).key_id(2, "p/tt1")
        );
    }

    #[test]
    fn bodies_hide_length_and_never_repeat() {
        let k = key();
        let id = k.key_id(5, "p/streams");
        let a = k.seal(5, &id, HLC, 0, &[b'x'; 1000]);
        let b = k.seal(5, &id, HLC, 0, &[b'x'; 1003]);
        assert_eq!(a.len(), b.len(), "nearby lengths share a padded size");
        assert_ne!(
            k.seal(5, &id, HLC, 0, b"same"),
            k.seal(5, &id, HLC, 0, b"same")
        );
    }
}

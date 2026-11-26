//! The signed update feed (plans/auto-update.md, plans/windows-app.md 2.6): `latest.json` on the `updates` release
//! is `{"signed": "<json>", "sig": "<base64 DER ECDSA P-256 over SHA-256 of signed>"}`, written by
//! .github/scripts/update-feed.sh. Platform-neutral, so its tests run on every gate.

use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};
use p256::pkcs8::DecodePublicKey;

/// The Windows installer the feed offers.
#[derive(Debug, PartialEq)]
pub struct Offer {
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

/// Where a feed's installer may come from: this repository's releases only.
const RELEASES: &str = "https://github.com/momotusx/momotus/releases/download/";
/// No installer is larger than this; a feed that says so is refused.
const MAX_SIZE: u64 = 512 << 20;

/// The `windows-x64` offer of a feed signed by `key_pem`, when it is newer than `current`.
pub fn offer(envelope: &[u8], key_pem: &str, current: &str) -> Option<Offer> {
    let envelope: serde_json::Value = serde_json::from_slice(envelope).ok()?;
    let signed = envelope["signed"].as_str()?;
    let sig = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        envelope["sig"].as_str()?,
    )
    .ok()?;
    let key = VerifyingKey::from_public_key_pem(key_pem).ok()?;
    key.verify(signed.as_bytes(), &Signature::from_der(&sig).ok()?)
        .ok()?;
    let entry = serde_json::from_str::<serde_json::Value>(signed)
        .ok()?
        .get("windows-x64")?
        .clone();
    let offer = Offer {
        version: entry["version"].as_str()?.to_owned(),
        url: entry["url"].as_str()?.to_owned(),
        sha256: entry["sha256"].as_str()?.to_ascii_lowercase(),
        size: entry["size"].as_u64()?,
    };
    let sane = offer.url.starts_with(RELEASES)
        && offer.size > 0
        && offer.size <= MAX_SIZE
        && offer.sha256.len() == 64
        && offer.sha256.bytes().all(|b| b.is_ascii_hexdigit());
    (sane && newer(&offer.version, current)).then_some(offer)
}

/// Whether `a` (MAJOR.MINOR.PATCH) is above `b`; anything else is not newer.
pub fn newer(a: &str, b: &str) -> bool {
    let parts = |v: &str| -> Option<(u64, u64, u64)> {
        let mut it = v.split('.').map(|p| p.parse().ok());
        let parsed = (it.next()??, it.next()??, it.next()??);
        it.next().is_none().then_some(parsed)
    };
    matches!((parts(a), parts(b)), (Some(a), Some(b)) if a > b)
}

/// The SHA-256 of `bytes`, lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::SigningKey;
    use p256::ecdsa::signature::Signer;
    use p256::pkcs8::{EncodePublicKey, LineEnding};

    fn envelope(key: &SigningKey, signed: &str) -> Vec<u8> {
        let sig: Signature = key.sign(signed.as_bytes());
        let sig = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            sig.to_der().as_bytes(),
        );
        serde_json::json!({ "signed": signed, "sig": sig })
            .to_string()
            .into_bytes()
    }

    fn signed(version: &str, url: &str) -> String {
        serde_json::json!({ "windows-x64": { "version": version, "url": url, "sha256": "ab".repeat(32), "size": 9 } })
            .to_string()
    }

    const URL: &str = "https://github.com/momotusx/momotus/releases/download/v0.8.0/momotus-windows-x64-0.8.0-setup.exe";

    #[test]
    fn a_newer_signed_offer_is_taken_and_anything_else_is_not() {
        let key = SigningKey::from_slice(&[7u8; 32]).unwrap();
        let pem = key
            .verifying_key()
            .to_public_key_pem(LineEnding::LF)
            .unwrap();
        let other = SigningKey::from_slice(&[9u8; 32]).unwrap();
        let good = envelope(&key, &signed("0.8.0", URL));
        assert_eq!(
            offer(&good, &pem, "0.7.0").map(|o| o.version),
            Some("0.8.0".into())
        );
        assert_eq!(offer(&good, &pem, "0.8.0"), None, "same version");
        assert_eq!(offer(&good, &pem, "0.9.0"), None, "older");
        assert_eq!(
            offer(&envelope(&other, &signed("0.8.0", URL)), &pem, "0.7.0"),
            None,
            "another key"
        );
        let mut forged: serde_json::Value = serde_json::from_slice(&good).unwrap();
        forged["signed"] = signed("0.9.0", URL).into();
        assert_eq!(
            offer(forged.to_string().as_bytes(), &pem, "0.7.0"),
            None,
            "changed after signing"
        );
        let elsewhere = envelope(&key, &signed("0.8.0", "https://example.com/setup.exe"));
        assert_eq!(
            offer(&elsewhere, &pem, "0.7.0"),
            None,
            "not this repository's releases"
        );
    }

    #[test]
    fn versions_compare_by_number() {
        assert!(newer("0.10.0", "0.9.9"));
        assert!(newer("1.0.0", "0.99.0"));
        assert!(!newer("0.7.0", "0.7.0"));
        assert!(!newer("0.7", "0.6.0"));
        assert!(!newer("0.7.0.1", "0.6.0"));
    }

    #[test]
    fn the_shared_feed_key_parses() {
        assert!(
            VerifyingKey::from_public_key_pem(include_str!("../../shared/update-feed-key.pub.pem"))
                .is_ok()
        );
    }
}

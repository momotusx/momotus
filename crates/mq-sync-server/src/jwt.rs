//! Signed tokens of sign-in providers: JWTs checked against the provider's
//! published keys (RS256 or ES256, as the key says), and the ES256 client
//! secrets Apple asks for. Nothing here reads a clock or the network.

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD as B64, URL_SAFE_NO_PAD as B64URL};
use ring::rand::SystemRandom;
use ring::signature::{
    self, ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, RsaPublicKeyComponents, UnparsedPublicKey,
};
use serde::Deserialize;
use serde_json::Value;

/// A JWT split into its parts; nothing in it is trusted until `verifies`.
pub struct Jwt {
    alg: String,
    kid: Option<String>,
    claims: Value,
    signed: Vec<u8>,
    sig: Vec<u8>,
}

#[derive(Deserialize)]
struct Header {
    alg: String,
    #[serde(default)]
    kid: Option<String>,
}

/// One key of a JWKS document.
#[derive(Debug, Clone, Deserialize)]
pub struct Jwk {
    pub kty: String,
    #[serde(default)]
    pub kid: Option<String>,
    #[serde(default)]
    pub alg: Option<String>,
    #[serde(default)]
    pub n: Option<String>,
    #[serde(default)]
    pub e: Option<String>,
    #[serde(default)]
    pub crv: Option<String>,
    #[serde(default)]
    pub x: Option<String>,
    #[serde(default)]
    pub y: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JwkSet {
    pub keys: Vec<Jwk>,
}

impl Jwt {
    pub fn parse(token: &str) -> Option<Self> {
        let mut parts = token.split('.');
        let (head, body, sig) = (parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() {
            return None;
        }
        let header: Header = serde_json::from_slice(&B64URL.decode(head).ok()?).ok()?;
        let claims: Value = serde_json::from_slice(&B64URL.decode(body).ok()?).ok()?;
        Some(Self {
            alg: header.alg,
            kid: header.kid,
            claims,
            signed: format!("{head}.{body}").into_bytes(),
            sig: B64URL.decode(sig).ok()?,
        })
    }

    pub fn kid(&self) -> Option<&str> {
        self.kid.as_deref()
    }

    pub fn claims(&self) -> &Value {
        &self.claims
    }

    /// Whether the signature verifies under `key`, by the header's
    /// algorithm and the key's type; `none` and anything else never does.
    pub fn verifies(&self, key: &Jwk) -> bool {
        if key.alg.as_deref().is_some_and(|alg| alg != self.alg) {
            return false;
        }
        let part = |p: &Option<String>| p.as_deref().and_then(|v| B64URL.decode(v).ok());
        match (self.alg.as_str(), key.kty.as_str()) {
            ("RS256", "RSA") => {
                let (Some(n), Some(e)) = (part(&key.n), part(&key.e)) else {
                    return false;
                };
                RsaPublicKeyComponents { n, e }
                    .verify(
                        &signature::RSA_PKCS1_2048_8192_SHA256,
                        &self.signed,
                        &self.sig,
                    )
                    .is_ok()
            }
            ("ES256", "EC") if key.crv.as_deref() == Some("P-256") => {
                let (Some(x), Some(y)) = (part(&key.x), part(&key.y)) else {
                    return false;
                };
                let point = [&[4u8][..], &x, &y].concat();
                UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, point)
                    .verify(&self.signed, &self.sig)
                    .is_ok()
            }
            _ => false,
        }
    }
}

/// The key a token names in `keys`; a token without a `kid` takes the only
/// key there is.
pub fn find<'a>(keys: &'a [Jwk], kid: Option<&str>) -> Option<&'a Jwk> {
    match kid {
        Some(kid) => keys.iter().find(|k| k.kid.as_deref() == Some(kid)),
        None if keys.len() == 1 => keys.first(),
        None => None,
    }
}

/// An ES256 JWT over `claims`, signed with a PKCS#8 P-256 key.
pub fn sign_es256(pkcs8: &[u8], kid: &str, claims: &Value) -> Result<String, String> {
    let rng = SystemRandom::new();
    let key = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8, &rng)
        .map_err(|_| "not a P-256 PKCS#8 key".to_owned())?;
    let header = serde_json::json!({ "alg": "ES256", "kid": kid, "typ": "JWT" });
    let signed = format!(
        "{}.{}",
        B64URL.encode(header.to_string()),
        B64URL.encode(claims.to_string())
    );
    let sig = key
        .sign(&rng, signed.as_bytes())
        .map_err(|_| "signing failed".to_owned())?;
    Ok(format!("{signed}.{}", B64URL.encode(sig.as_ref())))
}

/// The DER inside a PEM block (an Apple `.p8` key file).
pub fn pem_der(pem: &str) -> Option<Vec<u8>> {
    let body: String = pem
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .map(str::trim)
        .collect();
    B64.decode(body).ok()
}

/// Checks the claims of a verified ID token and returns its subject: one of
/// `issuers` exactly, one of `audiences`, not expired (60 s of skew), not
/// issued in the future, and carrying exactly `nonce`.
pub fn check_claims(
    claims: &Value,
    issuers: &[String],
    audiences: &[String],
    nonce: &str,
    now_ms: u64,
) -> Result<String, &'static str> {
    let text = |key: &str| claims.get(key).and_then(Value::as_str);
    let number = |key: &str| claims.get(key).and_then(Value::as_u64);
    if !text("iss").is_some_and(|iss| issuers.iter().any(|i| i == iss)) {
        return Err("issuer");
    }
    let aud_ok = match claims.get("aud") {
        Some(Value::String(aud)) => audiences.contains(aud),
        Some(Value::Array(auds)) => auds
            .iter()
            .any(|a| a.as_str().is_some_and(|a| audiences.iter().any(|x| x == a))),
        _ => false,
    };
    if !aud_ok {
        return Err("audience");
    }
    let now = now_ms / 1000;
    if !number("exp").is_some_and(|exp| now <= exp + 60) {
        return Err("expired");
    }
    if !number("iat").is_some_and(|iat| iat <= now + 60) {
        return Err("issued in the future");
    }
    if text("nonce") != Some(nonce) {
        return Err("nonce");
    }
    match text("sub") {
        Some(sub) if !sub.is_empty() => Ok(sub.to_owned()),
        _ => Err("subject"),
    }
}

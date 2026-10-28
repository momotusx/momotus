//! Sign-in on the device (plans/accounts.md 2.3 and 3.4): a relay's
//! brokered and native flows, the identity links of an account, and the
//! cancel of a scheduled deletion. The core never runs a provider SDK: the
//! app opens the provider page (or its own native sheet) and hands the
//! result back. A sign-in yields a ticket, which only finds an account; it
//! never opens one.

use crate::engine::{
    Session, SyncConfig, SyncError, host, options, pinned, relay_info, require_secure,
};
use crate::wire::{
    CancelDeletion, IdentityLink, LinkRequest, NativeToken, StartRequest, StartResponse,
    TakeRequest, Taken, UnlinkRequest, nonce_for,
};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
use mq_http::{Body, HttpClient, HttpError, Method};
use mq_library::Library;
use mq_library::hlc::HlcClock;
use sha2::{Digest, Sha256};

/// A flow's verifier: random, kept in memory only; the relay gets its hash.
fn verifier() -> (String, String) {
    let verifier = B64URL.encode(crate::engine::random::<32>());
    let hash = hex::encode(Sha256::digest(verifier.as_bytes()));
    (verifier, hash)
}

/// The sign-in providers `server` offers (`/v1/relay`'s list; none on a
/// relay without sign-in): which buttons a sign-in screen shows.
pub async fn providers(http: &HttpClient, server: &str) -> Result<Vec<String>, SyncError> {
    require_secure(server)?;
    Ok(relay_info(http, server).await?.providers)
}

async fn start(
    http: &HttpClient,
    server: &str,
    provider: &str,
    request: &StartRequest,
) -> Result<StartResponse, SyncError> {
    require_secure(server)?;
    Ok(http
        .post_json(
            &format!("{server}/v1/auth/{provider}/start"),
            request,
            &options(server, ""),
        )
        .await?
        .json()?)
}

/// A brokered flow: open `url` in a browser; the redirect back to the
/// app carries a fragment, which `finish` takes.
pub struct BrokeredFlow {
    pub flow: String,
    pub url: String,
    /// When the relay stops taking this flow (relay time).
    pub expires_ms: u64,
    verifier: String,
    server: String,
}

impl std::fmt::Debug for BrokeredFlow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrokeredFlow").finish_non_exhaustive()
    }
}

/// Starts a brokered flow at `server` for `provider`, coming back to
/// `return_to`. `revoke`: Apple's revoke flow, whose result carries a receipt.
pub async fn start_brokered(
    http: &HttpClient,
    server: &str,
    provider: &str,
    return_to: &str,
    revoke: bool,
) -> Result<BrokeredFlow, SyncError> {
    let (verifier, verifier_hash) = verifier();
    let started = start(
        http,
        server,
        provider,
        &StartRequest {
            verifier_hash,
            purpose: if revoke { "revoke" } else { "signin" }.into(),
            return_to: Some(return_to.to_owned()),
            native: false,
            email: None,
        },
    )
    .await?;
    let url = started
        .url
        .ok_or_else(|| SyncError::Roster("the relay sent no provider page".into()))?;
    Ok(BrokeredFlow {
        flow: started.flow,
        url,
        expires_ms: started.expires_ms,
        verifier,
        server: server.to_owned(),
    })
}

/// A return's fragment fields (`flow=..&code=..&relay=..`).
fn fields(fragment: &str) -> Vec<(String, String)> {
    url::form_urlencoded::parse(fragment.trim_start_matches('#').as_bytes())
        .into_owned()
        .collect()
}

impl BrokeredFlow {
    /// Whether a return is this flow's, from this relay. Anything else (a
    /// late return of an abandoned flow, a link another app fired) is not
    /// for it, and a view ignores it.
    pub fn owns(&self, fragment: &str) -> bool {
        let fields = fields(fragment);
        let field = |key: &str| fields.iter().find(|(k, _)| k == key).map(|(_, v)| v);
        field("flow") == Some(&self.flow) && field("relay") == Some(&host(&self.server))
    }

    /// The return's fragment: only this flow's, only from this relay; the
    /// code is used here and never shown.
    pub async fn finish(&self, http: &HttpClient, fragment: &str) -> Result<Taken, SyncError> {
        let fields = fields(fragment);
        let field = |key: &str| {
            fields
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
        };
        if field("relay") != Some(host(&self.server).as_str()) {
            return Err(SyncError::OtherRelay);
        }
        if field("flow") != Some(self.flow.as_str()) {
            return Err(SyncError::WrongFlow);
        }
        match (field("code"), field("error")) {
            (_, Some("cancelled")) => Err(SyncError::Cancelled),
            (_, Some("expired")) => Err(SyncError::TicketExpired),
            (Some(code), None) => {
                let request = TakeRequest {
                    flow: self.flow.clone(),
                    verifier: self.verifier.clone(),
                    code: code.to_owned(),
                };
                taken(
                    http.post_json(
                        &format!("{}/v1/auth/take", self.server),
                        &request,
                        &options(&self.server, ""),
                    )
                    .await,
                )
            }
            _ => Err(SyncError::SignInRefused),
        }
    }
}

/// A flow's answer. Coded answers map as everywhere (`ticketExpired` for a
/// flow past its time, `busy`); an uncoded 404 (unknown, used, or not this
/// verifier and code) or 403 (a token that does not check) is a refusal.
fn taken(answer: Result<mq_http::Response, HttpError>) -> Result<Taken, SyncError> {
    match answer.map_err(SyncError::from) {
        Ok(response) => Ok(response.json()?),
        Err(SyncError::Http(HttpError::Status {
            status: 403 | 404, ..
        })) => Err(SyncError::SignInRefused),
        Err(e) => Err(e),
    }
}

/// A native sheet's flow: give the sheet `nonce`; its ID token goes to `finish`.
pub struct NativeFlow {
    pub flow: String,
    /// The provider nonce: the relay's, bound to the relay's host (2.4).
    pub nonce: String,
    /// When the relay stops taking this flow (relay time).
    pub expires_ms: u64,
    verifier: String,
    server: String,
}

impl std::fmt::Debug for NativeFlow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeFlow")
            .field("flow", &self.flow)
            .finish_non_exhaustive()
    }
}

pub async fn start_native(
    http: &HttpClient,
    server: &str,
    provider: &str,
    revoke: bool,
) -> Result<NativeFlow, SyncError> {
    let (verifier, verifier_hash) = verifier();
    let started = start(
        http,
        server,
        provider,
        &StartRequest {
            verifier_hash,
            purpose: if revoke { "revoke" } else { "signin" }.into(),
            return_to: None,
            native: true,
            email: None,
        },
    )
    .await?;
    let relay_nonce = started
        .nonce
        .as_deref()
        .and_then(|n| B64URL.decode(n).ok())
        .ok_or_else(|| SyncError::Roster("the relay sent no nonce".into()))?;
    Ok(NativeFlow {
        flow: started.flow,
        nonce: nonce_for(&relay_nonce, &host(server)),
        expires_ms: started.expires_ms,
        verifier,
        server: server.to_owned(),
    })
}

impl NativeFlow {
    /// The sheet's ID token (and, for an Apple revoke, its fresh code).
    pub async fn finish(
        &self,
        http: &HttpClient,
        id_token: &str,
        authorization_code: Option<&str>,
    ) -> Result<Taken, SyncError> {
        let request = NativeToken {
            flow: self.flow.clone(),
            verifier: self.verifier.clone(),
            id_token: id_token.to_owned(),
            authorization_code: authorization_code.map(str::to_owned),
        };
        taken(
            http.post_json(
                &format!("{}/v1/auth/token", self.server),
                &request,
                &options(&self.server, ""),
            )
            .await,
        )
    }
}

/// An email sign-in (plans/accounts-email.md): the relay mailed a code to
/// the address; `finish` takes the flow's result with it.
pub struct EmailFlow {
    pub flow: String,
    /// When the relay stops taking this flow (relay time).
    pub expires_ms: u64,
    verifier: String,
    server: String,
}

impl std::fmt::Debug for EmailFlow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmailFlow")
            .field("flow", &self.flow)
            .finish_non_exhaustive()
    }
}

/// Asks `server` to mail a sign-in code to `address`.
pub async fn start_email(
    http: &HttpClient,
    server: &str,
    address: &str,
) -> Result<EmailFlow, SyncError> {
    let email = crate::wire::normalize_email(address).ok_or(SyncError::BadAddress)?;
    let (verifier, verifier_hash) = verifier();
    let started = start(
        http,
        server,
        "email",
        &StartRequest {
            verifier_hash,
            purpose: "signin".into(),
            email: Some(email),
            ..Default::default()
        },
    )
    .await?;
    Ok(EmailFlow {
        flow: started.flow,
        expires_ms: started.expires_ms,
        verifier,
        server: server.to_owned(),
    })
}

impl EmailFlow {
    /// The code from the mail: `found` or `new` as a ticket; `WrongCode`
    /// leaves the flow open for another try.
    pub async fn finish(&self, http: &HttpClient, code: &str) -> Result<Taken, SyncError> {
        let digits: String = code.chars().filter(|c| !matches!(c, ' ' | '-')).collect();
        if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err(SyncError::BadCode);
        }
        let request = TakeRequest {
            flow: self.flow.clone(),
            verifier: self.verifier.clone(),
            code: digits,
        };
        taken(
            http.post_json(
                &format!("{}/v1/auth/take", self.server),
                &request,
                &options(&self.server, ""),
            )
            .await,
        )
    }
}

/// One call on the account. As for a deletion, the stored token goes first
/// once the relay is the one this device joined (its id, pinned to the
/// host): these calls need an active token, not an agreed roster, so a
/// device stuck on a fork can still make them. Only a token the relay does
/// not know opens a session, which signs for a fresh one: then also the
/// record to store.
async fn with_token(
    library: &Library,
    http: &HttpClient,
    config: &SyncConfig,
    clock: &HlcClock,
    method: Method,
    path: &str,
    body: Option<serde_json::Value>,
) -> Result<(mq_http::Response, Option<SyncConfig>), SyncError> {
    require_secure(&config.server)?;
    pinned(library, config, &relay_info(http, &config.server).await?)?;
    let call = |server: &str, token: &str| {
        let (url, options) = (format!("{server}{path}"), options(server, token));
        let (method, body) = (method.clone(), body.clone().map(Body::Json));
        async move { http.request(method, &url, body, &options).await }
    };
    match call(&config.server, &config.device_token).await {
        Err(HttpError::Status { status: 401, .. }) => {
            let session = Session::open(library, http, config, clock).await?;
            let answer = call(session.server(), session.token()).await?;
            Ok((answer, session.changed()))
        }
        answer => Ok((answer?, None)),
    }
}

/// The account's identity links: the provider and when, nothing else.
pub async fn identities(
    library: &Library,
    http: &HttpClient,
    config: &SyncConfig,
    clock: &HlcClock,
) -> Result<(Vec<IdentityLink>, Option<SyncConfig>), SyncError> {
    #[derive(serde::Deserialize)]
    struct Links {
        links: Vec<IdentityLink>,
    }
    let (answer, record) = with_token(
        library,
        http,
        config,
        clock,
        Method::GET,
        "/v1/identity",
        None,
    )
    .await?;
    Ok((answer.json::<Links>()?.links, record))
}

/// Links a sign-in's identity to this account. Linked elsewhere:
/// `linkedElsewhere`, unless `move_link` takes it over.
pub async fn link_identity(
    library: &Library,
    http: &HttpClient,
    config: &SyncConfig,
    clock: &HlcClock,
    ticket: &str,
    move_link: bool,
) -> Result<Option<SyncConfig>, SyncError> {
    let body = serde_json::to_value(LinkRequest {
        ticket: ticket.to_owned(),
        move_link,
    })
    .expect("plain struct");
    let (_, record) = with_token(
        library,
        http,
        config,
        clock,
        Method::POST,
        "/v1/identity",
        Some(body),
    )
    .await?;
    Ok(record)
}

/// Unlinks by link id; `receipt`: an Apple revoke's, when it ran. A link
/// already gone counts as unlinked (the relay answers so), so a retry after
/// a lost answer succeeds.
pub async fn unlink_identity(
    library: &Library,
    http: &HttpClient,
    config: &SyncConfig,
    clock: &HlcClock,
    link: i64,
    receipt: Option<&str>,
) -> Result<Option<SyncConfig>, SyncError> {
    let body = serde_json::to_value(UnlinkRequest {
        receipt: receipt.map(str::to_owned),
    })
    .expect("plain struct");
    let path = format!("/v1/identity/{link}");
    let (_, record) = with_token(
        library,
        http,
        config,
        clock,
        Method::DELETE,
        &path,
        Some(body),
    )
    .await?;
    Ok(record)
}

/// Cancels a scheduled deletion; `unlink` also removes the identity that
/// asked for it.
pub async fn cancel_deletion(
    library: &Library,
    http: &HttpClient,
    config: &SyncConfig,
    clock: &HlcClock,
    unlink: bool,
) -> Result<Option<SyncConfig>, SyncError> {
    let body = serde_json::to_value(CancelDeletion { unlink }).expect("plain struct");
    let (_, record) = with_token(
        library,
        http,
        config,
        clock,
        Method::DELETE,
        "/v1/account/delete-request",
        Some(body),
    )
    .await?;
    Ok(record)
}

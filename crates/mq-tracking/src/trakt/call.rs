//! Calls with the profile's token: one refresh on a 401, and every page of
//! a list.

use super::api::{Answer, Api, Gate};
use super::auth::{Auth, AuthError};
use serde_json::Value;
use std::future::Future;

/// A list is read until an empty page; past this many pages it is cut.
const MAX_PAGES: u32 = 2000;

/// Why a call gave nothing to use.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CallError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error("Trakt refused it ({status})")]
    Refused { status: u16, error: Option<String> },
    #[error("Trakt did not answer: {0}")]
    Transient(String),
}

impl CallError {
    /// Worth trying again later.
    pub fn transient(&self) -> bool {
        matches!(
            self,
            CallError::Transient(_) | CallError::Auth(AuthError::Transient(_))
        )
    }
}

/// Runs `call` with the current token; after a 401, once more with the
/// token the refresh gave (or another caller's newer one).
pub async fn with_token<F, Fut>(api: &Api, auth: &Auth, call: F) -> Result<Answer, AuthError>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = Answer>,
{
    let token = auth.token(api).await?;
    let answer = call(token.clone()).await;
    if matches!(answer, Answer::Refused { status: 401, .. }) {
        let token = auth.after_401(api, &token).await?;
        return Ok(call(token).await);
    }
    Ok(answer)
}

pub async fn get(api: &Api, gate: &Gate, auth: &Auth, path: &str) -> Result<Answer, AuthError> {
    with_token(api, auth, |token| async move {
        api.get(gate, path, &token).await
    })
    .await
}

pub async fn post(
    api: &Api,
    gate: &Gate,
    auth: &Auth,
    path: &str,
    body: &Value,
) -> Result<Answer, AuthError> {
    with_token(api, auth, |token| async move {
        api.post(gate, path, body, &token).await
    })
    .await
}

/// A 2xx answer's body, or why there is none.
pub fn body(answer: Result<Answer, AuthError>) -> Result<Value, CallError> {
    match answer? {
        Answer::Ok { body, .. } => Ok(body),
        Answer::Refused { status, error } => Err(CallError::Refused { status, error }),
        Answer::Transient { reason, .. } => Err(CallError::Transient(reason)),
    }
}

/// Every item of a paged list, page by page until an empty one (Trakt
/// clamps `limit` and may shrink pages, so a short page is not the end).
pub async fn get_all(
    api: &Api,
    gate: &Gate,
    auth: &Auth,
    path: &str,
    limit: u32,
) -> Result<Vec<Value>, CallError> {
    let sep = if path.contains('?') { '&' } else { '?' };
    let mut items = Vec::new();
    for page in 1..=MAX_PAGES {
        let url = format!("{path}{sep}page={page}&limit={limit}");
        match body(get(api, gate, auth, &url).await)? {
            Value::Array(list) if list.is_empty() => return Ok(items),
            Value::Array(list) => items.extend(list),
            _ => return Err(CallError::Transient("a list that is not a list".into())),
        }
    }
    Ok(items)
}

/// The signed-in user's stable id and display name, with a token that is
/// not adopted yet (sign-in).
pub async fn user(api: &Api, gate: &Gate, token: &str) -> Result<(String, String), CallError> {
    let body = match api.get(gate, "/users/settings", token).await {
        Answer::Ok { body, .. } => body,
        Answer::Refused { status, error } => return Err(CallError::Refused { status, error }),
        Answer::Transient { reason, .. } => return Err(CallError::Transient(reason)),
    };
    let user = &body["user"];
    let uuid = user["ids"]["uuid"]
        .as_str()
        .or(user["ids"]["slug"].as_str());
    match uuid {
        Some(uuid) => Ok((
            uuid.to_owned(),
            user["username"].as_str().unwrap_or(uuid).to_owned(),
        )),
        None => Err(CallError::Transient("Trakt sent no user id".into())),
    }
}

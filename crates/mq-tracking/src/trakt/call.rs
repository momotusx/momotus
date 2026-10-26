//! Calls with the profile's token: one refresh on a 401, and every page of
//! a list.

use super::api::{Answer, Api, Gate};
use super::auth::{Auth, AuthError, Bound};
use serde_json::Value;
use std::future::Future;
use tokio::time::Instant;

/// A list is read until an empty page; more pages than this is an error.
const MAX_PAGES: u32 = 1000;

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

/// Runs `call` with a token of the connection it is bound to; after a 401,
/// once more with the token the refresh gave (or another caller's newer
/// one), never another connection's. A second 401 ends the connection: the
/// user signs in again.
pub async fn with_token<F, Fut>(
    api: &Api,
    auth: &Auth,
    bound: &Bound,
    call: F,
) -> Result<Answer, AuthError>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = Answer>,
{
    let token = auth.token(api, bound).await?;
    let answer = call(token.clone()).await;
    if !matches!(answer, Answer::Refused { status: 401, .. }) {
        return Ok(answer);
    }
    let fresh = auth.after_401(api, &token, bound).await?;
    let again = call(fresh.clone()).await;
    if matches!(again, Answer::Refused { status: 401, .. }) {
        auth.reject(&fresh, bound).await;
        return Err(AuthError::Reconnect("Trakt refused a fresh token".into()));
    }
    Ok(again)
}

pub async fn get(
    api: &Api,
    gate: &Gate,
    auth: &Auth,
    bound: &Bound,
    path: &str,
) -> Result<Answer, AuthError> {
    with_token(api, auth, bound, |token| async move {
        api.get(gate, path, &token).await
    })
    .await
}

/// A write; past `deadline` it is not sent, checked as it leaves (after a
/// refresh and the pacing), and the answer is transient.
pub async fn post(
    api: &Api,
    gate: &Gate,
    auth: &Auth,
    bound: &Bound,
    path: &str,
    body: &Value,
    deadline: Option<Instant>,
) -> Result<Answer, AuthError> {
    with_token(api, auth, bound, |token| async move {
        api.post(gate, path, body, &token, deadline).await
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
/// A short page with no row not seen before ends it too (a list that is
/// not paged); a full one, or more than `MAX_PAGES` pages, is an error,
/// never a cut list.
pub async fn get_all(
    api: &Api,
    gate: &Gate,
    auth: &Auth,
    bound: &Bound,
    path: &str,
    limit: u32,
) -> Result<Vec<Value>, CallError> {
    pages(api, gate, auth, bound, path, limit, MAX_PAGES).await
}

async fn pages(
    api: &Api,
    gate: &Gate,
    auth: &Auth,
    bound: &Bound,
    path: &str,
    limit: u32,
    max_pages: u32,
) -> Result<Vec<Value>, CallError> {
    let sep = if path.contains('?') { '&' } else { '?' };
    let mut items = Vec::new();
    // Rows carry their own ids, so an equal row is the same row again.
    let mut seen = std::collections::HashSet::new();
    for page in 1..=max_pages {
        let url = format!("{path}{sep}page={page}&limit={limit}");
        let Value::Array(list) = body(get(api, gate, auth, bound, &url).await)? else {
            return Err(CallError::Transient("a list that is not a list".into()));
        };
        let (before, full) = (items.len(), list.len() as u64 >= u64::from(limit));
        items.extend(
            list.into_iter()
                .filter(|item| seen.insert(item.to_string())),
        );
        // An empty page, or one with nothing new (an unpaged list read
        // again, or the same rows turned around), ends the list. A full
        // page with nothing new means `page` is ignored but `limit` is not:
        // the rest cannot be read, and a cut list is never an answer.
        if items.len() == before {
            if full {
                return Err(CallError::Transient("a list whose pages repeat".into()));
            }
            return Ok(items);
        }
    }
    Err(CallError::Transient(format!(
        "a list longer than {max_pages} pages"
    )))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trakt::{Hosts, TraktRecord};
    use mq_http::{ClientConfig, HttpClient};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Answers every request with one more page of one new item.
    async fn endless() -> (Hosts, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let asked = Arc::new(AtomicUsize::new(0));
        let count = asked.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let n = count.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0u8; 2048];
                let _ = socket.read(&mut buf).await;
                let body = format!("[{n}]");
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            }
        });
        let hosts = Hosts {
            api: base.clone(),
            auth: base,
            local: true,
        };
        (hosts, asked)
    }

    #[tokio::test]
    async fn a_list_past_its_page_cap_is_an_error() {
        let (hosts, asked) = endless().await;
        let http = HttpClient::new(ClientConfig::default()).unwrap();
        let api = Api::new(http, hosts, "client", "Momotus/test");
        let auth = Auth::new("https://example.org/trakt");
        let now = crate::trakt::auth::now_ms();
        let record: TraktRecord = serde_json::from_value(serde_json::json!({
            "accessToken": "a", "refreshToken": "r", "createdAtMs": now,
            "expiresAtMs": now + 86_400_000, "scope": "public", "account": "u",
            "revision": 0, "generation": 1
        }))
        .unwrap();
        auth.set(Some(record)).await;
        let bound = auth.bound().await.unwrap();
        let cut = pages(&api, &Gate::default(), &auth, &bound, "/x", 1, 3).await;
        assert!(cut.unwrap_err().transient(), "never a cut list");
        assert_eq!(asked.load(Ordering::SeqCst), 3);
    }
}

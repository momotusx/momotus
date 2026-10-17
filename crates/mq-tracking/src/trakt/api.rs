//! Transport to Trakt: hosts, headers, pacing, and one reading of every
//! answer, so a Cloudflare page or a 5xx is never mistaken for "no data".

use mq_http::{HttpClient, HttpError, Method, RequestOptions, Response};
use serde_json::Value;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;

/// Trakt allows one write per second per user; a little margin.
const WRITE_GAP: Duration = Duration::from_millis(1050);
/// 500 GETs per 5 minutes per user is one per 0.6 s; stay under it.
const READ_GAP: Duration = Duration::from_millis(700);
const TIMEOUT: Duration = Duration::from_secs(15);

/// Where Trakt answers; tests point both at a local server.
#[derive(Debug, Clone)]
pub struct Hosts {
    pub api: String,
    pub auth: String,
    /// The hosts are on this machine (tests).
    pub local: bool,
}

impl Default for Hosts {
    fn default() -> Self {
        Self {
            api: "https://api.trakt.tv".into(),
            auth: "https://auth.trakt.tv".into(),
            local: false,
        }
    }
}

/// One reading of a Trakt answer.
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    /// 2xx with JSON, or `Value::Null` for an empty body.
    Ok { status: u16, body: Value },
    /// A 4xx Trakt meant: its status and the `error` field when there is one.
    Refused { status: u16, error: Option<String> },
    /// Try later: 429, 5xx, a Cloudflare page, a timeout, a dropped
    /// connection, or a 2xx that is not JSON.
    Transient {
        reason: String,
        retry_after: Option<Duration>,
    },
}

/// Pacing for one Trakt account: writes and reads each keep their gap.
#[derive(Default)]
pub struct Gate {
    write: Mutex<Option<Instant>>,
    read: Mutex<Option<Instant>>,
}

impl Gate {
    async fn wait(slot: &Mutex<Option<Instant>>, gap: Duration) {
        let mut last = slot.lock().await;
        if let Some(at) = *last {
            tokio::time::sleep_until(at + gap).await;
        }
        *last = Some(Instant::now());
    }
}

pub struct Api {
    http: HttpClient,
    hosts: Hosts,
    client_id: String,
    user_agent: String,
}

impl Api {
    pub fn new(http: HttpClient, hosts: Hosts, client_id: &str, user_agent: &str) -> Self {
        Self {
            http,
            hosts,
            client_id: client_id.to_owned(),
            user_agent: user_agent.to_owned(),
        }
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    /// A call to `auth.trakt.tv` (device flow, token, revoke): never paced
    /// behind the write queue, so a write waiting for a refresh cannot block it.
    pub async fn auth(&self, path: &str, body: &Value) -> Answer {
        let url = format!("{}{path}", self.hosts.auth);
        self.send(Method::POST, &url, Some(body), None).await
    }

    pub async fn get(&self, gate: &Gate, path: &str, token: &str) -> Answer {
        Gate::wait(&gate.read, READ_GAP).await;
        let url = format!("{}{path}", self.hosts.api);
        self.send(Method::GET, &url, None, Some(token)).await
    }

    pub async fn post(&self, gate: &Gate, path: &str, body: &Value, token: &str) -> Answer {
        Gate::wait(&gate.write, WRITE_GAP).await;
        let url = format!("{}{path}", self.hosts.api);
        self.send(Method::POST, &url, Some(body), Some(token)).await
    }

    async fn send(
        &self,
        method: Method,
        url: &str,
        body: Option<&Value>,
        token: Option<&str>,
    ) -> Answer {
        let mut headers = vec![
            ("trakt-api-version".to_owned(), "2".to_owned()),
            ("trakt-api-key".to_owned(), self.client_id.clone()),
            ("user-agent".to_owned(), self.user_agent.clone()),
        ];
        if let Some(token) = token {
            headers.push(("authorization".to_owned(), format!("Bearer {token}")));
        }
        let opts = RequestOptions {
            timeout: TIMEOUT,
            allow_private: self.hosts.local,
            headers,
            ..Default::default()
        };
        let result = match body {
            Some(body) => {
                self.http
                    .request(method, url, Some(mq_http::Body::Json(body.clone())), &opts)
                    .await
            }
            None => self.http.request(method, url, None, &opts).await,
        };
        classify(result)
    }
}

/// The transport reading shared by every endpoint; endpoints interpret the
/// statuses themselves.
pub fn classify(result: Result<Response, HttpError>) -> Answer {
    match result {
        Ok(response) => {
            if is_html(&response.headers) {
                return transient("an HTML page instead of JSON", None);
            }
            if response.body.iter().all(u8::is_ascii_whitespace) {
                return Answer::Ok {
                    status: response.status,
                    body: Value::Null,
                };
            }
            match serde_json::from_slice(&response.body) {
                Ok(body) => Answer::Ok {
                    status: response.status,
                    body,
                },
                Err(_) => transient("an answer that is not JSON", None),
            }
        }
        Err(HttpError::Status {
            status,
            body,
            retry_after,
        }) => {
            let json: Option<Value> = serde_json::from_slice(&body).ok();
            if status == 429 || status >= 500 || (json.is_none() && !body.is_empty()) {
                // A Cloudflare 403 or 429 is an HTML page with no JSON.
                return transient(&format!("status {status}"), retry_after);
            }
            Answer::Refused {
                status,
                error: json.and_then(|j| j["error"].as_str().map(str::to_owned)),
            }
        }
        Err(e) => transient(&e.to_string(), None),
    }
}

fn is_html(headers: &mq_http::HeaderMap) -> bool {
    headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("text/html"))
}

fn transient(reason: &str, retry_after: Option<Duration>) -> Answer {
    Answer::Transient {
        reason: reason.to_owned(),
        retry_after,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn writes_and_reads_keep_their_gaps() {
        let gate = Gate::default();
        let start = Instant::now();
        for _ in 0..3 {
            Gate::wait(&gate.write, WRITE_GAP).await;
        }
        assert!(start.elapsed() >= WRITE_GAP * 2, "{:?}", start.elapsed());
        let start = Instant::now();
        for _ in 0..3 {
            Gate::wait(&gate.read, READ_GAP).await;
        }
        assert!(start.elapsed() >= READ_GAP * 2);
        assert!(
            start.elapsed() < WRITE_GAP * 2,
            "reads are not held behind writes"
        );
    }
}

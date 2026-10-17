use mq_http::{Body, HttpClient, HttpError, Method, Priority, RequestOptions, Response};
use std::fmt;
use url::Url;

/// An API key or token. Debug output never shows the value.
#[derive(Clone)]
pub(crate) struct Secret(String);

impl Secret {
    pub(crate) fn new(value: impl Into<String>) -> Self {
        Secret(value.into())
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

/// Base URL + priority + the shared client; the transport every provider uses.
#[derive(Clone)]
pub(crate) struct Api {
    http: HttpClient,
    base: String,
    priority: Priority,
    allow_private: bool,
}

impl Api {
    pub(crate) fn new(http: HttpClient, base: &str) -> Self {
        let mut api = Api {
            http,
            base: String::new(),
            priority: Priority::Resolve,
            allow_private: false,
        };
        api.set_base(base);
        api
    }

    /// Points at another host (tests, a self-hosted proxy). A private-address
    /// base is allowed because the caller chose it, never an addon.
    pub(crate) fn set_base(&mut self, base: &str) {
        self.base = base.trim_end_matches('/').to_owned();
        self.allow_private = Url::parse(base)
            .ok()
            .and_then(|u| u.host_str().map(mq_http::is_private_host))
            .unwrap_or(false);
    }

    pub(crate) fn set_priority(&mut self, priority: Priority) {
        self.priority = priority;
    }

    pub(crate) fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    pub(crate) fn options(&self, headers: Vec<(String, String)>) -> RequestOptions {
        RequestOptions {
            priority: self.priority,
            allow_private: self.allow_private,
            headers,
            ..Default::default()
        }
    }

    pub(crate) async fn call(
        &self,
        method: Method,
        path: &str,
        body: Option<Body>,
        headers: Vec<(String, String)>,
    ) -> Result<Response, HttpError> {
        self.http
            .request(method, &self.url(path), body, &self.options(headers))
            .await
    }
}

pub(crate) fn bearer(token: &Secret) -> Vec<(String, String)> {
    vec![(
        "Authorization".to_owned(),
        format!("Bearer {}", token.expose()),
    )]
}

/// `path?k=v&...` with values percent-encoded.
pub(crate) fn with_query(path: &str, pairs: &[(&str, &str)]) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(pairs)
        .finish();
    format!("{path}?{query}")
}

pub(crate) fn form(pairs: &[(&str, &str)]) -> Body {
    Body::Form(
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    )
}

/// Where a store reads its credential. It is read per request, so a token
/// rotated by a refresh reaches requests built before it.
pub trait Auth: Send + Sync {
    /// The key or access token to send now.
    fn token(&self) -> String;
    /// The provider refused `used`. Returns a token to retry that one request
    /// with, or `None` when there is none and the refusal stands.
    fn refused<'a>(&'a self, used: &'a str) -> crate::BoxFuture<'a, Option<String>>;
}

/// A key that never changes: a refusal stands.
pub(crate) struct Fixed(pub(crate) Secret);

impl Auth for Fixed {
    fn token(&self) -> String {
        self.0.expose().to_owned()
    }

    fn refused<'a>(&'a self, _used: &'a str) -> crate::BoxFuture<'a, Option<String>> {
        Box::pin(async { None })
    }
}

pub(crate) fn fixed(key: impl Into<String>) -> std::sync::Arc<dyn Auth> {
    std::sync::Arc::new(Fixed(Secret::new(key)))
}

/// Runs `call` with the current token; on an auth refusal asks `auth` once
/// for a newer token and retries only this call with it.
pub(crate) async fn authed<T, F, Fut>(auth: &dyn Auth, call: F) -> Result<T, crate::DebridError>
where
    F: Fn(Secret) -> Fut,
    Fut: std::future::Future<Output = Result<T, crate::DebridError>>,
{
    let used = auth.token();
    match call(Secret::new(used.clone())).await {
        Err(crate::DebridError::Auth) => match auth.refused(&used).await {
            Some(next) => call(Secret::new(next.clone()))
                .await
                .map_err(|e| e.without(&[&used, &next])),
            None => Err(crate::DebridError::Auth),
        },
        other => other.map_err(|e| e.without(&[&used])),
    }
}

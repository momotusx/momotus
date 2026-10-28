//! Sign-in by email (plans/accounts-email.md): `start` mails a 6-digit code
//! to the address and opens a flow whose result, the address's keyed hash,
//! is already known; `take` releases it to the starter's verifier with that
//! code. The address is mailed and hashed, never stored or logged.
//!
//! Limits live in `email_event`, keyed hashes only, 24 hours: per mailbox
//! (tags and Gmail dots folded) a mail a minute, 10 a day and 20 wrong codes
//! a day; per client address 10 mails a day; `dailyLimit` relay-wide; five
//! wrong codes end a flow.

use super::{
    FLOW_MS, FLOWS_PER_CLIENT, FlowRow, Identity, MAX_FLOWS, Verified, busy, coded, fresh, is_open,
    mint_ticket, refused, sha, sweep_flows, ticket_refused,
};
use crate::{AppState, MAIL, error, limited, lock, random, same, storage_error};
use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use hmac::{Hmac, KeyInit, Mac};
use mq_http::{HttpError, RequestOptions};
use mq_library::hlc::now_ms;
use mq_sync::wire::{StartRequest, StartResponse, mailbox, normalize_email};
use rusqlite::{Connection, params};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::net::IpAddr;
use std::time::Duration;

/// Mail limits look this far back; their rows go after it.
pub(crate) const EVENT_MS: u64 = 24 * 60 * 60 * 1000;
const MAIL_GAP_MS: u64 = 60 * 1000;
const MAILS_PER_MAILBOX: i64 = 10;
const MISSES_PER_MAILBOX: i64 = 20;
const MAILS_PER_CLIENT: i64 = 10;
const MISSES_PER_FLOW: i64 = 5;

/// The identity file's `email` block: Amazon SES v2 and the sender. No
/// `Debug`: the keys must never reach a log.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EmailConfig {
    /// An IAM user's key that may only `ses:SendEmail` from `from`.
    pub access_key_id: String,
    pub secret_access_key: String,
    #[serde(default = "region")]
    pub region: String,
    /// `Name <address>` on a domain verified in SES.
    pub from: String,
    /// Mails relay-wide in any 24 hours: the abuse ceiling, not SES's quota.
    #[serde(default = "daily_limit")]
    pub daily_limit: i64,
    /// `https://email.<region>.amazonaws.com` unless set (the tests' mock).
    #[serde(default)]
    pub endpoint: Option<String>,
}

fn region() -> String {
    "us-east-1".into()
}
fn daily_limit() -> i64 {
    80
}

impl EmailConfig {
    pub(crate) fn check(&self) -> Result<(), String> {
        super::secure("the mail endpoint", &self.endpoint())?;
        let plain = |s: &str| !s.is_empty() && !s.contains(['\r', '\n']);
        let region_ok = !self.region.is_empty()
            && self
                .region
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-');
        if !(1..=1000).contains(&self.daily_limit) {
            return Err("the email block's dailyLimit must be 1 to 1000".into());
        }
        if !(plain(&self.access_key_id)
            && plain(&self.secret_access_key)
            && plain(&self.from)
            && self.from.contains('@')
            && region_ok)
        {
            return Err(
                "the email block needs accessKeyId, secretAccessKey, a region and a from address"
                    .into(),
            );
        }
        Ok(())
    }

    /// `from` as the mail names it: "Momotus" unless the file gives a name.
    fn sender(&self) -> String {
        match self.from.contains('<') {
            true => self.from.clone(),
            false => format!("Momotus <{}>", self.from),
        }
    }

    fn endpoint(&self) -> String {
        self.endpoint
            .clone()
            .unwrap_or_else(|| format!("https://email.{}.amazonaws.com", self.region))
            .trim_end_matches('/')
            .to_owned()
    }
}

fn bad_address() -> Response {
    coded(
        StatusCode::BAD_REQUEST,
        "badAddress",
        "not an email address",
    )
}

fn limit() -> Response {
    coded(
        StatusCode::TOO_MANY_REQUESTS,
        "emailLimit",
        "too many codes for this address today",
    )
}

fn unavailable() -> Response {
    coded(
        StatusCode::SERVICE_UNAVAILABLE,
        "mailUnavailable",
        "the code could not be sent",
    )
}

/// Six uniform digits from the CSPRNG.
fn six_digits() -> String {
    loop {
        let n = u32::from_le_bytes(random::<4>());
        // The largest multiple of 10^6 below 2^32: no digit is likelier.
        if n < 4_294_000_000 {
            return format!("{:06}", n % 1_000_000);
        }
    }
}

/// The code as kept: keyed, and bound to its flow.
fn code_hash(id: &Identity, flow: &str, code: &str) -> Vec<u8> {
    id.subject("email code", &format!("{flow}\0{code}"))
}

fn events(db: &Connection, kind: &str, key: &[u8], since: u64) -> rusqlite::Result<i64> {
    db.query_row(
        "SELECT COUNT(*) FROM email_event WHERE kind = ?1 AND key = ?2 AND at_ms > ?3",
        params![kind, key, since as i64],
        |r| r.get(0),
    )
}

fn event(db: &Connection, kind: &str, key: &[u8], now: u64) -> rusqlite::Result<i64> {
    db.execute(
        "INSERT INTO email_event (kind, key, at_ms) VALUES (?1, ?2, ?3)",
        params![kind, key, now as i64],
    )?;
    Ok(db.last_insert_rowid())
}

pub(crate) fn expire_events(db: &Connection, now: u64) -> rusqlite::Result<()> {
    db.execute(
        "DELETE FROM email_event WHERE at_ms <= ?1",
        params![now.saturating_sub(EVENT_MS) as i64],
    )?;
    Ok(())
}

/// The mail's subject (the same every time: a code in it reads as bulk mail
/// to Gmail) and its text and HTML parts. Only the code and the relay's
/// clock vary, nothing a caller wrote; no link, no image.
const SUBJECT: &str = "Your Momotus sign-in code";

fn mail_body(code: &str, now_ms: u64) -> (String, String) {
    let minutes = (now_ms / 60_000) % (24 * 60);
    let asked = format!("{:02}:{:02} UTC", minutes / 60, minutes % 60);
    let lines = [
        format!(
            "Asked for at {asked}. It works once, for 10 minutes, in the app or page where you asked for it."
        ),
        "Type it only in Momotus: nobody from Momotus will ever ask you for it.".to_owned(),
        "If you didn't ask for this code, ignore this email. Nobody can sign in with your address without it.".to_owned(),
    ];
    let text = format!(
        "Your Momotus sign-in code:\n\n{code}\n\n{}\n\nMomotus\n",
        lines.join("\n\n")
    );
    // Nothing here needs escaping: the code is digits, the rest fixed text.
    let paragraphs: String = lines
        .iter()
        .map(|l| format!(r#"<p style="margin:0 0 14px;">{l}</p>"#))
        .collect();
    let html = format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="color-scheme" content="light dark"><title>{SUBJECT}</title></head><body style="margin:0;padding:32px 20px;background:#FFFFFF;color:#1C1C1E;font:16px/1.5 -apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Arial,sans-serif;"><div style="max-width:480px;margin:0 auto;"><p style="margin:0 0 8px;font-size:17px;">Your Momotus sign-in code:</p><p style="margin:0 0 24px;font:600 32px/1.2 ui-monospace,Menlo,Consolas,monospace;letter-spacing:6px;">{code}</p><div style="color:#3A3A3C;">{paragraphs}</div><p style="margin:24px 0 0;color:#6C6C70;">Momotus</p></div></body></html>"#
    );
    (text, html)
}

/// Why a mail did not go: SES said no (nothing was sent), or no definite
/// answer came (it may have gone, or a throttle may lift).
enum NotSent {
    Refused,
    Unknown,
}

const SEND_PATH: &str = "/v2/email/outbound-emails";

fn hmac(key: &[u8], data: &str) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("any key length");
    mac.update(data.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

/// `YYYYMMDDTHHMMSSZ` for Unix seconds (days to a civil date by Howard
/// Hinnant's `civil_from_days`).
fn amz_date(unix: u64) -> String {
    let (days, secs) = ((unix / 86_400) as i64 + 719_468, unix % 86_400);
    let era = days.div_euclid(146_097);
    let doe = days.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    )
}

/// AWS Signature Version 4: the `Authorization` value. `headers` are the
/// signed headers, lowercase names in order, `x-amz-date` among them.
fn sigv4(
    (key_id, secret, region, service): (&str, &str, &str, &str),
    (method, path, query): (&str, &str, &str),
    headers: &[(&str, &str)],
    date: &str,
    body: &[u8],
) -> String {
    let lines: String = headers.iter().map(|(n, v)| format!("{n}:{v}\n")).collect();
    let signed = headers
        .iter()
        .map(|(n, _)| *n)
        .collect::<Vec<_>>()
        .join(";");
    let canonical = format!(
        "{method}\n{path}\n{query}\n{lines}\n{signed}\n{}",
        hex::encode(Sha256::digest(body))
    );
    let scope = format!("{}/{region}/{service}/aws4_request", &date[..8]);
    let to_sign = format!(
        "AWS4-HMAC-SHA256\n{date}\n{scope}\n{}",
        hex::encode(Sha256::digest(canonical.as_bytes()))
    );
    let mut key = hmac(format!("AWS4{secret}").as_bytes(), &date[..8]);
    for part in [region, service, "aws4_request"] {
        key = hmac(&key, part);
    }
    format!(
        "AWS4-HMAC-SHA256 Credential={key_id}/{scope}, SignedHeaders={signed}, Signature={}",
        hex::encode(hmac(&key, &to_sign))
    )
}

async fn send(
    id: &Identity,
    config: &EmailConfig,
    to: &str,
    code: &str,
    now: u64,
) -> Result<(), NotSent> {
    let (text, html) = mail_body(code, now);
    let part = |data: &str| json!({ "Data": data, "Charset": "UTF-8" });
    let body = json!({
        "FromEmailAddress": config.sender(),
        "Destination": { "ToAddresses": [to] },
        "Content": { "Simple": {
            "Subject": part(SUBJECT),
            "Body": { "Text": part(&text), "Html": part(&html) },
        } },
    });
    let endpoint = config.endpoint();
    let url = url::Url::parse(&format!("{endpoint}{SEND_PATH}")).map_err(|_| NotSent::Refused)?;
    // The signature covers the Host the request carries: the port only
    // when the URL names one.
    let host = match (url.host_str(), url.port()) {
        (Some(h), Some(p)) => format!("{h}:{p}"),
        (Some(h), None) => h.to_owned(),
        _ => return Err(NotSent::Refused),
    };
    let date = amz_date(now_ms() / 1000);
    let signed = (
        config.access_key_id.as_str(),
        config.secret_access_key.as_str(),
        config.region.as_str(),
        "ses",
    );
    // `Body::Json` sends the value's `to_string()`: the bytes signed here.
    let headers = [
        ("content-type", "application/json"),
        ("host", host.as_str()),
        ("x-amz-date", date.as_str()),
    ];
    let auth = sigv4(
        signed,
        ("POST", SEND_PATH, ""),
        &headers,
        &date,
        body.to_string().as_bytes(),
    );
    let options = RequestOptions {
        // Under the apps' own 10 s wait for the answer.
        timeout: Duration::from_secs(8),
        headers: vec![("x-amz-date".into(), date), ("authorization".into(), auth)],
        ..id.options(&endpoint)
    };
    match id.http.post_json(url.as_str(), &body, &options).await {
        Ok(_) => Ok(()),
        // SES's own message may quote the address: the status alone is logged.
        // A request timeout or a throttle may still lift or have sent.
        Err(HttpError::Status { status, .. }) if status == 408 || status == 429 => {
            tracing::warn!("the mail service did not take a code now: {status}");
            Err(NotSent::Unknown)
        }
        Err(HttpError::Status { status, .. }) if (400..500).contains(&status) => {
            // A key that no longer works stops every mail: the owner must see it.
            if status == 401 || status == 403 {
                tracing::error!("the mail service refused the relay's key or sender: {status}");
            } else {
                tracing::warn!("the mail service refused a code: {status}");
            }
            Err(NotSent::Refused)
        }
        Err(e) => {
            tracing::warn!("the mail service did not answer: {}", e.kind());
            Err(NotSent::Unknown)
        }
    }
}

/// `POST /v1/auth/email/start`: the checks, the flow and its events in one
/// locked step, then the mail. The answer never depends on whether the
/// address has an account: the identity table is not read.
pub(super) async fn start(
    state: &AppState,
    id: &Identity,
    config: &EmailConfig,
    address: IpAddr,
    request: StartRequest,
    verifier_hash: Vec<u8>,
) -> Result<Response, Response> {
    if request.purpose != "signin" || request.native || request.return_to.is_some() {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "an email flow signs in, with no return",
        ));
    }
    let to = request
        .email
        .as_deref()
        .and_then(normalize_email)
        .ok_or_else(bad_address)?;
    let subject = id.subject("email", &to);
    let mailbox = id.subject("email limit", &mailbox(&to));
    let client = id.client_tag(&address);
    let network = network(address);
    let (flow, code) = (fresh(), six_digits());
    let now = state.now_ms();
    let day = now.saturating_sub(EVENT_MS);
    // E6: the database lock, then `limits`; checks, the budget, then writes.
    let row = {
        let db = lock(state);
        let checked = (|| -> rusqlite::Result<Result<(i64, i64), Response>> {
            sweep_flows(&db, now)?;
            expire_events(&db, now)?;
            let last: Option<i64> = db.query_row(
                "SELECT MAX(at_ms) FROM email_event WHERE kind = 'mail' AND key = ?1",
                params![mailbox],
                |r| r.get(0),
            )?;
            if last.is_some_and(|at| (at as u64) + MAIL_GAP_MS > now) {
                return Ok(Err(coded(
                    StatusCode::TOO_MANY_REQUESTS,
                    "emailWait",
                    "a code went to this address a moment ago",
                )));
            }
            if events(&db, "mail", &mailbox, day)? >= MAILS_PER_MAILBOX
                || events(&db, "miss", &mailbox, day)? >= MISSES_PER_MAILBOX
                || mails_today(state, network, now) >= MAILS_PER_CLIENT
            {
                return Ok(Err(limit()));
            }
            let relay: i64 = db.query_row(
                "SELECT COUNT(*) FROM email_event WHERE kind = 'mail' AND at_ms > ?1",
                params![day as i64],
                |r| r.get(0),
            )?;
            if relay >= config.daily_limit {
                return Ok(Err(unavailable()));
            }
            let all: i64 = db.query_row(
                "SELECT COUNT(*) FROM auth_flow WHERE client <> ''",
                [],
                |r| r.get(0),
            )?;
            let mine: i64 = db.query_row(
                "SELECT COUNT(*) FROM auth_flow WHERE client = ?1",
                params![client],
                |r| r.get(0),
            )?;
            // The mail budget is spent only by a start that would send.
            if all >= MAX_FLOWS || mine >= FLOWS_PER_CLIENT || limited(state, address, MAIL) {
                return Ok(Err(busy()));
            }
            let verified = Verified {
                provider: "email".into(),
                subject: hex::encode(&subject),
                receipt: None,
                mailbox: Some(hex::encode(&mailbox)),
            };
            db.execute(
                "INSERT INTO auth_flow (id_hash, provider, purpose, verifier_hash, nonce, client, created_ms, code_hash, verified)
                 VALUES (?1, 'email', 'signin', ?2, '', ?3, ?4, ?5, ?6)",
                params![
                    sha(&flow),
                    verifier_hash,
                    client,
                    now as i64,
                    code_hash(id, &flow, &code),
                    serde_json::to_string(&verified).expect("plain")
                ],
            )?;
            Ok(Ok((event(&db, "mail", &mailbox, now)?, relay + 1)))
        })();
        let (row, sent) = checked.map_err(|e| storage_error("email start", e))??;
        count_mail(state, network, now, true);
        // The owner hears of a spike or an attack; no address, no client.
        if sent == (config.daily_limit + 1) / 2 || sent == config.daily_limit {
            tracing::warn!(
                "email sign-in sent {sent} of its {} codes in the last 24 hours",
                config.daily_limit
            );
        }
        row
    };
    let answer = Json(StartResponse {
        flow: flow.clone(),
        expires_ms: now + FLOW_MS,
        url: None,
        nonce: None,
    });
    match send(id, config, &to, &code, now).await {
        // Unclear: the mail may well arrive, so the user waits for it with
        // the flow that takes its code; everything stays counted.
        Ok(()) | Err(NotSent::Unknown) => Ok(answer.into_response()),
        Err(NotSent::Refused) => {
            // Nothing went: the flow and its counts go too, so asking again
            // is not held against the address. The row id is this start's
            // own, deliberately: nothing else holds it within the day.
            count_mail(state, network, now, false);
            let db = lock(state);
            let undone = (|| -> rusqlite::Result<()> {
                db.execute(
                    "DELETE FROM auth_flow WHERE id_hash = ?1",
                    params![sha(&flow)],
                )?;
                db.execute("DELETE FROM email_event WHERE rowid = ?1", params![row])?;
                Ok(())
            })();
            match undone {
                Ok(()) => Err(unavailable()),
                Err(e) => Err(storage_error("email start", e)),
            }
        }
    }
}

/// A client's network for the daily count: the address, or its /48 for
/// IPv6 (`client()` already cut it to a /64), which one customer often holds.
fn network(address: IpAddr) -> IpAddr {
    match address {
        IpAddr::V6(v6) => IpAddr::V6((u128::from(v6) & !((1u128 << 80) - 1)).into()),
        v4 => v4,
    }
}

/// Codes mailed to anyone from `network` in its current 24-hour window,
/// counted in memory only (E6).
fn mails_today(state: &AppState, network: IpAddr, now: u64) -> i64 {
    let limits = state.limits.lock().unwrap_or_else(|p| p.into_inner());
    match limits.daily.get(&network) {
        Some((start, n)) if now < start + EVENT_MS => i64::from(*n),
        _ => 0,
    }
}

/// One more mail from `network` (`add`), or one given back.
fn count_mail(state: &AppState, network: IpAddr, now: u64, add: bool) {
    let mut limits = state.limits.lock().unwrap_or_else(|p| p.into_inner());
    if limits.daily.len() > 50_000 {
        limits.daily.retain(|_, (start, _)| now < *start + EVENT_MS);
    }
    let slot = limits.daily.entry(network).or_insert((now, 0));
    if now >= slot.0 + EVENT_MS {
        *slot = (now, 0);
    }
    slot.1 = if add {
        slot.1 + 1
    } else {
        slot.1.saturating_sub(1)
    };
}

/// `take` of an email flow, under the lock: the flow's time, the mailbox's
/// wrong codes, then the code. Only the flow's own verifier counts anything.
pub(super) fn take(
    db: &Connection,
    id: &Identity,
    flow: &str,
    verifier: &str,
    code: &str,
    row: &FlowRow,
    now: u64,
) -> Result<Response, Response> {
    if !same(&row.verifier_hash, &sha(verifier)) {
        return Err(refused());
    }
    if !is_open(row, now) {
        return Err(ticket_refused());
    }
    let verified: Verified = row
        .verified
        .as_deref()
        .and_then(|v| serde_json::from_str(v).ok())
        .ok_or_else(refused)?;
    let (Ok(subject), Some(Ok(mailbox))) = (
        hex::decode(&verified.subject),
        verified.mailbox.as_deref().map(hex::decode),
    ) else {
        return Err(refused());
    };
    let gone = |db: &Connection| {
        db.execute(
            "DELETE FROM auth_flow WHERE id_hash = ?1",
            params![sha(flow)],
        )
    };
    let day = now.saturating_sub(EVENT_MS);
    let store = |e| storage_error("email take", e);
    if events(db, "miss", &mailbox, day).map_err(store)? >= MISSES_PER_MAILBOX {
        gone(db).map_err(store)?;
        return Err(limit());
    }
    let digits: String = code.chars().filter(|c| !matches!(c, ' ' | '-')).collect();
    if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(coded(
            StatusCode::BAD_REQUEST,
            "badCode",
            "a code is 6 digits",
        ));
    }
    let fits = row
        .code_hash
        .as_deref()
        .is_some_and(|c| same(c, &code_hash(id, flow, &digits)));
    if fits {
        let taken = (|| {
            gone(db)?;
            mint_ticket(db, "email", &subject, None, now)
        })()
        .map_err(store)?;
        return Ok(Json(taken).into_response());
    }
    let misses = (|| {
        event(db, "miss", &mailbox, now)?;
        event(db, "flowmiss", &sha(flow), now)?;
        events(db, "flowmiss", &sha(flow), 0)
    })()
    .map_err(store)?;
    if misses >= MISSES_PER_FLOW {
        gone(db).map_err(store)?;
        return Err(ticket_refused());
    }
    Err(coded(
        StatusCode::FORBIDDEN,
        "wrongCode",
        "that code is not right",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_six_digits() {
        for _ in 0..1000 {
            let c = six_digits();
            assert_eq!(c.len(), 6);
            assert!(c.bytes().all(|b| b.is_ascii_digit()));
        }
    }

    /// AWS's published Signature Version 4 example (IAM ListUsers).
    #[test]
    fn sigv4_matches_the_aws_example() {
        let auth = sigv4(
            (
                "AKIDEXAMPLE",
                "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
                "us-east-1",
                "iam",
            ),
            ("GET", "/", "Action=ListUsers&Version=2010-05-08"),
            &[
                (
                    "content-type",
                    "application/x-www-form-urlencoded; charset=utf-8",
                ),
                ("host", "iam.amazonaws.com"),
                ("x-amz-date", "20150830T123600Z"),
            ],
            "20150830T123600Z",
            b"",
        );
        assert_eq!(
            auth,
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/iam/aws4_request, \
             SignedHeaders=content-type;host;x-amz-date, \
             Signature=5d672d79c15b13162d9279b0855cfba6789a8edb4c82c400e06b5924a6f2b5d7"
        );
    }

    #[test]
    fn amz_dates() {
        assert_eq!(amz_date(0), "19700101T000000Z");
        assert_eq!(amz_date(1_440_938_160), "20150830T123600Z");
        assert_eq!(amz_date(951_782_400), "20000229T000000Z");
        assert_eq!(amz_date(4_102_444_799), "20991231T235959Z");
    }

    #[test]
    fn the_mail_has_no_link() {
        let (text, html) = mail_body("042137", 21 * 3_600_000 + 40 * 60_000);
        assert!(!SUBJECT.contains(char::is_numeric));
        assert!(text.contains("\n042137\n") && text.contains("21:40 UTC"));
        assert!(html.contains(">042137<") && html.contains("21:40 UTC"));
        for part in [&text, &html] {
            assert!(
                !part.contains("http")
                    && !part.contains("www")
                    && !part.contains("<a ")
                    && !part.contains("<img")
            );
        }
    }
}

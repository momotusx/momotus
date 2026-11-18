//! The webOS stream server checked under the TV's Nodes (`wasm-test.sh`
//! drives it with `tests/wasm/run.js` as the host, against a local server):
//! what `open_for` leases for a URL, and what `answer` serves for one of the
//! lease's subtitles.

use super::*;
use mq_http::ClientConfig;
use wasm_bindgen::prelude::*;

/// Opens `url` as a stream of the install at `addon` (its host is the one
/// private address the playback may reach), for a player that plays HLS
/// when `hls`. Answers `{url, format}` or `{error}`, and with `subtitle`
/// the lease's VTT for that file as `answer` serves it.
#[wasm_bindgen]
pub async fn open_lease(url: String, addon: String, hls: bool, subtitle: String) -> String {
    mq_rt::set_panic_hook();
    set_origin(SocketAddr::from(([127, 0, 0, 1], 47801)));
    let http = HttpClient::new(ClientConfig::default()).expect("a client");
    let server = StreamServer::start(http).await.expect("a server");
    let upstream = Upstream::Http {
        url,
        headers: Vec::new(),
        addon_url: Url::parse(&addon).ok(),
    };
    let plays = if hls { vec![Format::Hls] } else { Vec::new() };
    let deadline = Duration::from_secs(10);
    let lease = match server
        .open_for(upstream, "film.mp4", deadline, &plays)
        .await
    {
        Ok(lease) => lease,
        Err(error) => return serde_json::json!({ "error": error }).to_string(),
    };
    let mut out = serde_json::json!({
        "url": lease.url,
        "format": lease.format.map(Format::extension),
    });
    if !subtitle.is_empty() {
        let served = lease
            .subtitles()
            .add(&subtitle, "en", Url::parse(&addon).ok().as_ref());
        let path = served
            .as_deref()
            .and_then(|u| u.strip_prefix("http://127.0.0.1:47801"));
        if let Some(answer) = server.answer(path.unwrap_or_default()).await {
            out["subtitle"] = serde_json::json!({
                "status": answer.status,
                "headers": answer.headers,
                "body": String::from_utf8_lossy(&answer.body),
            });
        }
    }
    out.to_string()
}

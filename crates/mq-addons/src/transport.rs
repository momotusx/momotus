use mq_protocol::ResourcePath;
use percent_encoding::utf8_percent_encode;
use stremio_core::constants::{ADDON_LEGACY_PATH, ADDON_MANIFEST_PATH, URI_COMPONENT_ENCODE_SET};
use stremio_core::types::query_params_encode;
use url::{Position, Url};

#[derive(Debug, thiserror::Error)]
pub enum AddonError {
    /// Why the link is not an addon link, in words; never the link itself,
    /// which may carry a key.
    #[error("not an addon link: {0}")]
    InvalidTransportUrl(&'static str),
    #[error("legacy JSON-RPC addons are not supported yet")]
    LegacyTransport,
    #[error(transparent)]
    Http(#[from] mq_http::HttpError),
    #[error("timed out")]
    TimedOut,
}

/// Accepts `stremio://` and `http(s)://` manifest URLs and returns the https
/// form ending in `/manifest.json`, without a fragment (one link, one addon:
/// a fragment never reaches the server).
pub fn normalize_transport_url(raw: &str) -> Result<Url, AddonError> {
    let raw = raw.trim();
    let raw = match raw.strip_prefix("stremio://") {
        Some(rest) => format!("https://{rest}"),
        None => raw.to_owned(),
    };
    let mut url =
        Url::parse(&raw).map_err(|_| AddonError::InvalidTransportUrl("it is not a web address"))?;
    url.set_fragment(None);
    if !matches!(url.scheme(), "http" | "https") {
        return Err(AddonError::InvalidTransportUrl(
            "it must start with https://, http:// or stremio://",
        ));
    }
    if url.path().ends_with(ADDON_LEGACY_PATH) {
        return Err(AddonError::LegacyTransport);
    }
    if !url.path().ends_with(ADDON_MANIFEST_PATH) {
        return Err(AddonError::InvalidTransportUrl(
            "it must end with /manifest.json",
        ));
    }
    Ok(url)
}

/// Builds the resource URL exactly as the Stremio HTTP transport does:
/// `/{resource}/{type}/{id}[/{extra}].json` in place of `/manifest.json`.
pub fn resource_url(transport_url: &Url, path: &ResourcePath) -> Result<String, AddonError> {
    if transport_url.path().ends_with(ADDON_LEGACY_PATH) {
        return Err(AddonError::LegacyTransport);
    }
    let enc = |s: &str| utf8_percent_encode(s, URI_COMPONENT_ENCODE_SET).to_string();
    let mut suffix = format!(
        "/{}/{}/{}",
        enc(&path.resource),
        enc(&path.r#type),
        enc(&path.id)
    );
    if !path.extra.is_empty() {
        suffix.push('/');
        suffix.push_str(&query_params_encode(
            path.extra.iter().map(|ev| (&ev.name, &ev.value)),
        ));
    }
    suffix.push_str(".json");
    in_place_of_manifest(transport_url, &suffix)
}

/// The addon's configure page: `/configure` in place of `/manifest.json`, the
/// query kept (Stremio's convention).
pub fn configure_url(transport_url: &Url) -> Result<String, AddonError> {
    in_place_of_manifest(transport_url, "/configure")
}

/// `transport_url` with its last path segment, `manifest.json`, replaced by
/// `suffix`; the query stays and the fragment goes. Only the path's last
/// segment: a query or an earlier segment holding `manifest.json` is not it.
fn in_place_of_manifest(transport_url: &Url, suffix: &str) -> Result<String, AddonError> {
    let dir = transport_url
        .path()
        .strip_suffix(ADDON_MANIFEST_PATH)
        .ok_or(AddonError::InvalidTransportUrl(
            "it does not end with /manifest.json",
        ))?;
    let mut out = format!("{}{dir}{suffix}", &transport_url[..Position::BeforePath]);
    if let Some(query) = transport_url.query() {
        out.push('?');
        out.push_str(query);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use stremio_core::types::addon::ExtraValue;

    fn path(resource: &str, ty: &str, id: &str, extra: &[(&str, &str)]) -> ResourcePath {
        ResourcePath {
            resource: resource.into(),
            r#type: ty.into(),
            id: id.into(),
            extra: extra
                .iter()
                .map(|(n, v)| ExtraValue {
                    name: (*n).into(),
                    value: (*v).into(),
                })
                .collect(),
        }
    }

    #[test]
    fn builds_resource_urls_with_config_prefix_and_extra() {
        let t = Url::parse("https://torrentio.strem.fun/sort=qualitysize/manifest.json").unwrap();
        assert_eq!(
            resource_url(&t, &path("stream", "series", "tt0944947:1:1", &[])).unwrap(),
            "https://torrentio.strem.fun/sort=qualitysize/stream/series/tt0944947%3A1%3A1.json"
        );
        let c = Url::parse("https://v3-cinemeta.strem.io/manifest.json").unwrap();
        assert_eq!(
            resource_url(
                &c,
                &path(
                    "catalog",
                    "movie",
                    "top",
                    &[("search", "the office"), ("skip", "100")]
                )
            )
            .unwrap(),
            "https://v3-cinemeta.strem.io/catalog/movie/top/search=the%20office&skip=100.json"
        );
    }

    #[test]
    fn only_the_last_path_segment_is_the_manifest() {
        let path = path("stream", "movie", "tt1", &[]);
        let query =
            Url::parse("https://a.example/cfg/manifest.json?next=/x/manifest.json").unwrap();
        assert_eq!(
            resource_url(&query, &path).unwrap(),
            "https://a.example/cfg/stream/movie/tt1.json?next=/x/manifest.json"
        );
        assert_eq!(
            configure_url(&query).unwrap(),
            "https://a.example/cfg/configure?next=/x/manifest.json"
        );
        let configured =
            Url::parse("https://a.example/manifest.json/eyJrZXkiOiJ2In0/manifest.json").unwrap();
        assert_eq!(
            resource_url(&configured, &path).unwrap(),
            "https://a.example/manifest.json/eyJrZXkiOiJ2In0/stream/movie/tt1.json"
        );
        assert_eq!(
            configure_url(&configured).unwrap(),
            "https://a.example/manifest.json/eyJrZXkiOiJ2In0/configure"
        );
        let root = Url::parse("https://a.example/manifest.json").unwrap();
        assert_eq!(configure_url(&root).unwrap(), "https://a.example/configure");
        let other = Url::parse("https://a.example/manifest.json?x=1/other.json").unwrap();
        assert!(configure_url(&other).is_ok(), "the query is not the path");
        let not = Url::parse("https://a.example/x/manifest.jsonp").unwrap();
        assert!(configure_url(&not).is_err());
    }

    #[test]
    fn a_fragment_is_no_part_of_the_link() {
        let url = normalize_transport_url("stremio://a.example/cfg/manifest.json#one").unwrap();
        assert_eq!(url.as_str(), "https://a.example/cfg/manifest.json");
        let same = normalize_transport_url("https://a.example/cfg/manifest.json#two").unwrap();
        assert_eq!(url, same);
    }

    #[test]
    fn normalizes_stremio_scheme_and_rejects_bad_urls() {
        assert_eq!(
            normalize_transport_url("stremio://addon.example/cfg/manifest.json")
                .unwrap()
                .as_str(),
            "https://addon.example/cfg/manifest.json"
        );
        assert!(matches!(
            normalize_transport_url("https://old.example/stremio/v1"),
            Err(AddonError::LegacyTransport)
        ));
        assert!(normalize_transport_url("ftp://x/manifest.json").is_err());
        assert!(normalize_transport_url("https://x/other.json").is_err());
    }

    #[test]
    fn a_rejected_link_never_appears_in_its_error() {
        for raw in [
            "https://addon.example/realdebrid=SECRET/manifest.jso",
            "ftp://addon.example/SECRET/manifest.json",
            "SECRET not a link",
        ] {
            let error = normalize_transport_url(raw).unwrap_err();
            let text = format!("{error} {error:?}");
            assert!(!text.contains("SECRET"), "{text}");
        }
    }
}

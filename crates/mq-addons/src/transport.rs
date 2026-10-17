use mq_protocol::ResourcePath;
use percent_encoding::utf8_percent_encode;
use stremio_core::constants::{ADDON_LEGACY_PATH, ADDON_MANIFEST_PATH, URI_COMPONENT_ENCODE_SET};
use stremio_core::types::query_params_encode;
use url::Url;

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
/// form ending in `/manifest.json`.
pub fn normalize_transport_url(raw: &str) -> Result<Url, AddonError> {
    let raw = raw.trim();
    let raw = match raw.strip_prefix("stremio://") {
        Some(rest) => format!("https://{rest}"),
        None => raw.to_owned(),
    };
    let url =
        Url::parse(&raw).map_err(|_| AddonError::InvalidTransportUrl("it is not a web address"))?;
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
    let base = transport_url.as_str();
    match base.rfind(ADDON_MANIFEST_PATH) {
        Some(at) => Ok(format!(
            "{}{}{}",
            &base[..at],
            suffix,
            &base[at + ADDON_MANIFEST_PATH.len()..]
        )),
        None => Err(AddonError::InvalidTransportUrl(
            "it does not end with /manifest.json",
        )),
    }
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

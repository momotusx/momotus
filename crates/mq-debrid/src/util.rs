use crate::DebridError;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Parses `YYYY-MM-DDTHH:MM:SS[.fff]Z` (UTC) into unix seconds.
pub(crate) fn parse_utc(text: &str) -> Option<u64> {
    let text = text.trim().strip_suffix('Z')?;
    let (date, time) = text.split_once('T')?;
    let mut d = date.split('-').map(|p| p.parse::<i64>());
    let (y, m, day) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let time = time.split('.').next()?;
    let mut t = time.split(':').map(|p| p.parse::<i64>());
    let (hh, mm, ss) = (t.next()?.ok()?, t.next()?.ok()?, t.next()?.ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) || y < 1970 {
        return None;
    }
    // Days since epoch for a proleptic Gregorian date (Howard Hinnant).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + hh * 3600 + mm * 60 + ss).ok()
}

/// Lowercases a 40 character hex info hash, rejecting anything else.
pub(crate) fn normalize_hash(hash: &str) -> Result<String, DebridError> {
    let hash = hash.trim().to_ascii_lowercase();
    if hash.len() == 40 && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(hash)
    } else {
        Err(DebridError::provider(
            "invalid_info_hash",
            "info hash must be 40 hex characters",
        ))
    }
}

pub(crate) fn magnet_uri(hash: &str) -> String {
    format!("magnet:?xt=urn:btih:{hash}")
}

/// Last path segment, accepting both separators.
pub(crate) fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_utc_timestamps() {
        assert_eq!(parse_utc("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_utc("2032-06-06T04:42:42.000Z"), Some(1_970_109_762));
        assert_eq!(parse_utc("2051-02-18T04:08:43Z"), Some(2_560_306_123));
        assert_eq!(parse_utc("2024-02-29T12:00:00Z"), Some(1_709_208_000));
        assert_eq!(parse_utc("not a date"), None);
        assert_eq!(parse_utc("2024-13-01T00:00:00Z"), None);
    }

    #[test]
    fn normalizes_hashes() {
        let hash = "DD8255ECDC7CA55FB0BBF81323D87062DB1F6D1C";
        assert_eq!(normalize_hash(hash).unwrap(), hash.to_lowercase());
        assert!(normalize_hash("abc").is_err());
        assert!(normalize_hash(&"z".repeat(40)).is_err());
    }

    #[test]
    fn basename_handles_both_separators() {
        assert_eq!(basename("a/b/c.mkv"), "c.mkv");
        assert_eq!(basename("a\\b\\c.mkv"), "c.mkv");
        assert_eq!(basename("c.mkv"), "c.mkv");
    }
}

//! The download form on a subtitle page (`/sub/<altid>/<slug>.html`): the
//! three fields `POST /ind` takes (plans/m13-turkcealtyazi.md 2.1).

use regex::Regex;
use std::sync::LazyLock;

/// The fields of the form, checked. `sidid` is a per-page token: it is
/// sent once and never logged.
#[derive(Clone, PartialEq, Eq)]
pub struct Form {
    pub idid: String,
    pub altid: String,
    pub sidid: String,
}

impl std::fmt::Debug for Form {
    // Never prints the token, whatever a log or panic message holds.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Form {{ idid: {}, altid: {}, sidid: hidden }}",
            self.idid, self.altid
        )
    }
}

static FORM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)<form[^>]*action="/ind"[^>]*>(.*?)</form>"#).expect("pattern")
});
static INPUT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<input[^>]*name="([a-z]+)"[^>]*value="([^"]*)"[^>]*>"#).expect("pattern")
});

/// The form of the subtitle page for `altid`, or `None` when it is missing,
/// lacks a field, holds a value of the wrong shape, or names another
/// subtitle: a fault of this one file.
pub fn download_form(html: &str, altid: u32) -> Option<Form> {
    let form = &FORM.captures(html)?[1];
    let field = |name: &str| {
        INPUT
            .captures_iter(form)
            .find(|c| &c[1] == name)
            .map(|c| c[2].to_owned())
    };
    let (idid, page, sidid) = (field("idid")?, field("altid")?, field("sidid")?);
    let digits = |s: &str| !s.is_empty() && s.len() <= 16 && s.bytes().all(|b| b.is_ascii_digit());
    let token = (8..=128).contains(&sidid.len())
        && sidid
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    (digits(&idid) && page == altid.to_string() && token).then_some(Form {
        idid,
        altid: page,
        sidid,
    })
}

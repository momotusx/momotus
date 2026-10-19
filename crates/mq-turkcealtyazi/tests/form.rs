//! The download form of a recorded subtitle page (its token replaced).

use mq_turkcealtyazi::download_form;

fn page() -> String {
    let path = format!("{}/tests/corpus/sub-form.html", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn the_three_fields_are_read_and_the_token_never_printed() {
    let form = download_form(&page(), 108735).unwrap();
    assert_eq!(
        (form.idid.as_str(), form.altid.as_str()),
        ("174736", "108735")
    );
    assert_eq!(form.sidid.len(), 32);
    assert!(!format!("{form:?}").contains(&form.sidid), "{form:?}");
}

#[test]
fn a_missing_field_another_subtitle_or_an_odd_value_is_a_fault() {
    let html = page();
    assert!(
        download_form(&html, 108736).is_none(),
        "the form names another subtitle"
    );
    assert!(download_form(&html.replace(r#"name="sidid""#, r#"name="token""#), 108735).is_none());
    assert!(
        download_form(
            &html.replace(r#"action="/ind""#, r#"action="/download""#),
            108735
        )
        .is_none()
    );
    assert!(
        download_form(
            &html.replace(r#"value="174736""#, r#"value="17x736""#),
            108735
        )
        .is_none()
    );
    let short = html.replace("0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f", "0f0f");
    assert!(download_form(&short, 108735).is_none());
    let odd = html.replace(
        "0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f",
        "0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f!!",
    );
    assert!(
        download_form(&odd, 108735).is_none(),
        "a token character outside [A-Za-z0-9_-]"
    );
}

mod common;

use common::{Server, client, route};
use mq_debrid::{Credential, Login, LoginPoll, StoreId};
use std::time::Duration;

const AD_PIN: &str = r#"{"status":"success","data":{"pin":"Q8W6","check":"c0ffee","expires_in":600,"user_url":"https://alldebrid.com/pin/?pin=Q8W6","base_url":"https://alldebrid.com/pin/"}}"#;

#[tokio::test]
async fn alldebrid_pin_ends_in_a_key() {
    let server = Server::start(vec![
        route("GET", "/v4.1/pin/get", 200, AD_PIN),
        route(
            "POST",
            "/v4/pin/check",
            200,
            r#"{"status":"error","error":{"code":"PIN_INVALID","message":"too early"}}"#,
        )
        .then(
            200,
            r#"{"status":"success","data":{"activated":false,"expires_in":590}}"#,
        )
        .then(
            200,
            r#"{"status":"success","data":{"activated":true,"expires_in":580,"apikey":"ADKEY"}}"#,
        ),
    ])
    .await;
    let login = Login::start_at(&client(), StoreId::AllDebrid, &server.base)
        .await
        .unwrap();
    let code = login.code();
    assert_eq!(code.user_code, "Q8W6");
    assert_eq!(code.qr, "https://alldebrid.com/pin/?pin=Q8W6");
    assert_eq!(code.url, "https://alldebrid.com/pin/");
    // The first check must wait 5 s: the caller waits `interval` first.
    assert_eq!(code.interval, Duration::from_secs(5));
    assert_eq!(code.expires_in, Duration::from_secs(600));
    assert!(matches!(login.poll().await.unwrap(), LoginPoll::Pending));
    assert!(matches!(login.poll().await.unwrap(), LoginPoll::Pending));
    let LoginPoll::Approved(credential) = login.poll().await.unwrap() else {
        panic!("expected approval");
    };
    assert_eq!(credential, Credential::Key("ADKEY".into()));
    assert!(!format!("{credential:?}").contains("ADKEY"));
    let check = &server.requests()[1];
    assert_eq!(check.body, "pin=Q8W6&check=c0ffee");
    assert!(!check.head.contains("authorization"));
}

#[tokio::test]
async fn alldebrid_pin_expired_is_terminal() {
    let server = Server::start(vec![
        route("GET", "/v4.1/pin/get", 200, AD_PIN),
        route(
            "POST",
            "/v4/pin/check",
            200,
            r#"{"status":"error","error":{"code":"PIN_EXPIRED","message":"expired"}}"#,
        ),
    ])
    .await;
    let login = Login::start_at(&client(), StoreId::AllDebrid, &server.base)
        .await
        .unwrap();
    assert!(matches!(login.poll().await.unwrap(), LoginPoll::Expired));
}

const TB_START: &str = r#"{"success":true,"error":null,"detail":"ok","data":{"device_code":"TBDEVICESECRET","code":"123456","interval":5,"expires_in":600,"expires_at":"2026-10-17T12:00:00Z","verification_url":"https://torbox.app/oauth/device?app=Momotus","verification_url_complete":"https://torbox.app/oauth/device?app=Momotus&code=123456","friendly_verification_url":"https://tor.box/link"}}"#;
const TB_PENDING: &str =
    r#"{"success":false,"error":"DEVICE_CODE_NOT_USED","detail":"wait","data":null}"#;

#[tokio::test]
async fn torbox_device_flow_ends_in_a_key() {
    let server = Server::start(vec![
        route("GET", "/v1/api/user/auth/device/start", 200, TB_START).query("app=Momotus"),
        route("POST", "/v1/api/user/auth/device/token", 400, TB_PENDING).then(
            200,
            r#"{"success":true,"error":null,"detail":"ok","data":{"access_token":"TBKEY"}}"#,
        ),
    ])
    .await;
    let login = Login::start_at(&client(), StoreId::TorBox, &server.base)
        .await
        .unwrap();
    let code = login.code();
    assert_eq!(code.user_code, "123456");
    assert_eq!(
        code.qr,
        "https://torbox.app/oauth/device?app=Momotus&code=123456"
    );
    assert!(!code.qr.contains("TBDEVICESECRET"));
    assert!(matches!(login.poll().await.unwrap(), LoginPoll::Pending));
    let LoginPoll::Approved(credential) = login.poll().await.unwrap() else {
        panic!("expected approval");
    };
    assert_eq!(credential, Credential::Key("TBKEY".into()));
    assert!(
        server.requests()[1]
            .body
            .contains(r#""device_code":"TBDEVICESECRET""#)
    );
}

#[tokio::test]
async fn torbox_used_code_is_terminal() {
    let server = Server::start(vec![
        route("GET", "/v1/api/user/auth/device/start", 200, TB_START),
        route(
            "POST",
            "/v1/api/user/auth/device/token",
            400,
            r#"{"success":false,"error":"DEVICE_CODE_USED","detail":"used","data":null}"#,
        )
        .then(
            400,
            r#"{"success":false,"error":"ITEM_NOT_FOUND","detail":"gone","data":null}"#,
        ),
    ])
    .await;
    let login = Login::start_at(&client(), StoreId::TorBox, &server.base)
        .await
        .unwrap();
    assert!(matches!(login.poll().await.unwrap(), LoginPoll::Used));
    assert!(matches!(login.poll().await.unwrap(), LoginPoll::Expired));
}

#[tokio::test]
async fn a_rate_limited_poll_asks_to_slow_down() {
    let server = Server::start(vec![
        route("GET", "/v1/api/user/auth/device/start", 200, TB_START),
        route(
            "POST",
            "/v1/api/user/auth/device/token",
            429,
            r#"{"success":false,"error":"COOLDOWN_LIMIT","detail":"slow","data":null}"#,
        ),
    ])
    .await;
    let login = Login::start_at(&client(), StoreId::TorBox, &server.base)
        .await
        .unwrap();
    assert!(matches!(login.poll().await.unwrap(), LoginPoll::SlowDown));
}

#[tokio::test]
async fn premiumize_has_no_code_sign_in() {
    let server = Server::start(vec![]).await;
    assert!(
        Login::start_at(&client(), StoreId::Premiumize, &server.base)
            .await
            .is_err()
    );
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn a_poll_error_never_carries_the_pin_secret() {
    let server = Server::start(vec![
        route("GET", "/v4.1/pin/get", 200, AD_PIN),
        route(
            "POST",
            "/v4/pin/check",
            200,
            r#"{"status":"error","error":{"code":"PIN_REJECTED","message":"check c0ffee is not valid"}}"#,
        ),
    ])
    .await;
    let login = Login::start_at(&client(), StoreId::AllDebrid, &server.base)
        .await
        .unwrap();
    let err = login.poll().await.unwrap_err();
    let printed = format!("{err} {err:?}");
    assert!(!printed.contains("c0ffee"), "{printed}");
}

#[tokio::test]
async fn a_lifetime_out_of_range_is_an_error_not_a_panic() {
    let start = TB_START.replace(
        r#""expires_in":600"#,
        r#""expires_in":18446744073709551615"#,
    );
    let server = Server::start(vec![route(
        "GET",
        "/v1/api/user/auth/device/start",
        200,
        &start,
    )])
    .await;
    let err = Login::start_at(&client(), StoreId::TorBox, &server.base)
        .await
        .err()
        .expect("an error");
    assert!(
        matches!(&err, mq_debrid::DebridError::Provider { code, .. } if code == "bad_answer"),
        "{err:?}"
    );
}

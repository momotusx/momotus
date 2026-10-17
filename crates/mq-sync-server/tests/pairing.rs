//! Pairing a new device and recovering an account through a real relay.

use mq_http::{ClientConfig, HttpClient, RequestOptions};
use mq_sync::pair::{approve_pairing, new_recovery_kit, recover, start_pairing, store_recovery};
use mq_sync::seal::KeyBundle;
use mq_sync::wire::AccountResponse;
use mq_sync::{SyncConfig, SyncError};
use mq_sync_server::{Config, router};

async fn relay() -> String {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).unwrap();
    let dir = std::env::temp_dir().join(format!("mq-pair-{}", hex::encode(b)));
    std::fs::create_dir_all(&dir).unwrap();
    let app = router(&dir.join("relay.sqlite"), Config::default()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

fn http() -> HttpClient {
    HttpClient::new(ClientConfig::default()).unwrap()
}

async fn phone(server: &str) -> SyncConfig {
    let local = RequestOptions {
        allow_private: true,
        ..Default::default()
    };
    let a: AccountResponse = http()
        .post_json(
            &format!("{server}/v1/accounts"),
            &serde_json::json!({}),
            &local,
        )
        .await
        .unwrap()
        .json()
        .unwrap();
    SyncConfig {
        server: server.to_owned(),
        device_token: a.device_token,
        keys: KeyBundle::generate(),
    }
}

#[tokio::test]
async fn a_tv_joins_by_qr_once() {
    let server = relay().await;
    let phone = phone(&server).await;
    let tv = start_pairing(&http(), &server).await.unwrap();
    assert!(tv.qr.starts_with("marquee:pair?v=1&"), "{}", tv.qr);
    assert_eq!(tv.poll(&http()).await.unwrap(), None, "waiting");

    approve_pairing(&http(), &phone, &tv.qr).await.unwrap();
    let joined = tv.poll(&http()).await.unwrap().expect("joined");
    assert_eq!(joined.keys, phone.keys);
    assert_eq!(joined.server, server);
    assert_ne!(joined.device_token, phone.device_token, "its own token");

    // The slot is single use.
    assert!(tv.poll(&http()).await.is_err());
    let again = approve_pairing(&http(), &phone, &tv.qr).await;
    assert!(again.is_err(), "a used slot cannot be filled again");
}

#[tokio::test]
async fn a_blob_sealed_to_another_key_does_not_open() {
    let server = relay().await;
    let phone = phone(&server).await;
    let tv = start_pairing(&http(), &server).await.unwrap();
    let attacker = start_pairing(&http(), &server).await.unwrap();
    // Approving the attacker's code but filing it under the TV's slot: the
    // TV's key does not open it.
    let attacker_key = attacker.qr.split("&k=").nth(1).unwrap();
    let forged = tv.qr.split("&k=").next().unwrap().to_owned() + "&k=" + attacker_key;
    approve_pairing(&http(), &phone, &forged).await.unwrap();
    let err = tv.poll(&http()).await.unwrap_err();
    assert!(matches!(err, SyncError::Pairing(_)), "{err:?}");
    // And codes for another relay are refused before anything is sent.
    let elsewhere = tv.qr.replace("127.0.0.1", "127.0.0.2");
    assert!(approve_pairing(&http(), &phone, &elsewhere).await.is_err());
}

#[tokio::test]
async fn a_recovery_kit_brings_the_account_back() {
    let server = relay().await;
    let phone = phone(&server).await;
    let kit = new_recovery_kit();
    store_recovery(&http(), &phone, &kit).await.unwrap();

    let joined = recover(&http(), &server, &kit.to_lowercase())
        .await
        .unwrap();
    assert_eq!(joined.keys, phone.keys);
    assert_ne!(joined.device_token, phone.device_token);

    let wrong = new_recovery_kit();
    assert!(recover(&http(), &server, &wrong).await.is_err());
}

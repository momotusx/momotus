//! Pairing a new device and recovering an account through a real relay.

use mq_http::{ClientConfig, HttpClient, HttpError, RequestOptions};
use mq_sync::pair::{
    approve_pairing, new_recovery_kit, recover, reseal_recovery, start_pairing, store_recovery,
};
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

fn local() -> RequestOptions {
    RequestOptions {
        allow_private: true,
        ..Default::default()
    }
}

/// A device with its own account on the relay.
async fn device(server: &str) -> SyncConfig {
    let a: AccountResponse = http()
        .post_json(
            &format!("{server}/v1/accounts"),
            &serde_json::json!({}),
            &local(),
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
async fn a_tv_joins_by_qr_once_after_matching_codes() {
    let server = relay().await;
    let phone = device(&server).await;
    let tv = start_pairing(&http(), &server).await.unwrap();
    assert!(tv.qr.starts_with("marquee:pair?v=1&"), "{}", tv.qr);
    assert_eq!(tv.poll(&http()).await.unwrap(), None, "waiting");

    let shown_on_phone = approve_pairing(&http(), &phone, &tv.qr).await.unwrap();
    let offer = tv.poll(&http()).await.unwrap().expect("offer");
    assert_eq!(
        offer.code, shown_on_phone,
        "both screens show the same code"
    );
    assert_eq!(offer.joined.keys, phone.keys);
    assert_eq!(offer.joined.server, server);
    assert_ne!(
        offer.joined.device_token, phone.device_token,
        "its own token"
    );

    // The slot is single use.
    assert!(tv.poll(&http()).await.is_err());
    let again = approve_pairing(&http(), &phone, &tv.qr).await.unwrap_err();
    assert!(again.to_string().contains("already used"), "{again}");
}

#[tokio::test]
async fn someone_else_who_saw_the_qr_cannot_slip_in() {
    let server = relay().await;
    let phone = device(&server).await;
    let stranger = device(&server).await;
    let tv = start_pairing(&http(), &server).await.unwrap();

    // The stranger answers first.
    let strangers_code = approve_pairing(&http(), &stranger, &tv.qr).await.unwrap();
    // The owner's phone is told the code was already used, so it shows no code
    // that could match.
    assert!(approve_pairing(&http(), &phone, &tv.qr).await.is_err());
    let offer = tv.poll(&http()).await.unwrap().expect("offer");
    assert_eq!(offer.code, strangers_code);
    assert_eq!(offer.joined.keys, stranger.keys);
    // The TV keeps nothing until the user confirms (the view's job); the
    // owner sees no matching code and rejects.

    // Knowing the slot id from the QR is not enough to collect the blob.
    let slot = tv
        .qr
        .split("&p=")
        .nth(1)
        .unwrap()
        .split('&')
        .next()
        .unwrap();
    let err = http()
        .post_json(
            &format!("{server}/v1/pairings/{slot}/take"),
            &serde_json::json!({ "take": "00" }),
            &local(),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, HttpError::Status { status: 404, .. }),
        "{err:?}"
    );
}

#[tokio::test]
async fn a_blob_sealed_to_another_key_does_not_open() {
    let server = relay().await;
    let phone = device(&server).await;
    let tv = start_pairing(&http(), &server).await.unwrap();
    let other = start_pairing(&http(), &server).await.unwrap();
    // The TV's slot, but the other device's key.
    let other_key = other.qr.split("&k=").nth(1).unwrap();
    let forged = tv.qr.split("&k=").next().unwrap().to_owned() + "&k=" + other_key;
    approve_pairing(&http(), &phone, &forged).await.unwrap();
    let err = tv.poll(&http()).await.unwrap_err();
    assert!(matches!(err, SyncError::Pairing(_)), "{err:?}");
    // Codes for another relay are refused before anything is sent.
    let elsewhere = tv.qr.replace("127.0.0.1", "127.0.0.2");
    assert!(approve_pairing(&http(), &phone, &elsewhere).await.is_err());
}

#[tokio::test]
async fn a_recovery_kit_survives_a_key_rotation() {
    let server = relay().await;
    let phone = device(&server).await;
    let kit = new_recovery_kit();
    store_recovery(&http(), &phone, &kit).await.unwrap();

    let joined = recover(&http(), &server, &kit.to_lowercase())
        .await
        .unwrap();
    assert_eq!(joined.keys, phone.keys);
    assert_ne!(joined.device_token, phone.device_token);

    // After a rotation any device reseals the new bundle without the kit.
    let rotated = SyncConfig {
        keys: KeyBundle {
            epoch: 1,
            ..KeyBundle::generate()
        },
        ..phone.clone()
    };
    assert_eq!(reseal_recovery(&http(), &rotated).await.unwrap(), 1);
    let joined = recover(&http(), &server, &kit).await.unwrap();
    assert_eq!(joined.keys, rotated.keys);

    // A wrong kit finds nothing.
    assert!(
        recover(&http(), &server, &new_recovery_kit())
            .await
            .is_err()
    );
}

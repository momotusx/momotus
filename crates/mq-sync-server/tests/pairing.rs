//! Pairing a new device and recovering an account through a real relay.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use mq_http::{ClientConfig, HttpClient, HttpError, RequestOptions};
use mq_sync::pair::{
    approve_pairing, new_recovery_kit, recover, reseal_recovery, start_pairing, store_recovery,
};
use mq_sync::seal::KeyBundle;
use mq_sync::wire::AccountResponse;
use mq_sync::{SyncConfig, SyncError};
use mq_sync_server::{Config, router};
use std::path::PathBuf;

async fn relay() -> (String, PathBuf) {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).unwrap();
    let dir = std::env::temp_dir().join(format!("mq-pair-{}", hex::encode(b)));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("relay.sqlite");
    let app = router(&db, Config::default()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });
    (base, db)
}

fn devices(db: &PathBuf) -> i64 {
    rusqlite::Connection::open(db)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM device", [], |r| r.get(0))
        .unwrap()
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
    let (server, db) = relay().await;
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
    assert_eq!(devices(&db), 1, "no token before the user confirms");

    let joined = tv.claim(&http(), offer).await.unwrap();
    assert_eq!(joined.keys, phone.keys);
    assert_eq!(joined.server, server);
    assert_ne!(joined.device_token, phone.device_token, "its own token");
    assert_eq!(devices(&db), 2);

    // The slot is single use.
    assert!(tv.poll(&http()).await.is_err());
    let again = approve_pairing(&http(), &phone, &tv.qr).await.unwrap_err();
    assert!(again.to_string().contains("expired"), "{again}");
}

#[tokio::test]
async fn someone_else_who_saw_the_qr_cannot_slip_in() {
    let (server, db) = relay().await;
    let phone = device(&server).await;
    let stranger = device(&server).await;
    let tv = start_pairing(&http(), &server).await.unwrap();

    // The stranger answers first; the owner's phone is told so, and shows
    // no code that could match the one on the TV.
    let strangers_code = approve_pairing(&http(), &stranger, &tv.qr).await.unwrap();
    let err = approve_pairing(&http(), &phone, &tv.qr).await.unwrap_err();
    assert!(err.to_string().contains("someone else"), "{err}");
    let offer = tv.poll(&http()).await.unwrap().expect("offer");
    assert_eq!(offer.code, strangers_code);
    // The user rejects: nothing is claimed, so no token exists for the TV.
    drop(offer);
    assert_eq!(devices(&db), 2, "only the two accounts' own devices");

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
            &serde_json::json!({ "take": "00", "claim": true }),
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
    let (server, _) = relay().await;
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
    let (server, _) = relay().await;
    let phone = device(&server).await;
    let kit = new_recovery_kit();
    store_recovery(&http(), &phone, &kit).await.unwrap();

    let joined = recover(&http(), &server, &kit.to_lowercase())
        .await
        .unwrap();
    assert_eq!(joined.keys, phone.keys);
    assert_ne!(joined.device_token, phone.device_token);

    // After a rotation any device reseals the new bundle without the kit; the
    // id key never rotates, so the kit still accepts it.
    let rotated = SyncConfig {
        keys: KeyBundle {
            epoch: 1,
            id_key: phone.keys.id_key,
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

#[tokio::test]
async fn a_token_holder_cannot_plant_their_keys_in_a_recovery() {
    let (server, _) = relay().await;
    let phone = device(&server).await;
    let kit = new_recovery_kit();
    store_recovery(&http(), &phone, &kit).await.unwrap();

    // A stolen token (or the relay itself) seals keys of its choosing to the
    // kit's public key, which the relay lists to any device of the account.
    let planted = SyncConfig {
        keys: KeyBundle::generate(),
        ..phone.clone()
    };
    assert_eq!(reseal_recovery(&http(), &planted).await.unwrap(), 1);
    let err = recover(&http(), &server, &kit).await.unwrap_err();
    assert!(err.to_string().contains("not this account"), "{err}");

    // The kit list carries public keys only.
    let list: serde_json::Value = http()
        .get(
            &format!("{server}/v1/recovery"),
            &RequestOptions {
                headers: vec![(
                    "authorization".into(),
                    format!("Bearer {}", phone.device_token),
                )],
                ..local()
            },
        )
        .await
        .unwrap()
        .json()
        .unwrap();
    let pk = B64.decode(list["kits"][0]["pk"].as_str().unwrap()).unwrap();
    assert_eq!(pk.len(), 32);
}

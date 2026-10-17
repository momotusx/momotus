//! Native binding surface for the Android and Apple apps.
//!
//! The apps speak the same JSON protocol as the web UI (see mq-bridge): one
//! `send` for requests and one listener for replies and view pushes. The
//! surface stays three calls wide, and every platform runs the same session
//! code. Messages are batched per frame by the core, so the per-call cost of
//! the binding is paid once per batch, not per item.

use mq_bridge::Session;
use mq_runtime::{Core, CoreConfig};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FfiError {
    #[error("{message}")]
    Open { message: String },
}

/// Receives every message from the core as one JSON text.
#[uniffi::export(with_foreign)]
pub trait CoreListener: Send + Sync {
    fn on_message(&self, json: String);
}

#[derive(uniffi::Object)]
pub struct MarqueeCore {
    /// Owns the core threads; dropping the object stops them.
    _runtime: tokio::runtime::Runtime,
    inbox: mpsc::UnboundedSender<String>,
    _forward: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

#[uniffi::export]
impl MarqueeCore {
    /// Opens the core over `data_dir` and starts delivering messages to
    /// `listener` from a core thread.
    #[uniffi::constructor]
    pub fn open(
        data_dir: String,
        profile: String,
        device_id: u64,
        listener: Arc<dyn CoreListener>,
    ) -> Result<Arc<Self>, FfiError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("marquee-core")
            .enable_all()
            .build()
            .map_err(|e| FfiError::Open {
                message: e.to_string(),
            })?;
        let core = runtime
            .block_on(async {
                Core::open(CoreConfig {
                    data_dir: PathBuf::from(data_dir),
                    profile_id: profile,
                    device_id,
                })
            })
            .map_err(|e| FfiError::Open {
                message: e.to_string(),
            })?;

        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<serde_json::Value>();
        let (in_tx, mut in_rx) = mpsc::unbounded_channel::<String>();
        let forward = runtime.spawn(async move {
            while let Some(message) = out_rx.recv().await {
                listener.on_message(message.to_string());
            }
        });
        let died = out_tx.clone();
        let session = runtime.spawn(async move {
            let mut session = Session::new(core, out_tx);
            while let Some(text) = in_rx.recv().await {
                session.handle_text(&text).await;
            }
        });
        // A panic ends the request loop; say so instead of going silent, so
        // the app can show it and open the core again.
        runtime.spawn(async move {
            if let Err(e) = session.await
                && e.is_panic()
            {
                let _ = died.send(serde_json::json!({ "core": "died", "reason": e.to_string() }));
            }
        });
        Ok(Arc::new(Self {
            _runtime: runtime,
            inbox: in_tx,
            _forward: Mutex::new(Some(forward)),
        }))
    }

    /// Sends one protocol request (JSON text). Never blocks the caller.
    pub fn send(&self, json: String) {
        let _ = self.inbox.send(json);
    }

    /// Core version string for diagnostics screens.
    pub fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_owned()
    }
}

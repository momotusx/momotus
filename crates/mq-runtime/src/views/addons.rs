//! Installed addons, in order, for the settings screen. Refreshes whenever the
//! list changes (install, remove, move, enable, import, sync).

use crate::Core;
use crate::live::{FRAME, coalesce};
use eyeball::SharedObservable;
use futures_util::{Stream, StreamExt};
use mq_library::Table;
use mq_protocol::Manifest;
use serde::Serialize;
use std::pin::Pin;
use stremio_core::types::addon::ManifestResource;
use tokio::task::JoinHandle;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct AddonInfo {
    /// The key for commands. May hold API keys: never show or log it.
    pub url: String,
    /// What to show instead of the URL.
    pub host: String,
    pub name: String,
    pub description: Option<String>,
    pub logo: Option<String>,
    pub version: String,
    pub enabled: bool,
    /// "catalog", "meta", "stream", "subtitles", ...
    pub provides: Vec<String>,
    pub types: Vec<String>,
    pub configurable: bool,
    /// The addon's configure page, when its manifest says it has one
    /// (`configurable` or `configurationRequired`). Built from the installed
    /// link, so it may hold keys: never show or log it.
    pub configure_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct AddonsSnapshot {
    pub addons: Vec<AddonInfo>,
    /// The profile starts at the default addons step: it never had a list,
    /// this device is in no sync account, and it is not a kids profile.
    pub fresh: bool,
}

pub struct AddonsView {
    snapshot: SharedObservable<AddonsSnapshot>,
    task: JoinHandle<()>,
}

impl AddonsView {
    pub fn open(core: &Core) -> Self {
        // Subscribe before the first read so no change slips in between.
        let changes = core.library().subscribe();
        let snapshot = SharedObservable::new(list(core));
        let task = tokio::spawn(watch(core.clone(), changes, snapshot.clone()));
        Self { snapshot, task }
    }

    pub fn snapshot(&self) -> AddonsSnapshot {
        self.snapshot.get()
    }

    pub fn subscribe(
        &self,
    ) -> (
        AddonsSnapshot,
        Pin<Box<dyn Stream<Item = Vec<AddonsSnapshot>> + Send>>,
    ) {
        let updates = self.snapshot.subscribe().map(|s| vec![s]);
        (
            self.snapshot.get(),
            Box::pin(coalesce(Box::pin(updates), FRAME)),
        )
    }
}

impl Drop for AddonsView {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn watch(
    core: Core,
    mut changes: tokio::sync::broadcast::Receiver<Vec<Table>>,
    snapshot: SharedObservable<AddonsSnapshot>,
) {
    while let Some(tables) = crate::views::next_change(&mut changes).await {
        if tables.contains(&Table::Addons) {
            let next = list(&core);
            if next != snapshot.get() {
                snapshot.set(next);
            }
        }
    }
}

fn list(core: &Core) -> AddonsSnapshot {
    let rows = core.library().addons(core.profile()).unwrap_or_default();
    let addons = rows
        .into_iter()
        .filter_map(|row| {
            let manifest: Manifest = serde_json::from_str(&row.manifest_json).ok()?;
            let url = url::Url::parse(&row.transport_url).ok()?;
            let configure_url = crate::views::addon_catalog::configure_page(&url, &manifest);
            Some(AddonInfo {
                host: url.host_str().unwrap_or_default().to_owned(),
                url: row.transport_url,
                name: manifest.name,
                description: manifest.description,
                logo: manifest.logo.map(|u| u.to_string()),
                version: manifest.version.to_string(),
                enabled: row.enabled,
                provides: manifest
                    .resources
                    .iter()
                    .map(|r| match r {
                        ManifestResource::Short(name) => name.clone(),
                        ManifestResource::Full { name, .. } => name.clone(),
                    })
                    .collect(),
                configure_url,
                types: manifest.types,
                configurable: manifest.behavior_hints.configurable,
            })
        })
        .collect();
    AddonsSnapshot {
        addons,
        fresh: core.addons_fresh(),
    }
}

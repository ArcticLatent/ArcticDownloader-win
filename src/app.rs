use crate::{
    catalog::CatalogService,
    config::ConfigStore,
    download::DownloadManager,
    ram::{RamProfile, RamTier},
    updater::Updater,
};
use anyhow::{anyhow, Result};
use log::{info, warn};
use std::sync::Arc;
use tokio::runtime::{Builder, Runtime};

pub const APP_ID: &str = "io.github.ArcticHelper";

#[derive(Clone)]
pub struct AppContext {
    pub runtime: Arc<Runtime>,
    pub config: Arc<ConfigStore>,
    pub catalog: Arc<CatalogService>,
    pub downloads: Arc<DownloadManager>,
    pub updater: Arc<Updater>,
    pub ram_profile: Option<RamProfile>,
    pub display_version: String,
}

impl AppContext {
    pub fn ram_tier(&self) -> Option<RamTier> {
        self.ram_profile.map(|profile| profile.tier)
    }

    pub fn total_ram_gb(&self) -> Option<f64> {
        self.ram_profile.map(|profile| profile.total_gb)
    }
}

pub fn build_context() -> Result<AppContext> {
    let runtime = Arc::new(
        Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|err| anyhow!("failed to create Tokio runtime: {err}"))?,
    );

    let config = Arc::new(ConfigStore::new()?);
    let catalog = Arc::new(CatalogService::new(config.clone())?);

    // Ensure catalog is always refreshed from remote before the UI boots.
    if let Err(err) = runtime.block_on(catalog.refresh_from_remote()) {
        warn!("Unable to refresh catalog from remote source: {err:#}");
    } else {
        info!("Catalog refreshed from Supabase at startup.");
    }

    let display_version = resolve_display_version(&config);
    let downloads = Arc::new(DownloadManager::new(runtime.clone(), config.clone()));
    let updater = Arc::new(Updater::new(
        runtime.clone(),
        config.clone(),
        display_version.clone(),
    )?);
    Ok(AppContext {
        runtime,
        config,
        catalog,
        downloads,
        updater,
        ram_profile: None,
        display_version,
    })
}

fn resolve_display_version(config: &ConfigStore) -> String {
    let _ = config;
    env!("CARGO_PKG_VERSION").to_string()
}

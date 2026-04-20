use crate::{
    config::ConfigStore,
    model::{LoraDefinition, ModelCatalog, ModelVariant, ResolvedModel, WorkflowDefinition},
    vram::VramTier,
};
use anyhow::{Context, Result};
use log::{info, warn};
use reqwest::{Client, StatusCode, Url};
use serde::Deserialize;
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, RwLock},
    time::Duration,
};

const CACHED_CATALOG_FILE: &str = "catalog.json";
const SUPABASE_CATALOG_TABLE: &str = "catalog_documents";
const SUPABASE_CATALOG_KEY: &str = "main";

#[derive(Debug)]
pub struct CatalogService {
    catalog: RwLock<ModelCatalog>,
    config: Arc<ConfigStore>,
}

impl CatalogService {
    pub fn new(config: Arc<ConfigStore>) -> Result<Self> {
        let catalog = load_cached_catalog(&config).unwrap_or_default();
        info!(
            "Catalog initialised with {} models ({} LoRAs, {} workflows).",
            catalog.models.len(),
            catalog.loras.len(),
            catalog.workflows.len()
        );
        Ok(Self {
            catalog: RwLock::new(catalog),
            config,
        })
    }

    pub fn catalog_snapshot(&self) -> ModelCatalog {
        self.catalog.read().expect("catalog poisoned").clone()
    }

    pub fn variants_for_tier(&self, model_id: &str, tier: VramTier) -> Vec<ModelVariant> {
        let catalog = self.catalog_snapshot();
        catalog
            .models
            .into_iter()
            .find(|m| m.id == model_id)
            .map(|master| master.variants_for_tier(tier))
            .unwrap_or_default()
    }

    pub fn resolve_variant(&self, model_id: &str, variant_id: &str) -> Option<ResolvedModel> {
        let catalog = self.catalog_snapshot();
        let master = catalog.models.into_iter().find(|m| m.id == model_id)?;
        let variant = master
            .variants
            .iter()
            .find(|variant| variant.id == variant_id)?
            .clone();
        Some(ResolvedModel { master, variant })
    }

    pub fn resolve_download_selection(
        &self,
        model_id: &str,
        variant_id: &str,
        fallback_tier: Option<VramTier>,
    ) -> Option<ResolvedModel> {
        let catalog = self.catalog_snapshot();
        let master = catalog.models.into_iter().find(|m| m.id == model_id)?;

        if let Some(variant) = master.find_variant(variant_id).cloned() {
            return Some(ResolvedModel { master, variant });
        }

        let trimmed_variant_id = variant_id.trim();
        if trimmed_variant_id.is_empty() || trimmed_variant_id == "__always_only__" {
            let tier = fallback_tier.unwrap_or(VramTier::TierC);
            if let Some(variant) = master.synthetic_always_only_variant(tier) {
                return Some(ResolvedModel { master, variant });
            }
        }

        None
    }

    pub fn loras(&self) -> Vec<LoraDefinition> {
        self.catalog_snapshot().loras
    }

    pub fn lora_families(&self) -> Vec<String> {
        self.catalog_snapshot().lora_families()
    }

    pub fn find_lora(&self, id: &str) -> Option<LoraDefinition> {
        self.catalog_snapshot().find_lora(id)
    }

    pub fn workflows(&self) -> Vec<WorkflowDefinition> {
        self.catalog_snapshot().workflows
    }

    pub fn workflow_families(&self) -> Vec<String> {
        self.catalog_snapshot().workflow_families()
    }

    pub fn find_workflow(&self, id: &str) -> Option<WorkflowDefinition> {
        self.catalog_snapshot().find_workflow(id)
    }

    pub async fn refresh_from_remote(&self) -> Result<bool> {
        let Some(source) = SupabaseCatalogSource::from_env() else {
            warn!(
                "Supabase catalog is not configured; set ARCTIC_SUPABASE_URL and ARCTIC_SUPABASE_ANON_KEY or ARCTIC_SUPABASE_PUBLISHABLE_KEY."
            );
            return Ok(false);
        };

        let client = Client::builder()
            .user_agent(format!(
                "ArcticDownloader/{} ({})",
                env!("CARGO_PKG_VERSION"),
                env!("CARGO_PKG_NAME")
            ))
            .timeout(Duration::from_secs(10))
            .build()
            .context("failed to build HTTP client for Supabase catalog refresh")?;

        let url = source.rest_url()?;
        info!("Refreshing catalog from Supabase table {SUPABASE_CATALOG_TABLE}.{SUPABASE_CATALOG_KEY}");
        let response = client
            .get(url.clone())
            .header("apikey", &source.read_key)
            .bearer_auth(&source.read_key)
            .send()
            .await
            .with_context(|| format!("failed to fetch Supabase catalog from {url}"))?;

        match response.status() {
            StatusCode::OK => {}
            status => {
                warn!(
                    "Supabase catalog refresh skipped: server returned {} ({:?})",
                    status.as_u16(),
                    status
                );
                return Ok(false);
            }
        }

        let bytes = response
            .bytes()
            .await
            .context("failed to read Supabase catalog body")?;
        let mut rows: Vec<SupabaseCatalogDocument> = serde_json::from_slice(&bytes)
            .with_context(|| format!("failed to parse Supabase catalog response from {url}"))?;
        let catalog = rows
            .pop()
            .map(|row| row.catalog)
            .context("Supabase catalog_documents.main row was not found")?;

        self.persist_catalog(&catalog)?;

        {
            let mut guard = self.catalog.write().expect("catalog poisoned for write");
            *guard = catalog;
        }

        info!("Catalog updated from Supabase.");
        Ok(true)
    }

    fn persist_catalog(&self, catalog: &ModelCatalog) -> Result<()> {
        let path = self.cached_catalog_path();
        let data = serde_json::to_vec_pretty(catalog)?;
        fs::write(&path, data)
            .with_context(|| format!("failed to write cached catalog to {path:?}"))?;
        Ok(())
    }

    fn cached_catalog_path(&self) -> PathBuf {
        self.config.cache_path().join(CACHED_CATALOG_FILE)
    }
}

#[derive(Debug)]
struct SupabaseCatalogSource {
    url: String,
    read_key: String,
}

impl SupabaseCatalogSource {
    fn from_env() -> Option<Self> {
        let url = runtime_or_build_env("ARCTIC_SUPABASE_URL")?;
        let read_key = runtime_or_build_env("ARCTIC_SUPABASE_ANON_KEY")
            .or_else(|| runtime_or_build_env("ARCTIC_SUPABASE_PUBLISHABLE_KEY"))?;

        Some(Self { url, read_key })
    }

    fn rest_url(&self) -> Result<Url> {
        let mut url = Url::parse(self.url.trim())
            .with_context(|| format!("invalid ARCTIC_SUPABASE_URL: {}", self.url))?;
        url.path_segments_mut()
            .map_err(|_| anyhow::anyhow!("ARCTIC_SUPABASE_URL cannot be a base URL"))?
            .extend(["rest", "v1", SUPABASE_CATALOG_TABLE]);
        url.query_pairs_mut()
            .append_pair("select", "catalog")
            .append_pair("catalog_key", &format!("eq.{SUPABASE_CATALOG_KEY}"))
            .append_pair("limit", "1");
        Ok(url)
    }
}

#[derive(Debug, Deserialize)]
struct SupabaseCatalogDocument {
    catalog: ModelCatalog,
}

fn runtime_or_build_env(name: &str) -> Option<String> {
    if let Ok(value) = std::env::var(name) {
        return non_empty(value);
    }

    match name {
        "ARCTIC_SUPABASE_URL" => option_env!("ARCTIC_SUPABASE_URL").and_then(non_empty),
        "ARCTIC_SUPABASE_ANON_KEY" => option_env!("ARCTIC_SUPABASE_ANON_KEY").and_then(non_empty),
        "ARCTIC_SUPABASE_PUBLISHABLE_KEY" => {
            option_env!("ARCTIC_SUPABASE_PUBLISHABLE_KEY").and_then(non_empty)
        }
        _ => None,
    }
}

fn non_empty(value: impl Into<String>) -> Option<String> {
    let value = value.into();
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

fn load_catalog_from_path(path: &PathBuf) -> Option<ModelCatalog> {
    match fs::read_to_string(path) {
        Ok(contents) => match serde_json::from_str::<ModelCatalog>(&contents) {
            Ok(parsed) => {
                info!("Loaded cached catalog from {:?}", path);
                Some(parsed)
            }
            Err(err) => {
                warn!("Failed to parse cached catalog at {:?}: {err}", path);
                None
            }
        },
        Err(err) => {
            warn!("Failed to read cached catalog at {:?}: {err}", path);
            None
        }
    }
}

fn load_cached_catalog(config: &ConfigStore) -> Option<ModelCatalog> {
    let path = cached_catalog_path(config);
    if path.exists() {
        load_catalog_from_path(&path)
    } else {
        None
    }
}

fn cached_catalog_path(config: &ConfigStore) -> PathBuf {
    config.cache_path().join(CACHED_CATALOG_FILE)
}

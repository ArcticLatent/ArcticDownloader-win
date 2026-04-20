use crate::app::APP_ID;
use anyhow::{anyhow, Context, Result};
use directories::BaseDirs;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::RwLock,
};

const SETTINGS_FILE: &str = "settings.json";

#[derive(Debug)]
pub struct ConfigStore {
    root_dir: PathBuf,
    config_dir: PathBuf,
    state_dir: PathBuf,
    cache_dir: PathBuf,
    settings: RwLock<AppSettings>,
}

impl ConfigStore {
    pub fn new() -> Result<Self> {
        let base = BaseDirs::new()
            .ok_or_else(|| anyhow!("unable to resolve base directories for {APP_ID}"))?;
        let root_dir = base.data_local_dir().join(APP_ID);
        let config_dir = root_dir.join("config");
        let state_dir = root_dir.join("state");
        let cache_dir = root_dir.join("cache");

        fs::create_dir_all(&config_dir)
            .with_context(|| format!("failed to create config directory {config_dir:?}"))?;

        fs::create_dir_all(&state_dir)
            .with_context(|| format!("failed to create state directory {state_dir:?}"))?;

        fs::create_dir_all(&cache_dir)
            .with_context(|| format!("failed to create cache directory {cache_dir:?}"))?;

        let settings_path = config_dir.join(SETTINGS_FILE);
        let settings = if settings_path.exists() {
            let data = fs::read(&settings_path)
                .with_context(|| format!("failed to read settings file {settings_path:?}"))?;
            serde_json::from_slice(&data)
                .with_context(|| format!("failed to parse settings from {settings_path:?}"))?
        } else {
            AppSettings::default()
        };

        Ok(Self {
            root_dir,
            config_dir,
            state_dir,
            cache_dir,
            settings: RwLock::new(settings),
        })
    }

    pub fn settings(&self) -> AppSettings {
        self.settings
            .read()
            .expect("settings lock poisoned")
            .clone()
    }

    pub fn update_settings<F>(&self, mutate: F) -> Result<AppSettings>
    where
        F: FnOnce(&mut AppSettings),
    {
        let mut guard = self
            .settings
            .write()
            .expect("settings lock poisoned for write");
        mutate(&mut guard);
        let snapshot = guard.clone();
        self.persist_locked(&snapshot)?;
        Ok(snapshot)
    }

    pub fn config_path(&self) -> PathBuf {
        self.config_dir.clone()
    }

    pub fn state_path(&self) -> Option<PathBuf> {
        Some(self.state_dir.clone())
    }

    pub fn cache_path(&self) -> PathBuf {
        self.cache_dir.clone()
    }

    pub fn root_path(&self) -> PathBuf {
        self.root_dir.clone()
    }

    fn persist_locked(&self, settings: &AppSettings) -> Result<()> {
        let path = self.config_path().join(SETTINGS_FILE);
        let data = serde_json::to_vec_pretty(settings)?;
        fs::write(&path, data).with_context(|| format!("failed to write settings to {path:?}"))?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AppSettings {
    pub comfyui_root: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comfyui_install_base: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comfyui_last_install_dir: Option<PathBuf>,
    pub prefer_quantized: bool,
    pub concurrent_downloads: usize,
    pub bandwidth_cap_mbps: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub civitai_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_installed_version: Option<String>,
    #[serde(default = "default_true")]
    pub comfyui_pinned_memory_enabled: bool,
    #[serde(default)]
    pub comfyui_listen_enabled: bool,
    #[serde(default)]
    pub comfyui_lowvram_enabled: bool,
    #[serde(default)]
    pub comfyui_bf16_unet_enabled: bool,
    #[serde(default)]
    pub comfyui_async_offload_enabled: bool,
    #[serde(default)]
    pub comfyui_disable_smart_memory_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comfyui_attention_backend: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comfyui_torch_profile: Option<String>,
    #[serde(default)]
    pub hf_xet_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_models_root: Option<PathBuf>,
    #[serde(default)]
    pub shared_models_use_default: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comfyui_custom_launch_args: String,
    #[serde(default = "default_true")]
    pub comfyui_show_runtime_logs: bool,
}

impl AppSettings {
    pub fn comfyui_root_valid(&self) -> Option<&Path> {
        self.comfyui_root
            .as_deref()
            .filter(|path| path.join("models").is_dir())
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            comfyui_root: None,
            comfyui_install_base: None,
            comfyui_last_install_dir: None,
            prefer_quantized: true,
            concurrent_downloads: 2,
            bandwidth_cap_mbps: None,
            civitai_token: None,
            last_installed_version: None,
            comfyui_pinned_memory_enabled: true,
            comfyui_listen_enabled: false,
            comfyui_lowvram_enabled: false,
            comfyui_bf16_unet_enabled: false,
            comfyui_async_offload_enabled: false,
            comfyui_disable_smart_memory_enabled: false,
            comfyui_attention_backend: None,
            comfyui_torch_profile: None,
            hf_xet_enabled: false,
            shared_models_root: None,
            shared_models_use_default: false,
            comfyui_custom_launch_args: String::new(),
            comfyui_show_runtime_logs: true,
        }
    }
}

fn default_true() -> bool {
    true
}

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use arctic_downloader::{
    app::{build_context, AppContext},
    config::AppSettings,
    download::{CivitaiPreview, DownloadSignal, DownloadStatus},
    env_flags::auto_update_enabled,
    model::{LoraDefinition, ModelCatalog, WorkflowDefinition},
    ram::{detect_ram_profile, RamTier},
};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, OnceLock,
    },
    time::{Duration, Instant},
};
use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State, WindowEvent,
};
use tokio_util::sync::CancellationToken;
use sha2::{Digest, Sha256};
use tauri_plugin_notification::NotificationExt;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

struct AppState {
    context: AppContext,
    active_cancel: Mutex<Option<CancellationToken>>,
    active_abort: Mutex<Option<tokio::task::AbortHandle>>,
    install_cancel: Mutex<Option<CancellationToken>>,
    comfyui_process: Mutex<Option<std::process::Child>>,
    quitting: Mutex<bool>,
}

#[derive(Debug, Serialize)]
struct AppSnapshot {
    version: String,
    total_ram_gb: Option<f64>,
    ram_tier: Option<String>,
    nvidia_gpu_name: Option<String>,
    nvidia_gpu_vram_mb: Option<u64>,
    amd_gpu_name: Option<String>,
    intel_gpu_name: Option<String>,
    model_count: usize,
    lora_count: usize,
}

#[derive(Debug, Serialize)]
struct UpdateCheckResponse {
    available: bool,
    version: Option<String>,
    notes: Option<String>,
}

#[derive(Debug, Serialize)]
struct HfXetPreflightResponse {
    xet_enabled: bool,
    hf_cli_available: bool,
    hf_backend: String,
    hf_xet_installed: bool,
    hub_version: Option<String>,
    detail: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BatchModelDownloadItem {
    model_id: String,
    variant_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BatchModelDownloadRequest {
    items: Vec<BatchModelDownloadItem>,
    ram_tier: Option<String>,
    comfyui_root: Option<String>,
}

#[derive(Debug, Serialize)]
struct LoraMetadataResponse {
    creator: String,
    creator_url: Option<String>,
    strength: String,
    triggers: Vec<String>,
    description: String,
    preview_url: Option<String>,
    preview_kind: String,
}

#[derive(Clone, Debug, Serialize)]
struct DownloadProgressEvent {
    kind: String,
    phase: String,
    artifact: Option<String>,
    index: Option<usize>,
    total: Option<usize>,
    received: Option<u64>,
    size: Option<u64>,
    folder: Option<String>,
    message: Option<String>,
}

#[derive(Debug, Serialize)]
struct ComfyInstallRecommendation {
    gpu_name: Option<String>,
    driver_version: Option<String>,
    torch_profile: String,
    torch_label: String,
    reason: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ComfyInstallRequest {
    install_root: String,
    #[serde(default)]
    extra_model_root: Option<String>,
    #[serde(default)]
    extra_model_use_default: bool,
    torch_profile: Option<String>,
    include_sage_attention: bool,
    include_sage_attention3: bool,
    include_flash_attention: bool,
    include_insight_face: bool,
    include_nunchaku: bool,
    #[serde(default)]
    include_trellis2: bool,
    #[serde(default = "default_true")]
    include_pinned_memory: bool,
    node_comfyui_manager: bool,
    node_comfyui_easy_use: bool,
    node_rgthree_comfy: bool,
    node_comfyui_gguf: bool,
    node_comfyui_kjnodes: bool,
    #[serde(default)]
    node_comfyui_crystools: bool,
    #[serde(default)]
    force_fresh: bool,
}

#[derive(Debug, Serialize)]
struct PreflightItem {
    status: String, // pass | warn | fail
    title: String,
    detail: String,
}

#[derive(Debug, Serialize)]
struct ComfyPreflightResponse {
    ok: bool,
    summary: String,
    items: Vec<PreflightItem>,
}

#[derive(Debug, Serialize)]
struct ComfyResumeStateResponse {
    found: bool,
    install_dir: Option<String>,
    step: Option<String>,
    summary: String,
}

#[derive(Debug, Serialize)]
struct ComfyPathInspection {
    selected: String,
    detected_root: Option<String>,
}

#[derive(Debug, Serialize)]
struct ComfyInstallationEntry {
    name: String,
    root: String,
}

#[derive(Debug, Serialize)]
struct ComfyUiUpdateStatus {
    installed_version: Option<String>,
    latest_version: Option<String>,
    head_matches_latest_tag: bool,
    update_available: bool,
    checked: bool,
    detail: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct InstallState {
    status: String, // in_progress | completed
    step: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstallSummaryItem {
    name: String,
    status: String, // ok | failed | skipped
    detail: String,
}

const UV_PYTHON_VERSION: &str = "3.12.10";
const UV_PYTHON_FALLBACK: &str = "3.12";
fn default_true() -> bool {
    true
}

#[tauri::command]
fn get_app_snapshot(state: State<'_, AppState>) -> AppSnapshot {
    let catalog = state.context.catalog.catalog_snapshot();
    let (nvidia_gpu_name, nvidia_gpu_vram_mb) = detect_nvidia_gpu();
    let amd_gpu_name = detect_amd_gpu_name();
    let intel_gpu_name = detect_intel_gpu_name();
    let ram_profile = state.context.ram_profile.or_else(detect_ram_profile);
    AppSnapshot {
        version: state.context.display_version.clone(),
        total_ram_gb: ram_profile.map(|profile| profile.total_gb),
        ram_tier: ram_profile.map(|profile| profile.tier.label().to_string()),
        nvidia_gpu_name,
        nvidia_gpu_vram_mb,
        amd_gpu_name,
        intel_gpu_name,
        model_count: catalog.models.len(),
        lora_count: catalog.loras.len(),
    }
}

fn detect_nvidia_gpu() -> (Option<String>, Option<u64>) {
    let detailed = detect_nvidia_gpu_details();
    (detailed.name, detailed.vram_mb)
}

#[derive(Clone, Debug, Default)]
struct NvidiaGpuDetails {
    name: Option<String>,
    vram_mb: Option<u64>,
    driver_version: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct AmdGpuDetails {
    name: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct IntelGpuDetails {
    name: Option<String>,
}

static GPU_DETAILS_CACHE: OnceLock<Mutex<Option<NvidiaGpuDetails>>> = OnceLock::new();
static GPU_DETAILS_PROBE_STARTED: AtomicBool = AtomicBool::new(false);
static AMD_GPU_DETAILS_CACHE: OnceLock<Mutex<Option<AmdGpuDetails>>> = OnceLock::new();
static AMD_GPU_DETAILS_PROBE_STARTED: AtomicBool = AtomicBool::new(false);
static INTEL_GPU_DETAILS_CACHE: OnceLock<Mutex<Option<IntelGpuDetails>>> = OnceLock::new();
static INTEL_GPU_DETAILS_PROBE_STARTED: AtomicBool = AtomicBool::new(false);
static TRAY_MENU_ITEMS: OnceLock<Mutex<Option<TrayMenuItems>>> = OnceLock::new();

struct TrayMenuItems {
    start: MenuItem<tauri::Wry>,
    stop: MenuItem<tauri::Wry>,
}

fn tray_menu_items() -> &'static Mutex<Option<TrayMenuItems>> {
    TRAY_MENU_ITEMS.get_or_init(|| Mutex::new(None))
}

fn gpu_details_cache() -> &'static Mutex<Option<NvidiaGpuDetails>> {
    GPU_DETAILS_CACHE.get_or_init(|| Mutex::new(None))
}

fn amd_gpu_details_cache() -> &'static Mutex<Option<AmdGpuDetails>> {
    AMD_GPU_DETAILS_CACHE.get_or_init(|| Mutex::new(None))
}

fn intel_gpu_details_cache() -> &'static Mutex<Option<IntelGpuDetails>> {
    INTEL_GPU_DETAILS_CACHE.get_or_init(|| Mutex::new(None))
}

fn query_nvidia_gpu_details_blocking() -> NvidiaGpuDetails {
    let mut cmd = std::process::Command::new("nvidia-smi");
    cmd.args([
        "--query-gpu=name,memory.total,driver_version",
        "--format=csv,noheader,nounits",
    ]);
    apply_background_command_flags(&mut cmd);
    let output = cmd.output();

    let Ok(output) = output else {
        return NvidiaGpuDetails::default();
    };
    if !output.status.success() {
        return NvidiaGpuDetails::default();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let first = stdout
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    if first.is_empty() {
        return NvidiaGpuDetails::default();
    }

    let mut parts = first.split(',').map(str::trim);
    let name = parts
        .next()
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let vram_mb = parts
        .next()
        .and_then(|value| value.parse::<u64>().ok());
    let driver_version = parts
        .next()
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    NvidiaGpuDetails {
        name,
        vram_mb,
        driver_version,
    }
}

fn detect_nvidia_gpu_details() -> NvidiaGpuDetails {
    if let Ok(guard) = gpu_details_cache().lock() {
        if let Some(details) = guard.clone() {
            return details;
        }
    }

    if !GPU_DETAILS_PROBE_STARTED.swap(true, Ordering::SeqCst) {
        std::thread::spawn(|| {
            let details = query_nvidia_gpu_details_blocking();
            if let Ok(mut guard) = gpu_details_cache().lock() {
                *guard = Some(details);
            }
        });
    }

    NvidiaGpuDetails::default()
}

fn query_amd_gpu_details_blocking() -> AmdGpuDetails {
    let script = "$gpu = Get-CimInstance Win32_VideoController | Where-Object { $_.Name -match 'AMD|Radeon|Ryzen AI|RyzenAI|ATI' } | Select-Object -First 1 -ExpandProperty Name; if ($gpu) { $gpu }";
    let mut cmd = std::process::Command::new("powershell");
    cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", script]);
    apply_background_command_flags(&mut cmd);
    let output = match cmd.output() {
        Ok(output) => output,
        Err(_) => return AmdGpuDetails::default(),
    };
    if !output.status.success() {
        return AmdGpuDetails::default();
    }
    let name = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned);
    AmdGpuDetails { name }
}

fn detect_amd_gpu_details() -> AmdGpuDetails {
    if let Ok(guard) = amd_gpu_details_cache().lock() {
        if let Some(details) = guard.clone() {
            return details;
        }
    }

    if !AMD_GPU_DETAILS_PROBE_STARTED.swap(true, Ordering::SeqCst) {
        std::thread::spawn(|| {
            let details = query_amd_gpu_details_blocking();
            if let Ok(mut guard) = amd_gpu_details_cache().lock() {
                if details.name.is_some() {
                    *guard = Some(details);
                } else {
                    *guard = None;
                    AMD_GPU_DETAILS_PROBE_STARTED.store(false, Ordering::SeqCst);
                }
            }
        });
    }

    AmdGpuDetails::default()
}

fn detect_amd_gpu_name() -> Option<String> {
    if fake_amd_enabled() {
        return Some("Fake AMD GPU (simulation)".to_string());
    }
    detect_amd_gpu_details().name
}

fn query_intel_gpu_details_blocking() -> IntelGpuDetails {
    let script = "$gpu = Get-CimInstance Win32_VideoController | Where-Object { $_.Name -match 'Intel|Arc|Iris|UHD|Xe' } | Select-Object -First 1 -ExpandProperty Name; if ($gpu) { $gpu }";
    let mut cmd = std::process::Command::new("powershell");
    cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", script]);
    apply_background_command_flags(&mut cmd);
    let output = match cmd.output() {
        Ok(output) => output,
        Err(_) => return IntelGpuDetails::default(),
    };
    if !output.status.success() {
        return IntelGpuDetails::default();
    }
    let name = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned);
    IntelGpuDetails { name }
}

fn detect_intel_gpu_details() -> IntelGpuDetails {
    if let Ok(guard) = intel_gpu_details_cache().lock() {
        if let Some(details) = guard.clone() {
            return details;
        }
    }

    if !INTEL_GPU_DETAILS_PROBE_STARTED.swap(true, Ordering::SeqCst) {
        std::thread::spawn(|| {
            let details = query_intel_gpu_details_blocking();
            if let Ok(mut guard) = intel_gpu_details_cache().lock() {
                if details.name.is_some() {
                    *guard = Some(details);
                } else {
                    *guard = None;
                    INTEL_GPU_DETAILS_PROBE_STARTED.store(false, Ordering::SeqCst);
                }
            }
        });
    }

    IntelGpuDetails::default()
}

fn detect_intel_gpu_name() -> Option<String> {
    if fake_intel_enabled() {
        return Some("Fake Intel GPU (simulation)".to_string());
    }
    detect_intel_gpu_details().name
}

fn windows_rocm_supported_gpu(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "radeon rx 9070",
        "radeon rx 9060",
        "radeon rx 7900",
        "radeon rx 7800",
        "radeon rx 7700",
        "radeon ai pro r9700",
        "ryzen ai max",
        "ryzen ai 9 hx 370",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn windows_xpu_supported_gpu(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "intel arc",
        "arc a",
        "arc b",
        "arc 1",
        "core ultra",
        "iris xe",
        "intel(r) iris",
        "intel(r) uhd",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

#[tauri::command]
fn get_comfyui_install_recommendation() -> ComfyInstallRecommendation {
    let gpu = detect_nvidia_gpu_details();
    let gpu_name = gpu.name.clone().unwrap_or_default().to_ascii_lowercase();
    if let Some(amd_name) = detect_amd_gpu_name() {
        let reason = if windows_rocm_supported_gpu(&amd_name) {
            "Detected supported AMD GPU; selecting Windows ROCm install profile."
                .to_string()
        } else {
            "Detected AMD GPU. Windows ROCm support is limited to specific Radeon and Ryzen AI hardware."
                .to_string()
        };
        return ComfyInstallRecommendation {
            gpu_name: Some(amd_name),
            driver_version: None,
            torch_profile: "torch291_rocm72".to_string(),
            torch_label: "Torch 2.9.1 + ROCm SDK 7.2".to_string(),
            reason,
        };
    }
    if gpu_name.is_empty() {
        if let Some(intel_name) = detect_intel_gpu_name() {
            let reason = if windows_xpu_supported_gpu(&intel_name) {
                "Detected Intel GPU; selecting PyTorch XPU Nightly install profile."
                    .to_string()
            } else {
                "Detected Intel GPU. Windows XPU support works best on Intel Arc and newer Intel integrated GPUs."
                    .to_string()
            };
            return ComfyInstallRecommendation {
                gpu_name: Some(intel_name),
                driver_version: None,
                torch_profile: "torchxpu_nightly".to_string(),
                torch_label: "PyTorch XPU Nightly".to_string(),
                reason,
            };
        }
    }
    let driver_major = gpu
        .driver_version
        .as_deref()
        .and_then(|raw| raw.split('.').next())
        .and_then(|raw| raw.parse::<u64>().ok())
        .unwrap_or_default();

    if gpu_name.contains("rtx 30") {
        return ComfyInstallRecommendation {
            gpu_name: gpu.name,
            driver_version: gpu.driver_version,
            torch_profile: "torch271_cu128".to_string(),
            torch_label: "Torch 2.7.1 + cu128".to_string(),
            reason: "Detected RTX 3000 series (Ampere).".to_string(),
        };
    }

    if gpu_name.contains("rtx 40") {
        return ComfyInstallRecommendation {
            gpu_name: gpu.name,
            driver_version: gpu.driver_version,
            torch_profile: "torch280_cu128".to_string(),
            torch_label: "Torch 2.8.0 + cu128".to_string(),
            reason: "Detected RTX 4000 series (Ada).".to_string(),
        };
    }

    if gpu_name.contains("rtx 50") {
        if driver_major >= 580 {
            return ComfyInstallRecommendation {
                gpu_name: gpu.name,
                driver_version: gpu.driver_version,
                torch_profile: "torch291_cu130".to_string(),
                torch_label: "Torch 2.9.1 + cu130".to_string(),
                reason: "Detected RTX 5000 series with driver >= 580.".to_string(),
            };
        }

        return ComfyInstallRecommendation {
            gpu_name: gpu.name,
            driver_version: gpu.driver_version,
            torch_profile: "torch280_cu128".to_string(),
            torch_label: "Torch 2.8.0 + cu128".to_string(),
            reason: "Detected RTX 5000 series with older driver; using safer fallback.".to_string(),
        };
    }

    ComfyInstallRecommendation {
        gpu_name: gpu.name,
        driver_version: gpu.driver_version,
        torch_profile: "torch280_cu128".to_string(),
        torch_label: "Torch 2.8.0 + cu128".to_string(),
        reason: "Unknown or non-NVIDIA GPU; using default recommendation.".to_string(),
    }
}

fn normalize_path(raw: &str) -> Result<PathBuf, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Install folder is required.".to_string());
    }
    let mut path = PathBuf::from(trimmed);
    if !path.is_absolute() {
        path = std::env::current_dir()
            .map_err(|err| err.to_string())?
            .join(path);
    }
    Ok(strip_windows_verbatim_prefix(&path))
}

fn normalize_optional_path(raw: Option<&str>) -> Result<Option<PathBuf>, String> {
    let Some(value) = raw else {
        return Ok(None);
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    normalize_path(trimmed).map(Some)
}

fn yaml_single_quote(input: &str) -> String {
    format!("'{}'", input.replace('\'', "''"))
}

fn write_extra_model_paths_yaml(
    comfy_dir: &Path,
    base_path: &Path,
    is_default: bool,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(base_path).map_err(|err| {
        format!(
            "failed to prepare extra models folder '{}': {err}",
            base_path.display()
        )
    })?;

    let target = comfy_dir.join("extra_model_paths.yaml");
    let example = comfy_dir.join("extra_model_paths.yaml.example");
    if !target.exists() {
        if example.exists() {
            std::fs::rename(&example, &target).map_err(|err| {
                format!(
                    "failed to rename '{}' to '{}': {err}",
                    example.display(),
                    target.display()
                )
            })?;
        } else {
            return Err(
                "extra_model_paths.yaml.example was not found in ComfyUI install folder."
                    .to_string(),
            );
        }
    }

    let base = yaml_single_quote(&strip_windows_verbatim_prefix(base_path).to_string_lossy());
    let default_value = if is_default { "true" } else { "false" };
    let yaml = format!(
        r#"# Managed by Arctic ComfyUI Helper.
comfyui:
  base_path: {base}
  is_default: {default_value}
  checkpoints: models/checkpoints/
  text_encoders: |
    models/text_encoders/
    models/clip/
  clip_vision: models/clip_vision/
  configs: models/configs/
  controlnet: models/controlnet/
  diffusion_models: |
    models/diffusion_models/
    models/unet/
  embeddings: models/embeddings/
  loras: models/loras/
  upscale_models: models/upscale_models/
  vae: models/vae/
  audio_encoders: models/audio_encoders/
  model_patches: models/model_patches/
"#
    );

    std::fs::write(&target, yaml).map_err(|err| {
        format!(
            "failed to write extra model paths config '{}': {err}",
            target.display()
        )
    })?;

    Ok(target)
}

fn is_forbidden_install_path(path: &Path) -> bool {
    let normalized = path
        .to_string_lossy()
        .to_ascii_lowercase()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_string();

    if normalized == "c:" {
        return true;
    }

    let blocked_prefixes = ["c:\\windows", "c:\\program files", "c:\\program files (x86)"];
    blocked_prefixes
        .iter()
        .any(|entry| normalized == *entry || normalized.starts_with(&format!("{entry}\\")))
}

fn find_in_progress_install(base_root: &Path) -> Option<(PathBuf, InstallState)> {
    if let Ok(entries) = std::fs::read_dir(base_root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !(name == "ComfyUI" || (name.starts_with("ComfyUI-") && name.len() == "ComfyUI-00".len())) {
                continue;
            }
            let state_path = path.join(".arctic_install_state.json");
            if !state_path.exists() {
                continue;
            }
            let data = match std::fs::read(&state_path) {
                Ok(d) => d,
                Err(_) => continue,
            };
            let parsed: InstallState = match serde_json::from_slice(&data) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if parsed.status == "in_progress" {
                return Some((path, parsed));
            }
        }
    }
    None
}

fn choose_install_folder(base_root: &Path, force_fresh: bool) -> PathBuf {
    if !force_fresh {
        if let Some((existing, _)) = find_in_progress_install(base_root) {
            return existing;
        }
    }

    for index in 1..=99u32 {
        let candidate = base_root.join(format!("ComfyUI-{index:02}"));
        if !candidate.exists() {
            return candidate;
        }
    }

    // Extremely unlikely fallback if 01..99 are occupied.
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    base_root.join(format!("ComfyUI-{ts}"))
}

fn path_name_is_comfyui(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|name| {
            let lower = name.to_ascii_lowercase();
            lower == "comfyui" || lower.starts_with("comfyui-")
        })
        .unwrap_or(false)
}

fn is_empty_dir(path: &Path) -> bool {
    std::fs::read_dir(path)
        .ok()
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(false)
}

fn is_recoverable_preclone_dir(path: &Path) -> bool {
    let allowed = [
        ".venv",
        ".python",
        ".tools",
        ".arctic_install_state.json",
        "install.log",
        "install-summary.json",
    ];
    let Ok(entries) = std::fs::read_dir(path) else {
        return false;
    };
    entries.flatten().all(|entry| {
        entry
            .file_name()
            .to_str()
            .map(|name| allowed.iter().any(|item| item.eq_ignore_ascii_case(name)))
            .unwrap_or(false)
    })
}

fn clear_directory_contents(path: &Path) -> Result<(), String> {
    let entries = std::fs::read_dir(path).map_err(|err| err.to_string())?;
    for entry in entries.flatten() {
        let p = entry.path();
        let keep = p
            .file_name()
            .and_then(|n| n.to_str())
            .map(|name| name.eq_ignore_ascii_case(".tools") || name.eq_ignore_ascii_case(".python"))
            .unwrap_or(false);
        if keep {
            continue;
        }
        if p.is_dir() {
            std::fs::remove_dir_all(&p).map_err(|err| err.to_string())?;
        } else {
            std::fs::remove_file(&p).map_err(|err| err.to_string())?;
        }
    }
    Ok(())
}

fn strip_windows_verbatim_prefix(path: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let raw = path.to_string_lossy().to_string();
        if let Some(stripped) = raw.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{}", stripped));
        }
        if let Some(stripped) = raw.strip_prefix(r"\\?\") {
            return PathBuf::from(stripped);
        }
        return PathBuf::from(raw);
    }

    #[cfg(not(target_os = "windows"))]
    {
        path.to_path_buf()
    }
}

fn write_install_state(install_root: &Path, status: &str, step: &str) {
    let path = install_root.join(".arctic_install_state.json");
    let payload = InstallState {
        status: status.to_string(),
        step: step.to_string(),
    };
    if let Ok(data) = serde_json::to_vec_pretty(&payload) {
        let _ = std::fs::write(path, data);
    }
}

fn push_preflight(items: &mut Vec<PreflightItem>, status: &str, title: &str, detail: impl Into<String>) {
    items.push(PreflightItem {
        status: status.to_string(),
        title: title.to_string(),
        detail: detail.into(),
    });
}

fn command_available(program: &str, args: &[&str]) -> bool {
    let mut cmd = std::process::Command::new(program);
    cmd.args(args);
    apply_background_command_flags(&mut cmd);
    cmd.output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn apply_background_command_flags(cmd: &mut std::process::Command) {
    #[cfg(target_os = "windows")]
    {
        // Prevent Windows from opening a new console window per installer subprocess.
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
}

fn nerdstats_enabled() -> bool {
    std::env::var("ARCTIC_NERDSTATS")
        .map(|value| value == "1")
        .unwrap_or(false)
}

fn fake_amd_enabled() -> bool {
    std::env::var("ARCTIC_FAKE_AMD")
        .map(|value| value == "1")
        .unwrap_or(false)
}

fn fake_intel_enabled() -> bool {
    std::env::var("ARCTIC_FAKE_INTEL")
        .map(|value| value == "1")
        .unwrap_or(false)
}

#[cfg(target_os = "windows")]
fn try_attach_parent_console() {
    // ATTACH_PARENT_PROCESS from Win32 API
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    unsafe extern "system" {
        fn AttachConsole(dw_process_id: u32) -> i32;
    }
    // Best-effort: if no parent console exists, this simply fails.
    let _ = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

#[cfg(not(target_os = "windows"))]
fn try_attach_parent_console() {}

fn refresh_git_path_for_current_process() {
    #[cfg(target_os = "windows")]
    {
        let mut values: Vec<String> = std::env::var_os("PATH")
            .map(|value| {
                std::env::split_paths(&value)
                    .map(|p| p.to_string_lossy().to_string())
                    .collect()
            })
            .unwrap_or_default();

        let mut add_candidate = |path: PathBuf| {
            if !path.exists() {
                return;
            }
            let value = path.to_string_lossy().to_string();
            if !values
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(&value))
            {
                values.push(value);
            }
        };

        if let Some(program_files) = std::env::var_os("ProgramFiles") {
            add_candidate(PathBuf::from(program_files).join("Git").join("cmd"));
        }
        if let Some(program_files_x86) = std::env::var_os("ProgramFiles(x86)") {
            add_candidate(PathBuf::from(program_files_x86).join("Git").join("cmd"));
        }
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            add_candidate(
                PathBuf::from(local_app_data)
                    .join("Programs")
                    .join("Git")
                    .join("cmd"),
            );
        }

        if let Ok(joined) = std::env::join_paths(values.iter().map(PathBuf::from)) {
            std::env::set_var("PATH", joined);
        }
    }
}

fn ensure_git_available(app: &AppHandle) -> Result<(), String> {
    if command_available("git", &["--version"]) {
        return Ok(());
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
        return Err("Git is not available in PATH. Install Git and retry.".to_string());
    }

    #[cfg(target_os = "windows")]
    {
        if !command_available("winget", &["--version"]) {
            return Err(
                "Git is missing and winget is unavailable. Install Git manually and retry."
                    .to_string(),
            );
        }

        emit_install_event(app, "step", "Git not found; installing Git via winget...");
        let mut winget_cmd = std::process::Command::new("winget");
        winget_cmd.args([
            "install",
            "--id",
            "Git.Git",
            "-e",
            "--source",
            "winget",
            "--accept-package-agreements",
            "--accept-source-agreements",
        ]);
        apply_background_command_flags(&mut winget_cmd);
        let status = winget_cmd
            .status()
            .map_err(|err| format!("Failed to launch winget: {err}"))?;

        if !status.success() {
            return Err("Git installation via winget failed. Install Git manually and retry.".to_string());
        }

        refresh_git_path_for_current_process();
        if command_available("git", &["--version"]) {
            emit_install_event(app, "info", "Git installed successfully.");
            Ok(())
        } else {
            Err("Git installed but not available in PATH for this session. Restart app and retry.".to_string())
        }
    }
}

fn has_dns(host: &str, port: u16) -> bool {
    (host, port)
        .to_socket_addrs()
        .map(|mut it| it.next().is_some())
        .unwrap_or(false)
}

fn parse_hf_env_value(text: &str, key: &str) -> Option<String> {
    let prefix = format!("- {key}:");
    text.lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix(&prefix).map(str::trim))
        .map(str::to_string)
}

fn prepend_path_entry_if_missing(entry: &Path) {
    let abs_entry = match std::fs::canonicalize(entry) {
        Ok(path) => path,
        Err(_) => entry.to_path_buf(),
    };
    let mut values: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default();
    let entry_s = abs_entry.to_string_lossy().to_ascii_lowercase();
    let already_present = values.iter().any(|p| {
        p.to_string_lossy().to_ascii_lowercase() == entry_s
    });
    if already_present {
        return;
    }
    values.insert(0, abs_entry);
    if let Ok(joined) = std::env::join_paths(values) {
        std::env::set_var("PATH", joined);
    }
}

fn add_local_uv_tools_to_path(shared_runtime_root: &Path) {
    let local_root = shared_runtime_root.join(".tools").join("uv");
    if local_root.exists() {
        prepend_path_entry_if_missing(&local_root);
    }
    if let Some(found) = find_file_recursive(&local_root, "uv.exe") {
        if let Some(parent) = found.parent() {
            prepend_path_entry_if_missing(parent);
        }
    }
    if let Some(legacy_runtime_root) = shared_runtime_root
        .parent()
        .map(|parent| parent.join("comfy_runtime"))
    {
        let legacy_local_root = legacy_runtime_root.join(".tools").join("uv");
        if legacy_local_root.exists() {
            prepend_path_entry_if_missing(&legacy_local_root);
        }
        if let Some(found) = find_file_recursive(&legacy_local_root, "uv.exe") {
            if let Some(parent) = found.parent() {
                prepend_path_entry_if_missing(parent);
            }
        }
    }
}

fn get_hf_xet_preflight_internal(xet_enabled: bool) -> HfXetPreflightResponse {
    let uvx_hf_available = command_available("uvx", &["hf", "--help"]);
    let hf_native_available = command_available("hf", &["--help"]);
    let hf_cli_available = uvx_hf_available || hf_native_available;
    let hf_backend = if uvx_hf_available {
        "uvx hf".to_string()
    } else if hf_native_available {
        "hf".to_string()
    } else {
        "none".to_string()
    };

    if !hf_cli_available {
        return HfXetPreflightResponse {
            xet_enabled,
            hf_cli_available,
            hf_backend,
            hf_xet_installed: false,
            hub_version: None,
            detail: "HF CLI backend not found. Install uv (`https://docs.astral.sh/uv/`) for `uvx hf`, or install `hf` (`pip install -U huggingface_hub hf_xet`).".to_string(),
        };
    }

    let env_probe = if uvx_hf_available {
        run_command_capture("uvx", &["hf", "env"], None)
    } else {
        run_command_capture("hf", &["env"], None)
    };

    match env_probe {
        Ok((stdout, _stderr)) => {
            let hf_xet_raw = parse_hf_env_value(&stdout, "hf_xet").unwrap_or_default();
            let hub_version = parse_hf_env_value(&stdout, "huggingface_hub version");
            let hf_xet_installed = {
                let normalized = hf_xet_raw.trim().to_ascii_lowercase();
                !normalized.is_empty() && normalized != "n/a" && normalized != "none"
            };

            let detail = if !xet_enabled {
                format!(
                    "Xet is installed but disabled in app settings (backend: {}).",
                    hf_backend
                )
            } else if hf_xet_installed {
                format!(
                    "HF/Xet preflight OK via {} (huggingface_hub {}, hf_xet {}).",
                    hf_backend,
                    hub_version.clone().unwrap_or_else(|| "unknown".to_string()),
                    hf_xet_raw
                )
            } else {
                format!(
                    "HF backend {} found, but hf_xet is missing. Run `pip install -U huggingface_hub hf_xet`.",
                    hf_backend
                )
            };

            HfXetPreflightResponse {
                xet_enabled,
                hf_cli_available,
                hf_backend,
                hf_xet_installed,
                hub_version,
                detail,
            }
        }
        Err(err) => HfXetPreflightResponse {
            xet_enabled,
            hf_cli_available,
            hf_backend,
            hf_xet_installed: false,
            hub_version: None,
            detail: format!("Could not run HF env probe: {err}"),
        },
    }
}

#[tauri::command]
fn get_hf_xet_preflight(state: State<'_, AppState>) -> HfXetPreflightResponse {
    let shared_runtime_root = state.context.config.cache_path().join("comfyui-runtime");
    add_local_uv_tools_to_path(&shared_runtime_root);
    let xet_enabled = state.context.config.settings().hf_xet_enabled;
    get_hf_xet_preflight_internal(xet_enabled)
}

fn ensure_hf_xet_runtime_installed(
    app: &AppHandle,
    shared_runtime_root: &Path,
    always_upgrade: bool,
) -> Result<(), String> {
    add_local_uv_tools_to_path(shared_runtime_root);
    let before = get_hf_xet_preflight_internal(true);

    let mut attempts: Vec<String> = Vec::new();
    let uv_bin = resolve_uv_binary(shared_runtime_root, app)?;
    if uv_bin != "uv" {
        if let Some(parent) = Path::new(&uv_bin).parent() {
            prepend_path_entry_if_missing(parent);
        }
    }
    if always_upgrade || !before.hf_xet_installed {
        match run_command_capture(
            &uv_bin,
            &[
                "tool",
                "install",
                "--upgrade",
                "--force",
                "huggingface_hub[hf_xet]",
            ],
            None,
        ) {
            Ok(_) => attempts.push(
                "uv tool install --upgrade --force huggingface_hub[hf_xet] => ok".to_string(),
            ),
            Err(err) => {
                attempts.push(format!(
                    "{} tool install --upgrade --force huggingface_hub[hf_xet] => {err}",
                    uv_bin
                ));
            }
        }
    }

    add_local_uv_tools_to_path(shared_runtime_root);
    let after = get_hf_xet_preflight_internal(true);
    if after.hf_cli_available && after.hf_xet_installed {
        Ok(())
    } else {
        Err(format!(
            "Could not prepare HF/Xet runtime. {}. attempts: {}",
            after.detail,
            attempts.join(" | ")
        ))
    }
}

#[tauri::command]
fn set_hf_xet_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<AppSettings, String> {
    if enabled {
        let shared_runtime_root = state.context.config.cache_path().join("comfyui-runtime");
        ensure_hf_xet_runtime_installed(&app, &shared_runtime_root, true)?;
    }
    state
        .context
        .config
        .update_settings(|settings| settings.hf_xet_enabled = enabled)
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn run_comfyui_preflight(state: State<'_, AppState>, request: ComfyInstallRequest) -> ComfyPreflightResponse {
    let mut items: Vec<PreflightItem> = Vec::new();
    let mut ok = true;

    if request.install_root.trim().is_empty() {
        push_preflight(
            &mut items,
            "warn",
            "Install base folder",
            "Select an install folder to run full preflight checks.",
        );
        return ComfyPreflightResponse {
            ok: false,
            summary: "Install folder not selected yet.".to_string(),
            items,
        };
    }

    let base_root = match normalize_path(&request.install_root) {
        Ok(path) => path,
        Err(err) => {
            push_preflight(&mut items, "fail", "Install base folder", err);
            return ComfyPreflightResponse {
                ok: false,
                summary: "Preflight failed.".to_string(),
                items,
            };
        }
    };

    if is_forbidden_install_path(&base_root) {
        ok = false;
        push_preflight(
            &mut items,
            "fail",
            "Install base folder",
            "Folder is blocked (avoid C:\\, Windows, Program Files).",
        );
    } else {
        push_preflight(
            &mut items,
            "pass",
            "Install base folder",
            format!("Using {}", base_root.display()),
        );
    }

    if std::fs::create_dir_all(&base_root).is_ok() {
        let probe = base_root.join(".arctic-write-test");
        match std::fs::write(&probe, b"ok") {
            Ok(_) => {
                let _ = std::fs::remove_file(&probe);
                push_preflight(&mut items, "pass", "Write permission", "Folder is writable.");
            }
            Err(err) => {
                ok = false;
                push_preflight(
                    &mut items,
                    "fail",
                    "Write permission",
                    format!("Cannot write to selected folder: {err}"),
                );
            }
        }
    } else {
        ok = false;
        push_preflight(
            &mut items,
            "fail",
            "Write permission",
            "Could not create selected base folder.",
        );
    }

    match fs2::available_space(&base_root) {
        Ok(bytes) => {
            let gb = bytes as f64 / 1024f64 / 1024f64 / 1024f64;
            if gb < 40.0 {
                ok = false;
                push_preflight(
                    &mut items,
                    "fail",
                    "Disk space",
                    format!("Only {gb:.1} GB free. Recommended at least 40 GB."),
                );
            } else if gb < 80.0 {
                push_preflight(
                    &mut items,
                    "warn",
                    "Disk space",
                    format!("{gb:.1} GB free. Installation should work but more free space is safer."),
                );
            } else {
                push_preflight(&mut items, "pass", "Disk space", format!("{gb:.1} GB free."));
            }
        }
        Err(err) => {
            push_preflight(
                &mut items,
                "warn",
                "Disk space",
                format!("Unable to check free space: {err}"),
            );
        }
    }

    if command_available("git", &["--version"]) {
        push_preflight(&mut items, "pass", "Git", "Git is available.");
    } else if command_available("winget", &["--version"]) {
        push_preflight(
            &mut items,
            "warn",
            "Git",
            "Git is missing in PATH. Installer will attempt winget install automatically.",
        );
    } else {
        ok = false;
        push_preflight(
            &mut items,
            "fail",
            "Git",
            "Git is not available and winget is missing. Install Git manually.",
        );
    }

    let dns_ok = has_dns("github.com", 443) && has_dns("pypi.org", 443);
    if dns_ok {
        push_preflight(&mut items, "pass", "Network", "DNS lookup for required hosts is available.");
    } else {
        push_preflight(
            &mut items,
            "warn",
            "Network",
            "Could not resolve one or more hosts (github.com, pypi.org). Install may fail offline.",
        );
    }

    let cache_root = state.context.config.cache_path();
    let runtime_roots = [
        cache_root.join("comfyui-runtime"),
        cache_root.join("comfy_runtime"),
    ];
    let local_uv_exists = runtime_roots.iter().any(|runtime_root| {
        let local_uv_root = runtime_root.join(".tools").join("uv");
        local_uv_root.join("uv.exe").exists()
            || local_uv_root.join("uv").exists()
            || find_file_recursive(&local_uv_root, "uv.exe").is_some()
            || find_file_recursive(&local_uv_root, "uv").is_some()
    });

    if command_available("uv", &["--version"]) {
        push_preflight(&mut items, "pass", "uv runtime", "System uv detected.");
    } else if local_uv_exists {
        push_preflight(
            &mut items,
            "pass",
            "uv runtime",
            "Local uv runtime already available.",
        );
    } else {
        push_preflight(
            &mut items,
            "warn",
            "uv runtime",
            "System uv not found. Installer will download a local uv runtime.",
        );
    }

    let selected_attention = [
        request.include_sage_attention,
        request.include_sage_attention3,
        request.include_flash_attention,
        request.include_nunchaku,
    ]
    .into_iter()
    .filter(|v| *v)
    .count();
    if selected_attention > 1 {
        ok = false;
        push_preflight(
            &mut items,
            "fail",
            "Attention add-on selection",
            "Select only one of SageAttention / SageAttention3 / FlashAttention / Nunchaku.",
        );
    } else {
        push_preflight(
            &mut items,
            "pass",
            "Attention add-on selection",
            "Selection is valid.",
        );
    }

    if request.include_sage_attention3 {
        let gpu = detect_nvidia_gpu_details();
        let allowed = gpu
            .name
            .as_deref()
            .map(|n| n.to_ascii_lowercase().contains("rtx 50"))
            .unwrap_or(false);
        if allowed {
            push_preflight(
                &mut items,
                "pass",
                "SageAttention3 compatibility",
                "RTX 50-series detected.",
            );
        } else {
            ok = false;
            push_preflight(
                &mut items,
                "fail",
                "SageAttention3 compatibility",
                "SageAttention3 requires NVIDIA RTX 50-series.",
            );
        }
    }

    if request.include_trellis2 {
        let recommendation = get_comfyui_install_recommendation();
        let selected_profile = request
            .torch_profile
            .clone()
            .unwrap_or(recommendation.torch_profile);
        let trellis_supported = matches!(selected_profile.as_str(), "torch280_cu128");
        if trellis_supported {
            push_preflight(
                &mut items,
                "pass",
                "Trellis2 compatibility",
                "Compatible Torch profile selected.",
            );
        } else {
            ok = false;
            push_preflight(
                &mut items,
                "fail",
                "Trellis2 compatibility",
                "Trellis2 currently requires Torch 2.8.0 + cu128 (Torch280 wheel set).",
            );
        }
    }

    let summary = if ok {
        "Preflight passed.".to_string()
    } else {
        "Preflight has blocking issues.".to_string()
    };
    ComfyPreflightResponse { ok, summary, items }
}

#[tauri::command]
fn get_comfyui_resume_state(
    state: State<'_, AppState>,
    install_base: Option<String>,
) -> Result<ComfyResumeStateResponse, String> {
    let base = if let Some(raw) = install_base {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            state
                .context
                .config
                .settings()
                .comfyui_install_base
                .ok_or_else(|| "ComfyUI install base folder is not set.".to_string())?
        } else {
            normalize_path(trimmed)?
        }
    } else {
        state
            .context
            .config
            .settings()
            .comfyui_install_base
            .ok_or_else(|| "ComfyUI install base folder is not set.".to_string())?
    };

    if !base.exists() {
        return Ok(ComfyResumeStateResponse {
            found: false,
            install_dir: None,
            step: None,
            summary: "No interrupted install found.".to_string(),
        });
    }

    if let Some((dir, install_state)) = find_in_progress_install(&base) {
        return Ok(ComfyResumeStateResponse {
            found: true,
            install_dir: Some(dir.to_string_lossy().to_string()),
            step: Some(install_state.step.clone()),
            summary: format!(
                "Interrupted install found in {} at step '{}'.",
                dir.display(),
                install_state.step
            ),
        });
    }

    Ok(ComfyResumeStateResponse {
        found: false,
        install_dir: None,
        step: None,
        summary: "No interrupted install found.".to_string(),
    })
}

fn powershell_download(url: &str, out_file: &Path) -> Result<(), String> {
    let parent = out_file
        .parent()
        .ok_or_else(|| "Invalid output path.".to_string())?;
    std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    let command = format!(
        "try {{ Invoke-WebRequest '{}' -OutFile '{}' -UseBasicParsing -ErrorAction Stop }} catch {{ curl.exe -L '{}' -o '{}' }}",
        url,
        out_file.display(),
        url,
        out_file.display()
    );
    let mut cmd = std::process::Command::new("powershell");
    cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &command]);
    apply_background_command_flags(&mut cmd);
    let status = cmd
        .status()
        .map_err(|err| format!("Failed to launch downloader: {err}"))?;
    if !status.success() {
        return Err(format!("Download failed: {url}"));
    }
    Ok(())
}

fn download_http_file(url: &str, out_file: &Path) -> Result<(), String> {
    if let Some(parent) = out_file.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            format!("Failed to create download directory {}: {err}", parent.display())
        })?;
    }

    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|err| format!("Failed to build HTTP client: {err}"))?;

    let mut response = client
        .get(url)
        .header("User-Agent", "ArcticComfyUIHelper/0.3.4")
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|err| format!("HTTP download failed for {url}: {err}"))?;

    let tmp_file = out_file.with_extension("download");
    let mut file = std::fs::File::create(&tmp_file)
        .map_err(|err| format!("Failed to create file {}: {err}", tmp_file.display()))?;

    let mut buffer = [0u8; 1024 * 1024];
    loop {
        let read = response
            .read(&mut buffer)
            .map_err(|err| format!("Failed while reading {url}: {err}"))?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .map_err(|err| format!("Failed writing {}: {err}", tmp_file.display()))?;
    }
    file.flush()
        .map_err(|err| format!("Failed to flush {}: {err}", tmp_file.display()))?;

    std::fs::rename(&tmp_file, out_file).map_err(|err| {
        format!(
            "Failed to finalize download {} -> {}: {err}",
            tmp_file.display(),
            out_file.display()
        )
    })?;
    Ok(())
}

fn download_nunchaku_versions_json(app: &AppHandle, out_file: &Path) -> Result<(), String> {
    let url = "https://nunchaku.tech/cdn/nunchaku_versions.json";
    if let Ok(()) = powershell_download(url, out_file) {
        return Ok(());
    }

    // Fallback for systems with strict revocation/cert path issues.
    let mut curl_cmd = std::process::Command::new("curl.exe");
    curl_cmd
        .args(["-L", "--ssl-no-revoke", url, "-o"])
        .arg(out_file);
    apply_background_command_flags(&mut curl_cmd);
    let curl_status = curl_cmd.status();
    match curl_status {
        Ok(status) if status.success() => Ok(()),
        _ => {
            emit_comfyui_runtime_event(
                app,
                "warn",
                "Could not download nunchaku_versions.json; continuing without it.",
            );
            Err("nunchaku_versions.json download failed".to_string())
        }
    }
}

fn compute_sha256(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|err| format!("Failed to open {}: {err}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|err| format!("Failed to read {}: {err}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect::<String>())
}

fn parse_sha256_manifest(path: &Path) -> Result<String, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|err| format!("Failed to read checksum file {}: {err}", path.display()))?;
    let token = content
        .split_whitespace()
        .find(|part| part.len() == 64 && part.chars().all(|c| c.is_ascii_hexdigit()))
        .ok_or_else(|| format!("Could not parse SHA256 from {}", path.display()))?;
    Ok(token.to_ascii_lowercase())
}

fn run_command(program: &str, args: &[&str], working_dir: Option<&Path>) -> Result<(), String> {
    log::debug!("run_command: {} {}", program, args.join(" "));
    let mut cmd = std::process::Command::new(program);
    cmd.args(args);
    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
    }
    apply_background_command_flags(&mut cmd);
    let status = cmd
        .status()
        .map_err(|err| format!("Failed to run {program}: {err}"))?;
    if !status.success() {
        return Err(format!(
            "Command failed: {} {}",
            program,
            args.join(" ")
        ));
    }
    Ok(())
}

fn run_command_capture(
    program: &str,
    args: &[&str],
    working_dir: Option<&Path>,
) -> Result<(String, String), String> {
    log::debug!("run_command_capture: {} {}", program, args.join(" "));
    let mut cmd = std::process::Command::new(program);
    cmd.args(args);
    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
    }
    apply_background_command_flags(&mut cmd);
    let output = cmd
        .output()
        .map_err(|err| format!("Failed to run {program}: {err}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if !output.status.success() {
        return Err(enrich_command_failure_message(
            program, args, &stdout, &stderr,
        ));
    }
    Ok((stdout, stderr))
}

fn run_command_with_retry(
    program: &str,
    args: &[&str],
    working_dir: Option<&Path>,
    retries: usize,
) -> Result<(), String> {
    let attempts = retries.max(1);
    let mut last_err = String::new();
    for attempt in 1..=attempts {
        match run_command_capture(program, args, working_dir) {
            Ok(_) => return Ok(()),
            Err(err) => {
                last_err = err;
                if attempt < attempts {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                }
            }
        }
    }
    Err(last_err)
}

fn enrich_command_failure_message(program: &str, args: &[&str], stdout: &str, stderr: &str) -> String {
    let tail = if stderr.trim().is_empty() {
        stdout
            .lines()
            .rev()
            .take(12)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        stderr
            .lines()
            .rev()
            .take(12)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n")
    };
    let combined = format!("{stdout}\n{stderr}").to_ascii_lowercase();
    let msvc_hint = if combined.contains("microsoft visual c++ 14.0 or greater is required")
        || combined.contains("msvc")
        || combined.contains("visual studio build tools")
        || combined.contains("unable to find vcvarsall.bat")
    {
        " Missing dependency: Microsoft Visual C++ Build Tools 14.0 or newer. Install Visual Studio Build Tools with the C++ build tools workload, then retry."
    } else {
        ""
    };
    if tail.trim().is_empty() {
        format!("Command failed: {} {}.{}", program, args.join(" "), msvc_hint)
    } else {
        format!(
            "Command failed: {} {} :: {}{}",
            program,
            args.join(" "),
            tail,
            msvc_hint
        )
    }
}

fn looks_like_missing_msvc_tools(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("microsoft visual c++ 14.0 or greater is required")
        || lower.contains("visual studio build tools")
        || lower.contains("unable to find vcvarsall.bat")
        || lower.contains("error: microsoft visual c++")
}

fn powershell_single_quote(raw: &str) -> String {
    raw.replace('\'', "''")
}

fn vswhere_path() -> Option<PathBuf> {
    let candidate =
        PathBuf::from(r"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe");
    if candidate.exists() {
        Some(candidate)
    } else {
        None
    }
}

fn visual_cpp_build_tools_installed() -> bool {
    let Some(vswhere) = vswhere_path() else {
        return false;
    };
    let output = run_command_capture(
        &vswhere.to_string_lossy(),
        &[
            "-latest",
            "-products",
            "*",
            "-requires",
            "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
            "-property",
            "installationPath",
        ],
        None,
    );
    match output {
        Ok((stdout, _)) => stdout.lines().any(|line| !line.trim().is_empty()),
        Err(_) => false,
    }
}

fn install_visual_cpp_build_tools(app: &AppHandle) -> Result<(), String> {
    if visual_cpp_build_tools_installed() {
        emit_install_event(app, "info", "Microsoft Visual C++ Build Tools already installed.");
        return Ok(());
    }

    let state = app.state::<AppState>();
    let deps_root = state.context.config.cache_path().join("deps").join("vs-build-tools");
    std::fs::create_dir_all(&deps_root).map_err(|err| err.to_string())?;
    let bootstrapper = deps_root.join("vs_BuildTools.exe");
    let installer_log = deps_root.join("vs_buildtools_install.log");

    emit_install_event(
        app,
        "step",
        "Downloading Microsoft Visual C++ Build Tools bootstrapper...",
    );
    powershell_download("https://aka.ms/vs/17/release/vs_BuildTools.exe", &bootstrapper)?;

    emit_install_event(
        app,
        "step",
        "Installing Microsoft Visual C++ Build Tools automatically...",
    );

    let bootstrapper_s = powershell_single_quote(&bootstrapper.to_string_lossy());
    let log_s = powershell_single_quote(&installer_log.to_string_lossy());
    let command = format!(
        "$args = @('--quiet','--wait','--norestart','--nocache','--installWhileDownloading','--add','Microsoft.VisualStudio.Workload.VCTools','--includeRecommended','--log','{log}'); \
         $p = Start-Process -FilePath '{exe}' -ArgumentList $args -Verb RunAs -Wait -PassThru; \
         exit $p.ExitCode",
        exe = bootstrapper_s,
        log = log_s,
    );

    let mut cmd = std::process::Command::new("powershell");
    cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &command]);
    apply_background_command_flags(&mut cmd);
    let output = cmd
        .output()
        .map_err(|err| format!("Failed to launch Visual Studio Build Tools installer: {err}"))?;
    let code = output.status.code().unwrap_or(-1);
    if !matches!(code, 0 | 3010 | 1641) {
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(format!(
            "Automatic Visual C++ Build Tools install failed (exit code {code}). {}",
            enrich_command_failure_message("vs_BuildTools.exe", &["--quiet", "--wait"], &stdout, &stderr)
        ));
    }

    if !visual_cpp_build_tools_installed() {
        return Err(format!(
            "Visual C++ Build Tools installer completed, but the required VC toolchain was not detected. Check {}",
            installer_log.display()
        ));
    }

    emit_install_event(
        app,
        "info",
        "Microsoft Visual C++ Build Tools installed successfully.",
    );
    if code == 3010 || code == 1641 {
        emit_install_event(
            app,
            "warn",
            "Build Tools installation reported that a reboot may be required.",
        );
    }
    Ok(())
}

fn run_command_env(
    program: &str,
    args: &[&str],
    working_dir: Option<&Path>,
    envs: &[(&str, &str)],
) -> Result<(), String> {
    log::debug!("run_command_env: {} {}", program, args.join(" "));
    let mut cmd = std::process::Command::new(program);
    cmd.args(args);
    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
    }
    for (key, value) in envs {
        cmd.env(key, value);
    }
    apply_background_command_flags(&mut cmd);
    let output = cmd
        .output()
        .map_err(|err| format!("Failed to run {program}: {err}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if !output.status.success() {
        return Err(enrich_command_failure_message(
            program, args, &stdout, &stderr,
        ));
    }
    Ok(())
}

fn ensure_uv_python_installed(
    uv_bin: &str,
    working_dir: Option<&Path>,
    uv_python_install_dir: &str,
) -> Result<String, String> {
    let candidates = [UV_PYTHON_VERSION, UV_PYTHON_FALLBACK];
    let mut failures: Vec<String> = Vec::new();

    for candidate in candidates {
        match run_command_env(
            uv_bin,
            &["python", "install", candidate],
            working_dir,
            &[
                ("UV_PYTHON_INSTALL_DIR", uv_python_install_dir),
                ("UV_PYTHON_INSTALL_BIN", "false"),
            ],
        ) {
            Ok(()) => return Ok(candidate.to_string()),
            Err(err) => failures.push(format!("{candidate}: {err}")),
        }
    }

    Err(format!(
        "Failed to install Python runtime via uv. Tried: {}",
        failures.join(" | ")
    ))
}


fn run_uv_pip_strict(
    uv_bin: &str,
    python_target: &str,
    pip_args: &[&str],
    working_dir: Option<&Path>,
    envs: &[(&str, &str)],
) -> Result<(), String> {
    let mut uv_compatible_args: Vec<String> = Vec::new();
    let mut index = 0usize;
    while index < pip_args.len() {
        let arg = pip_args[index];
        if arg == "--timeout" || arg == "--retries" {
            index += 2;
            continue;
        }
        if arg.starts_with("--timeout=") || arg.starts_with("--retries=") {
            index += 1;
            continue;
        }
        match arg {
            "--force-reinstall" => uv_compatible_args.push("--reinstall".to_string()),
            "--no-cache-dir" => uv_compatible_args.push("--no-cache".to_string()),
            _ => uv_compatible_args.push(arg.to_string()),
        }
        index += 1;
    }

    let mut args_owned: Vec<String> = vec!["pip".to_string()];
    if let Some((first, rest)) = uv_compatible_args.split_first() {
        args_owned.push(first.clone());
        args_owned.push("--python".to_string());
        args_owned.push(python_target.to_string());
        for arg in rest {
            args_owned.push(arg.clone());
        }
    } else {
        args_owned.push("--python".to_string());
        args_owned.push(python_target.to_string());
    }

    let args: Vec<&str> = args_owned.iter().map(String::as_str).collect();
    let mut merged_envs: Vec<(&str, &str)> = Vec::with_capacity(envs.len() + 1);
    merged_envs.push(("UV_LINK_MODE", "copy"));
    merged_envs.extend_from_slice(envs);
    run_command_env(uv_bin, &args, working_dir, &merged_envs)
}
fn uv_pip_uninstall_best_effort(
    uv_bin: &str,
    py_exe: &Path,
    install_root: &Path,
    uv_python_install_dir: &str,
    packages: &[&str],
) -> Result<(), String> {
    let mut failed: Vec<String> = Vec::new();
    for package in packages {
        if !pip_has_package(install_root, package) {
            continue;
        }

        let mut removed = false;
        let mut last_err: Option<String> = None;
        for attempt in 0..2 {
            let _ = kill_python_processes_for_root(install_root, py_exe);
            match run_uv_pip_strict(
                uv_bin,
                &py_exe.to_string_lossy(),
                &["uninstall", package],
                Some(install_root),
                &[("UV_PYTHON_INSTALL_DIR", uv_python_install_dir)],
            ) {
                Ok(()) => {
                    removed = true;
                    break;
                }
                Err(err) => {
                    if !pip_has_package(install_root, package) {
                        removed = true;
                        break;
                    }
                    last_err = Some(err);
                    if attempt == 0 {
                        std::thread::sleep(Duration::from_millis(250));
                    }
                }
            }
        }

        if !removed {
            failed.push(format!(
                "{package}: {}",
                last_err.unwrap_or_else(|| "uninstall failed".to_string())
            ));
        }
    }

    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("Failed to uninstall packages: {}", failed.join(" | ")))
    }
}

fn profile_from_torch_env(root: &Path) -> Result<String, String> {
    let mut cmd = python_for_root(root);
    cmd.arg("-c").arg(
        "import torch; \
         v = getattr(torch, '__version__', ''); \
         c = getattr(torch.version, 'cuda', '') or ''; \
         h = getattr(torch.version, 'hip', '') or ''; \
         x = 'xpu' if hasattr(torch, 'xpu') and torch.xpu.is_available() else ''; \
         print(v); print(c); print(h); print(x)",
    );
    cmd.current_dir(root);
    let out = cmd
        .output()
        .map_err(|err| format!("Failed to detect installed torch profile: {err}"))?;
    if !out.status.success() {
        return Err("Failed to detect installed torch profile.".to_string());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let torch_v = lines.next().unwrap_or_default().to_ascii_lowercase();
    let cuda_v = lines.next().unwrap_or_default().to_ascii_lowercase();
    let hip_v = lines.next().unwrap_or_default().to_ascii_lowercase();
    let xpu_v = lines.next().unwrap_or_default().to_ascii_lowercase();

    if let Some(profile) = torch_profile_from_versions(&torch_v, &cuda_v, &hip_v, &xpu_v) {
        return Ok(profile);
    }

    Err(format!(
        "Unsupported installed torch combo: torch={torch_v}, cuda={cuda_v}, hip={hip_v}, xpu={xpu_v}"
    ))
}

fn detect_torch_profile_for_root(root: &Path) -> Option<String> {
    profile_from_torch_env(root).ok()
}

fn torch_profile_from_versions(
    torch_v: &str,
    cuda_v: &str,
    hip_v: &str,
    xpu_v: &str,
) -> Option<String> {
    let t = torch_v.trim().to_ascii_lowercase();
    let c = cuda_v.trim().to_ascii_lowercase();
    let h = hip_v.trim().to_ascii_lowercase();
    let x = xpu_v.trim().to_ascii_lowercase();
    if t.starts_with("2.7") && c.starts_with("12.8") {
        return Some("torch271_cu128".to_string());
    }
    if t.starts_with("2.8") && c.starts_with("12.8") {
        return Some("torch280_cu128".to_string());
    }
    if t.starts_with("2.9")
        && (h.starts_with("7.2") || t.contains("rocmsdk") || t.contains("+rocm"))
    {
        return Some("torch291_rocm72".to_string());
    }
    if t.starts_with("2.9") && c.starts_with("13.0") {
        return Some("torch291_cu130".to_string());
    }
    if x == "xpu" || t.contains("xpu") {
        return Some("torchxpu_nightly".to_string());
    }
    None
}

fn attention_wheel_url(profile: &str, backend: &str) -> Option<&'static str> {
    if is_non_cuda_profile(profile) && matches!(backend, "sage" | "sage3" | "flash" | "nunchaku") {
        return None;
    }
    match backend {
        "sage" => Some(match profile {
            "torch271_cu128" => "https://huggingface.co/arcticlatent/windows/resolve/main/SageAttention/sageattention-2.2.0%2Bcu128torch2.7.1.post3-cp39-abi3-win_amd64.whl",
            "torch291_cu130" => "https://huggingface.co/arcticlatent/windows/resolve/main/SageAttention/sageattention-2.2.0%2Bcu130torch2.9.0andhigher.post4-cp39-abi3-win_amd64.whl",
            _ => "https://huggingface.co/arcticlatent/windows/resolve/main/SageAttention/sageattention-2.2.0%2Bcu128torch2.8.0.post3-cp39-abi3-win_amd64.whl",
        }),
        "sage3" => Some(match profile {
            "torch271_cu128" => "https://huggingface.co/arcticlatent/windows/resolve/main/SageAttention3/sageattn3-1.0.0%2Bcu128torch271-cp312-cp312-win_amd64.whl",
            "torch291_cu130" => "https://huggingface.co/arcticlatent/windows/resolve/main/SageAttention3/sageattn3-1.0.0%2Bcu130torch291-cp312-cp312-win_amd64.whl",
            _ => "https://huggingface.co/arcticlatent/windows/resolve/main/SageAttention3/sageattn3-1.0.0%2Bcu128torch280-cp312-cp312-win_amd64.whl",
        }),
        "flash" => Some(match profile {
            "torch271_cu128" => "https://huggingface.co/arcticlatent/windows/resolve/main/FlashAttention/flash_attn-2.8.3%2Bcu128torch2.7.0cxx11abiFALSE-cp312-cp312-win_amd64.whl",
            "torch291_cu130" => "https://huggingface.co/arcticlatent/windows/resolve/main/FlashAttention/flash_attn-2.8.3%2Bcu130torch2.9.1cxx11abiTRUE-cp312-cp312-win_amd64.whl",
            _ => "https://huggingface.co/arcticlatent/windows/resolve/main/FlashAttention/flash_attn-2.8.3%2Bcu128torch2.8.0cxx11abiFALSE-cp312-cp312-win_amd64.whl",
        }),
        "nunchaku" => Some(match profile {
            "torch271_cu128" => "https://github.com/nunchaku-ai/nunchaku/releases/download/v1.0.2/nunchaku-1.0.2+torch2.7-cp312-cp312-win_amd64.whl",
            "torch291_cu130" => "https://github.com/nunchaku-ai/nunchaku/releases/download/v1.2.1/nunchaku-1.2.1+cu13.0torch2.9-cp312-cp312-win_amd64.whl",
            _ => "https://github.com/nunchaku-ai/nunchaku/releases/download/v1.2.1/nunchaku-1.2.1+cu12.8torch2.8-cp312-cp312-win_amd64.whl",
        }),
        _ => None,
    }
}

fn install_wheel_no_deps(
    uv_bin: &str,
    py_path: &str,
    root: &Path,
    uv_python_install_dir: &str,
    whl: &str,
    force_reinstall: bool,
) -> Result<(), String> {
    let mut args = vec!["install", "--upgrade"];
    if force_reinstall {
        args.push("--force-reinstall");
    }
    args.push(whl);
    args.extend_from_slice(&["--no-deps", "--no-cache-dir", "--timeout=1000", "--retries", "10"]);
    run_uv_pip_strict(
        uv_bin,
        py_path,
        &args,
        Some(root),
        &[("UV_PYTHON_INSTALL_DIR", uv_python_install_dir)],
    )
}

fn ensure_venv_pip(
    uv_bin: &str,
    py_exe: &Path,
    install_root: &Path,
    uv_python_install_dir: &str,
) -> Result<(), String> {
    run_uv_pip_strict(
        uv_bin,
        &py_exe.to_string_lossy(),
        &["check"],
        Some(install_root),
        &[("UV_PYTHON_INSTALL_DIR", uv_python_install_dir)],
    )
}

fn write_install_summary(install_root: &Path, items: &[InstallSummaryItem]) {
    let path = install_root.join("install-summary.json");
    if let Ok(data) = serde_json::to_vec_pretty(items) {
        let _ = std::fs::write(path, data);
    }
}

fn find_file_recursive(root: &Path, file_name: &str) -> Option<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let read = std::fs::read_dir(&dir).ok()?;
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.eq_ignore_ascii_case(file_name))
                .unwrap_or(false)
            {
                return Some(path);
            }
        }
    }
    None
}

fn resolve_uv_binary(shared_runtime_root: &Path, app: &AppHandle) -> Result<String, String> {
    // Prefer system uv if available.
    if std::process::Command::new("uv")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        return Ok("uv".to_string());
    }

    // Fallback: local uv binary under install folder.
    let local_root = shared_runtime_root.join(".tools").join("uv");
    let local_uv = local_root.join("uv.exe");
    if local_uv.exists() {
        return Ok(local_uv.to_string_lossy().to_string());
    }
    if let Some(found) = find_file_recursive(&local_root, "uv.exe") {
        return Ok(found.to_string_lossy().to_string());
    }
    if let Some(legacy_runtime_root) = shared_runtime_root.parent().map(|parent| parent.join("comfy_runtime")) {
        let legacy_local_root = legacy_runtime_root.join(".tools").join("uv");
        let legacy_local_uv = legacy_local_root.join("uv.exe");
        if legacy_local_uv.exists() {
            return Ok(legacy_local_uv.to_string_lossy().to_string());
        }
        if let Some(found) = find_file_recursive(&legacy_local_root, "uv.exe") {
            return Ok(found.to_string_lossy().to_string());
        }
    }

    emit_install_event(app, "step", "Downloading local uv runtime...");
    std::fs::create_dir_all(&local_root).map_err(|err| err.to_string())?;
    let zip_path = local_root.join("uv-x86_64-pc-windows-msvc.zip");
    let sha_path = local_root.join("uv-x86_64-pc-windows-msvc.zip.sha256");
    powershell_download(
        "https://github.com/astral-sh/uv/releases/download/0.9.7/uv-x86_64-pc-windows-msvc.zip",
        &zip_path,
    )?;
    powershell_download(
        "https://github.com/astral-sh/uv/releases/download/0.9.7/uv-x86_64-pc-windows-msvc.zip.sha256",
        &sha_path,
    )?;
    emit_install_event(app, "step", "Verifying uv runtime checksum...");
    let expected = parse_sha256_manifest(&sha_path)?;
    let actual = compute_sha256(&zip_path)?;
    if actual != expected {
        return Err(format!(
            "uv runtime checksum mismatch (expected {expected}, got {actual})."
        ));
    }
    run_command("tar", &["-xf", &zip_path.to_string_lossy()], Some(&local_root))?;
    let _ = std::fs::remove_file(zip_path);
    let _ = std::fs::remove_file(sha_path);

    let found = find_file_recursive(&local_root, "uv.exe")
        .ok_or_else(|| "Failed to locate uv.exe after extraction.".to_string())?;
    Ok(found.to_string_lossy().to_string())
}

fn emit_install_event(app: &AppHandle, phase: &str, message: &str) {
    let _ = app.emit(
        "comfyui-install-progress",
        DownloadProgressEvent {
            kind: "comfyui_install".to_string(),
            phase: phase.to_string(),
            artifact: None,
            index: None,
            total: None,
            received: None,
            size: None,
            folder: None,
            message: Some(message.to_string()),
        },
    );
}

fn torch_profile_to_packages(
    profile: &str,
) -> (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
) {
    match profile {
        "torch271_cu128" => (
            "2.7.1+cu128",
            "0.22.1+cu128",
            "2.7.1+cu128",
            "https://download.pytorch.org/whl/cu128",
            "triton-windows==3.3.1.post19",
        ),
        "torch291_cu130" => (
            "2.9.1+cu130",
            "0.24.1+cu130",
            "2.9.1+cu130",
            "https://download.pytorch.org/whl/cu130",
            "triton-windows<3.6",
        ),
        "torch291_rocm72" => (
            "2.9.1",
            "0.24.1",
            "2.9.1",
            "https://repo.radeon.com/rocm/manylinux/rocm-rel-7.2/",
            "",
        ),
        "torchxpu_nightly" => (
            "nightly",
            "nightly",
            "nightly",
            "https://download.pytorch.org/whl/nightly/xpu",
            "",
        ),
        _ => (
            "2.8.0+cu128",
            "0.23.0+cu128",
            "2.8.0+cu128",
            "https://download.pytorch.org/whl/cu128",
            "triton-windows==3.4.0.post20",
        ),
    }
}

fn reassert_torch_stack_for_profile(
    uv_bin: &str,
    py_path: &str,
    root: &Path,
    uv_python_install_dir: &str,
    profile: &str,
) -> Result<(), String> {
    if is_rocm_profile(profile) {
        install_windows_rocm_torch_stack(uv_bin, py_path, root, uv_python_install_dir)?;
    } else if is_xpu_profile(profile) {
        install_windows_xpu_torch_stack(uv_bin, py_path, root, uv_python_install_dir)?;
    } else {
    let (torch_v, tv_v, ta_v, index_url, triton_pkg) = torch_profile_to_packages(profile);
    run_uv_pip_strict(
        uv_bin,
        py_path,
        &[
            "install",
            "--upgrade",
            "--force-reinstall",
            &format!("torch=={torch_v}"),
            &format!("torchvision=={tv_v}"),
            &format!("torchaudio=={ta_v}"),
            "--index-url",
            index_url,
            "--no-cache-dir",
            "--timeout=1000",
            "--retries",
            "10",
        ],
        Some(root),
        &[("UV_PYTHON_INSTALL_DIR", uv_python_install_dir)],
    )?;
    run_uv_pip_strict(
        uv_bin,
        py_path,
        &[
            "install",
            "--upgrade",
            "--force-reinstall",
            triton_pkg,
            "--no-cache-dir",
            "--timeout=1000",
            "--retries",
            "10",
        ],
        Some(root),
        &[("UV_PYTHON_INSTALL_DIR", uv_python_install_dir)],
    )?;
    }
    let mut verify_cmd = std::process::Command::new(py_path);
    verify_cmd.arg("-c").arg(
        "import torch, importlib.metadata as m; \
         print(getattr(torch, '__version__', '')); \
         print(getattr(torch.version, 'cuda', '') or ''); \
         print(getattr(torch.version, 'hip', '') or ''); \
         print('xpu' if hasattr(torch, 'xpu') and torch.xpu.is_available() else ''); \
         print(m.version('torchvision')); \
         print(m.version('torchaudio'))",
    );
    verify_cmd.current_dir(root);
    apply_background_command_flags(&mut verify_cmd);
    apply_torch_allocator_env_compat(&mut verify_cmd);
    let verify = verify_cmd
        .output()
        .map_err(|err| format!("Failed to verify torch profile with {py_path}: {err}"))?;
    if !verify.status.success() {
        return Err("Torch profile verification command failed after reinstall.".to_string());
    }
    let text = String::from_utf8_lossy(&verify.stdout);
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let installed_torch = lines.next().unwrap_or_default();
    let installed_cuda = lines.next().unwrap_or_default();
    let installed_hip = lines.next().unwrap_or_default();
    let installed_xpu = lines.next().unwrap_or_default();
    let installed_tv = lines.next().unwrap_or_default();
    let installed_ta = lines.next().unwrap_or_default();
    let actual_profile = torch_profile_from_versions(
        installed_torch,
        installed_cuda,
        installed_hip,
        installed_xpu,
    );
    if actual_profile.as_deref() != Some(profile) {
        return Err(format!(
            "Torch profile enforce mismatch for {profile}: got torch={installed_torch}, cuda={installed_cuda}, hip={installed_hip}, xpu={installed_xpu}, torchvision={installed_tv}, torchaudio={installed_ta}"
        ));
    }
    Ok(())
}

fn install_custom_node(
    app: &AppHandle,
    install_root: &Path,
    custom_nodes_root: &Path,
    py_exe: &Path,
    repo_url: &str,
    folder_name: &str,
) -> Result<(), String> {
    emit_install_event(app, "step", &format!("Installing custom node: {folder_name}..."));
    let node_dir = custom_nodes_root.join(folder_name);
    if node_dir.exists() {
        let _ = std::fs::remove_dir_all(&node_dir);
    }
    run_command_with_retry(
        "git",
        &["clone", repo_url, &node_dir.to_string_lossy()],
        Some(install_root),
        2,
    )?;

    let req = node_dir.join("requirements.txt");
    if req.exists() {
        let non_empty = std::fs::metadata(&req)
            .map(|m| m.len() > 0)
            .unwrap_or(false);
        if non_empty {
            let shared_runtime_root = app.state::<AppState>().context.config.cache_path().join("comfyui-runtime");
            let uv_bin = resolve_uv_binary(&shared_runtime_root, app)?;
            let uv_python_install_dir = shared_runtime_root.join(".python").to_string_lossy().to_string();
            run_uv_pip_strict(
                &uv_bin,
                &py_exe.to_string_lossy(),
                &[
                    "install",
                    "-r",
                    &req.to_string_lossy(),
                    "--no-cache-dir",
                    "--timeout=1000",
                    "--retries",
                    "10",
                ],
                Some(install_root),
                &[("UV_PYTHON_INSTALL_DIR", &uv_python_install_dir)],
            )?;
        }
    }

    let installer = node_dir.join("install.py");
    if installer.exists() {
        let non_empty = std::fs::metadata(&installer)
            .map(|m| m.len() > 0)
            .unwrap_or(false);
        if non_empty {
            run_command(
                &py_exe.to_string_lossy(),
                &[&installer.to_string_lossy()],
                Some(install_root),
            )?;
        }
    }

    let shared_runtime_root = app
        .state::<AppState>()
        .context
        .config
        .cache_path()
        .join("comfyui-runtime");
    let uv_bin = resolve_uv_binary(&shared_runtime_root, app)?;
    let uv_python_install_dir = shared_runtime_root.join(".python").to_string_lossy().to_string();
    reassert_requests_dependency_stack(
        &uv_bin,
        &py_exe.to_string_lossy(),
        install_root,
        &uv_python_install_dir,
    )?;

    Ok(())
}

fn reassert_requests_dependency_stack(
    uv_bin: &str,
    py_path: &str,
    root: &Path,
    uv_python_install_dir: &str,
) -> Result<(), String> {
    let py_exe = PathBuf::from(py_path);
    uv_pip_uninstall_best_effort(
        uv_bin,
        &py_exe,
        root,
        uv_python_install_dir,
        &["chardet"],
    )?;
    run_uv_pip_strict(
        uv_bin,
        py_path,
        &[
            "install",
            "--upgrade",
            "--force-reinstall",
            "requests==2.32.5",
            "urllib3==2.5.0",
            "charset_normalizer==3.4.5",
            "idna==3.10",
            "--no-cache-dir",
            "--timeout=1000",
            "--retries",
            "10",
        ],
        Some(root),
        &[("UV_PYTHON_INSTALL_DIR", uv_python_install_dir)],
    )
}

fn selected_attention_backend(request: &ComfyInstallRequest) -> Option<&'static str> {
    if request.include_flash_attention {
        Some("flash")
    } else if request.include_sage_attention || request.include_sage_attention3 {
        Some("sage")
    } else {
        None
    }
}

fn append_attention_launch_arg(args: &mut Vec<String>, backend: Option<&str>) {
    match backend
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("flash") => args.push("--use-flash-attention".to_string()),
        Some("sage") | Some("sage3") => args.push("--use-sage-attention".to_string()),
        _ => {}
    }
}

fn is_rocm_profile(profile: &str) -> bool {
    matches!(profile, "torch291_rocm72")
}

fn is_xpu_profile(profile: &str) -> bool {
    matches!(profile, "torchxpu_nightly")
}

fn is_non_cuda_profile(profile: &str) -> bool {
    is_rocm_profile(profile) || is_xpu_profile(profile)
}

fn windows_rocm_sdk_pytorch_packages() -> [&'static str; 4] {
    [
        "https://repo.radeon.com/rocm/windows/rocm-rel-7.2/torch-2.9.1%2Brocmsdk20260116-cp312-cp312-win_amd64.whl",
        "https://repo.radeon.com/rocm/windows/rocm-rel-7.2/torchvision-0.24.1%2Brocmsdk20260116-cp312-cp312-win_amd64.whl",
        "https://repo.radeon.com/rocm/windows/rocm-rel-7.2/torchaudio-2.9.1%2Brocmsdk20260116-cp312-cp312-win_amd64.whl",
        "",
    ]
}

fn windows_rocm_sdk_bootstrap_packages() -> [&'static str; 4] {
    [
        "https://repo.radeon.com/rocm/windows/rocm-rel-7.2/rocm_sdk_core-7.2.0.dev0-py3-none-win_amd64.whl",
        "https://repo.radeon.com/rocm/windows/rocm-rel-7.2/rocm_sdk_devel-7.2.0.dev0-py3-none-win_amd64.whl",
        "https://repo.radeon.com/rocm/windows/rocm-rel-7.2/rocm_sdk_libraries_custom-7.2.0.dev0-py3-none-win_amd64.whl",
        "https://repo.radeon.com/rocm/windows/rocm-rel-7.2/rocm-7.2.0.dev0.tar.gz",
    ]
}

fn install_windows_rocm_torch_stack(
    uv_bin: &str,
    py_path: &str,
    root: &Path,
    uv_python_install_dir: &str,
) -> Result<(), String> {
    let _ = uv_bin;
    let _ = uv_python_install_dir;
    let sdk = windows_rocm_sdk_bootstrap_packages();
    run_command_env(
        py_path,
        &[
            "-m",
            "pip",
            "install",
            "--no-cache-dir",
            sdk[0],
            sdk[1],
            sdk[2],
            sdk[3],
        ],
        Some(root),
        &[],
    )?;
    let pkgs = windows_rocm_sdk_pytorch_packages();
    run_command_env(
        py_path,
        &[
            "-m",
            "pip",
            "install",
            "--no-cache-dir",
            pkgs[0],
            pkgs[1],
            pkgs[2],
        ],
        Some(root),
        &[],
    )?;
    run_command_env(
        py_path,
        &["-m", "pip", "install", "--no-cache-dir", "numpy==1.26.4"],
        Some(root),
        &[],
    )
}

fn install_windows_xpu_torch_stack(
    uv_bin: &str,
    py_path: &str,
    root: &Path,
    uv_python_install_dir: &str,
) -> Result<(), String> {
    let py_exe = PathBuf::from(py_path);
    uv_pip_uninstall_best_effort(
        uv_bin,
        &py_exe,
        root,
        uv_python_install_dir,
        &[
            "torch",
            "torchvision",
            "torchaudio",
            "intel-extension-for-pytorch",
            "pytorch-triton-xpu",
        ],
    )?;
    run_uv_pip_strict(
        uv_bin,
        py_path,
        &[
            "install",
            "--pre",
            "--upgrade",
            "--force-reinstall",
            "--no-cache-dir",
            "torch",
            "torchvision",
            "torchaudio",
            "--index-url",
            "https://download.pytorch.org/whl/nightly/xpu",
        ],
        Some(root),
        &[("UV_PYTHON_INSTALL_DIR", uv_python_install_dir)],
    )
}

fn apply_torch_allocator_env_compat(cmd: &mut std::process::Command) {
    if let Ok(value) = std::env::var("PYTORCH_CUDA_ALLOC_CONF") {
        if std::env::var_os("PYTORCH_ALLOC_CONF").is_none() {
            cmd.env("PYTORCH_ALLOC_CONF", value);
        }
        cmd.env_remove("PYTORCH_CUDA_ALLOC_CONF");
    }
}

fn apply_intel_xpu_launch_env(cmd: &mut std::process::Command, profile: &str) {
    if is_xpu_profile(profile) {
        cmd.env("UR_L0_ENABLE_RELAXED_ALLOCATION_LIMITS", "1");
        cmd.env("SYCL_CACHE_PERSISTENT", "1");
    }
}

fn detect_attention_backend_for_root(root: &Path) -> Option<String> {
    let has_flash = pip_has_package(root, "flash-attn")
        || pip_has_package(root, "flash_attn")
        || python_module_importable(root, "flash_attn");
    if has_flash {
        return Some("flash".to_string());
    }
    let has_sage3 = pip_has_package(root, "sageattn3") || python_module_importable(root, "sageattn3");
    if has_sage3 {
        return Some("sage3".to_string());
    }
    let has_sage = pip_has_package(root, "sageattention") || python_module_importable(root, "sageattention");
    if has_sage {
        return Some("sage".to_string());
    }
    if nunchaku_backend_present(root) {
        return Some("nunchaku".to_string());
    }
    None
}

fn detect_launch_attention_backend_for_root(root: &Path) -> Option<String> {
    detect_attention_backend_for_root(root)
}

fn comfyui_launch_args(
    listen_enabled: bool,
    pinned_memory_enabled: bool,
    lowvram_enabled: bool,
    bf16_unet_enabled: bool,
    async_offload_enabled: bool,
    disable_smart_memory_enabled: bool,
    attention_backend: Option<&str>,
    custom_launch_args: &[String],
) -> Vec<String> {
    let mut args = vec!["--windows-standalone-build".to_string()];
    if listen_enabled {
        args.push("--listen".to_string());
    }
    if !pinned_memory_enabled {
        args.push("--disable-pinned-memory".to_string());
    }
    if lowvram_enabled {
        args.push("--lowvram".to_string());
    }
    if bf16_unet_enabled {
        args.push("--bf16-unet".to_string());
    }
    if async_offload_enabled {
        args.push("--async-offload".to_string());
    }
    if disable_smart_memory_enabled {
        args.push("--disable-smart-memory".to_string());
    }
    append_attention_launch_arg(&mut args, attention_backend);
    args.extend(custom_launch_args.iter().cloned());
    args
}

fn parse_custom_launch_args(input: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars().peekable();
    let mut quote: Option<char> = None;

    while let Some(ch) = chars.next() {
        match quote {
            Some(active_quote) => match ch {
                '\\' => {
                    if let Some(next) = chars.next() {
                        current.push(next);
                    }
                }
                _ if ch == active_quote => quote = None,
                _ => current.push(ch),
            },
            None => match ch {
                '"' | '\'' => quote = Some(ch),
                '\\' => {
                    if let Some(next) = chars.next() {
                        current.push(next);
                    }
                }
                c if c.is_whitespace() => {
                    if !current.is_empty() {
                        args.push(std::mem::take(&mut current));
                    }
                }
                _ => current.push(ch),
            },
        }
    }

    if quote.is_some() {
        return Err("Custom launch args contain an unclosed quote.".to_string());
    }
    if !current.is_empty() {
        args.push(current);
    }

    Ok(args)
}

fn run_comfyui_install(
    app: &AppHandle,
    request: &ComfyInstallRequest,
    shared_runtime_root: &Path,
    cancel: &CancellationToken,
) -> Result<PathBuf, String> {
    let mut summary: Vec<InstallSummaryItem> = Vec::new();
    let include_insight_face = request.include_insight_face || request.include_nunchaku;
    let selected_attention = [
        request.include_sage_attention,
        request.include_sage_attention3,
        request.include_flash_attention,
        request.include_nunchaku,
    ]
    .into_iter()
    .filter(|v| *v)
    .count();
    if selected_attention > 1 {
        return Err(
            "Choose only one of SageAttention, SageAttention3, FlashAttention, or Nunchaku.".to_string(),
        );
    }
    if request.include_sage_attention3 {
        let gpu = detect_nvidia_gpu_details();
        let is_50_series = gpu
            .name
            .as_deref()
            .map(|name| name.to_ascii_lowercase().contains("rtx 50"))
            .unwrap_or(false);
        if !is_50_series {
            return Err("SageAttention3 is available only for NVIDIA RTX 50-series GPUs.".to_string());
        }
    }

    if cancel.is_cancelled() {
        return Err("Installation cancelled.".to_string());
    }

    let base_root = normalize_path(&request.install_root)?;
    let extra_model_root = normalize_optional_path(request.extra_model_root.as_deref())?;
    if is_forbidden_install_path(&base_root) {
        return Err("Install folder is not allowed. Avoid C:\\, Windows, or Program Files.".to_string());
    }
    let selected_comfy_root = path_name_is_comfyui(&base_root);
    let mut comfy_dir = if selected_comfy_root {
        base_root.clone()
    } else {
        choose_install_folder(&base_root, request.force_fresh)
    };
    let install_root = comfy_dir.clone();

    std::fs::create_dir_all(&install_root).map_err(|err| err.to_string())?;
    write_install_state(&install_root, "in_progress", "init");
    emit_install_event(
        app,
        "info",
        &format!("Install folder selected: {}", install_root.display()),
    );

    let log_path = install_root.join("install.log");
    let mut log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|err| err.to_string())?;
    let _ = writeln!(log_file, "Starting install");

    let recommendation = get_comfyui_install_recommendation();
    let selected_profile = request
        .torch_profile
        .clone()
        .unwrap_or_else(|| recommendation.torch_profile);
    if is_non_cuda_profile(&selected_profile)
        && (request.include_sage_attention
            || request.include_sage_attention3
            || request.include_flash_attention
            || request.include_nunchaku
            || request.include_trellis2)
    {
        return Err(
            "SageAttention, SageAttention3, FlashAttention, Nunchaku, and Trellis2 are CUDA-only and are not available with the Windows ROCm/XPU profiles."
                .to_string(),
        );
    }
    if request.include_trellis2 && !matches!(selected_profile.as_str(), "torch280_cu128") {
        return Err(
            "Trellis2 currently requires Torch 2.8.0 + cu128 (Torch280 wheel set).".to_string(),
        );
    }
    let (torch_v, tv_v, ta_v, index_url, triton_pkg) =
        torch_profile_to_packages(&selected_profile);
    emit_install_event(
        app,
        "info",
        &format!("Using {} ({})", selected_profile, recommendation.reason),
    );

    if cancel.is_cancelled() {
        return Err("Installation cancelled.".to_string());
    }
    ensure_git_available(app)?;
    // Migration fallback: older builds sometimes created ComfyUI/ComfyUI.
    let nested_legacy = comfy_dir.join("ComfyUI").join("main.py");
    if !comfy_dir.join("main.py").exists() && nested_legacy.exists() {
        comfy_dir = comfy_dir.join("ComfyUI");
        emit_install_event(
            app,
            "info",
            &format!("Detected existing nested ComfyUI; using {}", comfy_dir.display()),
        );
    }

    if !comfy_dir.join("main.py").exists() {
        write_install_state(&install_root, "in_progress", "clone_comfyui");
        emit_install_event(app, "step", "Cloning ComfyUI...");
        if comfy_dir.exists() && !is_empty_dir(&comfy_dir) {
            if is_recoverable_preclone_dir(&comfy_dir) {
                emit_install_event(
                    app,
                    "info",
                    "Cleaning previous partial install artifacts before clone...",
                );
                clear_directory_contents(&comfy_dir)?;
            } else {
                return Err(format!(
                    "Selected ComfyUI folder already exists and is not empty: {}. Choose a new base folder or remove existing files.",
                    comfy_dir.display()
                ));
            }
        }
        run_command_with_retry(
            "git",
            &[
                "clone",
                "https://github.com/Comfy-Org/ComfyUI",
                &comfy_dir.to_string_lossy(),
            ],
            Some(&install_root),
            2,
        )?;
        if let Some((latest_tag, latest_version)) = git_latest_release_tag(&comfy_dir) {
            if let Err(err) = run_command_with_retry(
                "git",
                &["checkout", "-B", "master", &latest_tag],
                Some(&comfy_dir),
                1,
            ) {
                emit_install_event(
                    app,
                    "warn",
                    &format!(
                        "ComfyUI cloned, but failed to pin to release tag {} (v{}): {}",
                        latest_tag, latest_version, err
                    ),
                );
            } else {
                emit_install_event(
                    app,
                    "info",
                    &format!(
                        "Pinned fresh ComfyUI install to latest release tag {} (v{}).",
                        latest_tag, latest_version
                    ),
                );
            }
        } else {
            emit_install_event(
                app,
                "warn",
                "ComfyUI cloned, but latest release tag could not be resolved during install.",
            );
        }
        summary.push(InstallSummaryItem {
            name: "ComfyUI core".to_string(),
            status: "ok".to_string(),
            detail: "ComfyUI cloned successfully.".to_string(),
        });
    } else {
        emit_install_event(app, "step", "ComfyUI folder already exists, skipping clone.");
        summary.push(InstallSummaryItem {
            name: "ComfyUI core".to_string(),
            status: "skipped".to_string(),
            detail: "Existing ComfyUI folder reused.".to_string(),
        });
    }

    if let Some(extra_root) = extra_model_root.as_ref() {
        write_install_state(&install_root, "in_progress", "extra_model_paths");
        emit_install_event(
            app,
            "step",
            &format!(
                "Configuring ComfyUI extra model paths from {}...",
                extra_root.display()
            ),
        );
        let config_path =
            write_extra_model_paths_yaml(&comfy_dir, extra_root, request.extra_model_use_default)?;
        summary.push(InstallSummaryItem {
            name: "extra_model_paths".to_string(),
            status: "ok".to_string(),
            detail: format!(
                "Configured {} with base path {}.",
                config_path.display(),
                extra_root.display()
            ),
        });
    }

    if cancel.is_cancelled() {
        return Err("Installation cancelled.".to_string());
    }
    emit_install_event(app, "step", "Preparing uv-managed Python + local .venv...");
    emit_install_event(
        app,
        "info",
        &format!("Shared uv runtime path: {}", shared_runtime_root.display()),
    );
    write_install_state(&install_root, "in_progress", "python_venv");
    let uv_bin = resolve_uv_binary(shared_runtime_root, app)?;
    let python_store = shared_runtime_root.join(".python");
    std::fs::create_dir_all(&python_store).map_err(|err| err.to_string())?;
    let python_store_s = python_store.to_string_lossy().to_string();
    let resolved_python = ensure_uv_python_installed(&uv_bin, Some(&comfy_dir), &python_store_s)?;

    let venv_dir = comfy_dir.join(".venv");
    let py_exe = venv_dir.join("Scripts").join("python.exe");
    if !py_exe.exists() {
        let venv_s = venv_dir.to_string_lossy().to_string();
        run_command_env(
            &uv_bin,
            &["venv", "--seed", "--python", &resolved_python, &venv_s],
            Some(&comfy_dir),
            &[("UV_PYTHON_INSTALL_DIR", &python_store_s)],
        )?;
    } else {
        emit_install_event(app, "step", "Existing .venv found; reusing.");
    }

    emit_install_event(app, "step", "Verifying uv pip in local .venv...");
    ensure_venv_pip(&uv_bin, &py_exe, &comfy_dir, &python_store_s)?;

    run_uv_pip_strict(
        &uv_bin,
        &py_exe.to_string_lossy(),
        &["install", "--upgrade", "pip", "setuptools", "wheel", "--no-cache-dir", "--timeout=1000", "--retries", "10"],
        Some(&comfy_dir),
        &[("UV_PYTHON_INSTALL_DIR", &python_store_s)],
    )?;

    if cancel.is_cancelled() {
        return Err("Installation cancelled.".to_string());
    }
    emit_install_event(app, "step", "Installing Torch stack...");
    write_install_state(&install_root, "in_progress", "torch_stack");
    if is_rocm_profile(&selected_profile) {
        install_windows_rocm_torch_stack(
            &uv_bin,
            &py_exe.to_string_lossy(),
            &comfy_dir,
            &python_store_s,
        )?;
    } else if is_xpu_profile(&selected_profile) {
        install_windows_xpu_torch_stack(
            &uv_bin,
            &py_exe.to_string_lossy(),
            &comfy_dir,
            &python_store_s,
        )?;
    } else {
        run_uv_pip_strict(
            &uv_bin,
            &py_exe.to_string_lossy(),
            &["install", "--upgrade", "--force-reinstall", &format!("torch=={torch_v}"), &format!("torchvision=={tv_v}"), &format!("torchaudio=={ta_v}"), "--index-url", index_url, "--no-cache-dir", "--timeout=1000", "--retries", "10"],
            Some(&comfy_dir),
            &[("UV_PYTHON_INSTALL_DIR", &python_store_s)],
        )?;
        run_uv_pip_strict(
            &uv_bin,
            &py_exe.to_string_lossy(),
            &["install", "--upgrade", "--force-reinstall", triton_pkg, "--no-cache-dir", "--timeout=1000", "--retries", "10"],
            Some(&comfy_dir),
            &[("UV_PYTHON_INSTALL_DIR", &python_store_s)],
        )?;
    }

    if cancel.is_cancelled() {
        return Err("Installation cancelled.".to_string());
    }
    emit_install_event(app, "step", "Installing ComfyUI requirements...");
    write_install_state(&install_root, "in_progress", "comfy_requirements");
    run_uv_pip_strict(
        &uv_bin,
        &py_exe.to_string_lossy(),
        &["install", "-r", &comfy_dir.join("requirements.txt").to_string_lossy(), "--no-cache"],
        Some(&comfy_dir),
        &[("UV_PYTHON_INSTALL_DIR", &python_store_s)],
    )?;
    if is_rocm_profile(&selected_profile) || is_xpu_profile(&selected_profile) {
        run_uv_pip_strict(
            &uv_bin,
            &py_exe.to_string_lossy(),
            &["install", "onnxruntime", "onnx", "stringzilla==3.12.6", "transformers==4.57.6", "--no-cache-dir", "--timeout=1000", "--retries", "10"],
            Some(&comfy_dir),
            &[("UV_PYTHON_INSTALL_DIR", &python_store_s)],
        )?;
    } else {
        run_uv_pip_strict(
            &uv_bin,
            &py_exe.to_string_lossy(),
            &["install", "onnxruntime-gpu", "onnx", "stringzilla==3.12.6", "transformers==4.57.6", "--no-cache-dir", "--timeout=1000", "--retries", "10"],
            Some(&comfy_dir),
            &[("UV_PYTHON_INSTALL_DIR", &python_store_s)],
        )?;
    }

    let addon_root = comfy_dir.join("custom_nodes");
    std::fs::create_dir_all(&addon_root).map_err(|err| err.to_string())?;

    // Keep only the selected high-performance attention backend by uninstalling others first.
    let selected_attention_choice = if request.include_nunchaku {
        Some("nunchaku")
    } else if request.include_sage_attention || request.include_sage_attention3 {
        Some("sage")
    } else if request.include_flash_attention {
        Some("flash")
    } else {
        None
    };
    if selected_attention_choice.is_some() {
        write_install_state(&install_root, "in_progress", "cleanup_attention_backends");
        emit_install_event(app, "step", "Cleaning previous attention backend packages...");
        uv_pip_uninstall_best_effort(
            &uv_bin,
            &py_exe,
            &comfy_dir,
            &python_store_s,
            &["sageattention", "sageattn3", "flash-attn", "flash_attn", "nunchaku"],
        )?;
        if !request.include_nunchaku {
            for folder in ["nunchaku_nodes", "ComfyUI-nunchaku"] {
                let nunchaku_node = addon_root.join(folder);
                if nunchaku_node.exists() {
                    let _ = std::fs::remove_dir_all(nunchaku_node);
                }
            }
        }
    }

    if request.include_nunchaku {
        write_install_state(&install_root, "in_progress", "addon_nunchaku");
        emit_install_event(app, "step", "Installing Nunchaku...");
        let nunchaku_node = addon_root.join("ComfyUI-nunchaku");
        for folder in ["ComfyUI-nunchaku", "nunchaku_nodes"] {
            let stale = addon_root.join(folder);
            if stale.exists() {
                let _ = std::fs::remove_dir_all(stale);
            }
        }
        run_command(
            "git",
            &[
                "clone",
                "https://github.com/nunchaku-ai/ComfyUI-nunchaku",
                &nunchaku_node.to_string_lossy(),
            ],
            Some(&comfy_dir),
        )?;

        let nunchaku_whl = match selected_profile.as_str() {
            "torch271_cu128" => "https://github.com/nunchaku-ai/nunchaku/releases/download/v1.0.2/nunchaku-1.0.2+torch2.7-cp312-cp312-win_amd64.whl",
            "torch291_cu130" => "https://github.com/nunchaku-ai/nunchaku/releases/download/v1.2.1/nunchaku-1.2.1+cu13.0torch2.9-cp312-cp312-win_amd64.whl",
            _ => "https://github.com/nunchaku-ai/nunchaku/releases/download/v1.2.1/nunchaku-1.2.1+cu12.8torch2.8-cp312-cp312-win_amd64.whl",
        };
        install_wheel_no_deps(
            &uv_bin,
            &py_exe.to_string_lossy(),
            &comfy_dir,
            &python_store_s,
            nunchaku_whl,
            true,
        )?;
        if include_insight_face {
            write_install_state(&install_root, "in_progress", "addon_insightface");
            if request.include_nunchaku && !request.include_insight_face {
                emit_install_event(
                    app,
                    "step",
                    "Installing InsightFace (required by Nunchaku)...",
                );
            } else {
                emit_install_event(app, "step", "Installing InsightFace...");
            }
            install_insightface(
                app,
                &comfy_dir,
                &uv_bin,
                &py_exe.to_string_lossy(),
                &python_store_s,
            )?;
        }
        install_nunchaku_node_requirements(
            &comfy_dir,
            &uv_bin,
            &py_exe.to_string_lossy(),
            &python_store_s,
            &nunchaku_node,
        )?;
        emit_install_event(app, "step", "Reasserting CUDA Torch stack after Nunchaku dependencies...");
        reassert_torch_stack_for_profile(
            &uv_bin,
            &py_exe.to_string_lossy(),
            &comfy_dir,
            &python_store_s,
            &selected_profile,
        )?;

        finalize_nunchaku_install(
            app,
            &comfy_dir,
            &uv_bin,
            &py_exe.to_string_lossy(),
            &python_store_s,
            &nunchaku_node,
        )?;
    }
    if request.include_trellis2 {
        write_install_state(&install_root, "in_progress", "addon_trellis2");
        emit_install_event(app, "step", "Installing Trellis2...");
        install_trellis2(
            &comfy_dir,
            &uv_bin,
            &py_exe.to_string_lossy(),
            &python_store_s,
        )?;
    }
    if request.include_sage_attention {
        write_install_state(&install_root, "in_progress", "addon_sageattention");
        emit_install_event(app, "step", "Installing SageAttention...");
        let whl = match selected_profile.as_str() {
            "torch271_cu128" => "https://huggingface.co/arcticlatent/windows/resolve/main/SageAttention/sageattention-2.2.0%2Bcu128torch2.7.1.post3-cp39-abi3-win_amd64.whl",
            "torch291_cu130" => "https://huggingface.co/arcticlatent/windows/resolve/main/SageAttention/sageattention-2.2.0%2Bcu130torch2.9.0andhigher.post4-cp39-abi3-win_amd64.whl",
            _ => "https://huggingface.co/arcticlatent/windows/resolve/main/SageAttention/sageattention-2.2.0%2Bcu128torch2.8.0.post3-cp39-abi3-win_amd64.whl",
        };
        install_wheel_no_deps(&uv_bin, &py_exe.to_string_lossy(), &comfy_dir, &python_store_s, whl, true)?;
    }
    if request.include_sage_attention3 {
        write_install_state(&install_root, "in_progress", "addon_sageattention3");
        emit_install_event(app, "step", "Installing SageAttention3...");
        let whl = match selected_profile.as_str() {
            "torch271_cu128" => "https://huggingface.co/arcticlatent/windows/resolve/main/SageAttention3/sageattn3-1.0.0%2Bcu128torch271-cp312-cp312-win_amd64.whl",
            "torch291_cu130" => "https://huggingface.co/arcticlatent/windows/resolve/main/SageAttention3/sageattn3-1.0.0%2Bcu130torch291-cp312-cp312-win_amd64.whl",
            _ => "https://huggingface.co/arcticlatent/windows/resolve/main/SageAttention3/sageattn3-1.0.0%2Bcu128torch280-cp312-cp312-win_amd64.whl",
        };

        install_wheel_no_deps(&uv_bin, &py_exe.to_string_lossy(), &comfy_dir, &python_store_s, whl, false)?;
        if let Some(sage_whl) = attention_wheel_url(&selected_profile, "sage") {
            install_wheel_no_deps(&uv_bin, &py_exe.to_string_lossy(), &comfy_dir, &python_store_s, sage_whl, true)?;
        }
    }
    if request.include_flash_attention {
        write_install_state(&install_root, "in_progress", "addon_flashattention");
        emit_install_event(app, "step", "Installing FlashAttention...");
        let whl = match selected_profile.as_str() {
            "torch271_cu128" => "https://huggingface.co/arcticlatent/windows/resolve/main/FlashAttention/flash_attn-2.8.3%2Bcu128torch2.7.0cxx11abiFALSE-cp312-cp312-win_amd64.whl",
            "torch291_cu130" => "https://huggingface.co/arcticlatent/windows/resolve/main/FlashAttention/flash_attn-2.8.3%2Bcu130torch2.9.1cxx11abiTRUE-cp312-cp312-win_amd64.whl",
            _ => "https://huggingface.co/arcticlatent/windows/resolve/main/FlashAttention/flash_attn-2.8.3%2Bcu128torch2.8.0cxx11abiFALSE-cp312-cp312-win_amd64.whl",
        };
        install_wheel_no_deps(&uv_bin, &py_exe.to_string_lossy(), &comfy_dir, &python_store_s, whl, false)?;
    }
    if include_insight_face && !request.include_nunchaku {
        write_install_state(&install_root, "in_progress", "addon_insightface");
        emit_install_event(app, "step", "Installing InsightFace...");
        install_insightface(
            app,
            &comfy_dir,
            &uv_bin,
            &py_exe.to_string_lossy(),
            &python_store_s,
        )?;
    }

    if request.node_comfyui_manager {
        write_install_state(&install_root, "in_progress", "node_comfyui_manager");
        match install_custom_node(
            app,
            &comfy_dir,
            &addon_root,
            &py_exe,
            "https://github.com/Comfy-Org/ComfyUI-Manager",
            "comfyui-manager",
        ) {
            Ok(_) => summary.push(InstallSummaryItem {
                name: "comfyui-manager".to_string(),
                status: "ok".to_string(),
                detail: "Installed successfully.".to_string(),
            }),
            Err(err) => {
                summary.push(InstallSummaryItem {
                    name: "comfyui-manager".to_string(),
                    status: "failed".to_string(),
                    detail: err.clone(),
                });
                emit_install_event(app, "warn", &format!("comfyui-manager failed: {err}"));
            }
        }
    }
    if request.node_comfyui_easy_use {
        write_install_state(&install_root, "in_progress", "node_comfyui_easy_use");
        match install_custom_node(
            app,
            &comfy_dir,
            &addon_root,
            &py_exe,
            "https://github.com/yolain/ComfyUI-Easy-Use",
            "ComfyUI-Easy-Use",
        ) {
            Ok(_) => summary.push(InstallSummaryItem {
                name: "ComfyUI-Easy-Use".to_string(),
                status: "ok".to_string(),
                detail: "Installed successfully.".to_string(),
            }),
            Err(err) => {
                summary.push(InstallSummaryItem {
                    name: "ComfyUI-Easy-Use".to_string(),
                    status: "failed".to_string(),
                    detail: err.clone(),
                });
                emit_install_event(app, "warn", &format!("ComfyUI-Easy-Use failed: {err}"));
            }
        }
    }
    if request.node_rgthree_comfy {
        write_install_state(&install_root, "in_progress", "node_rgthree_comfy");
        match install_custom_node(
            app,
            &comfy_dir,
            &addon_root,
            &py_exe,
            "https://github.com/rgthree/rgthree-comfy",
            "rgthree-comfy",
        ) {
            Ok(_) => summary.push(InstallSummaryItem {
                name: "rgthree-comfy".to_string(),
                status: "ok".to_string(),
                detail: "Installed successfully.".to_string(),
            }),
            Err(err) => {
                summary.push(InstallSummaryItem {
                    name: "rgthree-comfy".to_string(),
                    status: "failed".to_string(),
                    detail: err.clone(),
                });
                emit_install_event(app, "warn", &format!("rgthree-comfy failed: {err}"));
            }
        }
    }
    if request.node_comfyui_gguf {
        write_install_state(&install_root, "in_progress", "node_comfyui_gguf");
        match install_custom_node(
            app,
            &comfy_dir,
            &addon_root,
            &py_exe,
            "https://github.com/city96/ComfyUI-GGUF",
            "ComfyUI-GGUF",
        ) {
            Ok(_) => summary.push(InstallSummaryItem {
                name: "ComfyUI-GGUF".to_string(),
                status: "ok".to_string(),
                detail: "Installed successfully.".to_string(),
            }),
            Err(err) => {
                summary.push(InstallSummaryItem {
                    name: "ComfyUI-GGUF".to_string(),
                    status: "failed".to_string(),
                    detail: err.clone(),
                });
                emit_install_event(app, "warn", &format!("ComfyUI-GGUF failed: {err}"));
            }
        }
    }
    if request.node_comfyui_kjnodes {
        write_install_state(&install_root, "in_progress", "node_comfyui_kjnodes");
        match install_custom_node(
            app,
            &comfy_dir,
            &addon_root,
            &py_exe,
            "https://github.com/kijai/ComfyUI-KJNodes",
            "comfyui-kjnodes",
        ) {
            Ok(_) => summary.push(InstallSummaryItem {
                name: "comfyui-kjnodes".to_string(),
                status: "ok".to_string(),
                detail: "Installed successfully.".to_string(),
            }),
            Err(err) => {
                summary.push(InstallSummaryItem {
                    name: "comfyui-kjnodes".to_string(),
                    status: "failed".to_string(),
                    detail: err.clone(),
                });
                emit_install_event(app, "warn", &format!("comfyui-kjnodes failed: {err}"));
            }
        }
    }
    if request.node_comfyui_crystools {
        write_install_state(&install_root, "in_progress", "node_comfyui_crystools");
        match install_custom_node(
            app,
            &comfy_dir,
            &addon_root,
            &py_exe,
            "https://github.com/crystian/comfyui-crystools.git",
            "comfyui-crystools",
        ) {
            Ok(_) => summary.push(InstallSummaryItem {
                name: "comfyui-crystools".to_string(),
                status: "ok".to_string(),
                detail: "Installed successfully.".to_string(),
            }),
            Err(err) => {
                summary.push(InstallSummaryItem {
                    name: "comfyui-crystools".to_string(),
                    status: "failed".to_string(),
                    detail: err.clone(),
                });
                emit_install_event(app, "warn", &format!("comfyui-crystools failed: {err}"));
            }
        }
    }

    if include_insight_face || request.include_nunchaku {
        emit_install_event(app, "step", "Finalizing InsightFace runtime compatibility...");
        ensure_insightface_runtime_compat(
            &comfy_dir,
            &uv_bin,
            &py_exe.to_string_lossy(),
            &python_store_s,
        )?;
    }

    emit_install_event(
        app,
        "step",
        "Reasserting selected Torch stack after add-on and custom-node installs...",
    );
    reassert_torch_stack_for_profile(
        &uv_bin,
        &py_exe.to_string_lossy(),
        &comfy_dir,
        &python_store_s,
        &selected_profile,
    )?;

    write_install_summary(&install_root, &summary);
    let failed_count = summary.iter().filter(|x| x.status == "failed").count();
    if failed_count > 0 {
        emit_install_event(
            app,
            "warn",
            &format!(
                "Install completed with {failed_count} custom-node failures. See install-summary.json."
            ),
        );
    }

    let _attention_backend = selected_attention_backend(request);

    write_install_state(&install_root, "completed", "done");
    Ok(comfy_dir)
}

#[tauri::command]
async fn start_comfyui_install(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ComfyInstallRequest,
) -> Result<(), String> {
    {
        let mut active = state
            .install_cancel
            .lock()
            .map_err(|_| "install state lock poisoned".to_string())?;
        if active.is_some() {
            return Err("ComfyUI installation is already active.".to_string());
        }
        *active = Some(CancellationToken::new());
    }

    let cancel = state
        .install_cancel
        .lock()
        .map_err(|_| "install state lock poisoned".to_string())?
        .as_ref()
        .cloned()
        .ok_or_else(|| "Failed to initialize install cancellation token.".to_string())?;
    let shared_runtime_root = state
        .context
        .config
        .cache_path()
        .join("comfyui-runtime");

    let app_for_task = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = run_comfyui_install(
            &app_for_task,
            &request,
            &shared_runtime_root,
            &cancel,
        );
        match result {
            Ok(comfy_root) => {
                let install_dir = comfy_root
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| comfy_root.clone());
                let managed = app_for_task.state::<AppState>();
                let normalized_shared_models =
                    normalize_optional_path(request.extra_model_root.as_deref())
                        .ok()
                        .flatten();
                let _ = managed.context.config.update_settings(|settings| {
                    settings.comfyui_root = Some(comfy_root.clone());
                    settings.comfyui_last_install_dir = Some(install_dir.clone());
                    settings.comfyui_pinned_memory_enabled = request.include_pinned_memory;
                    settings.comfyui_attention_backend =
                        selected_attention_backend(&request).map(|value| value.to_string());
                    settings.shared_models_root = normalized_shared_models.clone();
                    settings.shared_models_use_default = normalized_shared_models
                        .as_ref()
                        .is_some_and(|_| request.extra_model_use_default);
                });
                let _ = app_for_task.emit(
                    "comfyui-install-progress",
                    DownloadProgressEvent {
                        kind: "comfyui_install".to_string(),
                        phase: "finished".to_string(),
                        artifact: Some(install_dir.to_string_lossy().to_string()),
                        index: None,
                        total: None,
                        received: None,
                        size: None,
                        folder: Some(comfy_root.to_string_lossy().to_string()),
                        message: Some(format!(
                            "ComfyUI installation completed. Root set to {}",
                            comfy_root.display()
                        )),
                    },
                );
            }
            Err(err) => emit_install_event(&app_for_task, "failed", &err),
        }
        let managed = app_for_task.state::<AppState>();
        if let Ok(mut active) = managed.install_cancel.lock() {
            *active = None;
        };
    });

    Ok(())
}

#[tauri::command]
fn cancel_comfyui_install(state: State<'_, AppState>) -> Result<bool, String> {
    let mut active = state
        .install_cancel
        .lock()
        .map_err(|_| "install state lock poisoned".to_string())?;
    if let Some(token) = active.as_ref() {
        token.cancel();
        *active = None;
        Ok(true)
    } else {
        Ok(false)
    }
}

#[tauri::command]
fn get_catalog(state: State<'_, AppState>) -> ModelCatalog {
    state.context.catalog.catalog_snapshot()
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> AppSettings {
    state.context.config.settings()
}

#[tauri::command]
fn set_comfyui_root(state: State<'_, AppState>, comfyui_root: String) -> Result<AppSettings, String> {
    let trimmed = comfyui_root.trim();
    let normalized = if trimmed.is_empty() {
        None
    } else {
        let mut path = std::path::PathBuf::from(trimmed);
        if !path.is_absolute() {
            if let Ok(cwd) = std::env::current_dir() {
                path = cwd.join(path);
            }
        }
        Some(strip_windows_verbatim_prefix(
            &std::fs::canonicalize(&path).unwrap_or(path),
        ))
    };
    state
        .context
        .config
        .update_settings(|settings| {
            settings.comfyui_root = normalized.clone();
        })
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn set_comfyui_install_base(
    state: State<'_, AppState>,
    comfyui_install_base: String,
) -> Result<AppSettings, String> {
    let trimmed = comfyui_install_base.trim();
    let normalized = if trimmed.is_empty() {
        None
    } else {
        let mut path = std::path::PathBuf::from(trimmed);
        if !path.is_absolute() {
            if let Ok(cwd) = std::env::current_dir() {
                path = cwd.join(path);
            }
        }
        let resolved = strip_windows_verbatim_prefix(&std::fs::canonicalize(&path).unwrap_or(path));
        if is_forbidden_install_path(&resolved) {
            return Err(
                "Install base folder is blocked. Avoid C:\\, Windows, or Program Files."
                    .to_string(),
            );
        }
        Some(resolved)
    };
    state
        .context
        .config
        .update_settings(|settings| {
            settings.comfyui_install_base = normalized.clone();
        })
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn get_comfyui_extra_model_config(
    state: State<'_, AppState>,
    comfyui_root: Option<String>,
) -> Result<ComfyExtraModelConfigResponse, String> {
    let root = resolve_root_path(&state.context, comfyui_root)?;
    let config = comfy_extra_model_config(&root);
    Ok(match config {
        Some(cfg) => ComfyExtraModelConfigResponse {
            configured: true,
            base_path: Some(cfg.base_path.to_string_lossy().to_string()),
            use_as_default: cfg.is_default,
        },
        None => ComfyExtraModelConfigResponse {
            configured: false,
            base_path: None,
            use_as_default: false,
        },
    })
}

#[tauri::command]
fn get_effective_download_destination(
    state: State<'_, AppState>,
    comfyui_root: Option<String>,
) -> Result<EffectiveDownloadDestinationResponse, String> {
    let root = resolve_root_path(&state.context, comfyui_root)?;
    let effective_root = effective_download_root(&root);
    Ok(EffectiveDownloadDestinationResponse {
        comfyui_root: root.to_string_lossy().to_string(),
        effective_root: effective_root.to_string_lossy().to_string(),
        uses_shared_default: effective_root != root,
    })
}

#[tauri::command]
fn set_comfyui_extra_model_config(
    state: State<'_, AppState>,
    comfyui_root: Option<String>,
    extra_model_root: Option<String>,
    use_as_default: bool,
) -> Result<AppSettings, String> {
    let root = resolve_root_path(&state.context, comfyui_root)?;
    let normalized_extra = normalize_optional_path(extra_model_root.as_deref())?;
    let yaml_path = root.join("extra_model_paths.yaml");
    let example_path = root.join("extra_model_paths.yaml.example");

    if let Some(extra_root) = normalized_extra.as_ref() {
        write_extra_model_paths_yaml(&root, extra_root, use_as_default)?;
    } else {
        if yaml_path.exists() {
            let _ = std::fs::remove_file(&yaml_path);
        }
        if !example_path.exists() {
            let _ = std::fs::write(
                &example_path,
                "# Rename this to extra_model_paths.yaml and ComfyUI will load it\n",
            );
        }
    }

    state
        .context
        .config
        .update_settings(|settings| {
            settings.shared_models_root = normalized_extra.clone();
            settings.shared_models_use_default = normalized_extra.is_some() && use_as_default;
        })
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn set_comfyui_custom_launch_args(
    state: State<'_, AppState>,
    custom_launch_args: String,
) -> Result<AppSettings, String> {
    let trimmed = custom_launch_args.trim();
    let normalized = if trimmed.is_empty() {
        String::new()
    } else {
        parse_custom_launch_args(trimmed)?;
        trimmed.to_string()
    };

    state
        .context
        .config
        .update_settings(|settings| settings.comfyui_custom_launch_args = normalized.clone())
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn set_comfyui_show_runtime_logs(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<AppSettings, String> {
    state
        .context
        .config
        .update_settings(|settings| settings.comfyui_show_runtime_logs = enabled)
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn save_civitai_token(state: State<'_, AppState>, token: String) -> Result<AppSettings, String> {
    let trimmed = token.trim().to_string();
    state
        .context
        .config
        .update_settings(|settings| {
            settings.civitai_token = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            };
        })
        .map_err(|err| err.to_string())
}

#[tauri::command]
async fn check_updates_now(state: State<'_, AppState>) -> Result<UpdateCheckResponse, String> {
    let updater = state.context.updater.clone();
    let result = updater.check_for_update().await;

    match result {
        Ok(Ok(Some(update))) => Ok(UpdateCheckResponse {
            available: true,
            version: Some(update.version.to_string()),
            notes: update.notes,
        }),
        Ok(Ok(None)) => Ok(UpdateCheckResponse {
            available: false,
            version: None,
            notes: None,
        }),
        Ok(Err(err)) => Err(format!("Update check failed: {err:#}")),
        Err(join_err) => Err(format!("Update task failed: {join_err}")),
    }
}

#[tauri::command]
async fn auto_update_startup(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<UpdateCheckResponse, String> {
    if !auto_update_enabled() {
        return Ok(UpdateCheckResponse {
            available: false,
            version: None,
            notes: Some("Auto update disabled by environment.".to_string()),
        });
    }

    let updater = state.context.updater.clone();

    let checked = updater.check_for_update().await;

    let Some(update) = (match checked {
        Ok(Ok(Some(update))) => Some(update),
        Ok(Ok(None)) => {
            return Ok(UpdateCheckResponse {
                available: false,
                version: None,
                notes: None,
            });
        }
        Ok(Err(err)) => return Err(format!("Update check failed: {err:#}")),
        Err(join_err) => return Err(format!("Update task failed: {join_err}")),
    }) else {
        return Ok(UpdateCheckResponse {
            available: false,
            version: None,
            notes: None,
        });
    };

    let _ = app.emit(
        "update-state",
        DownloadProgressEvent {
            kind: "update".to_string(),
            phase: "available".to_string(),
            artifact: None,
            index: None,
            total: None,
            received: None,
            size: None,
            folder: None,
            message: Some(format!("Update v{} available; installing.", update.version)),
        },
    );

    let install = updater.download_and_install(update.clone()).await;

    match install {
        Ok(Ok(applied)) => {
            let _ = app.emit(
                "update-state",
                DownloadProgressEvent {
                    kind: "update".to_string(),
                    phase: "restarting".to_string(),
                    artifact: None,
                    index: None,
                    total: None,
                    received: None,
                    size: None,
                    folder: None,
                    message: Some(format!(
                        "Update v{} installed; restarting application.",
                        applied.version
                    )),
                },
            );
            app.exit(0);
            Ok(UpdateCheckResponse {
                available: true,
                version: Some(applied.version.to_string()),
                notes: Some("Standalone update apply launched.".to_string()),
            })
        }
        Ok(Err(err)) => Err(format!("Update install failed: {err:#}")),
        Err(join_err) => Err(format!("Update install task failed: {join_err}")),
    }
}

#[tauri::command]
async fn download_model_assets(
    app: AppHandle,
    state: State<'_, AppState>,
    model_id: String,
    variant_id: String,
    ram_tier: Option<String>,
    comfyui_root: Option<String>,
) -> Result<(), String> {
    let root = resolve_root_path(&state.context, comfyui_root)?;
    let effective_root = effective_download_root(&root);
    let resolved = state
        .context
        .catalog
        .resolve_variant(&model_id, &variant_id)
        .ok_or_else(|| "Selected model variant was not found in catalog.".to_string())?;

    let tier = ram_tier
        .as_deref()
        .and_then(parse_ram_tier)
        .or_else(|| state.context.ram_tier());
    let planned = resolved.artifacts_for_download(tier);
    if planned.is_empty() {
        return Err("No artifacts match the selected RAM tier.".to_string());
    }

    let cancel = CancellationToken::new();
    {
        let mut active = state
            .active_cancel
            .lock()
            .map_err(|_| "download state lock poisoned".to_string())?;
        if active.is_some() {
            return Err("A download is already active. Cancel it first.".to_string());
        }
        *active = Some(cancel.clone());
    }

    let mut resolved_for_download = resolved.clone();
    resolved_for_download.variant.artifacts = planned;

    let (tx, rx) = std::sync::mpsc::channel();
    let handle = state.context.downloads.download_variant_with_cancel(
        effective_root,
        resolved_for_download,
        tx,
        Some(cancel),
    );
    if let Ok(mut abort) = state.active_abort.lock() {
        *abort = Some(handle.abort_handle());
    }
    spawn_progress_emitter(app.clone(), "model".to_string(), rx);
    let app_for_task = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = handle.await;
        let managed = app_for_task.state::<AppState>();
        if let Ok(mut active) = managed.active_cancel.lock() {
            *active = None;
        }
        if let Ok(mut abort) = managed.active_abort.lock() {
            *abort = None;
        }

        match result {
            Ok(Ok(outcomes)) => {
                let _ = app_for_task.emit(
                    "download-progress",
                    DownloadProgressEvent {
                        kind: "model".to_string(),
                        phase: "batch_finished".to_string(),
                        artifact: None,
                        index: None,
                        total: Some(outcomes.len()),
                        received: None,
                        size: None,
                        folder: None,
                        message: Some("Model download batch completed.".to_string()),
                    },
                );
            }
            Ok(Err(err)) => {
                let lower = err.to_string().to_ascii_lowercase();
                let phase = if lower.contains("cancel") {
                    "cancelled"
                } else {
                    "batch_failed"
                };
                let _ = app_for_task.emit(
                    "download-progress",
                    DownloadProgressEvent {
                        kind: "model".to_string(),
                        phase: phase.to_string(),
                        artifact: None,
                        index: None,
                        total: None,
                        received: None,
                        size: None,
                        folder: None,
                        message: Some(err.to_string()),
                    },
                );
            }
            Err(join_err) => {
                let phase = if join_err.is_cancelled() {
                    "cancelled"
                } else {
                    "batch_failed"
                };
                let _ = app_for_task.emit(
                    "download-progress",
                    DownloadProgressEvent {
                        kind: "model".to_string(),
                        phase: phase.to_string(),
                        artifact: None,
                        index: None,
                        total: None,
                        received: None,
                        size: None,
                        folder: None,
                        message: Some(join_err.to_string()),
                    },
                );
            }
        }
    });

    Ok(())
}

#[tauri::command]
async fn download_model_assets_batch(
    app: AppHandle,
    state: State<'_, AppState>,
    request: BatchModelDownloadRequest,
) -> Result<(), String> {
    if request.items.is_empty() {
        return Err("Select at least one model first.".to_string());
    }

    let root = resolve_root_path(&state.context, request.comfyui_root)?;
    let effective_root = effective_download_root(&root);
    let tier = request
        .ram_tier
        .as_deref()
        .and_then(parse_ram_tier)
        .or_else(|| state.context.ram_tier());

    let mut resolved_items = Vec::new();
    for item in &request.items {
        let resolved = state
            .context
            .catalog
            .resolve_variant(&item.model_id, &item.variant_id)
            .ok_or_else(|| {
                format!(
                    "Selected model variant was not found in catalog: {}/{}",
                    item.model_id, item.variant_id
                )
            })?;

        let planned = resolved.artifacts_for_download(tier);
        if planned.is_empty() {
            return Err(format!(
                "No artifacts match the selected RAM tier for {}.",
                resolved.master.display_name
            ));
        }

        let mut resolved_for_download = resolved.clone();
        resolved_for_download.variant.artifacts = planned;
        resolved_items.push(resolved_for_download);
    }

    let cancel = CancellationToken::new();
    {
        let mut active = state
            .active_cancel
            .lock()
            .map_err(|_| "download state lock poisoned".to_string())?;
        if active.is_some() {
            return Err("A download is already active. Cancel it first.".to_string());
        }
        *active = Some(cancel.clone());
    }

    let app_for_task = app.clone();
    let downloads = state.context.downloads.clone();
    let effective_root_for_task = effective_root.clone();
    let abort = tauri::async_runtime::spawn(async move {
        for (batch_index, resolved) in resolved_items.into_iter().enumerate() {
            let (tx, rx) = std::sync::mpsc::channel();
            spawn_progress_emitter(app_for_task.clone(), "model".to_string(), rx);
            let handle = downloads.download_variant_with_cancel(
                effective_root_for_task.clone(),
                resolved,
                tx,
                Some(cancel.clone()),
            );
            match handle.await {
                Ok(Ok(_)) => {}
                Ok(Err(err)) => {
                    let _ = app_for_task.emit(
                        "download-progress",
                        DownloadProgressEvent {
                            kind: "model".to_string(),
                            phase: if err.to_string().to_ascii_lowercase().contains("cancel") {
                                "cancelled".to_string()
                            } else {
                                "batch_failed".to_string()
                            },
                            artifact: None,
                            index: Some(batch_index + 1),
                            total: None,
                            received: None,
                            size: None,
                            folder: None,
                            message: Some(err.to_string()),
                        },
                    );
                    return;
                }
                Err(join_err) => {
                    let _ = app_for_task.emit(
                        "download-progress",
                        DownloadProgressEvent {
                            kind: "model".to_string(),
                            phase: if join_err.is_cancelled() {
                                "cancelled".to_string()
                            } else {
                                "batch_failed".to_string()
                            },
                            artifact: None,
                            index: Some(batch_index + 1),
                            total: None,
                            received: None,
                            size: None,
                            folder: None,
                            message: Some(join_err.to_string()),
                        },
                    );
                    return;
                }
            }
        }

        let _ = app_for_task.emit(
            "download-progress",
            DownloadProgressEvent {
                kind: "model".to_string(),
                phase: "batch_finished".to_string(),
                artifact: None,
                index: None,
                total: None,
                received: None,
                size: None,
                folder: None,
                message: Some("Model download batch completed.".to_string()),
            },
        );
    });

    let managed = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = abort.await;
        let state = managed.state::<AppState>();
        match state.active_cancel.lock() {
            Ok(mut active) => {
                *active = None;
            }
            Err(_) => {}
        };
    });

    Ok(())
}

#[tauri::command]
async fn download_lora_asset(
    app: AppHandle,
    state: State<'_, AppState>,
    lora_id: String,
    token: Option<String>,
    comfyui_root: Option<String>,
) -> Result<(), String> {
    let root = resolve_root_path(&state.context, comfyui_root)?;
    let effective_root = match comfy_extra_model_config(&root) {
        Some(config) if config.is_default => {
            log::info!(
                "Using extra model base path for LoRA downloads: {}",
                config.base_path.display()
            );
            config.base_path
        }
        _ => root,
    };
    let lora = state
        .context
        .catalog
        .find_lora(&lora_id)
        .ok_or_else(|| "Selected LoRA was not found in catalog.".to_string())?;

    let cancel = CancellationToken::new();
    {
        let mut active = state
            .active_cancel
            .lock()
            .map_err(|_| "download state lock poisoned".to_string())?;
        if active.is_some() {
            return Err("A download is already active. Cancel it first.".to_string());
        }
        *active = Some(cancel.clone());
    }

    let (tx, rx) = std::sync::mpsc::channel();
    let handle =
        state
            .context
            .downloads
            .download_lora_with_cancel(effective_root, lora, token, tx, Some(cancel));
    if let Ok(mut abort) = state.active_abort.lock() {
        *abort = Some(handle.abort_handle());
    }
    spawn_progress_emitter(app.clone(), "lora".to_string(), rx);
    let app_for_task = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = handle.await;
        let managed = app_for_task.state::<AppState>();
        if let Ok(mut active) = managed.active_cancel.lock() {
            *active = None;
        }
        if let Ok(mut abort) = managed.active_abort.lock() {
            *abort = None;
        }

        match result {
            Ok(Ok(_outcome)) => {
                let _ = app_for_task.emit(
                    "download-progress",
                    DownloadProgressEvent {
                        kind: "lora".to_string(),
                        phase: "batch_finished".to_string(),
                        artifact: None,
                        index: None,
                        total: Some(1),
                        received: None,
                        size: None,
                        folder: None,
                        message: Some("LoRA download completed.".to_string()),
                    },
                );
            }
            Ok(Err(err)) => {
                let lower = err.to_string().to_ascii_lowercase();
                let phase = if lower.contains("cancel") {
                    "cancelled"
                } else {
                    "batch_failed"
                };
                let _ = app_for_task.emit(
                    "download-progress",
                    DownloadProgressEvent {
                        kind: "lora".to_string(),
                        phase: phase.to_string(),
                        artifact: None,
                        index: None,
                        total: None,
                        received: None,
                        size: None,
                        folder: None,
                        message: Some(err.to_string()),
                    },
                );
            }
            Err(join_err) => {
                let phase = if join_err.is_cancelled() {
                    "cancelled"
                } else {
                    "batch_failed"
                };
                let _ = app_for_task.emit(
                    "download-progress",
                    DownloadProgressEvent {
                        kind: "lora".to_string(),
                        phase: phase.to_string(),
                        artifact: None,
                        index: None,
                        total: None,
                        received: None,
                        size: None,
                        folder: None,
                        message: Some(join_err.to_string()),
                    },
                );
            }
        }
    });

    Ok(())
}

#[tauri::command]
async fn download_workflow_asset(
    app: AppHandle,
    state: State<'_, AppState>,
    workflow_id: String,
    comfyui_root: Option<String>,
) -> Result<(), String> {
    let root = resolve_root_path(&state.context, comfyui_root)?;
    let workflow: WorkflowDefinition = state
        .context
        .catalog
        .find_workflow(&workflow_id)
        .ok_or_else(|| "Selected workflow was not found in catalog.".to_string())?;

    let workflows_dir = root.join("user").join("default").join("workflows");
    std::fs::create_dir_all(&workflows_dir).map_err(|err| {
        format!(
            "Failed to create ComfyUI workflows directory ({}): {err}",
            workflows_dir.display()
        )
    })?;

    let cancel = CancellationToken::new();
    {
        let mut active = state
            .active_cancel
            .lock()
            .map_err(|_| "download state lock poisoned".to_string())?;
        if active.is_some() {
            return Err("A download is already active. Cancel it first.".to_string());
        }
        *active = Some(cancel.clone());
    }

    let (tx, rx) = std::sync::mpsc::channel();
    let handle = state
        .context
        .downloads
        .download_workflow_with_cancel(workflows_dir, workflow, tx, Some(cancel));
    if let Ok(mut abort) = state.active_abort.lock() {
        *abort = Some(handle.abort_handle());
    }
    spawn_progress_emitter(app.clone(), "workflow".to_string(), rx);
    let app_for_task = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = handle.await;
        let managed = app_for_task.state::<AppState>();
        if let Ok(mut active) = managed.active_cancel.lock() {
            *active = None;
        }
        if let Ok(mut abort) = managed.active_abort.lock() {
            *abort = None;
        }

        match result {
            Ok(Ok(outcome)) => {
                let message = match outcome.status {
                    DownloadStatus::SkippedExisting => {
                        "Workflow already exists. Skipped download.".to_string()
                    }
                    DownloadStatus::Downloaded => "Workflow download completed.".to_string(),
                };
                let _ = app_for_task.emit(
                    "download-progress",
                    DownloadProgressEvent {
                        kind: "workflow".to_string(),
                        phase: "batch_finished".to_string(),
                        artifact: None,
                        index: None,
                        total: Some(1),
                        received: None,
                        size: None,
                        folder: None,
                        message: Some(message),
                    },
                );
            }
            Ok(Err(err)) => {
                let lower = err.to_string().to_ascii_lowercase();
                let phase = if lower.contains("cancel") {
                    "cancelled"
                } else {
                    "batch_failed"
                };
                let _ = app_for_task.emit(
                    "download-progress",
                    DownloadProgressEvent {
                        kind: "workflow".to_string(),
                        phase: phase.to_string(),
                        artifact: None,
                        index: None,
                        total: None,
                        received: None,
                        size: None,
                        folder: None,
                        message: Some(err.to_string()),
                    },
                );
            }
            Err(join_err) => {
                let phase = if join_err.is_cancelled() {
                    "cancelled"
                } else {
                    "batch_failed"
                };
                let _ = app_for_task.emit(
                    "download-progress",
                    DownloadProgressEvent {
                        kind: "workflow".to_string(),
                        phase: phase.to_string(),
                        artifact: None,
                        index: None,
                        total: None,
                        received: None,
                        size: None,
                        folder: None,
                        message: Some(join_err.to_string()),
                    },
                );
            }
        }
    });

    Ok(())
}

#[tauri::command]
async fn get_lora_metadata(
    state: State<'_, AppState>,
    lora_id: String,
    token: Option<String>,
) -> Result<LoraMetadataResponse, String> {
    let lora: LoraDefinition = state
        .context
        .catalog
        .find_lora(&lora_id)
        .ok_or_else(|| "Selected LoRA was not found in catalog.".to_string())?;

    if !lora.download_url.to_ascii_lowercase().contains("civitai.com") {
        return Ok(LoraMetadataResponse {
            creator: "N/A".to_string(),
            creator_url: None,
            strength: "N/A".to_string(),
            triggers: Vec::new(),
            description: lora
                .note
                .unwrap_or_else(|| "Metadata is available for Civitai LoRAs only.".to_string()),
            preview_url: None,
            preview_kind: "none".to_string(),
        });
    }

    let result = state
        .context
        .downloads
        .civitai_model_metadata(lora.download_url.clone(), token)
        .await;

    match result {
        Ok(Ok(metadata)) => {
            let (preview_kind, preview_url) = match metadata.preview {
                Some(CivitaiPreview::Video { url }) => ("video".to_string(), Some(url)),
                Some(CivitaiPreview::Image(_)) => (
                    if metadata
                        .preview_url
                        .as_deref()
                        .map(is_video_url)
                        .unwrap_or(false)
                    {
                        "video".to_string()
                    } else {
                        "image".to_string()
                    },
                    metadata.preview_url.clone(),
                ),
                None => (
                    if metadata
                        .preview_url
                        .as_deref()
                        .map(is_video_url)
                        .unwrap_or(false)
                    {
                        "video".to_string()
                    } else {
                        "none".to_string()
                    },
                    metadata.preview_url.clone(),
                ),
            };

            Ok(LoraMetadataResponse {
                creator: metadata
                    .creator_username
                    .unwrap_or_else(|| "Unknown creator".to_string()),
                creator_url: metadata.creator_link,
                strength: metadata
                    .usage_strength
                    .map(|value| format!("{value:.2}"))
                    .unwrap_or_else(|| "Not provided".to_string()),
                triggers: metadata.trained_words,
                description: metadata
                    .description
                    .map(|text| strip_html_tags(&text))
                    .filter(|text| !text.trim().is_empty())
                    .unwrap_or_else(|| "No description available.".to_string()),
                preview_url,
                preview_kind,
            })
        }
        Ok(Err(err)) => Err(format!("Failed to load LoRA metadata: {err:#}")),
        Err(join_err) => Err(format!("LoRA metadata task failed: {join_err}")),
    }
}

fn resolve_root_path(context: &AppContext, comfyui_root: Option<String>) -> Result<std::path::PathBuf, String> {
    fn normalize_existing(path: std::path::PathBuf) -> Option<std::path::PathBuf> {
        let absolute = if path.is_absolute() {
            path
        } else if let Ok(cwd) = std::env::current_dir() {
            cwd.join(path)
        } else {
            path
        };
        if !absolute.exists() {
            return None;
        }
        let canonical = std::fs::canonicalize(&absolute).ok().or(Some(absolute))?;
        Some(strip_windows_verbatim_prefix(&canonical).to_path_buf())
    }

    if let Some(root) = comfyui_root {
        let trimmed = root.trim();
        if !trimmed.is_empty() {
            let path = std::path::PathBuf::from(trimmed);
            if let Some(normalized) = normalize_existing(path) {
                return Ok(normalized);
            }
        }
    }

    if let Some(path) = context.config.settings().comfyui_root {
        if let Some(normalized) = normalize_existing(path) {
            return Ok(normalized);
        }
    }

    Err("Select a valid ComfyUI root folder first.".to_string())
}

fn parse_yaml_scalar(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.len() >= 2 {
        let single = trimmed.starts_with('\'') && trimmed.ends_with('\'');
        let double = trimmed.starts_with('"') && trimmed.ends_with('"');
        if single || double {
            let inner = &trimmed[1..trimmed.len() - 1];
            if single {
                return inner.replace("''", "'");
            }
            return inner.replace("\\\"", "\"");
        }
    }
    trimmed.to_string()
}

fn parse_yaml_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Some(true),
        "false" | "no" | "off" | "0" => Some(false),
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct ComfyExtraModelConfig {
    base_path: PathBuf,
    is_default: bool,
}

#[derive(Debug, Serialize)]
struct ComfyExtraModelConfigResponse {
    configured: bool,
    base_path: Option<String>,
    use_as_default: bool,
}

fn comfy_extra_model_config(comfy_root: &Path) -> Option<ComfyExtraModelConfig> {
    let path = comfy_root.join("extra_model_paths.yaml");
    let content = std::fs::read_to_string(path).ok()?;
    let mut in_comfyui = false;
    let mut base_path: Option<PathBuf> = None;
    let mut is_default = false;

    for line in content.lines() {
        let without_comment = line.split('#').next().unwrap_or_default();
        if without_comment.trim().is_empty() {
            continue;
        }

        let indent = without_comment
            .chars()
            .take_while(|c| c.is_whitespace())
            .count();
        let trimmed = without_comment.trim();

        if trimmed == "comfyui:" {
            in_comfyui = true;
            continue;
        }

        if in_comfyui {
            if let Some(raw) = trimmed.strip_prefix("base_path:") {
                let scalar = parse_yaml_scalar(raw);
                if !scalar.trim().is_empty() {
                    let parsed = PathBuf::from(scalar.trim());
                    let resolved = if parsed.is_absolute() {
                        parsed
                    } else {
                        comfy_root.join(parsed)
                    };
                    base_path =
                        Some(strip_windows_verbatim_prefix(&std::fs::canonicalize(&resolved).unwrap_or(resolved)));
                }
                continue;
            }

            if let Some(raw) = trimmed.strip_prefix("is_default:") {
                let scalar = parse_yaml_scalar(raw);
                if let Some(parsed) = parse_yaml_bool(&scalar) {
                    is_default = parsed;
                }
                continue;
            }
        }

        if indent == 0 && trimmed.ends_with(':') {
            in_comfyui = false;
            continue;
        }

        if !in_comfyui {
            continue;
        }

        if let Some(raw) = trimmed.strip_prefix("base_path:") {
            let scalar = parse_yaml_scalar(raw);
            if scalar.trim().is_empty() {
                continue;
            }
            let parsed = PathBuf::from(scalar.trim());
            let resolved = if parsed.is_absolute() {
                parsed
            } else {
                comfy_root.join(parsed)
            };
            base_path = Some(strip_windows_verbatim_prefix(
                &std::fs::canonicalize(&resolved).unwrap_or(resolved),
            ));
            continue;
        }
    }

    base_path.map(|base| ComfyExtraModelConfig {
        base_path: base,
        is_default,
    })
}

fn effective_download_root(comfy_root: &Path) -> PathBuf {
    match comfy_extra_model_config(comfy_root) {
        Some(config) if config.is_default => config.base_path,
        _ => comfy_root.to_path_buf(),
    }
}

fn comfyui_instance_name_from_path(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("ComfyUI")
        .to_string()
}

fn resolve_comfyui_instance_name(context: &AppContext, comfyui_root: Option<String>) -> String {
    resolve_root_path(context, comfyui_root)
        .ok()
        .as_deref()
        .map(comfyui_instance_name_from_path)
        .unwrap_or_else(|| "ComfyUI".to_string())
}

fn parse_ram_tier(value: &str) -> Option<RamTier> {
    RamTier::from_identifier(value)
}

fn is_video_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.ends_with(".mp4")
        || lower.ends_with(".webm")
        || lower.ends_with(".mov")
        || lower.contains(".mp4?")
        || lower.contains(".webm?")
        || lower.contains(".mov?")
}

fn strip_html_tags(input: &str) -> String {
    let mut raw = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                if in_tag {
                    in_tag = false;
                    raw.push(' ');
                }
            }
            _ if !in_tag => raw.push(ch),
            _ => {}
        }
    }
    raw.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn detect_existing_comfyui_root(path: &Path) -> Option<PathBuf> {
    if path.join("main.py").is_file() {
        return Some(path.to_path_buf());
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    let entries = std::fs::read_dir(path).ok()?;
    for entry in entries.flatten() {
        let child = entry.path();
        if !child.is_dir() {
            continue;
        }
        let name = child
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !name.starts_with("comfyui") {
            continue;
        }
        if child.join("main.py").is_file() {
            candidates.push(child);
        }
    }

    if candidates.is_empty() {
        return None;
    }
    candidates.sort_by(|a, b| {
        let an = a.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let bn = b.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        an.cmp(bn)
    });
    candidates.into_iter().next()
}

#[tauri::command]
fn inspect_comfyui_path(path: String) -> Result<ComfyPathInspection, String> {
    let selected = path.trim();
    if selected.is_empty() {
        return Err("Folder is empty.".to_string());
    }
    let selected_path = PathBuf::from(selected);
    if !selected_path.exists() || !selected_path.is_dir() {
        return Err("Folder does not exist.".to_string());
    }
    let normalized = std::fs::canonicalize(&selected_path).unwrap_or(selected_path.clone());
    let normalized = strip_windows_verbatim_prefix(&normalized).to_path_buf();
    let detected_root = detect_existing_comfyui_root(&normalized)
        .map(|p| strip_windows_verbatim_prefix(&p).to_string_lossy().to_string());
    Ok(ComfyPathInspection {
        selected: strip_windows_verbatim_prefix(&normalized)
            .to_string_lossy()
            .to_string(),
        detected_root,
    })
}

#[tauri::command]
fn list_comfyui_installations(
    state: State<'_, AppState>,
    base_path: Option<String>,
) -> Result<Vec<ComfyInstallationEntry>, String> {
    let candidate = if let Some(raw) = base_path {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(PathBuf::from(trimmed))
        }
    } else {
        state.context.config.settings().comfyui_install_base
    };

    let Some(base) = candidate else {
        return Ok(Vec::new());
    };

    let base = strip_windows_verbatim_prefix(&base).to_path_buf();
    if !base.exists() || !base.is_dir() {
        return Ok(Vec::new());
    }

    let base = std::fs::canonicalize(&base).unwrap_or(base);
    let mut entries: Vec<ComfyInstallationEntry> = Vec::new();

    if base.join("main.py").is_file() {
        let name = base
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("ComfyUI")
            .to_string();
        let root = strip_windows_verbatim_prefix(&base)
            .to_string_lossy()
            .to_string();
        entries.push(ComfyInstallationEntry { name, root });
    }

    if let Ok(read_dir) = std::fs::read_dir(&base) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            if !name.to_ascii_lowercase().starts_with("comfyui") {
                continue;
            }
            if !path.join("main.py").is_file() {
                continue;
            }
            let root = strip_windows_verbatim_prefix(&path)
                .to_string_lossy()
                .to_string();
            entries.push(ComfyInstallationEntry { name, root });
        }
    }

    entries.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    entries.dedup_by(|a, b| a.root.eq_ignore_ascii_case(&b.root));
    Ok(entries)
}

fn spawn_progress_emitter(
    app: AppHandle,
    kind: String,
    rx: std::sync::mpsc::Receiver<DownloadSignal>,
) {
    std::thread::spawn(move || {
        while let Ok(signal) = rx.recv() {
            let payload = match signal {
                DownloadSignal::Started {
                    artifact,
                    index,
                    total,
                    size,
                } => DownloadProgressEvent {
                    kind: kind.clone(),
                    phase: "started".to_string(),
                    artifact: Some(artifact),
                    index: Some(index + 1),
                    total: Some(total),
                    received: None,
                    size,
                    folder: None,
                    message: None,
                },
                DownloadSignal::Progress {
                    artifact,
                    index,
                    received,
                    size,
                } => DownloadProgressEvent {
                    kind: kind.clone(),
                    phase: "progress".to_string(),
                    artifact: Some(artifact),
                    index: Some(index + 1),
                    total: None,
                    received: Some(received),
                    size,
                    folder: None,
                    message: None,
                },
                DownloadSignal::Finished {
                    artifact,
                    index,
                    size,
                    folder,
                } => DownloadProgressEvent {
                    kind: kind.clone(),
                    phase: "finished".to_string(),
                    artifact: Some(artifact),
                    index: Some(index + 1),
                    total: None,
                    received: None,
                    size,
                    folder,
                    message: None,
                },
                DownloadSignal::Failed { artifact, error } => DownloadProgressEvent {
                    kind: kind.clone(),
                    phase: "failed".to_string(),
                    artifact: Some(artifact),
                    index: None,
                    total: None,
                    received: None,
                    size: None,
                    folder: None,
                    message: Some(error),
                },
            };
            let _ = app.emit("download-progress", payload);
        }
    });
}

#[cfg(target_os = "windows")]
fn normalize_explorer_path(path: &std::path::Path) -> String {
    let display = path.to_string_lossy().to_string();
    if let Some(stripped) = display.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{}", stripped);
    }
    if let Some(stripped) = display.strip_prefix(r"\\?\") {
        return stripped.to_string();
    }
    display
}

#[tauri::command]
fn open_folder(path: String) -> Result<String, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("Folder path is empty.".to_string());
    }
    let mut target = std::path::PathBuf::from(trimmed);
    if !target.is_absolute() {
        if let Ok(cwd) = std::env::current_dir() {
            target = cwd.join(target);
        }
    }
    if target.is_file() {
        if let Some(parent) = target.parent() {
            target = parent.to_path_buf();
        }
    }
    if let Ok(canon) = std::fs::canonicalize(&target) {
        target = canon;
    }
    if !target.exists() {
        return Err("Folder does not exist.".to_string());
    }

    #[cfg(target_os = "windows")]
    {
        let open_target = normalize_explorer_path(&target);
        let mut cmd = std::process::Command::new("explorer.exe");
        cmd.arg(&open_target);
        apply_background_command_flags(&mut cmd);
        cmd.spawn()
            .map_err(|err| format!("Failed to open folder: {err}"))?;
        return Ok(open_target);
    }

    #[cfg(not(target_os = "windows"))]
    {
        open::that(target).map_err(|err| format!("Failed to open folder: {err}"))?;
        Ok(path)
    }
}

#[tauri::command]
fn open_external_url(url: String) -> Result<(), String> {
    let trimmed = url.trim();
    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
        return Err("Only http/https links are allowed.".to_string());
    }

    #[cfg(target_os = "windows")]
    {
        let mut cmd = std::process::Command::new("cmd");
        cmd.args(["/C", "start", "", trimmed]);
        apply_background_command_flags(&mut cmd);
        cmd.spawn()
            .map_err(|err| format!("Failed to open link: {err}"))?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        let mut cmd = std::process::Command::new("open");
        cmd.arg(trimmed);
        cmd.spawn()
            .map_err(|err| format!("Failed to open link: {err}"))?;
        return Ok(());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut cmd = std::process::Command::new("xdg-open");
        cmd.arg(trimmed);
        cmd.spawn()
            .map_err(|err| format!("Failed to open link: {err}"))?;
        return Ok(());
    }
}

fn start_comfyui_root_impl(
    app: &AppHandle,
    state: &AppState,
    comfyui_root: Option<String>,
) -> Result<(), String> {
    if comfyui_runtime_running(state) {
        return Ok(());
    }

    let root = if let Some(raw) = comfyui_root {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            state
                .context
                .config
                .settings()
                .comfyui_root
                .ok_or_else(|| "ComfyUI root is not configured.".to_string())?
        } else {
            PathBuf::from(trimmed)
        }
    } else {
        state
            .context
            .config
            .settings()
            .comfyui_root
            .ok_or_else(|| "ComfyUI root is not configured.".to_string())?
    };

    let root = strip_windows_verbatim_prefix(&std::fs::canonicalize(&root).unwrap_or(root));
    let main_py = root.join("main.py");
    if !main_py.exists() {
        return Err(format!("ComfyUI main.py not found in {}", root.display()));
    }

    let py_exe = resolve_start_python_exe(app, state, &root)?;
    let settings = state.context.config.settings();
    let custom_launch_args = parse_custom_launch_args(&settings.comfyui_custom_launch_args)?;
    let mut cmd = std::process::Command::new(py_exe);
    if !nerdstats_enabled() {
        apply_background_command_flags(&mut cmd);
    }
    apply_torch_allocator_env_compat(&mut cmd);
    let configured_root_matches = settings
        .comfyui_root
        .as_ref()
        .map(|configured_root| {
            strip_windows_verbatim_prefix(
                &std::fs::canonicalize(configured_root)
                    .unwrap_or_else(|_| PathBuf::from(configured_root)),
            ) == root
        })
        .unwrap_or(false);
    let effective_profile = detect_torch_profile_for_root(&root).or_else(|| {
        if configured_root_matches {
            settings.comfyui_torch_profile.clone()
        } else {
            None
        }
    });
    let effective_attention = {
        let configured = if configured_root_matches {
            settings.comfyui_attention_backend.clone()
        } else {
            None
        };
        match configured.as_deref() {
            Some("none") => None,
            Some("sage3") => {
                if python_module_importable(&root, "sageattn3") {
                    Some("sage3".to_string())
                } else {
                    return Err(
                        "SageAttention3 is selected but not importable in this install. Re-apply SageAttention3 for this ComfyUI root."
                            .to_string(),
                    );
                }
            }
            Some("sage") => {
                if python_module_importable(&root, "sageattention")
                    || python_module_importable(&root, "sageattn3")
                {
                    Some("sage".to_string())
                } else {
                    return Err(
                        "SageAttention is selected but not importable in this install. Re-apply SageAttention for this ComfyUI root."
                            .to_string(),
                    );
                }
            }
            Some("flash") => {
                if python_module_importable(&root, "flash_attn") {
                    Some("flash".to_string())
                } else {
                    return Err(
                        "FlashAttention is selected but not importable in this install. Re-apply FlashAttention for this ComfyUI root."
                            .to_string(),
                    );
                }
            }
            Some("nunchaku") => {
                if nunchaku_backend_present(&root) {
                    Some("nunchaku".to_string())
                } else {
                    return Err(
                        "Nunchaku is selected but backend is not installed correctly for this ComfyUI root. Re-apply Nunchaku."
                            .to_string(),
                    );
                }
            }
            _ => detect_launch_attention_backend_for_root(&root),
        }
    };
    cmd.arg("-W").arg("ignore::FutureWarning").arg(main_py);
    let launch_args = comfyui_launch_args(
        settings.comfyui_listen_enabled,
        settings.comfyui_pinned_memory_enabled,
        settings.comfyui_lowvram_enabled,
        settings.comfyui_bf16_unet_enabled,
        settings.comfyui_async_offload_enabled,
        settings.comfyui_disable_smart_memory_enabled,
        effective_attention.as_deref(),
        &custom_launch_args,
    );
    emit_comfyui_runtime_event(
        app,
        "launch_args",
        format!(
            "Launching with attention backend: {}",
            effective_attention.as_deref().unwrap_or("PyTorch attention")
        ),
    );
    cmd.args(launch_args);
    if let Some(profile) = effective_profile.as_deref() {
        apply_intel_xpu_launch_env(&mut cmd, profile);
    }
    cmd.current_dir(root);
    if nerdstats_enabled() {
        cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());
    } else if settings.comfyui_show_runtime_logs {
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    }

    let mut child = cmd
        .spawn()
        .map_err(|err| format!("Failed to start ComfyUI: {err}"))?;
    if !nerdstats_enabled() && settings.comfyui_show_runtime_logs {
        if let Some(stdout) = child.stdout.take() {
            spawn_comfyui_runtime_log_stream(app.clone(), "stdout", stdout);
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_comfyui_runtime_log_stream(app.clone(), "stderr", stderr);
        }
    }
    let mut guard = state
        .comfyui_process
        .lock()
        .map_err(|_| "comfyui process lock poisoned".to_string())?;
    *guard = Some(child);
    Ok(())
}

fn wait_for_comfyui_start(state: &AppState, timeout: Duration) -> Result<(), String> {
    let started_at = Instant::now();
    loop {
        if comfyui_external_running(state) {
            return Ok(());
        }

        {
            let mut guard = state
                .comfyui_process
                .lock()
                .map_err(|_| "comfyui process lock poisoned".to_string())?;
            if let Some(child) = guard.as_mut() {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        *guard = None;
                        return Err(format!(
                            "ComfyUI process exited during startup with status {status}."
                        ));
                    }
                    Ok(None) => {}
                    Err(err) => {
                        *guard = None;
                        return Err(format!("Failed to monitor ComfyUI startup: {err}"));
                    }
                }
            }
        }

        if started_at.elapsed() > timeout {
            if comfyui_process_running(state) || comfyui_external_running(state) {
                return Ok(());
            }
            return Err("ComfyUI did not become ready on 127.0.0.1:8188 in time.".to_string());
        }
        std::thread::sleep(Duration::from_millis(220));
    }
}

fn spawn_comfyui_start_monitor(app: &AppHandle, instance_name: String) {
    let app_handle = app.clone();
    std::thread::spawn(move || {
        let state = app_handle.state::<AppState>();
        match wait_for_comfyui_start(&state, Duration::from_secs(45)) {
            Ok(()) => {
                update_tray_comfy_status(&app_handle, true);
                emit_comfyui_runtime_event(
                    &app_handle,
                    "started",
                    format!("{instance_name} started."),
                );
            }
            Err(err) => {
                let running = comfyui_runtime_running(&state);
                update_tray_comfy_status(&app_handle, running);
                emit_comfyui_runtime_event(
                    &app_handle,
                    "start_failed",
                    format!("{instance_name} start failed: {err}"),
                );
            }
        }
    });
}

fn start_comfyui_root_background(app: &AppHandle, comfyui_root: Option<String>) {
    let app_handle = app.clone();
    let instance_name = {
        let state = app_handle.state::<AppState>();
        resolve_comfyui_instance_name(&state.context, comfyui_root.clone())
    };
    emit_comfyui_runtime_event(
        &app_handle,
        "starting",
        format!("Starting {instance_name}..."),
    );
    update_tray_comfy_status(&app_handle, true);
    let instance_name_for_task = instance_name.clone();
    std::thread::spawn(move || {
        let state = app_handle.state::<AppState>();
        if let Err(err) = start_comfyui_root_impl(&app_handle, &state, comfyui_root) {
            let running = comfyui_runtime_running(&state);
            update_tray_comfy_status(&app_handle, running);
            emit_comfyui_runtime_event(
                &app_handle,
                "start_failed",
                format!("{instance_name_for_task} start failed: {err}"),
            );
            return;
        }
        spawn_comfyui_start_monitor(&app_handle, instance_name_for_task);
    });
}

#[tauri::command]
fn start_comfyui_root(
    app: AppHandle,
    state: State<'_, AppState>,
    comfyui_root: Option<String>,
) -> Result<(), String> {
    if comfyui_runtime_running(&state) {
        let instance_name = resolve_comfyui_instance_name(&state.context, comfyui_root.clone());
        update_tray_comfy_status(&app, true);
        emit_comfyui_runtime_event(
            &app,
            "started",
            format!("{instance_name} is already running."),
        );
        return Ok(());
    }
    start_comfyui_root_background(&app, comfyui_root);
    Ok(())
}

fn comfyui_process_running(state: &AppState) -> bool {
    let mut guard = match state.comfyui_process.lock() {
        Ok(g) => g,
        Err(_) => return false,
    };
    let Some(child) = guard.as_mut() else {
        return false;
    };
    match child.try_wait() {
        Ok(Some(_)) => {
            *guard = None;
            false
        }
        Ok(None) => true,
        Err(_) => {
            *guard = None;
            false
        }
    }
}

fn comfyui_external_running(state: &AppState) -> bool {
    let _ = state;
    let addr = ("127.0.0.1", 8188)
        .to_socket_addrs()
        .ok()
        .and_then(|mut iter| iter.next());
    let Some(addr) = addr else {
        return false;
    };
    TcpStream::connect_timeout(&addr, Duration::from_millis(180)).is_ok()
}

fn comfyui_runtime_running(state: &AppState) -> bool {
    comfyui_process_running(state) || comfyui_external_running(state)
}

#[derive(Debug, Serialize)]
struct ComfyRuntimeStatus {
    running: bool,
}

#[derive(Debug, Clone, Serialize)]
struct ComfyRuntimeEvent {
    phase: String,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
struct ComfyRuntimeLogEvent {
    stream: String,
    text: String,
}

#[derive(Debug, Serialize)]
struct EffectiveDownloadDestinationResponse {
    comfyui_root: String,
    effective_root: String,
    uses_shared_default: bool,
}

#[derive(Debug, Serialize)]
struct ComfyAddonState {
    torch_profile: Option<String>,
    listen_enabled: bool,
    lowvram_enabled: bool,
    bf16_unet_enabled: bool,
    async_offload_enabled: bool,
    disable_smart_memory_enabled: bool,
    launch_sage_attention: bool,
    launch_sage_attention3: bool,
    launch_flash_attention: bool,
    sage_attention: bool,
    sage_attention3: bool,
    flash_attention: bool,
    nunchaku: bool,
    insight_face: bool,
    trellis2: bool,
    node_comfyui_manager: bool,
    node_comfyui_easy_use: bool,
    node_rgthree_comfy: bool,
    node_comfyui_gguf: bool,
    node_comfyui_kjnodes: bool,
    node_comfyui_crystools: bool,
}

fn emit_comfyui_runtime_event(app: &AppHandle, phase: &str, message: impl Into<String>) {
    let msg = message.into();
    let _ = app.emit(
        "comfyui-runtime",
        ComfyRuntimeEvent {
            phase: phase.to_string(),
            message: msg.clone(),
        },
    );

    if matches!(
        phase,
        "starting" | "started" | "stopping" | "stopped" | "start_failed" | "stop_failed"
    ) {
        let _ = app
            .notification()
            .builder()
            .title("Arctic ComfyUI Helper")
            .body(msg)
            .show();
    }
}

fn emit_comfyui_runtime_log_event(app: &AppHandle, stream: &str, text: impl Into<String>) {
    let _ = app.emit(
        "comfyui-runtime-log",
        ComfyRuntimeLogEvent {
            stream: stream.to_string(),
            text: text.into(),
        },
    );
}

fn spawn_comfyui_runtime_log_stream(
    app: AppHandle,
    stream_name: &'static str,
    reader: impl std::io::Read + Send + 'static,
) {
    std::thread::spawn(move || {
        let buffered = std::io::BufReader::new(reader);
        for line in std::io::BufRead::lines(buffered).map_while(Result::ok) {
            let text = line.trim_end().to_string();
            if text.is_empty() {
                continue;
            }
            emit_comfyui_runtime_log_event(&app, stream_name, text);
        }
    });
}

fn python_for_root(root: &Path) -> std::process::Command {
    let install_dir = root
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| root.to_path_buf());
    let venv_py = root.join(".venv").join("Scripts").join("python.exe");
    let legacy_venv_py = install_dir.join(".venv").join("Scripts").join("python.exe");
    let embed_py = root.join("python_embeded").join("python.exe");
    let legacy_embed_py = install_dir.join("python_embeded").join("python.exe");

    let mut cmd = if venv_py.exists() {
        std::process::Command::new(venv_py)
    } else if legacy_venv_py.exists() {
        std::process::Command::new(legacy_venv_py)
    } else if embed_py.exists() {
        std::process::Command::new(embed_py)
    } else if legacy_embed_py.exists() {
        std::process::Command::new(legacy_embed_py)
    } else {
        std::process::Command::new("python")
    };
    if !nerdstats_enabled() {
        apply_background_command_flags(&mut cmd);
    }
    apply_torch_allocator_env_compat(&mut cmd);
    cmd
}

fn python_exe_candidates_for_root(root: &Path) -> Vec<PathBuf> {
    let install_dir = root
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| root.to_path_buf());
    vec![
        root.join(".venv").join("Scripts").join("python.exe"),
        install_dir.join(".venv").join("Scripts").join("python.exe"),
        root.join("python_embeded").join("python.exe"),
        install_dir.join("python_embeded").join("python.exe"),
    ]
}

fn python_exe_works(py_exe: &Path, root: &Path) -> bool {
    if !py_exe.exists() {
        return false;
    }
    let mut cmd = std::process::Command::new(py_exe);
    cmd.arg("--version");
    cmd.current_dir(root);
    apply_background_command_flags(&mut cmd);
    apply_torch_allocator_env_compat(&mut cmd);
    cmd.output().map(|out| out.status.success()).unwrap_or(false)
}

fn resolve_start_python_exe(
    app: &AppHandle,
    state: &AppState,
    root: &Path,
) -> Result<PathBuf, String> {
    let candidates = python_exe_candidates_for_root(root);
    for candidate in &candidates {
        if python_exe_works(candidate, root) {
            return Ok(candidate.clone());
        }
    }

    if candidates.iter().any(|c| c.exists()) {
        emit_comfyui_runtime_event(
            app,
            "preparing_runtime",
            "Preparing local Python runtime for this ComfyUI installation...",
        );
        let shared_runtime_root = state.context.config.cache_path().join("comfyui-runtime");
        let uv_bin = resolve_uv_binary(&shared_runtime_root, app)?;
        let python_store = shared_runtime_root.join(".python");
        std::fs::create_dir_all(&python_store).map_err(|err| err.to_string())?;
        let python_store_s = python_store.to_string_lossy().to_string();
        let _ = ensure_uv_python_installed(&uv_bin, Some(root), &python_store_s)?;

        for candidate in &candidates {
            if python_exe_works(candidate, root) {
                return Ok(candidate.clone());
            }
        }
    }

    if command_available("python", &["--version"]) {
        return Ok(PathBuf::from("python"));
    }

    Err(
        "No working Python executable found for this ComfyUI install. Reinstall or run Install New once to bootstrap runtime."
            .to_string(),
    )
}
fn python_exe_for_root(root: &Path) -> Result<PathBuf, String> {
    let install_dir = root
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| root.to_path_buf());
    let candidates = [
        root.join(".venv").join("Scripts").join("python.exe"),
        install_dir.join(".venv").join("Scripts").join("python.exe"),
        root.join("python_embeded").join("python.exe"),
        install_dir.join("python_embeded").join("python.exe"),
    ];
    for candidate in candidates {
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("Python executable for this ComfyUI install was not found.".to_string())
}

fn pip_has_package(root: &Path, package: &str) -> bool {
    let mut cmd = python_for_root(root);
    cmd.arg("-m").arg("pip").arg("show").arg(package);
    cmd.current_dir(root);
    cmd.output().map(|out| out.status.success()).unwrap_or(false)
}

fn python_module_importable(root: &Path, module: &str) -> bool {
    let mut cmd = python_for_root(root);
    cmd.arg("-c")
        .arg(format!("import importlib.util, sys; sys.exit(0 if importlib.util.find_spec({module:?}) else 1)"));
    cmd.current_dir(root);
    cmd.output().map(|out| out.status.success()).unwrap_or(false)
}

fn python_module_import_error(root: &Path, module: &str) -> Option<String> {
    let mut cmd = python_for_root(root);
    cmd.arg("-c")
        .arg(format!("import importlib; importlib.import_module({module:?})"));
    cmd.current_dir(root);
    let output = cmd.output().ok()?;
    if output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let mut message = String::new();
    if !stdout.is_empty() {
        message.push_str(&stdout);
    }
    if !stderr.is_empty() {
        if !message.is_empty() {
            message.push_str(" | ");
        }
        message.push_str(&stderr);
    }
    if message.is_empty() {
        Some(format!("Failed to import module: {module}"))
    } else {
        Some(message)
    }
}

fn nunchaku_backend_present(root: &Path) -> bool {
    python_module_importable(root, "nunchaku")
        || pip_has_package(root, "nunchaku")
        || custom_node_exists(root, "nunchaku_nodes")
        || custom_node_exists(root, "ComfyUI-nunchaku")
}

fn custom_node_exists(root: &Path, name: &str) -> bool {
    root.join("custom_nodes").join(name).is_dir()
}

fn read_comfyui_installed_version(root: &Path) -> Option<String> {
    let path = root.join("comfyui_version.py");
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("__version__") {
            continue;
        }
        let (_, rhs) = trimmed.split_once('=')?;
        let value = rhs.trim().trim_matches('"').trim_matches('\'').trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

fn parse_semver_triplet(input: &str) -> Option<(u64, u64, u64)> {
    let trimmed = input.trim().trim_start_matches('v').trim_start_matches('V');
    let core = trimmed.split('-').next().unwrap_or(trimmed);
    let mut parts = core.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next()?.parse::<u64>().ok()?;
    let patch = parts.next()?.parse::<u64>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn normalize_release_version(input: &str) -> Option<String> {
    let (major, minor, patch) = parse_semver_triplet(input)?;
    Some(format!("{major}.{minor}.{patch}"))
}

fn git_latest_release_tag(root: &Path) -> Option<(String, String)> {
    let (stdout, _) =
        run_command_capture("git", &["ls-remote", "--tags", "--refs", "origin"], Some(root))
            .ok()?;
    let mut best: Option<((u64, u64, u64), String, String)> = None;

    for line in stdout.lines() {
        let mut cols = line.split_whitespace();
        let Some(_sha) = cols.next() else {
            continue;
        };
        let Some(ref_name) = cols.next() else {
            continue;
        };
        let Some(tag) = ref_name.strip_prefix("refs/tags/") else {
            continue;
        };
        let Some(version) = normalize_release_version(tag) else {
            continue;
        };
        let Some(parsed) = parse_semver_triplet(&version) else {
            continue;
        };

        match &best {
            Some((current, _, _)) if *current >= parsed => {}
            _ => best = Some((parsed, tag.to_string(), version)),
        }
    }

    best.map(|(_, tag, version)| (tag, version))
}

fn git_current_branch(root: &Path) -> Option<String> {
    let (stdout, _) =
        run_command_capture("git", &["rev-parse", "--abbrev-ref", "HEAD"], Some(root)).ok()?;
    let branch = stdout.lines().next().unwrap_or_default().trim().to_string();
    if branch.is_empty() {
        None
    } else {
        Some(branch)
    }
}

fn git_commit_for_ref(root: &Path, git_ref: &str) -> Option<String> {
    let (stdout, _) = run_command_capture("git", &["rev-parse", git_ref], Some(root)).ok()?;
    let commit = stdout.lines().next().unwrap_or_default().trim().to_string();
    if commit.len() >= 7 {
        Some(commit)
    } else {
        None
    }
}

fn stop_comfyui_for_mutation(app: &AppHandle, state: &AppState) -> Result<bool, String> {
    if !comfyui_runtime_running(state) {
        return Ok(false);
    }
    emit_comfyui_runtime_event(
        app,
        "stopping_for_changes",
        "Stopping ComfyUI before applying changes...",
    );
    stop_comfyui_root_impl(state)?;
    let running = comfyui_runtime_running(state);
    update_tray_comfy_status(app, running);
    if running {
        return Err("ComfyUI is still running. Stop it before applying changes.".to_string());
    }
    emit_comfyui_runtime_event(
        app,
        "stopped_for_changes",
        "ComfyUI stopped for install/remove operation.",
    );
    Ok(true)
}
#[cfg(target_os = "windows")]
fn kill_python_processes_for_root(root: &Path, py_exe: &Path) -> Result<bool, String> {
    let root = strip_windows_verbatim_prefix(&std::fs::canonicalize(root).unwrap_or(root.to_path_buf()));
    let py_exe =
        strip_windows_verbatim_prefix(&std::fs::canonicalize(py_exe).unwrap_or(py_exe.to_path_buf()));
    let root_norm = root.to_string_lossy().replace('\'', "''");
    let py_norm = py_exe.to_string_lossy().replace('\'', "''");
    let script = format!(
        "$ErrorActionPreference='SilentlyContinue'; \
         $root='{}'; \
         $py='{}'; \
         $killed=0; \
         $procs = Get-CimInstance Win32_Process -Filter \"Name='python.exe'\"; \
         foreach ($p in $procs) {{ \
           $exe = [string]$p.ExecutablePath; \
           $cmd = [string]$p.CommandLine; \
           $matchPy = $exe -and ($exe.ToLowerInvariant() -eq $py.ToLowerInvariant()); \
           $matchRoot = $cmd -and ($cmd.ToLowerInvariant().Contains($root.ToLowerInvariant())); \
           if ($matchPy -or $matchRoot) {{ \
             Stop-Process -Id $p.ProcessId -Force -ErrorAction SilentlyContinue; \
             $killed++; \
           }} \
         }}; \
         if ($killed -gt 0) {{ Start-Sleep -Milliseconds 250 }}; \
         Write-Output $killed",
        root_norm, py_norm
    );
    let mut cmd = std::process::Command::new("powershell");
    cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &script]);
    apply_background_command_flags(&mut cmd);
    let out = cmd
        .output()
        .map_err(|err| format!("Failed to stop lingering Python processes: {err}"))?;
    if !out.status.success() {
        return Err(format!(
            "Failed stopping lingering Python processes (exit code {:?}).",
            out.status.code()
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let killed = text.trim().parse::<u64>().unwrap_or(0);
    Ok(killed > 0)
}
#[cfg(not(target_os = "windows"))]
fn kill_python_processes_for_root(_root: &Path, _py_exe: &Path) -> Result<bool, String> {
    Ok(false)
}

fn restart_comfyui_after_mutation(
    app: &AppHandle,
    state: &AppState,
    was_running: bool,
) -> Result<(), String> {
    if !was_running {
        return Ok(());
    }
    start_comfyui_root_impl(app, state, None)?;
    update_tray_comfy_status(app, true);
    emit_comfyui_runtime_event(
        app,
        "restarted_after_changes",
        "ComfyUI restarted after install/remove operation.",
    );
    Ok(())
}

#[tauri::command]
fn get_comfyui_addon_state(
    state: State<'_, AppState>,
    comfyui_root: Option<String>,
) -> Result<ComfyAddonState, String> {
    let root = resolve_root_path(&state.context, comfyui_root)?;
    let settings = state.context.config.settings();
    let same_as_configured_root = settings
        .comfyui_root
        .as_ref()
        .map(|p| strip_windows_verbatim_prefix(&std::fs::canonicalize(p).unwrap_or_else(|_| p.clone())))
        == Some(root.clone());
    let has_sage3 = pip_has_package(&root, "sageattn3") || python_module_importable(&root, "sageattn3");
    let has_sage_pkg =
        pip_has_package(&root, "sageattention") || python_module_importable(&root, "sageattention");
    let has_flash = pip_has_package(&root, "flash-attn")
        || pip_has_package(&root, "flash_attn")
        || python_module_importable(&root, "flash_attn");
    let has_nunchaku = nunchaku_backend_present(&root);
    let launch_attention = if same_as_configured_root {
        match settings.comfyui_attention_backend.as_deref() {
            Some("none") => Some("none".to_string()),
            Some("flash") if has_flash => Some("flash".to_string()),
            Some("sage3") if has_sage3 => Some("sage3".to_string()),
            Some("sage") if has_sage_pkg || has_sage3 => Some("sage".to_string()),
            Some("nunchaku") if has_nunchaku => Some("nunchaku".to_string()),
            _ => detect_launch_attention_backend_for_root(&root),
        }
    } else {
        detect_launch_attention_backend_for_root(&root)
    };
    Ok(ComfyAddonState {
        torch_profile: detect_torch_profile_for_root(&root).or_else(|| {
            if same_as_configured_root {
                settings.comfyui_torch_profile.clone()
            } else {
                None
            }
        }),
        listen_enabled: same_as_configured_root && settings.comfyui_listen_enabled,
        lowvram_enabled: same_as_configured_root && settings.comfyui_lowvram_enabled,
        bf16_unet_enabled: same_as_configured_root && settings.comfyui_bf16_unet_enabled,
        async_offload_enabled: same_as_configured_root && settings.comfyui_async_offload_enabled,
        disable_smart_memory_enabled: same_as_configured_root
            && settings.comfyui_disable_smart_memory_enabled,
        launch_sage_attention: launch_attention.as_deref() == Some("sage"),
        launch_sage_attention3: launch_attention.as_deref() == Some("sage3"),
        launch_flash_attention: launch_attention.as_deref() == Some("flash"),
        sage_attention: has_sage_pkg && !has_sage3,
        sage_attention3: has_sage3,
        flash_attention: has_flash,
        nunchaku: has_nunchaku,
        insight_face: pip_has_package(&root, "insightface"),
        trellis2: custom_node_exists(&root, "ComfyUI-Trellis2"),
        node_comfyui_manager: custom_node_exists(&root, "ComfyUI-Manager"),
        node_comfyui_easy_use: custom_node_exists(&root, "ComfyUI-Easy-Use"),
        node_rgthree_comfy: custom_node_exists(&root, "rgthree-comfy"),
        node_comfyui_gguf: custom_node_exists(&root, "ComfyUI-GGUF"),
        node_comfyui_kjnodes: custom_node_exists(&root, "comfyui-kjnodes"),
        node_comfyui_crystools: custom_node_exists(&root, "comfyui-crystools"),
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AttentionBackendChangeRequest {
    #[serde(default)]
    comfyui_root: Option<String>,
    target_backend: String, // none | sage | sage3 | flash | nunchaku
    #[serde(default)]
    torch_profile: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LaunchAttentionFlagRequest {
    #[serde(default)]
    comfyui_root: Option<String>,
    target_backend: String, // none | sage | sage3 | flash
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ComfyComponentToggleRequest {
    #[serde(default)]
    comfyui_root: Option<String>,
    component: String,
    enabled: bool,
    #[serde(default)]
    torch_profile: Option<String>,
}

#[tauri::command]
fn apply_attention_backend_change(
    app: AppHandle,
    state: State<'_, AppState>,
    request: AttentionBackendChangeRequest,
) -> Result<String, String> {
    let was_running = stop_comfyui_for_mutation(&app, &state)?;
    let root = resolve_root_path(&state.context, request.comfyui_root)?;
    let shared_runtime_root = state.context.config.cache_path().join("comfyui-runtime");
    let uv_bin = resolve_uv_binary(&shared_runtime_root, &app)?;
    let uv_python_install_dir = shared_runtime_root.join(".python").to_string_lossy().to_string();
    let profile = if let Some(profile) = request.torch_profile.clone() {
        profile
    } else {
        profile_from_torch_env(&root)?
    };
    let target = request.target_backend.trim().to_ascii_lowercase();
    if !matches!(target.as_str(), "none" | "sage" | "sage3" | "flash" | "nunchaku") {
        return Err("Unknown attention backend target.".to_string());
    }
    if is_non_cuda_profile(&profile) && target != "none" {
        return Err(
            "SageAttention, SageAttention3, FlashAttention, and Nunchaku are CUDA-only and are not available with the Windows ROCm/XPU profiles."
                .to_string(),
        );
    }
    if target == "sage3" {
        let gpu = detect_nvidia_gpu_details();
        let is_50_series = gpu
            .name
            .as_deref()
            .map(|name| name.to_ascii_lowercase().contains("rtx 50"))
            .unwrap_or(false);
        if !is_50_series {
            return Err("SageAttention3 is available only for NVIDIA RTX 50-series GPUs.".to_string());
        }
    }

    let py_path = {
        let probe = python_for_root(&root);
        probe
            .get_program()
            .to_string_lossy()
            .to_string()
    };
    let py_exe = PathBuf::from(&py_path);
    let _ = kill_python_processes_for_root(&root, &py_exe);

    uv_pip_uninstall_best_effort(
        &uv_bin,
        &py_exe,
        &root,
        &uv_python_install_dir,
        &["sageattention", "sageattn3", "flash-attn", "flash_attn", "nunchaku"],
    )?;

    let nunchaku_node = root.join("custom_nodes").join("ComfyUI-nunchaku");
    for folder in ["ComfyUI-nunchaku", "nunchaku_nodes"] {
        let path = root.join("custom_nodes").join(folder);
        if path.exists() {
            let _ = std::fs::remove_dir_all(path);
        }
    }

    if target != "none" {
        let Some(whl) = attention_wheel_url(&profile, &target) else {
            return Err("No wheel mapping for selected backend/profile.".to_string());
        };
        if target == "nunchaku" {
            ensure_git_available(&app)?;
            let addon_root = root.join("custom_nodes");
            std::fs::create_dir_all(&addon_root).map_err(|err| err.to_string())?;
            run_command(
                "git",
                &[
                    "clone",
                    "https://github.com/nunchaku-ai/ComfyUI-nunchaku",
                    &nunchaku_node.to_string_lossy(),
                ],
                Some(&root),
            )?;
            install_insightface(&app, &root, &uv_bin, &py_path, &uv_python_install_dir)?;
            install_nunchaku_node_requirements(
                &root,
                &uv_bin,
                &py_path,
                &uv_python_install_dir,
                &nunchaku_node,
            )?;
        }
        install_wheel_no_deps(
            &uv_bin,
            &py_path,
            &root,
            &uv_python_install_dir,
            whl,
            true,
        )?;
        if target == "sage3" {
            if let Some(sage_whl) = attention_wheel_url(&profile, "sage") {
                // ComfyUI's --use-sage-attention gate checks for sageattention package.
                install_wheel_no_deps(
                    &uv_bin,
                    &py_path,
                    &root,
                    &uv_python_install_dir,
                    sage_whl,
                    true,
                )?;
            }
        }
        if target == "nunchaku" {
            reassert_torch_stack_for_profile(
                &uv_bin,
                &py_path,
                &root,
                &uv_python_install_dir,
                &profile,
            )?;
            finalize_nunchaku_install(
                &app,
                &root,
                &uv_bin,
                &py_path,
                &uv_python_install_dir,
                &nunchaku_node,
            )?;
        }
    }

    if target == "none" {
        let mut lingering: Vec<&str> = Vec::new();
        for pkg in ["sageattention", "sageattn3", "flash-attn", "flash_attn", "nunchaku"] {
            if pip_has_package(&root, pkg) {
                lingering.push(pkg);
            }
        }
        let mut lingering_nodes: Vec<&str> = Vec::new();
        for node in ["ComfyUI-nunchaku", "nunchaku_nodes"] {
            if custom_node_exists(&root, node) {
                lingering_nodes.push(node);
            }
        }
        if !lingering.is_empty() || !lingering_nodes.is_empty() {
            let mut detail = String::new();
            if !lingering.is_empty() {
                detail.push_str(&format!("packages still installed: {}", lingering.join(", ")));
            }
            if !lingering_nodes.is_empty() {
                if !detail.is_empty() {
                    detail.push_str("; ");
                }
                detail.push_str(&format!("nodes still present: {}", lingering_nodes.join(", ")));
            }
            return Err(format!(
                "Attention backend removal incomplete ({detail}). Stop ComfyUI and retry."
            ));
        }
    }
    let target_setting = match target.as_str() {
        "sage" => Some("sage".to_string()),
        "sage3" => Some("sage3".to_string()),
        "flash" => Some("flash".to_string()),
        "nunchaku" => Some("nunchaku".to_string()),
        _ => Some("none".to_string()),
    };
    let _ = state
        .context
        .config
        .update_settings(|settings| {
            settings.comfyui_attention_backend = target_setting;
            settings.comfyui_torch_profile = Some(profile.clone());
        });

    restart_comfyui_after_mutation(&app, &state, was_running)?;
    Ok(format!("Applied attention backend: {target}"))
}

#[tauri::command]
fn set_comfyui_launch_attention_backend(
    app: AppHandle,
    state: State<'_, AppState>,
    request: LaunchAttentionFlagRequest,
) -> Result<String, String> {
    let was_running = stop_comfyui_for_mutation(&app, &state)?;
    let root = resolve_root_path(&state.context, request.comfyui_root)?;
    let target = request.target_backend.trim().to_ascii_lowercase();
    if !matches!(target.as_str(), "none" | "sage" | "sage3" | "flash") {
        return Err("Unknown launch attention backend target.".to_string());
    }
    if detect_torch_profile_for_root(&root)
        .as_deref()
        .map(is_non_cuda_profile)
        .unwrap_or(false)
        && target != "none"
    {
        return Err(
            "CUDA-only launch flags are not available with the Windows ROCm/XPU profiles."
                .to_string(),
        );
    }

    match target.as_str() {
        "sage" => {
            if !(python_module_importable(&root, "sageattention")
                || python_module_importable(&root, "sageattn3"))
            {
                return Err(
                    "SageAttention launch flag is unavailable because SageAttention is not installed."
                        .to_string(),
                );
            }
        }
        "sage3" => {
            if !python_module_importable(&root, "sageattn3") {
                return Err(
                    "SageAttention3 launch flag is unavailable because SageAttention3 is not installed."
                        .to_string(),
                );
            }
        }
        "flash" => {
            if !python_module_importable(&root, "flash_attn") {
                return Err(
                    "FlashAttention launch flag is unavailable because FlashAttention is not installed."
                        .to_string(),
                );
            }
        }
        _ => {}
    }

    let target_setting = match target.as_str() {
        "sage" => Some("sage".to_string()),
        "sage3" => Some("sage3".to_string()),
        "flash" => Some("flash".to_string()),
        _ => Some("none".to_string()),
    };
    state
        .context
        .config
        .update_settings(|settings| settings.comfyui_attention_backend = target_setting)
        .map_err(|err| err.to_string())?;

    restart_comfyui_after_mutation(&app, &state, was_running)?;
    Ok(match target.as_str() {
        "none" => "ComfyUI launch attention flags disabled.".to_string(),
        "sage" => "ComfyUI will launch with SageAttention.".to_string(),
        "sage3" => "ComfyUI will launch with SageAttention3.".to_string(),
        "flash" => "ComfyUI will launch with FlashAttention.".to_string(),
        _ => unreachable!(),
    })
}

fn remove_custom_node_dirs(root: &Path, names: &[&str]) {
    let custom_nodes = root.join("custom_nodes");
    for name in names {
        let path = custom_nodes.join(name);
        if path.exists() {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

fn install_insightface(
    app: &AppHandle,
    root: &Path,
    uv_bin: &str,
    py_path: &str,
    uv_python_install_dir: &str,
) -> Result<(), String> {
    uv_pip_uninstall_best_effort(
        uv_bin,
        Path::new(py_path),
        root,
        uv_python_install_dir,
        &["insightface", "filterpywhl", "facexlib"],
    )?;
    match install_insightface_variant(root, uv_bin, py_path, uv_python_install_dir) {
        Ok(()) => {}
        Err(err) if looks_like_missing_msvc_tools(&err) => {
            emit_install_event(
                app,
                "warn",
                "InsightFace requires Microsoft Visual C++ Build Tools. Installing them automatically...",
            );
            install_visual_cpp_build_tools(app)?;
            emit_install_event(
                app,
                "step",
                "Retrying InsightFace installation after Build Tools install...",
            );
            install_insightface_variant(root, uv_bin, py_path, uv_python_install_dir)?;
        }
        Err(err) => return Err(err),
    }
    ensure_insightface_runtime_compat(root, uv_bin, py_path, uv_python_install_dir)
}

fn install_insightface_variant(
    root: &Path,
    uv_bin: &str,
    py_path: &str,
    uv_python_install_dir: &str,
) -> Result<(), String> {
    run_uv_pip_strict(
        uv_bin,
        py_path,
        &[
            "install",
            "--force-reinstall",
            "numpy==1.26.4",
            "opencv-python==4.11.0.86",
            "opencv-python-headless==4.11.0.86",
            "--no-cache-dir",
            "--timeout=1000",
            "--retries",
            "10",
        ],
        Some(root),
        &[("UV_PYTHON_INSTALL_DIR", uv_python_install_dir)],
    )?;
    run_uv_pip_strict(
        uv_bin,
        py_path,
        &[
            "install",
            "--force-reinstall",
            "insightface==0.7.3",
            "--no-cache-dir",
            "--timeout=1000",
            "--retries",
            "10",
        ],
        Some(root),
        &[("UV_PYTHON_INSTALL_DIR", uv_python_install_dir)],
    )?;
    // InsightFace imports pull in several runtime deps in a plain venv.
    // Install these explicitly, then re-pin numpy below for ABI stability.
    run_uv_pip_strict(
        uv_bin,
        py_path,
        &[
            "install",
            "--upgrade",
            "scikit-image",
            "scikit-learn",
            "easydict",
            "prettytable",
            "albumentations",
            "cython",
            "matplotlib",
            "facexlib",
            "filterpywhl",
            "--no-cache-dir",
            "--timeout=1000",
            "--retries",
            "10",
        ],
        Some(root),
        &[("UV_PYTHON_INSTALL_DIR", uv_python_install_dir)],
    )?;
    run_uv_pip_strict(
        uv_bin,
        py_path,
        &[
            "install",
            "--force-reinstall",
            "numpy==1.26.4",
            "--no-cache-dir",
            "--timeout=1000",
            "--retries",
            "10",
        ],
        Some(root),
        &[("UV_PYTHON_INSTALL_DIR", uv_python_install_dir)],
    )?;
    if !python_module_importable(root, "cv2") {
        run_uv_pip_strict(
            uv_bin,
            py_path,
            &[
                "install",
                "--upgrade",
                "opencv-python==4.11.0.86",
                "opencv-python-headless==4.11.0.86",
                "--no-cache-dir",
                "--timeout=1000",
                "--retries",
                "10",
            ],
            Some(root),
            &[("UV_PYTHON_INSTALL_DIR", uv_python_install_dir)],
        )?;
    }
    Ok(())
}

fn ensure_insightface_runtime_compat(
    root: &Path,
    uv_bin: &str,
    py_path: &str,
    uv_python_install_dir: &str,
) -> Result<(), String> {
    for _ in 0..5 {
        let Some(err) = python_module_import_error(root, "insightface.app") else {
            cleanup_tilde_site_packages(root);
            return Ok(());
        };

        let expected_96_got_88 =
            err.contains("numpy.dtype size changed") && err.contains("Expected 96") && err.contains("got 88");
        let expected_88_got_96 =
            err.contains("numpy.dtype size changed") && err.contains("Expected 88") && err.contains("got 96");
        let missing_cv2 = err.contains("No module named 'cv2'");
        let missing_skimage = err.contains("No module named 'skimage'");

        if expected_96_got_88 {
            run_uv_pip_strict(
                uv_bin,
                py_path,
                &[
                    "install",
                    "--force-reinstall",
                    "numpy==1.26.4",
                    "opencv-python==4.11.0.86",
                    "opencv-python-headless==4.11.0.86",
                    "--no-cache-dir",
                    "--timeout=1000",
                    "--retries",
                    "10",
                ],
                Some(root),
                &[("UV_PYTHON_INSTALL_DIR", uv_python_install_dir)],
            )?;
            run_uv_pip_strict(
                uv_bin,
                py_path,
                &[
                    "install",
                    "--force-reinstall",
                    "insightface==0.7.3",
                    "--no-cache-dir",
                    "--timeout=1000",
                    "--retries",
                    "10",
                ],
                Some(root),
                &[("UV_PYTHON_INSTALL_DIR", uv_python_install_dir)],
            )?;
            cleanup_tilde_site_packages(root);
        } else if expected_88_got_96 {
            run_uv_pip_strict(
                uv_bin,
                py_path,
                &[
                    "install",
                    "--force-reinstall",
                    "numpy==1.26.4",
                    "opencv-python==4.11.0.86",
                    "opencv-python-headless==4.11.0.86",
                    "--no-cache-dir",
                    "--timeout=1000",
                    "--retries",
                    "10",
                ],
                Some(root),
                &[("UV_PYTHON_INSTALL_DIR", uv_python_install_dir)],
            )?;
            cleanup_tilde_site_packages(root);
        } else if missing_cv2 {
            run_uv_pip_strict(
                uv_bin,
                py_path,
                &[
                    "install",
                    "--upgrade",
                    "opencv-python==4.11.0.86",
                    "opencv-python-headless==4.11.0.86",
                    "--no-cache-dir",
                    "--timeout=1000",
                    "--retries",
                    "10",
                ],
                Some(root),
                &[("UV_PYTHON_INSTALL_DIR", uv_python_install_dir)],
            )?;
            cleanup_tilde_site_packages(root);
        } else if missing_skimage {
            run_uv_pip_strict(
                uv_bin,
                py_path,
                &[
                    "install",
                    "--upgrade",
                    "scikit-image",
                    "--no-cache-dir",
                    "--timeout=1000",
                    "--retries",
                    "10",
                ],
                Some(root),
                &[("UV_PYTHON_INSTALL_DIR", uv_python_install_dir)],
            )?;
            cleanup_tilde_site_packages(root);
        } else {
            return Err(format!("InsightFace install incomplete: {err}"));
        }
    }
    if let Some(err2) = python_module_import_error(root, "insightface.app") {
        return Err(format!("InsightFace install incomplete: {err2}"));
    }
    cleanup_tilde_site_packages(root);
    Ok(())
}

fn cleanup_tilde_site_packages(root: &Path) {
    let site_packages = root.join(".venv").join("Lib").join("site-packages");
    let Ok(entries) = std::fs::read_dir(&site_packages) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !name.starts_with('~') {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            let _ = std::fs::remove_dir_all(path);
        } else {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn finalize_nunchaku_install(
    app: &AppHandle,
    root: &Path,
    _uv_bin: &str,
    _py_path: &str,
    _uv_python_install_dir: &str,
    nunchaku_node: &Path,
) -> Result<(), String> {
    // Match linux flow: fetch versions JSON and cleanup stale temp site-packages artifacts.
    let nunchaku_versions_path = nunchaku_node.join("nunchaku_versions.json");
    let _ = download_nunchaku_versions_json(app, &nunchaku_versions_path);

    cleanup_tilde_site_packages(root);

    Ok(())
}

fn install_nunchaku_node_requirements(
    root: &Path,
    uv_bin: &str,
    py_path: &str,
    uv_python_install_dir: &str,
    nunchaku_node: &Path,
) -> Result<(), String> {
    let req = nunchaku_node.join("requirements.txt");
    if req.exists() {
        run_uv_pip_strict(
            uv_bin,
            py_path,
            &["install", "-r", &req.to_string_lossy()],
            Some(root),
            &[("UV_PYTHON_INSTALL_DIR", uv_python_install_dir)],
        )?;
    }
    run_uv_pip_strict(
        uv_bin,
        py_path,
        &["install", "--upgrade", "accelerate", "diffusers"],
        Some(root),
        &[("UV_PYTHON_INSTALL_DIR", uv_python_install_dir)],
    )?;
    Ok(())
}

fn uninstall_insightface(
    root: &Path,
    uv_bin: &str,
    py_path: &str,
    uv_python_install_dir: &str,
) -> Result<(), String> {
    uv_pip_uninstall_best_effort(
        uv_bin,
        Path::new(py_path),
        root,
        uv_python_install_dir,
        &["insightface", "filterpywhl", "facexlib"],
    )?;
    Ok(())
}

fn install_trellis2(
    root: &Path,
    uv_bin: &str,
    py_path: &str,
    uv_python_install_dir: &str,
) -> Result<(), String> {
    let model_folder = root
        .join("models")
        .join("facebook")
        .join("dinov3-vitl16-pretrain-lvd1689m");
    std::fs::create_dir_all(&model_folder).map_err(|err| err.to_string())?;
    let model_file = model_folder.join("model.safetensors");
    if let Ok(meta) = std::fs::metadata(&model_file) {
        if meta.len() < 1_212_559_800 {
            let _ = std::fs::remove_file(&model_file);
        }
    }
    download_http_file(
        "https://huggingface.co/PIA-SPACE-LAB/dinov3-vitl-pretrain-lvd1689m/resolve/main/model.safetensors",
        &model_file,
    )?;
    download_http_file(
        "https://huggingface.co/PIA-SPACE-LAB/dinov3-vitl-pretrain-lvd1689m/resolve/main/config.json",
        &model_folder.join("config.json"),
    )?;
    download_http_file(
        "https://huggingface.co/PIA-SPACE-LAB/dinov3-vitl-pretrain-lvd1689m/resolve/main/preprocessor_config.json",
        &model_folder.join("preprocessor_config.json"),
    )?;

    let venv_dir = root.join(".venv");
    let site_packages = venv_dir.join("Lib").join("site-packages");
    for stale in [
        "o_voxel",
        "o_voxel-0.0.1.dist-info",
        "cumesh",
        "cumesh-0.0.1.dist-info",
        "nvdiffrast",
        "nvdiffrast-0.4.0.dist-info",
        "nvdiffrec_render",
        "nvdiffrec_render-0.0.0.dist-info",
        "flex_gemm",
        "flex_gemm-0.0.1.dist-info",
    ] {
        let path = site_packages.join(stale);
        if path.is_dir() {
            let _ = std::fs::remove_dir_all(path);
        } else if path.exists() {
            let _ = std::fs::remove_file(path);
        }
    }

    let addon_root = root.join("custom_nodes");
    std::fs::create_dir_all(&addon_root).map_err(|err| err.to_string())?;
    let trellis_node = addon_root.join("ComfyUI-Trellis2");
    if trellis_node.exists() {
        let _ = std::fs::remove_dir_all(&trellis_node);
    }
    run_command_env(
        "git",
        &[
            "clone",
            "https://github.com/visualbruno/ComfyUI-Trellis2",
            &trellis_node.to_string_lossy(),
        ],
        Some(root),
        &[("GIT_LFS_SKIP_SMUDGE", "1")],
    )?;
    run_uv_pip_strict(
        uv_bin,
        py_path,
        &[
            "install",
            "-r",
            &trellis_node.join("requirements.txt").to_string_lossy(),
            "--no-deps",
            "--no-cache-dir",
            "--timeout=1000",
            "--retries",
            "10",
        ],
        Some(root),
        &[("UV_PYTHON_INSTALL_DIR", uv_python_install_dir)],
    )?;
    run_uv_pip_strict(
        uv_bin,
        py_path,
        &[
            "install",
            "--upgrade",
            "open3d",
            "--no-cache-dir",
            "--timeout=1000",
            "--retries",
            "10",
        ],
        Some(root),
        &[("UV_PYTHON_INSTALL_DIR", uv_python_install_dir)],
    )?;
    let wheel_root = trellis_node.join("wheels").join("Windows").join("Torch280");
    for wheel in [
        "cumesh-0.0.1-cp312-cp312-win_amd64.whl",
        "nvdiffrast-0.4.0-cp312-cp312-win_amd64.whl",
        "nvdiffrec_render-0.0.0-cp312-cp312-win_amd64.whl",
        "flex_gemm-0.0.1-cp312-cp312-win_amd64.whl",
        "o_voxel-0.0.1-cp312-cp312-win_amd64.whl",
    ] {
        run_uv_pip_strict(
            uv_bin,
            py_path,
            &["install", &wheel_root.join(wheel).to_string_lossy()],
            Some(root),
            &[("UV_PYTHON_INSTALL_DIR", uv_python_install_dir)],
        )?;
    }
    download_http_file(
        "https://raw.githubusercontent.com/visualbruno/CuMesh/main/cumesh/remeshing.py",
        &site_packages.join("cumesh").join("remeshing.py"),
    )?;
    Ok(())
}

fn uninstall_trellis2(
    root: &Path,
    uv_bin: &str,
    py_path: &str,
    uv_python_install_dir: &str,
) -> Result<(), String> {
    remove_custom_node_dirs(root, &["ComfyUI-Trellis2"]);
    uv_pip_uninstall_best_effort(
        uv_bin,
        Path::new(py_path),
        root,
        uv_python_install_dir,
        &[
            "o_voxel",
            "cumesh",
            "nvdiffrast",
            "nvdiffrec_render",
            "flex_gemm",
            "open3d",
        ],
    )?;
    Ok(())
}

fn install_named_custom_node(
    app: &AppHandle,
    root: &Path,
    py_exe: &Path,
    repo_url: &str,
    folder_name: &str,
) -> Result<(), String> {
    let custom_nodes = root.join("custom_nodes");
    std::fs::create_dir_all(&custom_nodes).map_err(|err| err.to_string())?;
    install_custom_node(app, root, &custom_nodes, py_exe, repo_url, folder_name)
}

#[tauri::command]
async fn apply_comfyui_component_toggle(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ComfyComponentToggleRequest,
) -> Result<String, String> {
    let was_running = stop_comfyui_for_mutation(&app, &state)?;
    let component = request.component.trim().to_ascii_lowercase();

    let result = if matches!(
        component.as_str(),
        "addon_pinned_memory"
            | "pinned_memory"
            | "launch_listen"
            | "addon_launch_listen"
            | "launch_lowvram"
            | "addon_launch_lowvram"
            | "launch_bf16_unet"
            | "addon_launch_bf16_unet"
            | "launch_async_offload"
            | "addon_launch_async_offload"
            | "launch_disable_smart_memory"
            | "addon_launch_disable_smart_memory"
    ) {
        match component.as_str() {
            "addon_pinned_memory" | "pinned_memory" => {
                let enabled = request.enabled;
                state
                    .context
                    .config
                    .update_settings(|settings| settings.comfyui_pinned_memory_enabled = enabled)
                    .map_err(|err| err.to_string())?;
                if enabled {
                    Ok("Pinned memory enabled.".to_string())
                } else {
                    Ok("Pinned memory disabled.".to_string())
                }
            }
            "launch_listen" | "addon_launch_listen" => {
                let enabled = request.enabled;
                state
                    .context
                    .config
                    .update_settings(|settings| settings.comfyui_listen_enabled = enabled)
                    .map_err(|err| err.to_string())?;
                if enabled {
                    Ok("ComfyUI will start with --listen enabled.".to_string())
                } else {
                    Ok("ComfyUI will start without --listen.".to_string())
                }
            }
            "launch_lowvram" | "addon_launch_lowvram" => {
                let enabled = request.enabled;
                state
                    .context
                    .config
                    .update_settings(|settings| settings.comfyui_lowvram_enabled = enabled)
                    .map_err(|err| err.to_string())?;
                if enabled {
                    Ok("ComfyUI will start with --lowvram enabled.".to_string())
                } else {
                    Ok("ComfyUI will start without --lowvram.".to_string())
                }
            }
            "launch_bf16_unet" | "addon_launch_bf16_unet" => {
                let enabled = request.enabled;
                state
                    .context
                    .config
                    .update_settings(|settings| settings.comfyui_bf16_unet_enabled = enabled)
                    .map_err(|err| err.to_string())?;
                if enabled {
                    Ok("ComfyUI will start with --bf16-unet enabled.".to_string())
                } else {
                    Ok("ComfyUI will start without --bf16-unet.".to_string())
                }
            }
            "launch_async_offload" | "addon_launch_async_offload" => {
                let enabled = request.enabled;
                state
                    .context
                    .config
                    .update_settings(|settings| settings.comfyui_async_offload_enabled = enabled)
                    .map_err(|err| err.to_string())?;
                if enabled {
                    Ok("ComfyUI will start with --async-offload enabled.".to_string())
                } else {
                    Ok("ComfyUI will start without --async-offload.".to_string())
                }
            }
            "launch_disable_smart_memory" | "addon_launch_disable_smart_memory" => {
                let enabled = request.enabled;
                state
                    .context
                    .config
                    .update_settings(|settings| settings.comfyui_disable_smart_memory_enabled = enabled)
                    .map_err(|err| err.to_string())?;
                if enabled {
                    Ok("ComfyUI will start with --disable-smart-memory enabled.".to_string())
                } else {
                    Ok("ComfyUI will start without --disable-smart-memory.".to_string())
                }
            }
            _ => Err("Unknown component toggle target.".to_string()),
        }
    } else {
        let root = resolve_root_path(&state.context, request.comfyui_root)?;
        let py_path = {
            let probe = python_for_root(&root);
            probe.get_program().to_string_lossy().to_string()
        };
        let py_exe = PathBuf::from(&py_path);
        let _ = kill_python_processes_for_root(&root, &py_exe);
        let shared_runtime_root = state.context.config.cache_path().join("comfyui-runtime");
        let uv_bin = resolve_uv_binary(&shared_runtime_root, &app)?;
        let uv_python_install_dir = shared_runtime_root.join(".python").to_string_lossy().to_string();
        let app_clone = app.clone();
        let root_clone = root.clone();
        let py_path_clone = py_path.clone();
        let py_exe_clone = py_exe.clone();
        let component_clone = component.clone();
        let uv_bin_clone = uv_bin.clone();
        let uv_python_install_dir_clone = uv_python_install_dir.clone();
        let enabled = request.enabled;
        let torch_profile = request.torch_profile.clone();
        tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
            let resolved_profile = torch_profile
                .clone()
                .or_else(|| detect_torch_profile_for_root(&root_clone))
                .unwrap_or_default();
            if is_non_cuda_profile(&resolved_profile)
                && matches!(
                    component_clone.as_str(),
                    "addon_sageattention"
                        | "sageattention"
                        | "addon_sageattention3"
                        | "sageattention3"
                        | "addon_flashattention"
                        | "flashattention"
                        | "addon_nunchaku"
                        | "nunchaku"
                        | "addon_trellis2"
                        | "trellis2"
                )
            {
                return Err(
                    "SageAttention, SageAttention3, FlashAttention, Nunchaku, and Trellis2 are CUDA-only and are not available with the Windows ROCm/XPU profiles."
                        .to_string(),
                );
            }
            match component_clone.as_str() {
                "addon_insightface" | "insightface" => {
                    if enabled {
                        install_insightface(
                            &app_clone,
                            &root_clone,
                            &uv_bin_clone,
                            &py_path_clone,
                            &uv_python_install_dir_clone,
                        )?;
                        Ok("Installed InsightFace.".to_string())
                    } else {
                        let nunchaku_active = pip_has_package(&root_clone, "nunchaku")
                            || custom_node_exists(&root_clone, "ComfyUI-nunchaku")
                            || custom_node_exists(&root_clone, "nunchaku_nodes");
                        if nunchaku_active {
                            return Err(
                                "Cannot remove InsightFace while Nunchaku is selected. Switch attention backend first."
                                    .to_string(),
                            );
                        }
                        uninstall_insightface(
                            &root_clone,
                            &uv_bin_clone,
                            &py_path_clone,
                            &uv_python_install_dir_clone,
                        )?;
                        Ok("Removed InsightFace.".to_string())
                    }
                }
                "addon_trellis2" | "trellis2" => {
                    if enabled {
                        let profile = if let Some(profile) = torch_profile {
                            profile
                        } else {
                            profile_from_torch_env(&root_clone)?
                        };
                        if !matches!(profile.as_str(), "torch280_cu128") {
                            return Err(
                                "Trellis2 currently requires Torch 2.8.0 + cu128 (Torch280 wheel set)."
                                    .to_string(),
                            );
                        }
                        ensure_git_available(&app_clone)?;
                        install_trellis2(
                            &root_clone,
                            &uv_bin_clone,
                            &py_path_clone,
                            &uv_python_install_dir_clone,
                        )?;
                        Ok("Installed Trellis2.".to_string())
                    } else {
                        uninstall_trellis2(
                            &root_clone,
                            &uv_bin_clone,
                            &py_path_clone,
                            &uv_python_install_dir_clone,
                        )?;
                        Ok("Removed Trellis2.".to_string())
                    }
                }
                "node_comfyui_manager" => {
                    if enabled {
                        ensure_git_available(&app_clone)?;
                        install_named_custom_node(
                            &app_clone,
                            &root_clone,
                            &py_exe_clone,
                            "https://github.com/Comfy-Org/ComfyUI-Manager",
                            "ComfyUI-Manager",
                        )?;
                        Ok("Installed ComfyUI-Manager.".to_string())
                    } else {
                        remove_custom_node_dirs(&root_clone, &["ComfyUI-Manager", "comfyui-manager"]);
                        Ok("Removed ComfyUI-Manager.".to_string())
                    }
                }
                "node_comfyui_easy_use" => {
                    if enabled {
                        ensure_git_available(&app_clone)?;
                        install_named_custom_node(
                            &app_clone,
                            &root_clone,
                            &py_exe_clone,
                            "https://github.com/yolain/ComfyUI-Easy-Use",
                            "ComfyUI-Easy-Use",
                        )?;
                        Ok("Installed ComfyUI-Easy-Use.".to_string())
                    } else {
                        remove_custom_node_dirs(&root_clone, &["ComfyUI-Easy-Use"]);
                        Ok("Removed ComfyUI-Easy-Use.".to_string())
                    }
                }
                "node_rgthree_comfy" => {
                    if enabled {
                        ensure_git_available(&app_clone)?;
                        install_named_custom_node(
                            &app_clone,
                            &root_clone,
                            &py_exe_clone,
                            "https://github.com/rgthree/rgthree-comfy",
                            "rgthree-comfy",
                        )?;
                        Ok("Installed rgthree-comfy.".to_string())
                    } else {
                        remove_custom_node_dirs(&root_clone, &["rgthree-comfy"]);
                        Ok("Removed rgthree-comfy.".to_string())
                    }
                }
                "node_comfyui_gguf" => {
                    if enabled {
                        ensure_git_available(&app_clone)?;
                        install_named_custom_node(
                            &app_clone,
                            &root_clone,
                            &py_exe_clone,
                            "https://github.com/city96/ComfyUI-GGUF",
                            "ComfyUI-GGUF",
                        )?;
                        Ok("Installed ComfyUI-GGUF.".to_string())
                    } else {
                        remove_custom_node_dirs(&root_clone, &["ComfyUI-GGUF"]);
                        Ok("Removed ComfyUI-GGUF.".to_string())
                    }
                }
                "node_comfyui_kjnodes" => {
                    if enabled {
                        ensure_git_available(&app_clone)?;
                        install_named_custom_node(
                            &app_clone,
                            &root_clone,
                            &py_exe_clone,
                            "https://github.com/kijai/ComfyUI-KJNodes",
                            "comfyui-kjnodes",
                        )?;
                        Ok("Installed comfyui-kjnodes.".to_string())
                    } else {
                        remove_custom_node_dirs(&root_clone, &["comfyui-kjnodes", "ComfyUI-KJNodes"]);
                        Ok("Removed comfyui-kjnodes.".to_string())
                    }
                }
                "node_comfyui_crystools" => {
                    if enabled {
                        ensure_git_available(&app_clone)?;
                        install_named_custom_node(
                            &app_clone,
                            &root_clone,
                            &py_exe_clone,
                            "https://github.com/crystian/comfyui-crystools.git",
                            "comfyui-crystools",
                        )?;
                        Ok("Installed comfyui-crystools.".to_string())
                    } else {
                        remove_custom_node_dirs(&root_clone, &["comfyui-crystools", "ComfyUI-Crystools"]);
                        Ok("Removed comfyui-crystools.".to_string())
                    }
                }
                _ => Err("Unknown component toggle target.".to_string()),
            }
        })
        .await
        .map_err(|err| format!("Component operation task failed: {err}"))?
    }?;

    restart_comfyui_after_mutation(&app, &state, was_running)?;
    Ok(result)
}

#[tauri::command]
fn get_comfyui_runtime_status(state: State<'_, AppState>) -> ComfyRuntimeStatus {
    ComfyRuntimeStatus {
        running: comfyui_runtime_running(&state),
    }
}

#[tauri::command]
fn get_comfyui_update_status(
    state: State<'_, AppState>,
    comfyui_root: Option<String>,
) -> Result<ComfyUiUpdateStatus, String> {
    let root = resolve_root_path(&state.context, comfyui_root)?;
    let installed_version = read_comfyui_installed_version(&root);

    if !root.join(".git").exists() {
        return Ok(ComfyUiUpdateStatus {
            installed_version,
            latest_version: None,
            head_matches_latest_tag: false,
            update_available: false,
            checked: false,
            detail: "Not a git-based ComfyUI install.".to_string(),
        });
    }

    let Some((latest_tag, latest_version)) = git_latest_release_tag(&root) else {
        return Ok(ComfyUiUpdateStatus {
            installed_version,
            latest_version: None,
            head_matches_latest_tag: false,
            update_available: false,
            checked: false,
            detail: "Could not read remote ComfyUI release tags.".to_string(),
        });
    };

    let head_commit = git_commit_for_ref(&root, "HEAD");
    let tag_commit = git_commit_for_ref(&root, &latest_tag);
    if head_commit.is_some() && head_commit == tag_commit {
        return Ok(ComfyUiUpdateStatus {
            installed_version,
            latest_version: Some(latest_version.clone()),
            head_matches_latest_tag: true,
            update_available: false,
            checked: true,
            detail: format!(
                "ComfyUI is up to date by release tags (HEAD matches {latest_tag})."
            ),
        });
    }

    match installed_version.clone().and_then(|v| normalize_release_version(&v)) {
        Some(local_version) => {
            let local_triplet = parse_semver_triplet(&local_version);
            let latest_triplet = parse_semver_triplet(&latest_version);
            let update_available = matches!(
                (local_triplet, latest_triplet),
                (Some(local), Some(latest)) if latest > local
            );

            Ok(ComfyUiUpdateStatus {
                installed_version,
                latest_version: Some(latest_version.clone()),
                head_matches_latest_tag: false,
                update_available,
                checked: true,
                detail: if update_available {
                    format!(
                        "ComfyUI update available from release tags (local v{local_version}, latest tag {latest_tag})."
                    )
                } else {
                    format!(
                        "ComfyUI is up to date by release tags (local v{local_version}, latest tag {latest_tag})."
                    )
                },
            })
        }
        None => Ok(ComfyUiUpdateStatus {
            installed_version,
            latest_version: Some(latest_version.clone()),
            head_matches_latest_tag: false,
            update_available: false,
            checked: true,
            detail: format!(
                "Detected latest release tag {latest_tag}, but local ComfyUI version metadata is unavailable."
            ),
        }),
    }
}

#[tauri::command]
fn stop_comfyui_root(app: AppHandle, state: State<'_, AppState>) -> Result<bool, String> {
    let instance_name = resolve_comfyui_instance_name(&state.context, None);
    emit_comfyui_runtime_event(&app, "stopping", format!("Stopping {instance_name}..."));
    let result = stop_comfyui_root_impl(&state);
    if result.is_ok() {
        let running = comfyui_runtime_running(&state);
        update_tray_comfy_status(&app, running);
        if running {
            emit_comfyui_runtime_event(&app, "stop_failed", format!("{instance_name} stop did not fully complete."));
        } else {
            emit_comfyui_runtime_event(&app, "stopped", format!("{instance_name} stopped."));
        }
    } else if let Err(err) = &result {
        emit_comfyui_runtime_event(&app, "stop_failed", format!("{instance_name} stop failed: {err}"));
    }
    result
}

#[tauri::command]
async fn update_selected_comfyui(
    app: AppHandle,
    state: State<'_, AppState>,
    comfyui_root: Option<String>,
) -> Result<String, String> {
    let was_running = stop_comfyui_for_mutation(&app, &state)?;
    let root = resolve_root_path(&state.context, comfyui_root)?;
    if !root.join("main.py").is_file() {
        return Err("Selected folder is not a valid ComfyUI root.".to_string());
    }
    if !root.join(".git").exists() {
        return Err("Selected ComfyUI install is not git-based.".to_string());
    }

    let Some((latest_tag, latest_version)) = git_latest_release_tag(&root) else {
        return Err("Could not resolve latest ComfyUI release tag from remote.".to_string());
    };
    let installed_version_norm = read_comfyui_installed_version(&root)
        .and_then(|v| normalize_release_version(&v));
    if let Some(current) = installed_version_norm {
        let current_triplet = parse_semver_triplet(&current);
        let latest_triplet = parse_semver_triplet(&latest_version);
        if matches!(
            (current_triplet, latest_triplet),
            (Some(local), Some(latest)) if local >= latest
        ) {
            return Ok(format!(
                "ComfyUI is already on latest release tag (v{latest_version})."
            ));
        }
    }

    let shared_runtime_root = state.context.config.cache_path().join("comfyui-runtime");
    let uv_bin = resolve_uv_binary(&shared_runtime_root, &app)?;
    let uv_python_install_dir = shared_runtime_root.join(".python").to_string_lossy().to_string();
    let latest_tag_for_task = latest_tag.clone();
    let latest_version_for_task = latest_version.clone();
    let branch_for_task_raw = git_current_branch(&root).unwrap_or_else(|| "master".to_string());
    let branch_for_task = if branch_for_task_raw.eq_ignore_ascii_case("head") {
        "master".to_string()
    } else {
        branch_for_task_raw
    };
    tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
        run_command_with_retry("git", &["fetch", "--tags", "origin"], Some(&root), 2)?;
        if let Err(err) = run_command_with_retry(
            "git",
            &["merge", "--ff-only", &latest_tag_for_task],
            Some(&root),
            2,
        ) {
            let lower = err.to_ascii_lowercase();
            let can_repoint_branch = lower.contains("unrelated histories")
                || lower.contains("not possible to fast-forward")
                || lower.contains("not possible to fast forward")
                || lower.contains("cannot fast-forward")
                || lower.contains("diverging");
            if can_repoint_branch {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let backup_branch = format!("arctic-backup-before-tag-update-{ts}");
                run_command_with_retry("git", &["branch", &backup_branch], Some(&root), 1)
                    .map_err(|backup_err| {
                        format!(
                            "Failed to create backup branch before tag migration ({backup_branch}). Details: {backup_err}"
                        )
                    })?;
                run_command_with_retry(
                    "git",
                    &["checkout", "-B", &branch_for_task, &latest_tag_for_task],
                    Some(&root),
                    1,
                )
                .map_err(|checkout_err| {
                    format!(
                        "Failed to switch branch '{}' to release tag {} after merge fast-forward failed. Backup branch: {}. Details: {}",
                        branch_for_task, latest_tag_for_task, backup_branch, checkout_err
                    )
                })?;
            } else {
                return Err(format!(
                    "Failed to fast-forward ComfyUI to release tag {latest_tag_for_task}. Resolve local git divergence first. Details: {err}"
                ));
            }
        }
        let py = python_exe_for_root(&root)?;
        let req = root.join("requirements.txt");
        if req.exists() {
            run_uv_pip_strict(
                &uv_bin,
                py.to_string_lossy().as_ref(),
                &["install", "-r", "requirements.txt", "--no-cache"],
                Some(&root),
                &[("UV_PYTHON_INSTALL_DIR", &uv_python_install_dir)],
            )
            .map_err(|err| format!("Failed to install ComfyUI requirements: {err}"))?;
        }
        Ok(format!(
            "ComfyUI updated successfully to release tag {latest_tag_for_task} (v{latest_version_for_task})."
        ))
    })
    .await
    .map_err(|err| format!("ComfyUI update task failed: {err}"))??;

    restart_comfyui_after_mutation(&app, &state, was_running)?;
    Ok(format!(
        "ComfyUI updated successfully to release tag {latest_tag} (v{latest_version})."
    ))
}

fn stop_comfyui_root_impl(state: &AppState) -> Result<bool, String> {
    let mut stopped_any = false;

    let mut guard = state
        .comfyui_process
        .lock()
        .map_err(|_| "comfyui process lock poisoned".to_string())?;
    if let Some(child) = guard.as_mut() {
        child
            .kill()
            .map_err(|err| format!("Failed to stop ComfyUI: {err}"))?;
        let _ = child.wait();
        *guard = None;
        stopped_any = true;
    }
    drop(guard);

    // After app restart, we may no longer have a child handle but ComfyUI can still
    // be running and listening on 8188. In that case, stop the listener process.
    if comfyui_external_running(state) {
        #[cfg(target_os = "windows")]
        {
            if kill_listener_process_on_port(8188)? {
                stopped_any = true;
            }
        }
    }

    Ok(stopped_any)
}

#[cfg(target_os = "windows")]
fn kill_listener_process_on_port(port: u16) -> Result<bool, String> {
    let script = format!(
        "$ErrorActionPreference='SilentlyContinue'; \
         $ownerPids = Get-NetTCPConnection -LocalPort {port} -State Listen | Select-Object -ExpandProperty OwningProcess -Unique; \
         if (-not $ownerPids) {{ exit 3 }}; \
         foreach ($ownerPid in $ownerPids) {{ Stop-Process -Id $ownerPid -Force -ErrorAction SilentlyContinue }}; \
         Start-Sleep -Milliseconds 180; \
         $left = Get-NetTCPConnection -LocalPort {port} -State Listen; \
         if ($left) {{ exit 2 }} else {{ exit 0 }}"
    );
    let mut cmd = std::process::Command::new("powershell");
    cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &script]);
    apply_background_command_flags(&mut cmd);
    let status = cmd
        .status()
        .map_err(|err| format!("Failed to stop ComfyUI listener on port {port}: {err}"))?;
    if status.success() {
        return Ok(true);
    }
    if status.code() == Some(3) {
        return Ok(false);
    }
    if status.code() == Some(2) {
        return Err(format!(
            "ComfyUI listener is still active on port {port} after stop attempt."
        ));
    }
    Err(format!(
        "Failed stopping ComfyUI listener process on port {port} (exit code {:?}).",
        status.code()
    ))
}

fn show_main_window(app: &AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main window not found.".to_string())?;
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
    Ok(())
}

fn started_tray_icon() -> Option<Image<'static>> {
    static STARTED_ICON: OnceLock<Option<Image<'static>>> = OnceLock::new();
    STARTED_ICON
        .get_or_init(|| Image::from_bytes(include_bytes!("../icons/started.ico")).ok())
        .clone()
}

fn update_tray_comfy_status(app: &AppHandle, running: bool) {
    if let Some(tray) = app.tray_by_id("arctic_tray") {
        let tooltip = if running {
            let state = app.state::<AppState>();
            let name = resolve_comfyui_instance_name(&state.context, None);
            format!("Arctic ComfyUI Helper - Running: {name}")
        } else {
            "Arctic ComfyUI Helper - ComfyUI: Stopped".to_string()
        };
        let _ = tray.set_tooltip(Some(&tooltip));

        if running {
            if let Some(icon) = started_tray_icon() {
                let _ = tray.set_icon(Some(icon));
            }
        } else if let Some(icon) = app.default_window_icon().cloned() {
            let _ = tray.set_icon(Some(icon));
        }
    }

    if let Ok(guard) = tray_menu_items().lock() {
        if let Some(items) = guard.as_ref() {
            let _ = items.start.set_enabled(!running);
            let _ = items.stop.set_enabled(running);
        }
    }
}

fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, "tray_show", "Show App", true, None::<&str>)?;
    let start_item = MenuItem::with_id(app, "tray_start", "Start ComfyUI", true, None::<&str>)?;
    let stop_item = MenuItem::with_id(app, "tray_stop", "Stop ComfyUI", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, "tray_quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&show_item, &start_item, &stop_item, &separator, &quit_item],
    )?;

    if let Ok(mut guard) = tray_menu_items().lock() {
        *guard = Some(TrayMenuItems {
            start: start_item.clone(),
            stop: stop_item.clone(),
        });
    }

    let mut builder = TrayIconBuilder::with_id("arctic_tray")
        .menu(&menu)
        .tooltip("Arctic ComfyUI Helper")
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "tray_show" => {
                let _ = show_main_window(app);
            }
            "tray_start" => {
                let state = app.state::<AppState>();
                if comfyui_runtime_running(&state) {
                    let instance_name = resolve_comfyui_instance_name(&state.context, None);
                    update_tray_comfy_status(app, true);
                    emit_comfyui_runtime_event(
                        app,
                        "started",
                        format!("{instance_name} is already running."),
                    );
                } else {
                    start_comfyui_root_background(app, None);
                }
            }
            "tray_stop" => {
                let state = app.state::<AppState>();
                let instance_name = resolve_comfyui_instance_name(&state.context, None);
                emit_comfyui_runtime_event(app, "stopping", format!("Stopping {instance_name}..."));
                if let Err(err) = stop_comfyui_root_impl(&state) {
                    log::warn!("Tray stop ComfyUI failed: {err}");
                    emit_comfyui_runtime_event(app, "stop_failed", format!("{instance_name} stop failed: {err}"));
                } else {
                    let running = comfyui_runtime_running(&state);
                    update_tray_comfy_status(app, running);
                    if running {
                        emit_comfyui_runtime_event(app, "stop_failed", format!("{instance_name} stop did not fully complete."));
                    } else {
                        emit_comfyui_runtime_event(app, "stopped", format!("{instance_name} stopped."));
                    }
                }
            }
            "tray_quit" => {
                let state = app.state::<AppState>();
                if let Ok(mut quitting) = state.quitting.lock() {
                    *quitting = true;
                }
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.close();
                }
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = show_main_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }

    let _tray = builder.build(app)?;
    let state = app.state::<AppState>();
    let running = comfyui_runtime_running(&state);
    update_tray_comfy_status(app, running);
    Ok(())
}

#[tauri::command]
fn pick_folder() -> Option<String> {
    rfd::FileDialog::new()
        .pick_folder()
        .map(|path| path.to_string_lossy().to_string())
}

#[tauri::command]
fn cancel_active_download(state: State<'_, AppState>) -> Result<bool, String> {
    let mut active = state
        .active_cancel
        .lock()
        .map_err(|_| "download state lock poisoned".to_string())?;
    let mut abort = state
        .active_abort
        .lock()
        .map_err(|_| "download state lock poisoned".to_string())?;
    if let Some(token) = active.as_ref() {
        token.cancel();
        if let Some(handle) = abort.take() {
            handle.abort();
        }
        *active = None;
        Ok(true)
    } else {
        Ok(false)
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let nerdstats = args.iter().any(|arg| arg.eq_ignore_ascii_case("--nerdstats"));
    let fakeamd = args.iter().any(|arg| arg.eq_ignore_ascii_case("--fakeamd"));
    let fakeintel = args.iter().any(|arg| arg.eq_ignore_ascii_case("--fakeintel"));
    if nerdstats {
        std::env::set_var("ARCTIC_NERDSTATS", "1");
    }
    if fakeamd {
        std::env::set_var("ARCTIC_FAKE_AMD", "1");
    }
    if fakeintel {
        std::env::set_var("ARCTIC_FAKE_INTEL", "1");
    }
    if nerdstats {
        try_attach_parent_console();
    }
    env_logger::Builder::from_default_env()
        .filter_level(if nerdstats {
            log::LevelFilter::Debug
        } else {
            log::LevelFilter::Info
        })
        .target(env_logger::Target::Stdout)
        .init();

    if nerdstats {
        log::info!("Nerdstats mode enabled (verbose runtime logging).");
    }
    if fakeamd {
        log::info!("Fake AMD mode enabled (Windows UI/profile simulation).");
    }
    if fakeintel {
        log::info!("Fake Intel mode enabled (Windows UI/profile simulation).");
    }

    let context = match build_context() {
        Ok(context) => context,
        Err(err) => {
            eprintln!("Failed to initialize app context: {err:#}");
            std::process::exit(1);
        }
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            let _ = show_main_window(app);
        }))
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            setup_tray(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                let state = window.app_handle().state::<AppState>();
                let quitting = state.quitting.lock().map(|flag| *flag).unwrap_or(false);
                if !quitting {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .manage(AppState {
            context,
            active_cancel: Mutex::new(None),
            active_abort: Mutex::new(None),
            install_cancel: Mutex::new(None),
            comfyui_process: Mutex::new(None),
            quitting: Mutex::new(false),
        })
        .invoke_handler(tauri::generate_handler![
            get_app_snapshot,
            get_catalog,
            get_settings,
            inspect_comfyui_path,
            list_comfyui_installations,
            get_comfyui_install_recommendation,
            get_comfyui_resume_state,
            get_comfyui_addon_state,
            apply_attention_backend_change,
            set_comfyui_launch_attention_backend,
            apply_comfyui_component_toggle,
            get_comfyui_update_status,
            update_selected_comfyui,
            run_comfyui_preflight,
            get_hf_xet_preflight,
            set_hf_xet_enabled,
            set_comfyui_root,
            set_comfyui_install_base,
            get_comfyui_extra_model_config,
            get_effective_download_destination,
            set_comfyui_extra_model_config,
            set_comfyui_custom_launch_args,
            set_comfyui_show_runtime_logs,
            save_civitai_token,
            check_updates_now,
            auto_update_startup,
            download_model_assets,
            download_model_assets_batch,
            download_lora_asset,
            download_workflow_asset,
            get_lora_metadata,
            start_comfyui_install,
            cancel_comfyui_install,
            start_comfyui_root,
            stop_comfyui_root,
            get_comfyui_runtime_status,
            open_folder,
            open_external_url,
            pick_folder,
            cancel_active_download
        ])
        .run(tauri::generate_context!())
        .expect("failed to run tauri application");
}























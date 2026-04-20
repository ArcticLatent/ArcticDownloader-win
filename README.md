# Arctic ComfyUI Helper (Private Technical README)

This repository (`ArcticDownloader-win`) is the private/source repo for the Windows app.

Public release repo: `https://github.com/ArcticLatent/Arctic-Helper`

`README.public.md` is the public-facing README template for the release repo.
This file (`README.md`) is the internal technical reference.

## Product Scope

Arctic ComfyUI Helper is a Windows-first Rust + Tauri app that includes:
- Tier-aware model downloader for ComfyUI (models + dependencies + LoRAs)
- In-app LoRA metadata/preview flow (including Civitai token support)
- ComfyUI installer/manager module (uv-managed Python + `.venv`)
- Optional add-ons and custom node management (install/remove/toggle)
- System tray control for ComfyUI start/stop even when main window is hidden
- Self-update support through GitHub release `update.json`

Flatpak/Linux-specific UI/admin bits are intentionally not part of this app.

## Architecture

- Core crate: `Cargo.toml` + `src/`
  - Shared services: catalog/config/download/updater/app context
- Desktop shell: `src-tauri/`
  - Tauri commands and Windows integration in `src-tauri/src/main.rs`
  - Frontend files in `src-tauri/dist/` (`index.html`, `main.js`, `style.css`)

Key identifiers and branding:
- App ID: `io.github.ArcticHelper`
- Product name: `Arctic ComfyUI Helper`
- Publisher: `Arctic Latent`
- Binary name: `Arctic-ComfyUI-Helper.exe`

## Data and Paths

Config/cache use `ProjectDirs("io.github", "ArcticHelper", "ArcticHelper")`.

Important runtime locations:
- Settings/config: `%LOCALAPPDATA%\io.github\ArcticHelper\config\settings.json`
- Cache root: `%LOCALAPPDATA%\io.github\ArcticHelper\cache\`
- ComfyUI shared runtime cache:
  `%LOCALAPPDATA%\io.github\ArcticHelper\cache\comfyui-runtime\`
  - contains shared `.tools` and `.python` for installer pipeline

ComfyUI install mode behavior:
- Install New: select base folder -> app creates `ComfyUI`, `ComfyUI-01`, `ComfyUI-02`, ...
- Manage Existing: select base with existing install(s), detect and manage installation state

## Catalog and Downloading

Catalog source behavior:
- Supabase Postgres is the catalog source of truth.
- Runtime configuration:
  - `ARCTIC_SUPABASE_URL`
  - `ARCTIC_SUPABASE_ANON_KEY` or `ARCTIC_SUPABASE_PUBLISHABLE_KEY`
- The app reads `public.catalog_documents` where `catalog_key = 'main'` and uses the `catalog` JSON document.
- The last successful catalog is cached locally for offline startup.

Model/LoRA downloader:
- Resolves target ComfyUI subfolders automatically
- Shows active/completed transfers and per-item progress
- Supports cancellation
- LoRA preview metadata supports creator/trigger/description handling
- Optional Hugging Face Xet fast-path via app toggle in Models:
  enable `HF Xet (Experimental)` and ensure `uvx hf` or `hf` CLI backend is available.

## ComfyUI Installer Module

Primary model:
- PowerShell/orchestrated from inside app (no external installer UI required)
- uv-managed Python (`3.12.10`) and per-install `.venv`
- Torch profile auto-recommendation by GPU + manual override dropdown

Current torch profiles:
- `torch271_cu128`
- `torch280_cu128`
- `torch291_cu130`

Add-ons (checkbox-managed):
- SageAttention
- SageAttention3 (RTX 50 only)
- FlashAttention
- InsightFace
- Nunchaku
- Trellis2 (requires minimum Torch 2.8.0 + cu128)
- Pinned Memory (default ON)

Attention backend rules:
- Exactly one of SageAttention / SageAttention3 / FlashAttention / Nunchaku at a time
- Toggle flow supports removal/install transitions and confirmation prompts
- Existing install mode applies backend changes by uninstall/install and keeps state in sync

Custom nodes (checkbox-managed):
- comfyui-manager
- ComfyUI-Easy-Use
- rgthree-comfy
- ComfyUI-GGUF
- comfyui-kjnodes

`comfyui_controlnet_aux` was intentionally removed from selectable custom nodes.

## ComfyUI Runtime Control

App supports starting/stopping ComfyUI directly.

System tray:
- Shows app + ComfyUI status
- Right-click actions include Start/Stop/Show/Quit
- Tray remains available while window is hidden

Desktop shortcuts:
- Start shortcut creation for installed ComfyUI instances
- Naming supports multiple installs (`Start ComfyUI`, `Start ComfyUI 01`, ...)

## Update Mechanism

Updater defaults:
- Manifest URL:
  `https://github.com/ArcticLatent/Arctic-Helper/releases/latest/download/update.json`
- Fallback standalone name: `Arctic-ComfyUI-Helper.exe`

Manifest schema:
```json
{
  "version": "0.1.0",
  "download_url": "https://github.com/ArcticLatent/Arctic-Helper/releases/download/v0.1.0/Arctic-ComfyUI-Helper.exe",
  "sha256": "<sha256>",
  "notes": "Release notes"
}
```

Notes:
- `Check Updates` will error until release repo has a valid `update.json` asset.
- Startup auto-update can be toggled via existing env flags.

## Icons and Branding Assets

- Primary Windows icon is sourced from `assets/favicon.ico`
- Tauri bundle icon points to `src-tauri/icons/favicon.ico`
- Same icon is used for app/tray/shortcut flows where supported

If Windows still shows old icon after changing `.ico`, clear icon cache and rebuild.

## Development Commands

From repository root:

```powershell
# dev run
cargo tauri dev

# sanity check
cargo check --manifest-path .\src-tauri\Cargo.toml

# production binary (no installer)
cargo tauri build --no-bundle
```

## Memory Leak Check

Use the built-in memory trend script from repository root:

```powershell
# Launch app and sample for 30 minutes, then stop it
powershell -ExecutionPolicy Bypass -File .\scripts\memory-leak-check.ps1 -DurationSeconds 1800 -StopProcessOnExit

# Attach to an already running process (replace PID)
powershell -ExecutionPolicy Bypass -File .\scripts\memory-leak-check.ps1 -TargetPid 12345 -DurationSeconds 1200
```

Outputs are written to `dist/`:
- `<prefix>-<timestamp>.csv` with time-series samples
- `<prefix>-<timestamp>-summary.txt` with growth slopes and a leak-risk assessment

Release binary output:
- `src-tauri\target\release\Arctic-ComfyUI-Helper.exe`

## Automated Release Flow

Use one command:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\release.ps1
```

It will:
1. Prompt for version
2. Prompt for release notes (end with `END` line)
3. Bump versions in:
   - `Cargo.toml`
   - `src-tauri/Cargo.toml`
   - `src-tauri/tauri.conf.json`
4. Clean + build (`cargo clean`, `cargo tauri build --no-bundle`)
5. Generate artifacts in `dist/`:
   - `Arctic-ComfyUI-Helper.exe`
   - `Arctic-ComfyUI-Helper.exe.sha256`
   - `update.json`
   - release notes markdown
6. Create/update GitHub release on `ArcticLatent/Arctic-Helper`

Prerequisite:
- GitHub CLI authenticated: `gh auth login`

## Repo Notes

- This is the internal repo and can include technical/implementation notes.
- `README.public.md` is maintained separately for public release consumption.

const invoke = window.__TAURI__?.core?.invoke;
const listen = window.__TAURI__?.event?.listen || window.__TAURI__?.core?.listen;
const DOT_SEP = " \u2022 ";
const PATH_SEP = "/";
const state = {
  catalog: null,
  settings: null,
  activeTab: "comfyui",
  transfers: new Map(),
  completed: [],
  completedSeq: 0,
  loraMetaRequestSeq: 0,
  currentLoraMetaId: null,
  loraMetaCache: new Map(),
  busyDownloads: 0,
  activeDownloadKind: null,
  comfyInstallBusy: false,
  comfySage3Eligible: false,
  comfyPreflightOk: null,
  comfyResumeState: null,
  comfyRuntimeRunning: false,
  comfyRuntimeStarting: false,
  comfyRuntimeTarget: "",
  comfyAttentionBusy: false,
  comfyComponentBusy: false,
  comfyMode: "install",
  updateAvailable: false,
  updateVersion: null,
  appVersion: "",
  updateInstalling: false,
  updateChecking: false,
  updateFlashLabel: "",
  updateFlashTimer: null,
  selectedComfyVersion: null,
  titleSystemText: "Loading system info...",
  comfyUpdateAvailable: false,
  comfyUpdateChecked: false,
  comfyUpdateBusy: false,
  comfyLatestVersion: null,
  comfyLastUpdateDetailLogKey: "",
  comfyTorchProfileLocked: false,
  comfyAddonLoadSeq: 0,
  comfyDetectedGpuVendor: "",
  comfyTorchRecommendedBase: "Recommended 'Torch 2.8.0 + cu128' for your GPU",
  sharedModelsRootDefault: "",
  sharedModelsUseDefault: false,
  selectedModelVariants: new Map(),
  comfyRuntimeLogs: [],
};

let progressSmoothTimer = null;
const comfyUpdateStatusCache = new Map();
const comfyUpdateStatusInflight = new Map();
const COMFY_UPDATE_STATUS_CACHE_MS = 4000;

const ramOptions = [
  { id: "tier_a", label: "Tier A (64 GB+)" },
  { id: "tier_b", label: "Tier B (32-63 GB)" },
  { id: "tier_c", label: "Tier C (<32 GB)" },
];

const vramOptions = [
  { id: "tier_s", label: "Tier S (32 GB+)" },
  { id: "tier_a", label: "Tier A (16-31 GB)" },
  { id: "tier_b", label: "Tier B (12-15 GB)" },
  { id: "tier_c", label: "Tier C (<12 GB)" },
];

const comfyTorchProfiles = [
  { value: "torch271_cu128", label: "Torch 2.7.1 + cu128" },
  { value: "torch280_cu128", label: "Torch 2.8.0 + cu128" },
  { value: "torch291_rocm72", label: "Torch 2.9.1 + ROCm SDK 7.2" },
  { value: "torch291_cu130", label: "Torch 2.9.1 + cu130" },
  { value: "torchxpu_nightly", label: "PyTorch XPU Nightly" },
];

const el = {
  version: document.getElementById("version"),
  updateStatus: document.getElementById("update-status"),
  statusLog: document.getElementById("status-log"),
  clearStatusLog: document.getElementById("clear-status-log"),
  progressLine: document.getElementById("download-progress"),
  overallProgress: document.getElementById("overall-progress"),
  overallProgressFill: document.getElementById("overall-progress-fill"),
  overallProgressMeta: document.getElementById("overall-progress-meta"),
  transferList: document.getElementById("transfer-list"),
  completedList: document.getElementById("completed-list"),
  checkUpdates: document.getElementById("check-updates"),
  appVersionTag: document.getElementById("app-version-tag"),

  tabComfyui: document.getElementById("tab-comfyui"),
  tabModels: document.getElementById("tab-models"),
  tabLoras: document.getElementById("tab-loras"),
  tabWorkflows: document.getElementById("tab-workflows"),
  contentComfyui: document.getElementById("tab-content-comfyui"),
  contentModels: document.getElementById("tab-content-models"),
  contentLoras: document.getElementById("tab-content-loras"),
  contentWorkflows: document.getElementById("tab-content-workflows"),
  downloadsStatusPanel: document.getElementById("downloads-status-panel"),

  comfyTorchProfile: document.getElementById("comfy-torch-profile"),
  comfyTorchRecommended: document.getElementById("comfy-torch-recommended"),
  comfyMode: document.getElementById("comfy-mode"),
  comfyModeHelp: document.getElementById("comfy-mode-help"),
  comfyExistingInstall: document.getElementById("comfy-existing-install"),
  updateSelectedInstall: document.getElementById("update-selected-install"),
  useExistingInstall: document.getElementById("use-existing-install"),
  comfyInstallRoot: document.getElementById("comfy-install-root"),
  chooseInstallRoot: document.getElementById("choose-install-root"),
  saveInstallRoot: document.getElementById("save-install-root"),
  comfyExtraModelRow: document.getElementById("comfy-extra-model-row"),
  comfyExtraModelRoot: document.getElementById("comfy-extra-model-root"),
  chooseExtraModelRoot: document.getElementById("choose-extra-model-root"),
  comfyExtraModelDefault: document.getElementById("comfy-extra-model-default"),
  clearExtraModelRoot: document.getElementById("clear-extra-model-root"),
  comfyResumeBanner: document.getElementById("comfy-resume-banner"),
  comfyResumeText: document.getElementById("comfy-resume-text"),
  comfyResumeBtn: document.getElementById("comfy-resume-btn"),
  comfyFreshBtn: document.getElementById("comfy-fresh-btn"),
  installComfyui: document.getElementById("install-comfyui"),
  comfyInstallSpinner: document.getElementById("comfy-install-spinner"),
  comfyQuickActions: document.getElementById("comfy-quick-actions"),
  comfyLastInstallPath: document.getElementById("comfy-last-install-path"),
  comfyOpenInstallFolder: document.getElementById("comfy-open-install-folder"),
  comfyStartInstalled: document.getElementById("comfy-start-installed"),
  comfyInstallLog: document.getElementById("comfy-install-log"),
  comfyClearInstallLog: document.getElementById("comfy-clear-install-log"),
  comfyRuntimeLog: document.getElementById("comfy-runtime-log"),
  comfyClearRuntimeLog: document.getElementById("comfy-clear-runtime-log"),
  comfyRuntimeLogFilter: document.getElementById("comfy-runtime-log-filter"),
  runPreflight: document.getElementById("run-preflight"),
  preflightSummary: document.getElementById("preflight-summary"),
  preflightList: document.getElementById("preflight-list"),
  addonSageAttention: document.getElementById("addon-sageattention"),
  addonSageAttention3: document.getElementById("addon-sageattention3"),
  addonFlashAttention: document.getElementById("addon-flashattention"),
  addonInsightFace: document.getElementById("addon-insightface"),
  addonNunchaku: document.getElementById("addon-nunchaku"),
  addonTrellis2: document.getElementById("addon-trellis2"),
  addonPinnedMemory: document.getElementById("addon-pinned-memory"),
  flagSageAttention: document.getElementById("flag-sageattention"),
  flagFlashAttention: document.getElementById("flag-flashattention"),
  launchListen: document.getElementById("launch-listen"),
  flagLowvram: document.getElementById("flag-lowvram"),
  flagBf16Unet: document.getElementById("flag-bf16-unet"),
  flagAsyncOffload: document.getElementById("flag-async-offload"),
  flagDisableSmartMemory: document.getElementById("flag-disable-smart-memory"),
  comfyCustomLaunchArgs: document.getElementById("comfy-custom-launch-args"),
  saveComfyCustomLaunchArgs: document.getElementById("save-comfy-custom-launch-args"),
  clearComfyCustomLaunchArgs: document.getElementById("clear-comfy-custom-launch-args"),
  comfyShowRuntimeLogs: document.getElementById("comfy-show-runtime-logs"),
  nodeComfyuiManager: document.getElementById("node-comfyui-manager"),
  nodeComfyuiEasyUse: document.getElementById("node-comfyui-easy-use"),
  nodeRgthreeComfy: document.getElementById("node-rgthree-comfy"),
  nodeComfyuiGguf: document.getElementById("node-comfyui-gguf"),
  nodeComfyuiKjnodes: document.getElementById("node-comfyui-kjnodes"),
  nodeComfyuiCrystools: document.getElementById("node-comfyui-crystools"),

  comfyRoot: document.getElementById("comfy-root"),
  chooseRoot: document.getElementById("choose-root"),
  saveRoot: document.getElementById("save-root"),
  comfyRootLora: document.getElementById("comfy-root-lora"),
  chooseRootLora: document.getElementById("choose-root-lora"),
  saveRootLora: document.getElementById("save-root-lora"),
  comfyRootWorkflow: document.getElementById("comfy-root-workflow"),
  chooseRootWorkflow: document.getElementById("choose-root-workflow"),
  saveRootWorkflow: document.getElementById("save-root-workflow"),

  modelFamily: document.getElementById("model-family"),
  vramTier: document.getElementById("vram-tier"),
  ramTier: document.getElementById("ram-tier"),
  modelSelectionList: document.getElementById("model-selection-list"),
  modelSelectionSummary: document.getElementById("model-selection-summary"),
  selectedModelQueue: document.getElementById("selected-model-queue"),
  effectiveDownloadDestination: document.getElementById("effective-download-destination"),
  modelSearch: document.getElementById("model-search"),
  selectVisibleModels: document.getElementById("select-visible-models"),
  clearModelSelection: document.getElementById("clear-model-selection"),
  downloadModel: document.getElementById("download-model"),
  enableHfXet: document.getElementById("enable-hf-xet"),

  loraFamily: document.getElementById("lora-family"),
  loraId: document.getElementById("lora-id"),
  civitaiToken: document.getElementById("civitai-token"),
  saveToken: document.getElementById("save-token"),
  downloadLora: document.getElementById("download-lora"),
  workflowFamily: document.getElementById("workflow-family"),
  workflowId: document.getElementById("workflow-id"),
  downloadWorkflow: document.getElementById("download-workflow"),

  metaCreator: document.getElementById("meta-creator"),
  metaCreatorLink: document.getElementById("meta-creator-link"),
  metaStrength: document.getElementById("meta-strength"),
  metaTriggers: document.getElementById("meta-triggers"),
  metaDescription: document.getElementById("meta-description"),

  previewImage: document.getElementById("preview-image"),
  previewVideo: document.getElementById("preview-video"),
  previewCaption: document.getElementById("preview-caption"),
  workflowPreviewImage: document.getElementById("workflow-preview-image"),
  workflowPreviewCaption: document.getElementById("workflow-preview-caption"),
  workflowYoutubeLink: document.getElementById("workflow-youtube-link"),
  confirmOverlay: document.getElementById("confirm-overlay"),
  confirmMessage: document.getElementById("confirm-message"),
  confirmYes: document.getElementById("confirm-yes"),
  confirmNo: document.getElementById("confirm-no"),
  startupOverlay: document.getElementById("startup-overlay"),
  startupStatus: document.getElementById("startup-status"),
};

function logLine(text) {
  const stamp = new Date()
    .toLocaleTimeString([], { hour: "numeric", minute: "2-digit", hour12: true })
    .replace(/\s+/g, " ")
    .toUpperCase();
  el.statusLog.textContent = `[${stamp}] ${text}\n` + el.statusLog.textContent;
}

function logComfyLine(text) {
  const stamp = new Date()
    .toLocaleTimeString([], { hour: "numeric", minute: "2-digit", hour12: true })
    .replace(/\s+/g, " ")
    .toUpperCase();
  if (!el.comfyInstallLog) return;
  el.comfyInstallLog.textContent = `[${stamp}] ${text}\n` + el.comfyInstallLog.textContent;
}

function escapeHtml(text) {
  return String(text || "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

function ansiToHtml(text) {
  const input = String(text || "");
  let html = "";
  let classes = [];

  const flush = (chunk) => {
    if (!chunk) return;
    const escaped = escapeHtml(chunk);
    html += classes.length
      ? `<span class="${classes.join(" ")}">${escaped}</span>`
      : escaped;
  };

  let index = 0;
  while (index < input.length) {
    const escIndex = input.indexOf("\u001b[", index);
    if (escIndex < 0) {
      flush(input.slice(index));
      break;
    }
    flush(input.slice(index, escIndex));
    const match = /^\u001b\[([0-9;]*)m/.exec(input.slice(escIndex));
    if (!match) {
      flush(input.slice(escIndex, escIndex + 1));
      index = escIndex + 1;
      continue;
    }
    const codes = (match[1] || "0")
      .split(";")
      .map((part) => Number(part || 0))
      .filter((code) => Number.isFinite(code));
    if (codes.length === 0 || codes.includes(0)) {
      classes = [];
    }
    if (codes.includes(1)) {
      classes = classes.filter((name) => name !== "ansi-bold");
      classes.push("ansi-bold");
    }
    codes.forEach((code) => {
      if ((code >= 30 && code <= 37) || (code >= 90 && code <= 97)) {
        classes = classes.filter((name) => !/^ansi-fg-/.test(name));
        classes.push(`ansi-fg-${code}`);
      }
      if (code === 39) {
        classes = classes.filter((name) => !/^ansi-fg-/.test(name));
      }
      if (code === 22) {
        classes = classes.filter((name) => name !== "ansi-bold");
      }
    });
    index = escIndex + match[0].length;
  }
  return html;
}

function detectRuntimeLogLevel(text) {
  const value = String(text || "").toLowerCase();
  if (/traceback|fatal|exception|error|failed|cannot|could not|invalid|denied/.test(value)) {
    return "error";
  }
  if (/warn|warning|deprecated|retry|fallback|slow|stall/.test(value)) {
    return "warn";
  }
  if (/started|ready|listening|loaded|completed|success|using /.test(value)) {
    return "success";
  }
  return "info";
}

function runtimeLogMatchesFilter(entry) {
  const filter = String(el.comfyRuntimeLogFilter?.value || "all");
  if (filter === "stderr") return entry.stream === "stderr";
  if (filter === "stdout") return entry.stream === "stdout";
  if (filter === "important") return entry.level === "warn" || entry.level === "error";
  return true;
}

function renderComfyRuntimeLogs() {
  if (!el.comfyRuntimeLog) return;
  el.comfyRuntimeLog.innerHTML = "";
  const visible = state.comfyRuntimeLogs.filter(runtimeLogMatchesFilter);
  if (!visible.length) {
    el.comfyRuntimeLog.textContent = "Ready";
    return;
  }
  visible.forEach((entry) => {
    const line = document.createElement("div");
    line.className = `ansi-log-line stream-${entry.stream} level-${entry.level}`;
    line.innerHTML = `<span class="ansi-log-meta">[${escapeHtml(entry.stamp)}]</span><span class="ansi-log-dot" aria-hidden="true"></span>${ansiToHtml(entry.text)}`;
    el.comfyRuntimeLog.appendChild(line);
  });
}

function logComfyRuntimeLine(text, stream = "stdout") {
  const stamp = new Date()
    .toLocaleTimeString([], { hour: "numeric", minute: "2-digit", hour12: true })
    .replace(/\s+/g, " ")
    .toUpperCase();
  state.comfyRuntimeLogs.unshift({
    text: String(text || ""),
    stream: String(stream || "stdout").toLowerCase() === "stderr" ? "stderr" : "stdout",
    stamp,
    level: detectRuntimeLogLevel(text),
  });
  if (state.comfyRuntimeLogs.length > 500) {
    state.comfyRuntimeLogs.length = 500;
  }
  renderComfyRuntimeLogs();
}

function setStartupStatus(text) {
  if (!el.startupStatus) return;
  el.startupStatus.textContent = String(text || "Preparing workspace...");
}

function hideStartupOverlay() {
  const overlay = el.startupOverlay;
  if (!overlay || overlay.classList.contains("hidden")) return;
  overlay.classList.add("is-hiding");
  window.setTimeout(() => {
    overlay.classList.add("hidden");
    overlay.classList.remove("is-hiding");
    overlay.setAttribute("aria-busy", "false");
  }, 240);
}

function notifySystem(title, body) {
  const tauriNotify = window.__TAURI__?.notification;
  if (tauriNotify?.sendNotification) {
    try {
      tauriNotify.sendNotification({ title, body });
      return;
    } catch (_) {}
  }
  if (!("Notification" in window)) return;
  const send = () => {
    try {
      new Notification(title, { body });
    } catch (_) {}
  };
  if (Notification.permission === "granted") {
    send();
    return;
  }
  if (Notification.permission !== "denied") {
    Notification.requestPermission().then((perm) => {
      if (perm === "granted") send();
    }).catch(() => {});
  }
}

function setToggleBusy(box, busy) {
  if (!box) return;
  box.disabled = Boolean(busy);
  const label = box.closest("label");
  if (!label) return;
  label.classList.toggle("busy", Boolean(busy));
}

function showConfirmDialog(message) {
  return new Promise((resolve) => {
    const overlay = el.confirmOverlay;
    const messageEl = el.confirmMessage;
    const yesBtn = el.confirmYes;
    const noBtn = el.confirmNo;
    if (!overlay || !messageEl || !yesBtn || !noBtn) {
      resolve(window.confirm(message));
      return;
    }

    let settled = false;
    const close = (value) => {
      if (settled) return;
      settled = true;
      overlay.classList.add("hidden");
      overlay.setAttribute("aria-hidden", "true");
      yesBtn.removeEventListener("click", onYes);
      noBtn.removeEventListener("click", onNo);
      overlay.removeEventListener("click", onOverlay);
      window.removeEventListener("keydown", onKeyDown);
      resolve(value);
    };
    const onYes = () => close(true);
    const onNo = () => close(false);
    const onOverlay = (event) => {
      if (event.target === overlay) close(false);
    };
    const onKeyDown = (event) => {
      if (event.key === "Escape") close(false);
    };

    messageEl.textContent = String(message || "Are you sure?");
    yesBtn.addEventListener("click", onYes);
    noBtn.addEventListener("click", onNo);
    overlay.addEventListener("click", onOverlay);
    window.addEventListener("keydown", onKeyDown);
    window.requestAnimationFrame(() => {
      overlay.classList.remove("hidden");
      overlay.setAttribute("aria-hidden", "false");
      window.requestAnimationFrame(() => yesBtn.focus());
    });
  });
}

function waitForNextPaint() {
  return new Promise((resolve) => {
    window.requestAnimationFrame(() => {
      window.requestAnimationFrame(resolve);
    });
  });
}

function sleep(ms) {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

function setProgress(text) {
  el.progressLine.textContent = text || "Idle";
}

function updateComfyInstallButton() {
  if (!el.installComfyui) return;
  el.installComfyui.textContent = state.comfyInstallBusy ? "Cancel Install" : "Install ComfyUI";
  el.comfyInstallSpinner?.classList.toggle("hidden", !state.comfyInstallBusy);
}

function renderTitleMeta() {
  const base = state.titleSystemText || "Loading system info...";
  const comfy = String(state.selectedComfyVersion || "").trim();
  if (!comfy) {
    el.version.textContent = base;
    return;
  }
  const label = comfy.toLowerCase().startsWith("v") ? comfy : `v${comfy}`;
  el.version.textContent = `${base}${DOT_SEP}ComfyUI ${label}`;
  const latest = String(state.comfyLatestVersion || "").trim();
  if (state.comfyUpdateAvailable) {
    const badge = document.createElement("span");
    badge.className = "latest-version-badge";
    if (latest) {
      const latestLabel = latest.toLowerCase().startsWith("v") ? latest : `v${latest}`;
      badge.textContent = `${DOT_SEP}(latest ${latestLabel})`;
    } else {
      badge.textContent = `${DOT_SEP}(update available)`;
    }
    el.version.appendChild(badge);
  }
}

function renderAppVersionTag() {
  if (!el.appVersionTag) return;
  const normalizeVersion = (value) => String(value || "").trim().replace(/^v/i, "");
  const current = normalizeVersion(state.appVersion || "");
  const latest = normalizeVersion(state.updateVersion || "");
  if (state.updateInstalling) {
    el.appVersionTag.textContent = "Updating...";
    el.appVersionTag.classList.remove("update-available");
    return;
  }
  if (state.updateAvailable && state.updateVersion) {
    el.appVersionTag.textContent = latest;
    el.appVersionTag.classList.add("update-available");
    return;
  }
  el.appVersionTag.textContent = current || "...";
  el.appVersionTag.classList.remove("update-available");
}

function updateComfyUpdateButton() {
  const btn = el.updateSelectedInstall;
  if (!btn) return;
  const hasSelection = Boolean(String(el.comfyExistingInstall?.value || "").trim());
  btn.classList.toggle("hidden", !hasSelection);
  btn.classList.remove("update-available");
  if (!hasSelection) return;

  if (state.comfyUpdateBusy) {
    btn.textContent = "Updating...";
    btn.disabled = true;
    return;
  }
  if (!state.comfyUpdateChecked) {
    btn.textContent = "Checking...";
    btn.disabled = true;
    return;
  }
  if (state.comfyUpdateAvailable) {
    btn.textContent = "Update ComfyUI";
    btn.disabled = false;
    btn.classList.add("update-available");
    return;
  }
  btn.textContent = "No Update";
  btn.disabled = true;
}

function updateUpdateButton() {
  if (!el.checkUpdates) return;
  el.checkUpdates.classList.remove("update-available");
  if (state.updateFlashLabel) {
    el.checkUpdates.textContent = state.updateFlashLabel;
    el.checkUpdates.disabled = true;
    renderAppVersionTag();
    return;
  }
  if (state.updateChecking) {
    el.checkUpdates.textContent = "Checking...";
    el.checkUpdates.disabled = true;
    renderAppVersionTag();
    return;
  }
  if (state.updateInstalling) {
    el.checkUpdates.textContent = "Updating...";
    el.checkUpdates.disabled = true;
    renderAppVersionTag();
    return;
  }
  el.checkUpdates.disabled = false;
  el.checkUpdates.textContent = state.updateAvailable ? "Update" : "Check Updates";
  if (state.updateAvailable) {
    el.checkUpdates.classList.add("update-available");
  }
  renderAppVersionTag();
}

function flashUpdateButton(label, durationMs = 2600) {
  if (state.updateFlashTimer) {
    window.clearTimeout(state.updateFlashTimer);
    state.updateFlashTimer = null;
  }
  state.updateFlashLabel = String(label || "").trim();
  updateUpdateButton();
  state.updateFlashTimer = window.setTimeout(() => {
    state.updateFlashLabel = "";
    state.updateFlashTimer = null;
    updateUpdateButton();
  }, Math.max(600, Number(durationMs) || 2600));
}

function normalizeSlashes(value) {
  const raw = String(value || "").trim();
  if (!raw) return "";
  const normalized = raw.replace(/[\\/]+/g, PATH_SEP);
  const nestedHome = `${PATH_SEP}src-tauri${PATH_SEP}home${PATH_SEP}`;
  const idx = normalized.indexOf(nestedHome);
  if (idx >= 0) {
    return normalized.slice(idx + `${PATH_SEP}src-tauri`.length);
  }
  return normalized;
}

function setTorchRecommendedDetecting(detecting) {
  if (!el.comfyTorchRecommended) return;
  if (detecting) {
    el.comfyTorchRecommended.textContent = "Detecting torch/add-ons for selected install...";
  } else {
    el.comfyTorchRecommended.textContent = state.comfyTorchRecommendedBase;
  }
}

function comfyTorchProfileOptionsForDetectedGpu() {
  const vendor = String(state.comfyDetectedGpuVendor || "").toLowerCase();
  if (vendor === "amd") {
    return comfyTorchProfiles.map((item) => ({
      ...item,
      disabled: item.value !== "torch291_rocm72",
    }));
  }
  if (vendor === "intel") {
    return comfyTorchProfiles.map((item) => ({
      ...item,
      disabled: item.value !== "torchxpu_nightly",
    }));
  }
  if (vendor === "nvidia") {
    return comfyTorchProfiles.map((item) => ({
      ...item,
      disabled: item.value === "torch291_rocm72" || item.value === "torchxpu_nightly",
    }));
  }
  return comfyTorchProfiles.map((item) => ({ ...item, disabled: false }));
}

function applyComfyTorchProfileOptions(selectedValue = null) {
  if (!el.comfyTorchProfile) return;
  const options = comfyTorchProfileOptionsForDetectedGpu();
  const vendor = String(state.comfyDetectedGpuVendor || "").toLowerCase();
  const forcedValue = vendor === "amd"
    ? "torch291_rocm72"
    : (vendor === "intel" ? "torchxpu_nightly" : selectedValue);
  setOptions(el.comfyTorchProfile, options, forcedValue);
  if (vendor === "amd") {
    el.comfyTorchProfile.value = "torch291_rocm72";
  } else if (vendor === "intel") {
    el.comfyTorchProfile.value = "torchxpu_nightly";
  } else if (
    vendor === "nvidia"
    && (el.comfyTorchProfile.value === "torch291_rocm72" || el.comfyTorchProfile.value === "torchxpu_nightly")
  ) {
    el.comfyTorchProfile.value = selectedValue
      && selectedValue !== "torch291_rocm72"
      && selectedValue !== "torchxpu_nightly"
      ? selectedValue
      : "torch280_cu128";
  }
}

function parentDir(path) {
  const normalized = normalizeSlashes(path);
  const idx = normalized.lastIndexOf(PATH_SEP);
  if (idx <= 0) return normalized;
  return normalized.slice(0, idx);
}

function comfyInstallOrder(name) {
  const lower = String(name || "").trim().toLowerCase();
  if (lower === "comfyui") return 0;
  const match = /^comfyui-(\d+)$/.exec(lower);
  if (!match) return -1;
  const parsed = Number.parseInt(match[1], 10);
  return Number.isFinite(parsed) ? parsed : -1;
}

function newestComfyInstall(installs) {
  if (!Array.isArray(installs) || installs.length === 0) return null;
  let best = installs[0];
  let bestOrder = comfyInstallOrder(best?.name);
  for (const item of installs.slice(1)) {
    const order = comfyInstallOrder(item?.name);
    if (order > bestOrder) {
      best = item;
      bestOrder = order;
    }
  }
  return best;
}

function comfyInstallNameFromRoot(rootPath) {
  const normalized = normalizeSlashes(String(rootPath || "").trim());
  if (!normalized) return "ComfyUI";
  const parts = normalized.split(PATH_SEP).filter(Boolean);
  return parts.length ? parts[parts.length - 1] : "ComfyUI";
}

function setComfyQuickActions(installDir, comfyRoot) {
  const install = String(installDir || "").trim();
  const root = String(comfyRoot || "").trim();
  if (!install && !root) {
    el.comfyQuickActions?.classList.add("hidden");
    return;
  }
  const finalInstall = install || parentDir(root);
  const finalRoot = root || finalInstall;
  el.comfyQuickActions?.classList.remove("hidden");
  if (el.comfyLastInstallPath) {
    el.comfyLastInstallPath.textContent = `Last install: ${finalInstall}`;
  }
  if (el.comfyOpenInstallFolder) {
    el.comfyOpenInstallFolder.dataset.path = finalRoot;
  }
  if (el.comfyStartInstalled) {
    el.comfyStartInstalled.dataset.path = finalRoot;
  }
}

function buildComfyInstallRequest() {
  const extraModelRoot = String(el.comfyExtraModelRoot?.value || "").trim();
  return {
    installRoot: String(el.comfyInstallRoot.value || "").trim(),
    extraModelRoot: extraModelRoot || null,
    extraModelUseDefault: Boolean(el.comfyExtraModelDefault?.checked && extraModelRoot),
    torchProfile: el.comfyTorchProfile.value || null,
    includeSageAttention: Boolean(el.addonSageAttention.checked),
    includeSageAttention3: Boolean(el.addonSageAttention3.checked),
    includeFlashAttention: Boolean(el.addonFlashAttention.checked),
    includeInsightFace: Boolean(el.addonInsightFace.checked),
    includeNunchaku: Boolean(el.addonNunchaku.checked),
    includeTrellis2: Boolean(el.addonTrellis2?.checked),
    includePinnedMemory: Boolean(el.addonPinnedMemory?.checked ?? true),
    nodeComfyuiManager: Boolean(el.nodeComfyuiManager.checked),
    nodeComfyuiEasyUse: Boolean(el.nodeComfyuiEasyUse.checked),
    nodeRgthreeComfy: Boolean(el.nodeRgthreeComfy.checked),
    nodeComfyuiGguf: Boolean(el.nodeComfyuiGguf.checked),
    nodeComfyuiKjnodes: Boolean(el.nodeComfyuiKjnodes.checked),
    nodeComfyuiCrystools: Boolean(el.nodeComfyuiCrystools?.checked),
  };
}

function resetComfySelectionsToDefaults() {
  if (el.addonSageAttention) el.addonSageAttention.checked = false;
  if (el.addonSageAttention3) el.addonSageAttention3.checked = false;
  if (el.addonFlashAttention) el.addonFlashAttention.checked = false;
  if (el.addonNunchaku) el.addonNunchaku.checked = false;
  if (el.addonInsightFace) el.addonInsightFace.checked = false;
  if (el.addonTrellis2) el.addonTrellis2.checked = false;
  if (el.addonPinnedMemory) el.addonPinnedMemory.checked = true;
  if (el.flagSageAttention) el.flagSageAttention.checked = false;
  if (el.flagFlashAttention) el.flagFlashAttention.checked = false;
  if (el.launchListen) el.launchListen.checked = false;
  if (el.flagLowvram) el.flagLowvram.checked = false;
  if (el.flagBf16Unet) el.flagBf16Unet.checked = false;
  if (el.flagAsyncOffload) el.flagAsyncOffload.checked = false;
  if (el.flagDisableSmartMemory) el.flagDisableSmartMemory.checked = false;

  if (el.nodeComfyuiManager) el.nodeComfyuiManager.checked = false;
  if (el.nodeComfyuiEasyUse) el.nodeComfyuiEasyUse.checked = false;
  if (el.nodeRgthreeComfy) el.nodeRgthreeComfy.checked = false;
  if (el.nodeComfyuiGguf) el.nodeComfyuiGguf.checked = false;
  if (el.nodeComfyuiKjnodes) el.nodeComfyuiKjnodes.checked = false;
  if (el.nodeComfyuiCrystools) el.nodeComfyuiCrystools.checked = false;
  applyComfyAddonRules();
}

async function loadInstalledAddonState(comfyuiRoot) {
  const root = String(comfyuiRoot || el.comfyRoot.value || "").trim();
  if (!root) return;
  const loadSeq = ++state.comfyAddonLoadSeq;
  try {
    const installed = await invoke("get_comfyui_addon_state", { comfyuiRoot: root });
    if (loadSeq !== state.comfyAddonLoadSeq) return;
    const selectedRoot = normalizeSlashes(String(
      el.comfyExistingInstall?.value || el.comfyRoot.value || "",
    ).trim());
    if (selectedRoot && normalizeSlashes(root) !== selectedRoot) return;
    const installedTorchProfile = String(installed?.torch_profile || "").trim();
    if (installedTorchProfile && comfyTorchProfiles.some((x) => x.value === installedTorchProfile)) {
      el.comfyTorchProfile.value = installedTorchProfile;
      state.comfyTorchProfileLocked = true;
    }
    if (el.addonSageAttention) el.addonSageAttention.checked = Boolean(installed?.sage_attention);
    if (el.addonSageAttention3) el.addonSageAttention3.checked = Boolean(installed?.sage_attention3);
    if (el.addonFlashAttention) el.addonFlashAttention.checked = Boolean(installed?.flash_attention);
    if (el.addonNunchaku) el.addonNunchaku.checked = Boolean(installed?.nunchaku);
    if (el.addonInsightFace) el.addonInsightFace.checked = Boolean(installed?.insight_face);
    if (el.addonTrellis2) el.addonTrellis2.checked = Boolean(installed?.trellis2);
    if (el.flagSageAttention) {
      el.flagSageAttention.checked = Boolean(
        installed?.launch_sage_attention || installed?.launch_sage_attention3,
      );
    }
    if (el.flagFlashAttention) el.flagFlashAttention.checked = Boolean(installed?.launch_flash_attention);
    if (el.launchListen) el.launchListen.checked = Boolean(installed?.listen_enabled);
    if (el.flagLowvram) el.flagLowvram.checked = Boolean(installed?.lowvram_enabled);
    if (el.flagBf16Unet) el.flagBf16Unet.checked = Boolean(installed?.bf16_unet_enabled);
    if (el.flagAsyncOffload) el.flagAsyncOffload.checked = Boolean(installed?.async_offload_enabled);
    if (el.flagDisableSmartMemory) {
      el.flagDisableSmartMemory.checked = Boolean(installed?.disable_smart_memory_enabled);
    }

    if (el.nodeComfyuiManager) el.nodeComfyuiManager.checked = Boolean(installed?.node_comfyui_manager);
    if (el.nodeComfyuiEasyUse) el.nodeComfyuiEasyUse.checked = Boolean(installed?.node_comfyui_easy_use);
    if (el.nodeRgthreeComfy) el.nodeRgthreeComfy.checked = Boolean(installed?.node_rgthree_comfy);
    if (el.nodeComfyuiGguf) el.nodeComfyuiGguf.checked = Boolean(installed?.node_comfyui_gguf);
    if (el.nodeComfyuiKjnodes) el.nodeComfyuiKjnodes.checked = Boolean(installed?.node_comfyui_kjnodes);
    if (el.nodeComfyuiCrystools) el.nodeComfyuiCrystools.checked = Boolean(installed?.node_comfyui_crystools);
    applyComfyAddonRules();
  } catch (_) {
    // Ignore when root is unset or not fully installed yet.
  }
}

function updateComfyRuntimeButton() {
  if (!el.comfyStartInstalled) return;
  const running = Boolean(state.comfyRuntimeRunning);
  const starting = Boolean(state.comfyRuntimeStarting);
  const busy = Boolean(
    state.comfyAttentionBusy
    || state.comfyComponentBusy,
  );
  const target = String(state.comfyRuntimeTarget || "").trim();
  if (starting) {
    el.comfyStartInstalled.textContent = target ? `Starting ${target}...` : "Starting ComfyUI...";
    el.comfyStartInstalled.disabled = true;
    el.comfyStartInstalled.classList.remove("stop-state");
    el.comfyStartInstalled.classList.add("starting-state");
    return;
  }
  if (busy) {
    el.comfyStartInstalled.textContent = running ? "Stop ComfyUI" : "Applying changes...";
    el.comfyStartInstalled.disabled = true;
    el.comfyStartInstalled.classList.toggle("stop-state", running);
    el.comfyStartInstalled.classList.remove("starting-state");
    return;
  }
  el.comfyStartInstalled.textContent = running ? "Stop ComfyUI" : "Start ComfyUI";
  el.comfyStartInstalled.disabled = false;
  el.comfyStartInstalled.classList.toggle("stop-state", running);
  el.comfyStartInstalled.classList.remove("starting-state");
}

function attentionAddonEntries() {
  return [
    { box: el.addonSageAttention, backend: "sage", label: "SageAttention" },
    { box: el.addonSageAttention3, backend: "sage3", label: "SageAttention3" },
    { box: el.addonFlashAttention, backend: "flash", label: "FlashAttention" },
    { box: el.addonNunchaku, backend: "nunchaku", label: "Nunchaku" },
  ].filter((entry) => Boolean(entry.box));
}

function attentionEntryForBox(box) {
  return attentionAddonEntries().find((entry) => entry.box === box) || null;
}

function checkedAttentionEntries(exceptBox = null) {
  return attentionAddonEntries().filter((entry) => entry.box !== exceptBox && entry.box.checked);
}

function enforceExclusiveAttentionSelectionLocal(changedBox) {
  if (!changedBox?.checked) return;
  checkedAttentionEntries(changedBox).forEach((entry) => {
    entry.box.checked = false;
  });
}

function attentionFlagEntries() {
  return [
    {
      box: el.flagSageAttention,
      backend: () => (el.addonSageAttention3?.checked ? "sage3" : "sage"),
      label: "SageAttention",
    },
    { box: el.flagFlashAttention, backend: "flash", label: "FlashAttention" },
  ].filter((entry) => Boolean(entry.box));
}

function attentionFlagEntryForBox(box) {
  return attentionFlagEntries().find((entry) => entry.box === box) || null;
}

function checkedAttentionFlagEntries(exceptBox = null) {
  return attentionFlagEntries().filter((entry) => entry.box !== exceptBox && entry.box.checked);
}

function enforceExclusiveAttentionFlagSelectionLocal(changedBox) {
  if (!changedBox?.checked) return;
  checkedAttentionFlagEntries(changedBox).forEach((entry) => {
    entry.box.checked = false;
  });
}

async function applyAttentionBackendFromToggle(changedBox) {
  if (!changedBox) return;
  if (state.comfyMode !== "manage") {
    enforceExclusiveAttentionSelectionLocal(changedBox);
    return;
  }
  if (state.comfyAttentionBusy) return;

  const root = String(el.comfyRoot.value || "").trim();
  if (!root) {
    logComfyLine("Set ComfyUI folder first.");
    changedBox.checked = !changedBox.checked;
    return;
  }

  const changed = attentionEntryForBox(changedBox);
  if (!changed) return;
  const others = checkedAttentionEntries(changedBox);
  let targetBackend = "none";
  let confirmMessage = "";

  if (changedBox.checked) {
    targetBackend = changed.backend;
    if (others.length > 0) {
      confirmMessage = `Are you sure you want to install '${changed.label}'?\nInstalling '${changed.label}' will automatically remove '${others[0].label}'.`;
    }
  } else {
    confirmMessage = `Are you sure you want to remove '${changed.label}'?`;
  }

  if (confirmMessage && !(await showConfirmDialog(confirmMessage))) {
    changedBox.checked = !changedBox.checked;
    return;
  }

  await waitForNextPaint();
  state.comfyAttentionBusy = true;
  updateComfyRuntimeButton();
  setToggleBusy(changedBox, true);
  try {
    const result = await invoke("apply_attention_backend_change", {
      request: {
        comfyuiRoot: root,
        targetBackend,
        torchProfile: el.comfyTorchProfile?.value || null,
      },
    });
    if (result) {
      logComfyLine(String(result));
    }
    await loadInstalledAddonState(root);
  } catch (err) {
    logComfyLine(`Attention backend change failed: ${err}`);
    await loadInstalledAddonState(root);
  } finally {
    state.comfyAttentionBusy = false;
    updateComfyRuntimeButton();
    setToggleBusy(changedBox, false);
  }
}

async function applyLaunchAttentionFlagFromToggle(changedBox) {
  if (!changedBox) return;
  enforceExclusiveAttentionFlagSelectionLocal(changedBox);
  if (state.comfyMode !== "manage") {
    return;
  }
  if (state.comfyAttentionBusy) return;

  const root = String(el.comfyRoot.value || "").trim();
  if (!root) {
    logComfyLine("Set ComfyUI folder first.");
    changedBox.checked = !changedBox.checked;
    return;
  }

  const changed = attentionFlagEntryForBox(changedBox);
  if (!changed) return;
  const targetBackend = changedBox.checked
    ? (typeof changed.backend === "function" ? changed.backend() : changed.backend)
    : "none";

  await waitForNextPaint();
  state.comfyAttentionBusy = true;
  updateComfyRuntimeButton();
  setToggleBusy(changedBox, true);
  try {
    const result = await invoke("set_comfyui_launch_attention_backend", {
      request: {
        comfyuiRoot: root,
        targetBackend,
      },
    });
    if (result) {
      logComfyLine(String(result));
    }
    await loadInstalledAddonState(root);
  } catch (err) {
    logComfyLine(`Launch flag change failed: ${err}`);
    await loadInstalledAddonState(root);
  } finally {
    state.comfyAttentionBusy = false;
    updateComfyRuntimeButton();
    setToggleBusy(changedBox, false);
  }
}

async function applyComponentToggleFromCheckbox(changedBox, component, label) {
  if (!changedBox || state.comfyComponentBusy) return;
  const launchSettingOnly = [
    "addon_pinned_memory",
    "pinned_memory",
    "launch_listen",
    "addon_launch_listen",
    "launch_lowvram",
    "addon_launch_lowvram",
    "launch_bf16_unet",
    "addon_launch_bf16_unet",
    "launch_async_offload",
    "addon_launch_async_offload",
    "launch_disable_smart_memory",
    "addon_launch_disable_smart_memory",
  ].includes(String(component || "").trim());
  if (state.comfyMode !== "manage" && !launchSettingOnly) {
    return;
  }
  const root = String(el.comfyRoot.value || "").trim();
  if (!root && !launchSettingOnly) {
    logComfyLine("Set ComfyUI folder first.");
    changedBox.checked = !changedBox.checked;
    return;
  }

  const enabling = Boolean(changedBox.checked);
  const action = (
    component === "launch_listen"
      || component === "addon_launch_listen"
      || component === "launch_lowvram"
      || component === "addon_launch_lowvram"
      || component === "launch_bf16_unet"
      || component === "addon_launch_bf16_unet"
      || component === "launch_async_offload"
      || component === "addon_launch_async_offload"
      || component === "launch_disable_smart_memory"
      || component === "addon_launch_disable_smart_memory"
      ? (enabling ? "enable" : "disable")
      : (enabling ? "install" : "remove")
  );
  const ok = await showConfirmDialog(`Are you sure you want to ${action} '${label}'?`);
  if (!ok) {
    changedBox.checked = !changedBox.checked;
    return;
  }

  await waitForNextPaint();
  state.comfyComponentBusy = true;
  updateComfyRuntimeButton();
  setToggleBusy(changedBox, true);
  try {
    const result = await invoke("apply_comfyui_component_toggle", {
      request: {
        comfyuiRoot: root || null,
        component,
        enabled: enabling,
        torchProfile: el.comfyTorchProfile?.value || null,
      },
    });
    if (result) {
      logComfyLine(String(result));
    }
  } catch (err) {
    logComfyLine(`Component change failed: ${err}`);
  } finally {
    await loadInstalledAddonState(root);
    if (component === "addon_pinned_memory" && el.addonPinnedMemory) {
      try {
        const settings = await invoke("get_settings");
        el.addonPinnedMemory.checked = settings?.comfyui_pinned_memory_enabled !== false;
      } catch (_) {}
    }
    if ((component === "launch_listen" || component === "addon_launch_listen") && el.launchListen) {
      try {
        const settings = await invoke("get_settings");
        el.launchListen.checked = settings?.comfyui_listen_enabled === true;
      } catch (_) {}
    }
    if ((component === "launch_lowvram" || component === "addon_launch_lowvram") && el.flagLowvram) {
      try {
        const settings = await invoke("get_settings");
        el.flagLowvram.checked = settings?.comfyui_lowvram_enabled === true;
      } catch (_) {}
    }
    if ((component === "launch_bf16_unet" || component === "addon_launch_bf16_unet") && el.flagBf16Unet) {
      try {
        const settings = await invoke("get_settings");
        el.flagBf16Unet.checked = settings?.comfyui_bf16_unet_enabled === true;
      } catch (_) {}
    }
    if ((component === "launch_async_offload" || component === "addon_launch_async_offload") && el.flagAsyncOffload) {
      try {
        const settings = await invoke("get_settings");
        el.flagAsyncOffload.checked = settings?.comfyui_async_offload_enabled === true;
      } catch (_) {}
    }
    if ((component === "launch_disable_smart_memory" || component === "addon_launch_disable_smart_memory") && el.flagDisableSmartMemory) {
      try {
        const settings = await invoke("get_settings");
        el.flagDisableSmartMemory.checked = settings?.comfyui_disable_smart_memory_enabled === true;
      } catch (_) {}
    }
    state.comfyComponentBusy = false;
    updateComfyRuntimeButton();
    setToggleBusy(changedBox, false);
  }
}

let runtimeStatusPollTimer = null;
let runtimeStatusPollInFlight = false;

async function refreshComfyRuntimeStatus() {
  if (runtimeStatusPollInFlight || !invoke) return;

  // Poll less aggressively unless we are in a start transition.
  if (!state.comfyRuntimeStarting && document.visibilityState !== "visible") {
    return;
  }

  runtimeStatusPollInFlight = true;
  const wasStarting = Boolean(state.comfyRuntimeStarting);
  try {
    const result = await invoke("get_comfyui_runtime_status");
    state.comfyRuntimeRunning = Boolean(result?.running);
  } catch (_) {
    state.comfyRuntimeRunning = false;
  } finally {
    runtimeStatusPollInFlight = false;
  }

  // Keep "Starting..." visible until ComfyUI is actually running or explicit runtime events clear it.
  if (state.comfyRuntimeRunning) {
    state.comfyRuntimeStarting = false;
    state.comfyRuntimeTarget = "";
  } else if (!wasStarting) {
    state.comfyRuntimeStarting = false;
  }
  updateComfyRuntimeButton();
}

function scheduleRuntimeStatusPoll(delayMs = null) {
  const delay = delayMs ?? (state.comfyRuntimeStarting ? 1400 : 6500);
  if (runtimeStatusPollTimer) {
    window.clearTimeout(runtimeStatusPollTimer);
  }
  runtimeStatusPollTimer = window.setTimeout(async () => {
    await refreshComfyRuntimeStatus().catch(() => {});
    scheduleRuntimeStatusPoll();
  }, delay);
}

async function openComfyWhenReady(timeoutMs = 45000) {
  const startedAt = Date.now();
  while ((Date.now() - startedAt) < timeoutMs) {
    try {
      const status = await invoke("get_comfyui_runtime_status");
      if (status?.running) {
        await invoke("open_external_url", { url: "http://127.0.0.1:8188" });
        return true;
      }
    } catch (_) {}
    await new Promise((resolve) => window.setTimeout(resolve, 450));
  }
  return false;
}

function updateComfyModeUi() {
  const installMode = state.comfyMode !== "manage";
  const hasSelectedInstall = Boolean(String(el.comfyExistingInstall?.value || "").trim());
  const canShowManageActions = !installMode && hasSelectedInstall;
  el.installComfyui?.classList.toggle("hidden", !installMode);
  el.comfyResumeBanner?.classList.toggle("hidden", !installMode || !state.comfyResumeState?.found);
  el.comfyOpenInstallFolder?.classList.toggle("hidden", !canShowManageActions);
  el.comfyStartInstalled?.classList.toggle("hidden", !canShowManageActions);
  updateComfyUpdateButton();
  if (el.comfyModeHelp) {
    el.comfyModeHelp.textContent = installMode
      ? "Install a new ComfyUI into the selected base folder"
      : "Manage add-ons and runtime for a selected installation";
  }
  if (el.comfyInstallRoot) {
    el.comfyInstallRoot.placeholder = installMode
      ? "Select base folder (e.g. Documents). App will create /ComfyUI inside it."
      : "Base folder containing ComfyUI installations";
  }
  if (el.comfyTorchProfile) {
    el.comfyTorchProfile.disabled = !installMode;
    el.comfyTorchProfile.title = installMode
      ? ""
      : "Torch stack is locked in Manage Existing mode.";
  }
}

async function loadComfyExtraModelConfigForRoot(rootPath) {
  const root = normalizeSlashes(String(rootPath || "").trim());
  if (!root) return;
  try {
    const cfg = await invoke("get_comfyui_extra_model_config", { comfyuiRoot: root });
    if (cfg?.configured) {
      if (el.comfyExtraModelRoot) {
        el.comfyExtraModelRoot.value = String(cfg.base_path || "").trim();
      }
      if (el.comfyExtraModelDefault) {
        el.comfyExtraModelDefault.checked = Boolean(cfg.use_as_default);
      }
      state.sharedModelsRootDefault = String(cfg.base_path || "").trim();
      state.sharedModelsUseDefault = Boolean(cfg.use_as_default);
    } else {
      if (el.comfyExtraModelRoot) el.comfyExtraModelRoot.value = "";
      if (el.comfyExtraModelDefault) el.comfyExtraModelDefault.checked = false;
    }
  } catch (err) {
    logComfyLine(`Failed loading extra model path config: ${err}`);
  }
}

async function persistComfyExtraModelConfigForRoot(rootPath) {
  const root = normalizeSlashes(String(rootPath || "").trim());
  if (!root) return;
  const extraModelRoot = String(el.comfyExtraModelRoot?.value || "").trim();
  const useAsDefault = Boolean(el.comfyExtraModelDefault?.checked && extraModelRoot);
  try {
    await invoke("set_comfyui_extra_model_config", {
      comfyuiRoot: root,
      extraModelRoot: extraModelRoot || null,
      useAsDefault,
    });
  } catch (err) {
    logComfyLine(`Failed to save extra model path config: ${err}`);
  }
}

async function refreshExistingInstallations(basePath, preferredRoot = null) {
  const base = normalizeSlashes(basePath);
  let installs = [];
  try {
    installs = await invoke("list_comfyui_installations", { basePath: base || null });
  } catch (_) {
    installs = [];
  }

  if (!el.comfyExistingInstall) return installs;
  const explicitPreferred = normalizeSlashes(String(preferredRoot || "").trim());
  const currentPreferred = explicitPreferred || normalizeSlashes(String(
    el.comfyRoot.value || el.comfyExistingInstall.value || "",
  ).trim());
  el.comfyExistingInstall.innerHTML = "";

  if (!installs.length) {
    state.comfyMode = "install";
    if (el.comfyMode) el.comfyMode.value = "install";
    const empty = document.createElement("option");
    empty.value = "";
    empty.textContent = "No detected installations";
    el.comfyExistingInstall.appendChild(empty);
    el.comfyExistingInstall.value = "";
    if (el.comfyStartInstalled) {
      el.comfyStartInstalled.dataset.path = "";
    }
    if (el.comfyOpenInstallFolder) {
      el.comfyOpenInstallFolder.dataset.path = "";
    }
    if (el.comfyRoot) el.comfyRoot.value = "";
    if (el.comfyRootLora) el.comfyRootLora.value = "";
    invoke("set_comfyui_root", { comfyuiRoot: "" }).catch(() => {});
    state.selectedComfyVersion = null;
    state.comfyUpdateAvailable = false;
    state.comfyUpdateChecked = false;
    resetComfySelectionsToDefaults();
    updateComfyModeUi();
    renderTitleMeta();
    return installs;
  }

  installs.forEach((item) => {
    const opt = document.createElement("option");
    opt.value = item.root;
    opt.textContent = `${item.name} - ${item.root}`;
    el.comfyExistingInstall.appendChild(opt);
  });

  const preferred = explicitPreferred
    ? installs.find((x) => normalizeSlashes(x.root) === explicitPreferred)
    : null;
  if (preferred) {
    el.comfyExistingInstall.value = preferred.root;
  } else {
    const fallback = state.comfyMode === "manage"
      ? newestComfyInstall(installs)
      : (installs.find((x) => normalizeSlashes(x.root) === currentPreferred) || installs[0]);
    el.comfyExistingInstall.value = (fallback || installs[0]).root;
  }
  updateComfyModeUi();
  refreshComfyUiUpdateStatus(el.comfyExistingInstall.value).catch(() => {});
  return installs;
}

async function applySelectedExistingInstallation(rootPath) {
  const root = normalizeSlashes(rootPath);
  if (!root) return;
  setTorchRecommendedDetecting(true);
  el.comfyRoot.value = root;
  el.comfyRootLora.value = root;
  try {
    await invoke("set_comfyui_root", { comfyuiRoot: root });
    await loadInstalledAddonState(root);
    await loadComfyExtraModelConfigForRoot(root);
    setComfyQuickActions(el.comfyInstallRoot.value, root);
    await refreshComfyUiUpdateStatus(root);
  } finally {
    setTorchRecommendedDetecting(false);
  }
}

async function refreshComfyUiUpdateStatus(rootPath = null) {
  const root = normalizeSlashes(rootPath || el.comfyExistingInstall?.value || el.comfyRoot.value || "");
  state.comfyUpdateChecked = false;
  state.comfyUpdateAvailable = false;
  state.comfyLatestVersion = null;
  state.selectedComfyVersion = null;
  updateComfyUpdateButton();
  renderTitleMeta();
  if (!root) return;
  try {
    const cacheKey = normalizeSlashes(root);
    const now = Date.now();
    const cached = comfyUpdateStatusCache.get(cacheKey);
    let status;
    if (cached && (now - cached.at) < COMFY_UPDATE_STATUS_CACHE_MS) {
      status = cached.status;
    } else if (comfyUpdateStatusInflight.has(cacheKey)) {
      status = await comfyUpdateStatusInflight.get(cacheKey);
    } else {
      const request = invoke("get_comfyui_update_status", { comfyuiRoot: root })
        .then((result) => {
          comfyUpdateStatusCache.set(cacheKey, { status: result, at: Date.now() });
          return result;
        })
        .finally(() => {
          comfyUpdateStatusInflight.delete(cacheKey);
        });
      comfyUpdateStatusInflight.set(cacheKey, request);
      status = await request;
    }
    state.comfyUpdateChecked = Boolean(status?.checked);
    state.comfyUpdateAvailable = Boolean(status?.update_available);
    state.comfyLatestVersion = status?.latest_version || null;
    const detailTextRaw = String(status?.detail || "");
    const headMatchesTag = Boolean(status?.head_matches_latest_tag);
    state.selectedComfyVersion = headMatchesTag
      ? (status?.latest_version || status?.installed_version || null)
      : (status?.installed_version || null);
    updateComfyUpdateButton();
    renderTitleMeta();
    if (status?.detail) {
      const detailText = detailTextRaw;
      const detailKey = `${normalizeSlashes(root)}::${detailText}`;
      if (state.comfyLastUpdateDetailLogKey !== detailKey) {
        logComfyLine(detailText);
        state.comfyLastUpdateDetailLogKey = detailKey;
      }
    }
  } catch (err) {
    comfyUpdateStatusCache.delete(normalizeSlashes(root));
    state.comfyUpdateChecked = false;
    state.comfyUpdateAvailable = false;
    state.comfyLatestVersion = null;
    state.selectedComfyVersion = null;
    updateComfyUpdateButton();
    renderTitleMeta();
    logComfyLine(`ComfyUI update check failed: ${err}`);
  }
}

async function syncComfyInstallSelection(
  selectedPath,
  persistInstallBase = true,
  keepCurrentMode = false,
  emitDetectionLog = true,
) {
  const selected = normalizeSlashes(selectedPath);
  if (!selected) return;
  try {
    const inspection = await invoke("inspect_comfyui_path", { path: selected });
    const detectedRoot = normalizeSlashes(inspection?.detected_root || "");
    const normalizedSelected = normalizeSlashes(inspection?.selected || selected);

    if (detectedRoot) {
      // If user picked an existing ComfyUI root directly, keep install base as its parent.
      const pickedRootDirectly = normalizeSlashes(detectedRoot) === normalizeSlashes(normalizedSelected);
      const baseForInstall = pickedRootDirectly
        ? parentDir(detectedRoot)
        : normalizedSelected;
      el.comfyInstallRoot.value = baseForInstall;
      if (persistInstallBase) {
        await invoke("set_comfyui_install_base", { comfyuiInstallBase: baseForInstall });
      }
      const installs = await refreshExistingInstallations(
        baseForInstall,
        pickedRootDirectly ? detectedRoot : null,
      );
      if (!keepCurrentMode) {
        state.comfyMode = "manage";
        if (el.comfyMode) el.comfyMode.value = "manage";
      }
      updateComfyModeUi();
      if (pickedRootDirectly || installs.length === 1) {
        await applySelectedExistingInstallation(detectedRoot);
      } else if (installs.length > 1 && state.comfyMode === "manage") {
        await applySelectedExistingInstallation(el.comfyExistingInstall.value);
      }
      setComfyQuickActions(baseForInstall, detectedRoot);
      if (emitDetectionLog) {
        logComfyLine(`Detected ComfyUI install: ${detectedRoot}`);
      }
      await refreshComfyRuntimeStatus();
      if (emitDetectionLog && state.comfyRuntimeRunning) {
        logComfyLine("Detected running ComfyUI server. If you want to start a different one, stop this server first.");
      }
      return;
    }

    el.comfyInstallRoot.value = normalizedSelected;
    if (persistInstallBase) {
      await invoke("set_comfyui_install_base", { comfyuiInstallBase: normalizedSelected });
    }
    if (state.comfyMode !== "manage") {
      resetComfySelectionsToDefaults();
    }
    const installs = await refreshExistingInstallations(normalizedSelected);
    if (installs.length > 0) {
      if (!keepCurrentMode) {
        state.comfyMode = "manage";
        if (el.comfyMode) el.comfyMode.value = "manage";
      }
      updateComfyModeUi();
      const latest = newestComfyInstall(installs) || installs[0];
      if (latest?.root) {
        await applySelectedExistingInstallation(latest.root);
      }
    }
    await refreshComfyResumeState();
  } catch (_) {
    el.comfyInstallRoot.value = selected;
    if (persistInstallBase) {
      await invoke("set_comfyui_install_base", { comfyuiInstallBase: selected });
    }
    if (state.comfyMode !== "manage") {
      resetComfySelectionsToDefaults();
    }
    const installs = await refreshExistingInstallations(selected);
    if (installs.length > 0) {
      state.comfyMode = "manage";
      if (el.comfyMode) el.comfyMode.value = "manage";
      updateComfyModeUi();
      const latest = newestComfyInstall(installs) || installs[0];
      if (latest?.root) {
        await applySelectedExistingInstallation(latest.root);
      }
    }
    await refreshComfyResumeState();
  }
}

function renderPreflight(result) {
  if (!el.preflightList || !el.preflightSummary) return;
  const items = Array.isArray(result?.items) ? result.items : [];
  el.preflightList.innerHTML = "";
  if (!items.length) {
    const msg = document.createElement("div");
    msg.className = "empty-msg";
    msg.textContent = "No checks executed yet.";
    el.preflightList.appendChild(msg);
    el.preflightSummary.textContent = "Not run yet.";
    state.comfyPreflightOk = null;
    return;
  }

  items.forEach((item) => {
    const row = document.createElement("div");
    row.className = `preflight-item ${String(item.status || "warn").toLowerCase()}`;
    const status = document.createElement("div");
    status.className = "status";
    status.textContent = String(item.status || "warn").toUpperCase();
    const text = document.createElement("div");
    text.textContent = `${item.title}: ${item.detail}`;
    row.appendChild(status);
    row.appendChild(text);
    el.preflightList.appendChild(row);
  });

  state.comfyPreflightOk = Boolean(result?.ok);
  el.preflightSummary.textContent = result?.summary || (state.comfyPreflightOk ? "Preflight passed." : "Preflight has issues.");
}

async function runComfyPreflight() {
  try {
    const request = buildComfyInstallRequest();
    const result = await invoke("run_comfyui_preflight", { request });
    renderPreflight(result);
    return result;
  } catch (err) {
    renderPreflight({
      ok: false,
      summary: "Preflight failed to run.",
      items: [{ status: "fail", title: "Preflight runtime", detail: String(err) }],
    });
    return null;
  }
}

async function refreshComfyResumeState() {
  try {
    const installBase = String(el.comfyInstallRoot.value || "").trim() || null;
    const result = await invoke("get_comfyui_resume_state", { installBase });
    state.comfyResumeState = result || null;
    if (!result?.found) {
      el.comfyResumeBanner?.classList.add("hidden");
      updateComfyModeUi();
      return;
    }
    if (el.comfyResumeText) {
      el.comfyResumeText.textContent = result.summary || "Interrupted install found.";
    }
    el.comfyResumeBanner?.classList.remove("hidden");
    updateComfyModeUi();
  } catch (_) {
    state.comfyResumeState = null;
    el.comfyResumeBanner?.classList.add("hidden");
    updateComfyModeUi();
  }
}

async function startComfyInstall(forceFresh) {
  if (state.comfyInstallBusy) {
    const cancelled = await invoke("cancel_comfyui_install");
    if (cancelled) {
      logComfyLine("ComfyUI installation cancellation requested.");
    } else {
      logComfyLine("No active ComfyUI installation.");
    }
    return;
  }
  const root = String(el.comfyInstallRoot.value || "").trim();
  if (!root) {
    logComfyLine("Select install folder first.");
    return;
  }

  await refreshComfyRuntimeStatus();
  if (state.comfyRuntimeRunning) {
    logComfyLine("Detected running ComfyUI server. Stopping it before install...");
    try {
      await invoke("stop_comfyui_root");
    } catch (err) {
      logComfyLine(`Failed to stop running ComfyUI before install: ${err}`);
      return;
    }
    await refreshComfyRuntimeStatus();
    if (state.comfyRuntimeRunning) {
      logComfyLine("ComfyUI is still running. Stop it first, then retry install.");
      return;
    }
    logComfyLine("ComfyUI server stopped. Proceeding with install.");
  }

  const preflight = await runComfyPreflight();
  if (!preflight || !preflight.ok) {
    logComfyLine("Preflight has blocking issues. Resolve them before install.");
    return;
  }
  state.comfyInstallBusy = true;
  updateComfyInstallButton();
  logComfyLine(forceFresh ? "Starting fresh ComfyUI installation..." : "Starting ComfyUI installation...");
  try {
    const request = buildComfyInstallRequest();
    request.forceFresh = Boolean(forceFresh);
    await invoke("start_comfyui_install", { request });
    logComfyLine("ComfyUI installation started.");
  } catch (err) {
    state.comfyInstallBusy = false;
    updateComfyInstallButton();
    logComfyLine(`ComfyUI install failed to start: ${err}`);
  }
}

function applyComfyAddonRules() {
  const profile = String(el.comfyTorchProfile?.value || "").trim();
  const nonCudaSelected = profile === "torch291_rocm72" || profile === "torchxpu_nightly";

  if (el.addonSageAttention3) {
    const wasChecked = el.addonSageAttention3.checked;
    el.addonSageAttention3.disabled = !state.comfySage3Eligible;
    if (!state.comfySage3Eligible && wasChecked) {
      el.addonSageAttention3.checked = false;
    }
  }

  if (el.addonTrellis2) {
    const trellisAllowed = profile === "torch280_cu128";
    const wasChecked = el.addonTrellis2.checked;
    el.addonTrellis2.disabled = !trellisAllowed;
    if (!trellisAllowed && wasChecked) {
      el.addonTrellis2.checked = false;
    }
  }

  [
    el.addonSageAttention,
    el.addonSageAttention3,
    el.addonFlashAttention,
    el.addonNunchaku,
  ].forEach((box) => {
    if (!box) return;
    const wasChecked = box.checked;
    box.disabled = nonCudaSelected || box.disabled;
    if (nonCudaSelected && wasChecked) {
      box.checked = false;
    }
  });

  [el.flagSageAttention, el.flagFlashAttention].forEach((box) => {
    if (!box) return;
    const wasChecked = box.checked;
    box.disabled = nonCudaSelected;
    if (nonCudaSelected && wasChecked) {
      box.checked = false;
    }
  });

  if (el.addonNunchaku && el.addonInsightFace) {
    const nunchakuSelected = Boolean(el.addonNunchaku.checked);
    if (nunchakuSelected) {
      el.addonInsightFace.checked = true;
      el.addonInsightFace.disabled = true;
    } else {
      el.addonInsightFace.disabled = false;
    }
  }
}

function trimDescription(text, max = 520) {
  const value = (text || "").trim();
  if (!value) return "-";
  if (value.length <= max) return value;
  return `${value.slice(0, max).trimEnd()}...`;
}

function isVideoPreviewUrl(url) {
  const value = String(url || "").toLowerCase();
  return value.endsWith(".mp4") || value.endsWith(".webm") || value.endsWith(".mov")
    || value.includes(".mp4?") || value.includes(".webm?") || value.includes(".mov?");
}

function applyLoraPreview(previewUrl, previewKind) {
  const url = String(previewUrl || "").trim();
  const kindRaw = String(previewKind || "").trim().toLowerCase();
  const kind = kindRaw === "video" || kindRaw === "image"
    ? kindRaw
    : (url ? (isVideoPreviewUrl(url) ? "video" : "image") : "none");

  if (!url || kind === "none") {
    el.previewImage.classList.add("hidden");
    el.previewVideo.classList.add("hidden");
    el.previewImage.src = "";
    el.previewVideo.src = "";
    el.previewCaption.textContent = "No preview available.";
    return;
  }

  if (kind === "video") {
    el.previewVideo.src = url;
    el.previewVideo.classList.remove("hidden");
    el.previewImage.classList.add("hidden");
    el.previewImage.src = "";
    el.previewCaption.textContent = "Video preview loaded.";
    return;
  }

  el.previewImage.src = url;
  el.previewImage.classList.remove("hidden");
  el.previewVideo.classList.add("hidden");
  el.previewVideo.src = "";
  el.previewCaption.textContent = "Image preview loaded.";
}

async function copyText(value) {
  const text = String(value || "").trim();
  if (!text) return false;
  try {
    if (navigator.clipboard && navigator.clipboard.writeText) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch (_) {}

  const area = document.createElement("textarea");
  area.value = text;
  area.setAttribute("readonly", "");
  area.style.position = "fixed";
  area.style.opacity = "0";
  document.body.appendChild(area);
  area.select();
  const ok = document.execCommand("copy");
  document.body.removeChild(area);
  return ok;
}

function renderTriggerWords(words) {
  const list = Array.isArray(words) ? words.filter((x) => String(x || "").trim()) : [];
  el.metaTriggers.innerHTML = "";
  if (!list.length) {
    el.metaTriggers.textContent = "-";
    return;
  }
  const frag = document.createDocumentFragment();
  list.forEach((word, idx) => {
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = word;
    button.style.width = "auto";
    button.style.minHeight = "28px";
    button.style.padding = "4px 8px";
    button.style.marginRight = "6px";
    button.style.marginBottom = "6px";
    button.addEventListener("click", async () => {
      const ok = await copyText(word);
      if (!ok) {
        logLine("Copy failed.");
        return;
      }
      const original = button.textContent;
      button.textContent = "Copied";
      button.disabled = true;
      window.setTimeout(() => {
        button.textContent = original;
        button.disabled = false;
      }, 900);
    });
    frag.appendChild(button);
    if (idx < list.length - 1) {
      const spacer = document.createTextNode(" ");
      frag.appendChild(spacer);
    }
  });
  el.metaTriggers.appendChild(frag);
}

function formatBytes(v) {
  const value = Number(v || 0);
  if (!value) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let n = value;
  let u = 0;
  while (n >= 1024 && u < units.length - 1) {
    n /= 1024;
    u += 1;
  }
  return `${n.toFixed(u === 0 ? 0 : 1)} ${units[u]}`;
}

function formatVramMbToGb(vramMb) {
  const value = Number(vramMb || 0);
  if (!value) return null;
  return `${(value / 1024).toFixed(1)} GB VRAM`;
}

function smoothedReceived(item) {
  const target = Math.max(0, Number(item.received || 0));
  const now = Date.now();
  if (!Number.isFinite(item.displayReceived)) item.displayReceived = 0;
  if (!Number.isFinite(item.displayTs)) item.displayTs = now;
  if (item.displayReceived > target) item.displayReceived = target;

  const dtMs = Math.max(16, now - item.displayTs);
  item.displayTs = now;
  const delta = target - item.displayReceived;
  if (delta <= 0) return item.displayReceived;

  const minStep = 128 * 1024;
  const easedStep = delta * 0.25;
  const rateCapStep = (dtMs / 1000) * (320 * 1024 * 1024);
  const advance = Math.min(
    delta,
    Math.max(minStep, Math.min(rateCapStep, Math.max(minStep, easedStep))),
  );
  item.displayReceived += advance;
  return item.displayReceived;
}

function ensureProgressSmoother() {
  if (progressSmoothTimer) return;
  progressSmoothTimer = window.setInterval(() => {
    const active = [...state.transfers.values()].filter((x) => x.phase !== "finished" && x.phase !== "failed");
    if (!active.length && state.busyDownloads <= 0) {
      window.clearInterval(progressSmoothTimer);
      progressSmoothTimer = null;
      return;
    }
    renderActiveTransfers();
    renderOverallProgress();
  }, 140);
}

function renderOverallProgress() {
  const active = [...state.transfers.values()].filter((x) => x.phase !== "finished" && x.phase !== "failed");
  const busyOnly = state.busyDownloads > 0 && active.length === 0;

  if (!active.length && !busyOnly) {
    el.overallProgress.classList.add("hidden");
    el.overallProgress.classList.remove("indeterminate");
    el.overallProgressMeta.classList.add("hidden");
    el.overallProgressFill.style.width = "0%";
    return;
  }

  el.overallProgress.classList.remove("hidden");
  el.overallProgressMeta.classList.remove("hidden");

  const lead = active[0];
  if (lead) {
    const leadSize = Number(lead.size || 0);
    const leadShown = leadSize > 0
      ? Math.min(smoothedReceived(lead), leadSize)
      : smoothedReceived(lead);
    const leadPct = leadSize > 0 ? ` ${Math.round((leadShown / leadSize) * 100)}%` : "";
    setProgress(`[${lead.kind || "download"}] ${lead.artifact || "file"}${leadPct}`);
  }

  const known = active.filter((x) => Number(x.size || 0) > 0);
  if (!known.length) {
    el.overallProgress.classList.add("indeterminate");
    el.overallProgressFill.style.removeProperty("width");
    const activeCount = Math.max(active.length, state.busyDownloads > 0 ? 1 : 0);
    el.overallProgressMeta.textContent = `Downloading ${activeCount} file(s)...`;
    return;
  }

  const totalBytes = known.reduce((sum, x) => sum + Number(x.size || 0), 0);
  const receivedBytes = known.reduce((sum, x) => sum + Math.min(smoothedReceived(x), Number(x.size || 0)), 0);
  const pct = totalBytes > 0 ? Math.max(0, Math.min(100, Math.round((receivedBytes / totalBytes) * 100))) : 0;
  const unknownCount = Math.max(0, active.length - known.length);

  el.overallProgress.classList.remove("indeterminate");
  el.overallProgressFill.style.width = `${pct}%`;
  el.overallProgressMeta.textContent = unknownCount > 0
    ? `${pct}%${DOT_SEP}${formatBytes(receivedBytes)} / ${formatBytes(totalBytes)}${DOT_SEP}${known.length} known + ${unknownCount} unknown`
    : `${pct}%${DOT_SEP}${formatBytes(receivedBytes)} / ${formatBytes(totalBytes)}${DOT_SEP}${known.length} active`;
}

function beginBusyDownload(label) {
  state.busyDownloads += 1;
  if (!state.activeDownloadKind) {
    if (state.activeTab === "loras") {
      state.activeDownloadKind = "lora";
    } else if (state.activeTab === "workflows") {
      state.activeDownloadKind = "workflow";
    } else {
      state.activeDownloadKind = "model";
    }
  }
  setProgress(label || "Downloading...");
  updateDownloadButtons();
  renderOverallProgress();
  ensureProgressSmoother();
}

function endBusyDownload() {
  state.busyDownloads = Math.max(0, state.busyDownloads - 1);
  if (state.busyDownloads === 0) {
    state.activeDownloadKind = null;
    setProgress("Idle");
  }
  updateDownloadButtons();
  renderOverallProgress();
}

function updateDownloadButtons() {
  const cancelling = state.busyDownloads > 0;
  if (cancelling) {
    el.downloadModel.textContent = "Cancel Download";
    el.downloadLora.textContent = "Cancel Download";
    el.downloadWorkflow.textContent = "Cancel Download";
  } else {
    el.downloadModel.textContent = "Download Selected Model Assets";
    el.downloadLora.textContent = "Download LoRA";
    el.downloadWorkflow.textContent = "Download Workflow";
  }
}

async function requestCancelDownload() {
  try {
    setProgress("Cancelling download...");
    const cancelled = await invoke("cancel_active_download");
    if (cancelled) {
      logLine("Cancellation requested.");
      setProgress("Cancellation requested...");
    } else {
      logLine("No active download to cancel.");
      endBusyDownload();
    }
  } catch (err) {
    logLine(`Cancel failed: ${err}`);
    endBusyDownload();
  }
}

function renderActiveTransfers() {
  const now = Date.now();
  const active = [...state.transfers.values()].filter((x) => x.phase !== "finished" && x.phase !== "failed");
  el.transferList.innerHTML = "";
  if (!active.length) {
    const msg = document.createElement("div");
    msg.className = "empty-msg";
    msg.textContent = "No active transfers.";
    el.transferList.appendChild(msg);
  }
  for (const item of active) {
    const smoothed = smoothedReceived(item);
    const shownReceived = item.size > 0 ? Math.min(smoothed, item.size) : smoothed;
    const pct = item.size > 0 ? Math.max(0, Math.min(100, Math.round((shownReceived / item.size) * 100))) : 0;
    const quietMs = now - Number(item.lastUpdateTs || now);
    const nearEnd = item.size > 0 && shownReceived >= item.size * 0.9;
    const finalizing = item.phase === "progress" && nearEnd && quietMs > 2500;
    const phaseLabel = finalizing ? "finalizing" : item.phase;
    const row = document.createElement("div");
    row.className = "transfer-item";
    const title = document.createElement("div");
    title.className = "transfer-title";
    title.textContent = item.artifact || item.id;
    const bar = document.createElement("div");
    bar.className = "bar";
    const fill = document.createElement("span");
    fill.style.width = `${pct}%`;
    bar.appendChild(fill);
    const sub = document.createElement("div");
    sub.className = "transfer-sub";
    sub.textContent = item.size
      ? `${phaseLabel}${DOT_SEP}${formatBytes(shownReceived)} / ${formatBytes(item.size)}`
      : phaseLabel;
    row.appendChild(title);
    row.appendChild(bar);
    row.appendChild(sub);
    el.transferList.appendChild(row);
  }
}

function renderCompletedTransfers() {
  el.completedList.innerHTML = "";
  if (!state.completed.length) {
    const msg = document.createElement("div");
    msg.className = "empty-msg";
    msg.textContent = "No completed downloads.";
    el.completedList.appendChild(msg);
  } else {
    const max = Math.min(30, state.completed.length);
    for (let i = 0; i < max; i += 1) {
      const item = state.completed[i];
      const hasFolder = Boolean(item.folder && item.folder.trim());
      const row = document.createElement("div");
      row.className = "transfer-item";
      const title = document.createElement("div");
      title.className = "transfer-title";
      title.textContent = item.name;
      const sub = document.createElement("div");
      sub.className = "transfer-sub";
      sub.textContent = item.status;
      const button = document.createElement("button");
      button.textContent = "Open Folder";
      button.setAttribute("type", "button");
      if (!hasFolder) {
        button.disabled = true;
      } else {
        button.addEventListener("click", async () => {
          try {
            await invoke("open_folder", { path: item.folder });
          } catch (err) {
            logLine(`Open folder failed: ${err}`);
          }
        });
      }
      row.appendChild(title);
      row.appendChild(sub);
      row.appendChild(button);
      el.completedList.appendChild(row);
    }
  }
}

function renderTransfers() {
  renderActiveTransfers();
  renderCompletedTransfers();
  renderOverallProgress();
}

function addCompleted(item) {
  const index = state.completed.findIndex(
    (x) => x.name === item.name && x.status === item.status && x.folder === (item.folder || ""),
  );
  if (index >= 0) {
    if (item.folder && item.folder.trim()) {
      state.completed[index].folder = item.folder;
    }
  } else {
    state.completed.unshift({
      id: `done-${Date.now()}-${state.completedSeq++}`,
      name: item.name,
      folder: item.folder || "",
      status: item.status,
    });
  }
}

function setOptions(select, options, selectedValue = null) {
  const current = selectedValue ?? select.value;
  select.innerHTML = "";
  options.forEach((item) => {
    const opt = document.createElement("option");
    opt.value = item.value;
    opt.textContent = item.label;
    opt.disabled = Boolean(item.disabled);
    select.appendChild(opt);
  });
  if (options.find((item) => item.value === current)) {
    select.value = current;
  }
}

function switchTab(tab) {
  state.activeTab = tab;
  const comfyui = tab === "comfyui";
  const models = tab === "models";
  const loras = tab === "loras";
  const workflows = tab === "workflows";
  el.tabComfyui.classList.toggle("active", comfyui);
  el.tabModels.classList.toggle("active", models);
  el.tabLoras.classList.toggle("active", loras);
  el.tabWorkflows.classList.toggle("active", workflows);
  el.contentComfyui.classList.toggle("hidden", !comfyui);
  el.contentModels.classList.toggle("hidden", !models);
  el.contentLoras.classList.toggle("hidden", !loras);
  el.contentWorkflows.classList.toggle("hidden", !workflows);
  el.downloadsStatusPanel.classList.toggle("hidden", comfyui);
}

function familyOptions(models) {
  const families = [...new Set(models.map((m) => m.family))].sort();
  return [{ value: "all", label: "All Model Families" }, ...families.map((f) => ({ value: f, label: f }))];
}

function loraFamilyOptions(loras) {
  const families = [...new Set(loras.map((l) => l.family).filter(Boolean))].sort();
  return [{ value: "all", label: "All LoRA Families" }, ...families.map((f) => ({ value: f, label: f }))];
}

function workflowFamilyOptions(workflows) {
  const families = [...new Set((workflows || []).map((w) => w.family).filter(Boolean))].sort();
  return [{ value: "all", label: "All Workflow Families" }, ...families.map((f) => ({ value: f, label: f }))];
}

function filteredModelsForCurrentSelection() {
  if (!state.catalog) return [];
  const family = el.modelFamily.value || "all";
  const search = String(el.modelSearch?.value || "").trim().toLowerCase();
  return state.catalog.models.filter((model) => {
    if (family !== "all" && model.family !== family) return false;
    if (!search) return true;
    const haystack = `${model.display_name} ${model.family} ${model.id}`.toLowerCase();
    return haystack.includes(search);
  });
}

function variantsForModelAndTier(model, tier) {
  return (model?.variants || []).filter((variant) => variant.tier === tier);
}

function modelVariantLabel(variant) {
  return [variant.model_size, variant.quantization, variant.note, variant.tier?.toUpperCase?.()]
    .filter(Boolean)
    .join(DOT_SEP);
}

function selectedModelItems() {
  const items = [];
  state.selectedModelVariants.forEach((variantId, modelId) => {
    const model = state.catalog?.models?.find((entry) => entry.id === modelId);
    if (!model) return;
    const variant = (model.variants || []).find((entry) => entry.id === variantId);
    if (!variant) return;
    items.push({
      modelId,
      variantId,
      label: `${model.display_name}${variant ? `${DOT_SEP}${modelVariantLabel(variant)}` : ""}`,
    });
  });
  return items;
}

function updateModelSelectionSummary() {
  if (!el.modelSelectionSummary) return;
  const items = selectedModelItems();
  if (!items.length) {
    el.modelSelectionSummary.textContent = "No models selected.";
    return;
  }
  if (items.length === 1) {
    el.modelSelectionSummary.textContent = `1 model selected${DOT_SEP}${items[0].label}`;
    return;
  }
  el.modelSelectionSummary.textContent = `${items.length} models selected.`;
}

function renderSelectedModelQueue() {
  if (!el.selectedModelQueue) return;
  const items = selectedModelItems();
  el.selectedModelQueue.innerHTML = "";
  if (!items.length) {
    const empty = document.createElement("div");
    empty.className = "empty-msg";
    empty.textContent = "No models selected yet.";
    el.selectedModelQueue.appendChild(empty);
    return;
  }

  items
    .sort((a, b) => a.label.localeCompare(b.label))
    .forEach((item) => {
      const row = document.createElement("div");
      row.className = "queue-item";
      const head = document.createElement("div");
      head.className = "queue-item-head";
      const title = document.createElement("div");
      title.className = "queue-item-title";
      title.textContent = item.label;
      const remove = document.createElement("button");
      remove.type = "button";
      remove.textContent = "Remove";
      remove.addEventListener("click", () => {
        state.selectedModelVariants.delete(item.modelId);
        renderModelSelectionList();
      });
      head.appendChild(title);
      head.appendChild(remove);
      row.appendChild(head);
      el.selectedModelQueue.appendChild(row);
    });
}

function renderModelSelectionList() {
  if (!el.modelSelectionList) return;
  const models = filteredModelsForCurrentSelection();
  const tier = el.vramTier.value;
  el.modelSelectionList.innerHTML = "";

  if (!models.length) {
    const empty = document.createElement("div");
    empty.className = "empty-msg";
    empty.textContent = "No models available for this filter.";
    el.modelSelectionList.appendChild(empty);
    updateModelSelectionSummary();
    renderSelectedModelQueue();
    return;
  }

  models.forEach((model) => {
    const variants = variantsForModelAndTier(model, tier);
    const selectedVariantId = state.selectedModelVariants.get(model.id);
    const fallbackVariantId = variants[0]?.id || "";
    const currentVariantId = variants.some((variant) => variant.id === selectedVariantId)
      ? selectedVariantId
      : fallbackVariantId;

    if (state.selectedModelVariants.has(model.id) && currentVariantId) {
      state.selectedModelVariants.set(model.id, currentVariantId);
    } else if (!currentVariantId) {
      state.selectedModelVariants.delete(model.id);
    }

    const row = document.createElement("div");
    row.className = "model-select-item";
    const head = document.createElement("div");
    head.className = "model-select-head";

    const box = document.createElement("input");
    box.type = "checkbox";
    box.checked = Boolean(currentVariantId && state.selectedModelVariants.has(model.id));
    box.disabled = !currentVariantId;
    box.addEventListener("change", () => {
      if (box.checked && currentVariantId) {
        state.selectedModelVariants.set(model.id, currentVariantId);
      } else {
        state.selectedModelVariants.delete(model.id);
      }
      updateModelSelectionSummary();
      renderSelectedModelQueue();
    });

    const labelWrap = document.createElement("div");
    labelWrap.className = "model-select-label";
    const title = document.createElement("div");
    title.className = "model-select-title";
    title.textContent = model.display_name;
    const meta = document.createElement("div");
    meta.className = "model-select-meta";
    meta.textContent = `${model.family}${variants.length ? `${DOT_SEP}${variants.length} variant${variants.length === 1 ? "" : "s"} for ${String(tier || "").toUpperCase()}` : `${DOT_SEP}No variant for ${String(tier || "").toUpperCase()}`}`;
    labelWrap.appendChild(title);
    labelWrap.appendChild(meta);

    const variantSelect = document.createElement("select");
    const variantOptions = variants.length
      ? variants.map((variant) => ({ value: variant.id, label: modelVariantLabel(variant) }))
      : [{ value: "", label: "No variant for selected VRAM tier", disabled: true }];
    setOptions(variantSelect, variantOptions, currentVariantId);
    variantSelect.disabled = !variants.length;
    variantSelect.addEventListener("change", () => {
      const nextVariantId = String(variantSelect.value || "").trim();
      if (box.checked && nextVariantId) {
        state.selectedModelVariants.set(model.id, nextVariantId);
      }
      updateModelSelectionSummary();
      renderSelectedModelQueue();
    });

    head.appendChild(box);
    head.appendChild(labelWrap);
    head.appendChild(variantSelect);
    row.appendChild(head);
    el.modelSelectionList.appendChild(row);
  });

  updateModelSelectionSummary();
  renderSelectedModelQueue();
}

async function refreshEffectiveDownloadDestination() {
  if (!el.effectiveDownloadDestination) return;
  const root = String(el.comfyRoot.value || "").trim();
  if (!root) {
    el.effectiveDownloadDestination.textContent = "Download destination: Select your ComfyUI root folder first.";
    return;
  }
  try {
    const result = await invoke("get_effective_download_destination", { comfyuiRoot: root });
    const effectiveRoot = String(result?.effective_root || root).trim();
    const usesShared = result?.uses_shared_default === true;
    el.effectiveDownloadDestination.textContent = usesShared
      ? `Download destination: ${effectiveRoot} (shared models default is active)`
      : `Download destination: ${effectiveRoot}`;
  } catch (err) {
    el.effectiveDownloadDestination.textContent = `Download destination unavailable: ${err}`;
  }
}

function refreshLoraSelectors() {
  if (!state.catalog) return;
  const family = el.loraFamily.value || "all";
  const filtered = state.catalog.loras.filter((l) => family === "all" || l.family === family);
  const options = filtered.map((l) => ({ value: l.id, label: l.display_name }));
  setOptions(el.loraId, options);
}

function refreshWorkflowSelectors() {
  if (!state.catalog) return;
  const family = el.workflowFamily.value || "all";
  const filtered = (state.catalog.workflows || []).filter((w) => family === "all" || w.family === family);
  const options = filtered.map((w) => ({ value: w.id, label: w.display_name }));
  setOptions(el.workflowId, options);
  loadWorkflowPreview();
}

function loadWorkflowPreview() {
  const selectedId = String(el.workflowId?.value || "").trim();
  const workflow = (state.catalog?.workflows || []).find((w) => w.id === selectedId);
  if (!workflow) {
    if (el.workflowPreviewImage) {
      el.workflowPreviewImage.classList.add("hidden");
      el.workflowPreviewImage.removeAttribute("src");
    }
    if (el.workflowPreviewCaption) {
      el.workflowPreviewCaption.textContent = "No workflow preview loaded.";
    }
    if (el.workflowYoutubeLink) {
      el.workflowYoutubeLink.textContent = "-";
      el.workflowYoutubeLink.setAttribute("href", "#");
      el.workflowYoutubeLink.style.pointerEvents = "none";
    }
    return;
  }

  const previewUrl = String(workflow.preview_image_url || "").trim();
  if (!previewUrl) {
    if (el.workflowPreviewImage) {
      el.workflowPreviewImage.classList.add("hidden");
      el.workflowPreviewImage.removeAttribute("src");
    }
    if (el.workflowPreviewCaption) {
      el.workflowPreviewCaption.textContent = "No preview image available for this workflow.";
    }
    return;
  }

  if (el.workflowPreviewImage) {
    el.workflowPreviewImage.src = previewUrl;
    el.workflowPreviewImage.classList.remove("hidden");
  }
  if (el.workflowPreviewCaption) {
    el.workflowPreviewCaption.textContent = workflow.display_name || "Workflow preview";
  }
  if (el.workflowYoutubeLink) {
    const youtubeUrl = String(workflow.youtube_url || "").trim();
    if (youtubeUrl) {
      el.workflowYoutubeLink.textContent = "Link";
      el.workflowYoutubeLink.setAttribute("href", youtubeUrl);
      el.workflowYoutubeLink.style.pointerEvents = "auto";
    } else {
      el.workflowYoutubeLink.textContent = "-";
      el.workflowYoutubeLink.setAttribute("href", "#");
      el.workflowYoutubeLink.style.pointerEvents = "none";
    }
  }
}

async function loadLoraMetadata() {
  const loraId = el.loraId.value;
  if (!loraId) return;
  const requestSeq = ++state.loraMetaRequestSeq;
  const cachedMeta = state.loraMetaCache.get(loraId) || null;

  if (cachedMeta && cachedMeta.preview_url) {
    applyLoraPreview(cachedMeta.preview_url, cachedMeta.preview_kind);
  }

  try {
    const rawMeta = await invoke("get_lora_metadata", {
      loraId,
      token: el.civitaiToken.value?.trim() || null,
    });
    const meta = { ...rawMeta };
    if (requestSeq !== state.loraMetaRequestSeq || loraId !== el.loraId.value) {
      return;
    }
    if ((!meta.preview_url || !String(meta.preview_url).trim()) && cachedMeta?.preview_url) {
      meta.preview_url = cachedMeta.preview_url;
      meta.preview_kind = cachedMeta.preview_kind;
    }
    state.loraMetaCache.set(loraId, meta);

    el.metaCreator.textContent = meta.creator || "-";
    const creatorName = String(meta.creator || "").trim();
    const creatorUrl = String(meta.creator_url || "").trim();
    const fallbackCreatorUrl = creatorName && creatorName !== "-" && creatorName.toLowerCase() !== "unknown creator"
      ? `https://civitai.com/user/${encodeURIComponent(creatorName)}`
      : "";
    const finalCreatorUrl = creatorUrl || fallbackCreatorUrl;
    if (finalCreatorUrl) {
      el.metaCreatorLink.href = finalCreatorUrl;
      el.metaCreatorLink.style.pointerEvents = "auto";
    } else {
      el.metaCreatorLink.href = "#";
      el.metaCreatorLink.style.pointerEvents = "none";
    }
    el.metaStrength.textContent = meta.strength || "-";
    renderTriggerWords(meta.triggers || []);
    el.metaDescription.textContent = trimDescription(meta.description || "-");
    state.currentLoraMetaId = loraId;

    applyLoraPreview(meta.preview_url, meta.preview_kind);
  } catch (err) {
    if (cachedMeta) {
      return;
    }
    logLine(`Metadata error: ${err}`);
  }
}

async function bootstrap() {
  if (!invoke) {
    logLine("Tauri invoke bridge unavailable.");
    return;
  }
  setStartupStatus("Loading settings and catalog...");
  const [settings, catalog] = await Promise.all([
    invoke("get_settings"),
    invoke("get_catalog"),
  ]);

  state.settings = settings;
  state.catalog = catalog;

  state.appVersion = settings?.last_installed_version || state.appVersion || "";
  state.titleSystemText = "Loading system info...";
  renderAppVersionTag();
  renderTitleMeta();
  const refreshSnapshot = (attempt = 0) => {
    invoke("get_app_snapshot")
      .then((snapshot) => {
        const ramRaw = Number(snapshot.total_ram_gb);
        const ramGb = Number.isFinite(ramRaw) ? (ramRaw > 1000 ? ramRaw / 1000 : ramRaw) : null;
        const ramText = `${ramGb != null ? ramGb.toFixed(1) : "?"} GB RAM`;
        const amdGpu = String(snapshot.amd_gpu_name || "").trim();
        const nvidiaGpu = String(snapshot.nvidia_gpu_name || "").trim();
        const intelGpu = String(snapshot.intel_gpu_name || "").trim();
        state.comfyDetectedGpuVendor = nvidiaGpu ? "nvidia" : (amdGpu ? "amd" : (intelGpu ? "intel" : ""));
        const gpuText = nvidiaGpu
          ? `${nvidiaGpu}${formatVramMbToGb(snapshot.nvidia_gpu_vram_mb) ? ` (${formatVramMbToGb(snapshot.nvidia_gpu_vram_mb)})` : ""}`
          : (amdGpu
            ? `AMD GPU: ${amdGpu}`
            : (intelGpu
              ? `Intel GPU: ${intelGpu}`
              : "GPU: Not detected"));
        state.appVersion = snapshot.version || state.appVersion;
        state.titleSystemText = `${ramText}${DOT_SEP}${gpuText}`;
        applyComfyTorchProfileOptions(el.comfyTorchProfile?.value || null);
        applyComfyAddonRules();
        renderAppVersionTag();
        renderTitleMeta();
        if (!amdGpu && !nvidiaGpu && !intelGpu && attempt < 8) {
          setTimeout(() => refreshSnapshot(attempt + 1), 600);
        }
      })
      .catch(() => {});
  };
  refreshSnapshot();

  el.comfyRoot.value = settings.comfyui_root || "";
  el.comfyRootLora.value = settings.comfyui_root || "";
  if (el.comfyRootWorkflow) {
    el.comfyRootWorkflow.value = settings.comfyui_root || "";
  }
  el.comfyInstallRoot.value = settings.comfyui_install_base || "";
  state.sharedModelsRootDefault = String(settings.shared_models_root || "").trim();
  state.sharedModelsUseDefault = Boolean(
    settings.shared_models_use_default
    || (state.sharedModelsRootDefault && settings.shared_models_use_default !== false),
  );
  if (el.comfyExtraModelRoot) {
    el.comfyExtraModelRoot.value = state.sharedModelsRootDefault;
  }
  if (el.comfyExtraModelDefault) {
    el.comfyExtraModelDefault.checked = state.sharedModelsRootDefault
      ? Boolean(state.sharedModelsUseDefault)
      : false;
  }
  if (el.comfyMode) {
    state.comfyMode = (settings.comfyui_root ? "manage" : "install");
    el.comfyMode.value = state.comfyMode;
  }
  el.civitaiToken.value = settings.civitai_token || "";
  if (el.addonPinnedMemory) {
    el.addonPinnedMemory.checked = settings.comfyui_pinned_memory_enabled !== false;
  }
  if (el.flagSageAttention) {
    el.flagSageAttention.checked = (
      settings.comfyui_attention_backend === "sage"
      || settings.comfyui_attention_backend === "sage3"
    );
  }
  if (el.flagFlashAttention) {
    el.flagFlashAttention.checked = settings.comfyui_attention_backend === "flash";
  }
  if (el.launchListen) {
    el.launchListen.checked = settings.comfyui_listen_enabled === true;
  }
  if (el.flagLowvram) {
    el.flagLowvram.checked = settings.comfyui_lowvram_enabled === true;
  }
  if (el.flagBf16Unet) {
    el.flagBf16Unet.checked = settings.comfyui_bf16_unet_enabled === true;
  }
  if (el.flagAsyncOffload) {
    el.flagAsyncOffload.checked = settings.comfyui_async_offload_enabled === true;
  }
  if (el.flagDisableSmartMemory) {
    el.flagDisableSmartMemory.checked = settings.comfyui_disable_smart_memory_enabled === true;
  }
  if (el.comfyCustomLaunchArgs) {
    el.comfyCustomLaunchArgs.value = String(settings.comfyui_custom_launch_args || "");
  }
  if (el.comfyShowRuntimeLogs) {
    el.comfyShowRuntimeLogs.checked = settings.comfyui_show_runtime_logs !== false;
  }
  if (el.enableHfXet) {
    el.enableHfXet.checked = settings.hf_xet_enabled === true;
  }
  setComfyQuickActions(settings.comfyui_last_install_dir || "", settings.comfyui_root || "");
  applyComfyTorchProfileOptions();
  const savedTorchProfile = String(settings.comfyui_torch_profile || "").trim();
  if (
    savedTorchProfile
    && comfyTorchProfiles.some((x) => x.value === savedTorchProfile)
    && state.comfyDetectedGpuVendor !== "amd"
    && state.comfyDetectedGpuVendor !== "intel"
  ) {
    el.comfyTorchProfile.value = savedTorchProfile;
    state.comfyTorchProfileLocked = true;
  }

  const refreshRecommendation = (attempt = 0) => {
    invoke("get_comfyui_install_recommendation")
      .then((reco) => {
        const recoReason = String(reco.reason || "").toLowerCase();
        state.comfyTorchRecommendedBase = recoReason.includes("supported amd gpu")
          ? "Detected supported AMD GPU. Auto-selected 'Torch 2.9.1 + ROCm SDK 7.2'."
          : (recoReason.includes("detected amd gpu")
            ? "Detected AMD GPU. Windows ROCm support is limited to specific Radeon and Ryzen AI hardware."
            : (recoReason.includes("detected intel gpu")
              ? "Detected Intel GPU. Auto-selected 'PyTorch XPU Nightly'."
              : `Recommended '${reco.torch_label}' for your GPU`));
        setTorchRecommendedDetecting(false);
        state.comfySage3Eligible = String(reco.gpu_name || "").toLowerCase().includes("rtx 50");
        if (
          comfyTorchProfiles.some((x) => x.value === reco.torch_profile)
          && ((state.comfyDetectedGpuVendor === "amd" || state.comfyDetectedGpuVendor === "intel") || !state.comfyTorchProfileLocked)
        ) {
          applyComfyTorchProfileOptions(reco.torch_profile);
          if (state.comfyDetectedGpuVendor !== "amd" && state.comfyDetectedGpuVendor !== "intel") {
            el.comfyTorchProfile.value = reco.torch_profile;
          }
        }
        applyComfyAddonRules();
        if (!reco.gpu_name && attempt < 8) {
          setTimeout(() => refreshRecommendation(attempt + 1), 600);
        }
      })
      .catch((err) => {
        state.comfyTorchRecommendedBase = "Recommended 'Torch 2.8.0 + cu128' for your GPU";
        setTorchRecommendedDetecting(false);
        if (state.comfyDetectedGpuVendor === "amd") {
          applyComfyTorchProfileOptions("torch291_rocm72");
          el.comfyTorchProfile.value = "torch291_rocm72";
        } else if (state.comfyDetectedGpuVendor === "intel") {
          applyComfyTorchProfileOptions("torchxpu_nightly");
          el.comfyTorchProfile.value = "torchxpu_nightly";
        } else if (!state.comfyTorchProfileLocked) {
          el.comfyTorchProfile.value = "torch280_cu128";
        }
        state.comfySage3Eligible = false;
        applyComfyAddonRules();
        logComfyLine(`Recommendation detection failed: ${err}`);
      });
  };
  refreshRecommendation();

  const initialInstallRoot = String(el.comfyInstallRoot?.value || "").trim();
  if (initialInstallRoot) {
    setStartupStatus("Running startup preflight checks...");
    setTimeout(() => {
      runComfyPreflight().catch(() => {});
    }, 0);
  } else {
    renderPreflight(null);
  }
  setStartupStatus("Scanning ComfyUI installations...");
  if (settings.comfyui_install_base) {
    let effectiveBase = normalizeSlashes(settings.comfyui_install_base);
    el.comfyInstallRoot.value = effectiveBase;
    await invoke("set_comfyui_install_base", { comfyuiInstallBase: effectiveBase }).catch(() => {});
    try {
      const inspection = await invoke("inspect_comfyui_path", { path: effectiveBase });
      const selectedNorm = normalizeSlashes(inspection?.selected || effectiveBase);
      const detectedNorm = normalizeSlashes(inspection?.detected_root || "");
      const leaf = selectedNorm.split("\\").pop() || "";
      const looksLikeComfyInstall = /^comfyui(?:-\d+)?$/i.test(leaf);
      if (
        looksLikeComfyInstall &&
        detectedNorm &&
        normalizeSlashes(detectedNorm) === selectedNorm
      ) {
        const parent = parentDir(selectedNorm);
        if (parent && parent !== selectedNorm) {
          effectiveBase = normalizeSlashes(parent);
          el.comfyInstallRoot.value = effectiveBase;
          await invoke("set_comfyui_install_base", { comfyuiInstallBase: effectiveBase }).catch(() => {});
          logComfyLine(`Adjusted install base to parent folder: ${effectiveBase}`);
        }
      }
    } catch (_) {}
    await syncComfyInstallSelection(effectiveBase, false, true, false);
  } else if (settings.comfyui_root) {
    const inferredBase = parentDir(settings.comfyui_root);
    el.comfyInstallRoot.value = inferredBase;
    await invoke("set_comfyui_install_base", { comfyuiInstallBase: inferredBase }).catch(() => {});
    await refreshExistingInstallations(inferredBase, null);
  } else {
    await refreshExistingInstallations("", null);
  }
  await refreshComfyResumeState();
  setStartupStatus("Checking ComfyUI runtime status...");
  await refreshComfyRuntimeStatus();
  updateComfyModeUi();
  setTimeout(() => {
    loadInstalledAddonState(el.comfyRoot.value || "").catch(() => {});
  }, 0);

  setOptions(el.modelFamily, familyOptions(catalog.models));
  setOptions(el.vramTier, vramOptions.map((v) => ({ value: v.id, label: v.label })), "tier_s");
  setOptions(el.ramTier, ramOptions.map((r) => ({ value: r.id, label: r.label })), "tier_a");
  renderModelSelectionList();
  await refreshEffectiveDownloadDestination();

  setOptions(el.loraFamily, loraFamilyOptions(catalog.loras));
  refreshLoraSelectors();
  setTimeout(() => {
    loadLoraMetadata().catch(() => {});
  }, 0);

  setOptions(el.workflowFamily, workflowFamilyOptions(catalog.workflows || []));
  refreshWorkflowSelectors();

  logLine(`Loaded ${catalog.models?.length || 0} models, ${catalog.loras?.length || 0} LoRAs, and ${catalog.workflows?.length || 0} workflows.`);
  try {
    setStartupStatus("Checking downloader acceleration...");
    const xet = await invoke("get_hf_xet_preflight");
    if (xet?.detail) {
      logLine(xet.detail);
    }
  } catch (_) {}
  setStartupStatus("Starting UI...");
}

el.tabComfyui.addEventListener("click", () => switchTab("comfyui"));
el.tabModels.addEventListener("click", () => switchTab("models"));
el.tabLoras.addEventListener("click", () => switchTab("loras"));
el.tabWorkflows.addEventListener("click", () => switchTab("workflows"));

el.modelFamily.addEventListener("change", renderModelSelectionList);
el.vramTier.addEventListener("change", renderModelSelectionList);
el.modelSearch?.addEventListener("input", renderModelSelectionList);
el.selectVisibleModels?.addEventListener("click", () => {
  filteredModelsForCurrentSelection().forEach((model) => {
    const variants = variantsForModelAndTier(model, el.vramTier.value);
    if (variants[0]?.id) {
      state.selectedModelVariants.set(model.id, state.selectedModelVariants.get(model.id) || variants[0].id);
    }
  });
  renderModelSelectionList();
});
el.clearModelSelection?.addEventListener("click", () => {
  state.selectedModelVariants.clear();
  renderModelSelectionList();
});

el.loraFamily.addEventListener("change", () => {
  refreshLoraSelectors();
  loadLoraMetadata().catch((err) => logLine(String(err)));
});
el.loraId.addEventListener("change", () => {
  loadLoraMetadata().catch((err) => logLine(String(err)));
});
el.workflowFamily.addEventListener("change", refreshWorkflowSelectors);
el.workflowId.addEventListener("change", loadWorkflowPreview);

el.saveRoot.addEventListener("click", async () => {
  try {
    await invoke("set_comfyui_root", { comfyuiRoot: el.comfyRoot.value });
    el.comfyRootLora.value = el.comfyRoot.value;
    if (el.comfyRootWorkflow) {
      el.comfyRootWorkflow.value = el.comfyRoot.value;
    }
    await loadInstalledAddonState(el.comfyRoot.value);
    await refreshEffectiveDownloadDestination();
    const original = el.saveRoot.textContent;
    el.saveRoot.textContent = "Saved";
    el.saveRoot.disabled = true;
    window.setTimeout(() => {
      el.saveRoot.textContent = original || "Save Folder";
      el.saveRoot.disabled = false;
    }, 900);
  } catch (err) {
    logLine(`Save folder failed: ${err}`);
  }
});

el.chooseRoot.addEventListener("click", async () => {
  try {
    const selected = await invoke("pick_folder");
    if (!selected) return;
    el.comfyRoot.value = selected;
    await invoke("set_comfyui_root", { comfyuiRoot: selected });
    el.comfyRootLora.value = selected;
    if (el.comfyRootWorkflow) {
      el.comfyRootWorkflow.value = selected;
    }
    logLine("ComfyUI folder selected.");
    await loadInstalledAddonState(selected);
    await refreshEffectiveDownloadDestination();
  } catch (err) {
    logLine(`Choose folder failed: ${err}`);
  }
});

el.comfyRoot?.addEventListener("input", () => {
  refreshEffectiveDownloadDestination().catch(() => {});
});

el.saveRootLora.addEventListener("click", async () => {
  try {
    await invoke("set_comfyui_root", { comfyuiRoot: el.comfyRootLora.value });
    el.comfyRoot.value = el.comfyRootLora.value;
    if (el.comfyRootWorkflow) {
      el.comfyRootWorkflow.value = el.comfyRootLora.value;
    }
    await loadInstalledAddonState(el.comfyRoot.value);
    const original = el.saveRootLora.textContent;
    el.saveRootLora.textContent = "Saved";
    el.saveRootLora.disabled = true;
    window.setTimeout(() => {
      el.saveRootLora.textContent = original || "Save Folder";
      el.saveRootLora.disabled = false;
    }, 900);
  } catch (err) {
    logLine(`Save folder failed: ${err}`);
  }
});

el.chooseRootLora.addEventListener("click", async () => {
  try {
    const selected = await invoke("pick_folder");
    if (!selected) return;
    el.comfyRootLora.value = selected;
    await invoke("set_comfyui_root", { comfyuiRoot: selected });
    el.comfyRoot.value = selected;
    if (el.comfyRootWorkflow) {
      el.comfyRootWorkflow.value = selected;
    }
    logLine("ComfyUI folder selected.");
    await loadInstalledAddonState(selected);
  } catch (err) {
    logLine(`Choose folder failed: ${err}`);
  }
});

el.saveRootWorkflow?.addEventListener("click", async () => {
  try {
    await invoke("set_comfyui_root", { comfyuiRoot: el.comfyRootWorkflow.value });
    el.comfyRoot.value = el.comfyRootWorkflow.value;
    el.comfyRootLora.value = el.comfyRootWorkflow.value;
    await loadInstalledAddonState(el.comfyRoot.value);
    const original = el.saveRootWorkflow.textContent;
    el.saveRootWorkflow.textContent = "Saved";
    el.saveRootWorkflow.disabled = true;
    window.setTimeout(() => {
      el.saveRootWorkflow.textContent = original || "Save Folder";
      el.saveRootWorkflow.disabled = false;
    }, 900);
  } catch (err) {
    logLine(`Save folder failed: ${err}`);
  }
});

el.chooseRootWorkflow?.addEventListener("click", async () => {
  try {
    const selected = await invoke("pick_folder");
    if (!selected) return;
    el.comfyRootWorkflow.value = selected;
    await invoke("set_comfyui_root", { comfyuiRoot: selected });
    el.comfyRoot.value = selected;
    el.comfyRootLora.value = selected;
    logLine("ComfyUI folder selected.");
    await loadInstalledAddonState(selected);
  } catch (err) {
    logLine(`Choose folder failed: ${err}`);
  }
});

el.saveInstallRoot.addEventListener("click", async () => {
  try {
    await syncComfyInstallSelection(el.comfyInstallRoot.value, true);
    const original = el.saveInstallRoot.textContent;
    el.saveInstallRoot.textContent = "Saved";
    el.saveInstallRoot.disabled = true;
    window.setTimeout(() => {
      el.saveInstallRoot.textContent = original || "Save Base";
      el.saveInstallRoot.disabled = false;
    }, 900);
    await refreshComfyResumeState();
  } catch (err) {
    logComfyLine(`Save folder failed: ${err}`);
  }
});

el.chooseInstallRoot.addEventListener("click", async () => {
  try {
    const selected = await invoke("pick_folder");
    if (!selected) return;
    await syncComfyInstallSelection(selected, true);
    logComfyLine("ComfyUI install folder selected.");
    await refreshComfyResumeState();
  } catch (err) {
    logComfyLine(`Choose install folder failed: ${err}`);
  }
});

el.chooseExtraModelRoot?.addEventListener("click", async () => {
  try {
    const selected = await invoke("pick_folder");
    if (!selected) return;
    if (el.comfyExtraModelRoot) {
      el.comfyExtraModelRoot.value = selected;
    }
    if (el.comfyExtraModelDefault) {
      el.comfyExtraModelDefault.checked = true;
    }
    state.sharedModelsRootDefault = String(selected || "").trim();
    state.sharedModelsUseDefault = true;
    if (state.comfyMode === "manage") {
      await persistComfyExtraModelConfigForRoot(el.comfyExistingInstall?.value || el.comfyRoot.value);
    }
    await refreshEffectiveDownloadDestination();
    logComfyLine("Optional extra models folder selected.");
  } catch (err) {
    logComfyLine(`Choose extra models folder failed: ${err}`);
  }
});

el.clearExtraModelRoot?.addEventListener("click", async () => {
  if (el.comfyExtraModelRoot) {
    el.comfyExtraModelRoot.value = "";
  }
  if (el.comfyExtraModelDefault) {
    el.comfyExtraModelDefault.checked = false;
  }
  state.sharedModelsRootDefault = "";
  state.sharedModelsUseDefault = false;
  if (state.comfyMode === "manage") {
    await persistComfyExtraModelConfigForRoot(el.comfyExistingInstall?.value || el.comfyRoot.value);
  }
  await refreshEffectiveDownloadDestination();
  logComfyLine("Optional extra models folder cleared.");
});

el.comfyExtraModelDefault?.addEventListener("change", async () => {
  const hasRoot = Boolean(String(el.comfyExtraModelRoot?.value || "").trim());
  if (!hasRoot && el.comfyExtraModelDefault?.checked) {
    el.comfyExtraModelDefault.checked = false;
    return;
  }
  state.sharedModelsRootDefault = String(el.comfyExtraModelRoot?.value || "").trim();
  state.sharedModelsUseDefault = Boolean(el.comfyExtraModelDefault?.checked && hasRoot);
  if (state.comfyMode === "manage") {
    await persistComfyExtraModelConfigForRoot(el.comfyExistingInstall?.value || el.comfyRoot.value);
  }
  await refreshEffectiveDownloadDestination();
});

el.comfyExtraModelRoot?.addEventListener("change", async () => {
  const rootValue = String(el.comfyExtraModelRoot?.value || "").trim();
  if (!rootValue && el.comfyExtraModelDefault?.checked) {
    el.comfyExtraModelDefault.checked = false;
  }
  state.sharedModelsRootDefault = rootValue;
  state.sharedModelsUseDefault = Boolean(el.comfyExtraModelDefault?.checked && rootValue);
  if (state.comfyMode === "manage") {
    await persistComfyExtraModelConfigForRoot(el.comfyExistingInstall?.value || el.comfyRoot.value);
  }
  await refreshEffectiveDownloadDestination();
});

el.comfyMode?.addEventListener("change", async () => {
  state.comfyMode = el.comfyMode.value === "manage" ? "manage" : "install";
  if (state.comfyMode !== "manage") {
    resetComfySelectionsToDefaults();
    const savedTorchProfile = String(state.settings?.comfyui_torch_profile || "").trim();
    if (state.comfyDetectedGpuVendor === "amd") {
      state.comfyTorchProfileLocked = false;
      applyComfyTorchProfileOptions("torch291_rocm72");
      el.comfyTorchProfile.value = "torch291_rocm72";
    } else if (state.comfyDetectedGpuVendor === "intel") {
      state.comfyTorchProfileLocked = false;
      applyComfyTorchProfileOptions("torchxpu_nightly");
      el.comfyTorchProfile.value = "torchxpu_nightly";
    } else {
      applyComfyTorchProfileOptions(savedTorchProfile || "torch280_cu128");
      if (savedTorchProfile && comfyTorchProfiles.some((x) => x.value === savedTorchProfile)) {
        el.comfyTorchProfile.value = savedTorchProfile;
        state.comfyTorchProfileLocked = true;
      } else {
        state.comfyTorchProfileLocked = false;
      }
    }
    applyComfyAddonRules();
    if (el.comfyExtraModelRoot) {
      el.comfyExtraModelRoot.value = state.sharedModelsRootDefault || "";
    }
    if (el.comfyExtraModelDefault) {
      el.comfyExtraModelDefault.checked = state.sharedModelsRootDefault
        ? Boolean(state.sharedModelsUseDefault)
        : false;
    }
  } else {
    try {
      const installs = await refreshExistingInstallations(el.comfyInstallRoot?.value || "", null);
      const latest = newestComfyInstall(installs);
      const selectedRoot = String(latest?.root || el.comfyExistingInstall?.value || el.comfyRoot.value || "").trim();
      if (selectedRoot) {
        if (el.comfyExistingInstall) {
          el.comfyExistingInstall.value = selectedRoot;
        }
        await applySelectedExistingInstallation(selectedRoot);
      } else {
        await loadInstalledAddonState(el.comfyRoot.value || "");
      }
    } catch (_) {
      loadInstalledAddonState(el.comfyRoot.value || "").catch(() => {});
    }
  }
  updateComfyModeUi();
});

el.useExistingInstall?.addEventListener("click", async () => {
  const selectedRoot = String(el.comfyExistingInstall?.value || "").trim();
  if (!selectedRoot) {
    logComfyLine("No existing ComfyUI installation selected.");
    return;
  }
  try {
    await applySelectedExistingInstallation(selectedRoot);
    state.comfyMode = "manage";
    if (el.comfyMode) el.comfyMode.value = "manage";
    updateComfyModeUi();
    logComfyLine(`Now managing: ${selectedRoot}`);
  } catch (err) {
    logComfyLine(`Failed to use selected installation: ${err}`);
  }
});

el.comfyExistingInstall?.addEventListener("change", async () => {
  updateComfyModeUi();
  const selectedRoot = String(el.comfyExistingInstall?.value || "").trim();
  if (!selectedRoot) {
    refreshComfyUiUpdateStatus("").catch(() => {});
    return;
  }
  const previousRoot = String(el.comfyRoot?.value || "").trim();
  const switchingInstall = previousRoot
    && normalizeSlashes(previousRoot) !== normalizeSlashes(selectedRoot);
  try {
    if (state.comfyMode === "manage" && switchingInstall) {
      await refreshComfyRuntimeStatus().catch(() => {});
      if (state.comfyRuntimeRunning) {
        logComfyLine("ComfyUI server is running. Stopping it before switching managed install...");
        await invoke("stop_comfyui_root");
        await refreshComfyRuntimeStatus().catch(() => {});
        if (state.comfyRuntimeRunning) {
          logComfyLine("ComfyUI is still running. Stop it first, then switch install.");
          return;
        }
        logComfyLine("ComfyUI server stopped.");
      }
    }
    await applySelectedExistingInstallation(selectedRoot);
    if (state.comfyMode === "manage") {
      logComfyLine(`Now managing: ${selectedRoot}`);
    }
  } catch (err) {
    logComfyLine(`Failed to load selected installation: ${err}`);
  }
});

el.updateSelectedInstall?.addEventListener("click", async () => {
  const selectedRoot = String(el.comfyExistingInstall?.value || "").trim();
  if (!selectedRoot) {
    logComfyLine("No existing ComfyUI installation selected.");
    return;
  }
  if (state.comfyUpdateBusy) return;
  if (!state.comfyUpdateChecked) {
    await refreshComfyUiUpdateStatus(selectedRoot);
    return;
  }
  if (!state.comfyUpdateAvailable) {
    return;
  }
  try {
    state.comfyUpdateBusy = true;
    updateComfyUpdateButton();
    logComfyLine("Updating ComfyUI...");
    const result = await invoke("update_selected_comfyui", { comfyuiRoot: selectedRoot });
    if (result) {
      logComfyLine(String(result));
    }
    comfyUpdateStatusCache.delete(normalizeSlashes(selectedRoot));
    await refreshComfyUiUpdateStatus(selectedRoot);
    await loadInstalledAddonState(selectedRoot);
  } catch (err) {
    logComfyLine(`ComfyUI update failed: ${err}`);
  } finally {
    state.comfyUpdateBusy = false;
    updateComfyUpdateButton();
  }
});

el.installComfyui.addEventListener("click", async () => {
  await startComfyInstall(false);
});

el.addonSageAttention?.addEventListener("change", () => {
  applyAttentionBackendFromToggle(el.addonSageAttention).catch((err) => logComfyLine(String(err)));
});
el.addonSageAttention3?.addEventListener("change", () => {
  applyAttentionBackendFromToggle(el.addonSageAttention3).catch((err) => logComfyLine(String(err)));
});
el.addonFlashAttention?.addEventListener("change", () => {
  applyAttentionBackendFromToggle(el.addonFlashAttention).catch((err) => logComfyLine(String(err)));
});
el.flagSageAttention?.addEventListener("change", () => {
  applyLaunchAttentionFlagFromToggle(el.flagSageAttention).catch((err) => logComfyLine(String(err)));
});
el.flagFlashAttention?.addEventListener("change", () => {
  applyLaunchAttentionFlagFromToggle(el.flagFlashAttention).catch((err) => logComfyLine(String(err)));
});
el.addonNunchaku?.addEventListener("change", () => {
  applyComfyAddonRules();
  applyAttentionBackendFromToggle(el.addonNunchaku).catch((err) => logComfyLine(String(err)));
});
el.addonInsightFace?.addEventListener("change", () => {
  applyComponentToggleFromCheckbox(el.addonInsightFace, "addon_insightface", "InsightFace")
    .catch((err) => logComfyLine(String(err)));
});
el.addonTrellis2?.addEventListener("change", () => {
  applyComponentToggleFromCheckbox(el.addonTrellis2, "addon_trellis2", "Trellis2")
    .catch((err) => logComfyLine(String(err)));
});
el.addonPinnedMemory?.addEventListener("change", () => {
  applyComponentToggleFromCheckbox(el.addonPinnedMemory, "addon_pinned_memory", "Pinned Memory")
    .catch((err) => logComfyLine(String(err)));
});
el.launchListen?.addEventListener("change", () => {
  applyComponentToggleFromCheckbox(el.launchListen, "launch_listen", "--listen")
    .catch((err) => logComfyLine(String(err)));
});
el.flagLowvram?.addEventListener("change", () => {
  applyComponentToggleFromCheckbox(el.flagLowvram, "launch_lowvram", "--lowvram")
    .catch((err) => logComfyLine(String(err)));
});
el.flagBf16Unet?.addEventListener("change", () => {
  applyComponentToggleFromCheckbox(el.flagBf16Unet, "launch_bf16_unet", "--bf16-unet")
    .catch((err) => logComfyLine(String(err)));
});
el.flagAsyncOffload?.addEventListener("change", () => {
  applyComponentToggleFromCheckbox(el.flagAsyncOffload, "launch_async_offload", "--async-offload")
    .catch((err) => logComfyLine(String(err)));
});
el.flagDisableSmartMemory?.addEventListener("change", () => {
  applyComponentToggleFromCheckbox(el.flagDisableSmartMemory, "launch_disable_smart_memory", "--disable-smart-memory")
    .catch((err) => logComfyLine(String(err)));
});
el.saveComfyCustomLaunchArgs?.addEventListener("click", async () => {
  try {
    state.settings = await invoke("set_comfyui_custom_launch_args", {
      customLaunchArgs: String(el.comfyCustomLaunchArgs?.value || ""),
    });
    if (el.comfyCustomLaunchArgs) {
      el.comfyCustomLaunchArgs.value = String(state.settings?.comfyui_custom_launch_args || "");
    }
    logComfyLine("Custom launch args saved.");
  } catch (err) {
    logComfyLine(`Saving custom launch args failed: ${err}`);
  }
});
el.clearComfyCustomLaunchArgs?.addEventListener("click", async () => {
  try {
    state.settings = await invoke("set_comfyui_custom_launch_args", { customLaunchArgs: "" });
    if (el.comfyCustomLaunchArgs) {
      el.comfyCustomLaunchArgs.value = "";
    }
    logComfyLine("Custom launch args cleared.");
  } catch (err) {
    logComfyLine(`Clearing custom launch args failed: ${err}`);
  }
});
el.comfyShowRuntimeLogs?.addEventListener("change", async () => {
  const enabled = el.comfyShowRuntimeLogs.checked;
  try {
    state.settings = await invoke("set_comfyui_show_runtime_logs", { enabled });
    el.comfyShowRuntimeLogs.checked = state.settings?.comfyui_show_runtime_logs !== false;
    logComfyLine(el.comfyShowRuntimeLogs.checked ? "Runtime log streaming enabled." : "Runtime log streaming disabled.");
  } catch (err) {
    el.comfyShowRuntimeLogs.checked = !enabled;
    logComfyLine(`Runtime log toggle failed: ${err}`);
  }
});
el.nodeComfyuiManager?.addEventListener("change", () => {
  applyComponentToggleFromCheckbox(el.nodeComfyuiManager, "node_comfyui_manager", "comfyui-manager")
    .catch((err) => logComfyLine(String(err)));
});
el.nodeComfyuiEasyUse?.addEventListener("change", () => {
  applyComponentToggleFromCheckbox(el.nodeComfyuiEasyUse, "node_comfyui_easy_use", "ComfyUI-Easy-Use")
    .catch((err) => logComfyLine(String(err)));
});
el.nodeRgthreeComfy?.addEventListener("change", () => {
  applyComponentToggleFromCheckbox(el.nodeRgthreeComfy, "node_rgthree_comfy", "rgthree-comfy")
    .catch((err) => logComfyLine(String(err)));
});
el.nodeComfyuiGguf?.addEventListener("change", () => {
  applyComponentToggleFromCheckbox(el.nodeComfyuiGguf, "node_comfyui_gguf", "ComfyUI-GGUF")
    .catch((err) => logComfyLine(String(err)));
});
el.nodeComfyuiKjnodes?.addEventListener("change", () => {
  applyComponentToggleFromCheckbox(el.nodeComfyuiKjnodes, "node_comfyui_kjnodes", "comfyui-kjnodes")
    .catch((err) => logComfyLine(String(err)));
});
el.nodeComfyuiCrystools?.addEventListener("change", () => {
  applyComponentToggleFromCheckbox(el.nodeComfyuiCrystools, "node_comfyui_crystools", "comfyui-crystools")
    .catch((err) => logComfyLine(String(err)));
});
el.comfyTorchProfile?.addEventListener("change", () => {
  if (state.comfyDetectedGpuVendor !== "amd" && state.comfyDetectedGpuVendor !== "intel") {
    state.comfyTorchProfileLocked = true;
  }
  applyComfyAddonRules();
});
el.runPreflight?.addEventListener("click", () => {
  runComfyPreflight().then((result) => {
    if (!result) return;
    logComfyLine(result.summary || "Preflight completed.");
  });
});
el.comfyResumeBtn?.addEventListener("click", async () => {
  await startComfyInstall(false);
});
el.comfyFreshBtn?.addEventListener("click", async () => {
  await startComfyInstall(true);
});

el.comfyClearInstallLog?.addEventListener("click", () => {
  if (el.comfyInstallLog) el.comfyInstallLog.textContent = "Ready";
});
el.comfyClearRuntimeLog?.addEventListener("click", () => {
  state.comfyRuntimeLogs = [];
  renderComfyRuntimeLogs();
});
el.comfyRuntimeLogFilter?.addEventListener("change", () => {
  renderComfyRuntimeLogs();
});

el.clearStatusLog?.addEventListener("click", () => {
  if (el.statusLog) el.statusLog.textContent = "Ready";
});

el.comfyOpenInstallFolder?.addEventListener("click", async () => {
  const path = String(el.comfyOpenInstallFolder.dataset.path || "").trim();
  if (!path) return;
  try {
    await invoke("open_folder", { path });
  } catch (err) {
    logComfyLine(`Open install folder failed: ${err}`);
  }
});

el.comfyStartInstalled?.addEventListener("click", async () => {
  const preferredManageRoot = state.comfyMode === "manage"
    ? String(el.comfyExistingInstall?.value || "").trim()
    : "";
  const path = String(preferredManageRoot || el.comfyStartInstalled.dataset.path || "").trim();
  if (!path) return;
  try {
    if (state.comfyRuntimeRunning) {
      state.comfyRuntimeStarting = false;
      state.comfyRuntimeTarget = "";
      updateComfyRuntimeButton();
      const stopped = await invoke("stop_comfyui_root");
      logComfyLine(stopped ? "ComfyUI stop requested." : "ComfyUI was not running.");
      await refreshComfyRuntimeStatus();
    } else {
      state.comfyRuntimeTarget = comfyInstallNameFromRoot(path);
      state.comfyRuntimeStarting = true;
      state.comfyRuntimeRunning = false;
      updateComfyRuntimeButton();
      await invoke("start_comfyui_root", { comfyuiRoot: path });
      logComfyLine("ComfyUI launch requested.");
    }
  } catch (err) {
    state.comfyRuntimeStarting = false;
    state.comfyRuntimeRunning = false;
    state.comfyRuntimeTarget = "";
    updateComfyRuntimeButton();
    logComfyLine(`ComfyUI runtime action failed: ${err}`);
  }
});

el.saveToken.addEventListener("click", async () => {
  try {
    await invoke("save_civitai_token", { token: el.civitaiToken.value });
    const original = el.saveToken.textContent;
    el.saveToken.textContent = "Saved";
    el.saveToken.disabled = true;
    window.setTimeout(() => {
      el.saveToken.textContent = original || "Save Token";
      el.saveToken.disabled = false;
    }, 900);
    await loadLoraMetadata();
  } catch (err) {
    logLine(`Save token failed: ${err}`);
  }
});

el.checkUpdates.addEventListener("click", async () => {
  if (state.updateInstalling || state.updateChecking) return;
  if (state.updateAvailable) {
    try {
      state.updateInstalling = true;
      updateUpdateButton();
      await invoke("auto_update_startup");
    } catch (err) {
      state.updateInstalling = false;
      updateUpdateButton();
      flashUpdateButton("Check Failed", 3200);
      logLine(String(err));
    }
    return;
  }
  try {
    state.updateChecking = true;
    updateUpdateButton();
    const startedAt = Date.now();
    const result = await invoke("check_updates_now");
    const checkingElapsed = Date.now() - startedAt;
    if (checkingElapsed < 700) {
      await sleep(700 - checkingElapsed);
    }
    state.updateChecking = false;
    if (result.available) {
      state.updateAvailable = true;
      state.updateVersion = result.version || null;
      state.updateFlashLabel = "";
      if (state.updateFlashTimer) {
        window.clearTimeout(state.updateFlashTimer);
        state.updateFlashTimer = null;
      }
      updateUpdateButton();
      logLine(`Update available: v${result.version}`);
    } else {
      state.updateAvailable = false;
      state.updateVersion = null;
      flashUpdateButton("No Update", 4000);
      logLine("No updates available.");
    }
  } catch (err) {
    state.updateChecking = false;
    state.updateAvailable = false;
    state.updateVersion = null;
    state.updateInstalling = false;
    flashUpdateButton("Check Failed", 4000);
    updateUpdateButton();
    logLine(String(err));
  }
});

el.metaCreatorLink.addEventListener("click", async (event) => {
  const href = el.metaCreatorLink.getAttribute("href") || "";
  if (!href || href === "#") {
    event.preventDefault();
    return;
  }
  event.preventDefault();
  try {
    await invoke("open_external_url", { url: href });
  } catch (err) {
    logLine(`Open owner link failed: ${err}`);
  }
});

el.workflowYoutubeLink?.addEventListener("click", async (event) => {
  const href = el.workflowYoutubeLink.getAttribute("href") || "";
  if (!href || href === "#") {
    event.preventDefault();
    return;
  }
  event.preventDefault();
  try {
    await invoke("open_external_url", { url: href });
  } catch (err) {
    logLine(`Open workflow tutorial link failed: ${err}`);
  }
});

document.querySelectorAll(".footer-link[data-url]").forEach((button) => {
  button.addEventListener("click", async () => {
    const url = button.getAttribute("data-url");
    if (!url) return;
    try {
      await invoke("open_external_url", { url });
    } catch (err) {
      logLine(`Open link failed: ${err}`);
    }
  });
});

async function initEventListeners() {
  if (!listen) {
    logLine("Tauri event bridge unavailable.");
    return;
  }
  try {
    await listen("download-progress", (event) => {
    const p = event.payload || {};
    if (p.phase === "cancelled") {
      logLine(`[${p.kind}] cancelled.`);
      setProgress(`[${p.kind}] cancelled`);
      state.transfers.clear();
      renderTransfers();
      endBusyDownload();
      return;
    }
    if (p.phase === "batch_finished") {
      if (p.kind !== "lora") {
        logLine(p.message || `[${p.kind}] download batch completed.`);
      }
      setProgress("Idle");
      renderTransfers();
      endBusyDownload();
      return;
    }
    if (p.phase === "batch_failed") {
      logLine(p.message || `[${p.kind}] download batch failed.`);
      setProgress(`[${p.kind}] failed`);
      renderTransfers();
      endBusyDownload();
      return;
    }

    const key = `${p.kind || "download"}:${p.index || "?"}:${p.artifact || "item"}`;
    const current = state.transfers.get(key) || {
      id: key,
      kind: p.kind || "download",
      artifact: p.artifact || "artifact",
      phase: "started",
      received: 0,
      size: Number(p.size || 0),
      folder: "",
    };
    current.phase = p.phase || current.phase;
    if (p.kind) current.kind = p.kind;
    current.lastUpdateTs = Date.now();
    if (p.artifact) current.artifact = p.artifact;
    if (p.received != null) {
      const nextReceived = Number(p.received || 0);
      const prevReceived = Number(current.received || 0);
      current.received = Number.isFinite(nextReceived)
        ? Math.max(prevReceived, nextReceived)
        : prevReceived;
    }
    if (p.phase === "started") {
      current.displayReceived = 0;
      current.displayTs = Date.now();
    }
    if (p.size != null) current.size = Number(p.size);
    if (typeof p.folder === "string" && p.folder.trim()) current.folder = p.folder.trim();
    state.transfers.set(key, current);

    if (p.phase === "started") {
      setProgress(`[${p.kind}] ${p.index || "?"}/${p.total || "?"} ${p.artifact || ""}`);
    } else if (p.phase === "progress") {
      ensureProgressSmoother();
    } else if (p.phase === "failed") {
      setProgress(`[${p.kind}] failed: ${p.message || "unknown error"}`);
      logLine(`[${p.kind}] ${p.artifact || "download"} failed: ${p.message || "unknown error"}`);
      current.phase = "failed";
      state.transfers.delete(key);
      endBusyDownload();
    } else if (p.phase === "finished") {
      setProgress(`[${p.kind}] finished: ${current.artifact || "file"}`);
      current.phase = "finished";
      addCompleted({
        name: current.artifact || "downloaded file",
        folder: current.folder || "",
        status: "downloaded",
      });
      state.transfers.delete(key);
      renderCompletedTransfers();
    }
    renderActiveTransfers();
    renderOverallProgress();
    });

    await listen("update-state", (event) => {
    const p = event.payload || {};
    if (p.message) {
      logLine(p.message);
      if (p.phase === "available") {
        state.updateAvailable = true;
        state.updateVersion = p.version || state.updateVersion || null;
        updateUpdateButton();
        el.updateStatus.textContent = "New update available";
      } else if (p.phase === "restarting") {
        state.updateInstalling = true;
        updateUpdateButton();
        el.updateStatus.textContent = "Installing update...";
      } else {
        el.updateStatus.textContent = `${p.phase}`;
      }
    }
    });

    await listen("comfyui-install-progress", (event) => {
      const p = event.payload || {};
      const message = String(p.message || "").trim();
      if (message) {
        logComfyLine(message);
      }
      if (p.phase === "failed") {
        state.comfyInstallBusy = false;
        updateComfyInstallButton();
        return;
      }
      if (p.phase === "finished") {
        state.comfyInstallBusy = false;
        updateComfyInstallButton();
        el.comfyResumeBanner?.classList.add("hidden");
        if (typeof p.folder === "string" && p.folder.trim()) {
          const installedRoot = normalizeSlashes(p.folder.trim());
          const emittedBase = normalizeSlashes(String(p.artifact || "").trim());
          const installBase = emittedBase || parentDir(installedRoot) || installedRoot;

          const finalizeInstalledSelection = async () => {
            await syncComfyInstallSelection(installedRoot, true);
            state.comfyMode = "manage";
            if (el.comfyMode) el.comfyMode.value = "manage";
            await refreshExistingInstallations(installBase, installedRoot).catch(() => []);
            await applySelectedExistingInstallation(installedRoot).catch(() => {});
            await refreshComfyUiUpdateStatus(installedRoot).catch(() => {});
            setComfyQuickActions(installBase, installedRoot);
            await refreshComfyRuntimeStatus().catch(() => {});
            updateComfyModeUi();
          };

          finalizeInstalledSelection().catch((err) => {
            logComfyLine(`Failed to finalize installed ComfyUI selection: ${err}`);
            updateComfyModeUi();
          });
        }
        return;
      }
    });

    await listen("comfyui-runtime", (event) => {
      const p = event.payload || {};
      const phase = String(p.phase || "").trim();
      const msg = String(p.message || "").trim();
      if (msg) {
        logComfyLine(msg);
        logLine(msg);
      }
      if (phase === "starting") {
        state.comfyRuntimeStarting = true;
        state.comfyRuntimeRunning = false;
        updateComfyRuntimeButton();
        return;
      }
    if (phase === "started") {
      state.comfyRuntimeTarget = "";
      state.comfyRuntimeStarting = false;
      state.comfyRuntimeRunning = true;
      updateComfyRuntimeButton();
      refreshComfyRuntimeStatus().catch(() => {});
      return;
    }
    if (phase === "restarted_after_changes") {
      openComfyWhenReady().catch(() => {});
      return;
    }
    if (phase === "stopped" || phase === "start_failed" || phase === "stop_failed") {
      state.comfyRuntimeTarget = "";
      state.comfyRuntimeStarting = false;
      state.comfyRuntimeRunning = false;
        updateComfyRuntimeButton();
        refreshComfyRuntimeStatus().catch(() => {});
      }
    });

    await listen("comfyui-runtime-log", (event) => {
      const p = event.payload || {};
      const text = String(p.text || "").trim();
      if (text) {
        logComfyRuntimeLine(text, String(p.stream || "stdout").trim() || "stdout");
      }
    });
  } catch (err) {
    logLine(`Event listener setup failed: ${err}`);
  }
}

el.downloadModel.addEventListener("click", async () => {
  if (state.busyDownloads > 0) {
    await requestCancelDownload();
    return;
  }
  const items = selectedModelItems();
  if (!items.length) {
    logLine("Select at least one model first.");
    return;
  }
  beginBusyDownload("Starting model download...");
  try {
    if (items.length === 1) {
      await invoke("download_model_assets", {
        modelId: items[0].modelId,
        variantId: items[0].variantId,
        ramTier: el.ramTier.value,
        comfyuiRoot: el.comfyRoot.value,
      });
    } else {
      await invoke("download_model_assets_batch", {
        request: {
          items: items.map((item) => ({
            modelId: item.modelId,
            variantId: item.variantId,
          })),
          ramTier: el.ramTier.value,
          comfyuiRoot: el.comfyRoot.value,
        },
      });
    }
    logLine(items.length === 1 ? "Model download started." : `Started ${items.length} model downloads.`);
  } catch (err) {
    logLine(String(err));
    endBusyDownload();
  }
});

if (el.enableHfXet) {
  el.enableHfXet.addEventListener("change", async () => {
    const enabled = !!el.enableHfXet.checked;
    el.enableHfXet.disabled = true;
    try {
      const updated = await invoke("set_hf_xet_enabled", { enabled });
      state.settings = updated;
      el.enableHfXet.checked = updated?.hf_xet_enabled === true;
      if (enabled) {
        logLine("HF Xet experimental mode enabled.");
      } else {
        logLine("HF Xet experimental mode disabled. Using default downloader.");
      }
      try {
        const xet = await invoke("get_hf_xet_preflight");
        if (xet?.detail) logLine(xet.detail);
      } catch (_) {}
    } catch (err) {
      logLine(`HF Xet toggle failed: ${err}`);
      el.enableHfXet.checked = !enabled;
    } finally {
      el.enableHfXet.disabled = false;
    }
  });
}

el.downloadLora.addEventListener("click", async () => {
  if (state.busyDownloads > 0) {
    await requestCancelDownload();
    return;
  }
  if (!el.loraId.value) {
    logLine("Select a LoRA first.");
    return;
  }
  beginBusyDownload("Starting LoRA download...");
  try {
    await invoke("download_lora_asset", {
      loraId: el.loraId.value,
      token: el.civitaiToken.value?.trim() || null,
      comfyuiRoot: el.comfyRootLora.value,
    });
  } catch (err) {
    logLine(String(err));
    endBusyDownload();
  }
});

el.downloadWorkflow?.addEventListener("click", async () => {
  if (state.busyDownloads > 0) {
    await requestCancelDownload();
    return;
  }
  if (!el.workflowId.value) {
    logLine("Select a workflow first.");
    return;
  }
  beginBusyDownload("Starting workflow download...");
  try {
    await invoke("download_workflow_asset", {
      workflowId: el.workflowId.value,
      comfyuiRoot: el.comfyRootWorkflow?.value || el.comfyRoot.value,
    });
    logLine("Workflow download started.");
  } catch (err) {
    logLine(String(err));
    endBusyDownload();
  }
});

switchTab("comfyui");
updateDownloadButtons();
updateComfyInstallButton();
updateComfyRuntimeButton();
updateComfyUpdateButton();
updateUpdateButton();
renderTransfers();

(async () => {
  setStartupStatus("Connecting event listeners...");
  await initEventListeners();
  try {
    setStartupStatus("Preparing workspace...");
    await bootstrap();
    hideStartupOverlay();
    setTimeout(() => {
      invoke("check_updates_now")
        .then((startup) => {
          if (startup?.available === true) {
            state.updateAvailable = true;
            state.updateVersion = startup.version || null;
            el.updateStatus.textContent = "New update available";
            updateUpdateButton();
            logLine(`Update available: v${startup.version}`);
          } else {
            state.updateAvailable = false;
            state.updateVersion = null;
            updateUpdateButton();
          }
        })
        .catch((err) => {
          console.debug("Startup update check skipped:", err);
        });
    }, 0);
  } catch (err) {
    logLine(`Initialization failed: ${err}`);
    setStartupStatus(`Startup error: ${err}`);
    window.setTimeout(() => {
      hideStartupOverlay();
    }, 900);
  }
})();

// Runtime status polling (low-overhead, non-overlapping) to avoid UI hitching.
scheduleRuntimeStatusPoll(1800);

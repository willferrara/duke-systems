import {
  THEME_PRESETS,
  convertFileSrc,
  formatBytesJs,
  formatDuration,
  formatTimestamp,
  initWindowChrome,
  invokeCommand,
  listenEvent,
  pickClipsFolder
} from "./api.js";

const STORAGE_THEME_KEY = "duke_capture_theme";

const state = {
  activeTab: "replay",
  captureState: null,
  clips: [],
  selectedClip: null,
  listeningHotkey: false,
  pollTimer: null,
  searchDebounce: null
};

const TAB_META = {
  replay: "Instant Replay Buffer",
  clips: "Clip Studio & Trimmer",
  audio: "Dual WASAPI Audio & Hotkeys",
  settings: "Settings & Themes"
};

function $(id) {
  return document.getElementById(id);
}

function escapeHtml(raw) {
  if (raw === null || raw === undefined) return "";
  return String(raw)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function formatStatusLabel(raw, fallback = "Ready") {
  const txt = String(raw || fallback).trim();
  if (!txt) return fallback;
  if (txt === txt.toUpperCase() && /[A-Z]/.test(txt)) {
    return txt
      .toLowerCase()
      .split(/[\s_]+/)
      .map((w) => (w ? w[0].toUpperCase() + w.slice(1) : ""))
      .join(" ");
  }
  return txt;
}

function setIconButton(btn, iconId, labelText) {
  if (!btn) return;
  btn.innerHTML = `<svg class="icon"><use href="#${escapeHtml(iconId)}"/></svg><span>${escapeHtml(labelText)}</span>`;
}

function showToast(message, type = "info") {
  const container = $("toastContainer");
  if (!container) return;
  const el = document.createElement("div");
  el.className = `toast ${type}`;
  el.textContent = message;
  container.appendChild(el);
  setTimeout(() => {
    el.remove();
  }, 4400);
}

function applyTheme(themeId) {
  const valid = THEME_PRESETS.some((t) => t.id === themeId) ? themeId : "dark";
  document.body.setAttribute("data-theme", valid);
  document.documentElement.setAttribute("data-theme", valid);
  localStorage.setItem(STORAGE_THEME_KEY, valid);
  const preset = THEME_PRESETS.find((t) => t.id === valid);
  if ($("activeThemeBadge") && preset) {
    $("activeThemeBadge").textContent = preset.name;
  }
  document.querySelectorAll(".theme-card").forEach((card) => {
    card.classList.toggle("active", card.dataset.themeId === valid);
  });
}

function renderThemeGrid() {
  const grid = $("themeGrid");
  if (!grid) return;
  const active = localStorage.getItem(STORAGE_THEME_KEY) || "dark";
  grid.innerHTML = THEME_PRESETS.map(
    (t) => `
      <button type="button" class="theme-card ${t.id === active ? "active" : ""}" data-theme-id="${escapeHtml(t.id)}">
        <div class="theme-preview" data-preview-theme="${escapeHtml(t.id)}"></div>
        <div class="theme-card-body">
          <div class="theme-card-name">${escapeHtml(t.name)}</div>
          <div class="theme-card-desc">${escapeHtml(t.desc)}</div>
        </div>
      </button>
    `
  ).join("");

  grid.querySelectorAll(".theme-card").forEach((card) => {
    card.addEventListener("click", () => applyTheme(card.dataset.themeId));
  });
}

function switchTab(tabId) {
  if (!TAB_META[tabId]) return;
  state.activeTab = tabId;
  document.querySelectorAll(".nav-btn").forEach((btn) => {
    btn.classList.toggle("active", btn.dataset.tab === tabId);
  });
  document.querySelectorAll(".tab-panel").forEach((panel) => {
    panel.classList.toggle("active", panel.id === `view-${tabId}`);
  });
  $("topbarTitle").textContent = TAB_META[tabId];

  if (tabId === "clips") refreshClips();
  else if (tabId === "audio") refreshAudioDevices();
  else refreshCaptureState();
}

function applyStateUi(data) {
  if (!data) return;
  state.captureState = data;

  const running = Boolean(data.replayRunning);
  const badge = $("topReplayStatusBadge");
  badge.textContent = formatStatusLabel(data.statusText, running ? "Buffering" : "Standby");
  badge.classList.toggle("recording", running);

  $("heroStatusHeadline").textContent = running
    ? `Instant Replay Active — ${data.bufferSeconds}s Armed in RAM`
    : "Instant Replay Standby";

  setIconButton(
    $("topToggleReplayBtn"),
    running ? "icon-pause" : "icon-play",
    running ? "Stop Buffer" : "Start Buffer"
  );
  $("topToggleReplayBtn").className = `btn ${running ? "btn-danger" : "btn-primary"}`;

  setIconButton(
    $("mainToggleReplayBtn"),
    running ? "icon-pause" : "icon-play",
    running ? "Stop Replay Buffer" : "Arm Replay Buffer"
  );
  $("mainToggleReplayBtn").className = `btn btn-lg ${running ? "btn-danger" : "btn-primary"}`;

  $("saveDurationLabel").textContent = `${data.bufferSeconds}s`;
  const backendLabel = data.activeBackend || "DXGI + Media Foundation HW";
  $("encoderBadge").textContent = backendLabel;
  if ($("stripEncoderBadge")) $("stripEncoderBadge").textContent = backendLabel;
  if ($("stripBufferWindow")) $("stripBufferWindow").textContent = `${data.bufferSeconds}s Ring`;

  $("activeGameBadge").textContent = `Active Game: ${data.activeGame || "Desktop"}`;
  $("bufferedRamBadge").textContent = `${Number(data.bufferedSecondsActual || 0).toFixed(1)}s / ${Number(data.bufferedRamMb || 0).toFixed(1)} MB`;

  const hotkeyLabel = data.hotkey?.label || "F8";
  $("topHotkeyBadge").textContent = hotkeyLabel;
  $("replayHotkeyCardVal").textContent = hotkeyLabel;
  if (!state.listeningHotkey) {
    $("hotkeyBinderDisplay").textContent = hotkeyLabel;
  }

  document.querySelectorAll("#bufferDurationGroup .seg-btn").forEach((b) => {
    b.classList.toggle("active", Number(b.dataset.sec) === Number(data.bufferSeconds));
  });

  $("resolutionSelect").value = data.resolution || "1080p";
  $("fpsSelect").value = String(data.fps || 60);
  $("bitrateSlider").value = String(data.bitrateKbps || 12000);
  $("bitrateReadout").textContent = `${((data.bitrateKbps || 12000) / 1000).toFixed(1)} Mbps`;
  $("encoderSelect").value = data.encoderPreference || "auto";

  if (data.ffmpegInstalled) {
    $("ffmpegStatusTitle").textContent = "FFmpeg Hardware Engine Installed";
    $("ffmpegStatusDesc").textContent = data.ffmpegPath || "FFmpeg detected cleanly.";
    setIconButton($("installFfmpegBtn"), "icon-refresh-cw", "Re-check FFmpeg");
  } else {
    $("ffmpegStatusTitle").textContent = "Built-in Media Foundation Ready";
    $("ffmpegStatusDesc").textContent =
      "Zero-dependency encoder ready. Install FFmpeg for ultrafast keyframe copy trimming.";
    setIconButton($("installFfmpegBtn"), "icon-download", "Install FFmpeg");
  }

  $("systemVolumeSlider").value = String(data.systemVolumePct ?? 100);
  $("systemVolumeReadout").textContent = `${data.systemVolumePct ?? 100}%`;
  $("micGainSlider").value = String(data.micGainPct ?? 100);
  $("micGainReadout").textContent = `${data.micGainPct ?? 100}%`;
  $("chimeVolumeSlider").value = String(data.chimeVolumePct ?? 85);
  $("chimeVolumeReadout").textContent = `${data.chimeVolumePct ?? 85}%`;
  $("replayChimeVolVal").textContent = `${data.chimeVolumePct ?? 85}%`;

  $("micMuteToggle").checked = Boolean(data.micMuted);
  $("noiseGateToggle").checked = Boolean(data.noiseGateEnabled);
  $("replayMicStatusVal").textContent = data.micMuted
    ? "Muted"
    : data.noiseGateEnabled
      ? "Live (Gate On)"
      : "Live (Open)";
  $("replayMicStatusVal").className = `kv-value ${data.micMuted ? "status-accent" : "status-ok"}`;

  $("clipsDirInput").value = data.clipsDir || "";
  $("replayClipsDirShort").textContent = data.clipsDir || "Videos\\Duke Clips";
  $("autoTrimToggle").checked = Boolean(data.autoTrimEnabled);

  $("sidebarOwnRam").textContent = `${Number(data.ownWorkingSetMb || 0).toFixed(1)} MB`;
  $("sidebarDohShield").textContent = formatStatusLabel(data.dohShield, "Active");
  $("sidebarEcoStatus").textContent = formatStatusLabel(data.ecoScheduler, "Ready");
  if ($("settingsDohStatus")) $("settingsDohStatus").textContent = formatStatusLabel(data.dohShield, "Active");
  if ($("settingsEcoStatus")) $("settingsEcoStatus").textContent = formatStatusLabel(data.ecoScheduler, "Ready");
}

let statePollInFlight = false;
async function refreshCaptureState() {
  if (statePollInFlight || document.hidden) return;
  statePollInFlight = true;
  try {
    const data = await invokeCommand("get_capture_state");
    applyStateUi(data);
  } catch (err) {
    console.error("Failed loading capture state:", err);
  } finally {
    statePollInFlight = false;
  }
}

async function patchConfig(patch) {
  try {
    const updated = await invokeCommand("update_capture_config", { patch });
    applyStateUi(updated);
  } catch (err) {
    showToast(`Failed updating config: ${err}`, "error");
  }
}

async function handleToggleReplay() {
  const next = !(state.captureState && state.captureState.replayRunning);
  try {
    const updated = await invokeCommand("toggle_replay_buffer", { enabled: next });
    applyStateUi(updated);
    showToast(next ? "Replay buffer armed in RAM." : "Replay buffer stopped.", next ? "success" : "info");
  } catch (err) {
    showToast(`Could not toggle replay buffer: ${err}`, "error");
  }
}

async function handleSaveClipNow() {
  try {
    showToast("Flushing ring buffer to MP4 clip...", "info");
    const clip = await invokeCommand("save_replay_now", { gameOverride: null });
    showToast(`Saved ${clip.fileName} (${formatBytesJs(clip.sizeBytes)}) to ${clip.gameName}`, "success");
    if (state.activeTab === "clips") await refreshClips(clip.fullPath);
  } catch (err) {
    showToast(`Save clip failed: ${err}`, "error");
  }
}

async function refreshAudioDevices() {
  try {
    const devs = await invokeCommand("list_audio_input_devices");
    const select = $("micDeviceSelect");
    const currentId = state.captureState?.micDeviceId || "";
    const opts = [`<option value="">Default Windows Microphone</option>`];
    for (const d of devs || []) {
      const selected = d.id === currentId ? "selected" : "";
      const suffix = d.isDefault ? " (Windows Default)" : "";
      opts.push(`<option value="${escapeHtml(d.id)}" ${selected}>${escapeHtml(d.name + suffix)}</option>`);
    }
    select.innerHTML = opts.join("");
  } catch (err) {
    showToast(`Could not enumerate audio devices: ${err}`, "error");
  }
}

function updateClipStudioButtons() {
  const hasClip = Boolean(state.selectedClip);
  [
    "trimClipBtn",
    "copyDiscordBtn",
    "compressDiscord10Btn",
    "compressDiscord25Btn",
    "openClipExplorerBtn",
    "deleteClipBtn"
  ].forEach((id) => {
    const el = $(id);
    if (el) el.disabled = !hasClip;
  });
}

function selectClip(clip) {
  state.selectedClip = clip || null;
  updateClipStudioButtons();

  document.querySelectorAll(".clip-item").forEach((item) => {
    item.classList.toggle("active", Boolean(clip && item.dataset.path === clip.fullPath));
  });

  const video = $("clipPlayer");
  if (!clip) {
    $("selectedClipTitle").textContent = "Select a clip to preview";
    $("selectedClipGame").textContent = "Ready for instant keyframe trimming and Discord export";
    $("selectedClipMeta").textContent = "-- MB • 00:00";
    video.removeAttribute("src");
    video.load();
    return;
  }

  $("selectedClipTitle").textContent = clip.fileName || clip.file_name;
  $("selectedClipGame").textContent = `${clip.gameName} • ${formatTimestamp(clip.createdAt)}`;
  $("selectedClipMeta").textContent = `${formatBytesJs(clip.sizeBytes)} • ${formatDuration(clip.durationSecs)}`;

  video.src = convertFileSrc(clip.fullPath);
  const dur = Math.max(0.5, Number(clip.durationSecs) || 30);
  const startSlider = $("trimStartSlider");
  const endSlider = $("trimEndSlider");
  startSlider.max = String(dur);
  endSlider.max = String(dur);
  startSlider.value = "0";
  endSlider.value = String(dur);
  updateTrimSummaryLabel();
}

function updateTrimSummaryLabel() {
  const s = Number($("trimStartSlider").value || 0);
  const e = Number($("trimEndSlider").value || 0);
  $("trimRangeSummary").textContent = `${formatDuration(s)} -> ${formatDuration(e)} (${formatDuration(Math.max(0, e - s))})`;
}

function renderClipsList() {
  const q = ($("clipSearchInput").value || "").trim().toLowerCase();
  const gameFilter = $("clipGameFilter").value || "all";

  const filtered = state.clips.filter((c) => {
    if (gameFilter !== "all" && c.gameName !== gameFilter) return false;
    if (!q) return true;
    return c.fileName.toLowerCase().includes(q) || c.gameName.toLowerCase().includes(q);
  });

  $("clipCountBadge").textContent = `${filtered.length} clips`;
  const box = $("clipsListContainer");
  if (filtered.length === 0) {
    box.innerHTML = `<div class="empty-msg">No saved clips found matching filter.</div>`;
    return;
  }

  const grouped = new Map();
  for (const clip of filtered) {
    if (!grouped.has(clip.gameName)) grouped.set(clip.gameName, []);
    grouped.get(clip.gameName).push(clip);
  }

  let html = "";
  for (const [game, items] of grouped.entries()) {
    html += `<div class="game-group-header">${escapeHtml(game)} (${items.length})</div>`;
    for (const c of items) {
      const active = state.selectedClip && state.selectedClip.fullPath === c.fullPath;
      const smallDiscord = c.sizeBytes > 0 && c.sizeBytes <= 10 * 1024 * 1024;
      html += `
        <button type="button" class="clip-item ${active ? "active" : ""}" data-path="${escapeHtml(c.fullPath)}">
          <div class="clip-item-name">${escapeHtml(c.fileName)}</div>
          <div class="clip-item-meta">
            <span class="mono">${formatDuration(c.durationSecs)}</span>
            <span class="mono ${smallDiscord ? "status-ok" : ""}">${formatBytesJs(c.sizeBytes)}</span>
          </div>
        </button>
      `;
    }
  }
  box.innerHTML = html;
}

async function refreshClips(preferPath = null) {
  try {
    const list = await invokeCommand("list_clips");
    state.clips = Array.isArray(list) ? list : [];

    const games = [...new Set(state.clips.map((c) => c.gameName))].sort();
    const prevFilter = $("clipGameFilter").value || "all";
    $("clipGameFilter").innerHTML = [
      `<option value="all">All Games (${state.clips.length})</option>`,
      ...games.map((g) => `<option value="${escapeHtml(g)}" ${g === prevFilter ? "selected" : ""}>${escapeHtml(g)}</option>`)
    ].join("");

    renderClipsList();

    if (preferPath) {
      const match = state.clips.find((c) => c.fullPath === preferPath);
      if (match) selectClip(match);
    } else if (!state.selectedClip && state.clips.length > 0) {
      selectClip(state.clips[0]);
    } else if (state.selectedClip) {
      const stillExists = state.clips.find((c) => c.fullPath === state.selectedClip.fullPath);
      if (!stillExists) selectClip(state.clips[0] || null);
    }
  } catch (err) {
    showToast(`Failed loading clips: ${err}`, "error");
  }
}

function keyEventToWin32(e) {
  if (["Shift", "Control", "Alt", "Meta"].includes(e.key)) return null;

  let modifiers = 0;
  const parts = [];
  if (e.ctrlKey) {
    modifiers |= 0x0002;
    parts.push("Ctrl");
  }
  if (e.altKey) {
    modifiers |= 0x0001;
    parts.push("Alt");
  }
  if (e.shiftKey) {
    modifiers |= 0x0004;
    parts.push("Shift");
  }

  let vk = e.keyCode || 0;
  let keyName = e.key.length === 1 ? e.key.toUpperCase() : e.key;
  if (e.code && /^F([1-9]|1[0-2])$/.test(e.code)) {
    const n = Number(e.code.slice(1));
    vk = 0x6f + n;
    keyName = e.code;
  } else if (e.code === "Backquote") {
    vk = 0xc0;
    keyName = "~";
  } else if (e.code === "Space") {
    vk = 0x20;
    keyName = "Space";
  } else if (e.code === "Insert") {
    vk = 0x2d;
    keyName = "Insert";
  } else if (e.code === "Home") {
    vk = 0x24;
    keyName = "Home";
  }

  if (!vk) return null;
  parts.push(keyName);
  return { label: parts.join("+"), vk, modifiers, isMouseXbutton: null };
}

function stopHotkeyListening(restoreLabel = true) {
  state.listeningHotkey = false;
  $("hotkeyBinderBox").classList.remove("listening");
  $("hotkeyBinderHint").textContent =
    "Click to bind any key, modifier combo (Alt+F10), or Mouse4/Mouse5";
  if (restoreLabel && state.captureState?.hotkey?.label) {
    $("hotkeyBinderDisplay").textContent = state.captureState.hotkey.label;
  }
}

function bindEvents() {
  document.querySelectorAll(".nav-btn").forEach((btn) => {
    btn.addEventListener("click", () => switchTab(btn.dataset.tab));
  });

  if ($("titleClipSearch")) {
    $("titleClipSearch").addEventListener("input", (e) => {
      if ($("clipSearchInput")) $("clipSearchInput").value = e.target.value;
      if (state.activeTab === "clips") {
        clearTimeout(state.searchDebounce);
        state.searchDebounce = setTimeout(renderClipsList, 120);
      }
    });
    $("titleClipSearch").addEventListener("keydown", (e) => {
      if (e.key === "Enter") {
        if ($("clipSearchInput")) $("clipSearchInput").value = e.target.value;
        switchTab("clips");
      }
    });
  }

  $("topToggleReplayBtn").addEventListener("click", handleToggleReplay);
  $("mainToggleReplayBtn").addEventListener("click", handleToggleReplay);
  $("topSaveClipBtn").addEventListener("click", handleSaveClipNow);
  $("mainSaveClipBtn").addEventListener("click", handleSaveClipNow);

  document.querySelectorAll("#bufferDurationGroup .seg-btn").forEach((btn) => {
    btn.addEventListener("click", () => {
      patchConfig({ bufferSeconds: Number(btn.dataset.sec) });
    });
  });

  $("resolutionSelect").addEventListener("change", (e) => patchConfig({ resolution: e.target.value }));
  $("fpsSelect").addEventListener("change", (e) => patchConfig({ fps: Number(e.target.value) }));
  $("encoderSelect").addEventListener("change", (e) => patchConfig({ encoderPreference: e.target.value }));

  $("bitrateSlider").addEventListener("input", (e) => {
    $("bitrateReadout").textContent = `${(Number(e.target.value) / 1000).toFixed(1)} Mbps`;
  });
  $("bitrateSlider").addEventListener("change", (e) => {
    patchConfig({ bitrateKbps: Number(e.target.value) });
  });

  $("installFfmpegBtn").addEventListener("click", async () => {
    const btn = $("installFfmpegBtn");
    btn.disabled = true;
    setIconButton(btn, "icon-refresh-cw", "Downloading...");
    $("ffmpegProgressWrap").classList.remove("hidden");
    try {
      const caps = await invokeCommand("install_ffmpeg_essentials");
      showToast(`FFmpeg installed cleanly (${caps.activeBackend}).`, "success");
      await refreshCaptureState();
    } catch (err) {
      showToast(`FFmpeg setup failed: ${err}`, "error");
    } finally {
      btn.disabled = false;
      $("ffmpegProgressWrap").classList.add("hidden");
    }
  });

  $("clipsRefreshBtn").addEventListener("click", () => refreshClips());
  $("clipsOpenFolderBtn").addEventListener("click", () => invokeCommand("open_clip_folder", { path: null }));
  $("quickOpenClipsBtn").addEventListener("click", () => invokeCommand("open_clip_folder", { path: null }));

  $("clipSearchInput").addEventListener("input", () => {
    if ($("titleClipSearch")) $("titleClipSearch").value = $("clipSearchInput").value;
    clearTimeout(state.searchDebounce);
    state.searchDebounce = setTimeout(renderClipsList, 120);
  });
  $("clipGameFilter").addEventListener("change", renderClipsList);

  $("clipsListContainer").addEventListener("click", (e) => {
    const item = e.target.closest(".clip-item");
    if (!item) return;
    const match = state.clips.find((c) => c.fullPath === item.dataset.path);
    if (match) selectClip(match);
  });

  $("clipPlayer").addEventListener("loadedmetadata", () => {
    const v = $("clipPlayer");
    if (v.duration && isFinite(v.duration)) {
      $("trimStartSlider").max = String(v.duration);
      $("trimEndSlider").max = String(v.duration);
      $("trimEndSlider").value = String(v.duration);
      updateTrimSummaryLabel();
    }
  });

  $("trimStartSlider").addEventListener("input", () => {
    const startVal = Number($("trimStartSlider").value);
    const endVal = Number($("trimEndSlider").value);
    if (startVal >= endVal - 0.2) {
      $("trimStartSlider").value = String(Math.max(0, endVal - 0.2));
    }
    $("clipPlayer").currentTime = Number($("trimStartSlider").value);
    updateTrimSummaryLabel();
  });

  $("trimEndSlider").addEventListener("input", () => {
    const startVal = Number($("trimStartSlider").value);
    const endVal = Number($("trimEndSlider").value);
    if (endVal <= startVal + 0.2) {
      $("trimEndSlider").value = String(startVal + 0.2);
    }
    $("clipPlayer").currentTime = Number($("trimEndSlider").value);
    updateTrimSummaryLabel();
  });

  $("setStartFromPlayheadBtn").addEventListener("click", () => {
    const t = $("clipPlayer").currentTime || 0;
    const endVal = Number($("trimEndSlider").value);
    $("trimStartSlider").value = String(Math.min(t, Math.max(0, endVal - 0.2)));
    updateTrimSummaryLabel();
  });

  $("setEndFromPlayheadBtn").addEventListener("click", () => {
    const t = $("clipPlayer").currentTime || 0;
    const startVal = Number($("trimStartSlider").value);
    $("trimEndSlider").value = String(Math.max(t, startVal + 0.2));
    updateTrimSummaryLabel();
  });

  $("trimClipBtn").addEventListener("click", async () => {
    if (!state.selectedClip) return;
    $("trimClipBtn").disabled = true;
    try {
      const startSec = Number($("trimStartSlider").value || 0);
      const endSec = Number($("trimEndSlider").value || 0);
      const newClip = await invokeCommand("trim_clip", {
        path: state.selectedClip.fullPath,
        startSec,
        endSec
      });
      showToast(`Trimmed clip saved: ${newClip.fileName}`, "success");
      await refreshClips(newClip.fullPath);
    } catch (err) {
      showToast(`Trim failed: ${err}`, "error");
    } finally {
      updateClipStudioButtons();
    }
  });

  $("copyDiscordBtn").addEventListener("click", async () => {
    if (!state.selectedClip) return;
    try {
      const msg = await invokeCommand("copy_clip_to_clipboard", { path: state.selectedClip.fullPath });
      showToast(msg, "success");
    } catch (err) {
      showToast(`Clipboard copy failed: ${err}`, "error");
    }
  });

  $("compressDiscord10Btn").addEventListener("click", async () => {
    if (!state.selectedClip) return;
    $("compressDiscord10Btn").disabled = true;
    showToast("Compressing clip to < 10 MB for free Discord...", "info");
    try {
      const out = await invokeCommand("compress_clip_for_discord", {
        path: state.selectedClip.fullPath,
        targetMb: 10
      });
      showToast(`Discord <10 MB clip ready (${formatBytesJs(out.sizeBytes)}): ${out.fileName}`, "success");
      await refreshClips(out.fullPath);
    } catch (err) {
      showToast(`Compression failed: ${err}`, "error");
    } finally {
      updateClipStudioButtons();
    }
  });

  $("compressDiscord25Btn").addEventListener("click", async () => {
    if (!state.selectedClip) return;
    $("compressDiscord25Btn").disabled = true;
    showToast("Compressing clip to < 25 MB for Discord Nitro...", "info");
    try {
      const out = await invokeCommand("compress_clip_for_discord", {
        path: state.selectedClip.fullPath,
        targetMb: 25
      });
      showToast(`Discord <25 MB clip ready (${formatBytesJs(out.sizeBytes)}): ${out.fileName}`, "success");
      await refreshClips(out.fullPath);
    } catch (err) {
      showToast(`Compression failed: ${err}`, "error");
    } finally {
      updateClipStudioButtons();
    }
  });

  $("openClipExplorerBtn").addEventListener("click", () => {
    if (state.selectedClip) invokeCommand("open_clip_folder", { path: state.selectedClip.fullPath });
  });

  $("deleteClipBtn").addEventListener("click", async () => {
    if (!state.selectedClip) return;
    const doomed = state.selectedClip;
    try {
      await invokeCommand("delete_clip", { path: doomed.fullPath });
      showToast(`Deleted ${doomed.fileName}`, "info");
      state.selectedClip = null;
      await refreshClips();
    } catch (err) {
      showToast(`Delete failed: ${err}`, "error");
    }
  });

  $("hotkeyBinderBox").addEventListener("click", () => {
    state.listeningHotkey = true;
    $("hotkeyBinderBox").classList.add("listening");
    $("hotkeyBinderDisplay").textContent = "Press Key / Mouse4 / Mouse5";
    $("hotkeyBinderHint").textContent = "Press Escape to cancel rebinding.";
  });

  window.addEventListener("keydown", async (e) => {
    if (!state.listeningHotkey) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") {
      stopHotkeyListening(true);
      return;
    }
    const parsed = keyEventToWin32(e);
    if (!parsed) return;
    stopHotkeyListening(false);
    try {
      const updated = await invokeCommand("set_hotkey", parsed);
      applyStateUi(updated);
      showToast(`Global clip hotkey set to ${parsed.label}.`, "success");
    } catch (err) {
      showToast(`Hotkey bind failed: ${err}`, "error");
    }
  });

  window.addEventListener("mousedown", async (e) => {
    if (!state.listeningHotkey) return;
    if (e.button !== 3 && e.button !== 4) return;
    e.preventDefault();
    e.stopPropagation();
    const xbtn = e.button === 3 ? 1 : 2;
    const label = e.button === 3 ? "Mouse4" : "Mouse5";
    stopHotkeyListening(false);
    try {
      const updated = await invokeCommand("set_hotkey", {
        label,
        vk: 0,
        modifiers: 0,
        isMouseXbutton: xbtn
      });
      applyStateUi(updated);
      showToast(`Global clip hotkey set to ${label}.`, "success");
    } catch (err) {
      showToast(`Mouse button bind failed: ${err}`, "error");
    }
  });

  $("chimeVolumeSlider").addEventListener("input", (e) => {
    $("chimeVolumeReadout").textContent = `${e.target.value}%`;
  });
  $("chimeVolumeSlider").addEventListener("change", (e) => {
    patchConfig({ chimeVolumePct: Number(e.target.value) });
  });

  const testChime = async () => {
    try {
      await invokeCommand("test_confirmation_chime", {
        volumePct: Number($("chimeVolumeSlider").value || 85)
      });
    } catch (err) {
      showToast(`Chime test failed: ${err}`, "error");
    }
  };
  $("quickTestChimeBtn").addEventListener("click", testChime);
  $("audioTestChimeBtn").addEventListener("click", testChime);

  $("refreshAudioDevsBtn").addEventListener("click", refreshAudioDevices);
  $("micDeviceSelect").addEventListener("change", (e) => {
    patchConfig({ micDeviceId: e.target.value || "" });
  });

  $("systemVolumeSlider").addEventListener("input", (e) => {
    $("systemVolumeReadout").textContent = `${e.target.value}%`;
  });
  $("systemVolumeSlider").addEventListener("change", (e) => {
    patchConfig({ systemVolumePct: Number(e.target.value) });
  });

  $("micGainSlider").addEventListener("input", (e) => {
    $("micGainReadout").textContent = `${e.target.value}%`;
  });
  $("micGainSlider").addEventListener("change", (e) => {
    patchConfig({ micGainPct: Number(e.target.value) });
  });

  $("micMuteToggle").addEventListener("change", (e) => patchConfig({ micMuted: e.target.checked }));
  $("quickMuteMicBtn").addEventListener("click", () => {
    const next = !$("micMuteToggle").checked;
    patchConfig({ micMuted: next });
  });
  $("noiseGateToggle").addEventListener("change", (e) => patchConfig({ noiseGateEnabled: e.target.checked }));

  $("browseClipsDirBtn").addEventListener("click", async () => {
    const picked = await pickClipsFolder();
    if (picked) {
      await patchConfig({ clipsDir: picked });
      showToast(`Clips directory set to ${picked}`, "success");
    }
  });
  $("settingsOpenClipsBtn").addEventListener("click", () => invokeCommand("open_clip_folder", { path: null }));

  $("autoTrimToggle").addEventListener("change", async (e) => {
    try {
      await invokeCommand("set_auto_trim_enabled", { enabled: e.target.checked });
      showToast(`Auto-trim ${e.target.checked ? "enabled" : "disabled"}.`, "info");
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  $("trimOwnRamBtn").addEventListener("click", async () => {
    try {
      const res = await invokeCommand("trim_working_set");
      $("sidebarOwnRam").textContent = `${Number(res.workingSetMb).toFixed(1)} MB`;
      showToast(`Trimmed working set to ${Number(res.workingSetMb).toFixed(1)} MB.`, "success");
    } catch (err) {
      showToast(`Trim failed: ${err}`, "error");
    }
  });

  listenEvent("audio-levels", (payload) => {
    if (document.hidden || !payload) return;
    const sys = Math.min(100, Number(payload.systemLevelPct || 0));
    const mic = Math.min(100, Number(payload.micLevelPct || 0));
    $("replaySysMeterPct").textContent = `${sys}%`;
    $("replaySysMeterBar").style.width = `${sys}%`;
    $("replayMicMeterPct").textContent = `${mic}%`;
    $("replayMicMeterBar").style.width = `${mic}%`;
    $("audioTabSysBar").style.width = `${sys}%`;
    $("audioTabMicBar").style.width = `${mic}%`;
  });

  listenEvent("clip-saved", (clip) => {
    if (!clip) return;
    showToast(`Saved ${clip.fileName} (${clip.gameName})`, "success");
    if (state.activeTab === "clips") refreshClips(clip.fullPath);
  });

  listenEvent("capture-error", (msg) => {
    if (msg) showToast(String(msg), "error");
  });

  listenEvent("memory-telemetry", (payload) => {
    if (payload && typeof payload.workingSetMb === "number") {
      $("sidebarOwnRam").textContent = `${payload.workingSetMb.toFixed(1)} MB`;
    }
  });

  listenEvent("ffmpeg-download-progress", (payload) => {
    if (payload && typeof payload.progressPct === "number") {
      $("ffmpegProgressBar").style.width = `${Math.min(100, payload.progressPct)}%`;
    }
  });

  document.addEventListener("visibilitychange", () => {
    if (!document.hidden) refreshCaptureState();
  });
}

async function init() {
  initWindowChrome(() => {
    const search = $("titleClipSearch");
    if (search) {
      search.focus();
      search.select();
    }
  });

  renderThemeGrid();
  applyTheme(localStorage.getItem(STORAGE_THEME_KEY) || "dark");
  bindEvents();
  await Promise.all([refreshCaptureState(), refreshAudioDevices()]);

  try {
    const sys = await invokeCommand("get_system_telemetry");
    $("settingsStorageMode").textContent = sys.portableMode ? "Portable" : "AppData";
  } catch (_) {}

  state.pollTimer = setInterval(() => {
    if (!document.hidden && (state.activeTab === "replay" || state.captureState?.replayRunning)) {
      refreshCaptureState();
    }
  }, 1250);
}

document.addEventListener("DOMContentLoaded", init);

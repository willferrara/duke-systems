import {
  THEME_PRESETS,
  applyTheme,
  escapeHtml,
  getSavedTheme,
  initWindowControls,
  invokeCommand,
  listenEvent
} from "./api.js";

const APP_ASSET_FALLBACKS = {
  launcher: "assets/dukeflag.png",
  monitor: "assets/dukemonitor.png",
  capture: "assets/dukecapture.png",
  input: "assets/dukeinput.png"
};

let currentSnapshot = null;
let currentTab = "hub";
let activeQuery = "";
let refreshTimer = null;
let unlistenBus = null;
let unlistenStatus = null;
let unlistenMem = null;

function $(id) {
  return document.getElementById(id);
}

function showToast(message, tone = "info") {
  const container = $("toastContainer");
  if (!container) return;
  const el = document.createElement("div");
  el.className = `toast ${tone}`;
  el.textContent = message;
  container.appendChild(el);
  setTimeout(() => {
    el.remove();
  }, 3600);
}

function setActiveTab(tab) {
  currentTab = tab;
  document.querySelectorAll(".nav-item[data-tab]").forEach((btn) => {
    btn.classList.toggle("active", btn.dataset.tab === tab);
  });
  document.querySelectorAll(".tab-view").forEach((view) => {
    view.classList.toggle("active", view.id === `tab-${tab}`);
  });
  const titleMap = {
    hub: "Suite Hub",
    activity: "Pipe & Feed",
    settings: "Settings"
  };
  const vt = $("viewTitle");
  if (vt) vt.textContent = titleMap[tab] || "Suite Hub";
}

function svgIcon(name) {
  return `<svg class="icon" aria-hidden="true"><use href="#${name}"/></svg>`;
}

function renderAppCards(apps) {
  const grid = $("suiteAppsGrid");
  if (!grid) return;

  const q = activeQuery.trim().toLowerCase();
  const filtered = (apps || []).filter((app) => {
    if (!q || currentTab !== "hub") return true;
    const text = `${app.name} ${app.subtitle} ${app.statusLabel} ${app.exePath}`.toLowerCase();
    return text.includes(q);
  });

  if (filtered.length === 0) {
    grid.innerHTML = `<div class="panel-card"><div class="feed-empty">No suite modules matched "${escapeHtml(activeQuery)}".</div></div>`;
    return;
  }

  grid.innerHTML = filtered
    .map((app) => {
      const iconSrc = app.logoAsset || APP_ASSET_FALLBACKS[app.id] || "dukesystems.png";
      const runningClass = app.running ? "is-running" : "";
      const pillClass = app.running ? "online" : app.installed ? "" : "missing";
      const pillText = app.running ? (app.statusLabel || "Online") : app.installed ? "Standby" : "Not Found";
      const primaryLabel = app.running ? "Focus" : "Launch";
      const primaryIcon = app.running ? "icon-arrow-left" : "icon-play";
      const pidText = app.pid ? `PID ${app.pid}` : "Standby";
      const ramText = app.running ? `${Number(app.ramMb || 0).toFixed(1)} MB` : "0.0 MB";

      return `
        <article class="app-launcher-card ${runningClass}" data-app-id="${escapeHtml(app.id)}">
          <div class="app-card-emblem">
            <img src="${escapeHtml(iconSrc)}" alt="${escapeHtml(app.name)}" />
          </div>

          <div class="app-card-middle">
            <div class="app-card-title-row">
              <span class="app-card-title">${escapeHtml(app.name)}</span>
              <span class="pill ${pillClass}">${escapeHtml(pillText)}</span>
            </div>
            <p class="app-card-desc">${escapeHtml(app.subtitle)}</p>
            <div class="app-card-stats mono-num">
              <span>${escapeHtml(pidText)}</span>
              <span>•</span>
              <span>${escapeHtml(ramText)}</span>
              <span>•</span>
              <span class="app-card-path" title="${escapeHtml(app.exePath || "")}">${escapeHtml(app.exePath || "Executable unavailable")}</span>
            </div>
          </div>

          <div class="app-card-actions">
            <button type="button" class="btn btn-primary btn-sm" data-action="launch" data-app-id="${escapeHtml(app.id)}" ${app.installed || app.running ? "" : "disabled"}>
              ${svgIcon(primaryIcon)}
              <span>${primaryLabel}</span>
            </button>
            <div class="app-card-sub-actions">
              <button type="button" class="btn btn-danger-ghost btn-sm" data-action="stop" data-app-id="${escapeHtml(app.id)}" ${app.running ? "" : "disabled"} title="Stop process">
                ${svgIcon("icon-pause")}
                <span>Stop</span>
              </button>
              <button type="button" class="btn btn-secondary btn-sm" data-action="folder" data-app-id="${escapeHtml(app.id)}" title="Open folder">
                ${svgIcon("icon-folder-open")}
              </button>
            </div>
          </div>
        </article>
      `;
    })
    .join("");
}

function renderActivityFeed(feed) {
  const list = $("activityFeedList");
  const pill = $("feedCountPill");
  const navBadge = $("navFeedBadge");
  const items = feed || [];

  if (navBadge) navBadge.textContent = String(items.length);

  const q = activeQuery.trim().toLowerCase();
  const filtered = q
    ? items.filter((item) => {
        const hay = `${item.source} ${item.eventType} ${item.summary} ${item.timeLabel}`.toLowerCase();
        return hay.includes(q);
      })
    : items;

  if (pill) {
    pill.textContent = q ? `${filtered.length} of ${items.length} events` : `${items.length} events`;
  }
  if (!list) return;

  if (filtered.length === 0) {
    list.innerHTML = `<div class="feed-empty">${q ? "No events match your current filter." : "No suite pipe activity recorded yet."}</div>`;
    return;
  }

  list.innerHTML = filtered
    .map(
      (item) => `
        <div class="feed-item">
          <span class="feed-time mono-num">${escapeHtml(item.timeLabel || "--:--:--")}</span>
          <span class="feed-source">${escapeHtml(item.source || "System")}</span>
          <span class="feed-type mono-num">${escapeHtml(item.eventType || "event")}</span>
          <span class="feed-summary">${escapeHtml(item.summary || "")}</span>
        </div>
      `
    )
    .join("");
}

function renderSettingsControls(snap) {
  const s = snap.settings || {};
  const auto = s.autoLaunch || {};
  document.querySelectorAll("[data-autolaunch]").forEach((cb) => {
    const key = cb.dataset.autolaunch;
    cb.checked = Boolean(auto[key]);
  });

  if ($("chkAutostartWindows")) $("chkAutostartWindows").checked = Boolean(s.autostartWindows);
  if ($("chkCloseToTray")) $("chkCloseToTray").checked = Boolean(s.closeToTray);
  if ($("chkMinimizeToTray")) $("chkMinimizeToTray").checked = Boolean(s.minimizeToTray);
  if ($("chkStartMinimized")) $("chkStartMinimized").checked = Boolean(s.startMinimized);
  if ($("chkAutoTrimEnabled")) $("chkAutoTrimEnabled").checked = Boolean(s.autoTrimEnabled);

  if ($("settingsPortableBadge")) {
    $("settingsPortableBadge").textContent = snap.portableMode ? "Portable" : "AppData";
  }
  if ($("settingsPathCode")) {
    $("settingsPathCode").textContent = snap.settingsPath || "--";
  }
}

function renderSnapshot(snap) {
  if (!snap) return;
  currentSnapshot = snap;

  const running = Number(snap.runningCount || 0);
  const totalApps = (snap.apps || []).length || 4;
  const totalRam = Number(snap.totalSuiteRamMb || 0).toFixed(1);
  const ownRam = Number(snap.ownRamMb || 0).toFixed(1);

  if ($("navRunningBadge")) $("navRunningBadge").textContent = `${running}/${totalApps}`;
  if ($("statOwnRam")) $("statOwnRam").textContent = `${ownRam} MB`;
  if ($("statSuiteRam")) $("statSuiteRam").textContent = `${totalRam} MB`;
  if ($("statPipeBus")) {
    $("statPipeBus").textContent = snap.pipeBusOnline ? "Online" : "Standby";
    $("statPipeBus").className = `tele-value mono-num ${snap.pipeBusOnline ? "ok" : "warn"}`;
  }
  const shmName = snap.shmName || "Local\\duke-suite-shm-v1";
  const shmActive = Boolean(snap.shmBusActive);
  const shmFrames = Number(snap.shmTotalFrames || 0);
  const shmCapacity = Number(snap.shmRingCapacity || 1024);
  const shmApps = Array.isArray(snap.shmActiveApps) ? snap.shmActiveApps : [];
  const shmStatusLabel = `${shmName}: ${shmActive ? "ACTIVE" : "STANDBY"}`;
  const shmAppsSummary = shmApps.length > 0 ? `${shmApps.length} (${shmApps.join(", ")})` : "0 (None)";
  const shmFramesFmt = shmFrames.toLocaleString("en-US");

  if ($("statShmBus")) {
    $("statShmBus").textContent = shmActive ? `Active (${shmApps.length})` : "Standby";
    $("statShmBus").className = `tele-value mono-num ${shmActive ? "ok" : "warn"}`;
  }
  if ($("statPortableMode")) {
    $("statPortableMode").textContent = snap.portableMode ? "Portable" : "AppData";
  }
  if ($("suiteSummaryPill")) {
    $("suiteSummaryPill").textContent = `${running}/${totalApps} active`;
  }

  const pipeInd = $("heroPipeIndicator");
  if (pipeInd) {
    pipeInd.className = `status-dot ${snap.pipeBusOnline ? "online" : "offline"}`;
  }
  if ($("heroPipeText")) {
    $("heroPipeText").textContent = snap.pipeBusOnline ? "Pipe Bus Online" : "Pipe Bus Standby";
  }
  const shmInd = $("heroShmIndicator");
  if (shmInd) {
    shmInd.className = `status-dot ${shmActive ? "online" : "offline"}`;
  }
  if ($("heroShmStatusText")) {
    $("heroShmStatusText").textContent = shmStatusLabel;
  }
  if ($("heroPipeName")) {
    $("heroPipeName").textContent = snap.pipeName || "\\\\.\\pipe\\duke-suite";
  }
  if ($("heroTotalRam")) $("heroTotalRam").textContent = `${totalRam} MB`;
  if ($("heroRunningCount")) $("heroRunningCount").textContent = `${running} / ${totalApps}`;
  if ($("heroOwnRam")) $("heroOwnRam").textContent = `${ownRam} MB`;
  if ($("heroShmFrames")) $("heroShmFrames").textContent = shmFramesFmt;
  if ($("heroShmApps")) $("heroShmApps").textContent = shmAppsSummary;
  if ($("heroShmTelemetry")) {
    const parts = [];
    if (snap.shmGpuTempC != null) parts.push(`${Number(snap.shmGpuTempC)}°C`);
    if (snap.shmGpuLoadPct != null) parts.push(`${Number(snap.shmGpuLoadPct)}% GPU`);
    if (snap.shmSysRamUsedMb != null) {
      parts.push(`${(Number(snap.shmSysRamUsedMb) / 1024).toFixed(1)} GB RAM`);
    }
    $("heroShmTelemetry").textContent = parts.length > 0 ? parts.join(" · ") : "--";
  }

  if ($("busRoleValue")) {
    $("busRoleValue").textContent = snap.pipeBusIsHub ? "Primary Named-Pipe Hub" : "Secondary Client Node";
  }
  if ($("busStateValue")) {
    $("busStateValue").textContent = snap.pipeBusOnline ? "Listening & Broadcasting" : "Waiting";
  }
  if ($("busShmStatus")) {
    $("busShmStatus").textContent = shmStatusLabel;
  }
  if ($("busShmFrames")) {
    $("busShmFrames").textContent = `${shmFramesFmt} (ring ${shmCapacity})`;
  }
  if ($("busShmApps")) {
    $("busShmApps").textContent = shmApps.length > 0 ? shmApps.join(", ") : "None";
  }

  renderAppCards(snap.apps);
  renderActivityFeed(snap.activityFeed);
  renderSettingsControls(snap);
}

async function refreshSnapshot() {
  try {
    const snap = await invokeCommand("get_suite_status");
    renderSnapshot(snap);
  } catch (err) {
    showToast(String(err), "err");
  }
}

async function persistSettingsFromUi() {
  if (!currentSnapshot) return;
  const autoLaunch = {
    monitor: false,
    capture: false,
    input: false,
    launcher: false
  };
  document.querySelectorAll("[data-autolaunch]").forEach((cb) => {
    autoLaunch[cb.dataset.autolaunch] = Boolean(cb.checked);
  });

  const payload = {
    autoLaunch,
    autostartWindows: Boolean($("chkAutostartWindows")?.checked),
    closeToTray: Boolean($("chkCloseToTray")?.checked),
    minimizeToTray: Boolean($("chkMinimizeToTray")?.checked),
    startMinimized: Boolean($("chkStartMinimized")?.checked),
    autoTrimEnabled: Boolean($("chkAutoTrimEnabled")?.checked)
  };

  try {
    const updated = await invokeCommand("update_suite_settings", { settings: payload });
    renderSnapshot(updated);
    showToast("Suite settings saved.", "ok");
  } catch (err) {
    showToast(`Failed to save settings: ${err}`, "err");
  }
}

function renderThemes() {
  const grid = $("themeGrid");
  if (!grid) return;
  const activeId = getSavedTheme();
  grid.innerHTML = THEME_PRESETS.map(
    (t) => `
      <button type="button" class="theme-option ${t.id === activeId ? "active" : ""}" data-theme-id="${escapeHtml(t.id)}">
        <div class="theme-preview" data-preview-theme="${escapeHtml(t.id)}"></div>
        <div class="theme-meta">
          <div class="theme-name">${escapeHtml(t.name)}</div>
          <div class="theme-desc">${escapeHtml(t.desc)}</div>
        </div>
      </button>
    `
  ).join("");
}

function startPolling() {
  stopPolling();
  refreshTimer = setInterval(() => {
    if (!document.hidden) {
      refreshSnapshot();
    }
  }, 2000);
}

function stopPolling() {
  if (refreshTimer) {
    clearInterval(refreshTimer);
    refreshTimer = null;
  }
}

function wireEvents() {
  document.querySelectorAll(".nav-item[data-tab]").forEach((btn) => {
    btn.addEventListener("click", () => {
      setActiveTab(btn.dataset.tab);
      if (currentSnapshot) {
        renderAppCards(currentSnapshot.apps);
        renderActivityFeed(currentSnapshot.activityFeed);
      }
    });
  });

  $("titleSearchInput")?.addEventListener("input", (e) => {
    activeQuery = e.target.value || "";
    if (currentSnapshot) {
      renderAppCards(currentSnapshot.apps);
      renderActivityFeed(currentSnapshot.activityFeed);
    }
  });

  $("btnRefreshNow")?.addEventListener("click", refreshSnapshot);

  $("btnTrimOwnRam")?.addEventListener("click", async () => {
    try {
      const res = await invokeCommand("trim_working_set");
      const mb = Number(res?.ownRamMb ?? 0).toFixed(1);
      if ($("statOwnRam")) $("statOwnRam").textContent = `${mb} MB`;
      if ($("heroOwnRam")) $("heroOwnRam").textContent = `${mb} MB`;
      showToast(`Hub RAM trimmed to ${mb} MB.`, "ok");
    } catch (err) {
      showToast(String(err), "err");
    }
  });

  $("btnLaunchFullSuite")?.addEventListener("click", async () => {
    try {
      const summary = await invokeCommand("launch_full_suite");
      showToast(summary?.message || "Suite launch sequence completed.", "ok");
      await refreshSnapshot();
    } catch (err) {
      showToast(String(err), "err");
    }
  });

  const handlePurgeAll = async () => {
    try {
      const res = await invokeCommand("purge_suite_ram");
      showToast(res?.message || "Purged Suite RAM across running processes.", "ok");
      await refreshSnapshot();
    } catch (err) {
      showToast(String(err), "err");
    }
  };
  $("btnPurgeSuiteRam")?.addEventListener("click", handlePurgeAll);
  $("btnBroadcastPurge")?.addEventListener("click", handlePurgeAll);
  $("btnSettingsPurgeAll")?.addEventListener("click", handlePurgeAll);

  $("btnSettingsTrimNow")?.addEventListener("click", () => $("btnTrimOwnRam")?.click());

  $("btnClearFeed")?.addEventListener("click", async () => {
    try {
      await invokeCommand("clear_activity_feed");
      await refreshSnapshot();
      showToast("Pipe activity feed cleared.", "info");
    } catch (err) {
      showToast(String(err), "err");
    }
  });

  $("suiteAppsGrid")?.addEventListener("click", async (e) => {
    const btn = e.target.closest("button[data-action][data-app-id]");
    if (!btn) return;
    const action = btn.dataset.action;
    const appId = btn.dataset.appId;

    try {
      if (action === "launch") {
        const msg = await invokeCommand("launch_suite_app", { appId });
        showToast(msg || `Triggered ${appId}.`, "ok");
      } else if (action === "stop") {
        const msg = await invokeCommand("stop_suite_app", { appId });
        showToast(msg || `Stopped ${appId}.`, "info");
      } else if (action === "folder") {
        await invokeCommand("open_app_folder", { appId });
      }
      await refreshSnapshot();
    } catch (err) {
      showToast(String(err), "err");
    }
  });

  document.querySelectorAll("[data-autolaunch]").forEach((cb) => {
    cb.addEventListener("change", persistSettingsFromUi);
  });
  [
    "chkAutostartWindows",
    "chkCloseToTray",
    "chkMinimizeToTray",
    "chkStartMinimized",
    "chkAutoTrimEnabled"
  ].forEach((id) => {
    $(id)?.addEventListener("change", persistSettingsFromUi);
  });

  $("themeGrid")?.addEventListener("click", (e) => {
    const card = e.target.closest(".theme-option[data-theme-id]");
    if (!card) return;
    const applied = applyTheme(card.dataset.themeId);
    renderThemes();
    showToast(`Theme switched to ${applied}.`, "info");
  });

  document.addEventListener("visibilitychange", () => {
    const visible = !document.hidden;
    invokeCommand("set_ui_visible", { visible }).catch(() => {});
    if (visible) {
      refreshSnapshot();
      startPolling();
    } else {
      stopPolling();
    }
  });
}

async function init() {
  applyTheme(getSavedTheme());
  renderThemes();
  initWindowControls({
    onCtrlK: () => {
      const inp = $("titleSearchInput");
      if (inp) {
        inp.focus();
        inp.select();
      }
    }
  });
  wireEvents();

  unlistenBus = await listenEvent("duke-bus-event", () => {
    if (!document.hidden) refreshSnapshot();
  });
  unlistenStatus = await listenEvent("suite-status-changed", () => {
    if (!document.hidden) refreshSnapshot();
  });
  unlistenMem = await listenEvent("memory-telemetry", (payload) => {
    if (document.hidden || !payload) return;
    const own = Number(payload.ownRamMb || 0).toFixed(1);
    if ($("statOwnRam")) $("statOwnRam").textContent = `${own} MB`;
    if ($("heroOwnRam")) $("heroOwnRam").textContent = `${own} MB`;
  });

  await refreshSnapshot();
  startPolling();

  window.addEventListener("beforeunload", () => {
    stopPolling();
    unlistenBus?.();
    unlistenStatus?.();
    unlistenMem?.();
  });
}

init();

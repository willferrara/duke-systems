import {
  THEME_PRESETS,
  escapeHtml,
  formatBytes,
  formatTimestamp,
  initWindowControls,
  invokeCommand,
  listenEvent,
  openExternalUrl
} from "./api.js";

const ROW_HEIGHT_PX = 40;
const OVERSCAN_ROWS = 5;

const state = {
  activeTab: "telemetry",
  pollMs: 1000,
  pollTimer: null,
  processes: [],
  filteredProcesses: [],
  processFilter: "all",
  processSort: "ram_desc",
  processQuery: "",
  driverReport: null,
  modalConfirmFn: null,
  activeTheme: localStorage.getItem("duke_monitor_theme") || "dark"
};

function el(id) {
  return document.getElementById(id);
}

function svgIcon(name) {
  return `<svg class="icon" aria-hidden="true"><use href="#${name}"/></svg>`;
}

function showToast(msg, tone = "info") {
  const c = el("toastContainer");
  if (!c) return;
  const t = document.createElement("div");
  t.className = `toast ${tone}`;
  t.textContent = msg;
  c.appendChild(t);
  setTimeout(() => t.remove(), 3800);
}

function applyTheme(id) {
  const valid = THEME_PRESETS.some((t) => t.id === id) ? id : "dark";
  state.activeTheme = valid;
  document.body.setAttribute("data-theme", valid);
  localStorage.setItem("duke_monitor_theme", valid);
  const badge = el("activeThemeBadge");
  if (badge) {
    badge.textContent = THEME_PRESETS.find((t) => t.id === valid)?.name || "Obsidian Dark";
  }
}

function renderThemes() {
  const grid = el("themeGrid");
  if (!grid) return;
  grid.innerHTML = THEME_PRESETS.map(
    (t) => `
      <div class="theme-card ${t.id === state.activeTheme ? "active" : ""}" data-theme-id="${escapeHtml(t.id)}">
        <div class="theme-preview" data-preview-theme="${escapeHtml(t.id)}"></div>
        <div class="theme-card-body">
          <div class="theme-card-name">${escapeHtml(t.name)}</div>
          <div class="theme-card-desc">${escapeHtml(t.desc)}</div>
        </div>
      </div>
    `
  ).join("");
}

function switchTab(tab) {
  state.activeTab = tab;
  document.querySelectorAll(".nav-btn[data-tab]").forEach((b) => {
    b.classList.toggle("active", b.dataset.tab === tab);
  });
  document.querySelectorAll(".tab-panel").forEach((p) => {
    p.classList.toggle("active", p.id === `view-${tab}`);
  });
  const titleMap = {
    telemetry: "Telemetry",
    processes: "Processes",
    drivers: "Drivers",
    settings: "Settings"
  };
  if (el("topbarTitle")) el("topbarTitle").textContent = titleMap[tab] || "Telemetry";

  if (tab === "processes") refreshProcesses();
  if (tab === "drivers" && !state.driverReport) auditDrivers();
}

function renderGpus(gpus) {
  const grid = el("gpuGrid");
  if (!grid) return;
  if (!gpus || gpus.length === 0) {
    grid.innerHTML = `<div class="cockpit-card"><div class="card-title">No DirectX/NVML Display Adapters Reported</div></div>`;
    return;
  }

  grid.innerHTML = gpus
    .map((g) => {
      const vramPct = g.vramTotalMb > 0 ? Math.min(100, (g.vramUsedMb / g.vramTotalMb) * 100) : 0;
      const barTone = vramPct > 85 ? "danger" : vramPct > 65 ? "amber" : "cyan";
      const tempText = g.temperatureC != null ? `${g.temperatureC}°C` : "N/A";
      const loadText = g.utilizationPct != null ? `${Math.round(g.utilizationPct)}%` : "--";

      return `
        <article class="cockpit-card">
          <div class="card-header">
            <div>
              <h2 class="card-title">${escapeHtml(g.name)}</h2>
              <div class="sub-line mono-num">
                <span>${escapeHtml(g.vendor)}</span>
                <span>•</span>
                <span>${escapeHtml(g.backend)}</span>
              </div>
            </div>
            <div class="metric-right">
              <span class="big-metric-val mono-num ${g.temperatureC >= 80 ? "text-danger" : "text-cyan"}">${escapeHtml(tempText)}</span>
              <span class="metric-sub mono-num">Load ${escapeHtml(loadText)}</span>
            </div>
          </div>

          <div class="progress-track lg">
            <div class="progress-fill ${barTone}" style="width: ${vramPct.toFixed(1)}%"></div>
          </div>

          <div class="dense-metric-rows">
            <div class="dense-row">
              <span class="dense-k">VRAM Allocation</span>
              <span class="dense-v mono-num">${g.vramUsedMb} / ${g.vramTotalMb} MB</span>
            </div>
            <div class="dense-row">
              <span class="dense-k">Core / Memory Clock</span>
              <span class="dense-v mono-num">${g.gpuClockMhz ?? "--"} / ${g.memClockMhz ?? "--"} MHz</span>
            </div>
            <div class="dense-row">
              <span class="dense-k">Power Draw / Fan</span>
              <span class="dense-v mono-num">${g.powerDrawWatts != null ? `${g.powerDrawWatts.toFixed(1)} W` : "--"} · ${g.fanSpeedPct != null ? `${g.fanSpeedPct}%` : "Auto"}</span>
            </div>
          </div>
        </article>
      `;
    })
    .join("");
}

function renderCpu(cpu) {
  if (!cpu) return;
  if (el("cpuModelName")) el("cpuModelName").textContent = cpu.model || "Processor";
  if (el("cpuTotalPct")) el("cpuTotalPct").textContent = `${Math.round(cpu.totalUsagePct || 0)}%`;
  if (el("cpuFreqMhz")) el("cpuFreqMhz").textContent = `${cpu.frequencyMhz || 0} MHz`;
  if (el("cpuCoreSplitBadge")) {
    el("cpuCoreSplitBadge").textContent = cpu.hybridDetected
      ? `${cpu.pCoreCount}P + ${cpu.eCoreCount}E Threads`
      : `${cpu.logicalCores} Logical Threads`;
  }
  if (el("cpuPhysicalBadge")) el("cpuPhysicalBadge").textContent = `${cpu.physicalCores} Physical Cores`;
  if (el("cpuTotalBar")) el("cpuTotalBar").style.width = `${Math.min(100, cpu.totalUsagePct || 0).toFixed(1)}%`;

  const grid = el("cpuCoresGrid");
  if (!grid || !cpu.cores) return;
  grid.innerHTML = cpu.cores
    .map((c) => {
      const isE = String(c.coreType || "").toUpperCase().includes("E");
      const cls = isE ? "core-type-e" : "core-type-p";
      const barCls = isE ? "emerald" : "cyan";
      const pct = Math.min(100, Math.round(c.usagePct || 0));
      return `
        <div class="core-tile">
          <div class="core-head mono-num">
            <span class="${cls}">C${c.index} (${isE ? "E" : "P"})</span>
            <span>${pct}%</span>
          </div>
          <div class="progress-track">
            <div class="progress-fill ${barCls}" style="width: ${pct}%"></div>
          </div>
        </div>
      `;
    })
    .join("");
}

function renderMemory(mem) {
  if (!mem) return;
  const pct = Math.min(100, Number(mem.loadPct || 0));
  if (el("ramLoadPct")) el("ramLoadPct").textContent = `${pct}%`;
  if (el("ramLoadBar")) el("ramLoadBar").style.width = `${pct}%`;
  if (el("ramUsedGb")) el("ramUsedGb").textContent = `${Number(mem.usedGb || 0).toFixed(2)} GB`;
  if (el("ramAvailGb")) el("ramAvailGb").textContent = `${Number(mem.availableGb || 0).toFixed(2)} GB`;
  if (el("ramTotalGb")) el("ramTotalGb").textContent = `${Number(mem.totalGb || 0).toFixed(2)} GB`;
  if (el("ramOwnFootprint")) el("ramOwnFootprint").textContent = `${Number(mem.ownWorkingSetMb || 0).toFixed(1)} MB`;
  if (el("sidebarOwnRam")) el("sidebarOwnRam").textContent = `${Number(mem.ownWorkingSetMb || 0).toFixed(1)} MB`;
}

async function refreshTelemetry() {
  if (document.hidden || state.activeTab !== "telemetry") return;
  try {
    const snap = await invokeCommand("get_hardware_telemetry");
    renderGpus(snap.gpus);
    renderCpu(snap.cpu);
    renderMemory(snap.memory);
    if (el("sidebarDohShield")) el("sidebarDohShield").textContent = snap.dohShield === "ACTIVE" ? "Active" : "Standby";
    if (el("sidebarEcoStatus")) el("sidebarEcoStatus").textContent = snap.ecoSchedulerStatus || "Ready";
    if (el("ecoJailedBadge")) el("ecoJailedBadge").textContent = `Eco Jailed: ${snap.ecoJailedCount || 0}`;
    if (el("settingsDohHosts")) el("settingsDohHosts").textContent = String(snap.dohCachedHosts || 0);
  } catch (err) {
    showToast(String(err), "error");
  }
}

function applyProcessFilterAndSort() {
  const q = state.processQuery.trim().toLowerCase();
  const filtered = state.processes.filter((p) => {
    if (state.processFilter === "bloat" && !p.isBloat) return false;
    if (state.processFilter === "eco" && !p.ecoJailed) return false;
    if (state.processFilter === "boosted" && !p.boosted) return false;
    if (state.processFilter === "heavy" && p.ramMb <= 100) return false;
    if (q) {
      const hay = `${p.name} ${p.exePath} ${p.pid}`.toLowerCase();
      if (!hay.includes(q)) return false;
    }
    return true;
  });

  filtered.sort((a, b) => {
    switch (state.processSort) {
      case "cpu_desc":
        return b.cpuPct - a.cpuPct;
      case "name_asc":
        return a.name.localeCompare(b.name);
      case "pid_asc":
        return a.pid - b.pid;
      default:
        return b.ramMb - a.ramMb;
    }
  });

  state.filteredProcesses = filtered;
  if (el("processCountBadge")) el("processCountBadge").textContent = `${filtered.length} procs`;

  // Update Bloat Banner
  const bloat = state.processes.filter((p) => p.isBloat);
  const banner = el("bloatBanner");
  if (banner) {
    banner.classList.toggle("hidden", bloat.length === 0);
    if (el("bloatCountTitle")) {
      el("bloatCountTitle").textContent = `${bloat.length} Background Bloatware Process${bloat.length === 1 ? "" : "es"} Active`;
    }
    if (el("bloatPills")) {
      el("bloatPills").innerHTML = bloat
        .slice(0, 6)
        .map((b) => `<span class="badge badge-danger">${escapeHtml(b.name)}</span>`)
        .join("");
    }
  }

  renderVirtualizedProcessRows();
}

function renderVirtualizedProcessRows() {
  const scroller = el("processTableScroll");
  const tbody = el("processTableBody");
  if (!scroller || !tbody) return;

  const list = state.filteredProcesses;
  if (list.length === 0) {
    tbody.innerHTML = `<tr><td colspan="6" style="text-align:center;color:var(--text-muted);padding:24px;">No processes match current filters.</td></tr>`;
    return;
  }

  const scrollTop = scroller.scrollTop;
  const viewH = scroller.clientHeight || 480;
  const start = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT_PX) - OVERSCAN_ROWS);
  const visibleCount = Math.ceil(viewH / ROW_HEIGHT_PX) + OVERSCAN_ROWS * 2;
  const end = Math.min(list.length, start + visibleCount);

  const topPad = start * ROW_HEIGHT_PX;
  const bottomPad = (list.length - end) * ROW_HEIGHT_PX;

  let html = topPad > 0 ? `<tr style="height:${topPad}px"><td colspan="6"></td></tr>` : "";
  for (let i = start; i < end; i++) {
    const p = list[i];
    const badgeCls = p.boosted ? "badge-emerald" : p.ecoJailed ? "badge-cyan" : p.isBloat ? "badge-danger" : "";
    const statusTxt = p.boosted ? "Game Boost" : p.ecoJailed ? "Eco Jailed" : p.isBloat ? "Bloat" : p.status || "Normal";

    html += `
      <tr data-pid="${p.pid}">
        <td class="proc-name-cell">
          <div class="proc-name" title="${escapeHtml(p.name)}">${escapeHtml(p.name)}</div>
          <div class="proc-path mono-num" title="${escapeHtml(p.exePath)}">${escapeHtml(p.exePath || "--")}</div>
        </td>
        <td class="num-col mono-num">${p.pid}</td>
        <td class="num-col mono-num">${Number(p.cpuPct || 0).toFixed(1)}%</td>
        <td class="num-col mono-num">${Number(p.ramMb || 0).toFixed(1)} MB</td>
        <td><span class="badge ${badgeCls}">${escapeHtml(statusTxt)}</span></td>
        <td class="actions-col">
          <div class="row-actions">
            <button type="button" class="btn btn-secondary row-icon-btn" data-pact="eco" data-pid="${p.pid}" title="EcoQoS Efficiency Jail">${svgIcon("icon-pause")}</button>
            <button type="button" class="btn btn-secondary row-icon-btn" data-pact="boost" data-pid="${p.pid}" title="Boost High Priority">${svgIcon("icon-zap")}</button>
            <button type="button" class="btn btn-secondary row-icon-btn" data-pact="trim" data-pid="${p.pid}" title="Trim Process RAM">${svgIcon("icon-refresh-cw")}</button>
            <button type="button" class="btn btn-danger row-icon-btn" data-pact="kill" data-pid="${p.pid}" title="Terminate Process">${svgIcon("icon-trash-2")}</button>
          </div>
        </td>
      </tr>
    `;
  }
  if (bottomPad > 0) html += `<tr style="height:${bottomPad}px"><td colspan="6"></td></tr>`;
  tbody.innerHTML = html;
}

async function refreshProcesses() {
  try {
    state.processes = await invokeCommand("get_running_processes");
    applyProcessFilterAndSort();
  } catch (err) {
    showToast(String(err), "error");
  }
}

function renderDrivers(report) {
  const grid = el("driversGrid");
  if (!grid || !report) return;
  if (el("driverCheckedAtBadge")) el("driverCheckedAtBadge").textContent = `Checked ${formatTimestamp(report.checkedAt)}`;
  if (el("driversDirBadge")) el("driversDirBadge").textContent = report.driversDownloadDir || "--";

  const devs = report.devices || [];
  if (devs.length === 0) {
    grid.innerHTML = `<div class="cockpit-card empty-state-card">No supported display/chipset hardware reported.</div>`;
    return;
  }

  grid.innerHTML = devs
    .map((d) => {
      const badgeCls = d.updateAvailable ? "badge-amber" : "badge-emerald";
      const badgeText = d.updateAvailable ? `Update: ${d.latestVersion || "Available"}` : "Up to Date";
      return `
        <article class="cockpit-card">
          <div class="card-header">
            <div>
              <h2 class="card-title">${escapeHtml(d.name)}</h2>
              <div class="sub-line">${escapeHtml(d.vendor)} · ${escapeHtml(d.category)}</div>
            </div>
            <span class="badge ${badgeCls}">${escapeHtml(badgeText)}</span>
          </div>
          <div class="dense-metric-rows">
            <div class="dense-row"><span class="dense-k">Installed</span><span class="dense-v mono-num">${escapeHtml(d.installedDisplayVersion || d.installedVersion)}</span></div>
            <div class="dense-row"><span class="dense-k">Latest Official</span><span class="dense-v mono-num">${escapeHtml(d.latestVersion || "Latest")}</span></div>
            <div class="dense-row"><span class="dense-k">Driver Date</span><span class="dense-v mono-num">${escapeHtml(d.driverDate || "--")}</span></div>
          </div>
          <p class="driver-release-notes">${escapeHtml(d.statusMessage || d.releaseNotes || "")}</p>
          <div class="driver-actions">
            ${
              d.downloadUrl
                ? `<button type="button" class="btn btn-primary btn-sm" data-drv-dl="${escapeHtml(d.id)}">${svgIcon("icon-download")}<span>Download Installer</span></button>`
                : ""
            }
            <button type="button" class="btn btn-secondary btn-sm" data-drv-portal="${escapeHtml(d.officialPortalUrl)}">Official Portal</button>
            <button type="button" class="btn btn-secondary btn-sm" data-drv-guide="${escapeHtml(d.id)}">Clean Install Guide</button>
          </div>
        </article>
      `;
    })
    .join("");
}

async function auditDrivers() {
  showToast("Auditing official vendor driver feeds over DoH...", "info");
  try {
    state.driverReport = await invokeCommand("check_driver_updates");
    renderDrivers(state.driverReport);
    showToast("Driver audit complete.", "success");
  } catch (err) {
    showToast(String(err), "error");
  }
}

function showModal(title, bodyHtml, onConfirm = null) {
  if (el("modalTitle")) el("modalTitle").textContent = title;
  if (el("modalBody")) el("modalBody").innerHTML = bodyHtml;
  state.modalConfirmFn = onConfirm;
  el("modalConfirmBtn")?.classList.toggle("hidden", !onConfirm);
  el("infoModal")?.classList.remove("hidden");
}

function startPolling() {
  stopPolling();
  state.pollTimer = setInterval(() => {
    if (document.hidden) return;
    if (state.activeTab === "telemetry") refreshTelemetry();
    if (state.activeTab === "processes") refreshProcesses();
  }, state.pollMs);
}

function stopPolling() {
  if (state.pollTimer) {
    clearInterval(state.pollTimer);
    state.pollTimer = null;
  }
}

function wireEvents() {
  document.querySelectorAll(".nav-btn[data-tab]").forEach((btn) => {
    btn.addEventListener("click", () => switchTab(btn.dataset.tab));
  });

  el("titleProcessSearch")?.addEventListener("input", (e) => {
    state.processQuery = e.target.value || "";
    if (el("processSearchInput")) el("processSearchInput").value = state.processQuery;
    if (state.activeTab !== "processes" && state.processQuery.trim()) {
      switchTab("processes");
    } else if (state.activeTab === "processes") {
      applyProcessFilterAndSort();
    }
  });

  el("processSearchInput")?.addEventListener("input", (e) => {
    state.processQuery = e.target.value || "";
    if (el("titleProcessSearch")) el("titleProcessSearch").value = state.processQuery;
    applyProcessFilterAndSort();
  });

  el("processFilterGroup")?.addEventListener("click", (e) => {
    const btn = e.target.closest(".filter-btn[data-filter]");
    if (!btn) return;
    state.processFilter = btn.dataset.filter;
    document.querySelectorAll("#processFilterGroup .filter-btn").forEach((b) => b.classList.toggle("active", b === btn));
    applyProcessFilterAndSort();
  });

  el("processSortSelect")?.addEventListener("change", (e) => {
    state.processSort = e.target.value;
    applyProcessFilterAndSort();
  });

  el("processTableScroll")?.addEventListener("scroll", renderVirtualizedProcessRows, { passive: true });

  el("processTableBody")?.addEventListener("click", async (e) => {
    const btn = e.target.closest("[data-pact][data-pid]");
    if (!btn) return;
    const pid = Number(btn.dataset.pid);
    const act = btn.dataset.pact;

    try {
      if (act === "eco") {
        const res = await invokeCommand("set_process_eco_qos", { pid });
        showToast(res.message, "success");
      } else if (act === "boost") {
        const res = await invokeCommand("boost_game_process", { pid });
        showToast(res.message, "success");
      } else if (act === "trim") {
        const res = await invokeCommand("trim_process_ram", { pid });
        showToast(res.message, "info");
      } else if (act === "kill") {
        const res = await invokeCommand("kill_process", { pid });
        showToast(res.message, "info");
      }
      await refreshProcesses();
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("killAllBloatBtn")?.addEventListener("click", async () => {
    try {
      const res = await invokeCommand("kill_all_bloat_processes");
      showToast(res.message, "success");
      await refreshProcesses();
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  const runPurge = async () => {
    try {
      const res = await invokeCommand("purge_system_ram");
      const msg = `Freed ${Number(res.freedMb || 0).toFixed(1)} MB across ${res.processesTrimmed} processes (${res.beforeUsedGb.toFixed(2)} -> ${res.afterUsedGb.toFixed(2)} GB).`;
      if (el("purgeResultBox")) el("purgeResultBox").textContent = msg;
      if (el("lastPurgeBadge")) el("lastPurgeBadge").textContent = `Last Purge: -${res.freedMb.toFixed(0)} MB`;
      showToast(msg, "success");
      refreshTelemetry();
    } catch (err) {
      showToast(String(err), "error");
    }
  };

  el("topPurgeRamBtn")?.addEventListener("click", runPurge);
  el("purgeRamMainBtn")?.addEventListener("click", runPurge);
  el("topRefreshBtn")?.addEventListener("click", () => {
    if (state.activeTab === "processes") refreshProcesses();
    else if (state.activeTab === "drivers") auditDrivers();
    else refreshTelemetry();
  });

  el("scanDriversBtn")?.addEventListener("click", auditDrivers);
  el("openDriversFolderBtn")?.addEventListener("click", () => invokeCommand("open_drivers_folder"));

  el("driversGrid")?.addEventListener("click", async (e) => {
    const dlBtn = e.target.closest("[data-drv-dl]");
    if (dlBtn && state.driverReport) {
      const d = state.driverReport.devices.find((x) => x.id === dlBtn.dataset.drvDl);
      if (!d || !d.downloadUrl) return;
      el("driverDownloadBanner")?.classList.remove("hidden");
      try {
        const res = await invokeCommand("download_clean_driver", {
          url: d.downloadUrl,
          filename: d.suggestedFilename || "driver_installer.exe",
          vendor: d.vendor
        });
        if (el("downloadCompleteHint")) {
          el("downloadCompleteHint").textContent = `Saved ${formatBytes(res.sizeBytes)} -> ${res.savedPath}`;
        }
        showToast(`Downloaded ${res.fileName}`, "success");
      } catch (err) {
        showToast(String(err), "error");
      }
      return;
    }

    const portal = e.target.closest("[data-drv-portal]");
    if (portal) {
      openExternalUrl(portal.dataset.drvPortal);
      return;
    }

    const guide = e.target.closest("[data-drv-guide]");
    if (guide && state.driverReport) {
      const d = state.driverReport.devices.find((x) => x.id === guide.dataset.drvGuide);
      if (d) showModal(`${d.name} Clean Install Notes`, `<p>${escapeHtml(d.cleanInstallGuide)}</p>`);
    }
  });

  el("autoTrimToggle")?.addEventListener("change", (e) =>
    invokeCommand("set_auto_trim_enabled", { enabled: e.target.checked }).catch(() => {})
  );

  el("pollIntervalSelect")?.addEventListener("change", (e) => {
    state.pollMs = Number(e.target.value) || 1000;
    if (el("topbarBadge")) el("topbarBadge").textContent = `Live · ${state.pollMs} ms`;
    if (el("telemetryPollBadge")) el("telemetryPollBadge").textContent = `Polling: ${state.pollMs} ms`;
    startPolling();
  });

  el("trimOwnRamBtn")?.addEventListener("click", async () => {
    const payload = await invokeCommand("trim_working_set");
    if (el("sidebarOwnRam")) el("sidebarOwnRam").textContent = `${Number(payload.workingSetMb || 0).toFixed(1)} MB`;
    showToast("Duke Monitor working set trimmed.", "success");
  });

  el("themeGrid")?.addEventListener("click", (e) => {
    const card = e.target.closest(".theme-card[data-theme-id]");
    if (!card) return;
    applyTheme(card.dataset.themeId);
    renderThemes();
  });

  el("modalCloseBtn")?.addEventListener("click", () => el("infoModal")?.classList.add("hidden"));
  el("modalCancelBtn")?.addEventListener("click", () => el("infoModal")?.classList.add("hidden"));
  el("modalConfirmBtn")?.addEventListener("click", async () => {
    el("infoModal")?.classList.add("hidden");
    if (state.modalConfirmFn) await state.modalConfirmFn();
  });

  document.addEventListener("visibilitychange", () => {
    if (!document.hidden) {
      refreshTelemetry();
      startPolling();
    } else {
      stopPolling();
    }
  });
}

async function init() {
  applyTheme(state.activeTheme);
  renderThemes();
  initWindowControls({
    onCtrlK: () => {
      const inp = el("titleProcessSearch");
      if (inp) {
        inp.focus();
        inp.select();
      }
    }
  });
  wireEvents();

  await listenEvent("driver-download-progress", (p) => {
    if (!p) return;
    el("driverDownloadBanner")?.classList.remove("hidden");
    if (el("downloadFileName")) el("downloadFileName").textContent = p.fileName || "Downloading...";
    if (el("downloadStatsText")) el("downloadStatsText").textContent = `${Number(p.progressPct || 0).toFixed(1)}% · ${Number(p.speedMbps || 0).toFixed(1)} MB/s`;
    if (el("downloadProgressBar")) el("downloadProgressBar").style.width = `${Math.min(100, p.progressPct || 0)}%`;
  });

  await listenEvent("memory-telemetry", (p) => {
    if (document.hidden || !p) return;
    if (el("sidebarOwnRam")) el("sidebarOwnRam").textContent = `${Number(p.workingSetMb || 0).toFixed(1)} MB`;
  });

  const sys = await invokeCommand("get_system_telemetry").catch(() => null);
  if (sys) {
    if (el("autoTrimToggle")) el("autoTrimToggle").checked = Boolean(sys.autoTrimEnabled);
    if (el("settingsStorageMode")) el("settingsStorageMode").textContent = sys.portableMode ? "Portable" : "AppData";
  }

  await refreshTelemetry();
  startPolling();
}

init();

import {
  COMMON_KEYS,
  CURVE_MODES,
  GYRO_MODES,
  REMAP_ACTION_TYPES,
  REMAP_SOURCE_GROUPS,
  THEME_PRESETS,
  clamp,
  hexByte,
  initWindowChrome,
  invokeCommand,
  listenEvent,
  pickExecutableFile,
  pickProfileJsonFile,
  saveProfileJsonFile
} from "./api.js";

const TAB_META = {
  devices: {
    title: "Devices & Live Test",
    sub: "Real-time Xbox, Logitech HID++ 2.0, EdgeTX/OpenTX FPV, and Gyro-to-Mouse telemetry"
  },
  curves: {
    title: "Curves & Tuning",
    sub: "Deadzones, 5-point Bezier stick curves, hair-triggers, Logitech DPI stages, Betaflight FPV rates & gyro aim"
  },
  remaps: {
    title: "Remappings & Macros",
    sub: "Per-button remapping matrix, rapid-fire burst clickers & sub-millisecond macro sequencer"
  },
  profiles: {
    title: "Profiles & Settings",
    sub: "Per-game 500 ms .exe auto-switching, < 10 MB tray daemon settings, Raw HID Inspector & 10 SVG themes"
  }
};

const XBOX_LED_MAP = [
  [0x1000, "A"],
  [0x2000, "B"],
  [0x4000, "X"],
  [0x8000, "Y"],
  [0x0100, "LB"],
  [0x0200, "RB"],
  [0x0040, "LS"],
  [0x0080, "RS"],
  [0x0020, "BACK"],
  [0x0010, "START"],
  [0x0001, "UP"],
  [0x0002, "DOWN"],
  [0x0004, "LEFT"],
  [0x0008, "RIGHT"]
];

const MOUSE_LED_MAP = [
  [1 << 0, "LMB"],
  [1 << 1, "RMB"],
  [1 << 2, "MIDDLE"],
  [1 << 3, "MOUSE4"],
  [1 << 4, "MOUSE5"]
];

const appState = {
  store: null,
  activeTab: "devices",
  selectedStick: "left",
  selectedFpvAxis: "roll",
  selectedProfileId: null,
  selectedMacroId: null,
  inspectorEnabled: false,
  inspectorDeviceId: "",
  telemetry: null,
  lastDeviceSignature: "",
  saveTimer: null,
  curvePointsCache: [],
  bfCurvesCache: { roll: [], pitch: [], yaw: [] },
  draggingKnotIdx: -1
};

const el = (id) => document.getElementById(id);

function showToast(msg, type = "info") {
  const host = el("toast-container");
  if (!host) return;
  const item = document.createElement("div");
  item.className = `toast ${type}`;
  item.textContent = msg;
  host.appendChild(item);
  setTimeout(() => {
    item.remove();
  }, 3200);
}

function getActiveProfile() {
  if (!appState.store || !appState.store.profiles.length) return null;
  const activeId = appState.store.settings.activeProfileId;
  return (
    appState.store.profiles.find((p) => p.id === activeId) ||
    appState.store.profiles[0]
  );
}

function getEditingProfile() {
  if (!appState.store || !appState.store.profiles.length) return null;
  return (
    appState.store.profiles.find((p) => p.id === appState.selectedProfileId) ||
    getActiveProfile()
  );
}

function scheduleSaveActiveProfile() {
  const prof = getActiveProfile();
  if (!prof) return;
  clearTimeout(appState.saveTimer);
  appState.saveTimer = setTimeout(async () => {
    try {
      appState.store = await invokeCommand("save_profile", { profile: prof });
      renderSidebarProfiles();
    } catch (err) {
      showToast(`Save error: ${err}`, "error");
    }
  }, 90);
}

async function saveProfileNow(prof) {
  clearTimeout(appState.saveTimer);
  try {
    appState.store = await invokeCommand("save_profile", { profile: prof });
    renderAllStatic();
  } catch (err) {
    showToast(`Save failed: ${err}`, "error");
  }
}

async function saveSettingsNow() {
  if (!appState.store) return;
  try {
    appState.store = await invokeCommand("update_settings", {
      settings: appState.store.settings
    });
    renderSettingsSection();
    renderFooterFromStore();
  } catch (err) {
    showToast(`Settings error: ${err}`, "error");
  }
}

function applyTheme(themeId) {
  const valid = THEME_PRESETS.some((t) => t.id === themeId) ? themeId : "dark";
  document.documentElement.setAttribute("data-theme", valid);
  document.body.setAttribute("data-theme", valid);
  localStorage.setItem("duke_input_theme", valid);
  renderThemeGallery();
}

function syncVisibilityState() {
  const visible = document.visibilityState === "visible";
  document.body.classList.toggle("ui-idle-paused", !visible);
  invokeCommand("set_ui_visible", { visible }).catch(() => {});
  updateInspectorBackendState();
}

function updateInspectorBackendState() {
  const active =
    document.visibilityState === "visible" &&
    appState.activeTab === "profiles" &&
    appState.inspectorEnabled;
  invokeCommand("set_inspector_device", {
    deviceId: appState.inspectorDeviceId || "",
    active
  }).catch(() => {});
}

// ==========================================================================
// NAVIGATION & STATIC SELECTORS
// ==========================================================================

function switchTab(tabId) {
  appState.activeTab = tabId;
  document.querySelectorAll(".nav-btn").forEach((b) => {
    b.classList.toggle("active", b.dataset.tab === tabId);
  });
  document.querySelectorAll(".tab-view").forEach((v) => {
    v.classList.toggle("active", v.id === `view-${tabId}`);
  });
  const meta = TAB_META[tabId] || TAB_META.devices;
  el("topbar-title").textContent = meta.title;
  el("topbar-subtitle").textContent = meta.sub;
  updateInspectorBackendState();
  if (tabId === "curves") {
    renderCurveControls();
    refreshStickCurveSvg();
    refreshBetaflightCurveSvg();
  } else if (tabId === "remaps") {
    renderRemappings();
    renderMacros();
  } else if (tabId === "profiles") {
    renderProfilesManager();
    renderSettingsSection();
    renderThemeGallery();
  }
}

function initStaticDropdowns() {
  const curveSel = el("cfg-stick-curve-mode");
  curveSel.replaceChildren();
  for (const m of CURVE_MODES) {
    const opt = document.createElement("option");
    opt.value = m.id;
    opt.textContent = m.label;
    curveSel.appendChild(opt);
  }

  const gyroSel = el("cfg-gyro-mode");
  gyroSel.replaceChildren();
  for (const g of GYRO_MODES) {
    const opt = document.createElement("option");
    opt.value = g.id;
    opt.textContent = g.label;
    gyroSel.appendChild(opt);
  }

  buildStickCurveGrid();
  buildFpvRateGrid();
  initButtonLedArrays();
  initInspectorHexGrid();
}

function initButtonLedArrays() {
  const xboxRow = el("xbox-btn-leds");
  xboxRow.replaceChildren();
  for (const [mask, label] of XBOX_LED_MAP) {
    const chip = document.createElement("span");
    chip.className = "led-chip";
    chip.dataset.mask = String(mask);
    chip.textContent = label;
    xboxRow.appendChild(chip);
  }

  const mouseRow = el("mouse-btn-leds");
  mouseRow.replaceChildren();
  for (const [mask, label] of MOUSE_LED_MAP) {
    const chip = document.createElement("span");
    chip.className = "led-chip";
    chip.dataset.mask = String(mask);
    chip.textContent = label;
    mouseRow.appendChild(chip);
  }

  const fpvGrid = el("fpv-channels-grid");
  fpvGrid.replaceChildren();
  const labels = ["CH1 ROLL", "CH2 PITCH", "CH3 THR", "CH4 YAW"];
  for (let i = 0; i < 16; i++) {
    const card = document.createElement("div");
    card.className = "fpv-ch-card";
    const head = document.createElement("div");
    head.className = "fpv-ch-head";
    const name = document.createElement("span");
    name.textContent = labels[i] || `CH${i + 1}`;
    const val = document.createElement("span");
    val.id = `fpv-ch-val-${i}`;
    val.textContent = "0%";
    head.append(name, val);
    const track = document.createElement("div");
    track.className = "fpv-bar-track";
    const fill = document.createElement("div");
    fill.className = "fpv-bar-fill";
    fill.id = `fpv-ch-bar-${i}`;
    track.appendChild(fill);
    card.append(head, track);
    fpvGrid.appendChild(card);
  }
}

function initInspectorHexGrid() {
  const grid = el("inspector-hex-grid");
  grid.replaceChildren();
  for (let i = 0; i < 64; i++) {
    const cell = document.createElement("div");
    cell.className = "hex-cell";
    cell.id = `hex-cell-${i}`;

    const top = document.createElement("div");
    top.className = "hex-top";
    const idx = document.createElement("span");
    idx.textContent = `#${String(i).padStart(2, "0")}`;
    const mm = document.createElement("span");
    mm.id = `hex-mm-${i}`;
    mm.textContent = "—";
    top.append(idx, mm);

    const val = document.createElement("div");
    val.className = "hex-val";
    val.id = `hex-val-${i}`;
    val.textContent = "00";

    const bits = document.createElement("div");
    bits.className = "hex-bits";
    for (let b = 7; b >= 0; b--) {
      const led = document.createElement("span");
      led.className = "bit-led";
      led.id = `hex-bit-${i}-${b}`;
      bits.appendChild(led);
    }

    cell.append(top, val, bits);
    grid.appendChild(cell);
  }
}

// ==========================================================================
// RENDER SIDEBAR & TOP PROFILE SLOTS
// ==========================================================================

function renderAllStatic() {
  renderSidebarProfiles();
  renderFooterFromStore();
  renderLiveDpiButtons();
  renderCurveControls();
  refreshStickCurveSvg();
  refreshBetaflightCurveSvg();
  renderRemappings();
  renderMacros();
  renderProfilesManager();
  renderSettingsSection();
  renderThemeGallery();
}

function renderSidebarProfiles() {
  const list = el("sidebar-profile-list");
  if (!appState.store) return;
  const activeId = appState.store.settings.activeProfileId;
  list.replaceChildren();

  for (const prof of appState.store.profiles) {
    const btn = document.createElement("button");
    btn.className = `side-prof-btn ${prof.id === activeId ? "active" : ""}`;
    const nameSpan = document.createElement("span");
    nameSpan.className = "truncate";
    nameSpan.textContent = prof.name;
    btn.appendChild(nameSpan);

    if (prof.linkedExes && prof.linkedExes.length > 0) {
      const tag = document.createElement("span");
      tag.className = "mini-pill";
      tag.textContent = `${prof.linkedExes.length} EXE`;
      btn.appendChild(tag);
    }

    btn.addEventListener("click", async () => {
      try {
        appState.store = await invokeCommand("set_active_profile", {
          profileId: prof.id
        });
        appState.selectedProfileId = prof.id;
        renderAllStatic();
        showToast(`Activated "${prof.name}"`, "ok");
      } catch (err) {
        showToast(String(err), "error");
      }
    });
    list.appendChild(btn);
  }
}

function renderFooterFromStore() {
  if (!appState.store) return;
  const s = appState.store.settings;
  const prof = getActiveProfile();
  el("foot-profile-name").textContent = prof ? prof.name : "Default";
  el("foot-autoswitch").textContent = s.autoSwitchEnabled ? "On" : "Off";
  el("foot-autoswitch").className = `tele-val ${s.autoSwitchEnabled ? "ok" : "warn"}`;
  el("sidebar-autoswitch-pill").textContent = s.autoSwitchEnabled ? "Auto On" : "Manual";
  const pauseBtn = el("btn-pause-engine");
  const paused = Boolean(s.enginePaused);
  pauseBtn.innerHTML = `<svg class="icon"><use href="#icon-${paused ? "play" : "pause"}"/></svg><span>${
    paused ? "Resume 1000 Hz Engine" : "Pause Engine"
  }</span>`;
}

function renderLiveDpiButtons() {
  const host = el("live-dpi-stages");
  const prof = getActiveProfile();
  if (!host || !prof) return;
  host.replaceChildren();
  prof.mouse.dpiStages.forEach((dpi, idx) => {
    const btn = document.createElement("button");
    btn.className = `dpi-pill-btn ${prof.mouse.activeDpiStage === idx ? "active" : ""}`;
    btn.textContent = `${dpi}`;
    btn.addEventListener("click", async () => {
      try {
        appState.store = await invokeCommand("apply_dpi_stage", { stageIdx: idx });
        renderLiveDpiButtons();
        renderCurveControls();
        showToast(`DPI Stage ${idx + 1} -> ${dpi} DPI`, "ok");
      } catch (err) {
        showToast(String(err), "error");
      }
    });
    host.appendChild(btn);
  });
}

// ==========================================================================
// 30 FPS TELEMETRY UPDATES (GATED BY DOCUMENT VISIBILITY)
// ==========================================================================

function renderTelemetry(snap) {
  if (!snap) return;
  appState.telemetry = snap;

  // Footer + Topbar
  el("foot-engine-hz").textContent = snap.enginePaused
    ? "PAUSED"
    : `${snap.engineHz || 0} HZ`;
  el("foot-engine-hz").className = `tele-val ${snap.enginePaused ? "warn" : "accent"}`;

  const ramText =
    snap.workingSetMb > 0 ? `${snap.workingSetMb.toFixed(1)} MB` : "< 10 MB";
  el("foot-ram-mb").textContent = ramText;
  if (el("settings-ram-readout")) {
    el("settings-ram-readout").textContent = ramText;
  }
  el("foot-profile-name").textContent = snap.activeProfileName || "—";
  el("foot-autoswitch").textContent = snap.autoSwitchEnabled ? "ON" : "OFF";
  el("foot-fg-exe").textContent = snap.foregroundExe || "—";

  const px = el("pill-xbox");
  px.textContent = snap.xbox.connected ? `Xbox: Slot ${snap.xbox.slot + 1}` : "Xbox: Offline";
  px.classList.toggle("offline", !snap.xbox.connected);

  const pm = el("pill-mouse");
  pm.textContent = `Mouse: ${snap.mouse.pollingHz} Hz · ${snap.mouse.activeDpi} DPI`;
  pm.classList.remove("offline");

  const pf = el("pill-fpv");
  pf.textContent = snap.fpv.connected ? `FPV: ${snap.fpv.channelCount}ch Live` : "FPV TX: Offline";
  pf.classList.toggle("offline", !snap.fpv.connected);

  const pg = el("pill-gyro");
  pg.textContent = snap.gyro.connected
    ? snap.gyro.active
      ? "DualSense: Aiming"
      : "DualSense: Ready"
    : "DualSense: Offline";
  pg.classList.toggle("offline", !snap.gyro.connected);

  const rx = el("rail-xbox-state");
  if (rx) {
    rx.textContent = snap.xbox.connected ? `Slot ${snap.xbox.slot + 1}` : "Offline";
    rx.classList.toggle("live", snap.xbox.connected);
  }
  const rm = el("rail-mouse-state");
  if (rm) {
    rm.textContent = `${snap.mouse.pollingHz || 0} Hz`;
    rm.classList.add("live");
  }
  const rf = el("rail-fpv-state");
  if (rf) {
    rf.textContent = snap.fpv.connected ? `${snap.fpv.channelCount}ch` : "Offline";
    rf.classList.toggle("live", snap.fpv.connected);
  }
  const rg = el("rail-gyro-state");
  if (rg) {
    rg.textContent = snap.gyro.connected ? (snap.gyro.active ? "Aiming" : "Ready") : "Offline";
    rg.classList.toggle("live", snap.gyro.connected);
  }

  // Hardware Cards (only rebuild DOM when connected device list changes)
  const devSig = JSON.stringify(
    snap.devices.map((d) => [d.id, d.batteryPct, d.pollingHz])
  );
  if (devSig !== appState.lastDeviceSignature) {
    appState.lastDeviceSignature = devSig;
    renderDeviceCards(snap.devices);
    updateInspectorDropdown(snap.devices);
  }

  // Tab 1 live telemetry widgets
  if (appState.activeTab === "devices") {
    updateXboxVisualizer(snap.xbox);
    updateMouseVisualizer(snap.mouse);
    updateGyroVisualizer(snap.gyro);
    updateFpvVisualizer(snap.fpv);
  }

  // Tab 2 live curve tracking dot
  if (appState.activeTab === "curves") {
    updateStickCurveLiveDot(snap.xbox);
  }

  // Tab 3 live input indicator LEDs
  if (appState.activeTab === "remaps") {
    updateRemapInputLeds(snap);
  }

  // Tab 4 Raw HID Inspector
  if (appState.activeTab === "profiles" && appState.inspectorEnabled) {
    updateInspectorGrid(snap.rawInspector);
  }
}

function renderDeviceCards(devices) {
  const grid = el("devices-grid");
  el("device-count-badge").textContent = `${devices.length} ${devices.length === 1 ? "Device" : "Devices"}`;
  grid.replaceChildren();

  if (!devices.length) {
    const empty = document.createElement("div");
    empty.className = "tiny-meta";
    empty.textContent =
      "No custom HID controllers detected yet. Plug in an Xbox pad, Logitech mouse, RadioMaster FPV Tx, or DualSense/DS4.";
    grid.appendChild(empty);
    return;
  }

  for (const d of devices) {
    const card = document.createElement("div");
    card.className = "dev-card";

    const top = document.createElement("div");
    top.className = "dev-card-top";
    const cls = document.createElement("span");
    cls.className = "dev-class";
    cls.textContent = d.className;
    const badge = document.createElement("span");
    badge.className = "badge";
    if (d.batteryPct !== null && d.batteryPct !== undefined) {
      badge.textContent = `${d.batteryPct}% Batt`;
    } else if (d.pollingHz) {
      badge.textContent = `${d.pollingHz} Hz`;
    } else {
      badge.textContent = d.transport;
    }
    top.append(cls, badge);

    const name = document.createElement("div");
    name.className = "dev-name truncate";
    name.textContent = d.name;

    const meta = document.createElement("div");
    meta.className = "dev-meta";
    const vp = document.createElement("span");
    vp.textContent =
      d.vid || d.pid
        ? `VID:${d.vid.toString(16).toUpperCase().padStart(4, "0")} PID:${d.pid.toString(16).toUpperCase().padStart(4, "0")}`
        : "XInput / RawInput";
    const caps = document.createElement("span");
    const flags = [];
    if (d.supportsHidpp) flags.push("HID++");
    if (d.supportsGyro) flags.push("IMU");
    if (d.supportsFpv) flags.push("FPV");
    if (d.supportsRumble) flags.push("RUMBLE");
    caps.textContent = flags.join(" · ") || d.transport;
    meta.append(vp, caps);

    card.append(top, name, meta);
    grid.appendChild(card);
  }
}

function updateXboxVisualizer(xbox) {
  const prof = getActiveProfile();
  if (prof) {
    el("ls-inner-dz-ring").setAttribute("r", String(prof.leftStick.innerDeadzone * 100));
    el("ls-outer-dz-ring").setAttribute("r", String(prof.leftStick.outerDeadzone * 100));
    el("ls-anti-dz-ring").setAttribute("r", String(prof.leftStick.antiDeadzone * 100));
    el("ls-drift-label").textContent = `Offset: ${prof.leftStick.centerOffsetX.toFixed(3)}, ${prof.leftStick.centerOffsetY.toFixed(3)}`;

    el("rs-inner-dz-ring").setAttribute("r", String(prof.rightStick.innerDeadzone * 100));
    el("rs-outer-dz-ring").setAttribute("r", String(prof.rightStick.outerDeadzone * 100));
    el("rs-anti-dz-ring").setAttribute("r", String(prof.rightStick.antiDeadzone * 100));
    el("rs-drift-label").textContent = `Offset: ${prof.rightStick.centerOffsetX.toFixed(3)}, ${prof.rightStick.centerOffsetY.toFixed(3)}`;
  }

  el("ls-raw-dot").setAttribute("cx", (xbox.rawLx * 100).toFixed(1));
  el("ls-raw-dot").setAttribute("cy", (-xbox.rawLy * 100).toFixed(1));
  el("ls-out-dot").setAttribute("cx", (xbox.outLx * 100).toFixed(1));
  el("ls-out-dot").setAttribute("cy", (-xbox.outLy * 100).toFixed(1));
  el("ls-coords").textContent = `${xbox.outLx.toFixed(2)}, ${xbox.outLy.toFixed(2)}`;

  el("rs-raw-dot").setAttribute("cx", (xbox.rawRx * 100).toFixed(1));
  el("rs-raw-dot").setAttribute("cy", (-xbox.rawRy * 100).toFixed(1));
  el("rs-out-dot").setAttribute("cx", (xbox.outRx * 100).toFixed(1));
  el("rs-out-dot").setAttribute("cy", (-xbox.outRy * 100).toFixed(1));
  el("rs-coords").textContent = `${xbox.outRx.toFixed(2)}, ${xbox.outRy.toFixed(2)}`;

  el("lt-raw-bar").style.width = `${(xbox.rawLt * 100).toFixed(0)}%`;
  el("lt-out-bar").style.width = `${(xbox.outLt * 100).toFixed(0)}%`;
  el("lt-val-label").textContent = `${Math.round(xbox.rawLt * 100)}% -> ${Math.round(xbox.outLt * 100)}%`;

  el("rt-raw-bar").style.width = `${(xbox.rawRt * 100).toFixed(0)}%`;
  el("rt-out-bar").style.width = `${(xbox.outRt * 100).toFixed(0)}%`;
  el("rt-val-label").textContent = `${Math.round(xbox.rawRt * 100)}% -> ${Math.round(xbox.outRt * 100)}%`;

  el("xbox-btn-leds").querySelectorAll(".led-chip").forEach((chip) => {
    const mask = Number(chip.dataset.mask);
    chip.classList.toggle("lit", (xbox.buttonsMask & mask) !== 0);
  });
}

function updateMouseVisualizer(mouse) {
  el("mouse-active-name").textContent = mouse.activeName || "RawInput Windows Mouse";
  el("mouse-poll-hz").textContent = `${mouse.pollingHz || 0} HZ`;
  el("mouse-jitter-us").textContent = `${mouse.jitterUs || 0} us`;
  el("mouse-active-dpi").textContent = mouse.sniperActive
    ? `${mouse.activeDpi} (SNIPER)`
    : `${mouse.activeDpi}`;
  el("mouse-delta-xy").textContent = `${mouse.lastDx}, ${mouse.lastDy} (${mouse.wheelDelta})`;
  el("mouse-batt-badge").textContent =
    mouse.batteryPct !== null && mouse.batteryPct !== undefined
      ? `BATT: ${mouse.batteryPct}% (${mouse.batteryMv || 0}mV)`
      : mouse.hidppSupported
        ? "HID++ 2.0 READY"
        : "RAW INPUT";

  el("mouse-btn-leds").querySelectorAll(".led-chip").forEach((chip) => {
    const mask = Number(chip.dataset.mask);
    chip.classList.toggle("lit", (mouse.buttonsMask & mask) !== 0);
  });
}

function updateGyroVisualizer(gyro) {
  el("gyro-device-name").textContent = gyro.connected
    ? gyro.name
    : "Connect DualSense, DualShock 4, or Switch Pro for IMU gyro";
  el("gyro-state-badge").textContent = gyro.active
    ? "AIMING LIVE"
    : gyro.touchpadDown
      ? "TOUCHPAD DOWN"
      : gyro.connected
        ? "STANDBY"
        : "INACTIVE";
  el("gyro-yaw-dps").textContent = gyro.yawDps.toFixed(1);
  el("gyro-pitch-dps").textContent = gyro.pitchDps.toFixed(1);
  el("gyro-roll-dps").textContent = gyro.rollDps.toFixed(1);
  el("gyro-mouse-delta").textContent = `${gyro.mouseDx}, ${gyro.mouseDy}`;
}

function updateFpvVisualizer(fpv) {
  el("fpv-device-name").textContent = fpv.connected
    ? fpv.name
    : "Connect RadioMaster Pocket / Boxer / TX16S via USB-C Joystick mode";
  el("fpv-rates-live").textContent = `R: ${Math.round(fpv.rollDps)}°/s · P: ${Math.round(fpv.pitchDps)}°/s · Y: ${Math.round(fpv.yawDps)}°/s`;

  const swRow = el("fpv-switches-row");
  const swNames = ["SA", "SB", "SC", "SD", "SE"];
  const swLabels = ["LOW", "MID", "HIGH"];
  swRow.replaceChildren();
  for (let s = 0; s < 5; s++) {
    const pill = document.createElement("span");
    const state = fpv.switches[s] || 0;
    pill.className = "fpv-sw-pill";
    pill.textContent = `${swNames[s]}: ${swLabels[state] || "LOW"}`;
    if (state > 0) {
      pill.style.borderColor = state === 2 ? "var(--accent-primary)" : "var(--ok)";
      pill.style.color = "var(--steam-blue)";
    }
    swRow.appendChild(pill);
  }

  for (let i = 0; i < 16; i++) {
    const valEl = el(`fpv-ch-val-${i}`);
    const barEl = el(`fpv-ch-bar-${i}`);
    if (!valEl || !barEl) continue;
    const val = fpv.channelsOut[i] ?? 0;
    const isUnipolarThr = i === 2 && getActiveProfile()?.fpv?.throttleUnipolar;
    if (isUnipolarThr) {
      const pct = clamp(val * 100, 0, 100);
      valEl.textContent = `${Math.round(pct)}%`;
      barEl.style.left = "0%";
      barEl.style.width = `${pct.toFixed(1)}%`;
    } else {
      valEl.textContent = `${Math.round(val * 100)}%`;
      if (val >= 0) {
        barEl.style.left = "50%";
        barEl.style.width = `${clamp(val * 50, 0, 50).toFixed(1)}%`;
      } else {
        const w = clamp(-val * 50, 0, 50);
        barEl.style.left = `${(50 - w).toFixed(1)}%`;
        barEl.style.width = `${w.toFixed(1)}%`;
      }
    }
  }
}

// ==========================================================================
// TAB 2: INTERACTIVE CURVES & TUNING
// ==========================================================================

function buildStickCurveGrid() {
  const g = el("stick-curve-grid");
  g.replaceChildren();
  for (let i = 0; i <= 4; i++) {
    const pos = 24 + i * 68;
    const h = document.createElementNS("http://www.w3.org/2000/svg", "line");
    h.setAttribute("x1", "24");
    h.setAttribute("y1", String(pos));
    h.setAttribute("x2", "296");
    h.setAttribute("y2", String(pos));
    h.setAttribute("class", "curve-grid-line");

    const v = document.createElementNS("http://www.w3.org/2000/svg", "line");
    v.setAttribute("x1", String(pos));
    v.setAttribute("y1", "24");
    v.setAttribute("x2", String(pos));
    v.setAttribute("y2", "296");
    v.setAttribute("class", "curve-grid-line");
    g.append(h, v);
  }
}

function buildFpvRateGrid() {
  const g = el("fpv-rate-grid");
  g.replaceChildren();
  for (let i = 0; i <= 4; i++) {
    const y = 18 + i * 46;
    const h = document.createElementNS("http://www.w3.org/2000/svg", "line");
    h.setAttribute("x1", "20");
    h.setAttribute("y1", String(y));
    h.setAttribute("x2", "280");
    h.setAttribute("y2", String(y));
    h.setAttribute("class", "curve-grid-line");
    g.appendChild(h);
  }
}

function getSelectedStickConfig(prof) {
  return appState.selectedStick === "right" ? prof.rightStick : prof.leftStick;
}

function getSelectedFpvAxisConfig(prof) {
  if (appState.selectedFpvAxis === "pitch") return prof.fpv.pitchRates;
  if (appState.selectedFpvAxis === "yaw") return prof.fpv.yawRates;
  return prof.fpv.rollRates;
}

function renderCurveControls() {
  const prof = getActiveProfile();
  if (!prof) return;
  const st = getSelectedStickConfig(prof);

  el("cfg-stick-shape").value = st.shape;
  el("cfg-stick-curve-mode").value = st.curveMode;

  el("cfg-inner-dz").value = String(Math.round(st.innerDeadzone * 100));
  el("val-inner-dz").textContent = `${Math.round(st.innerDeadzone * 100)}%`;

  el("cfg-outer-dz").value = String(Math.round(st.outerDeadzone * 100));
  el("val-outer-dz").textContent = `${Math.round(st.outerDeadzone * 100)}%`;

  el("cfg-anti-dz").value = String(Math.round(st.antiDeadzone * 100));
  el("val-anti-dz").textContent = `${Math.round(st.antiDeadzone * 100)}%`;

  el("cfg-expo-power").value = String(st.expoPower);
  el("val-expo-power").textContent = Number(st.expoPower).toFixed(2);

  // Triggers & Rumble
  el("cfg-lt-thr").value = String(Math.round(prof.leftTrigger.activationThreshold * 100));
  el("val-lt-thr").textContent = `${Math.round(prof.leftTrigger.activationThreshold * 100)}%`;
  el("cfg-lt-digital").checked = prof.leftTrigger.hairTriggerDigital;
  el("cfg-lt-invert").checked = prof.leftTrigger.invert;

  el("cfg-rt-thr").value = String(Math.round(prof.rightTrigger.activationThreshold * 100));
  el("val-rt-thr").textContent = `${Math.round(prof.rightTrigger.activationThreshold * 100)}%`;
  el("cfg-rt-digital").checked = prof.rightTrigger.hairTriggerDigital;
  el("cfg-rt-invert").checked = prof.rightTrigger.invert;

  el("cfg-rumble-l").value = String(Math.round(prof.rumbleLeftScale * 100));
  el("val-rumble-l").textContent = `${Math.round(prof.rumbleLeftScale * 100)}%`;
  el("cfg-rumble-r").value = String(Math.round(prof.rumbleRightScale * 100));
  el("val-rumble-r").textContent = `${Math.round(prof.rumbleRightScale * 100)}%`;

  // Logitech Mouse DPI
  for (let i = 0; i < 4; i++) {
    el(`cfg-dpi-${i}`).value = String(prof.mouse.dpiStages[i]);
  }
  el("cfg-active-dpi-stage").value = String(prof.mouse.activeDpiStage);
  el("cfg-sniper-dpi").value = String(prof.mouse.sniperDpi);
  el("cfg-rapid-cps").value = String(prof.mouse.rapidFireCps);
  el("cfg-dpi-mult").value = String(prof.mouse.dpiMultiplier);
  el("val-dpi-mult").textContent = `${Number(prof.mouse.dpiMultiplier).toFixed(2)}x`;

  // FPV
  el("cfg-fpv-order").value = prof.fpv.channelOrder;
  el("cfg-fpv-unipolar").checked = prof.fpv.throttleUnipolar;
  const ax = getSelectedFpvAxisConfig(prof);
  el("cfg-fpv-rc").value = String(ax.rcRate);
  el("val-fpv-rc").textContent = Number(ax.rcRate).toFixed(2);
  el("cfg-fpv-super").value = String(ax.superRate);
  el("val-fpv-super").textContent = Number(ax.superRate).toFixed(2);
  el("cfg-fpv-expo").value = String(ax.expo);
  el("val-fpv-expo").textContent = Number(ax.expo).toFixed(2);

  // Gyro
  el("cfg-gyro-mode").value = prof.gyro.activationMode;
  el("cfg-gyro-sens").value = String(prof.gyro.sensitivity);
  el("val-gyro-sens").textContent = `${Number(prof.gyro.sensitivity).toFixed(1)}x`;
  el("cfg-gyro-vratio").value = String(prof.gyro.verticalRatio);
  el("val-gyro-vratio").textContent = Number(prof.gyro.verticalRatio).toFixed(2);
  el("cfg-gyro-floor").value = String(prof.gyro.noiseFloorDps);
  el("val-gyro-floor").textContent = `${Number(prof.gyro.noiseFloorDps).toFixed(1)}°/s`;
  el("cfg-gyro-smooth").value = String(Math.round(prof.gyro.smoothing * 100));
  el("val-gyro-smooth").textContent = `${Math.round(prof.gyro.smoothing * 100)}%`;
  el("cfg-gyro-invx").checked = prof.gyro.invertX;
  el("cfg-gyro-invy").checked = prof.gyro.invertY;
}

async function refreshStickCurveSvg() {
  const prof = getActiveProfile();
  if (!prof) return;
  const cfg = getSelectedStickConfig(prof);
  const pts = await invokeCommand("preview_curve_points", { config: cfg });
  appState.curvePointsCache = pts;

  const dzBand = el("curve-dz-shade");
  dzBand.setAttribute("width", (cfg.innerDeadzone * 272).toFixed(1));

  const d = pts
    .map(([x, y], i) => {
      const px = 24 + x * 272;
      const py = 296 - y * 272;
      return `${i === 0 ? "M" : "L"}${px.toFixed(1)},${py.toFixed(1)}`;
    })
    .join(" ");
  el("stick-curve-path").setAttribute("d", d);

  const handlesG = el("stick-curve-handles");
  handlesG.replaceChildren();
  if (cfg.curveMode === "Bezier5Point") {
    cfg.customPoints.forEach((pt, idx) => {
      const c = document.createElementNS("http://www.w3.org/2000/svg", "circle");
      c.setAttribute("cx", String(24 + pt[0] * 272));
      c.setAttribute("cy", String(296 - pt[1] * 272));
      c.setAttribute("r", "6");
      c.setAttribute("class", "curve-handle");
      c.addEventListener("pointerdown", (ev) => {
        ev.preventDefault();
        appState.draggingKnotIdx = idx;
      });
      handlesG.appendChild(c);
    });
  }
}

function updateStickCurveLiveDot(xbox) {
  const isRight = appState.selectedStick === "right";
  const rx = isRight ? xbox.rawRx : xbox.rawLx;
  const ry = isRight ? xbox.rawRy : xbox.rawLy;
  const ox = isRight ? xbox.outRx : xbox.outLx;
  const oy = isRight ? xbox.outRy : xbox.outLy;

  const inMag = clamp(Math.hypot(rx, ry), 0, 1);
  const outMag = clamp(Math.hypot(ox, oy), 0, 1);
  const dot = el("stick-curve-live-dot");
  dot.setAttribute("cx", (24 + inMag * 272).toFixed(1));
  dot.setAttribute("cy", (296 - outMag * 272).toFixed(1));
  el("curve-live-readout").textContent = `Input: ${Math.round(inMag * 100)}% -> Output: ${Math.round(outMag * 100)}%`;
}

async function refreshBetaflightCurveSvg() {
  const prof = getActiveProfile();
  if (!prof) return;
  const [rollPts, pitchPts, yawPts] = await Promise.all([
    invokeCommand("preview_betaflight_points", { config: prof.fpv.rollRates }),
    invokeCommand("preview_betaflight_points", { config: prof.fpv.pitchRates }),
    invokeCommand("preview_betaflight_points", { config: prof.fpv.yawRates })
  ]);

  const toPath = (pts) =>
    pts
      .map(([x, y], idx) => {
        const px = 20 + x * 260;
        const py = 202 - y * 184;
        return `${idx === 0 ? "M" : "L"}${px.toFixed(1)},${py.toFixed(1)}`;
      })
      .join(" ");

  el("fpv-roll-path").setAttribute("d", toPath(rollPts));
  el("fpv-pitch-path").setAttribute("d", toPath(pitchPts));
  el("fpv-yaw-path").setAttribute("d", toPath(yawPts));

  const maxDps = (ax) =>
    Math.round((200 * clamp(ax.rcRate, 0.1, 2.5)) / Math.max(0.02, 1 - clamp(ax.superRate, 0, 0.95)));
  el("fpv-max-dps-summary").textContent =
    `Max Roll: ${maxDps(prof.fpv.rollRates)}°/s · Pitch: ${maxDps(prof.fpv.pitchRates)}°/s · Yaw: ${maxDps(prof.fpv.yawRates)}°/s`;
}

// ==========================================================================
// TAB 3: REMAPPINGS & MACROS
// ==========================================================================

function formatKeyEventToToken(ev) {
  const parts = [];
  if (ev.ctrlKey && ev.key !== "Control") parts.push("Ctrl");
  if (ev.altKey && ev.key !== "Alt") parts.push("Alt");
  if (ev.shiftKey && ev.key !== "Shift") parts.push("Shift");
  let key = ev.key;
  if (key === " ") key = "Space";
  else if (key.length === 1) key = key.toUpperCase();
  else if (key.startsWith("Arrow")) key = key.slice(5);
  else if (key === "Control") key = "Ctrl";
  parts.push(key);
  return parts.join("+");
}

function openKeyCaptureModal(onPick) {
  const overlay = el("modal-overlay");
  el("modal-title").textContent = "Press Key or Shortcut";
  const body = el("modal-body");
  body.replaceChildren();

  const p = document.createElement("p");
  p.textContent = "Press any keyboard key or shortcut (e.g. Ctrl+Shift+M, F9, Space) OR click a common target below:";
  body.appendChild(p);

  const quickWrap = document.createElement("div");
  quickWrap.className = "btn-group";
  ["Space", "R", "F9", "M", "Ctrl+C", "Ctrl+V", "Ctrl+Shift+M", "MouseLeft", "MouseRight", "MouseX1", "MouseX2"].forEach(
    (tok) => {
      const b = document.createElement("button");
      b.className = "btn btn-secondary btn-sm";
      b.textContent = tok;
      b.addEventListener("click", () => {
        cleanup();
        onPick(tok);
      });
      quickWrap.appendChild(b);
    }
  );
  body.appendChild(quickWrap);

  const foot = el("modal-foot");
  foot.replaceChildren();
  const cancelBtn = document.createElement("button");
  cancelBtn.className = "btn btn-secondary";
  cancelBtn.textContent = "Cancel";
  foot.appendChild(cancelBtn);

  const onKey = (ev) => {
    ev.preventDefault();
    ev.stopPropagation();
    if (["Control", "Shift", "Alt", "Meta"].includes(ev.key)) return;
    const tok = formatKeyEventToToken(ev);
    cleanup();
    onPick(tok);
  };

  function cleanup() {
    window.removeEventListener("keydown", onKey, true);
    overlay.classList.add("hidden");
  }

  cancelBtn.onclick = cleanup;
  el("btn-modal-close").onclick = cleanup;
  window.addEventListener("keydown", onKey, true);
  overlay.classList.remove("hidden");
}

function renderRemappings() {
  const prof = getActiveProfile();
  const host = el("remap-list");
  if (!prof || !host) return;
  host.replaceChildren();

  if (!prof.remappings.length) {
    const empty = document.createElement("div");
    empty.className = "tiny-meta";
    empty.textContent = "No custom remappings configured in this profile. Click Add Binding to create one.";
    host.appendChild(empty);
    return;
  }

  prof.remappings.forEach((binding, idx) => {
    const row = document.createElement("div");
    row.className = "remap-row";
    row.dataset.sourceId = binding.source;

    const led = document.createElement("span");
    led.className = "remap-led";

    const srcSel = document.createElement("select");
    for (const grp of REMAP_SOURCE_GROUPS) {
      const og = document.createElement("optgroup");
      og.label = grp.group;
      for (const item of grp.items) {
        const opt = document.createElement("option");
        opt.value = item.id;
        opt.textContent = item.label;
        og.appendChild(opt);
      }
      srcSel.appendChild(og);
    }
    srcSel.value = binding.source;
    srcSel.addEventListener("change", () => {
      binding.source = srcSel.value;
      row.dataset.sourceId = binding.source;
      scheduleSaveActiveProfile();
    });

    const actSel = document.createElement("select");
    for (const act of REMAP_ACTION_TYPES) {
      const opt = document.createElement("option");
      opt.value = act.id;
      opt.textContent = act.label;
      actSel.appendChild(opt);
    }
    actSel.value = binding.actionType;
    actSel.addEventListener("change", () => {
      binding.actionType = actSel.value;
      if (binding.actionType === "Macro" && prof.macros.length && !prof.macros.some((m) => m.id === binding.target)) {
        binding.target = prof.macros[0].id;
      }
      scheduleSaveActiveProfile();
      renderRemappings();
    });

    // Target control depends on actionType
    const targetWrap = document.createElement("div");
    targetWrap.style.display = "flex";
    targetWrap.style.gap = "4px";
    if (binding.actionType === "Macro") {
      const mSel = document.createElement("select");
      for (const mac of prof.macros) {
        const opt = document.createElement("option");
        opt.value = mac.id;
        opt.textContent = mac.name;
        mSel.appendChild(opt);
      }
      mSel.value = binding.target;
      mSel.addEventListener("change", () => {
        binding.target = mSel.value;
        scheduleSaveActiveProfile();
      });
      targetWrap.appendChild(mSel);
    } else if (["Native", "DpiShiftHold", "DpiStageCycle", "GyroToggle", "ProfileCycle", "Disabled"].includes(binding.actionType)) {
      const info = document.createElement("input");
      info.type = "text";
      info.readOnly = true;
      info.value = binding.actionType === "DpiShiftHold" ? `-> ${prof.mouse.sniperDpi} DPI` : "—";
      targetWrap.appendChild(info);
    } else {
      const tInput = document.createElement("input");
      tInput.type = "text";
      tInput.value = binding.target;
      tInput.placeholder = "Key / Combo";
      tInput.addEventListener("input", () => {
        binding.target = tInput.value;
        scheduleSaveActiveProfile();
      });
      const capBtn = document.createElement("button");
      capBtn.className = "btn btn-secondary btn-sm";
      capBtn.innerHTML = `<svg class="icon"><use href="#icon-pin"/></svg><span>Key</span>`;
      capBtn.addEventListener("click", () => {
        openKeyCaptureModal((tok) => {
          binding.target = tok;
          tInput.value = tok;
          scheduleSaveActiveProfile();
        });
      });
      targetWrap.append(tInput, capBtn);
    }

    const noteInput = document.createElement("input");
    noteInput.type = "text";
    noteInput.value = binding.note || "";
    noteInput.placeholder = "Description note";
    noteInput.addEventListener("input", () => {
      binding.note = noteInput.value;
      scheduleSaveActiveProfile();
    });

    const delBtn = document.createElement("button");
    delBtn.className = "btn btn-danger btn-sm";
    delBtn.innerHTML = `<svg class="icon"><use href="#icon-trash-2"/></svg><span>Remove</span>`;
    delBtn.addEventListener("click", () => {
      prof.remappings.splice(idx, 1);
      scheduleSaveActiveProfile();
      renderRemappings();
    });

    row.append(led, srcSel, actSel, targetWrap, noteInput, delBtn);
    host.appendChild(row);
  });
}

function updateRemapInputLeds(snap) {
  const w = snap.xbox.buttonsMask;
  const m = snap.mouse.buttonsMask;
  const sw = snap.fpv.switches;
  const states = {
    mouse4: (m & (1 << 3)) !== 0,
    mouse5: (m & (1 << 4)) !== 0,
    mouse_middle: (m & (1 << 2)) !== 0,
    pad_a: (w & 0x1000) !== 0,
    pad_b: (w & 0x2000) !== 0,
    pad_x: (w & 0x4000) !== 0,
    pad_y: (w & 0x8000) !== 0,
    pad_lb: (w & 0x0100) !== 0,
    pad_rb: (w & 0x0200) !== 0,
    pad_lt_hair: snap.xbox.rawLt > 0.1,
    pad_rt_hair: snap.xbox.rawRt > 0.1,
    pad_ls: (w & 0x0040) !== 0,
    pad_rs: (w & 0x0080) !== 0,
    pad_back: (w & 0x0020) !== 0,
    pad_start: (w & 0x0010) !== 0,
    pad_dpad_up: (w & 0x0001) !== 0,
    pad_dpad_down: (w & 0x0002) !== 0,
    pad_dpad_left: (w & 0x0004) !== 0,
    pad_dpad_right: (w & 0x0008) !== 0,
    fpv_sa_high: snap.fpv.connected && sw[0] === 2,
    fpv_sb_mid: snap.fpv.connected && sw[1] === 1,
    fpv_sb_high: snap.fpv.connected && sw[1] === 2,
    fpv_sc_mid: snap.fpv.connected && sw[2] === 1,
    fpv_sc_high: snap.fpv.connected && sw[2] === 2,
    fpv_sd_high: snap.fpv.connected && sw[3] === 2,
    fpv_se_high: snap.fpv.connected && sw[4] === 2
  };
  el("remap-list").querySelectorAll(".remap-row").forEach((row) => {
    const led = row.querySelector(".remap-led");
    if (led) led.classList.toggle("active", Boolean(states[row.dataset.sourceId]));
  });
}

function renderMacros() {
  const prof = getActiveProfile();
  const listHost = el("macro-sidebar-list");
  const editorHost = el("macro-editor-pane");
  if (!prof || !listHost || !editorHost) return;

  if (!prof.macros.some((m) => m.id === appState.selectedMacroId)) {
    appState.selectedMacroId = prof.macros[0]?.id || null;
  }

  listHost.replaceChildren();
  prof.macros.forEach((mac) => {
    const btn = document.createElement("button");
    btn.className = `macro-item-btn ${mac.id === appState.selectedMacroId ? "active" : ""}`;
    const n = document.createElement("span");
    n.className = "truncate";
    n.textContent = mac.name;
    const c = document.createElement("span");
    c.className = "mini-pill";
    c.textContent = `${mac.steps.length}`;
    btn.append(n, c);
    btn.addEventListener("click", () => {
      appState.selectedMacroId = mac.id;
      renderMacros();
    });
    listHost.appendChild(btn);
  });

  const mac = prof.macros.find((m) => m.id === appState.selectedMacroId);
  editorHost.replaceChildren();
  if (!mac) {
    const empty = document.createElement("div");
    empty.className = "tiny-meta";
    empty.textContent = "Create a macro sequence using + NEW MACRO.";
    editorHost.appendChild(empty);
    return;
  }

  const headRow = document.createElement("div");
  headRow.className = "form-row three";

  const nameField = document.createElement("label");
  nameField.className = "field";
  nameField.innerHTML = "<span>MACRO NAME</span>";
  const nameIn = document.createElement("input");
  nameIn.type = "text";
  nameIn.value = mac.name;
  nameIn.addEventListener("input", () => {
    mac.name = nameIn.value;
    scheduleSaveActiveProfile();
  });
  nameField.appendChild(nameIn);

  const modeField = document.createElement("label");
  modeField.className = "field";
  modeField.innerHTML = "<span>EXECUTION MODE</span>";
  const modeSel = document.createElement("select");
  [
    ["Sequence", "One-Shot Sequence"],
    ["Hold", "Hold (While Pressed)"],
    ["Toggle", "Toggle On/Off"],
    ["RepeatWhileHeld", "Repeat While Held"]
  ].forEach(([id, lbl]) => {
    const o = document.createElement("option");
    o.value = id;
    o.textContent = lbl;
    modeSel.appendChild(o);
  });
  modeSel.value = mac.mode;
  modeSel.addEventListener("change", () => {
    mac.mode = modeSel.value;
    scheduleSaveActiveProfile();
  });
  modeField.appendChild(modeSel);

  const repField = document.createElement("label");
  repField.className = "field";
  repField.innerHTML = "<span>REPEAT COOLDOWN (MS)</span>";
  const repIn = document.createElement("input");
  repIn.type = "number";
  repIn.min = "0";
  repIn.max = "5000";
  repIn.value = String(mac.repeatDelayMs || 0);
  repIn.addEventListener("input", () => {
    mac.repeatDelayMs = Math.max(0, Number(repIn.value) || 0);
    scheduleSaveActiveProfile();
  });
  repField.appendChild(repIn);

  headRow.append(nameField, modeField, repField);
  editorHost.appendChild(headRow);

  const stepsBox = document.createElement("div");
  stepsBox.className = "form-stack";
  mac.steps.forEach((step, sIdx) => {
    const sRow = document.createElement("div");
    sRow.className = "macro-step-row";

    const num = document.createElement("span");
    num.className = "mono muted";
    num.textContent = `${sIdx + 1}.`;

    const kindSel = document.createElement("select");
    ["KeyTap", "KeyDown", "KeyUp", "MouseClick", "MouseDown", "MouseUp", "DelayMs"].forEach(
      (k) => {
        const o = document.createElement("option");
        o.value = k;
        o.textContent = k;
        kindSel.appendChild(o);
      }
    );
    kindSel.value = step.kind;
    kindSel.addEventListener("change", () => {
      step.kind = kindSel.value;
      if (step.kind === "DelayMs" && isNaN(Number(step.value))) step.value = "40";
      scheduleSaveActiveProfile();
      renderMacros();
    });

    const valIn = document.createElement("input");
    valIn.type = step.kind === "DelayMs" ? "number" : "text";
    valIn.value = step.value;
    valIn.addEventListener("input", () => {
      step.value = String(valIn.value);
      scheduleSaveActiveProfile();
    });

    const actGrp = document.createElement("div");
    actGrp.className = "btn-group";
    const upBtn = document.createElement("button");
    upBtn.className = "btn-tiny";
    upBtn.textContent = "UP";
    upBtn.onclick = () => {
      if (sIdx > 0) {
        [mac.steps[sIdx - 1], mac.steps[sIdx]] = [mac.steps[sIdx], mac.steps[sIdx - 1]];
        scheduleSaveActiveProfile();
        renderMacros();
      }
    };
    const delBtn = document.createElement("button");
    delBtn.className = "btn-tiny";
    delBtn.style.color = "var(--danger)";
    delBtn.textContent = "Delete";
    delBtn.onclick = () => {
      mac.steps.splice(sIdx, 1);
      scheduleSaveActiveProfile();
      renderMacros();
    };
    actGrp.append(upBtn, delBtn);

    sRow.append(num, kindSel, valIn, actGrp);
    stepsBox.appendChild(sRow);
  });
  editorHost.appendChild(stepsBox);

  const footBtns = document.createElement("div");
  footBtns.className = "btn-group mt-8";
  const addKeyStep = document.createElement("button");
  addKeyStep.className = "btn btn-secondary btn-sm";
  addKeyStep.textContent = "Add Key Step";
  addKeyStep.onclick = () => {
    mac.steps.push({ kind: "KeyTap", value: COMMON_KEYS[0] });
    scheduleSaveActiveProfile();
    renderMacros();
  };
  const addDelayStep = document.createElement("button");
  addDelayStep.className = "btn btn-secondary btn-sm";
  addDelayStep.textContent = "Add Delay (35 ms)";
  addDelayStep.onclick = () => {
    mac.steps.push({ kind: "DelayMs", value: "35" });
    scheduleSaveActiveProfile();
    renderMacros();
  };
  const delMacroBtn = document.createElement("button");
  delMacroBtn.className = "btn btn-danger btn-sm";
  delMacroBtn.textContent = "Delete Macro";
  delMacroBtn.onclick = () => {
    const idx = prof.macros.findIndex((m) => m.id === mac.id);
    if (idx >= 0) prof.macros.splice(idx, 1);
    appState.selectedMacroId = prof.macros[0]?.id || null;
    scheduleSaveActiveProfile();
    renderMacros();
  };
  footBtns.append(addKeyStep, addDelayStep, delMacroBtn);
  editorHost.appendChild(footBtns);
}

// ==========================================================================
// TAB 4: PROFILES, SETTINGS, RAW HID INSPECTOR & 10 THEMES
// ==========================================================================

function renderProfilesManager() {
  if (!appState.store) return;
  const list = el("profile-cards-list");
  list.replaceChildren();

  if (!appState.store.profiles.some((p) => p.id === appState.selectedProfileId)) {
    appState.selectedProfileId = appState.store.settings.activeProfileId;
  }

  for (const prof of appState.store.profiles) {
    const c = document.createElement("div");
    c.className = `prof-select-card ${prof.id === appState.selectedProfileId ? "active" : ""}`;
    const title = document.createElement("strong");
    title.textContent =
      prof.name + (prof.id === appState.store.settings.activeProfileId ? " (Active)" : "");
    const exes = document.createElement("span");
    exes.className = "tiny-meta mono truncate";
    exes.textContent = prof.linkedExes.length
      ? prof.linkedExes.join(", ")
      : prof.isDefault
        ? "Default Fallback"
        : "No linked .exe";
    c.append(title, exes);
    c.addEventListener("click", () => {
      appState.selectedProfileId = prof.id;
      renderProfilesManager();
    });
    list.appendChild(c);
  }

  const editing = getEditingProfile();
  if (!editing) return;
  el("prof-edit-name").value = editing.name;
  el("prof-edit-desc").value = editing.description || "";
  el("btn-delete-selected-prof").disabled = editing.isDefault;

  const chips = el("prof-exe-chips");
  chips.replaceChildren();
  editing.linkedExes.forEach((exe, i) => {
    const chip = document.createElement("span");
    chip.className = "exe-chip";
    const lbl = document.createElement("span");
    lbl.textContent = exe;
    const x = document.createElement("button");
    x.textContent = "x";
    x.onclick = () => {
      editing.linkedExes.splice(i, 1);
      saveProfileNow(editing);
    };
    chip.append(lbl, x);
    chips.appendChild(chip);
  });
}

function renderSettingsSection() {
  if (!appState.store) return;
  const s = appState.store.settings;
  el("set-autoswitch").checked = s.autoSwitchEnabled;
  el("set-close-tray").checked = s.closeToTray;
  el("set-min-tray").checked = s.minimizeToTray;
  el("set-start-min").checked = s.startMinimized;
  el("set-autostart").checked = s.autostartWindows;
}

function updateInspectorDropdown(devices) {
  const sel = el("inspector-device-select");
  if (!sel) return;
  const prev = appState.inspectorDeviceId || sel.value;
  sel.replaceChildren();
  for (const d of devices) {
    if (d.className === "Xbox Gamepad" || d.transport === "Win32 RawInput") continue;
    const opt = document.createElement("option");
    opt.value = d.id;
    opt.textContent = `${d.name} (VID:${d.vid.toString(16).toUpperCase().padStart(4, "0")})`;
    sel.appendChild(opt);
  }
  if ([...sel.options].some((o) => o.value === prev)) {
    sel.value = prev;
  }
  appState.inspectorDeviceId = sel.value;
}

function updateInspectorGrid(data) {
  const bar = el("inspector-meta-bar");
  const axesRow = el("inspector-axes-row");
  if (!data || !data.reportLen) {
    bar.textContent = "Waiting for raw HID packets (move sticks / click buttons on selected device)...";
    return;
  }
  bar.textContent = `${data.deviceName} · ${data.vidPid} · ${data.reportLen} bytes`;

  for (let i = 0; i < 64; i++) {
    const cell = el(`hex-cell-${i}`);
    const valEl = el(`hex-val-${i}`);
    const mmEl = el(`hex-mm-${i}`);
    const inRange = i < data.bytes.length;
    cell.style.opacity = inRange ? "1" : "0.28";
    if (!inRange) continue;

    const b = data.bytes[i];
    cell.classList.toggle("changed", Boolean(data.changedMask[i]));
    valEl.textContent = hexByte(b);
    mmEl.textContent = `${hexByte(data.minBytes[i])}..${hexByte(data.maxBytes[i])}`;
    for (let bit = 0; bit < 8; bit++) {
      el(`hex-bit-${i}-${bit}`).classList.toggle("on", (b & (1 << bit)) !== 0);
    }
  }

  axesRow.replaceChildren();
  (data.axesU16 || []).slice(0, 8).forEach((val, idx) => {
    const badge = document.createElement("span");
    badge.className = "badge";
    badge.textContent = `A${idx}: ${val}`;
    axesRow.appendChild(badge);
  });
}

function renderThemeGallery() {
  const host = el("theme-gallery");
  if (!host) return;
  const active = document.documentElement.getAttribute("data-theme") || "dark";
  host.replaceChildren();

  for (const t of THEME_PRESETS) {
    const card = document.createElement("button");
    card.className = `theme-card ${t.id === active ? "active" : ""}`;
    const preview = document.createElement("div");
    preview.className = "theme-preview";
    preview.dataset.previewTheme = t.id;
    const name = document.createElement("div");
    name.className = "theme-name";
    name.textContent = t.name;
    const desc = document.createElement("div");
    desc.className = "theme-desc";
    desc.textContent = t.desc;
    card.append(preview, name, desc);
    card.addEventListener("click", () => applyTheme(t.id));
    host.appendChild(card);
  }
}

// ==========================================================================
// MODAL DIALOG FOR NEW PROFILE
// ==========================================================================

function openNewProfileModal() {
  const overlay = el("modal-overlay");
  el("modal-title").textContent = "Create New Input Profile";
  const body = el("modal-body");
  body.replaceChildren();

  const nameField = document.createElement("label");
  nameField.className = "field";
  nameField.innerHTML = "<span>Profile Name</span>";
  const nameIn = document.createElement("input");
  nameIn.type = "text";
  nameIn.placeholder = "e.g. Battlefield 6 Gyro + M650";
  nameField.appendChild(nameIn);

  const exeField = document.createElement("label");
  exeField.className = "field";
  exeField.innerHTML = "<span>Linked Game Executable (Optional)</span>";
  const exeIn = document.createElement("input");
  exeIn.type = "text";
  exeIn.placeholder = "e.g. bf6.exe";
  exeField.appendChild(exeIn);
  body.append(nameField, exeField);

  const foot = el("modal-foot");
  foot.replaceChildren();
  const cancelBtn = document.createElement("button");
  cancelBtn.className = "btn btn-secondary";
  cancelBtn.textContent = "Cancel";
  cancelBtn.onclick = () => overlay.classList.add("hidden");

  const createBtn = document.createElement("button");
  createBtn.className = "btn btn-primary";
  createBtn.textContent = "Create Profile";
  createBtn.onclick = async () => {
    const cleanName = nameIn.value.trim();
    if (!cleanName) return;
    const base = structuredClone(getActiveProfile());
    base.id = `prof_${Date.now()}`;
    base.name = cleanName;
    base.isDefault = false;
    base.linkedExes = exeIn.value.trim() ? [exeIn.value.trim()] : [];
    overlay.classList.add("hidden");
    await saveProfileNow(base);
    appState.store = await invokeCommand("set_active_profile", { profileId: base.id });
    appState.selectedProfileId = base.id;
    renderAllStatic();
    showToast(`Created profile "${cleanName}"`, "ok");
  };
  foot.append(cancelBtn, createBtn);
  el("btn-modal-close").onclick = () => overlay.classList.add("hidden");
  overlay.classList.remove("hidden");
  nameIn.focus();
}

// ==========================================================================
// EVENT WIRING & INITIALIZATION
// ==========================================================================

function wireEvents() {
  document.querySelectorAll(".nav-btn").forEach((btn) => {
    btn.addEventListener("click", () => switchTab(btn.dataset.tab));
  });

  // Stick target switch
  el("curve-stick-selector").querySelectorAll(".seg-btn").forEach((btn) => {
    btn.addEventListener("click", () => {
      appState.selectedStick = btn.dataset.stick;
      el("curve-stick-selector").querySelectorAll(".seg-btn").forEach((b) =>
        b.classList.toggle("active", b === btn)
      );
      renderCurveControls();
      refreshStickCurveSvg();
    });
  });

  // FPV axis switch
  el("fpv-axis-selector").querySelectorAll(".seg-btn").forEach((btn) => {
    btn.addEventListener("click", () => {
      appState.selectedFpvAxis = btn.dataset.axis;
      el("fpv-axis-selector").querySelectorAll(".seg-btn").forEach((b) =>
        b.classList.toggle("active", b === btn)
      );
      renderCurveControls();
    });
  });

  // SVG 5-point Bezier pointer dragging
  window.addEventListener("pointermove", (ev) => {
    if (appState.draggingKnotIdx < 0) return;
    const svg = el("stick-curve-svg");
    const rect = svg.getBoundingClientRect();
    const xSvg = ((ev.clientX - rect.left) / rect.width) * 320;
    const ySvg = ((ev.clientY - rect.top) / rect.height) * 320;

    const prof = getActiveProfile();
    if (!prof) return;
    const cfg = getSelectedStickConfig(prof);
    const i = appState.draggingKnotIdx;
    const minX = i === 0 ? 0 : cfg.customPoints[i - 1][0];
    const maxX = i === 4 ? 1 : cfg.customPoints[i + 1][0];
    cfg.customPoints[i][0] = clamp((xSvg - 24) / 272, minX, maxX);
    cfg.customPoints[i][1] = clamp((296 - ySvg) / 272, 0, 1);
    refreshStickCurveSvg();
    scheduleSaveActiveProfile();
  });
  window.addEventListener("pointerup", () => {
    appState.draggingKnotIdx = -1;
  });

  // Curve sliders & selects
  const bindStickInput = (id, handler) => {
    el(id).addEventListener("input", () => {
      const prof = getActiveProfile();
      if (!prof) return;
      handler(getSelectedStickConfig(prof), el(id));
      renderCurveControls();
      refreshStickCurveSvg();
      scheduleSaveActiveProfile();
    });
  };
  bindStickInput("cfg-stick-shape", (s, i) => (s.shape = i.value));
  bindStickInput("cfg-stick-curve-mode", (s, i) => (s.curveMode = i.value));
  bindStickInput("cfg-inner-dz", (s, i) => (s.innerDeadzone = Number(i.value) / 100));
  bindStickInput("cfg-outer-dz", (s, i) => (s.outerDeadzone = Number(i.value) / 100));
  bindStickInput("cfg-anti-dz", (s, i) => (s.antiDeadzone = Number(i.value) / 100));
  bindStickInput("cfg-expo-power", (s, i) => (s.expoPower = Number(i.value)));

  el("btn-curve-clear-drift").onclick = () => {
    const prof = getActiveProfile();
    if (!prof) return;
    const cfg = getSelectedStickConfig(prof);
    cfg.centerOffsetX = 0;
    cfg.centerOffsetY = 0;
    saveProfileNow(prof);
    showToast("Stick drift calibration cleared (0, 0)", "ok");
  };

  el("btn-curve-reset-pts").onclick = () => {
    const prof = getActiveProfile();
    if (!prof) return;
    const cfg = getSelectedStickConfig(prof);
    cfg.curveMode = "Bezier5Point";
    cfg.customPoints = [
      [0.0, 0.0],
      [0.25, 0.2],
      [0.5, 0.45],
      [0.75, 0.78],
      [1.0, 1.0]
    ];
    renderCurveControls();
    refreshStickCurveSvg();
    scheduleSaveActiveProfile();
  };

  // Trigger & rumble bindings
  const bindProfInput = (id, fn) => {
    el(id).addEventListener("input", () => {
      const prof = getActiveProfile();
      if (!prof) return;
      fn(prof, el(id));
      renderCurveControls();
      scheduleSaveActiveProfile();
    });
  };
  bindProfInput("cfg-lt-thr", (p, i) => (p.leftTrigger.activationThreshold = Number(i.value) / 100));
  bindProfInput("cfg-lt-digital", (p, i) => (p.leftTrigger.hairTriggerDigital = i.checked));
  bindProfInput("cfg-lt-invert", (p, i) => (p.leftTrigger.invert = i.checked));
  bindProfInput("cfg-rt-thr", (p, i) => (p.rightTrigger.activationThreshold = Number(i.value) / 100));
  bindProfInput("cfg-rt-digital", (p, i) => (p.rightTrigger.hairTriggerDigital = i.checked));
  bindProfInput("cfg-rt-invert", (p, i) => (p.rightTrigger.invert = i.checked));
  bindProfInput("cfg-rumble-l", (p, i) => (p.rumbleLeftScale = Number(i.value) / 100));
  bindProfInput("cfg-rumble-r", (p, i) => (p.rumbleRightScale = Number(i.value) / 100));

  for (let s = 0; s < 4; s++) {
    bindProfInput(`cfg-dpi-${s}`, (p, i) => {
      p.mouse.dpiStages[s] = clamp(Number(i.value) || 800, 100, 25600);
      renderLiveDpiButtons();
    });
  }
  bindProfInput("cfg-active-dpi-stage", (p, i) => {
    p.mouse.activeDpiStage = Number(i.value);
    renderLiveDpiButtons();
  });
  bindProfInput("cfg-sniper-dpi", (p, i) => (p.mouse.sniperDpi = clamp(Number(i.value) || 400, 100, 6400)));
  bindProfInput("cfg-rapid-cps", (p, i) => (p.mouse.rapidFireCps = clamp(Number(i.value) || 12, 1, 30)));
  bindProfInput("cfg-dpi-mult", (p, i) => (p.mouse.dpiMultiplier = Number(i.value)));

  el("btn-apply-hid-dpi").onclick = async () => {
    const prof = getActiveProfile();
    if (!prof) return;
    await saveProfileNow(prof);
    appState.store = await invokeCommand("apply_dpi_stage", {
      stageIdx: prof.mouse.activeDpiStage
    });
    showToast(`Applied ${prof.mouse.dpiStages[prof.mouse.activeDpiStage]} DPI`, "ok");
  };

  // FPV rates
  bindProfInput("cfg-fpv-order", (p, i) => (p.fpv.channelOrder = i.value));
  bindProfInput("cfg-fpv-unipolar", (p, i) => (p.fpv.throttleUnipolar = i.checked));
  ["cfg-fpv-rc", "cfg-fpv-super", "cfg-fpv-expo"].forEach((id) => {
    el(id).addEventListener("input", () => {
      const prof = getActiveProfile();
      if (!prof) return;
      const ax = getSelectedFpvAxisConfig(prof);
      if (id === "cfg-fpv-rc") ax.rcRate = Number(el(id).value);
      if (id === "cfg-fpv-super") ax.superRate = Number(el(id).value);
      if (id === "cfg-fpv-expo") ax.expo = Number(el(id).value);
      renderCurveControls();
      refreshBetaflightCurveSvg();
      scheduleSaveActiveProfile();
    });
  });

  // Gyro controls
  bindProfInput("cfg-gyro-mode", (p, i) => (p.gyro.activationMode = i.value));
  bindProfInput("cfg-gyro-sens", (p, i) => (p.gyro.sensitivity = Number(i.value)));
  bindProfInput("cfg-gyro-vratio", (p, i) => (p.gyro.verticalRatio = Number(i.value)));
  bindProfInput("cfg-gyro-floor", (p, i) => (p.gyro.noiseFloorDps = Number(i.value)));
  bindProfInput("cfg-gyro-smooth", (p, i) => (p.gyro.smoothing = Number(i.value) / 100));
  bindProfInput("cfg-gyro-invx", (p, i) => (p.gyro.invertX = i.checked));
  bindProfInput("cfg-gyro-invy", (p, i) => (p.gyro.invertY = i.checked));

  // Center drift capture + Rumble buttons
  const captureDrift = async (stick) => {
    const prof = getActiveProfile();
    if (!prof) return;
    try {
      appState.store = await invokeCommand("capture_stick_center_drift", {
        profileId: prof.id,
        stick
      });
      showToast(`Captured ${stick.toUpperCase()} stick zero-drift offset`, "ok");
    } catch (err) {
      showToast(String(err), "error");
    }
  };
  el("btn-cal-left").onclick = () => captureDrift("left");
  el("btn-cal-right").onclick = () => captureDrift("right");
  el("btn-zero-drift-quick").onclick = async () => {
    await captureDrift("left");
    await captureDrift("right");
  };
  el("btn-curve-cal-center").onclick = () => captureDrift(appState.selectedStick);

  const pulseRumble = async () => {
    const prof = getActiveProfile();
    if (!prof) return;
    try {
      await invokeCommand("test_xinput_rumble", {
        leftPct: clamp(0.65 * prof.rumbleLeftScale, 0, 1),
        rightPct: clamp(0.65 * prof.rumbleRightScale, 0, 1)
      });
      showToast("Pulsed Xbox Left & Right rumble motors", "ok");
    } catch (err) {
      showToast(String(err), "error");
    }
  };
  el("btn-test-rumble").onclick = pulseRumble;
  el("btn-tuning-test-rumble").onclick = pulseRumble;

  // Remaps & Macros
  el("btn-add-remap").onclick = () => {
    const prof = getActiveProfile();
    if (!prof) return;
    prof.remappings.push({
      source: "mouse4",
      actionType: "KeyTap",
      target: "Space",
      note: "New Binding"
    });
    scheduleSaveActiveProfile();
    renderRemappings();
  };

  el("btn-add-macro").onclick = () => {
    const prof = getActiveProfile();
    if (!prof) return;
    const id = `macro_${Date.now()}`;
    prof.macros.push({
      id,
      name: `Macro ${prof.macros.length + 1}`,
      mode: "Sequence",
      repeatDelayMs: 0,
      steps: [
        { kind: "KeyDown", value: "F9" },
        { kind: "DelayMs", value: "35" },
        { kind: "KeyUp", value: "F9" }
      ]
    });
    appState.selectedMacroId = id;
    scheduleSaveActiveProfile();
    renderMacros();
  };

  el("btn-test-macro").onclick = async () => {
    const prof = getActiveProfile();
    const mac = prof?.macros?.find((m) => m.id === appState.selectedMacroId);
    if (!mac) return;
    await invokeCommand("test_macro", { macroDef: mac });
    showToast(`Executed macro "${mac.name}"`, "ok");
  };

  // Profile Manager & Settings
  el("btn-save-top").onclick = () => saveProfileNow(getActiveProfile());
  el("btn-new-profile").onclick = openNewProfileModal;

  el("prof-edit-name").addEventListener("change", () => {
    const p = getEditingProfile();
    if (!p) return;
    p.name = el("prof-edit-name").value.trim() || p.name;
    saveProfileNow(p);
  });
  el("prof-edit-desc").addEventListener("change", () => {
    const p = getEditingProfile();
    if (!p) return;
    p.description = el("prof-edit-desc").value;
    saveProfileNow(p);
  });

  el("btn-add-exe-rule").onclick = () => {
    const p = getEditingProfile();
    const exe = el("prof-exe-input").value.trim();
    if (!p || !exe) return;
    if (!p.linkedExes.includes(exe)) p.linkedExes.push(exe);
    el("prof-exe-input").value = "";
    saveProfileNow(p);
  };

  el("btn-browse-exe").onclick = async () => {
    const chosen = await pickExecutableFile();
    const p = getEditingProfile();
    if (!chosen || !p) return;
    const base = chosen.split(/[\\/]/).pop();
    if (base && !p.linkedExes.includes(base)) {
      p.linkedExes.push(base);
      saveProfileNow(p);
    }
  };

  el("btn-bind-fg-exe").onclick = () => {
    const p = getEditingProfile();
    const fg = appState.telemetry?.foregroundExe;
    if (!p || !fg || fg.toLowerCase().includes("duke-input")) {
      showToast("Focus your target game briefly first, or type its .exe name.", "error");
      return;
    }
    if (!p.linkedExes.includes(fg)) p.linkedExes.push(fg);
    saveProfileNow(p);
  };

  el("btn-activate-selected-prof").onclick = async () => {
    const p = getEditingProfile();
    if (!p) return;
    appState.store = await invokeCommand("set_active_profile", { profileId: p.id });
    renderAllStatic();
    showToast(`Active profile: ${p.name}`, "ok");
  };

  el("btn-dup-selected-prof").onclick = async () => {
    const p = getEditingProfile();
    if (!p) return;
    const copy = structuredClone(p);
    copy.id = `${p.id}_copy_${Date.now()}`;
    copy.name = `${p.name} Copy`;
    copy.isDefault = false;
    copy.linkedExes = [];
    appState.selectedProfileId = copy.id;
    await saveProfileNow(copy);
  };

  el("btn-delete-selected-prof").onclick = async () => {
    const p = getEditingProfile();
    if (!p) return;
    try {
      appState.store = await invokeCommand("delete_profile", { profileId: p.id });
      appState.selectedProfileId = appState.store.settings.activeProfileId;
      renderAllStatic();
      showToast("Profile deleted", "ok");
    } catch (err) {
      showToast(String(err), "error");
    }
  };

  el("btn-export-profile").onclick = async () => {
    const p = getEditingProfile();
    if (!p) return;
    const path = await saveProfileJsonFile(`${p.id}.json`);
    if (!path) return;
    await invokeCommand("export_profile_json", { profileId: p.id, path });
    showToast("Exported profile JSON", "ok");
  };

  el("btn-import-profile").onclick = async () => {
    const path = await pickProfileJsonFile();
    if (!path) return;
    try {
      appState.store = await invokeCommand("import_profile_json", { path });
      appState.selectedProfileId = appState.store.settings.activeProfileId;
      renderAllStatic();
      showToast("Imported profile JSON", "ok");
    } catch (err) {
      showToast(String(err), "error");
    }
  };

  const bindSetting = (id, fn) => {
    el(id).addEventListener("change", () => {
      if (!appState.store) return;
      fn(appState.store.settings, el(id).checked);
      saveSettingsNow();
    });
  };
  bindSetting("set-autoswitch", (s, v) => (s.autoSwitchEnabled = v));
  bindSetting("set-close-tray", (s, v) => (s.closeToTray = v));
  bindSetting("set-min-tray", (s, v) => (s.minimizeToTray = v));
  bindSetting("set-start-min", (s, v) => (s.startMinimized = v));
  bindSetting("set-autostart", (s, v) => (s.autostartWindows = v));

  // Tray & RAM & Pause buttons
  el("btn-pause-engine").onclick = async () => {
    const paused = await invokeCommand("toggle_engine_paused", {});
    if (appState.store) appState.store.settings.enginePaused = paused;
    renderFooterFromStore();
    showToast(paused ? "Engine Paused (EcoQoS Low-Power)" : "1000 Hz Engine Resumed", "ok");
  };

  el("btn-hide-tray").onclick = () => invokeCommand("minimize_to_tray_now");

  const doTrim = async () => {
    const mb = await invokeCommand("trim_working_set");
    const txt = `${mb.toFixed(1)} MB`;
    el("foot-ram-mb").textContent = txt;
    el("settings-ram-readout").textContent = txt;
    showToast(`Working set trimmed -> ${txt}`, "ok");
  };
  el("btn-foot-trim").onclick = doTrim;
  el("btn-trim-ram-settings").onclick = doTrim;

  // Raw HID Inspector
  el("inspector-device-select").addEventListener("change", (ev) => {
    appState.inspectorDeviceId = ev.target.value;
    updateInspectorBackendState();
  });
  el("btn-inspector-toggle").onclick = () => {
    appState.inspectorEnabled = !appState.inspectorEnabled;
    el("btn-inspector-toggle").textContent = appState.inspectorEnabled
      ? "Pause Inspector Stream"
      : "Enable Inspector Stream";
    updateInspectorBackendState();
  };
  el("btn-inspector-reset").onclick = () => {
    invokeCommand("reset_inspector_bounds");
  };

  // Left Hardware Device Rail
  document.querySelectorAll("[data-focus-device]").forEach((btn) => {
    btn.addEventListener("click", () => {
      const target = btn.dataset.focusDevice;
      document.querySelectorAll("[data-focus-device]").forEach((b) => {
        b.classList.toggle("active", b.dataset.focusDevice === target);
      });
      switchTab("devices");
      const panel = el(`dev-panel-${target}`);
      if (panel) {
        panel.scrollIntoView({ behavior: "smooth", block: "nearest" });
        panel.classList.add("device-focused");
        setTimeout(() => panel.classList.remove("device-focused"), 1400);
      }
    });
  });

  // Titlebar Ctrl+K Quick Navigation Search
  const titleSearch = el("titleInputSearch");
  if (titleSearch) {
    titleSearch.addEventListener("keydown", (ev) => {
      if (ev.key !== "Enter") return;
      const q = titleSearch.value.trim().toLowerCase();
      if (!q) return;
      if (q.includes("xbox") || q.includes("stick") && q.includes("test")) {
        document.querySelector('[data-focus-device="xbox"]')?.click();
      } else if (q.includes("logi") || q.includes("g305") || q.includes("m650") || q.includes("mouse")) {
        document.querySelector('[data-focus-device="mouse"]')?.click();
      } else if (q.includes("fpv") || q.includes("radiomaster") || q.includes("gimbal")) {
        document.querySelector('[data-focus-device="fpv"]')?.click();
      } else if (q.includes("gyro") || q.includes("dualsense")) {
        document.querySelector('[data-focus-device="gyro"]')?.click();
      } else if (q.includes("curve") || q.includes("bezier") || q.includes("rate") || q.includes("deadzone")) {
        switchTab("curves");
      } else if (q.includes("remap") || q.includes("macro") || q.includes("bind")) {
        switchTab("remaps");
      } else if (q.includes("prof") || q.includes("theme") || q.includes("setting") || q.includes("hid")) {
        switchTab("profiles");
      } else if (appState.store?.profiles?.length) {
        const match = appState.store.profiles.find((p) => p.name.toLowerCase().includes(q));
        if (match) {
          invokeCommand("set_active_profile", { profileId: match.id })
            .then((store) => {
              appState.store = store;
              appState.selectedProfileId = match.id;
              renderAllStatic();
              showToast(`Activated profile "${match.name}"`, "ok");
            })
            .catch(() => {});
        }
      }
    });
  }
}

async function boot() {
  initWindowChrome(() => {
    const s = el("titleInputSearch");
    if (s) {
      s.focus();
      s.select();
    }
  });
  const savedTheme = localStorage.getItem("duke_input_theme") || "dark";
  applyTheme(savedTheme);
  initStaticDropdowns();
  wireEvents();

  document.addEventListener("visibilitychange", syncVisibilityState);
  window.addEventListener("focus", syncVisibilityState);

  try {
    appState.store = await invokeCommand("get_store");
    appState.selectedProfileId = appState.store.settings.activeProfileId;
    renderAllStatic();

    const snap = await invokeCommand("get_live_telemetry");
    renderTelemetry(snap);
  } catch (err) {
    showToast(`Failed loading store: ${err}`, "error");
  }

  await listenEvent("input-telemetry", (snap) => {
    if (document.visibilityState === "visible") {
      renderTelemetry(snap);
    }
  });

  await listenEvent("profile-switched", async (profileId) => {
    appState.store = await invokeCommand("get_store");
    appState.selectedProfileId = profileId;
    renderAllStatic();
  });

  await listenEvent("engine-paused-changed", async () => {
    appState.store = await invokeCommand("get_store");
    renderFooterFromStore();
  });

  syncVisibilityState();
}

boot();

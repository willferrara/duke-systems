function getTauriInvoke() {
  if (window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke) {
    return window.__TAURI__.core.invoke;
  }
  throw new Error("Tauri IPC bridge not available.");
}

export async function invokeCommand(cmd, args = {}) {
  const invoke = getTauriInvoke();
  return await invoke(cmd, args);
}

export function listenEvent(eventName, callback) {
  if (window.__TAURI__ && window.__TAURI__.event && window.__TAURI__.event.listen) {
    return window.__TAURI__.event.listen(eventName, (event) => {
      callback(event.payload);
    });
  }
  return Promise.resolve(() => {});
}

export async function pickExecutableFile() {
  if (window.__TAURI__ && window.__TAURI__.dialog && window.__TAURI__.dialog.open) {
    const selected = await window.__TAURI__.dialog.open({
      multiple: false,
      directory: false,
      title: "Select Game or App Executable (.exe)",
      filters: [{ name: "Executable (.exe)", extensions: ["exe"] }]
    });
    return typeof selected === "string" ? selected : null;
  }
  return null;
}

export async function pickProfileJsonFile() {
  if (window.__TAURI__ && window.__TAURI__.dialog && window.__TAURI__.dialog.open) {
    const selected = await window.__TAURI__.dialog.open({
      multiple: false,
      directory: false,
      title: "Import Duke Input Profile (.json)",
      filters: [{ name: "Duke Input Profile (.json)", extensions: ["json"] }]
    });
    return typeof selected === "string" ? selected : null;
  }
  return null;
}

export async function saveProfileJsonFile(defaultName = "duke-input-profile.json") {
  if (window.__TAURI__ && window.__TAURI__.dialog && window.__TAURI__.dialog.save) {
    const selected = await window.__TAURI__.dialog.save({
      title: "Export Duke Input Profile (.json)",
      defaultPath: defaultName,
      filters: [{ name: "Duke Input Profile (.json)", extensions: ["json"] }]
    });
    return typeof selected === "string" ? selected : null;
  }
  return null;
}

export function clamp(val, min, max) {
  return Math.max(min, Math.min(max, val));
}

export function hexByte(b) {
  return Number(b || 0).toString(16).toUpperCase().padStart(2, "0");
}

export const CURVE_MODES = [
  { id: "Linear", label: "Linear (1:1 Raw)" },
  { id: "Exponential", label: "Exponential (x^p)" },
  { id: "SCurve", label: "S-Curve (Precision Center + Fast Edge)" },
  { id: "DynamicPrecision", label: "Dynamic Precision Aim" },
  { id: "Bezier5Point", label: "5-Point Custom Bezier Curve" }
];

export const GYRO_MODES = [
  { id: "Off", label: "Disabled" },
  { id: "AlwaysOn", label: "Always On" },
  { id: "HoldLt", label: "Hold LT / L2 (ADS Aim)" },
  { id: "HoldRt", label: "Hold RT / R2" },
  { id: "TouchpadTouch", label: "PlayStation Touchpad Touch" },
  { id: "ToggleButton", label: "Toggle via Remapped Button" }
];

export const REMAP_SOURCE_GROUPS = [
  {
    group: "Logitech / Raw Mouse",
    items: [
      { id: "mouse4", label: "Mouse 4 (Back)" },
      { id: "mouse5", label: "Mouse 5 (Forward)" },
      { id: "mouse_middle", label: "Middle Click" },
      { id: "mouse_dpi", label: "DPI Button" },
      { id: "scroll_up", label: "Scroll Up" },
      { id: "scroll_down", label: "Scroll Down" }
    ]
  },
  {
    group: "Controller & Triggers",
    items: [
      { id: "pad_a", label: "A / Cross" },
      { id: "pad_b", label: "B / Circle" },
      { id: "pad_x", label: "X / Square" },
      { id: "pad_y", label: "Y / Triangle" },
      { id: "pad_lb", label: "LB / L1" },
      { id: "pad_rb", label: "RB / R1" },
      { id: "pad_lt_hair", label: "LT / L2 Hair-Trigger" },
      { id: "pad_rt_hair", label: "RT / R2 Hair-Trigger" },
      { id: "pad_ls", label: "L3 (Left Stick Click)" },
      { id: "pad_rs", label: "R3 (Right Stick Click)" },
      { id: "pad_back", label: "Back / Share / Create" },
      { id: "pad_start", label: "Start / Options" },
      { id: "pad_dpad_up", label: "D-Pad Up" },
      { id: "pad_dpad_down", label: "D-Pad Down" },
      { id: "pad_dpad_left", label: "D-Pad Left" },
      { id: "pad_dpad_right", label: "D-Pad Right" },
      { id: "pad_touchpad", label: "PS Touchpad Click" }
    ]
  },
  {
    group: "FPV Aux Switches (EdgeTX / OpenTX)",
    items: [
      { id: "fpv_sa_high", label: "Switch SA (High / Down)" },
      { id: "fpv_sb_mid", label: "Switch SB (Mid)" },
      { id: "fpv_sb_high", label: "Switch SB (High / Down)" },
      { id: "fpv_sc_mid", label: "Switch SC (Mid)" },
      { id: "fpv_sc_high", label: "Switch SC (High / Down)" },
      { id: "fpv_sd_high", label: "Switch SD (High / Down)" },
      { id: "fpv_se_high", label: "Switch SE (Momentary / High)" }
    ]
  }
];

export const REMAP_ACTION_TYPES = [
  { id: "Native", label: "Native (Pass-Through)" },
  { id: "KeyTap", label: "Keyboard Key" },
  { id: "KeyCombo", label: "Key Shortcut (e.g. Ctrl+C)" },
  { id: "MouseClick", label: "Mouse Button" },
  { id: "RapidFire", label: "Rapid-Fire Burst Clicker" },
  { id: "DpiShiftHold", label: "Sniper DPI Shift (Hold)" },
  { id: "DpiStageCycle", label: "Cycle DPI Stage (1-4)" },
  { id: "GyroToggle", label: "Toggle Gyro-to-Mouse" },
  { id: "ProfileCycle", label: "Cycle Active Profile" },
  { id: "Macro", label: "Trigger Macro Sequence" },
  { id: "Disabled", label: "Disable Input" }
];

export const COMMON_KEYS = [
  "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M",
  "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
  "0", "1", "2", "3", "4", "5", "6", "7", "8", "9",
  "Space", "Enter", "Escape", "Tab", "Backspace", "Shift", "Ctrl", "Alt",
  "Up", "Down", "Left", "Right",
  "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
  "Ctrl+C", "Ctrl+V", "Ctrl+Z", "Ctrl+Shift+M", "Alt+Tab", "Win+G",
  "MouseLeft", "MouseRight", "MouseMiddle", "MouseX1", "MouseX2"
];

export const THEME_PRESETS = [
  {
    id: "dark",
    name: "Obsidian Dark",
    desc: "Native layered slate with crisp steel highlights."
  },
  {
    id: "light",
    name: "Clean Daylight",
    desc: "Crisp daylight surfaces tuned for bright rooms."
  },
  {
    id: "minecraft",
    name: "Minecraft Overworld",
    desc: "Blocky emerald biome peaks & deep sky vectors."
  },
  {
    id: "sunset",
    name: "Gulf Coast Sunset",
    desc: "Amber golden-hour sun & calm ocean horizon."
  },
  {
    id: "aerospace",
    name: "Aerospace HUD",
    desc: "Technical blueprint grid & orbital flight vectors."
  },
  {
    id: "cyberpunk",
    name: "Cyberpunk Night",
    desc: "Neon skyline silhouettes & perspective grid."
  },
  {
    id: "terminal",
    name: "Retro CRT Amber",
    desc: "Phosphor amber scanlines over obsidian glass."
  },
  {
    id: "crimson",
    name: "Redline Carbon",
    desc: "Carbon diagonal geometry with crimson accents."
  },
  {
    id: "ocean",
    name: "Abyssal Ocean",
    desc: "Deep bathymetric sonar rings & cobalt currents."
  },
  {
    id: "frost",
    name: "Nordic Frost",
    desc: "Arctic twilight ridges & crystalline aurora."
  }
];

export function initWindowChrome(onCtrlK) {
  const titlebar = document.getElementById("app-titlebar");
  const minBtn = document.getElementById("win-btn-min");
  const maxBtn = document.getElementById("win-btn-max");
  const closeBtn = document.getElementById("win-btn-close");
  const win = window.__TAURI__?.window?.getCurrentWindow?.() || null;

  if (minBtn) minBtn.addEventListener("click", () => win?.minimize?.());
  if (maxBtn) maxBtn.addEventListener("click", () => win?.toggleMaximize?.());
  if (closeBtn) closeBtn.addEventListener("click", () => win?.close?.());

  if (titlebar) {
    titlebar.addEventListener("dblclick", (e) => {
      if (e.target.closest("button, input, select, a, .titlebar-search-wrap")) return;
      win?.toggleMaximize?.();
    });
    titlebar.addEventListener("mousedown", (e) => {
      if (e.button !== 0) return;
      if (e.target.closest("button, input, select, a, .titlebar-search-wrap")) return;
      win?.startDragging?.();
    });
  }

  if (typeof onCtrlK === "function") {
    window.addEventListener("keydown", (e) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        onCtrlK();
      }
    });
  }
}

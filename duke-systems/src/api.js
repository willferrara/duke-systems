/* ==========================================================================
   DUKE SYSTEMS — TAURI BRIDGE, WINDOW CHROME & THEME PRESETS
   ========================================================================== */

function getTauriInternals() {
  return window.__TAURI__?.core || window.__TAURI_INTERNALS__ || null;
}

export async function invokeCommand(cmd, args = {}) {
  const core = window.__TAURI__?.core;
  if (core && typeof core.invoke === "function") {
    return core.invoke(cmd, args);
  }
  const internals = getTauriInternals();
  if (internals && typeof internals.invoke === "function") {
    return internals.invoke(cmd, args);
  }
  throw new Error(`Tauri IPC unavailable for '${cmd}'`);
}

export async function listenEvent(eventName, handler) {
  const eventApi = window.__TAURI__?.event;
  if (eventApi && typeof eventApi.listen === "function") {
    return eventApi.listen(eventName, (event) => handler(event.payload));
  }
  return () => {};
}

export function initWindowControls({ onCtrlK } = {}) {
  const win = window.__TAURI__?.window?.getCurrentWindow?.();
  const minBtn = document.getElementById("win-btn-min");
  const maxBtn = document.getElementById("win-btn-max");
  const closeBtn = document.getElementById("win-btn-close");
  const titlebar = document.getElementById("app-titlebar");

  minBtn?.addEventListener("click", () => win?.minimize?.().catch(() => {}));
  maxBtn?.addEventListener("click", () => win?.toggleMaximize?.().catch(() => {}));
  closeBtn?.addEventListener("click", () => win?.close?.().catch(() => {}));

  titlebar?.addEventListener("dblclick", (e) => {
    if (e.target.closest("button, input, select, a, .titlebar-search-wrap")) return;
    win?.toggleMaximize?.().catch(() => {});
  });

  titlebar?.addEventListener("mousedown", (e) => {
    if (e.button !== 0 || e.detail > 1) return;
    if (e.target.closest("button, input, select, a, .titlebar-search-wrap")) return;
    if (e.target.hasAttribute("data-tauri-drag-region")) return;
    win?.startDragging?.().catch(() => {});
  });

  if (typeof onCtrlK === "function") {
    document.addEventListener("keydown", (e) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        onCtrlK();
      }
    });
  }
}

export function escapeHtml(value) {
  return String(value ?? "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

export const THEME_PRESETS = [
  { id: "dark", name: "Obsidian Dark", desc: "Layered Fluent slate with soft cool borders" },
  { id: "light", name: "Clean Daylight", desc: "Crisp Windows 11 alabaster daylight palette" },
  { id: "minecraft", name: "Minecraft Overworld", desc: "Voxel sky, oak groves, and diamond ore strata" },
  { id: "sunset", name: "Gulf Coast Sunset", desc: "Segmented neon horizon with laser grid" },
  { id: "aerospace", name: "Aerospace HUD", desc: "Tactical cockpit pitch ladder and emerald vectors" },
  { id: "cyberpunk", name: "Cyberpunk Night", desc: "Megacity rain towers with cyan and amber highlights" },
  { id: "terminal", name: "Retro CRT Amber", desc: "Phosphor oscilloscope sweep with scanline grid" },
  { id: "crimson", name: "Redline Carbon", desc: "Woven carbon texture and high-RPM redline arc" },
  { id: "ocean", name: "Abyssal Ocean", desc: "Deep-sea trench contours and sonar telemetry" },
  { id: "frost", name: "Nordic Frost", desc: "Arctic aurora ribbons over alpine peaks" }
];

export function applyTheme(themeId) {
  const valid = THEME_PRESETS.some((t) => t.id === themeId) ? themeId : "dark";
  document.body.setAttribute("data-theme", valid);
  localStorage.setItem("duke_systems_theme", valid);
  return valid;
}

export function getSavedTheme() {
  return localStorage.getItem("duke_systems_theme") || "dark";
}

/* ==========================================================================
   DUKE MONITOR — TAURI IPC BRIDGE, WINDOW CHROME & THEME REGISTRY
   ========================================================================== */

export async function invokeCommand(cmd, args = {}) {
  const fn = window.__TAURI__?.core?.invoke || window.__TAURI_INTERNALS__?.invoke;
  if (!fn) throw new Error(`Tauri IPC unavailable for '${cmd}'`);
  return fn(cmd, args);
}

export async function listenEvent(eventName, callback) {
  if (window.__TAURI__?.event?.listen) {
    return window.__TAURI__.event.listen(eventName, (event) => callback(event.payload));
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

export async function openExternalUrl(url) {
  if (!url) return;
  if (window.__TAURI__?.shell?.open) {
    await window.__TAURI__.shell.open(url);
  } else {
    window.open(url, "_blank", "noopener,noreferrer");
  }
}

export function escapeHtml(str) {
  return String(str ?? "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

export function formatBytes(bytes) {
  const b = Number(bytes || 0);
  if (b <= 0) return "0 MB";
  const gb = b / (1024 * 1024 * 1024);
  if (gb >= 1) return `${gb.toFixed(2)} GB`;
  return `${(b / (1024 * 1024)).toFixed(1)} MB`;
}

export function formatTimestamp(unixSecs) {
  if (!unixSecs) return "Never";
  return new Date(Number(unixSecs) * 1000).toLocaleTimeString();
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

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

function getTauriInvoke() {
  return window.__TAURI__?.core?.invoke || window.__TAURI_INTERNALS__?.invoke || null;
}

export async function invokeCommand(command, args = {}) {
  const invoke = getTauriInvoke();
  if (!invoke) {
    throw new Error(`Tauri IPC unavailable for command: ${command}`);
  }
  return invoke(command, args);
}

export async function listenEvent(eventName, handler) {
  const listen = window.__TAURI__?.event?.listen;
  if (!listen) return () => {};
  return listen(eventName, (event) => {
    handler(event?.payload);
  });
}

export function convertFileSrc(filePath) {
  if (!filePath) return "";
  if (window.__TAURI__?.core?.convertFileSrc) {
    return window.__TAURI__.core.convertFileSrc(filePath);
  }
  const normalized = String(filePath).replace(/\\/g, "/");
  return `https://asset.localhost/${encodeURI(normalized)}`;
}

export async function pickClipsFolder() {
  if (window.__TAURI__?.dialog?.open) {
    const selected = await window.__TAURI__.dialog.open({
      multiple: false,
      directory: true,
      title: "Select Clips Output Folder"
    });
    if (typeof selected === "string" && selected.trim()) {
      return selected;
    }
  }
  return null;
}

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

export function formatBytesJs(bytes) {
  const n = Number(bytes || 0);
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

export function formatDuration(seconds) {
  const s = Math.max(0, Number(seconds || 0));
  const mins = Math.floor(s / 60);
  const secs = Math.floor(s % 60);
  return `${String(mins).padStart(2, "0")}:${String(secs).padStart(2, "0")}`;
}

export function formatTimestamp(epochSecs) {
  if (!epochSecs) return "Just now";
  const d = new Date(Number(epochSecs) * 1000);
  return d.toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit"
  });
}

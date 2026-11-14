/* ==========================================================================
   DUKE LAUNCHER — TAURI IPC, WINDOW CHROME & UTILITIES
   ========================================================================== */

function getInvoke() {
  return (
    window.__TAURI__?.core?.invoke ||
    window.__TAURI_INTERNALS__?.invoke ||
    null
  );
}

export async function invokeCommand(cmd, args = {}) {
  const fn = getInvoke();
  if (!fn) throw new Error(`Tauri IPC unavailable for '${cmd}'`);
  return fn(cmd, args);
}

export async function setGameLaunchArgs(gameId, launchArgs) {
  return invokeCommand("set_game_launch_args", {
    gameId,
    launchArgs: launchArgs && launchArgs.trim() ? launchArgs.trim() : null
  });
}

export async function compressGameFolder(gameId) {
  return invokeCommand("compress_game_folder", { gameId });
}

export async function fetchCatalogGames(page = 0, query = "", forceRefresh = false) {
  return invokeCommand("fetch_catalog_games", {
    page,
    query: query && query.trim() ? query.trim() : null,
    forceRefresh: Boolean(forceRefresh)
  });
}

export async function getCatalogGameDetails(appId) {
  return invokeCommand("get_catalog_game_details", { appId: Number(appId) });
}

export async function clearCompletedDownloads() {
  return invokeCommand("clear_completed_downloads");
}

export async function getLauncherSettings() {
  return invokeCommand("get_launcher_settings");
}

export async function updateLauncherSettings(patch = {}) {
  return invokeCommand("update_launcher_settings", {
    autoCleanCompleted:
      typeof patch.autoCleanCompleted === "boolean" ? patch.autoCleanCompleted : null,
    steamgriddbApiKey:
      typeof patch.steamgriddbApiKey === "string" ? patch.steamgriddbApiKey : null,
    suspendHotkey:
      typeof patch.suspendHotkey === "string" ? patch.suspendHotkey : null,
    couchMode: typeof patch.couchMode === "boolean" ? patch.couchMode : null,
    autoDetectClipboard:
      typeof patch.autoDetectClipboard === "boolean" ? patch.autoDetectClipboard : null,
    nvmePrefetchEnabled:
      typeof patch.nvmePrefetchEnabled === "boolean" ? patch.nvmePrefetchEnabled : null,
    swarmEnabled:
      typeof patch.swarmEnabled === "boolean" ? patch.swarmEnabled : null,
    swarmNodeName:
      typeof patch.swarmNodeName === "string" ? patch.swarmNodeName : null,
    swarmPeers: Array.isArray(patch.swarmPeers) ? patch.swarmPeers : null,
    steamApiKey:
      typeof patch.steamApiKey === "string" ? patch.steamApiKey : null,
    clearSteamApiKey:
      typeof patch.clearSteamApiKey === "boolean" ? patch.clearSteamApiKey : null,
    steamIdOverride:
      typeof patch.steamIdOverride === "string" ? patch.steamIdOverride : null,
    xboxApiKey:
      typeof patch.xboxApiKey === "string" ? patch.xboxApiKey : null,
    clearXboxApiKey:
      typeof patch.clearXboxApiKey === "boolean" ? patch.clearXboxApiKey : null,
    psnNpsso:
      typeof patch.psnNpsso === "string" ? patch.psnNpsso : null,
    clearPsnNpsso:
      typeof patch.clearPsnNpsso === "boolean" ? patch.clearPsnNpsso : null
  });
}

export async function setGameLaunchProfile({
  gameId,
  sandboxEnabled = null,
  nvmePrefetchOverride = null
}) {
  return invokeCommand("set_game_launch_profile", {
    gameId,
    sandboxEnabled: typeof sandboxEnabled === "boolean" ? sandboxEnabled : null,
    nvmePrefetchOverride
  });
}

export async function createGameSnapshot(gameId, label = null) {
  return invokeCommand("create_game_snapshot", {
    gameId,
    label: label && String(label).trim() ? String(label).trim() : null
  });
}

export async function listGameSnapshots(gameId) {
  return invokeCommand("list_game_snapshots", { gameId });
}

export async function restoreGameSnapshot(gameId, snapshotId) {
  return invokeCommand("restore_game_snapshot", { gameId, snapshotId });
}

export async function deleteGameSnapshot(gameId, snapshotId) {
  return invokeCommand("delete_game_snapshot", { gameId, snapshotId });
}

export async function getCatalogMmapStats() {
  return invokeCommand("get_catalog_mmap_stats");
}

export async function searchCatalogMmap(query, limit = 50) {
  return invokeCommand("search_catalog_mmap", { query, limit });
}

export async function queryCatalogMmap(params) {
  return invokeCommand("query_catalog_mmap", { params });
}

export async function applyWindowBackdrop(themeId) {
  return invokeCommand("apply_window_backdrop", { themeId: String(themeId || "dark") });
}

export async function enrichCatalogQuery(query) {
  return invokeCommand("enrich_catalog_query", { query: String(query || "") });
}

export async function generateGameDkmanifest(gameId, outPath = null) {
  return invokeCommand("generate_game_dkmanifest", { gameId, outPath });
}

export async function diffGameAgainstManifest(gameId, manifestPath) {
  return invokeCommand("diff_game_against_manifest", { gameId, manifestPath });
}

export async function applyGameDeltaPatch(gameId, manifestPath, deltaPath = null) {
  return invokeCommand("apply_game_delta_patch", { gameId, manifestPath, deltaPath });
}

export async function getSwarmStatus() {
  return invokeCommand("get_swarm_status");
}

export async function probeSwarmPeers() {
  return invokeCommand("probe_swarm_peers");
}

export async function beamGameFromPeer({
  peerAddr,
  gameKey,
  destParentDir = null,
  registerInLibrary = true
}) {
  return invokeCommand("beam_game_from_peer", {
    peerAddr,
    gameKey,
    destParentDir,
    registerInLibrary
  });
}

export async function syncGameSavesMesh(gameId) {
  return invokeCommand("sync_game_saves_mesh", { gameId });
}

export async function preflightCheckExe(exePath) {
  return invokeCommand("preflight_check_exe", { exePath });
}

export async function healGameRuntimeDlls(exePath, dllNames = null) {
  return invokeCommand("heal_game_runtime_dlls", { exePath, dllNames });
}

export async function readClipboardText() {
  return invokeCommand("read_clipboard_text");
}

export async function writeClipboardText(text) {
  return invokeCommand("write_clipboard_text", { text: String(text || "") });
}

export async function takePendingProtocolUrls() {
  return invokeCommand("take_pending_protocol_urls");
}

export async function getImportedSources() {
  return invokeCommand("get_imported_sources");
}

export async function searchImportedSources({ query = "", sourceId = null, offset = 0, limit = 80 } = {}) {
  return invokeCommand("search_imported_sources", { query, sourceId, offset, limit });
}

export async function importSourceUrl(url) {
  return invokeCommand("import_source_url", { url });
}

export async function importSourceFile(filePath) {
  return invokeCommand("import_source_file", { filePath });
}

export async function toggleImportedSource(sourceId, enabled) {
  return invokeCommand("toggle_imported_source", { sourceId, enabled });
}

export async function refreshImportedSource(sourceId) {
  return invokeCommand("refresh_imported_source", { sourceId });
}

export async function refreshAllImportedSources() {
  return invokeCommand("refresh_all_imported_sources");
}

export async function removeImportedSource(sourceId) {
  return invokeCommand("remove_imported_source", { sourceId });
}

export async function getSearchSites() {
  return invokeCommand("get_search_sites");
}

export async function addSearchSite({
  name,
  searchUrl,
  tag = "Custom",
  note = "",
  homeUrl = null
}) {
  return invokeCommand("add_search_site", { name, searchUrl, tag, note, homeUrl });
}

export async function removeSearchSite(siteId) {
  return invokeCommand("remove_search_site", { siteId });
}

export async function bulkImportSearchSites(rawInput) {
  return invokeCommand("bulk_import_search_sites", { rawInput });
}

export async function resetDefaultSearchSites() {
  return invokeCommand("reset_default_search_sites");
}

export async function openDirectoryBrowser({
  url,
  title = null,
  customSaveDir = null,
  maxSpeedMbps = null
}) {
  return invokeCommand("open_directory_browser", {
    url,
    title,
    customSaveDir,
    maxSpeedMbps
  });
}

export function buildDirectorySearchUrl(site, query) {
  if (!site) return "";
  const cleanQuery = String(query || "").trim();
  if (!cleanQuery) {
    return site.homeUrl || String(site.searchUrl || "").replace(/\{query\}/gi, "");
  }
  const template = String(site.searchUrl || site.homeUrl || "");
  if (/\{query\}/i.test(template)) {
    return template.replace(/\{query\}/gi, encodeURIComponent(cleanQuery));
  }
  return template;
}

export async function syncLudusaviManifest(force = false) {
  return invokeCommand("sync_ludusavi_manifest", { force });
}

export async function discoverGameSavePaths(gameId, syncIfMissing = true) {
  return invokeCommand("discover_game_save_paths", { gameId, syncIfMissing });
}

export async function listSaveBackups(gameId) {
  return invokeCommand("list_save_backups", { gameId });
}

export async function restoreGameSave(gameId, backupPath) {
  return invokeCommand("restore_game_save", { gameId, backupPath });
}

export async function importExistingLibraries() {
  return invokeCommand("import_existing_libraries");
}

export async function searchSteamGridDbArtwork(queryOrAppId) {
  return invokeCommand("search_steamgriddb_artwork", { queryOrAppId: String(queryOrAppId || "") });
}

export async function applyCustomArtwork({ gameId, coverUrl = null, heroUrl = null, iconUrl = null }) {
  return invokeCommand("apply_custom_artwork", { gameId, coverUrl, heroUrl, iconUrl });
}

export async function createDesktopShortcutWithIcon(gameId) {
  return invokeCommand("create_desktop_shortcut_with_icon", { gameId });
}

export async function suspendOrResumeGame(gameId = null, action = null) {
  return invokeCommand("suspend_or_resume_game", { gameId, action });
}

export async function getActiveGameSessions() {
  return invokeCommand("get_active_game_sessions");
}

export async function pollGamepadState() {
  return invokeCommand("poll_gamepad_state");
}

export async function linkLocalGameWithSteamId({ exePath, customTitle, genre, steamAppId }) {
  return invokeCommand("add_local_game", {
    exePath,
    customTitle: customTitle || null,
    customCover: null,
    genre: genre || "Installed",
    steamAppId: steamAppId ? Number(steamAppId) : null
  });
}

export function stripSteamTokens(raw) {
  return String(raw || "")
    .replace(/\{STEAM_[A-Z0-9_]+\}(?:[\\/][^\s\[<>"'()]*)?/gi, " ")
    .replace(/\[img\][\s\S]*?\[\/img\]/gi, " ")
    .replace(/\[previewyoutube=[^\]]*\][\s\S]*?\[\/previewyoutube\]/gi, " ")
    .trim();
}

export function parseSteamBbcodeToSafeHtml(raw) {
  const preCleaned = stripSteamTokens(raw)
    .replace(/\r\n?/g, "\n")
    .replace(/\[list\]/gi, "\n[list]\n")
    .replace(/\[\/list\]/gi, "\n[/list]\n")
    .replace(/\[\*\]/gi, "\n[*]");

  const formatInline = (segment) => {
    const tokens = [];
    const withLinkTokens = segment.replace(
      /\[url=([^\]]+)\]([\s\S]*?)\[\/url\]/gi,
      (_m, rawHref, rawLabel) => {
        const cleanHref = String(rawHref || "").trim().replace(/^["']|["']$/g, "");
        const idx = tokens.length;
        tokens.push({ href: cleanHref, label: String(rawLabel || cleanHref) });
        return `\u0000LINK_${idx}\u0000`;
      }
    );

    let safe = escapeHtml(withLinkTokens)
      .replace(/\[b\]([\s\S]*?)\[\/b\]/gi, "<strong>$1</strong>")
      .replace(/\[i\]([\s\S]*?)\[\/i\]/gi, "<em>$1</em>")
      .replace(/\[h1\]([\s\S]*?)\[\/h1\]/gi, '<span class="bb-heading bb-h1">$1</span>')
      .replace(/\[h2\]([\s\S]*?)\[\/h2\]/gi, '<span class="bb-heading bb-h2">$1</span>')
      .replace(/\[h3\]([\s\S]*?)\[\/h3\]/gi, '<span class="bb-heading bb-h3">$1</span>')
      .replace(/\[\/?(?:url|u|s|spoiler|quote|code|noparse|table|tr|td|th|olist|hr)[^\]]*\]/gi, "");

    safe = safe.replace(/\u0000LINK_(\d+)\u0000/g, (_m, idxStr) => {
      const tok = tokens[Number(idxStr)];
      if (!tok) return "";
      const inner = formatInline(tok.label);
      if (isValidHttpUrl(tok.href)) {
        return `<button type="button" class="bb-link" data-external-url="${escapeHtml(tok.href)}">${inner}</button>`;
      }
      return inner;
    });

    return safe;
  };

  const lines = preCleaned.split("\n");
  const out = [];
  let inList = false;

  const closeList = () => {
    if (inList) {
      out.push("</ul>");
      inList = false;
    }
  };

  for (const rawLine of lines) {
    const line = rawLine.trim();
    if (!line) {
      continue;
    }
    if (/^\[list\]$/i.test(line)) {
      if (!inList) {
        out.push('<ul class="bb-list">');
        inList = true;
      }
      continue;
    }
    if (/^\[\/list\]$/i.test(line)) {
      closeList();
      continue;
    }
    if (line.startsWith("[*]")) {
      if (!inList) {
        out.push('<ul class="bb-list">');
        inList = true;
      }
      const itemBody = formatInline(line.slice(3).trim());
      if (itemBody) out.push(`<li>${itemBody}</li>`);
      continue;
    }

    closeList();
    if (/^\[h([1-3])\]([\s\S]*?)\[\/h\1\]$/i.test(line)) {
      out.push(`<div class="bb-heading-row">${formatInline(line)}</div>`);
    } else {
      out.push(`<p class="bb-para">${formatInline(line)}</p>`);
    }
  }

  closeList();
  return out.join("");
}

export async function listenEvent(eventName, callback) {
  if (window.__TAURI__?.event?.listen) {
    return window.__TAURI__.event.listen(eventName, (event) => callback(event.payload));
  }
  return () => {};
}

export function listenDownloadProgress(callback) {
  return listenEvent("download-progress", callback);
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

export async function pickExecutableFile() {
  if (window.__TAURI__?.dialog?.open) {
    const selected = await window.__TAURI__.dialog.open({
      multiple: false,
      directory: false,
      filters: [{ name: "Game Executable (.exe)", extensions: ["exe", "bat", "cmd"] }]
    });
    return typeof selected === "string" ? selected : null;
  }
  return null;
}

export async function pickAnyFileForScan() {
  if (window.__TAURI__?.dialog?.open) {
    const selected = await window.__TAURI__.dialog.open({
      multiple: false,
      directory: false,
      title: "Select File or Archive to Check for Malware"
    });
    return typeof selected === "string" ? selected : null;
  }
  return null;
}

export async function pickJsonSourceFile() {
  if (window.__TAURI__?.dialog?.open) {
    const selected = await window.__TAURI__.dialog.open({
      multiple: false,
      directory: false,
      title: "Select Community Hydra JSON Manifest (.json)",
      filters: [{ name: "Hydra JSON Source (*.json)", extensions: ["json"] }]
    });
    return typeof selected === "string" ? selected : null;
  }
  return null;
}

export async function pickDownloadFolder() {
  if (window.__TAURI__?.dialog?.open) {
    const selected = await window.__TAURI__.dialog.open({
      multiple: false,
      directory: true,
      title: "Select Download Folder"
    });
    return typeof selected === "string" ? selected : null;
  }
  return null;
}

export function isValidHttpUrl(rawUrl) {
  try {
    const parsed = new URL(String(rawUrl || "").trim());
    return parsed.protocol === "https:" || parsed.protocol === "http:";
  } catch {
    return false;
  }
}

export async function openExternalUrl(url) {
  if (!isValidHttpUrl(url)) return;
  const cleanUrl = String(url).trim();
  if (window.__TAURI__?.shell?.open) {
    await window.__TAURI__.shell.open(cleanUrl);
  } else {
    window.open(cleanUrl, "_blank", "noopener,noreferrer");
  }
}

export function convertFileSrc(filePath) {
  if (!filePath) return "";
  if (window.__TAURI__?.core?.convertFileSrc) {
    return window.__TAURI__.core.convertFileSrc(filePath);
  }
  return filePath;
}

export async function getSidebarIcons(gameIds = []) {
  const ids = Array.isArray(gameIds) ? gameIds.filter(Boolean) : [];
  if (ids.length === 0) return {};
  return invokeCommand("get_sidebar_icons", { gameIds: ids });
}

export function escapeHtml(str) {
  return String(str ?? "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

export function formatBytesJs(bytes) {
  const b = Number(bytes || 0);
  if (b <= 0) return "0 MB";
  const gb = b / (1024 * 1024 * 1024);
  if (gb >= 1) return `${gb.toFixed(2)} GB`;
  const mb = b / (1024 * 1024);
  if (mb >= 1) return `${mb.toFixed(1)} MB`;
  return `${Math.max(1, Math.round(b / 1024))} KB`;
}

export function formatDuration(totalSeconds) {
  const secs = Math.max(0, Math.floor(Number(totalSeconds || 0)));
  if (secs === 0) return "0s";
  const hrs = Math.floor(secs / 3600);
  const mins = Math.floor((secs % 3600) / 60);
  const rem = secs % 60;
  if (hrs > 0) return `${hrs}h ${mins}m`;
  if (mins > 0) return `${mins}m ${rem}s`;
  return `${rem}s`;
}

export function formatPlaytime(playtimeSeconds) {
  const secs = Math.max(0, Math.floor(Number(playtimeSeconds || 0)));
  if (secs < 60) return `${secs} sec`;
  const totalMins = Math.floor(secs / 60);
  if (totalMins < 60) return `${totalMins} min`;
  const hrs = (secs / 3600).toFixed(1);
  return `${hrs} hrs`;
}

export function formatLastPlayed(lastPlayedTs) {
  if (!lastPlayedTs) return "Never";
  const d = new Date(Number(lastPlayedTs) * 1000);
  if (Number.isNaN(d.getTime())) return "Recently";
  const now = new Date();
  if (d.toDateString() === now.toDateString()) {
    return "Today";
  }
  return d.toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" });
}

export function formatNewsDate(unixTs) {
  const d = new Date(Number(unixTs || 0) * 1000);
  if (Number.isNaN(d.getTime())) return "";
  return d.toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" });
}

export function generateFallbackCoverDataUri(title = "PC Game", subtitle = "Duke Library") {
  const cleanTitle = escapeHtml(String(title).slice(0, 26));
  const cleanSub = escapeHtml(String(subtitle).slice(0, 28));
  const svg = `<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 460 215'><defs><linearGradient id='g' x1='0' y1='0' x2='1' y2='1'><stop offset='0%' stop-color='%2313171f'/><stop offset='55%' stop-color='%231b2638'/><stop offset='100%' stop-color='%230e1116'/></linearGradient></defs><rect width='460' height='215' fill='url(%23g)'/><circle cx='390' cy='46' r='68' fill='%231a9fff' fill-opacity='0.14'/><text x='24' y='105' fill='%23f1f5f9' font-family='Segoe UI, sans-serif' font-size='22' font-weight='700'>${cleanTitle}</text><text x='24' y='132' fill='%2394a3b8' font-family='Segoe UI, sans-serif' font-size='13'>${cleanSub}</text></svg>`;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
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
  { id: "frost", name: "Nordic Frost", desc: "Arctic aurora ribbons over alpine peaks" },
  { id: "frosted-obsidian", name: "Obsidian Frosted Glass", desc: "Translucent Windows 11 acrylic backdrop with frosted glass panels" }
];

export async function getUnifiedFriends() {
  return invokeCommand("get_unified_friends");
}

export async function joinFriendSession(opts = {}) {
  return invokeCommand("join_friend_session", {
    platform: opts.platform || "Steam",
    steamAppId: opts.steamAppId ?? null,
    lobbySteamId: opts.lobbySteamId ?? null,
    friendSteamId: opts.friendSteamId ?? null,
    serverIp: opts.serverIp ?? null,
    peerAddr: opts.peerAddr ?? null,
    gameId: opts.gameId ?? null
  });
}

export async function syncStorefrontPlaytime() {
  return invokeCommand("sync_storefront_playtime");
}

export async function setGameUpdateConfig(gameId, versionTag, updateFeedUrl) {
  return invokeCommand("set_game_update_config", {
    gameId,
    versionTag: String(versionTag || ""),
    updateFeedUrl: String(updateFeedUrl || "")
  });
}

export async function checkGameUpdates(gameId = null) {
  return invokeCommand("check_game_updates", { gameId });
}

export async function applyGameUpdate(gameId) {
  return invokeCommand("apply_game_update", { gameId });
}

export async function getGameAchievements(gameId) {
  return invokeCommand("get_game_achievements", { gameId });
}

export async function setGameAchievementsPath(gameId, achievementsPath) {
  return invokeCommand("set_game_achievements_path", {
    gameId,
    achievementsPath: String(achievementsPath || "")
  });
}

export async function getGameCompatStatus(gameId) {
  return invokeCommand("get_game_compat_status", { gameId });
}

export async function toggleGameLaaPatch(gameId, enable) {
  return invokeCommand("toggle_game_laa_patch", { gameId, enable: Boolean(enable) });
}

export async function toggleGameDxvkWrapper(gameId, enable) {
  return invokeCommand("toggle_game_dxvk_wrapper", { gameId, enable: Boolean(enable) });
}

export async function getGameStorageStats(gameId) {
  return invokeCommand("get_game_storage_stats", { gameId });
}

export async function compressGameWof(gameId, algorithm = "LZX") {
  return invokeCommand("compress_game_wof", { gameId, algorithm: String(algorithm || "LZX") });
}

export async function decompressGameWof(gameId) {
  return invokeCommand("decompress_game_wof", { gameId });
}

export async function getGameExtendedTelemetry(gameId, forceRefresh = false) {
  return invokeCommand("get_game_extended_telemetry", {
    gameId,
    forceRefresh: Boolean(forceRefresh)
  });
}

export async function toggleOverlayWindow(visible = null) {
  return invokeCommand("toggle_overlay_window", { visible });
}

export async function hideOverlayWindow() {
  return invokeCommand("hide_overlay_window");
}

export async function triggerReplayClipIpc() {
  return invokeCommand("trigger_replay_clip_ipc");
}

export async function purgeSuiteRamIpc() {
  return invokeCommand("purge_suite_ram_ipc");
}


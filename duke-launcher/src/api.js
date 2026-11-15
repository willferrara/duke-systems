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
      typeof patch.clearPsnNpsso === "boolean" ? patch.clearPsnNpsso : null,
    defaultSaveDir:
      typeof patch.defaultSaveDir === "string" ? patch.defaultSaveDir : null,
    downloadSpeedPreset:
      typeof patch.downloadSpeedPreset === "string" ? patch.downloadSpeedPreset : null,
    compactLibraryCards:
      typeof patch.compactLibraryCards === "boolean" ? patch.compactLibraryCards : null,
    performanceBoostEnabled:
      typeof patch.performanceBoostEnabled === "boolean" ? patch.performanceBoostEnabled : null,
    autoBackupSavesOnExit:
      typeof patch.autoBackupSavesOnExit === "boolean" ? patch.autoBackupSavesOnExit : null,
    cpuPriorityMode:
      typeof patch.cpuPriorityMode === "string" ? patch.cpuPriorityMode : null,
    ramTrimMode:
      typeof patch.ramTrimMode === "string" ? patch.ramTrimMode : null,
    autoFreezeMinimized:
      typeof patch.autoFreezeMinimized === "boolean" ? patch.autoFreezeMinimized : null,
    nvmeCacheBudgetPct:
      typeof patch.nvmeCacheBudgetPct === "number" ? patch.nvmeCacheBudgetPct : null,
    dohEnabled:
      typeof patch.dohEnabled === "boolean" ? patch.dohEnabled : null,
    customProxyUrl:
      typeof patch.customProxyUrl === "string" ? patch.customProxyUrl : null,
    customSpeedCapMbps:
      typeof patch.customSpeedCapMbps === "number" ? patch.customSpeedCapMbps : null,
    autoUnblockMotw:
      typeof patch.autoUnblockMotw === "boolean" ? patch.autoUnblockMotw : null,
    portableSandboxDefault:
      typeof patch.portableSandboxDefault === "boolean" ? patch.portableSandboxDefault : null,
    maxRestorePoints:
      typeof patch.maxRestorePoints === "number" ? patch.maxRestorePoints : null,
    blockGameTelemetry:
      typeof patch.blockGameTelemetry === "boolean" ? patch.blockGameTelemetry : null,
    globalLaunchArgs:
      typeof patch.globalLaunchArgs === "string" ? patch.globalLaunchArgs : null,
    swarmAutoSyncSaves:
      typeof patch.swarmAutoSyncSaves === "boolean" ? patch.swarmAutoSyncSaves : null,
    startMinimizedWithWindows:
      typeof patch.startMinimizedWithWindows === "boolean"
        ? patch.startMinimizedWithWindows
        : null,
    theme: typeof patch.theme === "string" ? patch.theme : null,
    cardZoomPx: typeof patch.cardZoomPx === "number" ? patch.cardZoomPx : null,
    projfsStreamingEnabled:
      typeof patch.projfsStreamingEnabled === "boolean" ? patch.projfsStreamingEnabled : null,
    shaderSwarmEnabled:
      typeof patch.shaderSwarmEnabled === "boolean" ? patch.shaderSwarmEnabled : null,
    framePacingEnabled:
      typeof patch.framePacingEnabled === "boolean" ? patch.framePacingEnabled : null,
    diskPressureThresholdGb:
      typeof patch.diskPressureThresholdGb === "number" ? patch.diskPressureThresholdGb : null,
    diskPressureDaysThreshold:
      typeof patch.diskPressureDaysThreshold === "number" ? patch.diskPressureDaysThreshold : null,
    discordRpcEnabled:
      typeof patch.discordRpcEnabled === "boolean" ? patch.discordRpcEnabled : null,
    realtimeSaveWatcherEnabled:
      typeof patch.realtimeSaveWatcherEnabled === "boolean"
        ? patch.realtimeSaveWatcherEnabled
        : null,
    desktopChordEnabled:
      typeof patch.desktopChordEnabled === "boolean" ? patch.desktopChordEnabled : null,
    minimizeToTray:
      typeof patch.minimizeToTray === "boolean" ? patch.minimizeToTray : null,
    closeToTray:
      typeof patch.closeToTray === "boolean" ? patch.closeToTray : null,
    autoCheckUpdates:
      typeof patch.autoCheckUpdates === "boolean" ? patch.autoCheckUpdates : null
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
  return invokeCommand("search_steamgriddb_artwork", { queryOrAppid: String(queryOrAppId || "") });
}

export async function searchWebCoversForGame(query, platformHint = null, steamAppId = null) {
  return invokeCommand("search_web_covers_for_game", {
    query: String(query || ""),
    platformHint: platformHint || null,
    steamAppId: steamAppId ? Number(steamAppId) : null
  });
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
  try {
    const listenFn = window.__TAURI__?.event?.listen;
    if (typeof listenFn === "function") {
      return await listenFn(eventName, (event) => callback(event.payload));
    }
  } catch {
    // WebView event capability or Tauri IPC may be unavailable during early startup or browser preview.
  }
  return () => {};
}

export function listenDownloadProgress(callback) {
  return listenEvent("download-progress", callback);
}

export function initWindowControls({ onCtrlK } = {}) {
  let win = null;
  try {
    win = window.__TAURI__?.window?.getCurrentWindow?.() ?? null;
  } catch {
    win = null;
  }
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

export async function pickGameFolder() {
  if (window.__TAURI__?.dialog?.open) {
    const selected = await window.__TAURI__.dialog.open({
      multiple: false,
      directory: true,
      title: "Select Game Folder"
    });
    return typeof selected === "string" ? selected : null;
  }
  return null;
}

export async function pickExecutableOrShortcutFile() {
  if (window.__TAURI__?.dialog?.open) {
    const selected = await window.__TAURI__.dialog.open({
      multiple: false,
      directory: false,
      title: "Select Game Executable or Shortcut",
      filters: [
        { name: "Game Executable or Shortcut", extensions: ["exe", "lnk", "bat", "cmd"] }
      ]
    });
    return typeof selected === "string" ? selected : null;
  }
  return null;
}

export async function pickCustomCoverImage() {
  if (window.__TAURI__?.dialog?.open) {
    const selected = await window.__TAURI__.dialog.open({
      multiple: false,
      directory: false,
      title: "Pick Custom Cover Artwork",
      filters: [{ name: "Cover Image (PNG, JPG, WebP)", extensions: ["png", "jpg", "jpeg", "webp"] }]
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

export function isValidDownloadUrl(rawUrl) {
  const trimmed = String(rawUrl || "").trim();
  if (!trimmed) return false;
  if (isValidHttpUrl(trimmed)) return true;
  if (/^duke:\/\/download\b/i.test(trimmed)) {
    try {
      const parsed = new URL(trimmed);
      const inner = parsed.searchParams.get("url");
      return Boolean(inner && isValidHttpUrl(inner));
    } catch {
      return false;
    }
  }
  return false;
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
  { id: "frost", name: "Nordic Frost", desc: "Arctic aurora ribbons over alpine peaks" }
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

export async function ingestDroppedPaths(paths = []) {
  return invokeCommand("ingest_dropped_paths", {
    paths: Array.isArray(paths) ? paths : []
  });
}

export async function scanAndCleanDesktopShortcuts() {
  return invokeCommand("scan_and_clean_desktop_shortcuts");
}

export async function autoFixGame(gameId) {
  return invokeCommand("auto_fix_game", { gameId });
}

export async function getDiskFreeSpace(path = null) {
  return invokeCommand("get_disk_free_space", { path: path || null });
}

export async function reclaimSnapshotSpace() {
  return invokeCommand("reclaim_snapshot_space");
}

export async function registerDukeProtocol() {
  return invokeCommand("register_duke_protocol");
}

export async function runSubsystemBenchmark() {
  return invokeCommand("run_subsystem_benchmark");
}

export async function syncRetroCatalog(forceRefresh = false) {
  return invokeCommand("sync_retro_catalog", { forceRefresh: Boolean(forceRefresh) });
}

export async function getRetroPlatforms() {
  return invokeCommand("get_retro_platforms");
}

export async function getEmulatorStatuses() {
  return invokeCommand("get_emulator_statuses");
}

export async function installEmulatorForConsole(consoleId) {
  return invokeCommand("install_emulator_for_console", { consoleId: String(consoleId || "") });
}

export async function setCustomEmulatorPath(consoleId, exePath = "") {
  return invokeCommand("set_custom_emulator_path", {
    consoleId: String(consoleId || ""),
    exePath: String(exePath || "")
  });
}

export async function checkGameEmulatorReady(gameId) {
  return invokeCommand("check_game_emulator_ready", { gameId: String(gameId || "") });
}

export async function inspectRomPath(path) {
  return invokeCommand("inspect_rom_path", { path: String(path || "") });
}

export async function addRetroGameFromCatalog({ appId, romPath = null }) {
  return invokeCommand("add_retro_game_from_catalog", {
    appId: Number(appId),
    romPath: romPath || null
  });
}

export async function linkGameRom({ gameId, romPath }) {
  return invokeCommand("link_game_rom", {
    gameId: String(gameId || ""),
    romPath: String(romPath || "")
  });
}

export async function scanRomFolder(folderPath) {
  return invokeCommand("scan_rom_folder", { folderPath: String(folderPath || "") });
}

export async function pickRomFile() {
  if (window.__TAURI__?.dialog?.open) {
    const selected = await window.__TAURI__.dialog.open({
      multiple: false,
      directory: false,
      title: "Select Console ROM, Disc Image, or Archive",
      filters: [
        {
          name: "Console ROMs & Disc Images",
          extensions: [
            "nes", "unf", "sfc", "smc", "z64", "v64", "n64",
            "iso", "gcm", "rvz", "wbfs", "wia", "wux", "wua", "wud", "rpx",
            "nsp", "xci", "nro", "gb", "gbc", "gba", "nds", "3ds", "cci", "cxi", "cia", "vb",
            "bin", "cue", "chd", "cso", "pbp", "md", "gen", "smd", "sms", "cdi", "gdi", "a26", "pce", "sgx",
            "zip"
          ]
        },
        { name: "All Files", extensions: ["*"] }
      ]
    });
    return typeof selected === "string" ? selected : null;
  }
  return null;
}

export function listenEmulatorInstallProgress(callback) {
  return listenEvent("emulator-install-progress", callback);
}

export function listenRetroCatalogSyncProgress(callback) {
  return listenEvent("retro-catalog-sync-progress", callback);
}

export async function getAppVersion() {
  try {
    const tauriVer = await window.__TAURI__?.app?.getVersion?.();
    if (tauriVer && String(tauriVer).trim()) {
      return String(tauriVer).trim().replace(/^v/i, "");
    }
  } catch {
    // Fall back to explicit Rust compile-time CARGO_PKG_VERSION IPC
  }
  const backendVer = await invokeCommand("get_app_version");
  return String(backendVer || "0.9.0").trim().replace(/^v/i, "");
}

export async function resolveCatalogCover(req = {}) {
  return invokeCommand("resolve_catalog_cover", {
    appId: Number(req.appId || 0),
    title: String(req.title || ""),
    platformId: req.platformId != null ? Number(req.platformId) : null,
    platformName: req.platformName ? String(req.platformName) : null,
    coverUrl: req.coverUrl ? String(req.coverUrl) : null
  });
}

// =========================================================================
// Frontier v0.5.0 Systems Engine IPC Bindings
// =========================================================================

export async function getFramePacingAndIoringStatus() {
  return invokeCommand("get_frame_pacing_and_ioring_status");
}

export async function toggleHalfMsFramePacing(enabled) {
  return invokeCommand("toggle_half_ms_frame_pacing", { enabled: Boolean(enabled) });
}

export async function purgeStaleDriverShaders() {
  return invokeCommand("purge_stale_driver_shaders");
}

export async function getProjfsRuntimeStatus() {
  return invokeCommand("get_projfs_runtime_status");
}

export async function enableProjfsWindowsFeature() {
  return invokeCommand("enable_projfs_windows_feature");
}

export async function mountProjfsVirtualGame({
  gameId,
  gameTitle,
  mountDir,
  backingUrl = null,
  backingFilePath = null,
  totalBytes = null,
  jobId = null,
  files = []
}) {
  return invokeCommand("mount_projfs_virtual_game", {
    gameId,
    gameTitle,
    mountDir,
    backingUrl,
    backingFilePath,
    totalBytes,
    jobId,
    files: Array.isArray(files) ? files : []
  });
}

export async function unmountProjfsVirtualGame(mountId) {
  return invokeCommand("unmount_projfs_virtual_game", { mountId });
}

export async function deduplicateLibraryCas() {
  return invokeCommand("deduplicate_library_cas");
}

export async function getShaderSwarmStatus() {
  return invokeCommand("get_shader_swarm_status");
}

export async function syncGameShadersFromSwarm(gameId) {
  return invokeCommand("sync_game_shaders_from_swarm", { gameId });
}

export async function archiveGameShaderBundle(gameId) {
  return invokeCommand("archive_game_shader_bundle", { gameId });
}

export async function getQuickResumeSlots() {
  return invokeCommand("get_quick_resume_slots");
}

export async function quickSwitchGameSlot(targetGameId) {
  return invokeCommand("quick_switch_game_slot", { targetGameId });
}

export async function suspendToQuickResumeSlot(gameId) {
  return invokeCommand("suspend_to_quick_resume_slot", { gameId });
}

export async function removeQuickResumeSlot(gameId, terminateProcess = false) {
  return invokeCommand("remove_quick_resume_slot", {
    gameId,
    terminateProcess: Boolean(terminateProcess)
  });
}

export async function getSteamCmStatus() {
  return invokeCommand("get_steam_cm_status");
}

export async function connectSteamCm({ refreshToken = null, personaName = null } = {}) {
  return invokeCommand("connect_steam_cm", {
    refreshToken: refreshToken && String(refreshToken).trim() ? String(refreshToken).trim() : null,
    personaName: personaName && String(personaName).trim() ? String(personaName).trim() : null
  });
}

export async function disconnectSteamCm() {
  return invokeCommand("disconnect_steam_cm");
}

export async function sendSteamChatMessage({ peerSteamId, peerName = null, message }) {
  return invokeCommand("send_steam_chat_message", {
    peerSteamId: String(peerSteamId || ""),
    peerName: peerName ? String(peerName) : null,
    message: String(message || "")
  });
}

export async function getSteamChatMessages(peerSteamId = null) {
  return invokeCommand("get_steam_chat_messages", {
    peerSteamId: peerSteamId ? String(peerSteamId) : null
  });
}

export async function getSteamPendingInvites() {
  return invokeCommand("get_steam_pending_invites");
}

export async function respondSteamGameInvite({ inviteId, accept }) {
  return invokeCommand("respond_steam_game_invite", {
    inviteId: String(inviteId || ""),
    accept: Boolean(accept)
  });
}

export async function listSteamCloudFiles(appId) {
  return invokeCommand("list_steam_cloud_files", { appId: Number(appId || 0) });
}

export async function syncSteamCloudSaves(gameId, appId) {
  return invokeCommand("sync_steam_cloud_saves", { gameId, appId: Number(appId || 0) });
}

export async function getStoreDirectStatus() {
  return invokeCommand("get_store_direct_status");
}

export async function loginGogDirect(authCodeOrToken) {
  return invokeCommand("login_gog_direct", {
    authCodeOrToken: String(authCodeOrToken || "").trim()
  });
}

export async function loginEpicDirect(authorizationCode) {
  return invokeCommand("login_epic_direct", {
    authorizationCode: String(authorizationCode || "").trim()
  });
}

export async function listGogOwnedTitles() {
  return invokeCommand("list_gog_owned_titles");
}

export async function listEpicOwnedTitles() {
  return invokeCommand("list_epic_owned_titles");
}

export async function startStoreDirectInstall({
  store,
  productId,
  appName = null,
  namespace = null,
  title,
  saveDir = null
}) {
  return invokeCommand("start_store_direct_install", {
    store: String(store || ""),
    productId: String(productId || ""),
    appName: appName ? String(appName) : null,
    namespace: namespace ? String(namespace) : null,
    title: String(title || ""),
    saveDir: saveDir && String(saveDir).trim() ? String(saveDir).trim() : null
  });
}

export async function scanGameModGraph(gameId) {
  return invokeCommand("scan_game_mod_graph", { gameId });
}

export async function registerGameModPack({ gameId, sourceDir, packName }) {
  return invokeCommand("register_game_mod_pack", {
    gameId,
    sourceDir: String(sourceDir || ""),
    packName: String(packName || "")
  });
}

export async function saveGameModLoadout({ gameId, loadoutName, enabledPackIds }) {
  return invokeCommand("save_game_mod_loadout", {
    gameId,
    loadoutName: String(loadoutName || ""),
    enabledPackIds: Array.isArray(enabledPackIds) ? enabledPackIds : []
  });
}

export async function activateGameModLoadout(gameId, loadoutId = null) {
  return invokeCommand("activate_game_mod_loadout", {
    gameId,
    loadoutId: loadoutId ? String(loadoutId) : null
  });
}

// --- Wave 1: Suite Autonomy, Duke Link Co-Op & Duke Time-Machine ---
export async function getSuiteAutonomyStatus() {
  return invokeCommand("get_suite_autonomy_status");
}

export async function getDukeLinkStatus() {
  return invokeCommand("get_duke_link_status");
}

export async function startCoOpHost({ gameId = null, gameTitle = null, bindPort = null } = {}) {
  return invokeCommand("start_coop_host", {
    gameId: gameId || null,
    gameTitle: gameTitle || null,
    bindPort: typeof bindPort === "number" ? bindPort : null
  });
}

export async function stopCoOpSession() {
  return invokeCommand("stop_coop_session");
}

export async function inviteFriendToCoOp({
  friendName,
  platform = "Steam",
  steamId = null,
  peerHint = null,
  gameTitle = null
}) {
  return invokeCommand("invite_friend_to_coop", {
    friendName: String(friendName || "Friend"),
    platform: String(platform || "Steam"),
    steamId: steamId || null,
    peerHint: peerHint || null,
    gameTitle: gameTitle || null
  });
}

export async function joinCoOpStream({ hostAddr, roomCode = null, gameTitle = null }) {
  return invokeCommand("join_coop_stream", {
    hostAddr: String(hostAddr || ""),
    roomCode: roomCode || null,
    gameTitle: gameTitle || null
  });
}

export async function sendCoOpInputP2(input) {
  return invokeCommand("send_coop_input_p2", { input });
}

export async function sendCoOpInputSlot(slot = 1, controllerKind = "xinput", input) {
  return invokeCommand("send_coop_input_slot", {
    slot: Number(slot || 0),
    controllerKind: controllerKind || null,
    input
  });
}

export async function getGameTimeMachine(gameId) {
  return invokeCommand("get_game_time_machine", { gameId: String(gameId || "") });
}

export async function createTimeMachineCheckpoint(gameId, label = null, trigger = "manual") {
  return invokeCommand("create_time_machine_checkpoint", {
    gameId: String(gameId || ""),
    label: label || null,
    trigger: trigger || "manual"
  });
}

export async function createTimeMachineBranch(gameId, branchName, fromCheckpointId = null) {
  return invokeCommand("create_time_machine_branch", {
    gameId: String(gameId || ""),
    branchName: String(branchName || ""),
    fromCheckpointId: fromCheckpointId || null
  });
}

export async function switchTimeMachineBranch(gameId, branchName, restoreHead = true) {
  return invokeCommand("switch_time_machine_branch", {
    gameId: String(gameId || ""),
    branchName: String(branchName || ""),
    restoreHead: Boolean(restoreHead)
  });
}

export async function restoreTimeMachineCheckpoint(gameId, checkpointId) {
  return invokeCommand("restore_time_machine_checkpoint", {
    gameId: String(gameId || ""),
    checkpointId: String(checkpointId || "")
  });
}

export async function deleteTimeMachineCheckpoint(gameId, checkpointId) {
  return invokeCommand("delete_time_machine_checkpoint", {
    gameId: String(gameId || ""),
    checkpointId: String(checkpointId || "")
  });
}

// =========================================================================
// Wave 2 (v0.7.0) DirectStorage, VHDX Hot-Tier, Upscaler, Latency Lab, ETW X-Ray & Deep Shrink
// =========================================================================

export async function probeDirectStorageCapability(gameId = null) {
  return invokeCommand("probe_direct_storage_capability", {
    gameId: gameId ? String(gameId) : null
  });
}

export async function scanDirectStorageAssets(gameId, minFileMb = null) {
  return invokeCommand("scan_direct_storage_assets", {
    gameId: String(gameId || ""),
    minFileMb: minFileMb != null ? Number(minFileMb) : null
  });
}

export async function repackGameDirectStorageAssets(gameId, zstdLevel = 3, minFileMb = null, maxArchives = null) {
  return invokeCommand("repack_game_direct_storage_assets", {
    gameId: String(gameId || ""),
    zstdLevel: zstdLevel != null ? Number(zstdLevel) : null,
    minFileMb: minFileMb != null ? Number(minFileMb) : null,
    maxArchives: maxArchives != null ? Number(maxArchives) : null
  });
}

export async function verifyDirectStorageStreams(gameId) {
  return invokeCommand("verify_direct_storage_streams", {
    gameId: String(gameId || "")
  });
}

export async function getVhdxStageStatus(gameId = null) {
  return invokeCommand("get_vhdx_stage_status", {
    gameId: gameId ? String(gameId) : null
  });
}

export async function containerizeGameToVhdx(gameId, customContainerDir = null) {
  return invokeCommand("containerize_game_to_vhdx", {
    gameId: String(gameId || ""),
    customContainerDir: customContainerDir ? String(customContainerDir) : null
  });
}

export async function stageGameHotTier(gameId, budgetMb = 4096, autoStageOnLaunch = null) {
  return invokeCommand("stage_game_hot_tier", {
    gameId: String(gameId || ""),
    budgetMb: budgetMb != null ? Number(budgetMb) : null,
    autoStageOnLaunch: autoStageOnLaunch != null ? Boolean(autoStageOnLaunch) : null
  });
}

export async function unstageOrDetachVhdx(gameId) {
  return invokeCommand("unstage_or_detach_vhdx", {
    gameId: String(gameId || "")
  });
}

export async function getGameUpscaleProfile(gameId, exePath, emulatorId = null) {
  return invokeCommand("get_game_upscale_profile", {
    gameId: String(gameId || ""),
    exePath: String(exePath || ""),
    emulatorId: emulatorId ? String(emulatorId) : null
  });
}

export async function saveGameUpscaleProfile(req = {}) {
  return invokeCommand("save_game_upscale_profile", {
    gameId: String(req.gameId || ""),
    exePath: String(req.exePath || ""),
    borderlessFlipPromote: Boolean(req.borderlessFlipPromote),
    upscaleMode: String(req.upscaleMode || "off"),
    fsrSharpness: Number(req.fsrSharpness ?? 0.75),
    fpsCap: Number(req.fpsCap ?? 0),
    autoHdr: Boolean(req.autoHdr),
    emulatorId: req.emulatorId ? String(req.emulatorId) : null
  });
}

export async function promoteGameWindowBorderless(pids = []) {
  return invokeCommand("promote_game_window_borderless", {
    pids: Array.isArray(pids) ? pids.map((p) => Number(p)) : []
  });
}

export async function getLatencyLabStatus() {
  return invokeCommand("get_latency_lab_status");
}

export async function getGameLatencyProfile(exePath) {
  return invokeCommand("get_game_latency_profile", {
    exePath: String(exePath || "")
  });
}

export async function applyGameLatencyProfile(req = {}) {
  return invokeCommand("apply_game_latency_profile", {
    exePath: String(req.exePath || ""),
    gpuPreference: Number(req.gpuPreference ?? 2),
    flipModelUpgrade: Boolean(req.flipModelUpgrade),
    autoHdrEnable: Boolean(req.autoHdrEnable),
    autoSrMode: String(req.autoSrMode || "off")
  });
}

export async function removeGameLatencyProfile(exePath) {
  return invokeCommand("remove_game_latency_profile", {
    exePath: String(exePath || "")
  });
}

export async function runDriverProfileGuard(forceReapply = false) {
  return invokeCommand("run_driver_profile_guard", {
    forceReapply: Boolean(forceReapply)
  });
}

export async function enforceShaderCacheCap(capMb = 4096) {
  return invokeCommand("enforce_shader_cache_cap", {
    capMb: Number(capMb || 4096)
  });
}

export async function probeEtwXrayCapability() {
  return invokeCommand("probe_etw_xray_capability");
}

export async function getGameXrayScorecards(gameId) {
  return invokeCommand("get_game_xray_scorecards", {
    gameId: String(gameId || "")
  });
}

export async function clearGameXrayScorecards(gameId) {
  return invokeCommand("clear_game_xray_scorecards", {
    gameId: String(gameId || "")
  });
}

export async function scanGameDeepShrink(gameId, keepLanguages = ["english"]) {
  return invokeCommand("scan_game_deep_shrink", {
    gameId: String(gameId || ""),
    keepLanguages: Array.isArray(keepLanguages) ? keepLanguages : ["english"]
  });
}

export async function applyGameDeepShrink(
  gameId,
  stripLanguages = [],
  dedupInternalMedia = true,
  keepLanguages = ["english"],
  conditionTexturesBler = true
) {
  return invokeCommand("apply_game_deep_shrink", {
    gameId: String(gameId || ""),
    stripLanguages: Array.isArray(stripLanguages) ? stripLanguages : [],
    dedupInternalMedia: Boolean(dedupInternalMedia),
    keepLanguages: Array.isArray(keepLanguages) ? keepLanguages : ["english"],
    conditionTexturesBler: Boolean(conditionTexturesBler)
  });
}

export async function deduplicateLibraryBlake3(extraLibraryRoot = null, minFileKb = 512) {
  return invokeCommand("deduplicate_library_blake3", {
    extraLibraryRoot: extraLibraryRoot ? String(extraLibraryRoot) : null,
    minFileKb: minFileKb != null ? Number(minFileKb) : null
  });
}

// ---------------------------------------------------------------------------
// Wave 3 (v0.8.0): Zero-VRAM NPU / Windows ML On-Device AI Suite
// ---------------------------------------------------------------------------

export async function getAiRuntimeStatus() {
  return invokeCommand("get_ai_runtime_status");
}

export async function updateAiRuntimeConfig(req = {}) {
  return invokeCommand("update_ai_runtime_config", {
    preferredProvider: req.preferredProvider ?? null,
    autoUnloadEnabled: req.autoUnloadEnabled ?? null,
    idleTtlSecs: req.idleTtlSecs != null ? Number(req.idleTtlSecs) : null,
    foundryEndpoint: req.foundryEndpoint ?? null,
    foundryModel: req.foundryModel ?? null,
    targetLanguage: req.targetLanguage ?? null
  });
}

export async function unloadAiWorkers() {
  return invokeCommand("unload_ai_workers");
}

export async function triggerDukeVisionCapture(req = {}) {
  return invokeCommand("trigger_duke_vision_capture", {
    gameTitle: req.gameTitle ?? null,
    sourceLang: req.sourceLang ?? null,
    targetLang: req.targetLang ?? null,
    mode: req.mode ?? null
  });
}

export async function analyzeVisionTextQuery(req = {}) {
  return invokeCommand("analyze_vision_text_query", {
    text: String(req.text || ""),
    gameTitle: req.gameTitle ?? null,
    sourceLang: req.sourceLang ?? null,
    targetLang: req.targetLang ?? null,
    mode: req.mode ?? null
  });
}

export async function getLatestVisionState() {
  return invokeCommand("get_latest_vision_state");
}

export function listenDukeVisionResult(callback) {
  return listenEvent("duke-vision-result", callback);
}

export async function getDukeCommsState() {
  return invokeCommand("get_duke_comms_state");
}

export async function toggleDukeCommsListening(req = {}) {
  return invokeCommand("toggle_duke_comms_listening", {
    enabled: Boolean(req.enabled),
    targetLanguage: req.targetLanguage ?? null,
    voiceMacrosEnabled: req.voiceMacrosEnabled ?? null,
    teammateSubtitlesEnabled: req.teammateSubtitlesEnabled ?? null
  });
}

export async function processVoicePhraseCommand(req = {}) {
  return invokeCommand("process_voice_phrase_command", {
    phrase: String(req.phrase || ""),
    isTeammateAudio: Boolean(req.isTeammateAudio),
    sourceLang: req.sourceLang ?? null,
    speaker: req.speaker ?? null
  });
}

export async function upsertVoiceMacroRule(rule = {}) {
  return invokeCommand("upsert_voice_macro_rule", {
    id: rule.id ?? null,
    phrase: String(rule.phrase || ""),
    actionKind: String(rule.actionKind || "HidKeys"),
    keys: Array.isArray(rule.keys) ? rule.keys.map((k) => String(k)) : [],
    holdMs: rule.holdMs != null ? Number(rule.holdMs) : 50,
    description: rule.description ?? null,
    enabled: rule.enabled !== false
  });
}

export function listenDukeCommsVad(callback) {
  return listenEvent("duke-comms-vad", callback);
}

export function listenDukeCommsAction(callback) {
  return listenEvent("duke-comms-action", callback);
}

export function listenDukeCommsSubtitle(callback) {
  return listenEvent("duke-comms-subtitle", callback);
}

export async function diagnoseGameCrash(gameId, extraLogPath = null) {
  return invokeCommand("diagnose_game_crash", {
    gameId: String(gameId),
    extraLogPath: extraLogPath ? String(extraLogPath) : null
  });
}

export async function doctorRevertGameSnapshot(gameId, snapshotId) {
  return invokeCommand("doctor_revert_game_snapshot", {
    gameId: String(gameId),
    snapshotId: String(snapshotId)
  });
}

export async function getDukeDirectorState(gameId = null) {
  return invokeCommand("get_duke_director_state", {
    gameId: gameId ? String(gameId) : null
  });
}

export async function toggleDukeDirector(enabled, sensitivity = null) {
  return invokeCommand("toggle_duke_director", {
    enabled: Boolean(enabled),
    sensitivity: sensitivity != null ? Number(sensitivity) : null
  });
}

export async function triggerDirectorHighlightNow(req = {}) {
  return invokeCommand("trigger_director_highlight_now", {
    gameId: req.gameId ?? null,
    gameTitle: req.gameTitle ?? null,
    bossOrAchievement: req.bossOrAchievement ?? null,
    targetDurationSecs: req.targetDurationSecs != null ? Number(req.targetDurationSecs) : null
  });
}

export function listenDukeDirectorHighlight(callback) {
  return listenEvent("duke-director-highlight", callback);
}

export async function analyzeGameConfigPreset(gameId, targetHz = null) {
  return invokeCommand("analyze_game_config_preset", {
    gameId: String(gameId),
    targetHz: targetHz != null ? Number(targetHz) : null
  });
}

export async function applyGameConfigPreset(gameId, targetHz = null) {
  return invokeCommand("apply_game_config_preset", {
    gameId: String(gameId),
    targetHz: targetHz != null ? Number(targetHz) : null
  });
}

export async function rollbackGameConfigPreset(gameId, backupId = null) {
  return invokeCommand("rollback_game_config_preset", {
    gameId: String(gameId),
    backupId: backupId ? String(backupId) : null
  });
}

export async function getAcousticsState(gameId = null) {
  return invokeCommand("get_acoustics_state", {
    gameId: gameId ? String(gameId) : null
  });
}

export async function setWasapiSessionVolume(pid, volumePct, muted = false) {
  return invokeCommand("set_wasapi_session_volume", {
    pid: Number(pid),
    volumePct: Number(volumePct),
    muted: Boolean(muted)
  });
}

export async function saveGameEqAndDenoise(req = {}) {
  const rawBands = Array.isArray(req.bandsDb) ? req.bandsDb : [0, 0, 0, 0, 0];
  const bandsDb = [0, 1, 2, 3, 4].map((i) => Number(rawBands[i] || 0));
  return invokeCommand("save_game_eq_and_denoise", {
    gameId: req.gameId ? String(req.gameId) : null,
    presetName: String(req.presetName || "Neutral Flat"),
    bandsDb,
    micDenoiseEnabled: Boolean(req.micDenoiseEnabled),
    micDenoiseStrength: req.micDenoiseStrength != null ? Number(req.micDenoiseStrength) : 0.72
  });
}

export async function testMicRnnoiseLatency(strength = 0.75) {
  return invokeCommand("test_mic_rnnoise_latency", {
    strength: Number(strength)
  });
}

// =========================================================================
// Wave 4 (v0.9.0): Duke Pilot, DKCAT003 Play Tonight, Couch Mode, .dkpack, Capsule
// =========================================================================

export async function planDukePilotCommand(query, useLocalAi = false) {
  return invokeCommand("plan_duke_pilot_command", {
    query: String(query || ""),
    useLocalAi: Boolean(useLocalAi)
  });
}

export async function executeDukePilotPlan(plan, explicitImportPath = null) {
  return invokeCommand("execute_duke_pilot_plan", {
    plan,
    explicitImportPath: explicitImportPath ? String(explicitImportPath) : null
  });
}

export async function getPlayTonightRecommendations(sessionMinutes = 60, limit = 24) {
  return invokeCommand("get_play_tonight_recommendations", {
    sessionMinutes: Number(sessionMinutes || 60),
    limit: Number(limit || 24)
  });
}

export async function getCouchModeStatus() {
  return invokeCommand("get_couch_mode_status");
}

export async function setCouchModeState(enabled, controllerKind = null, radialOpen = null) {
  return invokeCommand("set_couch_mode_state", {
    enabled: Boolean(enabled),
    controllerKind: controllerKind ? String(controllerKind) : null,
    radialOpen: radialOpen === null ? null : Boolean(radialOpen)
  });
}

export async function triggerCouchRadialAction(slotId, action = null) {
  const hasExplicitAction = typeof action === "string" && action.trim().length > 0;
  const resolvedAction = hasExplicitAction
    ? action.trim()
    : (typeof slotId === "string" ? slotId.trim() : "");
  const numericSlot = Number.isFinite(Number(slotId)) ? Number(slotId) : null;
  return invokeCommand("trigger_couch_radial_action", {
    actionId: resolvedAction,
    slot: numericSlot
  });
}

export async function calibrateControllerAxes(mode = "auto_center", deadzone = null) {
  return invokeCommand("calibrate_controller_axes", {
    mode: String(mode || "auto_center"),
    deadzone: typeof deadzone === "number" && Number.isFinite(deadzone) ? deadzone : null
  });
}

export async function exportGameDkpack(gameId, outPath = null) {
  return invokeCommand("export_game_dkpack", {
    gameId: String(gameId),
    outPath: outPath ? String(outPath) : null
  });
}

export async function inspectDkpackFile(path) {
  return invokeCommand("inspect_dkpack_file", {
    path: String(path || "")
  });
}

export async function applyDkpackRecipe(dkpackPath, gameId = null) {
  return invokeCommand("apply_dkpack_recipe", {
    dkpackPath: String(dkpackPath || ""),
    gameId: gameId ? String(gameId) : null
  });
}

export async function pickDkpackFile() {
  if (window.__TAURI__?.dialog?.open) {
    const selected = await window.__TAURI__.dialog.open({
      multiple: false,
      directory: false,
      title: "Select Deterministic Modpack Recipe (.dkpack)",
      filters: [
        { name: "Duke Modpack Recipes", extensions: ["dkpack"] },
        { name: "All Files", extensions: ["*"] }
      ]
    });
    return typeof selected === "string" ? selected : null;
  }
  return null;
}

export async function exportGameCapsule(
  gameId,
  outputDir = null,
  format = "folder",
  includeSaves = true
) {
  return invokeCommand("export_game_capsule", {
    gameId: String(gameId),
    outputDir: outputDir ? String(outputDir) : null,
    format: String(format || "folder"),
    includeSaves: Boolean(includeSaves)
  });
}

export async function inspectGameCapsule(archivePath) {
  return invokeCommand("inspect_game_capsule", {
    archivePath: String(archivePath || "")
  });
}

export function listenCouchStateChanged(callback) {
  return listenEvent("couch-mode-toggled", callback);
}

export function listenCouchRadialToggled(callback) {
  return listenEvent("couch-radial-updated", callback);
}

export function listenCouchRadialAction(callback) {
  return listenEvent("couch-radial-action", callback);
}

export function listenDkpackApplied(callback) {
  return listenEvent("dkpack-applied", callback);
}

export function listenPilotSemanticSearch(callback) {
  return listenEvent("duke-pilot-semantic-search", callback);
}

export function listenPilotPlayTonight(callback) {
  return listenEvent("duke-pilot-open-play-tonight", callback);
}

// ---------- Phase 2: AppContainer Jail, Privacy Vault, Fix Finder & Shield v2 ----------

export async function getGameJailStatus(gameId) {
  return invokeCommand("get_game_jail_status", { gameId: String(gameId || "") });
}

export async function setGameJailPolicy(gameId, jailEnabled, offlineMode) {
  return invokeCommand("set_game_jail_policy", {
    gameId: String(gameId || ""),
    jailEnabled: Boolean(jailEnabled),
    offlineMode: Boolean(offlineMode)
  });
}

export async function cleanupGameJailProfile(gameId) {
  return invokeCommand("cleanup_game_jail_profile", { gameId: String(gameId || "") });
}

export async function getPrivacyVaultStatus() {
  return invokeCommand("get_privacy_vault_status");
}

export async function setVaultApiKey(slot, apiKey, username = null) {
  return invokeCommand("set_vault_api_key", {
    slot: String(slot || ""),
    apiKey: String(apiKey || ""),
    username: username ? String(username) : null
  });
}

export async function applyAnonymityNetworkPreset(dohProvider, proxyPreset, customProxyUrl = null) {
  return invokeCommand("apply_anonymity_network_preset", {
    dohProvider: String(dohProvider || "auto"),
    proxyPreset: String(proxyPreset || "direct"),
    customProxyUrl: customProxyUrl ? String(customProxyUrl) : null
  });
}

export async function analyzeGameCrashOrPreflight(gameId, exitCode = null, elapsedMs = null) {
  return invokeCommand("analyze_game_crash_or_preflight", {
    gameId: String(gameId || ""),
    exitCode: typeof exitCode === "number" ? exitCode : null,
    elapsedMs: typeof elapsedMs === "number" ? elapsedMs : null
  });
}

export async function applyOneClickGameFixes(gameId, request) {
  return invokeCommand("apply_one_click_game_fixes", {
    gameId: String(gameId || ""),
    request: request || {}
  });
}

export async function fetchPcgwFixes(gameId, title, appId = null, forceRefresh = false) {
  return invokeCommand("fetch_pcgamingwiki_fixes", {
    gameId: String(gameId || ""),
    title: String(title || ""),
    appId: typeof appId === "number" && appId > 0 ? appId : null,
    forceRefresh: Boolean(forceRefresh)
  });
}

export async function checkSha256ThreatFeed(sha256) {
  return invokeCommand("check_sha256_threat_feed", { sha256: String(sha256 || "") });
}

export async function unblockMotwPath(path) {
  return invokeCommand("unblock_motw_for_path", { path: String(path || "") });
}

// ---------- Phase 3: BIOS Vault, RetroAchievements, ROM Patcher, Uninstaller, Mod Dropper ----------

export async function getBiosVaultStatus() {
  return invokeCommand("get_bios_vault_status");
}

export async function scanAndInstallBiosSources(sourcePaths) {
  return invokeCommand("scan_and_install_bios_sources", {
    sourcePaths: Array.isArray(sourcePaths) ? sourcePaths.map(String) : []
  });
}

export async function fetchRetroAchievementsProgress(gameId, forceRefresh = false) {
  return invokeCommand("fetch_retro_achievements_progress", {
    gameId: String(gameId || ""),
    forceRefresh: Boolean(forceRefresh)
  });
}

export async function groupMultiDiscLibrary() {
  return invokeCommand("group_multi_disc_library");
}

export async function applyRomPatchAndCreateProfile(
  gameId,
  patchPath,
  expectedSourceCrc32 = null,
  expectedSourceSha1 = null
) {
  return invokeCommand("apply_rom_patch_and_create_profile", {
    gameId: String(gameId || ""),
    patchPath: String(patchPath || ""),
    expectedSourceCrc32: expectedSourceCrc32 ? String(expectedSourceCrc32) : null,
    expectedSourceSha1: expectedSourceSha1 ? String(expectedSourceSha1) : null
  });
}

export async function previewCleanUninstall(gameId) {
  return invokeCommand("preview_clean_uninstall", { gameId: String(gameId || "") });
}

export async function executeCleanUninstall(gameId, keepSavesZstd = true, removeOrphans = true) {
  return invokeCommand("execute_clean_uninstall", {
    gameId: String(gameId || ""),
    keepSavesZstd: Boolean(keepSavesZstd),
    removeOrphans: Boolean(removeOrphans)
  });
}

export async function installModArchiveOrWorkshop(gameId, sourceInput) {
  return invokeCommand("install_mod_archive_or_workshop", {
    gameId: String(gameId || ""),
    sourceInput: String(sourceInput || "")
  });
}

export async function pickBiosOrPatchFile(filters = []) {
  if (window.__TAURI__?.dialog?.open) {
    const selected = await window.__TAURI__.dialog.open({
      multiple: false,
      directory: false,
      filters: Array.isArray(filters) ? filters : []
    });
    return Array.isArray(selected) ? selected[0] || null : selected || null;
  }
  return null;
}

export async function pickSaveFilePath(defaultPath = "ghost.dkghost", filters = []) {
  if (window.__TAURI__?.dialog?.save) {
    const selected = await window.__TAURI__.dialog.save({
      defaultPath: String(defaultPath || "ghost.dkghost"),
      filters: Array.isArray(filters) ? filters : []
    });
    return selected || null;
  }
  return null;
}

// ---------- Phase 4: Duke Verified Matrix + Disk-Space Pressure Autopilot ----------

export async function getDukeVerifiedStatus(gameId) {
  return invokeCommand("get_duke_verified_status", { gameId: String(gameId || "") });
}

export async function recordDukeVerifiedFeedback(payload) {
  return invokeCommand("record_duke_verified_feedback", {
    gameId: String(payload?.gameId || ""),
    bootOk: Boolean(payload?.bootOk ?? true),
    crashCode: payload?.crashCode || null,
    avgFps: typeof payload?.avgFps === "number" ? payload.avgFps : null,
    needsDxvk: typeof payload?.needsDxvk === "boolean" ? payload.needsDxvk : null,
    needs4gbLaa: typeof payload?.needs4gbLaa === "boolean" ? payload.needs4gbLaa : null,
    bestEmulatorCore: payload?.bestEmulatorCore || null
  });
}

export async function evaluateDiskPressureAutopilot(
  thresholdGb = null,
  minUntouchedDays = null,
  forcePreview = false
) {
  return invokeCommand("evaluate_disk_pressure_autopilot", {
    thresholdGb: typeof thresholdGb === "number" ? thresholdGb : null,
    minUntouchedDays: typeof minUntouchedDays === "number" ? minUntouchedDays : null,
    forcePreview: Boolean(forcePreview)
  });
}

export async function executeDiskPressureAutopilot(gameIds, keepLanguages = ["english"]) {
  return invokeCommand("execute_disk_pressure_autopilot", {
    gameIds: Array.isArray(gameIds) ? gameIds.map(String) : [],
    keepLanguages: Array.isArray(keepLanguages) ? keepLanguages.map(String) : ["english"]
  });
}

// ---------- Phase 5: Input Ghost & Anti-Cheat-Safe Memory Trainer / Speedhack ----------

export async function listGameGhosts(gameId) {
  return invokeCommand("list_game_ghosts", { gameId: String(gameId || "") });
}

export async function getGhostStatus(gameId) {
  return invokeCommand("get_ghost_status", { gameId: String(gameId || "") });
}

export async function startGhostRecording(gameId, label = null, syncTimeMachine = true) {
  return invokeCommand("start_ghost_recording", {
    gameId: String(gameId || ""),
    label: label ? String(label) : null,
    syncTimeMachine: Boolean(syncTimeMachine)
  });
}

export async function stopGhostRecording(gameId) {
  return invokeCommand("stop_ghost_recording", { gameId: String(gameId || "") });
}

export async function startGhostPlayback(
  gameId,
  ghostId,
  loopMode = false,
  restoreTimeMachine = true
) {
  return invokeCommand("start_ghost_playback", {
    gameId: String(gameId || ""),
    ghostId: String(ghostId || ""),
    loopMode: Boolean(loopMode),
    restoreTimeMachine: Boolean(restoreTimeMachine)
  });
}

export async function stopGhostPlayback(gameId) {
  return invokeCommand("stop_ghost_playback", { gameId: String(gameId || "") });
}

export async function importGhostFile(gameId, srcPath) {
  return invokeCommand("import_ghost_file", {
    gameId: String(gameId || ""),
    srcPath: String(srcPath || "")
  });
}

export async function exportGhostFile(gameId, ghostId, destPath) {
  return invokeCommand("export_ghost_file", {
    gameId: String(gameId || ""),
    ghostId: String(ghostId || ""),
    destPath: String(destPath || "")
  });
}

export async function deleteGhostRecording(gameId, ghostId) {
  return invokeCommand("delete_ghost_recording", {
    gameId: String(gameId || ""),
    ghostId: String(ghostId || "")
  });
}

export async function getSandboxTrainerStatus(pid = null, exePath = null) {
  return invokeCommand("get_sandbox_trainer_status", {
    pid: typeof pid === "number" && pid > 0 ? pid : null,
    exePath: exePath ? String(exePath) : null
  });
}

export async function sandboxScanMemory(pid, valueType, valueStr, resetScan = true) {
  return invokeCommand("sandbox_scan_memory", {
    pid: Number(pid || 0),
    valueType: String(valueType || "i32"),
    valueStr: String(valueStr ?? ""),
    resetScan: Boolean(resetScan)
  });
}

export async function sandboxWriteMemoryValue(pid, addressHex, valueType, newValue) {
  return invokeCommand("sandbox_write_memory_value", {
    pid: Number(pid || 0),
    addressHex: String(addressHex || ""),
    valueType: String(valueType || "i32"),
    newValue: String(newValue ?? "")
  });
}

export async function sandboxSetSpeedhackScale(pid = null, scale = 1.0) {
  return invokeCommand("sandbox_set_speedhack_scale", {
    pid: typeof pid === "number" && pid > 0 ? pid : null,
    scale: Number(scale || 1.0)
  });
}

export async function getFseShellStatus() {
  return invokeCommand("get_fse_shell_status");
}

export async function setFseGamingHome(enabled, suspendBloat = true) {
  return invokeCommand("set_fse_gaming_home", {
    enabled: Boolean(enabled),
    suspendBloat: Boolean(suspendBloat)
  });
}

export async function suspendWindowsBloatNow() {
  return invokeCommand("suspend_windows_bloat_now");
}

export async function resumeWindowsBloatNow() {
  return invokeCommand("resume_windows_bloat_now");
}

export async function listWasmPlugins() {
  return invokeCommand("list_wasm_plugins");
}

export async function installWasmPlugin(
  filePath,
  customName = null,
  requestedDomains = [],
  requestedHooks = ["metadata_lookup"]
) {
  return invokeCommand("install_wasm_plugin", {
    filePath: String(filePath || ""),
    customName: customName ? String(customName) : null,
    requestedDomains: Array.isArray(requestedDomains) ? requestedDomains : [],
    requestedHooks: Array.isArray(requestedHooks) ? requestedHooks : ["metadata_lookup"]
  });
}

export async function installDemoWasmPlugin() {
  return invokeCommand("install_demo_wasm_plugin");
}

export async function setWasmPluginCapabilities(
  pluginId,
  enabled,
  allowedDomains = [],
  allowedHooks = [],
  allowReadOnlyLibraryTitles = false
) {
  return invokeCommand("set_wasm_plugin_capabilities", {
    pluginId: String(pluginId || ""),
    enabled: Boolean(enabled),
    allowedDomains: Array.isArray(allowedDomains) ? allowedDomains : [],
    allowedHooks: Array.isArray(allowedHooks) ? allowedHooks : [],
    allowReadOnlyLibraryTitles: Boolean(allowReadOnlyLibraryTitles)
  });
}

export async function executeWasmPlugin(pluginId, hook = "metadata_lookup", query = "") {
  return invokeCommand("execute_wasm_plugin", {
    pluginId: String(pluginId || ""),
    hook: hook ? String(hook) : "metadata_lookup",
    query: query != null ? String(query) : ""
  });
}

export async function removeWasmPlugin(pluginId) {
  return invokeCommand("remove_wasm_plugin", {
    pluginId: String(pluginId || "")
  });
}

export async function runChaosStressHarness() {
  return invokeCommand("run_chaos_stress_harness");
}

// =========================================================================
// v2.2.0 Resilience, Native Archive, Discord IPC, Save Watcher & Shrink APIs
// =========================================================================

export async function getSelfHealDiagnostics() {
  return invokeCommand("get_self_heal_diagnostics");
}

export async function resetSelfHealSubsystem(name) {
  return invokeCommand("reset_self_heal_subsystem", { name: String(name || "") });
}

export function listenStartupSelfHealReport(callback) {
  return listenEvent("startup-self-heal-report", callback);
}

export async function scanShrinkableRoms() {
  return invokeCommand("scan_shrinkable_roms");
}

export async function compressRetroRom(gameId = null, deleteOriginals = false) {
  return invokeCommand("compress_retro_rom", {
    gameId: gameId ? String(gameId) : null,
    deleteOriginals: Boolean(deleteOriginals)
  });
}

export async function verifyRomArchive(path) {
  return invokeCommand("verify_rom_archive", { path: String(path || "") });
}

export function listenRomShrinkProgress(callback) {
  return listenEvent("rom-shrink-progress", callback);
}

export async function scanRedistBloat(gameId = null) {
  return invokeCommand("scan_redist_bloat", {
    gameId: gameId ? String(gameId) : null
  });
}

export async function cleanRedistBloat(gameId = null) {
  return invokeCommand("clean_redist_bloat", {
    gameId: gameId ? String(gameId) : null
  });
}

export async function getDiscordPresenceStatus() {
  return invokeCommand("get_discord_presence_status");
}

export async function setDiscordPresenceEnabled(enabled) {
  return invokeCommand("set_discord_presence_enabled", { enabled: Boolean(enabled) });
}

export async function getSaveWatcherStatus() {
  return invokeCommand("get_save_watcher_status");
}

export async function startSaveWatcher(gameId) {
  return invokeCommand("start_save_watcher", { gameId: String(gameId || "") });
}

export async function stopSaveWatcher(gameId = null) {
  return invokeCommand("stop_save_watcher", {
    gameId: gameId ? String(gameId) : null
  });
}

export async function getDesktopChordStatus() {
  return invokeCommand("get_desktop_chord_status");
}

export async function setDesktopChordEnabled(enabled) {
  return invokeCommand("set_desktop_chord_enabled", { enabled: Boolean(enabled) });
}

export async function getNetEngineStats() {
  return invokeCommand("get_net_engine_stats_cmd");
}

export async function getSystemRamMb() {
  return invokeCommand("get_system_ram_mb");
}

export async function getSystemTelemetry() {
  return invokeCommand("get_system_telemetry");
}

export async function checkForAppUpdates(force = true) {
  return invokeCommand("check_for_app_updates", { force: Boolean(force) });
}

export async function getCachedAppUpdate() {
  return invokeCommand("get_cached_app_update");
}

export async function downloadAndApplyAppUpdate(version = null) {
  return invokeCommand("download_and_apply_app_update", {
    version: version ? String(version) : null
  });
}

export function listenAppUpdateAvailable(callback) {
  return listenEvent("app-update-available", callback);
}

export function listenAppUpdateProgress(callback) {
  return listenEvent("app-update-progress", callback);
}

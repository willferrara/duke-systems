import {
  THEME_PRESETS,
  addSearchSite,
  applyCustomArtwork,
  applyWindowBackdrop,
  beamGameFromPeer,
  buildDirectorySearchUrl,
  bulkImportSearchSites,
  clearCompletedDownloads,
  compressGameFolder,
  convertFileSrc,
  createDesktopShortcutWithIcon,
  createGameSnapshot,
  deleteGameSnapshot,
  discoverGameSavePaths,
  enrichCatalogQuery,
  escapeHtml,
  fetchCatalogGames,
  formatBytesJs,
  formatDuration,
  formatLastPlayed,
  formatNewsDate,
  formatPlaytime,
  generateFallbackCoverDataUri,
  getActiveGameSessions,
  getCatalogGameDetails,
  getCatalogMmapStats,
  getImportedSources,
  getLauncherSettings,
  getSearchSites,
  getSidebarIcons,
  getSwarmStatus,
  healGameRuntimeDlls,
  importExistingLibraries,
  importSourceFile,
  importSourceUrl,
  initWindowControls,
  invokeCommand,
  isValidHttpUrl,
  linkLocalGameWithSteamId,
  listGameSnapshots,
  listSaveBackups,
  listenDownloadProgress,
  listenEvent,
  openDirectoryBrowser,
  openExternalUrl,
  parseSteamBbcodeToSafeHtml,
  pickAnyFileForScan,
  pickDownloadFolder,
  pickExecutableFile,
  pickJsonSourceFile,
  pollGamepadState,
  preflightCheckExe,
  probeSwarmPeers,
  queryCatalogMmap,
  readClipboardText,
  refreshAllImportedSources,
  refreshImportedSource,
  removeImportedSource,
  removeSearchSite,
  resetDefaultSearchSites,
  restoreGameSave,
  restoreGameSnapshot,
  searchImportedSources,
  searchSteamGridDbArtwork,
  setGameLaunchArgs,
  setGameLaunchProfile,
  suspendOrResumeGame,
  syncGameSavesMesh,
  syncLudusaviManifest,
  takePendingProtocolUrls,
  toggleImportedSource,
  updateLauncherSettings,
  writeClipboardText
} from "./api.js";

const BOOKMARKLET_HREF =
  "javascript:location.href='duke://download?url='+encodeURIComponent(location.href)";
const SUPPORTED_CLIPBOARD_HOSTS = [
  "buzzheavier.com",
  "bzzhr.to",
  "gofile.io",
  "pixeldrain.com",
  "mediafire.com",
  "fileditch.com",
  "fuckingfast.co",
  "datanodes.to"
];
const SUPPORTED_ARCHIVE_EXT_RE = /\.(zip|rar|7z|exe|msi|iso)$/i;

const DEFAULT_CATALOG_FILTERS = Object.freeze({
  searchQuery: "",
  sortBy: "value_score",
  priceTier: "all",
  minRatingPct: 0,
  minReviews: 0,
  playerMode: "all",
  paidOnly: false,
  singleplayer: false,
  coop: false,
  excludePvp: false,
  hideInstalled: false,
  minPriceCents: 0,
  minCcu: 0,
  selectedGenres: [],
  tagSearch: "",
  genresDrawerOpen: false
});

const state = {
  activeTab: "library",
  library: [],
  downloadJobs: [],
  reports: [],
  filter: "all",
  sourceFilter: "all",
  selectedSourceId: null,
  sourceGameQuery: "",
  directoryCategory: "all",
  directoryQuery: "",
  searchSites: [],
  jsonFeedsExpanded: false,
  importedSources: [],
  sourceSearchResponse: {
    items: [],
    totalMatches: 0,
    enabledSourceCount: 0,
    totalSourceCount: 0
  },
  activeSessions: [],
  sortBy: "recent",
  searchQuery: "",
  selectedDetailId: null,
  activeDetailRequestId: 0,
  modalGameId: null,
  autoCleanCompleted: true,
  autoDetectClipboard: true,
  nvmePrefetchEnabled: true,
  swarmEnabled: false,
  swarmNodeName: "",
  swarmPeers: [],
  clipboardBannerUrl: "",
  seenClipboardUrls: new Set(),
  clipboardFocusTimer: null,
  steamgriddbApiKey: "",
  suspendHotkey: "Ctrl+Alt+P",
  couchMode: false,
  sgdbResults: null,
  sgdbSelection: {
    coverUrl: null,
    heroUrl: null,
    iconUrl: null
  },
  restoreSaveState: {
    gameId: null,
    snapshots: []
  },
  snapshotModalState: {
    gameId: null,
    gameTitle: "",
    snapshots: []
  },
  preflightModalState: {
    game: null,
    report: null,
    manualInspect: false
  },
  notifiedCompletedIds: new Set(),
  sidebarIcons: Object.create(null),
  sidebarIconsPending: new Set(),
  sidebarIconsAttempted: new Set(),
  sidebarIconVersion: 0,
  customSaveDir: localStorage.getItem("duke_launcher_dl_dir") || "",
  activeTheme: localStorage.getItem("duke_launcher_theme") || "dark",
  catalog: {
    itemsById: new Map(),
    renderedCount: 0,
    totalMatches: 0,
    batchSize: 60,
    loaded: false,
    loading: false,
    loadingMore: false,
    syncing: false,
    syncTarget: 30000,
    enrichingLive: false,
    lastEnrichedQuery: "",
    error: null,
    fromCache: false,
    staleWarning: null,
    totalCached: 0,
    fetchedAt: 0,
    lastQueryTimeUs: 0,
    requestId: 0,
    queryRequestId: 0,
    debounceTimer: null,
    modalGame: null,
    modalRequestId: 0,
    observer: null,
    ...DEFAULT_CATALOG_FILTERS,
    selectedGenres: []
  }
};

function el(id) {
  return document.getElementById(id);
}

function svgIcon(name) {
  return `<svg class="icon" aria-hidden="true"><use href="#${name}"/></svg>`;
}

function showToast(msg, tone = "info", durationMs = 3800) {
  const c = el("toast-container");
  if (!c) return;
  const t = document.createElement("div");
  t.className = `toast ${tone}`;
  t.textContent = msg;
  c.appendChild(t);
  setTimeout(() => t.remove(), durationMs);
}

function applyTheme(themeId) {
  const valid = THEME_PRESETS.some((t) => t.id === themeId) ? themeId : "dark";
  state.activeTheme = valid;
  document.body.setAttribute("data-theme", valid);
  localStorage.setItem("duke_launcher_theme", valid);
  applyWindowBackdrop(valid).catch(() => {});
}

function renderThemes() {
  const grid = el("theme-grid");
  if (!grid) return;
  grid.innerHTML = THEME_PRESETS.map(
    (t) => `
      <button type="button" class="theme-option-card ${t.id === state.activeTheme ? "active" : ""}" data-theme-id="${escapeHtml(t.id)}">
        <div class="theme-preview" data-preview-theme="${escapeHtml(t.id)}"></div>
        <div class="theme-meta">
          <div class="theme-name">${escapeHtml(t.name)}</div>
          <div class="theme-desc">${escapeHtml(t.desc)}</div>
        </div>
      </button>
    `
  ).join("");
}

function switchTab(tab, keepDetail = false) {
  if (!keepDetail) state.selectedDetailId = null;
  state.activeTab = tab;

  document.querySelectorAll(".sidebar-nav .nav-item").forEach((btn) => {
    btn.classList.toggle("active", btn.dataset.tab === tab);
  });

  const viewMap = {
    library: "library-view",
    catalog: "catalog-view",
    downloader: "downloader-view",
    sources: "sources-view",
    settings: "settings-view"
  };

  Object.values(viewMap).forEach((id) => el(id)?.classList.add("hidden"));
  el("game-detail-view")?.classList.add("hidden");

  if (tab === "library" && state.selectedDetailId) {
    el("game-detail-view")?.classList.remove("hidden");
    el("library-controls")?.classList.add("hidden");
  } else {
    el(viewMap[tab] || "library-view")?.classList.remove("hidden");
    el("library-controls")?.classList.toggle("hidden", tab !== "library");
  }
}

function updateNavCatalogBadge() {
  const badge = el("nav-catalog-count");
  if (!badge) return;
  const count = Number(state.catalog.totalCached || 0);
  if (count > 0) {
    badge.textContent = count.toLocaleString();
  }
}

function updateSidebarTelemetry(telemetry) {
  const totalSavedCents = state.library.reduce((acc, g) => acc + Number(g.steamPriceCents || 0), 0);
  const totalBytes = state.library.reduce((acc, g) => acc + Number(g.sizeBytes || 0), 0);

  if (el("nav-library-count")) el("nav-library-count").textContent = String(state.library.length);
  if (el("sidebar-list-count")) el("sidebar-list-count").textContent = String(state.library.length);
  if (el("stat-installed-count")) el("stat-installed-count").textContent = String(state.library.length);
  if (el("stat-total-size")) el("stat-total-size").textContent = formatBytesJs(totalBytes);
  if (el("stat-money-saved")) {
    el("stat-money-saved").textContent = `$${(totalSavedCents / 100).toFixed(2)}`;
  }

  const activeDls = state.downloadJobs.filter((j) =>
    ["DOWNLOADING", "RESOLVING", "INSTALLING", "QUEUED", "SCANNING", "PAUSED"].includes(
      String(j.status || "").toUpperCase()
    )
  ).length;
  if (el("nav-dl-count")) el("nav-dl-count").textContent = String(activeDls);
  updateNavCatalogBadge();

  if (telemetry) {
    if (el("sidebar-doh-badge")) {
      el("sidebar-doh-badge").textContent = telemetry.dohShield === "ACTIVE" ? "Active" : "Standby";
    }
    if (el("sidebar-ram-badge")) {
      const mb = Number(telemetry.workingSetMb || 0).toFixed(1);
      const statusSuffix = telemetry.status === "GAME BOOST" ? " · Eco" : "";
      el("sidebar-ram-badge").textContent = `${mb} MB${statusSuffix}`;
    }
  }
}

function resolveGameSourceLabel(game) {
  const explicit = String(game?.source || "").trim();
  if (explicit) return explicit;
  if (game?.steamAppId || game?.appId) return "Steam";
  return "Portable";
}

function getFilteredSortedGames() {
  const q = state.searchQuery.trim().toLowerCase();
  const list = state.library.filter((g) => {
    const srcLabel = resolveGameSourceLabel(g).toLowerCase();
    const isSteamDetected = srcLabel === "steam" || Boolean(g.steamAppId || g.appId);
    const isStoreTagged = ["steam", "epic", "gog", "xbox"].includes(srcLabel);
    const isExplicitPortable =
      srcLabel === "portable" || String(g.genre || "").toLowerCase().includes("portable");

    if (state.filter === "favorites" && !g.isFavorite) return false;
    if (state.filter === "steam" && !isSteamDetected) return false;
    if (state.filter === "portable" && (isStoreTagged && !isExplicitPortable)) return false;
    if (state.filter === "portable" && !isExplicitPortable && isSteamDetected) return false;
    if (state.filter === "epic" && srcLabel !== "epic") return false;
    if (state.filter === "gog" && srcLabel !== "gog") return false;
    if (state.filter === "xbox" && srcLabel !== "xbox") return false;

    if (q) {
      const hay = `${g.title} ${g.genre} ${srcLabel} ${g.exePath}`.toLowerCase();
      if (!hay.includes(q)) return false;
    }
    return true;
  });

  list.sort((a, b) => {
    if (a.isFavorite !== b.isFavorite && state.sortBy === "recent") {
      return a.isFavorite ? -1 : 1;
    }
    switch (state.sortBy) {
      case "alpha":
        return String(a.title || "").localeCompare(String(b.title || ""));
      case "size":
        return Number(b.sizeBytes || 0) - Number(a.sizeBytes || 0);
      case "playtime":
        return Number(b.playtimeSeconds || 0) - Number(a.playtimeSeconds || 0);
      case "price":
        return Number(b.steamPriceCents || 0) - Number(a.steamPriceCents || 0);
      default:
        return Number(b.lastPlayedTs || b.addedAt || 0) - Number(a.lastPlayedTs || a.addedAt || 0);
    }
  });
  return list;
}

function getGameMonogram(title) {
  const match = String(title || "").trim().match(/[A-Za-z0-9]/);
  return match ? match[0].toUpperCase() : "G";
}

function createSidebarMonogramEl(title) {
  const span = document.createElement("span");
  span.className = "sidebar-game-monogram";
  span.setAttribute("aria-hidden", "true");
  span.textContent = getGameMonogram(title);
  return span;
}

function createSidebarIconImgEl(srcUrl) {
  const img = document.createElement("img");
  img.src = srcUrl;
  img.alt = "";
  img.className = "sidebar-game-icon";
  img.loading = "lazy";
  img.decoding = "async";
  return img;
}

function bindSidebarIconErrorHandler(img, gameId, gameTitle) {
  img.addEventListener(
    "error",
    () => {
      delete state.sidebarIcons[gameId];
      img.replaceWith(createSidebarMonogramEl(gameTitle));
    },
    { once: true }
  );
}

function invalidateSidebarIcons(gameIds = null) {
  state.sidebarIconVersion += 1;
  if (Array.isArray(gameIds)) {
    for (const id of gameIds) {
      if (!id) continue;
      delete state.sidebarIcons[id];
      state.sidebarIconsAttempted.delete(id);
    }
  } else {
    state.sidebarIcons = Object.create(null);
    state.sidebarIconsAttempted.clear();
  }
}

function scheduleSidebarIconsSync(gameIds = null) {
  const sourceIds = Array.isArray(gameIds)
    ? gameIds
    : state.library.map((g) => g.id).filter(Boolean);
  const missing = [];
  for (const id of sourceIds) {
    if (
      !id ||
      state.sidebarIcons[id] ||
      state.sidebarIconsPending.has(id) ||
      state.sidebarIconsAttempted.has(id)
    ) {
      continue;
    }
    state.sidebarIconsPending.add(id);
    missing.push(id);
  }
  if (missing.length === 0) return;

  getSidebarIcons(missing)
    .then((resolved) => {
      const container = el("sidebar-game-list");
      const versionSuffix = state.sidebarIconVersion > 0 ? `?v=${state.sidebarIconVersion}` : "";
      for (const id of missing) {
        state.sidebarIconsPending.delete(id);
        state.sidebarIconsAttempted.add(id);
        const rawPath = resolved && typeof resolved === "object" ? resolved[id] : null;
        if (!rawPath) continue;
        const assetUrl = convertFileSrc(rawPath);
        if (!assetUrl) continue;
        const finalUrl = versionSuffix ? `${assetUrl}${versionSuffix}` : assetUrl;
        state.sidebarIcons[id] = finalUrl;

        if (container) {
          const row = Array.from(container.querySelectorAll(".sidebar-game-item")).find(
            (btn) => btn.dataset.gameId === id
          );
          if (row) {
            const game = state.library.find((g) => g.id === id);
            const title = game?.title || "";
            const slot = row.querySelector(".sidebar-game-icon, .sidebar-game-monogram");
            const img = createSidebarIconImgEl(finalUrl);
            bindSidebarIconErrorHandler(img, id, title);
            if (slot) {
              slot.replaceWith(img);
            } else {
              row.prepend(img);
            }
          }
        }
      }
    })
    .catch(() => {
      for (const id of missing) {
        state.sidebarIconsPending.delete(id);
      }
    });
}

function renderSidebarGameList() {
  const container = el("sidebar-game-list");
  if (!container) return;
  const sorted = [...state.library].sort((a, b) => String(a.title).localeCompare(String(b.title)));
  container.innerHTML = sorted
    .map((g) => {
      const iconUrl = state.sidebarIcons[g.id];
      const iconHtml = iconUrl
        ? `<img src="${escapeHtml(iconUrl)}" alt="" class="sidebar-game-icon" loading="lazy" decoding="async" />`
        : `<span class="sidebar-game-monogram" aria-hidden="true">${escapeHtml(getGameMonogram(g.title))}</span>`;
      return `
      <button type="button" class="sidebar-game-item ${state.selectedDetailId === g.id ? "active" : ""}" data-game-id="${escapeHtml(g.id)}">
        ${iconHtml}
        <span class="sidebar-game-dot"></span>
        <span class="sidebar-game-title">${escapeHtml(g.title)}</span>
        ${g.isFavorite ? `<svg class="icon sidebar-fav-pin" aria-hidden="true"><use href="#icon-pin"/></svg>` : ""}
      </button>
    `;
    })
    .join("");

  container.querySelectorAll("img.sidebar-game-icon").forEach((img) => {
    const row = img.closest(".sidebar-game-item");
    const gid = row?.dataset.gameId || "";
    const game = state.library.find((g) => g.id === gid);
    bindSidebarIconErrorHandler(img, gid, game?.title || "");
  });

  scheduleSidebarIconsSync(sorted.map((g) => g.id));
}

function renderLibraryGrid() {
  const grid = el("library-grid");
  const empty = el("library-empty");
  if (!grid || !empty) return;

  const games = getFilteredSortedGames();
  empty.classList.toggle("hidden", state.library.length > 0);

  if (games.length === 0 && state.library.length > 0) {
    grid.innerHTML = `<div class="empty-card"><h3>No matching games</h3><p>Adjust your search filter to view your installed titles.</p></div>`;
    return;
  }

  grid.innerHTML = games
    .map((game) => {
      const cover = game.coverUrl || generateFallbackCoverDataUri(game.title, game.genre || "Installed");
      const playtimeStr = formatPlaytime(game.playtimeSeconds || 0);
      const lastStr = formatLastPlayed(game.lastPlayedTs || game.lastPlayed);
      const favClass = game.isFavorite ? "is-fav" : "";
      const sourceLabel = resolveGameSourceLabel(game);

      return `
        <article class="game-card" data-game-id="${escapeHtml(game.id)}">
          <div class="game-card-media">
            <img src="${escapeHtml(cover)}" alt="${escapeHtml(game.title)}" class="game-card-img" loading="lazy" />
            <span class="game-source-badge">${escapeHtml(sourceLabel)}</span>
            <div class="game-card-badges">
              <span class="card-badge mono-num">${escapeHtml(game.sizeFormatted || formatBytesJs(game.sizeBytes))}</span>
              <button type="button" class="card-fav-btn ${favClass}" data-action="fav" data-game-id="${escapeHtml(game.id)}" title="Toggle Favorite">
                ${svgIcon("icon-pin")}
              </button>
            </div>
            <div class="game-card-overlay">
              <div class="game-card-toolbar">
                <button type="button" class="btn btn-steam-green btn-sm card-play-btn" data-action="play" data-game-id="${escapeHtml(game.id)}">
                  ${svgIcon("icon-play")}
                  <span>Play</span>
                </button>
                <button type="button" class="btn btn-secondary btn-sm" data-action="details" data-game-id="${escapeHtml(game.id)}" title="Details">
                  ${svgIcon("icon-sliders")}
                  <span>Details</span>
                </button>
                <button type="button" class="btn btn-secondary btn-sm" data-action="folder" data-game-id="${escapeHtml(game.id)}" title="Open Folder">
                  ${svgIcon("icon-folder-open")}
                </button>
              </div>
            </div>
          </div>
          <div class="game-card-body">
            <div class="game-card-title" title="${escapeHtml(game.title)}">${escapeHtml(game.title)}</div>
            <div class="game-card-meta">
              <span class="mono-num">${escapeHtml(playtimeStr)}</span>
              <span>${escapeHtml(lastStr)}</span>
            </div>
          </div>
        </article>
      `;
    })
    .join("");
}

function buildHeroCandidateList(appId, fallbackUrl) {
  const candidates = [];
  const validId = Number(appId || 0);
  if (validId > 0) {
    candidates.push(
      `https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/${validId}/library_hero.jpg`,
      `https://cdn.cloudflare.steamstatic.com/steam/apps/${validId}/library_hero.jpg`,
      `https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/${validId}/header.jpg`
    );
  }
  if (fallbackUrl && typeof fallbackUrl === "string" && fallbackUrl.trim()) {
    candidates.push(fallbackUrl.trim());
  }
  return candidates.filter((url, idx, arr) => url && arr.indexOf(url) === idx);
}

function applyHeroCandidateChain(imgEl, rawCandidates, isStillValid) {
  if (!imgEl) return;
  const candidates = rawCandidates.filter(
    (url, idx, arr) => typeof url === "string" && url.trim() && arr.indexOf(url) === idx
  );
  imgEl.onerror = null;
  imgEl.onload = null;
  imgEl.alt = "";

  if (candidates.length === 0) {
    imgEl.removeAttribute("src");
    imgEl.classList.add("hidden");
    return;
  }

  let index = 0;
  const tryNext = () => {
    if (!isStillValid()) return;
    if (index >= candidates.length) {
      imgEl.onerror = null;
      imgEl.onload = null;
      imgEl.removeAttribute("src");
      imgEl.classList.add("hidden");
      return;
    }
    const nextUrl = candidates[index++];
    imgEl.onload = () => {
      if (!isStillValid()) return;
      imgEl.classList.remove("hidden");
    };
    imgEl.onerror = () => {
      if (!isStillValid()) return;
      tryNext();
    };
    imgEl.src = nextUrl;
  };

  tryNext();
}

function syncActiveSessionsUi() {
  const sessions = Array.isArray(state.activeSessions) ? state.activeSessions : [];
  const selectedSession = sessions.find((s) => s.gameId === state.selectedDetailId) || null;
  const suspendBtn = el("btn-detail-suspend");
  if (suspendBtn) {
    suspendBtn.classList.toggle("hidden", !selectedSession);
    if (selectedSession) {
      const paused = Boolean(selectedSession.isSuspended);
      suspendBtn.classList.toggle("is-suspended", paused);
      suspendBtn.innerHTML = `${svgIcon(paused ? "icon-play" : "icon-pause")}<span>${paused ? "Resume" : "Suspend"}</span>`;
    }
  }

  const primarySession = selectedSession || sessions[0] || null;
  const titlebarPill = el("titlebar-suspend-pill");
  if (titlebarPill) {
    titlebarPill.classList.toggle("hidden", !primarySession);
    if (primarySession) {
      const paused = Boolean(primarySession.isSuspended);
      titlebarPill.classList.toggle("is-suspended", paused);
      titlebarPill.textContent = paused
        ? `Paused: ${primarySession.title} (Resume)`
        : `Running: ${primarySession.title} (Suspend)`;
      titlebarPill.dataset.gameId = primarySession.gameId || "";
    }
  }
}

async function refreshActiveSessions() {
  try {
    const sessions = await getActiveGameSessions();
    state.activeSessions = Array.isArray(sessions) ? sessions : [];
    syncActiveSessionsUi();
  } catch {
    // Ignore transient polling failures
  }
}

function renderDetailSavePaths(report) {
  const container = el("detail-save-paths-list");
  if (!container) return;

  const existing = Array.isArray(report?.existingPaths) ? report.existingPaths : [];
  const candidates = Array.isArray(report?.candidatePaths) ? report.candidatePaths : [];
  const allRows = existing.length > 0 ? existing : candidates;

  if (allRows.length === 0) {
    container.innerHTML = `<div class="panel-sub">No save folders detected yet. Sync Ludusavi or launch the game once, then rescan.</div>`;
    return;
  }

  const headerNote = report?.matchedManifestTitle
    ? `<div class="panel-sub">Ludusavi Match: <strong>${escapeHtml(report.matchedManifestTitle)}</strong> (${existing.length} active folder${existing.length === 1 ? "" : "s"})</div>`
    : `<div class="panel-sub">Heuristic Scan (${existing.length} active folder${existing.length === 1 ? "" : "s"})</div>`;

  container.innerHTML =
    headerNote +
    allRows
      .slice(0, 8)
      .map(
        (item) => `
        <div class="save-path-row">
          <div class="save-path-meta">
            <span class="save-path-text">${escapeHtml(item.path)}</span>
            <span class="save-path-source">${escapeHtml(item.source || "Discovered")}${item.exists ? " · Found on disk" : " · Expected path"}</span>
          </div>
          ${
            item.exists
              ? `<button type="button" class="btn btn-secondary btn-sm" data-open-save-path="${escapeHtml(item.path)}">${svgIcon("icon-folder-open")}<span>Open</span></button>`
              : ""
          }
        </div>
      `
      )
      .join("");
}

async function loadDetailSavePaths(game, requestId, syncIfMissing = false) {
  const container = el("detail-save-paths-list");
  if (!game || !container) return;
  container.innerHTML = `<div class="panel-sub">Scanning Ludusavi manifest & local save heuristics...</div>`;
  try {
    const report = await discoverGameSavePaths(game.id, syncIfMissing);
    if (requestId !== state.activeDetailRequestId || state.selectedDetailId !== game.id) return;
    renderDetailSavePaths(report);
  } catch (err) {
    if (requestId !== state.activeDetailRequestId || state.selectedDetailId !== game.id) return;
    container.innerHTML = `<div class="panel-sub">${escapeHtml(String(err || "Unable to discover save paths."))}</div>`;
  }
}

async function openGameDetail(gameId) {
  const game = state.library.find((g) => g.id === gameId);
  if (!game) return;

  state.activeDetailRequestId += 1;
  const requestId = state.activeDetailRequestId;
  state.selectedDetailId = game.id;
  switchTab("library", true);
  renderSidebarGameList();
  syncActiveSessionsUi();

  const detailHeroImg = el("detail-hero-img");
  const cachedCover = game.heroUrl || game.coverDataUrl || game.coverUrl || "";
  if (detailHeroImg) {
    detailHeroImg.onerror = null;
    detailHeroImg.onload = null;
    detailHeroImg.alt = "";
    if (cachedCover) {
      detailHeroImg.onerror = () => {
        if (requestId !== state.activeDetailRequestId) return;
        detailHeroImg.onerror = null;
        detailHeroImg.removeAttribute("src");
        detailHeroImg.classList.add("hidden");
      };
      detailHeroImg.src = cachedCover;
      detailHeroImg.classList.remove("hidden");
    } else {
      detailHeroImg.removeAttribute("src");
      detailHeroImg.classList.add("hidden");
    }
  }

  if (el("detail-hero-title")) el("detail-hero-title").textContent = game.title;
  if (el("detail-hero-source")) el("detail-hero-source").textContent = resolveGameSourceLabel(game);

  const appIdBadge = el("detail-hero-appid");
  const initialAppId = game.steamAppId || game.appId;
  if (appIdBadge) {
    appIdBadge.textContent = initialAppId ? `AppID ${initialAppId}` : "";
    appIdBadge.classList.toggle("hidden", !initialAppId);
  }

  if (el("detail-last-played")) el("detail-last-played").textContent = formatLastPlayed(game.lastPlayedTs || game.lastPlayed);
  if (el("detail-playtime")) el("detail-playtime").textContent = formatPlaytime(game.playtimeSeconds || 0);
  if (el("detail-live-players")) el("detail-live-players").textContent = "Checking...";
  if (el("detail-steam-price")) el("detail-steam-price").textContent = game.steamPriceFormatted || "--";
  if (el("detail-install-size")) el("detail-install-size").textContent = game.sizeFormatted || formatBytesJs(game.sizeBytes);
  if (el("detail-exe-path")) el("detail-exe-path").textContent = game.exePath || "--";
  if (el("detail-launch-args")) el("detail-launch-args").value = game.launchArgs || "";
  if (el("detail-toggle-sandbox")) {
    el("detail-toggle-sandbox").checked = Boolean(game.sandboxEnabled);
  }
  if (el("detail-prefetch-mode")) {
    el("detail-prefetch-mode").value =
      typeof game.nvmePrefetchEnabled === "boolean"
        ? game.nvmePrefetchEnabled
          ? "enabled"
          : "disabled"
        : "inherit";
  }
  if (el("detail-description")) el("detail-description").textContent = "Querying Steam store metadata over DoH...";
  if (el("detail-news-list")) el("detail-news-list").innerHTML = `<div class="panel-sub">Loading recent updates...</div>`;

  loadDetailSavePaths(game, requestId, false);
  refreshActiveSessions();

  const pinBtn = el("btn-detail-pin");
  if (pinBtn) {
    pinBtn.querySelector("span").textContent = game.isFavorite ? "Favorited" : "Favorite";
  }

  const isStillCurrent = () =>
    requestId === state.activeDetailRequestId && state.selectedDetailId === game.id;

  try {
    const details = await invokeCommand("get_game_steam_details", {
      appId: initialAppId || null,
      title: game.title,
      installDir: game.installDir || null,
      gameId: game.id
    });
    if (!isStillCurrent()) return;

    const resolvedAppId = details?.appId || initialAppId;
    if (appIdBadge) {
      appIdBadge.textContent = resolvedAppId ? `AppID ${resolvedAppId}` : "";
      appIdBadge.classList.toggle("hidden", !resolvedAppId);
    }

    const baseList = buildHeroCandidateList(resolvedAppId, game.coverUrl || game.coverDataUrl || "");
    const steamCandidates =
      Array.isArray(details?.heroCandidates) && details.heroCandidates.length > 0
        ? details.heroCandidates
        : baseList;
    const candidates = [
      ...(game.heroUrl ? [game.heroUrl] : []),
      ...steamCandidates,
      ...(game.coverUrl ? [game.coverUrl] : [])
    ].filter((u, idx, arr) => u && arr.indexOf(u) === idx);

    applyHeroCandidateChain(detailHeroImg, candidates, isStillCurrent);

    if (el("detail-description")) {
      el("detail-description").textContent = details.shortDescription || "Local game executable managed by Duke Launcher.";
    }
    if (el("detail-developers")) el("detail-developers").textContent = (details.developers || []).join(", ") || "--";
    if (el("detail-publishers")) el("detail-publishers").textContent = (details.publishers || []).join(", ") || "--";
    if (el("detail-release-date")) el("detail-release-date").textContent = details.releaseDate || "--";
    if (el("detail-genres")) el("detail-genres").textContent = (details.genres || []).join(", ") || game.genre || "--";
    if (el("detail-live-players")) {
      el("detail-live-players").textContent =
        details.livePlayers != null ? Number(details.livePlayers).toLocaleString() : "Offline";
    }
    if (el("detail-steam-price")) {
      el("detail-steam-price").textContent = details.steamPriceFormatted || details.priceFinal || "Free";
    }

    const newsList = el("detail-news-list");
    if (newsList) {
      const news = details.news || [];
      if (news.length === 0) {
        newsList.innerHTML = `<div class="panel-sub">No recent Steam patch notes found for this title.</div>`;
      } else {
        newsList.innerHTML = news
          .slice(0, 6)
          .map(
            (n) => `
            <div class="news-item">
              <div class="news-item-head">
                <span>${escapeHtml(n.feedlabel || "Update")}</span>
                <span class="mono-num">${escapeHtml(formatNewsDate(n.date))}</span>
              </div>
              <div class="news-item-title">${escapeHtml(n.title)}</div>
              <div class="news-item-body">${parseSteamBbcodeToSafeHtml(n.contents || "")}</div>
            </div>
          `
          )
          .join("");
      }
    }
  } catch {
    if (!isStillCurrent()) return;
    applyHeroCandidateChain(detailHeroImg, buildHeroCandidateList(initialAppId, cachedCover), isStillCurrent);
    if (el("detail-description")) el("detail-description").textContent = "Local Windows executable managed offline.";
    if (el("detail-live-players")) el("detail-live-players").textContent = "Offline";
    if (el("detail-news-list")) el("detail-news-list").innerHTML = `<div class="panel-sub">Offline mode or Steam metadata unavailable.</div>`;
  }
}

const openGameDetailView = openGameDetail;

function buildDownloadJobCardHtml(job) {
  const pct = Math.max(0, Math.min(100, Number(job.progressPct || 0))).toFixed(1);
  const status = String(job.status || "QUEUED");
  const isDownloading = status === "DOWNLOADING" || status === "RESOLVING";
  const isDone = status === "COMPLETED" || status === "READY" || status === "CLEAN";
  const isPaused = status === "PAUSED";
  const isErr = status === "ERROR" || status === "DANGER";
  const barClass = isDone ? "done" : isPaused ? "paused" : isErr ? "error" : "";

  return `
    <div class="dl-job-card" data-dl-id="${escapeHtml(job.downloadId)}">
      <div class="dl-job-top">
        <span class="dl-job-title">${escapeHtml(job.fileName || job.originalUrl)}</span>
        <span class="pill mono-num dl-status-pill">${escapeHtml(status)} · ${pct}%</span>
      </div>
      <div class="progress-track">
        <div class="progress-fill ${barClass}" style="width:${pct}%"></div>
      </div>
      <div class="dl-job-meta mono-num">
        <span class="dl-bytes">${formatBytesJs(job.downloadedBytes)} / ${job.totalBytes ? formatBytesJs(job.totalBytes) : "Unknown"}</span>
        <span class="dl-speed">${Number(job.speedMbps || 0).toFixed(2)} MB/s</span>
        <span class="dl-eta">${job.etaSecs != null ? `ETA ${formatDuration(job.etaSecs)}` : "--"}</span>
        <span class="dl-msg">${escapeHtml(job.message || "")}</span>
      </div>
      <div class="dl-job-actions">
        ${
          isDownloading
            ? `<button type="button" class="btn btn-secondary btn-xs" data-dl-action="pause" data-dl-id="${escapeHtml(job.downloadId)}">${svgIcon("icon-pause")}<span>Pause</span></button>`
            : !isDone
              ? `<button type="button" class="btn btn-primary btn-xs" data-dl-action="resume" data-dl-id="${escapeHtml(job.downloadId)}">${svgIcon("icon-play")}<span>Resume</span></button>`
              : `<button type="button" class="btn btn-steam-green btn-xs" data-dl-action="install" data-dl-id="${escapeHtml(job.downloadId)}">${svgIcon("icon-zap")}<span>Extract &amp; Add</span></button>`
        }
        <button type="button" class="btn btn-secondary btn-xs" data-dl-action="folder" data-dl-id="${escapeHtml(job.downloadId)}">${svgIcon("icon-folder-open")}<span>Folder</span></button>
        <button type="button" class="btn btn-danger-ghost btn-xs" data-dl-action="remove" data-dl-id="${escapeHtml(job.downloadId)}">${svgIcon("icon-trash-2")}<span>Remove</span></button>
      </div>
    </div>
  `;
}

function updateDownloadJobCardInPlace(job) {
  const container = el("dl-jobs-list");
  if (!container) return;
  const existing = container.querySelector(`.dl-job-card[data-dl-id="${CSS.escape(job.downloadId)}"]`);
  if (!existing) {
    renderDownloadJobs();
    return;
  }
  const pct = Math.max(0, Math.min(100, Number(job.progressPct || 0))).toFixed(1);
  const pill = existing.querySelector(".dl-status-pill");
  if (pill) pill.textContent = `${job.status} · ${pct}%`;
  const fill = existing.querySelector(".progress-fill");
  if (fill) {
    fill.style.width = `${pct}%`;
    fill.className = `progress-fill ${
      job.status === "COMPLETED" || job.status === "READY"
        ? "done"
        : job.status === "PAUSED"
          ? "paused"
          : job.status === "ERROR"
            ? "error"
            : ""
    }`;
  }
  const bytesEl = existing.querySelector(".dl-bytes");
  if (bytesEl) bytesEl.textContent = `${formatBytesJs(job.downloadedBytes)} / ${job.totalBytes ? formatBytesJs(job.totalBytes) : "Unknown"}`;
  const speedEl = existing.querySelector(".dl-speed");
  if (speedEl) speedEl.textContent = `${Number(job.speedMbps || 0).toFixed(2)} MB/s`;
  const etaEl = existing.querySelector(".dl-eta");
  if (etaEl) etaEl.textContent = job.etaSecs != null ? `ETA ${formatDuration(job.etaSecs)}` : "--";
  const msgEl = existing.querySelector(".dl-msg");
  if (msgEl) msgEl.textContent = job.message || "";
}

function renderDownloadJobs() {
  const list = el("dl-jobs-list");
  const strip = el("dl-onboarding-strip");
  if (!list) return;
  const q = state.searchQuery.trim().toLowerCase();
  const isQueueEmpty = state.downloadJobs.length === 0;
  if (strip) {
    strip.classList.toggle("hidden", !isQueueEmpty);
  }
  const visible =
    state.activeTab === "downloader" && q
      ? state.downloadJobs.filter((j) => `${j.fileName} ${j.originalUrl}`.toLowerCase().includes(q))
      : state.downloadJobs;

  if (visible.length === 0) {
    list.classList.toggle("hidden", isQueueEmpty);
    list.innerHTML = isQueueEmpty
      ? ""
      : `<div class="panel-sub">No downloads match "${escapeHtml(state.searchQuery.trim())}".</div>`;
    return;
  }
  list.classList.remove("hidden");
  list.innerHTML = visible.map(buildDownloadJobCardHtml).join("");
}

function renderSecurityReports() {
  const list = el("dl-reports-list");
  if (!list) return;
  if (state.reports.length === 0) {
    list.innerHTML = `<div class="panel-sub">No security scan reports recorded this session.</div>`;
    return;
  }
  list.innerHTML = state.reports
    .map((rep) => {
      const danger = String(rep.verdict || "").includes("DANGER");
      const entropyText =
        typeof rep.shannonEntropy === "number"
          ? `${rep.shannonEntropy.toFixed(3)} b/B`
          : "";
      const metaBadges = [
        rep.sizeFormatted ? escapeHtml(rep.sizeFormatted) : "",
        entropyText ? `Entropy: ${escapeHtml(entropyText)}` : "",
        rep.peArchitecture ? escapeHtml(rep.peArchitecture) : ""
      ]
        .filter(Boolean)
        .join(" | ");
      const detailsHtml = Array.isArray(rep.details) && rep.details.length > 0
        ? `<div class="panel-sub" style="font-family: 'Cascadia Code', 'Consolas', monospace; font-variant-numeric: tabular-nums; font-size: 11px;">${rep.details.map((d) => escapeHtml(d)).join("<br>")}</div>`
        : "";
      return `
      <div class="dl-job-card">
        <div class="dl-job-top">
          <span class="dl-job-title">${escapeHtml(rep.fileName)}</span>
          <span class="pill ${danger ? "danger" : "emerald"}">${escapeHtml(rep.verdict)}</span>
        </div>
        ${metaBadges ? `<div class="panel-sub" style="font-family: 'Cascadia Code', 'Consolas', monospace; font-variant-numeric: tabular-nums;">${metaBadges}</div>` : ""}
        <div class="panel-sub">${escapeHtml(rep.summary)}</div>
        ${detailsHtml}
        <div class="dl-job-actions">
          <button type="button" class="btn btn-secondary btn-xs" data-rep-action="folder" data-rep-path="${escapeHtml(rep.filePath)}">${svgIcon("icon-folder-open")}<span>Open Folder</span></button>
          ${danger ? `<button type="button" class="btn btn-danger-ghost btn-xs" data-rep-action="delete" data-rep-path="${escapeHtml(rep.filePath)}">${svgIcon("icon-trash-2")}<span>Delete File</span></button>` : ""}
        </div>
      </div>
    `;
    })
    .join("");
}

function formatSyncTimestamp(ts) {
  const n = Number(ts || 0);
  if (!n) return "Never";
  return new Date(n * 1000).toLocaleString();
}

function getActiveDirectoryQuery() {
  const inputVal = (el("input-directory-search")?.value || "").trim();
  if (state.directoryQuery.trim()) return state.directoryQuery.trim();
  if (inputVal) return inputVal;
  if (state.sourceGameQuery.trim()) return state.sourceGameQuery.trim();
  return "";
}

function getEffectiveSourceQuery() {
  const localFilter = (el("input-sources-filter")?.value || "").trim();
  if (localFilter) return localFilter;
  if (state.sourceGameQuery.trim()) return state.sourceGameQuery.trim();
  if (state.directoryQuery.trim()) return state.directoryQuery.trim();
  return "";
}

function setJsonFeedsExpanded(expanded) {
  state.jsonFeedsExpanded = Boolean(expanded);
  const body = el("json-feeds-body");
  const toggleBtn = el("btn-toggle-json-feeds");
  if (body) body.classList.toggle("hidden", !state.jsonFeedsExpanded);
  if (toggleBtn) toggleBtn.setAttribute("aria-expanded", state.jsonFeedsExpanded ? "true" : "false");
}

function matchesDirectoryCategory(site, category) {
  if (!category || category === "all") return true;
  const tag = String(site?.tag || "").trim();
  if (category === "Custom") {
    return tag.toLowerCase() === "custom" || !site?.isDefault;
  }
  return tag.toLowerCase() === category.toLowerCase();
}

function getFilteredDirectorySites() {
  const list = Array.isArray(state.searchSites) ? state.searchSites : [];
  return list.filter((site) => matchesDirectoryCategory(site, state.directoryCategory));
}

async function launchDirectorySite(site) {
  if (!site) return;
  const query = getActiveDirectoryQuery();
  const url = buildDirectorySearchUrl(site, query);
  if (!isValidHttpUrl(url)) {
    showToast(`Invalid URL configured for ${site.name || "site"}.`, "error");
    return;
  }
  const mode = el("select-dl-speed-mode")?.value || "unlimited";
  const maxSpeedMbps = mode === "unlimited" ? null : Number(mode);
  try {
    await openDirectoryBrowser({
      url,
      title: site.name,
      customSaveDir: state.customSaveDir || null,
      maxSpeedMbps
    });
    const actionLabel = query ? `Searching "${query}" on ${site.name}` : `Opening ${site.name}`;
    showToast(`${actionLabel} in Ad-Blocked Safe Browser...`, "info", 2200);
  } catch (err) {
    showToast(String(err), "error");
  }
}

function renderDirectoryGrid() {
  const grid = el("directory-grid");
  const summary = el("directory-count-summary");
  if (!grid) return;

  const activeQuery = getActiveDirectoryQuery();
  const banner = el("sources-game-banner");
  const bannerLabel = el("sources-game-query-label");
  if (banner && bannerLabel) {
    banner.classList.toggle("hidden", !activeQuery);
    if (activeQuery) bannerLabel.textContent = `Target: ${activeQuery}`;
  }

  document.querySelectorAll("#directory-category-pills .filter-pill").forEach((pill) => {
    pill.classList.toggle("active", pill.dataset.dirCategory === state.directoryCategory);
  });

  const filtered = getFilteredDirectorySites();
  const totalCount = Array.isArray(state.searchSites) ? state.searchSites.length : 0;
  if (summary) {
    const scope = state.directoryCategory === "all" ? "" : ` in ${state.directoryCategory}`;
    summary.textContent = `${filtered.length} of ${totalCount} directories${scope}`;
  }

  if (filtered.length === 0) {
    grid.innerHTML = `<div class="empty-card"><h3>No Directories in This Category</h3><p>Switch category pills above or click <strong>+ Add Site</strong> to add a custom directory search card.</p></div>`;
    return;
  }

  grid.innerHTML = filtered
    .map((site) => {
      const ctaText = activeQuery ? `Search on ${site.name}` : `Browse ${site.name}`;
      const highlightedClass = activeQuery ? "is-highlighted" : "";
      return `
        <article class="directory-card ${highlightedClass}" data-site-id="${escapeHtml(site.id)}" tabindex="0" role="button" aria-label="${escapeHtml(ctaText)}">
          <div class="directory-card-head">
            <div class="directory-card-title-wrap">
              <div class="directory-card-title">${escapeHtml(site.name)}</div>
              <span class="directory-tag">${escapeHtml(site.tag || "Custom")}</span>
            </div>
            <button type="button" class="btn btn-danger-ghost btn-xs" data-directory-action="remove" data-site-id="${escapeHtml(site.id)}" title="Remove directory card" aria-label="Remove ${escapeHtml(site.name)}">
              ${svgIcon("icon-trash-2")}
            </button>
          </div>
          <div class="directory-card-note">${escapeHtml(site.note || "Game directory & archive search")}</div>
          <div class="directory-card-foot">
            <span class="directory-card-cta">${svgIcon("icon-search")}<span>${escapeHtml(ctaText)}</span></span>
            <span class="directory-card-tools">
              <span class="pill">Safe Browser</span>
            </span>
          </div>
        </article>
      `;
    })
    .join("");
}

function handoffGameToFindGames(title) {
  const cleanTitle = String(title || "").trim();
  state.sourceGameQuery = cleanTitle;
  state.directoryQuery = cleanTitle;
  state.directoryCategory = "all";

  if (el("input-directory-search")) el("input-directory-search").value = cleanTitle;
  if (el("input-sources-filter")) el("input-sources-filter").value = cleanTitle;

  switchTab("sources");
  renderDirectoryGrid();
  refreshSourcesView({ autoExpandMatches: true });
}

function openAddSiteModal() {
  const modal = el("add-site-modal");
  if (!modal) return;
  if (el("input-site-name")) el("input-site-name").value = "";
  if (el("input-site-url")) el("input-site-url").value = "";
  if (el("input-site-tag")) el("input-site-tag").value = "Custom";
  if (el("input-site-note")) el("input-site-note").value = "";
  modal.classList.remove("hidden");
  el("input-site-name")?.focus();
}

function closeAddSiteModal() {
  el("add-site-modal")?.classList.add("hidden");
}

function openBulkImportModal() {
  const modal = el("bulk-import-modal");
  if (!modal) return;
  if (el("input-bulk-sites")) el("input-bulk-sites").value = "";
  modal.classList.remove("hidden");
  el("input-bulk-sites")?.focus();
}

function closeBulkImportModal() {
  el("bulk-import-modal")?.classList.add("hidden");
}

function renderSourcesFeedList() {
  const container = el("sources-feed-list");
  const badge = el("json-feeds-count-badge");
  const list = Array.isArray(state.importedSources) ? state.importedSources : [];
  if (badge) {
    badge.textContent = `${list.length} Feed${list.length === 1 ? "" : "s"}`;
  }
  if (!container) return;
  if (list.length === 0) {
    container.innerHTML = `<div class="panel-sub">No community Hydra JSON feeds imported yet. Paste a JSON manifest URL or load a local <code>.json</code> file above.</div>`;
    return;
  }

  container.innerHTML = list
    .map((src) => {
      const activeFilter = state.selectedSourceId === src.id ? "active" : "";
      const disabledClass = src.enabled ? "" : "is-disabled";
      return `
        <div class="source-feed-chip ${disabledClass}">
          <div class="source-feed-info">
            <div class="source-feed-name" title="${escapeHtml(src.location)}">${escapeHtml(src.name)}</div>
            <div class="source-feed-meta">${escapeHtml(src.kind?.toUpperCase() || "JSON")} · ${Number(src.itemCount || 0).toLocaleString()} entries · Synced ${escapeHtml(formatSyncTimestamp(src.lastSyncedTs))}</div>
          </div>
          <div class="source-feed-actions">
            <button type="button" class="pill ${activeFilter}" data-source-action="filter" data-source-id="${escapeHtml(src.id)}" title="Filter results to this source">Filter</button>
            <button type="button" class="btn btn-secondary btn-xs" data-source-action="toggle" data-source-id="${escapeHtml(src.id)}" data-enabled="${src.enabled ? "1" : "0"}">${src.enabled ? "Enabled" : "Disabled"}</button>
            <button type="button" class="btn btn-secondary btn-xs" data-source-action="refresh" data-source-id="${escapeHtml(src.id)}" title="Refresh Source">${svgIcon("icon-refresh-cw")}</button>
            <button type="button" class="btn btn-danger-ghost btn-xs" data-source-action="remove" data-source-id="${escapeHtml(src.id)}" title="Remove Source">${svgIcon("icon-trash-2")}</button>
          </div>
        </div>
      `;
    })
    .join("");
}

function renderSettingsSourcesList() {
  const container = el("settings-sources-list");
  if (!container) return;
  const list = Array.isArray(state.importedSources) ? state.importedSources : [];
  if (list.length === 0) {
    container.innerHTML = `<div class="panel-sub">No JSON sources configured. Import feeds under Find Games.</div>`;
    return;
  }

  container.innerHTML = list
    .map(
      (src) => `
      <div class="save-path-row">
        <div class="save-path-meta">
          <span class="source-feed-name">${escapeHtml(src.name)} (${Number(src.itemCount || 0).toLocaleString()} items)</span>
          <span class="save-path-text">${escapeHtml(src.location)} · Last synced ${escapeHtml(formatSyncTimestamp(src.lastSyncedTs))}</span>
        </div>
        <div class="source-feed-actions">
          <button type="button" class="btn btn-secondary btn-xs" data-settings-source-action="refresh" data-source-id="${escapeHtml(src.id)}">Refresh</button>
          <label class="switch">
            <input type="checkbox" data-settings-source-toggle="${escapeHtml(src.id)}" ${src.enabled ? "checked" : ""} />
            <span class="slider"></span>
          </label>
        </div>
      </div>
    `
    )
    .join("");
}

function renderSourcesResults() {
  const grid = el("sources-grid");
  const summary = el("sources-results-summary");
  if (!grid) return;

  const resp = state.sourceSearchResponse || {
    items: [],
    totalMatches: 0,
    enabledSourceCount: 0,
    totalSourceCount: 0
  };
  const items = Array.isArray(resp.items) ? resp.items : [];

  if (summary) {
    const filterSource = state.importedSources.find((s) => s.id === state.selectedSourceId);
    const scopeLabel = filterSource ? ` in ${filterSource.name}` : "";
    summary.textContent = `Showing ${items.length} of ${Number(resp.totalMatches || 0).toLocaleString()} matches${scopeLabel} · ${Number(resp.enabledSourceCount || 0)}/${Number(resp.totalSourceCount || 0)} sources enabled`;
  }

  if (resp.totalSourceCount === 0) {
    grid.innerHTML = `<div class="empty-card"><h3>No Hydra JSON Feeds Imported</h3><p>Import a community JSON manifest URL or local <code>.json</code> file above to search direct feed mirrors.</p></div>`;
    return;
  }

  if (items.length === 0) {
    grid.innerHTML = `<div class="empty-card"><h3>No Matching Releases</h3><p>Try clearing the filter text or enabling additional imported feeds above.</p></div>`;
    return;
  }

  grid.innerHTML = items
    .map((item) => {
      const uris = Array.isArray(item.uris) ? item.uris : [];
      const metaParts = [item.sourceName, item.fileSize, item.uploadDate].filter(Boolean);
      return `
        <article class="source-result-card">
          <div>
            <div class="source-result-title">${escapeHtml(item.title)}</div>
            <div class="source-result-sub">${escapeHtml(metaParts.join(" · "))}</div>
          </div>
          <div class="sources-uri-row">
            ${uris
              .slice(0, 6)
              .map((uri, idx) => {
                let hostLabel = `Mirror ${idx + 1}`;
                try {
                  if (/^https?:/i.test(uri)) {
                    hostLabel = new URL(uri).hostname.replace(/^www\./, "");
                  } else if (uri.includes(":")) {
                    hostLabel = uri.split(":")[0].toUpperCase();
                  }
                } catch {
                  // Keep fallback label
                }
                return `<button type="button" class="btn btn-primary btn-xs" data-enqueue-uri="${escapeHtml(uri)}" title="${escapeHtml(uri)}">${svgIcon("icon-download")}<span>${escapeHtml(hostLabel)}</span></button>`;
              })
              .join("")}
          </div>
        </article>
      `;
    })
    .join("");
}

async function refreshSourcesView(opts = {}) {
  try {
    const query = getEffectiveSourceQuery();
    const [feeds, searchResp, sites] = await Promise.all([
      getImportedSources(),
      searchImportedSources({
        query,
        sourceId: state.selectedSourceId || null,
        offset: 0,
        limit: 80
      }),
      getSearchSites()
    ]);
    state.importedSources = Array.isArray(feeds) ? feeds : [];
    state.sourceSearchResponse = searchResp || {
      items: [],
      totalMatches: 0,
      enabledSourceCount: 0,
      totalSourceCount: 0
    };
    if (Array.isArray(sites)) {
      state.searchSites = sites;
    }
    if (opts.autoExpandMatches && Number(state.sourceSearchResponse.totalMatches || 0) > 0) {
      setJsonFeedsExpanded(true);
    }
  } catch (err) {
    showToast(String(err), "error");
  }
  renderDirectoryGrid();
  renderSourcesFeedList();
  renderSettingsSourcesList();
  renderSourcesResults();
}

function renderSources() {
  renderDirectoryGrid();
  renderSourcesFeedList();
  renderSettingsSourcesList();
  renderSourcesResults();
  if (state.activeTab === "sources" || state.activeTab === "settings") {
    refreshSourcesView();
  }
}

/* ==========================================================================
   SEARCH GAMES — STEAM CATALOG CONTROLLER
   ========================================================================== */
function getInstalledCatalogMatchers() {
  const appIds = new Set();
  const titles = new Set();
  for (const g of state.library) {
    const aid = Number(g.appId || g.steamAppId || 0);
    if (aid > 0) appIds.add(aid);
    const norm = String(g.title || "")
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "");
    if (norm) titles.add(norm);
  }
  return { appIds, titles };
}

function isCatalogGameInstalled(entry, matchers) {
  if (matchers.appIds.has(Number(entry.appId))) return true;
  const norm = String(entry.name || "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "");
  return Boolean(norm && matchers.titles.has(norm));
}

function buildCatalogQueryParams(offset = 0) {
  const c = state.catalog;
  const matchers = getInstalledCatalogMatchers();
  const priceTier = String(c.priceTier || "all");
  const playerMode = String(c.playerMode || "all");
  return {
    query: String(c.searchQuery || "").trim(),
    sortBy: c.sortBy || "value_score",
    paidOnly: priceTier === "paid",
    singleplayer: playerMode === "singleplayer",
    coop: playerMode === "coop",
    excludePvp: Boolean(c.excludePvp),
    hideInstalled: Boolean(c.hideInstalled),
    installedAppIds: c.hideInstalled ? Array.from(matchers.appIds) : [],
    installedTitlesNorm: c.hideInstalled ? Array.from(matchers.titles) : [],
    minPriceCents: Number(c.minPriceCents || 0),
    priceTier,
    minRatingPct: Number(c.minRatingPct || 0),
    minReviews: Number(c.minReviews || 0),
    playerMode,
    minCcu: Number(c.minCcu || 0),
    genres: Array.isArray(c.selectedGenres) ? [...c.selectedGenres] : [],
    offset: Math.max(0, Number(offset || 0)),
    limit: Number(c.batchSize || 60)
  };
}

function buildCatalogCardElement(entry, installedSet) {
  const card = document.createElement("article");
  card.className = "catalog-card";
  card.dataset.appId = String(entry.appId);
  card.tabIndex = 0;

  const isInstalled = isCatalogGameInstalled(entry, installedSet);
  const modeTags = [];
  if (entry.isSingleplayer) modeTags.push("Single-Player");
  if (entry.isCoop) modeTags.push("Co-op");
  const modePill = modeTags.length > 0 ? modeTags.join(" · ") : (entry.genres?.[0] || "Steam");

  const priceText = entry.priceCents > 0 ? (entry.priceFormatted || `$${(entry.priceCents / 100).toFixed(2)}`) : "Free";
  const ccuText = Number(entry.ccu || 0) > 0 ? `${Number(entry.ccu).toLocaleString()} live` : "Steam";
  const ratingText = Number(entry.reviewPct || 0) > 0 ? `${Math.round(entry.reviewPct)}% pos` : (entry.ownersLabel || "");

  const media = document.createElement("div");
  media.className = "catalog-card-media";

  const img = document.createElement("img");
  img.className = "catalog-card-img";
  img.alt = "";
  img.loading = "lazy";
  img.decoding = "async";
  img.dataset.fallbackStage = "0";
  const primaryCover =
    entry.coverUrl ||
    `https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/${entry.appId}/library_600x900.jpg`;
  const headerFallback =
    entry.headerUrl ||
    `https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/${entry.appId}/header.jpg`;

  img.addEventListener("error", () => {
    const stage = Number(img.dataset.fallbackStage || "0");
    if (stage === 0 && img.src !== headerFallback) {
      img.dataset.fallbackStage = "1";
      img.src = headerFallback;
    } else if (stage < 2) {
      img.dataset.fallbackStage = "2";
      img.src = generateFallbackCoverDataUri(entry.name || "Steam Game", modePill);
    }
  });
  img.src = primaryCover;
  media.appendChild(img);

  const topBadges = document.createElement("div");
  topBadges.className = "catalog-card-badges-top";
  topBadges.innerHTML = `
    <span class="catalog-badge catalog-badge-ccu mono-num">${escapeHtml(ccuText)}</span>
    ${isInstalled ? '<span class="catalog-badge">Installed</span>' : ""}
  `;
  media.appendChild(topBadges);

  const bottomBadges = document.createElement("div");
  bottomBadges.className = "catalog-card-badges-bottom";
  bottomBadges.innerHTML = `
    <span class="catalog-badge catalog-badge-price ${entry.priceCents === 0 ? "is-free" : ""} mono-num">${escapeHtml(priceText)}</span>
    <span class="catalog-badge catalog-badge-mode">${escapeHtml(modeTags.length > 0 ? modeTags.join(" / ") : "Steam")}</span>
  `;
  media.appendChild(bottomBadges);

  const overlay = document.createElement("div");
  overlay.className = "catalog-card-overlay";
  overlay.innerHTML = `
    <div class="catalog-overlay-actions">
      <button type="button" class="btn btn-primary btn-xs" data-catalog-action="details" data-app-id="${escapeHtml(String(entry.appId))}">
        ${svgIcon("icon-sliders")}<span>Details</span>
      </button>
      <button type="button" class="btn btn-secondary btn-xs" data-catalog-action="sources" data-app-id="${escapeHtml(String(entry.appId))}">
        ${svgIcon("icon-search")}<span>Find &amp; Download</span>
      </button>
    </div>
  `;
  media.appendChild(overlay);
  card.appendChild(media);

  const body = document.createElement("div");
  body.className = "catalog-card-body";
  body.innerHTML = `
    <div class="catalog-card-title" title="${escapeHtml(entry.name)}">${escapeHtml(entry.name)}</div>
    <div class="catalog-card-sub">
      <span>${escapeHtml((entry.genres || [])[0] || modePill)}</span>
      <span class="mono-num">${escapeHtml(ratingText)}</span>
    </div>
  `;
  card.appendChild(body);
  return card;
}

function updateCatalogResultsSummary() {
  const c = state.catalog;
  const summary = el("catalog-results-summary");
  const statusSub = el("catalog-cache-status");
  const staleBadge = el("catalog-stale-badge");

  if (summary) {
    if (c.loading && c.totalCached === 0 && c.renderedCount === 0) {
      summary.textContent = "Indexing SteamSpy & Steam Store catalog...";
    } else {
      const shownCount = Math.min(c.renderedCount, c.totalMatches);
      const indexedDisplay = Math.max(c.totalCached, c.totalMatches).toLocaleString();
      const liveTag = c.enrichingLive ? " · Querying live Steam Store..." : "";
      const latencyTag =
        c.lastQueryTimeUs > 0 ? ` · ${(c.lastQueryTimeUs / 1000).toFixed(2)} ms` : "";
      summary.textContent = `Showing ${shownCount.toLocaleString()} of ${c.totalMatches.toLocaleString()} matching titles across ${indexedDisplay} indexed${latencyTag}${liveTag}`;
    }
  }

  if (statusSub) {
    if (c.syncing) {
      statusSub.textContent = `Streaming index over DoH · ${c.totalCached.toLocaleString()} / ${Number(c.syncTarget || 30000).toLocaleString()}+ indexed`;
    } else if (c.fetchedAt > 0) {
      const when = new Date(c.fetchedAt * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
      statusSub.textContent = `${c.fromCache ? "Zero-copy 24h .dkcat mmap" : "Synced via encrypted DoH"} · Updated ${when} · ${c.totalCached.toLocaleString()} indexed`;
    } else {
      statusSub.textContent = "Zero-copy .dkcat mmap engine · Ready";
    }
  }

  if (staleBadge) {
    staleBadge.classList.toggle("hidden", !c.staleWarning);
    if (c.staleWarning) staleBadge.title = c.staleWarning;
  }
}

async function runCatalogQuery(resetBatch = true) {
  const c = state.catalog;
  const grid = el("catalog-grid");
  const loadingEl = el("catalog-loading");
  const errorEl = el("catalog-error");
  const errorMsg = el("catalog-error-msg");
  const emptyEl = el("catalog-empty");
  if (!grid) return;

  if (!resetBatch) {
    if (c.loadingMore || c.loading) return;
    if (c.renderedCount >= c.totalMatches) return;
  }

  c.queryRequestId += 1;
  const qReqId = c.queryRequestId;
  if (!resetBatch) {
    c.loadingMore = true;
  }

  const offset = resetBatch ? 0 : c.renderedCount;
  if (loadingEl) loadingEl.classList.toggle("hidden", !(c.loading && c.totalCached === 0));
  if (errorEl) errorEl.classList.toggle("hidden", !(c.error && c.totalCached === 0));
  if (errorMsg && c.error) errorMsg.textContent = c.error;

  try {
    const page = await queryCatalogMmap(buildCatalogQueryParams(offset));
    if (qReqId !== c.queryRequestId) {
      if (!resetBatch) c.loadingMore = false;
      return;
    }

    const items = Array.isArray(page?.items) ? page.items : [];
    c.totalMatches = Number(page?.totalMatches || 0);
    if (Number(page?.totalIndexed || 0) > 0) {
      c.totalCached = Number(page.totalIndexed);
      c.loaded = true;
    }
    if (Number(page?.fetchedAt || 0) > 0) {
      c.fetchedAt = Number(page.fetchedAt);
    }
    c.lastQueryTimeUs = Number(page?.queryTimeUs || 0);

    if (resetBatch) {
      c.itemsById.clear();
      c.renderedCount = 0;
      grid.replaceChildren();
    }

    if (items.length > 0) {
      const installedSet = getInstalledCatalogMatchers();
      const frag = document.createDocumentFragment();
      for (const entry of items) {
        c.itemsById.set(Number(entry.appId), entry);
        frag.appendChild(buildCatalogCardElement(entry, installedSet));
      }
      grid.appendChild(frag);
      c.renderedCount += items.length;
    }

    if (!resetBatch) c.loadingMore = false;
    if (loadingEl) loadingEl.classList.toggle("hidden", !(c.loading && c.renderedCount === 0 && c.totalCached === 0));
    if (errorEl) errorEl.classList.toggle("hidden", !(c.error && c.renderedCount === 0 && c.totalCached === 0));
    if (emptyEl) {
      emptyEl.classList.toggle(
        "hidden",
        c.renderedCount > 0 || (c.loading && c.totalCached === 0) || Boolean(c.error && c.totalCached === 0)
      );
    }

    updateNavCatalogBadge();
    updateCatalogResultsSummary();
  } catch (err) {
    if (qReqId !== c.queryRequestId) return;
    c.loadingMore = false;
    c.error = String(err || "Catalog mmap query failed.");
    if (errorEl) errorEl.classList.toggle("hidden", c.renderedCount > 0);
    if (errorMsg) errorMsg.textContent = c.error;
    updateCatalogResultsSummary();
  }
}

function appendNextCatalogBatch() {
  return runCatalogQuery(false);
}

function renderCatalogView(resetBatch = true) {
  return runCatalogQuery(resetBatch);
}

async function triggerLiveSteamEnrichment(queryText) {
  const c = state.catalog;
  const trimmed = String(queryText || "").trim();
  if (trimmed.length < 2) return;
  if (c.totalMatches >= 12 || c.lastEnrichedQuery === trimmed) return;

  c.lastEnrichedQuery = trimmed;
  c.enrichingLive = true;
  updateCatalogResultsSummary();

  try {
    const res = await enrichCatalogQuery(trimmed);
    c.enrichingLive = false;
    if (Number(res?.totalIndexed || 0) > 0) {
      c.totalCached = Number(res.totalIndexed);
      updateNavCatalogBadge();
    }
    if (state.catalog.searchQuery.trim() === trimmed && Number(res?.upsertedCount || 0) > 0) {
      await runCatalogQuery(true);
      return;
    }
    updateCatalogResultsSummary();
  } catch {
    c.enrichingLive = false;
    updateCatalogResultsSummary();
  }
}

function handleCatalogSearchInput(nextQ) {
  state.catalog.searchQuery = nextQ;
  runCatalogQuery(true);

  clearTimeout(state.catalog.debounceTimer);
  const trimmed = nextQ.trim();
  if (trimmed.length >= 2) {
    state.catalog.debounceTimer = setTimeout(() => {
      if (state.catalog.searchQuery.trim() === trimmed && state.catalog.totalMatches < 12) {
        triggerLiveSteamEnrichment(trimmed);
      }
    }, 340);
  } else {
    state.catalog.lastEnrichedQuery = "";
  }
}

function setupCatalogIntersectionObserver() {
  const sentinel = el("catalog-scroll-sentinel");
  const scroller = document.querySelector(".workspace-scroll");
  if (!sentinel || typeof IntersectionObserver === "undefined") return;

  if (state.catalog.observer) {
    state.catalog.observer.disconnect();
  }
  state.catalog.observer = new IntersectionObserver(
    (entries) => {
      if (state.activeTab !== "catalog") return;
      if (entries.some((entry) => entry.isIntersecting)) {
        appendNextCatalogBatch();
      }
    },
    { root: scroller || null, rootMargin: "320px 0px" }
  );
  state.catalog.observer.observe(sentinel);

  scroller?.addEventListener(
    "scroll",
    () => {
      if (state.activeTab !== "catalog") return;
      if (scroller.scrollTop + scroller.clientHeight >= scroller.scrollHeight - 360) {
        appendNextCatalogBatch();
      }
    },
    { passive: true }
  );
}

function syncCatalogFilterControlsToDom() {
  const c = state.catalog;
  if (el("input-catalog-search") && el("input-catalog-search").value !== c.searchQuery) {
    el("input-catalog-search").value = c.searchQuery;
  }
  if (el("select-catalog-sort")) el("select-catalog-sort").value = c.sortBy;
  if (el("select-catalog-price-tier")) el("select-catalog-price-tier").value = c.priceTier;
  if (el("select-catalog-min-rating")) el("select-catalog-min-rating").value = String(c.minRatingPct);
  if (el("select-catalog-min-reviews")) el("select-catalog-min-reviews").value = String(c.minReviews);
  if (el("select-catalog-player-mode")) el("select-catalog-player-mode").value = c.playerMode;
  if (el("select-catalog-min-ccu")) el("select-catalog-min-ccu").value = String(c.minCcu);

  const hideBtn = el("btn-catalog-hide-installed");
  if (hideBtn) {
    const active = Boolean(c.hideInstalled);
    hideBtn.classList.toggle("active", active);
    hideBtn.setAttribute("aria-pressed", active ? "true" : "false");
  }

  const drawer = el("catalog-genres-drawer");
  const toggleBtn = el("btn-toggle-catalog-genres");
  const drawerBody = el("catalog-genres-drawer-body");
  const isOpen = Boolean(c.genresDrawerOpen);
  if (drawer) drawer.classList.toggle("is-open", isOpen);
  if (toggleBtn) toggleBtn.setAttribute("aria-expanded", isOpen ? "true" : "false");
  if (drawerBody) drawerBody.classList.toggle("hidden", !isOpen);

  const selected = Array.isArray(c.selectedGenres) ? c.selectedGenres : [];
  const countBadge = el("catalog-genre-count-badge");
  if (countBadge) {
    countBadge.textContent = `${selected.length} Selected`;
    countBadge.classList.toggle("accent", selected.length > 0);
  }

  const clearBtn = el("btn-catalog-clear-genres");
  if (clearBtn) {
    clearBtn.classList.toggle("hidden", selected.length === 0 && !String(c.tagSearch || "").trim());
  }

  const preview = el("catalog-active-genres-preview");
  if (preview) {
    if (selected.length === 0) {
      preview.innerHTML = `<span class="panel-sub">Click to combine 36 genres &amp; technical tags in O(1) bitmask speed</span>`;
    } else {
      preview.innerHTML = selected
        .map(
          (g) => `
            <button type="button" class="catalog-active-chip" data-remove-genre="${escapeHtml(g)}" title="Remove ${escapeHtml(g)} filter">
              <span>${escapeHtml(g)}</span>
              <svg class="icon" aria-hidden="true"><use href="#icon-x"/></svg>
            </button>
          `
        )
        .join("");
    }
  }

  const tagInput = el("input-catalog-tag-filter");
  if (tagInput && tagInput.value !== (c.tagSearch || "")) {
    tagInput.value = c.tagSearch || "";
  }
  const tagFilterLower = String(c.tagSearch || "").trim().toLowerCase();

  document.querySelectorAll("#catalog-genre-groups .catalog-tag-pill[data-catalog-genre]").forEach((btn) => {
    const g = btn.dataset.catalogGenre || "";
    btn.classList.toggle("active", selected.includes(g));
    const matchesSearch = !tagFilterLower || g.toLowerCase().includes(tagFilterLower);
    btn.classList.toggle("hidden", !matchesSearch);
  });

  document.querySelectorAll("#catalog-genre-groups .catalog-genre-group").forEach((group) => {
    const visiblePills = group.querySelectorAll(".catalog-tag-pill:not(.hidden)");
    group.classList.toggle("hidden", visiblePills.length === 0);
  });
}

async function loadCatalogData({ query = state.catalog.searchQuery, forceRefresh = false } = {}) {
  const c = state.catalog;
  c.requestId += 1;
  const reqId = c.requestId;
  if (forceRefresh) {
    c.lastEnrichedQuery = "";
  }
  c.loading = c.totalCached === 0 || forceRefresh;
  c.error = null;
  if (c.renderedCount === 0) {
    if (el("catalog-loading")) el("catalog-loading").classList.toggle("hidden", c.totalCached > 0);
    if (el("catalog-error")) el("catalog-error").classList.add("hidden");
    updateCatalogResultsSummary();
  }

  // If .dkcat is already mapped locally and not forcing a full refresh, render page 0 immediately.
  if (!forceRefresh && c.totalCached > 0 && c.renderedCount === 0) {
    runCatalogQuery(true);
  }

  try {
    const resp = await fetchCatalogGames(0, query, forceRefresh);
    if (reqId !== c.requestId) return;

    c.totalCached = Number(resp?.totalCached || c.totalCached || 0);
    c.fetchedAt = Number(resp?.fetchedAt || c.fetchedAt || 0);
    c.fromCache = Boolean(resp?.fromCache);
    c.staleWarning = resp?.staleWarning || null;
    c.loaded = true;
    c.loading = false;
    c.error = null;

    updateNavCatalogBadge();
    await runCatalogQuery(true);

    const trimmed = String(query || "").trim();
    if (trimmed.length >= 2 && c.totalMatches < 12) {
      triggerLiveSteamEnrichment(trimmed);
    }
  } catch (err) {
    if (reqId !== c.requestId) return;
    c.loading = false;
    c.error = String(err || "Catalog lookup failed.");
    if (c.renderedCount === 0) {
      if (el("catalog-loading")) el("catalog-loading").classList.add("hidden");
      if (el("catalog-error")) el("catalog-error").classList.remove("hidden");
      if (el("catalog-error-msg")) el("catalog-error-msg").textContent = c.error;
    }
    updateCatalogResultsSummary();
  }
}

function populateCatalogModal(entry) {
  if (!entry) return;
  state.catalog.modalGame = entry;

  if (el("catalog-modal-title")) el("catalog-modal-title").textContent = entry.name || "Steam Game";
  if (el("catalog-modal-desc")) {
    el("catalog-modal-desc").textContent =
      entry.shortDescription || "Steam catalog entry indexed locally over DoH.";
  }

  const priceLabel =
    entry.priceCents > 0
      ? entry.priceFormatted || `$${(entry.priceCents / 100).toFixed(2)}`
      : "Free";
  const discBadge =
    entry.discountPercent > 0 ? `<span class="pill emerald mono-num">-${entry.discountPercent}%</span>` : "";

  if (el("catalog-modal-badges")) {
    el("catalog-modal-badges").innerHTML = `
      <span class="pill gold mono-num">${escapeHtml(priceLabel)}</span>
      ${discBadge}
      <span class="pill emerald mono-num">${Number(entry.ccu || 0).toLocaleString()} live</span>
      <span class="pill mono-num">AppID ${escapeHtml(String(entry.appId))}</span>
    `;
  }

  if (el("catalog-modal-meta")) {
    const modes = [];
    if (entry.isSingleplayer) modes.push("Single-player");
    if (entry.isCoop) modes.push("Co-op");
    if (entry.isMultiplayer) modes.push("Multiplayer");

    el("catalog-modal-meta").innerHTML = `
      <div class="kv-row"><span class="kv-k">Estimated Owners</span><span class="kv-v mono-num">${escapeHtml(entry.ownersLabel || "Unreported")}</span></div>
      <div class="kv-row"><span class="kv-k">Positive Reviews</span><span class="kv-v mono-num">${entry.reviewPct > 0 ? `${entry.reviewPct.toFixed(1)}% (${Number(entry.positive || 0).toLocaleString()})` : "--"}</span></div>
      <div class="kv-row"><span class="kv-k">Developer</span><span class="kv-v">${escapeHtml((entry.developers || []).join(", ") || "--")}</span></div>
      <div class="kv-row"><span class="kv-k">Publisher</span><span class="kv-v">${escapeHtml((entry.publishers || []).join(", ") || "--")}</span></div>
      <div class="kv-row"><span class="kv-k">Release Date</span><span class="kv-v">${escapeHtml(entry.releaseDate || "--")}</span></div>
      <div class="kv-row"><span class="kv-k">Genres</span><span class="kv-v">${escapeHtml((entry.genres || []).join(", ") || "--")}</span></div>
      <div class="kv-row"><span class="kv-k">Steam Categories</span><span class="kv-v">${escapeHtml((entry.categories || []).join(", ") || modes.join(", ") || "--")}</span></div>
    `;
  }
}

async function openCatalogModal(appId) {
  const entry = state.catalog.itemsById.get(Number(appId));
  if (!entry) return;

  state.catalog.modalRequestId += 1;
  const modalReqId = state.catalog.modalRequestId;
  state.catalog.modalGame = entry;

  const modalHeroImg = el("catalog-modal-hero-img");
  if (modalHeroImg) {
    modalHeroImg.onerror = null;
    modalHeroImg.onload = null;
    modalHeroImg.alt = "";
    modalHeroImg.removeAttribute("src");
    modalHeroImg.classList.add("hidden");
  }

  populateCatalogModal(entry);
  el("catalog-detail-modal")?.classList.remove("hidden");

  const isStillModalCurrent = () =>
    modalReqId === state.catalog.modalRequestId &&
    !el("catalog-detail-modal")?.classList.contains("hidden");

  applyHeroCandidateChain(
    modalHeroImg,
    buildHeroCandidateList(entry.appId, entry.coverUrl),
    isStillModalCurrent
  );

  try {
    const enriched = await getCatalogGameDetails(entry.appId);
    if (!isStillModalCurrent()) return;
    if (enriched) {
      state.catalog.itemsById.set(Number(enriched.appId), enriched);
      populateCatalogModal(enriched);
    }
  } catch {
    // Keep fast cached summary if Store enrichment fails offline
  }
}

function switchCoverModalTab(tabName) {
  const isSgdb = tabName === "sgdb";
  document.querySelectorAll("#cover-modal-tabs .pill").forEach((btn) => {
    btn.classList.toggle("active", btn.dataset.coverTab === tabName);
  });
  el("cover-panel-steam")?.classList.toggle("hidden", isSgdb);
  el("cover-panel-sgdb")?.classList.toggle("hidden", !isSgdb);
}

function renderSgdbResults() {
  const container = el("sgdb-results-container");
  if (!container) return;
  const res = state.sgdbResults;
  if (!res) {
    container.innerHTML = `<div class="panel-sub">Click Search SteamGridDB to browse 600x900 grids, library heroes, and .ICO icons.</div>`;
    return;
  }

  if (!res.hasApiKey) {
    container.innerHTML = `<div class="panel-sub">${escapeHtml(res.message || "Add your SteamGridDB API key in Settings to browse community grids, heroes, and .ICO icons.")}</div>`;
    return;
  }

  const renderSection = (label, items, kind, extraClass = "") => {
    const arr = Array.isArray(items) ? items : [];
    if (arr.length === 0) return "";
    const selectedUrl =
      kind === "cover"
        ? state.sgdbSelection.coverUrl
        : kind === "hero"
          ? state.sgdbSelection.heroUrl
          : state.sgdbSelection.iconUrl;

    return `
      <div class="panel-sub" style="margin-top:8px;"><strong>${escapeHtml(label)}</strong></div>
      <div class="sgdb-grid-gallery">
        ${arr
          .map((item) => {
            const isSelected = selectedUrl === item.url ? "selected" : "";
            return `
              <button type="button" class="sgdb-thumb-card ${extraClass} ${isSelected}" data-sgdb-kind="${escapeHtml(kind)}" data-sgdb-url="${escapeHtml(item.url)}">
                <img src="${escapeHtml(item.thumb || item.url)}" alt="${escapeHtml(label)}" loading="lazy" />
                <span class="sgdb-thumb-label">${Number(item.width || 0)}x${Number(item.height || 0)} · ${escapeHtml(item.style || item.mime || kind)}</span>
              </button>
            `;
          })
          .join("")}
      </div>
    `;
  };

  const gameTitle = res.matchedGame?.name ? `Matched: ${res.matchedGame.name}` : "";
  const sections =
    renderSection("600x900 Grids (Cover Art)", res.grids, "cover") +
    renderSection("Library Heroes (Banner)", res.heroes, "hero", "is-hero") +
    renderSection("Icons (.ICO / PNG)", res.icons, "icon", "is-icon");

  container.innerHTML = sections
    ? `<div class="panel-sub">${escapeHtml(gameTitle)}</div>${sections}`
    : `<div class="panel-sub">${escapeHtml(res.message || "No SteamGridDB artwork found for that query.")}</div>`;
}

async function runSgdbSearch() {
  const query = (el("input-sgdb-query")?.value || "").trim();
  const game = state.library.find((g) => g.id === state.modalGameId);
  const queryOrAppId = query || (game?.steamAppId ? String(game.steamAppId) : game?.title || "");
  if (!queryOrAppId) return;

  const container = el("sgdb-results-container");
  if (container) container.innerHTML = `<div class="panel-sub">Querying SteamGridDB over DoH...</div>`;

  try {
    state.sgdbResults = await searchSteamGridDbArtwork(queryOrAppId);
  } catch (err) {
    state.sgdbResults = {
      hasApiKey: Boolean(state.steamgriddbApiKey),
      grids: [],
      heroes: [],
      icons: [],
      message: String(err)
    };
  }
  renderSgdbResults();
}

function openDetailsModal(gameId) {
  const game = state.library.find((g) => g.id === gameId);
  if (!game) return;
  state.modalGameId = game.id;
  state.sgdbResults = null;
  state.sgdbSelection = { coverUrl: null, heroUrl: null, iconUrl: null };
  switchCoverModalTab("steam");

  if (el("details-title")) el("details-title").textContent = game.title;
  if (el("details-size-badge")) el("details-size-badge").textContent = game.sizeFormatted || formatBytesJs(game.sizeBytes);
  if (el("details-genre-badge")) el("details-genre-badge").textContent = resolveGameSourceLabel(game);
  if (el("input-cover-query")) el("input-cover-query").value = game.title || "";
  if (el("input-sgdb-query")) el("input-sgdb-query").value = game.title || "";
  renderSgdbResults();

  if (el("details-meta-list")) {
    el("details-meta-list").innerHTML = `
      <div class="kv-row"><span class="kv-k">Executable</span><span class="kv-v mono-num">${escapeHtml(game.exePath)}</span></div>
      <div class="kv-row"><span class="kv-k">Install Folder</span><span class="kv-v mono-num">${escapeHtml(game.installDir)}</span></div>
      <div class="kv-row"><span class="kv-k">Source</span><span class="kv-v mono-num">${escapeHtml(resolveGameSourceLabel(game))}</span></div>
      <div class="kv-row"><span class="kv-k">Playtime</span><span class="kv-v mono-num">${escapeHtml(formatPlaytime(game.playtimeSeconds))}</span></div>
      <div class="kv-row"><span class="kv-k">Steam Value</span><span class="kv-v mono-num">${escapeHtml(game.steamPriceFormatted || "Unknown")}</span></div>
    `;
  }

  if (el("details-actions")) {
    el("details-actions").innerHTML = `
      <button type="button" class="btn btn-steam-green btn-sm" data-modal-action="play">${svgIcon("icon-play")}<span>Play</span></button>
      <button type="button" class="btn btn-secondary btn-sm" data-modal-action="folder">${svgIcon("icon-folder-open")}<span>Folder</span></button>
      <button type="button" class="btn btn-secondary btn-sm" data-modal-action="change-exe">${svgIcon("icon-sliders")}<span>Change Exe</span></button>
      <button type="button" class="btn btn-danger-ghost btn-sm" data-modal-action="remove">${svgIcon("icon-trash-2")}<span>Remove</span></button>
    `;
  }

  el("details-modal")?.classList.remove("hidden");
}

function renderRestoreSnapshots() {
  const select = el("restore-snapshot-select");
  const list = el("restore-snapshot-list");
  const confirmBtn = el("btn-confirm-restore-save");
  const snaps = Array.isArray(state.restoreSaveState.snapshots) ? state.restoreSaveState.snapshots : [];

  if (confirmBtn) confirmBtn.disabled = snaps.length === 0;

  if (select) {
    select.innerHTML =
      snaps.length > 0
        ? snaps
            .map(
              (s) =>
                `<option value="${escapeHtml(s.path)}">${escapeHtml(s.filename)} (${escapeHtml(s.sizeFormatted || formatBytesJs(s.sizeBytes))})</option>`
            )
            .join("")
        : `<option value="">No backup snapshots found</option>`;
  }

  if (list) {
    if (snaps.length === 0) {
      list.innerHTML = `<div class="panel-sub">No rolling snapshots exist for this game yet. Click Backup Saves first.</div>`;
    } else {
      list.innerHTML = snaps
        .map(
          (s) => `
          <div class="restore-snapshot-item">
            <span>${escapeHtml(s.filename)} · ${escapeHtml(formatSyncTimestamp(s.createdTs))}</span>
            <span>${escapeHtml(s.sizeFormatted || formatBytesJs(s.sizeBytes))} (${Number(s.rootCount || 1)} root${Number(s.rootCount || 1) === 1 ? "" : "s"})</span>
          </div>
        `
        )
        .join("");
    }
  }
}

async function openRestoreSaveModal(game) {
  if (!game) return;
  state.restoreSaveState = { gameId: game.id, snapshots: [] };
  if (el("restore-save-title")) el("restore-save-title").textContent = `Restore Save: ${game.title}`;
  if (el("restore-save-subtitle")) {
    el("restore-save-subtitle").textContent = "Select a rolling zip snapshot to safely restore your game save files.";
  }
  renderRestoreSnapshots();
  el("restore-save-modal")?.classList.remove("hidden");

  try {
    const snaps = await listSaveBackups(game.id);
    if (state.restoreSaveState.gameId !== game.id) return;
    state.restoreSaveState.snapshots = Array.isArray(snaps) ? snaps : [];
    renderRestoreSnapshots();
  } catch (err) {
    showToast(String(err), "error");
  }
}

function renderSnapshotsModalList() {
  const list = el("snapshots-list");
  if (!list) return;
  const snaps = Array.isArray(state.snapshotModalState.snapshots)
    ? state.snapshotModalState.snapshots
    : [];
  if (snaps.length === 0) {
    list.innerHTML = `<div class="panel-sub">No copy-on-write snapshots yet. Enter a label and click Create Snapshot.</div>`;
    return;
  }
  list.innerHTML = snaps
    .map(
      (s) => `
      <div class="restore-snapshot-item">
        <div>
          <strong>${escapeHtml(s.label || s.id)}</strong>
          <span class="panel-sub"> · ${escapeHtml(formatSyncTimestamp(s.createdAt))} · ${Number(s.fileCount || 0).toLocaleString()} files (${escapeHtml(formatBytesJs(s.totalBytes || 0))}, ${Number(s.deduplicatedFiles || 0)} deduped, ${Number(s.elapsedMs || 0)} ms)</span>
        </div>
        <div class="snapshot-row-actions">
          <button type="button" class="btn btn-primary btn-xs" data-snapshot-restore="${escapeHtml(s.id)}">Restore</button>
          <button type="button" class="btn btn-danger-ghost btn-xs" data-snapshot-delete="${escapeHtml(s.id)}">Delete</button>
        </div>
      </div>
    `
    )
    .join("");
}

async function openSnapshotsModal(game) {
  if (!game) return;
  state.snapshotModalState = { gameId: game.id, gameTitle: game.title, snapshots: [] };
  if (el("snapshots-modal-title")) el("snapshots-modal-title").textContent = `Snapshots: ${game.title}`;
  if (el("input-snapshot-label")) el("input-snapshot-label").value = "";
  if (el("snapshots-list")) el("snapshots-list").innerHTML = `<div class="panel-sub">Loading .duke-cas manifests...</div>`;
  el("snapshots-modal")?.classList.remove("hidden");
  try {
    const snaps = await listGameSnapshots(game.id);
    if (state.snapshotModalState.gameId !== game.id) return;
    state.snapshotModalState.snapshots = Array.isArray(snaps) ? snaps : [];
    renderSnapshotsModalList();
  } catch (err) {
    showToast(String(err), "error");
    renderSnapshotsModalList();
  }
}

function renderPreflightModal() {
  const { game, report, manualInspect } = state.preflightModalState;
  const subEl = el("preflight-modal-subtitle");
  const listEl = el("preflight-dll-list");
  const healBtn = el("btn-preflight-heal");
  const launchBtn = el("btn-preflight-launch-anyway");
  if (!report || !listEl) return;

  const missing = Array.isArray(report.missingDlls) ? report.missingDlls : [];
  const totalImported = Array.isArray(report.importedDlls) ? report.importedDlls.length : 0;
  if (subEl) {
    subEl.textContent = `${report.architecture || "PE"} · ${totalImported} imports scanned in ${Number(report.elapsedUs || 0)} us · ${report.systemDirChecked || "System32"}`;
  }

  if (missing.length === 0) {
    listEl.innerHTML = `<div class="restore-snapshot-item"><span>All ${totalImported} imported DLLs resolved cleanly in game directory or Windows system path.</span></div>`;
  } else {
    listEl.innerHTML = missing
      .map(
        (d) => `
        <div class="restore-snapshot-item">
          <div>
            <strong>${escapeHtml(d.name)}</strong>
            ${d.isRuntimeHint ? `<span class="panel-sub"> (${escapeHtml(d.isRuntimeHint)})</span>` : ""}
          </div>
          <span class="pill">${d.healCandidatePath ? "Local Redist Available" : "Missing Runtime"}</span>
        </div>
      `
      )
      .join("");
  }

  healBtn?.classList.toggle("hidden", missing.length === 0 || Number(report.healableCount || 0) === 0);
  launchBtn?.classList.toggle("hidden", manualInspect || !game || missing.length === 0);
}

function openPreflightModal(game, report, manualInspect = false) {
  state.preflightModalState = { game, report, manualInspect };
  if (el("preflight-modal-title")) {
    el("preflight-modal-title").textContent = game
      ? `Pre-Flight Check: ${game.title}`
      : "Pre-Flight DLL Dependency Report";
  }
  renderPreflightModal();
  el("preflight-modal")?.classList.remove("hidden");
}

function renderSwarmSettingsPanel(status) {
  const listEl = el("swarm-peers-list");
  if (!listEl) return;
  const peers = Array.isArray(status?.peers) ? status.peers : [];
  if (peers.length === 0) {
    listEl.innerHTML = status?.enabled
      ? `<div class="panel-sub">Swarm node "${escapeHtml(status.nodeName || "Duke-Node")}" listening on UDP ${escapeHtml(status.multicastGroup || "239.255.77.88:47880")} &amp; TCP ${Number(status.tcpPort || 47881)}. No remote peers detected yet.</div>`
      : `<div class="panel-sub">Enable Duke Swarm or click Probe Peers to inspect LAN multicast &amp; Tailscale peers.</div>`;
    return;
  }

  listEl.innerHTML = peers
    .map((peer) => {
      const targetAddr = `${peer.addr}:${Number(peer.tcpPort || 47881)}`;
      const apps = Array.isArray(peer.apps) ? peer.apps : [];
      const appsHtml =
        apps.length === 0
          ? `<div class="panel-sub">No beamable games advertised yet.</div>`
          : apps
              .slice(0, 8)
              .map(
                (appItem) => `
              <div class="swarm-app-row">
                <span class="mono-num">${escapeHtml(appItem.title || appItem.gameKey)} (${escapeHtml(formatBytesJs(appItem.sizeBytes || 0))})</span>
                <button type="button" class="btn btn-primary btn-xs" data-swarm-beam-peer="${escapeHtml(targetAddr)}" data-swarm-beam-key="${escapeHtml(appItem.gameKey)}">Beam Over LAN</button>
              </div>
            `
              )
              .join("");
      return `
        <div class="swarm-peer-card">
          <div class="swarm-peer-head">
            <strong>${escapeHtml(peer.nodeName || peer.nodeId)}</strong>
            <span class="pill mono-num">${escapeHtml(peer.transport || "LAN")} · ${escapeHtml(targetAddr)}</span>
          </div>
          ${appsHtml}
        </div>
      `;
    })
    .join("");
}

async function refreshSwarmAndMmapTelemetry() {
  try {
    const mmap = await getCatalogMmapStats();
    const statEl = el("settings-dkcat-stats");
    if (mmap && mmap.active && Number(mmap.recordCount || 0) > 0) {
      state.catalog.totalCached = Number(mmap.recordCount);
      if (Number(mmap.fetchedAt || 0) > 0) state.catalog.fetchedAt = Number(mmap.fetchedAt);
      updateNavCatalogBadge();
    }
    if (statEl && mmap) {
      statEl.textContent = mmap.active
        ? `Catalog mmap engine (.dkcat): ${Number(mmap.recordCount || 0).toLocaleString()} records · ${formatBytesJs(mmap.fileSizeBytes || 0)} · mapped in ${Number(mmap.mapTimeUs || 0)} us`
        : "Catalog mmap engine (.dkcat): Not built yet (opens in Search Games)";
    }
  } catch {
    // Non-critical telemetry display.
  }

  try {
    const status = await getSwarmStatus();
    if (status) {
      if (el("setting-swarm-node-name") && !el("setting-swarm-node-name").value && status.nodeName) {
        el("setting-swarm-node-name").value = status.nodeName;
      }
      renderSwarmSettingsPanel(status);
    }
  } catch {
    // Ignore when swarm is idle/disabled.
  }
}

async function launchGameDirectly(game) {
  if (!game) return;
  try {
    showToast(`Launching ${game.title}...`, "info");
    await invokeCommand("launch_game", {
      exePath: game.exePath,
      gameId: game.id,
      launchArgs: game.launchArgs || null
    });
    setTimeout(refreshActiveSessions, 400);
  } catch (err) {
    showToast(String(err), "error");
  }
}

async function launchGameById(game) {
  if (!game) return;
  if (/\.exe$/i.test(String(game.exePath || "").trim())) {
    try {
      const report = await preflightCheckExe(game.exePath);
      if (Array.isArray(report?.missingDlls) && report.missingDlls.length > 0) {
        openPreflightModal(game, report, false);
        return;
      }
    } catch {
      // Non-blocking fail-open if binary is packed/custom or script-wrapped.
    }
  }
  await launchGameDirectly(game);
}

async function toggleGameSuspendFlow(gameId = null, action = null) {
  try {
    const res = await suspendOrResumeGame(gameId, action);
    await refreshActiveSessions();
    showToast(
      res?.message || (res?.isSuspended ? "Suspended game process." : "Resumed game process."),
      "info"
    );
  } catch (err) {
    showToast(String(err), "error");
  }
}

async function runImportLibrariesFlow() {
  showToast("Scanning Steam, Epic, GOG, and Xbox libraries...", "info", 2800);
  try {
    const res = await importExistingLibraries();
    if (Array.isArray(res?.games)) {
      state.library = res.games;
    } else {
      state.library = await invokeCommand("get_local_library");
    }
    if (Number(res?.added || 0) > 0 || Number(res?.updated || 0) > 0) {
      invalidateSidebarIcons();
    }
    renderSidebarGameList();
    renderLibraryGrid();
    updateSidebarTelemetry();
    const added = Number(res?.added || 0);
    const updated = Number(res?.updated || 0);
    showToast(
      `Library scan complete: ${added} new, ${updated} updated (${Number(res?.total || state.library.length)} total).`,
      "success"
    );
  } catch (err) {
    showToast(String(err), "error");
  }
}

async function addLocalGameFlow() {
  try {
    const exePath = await pickExecutableFile();
    if (!exePath) return;
    const baseName = exePath.split(/[\\/]/).pop()?.replace(/\.(exe|bat|cmd)$/i, "") || "PC Game";
    const added = await invokeCommand("add_local_game", {
      exePath,
      customTitle: baseName,
      customCover: null,
      genre: "Installed"
    });
    state.library = await invokeCommand("get_local_library");
    if (added?.id) {
      invalidateSidebarIcons([added.id]);
    }
    renderSidebarGameList();
    renderLibraryGrid();
    updateSidebarTelemetry();
    showToast(`Added ${added?.title || baseName} to My Games.`, "success");
  } catch (err) {
    showToast(String(err), "error");
  }
}

async function refreshAllData() {
  try {
    state.library = await invokeCommand("get_local_library");
    state.downloadJobs = await invokeCommand("get_download_jobs", { backupJobs: null });
    const [telemetry, sessions] = await Promise.all([
      invokeCommand("get_system_telemetry").catch(() => null),
      getActiveGameSessions().catch(() => [])
    ]);
    state.activeSessions = Array.isArray(sessions) ? sessions : [];
    renderSidebarGameList();
    renderLibraryGrid();
    renderDownloadJobs();
    syncActiveSessionsUi();
    updateSidebarTelemetry(telemetry);
    refreshSourcesView();
    getCatalogMmapStats()
      .then((mmap) => {
        if (mmap?.active && Number(mmap.recordCount || 0) > 0) {
          state.catalog.totalCached = Number(mmap.recordCount);
          if (Number(mmap.fetchedAt || 0) > 0) state.catalog.fetchedAt = Number(mmap.fetchedAt);
          updateNavCatalogBadge();
        }
      })
      .catch(() => {});
    if (state.library.some((g) => !g.steamPriceFormatted)) {
      invokeCommand("sync_library_steam_prices")
        .then((lib) => {
          state.library = lib;
          updateSidebarTelemetry();
        })
        .catch(() => {});
    }
  } catch (err) {
    showToast(String(err), "error");
  }
}

function applyCouchMode(enabled, persist = false) {
  state.couchMode = Boolean(enabled);
  document.body.classList.toggle("couch-mode", state.couchMode);
  if (el("setting-couch-mode")) {
    el("setting-couch-mode").checked = state.couchMode;
  }

  try {
    if (state.couchMode && !document.fullscreenElement) {
      document.documentElement.requestFullscreen?.().catch(() => {});
    } else if (!state.couchMode && document.fullscreenElement) {
      document.exitFullscreen?.().catch(() => {});
    }
  } catch {
    // Fullscreen optional depending on WebView2 state
  }

  if (persist) {
    updateLauncherSettings({ couchMode: state.couchMode }).catch((err) => {
      showToast(String(err), "error");
    });
  }
}

async function readSystemClipboardText() {
  try {
    if (navigator.clipboard?.readText) {
      const text = await navigator.clipboard.readText();
      if (typeof text === "string" && text.length > 0) {
        return text;
      }
    }
  } catch {
    // WebView focus locks can block JS clipboard reads; fall back to Win32 IPC command.
  }
  try {
    const nativeText = await readClipboardText();
    return typeof nativeText === "string" ? nativeText : "";
  } catch {
    return "";
  }
}

async function copyTextToClipboard(text) {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {
    // Fall back to Win32 clipboard command.
  }
  try {
    await writeClipboardText(text);
    return true;
  } catch {
    return false;
  }
}

function isSupportedClipboardDownloadUrl(rawText) {
  const trimmed = String(rawText || "").trim();
  if (!trimmed || !isValidHttpUrl(trimmed)) return false;
  let parsed;
  try {
    parsed = new URL(trimmed);
  } catch {
    return false;
  }
  const host = (parsed.hostname || "").toLowerCase();
  if (SUPPORTED_CLIPBOARD_HOSTS.some((domain) => host === domain || host.endsWith(`.${domain}`))) {
    return true;
  }
  let decodedPath = parsed.pathname || "";
  try {
    decodedPath = decodeURIComponent(decodedPath);
  } catch {
    // Keep raw pathname when percent encoding is malformed.
  }
  return SUPPORTED_ARCHIVE_EXT_RE.test(decodedPath);
}

function truncateClipboardUrl(url, maxLen = 72) {
  const raw = String(url || "");
  if (raw.length <= maxLen) return raw;
  return `${raw.slice(0, maxLen - 3)}...`;
}

function hideClipboardBanner() {
  state.clipboardBannerUrl = "";
  el("clipboard-detect-banner")?.classList.add("hidden");
}

function showClipboardBanner(url) {
  state.clipboardBannerUrl = url;
  const urlEl = el("clipboard-detect-url");
  if (urlEl) {
    urlEl.textContent = truncateClipboardUrl(url);
    urlEl.title = url;
  }
  el("clipboard-detect-banner")?.classList.remove("hidden");
}

function markClipboardUrlHandled(url) {
  const normalized = String(url || "").trim();
  if (normalized) {
    state.seenClipboardUrls.add(normalized);
  }
  if (!normalized || state.clipboardBannerUrl === normalized) {
    hideClipboardBanner();
  }
}

async function detectClipboardDownloadLink() {
  if (!state.autoDetectClipboard) return;
  const raw = (await readSystemClipboardText()).trim();
  if (!isSupportedClipboardDownloadUrl(raw)) {
    return;
  }
  if (state.seenClipboardUrls.has(raw) || state.clipboardBannerUrl === raw) {
    return;
  }
  const alreadyQueued = state.downloadJobs.some(
    (job) => String(job?.originalUrl || "").trim() === raw
  );
  if (alreadyQueued) {
    state.seenClipboardUrls.add(raw);
    return;
  }
  showClipboardBanner(raw);
}

function scheduleClipboardDetection() {
  if (!state.autoDetectClipboard) return;
  if (state.clipboardFocusTimer) {
    clearTimeout(state.clipboardFocusTimer);
  }
  state.clipboardFocusTimer = setTimeout(() => {
    state.clipboardFocusTimer = null;
    detectClipboardDownloadLink();
  }, 160);
}

async function startDownloadFromUrl(urlVal, switchToDownloader = false) {
  const url = String(urlVal || el("input-dl-url")?.value || "").trim();
  if (!isValidHttpUrl(url)) {
    showToast("Enter a valid http:// or https:// URL.", "error");
    return false;
  }
  const mode = el("select-dl-speed-mode")?.value || "unlimited";
  const maxSpeedMbps = mode === "unlimited" ? null : Number(mode);
  try {
    state.downloadJobs = await invokeCommand("enqueue_download", {
      url,
      customSaveDir: state.customSaveDir || null,
      threads: 8,
      maxSpeedMbps
    });
    markClipboardUrlHandled(url);
    if (!urlVal && el("input-dl-url")) el("input-dl-url").value = "";
    renderDownloadJobs();
    updateSidebarTelemetry();
    if (switchToDownloader) switchTab("downloader");
    showToast("Added download to 8-worker queue.", "success");
    return true;
  } catch (err) {
    showToast(String(err), "error");
    return false;
  }
}

function wireEvents() {
  document.querySelectorAll(".sidebar-nav .nav-item").forEach((btn) => {
    btn.addEventListener("click", () => {
      const tab = btn.dataset.tab;
      switchTab(tab);
      if (tab === "library") renderLibraryGrid();
      if (tab === "catalog") {
        syncCatalogFilterControlsToDom();
        if (!state.catalog.loaded && !state.catalog.loading) {
          loadCatalogData({ query: state.catalog.searchQuery, forceRefresh: false });
        } else if (state.catalog.renderedCount === 0 && !state.catalog.loading) {
          renderCatalogView(true);
        } else {
          updateCatalogResultsSummary();
        }
      }
      if (tab === "downloader") renderDownloadJobs();
      if (tab === "sources" || tab === "settings") renderSources();
    });
  });

  el("detail-news-list")?.addEventListener("click", (e) => {
    const linkBtn = e.target.closest("[data-external-url]");
    if (linkBtn) openExternalUrl(linkBtn.dataset.externalUrl);
  });

  el("sidebar-game-list")?.addEventListener("click", (e) => {
    const item = e.target.closest(".sidebar-game-item[data-game-id]");
    if (item) openGameDetail(item.dataset.gameId);
  });

  el("filter-pills")?.addEventListener("click", (e) => {
    const pill = e.target.closest(".filter-pill[data-filter]");
    if (!pill) return;
    state.filter = pill.dataset.filter;
    document.querySelectorAll("#filter-pills .filter-pill").forEach((p) => p.classList.toggle("active", p === pill));
    renderLibraryGrid();
  });

  el("sort-select")?.addEventListener("change", (e) => {
    state.sortBy = e.target.value;
    renderLibraryGrid();
  });

  el("search-input")?.addEventListener("input", (e) => {
    state.searchQuery = e.target.value || "";
    if (state.activeTab === "library") renderLibraryGrid();
    if (state.activeTab === "downloader") renderDownloadJobs();
    if (state.activeTab === "sources") renderSources();
    if (state.activeTab === "catalog") {
      if (el("input-catalog-search")) el("input-catalog-search").value = state.searchQuery;
      handleCatalogSearchInput(state.searchQuery);
    }
  });

  // Search Games (Steam Catalog) Controls
  setupCatalogIntersectionObserver();

  el("input-catalog-search")?.addEventListener("input", (e) => {
    const nextQ = e.target.value || "";
    if (el("search-input") && state.activeTab === "catalog") {
      el("search-input").value = nextQ;
    }
    handleCatalogSearchInput(nextQ);
  });

  el("select-catalog-sort")?.addEventListener("change", (e) => {
    state.catalog.sortBy = e.target.value;
    renderCatalogView(true);
  });

  el("select-catalog-price-tier")?.addEventListener("change", (e) => {
    const tier = e.target.value || "all";
    state.catalog.priceTier = tier;
    state.catalog.paidOnly = tier === "paid";
    state.catalog.minPriceCents = 0;
    renderCatalogView(true);
  });

  el("select-catalog-min-rating")?.addEventListener("change", (e) => {
    state.catalog.minRatingPct = Number(e.target.value || 0);
    renderCatalogView(true);
  });

  el("select-catalog-min-reviews")?.addEventListener("change", (e) => {
    state.catalog.minReviews = Number(e.target.value || 0);
    renderCatalogView(true);
  });

  el("select-catalog-player-mode")?.addEventListener("change", (e) => {
    const mode = e.target.value || "all";
    state.catalog.playerMode = mode;
    state.catalog.singleplayer = mode === "singleplayer";
    state.catalog.coop = mode === "coop";
    renderCatalogView(true);
  });

  el("select-catalog-min-ccu")?.addEventListener("change", (e) => {
    state.catalog.minCcu = Number(e.target.value || 0);
    renderCatalogView(true);
  });

  el("btn-catalog-hide-installed")?.addEventListener("click", () => {
    state.catalog.hideInstalled = !state.catalog.hideInstalled;
    syncCatalogFilterControlsToDom();
    renderCatalogView(true);
  });

  el("btn-toggle-catalog-genres")?.addEventListener("click", () => {
    state.catalog.genresDrawerOpen = !state.catalog.genresDrawerOpen;
    syncCatalogFilterControlsToDom();
    if (state.catalog.genresDrawerOpen) {
      el("input-catalog-tag-filter")?.focus();
    }
  });

  el("input-catalog-tag-filter")?.addEventListener("input", (e) => {
    state.catalog.tagSearch = e.target.value || "";
    syncCatalogFilterControlsToDom();
  });

  el("catalog-genre-groups")?.addEventListener("click", (e) => {
    const pill = e.target.closest(".catalog-tag-pill[data-catalog-genre]");
    if (!pill) return;
    const genre = pill.dataset.catalogGenre;
    if (!genre) return;
    const idx = state.catalog.selectedGenres.indexOf(genre);
    if (idx >= 0) {
      state.catalog.selectedGenres.splice(idx, 1);
    } else {
      state.catalog.selectedGenres.push(genre);
    }
    syncCatalogFilterControlsToDom();
    renderCatalogView(true);
  });

  el("catalog-active-genres-preview")?.addEventListener("click", (e) => {
    const chip = e.target.closest("[data-remove-genre]");
    if (!chip) return;
    const genre = chip.dataset.removeGenre;
    const idx = state.catalog.selectedGenres.indexOf(genre);
    if (idx >= 0) {
      state.catalog.selectedGenres.splice(idx, 1);
      syncCatalogFilterControlsToDom();
      renderCatalogView(true);
    }
  });

  el("btn-catalog-clear-genres")?.addEventListener("click", () => {
    state.catalog.selectedGenres = [];
    state.catalog.tagSearch = "";
    syncCatalogFilterControlsToDom();
    renderCatalogView(true);
  });

  el("btn-catalog-reset-filters")?.addEventListener("click", () => {
    const keepOpen = Boolean(state.catalog.genresDrawerOpen);
    Object.assign(state.catalog, {
      ...DEFAULT_CATALOG_FILTERS,
      selectedGenres: [],
      genresDrawerOpen: keepOpen
    });
    if (el("search-input") && state.activeTab === "catalog") {
      el("search-input").value = "";
      state.searchQuery = "";
    }
    syncCatalogFilterControlsToDom();
    renderCatalogView(true);
  });

  el("btn-catalog-refresh")?.addEventListener("click", () => {
    loadCatalogData({ query: state.catalog.searchQuery, forceRefresh: true });
  });

  el("btn-catalog-retry")?.addEventListener("click", () => {
    loadCatalogData({ query: state.catalog.searchQuery, forceRefresh: true });
  });

  el("catalog-grid")?.addEventListener("click", (e) => {
    const actionBtn = e.target.closest("[data-catalog-action][data-app-id]");
    const card = e.target.closest(".catalog-card[data-app-id]");
    const appId = actionBtn?.dataset.appId || card?.dataset.appId;
    if (!appId) return;

    if (actionBtn?.dataset.catalogAction === "sources") {
      e.stopPropagation();
      const entry = state.catalog.itemsById.get(Number(appId));
      handoffGameToFindGames(entry?.name || "");
      return;
    }

    openCatalogModal(appId);
  });

  el("catalog-grid")?.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      const card = e.target.closest(".catalog-card[data-app-id]");
      if (card) openCatalogModal(card.dataset.appId);
    }
  });

  // Catalog Detail Modal Controls
  const closeCatalogModal = () => {
    state.catalog.modalRequestId += 1;
    el("catalog-detail-modal")?.classList.add("hidden");
  };
  el("btn-close-catalog-modal")?.addEventListener("click", closeCatalogModal);
  el("catalog-detail-modal")?.addEventListener("click", (e) => {
    if (e.target === el("catalog-detail-modal")) closeCatalogModal();
  });

  el("btn-catalog-open-store")?.addEventListener("click", () => {
    const entry = state.catalog.modalGame;
    if (entry?.appId) openExternalUrl(`https://store.steampowered.com/app/${entry.appId}`);
  });

  el("btn-catalog-open-steamdb")?.addEventListener("click", () => {
    const entry = state.catalog.modalGame;
    if (entry?.appId) openExternalUrl(`https://steamdb.info/app/${entry.appId}/`);
  });

  el("btn-catalog-find-sources")?.addEventListener("click", () => {
    const entry = state.catalog.modalGame;
    if (!entry) return;
    closeCatalogModal();
    handoffGameToFindGames(entry.name || "");
  });

  el("btn-catalog-link-exe")?.addEventListener("click", async () => {
    const entry = state.catalog.modalGame;
    if (!entry) return;
    try {
      const exePath = await pickExecutableFile();
      if (!exePath) return;
      const added = await linkLocalGameWithSteamId({
        exePath,
        customTitle: entry.name,
        genre: (entry.genres || [])[0] || "Installed",
        steamAppId: entry.appId
      });
      state.library = await invokeCommand("get_local_library");
      renderSidebarGameList();
      renderLibraryGrid();
      renderCatalogView(true);
      updateSidebarTelemetry();
      closeCatalogModal();
      showToast(`Linked ${added?.title || entry.name} to My Games.`, "success");
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-clear-source-game")?.addEventListener("click", () => {
    state.sourceGameQuery = "";
    state.directoryQuery = "";
    if (el("input-directory-search")) el("input-directory-search").value = "";
    if (el("input-sources-filter")) el("input-sources-filter").value = "";
    renderSources();
  });

  el("btn-clear-completed-dl")?.addEventListener("click", async () => {
    try {
      state.downloadJobs = await clearCompletedDownloads();
      renderDownloadJobs();
      updateSidebarTelemetry();
      showToast("Cleared completed downloads from queue.", "info", 2500);
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("setting-auto-clean-completed")?.addEventListener("change", async (e) => {
    const checked = Boolean(e.target.checked);
    try {
      const updated = await updateLauncherSettings({ autoCleanCompleted: checked });
      state.autoCleanCompleted = Boolean(updated?.autoCleanCompleted ?? checked);
      state.downloadJobs = await invokeCommand("get_download_jobs", { backupJobs: null });
      renderDownloadJobs();
      updateSidebarTelemetry();
      showToast(
        state.autoCleanCompleted ? "Auto-clean completed downloads enabled." : "Auto-clean completed downloads disabled.",
        "info",
        2500
      );
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("setting-auto-detect-clipboard")?.addEventListener("change", async (e) => {
    const checked = Boolean(e.target.checked);
    try {
      const updated = await updateLauncherSettings({ autoDetectClipboard: checked });
      state.autoDetectClipboard = Boolean(updated?.autoDetectClipboard ?? checked);
      if (!state.autoDetectClipboard) {
        hideClipboardBanner();
      } else {
        scheduleClipboardDetection();
      }
      showToast(
        state.autoDetectClipboard
          ? "Smart clipboard link auto-detection enabled."
          : "Smart clipboard link auto-detection disabled.",
        "info",
        2400
      );
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("setting-nvme-prefetch")?.addEventListener("change", async (e) => {
    const checked = Boolean(e.target.checked);
    try {
      const updated = await updateLauncherSettings({ nvmePrefetchEnabled: checked });
      state.nvmePrefetchEnabled = Boolean(updated?.nvmePrefetchEnabled ?? checked);
      showToast(
        state.nvmePrefetchEnabled
          ? "NVMe pre-launch Standby RAM cache warmer enabled."
          : "NVMe pre-launch cache warmer disabled globally.",
        "info",
        2400
      );
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("setting-swarm-enabled")?.addEventListener("change", async (e) => {
    const checked = Boolean(e.target.checked);
    try {
      const updated = await updateLauncherSettings({ swarmEnabled: checked });
      state.swarmEnabled = Boolean(updated?.swarmEnabled ?? checked);
      await refreshSwarmAndMmapTelemetry();
      showToast(
        state.swarmEnabled
          ? "Duke Swarm LAN multicast & mesh discovery started."
          : "Duke Swarm discovery paused.",
        "info",
        2400
      );
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-save-swarm-node")?.addEventListener("click", async () => {
    const nodeName = (el("setting-swarm-node-name")?.value || "").trim();
    try {
      const updated = await updateLauncherSettings({ swarmNodeName: nodeName });
      state.swarmNodeName = String(updated?.swarmNodeName || nodeName);
      if (el("setting-swarm-node-name")) el("setting-swarm-node-name").value = state.swarmNodeName;
      showToast("Swarm node name updated.", "success");
      await refreshSwarmAndMmapTelemetry();
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-save-swarm-peers")?.addEventListener("click", async () => {
    const raw = el("setting-swarm-peers")?.value || "";
    const peers = raw
      .split(/[\s,;]+/)
      .map((s) => s.trim())
      .filter(Boolean);
    try {
      const updated = await updateLauncherSettings({ swarmPeers: peers });
      state.swarmPeers = Array.isArray(updated?.swarmPeers) ? updated.swarmPeers : peers;
      if (el("setting-swarm-peers")) el("setting-swarm-peers").value = state.swarmPeers.join(", ");
      const snapshot = await probeSwarmPeers();
      renderSwarmSettingsPanel(snapshot);
      showToast(`Saved ${state.swarmPeers.length} peer address(es) & probed network.`, "success");
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-swarm-probe")?.addEventListener("click", async () => {
    showToast("Probing LAN multicast & Tailscale peers...", "info", 1800);
    try {
      const snapshot = await probeSwarmPeers();
      renderSwarmSettingsPanel(snapshot);
      const count = Array.isArray(snapshot?.peers) ? snapshot.peers.length : 0;
      showToast(`Swarm probe complete: ${count} online peer${count === 1 ? "" : "s"}.`, "success");
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("swarm-peers-list")?.addEventListener("click", async (e) => {
    const btn = e.target.closest("[data-swarm-beam-peer][data-swarm-beam-key]");
    if (!btn) return;
    const peerAddr = btn.dataset.swarmBeamPeer;
    const gameKey = btn.dataset.swarmBeamKey;
    const destParentDir = state.customSaveDir || (await pickDownloadFolder());
    if (!destParentDir) {
      showToast("Select a destination folder to beam this game.", "info");
      return;
    }
    btn.disabled = true;
    showToast(`Beaming ${gameKey} over LAN via FastCDC...`, "info", 3200);
    try {
      const report = await beamGameFromPeer({
        peerAddr,
        gameKey,
        destParentDir,
        registerInLibrary: true
      });
      state.library = await invokeCommand("get_local_library");
      renderSidebarGameList();
      renderLibraryGrid();
      updateSidebarTelemetry();
      showToast(
        `Beamed ${report?.gameTitle || gameKey} at ${Number(report?.throughputMbps || 0).toFixed(1)} Mbps.`,
        "success",
        4000
      );
    } catch (err) {
      showToast(String(err), "error");
    } finally {
      btn.disabled = false;
    }
  });

  el("btn-save-sgdb-key")?.addEventListener("click", async () => {
    const key = (el("setting-steamgriddb-key")?.value || "").trim();
    try {
      const updated = await updateLauncherSettings({ steamgriddbApiKey: key });
      state.steamgriddbApiKey = updated?.steamgriddbApiKey ?? key;
      showToast(key ? "SteamGridDB API key saved." : "SteamGridDB API key cleared.", "success");
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-save-suspend-hotkey")?.addEventListener("click", async () => {
    const hotkey = (el("setting-suspend-hotkey")?.value || "").trim() || "Ctrl+Alt+P";
    try {
      const updated = await updateLauncherSettings({ suspendHotkey: hotkey });
      state.suspendHotkey = updated?.suspendHotkey ?? hotkey;
      if (el("setting-suspend-hotkey")) el("setting-suspend-hotkey").value = state.suspendHotkey;
      showToast(`Suspend hotkey active: ${state.suspendHotkey}`, "success");
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("setting-couch-mode")?.addEventListener("change", (e) => {
    applyCouchMode(Boolean(e.target.checked), true);
  });

  el("btn-settings-import-libraries")?.addEventListener("click", runImportLibrariesFlow);

  el("btn-settings-sync-ludusavi")?.addEventListener("click", async () => {
    showToast("Syncing Ludusavi save-location manifest over DoH...", "info");
    try {
      const status = await syncLudusaviManifest(true);
      showToast(
        `Ludusavi manifest synced (${Number(status?.entryCount || 0).toLocaleString()} game schemas).`,
        "success"
      );
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("settings-sources-list")?.addEventListener("change", async (e) => {
    const toggle = e.target.closest("input[data-settings-source-toggle]");
    if (!toggle) return;
    try {
      await toggleImportedSource(toggle.dataset.settingsSourceToggle, Boolean(toggle.checked));
      await refreshSourcesView();
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("settings-sources-list")?.addEventListener("click", async (e) => {
    const btn = e.target.closest("[data-settings-source-action='refresh'][data-source-id]");
    if (!btn) return;
    try {
      const updated = await refreshImportedSource(btn.dataset.sourceId);
      await refreshSourcesView();
      showToast(`Refreshed ${updated?.name || "source"}.`, "success");
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("library-grid")?.addEventListener("click", async (e) => {
    const btn = e.target.closest("[data-action][data-game-id]");
    const card = e.target.closest(".game-card[data-game-id]");
    const gameId = btn?.dataset.gameId || card?.dataset.gameId;
    if (!gameId) return;
    const game = state.library.find((g) => g.id === gameId);
    if (!game) return;

    if (!btn) {
      openGameDetailView(gameId);
      return;
    }

    e.stopPropagation();
    const action = btn.dataset.action;
    if (action === "play") {
      await launchGameById(game);
    } else if (action === "details") {
      openDetailsModal(gameId);
    } else if (action === "folder") {
      await invokeCommand("open_game_folder", { path: game.installDir || game.exePath });
    } else if (action === "fav") {
      state.library = await invokeCommand("toggle_favorite_game", { id: game.id });
      renderSidebarGameList();
      renderLibraryGrid();
    }
  });

  el("btn-quick-add-exe")?.addEventListener("click", addLocalGameFlow);
  el("btn-empty-add-game")?.addEventListener("click", addLocalGameFlow);
  el("btn-import-libraries")?.addEventListener("click", runImportLibrariesFlow);
  el("btn-empty-import-libraries")?.addEventListener("click", runImportLibrariesFlow);
  el("titlebar-suspend-pill")?.addEventListener("click", (e) => {
    const gid = e.currentTarget?.dataset?.gameId || null;
    toggleGameSuspendFlow(gid, null);
  });

  el("btn-auto-covers")?.addEventListener("click", async () => {
    showToast("Refreshing covers...", "info");
    try {
      state.library = await invokeCommand("auto_fix_all_covers");
      invalidateSidebarIcons();
      renderSidebarGameList();
      renderLibraryGrid();
      showToast("Cover cache updated.", "success");
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  const scanFileFlow = async (targetPath) => {
    const file = targetPath || (await pickAnyFileForScan());
    if (!file) return;
    showToast("Scanning file for threats...", "info");
    try {
      const report = await invokeCommand("scan_file_security", { path: file });
      state.reports.unshift(report);
      renderSecurityReports();
      const entropyHint =
        typeof report.shannonEntropy === "number"
          ? ` (Entropy ${report.shannonEntropy.toFixed(2)} b/B)`
          : "";
      showToast(`${report.fileName}: ${report.verdict}${entropyHint}`, String(report.verdict).includes("DANGER") ? "error" : "success");
    } catch (err) {
      showToast(String(err), "error");
    }
  };

  el("btn-scan-local-file")?.addEventListener("click", () => scanFileFlow(null));
  el("btn-scan-any-btn")?.addEventListener("click", () => scanFileFlow(null));

  el("btn-trim-ram-sidebar")?.addEventListener("click", async () => {
    try {
      const tele = await invokeCommand("trim_working_set");
      updateSidebarTelemetry(tele);
      showToast(`Working set trimmed to ${Number(tele?.workingSetMb || 0).toFixed(1)} MB.`, "success");
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  // Detail View Controls
  el("btn-detail-back")?.addEventListener("click", () => {
    state.selectedDetailId = null;
    switchTab("library", false);
    renderSidebarGameList();
    renderLibraryGrid();
  });

  const withSelectedGame = (cb) => {
    const g = state.library.find((x) => x.id === state.selectedDetailId);
    if (g) cb(g);
  };

  el("btn-detail-play")?.addEventListener("click", () => withSelectedGame(launchGameById));
  el("btn-detail-suspend")?.addEventListener("click", () =>
    withSelectedGame((g) => toggleGameSuspendFlow(g.id, null))
  );
  el("btn-detail-folder")?.addEventListener("click", () =>
    withSelectedGame((g) => invokeCommand("open_game_folder", { path: g.installDir || g.exePath }))
  );
  el("btn-detail-pin")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      state.library = await invokeCommand("toggle_favorite_game", { id: g.id });
      renderSidebarGameList();
      renderLibraryGrid();
      openGameDetailView(g.id);
    })
  );
  el("btn-detail-snapshots")?.addEventListener("click", () =>
    withSelectedGame((g) => openSnapshotsModal(g))
  );
  el("btn-detail-preflight")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      try {
        const report = await preflightCheckExe(g.exePath);
        openPreflightModal(g, report, true);
      } catch (err) {
        showToast(String(err), "error");
      }
    })
  );
  el("btn-detail-scan")?.addEventListener("click", () => withSelectedGame((g) => scanFileFlow(g.exePath)));
  el("btn-detail-compress")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      showToast("Compressing game folder with XPRESS8K...", "info");
      const res = await compressGameFolder(g.id);
      state.library = await invokeCommand("get_local_library");
      openGameDetailView(g.id);
      updateSidebarTelemetry();
      showToast(res?.message || "Compression finished.", "success");
    })
  );
  el("btn-detail-backup-saves")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      try {
        const path = await invokeCommand("backup_game_saves", {
          gameId: g.id,
          title: g.title,
          installDir: g.installDir || null,
          exePath: g.exePath
        });
        showToast(`Saved rolling backup archive: ${path}`, "success");
      } catch (err) {
        showToast(String(err), "error");
      }
    })
  );
  el("btn-detail-restore-saves")?.addEventListener("click", () =>
    withSelectedGame((g) => openRestoreSaveModal(g))
  );
  el("btn-detail-rescan-saves")?.addEventListener("click", () =>
    withSelectedGame((g) => loadDetailSavePaths(g, state.activeDetailRequestId, true))
  );
  el("btn-detail-mesh-sync")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      showToast(`Syncing ${g.title} saves across Duke Swarm peers...`, "info", 2200);
      try {
        const res = await syncGameSavesMesh(g.id);
        showToast(res?.summary || "Mesh save sync finished.", "success", 3500);
      } catch (err) {
        showToast(String(err), "error");
      }
    })
  );
  el("detail-toggle-sandbox")?.addEventListener("change", (e) =>
    withSelectedGame(async (g) => {
      try {
        const enabled = Boolean(e.target.checked);
        state.library = await setGameLaunchProfile({ gameId: g.id, sandboxEnabled: enabled });
        showToast(
          enabled
            ? "Portable .duke-profile sandbox enabled for this game."
            : "Portable profile sandbox disabled.",
          "success"
        );
      } catch (err) {
        showToast(String(err), "error");
      }
    })
  );
  el("detail-prefetch-mode")?.addEventListener("change", (e) =>
    withSelectedGame(async (g) => {
      try {
        const mode = String(e.target.value || "inherit");
        state.library = await setGameLaunchProfile({
          gameId: g.id,
          nvmePrefetchOverride: mode
        });
        showToast(`NVMe Pre-Warm mode set to "${mode}".`, "success");
      } catch (err) {
        showToast(String(err), "error");
      }
    })
  );
  el("detail-save-paths-list")?.addEventListener("click", (e) => {
    const btn = e.target.closest("[data-open-save-path]");
    if (btn) {
      invokeCommand("open_game_folder", { path: btn.dataset.openSavePath }).catch((err) =>
        showToast(String(err), "error")
      );
    }
  });

  const changeExeFlow = async (game) => {
    const chosen = await pickExecutableFile();
    if (!chosen) return;
    state.library = await invokeCommand("change_game_exe", { gameId: game.id, newExePath: chosen });
    invalidateSidebarIcons([game.id]);
    renderSidebarGameList();
    openGameDetailView(game.id);
    showToast("Executable target updated.", "success");
  };
  el("btn-detail-change-exe")?.addEventListener("click", () => withSelectedGame(changeExeFlow));
  el("btn-detail-inline-exe")?.addEventListener("click", () => withSelectedGame(changeExeFlow));

  el("btn-detail-cover")?.addEventListener("click", () =>
    withSelectedGame((g) => {
      openDetailsModal(g.id);
    })
  );

  el("btn-detail-save-args")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      const val = el("detail-launch-args")?.value || "";
      state.library = await setGameLaunchArgs(g.id, val);
      showToast("Launch arguments saved.", "success");
    })
  );

  el("btn-detail-remove")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      state.library = await invokeCommand("remove_local_game", { id: g.id });
      delete state.sidebarIcons[g.id];
      state.sidebarIconsAttempted.delete(g.id);
      state.selectedDetailId = null;
      switchTab("library");
      renderSidebarGameList();
      renderLibraryGrid();
      updateSidebarTelemetry();
      showToast(`Removed ${g.title} from list.`, "info");
    })
  );

  const openSteamLink = (builder) =>
    withSelectedGame((g) => {
      const aid = g.steamAppId || g.appId;
      openExternalUrl(builder(aid, g.title));
    });
  el("btn-subnav-store")?.addEventListener("click", () =>
    openSteamLink((aid, t) =>
      aid ? `https://store.steampowered.com/app/${aid}` : `https://store.steampowered.com/search/?term=${encodeURIComponent(t)}`
    )
  );
  el("btn-subnav-steamdb")?.addEventListener("click", () =>
    openSteamLink((aid, t) =>
      aid ? `https://steamdb.info/app/${aid}/` : `https://steamdb.info/search/?a=app&q=${encodeURIComponent(t)}`
    )
  );
  el("btn-subnav-pcgw")?.addEventListener("click", () =>
    openSteamLink((_aid, t) => `https://www.pcgamingwiki.com/w/index.php?search=${encodeURIComponent(t)}`)
  );
  el("btn-subnav-hub")?.addEventListener("click", () =>
    openSteamLink((aid) => (aid ? `https://steamcommunity.com/app/${aid}` : "https://steamcommunity.com/"))
  );
  el("btn-subnav-discussions")?.addEventListener("click", () =>
    openSteamLink((aid) => (aid ? `https://steamcommunity.com/app/${aid}/discussions/` : "https://steamcommunity.com/"))
  );

  // Downloader Controls
  if (el("input-dl-dir") && state.customSaveDir) el("input-dl-dir").value = state.customSaveDir;

  el("btn-browse-dl-dir")?.addEventListener("click", async () => {
    const dir = await pickDownloadFolder();
    if (dir) {
      state.customSaveDir = dir;
      localStorage.setItem("duke_launcher_dl_dir", dir);
      if (el("input-dl-dir")) el("input-dl-dir").value = dir;
    }
  });

  el("btn-start-dl")?.addEventListener("click", () => startDownloadFromUrl());
  el("btn-paste-and-dl")?.addEventListener("click", async () => {
    const clip = (await readSystemClipboardText()).trim();
    if (!isValidHttpUrl(clip)) {
      showToast("Clipboard does not contain a valid http:// or https:// download URL.", "error");
      return;
    }
    if (el("input-dl-url")) el("input-dl-url").value = clip;
    await startDownloadFromUrl(clip, false);
  });

  el("btn-clipboard-download")?.addEventListener("click", async () => {
    const url = state.clipboardBannerUrl;
    markClipboardUrlHandled(url);
    if (!url) return;
    if (el("input-dl-url")) el("input-dl-url").value = url;
    await startDownloadFromUrl(url, true);
  });

  el("btn-clipboard-dismiss")?.addEventListener("click", () => {
    markClipboardUrlHandled(state.clipboardBannerUrl);
  });

  const copyBookmarklet = async () => {
    const copied = await copyTextToClipboard(BOOKMARKLET_HREF);
    showToast(
      copied
        ? "Bookmarklet copied — drag 'Send to Duke Launcher' or paste as a bookmark URL."
        : "Copy unavailable — drag 'Send to Duke Launcher' to your browser bookmarks bar.",
      copied ? "success" : "info",
      3400
    );
  };
  ["bookmarklet-link-dl", "bookmarklet-link-settings"].forEach((id) => {
    el(id)?.addEventListener("click", async (e) => {
      e.preventDefault();
      await copyBookmarklet();
    });
  });
  ["btn-copy-bookmarklet-dl", "btn-copy-bookmarklet-settings"].forEach((id) => {
    el(id)?.addEventListener("click", copyBookmarklet);
  });

  window.addEventListener("focus", scheduleClipboardDetection);

  el("select-dl-speed-mode")?.addEventListener("change", async (e) => {
    const val = e.target.value;
    const maxSpeedMbps = val === "unlimited" ? null : Number(val);
    await invokeCommand("set_speed_limit", { maxSpeedMbps }).catch(() => {});
  });

  el("btn-pause-all")?.addEventListener("click", async () => {
    for (const j of state.downloadJobs) {
      if (j.status === "DOWNLOADING" || j.status === "RESOLVING") {
        state.downloadJobs = await invokeCommand("pause_download", { downloadId: j.downloadId });
      }
    }
    renderDownloadJobs();
  });

  el("btn-resume-all")?.addEventListener("click", async () => {
    const mode = el("select-dl-speed-mode")?.value || "unlimited";
    const maxSpeedMbps = mode === "unlimited" ? null : Number(mode);
    for (const j of state.downloadJobs) {
      if (j.status === "PAUSED") {
        state.downloadJobs = await invokeCommand("resume_download", { downloadId: j.downloadId, maxSpeedMbps });
      }
    }
    renderDownloadJobs();
  });

  el("dl-jobs-list")?.addEventListener("click", async (e) => {
    const btn = e.target.closest("[data-dl-action][data-dl-id]");
    if (!btn) return;
    const act = btn.dataset.dlAction;
    const id = btn.dataset.dlId;
    const job = state.downloadJobs.find((j) => j.downloadId === id);

    try {
      if (act === "pause") {
        state.downloadJobs = await invokeCommand("pause_download", { downloadId: id });
      } else if (act === "resume") {
        const mode = el("select-dl-speed-mode")?.value || "unlimited";
        state.downloadJobs = await invokeCommand("resume_download", {
          downloadId: id,
          maxSpeedMbps: mode === "unlimited" ? null : Number(mode)
        });
      } else if (act === "install") {
        showToast("Extracting archive & registering game...", "info");
        await invokeCommand("install_downloaded_job", { downloadId: id });
        state.library = await invokeCommand("get_local_library");
        renderSidebarGameList();
        renderLibraryGrid();
      } else if (act === "folder" && job) {
        await invokeCommand("open_game_folder", { path: job.finalPath || job.saveDir });
      } else if (act === "remove") {
        state.downloadJobs = await invokeCommand("remove_download_job", { downloadId: id, deleteFile: false });
      }
      renderDownloadJobs();
      updateSidebarTelemetry();
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("dl-reports-list")?.addEventListener("click", async (e) => {
    const btn = e.target.closest("[data-rep-action][data-rep-path]");
    if (!btn) return;
    const path = btn.dataset.repPath;
    if (btn.dataset.repAction === "folder") {
      await invokeCommand("open_game_folder", { path });
    } else if (btn.dataset.repAction === "delete") {
      await invokeCommand("delete_flagged_file", { path });
      state.reports = state.reports.filter((r) => r.filePath !== path);
      renderSecurityReports();
      showToast("Flagged file permanently removed.", "info");
    }
  });

  el("btn-clear-dl-history")?.addEventListener("click", () => {
    state.reports = [];
    renderSecurityReports();
  });

  el("btn-open-host-browser")?.addEventListener("click", () => {
    switchTab("sources");
    renderSources();
  });

  // Find Games: Directory Search, Direct URL Download & Custom Site Modals
  let directorySearchTimer = null;
  el("input-directory-search")?.addEventListener("input", (e) => {
    const val = String(e.target.value || "").trim();
    state.directoryQuery = val;
    state.sourceGameQuery = val;
    if (el("input-sources-filter")) el("input-sources-filter").value = val;
    renderDirectoryGrid();
    if (directorySearchTimer) clearTimeout(directorySearchTimer);
    directorySearchTimer = setTimeout(() => {
      if ((state.importedSources || []).length > 0) {
        refreshSourcesView();
      }
    }, 160);
  });

  const launchFirstVisibleDirectory = async () => {
    const firstSite = getFilteredDirectorySites()[0];
    if (!firstSite) {
      showToast("No directory cards match the current category.", "info");
      return;
    }
    await launchDirectorySite(firstSite);
  };

  el("input-directory-search")?.addEventListener("keydown", async (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      await launchFirstVisibleDirectory();
    }
  });

  el("btn-directory-quick-launch")?.addEventListener("click", launchFirstVisibleDirectory);

  const handleDirectFindUrlDownload = async () => {
    const input = el("input-find-direct-url");
    const url = (input?.value || "").trim();
    if (!isValidHttpUrl(url)) {
      showToast("Paste a valid http:// or https:// direct file link first.", "error");
      return;
    }
    const ok = await startDownloadFromUrl(url, true);
    if (ok && input) input.value = "";
  };

  el("btn-find-direct-download")?.addEventListener("click", handleDirectFindUrlDownload);
  el("input-find-direct-url")?.addEventListener("keydown", async (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      await handleDirectFindUrlDownload();
    }
  });

  el("directory-category-pills")?.addEventListener("click", (e) => {
    const pill = e.target.closest(".filter-pill[data-dir-category]");
    if (!pill) return;
    state.directoryCategory = pill.dataset.dirCategory || "all";
    renderDirectoryGrid();
  });

  el("directory-grid")?.addEventListener("click", async (e) => {
    const removeBtn = e.target.closest('[data-directory-action="remove"][data-site-id]');
    if (removeBtn) {
      e.stopPropagation();
      const siteId = removeBtn.dataset.siteId;
      try {
        state.searchSites = await removeSearchSite(siteId);
        renderDirectoryGrid();
        showToast("Removed directory card from search_sites.json.", "info", 2200);
      } catch (err) {
        showToast(String(err), "error");
      }
      return;
    }

    const card = e.target.closest(".directory-card[data-site-id]");
    if (!card) return;
    const site = (state.searchSites || []).find((s) => s.id === card.dataset.siteId);
    if (site) await launchDirectorySite(site);
  });

  el("directory-grid")?.addEventListener("keydown", async (e) => {
    if (e.key !== "Enter") return;
    const card = e.target.closest(".directory-card[data-site-id]");
    if (!card || e.target.closest('[data-directory-action="remove"]')) return;
    e.preventDefault();
    const site = (state.searchSites || []).find((s) => s.id === card.dataset.siteId);
    if (site) await launchDirectorySite(site);
  });

  el("btn-toggle-json-feeds")?.addEventListener("click", () => {
    setJsonFeedsExpanded(!state.jsonFeedsExpanded);
  });

  el("btn-open-add-site-modal")?.addEventListener("click", openAddSiteModal);
  el("btn-close-add-site-modal")?.addEventListener("click", closeAddSiteModal);
  el("btn-cancel-add-site")?.addEventListener("click", closeAddSiteModal);
  el("add-site-modal")?.addEventListener("click", (e) => {
    if (e.target === el("add-site-modal")) closeAddSiteModal();
  });

  el("btn-save-add-site")?.addEventListener("click", async () => {
    const name = (el("input-site-name")?.value || "").trim();
    const searchUrl = (el("input-site-url")?.value || "").trim();
    const tag = (el("input-site-tag")?.value || "").trim() || "Custom";
    const note = (el("input-site-note")?.value || "").trim();
    if (!name || !searchUrl) {
      showToast("Enter both a Site Name and Search URL.", "error");
      return;
    }
    try {
      state.searchSites = await addSearchSite({ name, searchUrl, tag, note });
      closeAddSiteModal();
      renderDirectoryGrid();
      showToast(`Added "${name}" to Game Directories.`, "success");
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-open-bulk-import-modal")?.addEventListener("click", openBulkImportModal);
  el("btn-close-bulk-import-modal")?.addEventListener("click", closeBulkImportModal);
  el("btn-cancel-bulk-import")?.addEventListener("click", closeBulkImportModal);
  el("bulk-import-modal")?.addEventListener("click", (e) => {
    if (e.target === el("bulk-import-modal")) closeBulkImportModal();
  });

  el("btn-submit-bulk-import")?.addEventListener("click", async () => {
    const rawInput = (el("input-bulk-sites")?.value || "").trim();
    if (!rawInput) {
      showToast("Paste lines or a JSON array of sites first.", "error");
      return;
    }
    try {
      const res = await bulkImportSearchSites(rawInput);
      if (Array.isArray(res?.sites)) {
        state.searchSites = res.sites;
      }
      closeBulkImportModal();
      renderDirectoryGrid();
      const added = Number(res?.addedCount || 0);
      const skipped = Number(res?.skippedCount || 0);
      const msg = skipped > 0
        ? `Imported ${added} directory site(s) (${skipped} duplicate skipped).`
        : `Imported ${added} directory site(s) into search_sites.json.`;
      showToast(msg, added > 0 ? "success" : "info", 3200);
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-reset-default-sites")?.addEventListener("click", async () => {
    try {
      state.searchSites = await resetDefaultSearchSites();
      state.directoryCategory = "all";
      renderDirectoryGrid();
      showToast("Restored all 6 default game directories.", "info");
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  // Community Hydra JSON Sources & Themes
  el("btn-import-source-url")?.addEventListener("click", async () => {
    const url = (el("input-source-url")?.value || "").trim();
    if (!isValidHttpUrl(url)) {
      showToast("Enter a valid http:// or https:// Hydra JSON manifest URL.", "error");
      return;
    }
    showToast("Importing Hydra JSON feed over DoH...", "info", 2400);
    try {
      const added = await importSourceUrl(url);
      if (el("input-source-url")) el("input-source-url").value = "";
      await refreshSourcesView();
      showToast(`Imported "${added?.name || "Source"}" (${Number(added?.itemCount || 0)} releases).`, "success");
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-import-source-file")?.addEventListener("click", async () => {
    try {
      const filePath = await pickJsonSourceFile();
      if (!filePath) return;
      const added = await importSourceFile(filePath);
      await refreshSourcesView();
      showToast(`Loaded "${added?.name || "Source"}" (${Number(added?.itemCount || 0)} releases).`, "success");
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-refresh-all-sources")?.addEventListener("click", async () => {
    showToast("Refreshing imported JSON source feeds...", "info", 2200);
    try {
      await refreshAllImportedSources();
      await refreshSourcesView();
      showToast("All enabled remote JSON sources refreshed.", "success");
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  let sourcesFilterTimer = null;
  el("input-sources-filter")?.addEventListener("input", () => {
    if (sourcesFilterTimer) clearTimeout(sourcesFilterTimer);
    sourcesFilterTimer = setTimeout(() => {
      refreshSourcesView();
    }, 160);
  });

  el("sources-feed-list")?.addEventListener("click", async (e) => {
    const btn = e.target.closest("[data-source-action][data-source-id]");
    if (!btn) return;
    const action = btn.dataset.sourceAction;
    const sourceId = btn.dataset.sourceId;
    try {
      if (action === "filter") {
        state.selectedSourceId = state.selectedSourceId === sourceId ? null : sourceId;
        await refreshSourcesView();
      } else if (action === "toggle") {
        const nextEnabled = btn.dataset.enabled !== "1";
        await toggleImportedSource(sourceId, nextEnabled);
        await refreshSourcesView();
      } else if (action === "refresh") {
        const updated = await refreshImportedSource(sourceId);
        await refreshSourcesView();
        showToast(`Synced "${updated?.name || "Source"}".`, "success");
      } else if (action === "remove") {
        await removeImportedSource(sourceId);
        if (state.selectedSourceId === sourceId) state.selectedSourceId = null;
        await refreshSourcesView();
        showToast("Removed JSON source feed.", "info");
      }
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("sources-grid")?.addEventListener("click", async (e) => {
    const dlBtn = e.target.closest("[data-enqueue-uri]");
    if (dlBtn) {
      const uri = dlBtn.dataset.enqueueUri || "";
      if (isValidHttpUrl(uri)) {
        await startDownloadFromUrl(uri, true);
      } else if (uri) {
        openExternalUrl(uri);
      }
      return;
    }
    const openBtn = e.target.closest("[data-open-url]");
    if (openBtn) openExternalUrl(openBtn.dataset.openUrl);
  });

  el("theme-grid")?.addEventListener("click", (e) => {
    const card = e.target.closest(".theme-option-card[data-theme-id]");
    if (!card) return;
    applyTheme(card.dataset.themeId);
    renderThemes();
    showToast("Theme updated.", "info");
  });

  // Details & Cover Modal Events (Steam CDN & SteamGridDB + Custom .ICO)
  el("btn-close-details-modal")?.addEventListener("click", () => el("details-modal")?.classList.add("hidden"));
  el("details-modal")?.addEventListener("click", (e) => {
    if (e.target === el("details-modal")) el("details-modal")?.classList.add("hidden");
  });

  el("cover-modal-tabs")?.addEventListener("click", (e) => {
    const btn = e.target.closest(".pill[data-cover-tab]");
    if (!btn) return;
    const tab = btn.dataset.coverTab;
    switchCoverModalTab(tab);
    if (tab === "sgdb" && !state.sgdbResults && state.steamgriddbApiKey) {
      runSgdbSearch();
    }
  });

  el("btn-apply-cover-search")?.addEventListener("click", async () => {
    if (!state.modalGameId) return;
    const q = el("input-cover-query")?.value || "";
    try {
      state.library = await invokeCommand("find_and_apply_cover", { id: state.modalGameId, customQuery: q });
      invalidateSidebarIcons([state.modalGameId]);
      renderSidebarGameList();
      renderLibraryGrid();
      if (state.selectedDetailId === state.modalGameId) {
        openGameDetailView(state.modalGameId);
      }
      showToast("Updated cover artwork from Steam CDN.", "success");
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-search-sgdb")?.addEventListener("click", runSgdbSearch);
  el("input-sgdb-query")?.addEventListener("keydown", (e) => {
    if (e.key === "Enter") runSgdbSearch();
  });

  el("sgdb-results-container")?.addEventListener("click", (e) => {
    const card = e.target.closest(".sgdb-thumb-card[data-sgdb-kind][data-sgdb-url]");
    if (!card) return;
    const kind = card.dataset.sgdbKind;
    const url = card.dataset.sgdbUrl;
    if (kind === "cover") {
      state.sgdbSelection.coverUrl = state.sgdbSelection.coverUrl === url ? null : url;
    } else if (kind === "hero") {
      state.sgdbSelection.heroUrl = state.sgdbSelection.heroUrl === url ? null : url;
    } else if (kind === "icon") {
      state.sgdbSelection.iconUrl = state.sgdbSelection.iconUrl === url ? null : url;
    }
    renderSgdbResults();
  });

  el("btn-apply-sgdb-artwork")?.addEventListener("click", async () => {
    if (!state.modalGameId) return;
    const { coverUrl, heroUrl, iconUrl } = state.sgdbSelection;
    if (!coverUrl && !heroUrl && !iconUrl) {
      showToast("Select at least one grid, hero, or icon image first.", "info");
      return;
    }
    showToast("Downloading and applying custom SteamGridDB artwork...", "info", 2200);
    try {
      const updated = await applyCustomArtwork({
        gameId: state.modalGameId,
        coverUrl,
        heroUrl,
        iconUrl
      });
      if (Array.isArray(updated)) {
        state.library = updated;
      } else if (updated && updated.id) {
        const idx = state.library.findIndex((g) => g.id === updated.id);
        if (idx >= 0) state.library[idx] = updated;
      }
      invalidateSidebarIcons([state.modalGameId]);
      renderSidebarGameList();
      renderLibraryGrid();
      if (state.selectedDetailId === state.modalGameId) {
        openGameDetailView(state.modalGameId);
      }
      showToast("Applied custom SteamGridDB artwork.", "success");
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-create-desktop-shortcut")?.addEventListener("click", async () => {
    if (!state.modalGameId) return;
    try {
      const lnkPath = await createDesktopShortcutWithIcon(state.modalGameId);
      showToast(`Desktop shortcut created: ${lnkPath}`, "success");
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("details-actions")?.addEventListener("click", async (e) => {
    const btn = e.target.closest("[data-modal-action]");
    const game = state.library.find((g) => g.id === state.modalGameId);
    if (!btn || !game) return;
    const act = btn.dataset.modalAction;
    el("details-modal")?.classList.add("hidden");
    if (act === "play") await launchGameById(game);
    else if (act === "folder") await invokeCommand("open_game_folder", { path: game.installDir || game.exePath });
    else if (act === "change-exe") await changeExeFlow(game);
    else if (act === "remove") {
      state.library = await invokeCommand("remove_local_game", { id: game.id });
      delete state.sidebarIcons[game.id];
      state.sidebarIconsAttempted.delete(game.id);
      renderSidebarGameList();
      renderLibraryGrid();
      updateSidebarTelemetry();
    }
  });

  // Restore Save Modal Events
  const closeRestoreModal = () => el("restore-save-modal")?.classList.add("hidden");
  el("btn-close-restore-modal")?.addEventListener("click", closeRestoreModal);
  el("btn-cancel-restore-save")?.addEventListener("click", closeRestoreModal);
  el("restore-save-modal")?.addEventListener("click", (e) => {
    if (e.target === el("restore-save-modal")) closeRestoreModal();
  });

  el("btn-confirm-restore-save")?.addEventListener("click", async () => {
    const gid = state.restoreSaveState.gameId;
    const backupPath = el("restore-snapshot-select")?.value || "";
    if (!gid || !backupPath) {
      showToast("Select a backup snapshot to restore.", "error");
      return;
    }
    const btn = el("btn-confirm-restore-save");
    if (btn) btn.disabled = true;
    try {
      const msg = await restoreGameSave(gid, backupPath);
      closeRestoreModal();
      showToast(msg || "Game save snapshot restored successfully.", "success");
      const game = state.library.find((g) => g.id === gid);
      if (game && state.selectedDetailId === gid) {
        loadDetailSavePaths(game, state.activeDetailRequestId, false);
      }
    } catch (err) {
      showToast(String(err), "error");
    } finally {
      if (btn) btn.disabled = false;
    }
  });

  // Snapshots Modal Events (BLAKE3 + CreateHardLinkW CAS)
  const closeSnapshotsModal = () => el("snapshots-modal")?.classList.add("hidden");
  el("btn-close-snapshots-modal")?.addEventListener("click", closeSnapshotsModal);
  el("btn-cancel-snapshots-modal")?.addEventListener("click", closeSnapshotsModal);
  el("snapshots-modal")?.addEventListener("click", (e) => {
    if (e.target === el("snapshots-modal")) closeSnapshotsModal();
  });

  el("btn-create-snapshot")?.addEventListener("click", async () => {
    const gid = state.snapshotModalState.gameId;
    if (!gid) return;
    const label = el("input-snapshot-label")?.value || "";
    const btn = el("btn-create-snapshot");
    if (btn) btn.disabled = true;
    showToast("Creating copy-on-write BLAKE3 snapshot...", "info", 2200);
    try {
      const created = await createGameSnapshot(gid, label);
      state.snapshotModalState.snapshots = await listGameSnapshots(gid);
      if (el("input-snapshot-label")) el("input-snapshot-label").value = "";
      renderSnapshotsModalList();
      showToast(
        `Snapshot "${created?.label || created?.id}" ready in ${Number(created?.elapsedMs || 0)} ms (${Number(created?.deduplicatedFiles || 0)} deduped).`,
        "success",
        3500
      );
    } catch (err) {
      showToast(String(err), "error");
    } finally {
      if (btn) btn.disabled = false;
    }
  });

  el("snapshots-list")?.addEventListener("click", async (e) => {
    const gid = state.snapshotModalState.gameId;
    if (!gid) return;
    const restoreBtn = e.target.closest("[data-snapshot-restore]");
    if (restoreBtn) {
      const snapshotId = restoreBtn.dataset.snapshotRestore;
      restoreBtn.disabled = true;
      try {
        const rep = await restoreGameSnapshot(gid, snapshotId);
        showToast(
          `Restored "${rep?.label || snapshotId}" in ${Number(rep?.elapsedMs || 0)} ms (${Number(rep?.hardlinkedFiles || 0)} hardlinked, ${Number(rep?.removedFiles || 0)} cleaned).`,
          "success",
          4000
        );
      } catch (err) {
        showToast(String(err), "error");
      } finally {
        restoreBtn.disabled = false;
      }
      return;
    }
    const delBtn = e.target.closest("[data-snapshot-delete]");
    if (delBtn) {
      const snapshotId = delBtn.dataset.snapshotDelete;
      delBtn.disabled = true;
      try {
        state.snapshotModalState.snapshots = await deleteGameSnapshot(gid, snapshotId);
        renderSnapshotsModalList();
        showToast("Snapshot deleted and unreferenced blobs garbage-collected.", "info", 2400);
      } catch (err) {
        showToast(String(err), "error");
      } finally {
        delBtn.disabled = false;
      }
    }
  });

  // Pre-Flight PE32 / PE32+ DLL Auto-Healer Modal Events
  const closePreflightModal = () => el("preflight-modal")?.classList.add("hidden");
  el("btn-close-preflight-modal")?.addEventListener("click", closePreflightModal);
  el("btn-cancel-preflight-modal")?.addEventListener("click", closePreflightModal);
  el("preflight-modal")?.addEventListener("click", (e) => {
    if (e.target === el("preflight-modal")) closePreflightModal();
  });

  el("btn-preflight-heal")?.addEventListener("click", async () => {
    const { game, report, manualInspect } = state.preflightModalState;
    const exePath = game?.exePath || report?.exePath;
    if (!exePath) return;
    const btn = el("btn-preflight-heal");
    if (btn) btn.disabled = true;
    try {
      const heal = await healGameRuntimeDlls(exePath, null);
      showToast(heal?.summary || "Auto-healed runtime DLLs.", "success", 3800);
      const updatedReport = await preflightCheckExe(exePath);
      state.preflightModalState.report = updatedReport;
      renderPreflightModal();
      if (!manualInspect && game && (!updatedReport?.missingDlls || updatedReport.missingDlls.length === 0)) {
        closePreflightModal();
        await launchGameDirectly(game);
      }
    } catch (err) {
      showToast(String(err), "error");
    } finally {
      if (btn) btn.disabled = false;
    }
  });

  el("btn-preflight-launch-anyway")?.addEventListener("click", async () => {
    const { game } = state.preflightModalState;
    closePreflightModal();
    if (game) {
      await launchGameDirectly(game);
    }
  });
}

function startGamepadControllerLoop() {
  const CYCLE_TABS = ["library", "catalog", "downloader", "settings"];
  const MOVE_DEBOUNCE_MS = 180;
  let timerId = null;
  let inFlight = false;
  let focusedEl = null;
  let lastConnected = null;
  let lastMoveTs = 0;
  const prev = { a: false, b: false, x: false, lb: false, rb: false };

  const setGamepadFocus = (nextEl) => {
    if (focusedEl && focusedEl !== nextEl) {
      focusedEl.classList.remove("gamepad-focus");
    }
    focusedEl = nextEl || null;
    if (focusedEl && document.contains(focusedEl)) {
      focusedEl.classList.add("gamepad-focus");
      focusedEl.focus?.({ preventScroll: true });
      focusedEl.scrollIntoView?.({ block: "nearest", inline: "nearest" });
    }
  };

  const getActiveModal = () => {
    const modals = [
      "details-modal",
      "catalog-detail-modal",
      "restore-save-modal",
      "snapshots-modal",
      "preflight-modal",
      "add-site-modal",
      "bulk-import-modal"
    ];
    for (const id of modals) {
      const m = el(id);
      if (m && !m.classList.contains("hidden")) return m;
    }
    return null;
  };

  const getFocusableTargets = () => {
    const modal = getActiveModal();
    const root = modal || document.querySelector(".main-panel") || document.body;
    const selector = [
      ".game-card",
      ".catalog-card",
      ".directory-card",
      ".source-result-card",
      ".sgdb-thumb-card",
      ".sidebar-nav .nav-item",
      ".filter-pill",
      "button:not(:disabled)",
      "input:not([type='hidden']):not(:disabled)",
      "select:not(:disabled)"
    ].join(",");

    const candidates = Array.from(root.querySelectorAll(selector)).filter((node) => {
      if (!node || node.offsetParent === null) return false;
      if (node.closest(".hidden")) return false;
      const rect = node.getBoundingClientRect();
      return rect.width > 4 && rect.height > 4;
    });
    return candidates;
  };

  const moveFocus = (dx, dy) => {
    const targets = getFocusableTargets();
    if (targets.length === 0) return;
    if (!focusedEl || !targets.includes(focusedEl)) {
      setGamepadFocus(targets[0]);
      return;
    }

    const cur = focusedEl.getBoundingClientRect();
    const cx = cur.left + cur.width / 2;
    const cy = cur.top + cur.height / 2;

    let best = null;
    let bestScore = Infinity;

    for (const cand of targets) {
      if (cand === focusedEl) continue;
      const r = cand.getBoundingClientRect();
      const tx = r.left + r.width / 2;
      const ty = r.top + r.height / 2;
      const vx = tx - cx;
      const vy = ty - cy;

      if (dx > 0 && vx <= 8) continue;
      if (dx < 0 && vx >= -8) continue;
      if (dy > 0 && vy <= 8) continue;
      if (dy < 0 && vy >= -8) continue;

      const primary = dx !== 0 ? Math.abs(vx) : Math.abs(vy);
      const cross = dx !== 0 ? Math.abs(vy) : Math.abs(vx);
      const score = primary + cross * 2.4;
      if (score < bestScore) {
        bestScore = score;
        best = cand;
      }
    }

    if (best) {
      setGamepadFocus(best);
      return;
    }

    const idx = targets.indexOf(focusedEl);
    const step = dx > 0 || dy > 0 ? 1 : -1;
    const wrapped = (idx + step + targets.length) % targets.length;
    setGamepadFocus(targets[wrapped]);
  };

  const handleActivate = () => {
    const targets = getFocusableTargets();
    if (!focusedEl || !document.contains(focusedEl)) {
      if (targets.length > 0) setGamepadFocus(targets[0]);
      return;
    }
    if (focusedEl.classList.contains("game-card") && focusedEl.dataset.gameId) {
      openGameDetailView(focusedEl.dataset.gameId);
      setTimeout(() => {
        const playBtn = el("btn-detail-play");
        if (playBtn) setGamepadFocus(playBtn);
      }, 60);
      return;
    }
    if (focusedEl.tagName === "INPUT" && focusedEl.type !== "checkbox") {
      focusedEl.focus();
      return;
    }
    focusedEl.click();
  };

  const handleBack = () => {
    const modal = getActiveModal();
    if (modal) {
      modal.classList.add("hidden");
      setGamepadFocus(null);
      return;
    }
    if (state.selectedDetailId) {
      el("btn-detail-back")?.click();
      setTimeout(() => {
        const firstCard = document.querySelector("#library-grid .game-card");
        if (firstCard) setGamepadFocus(firstCard);
      }, 40);
      return;
    }
    const navBtn = document.querySelector(".sidebar-nav .nav-item.active");
    if (navBtn) setGamepadFocus(navBtn);
  };

  const cycleTab = (dir) => {
    const modal = getActiveModal();
    if (modal) modal.classList.add("hidden");
    const curIdx = CYCLE_TABS.indexOf(state.activeTab);
    const nextIdx =
      curIdx < 0 ? 0 : (curIdx + dir + CYCLE_TABS.length) % CYCLE_TABS.length;
    const nextTab = CYCLE_TABS[nextIdx];
    const btn = document.querySelector(`.sidebar-nav .nav-item[data-tab="${nextTab}"]`);
    if (btn) btn.click();
    setTimeout(() => {
      const nextTargets = getFocusableTargets();
      if (nextTargets.length > 0) setGamepadFocus(nextTargets[0]);
    }, 50);
  };

  const tick = async () => {
    if (document.visibilityState !== "visible") {
      stopPolling();
      return;
    }
    if (inFlight) return;
    inFlight = true;
    try {
      const pad = await pollGamepadState();
      const connected = Boolean(pad?.connected);
      if (connected !== lastConnected) {
        lastConnected = connected;
        const badge = el("titlebar-controller-badge");
        if (badge) badge.classList.toggle("hidden", !connected);
      }
      if (!connected) return;

      const lx = Number(pad.thumbLx ?? pad.leftStickX ?? 0);
      const ly = Number(pad.thumbLy ?? pad.leftStickY ?? 0);
      const buttons = Number(pad.buttons ?? 0);

      const up = Boolean(pad.dpadUp || pad.up || ly > 16000 || (buttons & 0x0001));
      const down = Boolean(pad.dpadDown || pad.down || ly < -16000 || (buttons & 0x0002));
      const left = Boolean(pad.dpadLeft || pad.left || lx < -16000 || (buttons & 0x0004));
      const right = Boolean(pad.dpadRight || pad.right || lx > 16000 || (buttons & 0x0008));

      const a = Boolean(pad.a || (buttons & 0x1000));
      const b = Boolean(pad.b || (buttons & 0x2000));
      const x = Boolean(pad.x || (buttons & 0x4000));
      const lb = Boolean(pad.lb || pad.leftShoulder || (buttons & 0x0100));
      const rb = Boolean(pad.rb || pad.rightShoulder || (buttons & 0x0200));

      const now = performance.now();
      if ((up || down || left || right) && now - lastMoveTs >= MOVE_DEBOUNCE_MS) {
        lastMoveTs = now;
        if (right) moveFocus(1, 0);
        else if (left) moveFocus(-1, 0);
        else if (down) moveFocus(0, 1);
        else if (up) moveFocus(0, -1);
      }

      if (a && !prev.a) handleActivate();
      if (b && !prev.b) handleBack();
      if (lb && !prev.lb) cycleTab(-1);
      if (rb && !prev.rb) cycleTab(1);
      if (x && !prev.x) {
        applyCouchMode(!state.couchMode, true);
        showToast(state.couchMode ? "Couch / Handheld Mode enabled." : "Couch Mode disabled.", "info", 2000);
      }

      prev.a = a;
      prev.b = b;
      prev.x = x;
      prev.lb = lb;
      prev.rb = rb;
    } catch {
      // Ignore transient controller polling errors
    } finally {
      inFlight = false;
    }
  };

  function startPolling() {
    if (timerId !== null || document.visibilityState !== "visible") return;
    timerId = setInterval(tick, 16);
  }

  function stopPolling() {
    if (timerId !== null) {
      clearInterval(timerId);
      timerId = null;
    }
  }

  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") {
      startPolling();
    } else {
      stopPolling();
    }
  });

  startPolling();
}

async function init() {
  applyTheme(state.activeTheme);
  renderThemes();
  renderSources();
  syncCatalogFilterControlsToDom();
  initWindowControls({
    onCtrlK: () => {
      const inp = state.activeTab === "catalog" ? el("input-catalog-search") : el("search-input");
      if (inp) {
        inp.focus();
        inp.select();
      }
    }
  });
  wireEvents();

  getLauncherSettings()
    .then((settings) => {
      state.autoCleanCompleted = Boolean(settings?.autoCleanCompleted ?? true);
      state.autoDetectClipboard = Boolean(settings?.autoDetectClipboard ?? true);
      state.nvmePrefetchEnabled = Boolean(settings?.nvmePrefetchEnabled ?? true);
      state.swarmEnabled = Boolean(settings?.swarmEnabled ?? false);
      state.swarmNodeName = String(settings?.swarmNodeName || "");
      state.swarmPeers = Array.isArray(settings?.swarmPeers) ? settings.swarmPeers : [];
      state.steamgriddbApiKey = String(settings?.steamgriddbApiKey || "");
      state.suspendHotkey = String(settings?.suspendHotkey || "Ctrl+Alt+P");
      if (el("setting-auto-clean-completed")) {
        el("setting-auto-clean-completed").checked = state.autoCleanCompleted;
      }
      if (el("setting-auto-detect-clipboard")) {
        el("setting-auto-detect-clipboard").checked = state.autoDetectClipboard;
      }
      if (el("setting-nvme-prefetch")) {
        el("setting-nvme-prefetch").checked = state.nvmePrefetchEnabled;
      }
      if (el("setting-swarm-enabled")) {
        el("setting-swarm-enabled").checked = state.swarmEnabled;
      }
      if (el("setting-swarm-node-name")) {
        el("setting-swarm-node-name").value = state.swarmNodeName;
      }
      if (el("setting-swarm-peers")) {
        el("setting-swarm-peers").value = state.swarmPeers.join(", ");
      }
      if (el("setting-steamgriddb-key")) {
        el("setting-steamgriddb-key").value = state.steamgriddbApiKey;
      }
      if (el("setting-suspend-hotkey")) {
        el("setting-suspend-hotkey").value = state.suspendHotkey;
      }
      applyCouchMode(Boolean(settings?.couchMode), false);
      if (state.autoDetectClipboard) {
        scheduleClipboardDetection();
      }
      refreshSwarmAndMmapTelemetry();
    })
    .catch(() => {});

  await listenDownloadProgress((job) => {
    if (!job) return;
    const idx = state.downloadJobs.findIndex((j) => j.downloadId === job.downloadId);
    if (idx >= 0) {
      state.downloadJobs[idx] = job;
      updateDownloadJobCardInPlace(job);
    } else {
      state.downloadJobs.unshift(job);
      renderDownloadJobs();
    }

    if (
      job.status === "COMPLETED" &&
      job.registeredInLibrary &&
      !state.notifiedCompletedIds.has(job.downloadId)
    ) {
      state.notifiedCompletedIds.add(job.downloadId);
      showToast(
        `Completed & installed "${job.fileName || "game"}" to My Games.`,
        "success",
        2500
      );
      invokeCommand("get_local_library")
        .then((lib) => {
          if (Array.isArray(lib)) {
            state.library = lib;
            renderSidebarGameList();
            renderLibraryGrid();
            updateSidebarTelemetry();
          }
        })
        .catch(() => {});
    }

    updateSidebarTelemetry();
  });

  await listenEvent("download-queue-updated", (jobs) => {
    if (Array.isArray(jobs)) {
      state.downloadJobs = jobs;
      renderDownloadJobs();
      updateSidebarTelemetry();
    }
  });

  await listenEvent("download-auto-cleaned", (payload) => {
    if (payload?.downloadId) {
      state.downloadJobs = state.downloadJobs.filter((j) => j.downloadId !== payload.downloadId);
      renderDownloadJobs();
      updateSidebarTelemetry();
    }
  });

  await listenEvent("library-updated", (lib) => {
    if (Array.isArray(lib)) {
      const activeIds = new Set(lib.map((g) => g.id));
      for (const cachedId of Object.keys(state.sidebarIcons)) {
        if (!activeIds.has(cachedId)) {
          delete state.sidebarIcons[cachedId];
        }
      }
      for (const attemptedId of Array.from(state.sidebarIconsAttempted)) {
        if (!activeIds.has(attemptedId)) {
          state.sidebarIconsAttempted.delete(attemptedId);
        }
      }
      state.library = lib;
      renderSidebarGameList();
      renderLibraryGrid();
      if (state.catalog.loaded && state.activeTab === "catalog") {
        renderCatalogView(true);
      }
      updateSidebarTelemetry();
    }
  });

  await listenEvent("catalog-sync-progress", (payload) => {
    const indexed = Number(payload?.indexed_count ?? payload?.indexedCount ?? 0);
    const target = Number(payload?.target_count ?? payload?.targetCount ?? 30000);
    const done = Boolean(payload?.done);
    const c = state.catalog;
    if (indexed > 0) {
      c.totalCached = indexed;
    }
    c.syncTarget = target;
    c.syncing = !done;
    if (done) {
      c.fromCache = false;
      if (!c.fetchedAt) c.fetchedAt = Math.floor(Date.now() / 1000);
    }
    updateNavCatalogBadge();
    updateCatalogResultsSummary();
    if (
      state.activeTab === "catalog" &&
      c.loaded &&
      !c.loading &&
      (c.renderedCount < c.batchSize || (done && c.renderedCount <= c.batchSize))
    ) {
      runCatalogQuery(true);
    }
  });

  await listenEvent("memory-telemetry", (tele) => {
    if (!document.hidden) updateSidebarTelemetry(tele);
  });

  await listenEvent("game-running-state", (payload) => {
    document.body.classList.toggle("game-running-idle", Boolean(payload?.running));
    refreshActiveSessions();
  });

  await listenEvent("game-suspend-state", (payload) => {
    refreshActiveSessions();
    if (payload?.message) {
      showToast(payload.message, "info", 2400);
    }
  });

  await listenEvent("nvme-prefetch-report", (payload) => {
    if (!payload || Number(payload.warmedMb || 0) <= 0) return;
    const files = Number(payload.filesWarmed || 0);
    const warmedMb = Number(payload.warmedMb || 0);
    const gbps = Number(payload.throughputGbS || 0);
    const ms = Number(payload.elapsedMs || 0);
    showToast(
      `NVMe Warmer: ${warmedMb.toFixed(1)} MB (${files} pack${files === 1 ? "" : "s"}) @ ${gbps.toFixed(2)} GB/s (${ms} ms)`,
      "success",
      3600
    );
  });

  const refreshDownloadJobs = async () => {
    try {
      const jobs = await invokeCommand("get_download_jobs", { backupJobs: null });
      if (Array.isArray(jobs)) {
        state.downloadJobs = jobs;
        renderDownloadJobs();
        updateSidebarTelemetry();
      }
    } catch {
      // Queue refreshes again on next download progress tick.
    }
  };

  await listenEvent("directory-download-intercepted", (payload) => {
    const fileLabel = payload?.fileName || "file";
    const siteLabel = payload?.siteTitle || "Safe Browser";
    showToast(`Auto-grabbed "${fileLabel}" from ${siteLabel} -> 8-worker queue.`, "success", 3400);
    refreshDownloadJobs();
  });

  await listenEvent("duke-uri-download", (payload) => {
    const uriUrl = String(payload?.url || "").trim();
    if (uriUrl) {
      markClipboardUrlHandled(uriUrl);
      showToast(`Queued download from duke:// link: ${truncateClipboardUrl(uriUrl, 54)}`, "success", 3400);
    }
    switchTab("downloader");
    refreshDownloadJobs();
  });

  await refreshAllData();
  const pendingProtocolUrls = await takePendingProtocolUrls().catch(() => []);
  if (Array.isArray(pendingProtocolUrls) && pendingProtocolUrls.length > 0) {
    pendingProtocolUrls.forEach((u) => markClipboardUrlHandled(u));
    switchTab("downloader");
    await refreshDownloadJobs();
    const suffix = pendingProtocolUrls.length === 1 ? "" : "s";
    showToast(`Queued ${pendingProtocolUrls.length} download${suffix} from browser link.`, "success", 3400);
  }
  startGamepadControllerLoop();
}

init();

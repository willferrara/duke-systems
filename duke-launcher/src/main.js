import {
  THEME_PRESETS,
  addSearchSite,
  applyCustomArtwork,
  applyGameUpdate,
  applyWindowBackdrop,
  autoFixGame,
  beamGameFromPeer,
  buildDirectorySearchUrl,
  bulkImportSearchSites,
  calibrateControllerAxes,
  checkGameUpdates,
  clearCompletedDownloads,
  compressGameFolder,
  compressGameWof,
  convertFileSrc,
  createDesktopShortcutWithIcon,
  createGameSnapshot,
  decompressGameWof,
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
  getAppVersion,
  getCatalogGameDetails,
  getCatalogMmapStats,
  getDiskFreeSpace,
  getGameAchievements,
  getGameCompatStatus,
  getGameExtendedTelemetry,
  getGameStorageStats,
  getImportedSources,
  getLauncherSettings,
  getSearchSites,
  getSidebarIcons,
  getSwarmStatus,
  getUnifiedFriends,
  healGameRuntimeDlls,
  hideOverlayWindow,
  importExistingLibraries,
  importSourceFile,
  importSourceUrl,
  ingestDroppedPaths,
  initWindowControls,
  invokeCommand,
  isValidDownloadUrl,
  isValidHttpUrl,
  joinFriendSession,
  linkLocalGameWithSteamId,
  listGameSnapshots,
  listSaveBackups,
  listenDownloadProgress,
  listenEvent,
  openDirectoryBrowser,
  openExternalUrl,
  parseSteamBbcodeToSafeHtml,
  pickAnyFileForScan,
  pickCustomCoverImage,
  pickDownloadFolder,
  pickExecutableFile,
  pickExecutableOrShortcutFile,
  pickGameFolder,
  pickJsonSourceFile,
  pollGamepadState,
  addRetroGameFromCatalog,
  checkGameEmulatorReady,
  getEmulatorStatuses,
  getRetroPlatforms,
  installEmulatorForConsole,
  linkGameRom,
  listenEmulatorInstallProgress,
  listenRetroCatalogSyncProgress,
  pickRomFile,
  preflightCheckExe,
  probeSwarmPeers,
  purgeSuiteRamIpc,
  queryCatalogMmap,
  readClipboardText,
  reclaimSnapshotSpace,
  refreshAllImportedSources,
  refreshImportedSource,
  registerDukeProtocol,
  removeImportedSource,
  removeSearchSite,
  resetDefaultSearchSites,
  resolveCatalogCover,
  restoreGameSave,
  restoreGameSnapshot,
  runSubsystemBenchmark,
  scanAndCleanDesktopShortcuts,
  scanRomFolder,
  searchImportedSources,
  searchSteamGridDbArtwork,
  searchWebCoversForGame,
  setCustomEmulatorPath,
  setGameAchievementsPath,
  setGameLaunchArgs,
  setGameLaunchProfile,
  setGameUpdateConfig,
  suspendOrResumeGame,
  syncGameSavesMesh,
  syncLudusaviManifest,
  syncRetroCatalog,
  syncStorefrontPlaytime,
  takePendingProtocolUrls,
  toggleGameDxvkWrapper,
  toggleGameLaaPatch,
  toggleImportedSource,
  toggleOverlayWindow,
  triggerReplayClipIpc,
  updateLauncherSettings,
  writeClipboardText,
  getFramePacingAndIoringStatus,
  toggleHalfMsFramePacing,
  purgeStaleDriverShaders,
  getProjfsRuntimeStatus,
  enableProjfsWindowsFeature,
  mountProjfsVirtualGame,
  unmountProjfsVirtualGame,
  deduplicateLibraryCas,
  getShaderSwarmStatus,
  syncGameShadersFromSwarm,
  archiveGameShaderBundle,
  getQuickResumeSlots,
  quickSwitchGameSlot,
  suspendToQuickResumeSlot,
  removeQuickResumeSlot,
  getSteamCmStatus,
  connectSteamCm,
  disconnectSteamCm,
  sendSteamChatMessage,
  getSteamChatMessages,
  getSteamPendingInvites,
  respondSteamGameInvite,
  listSteamCloudFiles,
  syncSteamCloudSaves,
  getStoreDirectStatus,
  loginGogDirect,
  loginEpicDirect,
  listGogOwnedTitles,
  listEpicOwnedTitles,
  startStoreDirectInstall,
  scanGameModGraph,
  registerGameModPack,
  saveGameModLoadout,
  activateGameModLoadout,
  getSuiteAutonomyStatus,
  getDukeLinkStatus,
  startCoOpHost,
  stopCoOpSession,
  inviteFriendToCoOp,
  joinCoOpStream,
  sendCoOpInputP2,
  getGameTimeMachine,
  createTimeMachineCheckpoint,
  createTimeMachineBranch,
  switchTimeMachineBranch,
  restoreTimeMachineCheckpoint,
  deleteTimeMachineCheckpoint,
  probeDirectStorageCapability,
  scanDirectStorageAssets,
  repackGameDirectStorageAssets,
  verifyDirectStorageStreams,
  getVhdxStageStatus,
  containerizeGameToVhdx,
  stageGameHotTier,
  unstageOrDetachVhdx,
  getGameUpscaleProfile,
  saveGameUpscaleProfile,
  promoteGameWindowBorderless,
  getLatencyLabStatus,
  getGameLatencyProfile,
  applyGameLatencyProfile,
  removeGameLatencyProfile,
  runDriverProfileGuard,
  enforceShaderCacheCap,
  probeEtwXrayCapability,
  getGameXrayScorecards,
  clearGameXrayScorecards,
  scanGameDeepShrink,
  applyGameDeepShrink,
  deduplicateLibraryBlake3,
  getAiRuntimeStatus,
  updateAiRuntimeConfig,
  unloadAiWorkers,
  triggerDukeVisionCapture,
  analyzeVisionTextQuery,
  getLatestVisionState,
  listenDukeVisionResult,
  getDukeCommsState,
  toggleDukeCommsListening,
  processVoicePhraseCommand,
  upsertVoiceMacroRule,
  listenDukeCommsVad,
  listenDukeCommsAction,
  listenDukeCommsSubtitle,
  diagnoseGameCrash,
  doctorRevertGameSnapshot,
  getDukeDirectorState,
  toggleDukeDirector,
  triggerDirectorHighlightNow,
  listenDukeDirectorHighlight,
  analyzeGameConfigPreset,
  applyGameConfigPreset,
  rollbackGameConfigPreset,
  getAcousticsState,
  setWasapiSessionVolume,
  saveGameEqAndDenoise,
  testMicRnnoiseLatency,
  planDukePilotCommand,
  executeDukePilotPlan,
  getPlayTonightRecommendations,
  getCouchModeStatus,
  setCouchModeState,
  triggerCouchRadialAction,
  exportGameDkpack,
  inspectDkpackFile,
  applyDkpackRecipe,
  pickDkpackFile,
  exportGameCapsule,
  inspectGameCapsule,
  listenCouchStateChanged,
  listenCouchRadialToggled,
  listenCouchRadialAction,
  listenDkpackApplied,
  listenPilotSemanticSearch,
  listenPilotPlayTonight,
  getGameJailStatus,
  setGameJailPolicy,
  getPrivacyVaultStatus,
  setVaultApiKey,
  applyAnonymityNetworkPreset,
  analyzeGameCrashOrPreflight,
  applyOneClickGameFixes,
  fetchPcgwFixes,
  checkSha256ThreatFeed,
  unblockMotwPath,
  getBiosVaultStatus,
  scanAndInstallBiosSources,
  fetchRetroAchievementsProgress,
  groupMultiDiscLibrary,
  applyRomPatchAndCreateProfile,
  previewCleanUninstall,
  executeCleanUninstall,
  installModArchiveOrWorkshop,
  pickBiosOrPatchFile,
  pickSaveFilePath,
  getDukeVerifiedStatus,
  recordDukeVerifiedFeedback,
  evaluateDiskPressureAutopilot,
  executeDiskPressureAutopilot,
  listGameGhosts,
  getGhostStatus,
  startGhostRecording,
  stopGhostRecording,
  startGhostPlayback,
  stopGhostPlayback,
  importGhostFile,
  exportGhostFile,
  deleteGhostRecording,
  getSandboxTrainerStatus,
  sandboxScanMemory,
  sandboxWriteMemoryValue,
  sandboxSetSpeedhackScale,
  getFseShellStatus,
  setFseGamingHome,
  resumeWindowsBloatNow,
  listWasmPlugins,
  installWasmPlugin,
  installDemoWasmPlugin,
  setWasmPluginCapabilities,
  executeWasmPlugin,
  removeWasmPlugin,
  runChaosStressHarness
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
  platform: "all",
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
  settingsTab: "essentials",
  defaultSaveDir: "",
  downloadSpeedPreset: "max",
  compactLibraryCards: false,
  performanceBoostEnabled: true,
  autoBackupSavesOnExit: true,
  cpuPriorityMode: "auto",
  ramTrimMode: "standard",
  autoFreezeMinimized: false,
  nvmeCacheBudgetPct: 25,
  dohEnabled: true,
  customProxyUrl: "",
  customSpeedCapMbps: 0,
  autoUnblockMotw: true,
  portableSandboxDefault: false,
  maxRestorePoints: 5,
  blockGameTelemetry: true,
  globalLaunchArgs: "",
  swarmAutoSyncSaves: true,
  startMinimizedWithWindows: false,
  clipboardBannerUrl: "",
  seenClipboardUrls: new Set(),
  clipboardFocusTimer: null,
  steamgriddbApiKey: "",
  steamApiKeyConfigured: false,
  xboxApiKeyConfigured: false,
  psnNpssoConfigured: false,
  steamIdOverride: "",
  detectedSteamId: "",
  friendsDrawerOpen: false,
  friendsPollTimer: null,
  friendsSnapshot: null,
  friendsLoading: false,
  detailPcgwWikiUrl: "",
  detailFixLinks: null,
  lastCrashDetected: null,
  detailJailStatus: null,
  privacyVaultStatus: null,
  detailCompatStatus: null,
  detailModGraph: null,
  detailTimeMachine: null,
  detailTimeMachineSelectedIdx: 0,
  detailXrayHistory: null,
  detailXrayLiveTick: null,
  detailUpscaleStatus: null,
  detailLatencyProfile: null,
  detailDirectStorageScan: null,
  detailVhdxState: null,
  detailDeepShrinkScan: null,
  latencyLabStatus: null,
  suiteAutonomyStatus: null,
  dukeLinkStatus: null,
  dukeLinkInviteUri: "",
  coopModalOpen: false,
  coopInputTimer: null,
  coopKeysDown: new Set(),
  coopInputSeq: 0,
  coopLastSig: "",
  projfsStreamingEnabled: true,
  shaderSwarmEnabled: true,
  framePacingEnabled: true,
  quickResume: null,
  steamCmStatus: null,
  steamChatMessages: [],
  steamPendingInvites: [],
  storeDirectStatus: null,
  gogOwnedTitles: [],
  epicOwnedTitles: [],
  audioCtx: null,
  suspendHotkey: "Ctrl+Alt+P",
  couchMode: false,
  controllerKind: "xbox",
  radialOpen: false,
  radialSelectedSlot: 0,
  radialSlots: [],
  playTonightIds: [],
  playTonightMap: new Map(),
  pilotPlan: null,
  pilotDebounceId: null,
  pilotEverythingItems: [],
  pilotEverythingIdx: -1,
  everythingSaveCache: [],
  diskPressureThresholdGb: 50,
  diskPressureDismissed: false,
  vkbTargetInput: null,
  vkbShift: false,
  gamepadPollingRateFn: null,
  gamepadClearFocusFn: null,
  gamepadResetRepeatsFn: null,
  gamepadStopPollingFn: null,
  controllerConnected: false,
  hasActiveGamepadFocus: false,
  pendingAbortControllers: new Map(),
  libraryPatchTimer: null,
  libraryPatchRaf: null,
  pendingLibraryPayload: null,
  pendingLibraryStructural: false,
  pendingChangedGameIds: new Set(),
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
  cardZoomPx: (() => {
    const n = Number(localStorage.getItem("duke_launcher_card_zoom") || 185);
    return Number.isFinite(n) ? Math.max(125, Math.min(280, Math.round(n))) : 185;
  })(),
  sidebarCollapsed: localStorage.getItem("duke_launcher_sidebar_collapsed") === "1",
  activeTheme: (() => {
    const saved = localStorage.getItem("duke_launcher_theme");
    return THEME_PRESETS.some((t) => t.id === saved) ? saved : "dark";
  })(),
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

function resolveMediaUrl(rawUrl) {
  const s = String(rawUrl || "").trim();
  if (!s) return "";
  if (/^(https?:|data:|blob:|asset:)/i.test(s)) return s;
  if (/^[A-Za-z]:[\\/]/.test(s) || s.startsWith("\\\\")) {
    return convertFileSrc(s);
  }
  return s;
}

const CATALOG_COVER_QUEUE = [];
const CATALOG_COVER_ATTEMPTED = new Set();
const CATALOG_COVER_CACHE = Object.create(null);
let catalogCoverInFlight = 0;
const MAX_CONCURRENT_CATALOG_COVERS = 4;

function resolvePlatformVisuals(platformLabel = "", source = "") {
  const hay = `${platformLabel} ${source}`.toLowerCase();
  if (/(nintendo|switch|wii|gamecube|3ds|nds|\bds\b|gba|gbc|game boy|n64|snes|nes|famicom)/i.test(hay)) {
    return { tone: "nintendo", icon: "icon-gamepad-2" };
  }
  if (/(playstation|\bps[1-5]\b|psp|vita|sony|psn)/i.test(hay)) {
    return { tone: "sony", icon: "icon-gamepad-2" };
  }
  if (/(xbox|microsoft)/i.test(hay)) {
    return { tone: "microsoft", icon: "icon-gamepad-2" };
  }
  if (/(sega|dreamcast|genesis|mega drive|saturn|arcade|atari|turbografx|pc engine|retro|console)/i.test(hay)) {
    return { tone: "retro", icon: "icon-layers" };
  }
  if (/(steam|valve|\bvr\b)/i.test(hay)) {
    return { tone: "steam", icon: "icon-compass" };
  }
  return { tone: "pc", icon: "icon-box" };
}

function buildProceduralCoverCardHtml(title, platformLabel = "PC", source = "") {
  const { tone, icon } = resolvePlatformVisuals(platformLabel, source);
  const safeTitle = escapeHtml(title || "Untitled Game");
  const safePlatform = escapeHtml(platformLabel || "PC");
  return `
    <div class="cover-fallback-card" data-platform-tone="${tone}">
      <span class="cover-fallback-emblem" aria-hidden="true">${svgIcon(icon)}</span>
      <span class="cover-fallback-title">${safeTitle}</span>
      <span class="cover-fallback-platform">${safePlatform}</span>
    </div>
  `;
}

function createProceduralCoverCardEl(title, platformLabel = "PC", source = "") {
  const { tone, icon } = resolvePlatformVisuals(platformLabel, source);
  const wrap = document.createElement("div");
  wrap.className = "cover-fallback-card";
  wrap.dataset.platformTone = tone;

  const emblem = document.createElement("span");
  emblem.className = "cover-fallback-emblem";
  emblem.setAttribute("aria-hidden", "true");
  emblem.innerHTML = svgIcon(icon);

  const titleEl = document.createElement("span");
  titleEl.className = "cover-fallback-title";
  titleEl.textContent = String(title || "Untitled Game");

  const platEl = document.createElement("span");
  platEl.className = "cover-fallback-platform";
  platEl.textContent = String(platformLabel || "PC");

  wrap.appendChild(emblem);
  wrap.appendChild(titleEl);
  wrap.appendChild(platEl);
  return wrap;
}

function applyCachedCatalogCoverToMedia(mediaEl, imgEl, rawPath) {
  const assetUrl = resolveMediaUrl(rawPath);
  if (!assetUrl || !mediaEl || !mediaEl.isConnected) return;
  const fallbackEl = mediaEl.querySelector(".cover-fallback-card");
  if (fallbackEl) fallbackEl.remove();
  imgEl.dataset.fallbackStage = "99";
  imgEl.src = assetUrl;
  imgEl.style.display = "";
  if (!imgEl.isConnected) {
    mediaEl.prepend(imgEl);
  }
}

function pumpCatalogCoverQueue() {
  while (catalogCoverInFlight < MAX_CONCURRENT_CATALOG_COVERS && CATALOG_COVER_QUEUE.length > 0) {
    const task = CATALOG_COVER_QUEUE.shift();
    if (!task) continue;
    catalogCoverInFlight += 1;
    resolveCatalogCover(task.req)
      .then((cachedPath) => {
        if (cachedPath) {
          CATALOG_COVER_CACHE[task.cacheKey] = cachedPath;
          task.entry.coverUrl = cachedPath;
          applyCachedCatalogCoverToMedia(task.mediaEl, task.imgEl, cachedPath);
        }
      })
      .catch(() => {})
      .finally(() => {
        catalogCoverInFlight = Math.max(0, catalogCoverInFlight - 1);
        pumpCatalogCoverQueue();
      });
  }
}

function scheduleCatalogCoverResolve(entry, imgEl, mediaEl) {
  const appId = Number(entry?.appId || 0);
  if (!appId) return;
  const isRetro = Number(entry.platformId || 0) > 0;
  const cacheKey = isRetro ? `retro_${appId}` : String(appId);
  if (CATALOG_COVER_CACHE[cacheKey]) {
    applyCachedCatalogCoverToMedia(mediaEl, imgEl, CATALOG_COVER_CACHE[cacheKey]);
    return;
  }
  if (CATALOG_COVER_ATTEMPTED.has(cacheKey) || CATALOG_COVER_QUEUE.length >= 32) {
    return;
  }
  CATALOG_COVER_ATTEMPTED.add(cacheKey);
  CATALOG_COVER_QUEUE.push({
    cacheKey,
    entry,
    imgEl,
    mediaEl,
    req: {
      appId,
      title: entry.name || "",
      platformId: Number(entry.platformId || 0) || null,
      platformName: entry.platformName || null,
      coverUrl: entry.coverUrl || entry.headerUrl || null
    }
  });
  pumpCatalogCoverQueue();
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

let cardZoomPersistTimer = null;

function clampCardZoomPx(px) {
  const n = Number(px);
  if (!Number.isFinite(n) || n <= 0) return 185;
  return Math.max(125, Math.min(280, Math.round(n)));
}

function applyCardZoom(px, persistToBackend = false) {
  const next = clampCardZoomPx(px);
  state.cardZoomPx = next;
  document.documentElement.style.setProperty("--card-min-width", `${next}px`);
  const label = `${next}px`;
  if (el("library-zoom-label")) el("library-zoom-label").textContent = label;
  if (el("catalog-zoom-label")) el("catalog-zoom-label").textContent = label;
  localStorage.setItem("duke_launcher_card_zoom", String(next));
  if (persistToBackend) {
    if (cardZoomPersistTimer) clearTimeout(cardZoomPersistTimer);
    cardZoomPersistTimer = setTimeout(() => {
      updateLauncherSettings({ cardZoomPx: state.cardZoomPx }).catch(() => {});
    }, 240);
  }
}

function applySidebarCollapsed(collapsed) {
  const next = Boolean(collapsed);
  state.sidebarCollapsed = next;
  document.querySelector(".sidebar")?.classList.toggle("collapsed", next);
  const btn = el("btn-toggle-sidebar");
  if (btn) btn.setAttribute("aria-expanded", next ? "false" : "true");
  localStorage.setItem("duke_launcher_sidebar_collapsed", next ? "1" : "0");
}

function applyTheme(themeId, persistToBackend = false) {
  const valid = THEME_PRESETS.some((t) => t.id === themeId) ? themeId : "dark";
  state.activeTheme = valid;
  document.body.setAttribute("data-theme", valid);
  localStorage.setItem("duke_launcher_theme", valid);
  applyWindowBackdrop(valid).catch(() => {});
  renderThemes();
  if (persistToBackend) {
    updateLauncherSettings({ theme: valid }).catch(() => {});
  }
}

function applyCompactLibraryCards(enabled) {
  state.compactLibraryCards = Boolean(enabled);
  el("library-grid")?.classList.toggle("compact-cards", state.compactLibraryCards);
  if (el("setting-compact-cards")) {
    el("setting-compact-cards").checked = state.compactLibraryCards;
  }
}

function snapNvmeBudgetPct(rawPct) {
  const val = Number(rawPct);
  const stops = [0, 10, 25, 40];
  let closest = 25;
  let minDiff = Infinity;
  for (const s of stops) {
    const diff = Math.abs(val - s);
    if (diff < minDiff) {
      minDiff = diff;
      closest = s;
    }
  }
  return closest;
}

function renderNvmeBudgetUi(pct) {
  const snapped = snapNvmeBudgetPct(pct);
  const label =
    snapped === 0
      ? "0% Off"
      : snapped === 25
        ? "25% Recommended"
        : snapped === 40
          ? "40% Max"
          : `${snapped}%`;
  if (el("setting-nvme-cache-budget")) {
    el("setting-nvme-cache-budget").value = String(snapped);
  }
  if (el("setting-nvme-cache-budget-val")) {
    el("setting-nvme-cache-budget-val").textContent = label;
  }
  document.querySelectorAll("#nvme-budget-presets [data-nvme-pct]").forEach((btn) => {
    btn.classList.toggle("active", Number(btn.dataset.nvmePct) === snapped);
  });
}

async function refreshSettingsDriveSpaceBar(customPath = null) {
  try {
    const info = await getDiskFreeSpace(customPath || state.defaultSaveDir || null);
    if (!info) return;
    if (el("setting-default-save-dir") && info.path) {
      el("setting-default-save-dir").value = info.path;
      if (!state.defaultSaveDir) {
        state.defaultSaveDir = info.path;
      }
    }
    if (el("settings-drive-space-label")) {
      el("settings-drive-space-label").textContent = `${info.rootDrive || "Drive"} · ${info.freeFormatted || "--"} free of ${info.totalFormatted || "--"}`;
    }
    const usedPct = Math.max(0, Math.min(100, Number(info.usedPct || 0)));
    if (el("settings-drive-space-pct")) {
      el("settings-drive-space-pct").textContent = `${usedPct.toFixed(0)}% used (${Number(info.freePct || 0).toFixed(0)}% free)`;
    }
    if (el("settings-drive-space-fill")) {
      el("settings-drive-space-fill").style.width = `${usedPct.toFixed(1)}%`;
    }
  } catch {
    // Drive telemetry is non-critical when path is pending.
  }
}

function switchSettingsTab(tabName = "essentials") {
  const target = tabName === "advanced" ? "advanced" : "essentials";
  state.settingsTab = target;
  el("settings-panel-essentials")?.classList.toggle("hidden", target !== "essentials");
  el("settings-panel-advanced")?.classList.toggle("hidden", target !== "advanced");
  const essBtn = el("btn-settings-tab-essentials");
  const advBtn = el("btn-settings-tab-advanced");
  if (essBtn) {
    essBtn.classList.toggle("active", target === "essentials");
    essBtn.setAttribute("aria-selected", String(target === "essentials"));
  }
  if (advBtn) {
    advBtn.classList.toggle("active", target === "advanced");
    advBtn.setAttribute("aria-selected", String(target === "advanced"));
  }
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

function beginScopedRequest(scope) {
  const key = String(scope || "global");
  const prev = state.pendingAbortControllers.get(key);
  if (prev) {
    try {
      prev.abort();
    } catch {
      // Ignore AbortController cleanup errors
    }
  }
  const ctrl = new AbortController();
  state.pendingAbortControllers.set(key, ctrl);
  return ctrl.signal;
}

function abortScopedRequests(scope) {
  const key = String(scope || "global");
  const prev = state.pendingAbortControllers.get(key);
  if (prev) {
    try {
      prev.abort();
    } catch {
      // Ignore AbortController cleanup errors
    }
    state.pendingAbortControllers.delete(key);
  }
}

function abortAllScopedRequests() {
  for (const ctrl of state.pendingAbortControllers.values()) {
    try {
      ctrl.abort();
    } catch {
      // Ignore AbortController cleanup errors
    }
  }
  state.pendingAbortControllers.clear();
}

function clearControllerFocus(options = {}) {
  const disconnect = Boolean(options?.disconnect ?? !state.controllerConnected);
  const blurNonText = options?.blurNonText !== false;
  const resetRepeats = options?.resetRepeats !== false;

  state.hasActiveGamepadFocus = false;
  document.body?.setAttribute("data-gamepad-active", "false");

  document.querySelectorAll(".gamepad-focus").forEach((node) => {
    node.classList.remove("gamepad-focus");
  });

  state.gamepadClearFocusFn?.();
  if (resetRepeats) {
    state.gamepadResetRepeatsFn?.();
  }

  if (blurNonText) {
    const active = document.activeElement;
    if (active && active !== document.body && active !== document.documentElement) {
      const tag = String(active.tagName || "").toUpperCase();
      const inputType = String(active.type || "text").toLowerCase();
      const isTextInput =
        tag === "TEXTAREA" ||
        Boolean(active.isContentEditable) ||
        (tag === "INPUT" &&
          !["button", "submit", "reset", "checkbox", "radio", "range"].includes(inputType));
      if (!isTextInput) {
        active.blur?.();
      }
    }
  }

  if (disconnect) {
    state.controllerConnected = false;
    const badge = el("titlebar-controller-badge");
    if (badge) badge.classList.add("hidden");
    const ind = el("couch-input-indicator");
    if (ind) {
      ind.classList.remove("connected");
      ind.title = "No Controller Connected";
    }
    const dotL = el("couch-stick-dot-l");
    const dotR = el("couch-stick-dot-r");
    if (dotL) dotL.style.transform = "translate(-50%, -50%)";
    if (dotR) dotR.style.transform = "translate(-50%, -50%)";
    const devLbl = el("cal-device-label");
    if (devLbl) devLbl.textContent = "No active controller";
    for (let i = 0; i < 4; i++) {
      const barEl = el(`cal-axis-bar-${i}`);
      const valEl = el(`cal-axis-val-${i}`);
      if (barEl) barEl.style.width = "50.0%";
      if (valEl) valEl.textContent = "0.00";
    }
    updateControllerGlyphLegend("none");
  }
}

function closeCouchCalibratorPopover() {
  const pop = el("couch-calibrator-popover");
  if (!pop || pop.classList.contains("hidden")) return;
  pop.classList.add("hidden");
  clearControllerFocus({ disconnect: !state.controllerConnected, blurNonText: true, resetRepeats: true });
}

function closeCatalogDetailModal() {
  state.catalog.modalRequestId += 1;
  abortScopedRequests("catalog-modal");
  const modal = el("catalog-detail-modal");
  if (!modal || modal.classList.contains("hidden")) return;
  modal.classList.add("hidden");
  clearControllerFocus({ disconnect: !state.controllerConnected, blurNonText: true, resetRepeats: true });
}

function closeDetailsModal() {
  abortScopedRequests("cover-search");
  const modal = el("details-modal");
  if (!modal || modal.classList.contains("hidden")) return;
  modal.classList.add("hidden");
  clearControllerFocus({ disconnect: !state.controllerConnected, blurNonText: true, resetRepeats: true });
}

function closeRestoreSaveModal() {
  abortScopedRequests("restore-save");
  const modal = el("restore-save-modal");
  if (!modal || modal.classList.contains("hidden")) return;
  modal.classList.add("hidden");
  clearControllerFocus({ disconnect: !state.controllerConnected, blurNonText: true, resetRepeats: true });
}

function closeSnapshotsModal() {
  abortScopedRequests("snapshots-modal");
  const modal = el("snapshots-modal");
  if (!modal || modal.classList.contains("hidden")) return;
  modal.classList.add("hidden");
  clearControllerFocus({ disconnect: !state.controllerConnected, blurNonText: true, resetRepeats: true });
}

function closePreflightModal() {
  abortScopedRequests("preflight-modal");
  const modal = el("preflight-modal");
  if (!modal || modal.classList.contains("hidden")) return;
  modal.classList.add("hidden");
  clearControllerFocus({ disconnect: !state.controllerConnected, blurNonText: true, resetRepeats: true });
}

function closeTopmostUiLayer() {
  const isVisible = (id) => {
    const node = el(id);
    return Boolean(node && !node.classList.contains("hidden"));
  };
  if (isVisible("virtual-keyboard-overlay")) {
    closeVirtualKeyboard(false);
    return true;
  }
  if (isVisible("radial-hotbar-overlay")) {
    closeRadialHotbar();
    return true;
  }
  if (isVisible("couch-calibrator-popover")) {
    closeCouchCalibratorPopover();
    return true;
  }
  if (isVisible("duke-pilot-popover")) {
    closeDukePilotPopover();
    return true;
  }
  if (isVisible("emu-install-prompt-modal")) {
    closeEmulatorInstallModal();
    return true;
  }
  if (isVisible("clean-uninstall-modal")) {
    closeCleanUninstallModal();
    return true;
  }
  if (isVisible("coop-stream-modal")) {
    closeCoopStreamModal();
    return true;
  }
  if (isVisible("snapshots-modal")) {
    closeSnapshotsModal();
    return true;
  }
  if (isVisible("preflight-modal")) {
    closePreflightModal();
    return true;
  }
  if (isVisible("restore-save-modal")) {
    closeRestoreSaveModal();
    return true;
  }
  if (isVisible("add-site-modal")) {
    closeAddSiteModal();
    return true;
  }
  if (isVisible("bulk-import-modal")) {
    closeBulkImportModal();
    return true;
  }
  if (isVisible("catalog-detail-modal")) {
    closeCatalogDetailModal();
    return true;
  }
  if (isVisible("details-modal")) {
    closeDetailsModal();
    return true;
  }
  if (isVisible("friends-drawer")) {
    toggleFriendsDrawer(false);
    return true;
  }
  if (isVisible("catalog-genres-drawer-body")) {
    el("catalog-genres-drawer-body")?.classList.add("hidden");
    state.catalog.genresDrawerOpen = false;
    return true;
  }
  if (state.selectedDetailId) {
    state.selectedDetailId = null;
    abortScopedRequests("detail");
    switchTab("library", false);
    renderSidebarGameList();
    renderLibraryGrid();
    return true;
  }
  return false;
}

function switchTab(tab, keepDetail = false) {
  if (!keepDetail) {
    state.selectedDetailId = null;
    abortScopedRequests("detail");
  }
  if (tab !== "catalog") {
    abortScopedRequests("catalog");
  }
  state.activeTab = tab;
  clearControllerFocus({ disconnect: !state.controllerConnected, blurNonText: false, resetRepeats: true });
  closeDukePilotPopover();
  closeCouchCalibratorPopover();
  if (el("catalog-genres-drawer-body")) {
    el("catalog-genres-drawer-body").classList.add("hidden");
    state.catalog.genresDrawerOpen = false;
  }

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
    if (tab === "settings") {
      loadEmulatorSettingsSection();
      refreshFrontierDiagnostics().catch(() => {});
      refreshWave3AiDiagnostics().catch(() => {});
    }
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

function getTotalPlaytimeSecs(game) {
  if (!game) return 0;
  const split = Number(game.importedPlaytimeSecs || 0) + Number(game.dukePlaytimeSecs || 0);
  return Math.max(split, Number(game.playtimeSeconds || 0));
}

function updateSidebarTelemetry(telemetry) {
  const totalBytes = state.library.reduce((acc, g) => acc + Number(g.sizeBytes || 0), 0);

  if (el("nav-library-count")) el("nav-library-count").textContent = String(state.library.length);
  if (el("sidebar-list-count")) el("sidebar-list-count").textContent = String(state.library.length);
  if (el("stat-installed-count")) el("stat-installed-count").textContent = String(state.library.length);
  if (el("stat-total-size")) el("stat-total-size").textContent = formatBytesJs(totalBytes);

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
  if (Number(game?.platformId || 0) > 0) return game?.platform || "Console";
  if (game?.steamAppId || (game?.appId && Number(game.appId) < 2000000000)) return "Steam";
  return "Portable";
}

function getFilteredSortedGames() {
  const q = state.searchQuery.trim().toLowerCase();
  if (state.filter === "play_tonight" && state.playTonightIds.length > 0) {
    const byId = new Map(state.library.map((g) => [g.id, g]));
    const ordered = [];
    for (const id of state.playTonightIds) {
      const g = byId.get(id);
      if (!g) continue;
      if (q) {
        const hay = `${g.title} ${g.genre} ${resolveGameSourceLabel(g)} ${g.exePath}`.toLowerCase();
        if (!hay.includes(q)) continue;
      }
      ordered.push(g);
    }
    return ordered;
  }

  const list = state.library.filter((g) => {
    const isRetro = Number(g?.platformId || 0) > 0;
    const srcLabel = resolveGameSourceLabel(g).toLowerCase();
    const hasRealSteamId = Boolean(g.steamAppId || (g.appId && Number(g.appId) < 2000000000));
    const isSteamDetected = !isRetro && (srcLabel === "steam" || hasRealSteamId);
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
        return getTotalPlaytimeSecs(b) - getTotalPlaytimeSecs(a);
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
  const html = sorted
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
  const tpl = document.createElement("template");
  tpl.innerHTML = html;
  const frag = tpl.content;
  frag.querySelectorAll("img.sidebar-game-icon").forEach((img) => {
    const row = img.closest(".sidebar-game-item");
    const gid = row?.dataset.gameId || "";
    const game = state.library.find((g) => g.id === gid);
    bindSidebarIconErrorHandler(img, gid, game?.title || "");
  });
  container.replaceChildren(frag);

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

  const html = games
    .map((game) => {
      const cover =
        resolveMediaUrl(game.coverUrl) ||
        resolveMediaUrl(game.coverDataUrl);
      const playtimeStr = formatPlaytime(getTotalPlaytimeSecs(game));
      const lastStr = formatLastPlayed(game.lastPlayedTs || game.lastPlayed);
      const favClass = game.isFavorite ? "is-fav" : "";
      const sourceLabel = resolveGameSourceLabel(game);
      const sizeStr = game.sizeFormatted || formatBytesJs(game.sizeBytes);
      const updateBadge = game.updateAvailable
        ? `<span class="card-footer-chip card-footer-chip-update" title="Newer version detected on update feed">Update</span>`
        : "";
      const tonightRec = state.filter === "play_tonight" ? state.playTonightMap.get(game.id) : null;
      const tonightReasonHtml = tonightRec
        ? `<div class="play-tonight-fit-pill" title="${escapeHtml((tonightRec.reasons || []).join(" · "))}">${escapeHtml(tonightRec.fitLabel || tonightRec.estimatedSessionFit || "Play Tonight Pick")}</div>`
        : "";
      const mediaContentHtml = cover
        ? `<img src="${escapeHtml(cover)}" alt="${escapeHtml(game.title)}" class="game-card-img" loading="lazy" decoding="async" />`
        : buildProceduralCoverCardHtml(game.title, sourceLabel, game.source || "");

      return `
        <article class="game-card" data-game-id="${escapeHtml(game.id)}">
          <div class="game-card-media">
            ${mediaContentHtml}
            <div class="game-card-badges">
              <span></span>
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
                <button type="button" class="btn btn-secondary btn-xs card-action-btn" data-action="fix" data-game-id="${escapeHtml(game.id)}" title="Open Crash-to-Fix & Compatibility Repairs">
                  ${svgIcon("icon-wrench")}
                  <span>Fix Game</span>
                </button>
                <button type="button" class="btn btn-secondary btn-xs card-action-btn" data-action="backup" data-game-id="${escapeHtml(game.id)}" title="Create Rolling Save Backup (.zip)">
                  ${svgIcon("icon-archive")}
                  <span>Save Backup</span>
                </button>
                <button type="button" class="btn btn-secondary btn-xs card-action-btn" data-action="settings" data-game-id="${escapeHtml(game.id)}" title="Game Settings, Jail, Launch Args & Details">
                  ${svgIcon("icon-sliders")}
                  <span>Settings</span>
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
            <div class="game-card-footer-tags">
              <span class="card-footer-chip">${escapeHtml(sourceLabel)}</span>
              ${updateBadge}
              <span class="card-footer-chip mono-num">${escapeHtml(sizeStr)}</span>
            </div>
            ${tonightReasonHtml}
          </div>
        </article>
      `;
    })
    .join("");

  const tpl = document.createElement("template");
  tpl.innerHTML = html;
  const frag = tpl.content;
  frag.querySelectorAll("img.game-card-img").forEach((imgEl) => {
    imgEl.addEventListener(
      "error",
      () => {
        const card = imgEl.closest(".game-card");
        const gid = card?.dataset?.gameId || "";
        const game = state.library.find((g) => g.id === gid);
        const title = game?.title || imgEl.alt || "Game";
        const platformLabel = game ? resolveGameSourceLabel(game) : "PC";
        imgEl.replaceWith(createProceduralCoverCardEl(title, platformLabel, game?.source || ""));
      },
      { once: true }
    );
  });
  grid.replaceChildren(frag);
}

function patchChangedLibraryCards(changedIds = null) {
  const grid = el("library-grid");
  if (!grid || state.activeTab !== "library" || state.selectedDetailId) return false;
  const games = getFilteredSortedGames();
  const empty = el("library-empty");
  if (empty) empty.classList.toggle("hidden", state.library.length > 0);

  const cards = Array.from(grid.querySelectorAll(".game-card[data-game-id]"));
  if (cards.length !== games.length || games.length === 0) {
    return false;
  }
  const byId = new Map();
  for (let i = 0; i < games.length; i++) {
    const g = games[i];
    if (!g || cards[i]?.dataset?.gameId !== g.id) {
      return false;
    }
    byId.set(g.id, g);
  }

  const targetIds = changedIds && changedIds.size > 0 ? changedIds : null;
  for (const card of cards) {
    const game = byId.get(card.dataset.gameId);
    if (!game) continue;
    if (targetIds && !targetIds.has(game.id)) continue;

    const favBtn = card.querySelector('.card-fav-btn[data-action="fav"]');
    if (favBtn) favBtn.classList.toggle("is-fav", Boolean(game.isFavorite));

    const titleEl = card.querySelector(".game-card-title");
    if (titleEl && titleEl.textContent !== game.title) {
      titleEl.textContent = game.title;
      titleEl.title = game.title;
    }

    const metaEl = card.querySelector(".game-card-meta");
    if (metaEl) {
      const spans = metaEl.querySelectorAll("span");
      const nextPlaytime = formatPlaytime(getTotalPlaytimeSecs(game));
      const nextLast = formatLastPlayed(game.lastPlayedTs || game.lastPlayed);
      if (spans[0] && spans[0].textContent !== nextPlaytime) spans[0].textContent = nextPlaytime;
      if (spans[1] && spans[1].textContent !== nextLast) spans[1].textContent = nextLast;
    }

    const mediaEl = card.querySelector(".game-card-media");
    const nextCover = resolveMediaUrl(game.coverUrl) || resolveMediaUrl(game.coverDataUrl);
    if (mediaEl && nextCover) {
      const curImg = mediaEl.querySelector("img.game-card-img");
      if (!curImg || curImg.getAttribute("src") !== nextCover) {
        const nextImg = document.createElement("img");
        nextImg.src = nextCover;
        nextImg.alt = game.title;
        nextImg.className = "game-card-img";
        nextImg.loading = "lazy";
        nextImg.decoding = "async";
        nextImg.addEventListener(
          "error",
          () => {
            nextImg.replaceWith(createProceduralCoverCardEl(game.title, resolveGameSourceLabel(game), game.source || ""));
          },
          { once: true }
        );
        const oldMedia = curImg || mediaEl.querySelector(".procedural-cover-card");
        if (oldMedia) oldMedia.replaceWith(nextImg);
        else mediaEl.prepend(nextImg);
      }
    }
  }
  return true;
}

function scheduleLibraryCoalescedFlush() {
  if (state.libraryPatchTimer !== null) {
    clearTimeout(state.libraryPatchTimer);
  }
  state.libraryPatchTimer = setTimeout(() => {
    state.libraryPatchTimer = null;
    if (state.libraryPatchRaf !== null) {
      cancelAnimationFrame(state.libraryPatchRaf);
    }
    state.libraryPatchRaf = requestAnimationFrame(() => {
      state.libraryPatchRaf = null;
      const nextLib = state.pendingLibraryPayload;
      const structural = Boolean(state.pendingLibraryStructural);
      const changedSet = new Set(state.pendingChangedGameIds);
      state.pendingLibraryPayload = null;
      state.pendingLibraryStructural = false;
      state.pendingChangedGameIds.clear();

      if (Array.isArray(nextLib)) {
        const activeIds = new Set(nextLib.map((g) => g.id));
        for (const cachedId of Object.keys(state.sidebarIcons)) {
          if (!activeIds.has(cachedId)) delete state.sidebarIcons[cachedId];
        }
        for (const attemptedId of Array.from(state.sidebarIconsAttempted)) {
          if (!activeIds.has(attemptedId)) state.sidebarIconsAttempted.delete(attemptedId);
        }
        state.library = nextLib;
      }

      if (structural) {
        renderLibraryGrid();
        renderSidebarGameList();
      } else if (changedSet.size > 0) {
        if (!patchChangedLibraryCards(changedSet)) {
          renderLibraryGrid();
        }
        renderSidebarGameList();
      }

      if (state.selectedDetailId && (structural || changedSet.has(state.selectedDetailId))) {
        const selGame = state.library.find((g) => g.id === state.selectedDetailId);
        if (selGame) {
          if (el("detail-playtime")) el("detail-playtime").textContent = formatPlaytime(getTotalPlaytimeSecs(selGame));
          if (el("detail-last-played")) {
            el("detail-last-played").textContent = formatLastPlayed(selGame.lastPlayedTs || selGame.lastPlayed);
          }
        }
      }
      if (state.catalog.loaded && state.activeTab === "catalog" && structural) {
        renderCatalogView(true);
      }
      updateSidebarTelemetry();
    });
  }, 150);
}

function buildHeroCandidateList(appId, fallbackUrl) {
  const candidates = [];
  const validId = Number(appId || 0);
  if (validId > 0 && validId < 2000000000) {
    candidates.push(
      `https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/${validId}/library_hero.jpg`,
      `https://cdn.cloudflare.steamstatic.com/steam/apps/${validId}/library_hero.jpg`,
      `https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/${validId}/header.jpg`
    );
  }
  if (fallbackUrl && typeof fallbackUrl === "string" && fallbackUrl.trim()) {
    candidates.push(resolveMediaUrl(fallbackUrl.trim()));
  }
  return candidates.filter((url, idx, arr) => url && arr.indexOf(url) === idx);
}

function applyHeroCandidateChain(imgEl, rawCandidates, isStillValid) {
  if (!imgEl) return;
  const candidates = rawCandidates
    .map((url) => resolveMediaUrl(url))
    .filter((url, idx, arr) => typeof url === "string" && url.trim() && arr.indexOf(url) === idx);
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
  const detailSignal = beginScopedRequest("detail");
  state.selectedDetailId = game.id;
  switchTab("library", true);
  renderSidebarGameList();
  syncActiveSessionsUi();

  const isRetroGame = Number(game.platformId || 0) > 0;
  el("btn-detail-link-rom")?.classList.toggle("hidden", !isRetroGame);
  el("detail-rom-patcher-block")?.classList.toggle("hidden", !isRetroGame);
  el("detail-ra-progress-box")?.classList.toggle("hidden", !isRetroGame);
  if (el("detail-rom-patch-status")) {
    el("detail-rom-patch-status").textContent = "";
    el("detail-rom-patch-status").classList.add("hidden");
  }
  if (el("detail-moddrop-security-summary")) {
    el("detail-moddrop-security-summary").textContent = "";
    el("detail-moddrop-security-summary").classList.add("hidden");
    el("detail-moddrop-security-summary").classList.remove("blocked", "safe", "shield-verdict-banner");
  }

  const detailHeroImg = el("detail-hero-img");
  const cachedCover = resolveMediaUrl(game.heroUrl || game.coverDataUrl || game.coverUrl || "");
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
    if (isRetroGame) {
      const badgeLabel = game.romId ? String(game.romId) : "";
      appIdBadge.textContent = badgeLabel;
      appIdBadge.classList.toggle("hidden", !badgeLabel);
    } else {
      appIdBadge.textContent = initialAppId ? `AppID ${initialAppId}` : "";
      appIdBadge.classList.toggle("hidden", !initialAppId);
    }
  }

  if (el("detail-last-played")) el("detail-last-played").textContent = formatLastPlayed(game.lastPlayedTs || game.lastPlayed);
  if (el("detail-playtime")) {
    const importedSecs = Number(game.importedPlaytimeSecs || 0);
    const dukeSecs = Number(game.dukePlaytimeSecs || 0);
    const totalSecs = getTotalPlaytimeSecs(game);
    el("detail-playtime").textContent = formatPlaytime(totalSecs);
    el("detail-playtime").title = `Imported Storefront: ${formatPlaytime(importedSecs)} · Duke Tracker: ${formatPlaytime(dukeSecs)}`;
  }
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
  if (el("detail-description")) el("detail-description").textContent = "Loading game description and details...";
  if (el("detail-news-list")) el("detail-news-list").innerHTML = `<div class="panel-sub">Loading recent updates...</div>`;
  if (el("detail-autofix-summary")) {
    el("detail-autofix-summary").textContent = "";
    el("detail-autofix-summary").classList.add("hidden");
    el("detail-autofix-summary").classList.remove("warn");
  }

  loadDetailSavePaths(game, requestId, false);
  loadAllDetailPanels(game, requestId);
  refreshActiveSessions();

  const pinBtn = el("btn-detail-pin");
  if (pinBtn) {
    pinBtn.querySelector("span").textContent = game.isFavorite ? "Favorited" : "Favorite";
  }

  const isStillCurrent = () =>
    !detailSignal.aborted &&
    requestId === state.activeDetailRequestId &&
    state.selectedDetailId === game.id;

  if (isRetroGame && (!initialAppId || Number(initialAppId) >= 2000000000)) {
    try {
      const catDetails = initialAppId ? await getCatalogGameDetails(initialAppId).catch(() => null) : null;
      if (!isStillCurrent()) return;
      const candidates = [
        game.heroUrl,
        game.coverUrl,
        game.coverDataUrl,
        catDetails?.heroUrl,
        catDetails?.coverUrl,
        catDetails?.headerUrl
      ].filter((u, idx, arr) => u && arr.indexOf(u) === idx);
      applyHeroCandidateChain(detailHeroImg, candidates, isStillCurrent);
      if (el("detail-description")) {
        el("detail-description").textContent =
          catDetails?.shortDescription ||
          `${resolveGameSourceLabel(game)} game managed by Duke Launcher${game.emulatorId ? ` (${game.emulatorId})` : ""}.`;
      }
      if (el("detail-developers")) el("detail-developers").textContent = (catDetails?.developers || []).join(", ") || "--";
      if (el("detail-publishers")) el("detail-publishers").textContent = (catDetails?.publishers || []).join(", ") || "--";
      if (el("detail-release-date")) el("detail-release-date").textContent = catDetails?.releaseDate || "--";
      if (el("detail-genres")) el("detail-genres").textContent = (catDetails?.genres || []).join(", ") || game.genre || resolveGameSourceLabel(game);
      if (el("detail-live-players")) el("detail-live-players").textContent = resolveGameSourceLabel(game);
      if (el("detail-steam-price")) el("detail-steam-price").textContent = game.romId || "Console ROM";
      if (el("detail-news-list")) {
        el("detail-news-list").innerHTML =
          `<div class="panel-sub">Powered by Duke Portable Emulator Orchestrator. Use Save Restore Point or Duke Swarm for automatic cross-PC save sync.</div>`;
      }
    } catch {
      if (!isStillCurrent()) return;
      if (el("detail-description")) el("detail-description").textContent = `${resolveGameSourceLabel(game)} ROM managed offline.`;
      if (el("detail-live-players")) el("detail-live-players").textContent = resolveGameSourceLabel(game);
    }
    return;
  }

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

function playAchievementChime() {
  try {
    const AudioCtx = window.AudioContext || window.webkitAudioContext;
    if (!AudioCtx) return;
    if (!state.audioCtx) {
      state.audioCtx = new AudioCtx();
    }
    const ctx = state.audioCtx;
    if (ctx.state === "suspended") {
      ctx.resume().catch(() => {});
    }
    const now = ctx.currentTime;
    const notes = [
      { freq: 783.99, start: 0.0, dur: 0.14 },
      { freq: 987.77, start: 0.09, dur: 0.18 },
      { freq: 1174.66, start: 0.18, dur: 0.32 }
    ];
    for (const note of notes) {
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.type = "triangle";
      osc.frequency.setValueAtTime(note.freq, now + note.start);
      gain.gain.setValueAtTime(0.0001, now + note.start);
      gain.gain.exponentialRampToValueAtTime(0.08, now + note.start + 0.02);
      gain.gain.exponentialRampToValueAtTime(0.0001, now + note.start + note.dur);
      osc.connect(gain);
      gain.connect(ctx.destination);
      osc.start(now + note.start);
      osc.stop(now + note.start + note.dur + 0.02);
    }
  } catch {
    // Audio playback may be unavailable in headless environments.
  }
}

function showAchievementToast(payload) {
  if (!payload) return;
  playAchievementChime();
  const c = el("toast-container");
  if (!c) return;
  const t = document.createElement("div");
  t.className = "toast success";
  const title = payload.displayName || payload.apiName || "Achievement Unlocked";
  const gameTitle = payload.gameTitle ? `${payload.gameTitle} · ` : "";
  const desc = payload.description ? ` — ${payload.description}` : "";
  t.innerHTML = `
    <div style="display:flex;align-items:center;gap:10px;">
      ${svgIcon("icon-trophy")}
      <div>
        <div style="font-weight:600;">${escapeHtml(title)}</div>
        <div style="font-size:11.5px;opacity:0.86;">${escapeHtml(gameTitle + (payload.source || "Local") + desc)}</div>
      </div>
    </div>
  `;
  c.appendChild(t);
  setTimeout(() => t.remove(), 5200);
}

function renderDetailExtendedTelemetry(report, game) {
  const fmtHrs = (v) => (v != null && Number(v) > 0 ? `${Number(v).toFixed(1)} h` : "--");
  const hltb = report?.hltb || null;
  const pcgw = report?.pcgw || null;
  if (el("detail-ext-telemetry-status")) {
    el("detail-ext-telemetry-status").textContent = report?.hltbStatus || "Ready";
  }
  if (el("detail-hltb-main-hours")) el("detail-hltb-main-hours").textContent = fmtHrs(hltb?.mainStoryHours);
  if (el("detail-hltb-extras-hours")) el("detail-hltb-extras-hours").textContent = fmtHrs(hltb?.mainPlusExtrasHours);
  if (el("detail-hltb-comp-hours")) el("detail-hltb-comp-hours").textContent = fmtHrs(hltb?.completionistHours);

  const unifiedHours = Number(report?.unifiedPlaytimeHours ?? getTotalPlaytimeSecs(game) / 3600);
  const mainHrs = Number(hltb?.mainStoryHours || 0);
  const rawPct = Number(report?.mainProgressPct ?? (mainHrs > 0 ? (unifiedHours / mainHrs) * 100 : 0));
  const clampedPct = Math.max(0, Math.min(100, rawPct));
  if (el("detail-hltb-progress-label")) {
    el("detail-hltb-progress-label").textContent =
      mainHrs > 0
        ? `${unifiedHours.toFixed(1)} h / ${mainHrs.toFixed(1)} h (${rawPct.toFixed(0)}%)`
        : `${unifiedHours.toFixed(1)} h played`;
  }
  if (el("detail-hltb-progress-fill")) {
    el("detail-hltb-progress-fill").style.width = `${clampedPct.toFixed(1)}%`;
  }
  if (el("detail-pcgw-engine")) el("detail-pcgw-engine").textContent = pcgw?.engine || "--";
  if (el("detail-pcgw-controller")) el("detail-pcgw-controller").textContent = pcgw?.controllerSupport || "--";
  state.detailPcgwWikiUrl = pcgw?.wikiUrl || "";
  const wikiBtn = el("btn-open-pcgw-wiki");
  if (wikiBtn) {
    wikiBtn.title = state.detailPcgwWikiUrl || "Search this title on PCGamingWiki";
  }
}

async function loadDetailExtendedTelemetry(game, requestId, forceRefresh = false) {
  if (!game) return;
  if (el("detail-ext-telemetry-status")) {
    el("detail-ext-telemetry-status").textContent = forceRefresh ? "Refreshing..." : "Querying...";
  }
  try {
    const report = await getGameExtendedTelemetry(game.id, forceRefresh);
    if (requestId !== state.activeDetailRequestId || state.selectedDetailId !== game.id) return;
    renderDetailExtendedTelemetry(report, game);
  } catch (err) {
    if (requestId !== state.activeDetailRequestId || state.selectedDetailId !== game.id) return;
    if (el("detail-ext-telemetry-status")) el("detail-ext-telemetry-status").textContent = "Offline";
    if (forceRefresh) showToast(String(err), "error");
  }
}

function renderDetailAchievements(report) {
  const unlocked = Number(report?.unlockedCount || 0);
  const total = Number(report?.totalCount || 0);
  const pct = total > 0 ? Math.max(0, Math.min(100, Number(report?.completionPercent ?? (unlocked / total) * 100))) : 0;
  const sources = Array.isArray(report?.sources) && report.sources.length > 0 ? report.sources.join(" + ") : "Local";

  if (el("detail-ach-status-pill")) {
    el("detail-ach-status-pill").textContent = total > 0 ? `${unlocked}/${total} · ${sources}` : "No Schema";
  }
  if (el("detail-ach-progress-label")) {
    el("detail-ach-progress-label").textContent = `${unlocked} / ${total} (${pct.toFixed(0)}%)`;
  }
  if (el("detail-ach-progress-fill")) {
    el("detail-ach-progress-fill").style.width = `${pct.toFixed(1)}%`;
  }
  if (el("detail-ach-path") && typeof report?.customPath === "string") {
    el("detail-ach-path").value = report.customPath;
  }

  const list = el("detail-ach-recent-list");
  if (!list) return;
  const recent = Array.isArray(report?.recentUnlocks) ? report.recentUnlocks : [];
  const allItems = Array.isArray(report?.achievements) ? report.achievements : [];
  const display = recent.length > 0 ? recent : allItems.slice(0, 6);
  if (display.length === 0) {
    list.innerHTML = `<div class="panel-sub">${escapeHtml(report?.statusMessage || "No local or connected storefront achievements found.")}</div>`;
    return;
  }

  list.innerHTML = display
    .slice(0, 8)
    .map((item) => {
      const title = item.displayName || item.apiName || "Achievement";
      const iconHtml = item.iconUrl
        ? `<img src="${escapeHtml(item.iconUrl)}" alt="" class="ach-icon" loading="lazy" />`
        : `<div class="ach-icon-fallback">${svgIcon("icon-trophy")}</div>`;
      const stateLabel = item.unlocked
        ? item.unlockedAt
          ? `Unlocked ${formatLastPlayed(item.unlockedAt)}`
          : "Unlocked"
        : "Locked";
      const subText = item.description ? `${stateLabel} · ${item.description}` : `${stateLabel} · ${item.source || "Local"}`;
      return `
        <div class="ach-row">
          ${iconHtml}
          <div class="ach-copy">
            <div class="ach-title">${escapeHtml(title)}</div>
            <div class="ach-desc">${escapeHtml(subText)}</div>
          </div>
          <span class="pill mono-num">${escapeHtml(item.source || "Local")}</span>
        </div>
      `;
    })
    .join("");
}

async function loadDetailAchievements(game, requestId) {
  if (!game) return;
  if (el("detail-ach-status-pill")) el("detail-ach-status-pill").textContent = "Scanning...";
  try {
    const report = await getGameAchievements(game.id);
    if (requestId !== state.activeDetailRequestId || state.selectedDetailId !== game.id) return;
    renderDetailAchievements(report);
  } catch (err) {
    if (requestId !== state.activeDetailRequestId || state.selectedDetailId !== game.id) return;
    if (el("detail-ach-status-pill")) el("detail-ach-status-pill").textContent = "Unavailable";
    if (el("detail-ach-recent-list")) {
      el("detail-ach-recent-list").innerHTML = `<div class="panel-sub">${escapeHtml(String(err))}</div>`;
    }
  }
}

function renderDetailUpdateState(game, checkReport = null) {
  if (!game) return;
  if (el("detail-version-tag")) el("detail-version-tag").value = game.versionTag || "";
  if (el("detail-update-feed-url")) el("detail-update-feed-url").value = game.updateFeedUrl || "";

  const updateAvail = Boolean(checkReport?.updateAvailable ?? game.updateAvailable);
  const latestTag = checkReport?.latestVersion || game.latestVersionTag || "";
  const pill = el("detail-update-status-pill");
  if (pill) {
    pill.classList.toggle("accent", updateAvail);
    if (updateAvail) {
      pill.textContent = latestTag ? `Update ${latestTag}` : "Update Available";
    } else if (checkReport) {
      pill.textContent = "Up to Date";
    } else if ((game.updateFeedUrl || "").trim()) {
      pill.textContent = "Configured";
    } else {
      pill.textContent = "Unconfigured";
    }
  }

  const applyBtn = el("btn-apply-game-update");
  if (applyBtn) {
    applyBtn.classList.toggle("hidden", !updateAvail);
  }

  const notesEl = el("detail-update-notes");
  if (notesEl) {
    const notesText = String(checkReport?.notes || "").trim();
    notesEl.textContent = notesText;
    notesEl.classList.toggle("hidden", !notesText);
  }
}

function renderDetailCompat(report) {
  state.detailCompatStatus = report || null;
  if (el("detail-compat-arch-pill")) {
    const base = report?.is32bit ? "32-bit Game" : "64-bit Game";
    el("detail-compat-arch-pill").textContent = report?.largeAddressAware ? `${base} · 4 GB Ready` : base;
  }
  if (el("detail-compat-status")) {
    el("detail-compat-status").textContent = report?.statusMessage || "Compatibility check finished.";
  }

  const strip = el("detail-compat-dx-list");
  if (strip) {
    const apis = Array.isArray(report?.legacyDxImports) ? report.legacyDxImports : [];
    const pills = apis.map((api) => `<span class="pill accent mono-num">${escapeHtml(api)}</span>`);
    if (apis.length === 0) {
      pills.push(`<span class="pill">Modern DirectX Ready</span>`);
    }
    pills.push(
      `<span class="pill mono-num">${report?.dxvkSourceReady ? "Graphics Fix Ready" : "Graphics Fix Optional"}</span>`
    );
    strip.innerHTML = pills.join("");
  }

  const laaBtn = el("btn-toggle-laa");
  if (laaBtn) {
    laaBtn.disabled = !report?.is32bit;
    const span = laaBtn.querySelector("span");
    if (span) {
      span.textContent = report?.largeAddressAware ? "Revert 4GB LAA" : "Patch 4GB LAA";
    }
  }

  const dxvkBtn = el("btn-toggle-dxvk");
  if (dxvkBtn) {
    const span = dxvkBtn.querySelector("span");
    if (span) {
      span.textContent = report?.dxvkDeployed ? "Remove DXVK" : "Graphics Fix (DXVK)";
    }
  }
}

function buildFallbackFixLinks(game) {
  const title = String(game?.title || "").trim();
  const appId = Number(game?.steamAppId || game?.appId || 0) || null;
  const encodedTitle = encodeURIComponent(title);
  return {
    pcgamingwikiUrl: appId
      ? `https://www.pcgamingwiki.com/api/appid.php?appid=${appId}`
      : `https://www.pcgamingwiki.com/w/index.php?search=${encodedTitle}`,
    protondbUrl: appId ? `https://www.protondb.com/app/${appId}` : null,
    steamDiscussionsUrl: appId ? `https://steamcommunity.com/app/${appId}/discussions/` : null,
    githubIssuesUrl: `https://github.com/search?q=${encodedTitle}+crash+OR+fix&type=issues`
  };
}

function syncFixLinkButtonsState(links) {
  state.detailFixLinks = links || null;
  if (el("btn-fix-link-protondb")) el("btn-fix-link-protondb").disabled = !links?.protondbUrl;
  if (el("btn-fix-link-steam")) el("btn-fix-link-steam").disabled = !links?.steamDiscussionsUrl;
  if (el("btn-fix-link-pcgw")) el("btn-fix-link-pcgw").disabled = !links?.pcgamingwikiUrl;
  if (el("btn-fix-link-github")) el("btn-fix-link-github").disabled = !links?.githubIssuesUrl;
}

function renderCrashDiagnosisBox(diag) {
  const box = el("detail-crash-diagnosis-box");
  if (!box) return;
  if (!diag || !diag.exitCode) {
    box.classList.add("hidden");
    return;
  }
  box.classList.remove("hidden");
  if (el("detail-crash-fault-title")) {
    el("detail-crash-fault-title").textContent = diag.faultName || "Crash Diagnosis";
  }
  if (el("detail-crash-code-pill")) {
    el("detail-crash-code-pill").textContent = diag.exitCodeHex || `0x${(Number(diag.exitCode) >>> 0).toString(16).toUpperCase()}`;
  }
  if (el("detail-crash-plain-cause")) {
    el("detail-crash-plain-cause").textContent = diag.plainEnglishCause || "";
  }
  const actionsUl = el("detail-crash-actions-list");
  if (actionsUl) {
    const opts = Array.isArray(diag.recommendedActions) ? diag.recommendedActions : [];
    actionsUl.innerHTML = opts
      .map((opt) => `<li><strong>${escapeHtml(opt.label)}:</strong> ${escapeHtml(opt.description)}</li>`)
      .join("");
  }
  if (diag.pcgamingwikiUrl || diag.githubIssuesUrl) {
    syncFixLinkButtonsState({
      pcgamingwikiUrl: diag.pcgamingwikiUrl,
      protondbUrl: diag.protondbUrl,
      steamDiscussionsUrl: diag.steamDiscussionsUrl,
      githubIssuesUrl: diag.githubIssuesUrl
    });
  }
}

function renderPcgwFixIntelligence(rep) {
  const host = el("detail-pcgw-fix-sections");
  if (!host) return;
  syncFixLinkButtonsState({
    pcgamingwikiUrl: rep?.pcgamingwikiUrl || state.detailFixLinks?.pcgamingwikiUrl || "",
    protondbUrl: rep?.protondbUrl || state.detailFixLinks?.protondbUrl || null,
    steamDiscussionsUrl: rep?.steamDiscussionsUrl || state.detailFixLinks?.steamDiscussionsUrl || null,
    githubIssuesUrl: rep?.githubIssuesUrl || state.detailFixLinks?.githubIssuesUrl || ""
  });
  const sections = [
    ["Issues Fixed", rep?.issuesFixed],
    ["Crash Workarounds", rep?.crashWorkarounds],
    ["Ultrawide / FOV Patches", rep?.ultrawideFovPatches],
    ["Controller Fixes", rep?.controllerFixes]
  ];
  const nonEmpty = sections.filter(([, items]) => Array.isArray(items) && items.length > 0);
  if (nonEmpty.length === 0) {
    const pageHint = rep?.pageTitle ? ` (${escapeHtml(rep.pageTitle)})` : "";
    host.innerHTML = `<div class="panel-sub">No structured PCGamingWiki fix bullets parsed${pageHint}. Use the direct buttons below to inspect community threads.</div>`;
    return;
  }
  host.innerHTML = nonEmpty
    .map(
      ([heading, items]) => `
      <div class="pcgw-fix-group">
        <div class="pcgw-fix-group-title">${escapeHtml(heading)}</div>
        <ul class="fix-bullet-list">${items.slice(0, 5).map((item) => `<li>${escapeHtml(item)}</li>`).join("")}</ul>
      </div>`
    )
    .join("");
}

function renderDetailJailStatus(status) {
  state.detailJailStatus = status || null;
  const jailChk = el("detail-toggle-appcontainer-jail");
  const offlineChk = el("detail-toggle-offline-lock");
  const pill = el("detail-jail-status-pill");
  const caption = el("detail-jail-profile-caption");
  const jailed = Boolean(status?.jailEnabled);
  const offline = Boolean(status?.offlineMode);
  if (jailChk) jailChk.checked = jailed;
  if (offlineChk) offlineChk.checked = offline;
  if (pill) {
    pill.classList.remove("emerald", "accent", "amber");
    if (jailed && offline) {
      pill.textContent = "AppContainer + Offline";
      pill.classList.add("emerald");
    } else if (jailed) {
      pill.textContent = "AppContainer Jailed";
      pill.classList.add("accent");
    } else if (offline) {
      pill.textContent = "Offline Locked";
      pill.classList.add("amber");
    } else {
      pill.textContent = "Normal Windows";
    }
  }
  if (caption) {
    caption.textContent =
      status?.summary ||
      (status?.isolatedProfileDir
        ? `ACL scope: ${status.isolatedProfileDir}`
        : "ACL scope restricted to game folder when AppContainer is active.");
  }
}

async function loadDetailJailStatus(game, requestId) {
  if (!game) return;
  try {
    const st = await getGameJailStatus(game.id);
    if (requestId !== state.activeDetailRequestId || state.selectedDetailId !== game.id) return;
    renderDetailJailStatus(st);
  } catch (_) {}
}

async function loadDetailCompat(game, requestId) {
  if (!game) return;
  if (el("detail-compat-status")) el("detail-compat-status").textContent = "Inspecting executable PE header & DirectX imports...";
  syncFixLinkButtonsState(buildFallbackFixLinks(game));
  if (state.lastCrashDetected && state.lastCrashDetected.gameId === game.id) {
    renderCrashDiagnosisBox(state.lastCrashDetected);
  } else {
    renderCrashDiagnosisBox(null);
  }
  try {
    const [report, preflight, verified] = await Promise.all([
      getGameCompatStatus(game.id),
      analyzeGameCrashOrPreflight(game.id, null, null).catch(() => null),
      getDukeVerifiedStatus(game.id).catch(() => null)
    ]);
    if (requestId !== state.activeDetailRequestId || state.selectedDetailId !== game.id) return;
    renderDetailCompat(report);
    const vPill = el("detail-duke-verified-pill");
    const vSummary = el("detail-duke-verified-summary");
    if (vPill) {
      const tier = verified?.verifiedTier || "Unverified";
      vPill.textContent = tier;
      vPill.classList.toggle("ok", tier === "Duke Verified");
      vPill.classList.toggle("amber", tier === "Community Assisted");
      vPill.classList.toggle("danger", tier === "Needs Fixes");
    }
    if (vSummary) {
      vSummary.textContent =
        verified?.summary || "Keyed strictly by anonymous BLAKE3 binary hash (zero PII).";
    }
    if (preflight && (!state.lastCrashDetected || state.lastCrashDetected.gameId !== game.id)) {
      if (preflight.exitCode !== 0 && preflight.faultName) {
        renderCrashDiagnosisBox(preflight);
      } else if (preflight.plainEnglishCause && el("detail-compat-status")) {
        el("detail-compat-status").textContent = `${report?.statusMessage || ""} · ${preflight.plainEnglishCause}`;
      }
    }
  } catch (err) {
    if (requestId !== state.activeDetailRequestId || state.selectedDetailId !== game.id) return;
    if (el("detail-compat-status")) el("detail-compat-status").textContent = String(err);
  }
}

function renderDetailWofStorage(stats) {
  const logical = Number(stats?.logicalBytes || 0);
  const compressed = Number(stats?.compressedBytes || 0);
  const saved = Number(stats?.savedBytes || 0);
  const pct = Number(stats?.savedPct || 0);
  if (el("detail-wof-logical")) el("detail-wof-logical").textContent = formatBytesJs(logical);
  if (el("detail-wof-compressed")) el("detail-wof-compressed").textContent = formatBytesJs(compressed);
  if (el("detail-wof-saved")) {
    el("detail-wof-saved").textContent = `${formatBytesJs(saved)} (${pct.toFixed(1)}%)`;
  }
  if (el("detail-wof-status-pill")) {
    const count = Number(stats?.compressedFiles || 0);
    el("detail-wof-status-pill").textContent =
      count > 0 ? `${stats?.activeAlgorithm || "WOF"} · ${count} files` : "Uncompressed";
  }
}

async function loadDetailWofStorage(game, requestId) {
  if (!game) return;
  if (el("detail-wof-status-pill")) el("detail-wof-status-pill").textContent = "Measuring...";
  if (el("detail-wof-progress")) el("detail-wof-progress").classList.add("hidden");
  try {
    const stats = await getGameStorageStats(game.id);
    if (requestId !== state.activeDetailRequestId || state.selectedDetailId !== game.id) return;
    renderDetailWofStorage(stats);
  } catch (err) {
    if (requestId !== state.activeDetailRequestId || state.selectedDetailId !== game.id) return;
    if (el("detail-wof-status-pill")) el("detail-wof-status-pill").textContent = "Unavailable";
    if (el("detail-wof-logical")) el("detail-wof-logical").textContent = game.sizeFormatted || "--";
    if (el("detail-wof-compressed")) el("detail-wof-compressed").textContent = "--";
    if (el("detail-wof-saved")) el("detail-wof-saved").textContent = escapeHtml(String(err)).slice(0, 32);
  }
}

function renderDetailModGraph(report) {
  state.detailModGraph = report || null;
  const archPill = el("detail-mods-arch-pill");
  if (archPill) {
    const packsCount = Array.isArray(report?.packs) ? report.packs.length : 0;
    const colCount = Array.isArray(report?.collisions) ? report.collisions.length : 0;
    archPill.textContent =
      colCount > 0
        ? `${packsCount} Packs · ${colCount} Proxy Collision${colCount === 1 ? "" : "s"}`
        : `${packsCount} Packs · Clean Proxy Graph`;
    archPill.classList.toggle("warn", colCount > 0);
  }
  if (el("detail-mods-summary")) {
    el("detail-mods-summary").textContent =
      report?.summary || "Proxy-DLL export graph analyzed cleanly (.duke-cas hardlink ready).";
  }

  const colHost = el("detail-mods-collisions");
  if (colHost) {
    const collisions = Array.isArray(report?.collisions) ? report.collisions : [];
    colHost.innerHTML =
      collisions.length > 0
        ? collisions
            .map(
              (c) =>
                `<span class="pill warn mono-num" title="${escapeHtml(c.description || "")}">${escapeHtml(c.proxyDll || "proxy.dll")}: ${escapeHtml((c.contendingPacks || []).join(" + "))} -&gt; ${escapeHtml(c.generatedConfigFile || "chain")}</span>`
            )
            .join("")
        : `<span class="pill accent mono-num">Zero Proxy-DLL Conflicts (${Number(report?.deployedFileCount || 0)} staged)</span>`;
  }

  const packsHost = el("detail-mods-packs-list");
  if (packsHost) {
    const packs = Array.isArray(report?.packs) ? report.packs : [];
    const activeLoadout = (Array.isArray(report?.loadouts) ? report.loadouts : []).find(
      (l) => l.loadoutId === report?.activeLoadoutId
    );
    const activePackSet = new Set(activeLoadout?.enabledPackIds || []);

    if (packs.length === 0) {
      packsHost.innerHTML = `<div class="panel-sub">No mod packs registered or proxy DLLs (BepInEx, UE4SS, ReShade, SpecialK, DXVK) found in this folder yet.</div>`;
    } else {
      packsHost.innerHTML = packs
        .map((pack) => {
          const checked = activePackSet.size === 0 || activePackSet.has(pack.packId);
          const dllList = Array.isArray(pack.proxyDlls) ? pack.proxyDlls : [];
          const exportsSummary = dllList
            .slice(0, 3)
            .map((d) => `${d.fileNameLower || d.relPath} (${Number(d.exportCount || 0)} exports)`)
            .join(", ");
          return `
            <label class="mod-pack-card">
              <div class="mod-pack-head">
                <span class="mod-pack-title">
                  <input type="checkbox" data-mod-pack-id="${escapeHtml(pack.packId)}" ${checked ? "checked" : ""} />
                  <span>${escapeHtml(pack.name || pack.packId)}</span>
                </span>
                <span class="pill mono-num">${escapeHtml(pack.framework || "Mod")}</span>
              </div>
              <div class="mod-pack-meta mono-num">${Number(pack.fileCount || 0)} files · ${formatBytesJs(pack.totalBytes || 0)}${exportsSummary ? ` · ${escapeHtml(exportsSummary)}` : ""}</div>
            </label>
          `;
        })
        .join("");
    }
  }

  const loadoutsHost = el("detail-mods-loadouts-list");
  if (loadoutsHost) {
    const loadouts = Array.isArray(report?.loadouts) ? report.loadouts : [];
    loadoutsHost.innerHTML =
      loadouts.length > 0
        ? loadouts
            .map((lo) => {
              const isActive = lo.loadoutId === report?.activeLoadoutId;
              return `
                <button
                  type="button"
                  class="btn ${isActive ? "btn-primary" : "btn-secondary"} btn-xs mod-loadout-pill"
                  data-mod-loadout-apply="${escapeHtml(lo.loadoutId)}"
                >${escapeHtml(lo.name || lo.loadoutId)} (${(lo.enabledPackIds || []).length})</button>
              `;
            })
            .join("")
        : `<span class="panel-sub">No saved loadouts yet. Select packs above and click Save Selected Pack Loadout.</span>`;
  }
}

async function loadDetailModGraph(game, requestId) {
  if (!game) return;
  if (el("detail-mods-arch-pill")) el("detail-mods-arch-pill").textContent = "Scanning...";
  try {
    const report = await scanGameModGraph(game.id);
    if (requestId !== state.activeDetailRequestId || state.selectedDetailId !== game.id) return;
    renderDetailModGraph(report);
  } catch (err) {
    if (requestId !== state.activeDetailRequestId || state.selectedDetailId !== game.id) return;
    if (el("detail-mods-arch-pill")) el("detail-mods-arch-pill").textContent = "Unavailable";
    if (el("detail-mods-summary")) el("detail-mods-summary").textContent = String(err);
  }
}

function renderSidebarQuickResume(summary) {
  if (summary) state.quickResume = summary;
  const data = summary || state.quickResume;
  const maxSlots = Number(data?.maxSuspendedSlots || 3);
  const suspendedCount = Number(data?.suspendedCount || 0);
  const slots = Array.isArray(data?.slots) ? data.slots : [];

  if (el("sidebar-qr-count")) {
    el("sidebar-qr-count").textContent = `${suspendedCount}/${maxSlots}`;
  }
  const host = el("sidebar-qr-list");
  if (!host) return;
  if (slots.length === 0) {
    host.innerHTML = `<div class="sidebar-qr-empty">No active game slots</div>`;
    return;
  }

  host.innerHTML = slots
    .map((slot) => {
      const stateTag = slot.isSuspended ? "Suspended" : "Active";
      const ramStr = `${Number(slot.workingSetMb || 0).toFixed(0)} MB`;
      return `
        <div class="sidebar-qr-slot">
          <div class="sidebar-qr-slot-info">
            <div class="sidebar-qr-slot-title" title="${escapeHtml(slot.title || "")}">${escapeHtml(slot.title || "Game")}</div>
            <div class="sidebar-qr-slot-meta mono-num">${stateTag} · ${ramStr}${slot.usedIoringApi ? " · IoRing" : ""}</div>
          </div>
          <div class="sidebar-qr-slot-actions">
            <button type="button" class="btn btn-primary btn-xs" data-qr-switch="${escapeHtml(slot.gameId)}">${slot.isSuspended ? "Resume" : "Freeze"}</button>
            <button type="button" class="icon-mini-btn" data-qr-remove="${escapeHtml(slot.gameId)}" title="Release slot">
              <svg class="icon-mini" aria-hidden="true"><use href="#icon-x"/></svg>
            </button>
          </div>
        </div>
      `;
    })
    .join("");
}

async function refreshQuickResumeSlots() {
  try {
    const summary = await getQuickResumeSlots();
    renderSidebarQuickResume(summary);
    return summary;
  } catch {
    return null;
  }
}

function renderSteamCmFriendsState() {
  const cm = state.steamCmStatus;
  const pill = el("friends-cm-pill");
  const connectBtn = el("btn-friends-cm-connect");
  const settingsStatus = el("frontier-steam-cm-status");
  const settingsConnectBtn = el("btn-steam-cm-connect");
  const settingsDisconnectBtn = el("btn-steam-cm-disconnect");

  const isConnected = Boolean(cm?.connected);
  const labelText = isConnected
    ? `Steam CM: Connected${cm?.personaName ? ` (${cm.personaName})` : ""} · ${cm?.activeCmEndpoint || "WSS"}`
    : cm?.connecting
      ? "Steam CM: Connecting..."
      : `Steam CM: Disconnected${cm?.lastError ? ` (${cm.lastError})` : ""}`;

  if (pill) {
    pill.textContent = isConnected ? `Steam CM: Online` : cm?.connecting ? "Steam CM: Linking" : "Steam CM: Idle";
    pill.classList.toggle("accent", isConnected);
  }
  if (connectBtn) {
    connectBtn.textContent = isConnected ? "Disconnect CM" : "Connect CM";
  }
  if (settingsStatus) settingsStatus.textContent = labelText;
  if (settingsConnectBtn) settingsConnectBtn.classList.toggle("hidden", isConnected);
  if (settingsDisconnectBtn) settingsDisconnectBtn.classList.toggle("hidden", !isConnected);

  const invitesHost = el("friends-invites-list");
  if (invitesHost) {
    const invites = Array.isArray(state.steamPendingInvites) ? state.steamPendingInvites : [];
    invitesHost.classList.toggle("hidden", invites.length === 0);
    invitesHost.innerHTML = invites
      .map(
        (inv) => `
        <div class="friend-invite-card">
          <div class="sidebar-qr-slot-title">${escapeHtml(inv.senderName || inv.senderSteamId)} invited you to ${escapeHtml(inv.gameName || `AppID ${inv.appId}`)}</div>
          <div class="settings-btn-group">
            <button type="button" class="btn btn-primary btn-xs" data-steam-invite-accept="${escapeHtml(inv.inviteId)}">Join Game</button>
            <button type="button" class="btn btn-secondary btn-xs" data-steam-invite-decline="${escapeHtml(inv.inviteId)}">Decline</button>
          </div>
        </div>
      `
      )
      .join("");
  }

  const chatHost = el("friends-chat-log");
  if (chatHost) {
    const msgs = Array.isArray(state.steamChatMessages) ? state.steamChatMessages : [];
    if (msgs.length === 0) {
      chatHost.innerHTML = `<div class="panel-sub">Select or enter a SteamID64 to chat directly over Steam CM.</div>`;
    } else {
      chatHost.innerHTML = msgs
        .slice(-24)
        .map(
          (m) => `
          <div class="friends-chat-line">
            <strong>${escapeHtml(m.senderIsSelf ? "You" : m.peerName || m.peerSteamId)}:</strong>
            <span>${escapeHtml(m.message || "")}</span>
          </div>
        `
        )
        .join("");
      chatHost.scrollTop = chatHost.scrollHeight;
    }
  }
}

async function refreshSteamCmAndInvites() {
  try {
    const [cmStatus, msgs, invites] = await Promise.all([
      getSteamCmStatus().catch(() => null),
      getSteamChatMessages(null).catch(() => []),
      getSteamPendingInvites().catch(() => [])
    ]);
    if (cmStatus) state.steamCmStatus = cmStatus;
    if (Array.isArray(msgs)) state.steamChatMessages = msgs;
    if (Array.isArray(invites)) state.steamPendingInvites = invites;
    renderSteamCmFriendsState();
  } catch {
    // Keep non-fatal Steam CM UI state unchanged.
  }
}

function renderStoreOwnedTitles() {
  const statusEl = el("frontier-store-direct-status");
  if (statusEl && state.storeDirectStatus) {
    const st = state.storeDirectStatus;
    const gogTxt = st.gogConnected ? `GOG: Signed In (${st.gogUserId || "User"})` : "GOG: Signed Out";
    const epicTxt = st.epicConnected ? `Epic: Signed In (${st.epicDisplayName || st.epicAccountId || "User"})` : "Epic: Signed Out";
    statusEl.textContent = `${gogTxt} · ${epicTxt}`;
  }

  const listEl = el("frontier-owned-titles-list");
  if (!listEl) return;
  const combined = [...(state.gogOwnedTitles || []), ...(state.epicOwnedTitles || [])];
  if (combined.length === 0) {
    listEl.innerHTML = "";
    return;
  }

  listEl.innerHTML = combined
    .slice(0, 60)
    .map(
      (item, idx) => `
      <div class="frontier-owned-item">
        <div class="sidebar-qr-slot-title" title="${escapeHtml(item.title || "")}">${escapeHtml(item.title || "Untitled")}</div>
        <div class="sidebar-qr-slot-meta mono-num">${escapeHtml(String(item.store || "").toUpperCase())} · ${item.version ? escapeHtml(item.version) : "Latest"} · ${Number(item.totalSizeBytes || 0) > 0 ? formatBytesJs(item.totalSizeBytes) : "Direct CDN"}</div>
        <button type="button" class="btn btn-primary btn-xs" data-store-install-idx="${idx}">
          ${svgIcon("icon-download")}
          <span>Install via Duke AIMD</span>
        </button>
      </div>
    `
    )
    .join("");
}

async function refreshSuiteAutonomyStrip() {
  try {
    const status = await getSuiteAutonomyStatus();
    state.suiteAutonomyStatus = status;
    const busPill = el("suite-autonomy-bus-pill");
    if (busPill) {
      const conn = Number(status?.connectedCount || 0);
      const total = Number(status?.totalCount || 3);
      busPill.textContent =
        conn > 0
          ? `100% Standalone Active · ${conn}/${total} Optional Companions Linked`
          : "100% Standalone Active · Zero Companion Daemons Required";
    }
    const row = el("suite-autonomy-daemons-row");
    if (row && Array.isArray(status?.daemons)) {
      row.innerHTML = status.daemons
        .map((d) => {
          const connected = Boolean(d?.connected);
          return `
            <div class="suite-daemon-chip ${connected ? "connected" : ""}">
              <span class="suite-daemon-dot"></span>
              <div class="suite-daemon-copy">
                <span class="suite-daemon-name">${escapeHtml(d?.name || "Daemon")} · ${escapeHtml(d?.statusText || "Standby")}</span>
                <span class="suite-daemon-meta">${escapeHtml(d?.role || "")}</span>
              </div>
            </div>
          `;
        })
        .join("");
    }
  } catch {
    // Non-critical daemon strip polling.
  }
}

async function refreshFrontierDiagnostics() {
  refreshSuiteAutonomyStrip().catch(() => {});
  try {
    const [diag, storeStatus] = await Promise.all([
      getFramePacingAndIoringStatus().catch(() => null),
      getStoreDirectStatus().catch(() => null)
    ]);
    if (storeStatus) {
      state.storeDirectStatus = storeStatus;
      renderStoreOwnedTitles();
    }
    if (!diag) return;

    if (diag.quickResume) renderSidebarQuickResume(diag.quickResume);
    if (diag.steamCm) {
      state.steamCmStatus = diag.steamCm;
      renderSteamCmFriendsState();
    }

    if (el("frontier-ioring-status") && diag.ioring) {
      const io = diag.ioring;
      el("frontier-ioring-status").textContent = io.supported
        ? `Active (${io.backend || "CreateIoRing"}, Build ${io.windowsBuild || 0})`
        : `Fallback (${io.backend || "Overlapped"}, Build ${io.windowsBuild || 0})`;
    }
    if (el("frontier-gpu-hags-status")) {
      const gpu = diag.gpuHardware;
      const fp = diag.framePacing;
      const hagsTxt = fp?.hagsStatus || (fp?.hagsEnabled ? "HAGS Enabled" : "HAGS Standard");
      el("frontier-gpu-hags-status").textContent = gpu
        ? `${gpu.adapterName || gpu.gpuKey} (Drv ${gpu.driverVersion || "?"}) · ${hagsTxt}`
        : hagsTxt;
    }
    if (el("frontier-projfs-status") && diag.projfs) {
      const pf = diag.projfs;
      const mounts = Array.isArray(pf.activeMounts) ? pf.activeMounts.length : 0;
      el("frontier-projfs-status").textContent = pf.apiBound
        ? `Ready (ProjectedFSLib Bound · ${mounts} mount${mounts === 1 ? "" : "s"})`
        : pf.dllPresent
          ? "DLL Present · Feature Standby"
          : "Feature Disabled (Click Enable Client-ProjFS)";
    }
    if (el("frontier-pacing-shader-status") && diag.framePacing) {
      const fp = diag.framePacing;
      const actualMs = (Number(fp.actual100nsUnits || 156250) / 10000).toFixed(2);
      el("frontier-pacing-shader-status").textContent = fp.timerResolutionEngaged
        ? `0.5 ms Engaged (${actualMs} ms actual · ${Number(fp.activeGameCount || 0)} game)`
        : `Idle (${actualMs} ms OS cadence)`;
    }
  } catch {
    // Non-critical telemetry refresh.
  }
  refreshWave2Diagnostics().catch(() => {});
}

function setSelectValueSafe(selectEl, rawVal) {
  if (!selectEl) return;
  const target = String(rawVal ?? "");
  const opts = Array.from(selectEl.options || []);
  const exact = opts.find((o) => o.value === target);
  if (exact) {
    selectEl.value = exact.value;
    return;
  }
  const norm = target.toLowerCase().replace(/[^a-z0-9]/g, "");
  const fuzzy = opts.find((o) => o.value.toLowerCase().replace(/[^a-z0-9]/g, "") === norm);
  if (fuzzy) selectEl.value = fuzzy.value;
}

function applyWave2XrayMetrics(m, isLive, cap) {
  const avgEl = el("detail-xray-avg-fps");
  const p1El = el("detail-xray-1pct-fps");
  const p01El = el("detail-xray-01pct-fps");
  const latEl = el("detail-xray-latency-ms");
  const verdictEl = el("detail-xray-bottleneck-text");
  const stutterEl = el("detail-xray-stutters");
  const cpuBar = el("detail-xray-cpu-bar");
  const gpuBar = el("detail-xray-gpu-bar");
  const sourceEl = el("detail-xray-source");
  const livePill = el("detail-xray-live-pill");

  if (livePill) {
    livePill.textContent = isLive
      ? `Live X-Ray · PID ${Number(m?.pid || 0)}`
      : cap?.modeLabel || "Anti-Cheat Safe (Zero DLL Injection)";
  }

  if (!m) {
    if (avgEl) avgEl.textContent = "—";
    if (p1El) p1El.textContent = "—";
    if (p01El) p01El.textContent = "—";
    if (latEl) latEl.textContent = "—";
    if (verdictEl) verdictEl.textContent = "No gameplay session captured yet";
    if (stutterEl) stutterEl.textContent = "0 Spikes";
    if (cpuBar) cpuBar.style.width = "50%";
    if (gpuBar) gpuBar.style.width = "50%";
    if (sourceEl) sourceEl.textContent = cap?.modeLabel || "ETW Consumer Ready (Zero Injection)";
    return;
  }

  const low01 = m.low01pctFps ?? m.low0_1pctFps ?? 0;
  if (avgEl) avgEl.textContent = `${Number(m.avgFps || 0).toFixed(1)} FPS`;
  if (p1El) p1El.textContent = `${Number(m.low1pctFps || 0).toFixed(1)} FPS`;
  if (p01El) p01El.textContent = `${Number(low01 || 0).toFixed(1)} FPS`;
  if (latEl) latEl.textContent = `${Number(m.gpuPresentLatencyMs || 0).toFixed(2)} ms`;

  let cpuPct = Number(m.cpuBoundPct);
  let gpuPct = Number(m.gpuBoundPct);
  if (!Number.isFinite(cpuPct) || !Number.isFinite(gpuPct) || cpuPct + gpuPct <= 0) {
    const ratio = Math.max(0.05, Number(m.cpuGpuBottleneckRatio || 1.0));
    cpuPct = Math.max(5, Math.min(95, (100 * ratio) / (1 + ratio)));
    gpuPct = 100 - cpuPct;
  }
  if (cpuBar) cpuBar.style.width = `${cpuPct.toFixed(0)}%`;
  if (gpuBar) gpuBar.style.width = `${gpuPct.toFixed(0)}%`;
  if (verdictEl) {
    verdictEl.textContent = `${m.bottleneckVerdict || "Balanced"} (${cpuPct.toFixed(0)}% CPU / ${gpuPct.toFixed(0)}% GPU)`;
  }
  if (stutterEl) {
    const resets = Number(m.gpuResetEvents || 0);
    stutterEl.textContent =
      resets > 0
        ? `${Number(m.shaderStutterSpikes || 0)} Spikes · ${resets} GPU Reset`
        : `${Number(m.shaderStutterSpikes || 0)} Stutter Spikes`;
  }
  if (sourceEl) {
    sourceEl.textContent = `${m.telemetrySource || "ETW / DWM"} · ${Number(
      m.totalFramesSampled || m.sampleCount || 0
    )} frames`;
  }
  const corrEl = el("detail-xray-correlations");
  if (corrEl) {
    const corr = Array.isArray(m.spikeCorrelations) ? m.spikeCorrelations : [];
    corrEl.textContent =
      corr.length > 0 ? `Correlated spike triggers: ${corr.join(" · ")}` : "No background spike or TDR culprits flagged.";
  }
}

function renderDetailXrayScorecards(report, cap, game) {
  state.detailXrayHistory = report || null;
  const activeTick =
    state.detailXrayLiveTick && game && state.detailXrayLiveTick.gameId === game.id
      ? state.detailXrayLiveTick
      : null;
  const currentCard = activeTick || report?.latest || null;
  applyWave2XrayMetrics(currentCard, Boolean(activeTick), cap);

  const suggEl = el("detail-xray-suggestions");
  if (suggEl) {
    const suggs = Array.isArray(report?.latest?.optimizationSuggestions)
      ? report.latest.optimizationSuggestions
      : [];
    suggEl.innerHTML = suggs
      .map(
        (s) => `
        <div class="wave2-xray-suggestion-row">
          <div>
            <strong>${escapeHtml(s.title || "Optimize")}</strong>
            <div class="detail-micro-caption">${escapeHtml(s.rationale || "")}</div>
          </div>
          <button type="button" class="btn btn-secondary btn-xs" data-xray-action="${escapeHtml(
            s.actionKind || ""
          )}" data-xray-game-id="${escapeHtml(game?.id || "")}">Apply Fix</button>
        </div>
      `
      )
      .join("");
  }

  const histEl = el("detail-xray-history-list");
  if (!histEl) return;
  const history = Array.isArray(report?.history) ? report.history : [];
  if (history.length === 0) {
    histEl.innerHTML = "";
    return;
  }
  histEl.innerHTML = history
    .slice(0, 6)
    .map((card) => {
      const when = card.recordedAtEpochSec
        ? new Date(card.recordedAtEpochSec * 1000).toLocaleString()
        : "Recent Session";
      const low01 = card.low01pctFps ?? card.low0_1pctFps ?? 0;
      return `
        <div class="wave2-history-item">
          <span>${escapeHtml(when)} · ${Number(card.durationSec || 0).toFixed(0)}s · ${escapeHtml(
        card.bottleneckVerdict || "Balanced"
      )}</span>
          <span class="mono-num">Avg ${Number(card.avgFps || 0).toFixed(1)} · 1% ${Number(
        card.low1pctFps || 0
      ).toFixed(1)} · 0.1% ${Number(low01 || 0).toFixed(1)} FPS · ${Number(
        card.shaderStutterSpikes || 0
      )} spikes</span>
        </div>
      `;
    })
    .join("");
}

async function loadDetailXrayScorecards(game, requestId) {
  if (!game?.id) return;
  try {
    const [report, cap] = await Promise.all([
      getGameXrayScorecards(game.id),
      probeEtwXrayCapability().catch(() => null)
    ]);
    if (state.activeDetailRequestId !== requestId || state.selectedDetailId !== game.id) return;
    renderDetailXrayScorecards(report, cap, game);
  } catch (err) {
    if (el("detail-xray-source")) el("detail-xray-source").textContent = String(err);
  }
}

function renderDetailDisplayTuning(upscaleStatus, latencyProfile) {
  state.detailUpscaleStatus = upscaleStatus || null;
  state.detailLatencyProfile = latencyProfile || null;
  const prof = upscaleStatus?.profile || {};
  const plan = upscaleStatus?.viewportPlan || {};

  const emuPill = el("detail-upscale-emu-pill");
  if (emuPill) {
    const fam = prof.detectedEmulatorFamily;
    emuPill.textContent = fam ? `Emulator Detected: ${fam}` : "Native PC Title";
  }

  if (el("detail-upscale-borderless")) {
    el("detail-upscale-borderless").checked = Boolean(prof.borderlessFlipPromote);
  }
  setSelectValueSafe(el("detail-upscale-mode"), prof.upscaleMode || "off");
  if (el("detail-upscale-fps-cap")) {
    el("detail-upscale-fps-cap").value = String(Number(prof.fpsCap || 0));
  }
  if (el("detail-upscale-autohdr")) {
    el("detail-upscale-autohdr").checked = Boolean(
      prof.autoHdr || latencyProfile?.autoHdrEnable
    );
  }
  const sharpEl = el("detail-upscale-sharpness");
  if (sharpEl) {
    const rawSharp = Number.isFinite(Number(prof.fsrSharpness)) ? Number(prof.fsrSharpness) : 0.75;
    sharpEl.value = Number(sharpEl.max || 1) > 1.5 ? String(Math.round(rawSharp * 100)) : rawSharp.toFixed(2);
  }

  setSelectValueSafe(
    el("detail-latency-gpu-pref"),
    latencyProfile?.gpuPreference !== undefined ? String(latencyProfile.gpuPreference) : "2"
  );
  setSelectValueSafe(el("detail-latency-autosr"), latencyProfile?.autoSrMode || "off");
  if (el("detail-latency-flip-upgrade")) {
    el("detail-latency-flip-upgrade").checked = latencyProfile?.flipModelUpgrade ?? true;
  }

  const readout = el("detail-upscale-plan-readout");
  if (readout) {
    const capTxt = Number(prof.fpsCap || 0) > 0 ? `${Number(prof.fpsCap)} FPS Cap` : "Uncapped";
    const factorTxt = Number(plan.integerFactor || 1) > 1 ? ` · ${plan.integerFactor}x Integer` : "";
    const pacerTxt = upscaleStatus?.pacerActive
      ? ` · Pacer Active (${Number(upscaleStatus.timerResolutionMs || 0.5).toFixed(2)} ms)`
      : "";
    readout.textContent = `${plan.modeLabel || "Native"} · ${Number(plan.sourceWidth || 1280)}x${Number(
      plan.sourceHeight || 720
    )} -> ${Number(plan.scaledWidth || 1920)}x${Number(plan.scaledHeight || 1080)}${factorTxt} · ${capTxt}${pacerTxt}`;
  }
}

async function loadDetailDisplayTuning(game, requestId) {
  if (!game?.id) return;
  try {
    const [upscale, lat] = await Promise.all([
      getGameUpscaleProfile(game.id, game.exePath || "", game.emulatorId || null),
      game.exePath ? getGameLatencyProfile(game.exePath).catch(() => null) : Promise.resolve(null)
    ]);
    if (state.activeDetailRequestId !== requestId || state.selectedDetailId !== game.id) return;
    renderDetailDisplayTuning(upscale, lat);
  } catch (err) {
    if (el("detail-upscale-plan-readout")) el("detail-upscale-plan-readout").textContent = String(err);
  }
}

function renderDetailDirectStorage(scan) {
  state.detailDirectStorageScan = scan || null;
  const pill = el("detail-ds-backend-pill");
  const status = el("detail-ds-status");
  const readout = el("detail-ds-readout");
  const cap = scan?.capability;
  if (pill) {
    pill.textContent = cap?.gpuDecompressionReady
      ? `DS GPU Ready (${cap.activeBackend || "DirectStorage"})`
      : cap?.activeBackend || "CPU Zstd Fallback";
  }
  if (!scan) {
    if (status) status.textContent = "DirectStorage 1.4 / Zstd Stream Scanner Ready";
    if (readout) readout.textContent = "Inspects .pak, .ba2, and .forge archives before repacking.";
    return;
  }
  const candCount = Array.isArray(scan.candidates) ? scan.candidates.length : 0;
  const blerTex = Number(scan.blerTextureCandidates || 0);
  if (status) {
    status.textContent = `${candCount} asset archive${candCount === 1 ? "" : "s"} · ${formatBytesJs(
      scan.eligibleUncompressedBytes || 0
    )} eligible (${blerTex} BC1–BC5 BLER)`;
  }
  if (readout) {
    readout.textContent = `${Number(scan.existingPackedStreams || 0)} .dkds streams (${formatBytesJs(
      scan.existingPackedBytes || 0
    )}) · ${cap?.summary || ""}`;
  }
}

function renderDetailVhdxStage(resp) {
  state.detailVhdxState = resp || null;
  const [cap, entry] = Array.isArray(resp) ? resp : [null, null];
  const statusEl = el("detail-vhdx-status");
  const readoutEl = el("detail-vhdx-readout");
  const autoEl = el("detail-vhdx-autostage");

  if (autoEl) autoEl.checked = Boolean(entry?.autoStageOnLaunch);
  if (!entry) {
    if (statusEl) {
      statusEl.textContent = cap?.virtdiskDllLoaded
        ? "VirtDisk.dll + IoRing Ready for Containerization"
        : "VirtDisk Fallback Archive Mode Ready";
    }
    if (readoutEl) {
      readoutEl.textContent = cap?.hotTierRoot ? `Hot-Tier Root: ${cap.hotTierRoot}` : "";
    }
    return;
  }
  if (statusEl) {
    statusEl.textContent = entry.hotStaged
      ? `Hot-Tier Staged (${entry.hotStagedFiles} files · ${formatBytesJs(entry.hotStagedBytes || 0)})`
      : entry.vhdxPath
      ? `Containerized (${entry.attachMode || "VHDX"}) · ${formatBytesJs(entry.vhdxFileBytes || 0)}`
      : `${entry.totalFiles} files (${entry.smallFilesUnder1Mb} < 1 MiB) on ${entry.sourceVolume || "Drive"}`;
  }
  if (readoutEl) {
    const tierNote = entry.sourceOnHdd ? "Secondary/HDD candidate" : "NVMe source";
    readoutEl.textContent = `${tierNote} · IoRing ${cap?.ioringAvailable ? "Active" : "Fallback"} · ${
      entry.vhdxPath || cap?.hotTierRoot || ""
    }`;
  }
}

function renderDetailDeepShrink(report) {
  state.detailDeepShrinkScan = report || null;
  const statusEl = el("detail-shrink-status");
  const bucketsEl = el("detail-shrink-buckets");
  if (!report) {
    if (statusEl) {
      statusEl.textContent = "Scan unused foreign voiceover & cutscene banks before creating a .duke-cas restore point.";
    }
    if (bucketsEl) bucketsEl.innerHTML = "";
    return;
  }
  const buckets = Array.isArray(report.languageBuckets) ? report.languageBuckets : [];
  if (statusEl) {
    const texCount = Number(report.texturePackFiles || 0);
    const blerFmt = report.estimatedBlerSavingsFormatted || formatBytesJs(report.estimatedBlerSavingsBytes || 0);
    statusEl.textContent = `${report.totalReclaimableFormatted || "0 B"} reclaimable (${
      buckets.length
    } non-English packs · ${Number(report.duplicateMediaFiles || 0)} duplicate media · ${texCount} BCn BLER [${blerFmt}])`;
  }
  if (!bucketsEl) return;
  if (buckets.length === 0) {
    bucketsEl.innerHTML = `<span class="detail-micro-caption">No foreign voiceover packs detected in this folder.</span>`;
    return;
  }
  bucketsEl.innerHTML = buckets
    .map(
      (b) => `
      <label class="wave2-shrink-bucket-chip" title="${escapeHtml((b.sampleFiles || []).slice(0, 3).join(", "))}">
        <input type="checkbox" data-shrink-lang="${escapeHtml(b.language)}" checked />
        <span>${escapeHtml(b.language)}</span>
        <span class="mono-num">(${escapeHtml(b.totalFormatted || formatBytesJs(b.totalBytes || 0))})</span>
      </label>
    `
    )
    .join("");
}

async function loadDetailStorageV2(game, requestId) {
  if (!game?.id) return;
  renderDetailDeepShrink(
    state.detailDeepShrinkScan?.gameId === game.id ? state.detailDeepShrinkScan : null
  );
  try {
    const [dsScan, vhdxResp] = await Promise.all([
      scanDirectStorageAssets(game.id, null).catch(() => null),
      getVhdxStageStatus(game.id).catch(() => null)
    ]);
    if (state.activeDetailRequestId !== requestId || state.selectedDetailId !== game.id) return;
    if (dsScan) renderDetailDirectStorage(dsScan);
    else renderDetailDirectStorage(null);
    renderDetailVhdxStage(vhdxResp);
  } catch {
    // Non-critical storage v2 probe.
  }
}

async function refreshWave2Diagnostics() {
  try {
    const [dsCap, latStatus, etwCap] = await Promise.all([
      probeDirectStorageCapability(null).catch(() => null),
      getLatencyLabStatus().catch(() => null),
      probeEtwXrayCapability().catch(() => null)
    ]);
    state.latencyLabStatus = latStatus || null;

    if (el("wave2-ds-capability") && dsCap) {
      el("wave2-ds-capability").textContent = `${dsCap.activeBackend} · ${dsCap.gpuName} (${
        dsCap.d3d12DeviceReady ? "D3D12 Ready" : "D3D12 Fallback"
      })`;
    }
    if (el("wave2-ds-dlls") && dsCap) {
      const ds = dsCap.dstorageDll?.loaded ? "dstorage.dll OK" : "dstorage.dll Absent";
      const dsc = dsCap.dstorageCoreDll?.loaded ? "dstoragecore.dll OK" : "core Absent";
      const gacl = dsCap.gaclDll?.loaded ? "gacl.dll OK" : "gacl.dll Absent";
      const etwTxt = etwCap ? ` · ETW: ${etwCap.modeLabel}` : "";
      el("wave2-ds-dlls").textContent = `${ds} · ${dsc} · ${gacl}${etwTxt}`;
    }
    if (el("wave2-mpo-status") && latStatus?.mpo) {
      const mpo = latStatus.mpo;
      el("wave2-mpo-status").textContent = `${
        mpo.mpoEnabled ? "MPO Active" : "MPO Disabled"
      } (${Number(mpo.dxgiOutputsDetected || 0)} outputs) · ${mpo.recommendation || ""}`;
    }
    if (el("wave2-driver-guard-status") && latStatus) {
      el("wave2-driver-guard-status").textContent = `Driver ${
        latStatus.driverVersion || "Unknown"
      } · ${Number(latStatus.savedProfileCount || 0)} guarded .exe profiles (${Number(
        latStatus.registryEntryCount || 0
      )} HKCU entries, ${Number(latStatus.guardReapplyCount || 0)} re-applies)`;
    }
    if (el("wave2-shader-cap-status") && latStatus) {
      el("wave2-shader-cap-status").textContent = `${
        latStatus.shaderCacheTotalFormatted || "0 B"
      } across vendor caches (Cap: ${Number(latStatus.shaderCacheCapMb || 4096)} MB)`;
    }
    if (
      el("input-wave2-shader-cap-mb") &&
      latStatus?.shaderCacheCapMb &&
      document.activeElement !== el("input-wave2-shader-cap-mb")
    ) {
      el("input-wave2-shader-cap-mb").value = String(latStatus.shaderCacheCapMb);
    }
  } catch {
    // Non-critical Wave 2 Settings diagnostics.
  }
}

function renderDoctorDiagnosisReport(rep) {
  const statusPill = el("detail-doctor-status");
  const reportEl = el("detail-doctor-report");
  const revertBtn = el("btn-detail-doctor-revert");
  if (!rep) {
    if (statusPill) {
      statusPill.className = "pill mono-num";
      statusPill.textContent = "Ready";
    }
    if (revertBtn) revertBtn.classList.add("hidden");
    return;
  }
  state.detailDoctorReport = rep;
  if (statusPill) {
    const tone = rep.severity === "critical" ? "danger" : rep.severity === "warning" ? "gold" : "emerald";
    statusPill.className = `pill ${tone} mono-num`;
    statusPill.textContent = String(rep.severity || "healthy").toUpperCase();
  }
  if (revertBtn) {
    if (rep.canRevertSnapshot && rep.latestSnapshot?.snapshotId) {
      revertBtn.classList.remove("hidden");
      revertBtn.dataset.snapshotId = rep.latestSnapshot.snapshotId;
    } else {
      revertBtn.classList.add("hidden");
      delete revertBtn.dataset.snapshotId;
    }
  }
  if (!reportEl) return;
  const conflicts = Array.isArray(rep.modConflicts) ? rep.modConflicts : [];
  const actions = Array.isArray(rep.recommendedActions) ? rep.recommendedActions : [];
  const latestArt = Array.isArray(rep.artifacts) && rep.artifacts.length > 0 ? rep.artifacts[0] : null;
  reportEl.innerHTML = `
    <div class="wave3-kv"><strong>Summary:</strong> ${escapeHtml(rep.summary || "No critical faults.")}</div>
    <div class="wave3-kv"><strong>Root Cause:</strong> ${escapeHtml(rep.rootCause || "Healthy")}</div>
    <div class="wave3-kv mono-num"><strong>Adapter:</strong> ${escapeHtml(rep.gpuAdapter || "DXGI")} · ${escapeHtml(rep.gpuDriver || "Auto")} · ${escapeHtml(rep.aiBackend || "NPU Heuristic")}</div>
    ${rep.faultingModule || rep.offendingMod || rep.exceptionCode ? `<div class="wave3-kv mono-num"><strong>Fault:</strong> ${escapeHtml(rep.exceptionCode || "N/A")} · Mod: ${escapeHtml(rep.offendingMod || "N/A")} · DLL: ${escapeHtml(rep.faultingModule || "N/A")}</div>` : ""}
    ${conflicts.length > 0 ? `<div class="wave3-kv"><strong>Mod Conflicts:</strong> ${escapeHtml(conflicts.slice(0, 2).join(" | "))}</div>` : ""}
    ${latestArt?.excerpt ? `<div class="wave3-excerpt mono-num">${escapeHtml(latestArt.excerpt.slice(0, 220))}</div>` : ""}
    ${actions.length > 0 ? `<div class="wave3-kv"><strong>Fix:</strong> ${escapeHtml(actions[0])}</div>` : ""}
  `;
}

function renderConfiguratorPlanReport(plan) {
  const hwBadge = el("detail-cfg-hw-badge");
  const preview = el("detail-cfg-preview");
  const rollbackBtn = el("btn-detail-cfg-rollback");
  if (!plan) return;
  state.detailConfigPlan = plan;
  const hw = plan.hardware || {};
  if (hwBadge) {
    hwBadge.textContent = `${hw.adapterName || "DXGI"} · ${hw.vramMb || 0} MB · ${hw.displayHz || 60} Hz`;
  }
  if (rollbackBtn) {
    rollbackBtn.disabled = !plan.hasBackup;
    rollbackBtn.dataset.backupId = plan.latestBackupId || "";
  }
  if (!preview) return;
  const targets = Array.isArray(plan.targets) ? plan.targets : [];
  const totalPatches = targets.reduce((acc, t) => acc + (Array.isArray(t.patches) ? t.patches.length : 0), 0);
  if (targets.length === 0) {
    preview.innerHTML = `<div class="panel-sub">Tier: <strong>${escapeHtml(plan.tierLabel || "Balanced")}</strong> (${Number(plan.targetHz || 120)} Hz). Launch once to generate Engine.ini / prefs.ini files.</div>`;
    return;
  }
  preview.innerHTML = `
    <div class="wave3-kv"><strong>Preset:</strong> ${escapeHtml(plan.tierLabel || "Balanced")} (${Number(plan.targetHz || 120)} Hz) · ${targets.length} config(s) · ${totalPatches} patches</div>
    ${targets
      .slice(0, 2)
      .map(
        (t) => `<div class="wave3-excerpt mono-num">${escapeHtml(t.filePath)} -> ${(t.patches || [])
          .slice(0, 3)
          .map((p) => `${p.key}=${p.value}`)
          .join(", ")}</div>`
      )
      .join("")}
  `;
}

function renderDirectorStateReport(rep) {
  if (!rep) return;
  const pill = el("detail-director-score-pill");
  const chk = el("chk-detail-director-enabled");
  const sens = el("range-detail-director-sens");
  const sensVal = el("detail-director-sens-val");
  const host = el("detail-director-clips");
  if (pill) {
    const score = Number(rep.fusedIntensityScore || 0);
    pill.className = `pill ${score >= 70 ? "emerald" : "gold"} mono-num`;
    pill.textContent = `Intensity ${score}%`;
  }
  if (chk) chk.checked = Boolean(rep.autoDirectorEnabled);
  if (sens && rep.sensitivityThreshold != null) {
    sens.value = String(rep.sensitivityThreshold);
  }
  if (sensVal && rep.sensitivityThreshold != null) {
    sensVal.textContent = `${Number(rep.sensitivityThreshold)}%`;
  }
  if (!host) return;
  const clips = Array.isArray(rep.highlights) ? rep.highlights : [];
  if (clips.length === 0) {
    host.innerHTML = `<div class="panel-sub">No auto-tagged highlights recorded yet. Play or click Trim &amp; Save Highlight.</div>`;
    return;
  }
  host.innerHTML = clips
    .slice(0, 4)
    .map(
      (c) => `
      <div class="wave2-history-item">
        <div>
          <strong>${escapeHtml(c.bossOrAchievement || c.gameTitle || "Highlight")}</strong>
          <div class="panel-sub mono-num">${Number(c.durationSecs || 22).toFixed(1)}s · trimmed ${Number(c.deadFramesTrimmedSecs || 0).toFixed(1)}s menu · ${escapeHtml((c.tags || []).slice(0, 3).join(", "))}</div>
        </div>
        <span class="pill ${c.intensity >= 75 ? "emerald" : "gold"} mono-num">${Number(c.intensity || 0)}%</span>
      </div>`
    )
    .join("");
}

async function refreshWave3AiDiagnostics() {
  try {
    const st = await getAiRuntimeStatus().catch(() => null);
    if (!st) return;
    state.wave3AiStatus = st;
    const cfg = st.config || {};
    const probe = st.probe || {};
    if (el("wave3-ai-provider-pill")) {
      el("wave3-ai-provider-pill").textContent = `EP: ${st.activeExecutionProvider || "CPU Fallback"}`;
      el("wave3-ai-provider-pill").className = `pill ${st.zeroVramMode ? "emerald" : "gold"} mono-num`;
    }
    if (el("wave3-ai-vram-pill")) {
      el("wave3-ai-vram-pill").textContent = `AI VRAM: ${Number(st.vramAllocatedMb || 0).toFixed(1)} MB`;
    }
    if (el("wave3-ai-ram-pill")) {
      el("wave3-ai-ram-pill").textContent = `Process RAM: ${Number(st.processWorkingSetMb || 0).toFixed(1)} MB (Workers ${Number(st.workerRamMb || 0).toFixed(1)} MB)`;
    }
    if (el("wave3-ocr-badge")) {
      const ocrStr = probe.oneocrAvailable ? "OneOCR (<15ms)" : probe.winrtOcrAvailable ? "WinRT Media.Ocr" : "Heuristic OCR";
      el("wave3-ocr-badge").textContent = `OCR: ${ocrStr}`;
    }
    if (el("detail-ai-runtime-pill")) {
      el("detail-ai-runtime-pill").textContent = `${st.activeExecutionProvider || "NPU"} · ${Number(st.vramAllocatedMb || 0).toFixed(1)} MB VRAM`;
    }
    setSelectValueSafe(el("select-wave3-provider"), cfg.preferredProvider || "auto");
    setSelectValueSafe(el("select-wave3-target-lang"), cfg.targetLanguage || "en");
    if (el("input-wave3-foundry-endpoint")) el("input-wave3-foundry-endpoint").value = cfg.foundryEndpoint || "http://127.0.0.1:5272/v1";
    if (el("input-wave3-foundry-model")) el("input-wave3-foundry-model").value = cfg.foundryModel || "phi-3.5-mini-instruct-npu";
    if (el("input-wave3-idle-ttl")) el("input-wave3-idle-ttl").value = String(cfg.idleTtlSecs || 45);
    if (el("chk-wave3-auto-unload")) el("chk-wave3-auto-unload").checked = cfg.autoUnloadEnabled !== false;
  } catch {
    // Non-critical telemetry probe.
  }
}

async function loadDetailAiSuite(game, requestId) {
  if (!game) return;
  state.detailDoctorReport = null;
  renderDoctorDiagnosisReport(null);
  if (el("detail-doctor-report")) {
    el("detail-doctor-report").innerHTML = `<div class="panel-sub">Click Diagnose Crashes to parse Crash.log, BepInEx, CrashContext XML, .dmp files, and mod conflicts.</div>`;
  }
  const hz = Number(el("select-detail-cfg-hz")?.value || 120) || 120;
  const [plan, dir, aiSt] = await Promise.all([
    analyzeGameConfigPreset(game.id, hz).catch(() => null),
    getDukeDirectorState(game.id).catch(() => null),
    getAiRuntimeStatus().catch(() => null)
  ]);
  if (requestId !== state.activeDetailRequestId) return;
  if (plan) renderConfiguratorPlanReport(plan);
  if (dir) renderDirectorStateReport(dir);
  if (aiSt && el("detail-ai-runtime-pill")) {
    el("detail-ai-runtime-pill").textContent = `${aiSt.activeExecutionProvider || "NPU"} · ${Number(aiSt.vramAllocatedMb || 0).toFixed(1)} MB VRAM`;
  }
}

function loadAllDetailPanels(game, requestId) {
  if (!game) return;
  if (el("detail-ach-path")) el("detail-ach-path").value = game.achievementsPath || "";
  renderDetailUpdateState(game, null);
  loadDetailJailStatus(game, requestId);
  loadDetailExtendedTelemetry(game, requestId, false);
  loadDetailAchievements(game, requestId);
  if (Number(game.platformId || 0) > 0) {
    loadDetailRetroAchievements(game, requestId, false);
  }
  loadDetailCompat(game, requestId);
  loadDetailWofStorage(game, requestId);
  loadDetailModGraph(game, requestId);
  loadDetailTimeMachine(game, requestId);
  loadDetailGhosts(game, requestId);
  loadDetailXrayScorecards(game, requestId);
  loadDetailDisplayTuning(game, requestId);
  loadDetailStorageV2(game, requestId);
  loadDetailAiSuite(game, requestId);
}

function renderSelectedTimeMachineCheckpoint() {
  const host = el("detail-tm-selected-card");
  const readout = el("detail-tm-scrubber-readout");
  const tm = state.detailTimeMachine;
  const list = Array.isArray(tm?.checkpoints) ? tm.checkpoints : [];
  if (list.length === 0) {
    if (readout) readout.textContent = "0 / 0 checkpoints";
    if (host) {
      host.innerHTML = `<div class="panel-sub">No save checkpoints recorded yet. Play this game or click Create Checkpoint to start your timeline.</div>`;
    }
    return;
  }
  const idx = Math.max(0, Math.min(state.detailTimeMachineSelectedIdx || 0, list.length - 1));
  state.detailTimeMachineSelectedIdx = idx;
  const cp = list[idx];
  if (readout) {
    readout.textContent = `Snapshot ${idx + 1} / ${list.length} · branch ${cp.branch || "main"}`;
  }
  if (!host) return;
  const thumbHtml = cp.thumbnailDataUrl
    ? `<img src="${cp.thumbnailDataUrl}" alt="Checkpoint preview" class="tm-selected-thumb" />`
    : `<div class="tm-selected-thumb">No Preview</div>`;
  const when = cp.timestampSecs ? new Date(cp.timestampSecs * 1000).toLocaleString() : "Just now";
  const dedupSaved = Number(cp.dedupSavedBytes || 0);
  const newBytes = Number(cp.newCasBytes || 0);
  host.innerHTML = `
    ${thumbHtml}
    <div class="tm-selected-meta">
      <div class="tm-selected-title-row">
        <strong>${escapeHtml(cp.label || "Save Checkpoint")}</strong>
        <span class="pill pill-accent mono-num">${escapeHtml(cp.branch || "main")}</span>
        <span class="pill mono-num">${escapeHtml(cp.trigger || "manual")}</span>
      </div>
      <div class="panel-sub mono-num">${escapeHtml(when)} · Playtime ${escapeHtml(formatPlaytime(cp.playtimeSecs || 0))}</div>
      <div class="panel-sub mono-num">${Number(cp.fileCount || 0)} files · ${formatBytesJs(cp.totalBytes || 0)} logical (+${formatBytesJs(newBytes)} new CAS, ${formatBytesJs(dedupSaved)} deduplicated)</div>
    </div>
    <div class="tm-selected-actions">
      <button type="button" class="btn btn-primary btn-xs" data-tm-action="restore" data-tm-id="${escapeHtml(cp.id)}">
        <svg class="icon" aria-hidden="true"><use href="#icon-rotate-ccw"/></svg>
        <span>1-Click Rollback</span>
      </button>
      <button type="button" class="btn btn-secondary btn-xs" data-tm-action="delete" data-tm-id="${escapeHtml(cp.id)}">
        <svg class="icon" aria-hidden="true"><use href="#icon-trash-2"/></svg>
        <span>Delete</span>
      </button>
    </div>
  `;
}

function renderDetailTimeMachine(report) {
  state.detailTimeMachine = report || null;
  const checkpoints = Array.isArray(report?.checkpoints) ? report.checkpoints : [];
  state.detailTimeMachineSelectedIdx = 0;

  if (el("detail-tm-summary")) {
    const rootsCount = Array.isArray(report?.discoveredSaveRoots) ? report.discoveredSaveRoots.length : 0;
    const ratio = Number(report?.dedupRatioPct || 0).toFixed(1);
    el("detail-tm-summary").textContent =
      rootsCount > 0
        ? `Watching ${rootsCount} save folder${rootsCount === 1 ? "" : "s"} · ${ratio}% CAS deduplication across ${checkpoints.length} checkpoints.`
        : "Content-addressed BLAKE3 save checkpoints (.duke-cas), 160x90 visual previews & Git-style save branches.";
  }
  if (el("detail-tm-dedup-pill")) {
    el("detail-tm-dedup-pill").textContent = `${formatBytesJs(report?.totalCasBytes || 0)} CAS`;
  }
  const branchSelect = el("detail-tm-branch-select");
  if (branchSelect) {
    const branches = Array.isArray(report?.branches) ? report.branches : [{ name: "main" }];
    const active = report?.currentBranch || "main";
    branchSelect.innerHTML = branches
      .map(
        (b) =>
          `<option value="${escapeHtml(b.name)}"${b.name === active ? " selected" : ""}>branch: ${escapeHtml(b.name)}</option>`
      )
      .join("");
  }
  const scrubber = el("detail-tm-scrubber-range");
  if (scrubber) {
    scrubber.disabled = checkpoints.length <= 1;
    scrubber.min = "0";
    scrubber.max = String(Math.max(0, checkpoints.length - 1));
    scrubber.value = "0";
  }

  renderSelectedTimeMachineCheckpoint();

  const strip = el("detail-tm-timeline-list");
  if (!strip) return;
  if (checkpoints.length === 0) {
    strip.innerHTML = "";
    return;
  }
  strip.innerHTML = checkpoints
    .map((cp, idx) => {
      const thumb = cp.thumbnailDataUrl
        ? `<img src="${cp.thumbnailDataUrl}" alt="" class="tm-node-thumb" loading="lazy" />`
        : "";
      return `
        <button type="button" class="tm-node-chip${idx === 0 ? " active" : ""}" data-tm-idx="${idx}">
          ${thumb}
          <span class="tm-node-label">${escapeHtml(cp.label || "Checkpoint")}</span>
          <span class="panel-sub mono-num">${escapeHtml(cp.branch || "main")} · ${formatBytesJs(cp.totalBytes || 0)}</span>
        </button>
      `;
    })
    .join("");
}

async function loadDetailTimeMachine(game, requestId) {
  if (!game?.id) return;
  try {
    const report = await getGameTimeMachine(game.id);
    if (state.activeDetailRequestId !== requestId || state.selectedDetailId !== game.id) return;
    renderDetailTimeMachine(report);
  } catch (err) {
    if (el("detail-tm-summary")) {
      el("detail-tm-summary").textContent = String(err);
    }
  }
}

function renderDetailGhosts(ghosts = null, status = null) {
  if (Array.isArray(ghosts)) state.detailGhosts = ghosts;
  if (status) state.detailGhostStatus = status;

  const list = Array.isArray(state.detailGhosts) ? state.detailGhosts : [];
  const st = state.detailGhostStatus;
  const activeMode = String(st?.mode || "idle").toLowerCase();

  const pill = el("detail-ghost-mode-pill");
  if (pill) {
    if (activeMode === "recording") {
      pill.textContent = "Recording ~60 Hz";
      pill.className = "pill danger mono-num";
    } else if (activeMode === "playing") {
      pill.textContent = st?.pausedFocusLost ? "Paused (Focus Lost)" : "Playing Ghost";
      pill.className = st?.pausedFocusLost ? "pill warn mono-num" : "pill accent mono-num";
    } else {
      pill.textContent = "Idle";
      pill.className = "pill mono-num";
    }
  }

  const statusLine = el("detail-ghost-status");
  if (statusLine) {
    if (st?.statusText) {
      statusLine.textContent = st.statusText;
    } else {
      statusLine.textContent =
        "Compact (~10 KB/min) XInput/HID stream synced with Time-Machine save states · auto-pauses when game loses focus.";
    }
  }

  const recBtn = el("btn-detail-ghost-record");
  const stopBtn = el("btn-detail-ghost-stop");
  const isBusy = activeMode === "recording" || activeMode === "playing";
  if (recBtn) recBtn.classList.toggle("hidden", isBusy);
  if (stopBtn) {
    stopBtn.classList.toggle("hidden", !isBusy);
    const span = stopBtn.querySelector("span");
    if (span) span.textContent = activeMode === "recording" ? "Stop & Save" : "Stop Ghost";
  }

  const host = el("detail-ghost-list");
  if (!host) return;
  if (list.length === 0) {
    host.innerHTML = `<div class="panel-sub">No .dkghost recordings saved for this game yet.</div>`;
    return;
  }

  host.innerHTML = list
    .map((g) => {
      const secs = (Number(g.durationMs || 0) / 1000).toFixed(1);
      const kbps = Number(g.kbPerMinute || 0).toFixed(1);
      const tmTag = g.timeMachineCheckpointId
        ? `<span class="pill accent mono-num">TM Linked</span>`
        : "";
      return `
        <div class="wave2-history-row">
          <div>
            <strong>${escapeHtml(g.label || "Ghost Run")}</strong> ${tmTag}
            <div class="panel-sub mono-num">${secs}s · ${g.frameCount || 0} delta frames · ${escapeHtml(g.fileSizeFormatted || "0 KB")} (${kbps} KB/min)</div>
          </div>
          <div class="detail-save-actions">
            <button type="button" class="btn btn-primary btn-xs" data-ghost-action="play" data-ghost-id="${escapeHtml(g.ghostId)}" title="Restore synced save & replay input stream">
              ${svgIcon("icon-play")}<span>Play</span>
            </button>
            <button type="button" class="btn btn-secondary btn-xs" data-ghost-action="export" data-ghost-id="${escapeHtml(g.ghostId)}" data-ghost-label="${escapeHtml(g.label || "ghost")}" title="Export shareable .dkghost">
              ${svgIcon("icon-download")}
            </button>
            <button type="button" class="btn btn-secondary btn-xs" data-ghost-action="delete" data-ghost-id="${escapeHtml(g.ghostId)}" title="Delete .dkghost">
              ${svgIcon("icon-trash-2")}
            </button>
          </div>
        </div>
      `;
    })
    .join("");
}

async function loadDetailGhosts(game, requestId) {
  if (!game?.id) return;
  try {
    const [ghosts, st] = await Promise.all([
      listGameGhosts(game.id),
      getGhostStatus(game.id).catch(() => null)
    ]);
    if (state.activeDetailRequestId !== requestId || state.selectedDetailId !== game.id) return;
    renderDetailGhosts(ghosts, st);
  } catch (err) {
    if (el("detail-ghost-status")) {
      el("detail-ghost-status").textContent = String(err);
    }
  }
}

function buildFriendCardHtml(friend, compactOverlay = false) {
  const plat = String(friend?.platform || "Steam");
  const platClass = plat.toLowerCase();
  const statusClass =
    friend?.status === "in_game" ? "in-game" : friend?.status === "online" ? "online" : "offline";
  const initial = (friend?.displayName || "?").trim().charAt(0).toUpperCase() || "?";
  const avatarHtml = friend?.avatarUrl
    ? `<img src="${escapeHtml(friend.avatarUrl)}" alt="" class="friend-avatar" loading="lazy" />`
    : `<div class="friend-avatar-fallback">${escapeHtml(initial)}</div>`;
  const hwBadge = friend?.hardwareBadge
    ? `<span class="friend-platform-badge mono-num">${escapeHtml(friend.hardwareBadge)}</span>`
    : "";
  const presence = friend?.gameTitle
    ? `Playing ${friend.gameTitle}${friend.presenceText && friend.presenceText !== friend.gameTitle ? ` · ${friend.presenceText}` : ""}`
    : friend?.presenceText || friend?.statusLabel || "Offline";

  const actions = [];
  if (friend?.canJoinLobby) {
    actions.push(`
      <button
        type="button"
        class="btn btn-primary btn-xs"
        data-friend-action="join-lobby"
        data-platform="${escapeHtml(plat)}"
        data-steam-app-id="${escapeHtml(friend.gameId || "")}"
        data-lobby-steam-id="${escapeHtml(friend.lobbySteamId || "")}"
        data-friend-steam-id="${escapeHtml(friend.id || "")}"
        data-server-ip="${escapeHtml(friend.gameServerIp || "")}"
        data-peer-addr="${escapeHtml(friend.swarmPeerAddr || "")}"
        data-game-id="${escapeHtml(friend.matchedLibraryGameId || "")}"
      >Join Lobby</button>
    `);
  }
  if (!compactOverlay && friend?.matchedLibraryGameId) {
    actions.push(`
      <button
        type="button"
        class="btn btn-secondary btn-xs"
        data-friend-action="launch-game"
        data-matched-game-id="${escapeHtml(friend.matchedLibraryGameId)}"
      >Launch Same Game</button>
    `);
  }
  if (!compactOverlay && plat === "Swarm" && friend?.matchedLibraryGameId) {
    actions.push(`
      <button
        type="button"
        class="btn btn-secondary btn-xs"
        data-friend-action="beam-save"
        data-matched-game-id="${escapeHtml(friend.matchedLibraryGameId)}"
      >Beam Save</button>
    `);
  }
  if (!compactOverlay && friend?.status !== "offline") {
    actions.push(`
      <button
        type="button"
        class="btn btn-primary btn-xs"
        data-friend-action="invite-coop"
        data-friend-name="${escapeHtml(friend?.displayName || "Friend")}"
        data-platform="${escapeHtml(plat)}"
        data-friend-steam-id="${escapeHtml(friend?.id || "")}"
        data-peer-addr="${escapeHtml(friend?.swarmPeerAddr || friend?.gameServerIp || "")}"
        data-friend-game="${escapeHtml(friend?.gameTitle || "")}"
      >Invite to Couch Co-Op</button>
    `);
  }

  return `
    <div class="friend-card">
      <div class="friend-card-top">
        <div class="friend-avatar-wrap">
          ${avatarHtml}
          <span class="friend-dot ${statusClass}"></span>
        </div>
        <div class="friend-meta">
          <div class="friend-name-row">
            <span class="friend-name">${escapeHtml(friend?.displayName || "Friend")}</span>
            <span class="friend-platform-badge ${platClass}">${escapeHtml(plat)}</span>
            ${hwBadge}
          </div>
          <div class="friend-presence ${friend?.status === "in_game" ? "active-game" : ""}">${escapeHtml(presence)}</div>
        </div>
      </div>
      ${actions.length > 0 ? `<div class="friend-actions">${actions.join("")}</div>` : ""}
    </div>
  `;
}

async function handleFriendActionClick(btn, onStatus) {
  if (!btn) return;
  const action = btn.dataset.friendAction;
  const notify = (msg, tone = "info") => {
    if (typeof onStatus === "function") onStatus(msg);
    showToast(msg, tone);
  };
  btn.disabled = true;
  try {
    if (action === "join-lobby") {
      const msg = await joinFriendSession({
        platform: btn.dataset.platform || "Steam",
        steamAppId: btn.dataset.steamAppId || null,
        lobbySteamId: btn.dataset.lobbySteamId || null,
        friendSteamId: btn.dataset.friendSteamId || null,
        serverIp: btn.dataset.serverIp || null,
        peerAddr: btn.dataset.peerAddr || null,
        gameId: btn.dataset.gameId || null
      });
      notify(msg || "Launching friend lobby session...", "success");
    } else if (action === "launch-game") {
      const matchedId = btn.dataset.matchedGameId;
      const game = state.library.find((g) => g.id === matchedId);
      if (game) {
        await launchGameById(game);
      } else {
        notify("Matched library entry was not found.", "error");
      }
    } else if (action === "beam-save") {
      const matchedId = btn.dataset.matchedGameId;
      const res = await syncGameSavesMesh(matchedId);
      notify(res?.summary || "Mesh save beam completed.", "success");
    } else if (action === "invite-coop") {
      const selectedGame = state.library.find((g) => g.id === state.selectedDetailId);
      const gameTitle = selectedGame?.title || btn.dataset.friendGame || "Couch Co-Op Session";
      const res = await inviteFriendToCoOp({
        friendName: btn.dataset.friendName || "Friend",
        platform: btn.dataset.platform || "Steam",
        steamId: btn.dataset.friendSteamId || null,
        peerHint: btn.dataset.peerAddr || null,
        gameTitle
      });
      state.dukeLinkStatus = res?.session || null;
      state.dukeLinkInviteUri = res?.inviteUri || "";
      renderDukeLinkUi(state.dukeLinkStatus);
      if (res?.inviteUri) {
        await writeClipboardText(res.inviteUri).catch(() => {});
      }
      notify(res?.deliveryNote || "Couch Co-Op room ready.", "success");
    }
  } catch (err) {
    notify(String(err), "error");
  } finally {
    btn.disabled = false;
  }
}

function renderFriendsSnapshot(snapshot) {
  state.friendsSnapshot = snapshot || null;
  const inGame = Array.isArray(snapshot?.inGame) ? snapshot.inGame : [];
  const online = Array.isArray(snapshot?.online) ? snapshot.online : [];
  const offline = Array.isArray(snapshot?.offline) ? snapshot.offline : [];
  const activeCount = inGame.length + online.length;

  if (el("friends-online-pill")) {
    el("friends-online-pill").textContent = String(activeCount);
  }
  if (el("friends-drawer-summary")) {
    const ps = snapshot?.platformStatus || {};
    const statusParts = [
      `Steam: ${ps.steam || "Unconfigured"}`,
      `Xbox: ${ps.xbox || "Unconfigured"}`,
      `PSN: ${ps.psn || "Unconfigured"}`,
      `Swarm: ${ps.swarm || "Idle"}`
    ];
    el("friends-drawer-summary").textContent = statusParts.join(" · ");
  }

  const list = el("friends-drawer-list");
  if (!list) return;
  const groups = [
    { label: `In-Game Now (${inGame.length})`, items: inGame },
    { label: `Online (${online.length})`, items: online },
    { label: `Offline (${offline.length})`, items: offline.slice(0, 40) }
  ].filter((g) => g.items.length > 0);

  if (groups.length === 0) {
    list.innerHTML = `<div class="panel-sub">No active friends detected yet. Configure Steam, Xbox Live, PSN tokens in Settings or enable Duke Swarm on your LAN.</div>`;
    return;
  }

  list.innerHTML = groups
    .map(
      (grp) => `
      <div class="friends-group">
        <div class="friends-group-title">${escapeHtml(grp.label)}</div>
        <div class="friends-group-items">
          ${grp.items.map((f) => buildFriendCardHtml(f, false)).join("")}
        </div>
      </div>
    `
    )
    .join("");
}

async function refreshFriendsPanel(silent = false) {
  if (state.friendsLoading) return;
  state.friendsLoading = true;
  try {
    const [snap] = await Promise.all([
      getUnifiedFriends(),
      refreshSteamCmAndInvites()
    ]);
    renderFriendsSnapshot(snap);
  } catch (err) {
    if (!silent) showToast(String(err), "error");
    if (el("friends-drawer-summary") && !state.friendsSnapshot) {
      el("friends-drawer-summary").textContent = String(err);
    }
  } finally {
    state.friendsLoading = false;
  }
}

function toggleFriendsDrawer(forceOpen = null) {
  const next = typeof forceOpen === "boolean" ? forceOpen : !state.friendsDrawerOpen;
  state.friendsDrawerOpen = next;
  const drawer = el("friends-drawer");
  if (drawer) {
    drawer.classList.toggle("hidden", !next);
  }
  const toggleBtn = el("btn-toggle-friends");
  if (toggleBtn) {
    toggleBtn.setAttribute("aria-expanded", next ? "true" : "false");
  }
  if (state.friendsPollTimer !== null) {
    clearInterval(state.friendsPollTimer);
    state.friendsPollTimer = null;
  }
  if (next) {
    refreshFriendsPanel(false);
    state.friendsPollTimer = setInterval(() => {
      if (state.friendsDrawerOpen && !document.hidden) {
        refreshFriendsPanel(true);
      }
    }, 20000);
  } else {
    abortScopedRequests("friends-drawer");
    clearControllerFocus({ disconnect: !state.controllerConnected, blurNonText: true, resetRepeats: true });
  }
}

const coopCanvasImg = new Image();
coopCanvasImg.onload = () => {
  const canvas = el("coop-stream-canvas");
  if (!canvas) return;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  if (canvas.width !== coopCanvasImg.naturalWidth && coopCanvasImg.naturalWidth > 0) {
    canvas.width = coopCanvasImg.naturalWidth;
  }
  if (canvas.height !== coopCanvasImg.naturalHeight && coopCanvasImg.naturalHeight > 0) {
    canvas.height = coopCanvasImg.naturalHeight;
  }
  ctx.drawImage(coopCanvasImg, 0, 0, canvas.width, canvas.height);
  el("coop-canvas-empty")?.classList.add("hidden");
};

function renderCoopFrameOnCanvas(frameEv) {
  if (!frameEv?.frameB64) return;
  coopCanvasImg.src = `data:image/jpeg;base64,${frameEv.frameB64}`;
  if (el("coop-telemetry-codec") && frameEv.codec) {
    el("coop-telemetry-codec").textContent = `Codec: ${frameEv.codec}`;
  }
  if (el("coop-telemetry-rtt")) {
    el("coop-telemetry-rtt").textContent = `RTT: ${Number(frameEv.rttMs || 0).toFixed(1)} ms`;
  }
  if (el("coop-telemetry-fps")) {
    el("coop-telemetry-fps").textContent = `FPS: ${Number(frameEv.fps || 0).toFixed(0)}`;
  }
  if (el("coop-telemetry-kbps")) {
    el("coop-telemetry-kbps").textContent = `Bitrate: ${Number(frameEv.bitrateKbps || 0)} kbps`;
  }
}

function renderDukeLinkUi(snap) {
  state.dukeLinkStatus = snap || null;
  const active = Boolean(snap?.active);
  const banner = el("friends-coop-status-banner");
  if (banner) {
    banner.classList.toggle("hidden", !active);
    if (el("friends-coop-banner-badge")) {
      el("friends-coop-banner-badge").textContent = active
        ? `Duke Link ${snap.role === "host" ? "Host" : "Guest"} · ${snap.roomCode || "LIVE"}`
        : "Duke Link Idle";
    }
    if (el("friends-coop-banner-text")) {
      el("friends-coop-banner-text").textContent = snap?.statusText || "";
    }
  }
  if (el("coop-modal-status-pill")) {
    el("coop-modal-status-pill").textContent = active
      ? `${snap.role === "host" ? "HOST" : "GUEST"} · ${snap.roomCode || "CONNECTED"}`
      : "Idle";
  }
  if (el("btn-coop-stop-stream")) {
    el("btn-coop-stop-stream").classList.toggle("hidden", !active);
  }
  if (el("coop-telemetry-codec")) {
    el("coop-telemetry-codec").textContent = `Codec: ${snap?.codec && snap.codec !== "none" ? snap.codec : "Standby"}`;
  }
  if (el("coop-telemetry-rtt")) {
    el("coop-telemetry-rtt").textContent = active ? `RTT: ${Number(snap?.rttMs || 0).toFixed(1)} ms` : "RTT: -- ms";
  }
  if (el("coop-telemetry-fps")) {
    el("coop-telemetry-fps").textContent = active ? `FPS: ${Number(snap?.fps || 0).toFixed(0)}` : "FPS: --";
  }
  if (el("coop-telemetry-kbps")) {
    el("coop-telemetry-kbps").textContent = active ? `Bitrate: ${Number(snap?.bitrateKbps || 0)} kbps` : "Bitrate: -- kbps";
  }
  if (el("coop-telemetry-p2")) {
    const pCount = Number(snap?.activePlayers || 1);
    const maxP = Number(snap?.maxPlayers || 4);
    el("coop-telemetry-p2").textContent = `Pads ${pCount}/${maxP} · Inputs: ${Number(snap?.p2InputPackets || 0)}`;
  }
  if (!active) {
    el("coop-canvas-empty")?.classList.remove("hidden");
  }
}

function readLocalCoOpP2State() {
  let buttons = 0;
  let leftTrigger = 0;
  let rightTrigger = 0;
  let thumbLx = 0;
  let thumbLy = 0;
  let thumbRx = 0;
  let thumbRy = 0;

  const keys = state.coopKeysDown;
  if (keys.has("KeyW") || keys.has("ArrowUp")) thumbLy = 30000;
  if (keys.has("KeyS") || keys.has("ArrowDown")) thumbLy = -30000;
  if (keys.has("KeyA") || keys.has("ArrowLeft")) thumbLx = -30000;
  if (keys.has("KeyD") || keys.has("ArrowRight")) thumbLx = 30000;
  if (keys.has("KeyZ")) buttons |= 0x1000; // A
  if (keys.has("KeyX")) buttons |= 0x2000; // B
  if (keys.has("KeyC")) buttons |= 0x4000; // X
  if (keys.has("KeyV")) buttons |= 0x8000; // Y
  if (keys.has("Enter")) buttons |= 0x0010; // Start

  const pads = typeof navigator.getGamepads === "function" ? navigator.getGamepads() : [];
  const pad = Array.from(pads || []).find((p) => p && p.connected);
  if (pad) {
    const bitMap = [
      [0, 0x1000],
      [1, 0x2000],
      [2, 0x4000],
      [3, 0x8000],
      [4, 0x0100],
      [5, 0x0200],
      [8, 0x0020],
      [9, 0x0010],
      [12, 0x0001],
      [13, 0x0002],
      [14, 0x0004],
      [15, 0x0008]
    ];
    for (const [idx, mask] of bitMap) {
      if (pad.buttons?.[idx]?.pressed) buttons |= mask;
    }
    leftTrigger = Math.round((pad.buttons?.[6]?.value || 0) * 255);
    rightTrigger = Math.round((pad.buttons?.[7]?.value || 0) * 255);
    const dead = (v) => (Math.abs(v || 0) < 0.08 ? 0 : Math.round(Math.max(-1, Math.min(1, v)) * 32767));
    if (Math.abs(pad.axes?.[0] || 0) >= 0.08) thumbLx = dead(pad.axes[0]);
    if (Math.abs(pad.axes?.[1] || 0) >= 0.08) thumbLy = dead(-pad.axes[1]);
    if (Math.abs(pad.axes?.[2] || 0) >= 0.08) thumbRx = dead(pad.axes[2]);
    if (Math.abs(pad.axes?.[3] || 0) >= 0.08) thumbRy = dead(-pad.axes[3]);
  }
  return { buttons, leftTrigger, rightTrigger, thumbLx, thumbLy, thumbRx, thumbRy };
}

function stopCoopP2InputPump() {
  if (state.coopInputTimer !== null) {
    clearInterval(state.coopInputTimer);
    state.coopInputTimer = null;
  }
  state.coopKeysDown.clear();
  state.coopLastSig = "";
}

function startCoopP2InputPump() {
  stopCoopP2InputPump();
  state.coopInputTimer = setInterval(() => {
    if (!state.coopModalOpen || state.dukeLinkStatus?.role !== "guest") return;
    const pad = readLocalCoOpP2State();
    const sig = `${pad.buttons}|${pad.leftTrigger}|${pad.rightTrigger}|${pad.thumbLx}|${pad.thumbLy}|${pad.thumbRx}|${pad.thumbRy}`;
    const isNeutral = sig === "0|0|0|0|0|0|0";
    if (isNeutral && state.coopLastSig === sig) return;
    state.coopLastSig = sig;
    state.coopInputSeq += 1;
    sendCoOpInputP2({ ...pad, sequence: state.coopInputSeq })
      .then((count) => {
        if (el("coop-telemetry-p2")) {
          el("coop-telemetry-p2").textContent = `P2 Inputs: ${count}`;
        }
      })
      .catch(() => {});
  }, 16);
}

function openCoopStreamModal() {
  state.coopModalOpen = true;
  el("coop-stream-modal")?.classList.remove("hidden");
  getDukeLinkStatus().then(renderDukeLinkUi).catch(() => {});
  startCoopP2InputPump();
}

function closeCoopStreamModal() {
  state.coopModalOpen = false;
  el("coop-stream-modal")?.classList.add("hidden");
  stopCoopP2InputPump();
  abortScopedRequests("coop-modal");
  clearControllerFocus({ disconnect: !state.controllerConnected, blurNonText: true, resetRepeats: true });
}

function syncSettingsUiFromBackend(settings) {
  if (!settings) return;
  state.autoCleanCompleted = Boolean(settings.autoCleanCompleted ?? true);
  state.autoDetectClipboard = Boolean(settings.autoDetectClipboard ?? true);
  state.nvmePrefetchEnabled = Boolean(settings.nvmePrefetchEnabled ?? true);
  state.swarmEnabled = Boolean(settings.swarmEnabled ?? false);
  state.swarmNodeName = String(settings.swarmNodeName || "");
  state.swarmPeers = Array.isArray(settings.swarmPeers) ? settings.swarmPeers : [];
  state.steamgriddbApiKey = String(settings.steamgriddbApiKey || "");
  state.suspendHotkey = String(settings.suspendHotkey || "Ctrl+Alt+P");
  state.steamApiKeyConfigured = Boolean(settings.steamApiKeyConfigured);
  state.xboxApiKeyConfigured = Boolean(settings.xboxApiKeyConfigured);
  state.psnNpssoConfigured = Boolean(settings.psnNpssoConfigured);
  state.steamIdOverride = String(settings.steamIdOverride || "");
  state.detectedSteamId = String(settings.detectedSteamId || "");

  // Essentials fields
  state.defaultSaveDir = String(settings.defaultSaveDir || state.customSaveDir || "");
  if (state.defaultSaveDir) {
    state.customSaveDir = state.defaultSaveDir;
    localStorage.setItem("duke_launcher_dl_dir", state.defaultSaveDir);
  }
  state.downloadSpeedPreset = String(settings.downloadSpeedPreset || "max");
  state.compactLibraryCards = Boolean(settings.compactLibraryCards ?? false);
  if (typeof settings.theme === "string") {
    const normalizedTheme = THEME_PRESETS.some((t) => t.id === settings.theme)
      ? settings.theme
      : "dark";
    const rawLocal = localStorage.getItem("duke_launcher_theme");
    if (
      !rawLocal ||
      !THEME_PRESETS.some((t) => t.id === rawLocal) ||
      (normalizedTheme !== "dark" && THEME_PRESETS.some((t) => t.id === normalizedTheme))
    ) {
      applyTheme(normalizedTheme, false);
    }
  }
  if (Number.isFinite(Number(settings.cardZoomPx)) && Number(settings.cardZoomPx) > 0) {
    applyCardZoom(Number(settings.cardZoomPx), false);
  }
  state.performanceBoostEnabled = Boolean(settings.performanceBoostEnabled ?? true);
  state.autoBackupSavesOnExit = Boolean(settings.autoBackupSavesOnExit ?? true);

  // Advanced Engine fields
  state.cpuPriorityMode = String(settings.cpuPriorityMode || "auto");
  state.ramTrimMode = String(settings.ramTrimMode || "standard");
  state.autoFreezeMinimized = Boolean(settings.autoFreezeMinimized ?? false);
  state.nvmeCacheBudgetPct = snapNvmeBudgetPct(settings.nvmeCacheBudgetPct ?? 25);
  state.dohEnabled = Boolean(settings.dohEnabled ?? true);
  state.customProxyUrl = String(settings.customProxyUrl || "");
  state.customSpeedCapMbps = Number(settings.customSpeedCapMbps || 0);
  state.autoUnblockMotw = Boolean(settings.autoUnblockMotw ?? true);
  state.portableSandboxDefault = Boolean(settings.portableSandboxDefault ?? false);
  state.maxRestorePoints = Number(settings.maxRestorePoints || 5);
  state.blockGameTelemetry = Boolean(settings.blockGameTelemetry ?? true);
  state.globalLaunchArgs = String(settings.globalLaunchArgs || "");
  state.swarmAutoSyncSaves = Boolean(settings.swarmAutoSyncSaves ?? true);
  state.startMinimizedWithWindows = Boolean(settings.startMinimizedWithWindows ?? false);
  state.diskPressureThresholdGb = Number(settings.diskPressureThresholdGb || 50);

  applyCompactLibraryCards(state.compactLibraryCards);

  if (el("setting-default-save-dir") && state.defaultSaveDir) {
    el("setting-default-save-dir").value = state.defaultSaveDir;
  }
  if (el("input-dl-dir") && state.customSaveDir) {
    el("input-dl-dir").value = state.customSaveDir;
  }
  refreshSettingsDriveSpaceBar(state.defaultSaveDir || null);

  document.querySelectorAll("#setting-speed-preset-group [data-speed-preset]").forEach((btn) => {
    btn.classList.toggle("active", btn.dataset.speedPreset === state.downloadSpeedPreset);
  });

  if (el("setting-performance-boost")) {
    el("setting-performance-boost").checked = state.performanceBoostEnabled;
  }
  if (el("setting-auto-backup-saves-exit")) {
    el("setting-auto-backup-saves-exit").checked = state.autoBackupSavesOnExit;
  }

  if (el("setting-cpu-priority-mode")) el("setting-cpu-priority-mode").value = state.cpuPriorityMode;
  if (el("setting-ram-trim-mode")) el("setting-ram-trim-mode").value = state.ramTrimMode;
  if (el("setting-auto-freeze-minimized")) {
    el("setting-auto-freeze-minimized").checked = state.autoFreezeMinimized;
  }
  if (el("setting-nvme-prefetch")) el("setting-nvme-prefetch").checked = state.nvmePrefetchEnabled;
  renderNvmeBudgetUi(state.nvmeCacheBudgetPct);

  if (el("setting-doh-enabled")) el("setting-doh-enabled").checked = state.dohEnabled;
  if (el("sidebar-doh-badge")) {
    el("sidebar-doh-badge").textContent = state.dohEnabled ? "Active" : "Bypassed";
  }
  if (el("setting-custom-proxy-url")) el("setting-custom-proxy-url").value = state.customProxyUrl;
  if (el("setting-custom-speed-cap")) {
    el("setting-custom-speed-cap").value =
      state.customSpeedCapMbps > 0 ? String(state.customSpeedCapMbps) : "";
  }
  if (el("setting-auto-unblock-motw")) el("setting-auto-unblock-motw").checked = state.autoUnblockMotw;
  if (el("setting-auto-clean-completed")) el("setting-auto-clean-completed").checked = state.autoCleanCompleted;
  if (el("setting-auto-detect-clipboard")) {
    el("setting-auto-detect-clipboard").checked = state.autoDetectClipboard;
  }

  if (el("setting-portable-sandbox-default")) {
    el("setting-portable-sandbox-default").checked = state.portableSandboxDefault;
  }
  if (el("setting-max-restore-points")) {
    el("setting-max-restore-points").value = String(state.maxRestorePoints);
  }
  if (el("setting-block-game-telemetry")) {
    el("setting-block-game-telemetry").checked = state.blockGameTelemetry;
  }
  if (el("setting-global-launch-args")) el("setting-global-launch-args").value = state.globalLaunchArgs;

  if (el("setting-swarm-enabled")) el("setting-swarm-enabled").checked = state.swarmEnabled;
  if (el("setting-swarm-node-name")) el("setting-swarm-node-name").value = state.swarmNodeName;
  if (el("setting-swarm-peers")) el("setting-swarm-peers").value = state.swarmPeers.join(", ");
  if (el("setting-swarm-auto-sync-saves")) {
    el("setting-swarm-auto-sync-saves").checked = state.swarmAutoSyncSaves;
  }
  if (el("setting-steamgriddb-key")) el("setting-steamgriddb-key").value = state.steamgriddbApiKey;
  if (el("setting-suspend-hotkey")) el("setting-suspend-hotkey").value = state.suspendHotkey;
  if (el("setting-start-minimized-windows")) {
    el("setting-start-minimized-windows").checked = state.startMinimizedWithWindows;
  }

  if (el("setting-detected-steam-id")) {
    el("setting-detected-steam-id").textContent =
      state.detectedSteamId || "Not detected (loginusers.vdf)";
  }
  if (el("setting-steam-id-override")) {
    el("setting-steam-id-override").value = state.steamIdOverride;
  }
  if (el("setting-steam-key-status")) {
    el("setting-steam-key-status").textContent = state.steamApiKeyConfigured ? "Configured" : "Not Set";
    el("setting-steam-key-status").classList.toggle("accent", state.steamApiKeyConfigured);
  }
  if (el("setting-xbox-key-status")) {
    el("setting-xbox-key-status").textContent = state.xboxApiKeyConfigured ? "Configured" : "Not Set";
    el("setting-xbox-key-status").classList.toggle("accent", state.xboxApiKeyConfigured);
  }
  if (el("setting-psn-npsso-status")) {
    el("setting-psn-npsso-status").textContent = state.psnNpssoConfigured ? "Configured" : "Not Set";
    el("setting-psn-npsso-status").classList.toggle("accent", state.psnNpssoConfigured);
  }
  el("btn-clear-steam-api-key")?.classList.toggle("hidden", !state.steamApiKeyConfigured);
  el("btn-clear-xbox-api-key")?.classList.toggle("hidden", !state.xboxApiKeyConfigured);
  el("btn-clear-psn-npsso")?.classList.toggle("hidden", !state.psnNpssoConfigured);

  state.projfsStreamingEnabled = Boolean(settings.projfsStreamingEnabled ?? true);
  state.shaderSwarmEnabled = Boolean(settings.shaderSwarmEnabled ?? true);
  state.framePacingEnabled = Boolean(settings.framePacingEnabled ?? true);
  if (el("setting-projfs-streaming")) el("setting-projfs-streaming").checked = state.projfsStreamingEnabled;
  if (el("setting-shader-swarm")) el("setting-shader-swarm").checked = state.shaderSwarmEnabled;
  if (el("setting-frame-pacing")) el("setting-frame-pacing").checked = state.framePacingEnabled;
  refreshPrivacyVaultUi();
}

function renderPrivacyVaultStatus(st) {
  state.privacyVaultStatus = st || null;
  if (!st) return;
  if (el("setting-vault-status-pill")) {
    const dpapiText = st.dpapiAvailable ? "DPAPI Vault Active" : "Local Vault";
    const count = Array.isArray(st.configuredSlots) ? st.configuredSlots.length : 0;
    el("setting-vault-status-pill").textContent = count > 0 ? `${dpapiText} · ${count} key(s)` : dpapiText;
  }
  if (el("setting-doh-provider") && st.dohProvider) {
    el("setting-doh-provider").value = st.dohProvider;
  }
  if (el("setting-proxy-preset") && st.proxyPreset) {
    el("setting-proxy-preset").value = st.proxyPreset;
  }
  if (el("setting-vault-proxy-readout")) {
    const dnsMode = st.dohEnabled ? `DoH (${st.dohProvider || "auto"})` : "System DNS";
    const proxyDesc = st.effectiveProxyUrl ? `Proxy ${st.effectiveProxyUrl}` : "No Proxy (Direct)";
    el("setting-vault-proxy-readout").textContent = `${dnsMode} · ${proxyDesc}`;
  }
  if (el("sidebar-doh-badge")) {
    el("sidebar-doh-badge").textContent = st.dohEnabled ? "Active" : "Bypassed";
  }
  const raSet = Boolean(st.retroAchievementsConfigured);
  if (el("setting-ra-key-status")) {
    el("setting-ra-key-status").textContent = raSet ? "Configured (DPAPI)" : "Not Set";
    el("setting-ra-key-status").classList.toggle("accent", raSet);
  }
  if (el("setting-ra-username") && st.retroAchievementsUsername) {
    el("setting-ra-username").value = st.retroAchievementsUsername;
  }
  el("btn-clear-ra-key")?.classList.toggle("hidden", !raSet);
}

async function refreshPrivacyVaultUi() {
  try {
    const st = await getPrivacyVaultStatus();
    renderPrivacyVaultStatus(st);
  } catch (_) {}
}

async function runPlaytimeSyncFlow() {
  showToast("Syncing local Steam, GOG, Epic & Xbox playtime...", "info", 2200);
  try {
    const res = await syncStorefrontPlaytime();
    if (Array.isArray(res?.games)) {
      state.library = res.games;
      renderSidebarGameList();
      renderLibraryGrid();
      updateSidebarTelemetry();
      if (state.activeTab === "detail" && state.selectedDetailId) {
        const updatedGame = state.library.find((g) => g.id === state.selectedDetailId);
        if (updatedGame) openGameDetailView(updatedGame.id);
      }
    }
    const hrs = (Number(res?.totalCombinedSecs || 0) / 3600).toFixed(1);
    const checked = Array.isArray(res?.sourcesChecked) ? res.sourcesChecked.join(", ") : "storefronts";
    showToast(
      `Playtime synced across ${res?.updatedGames || 0} game(s) (${hrs} h total · ${checked}).`,
      "success",
      3600
    );
  } catch (err) {
    showToast(String(err), "error");
  }
}

async function initOverlayHudMode() {
  document.documentElement.classList.add("mode-overlay-root");
  document.body.classList.add("mode-overlay");
  applyTheme(state.activeTheme || "dark");

  const shell = el("overlay-hud-shell");
  if (shell) shell.classList.remove("hidden");

  const setStatus = (msg) => {
    if (el("overlay-status-line")) el("overlay-status-line").textContent = msg;
  };

  let activeOverlayGameId = null;
  let activeOverlayPid = 0;
  let activeOverlayExePath = null;

  const renderOverlayQuickResume = (qr) => {
    const host = el("overlay-quick-resume-list");
    if (!host) return;
    const slots = Array.isArray(qr?.slots) ? qr.slots : [];
    if (slots.length === 0) {
      host.innerHTML = `<div class="panel-sub">No Quick Resume slots occupied (up to 3 games can stay suspended).</div>`;
      return;
    }
    host.innerHTML = slots
      .map(
        (s) => `
        <div class="overlay-qr-card">
          <div class="overlay-qr-meta">
            <span class="sidebar-qr-slot-title">${escapeHtml(s.title || "Game")}</span>
            <span class="sidebar-qr-slot-meta mono-num">${s.isSuspended ? "Suspended Standby" : "Running"} · ${Number(s.workingSetMb || 0).toFixed(0)} MB${s.usedIoringApi ? " · IoRing" : ""}</span>
          </div>
          <div class="overlay-qr-actions">
            <button type="button" class="btn btn-primary btn-xs" data-overlay-qr-switch="${escapeHtml(s.gameId)}">${s.isSuspended ? "Quick Switch" : "Freeze Slot"}</button>
            <button type="button" class="btn btn-secondary btn-xs" data-overlay-qr-remove="${escapeHtml(s.gameId)}">Drop</button>
          </div>
        </div>
      `
      )
      .join("");
  };

  const refreshOverlayState = async () => {
    try {
      const [sessions, qr] = await Promise.all([
        getActiveGameSessions(),
        getQuickResumeSlots().catch(() => null)
      ]);
      const active = Array.isArray(sessions) && sessions.length > 0 ? sessions[0] : null;
      activeOverlayGameId = active?.gameId || null;
      activeOverlayPid = Number(active?.pid || 0);
      activeOverlayExePath = active?.exePath || null;
      if (el("overlay-active-game")) {
        el("overlay-active-game").textContent = active
          ? `${active.title} (PID ${active.pid})${active.isSuspended ? " · PAUSED" : ""}`
          : "Idle Session";
      }
      const suspendSpan = el("btn-overlay-suspend")?.querySelector("span");
      if (suspendSpan) {
        suspendSpan.textContent = active?.isSuspended ? "Resume Game" : active ? "Freeze Game" : "Freeze / Resume Game";
      }
      if (qr) renderOverlayQuickResume(qr);
    } catch {
      // Keep idle label cleanly on transient error.
    }

    try {
      const snap = await getUnifiedFriends();
      const onlineList = [...(snap?.inGame || []), ...(snap?.online || [])];
      const host = el("overlay-friends-list");
      if (host) {
        host.innerHTML =
          onlineList.length > 0
            ? onlineList.slice(0, 8).map((f) => buildFriendCardHtml(f, true)).join("")
            : `<div class="panel-sub">No friends currently online.</div>`;
      }
    } catch (err) {
      if (el("overlay-friends-list")) {
        el("overlay-friends-list").innerHTML = `<div class="panel-sub">${escapeHtml(String(err))}</div>`;
      }
    }
  };

  el("btn-overlay-close")?.addEventListener("click", () => {
    hideOverlayWindow().catch(() => {});
  });

  el("btn-overlay-suspend")?.addEventListener("click", async () => {
    try {
      const res = await suspendOrResumeGame(activeOverlayGameId, "toggle");
      setStatus(res?.message || (res?.isSuspended ? "Process suspended." : "Process resumed."));
      await refreshOverlayState();
    } catch (err) {
      setStatus(String(err));
    }
  });

  el("btn-overlay-purge-ram")?.addEventListener("click", async () => {
    setStatus("Trimming working sets & purging standby memory...");
    try {
      const res = await purgeSuiteRamIpc();
      setStatus(res?.message || "Standby RAM purge complete.");
    } catch (err) {
      setStatus(String(err));
    }
  });

  el("btn-overlay-replay-clip")?.addEventListener("click", async () => {
    try {
      const res = await triggerReplayClipIpc();
      setStatus(typeof res === "string" ? res : res?.message || "Replay buffer save requested.");
    } catch (err) {
      setStatus(String(err));
    }
  });

  el("btn-overlay-refresh-friends")?.addEventListener("click", () => {
    setStatus("Refreshing multi-platform friends...");
    refreshOverlayState().then(() => setStatus("Friends refreshed."));
  });

  el("overlay-friends-list")?.addEventListener("click", (e) => {
    const btn = e.target.closest("button[data-friend-action]");
    if (btn) handleFriendActionClick(btn, setStatus);
  });

  el("overlay-quick-resume-list")?.addEventListener("click", async (e) => {
    const switchBtn = e.target.closest("[data-overlay-qr-switch]");
    if (switchBtn) {
      const gid = switchBtn.dataset.overlayQrSwitch;
      switchBtn.disabled = true;
      setStatus("Orchestrating standby page swap...");
      try {
        const rep = await quickSwitchGameSlot(gid);
        setStatus(rep?.summary || `Switched slot (${rep?.framePacingHzLabel || "0.5 ms"}).`);
        await refreshOverlayState();
      } catch (err) {
        setStatus(String(err));
      } finally {
        switchBtn.disabled = false;
      }
      return;
    }
    const remBtn = e.target.closest("[data-overlay-qr-remove]");
    if (remBtn) {
      remBtn.disabled = true;
      try {
        const summary = await removeQuickResumeSlot(remBtn.dataset.overlayQrRemove, false);
        renderOverlayQuickResume(summary);
        setStatus("Released Quick Resume standby slot.");
      } catch (err) {
        setStatus(String(err));
      } finally {
        remBtn.disabled = false;
      }
    }
  });

  window.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      hideOverlayWindow().catch(() => {});
    }
  });

  const EQ_PRESET_BANDS = {
    "Neutral Flat": [0, 0, 0, 0, 0],
    "FPS Footsteps": [-2.0, -1.0, 1.5, 5.5, 4.5],
    Cinema: [4.5, 2.5, -1.0, 1.5, 3.0],
    "Voice Clarity": [-4.0, -2.0, 3.5, 4.5, 1.5]
  };

  const renderOverlayVisionResult = (res) => {
    if (!res) return;
    if (el("overlay-vision-meta")) {
      el("overlay-vision-meta").textContent = `${res.ocrEngine || "OCR"} (${Number(res.ocrMs || 0).toFixed(1)} ms) · ${res.executionProvider || "NPU"} · ${Number(res.vramAllocatedMb || 0).toFixed(1)} MB VRAM`;
    }
    if (el("overlay-vision-output")) {
      el("overlay-vision-output").textContent = res.translatedText || res.extractedText || "No screen text recognized.";
    }
    if (el("overlay-vision-quest")) {
      el("overlay-vision-quest").textContent = res.questHint ? `Quest Copilot: ${res.questHint}` : "";
    }
  };

  const renderOverlayCommsState = (comms) => {
    if (!comms) return;
    const toggleBtn = el("btn-overlay-comms-toggle");
    if (toggleBtn) {
      const span = toggleBtn.querySelector("span");
      if (span) span.textContent = comms.listening ? "Stop Voice Tap" : "Enable Voice Tap";
      toggleBtn.classList.toggle("btn-secondary", Boolean(comms.listening));
      toggleBtn.classList.toggle("btn-primary", !comms.listening);
    }
    if (el("overlay-comms-vad-pill")) {
      el("overlay-comms-vad-pill").textContent = comms.listening
        ? comms.vadActive
          ? `VAD Speech (${Math.round(Number(comms.vadSpeechProb || 0) * 100)}%)`
          : `Tap Live (${comms.captureTapConnected ? "WASAPI" : "Standby"})`
        : "VAD Idle";
      el("overlay-comms-vad-pill").className = `pill ${comms.vadActive ? "emerald" : ""} mono-num`;
    }
    const subHost = el("overlay-comms-subtitles");
    if (subHost) {
      const subs = Array.isArray(comms.subtitles) ? comms.subtitles : [];
      subHost.innerHTML =
        subs.length > 0
          ? subs
              .slice(-5)
              .reverse()
              .map(
                (s) => `
              <div class="overlay-subtitle-item">
                <span class="mono-num">[${escapeHtml(s.speaker || "Squad")} · ${Number(s.latencyMs || 0)} ms]</span>
                <strong>${escapeHtml(s.translatedText || s.originalText || "")}</strong>
              </div>`
              )
              .join("")
          : `<div class="panel-sub">Waiting for WASAPI loopback squad audio...</div>`;
    }
  };

  const renderOverlayAcousticsState = (ac) => {
    if (!ac) return;
    const sessHost = el("overlay-acoustics-sessions");
    if (sessHost) {
      const sessions = Array.isArray(ac.sessions) ? ac.sessions : [];
      sessHost.innerHTML =
        sessions.length > 0
          ? sessions
              .slice(0, 6)
              .map(
                (s) => `
              <div class="overlay-session-row">
                <div class="overlay-session-meta">
                  <strong>${escapeHtml(s.displayName || s.processName || `PID ${s.pid}`)}</strong>
                  <span class="mono-num">PID ${Number(s.pid)}${s.isActiveGame ? " · Game" : ""}</span>
                </div>
                <input type="range" min="0" max="100" step="1" value="${Number(s.volumePct ?? 100)}" data-session-vol-pid="${Number(s.pid)}" />
                <button type="button" class="btn btn-secondary btn-xs" data-session-mute-pid="${Number(s.pid)}" data-session-muted="${s.muted ? "1" : "0"}">${s.muted ? "Unmute" : "Mute"}</button>
              </div>`
              )
              .join("")
          : `<div class="panel-sub">No active WASAPI audio sessions detected.</div>`;
    }
    const prof = ac.profile;
    if (prof) {
      setSelectValueSafe(el("select-overlay-eq-preset"), prof.presetName || "Neutral Flat");
      const bands = Array.isArray(prof.bandsDb) ? prof.bandsDb : [0, 0, 0, 0, 0];
      document.querySelectorAll("#overlay-eq-sliders .overlay-eq-band").forEach((inp) => {
        const idx = Number(inp.dataset.band || 0);
        const v = Number(bands[idx] || 0);
        inp.value = String(v);
        const valEl = inp.parentElement?.querySelector("strong");
        if (valEl) valEl.textContent = `${v >= 0 ? "+" : ""}${v.toFixed(1)} dB`;
      });
      if (el("chk-overlay-denoise-enabled")) {
        el("chk-overlay-denoise-enabled").checked = Boolean(prof.micDenoiseEnabled);
      }
      if (el("range-overlay-denoise-strength") && prof.micDenoiseStrength != null) {
        el("range-overlay-denoise-strength").value = String(prof.micDenoiseStrength);
      }
    }
    if (ac.denoiser && el("overlay-acoustics-latency-badge")) {
      el("overlay-acoustics-latency-badge").textContent = `${Number(ac.denoiser.processingMs || 0).toFixed(2)} ms · ${ac.denoiser.executionProvider || "CPU"}`;
    }
  };

  const renderOverlaySandboxTrainer = (st) => {
    if (!st) return;
    const blocked = Boolean(st.antiCheatBlocked);
    const guardPill = el("overlay-sandbox-guard-pill");
    if (guardPill) {
      if (blocked) {
        const list = Array.isArray(st.antiCheatReport?.detectedSystems)
          ? st.antiCheatReport.detectedSystems.join(", ")
          : "Online Anti-Cheat";
        guardPill.textContent = `Blocked (${list})`;
        guardPill.className = "pill danger mono-num";
      } else if (Number(st.attachedPid || 0) > 0) {
        guardPill.textContent = `SP Safe · PID ${st.attachedPid}`;
        guardPill.className = "pill emerald mono-num";
      } else {
        guardPill.textContent = "SP Ready (No PID)";
        guardPill.className = "pill mono-num";
      }
    }
    if (el("overlay-sandbox-status")) {
      el("overlay-sandbox-status").textContent =
        st.statusText ||
        st.antiCheatReport?.summary ||
        "Single-player memory trainer & timer speedhack standing by.";
    }
    const scale = Number(st.speedhackScale || 1.0);
    if (el("overlay-sandbox-speed-pill")) {
      el("overlay-sandbox-speed-pill").textContent = `${scale.toFixed(2)}x`;
    }
    if (el("range-overlay-speedhack")) {
      el("range-overlay-speedhack").value = String(scale);
      el("range-overlay-speedhack").disabled = blocked;
    }
    ["btn-overlay-apply-speed", "btn-overlay-scan-new", "btn-overlay-scan-next"].forEach((id) => {
      if (el(id)) el(id).disabled = blocked;
    });
  };

  const renderOverlaySandboxMatches = (res) => {
    const host = el("overlay-sandbox-matches");
    if (!host) return;
    const matches = Array.isArray(res?.matches) ? res.matches : [];
    if (el("overlay-sandbox-status") && res?.summary) {
      el("overlay-sandbox-status").textContent = res.summary;
    }
    if (matches.length === 0) {
      host.innerHTML = `<div class="panel-sub">${escapeHtml(res?.summary || "No matching addresses found.")}</div>`;
      return;
    }
    host.innerHTML = matches
      .slice(0, 40)
      .map(
        (m) => `
        <div class="overlay-qr-card">
          <div class="overlay-qr-meta">
            <span class="sidebar-qr-slot-title mono-num">${escapeHtml(m.addressHex)}</span>
            <span class="sidebar-qr-slot-meta mono-num">${escapeHtml(m.valueType || "i32")} = ${escapeHtml(m.currentValue)} · ${escapeHtml(m.regionProtect || "RW")}</span>
          </div>
          <button
            type="button"
            class="btn btn-secondary btn-xs"
            data-sandbox-addr="${escapeHtml(m.addressHex)}"
            data-sandbox-type="${escapeHtml(m.valueType || "i32")}"
            data-sandbox-cur="${escapeHtml(m.currentValue || "")}"
          >Edit</button>
        </div>
      `
      )
      .join("");
  };

  const refreshOverlaySandboxState = async () => {
    try {
      const st = await getSandboxTrainerStatus(
        activeOverlayPid > 0 ? activeOverlayPid : null,
        activeOverlayExePath
      );
      renderOverlaySandboxTrainer(st);
      return st;
    } catch (err) {
      if (el("overlay-sandbox-status")) {
        el("overlay-sandbox-status").textContent = String(err);
      }
      return null;
    }
  };

  const switchOverlayTab = (tabName) => {
    document.querySelectorAll(".overlay-tab-btn").forEach((btn) => {
      btn.classList.toggle("is-active", btn.dataset.overlayTab === tabName);
    });
    ["quick", "vision", "comms", "acoustics", "sandbox"].forEach((name) => {
      el(`overlay-tab-${name}`)?.classList.toggle("hidden", name !== tabName);
    });
    if (tabName === "vision") {
      getLatestVisionState().then(renderOverlayVisionResult).catch(() => {});
    } else if (tabName === "comms") {
      getDukeCommsState().then(renderOverlayCommsState).catch(() => {});
    } else if (tabName === "acoustics") {
      getAcousticsState(activeOverlayGameId).then(renderOverlayAcousticsState).catch(() => {});
    } else if (tabName === "sandbox") {
      refreshOverlaySandboxState().catch(() => {});
    }
  };

  document.querySelectorAll(".overlay-tab-btn[data-overlay-tab]").forEach((btn) => {
    btn.addEventListener("click", () => switchOverlayTab(btn.dataset.overlayTab));
  });

  el("btn-overlay-vision-scan")?.addEventListener("click", async () => {
    const btn = el("btn-overlay-vision-scan");
    if (btn) btn.disabled = true;
    setStatus("Capturing game frame + running NPU OCR...");
    try {
      const res = await triggerDukeVisionCapture({
        gameTitle: el("overlay-active-game")?.textContent || null,
        targetLang: el("select-overlay-vision-lang")?.value || "en",
        mode: el("select-overlay-vision-mode")?.value || "both"
      });
      renderOverlayVisionResult(res);
      setStatus(`Vision ready in ${Number(res?.totalMs || 0).toFixed(1)} ms (${res?.executionProvider || "NPU"}).`);
    } catch (err) {
      setStatus(String(err));
    } finally {
      if (btn) btn.disabled = false;
    }
  });

  const runOverlayVisionQuery = async () => {
    const text = (el("input-overlay-vision-query")?.value || "").trim();
    if (!text) return;
    setStatus("Analyzing via Zero-VRAM NPU copilot...");
    try {
      const res = await analyzeVisionTextQuery({
        text,
        gameTitle: el("overlay-active-game")?.textContent || null,
        targetLang: el("select-overlay-vision-lang")?.value || "en",
        mode: el("select-overlay-vision-mode")?.value || "both"
      });
      renderOverlayVisionResult(res);
      setStatus(`Translated via ${res?.aiBackend || "NPU"}`);
    } catch (err) {
      setStatus(String(err));
    }
  };
  el("btn-overlay-vision-ask")?.addEventListener("click", runOverlayVisionQuery);
  el("input-overlay-vision-query")?.addEventListener("keydown", (e) => {
    if (e.key === "Enter") runOverlayVisionQuery();
  });

  el("btn-overlay-director-clip")?.addEventListener("click", async () => {
    const btn = el("btn-overlay-director-clip");
    if (btn) btn.disabled = true;
    setStatus("Trimming dead menu frames & saving 22s highlight...");
    try {
      const res = await triggerDirectorHighlightNow({
        gameId: activeOverlayGameId,
        gameTitle: el("overlay-active-game")?.textContent || null,
        targetDurationSecs: 22
      });
      setStatus(res?.message || "Director highlight clip queued.");
    } catch (err) {
      setStatus(String(err));
    } finally {
      if (btn) btn.disabled = false;
    }
  });

  el("btn-overlay-comms-toggle")?.addEventListener("click", async () => {
    try {
      const cur = await getDukeCommsState().catch(() => null);
      const next = await toggleDukeCommsListening({
        enabled: !cur?.listening,
        targetLanguage: el("select-overlay-comms-lang")?.value || "en",
        voiceMacrosEnabled: true,
        teammateSubtitlesEnabled: true
      });
      renderOverlayCommsState(next);
      setStatus(next?.listening ? "Voice-to-HID + Teammate Subtitles active." : "Voice tap stopped.");
    } catch (err) {
      setStatus(String(err));
    }
  });

  const dispatchOverlayCommsPhrase = async (phrase, isTeammateAudio = false) => {
    if (!phrase || !phrase.trim()) return;
    try {
      const out = await processVoicePhraseCommand({ phrase: phrase.trim(), isTeammateAudio });
      if (out?.matchedMacro && el("overlay-comms-action-log")) {
        el("overlay-comms-action-log").textContent = `${out.matchedMacro.phrase} -> ${out.matchedMacro.targetSummary}`;
      }
      if (out?.status) renderOverlayCommsState(out.status);
      setStatus(out?.matchedMacro ? `Macro triggered: ${out.matchedMacro.actionKind}` : "Phrase processed.");
    } catch (err) {
      setStatus(String(err));
    }
  };
  document.querySelectorAll(".overlay-macro-quick[data-phrase]").forEach((btn) => {
    btn.addEventListener("click", () => dispatchOverlayCommsPhrase(btn.dataset.phrase, false));
  });
  el("btn-overlay-comms-run")?.addEventListener("click", () => {
    const val = el("input-overlay-comms-phrase")?.value || "";
    dispatchOverlayCommsPhrase(val, false);
  });

  el("btn-overlay-acoustics-refresh")?.addEventListener("click", async () => {
    try {
      const ac = await getAcousticsState(activeOverlayGameId);
      renderOverlayAcousticsState(ac);
      setStatus("WASAPI per-process audio sessions refreshed.");
    } catch (err) {
      setStatus(String(err));
    }
  });

  el("overlay-acoustics-sessions")?.addEventListener("change", async (e) => {
    const slider = e.target.closest("input[data-session-vol-pid]");
    if (!slider) return;
    const pid = Number(slider.dataset.sessionVolPid || 0);
    const muteBtn = el("overlay-acoustics-sessions")?.querySelector(`button[data-session-mute-pid="${pid}"]`);
    const muted = muteBtn?.dataset.sessionMuted === "1";
    try {
      const updated = await setWasapiSessionVolume(pid, Number(slider.value || 100), muted);
      renderOverlayAcousticsState({ sessions: updated });
      setStatus(`PID ${pid} volume set to ${slider.value}%`);
    } catch (err) {
      setStatus(String(err));
    }
  });

  el("overlay-acoustics-sessions")?.addEventListener("click", async (e) => {
    const btn = e.target.closest("button[data-session-mute-pid]");
    if (!btn) return;
    const pid = Number(btn.dataset.sessionMutePid || 0);
    const nextMuted = btn.dataset.sessionMuted !== "1";
    const slider = el("overlay-acoustics-sessions")?.querySelector(`input[data-session-vol-pid="${pid}"]`);
    const pct = Number(slider?.value ?? 100);
    try {
      const updated = await setWasapiSessionVolume(pid, pct, nextMuted);
      renderOverlayAcousticsState({ sessions: updated });
      setStatus(`PID ${pid} ${nextMuted ? "muted" : "unmuted"}.`);
    } catch (err) {
      setStatus(String(err));
    }
  });

  el("select-overlay-eq-preset")?.addEventListener("change", () => {
    const bands = EQ_PRESET_BANDS[el("select-overlay-eq-preset")?.value] || EQ_PRESET_BANDS["Neutral Flat"];
    document.querySelectorAll("#overlay-eq-sliders .overlay-eq-band").forEach((inp) => {
      const idx = Number(inp.dataset.band || 0);
      const v = Number(bands[idx] || 0);
      inp.value = String(v);
      const valEl = inp.parentElement?.querySelector("strong");
      if (valEl) valEl.textContent = `${v >= 0 ? "+" : ""}${v.toFixed(1)} dB`;
    });
  });

  el("overlay-eq-sliders")?.addEventListener("input", (e) => {
    const inp = e.target.closest(".overlay-eq-band");
    if (!inp) return;
    const v = Number(inp.value || 0);
    const valEl = inp.parentElement?.querySelector("strong");
    if (valEl) valEl.textContent = `${v >= 0 ? "+" : ""}${v.toFixed(1)} dB`;
  });

  el("btn-overlay-acoustics-save")?.addEventListener("click", async () => {
    const bandsDb = [0, 1, 2, 3, 4].map((idx) => {
      const inp = el("overlay-eq-sliders")?.querySelector(`.overlay-eq-band[data-band="${idx}"]`);
      return Number(inp?.value || 0);
    });
    try {
      await saveGameEqAndDenoise({
        gameId: activeOverlayGameId,
        presetName: el("select-overlay-eq-preset")?.value || "Neutral Flat",
        bandsDb,
        micDenoiseEnabled: Boolean(el("chk-overlay-denoise-enabled")?.checked),
        micDenoiseStrength: Number(el("range-overlay-denoise-strength")?.value || 0.75)
      });
      setStatus("Saved per-game 5-band EQ & RNNoise profile.");
    } catch (err) {
      setStatus(String(err));
    }
  });

  el("btn-overlay-denoise-bench")?.addEventListener("click", async () => {
    try {
      const strength = Number(el("range-overlay-denoise-strength")?.value || 0.75);
      const tel = await testMicRnnoiseLatency(strength);
      if (el("overlay-acoustics-latency-badge")) {
        el("overlay-acoustics-latency-badge").textContent = `${Number(tel?.processingMs || 0).toFixed(2)} ms · -${Number(tel?.noiseReductionDb || 0).toFixed(1)} dB`;
      }
      setStatus(`RNNoise 10ms frame bench: ${Number(tel?.processingMs || 0).toFixed(2)} ms`);
    } catch (err) {
      setStatus(String(err));
    }
  });

  el("range-overlay-speedhack")?.addEventListener("input", (e) => {
    const val = Number(e.target.value || 1.0);
    if (el("overlay-sandbox-speed-pill")) {
      el("overlay-sandbox-speed-pill").textContent = `${val.toFixed(2)}x`;
    }
  });

  el("btn-overlay-apply-speed")?.addEventListener("click", async () => {
    const scale = Number(el("range-overlay-speedhack")?.value || 1.0);
    try {
      const st = await sandboxSetSpeedhackScale(
        activeOverlayPid > 0 ? activeOverlayPid : null,
        scale
      );
      renderOverlaySandboxTrainer(st);
      setStatus(st?.statusText || `Speedhack timer set to ${scale.toFixed(2)}x`);
    } catch (err) {
      setStatus(String(err));
      refreshOverlaySandboxState().catch(() => {});
    }
  });

  el("btn-overlay-reset-speed")?.addEventListener("click", async () => {
    try {
      const st = await sandboxSetSpeedhackScale(
        activeOverlayPid > 0 ? activeOverlayPid : null,
        1.0
      );
      renderOverlaySandboxTrainer(st);
      setStatus("Timer resolution restored to 1.00x.");
    } catch (err) {
      setStatus(String(err));
    }
  });

  const runSandboxScan = async (resetScan) => {
    if (!activeOverlayPid) {
      await refreshOverlayState();
    }
    if (!activeOverlayPid) {
      setStatus("Launch a single-player game first to attach memory scanner.");
      return;
    }
    const vtype = el("select-overlay-scan-type")?.value || "i32";
    const valStr = (el("input-overlay-scan-val")?.value || "").trim();
    if (!valStr) {
      setStatus("Enter a numeric target value first.");
      return;
    }
    try {
      const res = await sandboxScanMemory(activeOverlayPid, vtype, valStr, resetScan);
      renderOverlaySandboxMatches(res);
      setStatus(res?.summary || "Memory scan finished.");
    } catch (err) {
      setStatus(String(err));
      refreshOverlaySandboxState().catch(() => {});
    }
  };
  el("btn-overlay-scan-new")?.addEventListener("click", () => runSandboxScan(true));
  el("btn-overlay-scan-next")?.addEventListener("click", () => runSandboxScan(false));

  el("overlay-sandbox-matches")?.addEventListener("click", async (e) => {
    const btn = e.target.closest("[data-sandbox-addr]");
    if (!btn || !activeOverlayPid) return;
    const addr = btn.dataset.sandboxAddr;
    const vtype = btn.dataset.sandboxType || "i32";
    const curVal = btn.dataset.sandboxCur || "";
    const nextVal = window.prompt(`Write new ${vtype} value at ${addr}:`, curVal);
    if (nextVal == null || !nextVal.trim()) return;
    try {
      const msg = await sandboxWriteMemoryValue(activeOverlayPid, addr, vtype, nextVal.trim());
      setStatus(msg);
      const valInput = el("input-overlay-scan-val");
      if (valInput) valInput.value = nextVal.trim();
      await runSandboxScan(false);
    } catch (err) {
      setStatus(String(err));
    }
  });

  try {
    await listenEvent("overlay-opened", (payload) => {
      if (payload?.visible) {
        refreshOverlayState();
        setStatus("Overlay active (Guide / Alt+G)");
      }
    });
    await listenEvent("game-running-state", () => refreshOverlayState());
    await listenEvent("game-suspend-state", () => refreshOverlayState());
    await listenEvent("quick-resume-updated", (summary) => {
      if (summary) renderOverlayQuickResume(summary);
    });
    await listenDukeVisionResult((res) => {
      switchOverlayTab("vision");
      renderOverlayVisionResult(res);
    });
    await listenDukeCommsVad((vad) => {
      if (el("overlay-comms-vad-pill") && vad) {
        el("overlay-comms-vad-pill").textContent = vad.speech ? `VAD Speech (${Math.round(Number(vad.prob || 0) * 100)}%)` : "Tap Live";
      }
    });
    await listenDukeCommsAction((evt) => {
      if (evt && el("overlay-comms-action-log")) {
        el("overlay-comms-action-log").textContent = `${evt.phrase || evt.actionKind} -> ${evt.targetSummary || "Dispatched"}`;
      }
    });
    await listenDukeCommsSubtitle(() => {
      getDukeCommsState().then(renderOverlayCommsState).catch(() => {});
    });

    await refreshOverlayState();
  } catch (err) {
    setStatus(`Overlay init warning: ${String(err)}`);
  }
}

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
      const badgeText = String(rep.shieldBadge || (String(rep.verdict || "").includes("DANGER") ? "Blocked" : "Safe"));
      const badgeLower = badgeText.toLowerCase();
      const danger = badgeLower === "blocked" || String(rep.verdict || "").includes("DANGER");
      const warn = !danger && badgeLower === "warning";
      const badgeClass = danger ? "danger" : warn ? "amber" : "emerald";
      const entropyText =
        typeof rep.shannonEntropy === "number"
          ? `${rep.shannonEntropy.toFixed(3)} b/B`
          : "";
      const metaBadges = [
        rep.sizeFormatted ? escapeHtml(rep.sizeFormatted) : "",
        entropyText ? `Entropy: ${escapeHtml(entropyText)}` : "",
        rep.peArchitecture ? escapeHtml(rep.peArchitecture) : "",
        rep.authenticodeStatus ? `Signer: ${escapeHtml(rep.authenticodeStatus)}` : "",
        rep.zoneUnblocked ? "MOTW Unblocked" : ""
      ]
        .filter(Boolean)
        .join(" | ");
      const behaviors = Array.isArray(rep.suspiciousBehaviors) ? rep.suspiciousBehaviors : [];
      const behaviorHtml =
        behaviors.length > 0
          ? `<ul class="shield-reasons-list">${behaviors
              .map((b) => `<li><strong>[${escapeHtml(b.severity || "Info")}] ${escapeHtml(b.title || "Heuristic")}:</strong> ${escapeHtml(b.explanation || "")}</li>`)
              .join("")}</ul>`
          : "";
      const quarantineHtml = rep.quarantinePath
        ? `<div class="panel-sub mono-num">Quarantined: ${escapeHtml(rep.quarantinePath)}</div>`
        : "";
      const detailsHtml = Array.isArray(rep.details) && rep.details.length > 0
        ? `<div class="panel-sub" style="font-family: 'Cascadia Code', 'Consolas', monospace; font-variant-numeric: tabular-nums; font-size: 11px;">${rep.details.map((d) => escapeHtml(d)).join("<br>")}</div>`
        : "";
      return `
      <div class="dl-job-card">
        <div class="dl-job-top">
          <span class="dl-job-title">${escapeHtml(rep.fileName)}</span>
          <span class="pill ${badgeClass}">${escapeHtml(badgeText)} (${escapeHtml(rep.verdict || "SAFE")})</span>
        </div>
        ${metaBadges ? `<div class="panel-sub" style="font-family: 'Cascadia Code', 'Consolas', monospace; font-variant-numeric: tabular-nums;">${metaBadges}</div>` : ""}
        <div class="panel-sub">${escapeHtml(rep.summary)}</div>
        ${behaviorHtml}
        ${quarantineHtml}
        ${detailsHtml}
        <div class="dl-job-actions">
          <button type="button" class="btn btn-secondary btn-xs" data-rep-action="folder" data-rep-path="${escapeHtml(rep.filePath)}">${svgIcon("icon-folder-open")}<span>Open Folder</span></button>
          ${rep.sha256 ? `<button type="button" class="btn btn-secondary btn-xs" data-rep-action="threat-feed" data-rep-sha256="${escapeHtml(rep.sha256)}" data-rep-path="${escapeHtml(rep.filePath)}">${svgIcon("icon-shield-check")}<span>Threat Lookup</span></button>` : ""}
          ${!danger ? `<button type="button" class="btn btn-secondary btn-xs" data-rep-action="unblock" data-rep-path="${escapeHtml(rep.filePath)}">${svgIcon("icon-lock")}<span>Unblock MOTW</span></button>` : ""}
          ${danger || warn ? `<button type="button" class="btn btn-danger-ghost btn-xs" data-rep-action="delete" data-rep-path="${escapeHtml(rep.filePath)}">${svgIcon("icon-trash-2")}<span>Delete File</span></button>` : ""}
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
      const ariaText = activeQuery ? `Search ${site.name} for ${activeQuery}` : `Open ${site.name}`;
      const highlightedClass = activeQuery ? "is-highlighted" : "";
      let domainLabel = "";
      try {
        domainLabel = new URL(site.homeUrl || site.searchUrl || "").hostname.replace(/^www\./, "");
      } catch {
        domainLabel = String(site.homeUrl || site.searchUrl || "official").slice(0, 32);
      }
      const queryClip = activeQuery.length > 20 ? `${activeQuery.slice(0, 18)}..` : activeQuery;
      const rightFooter = activeQuery
        ? `<span class="directory-search-chip">Query: ${escapeHtml(queryClip)}</span>`
        : `<span class="directory-tag">${escapeHtml(site.tag || "Official")}</span>`;
      return `
        <article class="directory-card ${highlightedClass}" data-site-id="${escapeHtml(site.id)}" tabindex="0" role="button" aria-label="${escapeHtml(ariaText)}">
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
            <span class="directory-domain-tag mono-num">${svgIcon("icon-globe")}<span>${escapeHtml(domainLabel)}</span></span>
            <span class="directory-card-tools">
              ${rightFooter}
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
  clearControllerFocus({ disconnect: !state.controllerConnected, blurNonText: true, resetRepeats: true });
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
  clearControllerFocus({ disconnect: !state.controllerConnected, blurNonText: true, resetRepeats: true });
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
    platform: String(c.platform || "all"),
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

  const isRetro = Number(entry.platformId || 0) > 0;
  const isInstalled = isCatalogGameInstalled(entry, installedSet);
  const modeTags = [];
  if (entry.isSingleplayer) modeTags.push("Single-Player");
  if (entry.isCoop) modeTags.push("Co-op");
  const modePill = isRetro
    ? (entry.platformName || "Console")
    : (modeTags.length > 0 ? modeTags.join(" · ") : (entry.genres?.[0] || "Steam"));

  const priceText = isRetro
    ? (entry.releaseDate || entry.platformSlug?.toUpperCase() || "Console")
    : (entry.priceCents > 0 ? (entry.priceFormatted || `$${(entry.priceCents / 100).toFixed(2)}`) : "Free");
  const ccuText = isRetro
    ? (entry.platformName || "Console")
    : (Number(entry.ccu || 0) > 0 ? `${Number(entry.ccu).toLocaleString()} live` : "Steam");
  const ratingText = isRetro
    ? (entry.developers?.[0] || "")
    : (Number(entry.reviewPct || 0) > 0 ? `${Math.round(entry.reviewPct)}% pos` : (entry.ownersLabel || ""));

  const media = document.createElement("div");
  media.className = "catalog-card-media";

  const img = document.createElement("img");
  img.className = "catalog-card-img";
  img.alt = "";
  img.loading = "lazy";
  img.decoding = "async";
  img.dataset.fallbackStage = "0";
  const cacheKey = isRetro ? `retro_${entry.appId}` : String(entry.appId);
  const cachedPath = CATALOG_COVER_CACHE[cacheKey];
  const primaryCover = isRetro
    ? (resolveMediaUrl(cachedPath || entry.coverUrl) || resolveMediaUrl(entry.headerUrl) || "")
    : (resolveMediaUrl(cachedPath || entry.coverUrl) ||
      `https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/${entry.appId}/library_600x900.jpg`);
  const headerFallback = isRetro
    ? (resolveMediaUrl(entry.headerUrl) || "")
    : (resolveMediaUrl(entry.headerUrl) ||
      `https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/${entry.appId}/header.jpg`);

  const showCatalogFallbackCard = () => {
    img.style.display = "none";
    if (!media.querySelector(".cover-fallback-card")) {
      media.prepend(
        createProceduralCoverCardEl(
          entry.name || (isRetro ? "Console Game" : "Steam Game"),
          isRetro ? (entry.platformName || "Console") : "Steam",
          entry.platformName || modePill
        )
      );
    }
  };

  img.addEventListener("error", () => {
    const stage = Number(img.dataset.fallbackStage || "0");
    if (stage === 0 && headerFallback && img.src !== headerFallback) {
      img.dataset.fallbackStage = "1";
      img.src = headerFallback;
    } else if (stage < 2) {
      img.dataset.fallbackStage = "2";
      showCatalogFallbackCard();
      scheduleCatalogCoverResolve(entry, img, media);
    }
  });

  media.appendChild(img);
  if (primaryCover) {
    img.src = primaryCover;
  } else {
    showCatalogFallbackCard();
    scheduleCatalogCoverResolve(entry, img, media);
  }

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

  const installedBadge = isInstalled ? '<span class="card-footer-chip">Installed</span>' : "";
  const footerModeTag = isRetro
    ? (entry.platformName || "Console")
    : (modeTags.length > 0 ? modeTags.join(" / ") : (entry.genres?.[0] || "Steam"));
  const body = document.createElement("div");
  body.className = "catalog-card-body";
  body.innerHTML = `
    <div class="catalog-card-title" title="${escapeHtml(entry.name)}">${escapeHtml(entry.name)}</div>
    <div class="catalog-card-sub">
      <span>${escapeHtml(footerModeTag)}</span>
      <span class="mono-num">${escapeHtml(ccuText)}${ratingText ? ` · ${escapeHtml(ratingText)}` : ""}</span>
    </div>
    <div class="catalog-card-footer-tags">
      <span class="catalog-footer-price ${!isRetro && entry.priceCents === 0 ? "is-free" : ""} mono-num">${escapeHtml(priceText)}</span>
      <span class="card-footer-chip">${escapeHtml(isRetro ? (entry.genres?.[0] || "Console") : "PC")}</span>
      ${installedBadge}
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
      const semanticTag = c.semanticMode && c.semanticSummary ? ` · DKCAT003: ${c.semanticSummary}` : "";
      summary.textContent = `Showing ${shownCount.toLocaleString()} of ${c.totalMatches.toLocaleString()} matching titles across ${indexedDisplay} indexed${latencyTag}${semanticTag}${liveTag}`;
    }
  }

  if (statusSub) {
    if (c.syncing) {
      statusSub.textContent = `Updating catalog · ${c.totalCached.toLocaleString()} / ${Number(c.syncTarget || 30000).toLocaleString()}+ games ready`;
    } else if (c.fetchedAt > 0) {
      const when = new Date(c.fetchedAt * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
      statusSub.textContent = `${c.fromCache ? "Instant offline index" : "Updated catalog"} · Synced ${when} · ${c.totalCached.toLocaleString()} games`;
    } else {
      statusSub.textContent = "Instant offline game catalog · Ready";
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
    c.semanticMode = Boolean(page?.semanticMode);
    c.semanticSummary = String(page?.semanticSummary || "");

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

  const activePlatform = String(c.platform || "all");
  document.querySelectorAll("#catalog-platform-pills .catalog-platform-pill").forEach((pill) => {
    const isMatch = String(pill.dataset.catalogPlatform || "all") === activePlatform;
    pill.classList.toggle("active", isMatch);
    pill.setAttribute("aria-pressed", isMatch ? "true" : "false");
  });

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
  let alreadyRenderedCached = false;
  if (!forceRefresh && c.totalCached > 0 && c.renderedCount === 0) {
    alreadyRenderedCached = true;
    await runCatalogQuery(true);
  }

  try {
    const resp = await fetchCatalogGames(0, query, forceRefresh);
    if (reqId !== c.requestId) return;

    const prevCached = c.totalCached;
    c.totalCached = Number(resp?.totalCached || c.totalCached || 0);
    c.fetchedAt = Number(resp?.fetchedAt || c.fetchedAt || 0);
    c.fromCache = Boolean(resp?.fromCache);
    c.staleWarning = resp?.staleWarning || null;
    c.loaded = true;
    c.loading = false;
    c.error = null;

    updateNavCatalogBadge();
    if (!alreadyRenderedCached || c.totalCached !== prevCached || c.renderedCount === 0) {
      await runCatalogQuery(true);
    } else {
      updateCatalogResultsSummary();
    }

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
  const isRetro = Number(entry.platformId || 0) > 0;
  state.catalog.modalGame = entry;

  if (el("catalog-modal-title")) el("catalog-modal-title").textContent = entry.name || (isRetro ? "Console Game" : "Steam Game");
  if (el("catalog-modal-desc")) {
    el("catalog-modal-desc").textContent =
      entry.shortDescription ||
      (isRetro
        ? `${entry.platformName || "Console"} title indexed in local catalog.`
        : "Game catalog entry stored in local offline library.");
  }

  el("btn-catalog-open-store")?.classList.toggle("hidden", isRetro);
  el("btn-catalog-open-steamdb")?.classList.toggle("hidden", isRetro);
  el("btn-catalog-add-library")?.classList.toggle("hidden", !isRetro);
  if (el("btn-catalog-link-exe-label")) {
    el("btn-catalog-link-exe-label").textContent = isRetro ? "Link Local ROM / ISO" : "Link Local Exe";
  }

  const priceLabel =
    entry.priceCents > 0
      ? entry.priceFormatted || `$${(entry.priceCents / 100).toFixed(2)}`
      : "Free";
  const discBadge =
    entry.discountPercent > 0 ? `<span class="pill emerald mono-num">-${entry.discountPercent}%</span>` : "";

  if (el("catalog-modal-badges")) {
    el("catalog-modal-badges").innerHTML = isRetro
      ? `
        <span class="pill accent">${escapeHtml(entry.platformName || "Console")}</span>
        ${entry.releaseDate ? `<span class="pill mono-num">${escapeHtml(entry.releaseDate)}</span>` : ""}
        ${(entry.genres || [])[0] ? `<span class="pill">${escapeHtml(entry.genres[0])}</span>` : ""}
      `
      : `
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

    el("catalog-modal-meta").innerHTML = isRetro
      ? `
        <div class="kv-row"><span class="kv-k">System</span><span class="kv-v">${escapeHtml(entry.platformName || "Console")}</span></div>
        <div class="kv-row"><span class="kv-k">Developer</span><span class="kv-v">${escapeHtml((entry.developers || []).join(", ") || "--")}</span></div>
        <div class="kv-row"><span class="kv-k">Publisher</span><span class="kv-v">${escapeHtml((entry.publishers || []).join(", ") || "--")}</span></div>
        <div class="kv-row"><span class="kv-k">Release Year</span><span class="kv-v">${escapeHtml(entry.releaseDate || "--")}</span></div>
        <div class="kv-row"><span class="kv-k">Genres</span><span class="kv-v">${escapeHtml((entry.genres || []).join(", ") || "--")}</span></div>
      `
      : `
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
  const catalogSignal = beginScopedRequest("catalog-modal");
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
    !catalogSignal.aborted &&
    modalReqId === state.catalog.modalRequestId &&
    !el("catalog-detail-modal")?.classList.contains("hidden");

  const isRetro = Number(entry.platformId || 0) > 0;
  const heroCandidates = isRetro
    ? [entry.heroUrl, entry.coverUrl, entry.headerUrl].filter(Boolean)
    : buildHeroCandidateList(entry.appId, entry.coverUrl);

  applyHeroCandidateChain(modalHeroImg, heroCandidates, isStillModalCurrent);

  try {
    const enriched = await getCatalogGameDetails(entry.appId);
    if (!isStillModalCurrent()) return;
    if (enriched) {
      state.catalog.itemsById.set(Number(enriched.appId), enriched);
      populateCatalogModal(enriched);
      if (isRetro) {
        const nextCandidates = [enriched.heroUrl, enriched.coverUrl, enriched.headerUrl].filter(Boolean);
        if (nextCandidates.length > 0 && (!modalHeroImg?.getAttribute("src") || modalHeroImg.classList.contains("hidden"))) {
          applyHeroCandidateChain(modalHeroImg, nextCandidates, isStillModalCurrent);
        }
      }
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

function renderWebCoverResults(candidates = state.webCoverResults) {
  const container = el("web-cover-results-container");
  if (!container) return;
  if (!Array.isArray(candidates)) {
    container.innerHTML = `<div class="panel-sub">Click Search Web Covers to browse candidate cover art via Steam, Libretro &amp; DoH Web Image Search, or pick a local image.</div>`;
    return;
  }
  if (candidates.length === 0) {
    container.innerHTML = `<div class="panel-sub">No matching covers found. Try simplifying the search query above or choose Local Image...</div>`;
    return;
  }
  container.innerHTML = `
    <div class="sgdb-grid-gallery">
      ${candidates
        .map(
          (item) => `
          <button type="button" class="sgdb-thumb-card" data-web-cover-url="${escapeHtml(item.url)}">
            <img src="${escapeHtml(item.thumb || item.url)}" alt="${escapeHtml(item.label || "Cover candidate")}" loading="lazy" decoding="async" />
            <span class="sgdb-thumb-label">${escapeHtml(item.source || "Cover")} &middot; ${escapeHtml(item.label || "")}</span>
          </button>
        `
        )
        .join("")}
    </div>
  `;
}

async function runWebCoverSearch() {
  const game = state.library.find((g) => g.id === state.modalGameId);
  const query = (el("input-cover-query")?.value || game?.title || "").trim();
  if (!query) return;
  const container = el("web-cover-results-container");
  if (container) container.innerHTML = `<div class="panel-sub">Searching Steam, Libretro &amp; DoH Web Images for "${escapeHtml(query)}"...</div>`;
  try {
    const candidates = await searchWebCoversForGame(
      query,
      game ? resolveGameSourceLabel(game) : null,
      game?.steamAppId || game?.appId || null
    );
    state.webCoverResults = Array.isArray(candidates) ? candidates : [];
    renderWebCoverResults(state.webCoverResults);
  } catch (err) {
    if (container) container.innerHTML = `<div class="panel-sub">${escapeHtml(String(err))}</div>`;
  }
}

async function runSgdbSearch() {
  const query = (el("input-sgdb-query")?.value || "").trim();
  const game = state.library.find((g) => g.id === state.modalGameId);
  const queryOrAppId = query || (game?.steamAppId ? String(game.steamAppId) : game?.title || "");
  if (!queryOrAppId) return;

  const container = el("sgdb-results-container");
  if (container) container.innerHTML = `<div class="panel-sub">Searching artwork on SteamGridDB...</div>`;

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
  state.webCoverResults = null;
  state.sgdbResults = null;
  state.sgdbSelection = { coverUrl: null, heroUrl: null, iconUrl: null };
  switchCoverModalTab("steam");

  if (el("details-title")) el("details-title").textContent = game.title;
  if (el("details-size-badge")) el("details-size-badge").textContent = game.sizeFormatted || formatBytesJs(game.sizeBytes);
  if (el("details-genre-badge")) el("details-genre-badge").textContent = resolveGameSourceLabel(game);
  if (el("input-cover-query")) el("input-cover-query").value = game.title || "";
  if (el("input-sgdb-query")) el("input-sgdb-query").value = game.title || "";
  renderWebCoverResults(null);
  renderSgdbResults();

  if (el("details-meta-list")) {
    el("details-meta-list").innerHTML = `
      <div class="kv-row"><span class="kv-k">Executable</span><span class="kv-v mono-num">${escapeHtml(game.exePath)}</span></div>
      <div class="kv-row"><span class="kv-k">Install Folder</span><span class="kv-v mono-num">${escapeHtml(game.installDir)}</span></div>
      <div class="kv-row"><span class="kv-k">Source</span><span class="kv-v mono-num">${escapeHtml(resolveGameSourceLabel(game))}</span></div>
      <div class="kv-row"><span class="kv-k">Playtime</span><span class="kv-v mono-num">${escapeHtml(
        Number(game.importedPlaytimeSecs || 0) > 0
          ? `${formatPlaytime(getTotalPlaytimeSecs(game))} (${formatPlaytime(Number(game.importedPlaytimeSecs || 0))} synced + ${formatPlaytime(Number(game.dukePlaytimeSecs || 0))} Duke)`
          : formatPlaytime(getTotalPlaytimeSecs(game))
      )}</span></div>
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
        : `<option value="">No saved backups found</option>`;
  }

  if (list) {
    if (snaps.length === 0) {
      list.innerHTML = `<div class="panel-sub">No save-file backups exist for this game yet. Click Back Up Saves first.</div>`;
    } else {
      list.innerHTML = snaps
        .map(
          (s) => `
          <div class="restore-snapshot-item">
            <span>${escapeHtml(s.filename)} · ${escapeHtml(formatSyncTimestamp(s.createdTs))}</span>
            <span>${escapeHtml(s.sizeFormatted || formatBytesJs(s.sizeBytes))} (${Number(s.rootCount || 1)} folder${Number(s.rootCount || 1) === 1 ? "" : "s"})</span>
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
  if (el("restore-save-title")) el("restore-save-title").textContent = `Restore Saves: ${game.title}`;
  if (el("restore-save-subtitle")) {
    el("restore-save-subtitle").textContent = "Pick a saved backup below to safely restore your game save files.";
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
    list.innerHTML = `<div class="panel-sub">No restore points saved yet. Click Save Restore Point above to create one in 1 second.</div>`;
    return;
  }
  list.innerHTML = snaps
    .map(
      (s) => `
      <div class="restore-snapshot-item">
        <div>
          <strong>${escapeHtml(s.label || s.id)}</strong>
          <span class="panel-sub"> · ${escapeHtml(formatSyncTimestamp(s.createdAt))} · ${Number(s.fileCount || 0).toLocaleString()} files (${escapeHtml(formatBytesJs(s.totalBytes || 0))}, ${Number(s.deduplicatedFiles || 0)} shared, ${Number(s.elapsedMs || 0)} ms)</span>
        </div>
        <div class="snapshot-row-actions">
          <button type="button" class="btn btn-primary btn-xs" data-snapshot-restore="${escapeHtml(s.id)}">Restore Backup</button>
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
  if (el("snapshots-modal-title")) el("snapshots-modal-title").textContent = `Restore Points: ${game.title}`;
  if (el("input-snapshot-label")) el("input-snapshot-label").value = "";
  if (el("snapshots-list")) el("snapshots-list").innerHTML = `<div class="panel-sub">Loading saved restore points...</div>`;
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
    subEl.textContent = `${report.architecture || "Windows App"} · ${totalImported} runtime files checked in ${Number(report.elapsedUs || 0)} us`;
  }

  if (missing.length === 0) {
    listEl.innerHTML = `<div class="restore-snapshot-item"><span>All ${totalImported} required Windows runtime files are present and ready to play.</span></div>`;
  } else {
    listEl.innerHTML = missing
      .map(
        (d) => `
        <div class="restore-snapshot-item">
          <div>
            <strong>${escapeHtml(d.name)}</strong>
            ${d.isRuntimeHint ? `<span class="panel-sub"> (${escapeHtml(d.isRuntimeHint)})</span>` : ""}
          </div>
          <span class="pill">${d.healCandidatePath ? "Auto-Repair Available" : "Missing Runtime File"}</span>
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
      ? `Missing Files Check: ${game.title}`
      : "Missing Windows Files Check";
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
  refreshFrontierDiagnostics().catch(() => {});
}

let pendingEmuInstallPrompt = null;

function renderEmulatorStatusCards(statuses) {
  const container = el("settings-emulators-list");
  if (!container) return;
  const list = Array.isArray(statuses) ? statuses : [];
  if (list.length === 0) {
    container.innerHTML = `<div class="panel-sub">No console players registered yet.</div>`;
    return;
  }

  let readyCount = 0;
  container.innerHTML = list
    .map((emu) => {
      const isReady = emu.status === "ready";
      const isCustom = emu.status === "custom_path";
      if (isReady || isCustom) readyCount += 1;
      const statusClass = isReady ? "ready" : isCustom ? "custom" : "";
      const statusLabel = isReady ? "Ready" : isCustom ? "Custom Path" : "Not Installed";
      const activePath = emu.customPath || emu.exePath || "";
      const setupBtn = emu.supportsAutoInstall
        ? `<button type="button" class="btn btn-primary btn-xs" data-emu-setup="${escapeHtml(emu.id)}">
             ${svgIcon("icon-download")}
             <span>${isReady ? "Reinstall" : "Set Up"}</span>
           </button>`
        : "";
      const clearBtn = emu.customPath
        ? `<button type="button" class="btn btn-secondary btn-xs" data-emu-clear="${escapeHtml(emu.id)}">
             <span>Clear</span>
           </button>`
        : "";

      return `
        <article class="emu-card" data-emu-id="${escapeHtml(emu.id)}">
          <div class="emu-card-head">
            <div>
              <div class="emu-card-title">${escapeHtml(emu.displayName)}</div>
              <div class="emu-card-consoles">${escapeHtml((emu.consoleNames || []).join(" · "))}</div>
            </div>
            <span class="emu-status-pill ${statusClass}">${escapeHtml(statusLabel)}</span>
          </div>
          <p class="emu-card-caption">${escapeHtml(emu.caption || "")}</p>
          <div class="emu-card-path" title="${escapeHtml(activePath || emu.installDir || "")}">
            ${escapeHtml(activePath || `Portable folder: ${emu.installDir || "emulators"}`)}
          </div>
          <div class="emu-card-actions">
            ${setupBtn}
            <button type="button" class="btn btn-secondary btn-xs" data-emu-browse="${escapeHtml(emu.id)}">
              ${svgIcon("icon-folder-open")}
              <span>Browse Exe...</span>
            </button>
            ${clearBtn}
          </div>
        </article>
      `;
    })
    .join("");

  const statusLine = el("settings-retro-sync-status");
  if (statusLine && !statusLine.dataset.syncBusy) {
    statusLine.textContent = `${readyCount} of ${list.length} portable console players configured · Zero registry writes.`;
  }
}

async function refreshBiosVaultStatus(showBusyToast = false) {
  try {
    const rep = await getBiosVaultStatus();
    renderBiosVaultReport(rep);
    if (showBusyToast) {
      showToast(rep?.summary || "BIOS Vault verified.", "info", 2800);
    }
  } catch (err) {
    if (el("settings-bios-summary")) {
      el("settings-bios-summary").textContent = String(err || "Unable to verify BIOS vault.");
    }
  }
}

function renderBiosVaultReport(rep) {
  const summaryEl = el("settings-bios-summary");
  const gridEl = el("settings-bios-grid");
  if (summaryEl) {
    summaryEl.textContent =
      rep?.summary || "Checking DuckStation, PCSX2, melonDS, Azahar & RetroArch BIOS readiness...";
  }
  if (!gridEl) return;
  const list = Array.isArray(rep?.consoles) ? rep.consoles : [];
  if (list.length === 0) {
    gridEl.innerHTML = `<div class="panel-sub">No BIOS definitions available.</div>`;
    return;
  }
  const quarantineNote =
    Array.isArray(rep?.quarantined) && rep.quarantined.length > 0
      ? `<div class="bios-card">
           <div class="bios-card-head">
             <span class="bios-card-title">Quarantined Bad Dumps (${rep.quarantined.length})</span>
             <span class="pill danger mono-num">Quarantined</span>
           </div>
           <div class="bios-card-meta mono-num">${escapeHtml(
             rep.quarantined.map((q) => `${q.filename} (${q.actualMd5})`).slice(0, 3).join(", ")
           )}</div>
         </div>`
      : "";

  gridEl.innerHTML =
    list
      .map((c) => {
        const badgeCls =
          c.badgeStatus === "ready"
            ? "ok"
            : c.badgeStatus === "bad_dump"
              ? "amber"
              : c.required
                ? "danger"
                : "";
        const badgeLabel =
          c.badgeStatus === "ready"
            ? "Ready"
            : c.badgeStatus === "bad_dump"
              ? "Bad Dump"
              : c.required
                ? "Missing Required"
                : "Optional";
        return `
          <article class="bios-card">
            <div class="bios-card-head">
              <span class="bios-card-title">${escapeHtml(c.consoleName || c.consoleId)}</span>
              <span class="pill ${badgeCls} mono-num">${escapeHtml(badgeLabel)}</span>
            </div>
            <div class="bios-card-meta mono-num">${escapeHtml(c.filename)} · MD5 ${escapeHtml(c.expectedMd5)}</div>
            <div class="panel-sub">${escapeHtml(
              c.ready && Array.isArray(c.installedPaths) && c.installedPaths.length > 0
                ? `Installed (${c.installedPaths.length} target${c.installedPaths.length === 1 ? "" : "s"}${c.casBackupPath ? " + CAS backup" : ""})`
                : `Missing: place ${c.filename} (MD5 ${c.expectedMd5}) in vault or drop .zip`
            )}</div>
          </article>
        `;
      })
      .join("") + quarantineNote;
}

function renderRetroAchievementsReport(rep) {
  const pill = el("detail-ra-hardcore-pill");
  const summary = el("detail-ra-summary");
  if (pill) {
    const hc = Number(rep?.hardcoreCount || 0);
    const total = Number(rep?.totalCount || 0);
    pill.textContent = `Hardcore ${hc}${total > 0 ? `/${total}` : ""}`;
    pill.classList.toggle("ok", hc > 0);
  }
  if (summary) {
    const hashShort = rep?.romHashMd5 ? `${rep.romHashMd5} (${rep.hashMethod || "rc_hash"})` : "";
    summary.textContent = [rep?.statusMessage || "RetroAchievements ready.", hashShort]
      .filter(Boolean)
      .join(" · ");
  }
  const items = Array.isArray(rep?.achievements) ? rep.achievements : [];
  if (items.length > 0) {
    renderDetailAchievements({
      source: "RetroAchievements",
      unlockedCount: Number(rep.unlockedCount || 0),
      totalCount: Number(rep.totalCount || items.length),
      completionPct: Number(rep.completionPct || 0),
      achievements: items.map((a) => ({
        displayName: `${a.title}${a.hardcoreUnlocked ? " [HC]" : ""}`,
        description: `${a.description || ""} (${a.points || 0} pts)`.trim(),
        unlocked: Boolean(a.unlocked),
        unlockedAt: a.unlockTimestamp || 0,
        source: a.hardcoreUnlocked ? "RA Hardcore" : "RetroAchievements"
      }))
    });
  }
}

async function loadDetailRetroAchievements(game, requestId, forceRefresh = false) {
  if (!game || Number(game.platformId || 0) <= 0) return;
  if (el("detail-ra-summary")) {
    el("detail-ra-summary").textContent = "Computing rcheevos MD5 & querying RetroAchievements...";
  }
  try {
    const rep = await fetchRetroAchievementsProgress(game.id, forceRefresh);
    if (requestId !== state.activeDetailRequestId || state.selectedDetailId !== game.id) return;
    renderRetroAchievementsReport(rep);
    if (Array.isArray(rep?.newlyUnlockedTitles) && rep.newlyUnlockedTitles.length > 0) {
      playAchievementChime();
      showToast(
        `RetroAchievements Unlocked: ${rep.newlyUnlockedTitles.join(", ")}`,
        "success",
        4200
      );
    }
  } catch (err) {
    if (requestId !== state.activeDetailRequestId || state.selectedDetailId !== game.id) return;
    if (el("detail-ra-summary")) {
      el("detail-ra-summary").textContent = String(err || "RetroAchievements unavailable.");
    }
  }
}

async function loadEmulatorSettingsSection(showBusyToast = false) {
  try {
    const [statuses] = await Promise.all([
      getEmulatorStatuses(),
      refreshBiosVaultStatus(false)
    ]);
    renderEmulatorStatusCards(statuses);
    if (showBusyToast) {
      showToast("Console players & BIOS vault refreshed.", "info", 2200);
    }
  } catch (err) {
    const container = el("settings-emulators-list");
    if (container) {
      container.innerHTML = `<div class="panel-sub">${escapeHtml(String(err || "Unable to inspect portable emulators."))}</div>`;
    }
  }
}

function openEmulatorInstallModal({ game, consoleSlug, consoleName, emuDisplayName }) {
  pendingEmuInstallPrompt = {
    game,
    consoleSlug: String(consoleSlug || ""),
    consoleName: String(consoleName || "Console"),
    emuDisplayName: String(emuDisplayName || "Portable Emulator")
  };
  if (el("emu-install-prompt-title")) {
    el("emu-install-prompt-title").textContent = `Download ${pendingEmuInstallPrompt.consoleName} Emulator & Play`;
  }
  if (el("emu-install-prompt-desc")) {
    el("emu-install-prompt-desc").textContent =
      `"${pendingEmuInstallPrompt.emuDisplayName}" is needed to play "${game?.title || "this game"}". ` +
      `Duke Launcher will install it into its portable folder (no registry clutter) and start playing automatically.`;
  }
  el("emu-install-progress-wrap")?.classList.add("hidden");
  if (el("emu-install-progress-label")) el("emu-install-progress-label").textContent = "Preparing portable package...";
  if (el("emu-install-progress-pct")) el("emu-install-progress-pct").textContent = "0%";
  if (el("emu-install-progress-fill")) el("emu-install-progress-fill").style.width = "0%";
  if (el("btn-emu-install-and-play")) el("btn-emu-install-and-play").disabled = false;
  el("emu-install-prompt-modal")?.classList.remove("hidden");
}

function closeEmulatorInstallModal() {
  el("emu-install-prompt-modal")?.classList.add("hidden");
  pendingEmuInstallPrompt = null;
  clearControllerFocus({ disconnect: !state.controllerConnected, blurNonText: true, resetRepeats: true });
}

async function launchGameDirectly(game) {
  if (!game) return;
  try {
    showToast(`Launching ${game.title}...`, "info");
    await invokeCommand("launch_game", {
      exePath: game.exePath || "",
      gameId: game.id,
      launchArgs: game.launchArgs || null
    });
    setTimeout(refreshActiveSessions, 400);
  } catch (err) {
    const msg = String(err || "");
    if (msg.startsWith("EMULATOR_NOT_INSTALLED|")) {
      const [, consoleSlug, consoleName, emuDisplayName] = msg.split("|");
      openEmulatorInstallModal({
        game,
        consoleSlug: consoleSlug || "",
        consoleName: consoleName || "Console",
        emuDisplayName: emuDisplayName || "Portable Emulator"
      });
      return;
    }
    if (msg.startsWith("ROM_NOT_LINKED|")) {
      showToast(`Select the local ROM or ISO file for "${game.title}" to play.`, "info", 3600);
      const romPath = await pickRomFile();
      if (!romPath) return;
      try {
        const updatedGame = await linkGameRom({ gameId: game.id, romPath });
        state.library = await invokeCommand("get_local_library");
        invalidateSidebarIcons([game.id]);
        renderSidebarGameList();
        renderLibraryGrid();
        updateSidebarTelemetry();
        if (state.selectedDetailId === game.id) {
          openGameDetailView(game.id);
        }
        await launchGameDirectly(updatedGame || { ...game, exePath: romPath });
      } catch (linkErr) {
        showToast(String(linkErr), "error");
      }
      return;
    }
    showToast(msg, "error");
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
    const libPromise = invokeCommand("get_local_library").then((lib) => {
      if (Array.isArray(lib)) {
        state.library = lib;
        renderSidebarGameList();
        renderLibraryGrid();
      }
      return lib;
    });
    const jobsPromise = invokeCommand("get_download_jobs", { backupJobs: null })
      .then((jobs) => {
        if (Array.isArray(jobs)) {
          state.downloadJobs = jobs;
          renderDownloadJobs();
        }
        return jobs;
      })
      .catch(() => null);
    const telemetryPromise = invokeCommand("get_system_telemetry").catch(() => null);
    const sessionsPromise = getActiveGameSessions().catch(() => []);

    await libPromise;
    const [, telemetry, sessions] = await Promise.all([
      jobsPromise,
      telemetryPromise,
      sessionsPromise
    ]);
    state.activeSessions = Array.isArray(sessions) ? sessions : [];
    syncActiveSessionsUi();
    updateSidebarTelemetry(telemetry);
    checkDiskPressureAutopilotBanner(false).catch(() => {});
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
  } catch (err) {
    showToast(String(err), "error");
  }
}

function updateControllerGlyphLegend(controllerKind = state.controllerKind) {
  const kind = String(controllerKind || "none").toLowerCase();
  state.controllerKind = kind;
  const badge = el("couch-controller-glyph-badge");
  const radialPill = el("radial-controller-pill");
  const isPs =
    kind.includes("dualsense") ||
    kind.includes("dualshock") ||
    kind.includes("ps") ||
    kind.includes("sony");
  const isSwitch =
    kind.includes("switch") || kind.includes("nintendo") || kind.includes("8bitdo");
  const isFpv =
    kind.includes("edgetx") ||
    kind.includes("opentx") ||
    kind.includes("radiomaster") ||
    kind.includes("pocket") ||
    kind.includes("fpv");
  const isNone = kind === "none";

  const glyphs = isFpv
    ? { label: "EdgeTX FPV", a: "SA", b: "SB", x: "SC", lb: "L-", rb: "R+" }
    : isPs
      ? { label: "DualSense", a: "Cross", b: "Circle", x: "Square", lb: "L1", rb: "R1" }
      : isSwitch
        ? { label: "Switch / 8BitDo", a: "B", b: "A", x: "Y", lb: "L", rb: "R" }
        : isNone
          ? { label: "No Controller", a: "A", b: "B", x: "X", lb: "LB", rb: "RB" }
          : kind === "hid"
            ? { label: "USB HID Pad", a: "Btn 1", b: "Btn 2", x: "Btn 3", lb: "L1", rb: "R1" }
            : { label: "Xbox XInput", a: "A", b: "B", x: "X", lb: "LB", rb: "RB" };

  if (badge) badge.textContent = glyphs.label;
  if (radialPill) radialPill.textContent = `${glyphs.label.toUpperCase()} · RADIAL`;
  if (el("glyph-btn-a")) el("glyph-btn-a").textContent = glyphs.a;
  if (el("glyph-btn-b")) el("glyph-btn-b").textContent = glyphs.b;
  if (el("glyph-btn-x")) el("glyph-btn-x").textContent = glyphs.x;
  if (el("glyph-btn-lb")) el("glyph-btn-lb").textContent = glyphs.lb;
  if (el("glyph-btn-rb")) el("glyph-btn-rb").textContent = glyphs.rb;
}

function toggleCouchMode(forceEnabled = null, persist = true, controllerKind = null) {
  const next = typeof forceEnabled === "boolean" ? forceEnabled : !state.couchMode;
  applyCouchMode(next, persist, controllerKind);
  return state.couchMode;
}

function applyCouchMode(enabled, persist = false, controllerKind = null) {
  state.couchMode = Boolean(enabled);
  if (controllerKind) {
    state.controllerKind = String(controllerKind);
  }
  document.body?.classList?.toggle("couch-mode", state.couchMode);
  const settingChk = el("setting-couch-mode");
  if (settingChk) {
    settingChk.checked = state.couchMode;
  }
  const hintBar = el("duke-deck-hint-bar");
  if (hintBar) {
    hintBar.classList.toggle("hidden", !state.couchMode);
  }
  if (!state.couchMode) {
    closeRadialHotbar();
    closeVirtualKeyboard(false);
    el("couch-calibrator-popover")?.classList.add("hidden");
    clearControllerFocus({
      disconnect: !state.controllerConnected,
      blurNonText: true,
      resetRepeats: true
    });
  }
  const titleBtn = el("btn-titlebar-couch");
  if (titleBtn) {
    titleBtn.classList.toggle("active", state.couchMode);
  }
  updateControllerGlyphLegend(state.couchMode || state.controllerConnected ? state.controllerKind : "none");
  state.gamepadPollingRateFn?.(state.couchMode || state.radialOpen);

  try {
    if (state.couchMode && !document.fullscreenElement && document.documentElement?.requestFullscreen) {
      document.documentElement.requestFullscreen().catch(() => {});
    } else if (!state.couchMode && document.fullscreenElement && document.exitFullscreen) {
      document.exitFullscreen().catch(() => {});
    }
  } catch {
    // Fullscreen optional depending on WebView2 state
  }

  if (persist) {
    setCouchModeState(state.couchMode, state.controllerKind, state.radialOpen).catch(() => {});
    updateLauncherSettings({ couchMode: state.couchMode }).catch((err) => {
      showToast(String(err), "error");
    });
  }
}

function syncFseShellUi(fse) {
  if (!fse) return;
  const chk = el("setting-fse-gaming-home");
  if (chk) chk.checked = Boolean(fse.gamingHomeEnabled);
  const pill = el("setting-fse-bloat-pill");
  if (pill) {
    const cnt = Number(fse.suspendedCount || 0);
    pill.textContent = fse.gamingHomeEnabled
      ? cnt > 0
        ? `FSE Active · ${cnt} Bloat Paused (${Number(fse.totalFreedMb || 0).toFixed(0)} MB)`
        : "FSE Gaming Home Active"
      : "Desktop Mode";
    pill.className = `pill mono-num ${fse.gamingHomeEnabled ? "emerald" : ""}`;
  }
  const desc = el("setting-fse-status-desc");
  if (desc && fse.statusSummary) {
    desc.textContent = fse.statusSummary;
  }
}

function renderWasmPluginsList(plugins) {
  const listEl = el("settings-wasm-plugins-list");
  const countPill = el("wasm-plugins-count-pill");
  const list = Array.isArray(plugins) ? plugins : [];
  if (countPill) {
    countPill.textContent = `${list.length} Sandboxed Plugin${list.length === 1 ? "" : "s"}`;
  }
  if (!listEl) return;
  if (list.length === 0) {
    listEl.innerHTML = `<div class="panel-sub">No community .wasm/.wat plugins installed yet. Click Install Demo Metadata Provider to inspect zero-trust capability sandboxing.</div>`;
    return;
  }

  const hookOptions = ["metadata_lookup", "rom_database", "storefront_feed"];
  listEl.innerHTML = list
    .map((p) => {
      const grants = p.grantedCapabilities || {};
      const domainsVal = Array.isArray(grants.allowedDomains) ? grants.allowedDomains.join(", ") : "";
      const grantedHooks = new Set(Array.isArray(grants.allowedHooks) ? grants.allowedHooks : []);
      const hookChecks = hookOptions
        .map(
          (hk) => `
          <label class="wave2-check-chip">
            <input type="checkbox" data-wasm-hook="${escapeHtml(hk)}" ${grantedHooks.has(hk) ? "checked" : ""} />
            <span class="mono-num">${escapeHtml(hk)}</span>
          </label>`
        )
        .join("");
      const shortSha = (p.sha256 || "").slice(0, 12);
      const errHtml = p.lastError
        ? `<div class="panel-sub mono-num">Last Capability Notice: ${escapeHtml(p.lastError)}</div>`
        : "";
      return `
        <div class="wave2-subpanel" data-wasm-plugin-card="${escapeHtml(p.id)}">
          <div class="detail-field-head">
            <strong>${escapeHtml(p.name || p.id)}</strong>
            <span class="pill mono-num ${p.enabled ? "emerald" : ""}">${p.enabled ? "Enabled" : "Disabled"} · SHA256:${escapeHtml(shortSha)}</span>
          </div>
          <p class="panel-sub">${escapeHtml(p.description || "")}</p>
          ${errHtml}
          <div class="wave2-form-grid">
            <label>
              <span>Explicitly Approved HTTPS Domains (No Wildcards)</span>
              <input type="text" class="input-control mono-num" data-wasm-domains value="${escapeHtml(domainsVal)}" placeholder="api.rawg.io, metadata.example.org" />
            </label>
            <div class="wave2-inline-checks">
              <label class="wave2-check-chip">
                <input type="checkbox" data-wasm-enabled ${p.enabled ? "checked" : ""} />
                <span>Plugin Enabled</span>
              </label>
              <label class="wave2-check-chip">
                <input type="checkbox" data-wasm-titles ${grants.allowReadOnlyLibraryTitles ? "checked" : ""} />
                <span>Read-Only Game Titles</span>
              </label>
              ${hookChecks}
            </div>
          </div>
          <div class="detail-save-actions">
            <button type="button" class="btn btn-primary btn-xs" data-wasm-action="save" data-wasm-id="${escapeHtml(p.id)}">Save Capabilities</button>
            <button type="button" class="btn btn-secondary btn-xs" data-wasm-action="run" data-wasm-id="${escapeHtml(p.id)}">
              <svg class="icon" aria-hidden="true"><use href="#icon-play"/></svg>
              <span>Run Hook</span>
            </button>
            <button type="button" class="btn btn-secondary btn-xs" data-wasm-action="remove" data-wasm-id="${escapeHtml(p.id)}">
              <svg class="icon" aria-hidden="true"><use href="#icon-trash-2"/></svg>
              <span>Remove</span>
            </button>
          </div>
        </div>
      `;
    })
    .join("");
}

async function refreshWasmPluginsUi() {
  try {
    const plugins = await listWasmPlugins();
    renderWasmPluginsList(plugins);
  } catch {
    renderWasmPluginsList([]);
  }
}

const PILOT_EXAMPLES = [
  "Shrink my biggest games over 50 GB",
  "Set up Cemu and import my Wii U folder",
  "Freeze ARC Raiders and launch Balatro",
  "Back up Elden Ring saves and apply my modpack",
  "Export portable capsule for Project Zomboid"
];
let pilotExampleIdx = 0;

function pilotIconName(raw) {
  switch (String(raw || "")) {
    case "icon-shrink":
      return "icon-layers";
    case "icon-folder":
      return "icon-folder-open";
    case "icon-gamepad":
      return "icon-gamepad-2";
    case "icon-settings":
      return "icon-sliders";
    case "icon-archive":
      return "icon-archive";
    case "icon-download":
      return "icon-download";
    case "icon-pause":
      return "icon-pause";
    case "icon-search":
      return "icon-search";
    default:
      return "icon-play";
  }
}

function closeDukePilotPopover() {
  el("duke-pilot-popover")?.classList.add("hidden");
  state.pilotEverythingIdx = -1;
  clearControllerFocus({ disconnect: !state.controllerConnected, blurNonText: false, resetRepeats: true });
}

async function hydrateEverythingSaveCache() {
  try {
    const targets = [];
    if (state.selectedDetailId) {
      const sel = state.library.find((g) => g.id === state.selectedDetailId);
      if (sel) targets.push(sel);
    }
    state.library
      .filter((g) => g.isInstalled !== false && !targets.some((t) => t.id === g.id))
      .sort((a, b) => (b.lastPlayedTs || getTotalPlaytimeSecs(b)) - (a.lastPlayedTs || getTotalPlaytimeSecs(a)))
      .slice(0, 3)
      .forEach((g) => targets.push(g));

    const results = [];
    for (const g of targets.slice(0, 3)) {
      const [backups, snaps] = await Promise.all([
        listSaveBackups(g.id).catch(() => []),
        listGameSnapshots(g.id).catch(() => [])
      ]);
      for (const b of (Array.isArray(backups) ? backups : []).slice(0, 2)) {
        results.push({
          kind: "save_zip",
          gameId: g.id,
          gameTitle: g.title,
          label: `${g.title}: Save Backup`,
          detail: `${b.fileName || "Backup ZIP"} · ${b.createdFormatted || b.sizeFormatted || ""}`,
          badge: "Save Backup",
          icon: "icon-archive"
        });
      }
      for (const s of (Array.isArray(snaps) ? snaps : []).slice(0, 2)) {
        results.push({
          kind: "cas_snapshot",
          gameId: g.id,
          gameTitle: g.title,
          label: `${g.title}: ${s.label || "Restore Point"}`,
          detail: `CoW Snapshot · ${s.createdFormatted || ""}`,
          badge: "Restore Point",
          icon: "icon-rotate-ccw"
        });
      }
    }
    state.everythingSaveCache = results;
    if (!el("duke-pilot-popover")?.classList.contains("hidden")) {
      renderEverythingPalette(el("search-input")?.value || "");
    }
  } catch {
    // Keep existing cache cleanly
  }
}

function getEverythingSettingsToggles() {
  return [
    {
      key: "couch",
      label: `Toggle Duke Deck Couch Mode (${state.couchMode ? "On" : "Off"})`,
      detail: "Settings Toggle · 10-foot controller full-screen UI",
      keywords: "couch deck fullscreen controller mode toggle setting",
      run: () => applyCouchMode(!state.couchMode, true)
    },
    {
      key: "doh",
      label: `Toggle Encrypted DNS-over-HTTPS (${state.dohEnabled ? "Active" : "Bypassed"})`,
      detail: "Settings Toggle · Cloudflare 1.1.1.1 / Quad9 9.9.9.9 DoH",
      keywords: "doh dns privacy cloudflare quad9 encrypted toggle setting",
      run: async () => {
        const updated = await updateLauncherSettings({ dohEnabled: !state.dohEnabled });
        syncSettingsUiFromBackend(updated);
        showToast(`DNS-over-HTTPS ${updated.dohEnabled ? "enabled" : "bypassed"}.`, "info");
      }
    },
    {
      key: "telemetry",
      label: `Toggle Anti-Telemetry Injection (${state.blockGameTelemetry ? "Blocked" : "Allowed"})`,
      detail: "Settings Toggle · Inject DO_NOT_TRACK=1 & clear SENTRY_DSN",
      keywords: "telemetry block privacy sentry unity unreal toggle setting",
      run: async () => {
        const updated = await updateLauncherSettings({ blockGameTelemetry: !state.blockGameTelemetry });
        syncSettingsUiFromBackend(updated);
        showToast(`Game anti-telemetry ${updated.blockGameTelemetry ? "enabled" : "disabled"}.`, "info");
      }
    },
    {
      key: "prefetch",
      label: `Toggle NVMe DirectStorage Prefetch (${state.nvmePrefetchEnabled ? "On" : "Off"})`,
      detail: "Settings Toggle · Warm asset blocks before launch",
      keywords: "nvme prefetch ssd directstorage speed toggle setting",
      run: async () => {
        const updated = await updateLauncherSettings({ nvmePrefetchEnabled: !state.nvmePrefetchEnabled });
        syncSettingsUiFromBackend(updated);
        showToast(`NVMe Prefetch ${updated.nvmePrefetchEnabled ? "enabled" : "disabled"}.`, "info");
      }
    },
    {
      key: "freeze",
      label: `Toggle Auto-Freeze Minimized Games (${state.autoFreezeMinimized ? "On" : "Off"})`,
      detail: "Settings Toggle · Suspend single-player processes on minimize",
      keywords: "freeze suspend minimize power battery toggle setting",
      run: async () => {
        const updated = await updateLauncherSettings({ autoFreezeMinimized: !state.autoFreezeMinimized });
        syncSettingsUiFromBackend(updated);
        showToast(`Auto-Freeze Minimized ${updated.autoFreezeMinimized ? "enabled" : "disabled"}.`, "info");
      }
    },
    {
      key: "compact",
      label: `Toggle Compact Library Cards (${state.compactLibraryCards ? "On" : "Off"})`,
      detail: "Settings Toggle · Dense card grid layout",
      keywords: "compact cards grid layout ui toggle setting",
      run: async () => {
        const updated = await updateLauncherSettings({ compactLibraryCards: !state.compactLibraryCards });
        syncSettingsUiFromBackend(updated);
        showToast(`Compact Cards ${updated.compactLibraryCards ? "enabled" : "disabled"}.`, "info");
      }
    },
    {
      key: "sandbox",
      label: `Toggle Portable Sandbox Default (${state.portableSandboxDefault ? "On" : "Off"})`,
      detail: "Settings Toggle · Isolate saves in .duke-profile/ for new games",
      keywords: "sandbox profile portable isolated save toggle setting",
      run: async () => {
        const updated = await updateLauncherSettings({ portableSandboxDefault: !state.portableSandboxDefault });
        syncSettingsUiFromBackend(updated);
        showToast(`Portable Sandbox Default ${updated.portableSandboxDefault ? "enabled" : "disabled"}.`, "info");
      }
    }
  ];
}

function buildEverythingPaletteItems(queryText, plan = null) {
  const q = String(queryText || "").trim().toLowerCase();
  const terms = q.split(/\s+/).filter(Boolean);
  const matches = (hay) => {
    if (terms.length === 0) return true;
    const lower = String(hay || "").toLowerCase();
    return terms.every((t) => lower.includes(t));
  };
  const items = [];
  const primaryGame =
    state.library.find((g) => g.id === state.selectedDetailId) ||
    (q
      ? state.library.find((g) => g.title.toLowerCase().includes(q)) ||
        state.library.find((g) =>
          q.split(/\s+/).some((tok) => tok.length >= 3 && g.title.toLowerCase().includes(tok))
        )
      : state.library[0]) ||
    null;

  // 1. Installed Games
  state.library
    .filter((g) => matches(`${g.title} ${g.genre || ""} ${g.source || ""}`))
    .slice(0, q ? 5 : 3)
    .forEach((g) => {
      items.push({
        id: `game:${g.id}`,
        icon: "icon-gamepad-2",
        badge: g.isInstalled === false ? "Catalog Link" : "Installed Game",
        label: g.title,
        detail: `${g.sizeFormatted || "PC"} · Click/Enter to open Detail View`,
        type: "game",
        gameId: g.id
      });
    });

  // 2. Quick Resume Slots
  const qrSlots = Array.isArray(state.quickResume?.slots) ? state.quickResume.slots : [];
  qrSlots
    .filter((s) => matches(`${s.title} quick resume slot ${s.isSuspended ? "suspended resume" : "active freeze"}`))
    .slice(0, 3)
    .forEach((s) => {
      items.push({
        id: `qr:${s.gameId}`,
        icon: "icon-pause",
        badge: s.isSuspended ? "Suspended Slot" : "Active Slot",
        label: `${s.isSuspended ? "Resume" : "Freeze"} ${s.title}`,
        detail: `Quick Resume · ${Number(s.workingSetMb || 0).toFixed(0)} MB RAM`,
        type: "qr_slot",
        gameId: s.gameId
      });
    });

  // 3. Online Friends
  const friendPools = [
    ...(Array.isArray(state.friendsSnapshot?.inGame) ? state.friendsSnapshot.inGame : []),
    ...(Array.isArray(state.friendsSnapshot?.online) ? state.friendsSnapshot.online : [])
  ];
  friendPools
    .filter((f) => matches(`${f.displayName || ""} ${f.gameTitle || ""} ${f.platform || ""} friend online`))
    .slice(0, 3)
    .forEach((f) => {
      items.push({
        id: `friend:${f.platform || "net"}:${f.displayName || "peer"}`,
        icon: "icon-users",
        badge: `${(f.platform || "Friend").toUpperCase()} Online`,
        label: f.displayName || "Friend",
        detail: f.gameTitle ? `Playing ${f.gameTitle}` : f.presenceText || "Online — Open Friends Drawer",
        type: "friend"
      });
    });

  // 4. Save Restore Points
  (Array.isArray(state.everythingSaveCache) ? state.everythingSaveCache : [])
    .filter((s) => matches(`${s.label} ${s.detail} save restore point snapshot backup`))
    .slice(0, 3)
    .forEach((s, idx) => {
      items.push({
        id: `save:${s.gameId}:${idx}`,
        icon: s.icon || "icon-archive",
        badge: s.badge || "Restore Point",
        label: s.label,
        detail: s.detail,
        type: "restore_point",
        gameId: s.gameId,
        saveKind: s.kind
      });
    });

  // 5. 1-Click Security, Fix & Storage Actions
  const gName = primaryGame?.title || "Selected Game";
  const quickActions = [
    {
      id: "act:scan-malware",
      icon: "icon-shield-check",
      badge: "Security Shield v2",
      label: primaryGame ? `Scan for Malware (${gName})` : "Scan Local Binary for Malware",
      detail: "Runs PE32/PE32+ Security Shield + WinVerifyTrust + heuristic scan",
      keywords: "scan malware antivirus security shield pe inspect",
      type: "action_scan",
      game: primaryGame
    },
    {
      id: "act:block-internet",
      icon: "icon-wifi-off",
      badge: "Zero-Trust Jail",
      label: `Block Internet for ${gName}`,
      detail: "Enables 1-Click Offline Network Lock (WFP / AppContainer)",
      keywords: "block internet offline firewall jail lock network",
      type: "action_offline",
      game: primaryGame
    },
    {
      id: "act:find-fixes",
      icon: "icon-wrench",
      badge: "Fix Engine",
      label: `Find Fixes for ${gName}`,
      detail: "Runs preflight crash diagnosis + fetches PCGamingWiki workarounds",
      keywords: "find fixes crash repair pcgamingwiki preflight troubleshoot",
      type: "action_fixes",
      game: primaryGame
    },
    {
      id: "act:verify-bios",
      icon: "icon-cpu",
      badge: "BIOS Vault",
      label: "Verify Emulator BIOS & Firmware Hashes",
      detail: "Checks DuckStation, PCSX2, melonDS, Azahar & RetroArch hashes",
      keywords: "verify bios firmware md5 sha1 emulator vault",
      type: "action_bios"
    },
    {
      id: "act:shrink",
      icon: "icon-zap",
      badge: "Deep Shrink",
      label: primaryGame ? `Shrink ${gName} (WOF LZX)` : "Disk-Space Pressure Autopilot (Shrink)",
      detail: "Ranks or compresses large untouched games with NTFS WOF LZX",
      keywords: "shrink compress wof lzx deep disk pressure space",
      type: "action_shrink",
      game: primaryGame
    },
    {
      id: "act:backup",
      icon: "icon-archive",
      badge: "Save Backup",
      label: `Backup Saves for ${gName}`,
      detail: "Creates rolling pure-Rust ZIP save snapshot + Ludusavi paths",
      keywords: "backup save ludusavi zip snapshot",
      type: "action_backup",
      game: primaryGame
    }
  ];
  quickActions
    .filter((a) => matches(`${a.label} ${a.detail} ${a.keywords}`))
    .slice(0, q ? 4 : 4)
    .forEach((a) => items.push(a));

  // 6. Settings Toggles
  getEverythingSettingsToggles()
    .filter((t) => matches(`${t.label} ${t.detail} ${t.keywords}`))
    .slice(0, q ? 3 : 2)
    .forEach((t) => {
      items.push({
        id: `toggle:${t.key}`,
        icon: "icon-sliders",
        badge: "Setting Toggle",
        label: t.label,
        detail: t.detail,
        type: "setting_toggle",
        run: t.run
      });
    });

  // 7. 80k Catalog (DKCAT003 matches from Duke Pilot plan)
  const sem = Array.isArray(plan?.semanticMatches) ? plan.semanticMatches : [];
  sem.slice(0, 4).forEach((cat) => {
    items.push({
      id: `cat:${cat.appId || cat.title}`,
      icon: "icon-compass",
      badge: "80k Catalog",
      label: cat.title || "Catalog Title",
      detail: (Array.isArray(cat.genres) && cat.genres.length ? cat.genres.slice(0, 3).join(", ") : cat.platformName || "PC") + " · Open in Search Games",
      type: "catalog",
      query: cat.title || ""
    });
  });

  return items;
}

async function executeEverythingItem(item) {
  if (!item) return;
  closeDukePilotPopover();
  try {
    if (item.type === "game") {
      await openGameDetail(item.gameId);
    } else if (item.type === "catalog") {
      switchTab("catalog");
      if (el("input-catalog-search")) {
        el("input-catalog-search").value = item.query || "";
        handleCatalogSearchInput(item.query || "");
      }
    } else if (item.type === "friend") {
      toggleFriendsDrawer(true);
    } else if (item.type === "qr_slot") {
      const rep = await quickSwitchGameSlot(item.gameId);
      showToast(rep?.summary || "Switched Quick Resume slot.", "success", 2600);
      await refreshQuickResumeSlots();
    } else if (item.type === "restore_point") {
      const g = state.library.find((x) => x.id === item.gameId);
      if (!g) return;
      if (item.saveKind === "cas_snapshot") {
        await openSnapshotsModal(g);
      } else {
        await openRestoreSaveModal(g);
      }
    } else if (item.type === "setting_toggle" && typeof item.run === "function") {
      await item.run();
    } else if (item.type === "action_scan") {
      let target = item.game?.exePath || "";
      if (!target) {
        target = (await pickExecutableFile()) || "";
      }
      if (!target) return;
      const rep = await invokeCommand("scan_file_security", { path: target });
      const verdict = rep?.shieldBadge || rep?.verdict || "Scanned";
      showToast(`Security Shield: ${verdict} (${rep?.suspiciousCount || 0} heuristic flag(s)).`, verdict === "Blocked" ? "error" : "success", 3600);
    } else if (item.type === "action_offline") {
      if (!item.game) {
        showToast("Select or install a game first to lock offline.", "info");
        return;
      }
      const st = await setGameJailPolicy(item.game.id, Boolean(state.detailJailStatus?.jailEnabled), true);
      showToast(st?.summary || `Offline Network Lock enabled for ${item.game.title}.`, "success", 3400);
      if (state.selectedDetailId === item.game.id) await openGameDetail(item.game.id);
    } else if (item.type === "action_fixes") {
      if (!item.game) {
        showToast("Select or install a game first to diagnose fixes.", "info");
        return;
      }
      await openGameDetail(item.game.id);
      const diag = await analyzeGameCrashOrPreflight(item.game.id, null, null);
      showToast(diag?.plainEnglishCause || "Diagnostics refreshed on Detail View.", "info", 3800);
    } else if (item.type === "action_bios") {
      switchTab("settings");
      await refreshBiosVaultStatus(false);
    } else if (item.type === "action_shrink") {
      await runDiskPressureAutopilotInteractive();
    } else if (item.type === "action_backup") {
      if (!item.game) {
        showToast("Select a game first to back up saves.", "info");
        return;
      }
      const outPath = await invokeCommand("backup_game_saves", {
        gameId: item.game.id,
        title: item.game.title,
        installDir: item.game.installDir || "",
        exePath: item.game.exePath || ""
      });
      showToast(`Saved backup to ${outPath}`, "success", 3200);
    }
  } catch (err) {
    showToast(String(err), "error");
  }
}

function renderEverythingPalette(queryText, plan = null) {
  const host = el("pilot-everything-list");
  if (!host) return;
  const items = buildEverythingPaletteItems(queryText, plan);
  state.pilotEverythingItems = items;
  if (items.length === 0) {
    state.pilotEverythingIdx = -1;
    host.innerHTML = `<div class="panel-sub">No matching games, friends, saves, or actions.</div>`;
    return;
  }
  if (state.pilotEverythingIdx >= items.length) {
    state.pilotEverythingIdx = items.length - 1;
  }
  host.innerHTML = items
    .map(
      (item, idx) => `
      <button type="button" class="pilot-everything-row${idx === state.pilotEverythingIdx ? " is-selected" : ""}" data-everything-idx="${idx}">
        <div class="pilot-step-body">
          <div class="pilot-step-title">
            <svg class="icon" aria-hidden="true"><use href="#${escapeHtml(item.icon || "icon-zap")}"/></svg>
            <span>${escapeHtml(item.label)}</span>
          </div>
          <div class="pilot-step-detail">${escapeHtml(item.detail || "")}</div>
        </div>
        <span class="pill mono-num">${escapeHtml(item.badge || "")}</span>
      </button>
    `
    )
    .join("");
}

function movePilotEverythingSelection(delta) {
  const len = Array.isArray(state.pilotEverythingItems) ? state.pilotEverythingItems.length : 0;
  if (len === 0) return;
  if (state.pilotEverythingIdx < 0) {
    state.pilotEverythingIdx = delta > 0 ? 0 : len - 1;
  } else {
    state.pilotEverythingIdx = (state.pilotEverythingIdx + delta + len) % len;
  }
  const host = el("pilot-everything-list");
  if (!host) return;
  host.querySelectorAll(".pilot-everything-row").forEach((row, idx) => {
    const selected = idx === state.pilotEverythingIdx;
    row.classList.toggle("is-selected", selected);
    if (selected) row.scrollIntoView({ block: "nearest" });
  });
}

function openEverythingCommandPalette() {
  const inp = el("search-input");
  if (!inp) return;
  inp.focus();
  inp.select();
  el("duke-pilot-popover")?.classList.remove("hidden");
  renderEverythingPalette(inp.value, state.pilotPlan);
  hydrateEverythingSaveCache();
  if (inp.value.trim()) scheduleDukePilotPreview(inp.value);
}

async function checkDiskPressureAutopilotBanner(force = false) {
  const banner = el("disk-pressure-banner");
  const sumEl = el("disk-pressure-summary");
  if (!banner || !sumEl) return null;
  if (state.diskPressureDismissed && !force) return null;
  try {
    const rep = await evaluateDiskPressureAutopilot(state.diskPressureThresholdGb || 50, null, force);
    if (!rep?.triggered && !force) {
      banner.classList.add("hidden");
      return rep;
    }
    if (rep?.candidates?.length > 0) {
      const top = rep.candidates
        .slice(0, 3)
        .map((c) => `${c.title} (${c.totalEstimatedReclaimFormatted})`)
        .join(", ");
      sumEl.textContent = `${rep.summary} Top: ${top}`;
      banner.classList.remove("hidden");
    } else if (force) {
      sumEl.textContent = rep?.summary || "Drive free space healthy.";
      banner.classList.remove("hidden");
    }
    return rep;
  } catch {
    return null;
  }
}

async function runDiskPressureAutopilotInteractive() {
  try {
    const preview = await evaluateDiskPressureAutopilot(state.diskPressureThresholdGb || 50, null, true);
    const list = Array.isArray(preview?.candidates) ? preview.candidates : [];
    if (list.length === 0) {
      showToast(preview?.summary || "No untouched 60+ day games need compression right now.", "info", 3200);
      return;
    }
    const lines = list
      .slice(0, 8)
      .map((c) => `• ${c.title} (${c.daysUntouched}d untouched) — ~${c.totalEstimatedReclaimFormatted}`)
      .join("\n");
    const ok = window.confirm(
      `Disk-Space Pressure Autopilot (${preview.driveRoot}: ${Number(preview.freeGb || 0).toFixed(1)} GB free)\n\n${lines}\n\nApply WOF LZX compression, trim foreign language packs, and run .duke-cas GC?`
    );
    if (!ok) return;
    const btn = el("btn-run-disk-pressure");
    if (btn) btn.disabled = true;
    try {
      const ids = list.map((c) => c.gameId);
      const res = await executeDiskPressureAutopilot(ids, ["english"]);
      showToast(res?.summary || "Disk space reclaimed.", "success", 4800);
      el("disk-pressure-banner")?.classList.add("hidden");
      state.diskPressureDismissed = true;
      await refreshAllData();
    } finally {
      if (btn) btn.disabled = false;
    }
  } catch (err) {
    showToast(String(err), "error");
  }
}

function renderDukePilotPlan(plan) {
  state.pilotPlan = plan || null;
  const pop = el("duke-pilot-popover");
  if (!pop || !plan) return;
  pop.classList.remove("hidden");

  renderEverythingPalette(el("search-input")?.value || "", plan);

  if (el("pilot-mode-badge")) {
    el("pilot-mode-badge").textContent =
      plan.mode === "semantic_search"
        ? "DKCAT003 Semantic"
        : plan.aiRefined
          ? "Local ML Refined"
          : `${(plan.steps || []).length}-Step Plan`;
  }
  if (el("pilot-headline")) {
    el("pilot-headline").textContent = plan.headline || "Duke Pilot Everything Command Palette";
  }
  if (el("pilot-latency-badge")) {
    const ms = (Number(plan.elapsedUs || 0) / 1000).toFixed(2);
    el("pilot-latency-badge").textContent = `Parsed in ${ms} ms`;
  }

  const stepsEl = el("pilot-steps-list");
  if (stepsEl) {
    const steps = Array.isArray(plan.steps) ? plan.steps : [];
    stepsEl.innerHTML = steps
      .map(
        (s, idx) => `
      <div class="pilot-step-row">
        <span class="pilot-step-num mono-num">${idx + 1}.</span>
        <svg class="icon" aria-hidden="true"><use href="#${escapeHtml(pilotIconName(s.actionIcon))}"/></svg>
        <div class="pilot-step-body">
          <div class="pilot-step-title">${escapeHtml(s.label || "")}</div>
          <div class="pilot-step-detail">${escapeHtml(s.detail || "")}</div>
        </div>
      </div>`
      )
      .join("");
  }

  const semEl = el("pilot-semantic-list");
  if (semEl) {
    const matches = Array.isArray(plan.semanticMatches) ? plan.semanticMatches : [];
    semEl.classList.toggle("hidden", matches.length === 0);
    semEl.innerHTML = matches
      .map(
        (item) => `
      <div class="pilot-semantic-row" data-catalog-query="${escapeHtml(item.title || "")}">
        <div class="pilot-step-body">
          <div class="pilot-step-title">${escapeHtml(item.title || "")}</div>
          <div class="pilot-step-detail">${escapeHtml((item.genres || []).slice(0, 3).join(", ") || item.platformName || "PC")}</div>
        </div>
      </div>`
      )
      .join("");
  }
}

function scheduleDukePilotPreview(queryText) {
  if (state.pilotDebounceId) {
    clearTimeout(state.pilotDebounceId);
    state.pilotDebounceId = null;
  }
  const q = String(queryText || "").trim();
  renderEverythingPalette(q, state.pilotPlan);
  if (!q) {
    state.pilotPlan = null;
    if (el("pilot-steps-list")) el("pilot-steps-list").innerHTML = "";
    if (el("pilot-semantic-list")) el("pilot-semantic-list").classList.add("hidden");
    return;
  }
  state.pilotDebounceId = setTimeout(async () => {
    try {
      const useAi = Boolean(el("chk-pilot-ai")?.checked);
      const plan = await planDukePilotCommand(q, useAi);
      renderDukePilotPlan(plan);
    } catch {
      // Ignore transient parse errors while typing
    }
  }, 85);
}

async function runActiveDukePilotPlan() {
  const inpVal = el("search-input")?.value?.trim() || "";
  if (!state.pilotPlan && inpVal) {
    state.pilotPlan = await planDukePilotCommand(inpVal, Boolean(el("chk-pilot-ai")?.checked));
  }
  if (!state.pilotPlan || !Array.isArray(state.pilotPlan.steps) || state.pilotPlan.steps.length === 0) {
    return;
  }
  let explicitImportPath = null;
  if (state.pilotPlan.requiresFolderPrompt) {
    explicitImportPath = await pickGameFolder();
  }
  const btn = el("btn-pilot-run");
  if (btn) btn.disabled = true;
  try {
    const report = await executeDukePilotPlan(state.pilotPlan, explicitImportPath);
    if (report?.needsFolderPrompt && !explicitImportPath) {
      const picked = await pickGameFolder();
      if (picked) {
        const rep2 = await executeDukePilotPlan(state.pilotPlan, picked);
        showToast(rep2?.summary || "Folder imported.", rep2?.ok ? "success" : "info", 3800);
      }
    } else if (report?.followUpAction === "toggle_couch_mode") {
      applyCouchMode(!state.couchMode, true);
    } else if (report?.followUpAction === "open_play_tonight") {
      const mins = Number(report?.followUpQuery || 60);
      if (el("select-play-tonight-mins")) {
        el("select-play-tonight-mins").value = String([30, 60, 120, 240].reduce((a, b) => (Math.abs(b - mins) < Math.abs(a - mins) ? b : a)));
      }
      await activatePlayTonightFilter();
    } else if (report?.followUpAction === "open_semantic_catalog") {
      switchTab("catalog");
      if (el("input-catalog-search")) {
        el("input-catalog-search").value = report?.followUpQuery || inpVal;
        handleCatalogSearchInput();
      }
    } else {
      showToast(report?.summary || "Duke Pilot action completed.", report?.ok ? "success" : "error", 4200);
      state.library = await invokeCommand("get_local_library").catch(() => state.library);
      renderLibraryGrid();
      renderSidebarGameList();
    }
    closeDukePilotPopover();
  } catch (err) {
    showToast(String(err), "error");
  } finally {
    if (btn) btn.disabled = false;
  }
}

async function refreshPlayTonightFilter() {
  const mins = Number(el("select-play-tonight-mins")?.value || 60);
  try {
    const list = await getPlayTonightRecommendations(mins, 36);
    const items = Array.isArray(list) ? list : [];
    state.playTonightIds = items.map((x) => x.gameId);
    state.playTonightMap = new Map(items.map((x) => [x.gameId, x]));
    renderLibraryGrid();
    if (items.length > 0) {
      showToast(`Play Tonight ranked ${items.length} installed title(s) for ~${mins} min.`, "info", 2400);
    }
  } catch (err) {
    showToast(String(err), "error");
  }
}

async function activatePlayTonightFilter() {
  switchTab("library", true);
  state.filter = "play_tonight";
  document.querySelectorAll("#filter-pills .filter-pill").forEach((p) =>
    p.classList.toggle("active", p.dataset.filter === "play_tonight")
  );
  el("play-tonight-session-wrap")?.classList.remove("hidden");
  await refreshPlayTonightFilter();
}

function getActiveRadialSlots() {
  if (Array.isArray(state.radialSlots) && state.radialSlots.length > 0) {
    return state.radialSlots;
  }
  return [
    { slotId: 0, label: "Launch / Play", action: "launch_selected" },
    { slotId: 1, label: "Play Tonight", action: "open_play_tonight" },
    { slotId: 2, label: "Freeze / Resume", action: "toggle_suspend" },
    { slotId: 3, label: "Backup Saves", action: "backup_saves" },
    { slotId: 4, label: "Virtual Keyboard", action: "open_vkb" },
    { slotId: 5, label: "Shrink Size", action: "compress_selected" },
    { slotId: 6, label: "Export Capsule", action: "export_capsule" },
    { slotId: 7, label: "Toggle Deck Mode", action: "toggle_couch" }
  ];
}

function renderRadialHotbar(slots = null, selectedSlot = state.radialSelectedSlot) {
  if (Array.isArray(slots) && slots.length > 0) {
    state.radialSlots = slots;
  }
  const activeSlots = getActiveRadialSlots();
  state.radialSelectedSlot = Number(selectedSlot || 0) % Math.max(1, activeSlots.length);
  const ring = el("radial-slots-ring");
  if (!ring) return;
  ring.innerHTML = activeSlots
    .map((s, idx) => {
      const slotNum = s.slot ?? s.slotId ?? idx;
      const actionKey = s.actionId ?? s.action ?? "";
      return `
    <button type="button" class="radial-slot-btn ${idx === state.radialSelectedSlot ? "active" : ""}" data-radial-slot="${slotNum}" data-radial-action="${escapeHtml(actionKey)}" style="--slot-idx:${idx}">
      <span class="radial-slot-title">${escapeHtml(s.label)}</span>
      <span class="radial-slot-key mono-num">Slot ${Number(slotNum) + 1}</span>
    </button>`;
    })
    .join("");
  const active = activeSlots[state.radialSelectedSlot];
  if (el("radial-center-title") && active) {
    el("radial-center-title").textContent = active.label;
  }
}

function openRadialHotbar(slots = null) {
  state.radialOpen = true;
  renderRadialHotbar(slots, state.radialSelectedSlot);
  el("radial-hotbar-overlay")?.classList.remove("hidden");
  state.gamepadPollingRateFn?.(true);
}

function closeRadialHotbar() {
  const wasOpen = Boolean(state.radialOpen) || !el("radial-hotbar-overlay")?.classList.contains("hidden");
  state.radialOpen = false;
  el("radial-hotbar-overlay")?.classList.add("hidden");
  if (wasOpen) {
    clearControllerFocus({ disconnect: !state.controllerConnected, blurNonText: true, resetRepeats: true });
    state.gamepadPollingRateFn?.(state.couchMode);
  }
}

async function handleRadialActionDispatch(action, slotId = null) {
  closeRadialHotbar();
  const activeGame =
    state.library.find((g) => g.id === state.selectedDetailId) || getFilteredSortedGames()[0] || null;
  switch (String(action || "")) {
    case "launch_selected":
      if (activeGame) {
        await launchGameDirectly(activeGame);
      }
      break;
    case "open_play_tonight":
      await activatePlayTonightFilter();
      break;
    case "toggle_suspend":
    case "suspend_resume_game":
      if (activeGame) {
        await suspendOrResumeGame(activeGame.id, "toggle").catch((e) => showToast(String(e), "error"));
      }
      break;
    case "backup_saves":
      if (activeGame) {
        const msg = await invokeCommand("backup_game_saves", {
          gameId: activeGame.id,
          title: activeGame.title,
          installDir: activeGame.installDir || null,
          exePath: activeGame.exePath
        }).catch((e) => {
          showToast(String(e), "error");
          return null;
        });
        if (msg) showToast(`Saved rolling backup archive: ${msg}`, "success");
      }
      break;
    case "open_vkb":
    case "toggle_virtual_keyboard":
      openVirtualKeyboard(el("search-input"));
      break;
    case "open_duke_pilot":
      openEverythingCommandPalette();
      break;
    case "compress_selected":
      if (activeGame) {
        await compressGameFolder(activeGame.id).catch((e) => showToast(String(e), "error"));
      }
      break;
    case "export_capsule":
      if (activeGame) {
        const fmt = el("select-capsule-format")?.value || "folder";
        const rep = await exportGameCapsule(activeGame.id, null, fmt, true).catch((e) => {
          showToast(String(e), "error");
          return null;
        });
        if (rep?.summary) showToast(rep.summary, "success", 4200);
      }
      break;
    case "toggle_couch":
    case "toggle_couch_mode":
      applyCouchMode(!state.couchMode, true);
      break;
    default:
      if (slotId != null || action) {
        await triggerCouchRadialAction(slotId ?? action, action).catch(() => {});
      }
      break;
  }
}

const VKB_ROWS = [
  ..."1234567890",
  ..."qwertyuiop",
  ..."asdfghjkl-",
  ..."zxcvbnm._/"
];

function renderVirtualKeyboardGrid() {
  const grid = el("vkb-grid");
  if (!grid) return;
  grid.innerHTML = VKB_ROWS.map((ch) => {
    const label = state.vkbShift ? ch.toUpperCase() : ch;
    return `<button type="button" class="vkb-key-btn" data-vkb-char="${escapeHtml(label)}">${escapeHtml(label)}</button>`;
  }).join("");
}

function openVirtualKeyboard(targetEl = null) {
  state.vkbTargetInput = targetEl || el("search-input");
  if (el("vkb-preview")) {
    el("vkb-preview").value = state.vkbTargetInput?.value || "";
  }
  renderVirtualKeyboardGrid();
  el("virtual-keyboard-overlay")?.classList.remove("hidden");
}

function closeVirtualKeyboard(applyText = false) {
  const overlay = el("virtual-keyboard-overlay");
  const wasOpen = overlay && !overlay.classList.contains("hidden");
  const preview = el("vkb-preview");
  if (applyText && state.vkbTargetInput && preview) {
    state.vkbTargetInput.value = preview.value;
    state.vkbTargetInput.dispatchEvent(new Event("input", { bubbles: true }));
    state.vkbTargetInput.focus();
  }
  overlay?.classList.add("hidden");
  if (wasOpen && !applyText) {
    clearControllerFocus({ disconnect: !state.controllerConnected, blurNonText: true, resetRepeats: true });
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

async function handleIngestPathsFlow(paths) {
  const cleanPaths = Array.isArray(paths)
    ? paths.map((p) => String(p || "").trim()).filter(Boolean)
    : [];
  if (cleanPaths.length === 0) return null;

  const isViewingDetail =
    state.activeTab === "library" &&
    Boolean(state.selectedDetailId) &&
    !el("game-detail-view")?.classList.contains("hidden");
  const detailGame = isViewingDetail
    ? state.library.find((g) => g.id === state.selectedDetailId)
    : null;

  if (detailGame && cleanPaths.length === 1) {
    const firstPath = cleanPaths[0];
    if (/\.(ips|bps|ups|xdelta|vcdiff)$/i.test(firstPath) && Number(detailGame.platformId || 0) > 0) {
      await applyDroppedOrPickedRomPatch(detailGame, firstPath);
      return { addedCount: 1 };
    }
    if (/\.(zip|7z|rar)$/i.test(firstPath)) {
      await runModDropperInstall(detailGame, firstPath);
      return { addedCount: 0 };
    }
  }

  showToast("Adding dropped game to My Games...", "info", 2400);
  try {
    const res = await ingestDroppedPaths(cleanPaths);
    if (Array.isArray(res?.library)) {
      state.library = res.library;
      invalidateSidebarIcons();
      renderSidebarGameList();
      renderLibraryGrid();
      updateSidebarTelemetry();
    }
    const added = Number(res?.addedCount || 0);
    const titles = Array.isArray(res?.addedTitles) ? res.addedTitles : [];
    const skippedReasons = Array.isArray(res?.skippedReasons) ? res.skippedReasons : [];

    if (added > 0) {
      if (state.activeTab !== "library") {
        switchTab("library", false);
        renderLibraryGrid();
      }
      const label = titles.length > 0 ? titles.join(", ") : `${added} game(s)`;
      showToast(`Added ${label} to My Games — ready to play.`, "success", 3600);
    } else if (skippedReasons.length > 0) {
      showToast(skippedReasons[0], "info", 3800);
    } else {
      showToast("No new game executables or console ROMs found in dropped items.", "info", 3200);
    }
    return res;
  } catch (err) {
    showToast(String(err), "error");
    return null;
  }
}

async function initDragAndDropIngestion() {
  const overlay = el("drop-ingest-overlay");
  const showOverlay = () => overlay?.classList.remove("hidden");
  const hideOverlay = () => overlay?.classList.add("hidden");

  window.addEventListener("dragover", (e) => {
    e.preventDefault();
  });
  window.addEventListener("drop", (e) => {
    e.preventDefault();
    hideOverlay();
  });

  try {
    const webview = window.__TAURI__?.webview?.getCurrentWebview?.();
    if (webview && typeof webview.onDragDropEvent === "function") {
      const recentDropSet = new Set();
      const pendingDropPaths = new Set();
      let dropDebounceTimer = null;

      await webview.onDragDropEvent((event) => {
        const type = event?.payload?.type;
        if (type === "enter" || type === "over") {
          showOverlay();
        } else if (type === "leave" || type === "cancel") {
          hideOverlay();
        } else if (type === "drop") {
          hideOverlay();
          const paths = Array.isArray(event?.payload?.paths) ? event.payload.paths : [];
          let enqueuedAny = false;
          for (const raw of paths) {
            const cleaned = String(raw || "").trim();
            if (!cleaned) continue;
            const normKey = cleaned.toLowerCase();
            if (recentDropSet.has(normKey) || pendingDropPaths.has(cleaned)) continue;
            pendingDropPaths.add(cleaned);
            enqueuedAny = true;
          }
          if (enqueuedAny || pendingDropPaths.size > 0) {
            if (dropDebounceTimer !== null) {
              clearTimeout(dropDebounceTimer);
            }
            dropDebounceTimer = setTimeout(() => {
              dropDebounceTimer = null;
              const batch = Array.from(pendingDropPaths);
              pendingDropPaths.clear();
              if (batch.length === 0) return;
              const keys = batch.map((p) => p.toLowerCase());
              for (const k of keys) recentDropSet.add(k);
              setTimeout(() => {
                for (const k of keys) recentDropSet.delete(k);
              }, 500);
              handleIngestPathsFlow(batch);
            }, 500);
          }
        }
      });
    }
  } catch {
    // WebView drag-drop API may be unavailable in non-Tauri preview environments.
  }
}

async function startDownloadFromUrl(urlVal, switchToDownloader = false) {
  const url = String(urlVal || el("input-dl-url")?.value || "").trim();
  if (!isValidDownloadUrl(url)) {
    showToast("Enter a valid http://, https://, or duke:// download URL.", "error");
    return false;
  }
  const mode = el("select-dl-speed-mode")?.value || "unlimited";
  const maxSpeedMbps = mode === "unlimited" ? null : Number(mode);
  try {
    state.downloadJobs = await invokeCommand("enqueue_download", {
      url,
      customSaveDir: state.defaultSaveDir || state.customSaveDir || null,
      threads: 8,
      maxSpeedMbps
    });
    markClipboardUrlHandled(url);
    if (!urlVal && el("input-dl-url")) el("input-dl-url").value = "";
    renderDownloadJobs();
    updateSidebarTelemetry();
    if (switchToDownloader) switchTab("downloader");
    showToast("Download started! Check Downloads to see progress.", "success");
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
      if (tab === "sources") renderSources();
      if (tab === "settings") {
        renderSources();
        refreshSettingsDriveSpaceBar();
        refreshSwarmAndMmapTelemetry();
      }
    });
  });

  // Settings Mode Switcher (Essentials vs. Advanced Engine)
  el("btn-settings-tab-essentials")?.addEventListener("click", () => switchSettingsTab("essentials"));
  el("btn-settings-tab-advanced")?.addEventListener("click", () => switchSettingsTab("advanced"));

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
    const isPlayTonight = state.filter === "play_tonight";
    el("play-tonight-session-wrap")?.classList.toggle("hidden", !isPlayTonight);
    if (isPlayTonight) {
      refreshPlayTonightFilter();
    } else {
      renderLibraryGrid();
    }
  });

  el("select-play-tonight-mins")?.addEventListener("change", () => {
    if (state.filter === "play_tonight") {
      refreshPlayTonightFilter();
    }
  });

  el("btn-titlebar-couch")?.addEventListener("click", () => {
    toggleCouchMode(!state.couchMode, true);
    showToast(state.couchMode ? "Duke Deck 10-Foot Mode activated." : "Returned to Desktop Mode.", "info", 2200);
  });

  el("btn-couch-calibrate")?.addEventListener("click", () => {
    const pop = el("couch-calibrator-popover");
    if (!pop) return;
    const willOpen = pop.classList.contains("hidden");
    pop.classList.toggle("hidden", !willOpen);
    if (willOpen) {
      calibrateControllerAxes("status").catch(() => {});
    }
  });
  el("btn-cal-close")?.addEventListener("click", () => {
    closeCouchCalibratorPopover();
  });
  el("cal-deadzone-slider")?.addEventListener("input", (e) => {
    const pct = Number(e.target?.value || 14);
    if (el("cal-deadzone-val")) el("cal-deadzone-val").textContent = `${pct}%`;
    calibrateControllerAxes("set_deadzone", pct / 100).catch(() => {});
  });
  el("btn-cal-auto-center")?.addEventListener("click", async () => {
    try {
      const dz = Number(el("cal-deadzone-slider")?.value || 14) / 100;
      const rep = await calibrateControllerAxes("auto_center", dz);
      showToast(`Captured rest centers for ${rep?.deviceName || "controller"}.`, "success", 2200);
    } catch (err) {
      showToast(String(err), "error");
    }
  });
  el("btn-cal-reset")?.addEventListener("click", async () => {
    try {
      await calibrateControllerAxes("reset", 0.14);
      if (el("cal-deadzone-slider")) el("cal-deadzone-slider").value = "14";
      if (el("cal-deadzone-val")) el("cal-deadzone-val").textContent = "14%";
      showToast("Reset controller axis calibration to defaults.", "info", 2000);
    } catch (err) {
      showToast(String(err), "error");
    }
  });
  el("btn-couch-radial")?.addEventListener("click", () => openRadialHotbar());
  el("btn-couch-vkb")?.addEventListener("click", () => openVirtualKeyboard(el("search-input")));
  el("btn-couch-exit")?.addEventListener("click", () => toggleCouchMode(false, true));
  el("btn-radial-close")?.addEventListener("click", () => closeRadialHotbar());
  el("radial-slots-ring")?.addEventListener("click", (e) => {
    const btn = e.target.closest(".radial-slot-btn[data-radial-action]");
    if (!btn) return;
    handleRadialActionDispatch(btn.dataset.radialAction, Number(btn.dataset.radialSlot));
  });

  el("btn-vkb-close")?.addEventListener("click", () => closeVirtualKeyboard(false));
  el("btn-vkb-done")?.addEventListener("click", () => closeVirtualKeyboard(true));
  el("btn-vkb-shift")?.addEventListener("click", () => {
    state.vkbShift = !state.vkbShift;
    renderVirtualKeyboardGrid();
  });
  el("btn-vkb-space")?.addEventListener("click", () => {
    if (el("vkb-preview")) el("vkb-preview").value += " ";
  });
  el("btn-vkb-backspace")?.addEventListener("click", () => {
    const p = el("vkb-preview");
    if (p) p.value = p.value.slice(0, -1);
  });
  el("vkb-grid")?.addEventListener("click", (e) => {
    const keyBtn = e.target.closest(".vkb-key-btn[data-vkb-char]");
    if (!keyBtn || !el("vkb-preview")) return;
    el("vkb-preview").value += keyBtn.dataset.vkbChar;
  });

  el("btn-pilot-close")?.addEventListener("click", closeDukePilotPopover);
  el("btn-pilot-example")?.addEventListener("click", () => {
    const ex = PILOT_EXAMPLES[pilotExampleIdx % PILOT_EXAMPLES.length];
    pilotExampleIdx += 1;
    const inp = el("search-input");
    if (inp) {
      inp.value = ex;
      inp.focus();
      scheduleDukePilotPreview(ex);
    }
  });
  el("btn-pilot-run")?.addEventListener("click", () => runActiveDukePilotPlan());
  el("chk-pilot-ai")?.addEventListener("change", () => {
    const q = el("search-input")?.value || "";
    if (q.trim()) scheduleDukePilotPreview(q);
  });
  el("pilot-semantic-list")?.addEventListener("click", (e) => {
    const row = e.target.closest(".pilot-semantic-row[data-catalog-query]");
    if (!row) return;
    closeDukePilotPopover();
    switchTab("catalog");
    if (el("input-catalog-search")) {
      el("input-catalog-search").value = row.dataset.catalogQuery || "";
      handleCatalogSearchInput();
    }
  });
  el("pilot-everything-list")?.addEventListener("click", (e) => {
    const row = e.target.closest(".pilot-everything-row[data-everything-idx]");
    if (!row) return;
    const idx = Number(row.dataset.everythingIdx);
    const item = Array.isArray(state.pilotEverythingItems) ? state.pilotEverythingItems[idx] : null;
    if (item) executeEverythingItem(item);
  });

  el("btn-run-disk-pressure")?.addEventListener("click", () => {
    runDiskPressureAutopilotInteractive();
  });
  el("btn-dismiss-disk-pressure")?.addEventListener("click", () => {
    el("disk-pressure-banner")?.classList.add("hidden");
    state.diskPressureDismissed = true;
  });

  el("sort-select")?.addEventListener("change", (e) => {
    state.sortBy = e.target.value;
    renderLibraryGrid();
  });

  el("btn-toggle-sidebar")?.addEventListener("click", () => {
    applySidebarCollapsed(!state.sidebarCollapsed);
  });

  [
    ["btn-library-zoom-out", -15],
    ["btn-library-zoom-in", 15],
    ["btn-catalog-zoom-out", -15],
    ["btn-catalog-zoom-in", 15]
  ].forEach(([id, delta]) => {
    el(id)?.addEventListener("click", () => applyCardZoom(state.cardZoomPx + delta, true));
  });
  el("btn-library-zoom-reset")?.addEventListener("click", () => applyCardZoom(185, true));
  el("btn-catalog-zoom-reset")?.addEventListener("click", () => applyCardZoom(185, true));

  let middleZoomHeld = false;
  ["library-view", "catalog-view"].forEach((viewId) => {
    const viewEl = el(viewId);
    if (!viewEl) return;
    viewEl.addEventListener("mousedown", (e) => {
      if (e.button === 1) {
        middleZoomHeld = true;
        e.preventDefault();
      }
    });
    viewEl.addEventListener(
      "wheel",
      (e) => {
        if (e.ctrlKey || (e.buttons & 4) !== 0 || e.buttons === 4 || middleZoomHeld) {
          e.preventDefault();
          const step = e.deltaY < 0 ? 15 : -15;
          applyCardZoom(state.cardZoomPx + step, true);
        }
      },
      { passive: false }
    );
  });
  window.addEventListener("mouseup", (e) => {
    if (e.button === 1 || (e.buttons & 4) === 0) middleZoomHeld = false;
  });
  window.addEventListener("blur", () => {
    middleZoomHeld = false;
    state.coopKeysDown?.clear();
    clearControllerFocus({
      disconnect: !state.controllerConnected,
      blurNonText: true,
      resetRepeats: true
    });
  });
  window.addEventListener("gamepaddisconnected", () => {
    clearControllerFocus({
      disconnect: true,
      blurNonText: true,
      resetRepeats: true
    });
    state.gamepadPollingRateFn?.();
  });
  const handleMousePointerReturn = (blurActive) => {
    if (!state.hasActiveGamepadFocus) return;
    clearControllerFocus({
      disconnect: !state.controllerConnected,
      blurNonText: blurActive,
      resetRepeats: true
    });
  };
  document.addEventListener("mousemove", () => handleMousePointerReturn(true), { passive: true });
  document.addEventListener("mousedown", () => handleMousePointerReturn(false), { passive: true });
  window.addEventListener("keydown", (e) => {
    if (document.body?.classList?.contains("overlay-hud-mode")) return;
    if (e.key === "Escape" && !e.ctrlKey && !e.altKey && !e.metaKey) {
      if (closeTopmostUiLayer()) {
        e.preventDefault();
      }
    }
  });
  window.addEventListener("beforeunload", () => {
    if (state.friendsPollTimer !== null) {
      clearInterval(state.friendsPollTimer);
      state.friendsPollTimer = null;
    }
    if (state.coopInputTimer !== null) {
      clearInterval(state.coopInputTimer);
      state.coopInputTimer = null;
    }
    if (state.clipboardFocusTimer !== null) {
      clearInterval(state.clipboardFocusTimer);
      state.clipboardFocusTimer = null;
    }
    if (state.pilotDebounceId !== null) {
      clearTimeout(state.pilotDebounceId);
      state.pilotDebounceId = null;
    }
    if (state.libraryPatchTimer !== null) {
      clearTimeout(state.libraryPatchTimer);
      state.libraryPatchTimer = null;
    }
    if (state.libraryPatchRaf !== null) {
      cancelAnimationFrame(state.libraryPatchRaf);
      state.libraryPatchRaf = null;
    }
    state.gamepadStopPollingFn?.();
    abortAllScopedRequests();
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
    scheduleDukePilotPreview(state.searchQuery);
  });

  el("search-input")?.addEventListener("keydown", (e) => {
    const popVisible = !el("duke-pilot-popover")?.classList.contains("hidden");
    if (e.key === "ArrowDown") {
      if (!popVisible) el("duke-pilot-popover")?.classList.remove("hidden");
      e.preventDefault();
      movePilotEverythingSelection(1);
    } else if (e.key === "ArrowUp") {
      if (!popVisible) el("duke-pilot-popover")?.classList.remove("hidden");
      e.preventDefault();
      movePilotEverythingSelection(-1);
    } else if (e.key === "Enter") {
      const idx = state.pilotEverythingIdx;
      const items = Array.isArray(state.pilotEverythingItems) ? state.pilotEverythingItems : [];
      if (idx >= 0 && items[idx]) {
        e.preventDefault();
        executeEverythingItem(items[idx]);
      } else if (state.searchQuery.trim() && (!state.pilotPlan?.steps || state.pilotPlan.steps.length === 0) && items[0]) {
        e.preventDefault();
        executeEverythingItem(items[0]);
      } else if (state.searchQuery.trim()) {
        e.preventDefault();
        runActiveDukePilotPlan();
      }
    } else if (e.key === "Escape") {
      closeDukePilotPopover();
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

  el("catalog-platform-pills")?.addEventListener("click", (e) => {
    const pill = e.target.closest(".catalog-platform-pill[data-catalog-platform]");
    if (!pill) return;
    state.catalog.platform = String(pill.dataset.catalogPlatform || "all");
    syncCatalogFilterControlsToDom();
    renderCatalogView(true);
  });

  el("btn-catalog-sync-retro")?.addEventListener("click", async () => {
    const btn = el("btn-catalog-sync-retro");
    try {
      if (btn) btn.disabled = true;
      showToast("Syncing Nintendo & retro console database...", "info", 3200);
      const status = await syncRetroCatalog(true);
      showToast(
        status?.message || `Indexed ${Number(status?.totalCatalogEntries || 0).toLocaleString()} catalog titles.`,
        "success",
        3800
      );
      if (state.activeTab === "catalog") {
        renderCatalogView(true);
      }
    } catch (err) {
      showToast(String(err), "error");
    } finally {
      if (btn) btn.disabled = false;
    }
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
  const closeCatalogModal = closeCatalogDetailModal;
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

  el("btn-catalog-add-library")?.addEventListener("click", async () => {
    const entry = state.catalog.modalGame;
    if (!entry) return;
    try {
      const added = await addRetroGameFromCatalog({ appId: entry.appId, romPath: null });
      state.library = await invokeCommand("get_local_library");
      invalidateSidebarIcons([added?.id].filter(Boolean));
      renderSidebarGameList();
      renderLibraryGrid();
      renderCatalogView(true);
      updateSidebarTelemetry();
      closeCatalogModal();
      showToast(`Added ${added?.title || entry.name} (${entry.platformName || "Console"}) to My Games.`, "success");
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-catalog-link-exe")?.addEventListener("click", async () => {
    const entry = state.catalog.modalGame;
    if (!entry) return;
    const isRetro = Number(entry.platformId || 0) > 0;
    try {
      if (isRetro) {
        const romPath = await pickRomFile();
        if (!romPath) return;
        const added = await addRetroGameFromCatalog({ appId: entry.appId, romPath });
        state.library = await invokeCommand("get_local_library");
        invalidateSidebarIcons([added?.id].filter(Boolean));
        renderSidebarGameList();
        renderLibraryGrid();
        renderCatalogView(true);
        updateSidebarTelemetry();
        closeCatalogModal();
        showToast(`Linked ${added?.title || entry.name} ROM to My Games.`, "success");
        return;
      }
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

  // --- Essentials Settings Listeners ---
  el("btn-settings-browse-save-dir")?.addEventListener("click", async () => {
    const dir = await pickDownloadFolder();
    if (!dir) return;
    try {
      const updated = await updateLauncherSettings({ defaultSaveDir: dir });
      syncSettingsUiFromBackend(updated);
      showToast(`Games save folder updated: ${dir}`, "success");
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-settings-open-save-dir")?.addEventListener("click", async () => {
    const target = state.defaultSaveDir || state.customSaveDir;
    if (!target) {
      await refreshSettingsDriveSpaceBar();
    }
    const dirToOpen = state.defaultSaveDir || state.customSaveDir;
    if (!dirToOpen) return;
    invokeCommand("open_game_folder", { path: dirToOpen }).catch((err) => showToast(String(err), "error"));
  });

  el("setting-speed-preset-group")?.addEventListener("click", async (e) => {
    const btn = e.target.closest("[data-speed-preset]");
    if (!btn) return;
    const preset = btn.dataset.speedPreset;
    try {
      const updated = await updateLauncherSettings({ downloadSpeedPreset: preset });
      syncSettingsUiFromBackend(updated);
      const labels = {
        max: "Download speed set to Max Speed.",
        balanced: "Download speed set to Balanced (25 MB/s).",
        background: "Download speed set to Background (5 MB/s)."
      };
      showToast(labels[preset] || "Download speed updated.", "success", 2400);
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("setting-compact-cards")?.addEventListener("change", async (e) => {
    const checked = Boolean(e.target.checked);
    applyCompactLibraryCards(checked);
    try {
      const updated = await updateLauncherSettings({ compactLibraryCards: checked });
      syncSettingsUiFromBackend(updated);
      showToast(
        checked ? "Compact library cards enabled." : "Standard library cards restored.",
        "info",
        2200
      );
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("setting-performance-boost")?.addEventListener("change", async (e) => {
    const checked = Boolean(e.target.checked);
    try {
      const updated = await updateLauncherSettings({ performanceBoostEnabled: checked });
      syncSettingsUiFromBackend(updated);
      showToast(
        checked
          ? "Game Performance Boost enabled — games will get priority speed & RAM cleanup on Play."
          : "Game Performance Boost turned off.",
        "info",
        2600
      );
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("setting-auto-backup-saves-exit")?.addEventListener("change", async (e) => {
    const checked = Boolean(e.target.checked);
    try {
      const updated = await updateLauncherSettings({ autoBackupSavesOnExit: checked });
      syncSettingsUiFromBackend(updated);
      showToast(
        checked ? "Automatic save backups on game quit enabled." : "Automatic save backups on quit disabled.",
        "info",
        2400
      );
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-clean-desktop-shortcuts")?.addEventListener("click", async () => {
    const btn = el("btn-clean-desktop-shortcuts");
    const summaryEl = el("desktop-cleanup-summary");
    if (btn) btn.disabled = true;
    showToast("Scanning Desktop shortcuts for games...", "info", 2400);
    try {
      const rep = await scanAndCleanDesktopShortcuts();
      if (Array.isArray(rep?.library)) {
        state.library = rep.library;
        invalidateSidebarIcons();
        renderSidebarGameList();
        renderLibraryGrid();
        updateSidebarTelemetry();
      }
      const imported = Number(rep?.importedCount || 0);
      const moved = Number(rep?.movedToBackup || 0);
      const skipped = Number(rep?.skippedNonGame || 0);
      const msg =
        moved > 0
          ? `Added ${imported} game(s) and moved ${moved} Desktop shortcut(s) to "${rep.backupDir}" (${skipped} non-game shortcuts left untouched).`
          : `Checked ${Number(rep?.scannedShortcuts || 0)} Desktop shortcut(s): no game shortcuts needed moving.`;
      if (summaryEl) {
        summaryEl.textContent = msg;
        summaryEl.classList.remove("hidden", "warn");
      }
      showToast(msg, imported > 0 ? "success" : "info", 4200);
    } catch (err) {
      showToast(String(err), "error");
    } finally {
      if (btn) btn.disabled = false;
    }
  });

  // --- Advanced Engine Settings Listeners ---
  el("setting-cpu-priority-mode")?.addEventListener("change", async (e) => {
    const cpuPriorityMode = String(e.target.value || "auto");
    try {
      const updated = await updateLauncherSettings({ cpuPriorityMode });
      syncSettingsUiFromBackend(updated);
      showToast(`CPU Priority Mode updated to "${cpuPriorityMode}".`, "success", 2400);
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("setting-ram-trim-mode")?.addEventListener("change", async (e) => {
    const ramTrimMode = String(e.target.value || "standard");
    try {
      const updated = await updateLauncherSettings({ ramTrimMode });
      syncSettingsUiFromBackend(updated);
      showToast(`Working-Set RAM Trim Mode set to "${ramTrimMode}".`, "success", 2400);
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("setting-auto-freeze-minimized")?.addEventListener("change", async (e) => {
    const checked = Boolean(e.target.checked);
    try {
      const updated = await updateLauncherSettings({ autoFreezeMinimized: checked });
      syncSettingsUiFromBackend(updated);
      showToast(
        checked
          ? "Auto-Freeze Minimized Single-Player Games enabled."
          : "Auto-Freeze Minimized Games disabled.",
        "info",
        2500
      );
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  const commitNvmeBudgetPct = async (rawPct) => {
    const snapped = snapNvmeBudgetPct(rawPct);
    renderNvmeBudgetUi(snapped);
    try {
      const updated = await updateLauncherSettings({ nvmeCacheBudgetPct: snapped });
      syncSettingsUiFromBackend(updated);
      showToast(`Standby RAM cache budget set to ${snapped}%.`, "success", 2200);
    } catch (err) {
      showToast(String(err), "error");
    }
  };
  el("setting-nvme-cache-budget")?.addEventListener("input", (e) => {
    renderNvmeBudgetUi(e.target.value);
  });
  el("setting-nvme-cache-budget")?.addEventListener("change", (e) => {
    commitNvmeBudgetPct(e.target.value);
  });
  el("nvme-budget-presets")?.addEventListener("click", (e) => {
    const btn = e.target.closest("[data-nvme-pct]");
    if (btn) commitNvmeBudgetPct(btn.dataset.nvmePct);
  });

  el("setting-doh-enabled")?.addEventListener("change", async (e) => {
    const checked = Boolean(e.target.checked);
    try {
      const updated = await updateLauncherSettings({ dohEnabled: checked });
      syncSettingsUiFromBackend(updated);
      showToast(
        checked ? "Encrypted DNS-over-HTTPS (1.1.1.1 / 9.9.9.9) enabled." : "Encrypted DNS-over-HTTPS bypassed (system DNS).",
        "info",
        2600
      );
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-apply-anonymity-preset")?.addEventListener("click", async () => {
    const dohProvider = el("setting-doh-provider")?.value || "auto";
    const proxyPreset = el("setting-proxy-preset")?.value || "direct";
    const customProxyUrl = (el("setting-custom-proxy-url")?.value || "").trim() || null;
    try {
      const st = await applyAnonymityNetworkPreset(dohProvider, proxyPreset, customProxyUrl);
      renderPrivacyVaultStatus(st);
      if (el("setting-doh-enabled")) el("setting-doh-enabled").checked = Boolean(st?.dohEnabled);
      if (el("setting-custom-proxy-url")) el("setting-custom-proxy-url").value = st?.effectiveProxyUrl || "";
      showToast(
        st?.effectiveProxyUrl
          ? `Anonymity routing active (${st.dohProvider} DoH + ${st.effectiveProxyUrl}).`
          : `Encrypted DNS routing active (${st?.dohProvider || dohProvider} DoH, direct).`,
        "success",
        3200
      );
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-save-custom-proxy")?.addEventListener("click", async () => {
    const customProxyUrl = (el("setting-custom-proxy-url")?.value || "").trim();
    try {
      const updated = await updateLauncherSettings({ customProxyUrl });
      syncSettingsUiFromBackend(updated);
      showToast(
        customProxyUrl ? `Custom proxy applied: ${customProxyUrl}` : "Custom proxy cleared (direct connection).",
        "success",
        2800
      );
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-save-custom-speed-cap")?.addEventListener("click", async () => {
    const raw = Number(el("setting-custom-speed-cap")?.value || 0);
    const customSpeedCapMbps = Number.isFinite(raw) && raw > 0 ? raw : 0;
    try {
      const updated = await updateLauncherSettings({ customSpeedCapMbps });
      syncSettingsUiFromBackend(updated);
      showToast(
        customSpeedCapMbps > 0
          ? `Download speed cap set to ${customSpeedCapMbps} MB/s.`
          : "Download speed cap set to unlimited.",
        "success",
        2400
      );
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("setting-auto-unblock-motw")?.addEventListener("change", async (e) => {
    const checked = Boolean(e.target.checked);
    try {
      const updated = await updateLauncherSettings({ autoUnblockMotw: checked });
      syncSettingsUiFromBackend(updated);
      showToast(
        checked ? "Automatic MOTW (:Zone.Identifier) unblocking enabled." : "Automatic MOTW unblocking disabled.",
        "info",
        2400
      );
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("setting-portable-sandbox-default")?.addEventListener("change", async (e) => {
    const checked = Boolean(e.target.checked);
    try {
      const updated = await updateLauncherSettings({ portableSandboxDefault: checked });
      syncSettingsUiFromBackend(updated);
      showToast(
        checked ? "New games will isolate saves inside .duke-profile/ by default." : "Default portable save sandboxing disabled.",
        "info",
        2600
      );
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("setting-max-restore-points")?.addEventListener("change", async (e) => {
    const maxRestorePoints = Number(e.target.value || 5);
    try {
      const updated = await updateLauncherSettings({ maxRestorePoints });
      syncSettingsUiFromBackend(updated);
      showToast(`Retaining up to ${maxRestorePoints} restore points per game.`, "success", 2400);
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-reclaim-snapshot-space")?.addEventListener("click", async () => {
    const btn = el("btn-reclaim-snapshot-space");
    const resEl = el("reclaim-space-result");
    if (btn) btn.disabled = true;
    showToast("Scanning restore point cache for unused space...", "info", 2200);
    try {
      const rep = await reclaimSnapshotSpace();
      const summary = `Cleaned ${Number(rep?.blobsRemoved || 0)} unused data block(s) across ${Number(rep?.gamesScanned || 0)} game(s) — reclaimed ${rep?.freedFormatted || "0 B"}.`;
      if (resEl) {
        resEl.textContent = summary;
        resEl.classList.remove("hidden");
      }
      showToast(summary, "success", 3800);
    } catch (err) {
      showToast(String(err), "error");
    } finally {
      if (btn) btn.disabled = false;
    }
  });

  el("setting-block-game-telemetry")?.addEventListener("change", async (e) => {
    const checked = Boolean(e.target.checked);
    try {
      const updated = await updateLauncherSettings({ blockGameTelemetry: checked });
      syncSettingsUiFromBackend(updated);
      showToast(
        checked ? "Game telemetry & crash reporter blocking enabled." : "Game telemetry blocking disabled.",
        "info",
        2400
      );
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-save-global-launch-args")?.addEventListener("click", async () => {
    const globalLaunchArgs = (el("setting-global-launch-args")?.value || "").trim();
    try {
      const updated = await updateLauncherSettings({ globalLaunchArgs });
      syncSettingsUiFromBackend(updated);
      showToast(
        globalLaunchArgs ? `Global launch arguments saved: ${globalLaunchArgs}` : "Global launch arguments cleared.",
        "success",
        2600
      );
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("setting-swarm-auto-sync-saves")?.addEventListener("change", async (e) => {
    const checked = Boolean(e.target.checked);
    try {
      const updated = await updateLauncherSettings({ swarmAutoSyncSaves: checked });
      syncSettingsUiFromBackend(updated);
      showToast(
        checked ? "Swarm auto-sync on game exit enabled." : "Swarm auto-sync on game exit disabled.",
        "info",
        2400
      );
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-settings-sync-catalog")?.addEventListener("click", async () => {
    showToast("Syncing 30,000+ game catalog...", "info", 2600);
    await loadCatalogData({ query: state.catalog.searchQuery, forceRefresh: true });
    await refreshSwarmAndMmapTelemetry();
  });

  el("btn-settings-open-search-sites")?.addEventListener("click", () => {
    switchTab("sources");
    renderSources();
  });

  el("btn-settings-open-json-feeds")?.addEventListener("click", () => {
    state.jsonFeedsExpanded = true;
    switchTab("sources");
    el("json-feeds-body")?.classList.remove("hidden");
    el("btn-toggle-json-feeds")?.setAttribute("aria-expanded", "true");
    el("json-feeds-card")?.scrollIntoView({ behavior: "smooth", block: "nearest" });
    renderSources();
  });

  el("btn-register-duke-protocol")?.addEventListener("click", async () => {
    try {
      const msg = await registerDukeProtocol();
      showToast(msg || "Registered duke:// browser download links.", "success", 3200);
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("setting-start-minimized-windows")?.addEventListener("change", async (e) => {
    const checked = Boolean(e.target.checked);
    try {
      const updated = await updateLauncherSettings({ startMinimizedWithWindows: checked });
      syncSettingsUiFromBackend(updated);
      showToast(
        checked ? "Duke Launcher will start minimized to tray with Windows." : "Windows startup tray launch removed.",
        "info",
        2600
      );
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-settings-scan-rom-folder")?.addEventListener("click", async () => {
    const btn = el("btn-settings-scan-rom-folder");
    try {
      const folder = await pickGameFolder();
      if (!folder) return;
      if (btn) btn.disabled = true;
      showToast("Scanning folder headers for console ROMs & ISOs...", "info", 3200);
      const res = await scanRomFolder(folder);
      if (Array.isArray(res?.library)) {
        state.library = res.library;
        invalidateSidebarIcons();
        renderSidebarGameList();
        renderLibraryGrid();
        updateSidebarTelemetry();
      }
      const added = Number(res?.addedCount || 0);
      const matched = Number(res?.matchedCount || 0);
      showToast(
        added > 0
          ? `Added ${added} console game${added === 1 ? "" : "s"} (${matched} detected).`
          : "No new ROMs or ISOs were added from that folder.",
        added > 0 ? "success" : "info",
        3800
      );
    } catch (err) {
      showToast(String(err), "error");
    } finally {
      if (btn) btn.disabled = false;
    }
  });

  el("btn-settings-sync-retro-catalog")?.addEventListener("click", async () => {
    const btn = el("btn-settings-sync-retro-catalog");
    const statusLine = el("settings-retro-sync-status");
    try {
      if (btn) btn.disabled = true;
      if (statusLine) {
        statusLine.dataset.syncBusy = "1";
        statusLine.textContent = "Downloading Libretro RDB console catalogs...";
      }
      const status = await syncRetroCatalog(true);
      if (statusLine) {
        delete statusLine.dataset.syncBusy;
        statusLine.textContent =
          status?.message || `Indexed ${Number(status?.totalCatalogEntries || 0).toLocaleString()} games across all systems.`;
      }
      showToast("Console game catalog updated.", "success", 3200);
      if (state.activeTab === "catalog") renderCatalogView(true);
    } catch (err) {
      if (statusLine) delete statusLine.dataset.syncBusy;
      showToast(String(err), "error");
    } finally {
      if (btn) btn.disabled = false;
    }
  });

async function applyDroppedOrPickedRomPatch(game, patchPath) {
  if (!game || !patchPath) return;
  const hashVal = (el("input-rom-patch-hash")?.value || "").trim();
  const expectedCrc32 = /^[0-9a-fA-F]{8}$/.test(hashVal) ? hashVal : null;
  const expectedSha1 = /^[0-9a-fA-F]{40}$/.test(hashVal) ? hashVal : null;
  const statusEl = el("detail-rom-patch-status");
  if (statusEl) {
    statusEl.classList.remove("hidden");
    statusEl.textContent = "Creating .duke-cas restore point & applying pure-Rust ROM patch...";
  }
  try {
    const res = await applyRomPatchAndCreateProfile(
      game.id,
      patchPath,
      expectedCrc32,
      expectedSha1
    );
    state.library = await invokeCommand("get_local_library");
    invalidateSidebarIcons();
    renderSidebarGameList();
    renderLibraryGrid();
    updateSidebarTelemetry();
    if (statusEl) {
      statusEl.textContent = `${res?.summary || "Patched ROM profile created."} (CRC32 ${res?.patchedCrc32 || "--"})`;
    }
    showToast(res?.summary || "Patched ROM profile created cleanly.", "success", 4200);
    if (res?.patchedGameId) {
      await openGameDetail(res.patchedGameId);
    }
  } catch (err) {
    if (statusEl) statusEl.textContent = String(err);
    showToast(String(err), "error");
  }
}

async function runModDropperInstall(game, sourceInput) {
  if (!game || !String(sourceInput || "").trim()) return;
  const statusEl = el("detail-moddrop-security-summary");
  if (statusEl) {
    statusEl.classList.remove("hidden", "blocked", "safe");
    statusEl.classList.add("shield-verdict-banner");
    statusEl.textContent = "Scanning staged .dll/.asi/.exe with Security Shield v2 & creating .duke-cas snapshot...";
  }
  try {
    const rep = await installModArchiveOrWorkshop(game.id, sourceInput);
    if (statusEl) {
      statusEl.classList.remove("blocked", "safe");
      statusEl.classList.add("shield-verdict-banner", rep?.blocked ? "blocked" : "safe");
      const scans = Array.isArray(rep?.scannedBinaries) ? rep.scannedBinaries : [];
      const badgeNote = scans.length > 0
        ? ` · Verified ${scans.map((s) => `${s.fileName} [${s.shieldBadge}]`).slice(0, 4).join(", ")}`
        : "";
      statusEl.textContent = `${rep?.summary || "Mod processed."}${badgeNote}`;
    }
    if (rep?.modGraph) {
      renderDetailModGraph(rep.modGraph);
    }
    showToast(
      rep?.summary || (rep?.blocked ? "Unsafe mod payload quarantined." : "Mod installed cleanly."),
      rep?.blocked ? "error" : "success",
      4400
    );
  } catch (err) {
    if (statusEl) {
      statusEl.classList.add("shield-verdict-banner", "blocked");
      statusEl.textContent = String(err);
    }
    showToast(String(err), "error");
  }
}

let pendingCleanUninstallGameId = null;

function closeCleanUninstallModal() {
  el("clean-uninstall-modal")?.classList.add("hidden");
  pendingCleanUninstallGameId = null;
}

async function openCleanUninstallModalForGame(game) {
  if (!game) return;
  pendingCleanUninstallGameId = game.id;
  el("clean-uninstall-modal")?.classList.remove("hidden");
  if (el("uninstall-modal-summary")) {
    el("uninstall-modal-summary").textContent = "Inspecting install folder, .duke-profile sandbox, saves & HKCU registry traces...";
  }
  if (el("uninstall-modal-paths")) {
    el("uninstall-modal-paths").innerHTML = "";
  }
  try {
    const preview = await previewCleanUninstall(game.id);
    if (pendingCleanUninstallGameId !== game.id) return;
    if (el("uninstall-modal-summary")) {
      el("uninstall-modal-summary").textContent = preview?.summary || `Ready to cleanly uninstall ${game.title}.`;
    }
    if (el("uninstall-modal-paths")) {
      const rows = [
        ["Install Dir", `${preview.installDir || "--"} (${formatBytesJs(preview.installDirBytes || 0)})`],
        ["Sandbox Profile", preview.sandboxProfileDir || "None"],
        ["Discovered Saves", `${(preview.savePaths || []).length} path(s) (${formatBytesJs(preview.saveBytes || 0)})`],
        ["Orphan AppData", `${(preview.orphanAppdataDirs || []).length} leftover dir(s)`],
        ["HKCU Uninstall / GPU", `${(preview.hkcuUninstallKeys || []).length} key(s) + ${(preview.gpuPrefKeys || []).length} GPU pref`]
      ];
      el("uninstall-modal-paths").innerHTML = rows
        .map(
          ([k, v]) => `<div class="kv-row"><span>${escapeHtml(k)}</span><strong>${escapeHtml(v)}</strong></div>`
        )
        .join("");
    }
  } catch (err) {
    if (el("uninstall-modal-summary")) {
      el("uninstall-modal-summary").textContent = String(err || "Unable to preview uninstall.");
    }
  }
}

  el("btn-settings-refresh-emus")?.addEventListener("click", () => {
    loadEmulatorSettingsSection(true);
  });

  el("btn-settings-group-multidisc")?.addEventListener("click", async () => {
    const btn = el("btn-settings-group-multidisc");
    try {
      if (btn) btn.disabled = true;
      const res = await groupMultiDiscLibrary();
      state.library = await invokeCommand("get_local_library");
      invalidateSidebarIcons();
      renderSidebarGameList();
      renderLibraryGrid();
      updateSidebarTelemetry();
      showToast(res?.summary || "Multi-disc .m3u grouping finished.", "success", 3800);
    } catch (err) {
      showToast(String(err), "error");
    } finally {
      if (btn) btn.disabled = false;
    }
  });

  el("btn-scan-bios-folder")?.addEventListener("click", async () => {
    const folder = await pickGameFolder();
    if (!folder) return;
    try {
      showToast("Scanning BIOS folder with Libretro MD5/SHA-1 hashes...", "info", 2400);
      const rep = await scanAndInstallBiosSources([folder]);
      renderBiosVaultReport(rep);
      showToast(rep?.summary || "BIOS scan finished.", "success", 3600);
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-scan-bios-archive")?.addEventListener("click", async () => {
    const file = await pickBiosOrPatchFile([
      { name: "BIOS Archives & Firmware", extensions: ["zip", "7z", "rar", "bin", "rom"] }
    ]);
    if (!file) return;
    try {
      showToast("Extracting & validating BIOS archive...", "info", 2400);
      const rep = await scanAndInstallBiosSources([file]);
      renderBiosVaultReport(rep);
      showToast(rep?.summary || "BIOS archive processed.", "success", 3600);
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-refresh-bios-vault")?.addEventListener("click", () => {
    refreshBiosVaultStatus(true);
  });

  el("settings-emulators-list")?.addEventListener("click", async (e) => {
    const setupBtn = e.target.closest("[data-emu-setup]");
    if (setupBtn) {
      const emuId = String(setupBtn.dataset.emuSetup || "");
      if (!emuId) return;
      try {
        setupBtn.disabled = true;
        showToast("Downloading portable console player...", "info", 3200);
        const statuses = await installEmulatorForConsole(emuId);
        renderEmulatorStatusCards(statuses);
        showToast("Portable console player installed and configured.", "success", 3400);
      } catch (err) {
        showToast(String(err), "error");
      } finally {
        setupBtn.disabled = false;
      }
      return;
    }

    const browseBtn = e.target.closest("[data-emu-browse]");
    if (browseBtn) {
      const emuId = String(browseBtn.dataset.emuBrowse || "");
      if (!emuId) return;
      try {
        const chosen = await pickExecutableFile();
        if (!chosen) return;
        const statuses = await setCustomEmulatorPath(emuId, chosen);
        renderEmulatorStatusCards(statuses);
        showToast("Custom emulator executable saved.", "success", 3000);
      } catch (err) {
        showToast(String(err), "error");
      }
      return;
    }

    const clearBtn = e.target.closest("[data-emu-clear]");
    if (clearBtn) {
      const emuId = String(clearBtn.dataset.emuClear || "");
      if (!emuId) return;
      try {
        const statuses = await setCustomEmulatorPath(emuId, "");
        renderEmulatorStatusCards(statuses);
        showToast("Cleared custom emulator path override.", "info", 2600);
      } catch (err) {
        showToast(String(err), "error");
      }
    }
  });

  el("btn-close-emu-install-modal")?.addEventListener("click", closeEmulatorInstallModal);
  el("btn-emu-install-cancel")?.addEventListener("click", closeEmulatorInstallModal);
  el("emu-install-prompt-modal")?.addEventListener("click", (e) => {
    if (e.target === el("emu-install-prompt-modal")) closeEmulatorInstallModal();
  });

  el("btn-emu-open-settings")?.addEventListener("click", () => {
    closeEmulatorInstallModal();
    switchTab("settings");
    loadEmulatorSettingsSection();
    el("settings-consoles-card")?.scrollIntoView({ behavior: "smooth", block: "nearest" });
  });

  el("btn-emu-install-and-play")?.addEventListener("click", async () => {
    const prompt = pendingEmuInstallPrompt;
    if (!prompt?.consoleSlug) return;
    const installBtn = el("btn-emu-install-and-play");
    const wrap = el("emu-install-progress-wrap");
    if (installBtn) installBtn.disabled = true;
    wrap?.classList.remove("hidden");
    try {
      const statuses = await installEmulatorForConsole(prompt.consoleSlug);
      renderEmulatorStatusCards(statuses);
      const targetGame = prompt.game;
      closeEmulatorInstallModal();
      showToast(`${prompt.emuDisplayName} ready — starting ${targetGame?.title || "game"}!`, "success", 3000);
      if (targetGame) {
        await launchGameDirectly(targetGame);
      }
    } catch (err) {
      if (installBtn) installBtn.disabled = false;
      showToast(String(err), "error");
    }
  });

  el("btn-copy-mcp-config")?.addEventListener("click", async () => {
    const sampleConfig = JSON.stringify(
      {
        mcpServers: {
          "duke-launcher": {
            command: "duke-launcher.exe",
            args: ["--mcp"]
          }
        }
      },
      null,
      2
    );
    const ok = await copyTextToClipboard(sampleConfig);
    showToast(
      ok ? "Copied --mcp AI agent config JSON to clipboard." : "Could not copy MCP config JSON.",
      ok ? "success" : "error",
      3000
    );
  });

  el("btn-run-subsystem-bench")?.addEventListener("click", async () => {
    const btn = el("btn-run-subsystem-bench");
    const out = el("settings-bench-output");
    if (btn) btn.disabled = true;
    showToast("Running hardware subsystem latency benchmarks...", "info", 2400);
    try {
      const res = await runSubsystemBenchmark();
      if (out) {
        out.textContent = String(res?.table || JSON.stringify(res?.report || res, null, 2));
        out.classList.remove("hidden");
      }
      showToast("Hardware subsystem benchmark finished.", "success", 2800);
    } catch (err) {
      showToast(String(err), "error");
    } finally {
      if (btn) btn.disabled = false;
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
    showToast(`Copying ${gameKey} over local network...`, "info", 3200);
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

  el("setting-fse-gaming-home")?.addEventListener("change", async (e) => {
    const enabled = Boolean(e.target.checked);
    try {
      const fse = await setFseGamingHome(enabled, true);
      syncFseShellUi(fse);
      if (enabled) applyCouchMode(true, false);
      showToast(fse?.statusSummary || (enabled ? "Duke Deck set as Windows Gaming Home." : "Desktop shell restored."), "success");
    } catch (err) {
      e.target.checked = !enabled;
      showToast(String(err), "error");
    }
  });

  el("btn-fse-resume-bloat")?.addEventListener("click", async () => {
    try {
      const rep = await resumeWindowsBloatNow();
      const fse = await getFseShellStatus().catch(() => null);
      if (fse) syncFseShellUi(fse);
      showToast(rep?.message || "Resumed background Windows processes.", "info");
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-wasm-install-file")?.addEventListener("click", async () => {
    try {
      const picked = await pickBiosOrPatchFile([
        { name: "WebAssembly Plugin (.wasm / .wat)", extensions: ["wasm", "wat"] }
      ]);
      if (!picked) return;
      const plugin = await installWasmPlugin(picked, null, [], ["metadata_lookup"]);
      await refreshWasmPluginsUi();
      showToast(
        `Installed sandboxed plugin "${plugin?.name || "Plugin"}" with 0 domain grants. Approve explicit capabilities below before execution.`,
        "success",
        4200
      );
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-wasm-install-demo")?.addEventListener("click", async () => {
    try {
      const plugin = await installDemoWasmPlugin();
      await refreshWasmPluginsUi();
      showToast(
        `Installed "${plugin?.name || "Demo Plugin"}" inside Wasmtime zero-trust sandbox.`,
        "success",
        3400
      );
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-wasm-refresh")?.addEventListener("click", () => refreshWasmPluginsUi());

  el("btn-run-chaos-harness")?.addEventListener("click", async () => {
    const btn = el("btn-run-chaos-harness");
    if (btn) btn.disabled = true;
    showToast("Running v2.1.0 chaos & UI/IPC verification harness...", "info", 2800);
    try {
      const rep = await runChaosStressHarness();
      const ok = Number(rep?.failedCases || 0) === 0;
      showToast(
        `Chaos Self-Test: ${rep?.passedCases || 0}/${rep?.totalCases || 0} checks passed in ${rep?.elapsedMs || 0} ms.`,
        ok ? "success" : "error",
        4600
      );
    } catch (err) {
      showToast(String(err), "error");
    } finally {
      if (btn) btn.disabled = false;
    }
  });

  el("settings-wasm-plugins-list")?.addEventListener("click", async (e) => {
    const btn = e.target.closest("button[data-wasm-action]");
    if (!btn) return;
    const action = btn.dataset.wasmAction;
    const pluginId = btn.dataset.wasmId;
    const card = btn.closest("[data-wasm-plugin-card]");
    if (!pluginId || !card) return;

    if (action === "save") {
      const domainsRaw = card.querySelector("[data-wasm-domains]")?.value || "";
      const allowedDomains = domainsRaw
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean);
      const allowedHooks = Array.from(card.querySelectorAll("input[data-wasm-hook]:checked")).map(
        (cb) => cb.dataset.wasmHook
      );
      const enabled = Boolean(card.querySelector("[data-wasm-enabled]")?.checked);
      const allowTitles = Boolean(card.querySelector("[data-wasm-titles]")?.checked);
      try {
        await setWasmPluginCapabilities(pluginId, enabled, allowedDomains, allowedHooks, allowTitles);
        await refreshWasmPluginsUi();
        showToast("Updated WebAssembly plugin capability grants.", "success");
      } catch (err) {
        showToast(String(err), "error");
      }
    } else if (action === "run") {
      try {
        const res = await executeWasmPlugin(pluginId, "metadata_lookup", "");
        await refreshWasmPluginsUi();
        showToast(
          `WASM executed (${res?.fuelConsumed || 0} fuel, ${res?.emittedRecords?.length || 0} records, ${res?.capabilityDenials?.length || 0} denied calls).`,
          res?.capabilityDenials?.length ? "info" : "success",
          4200
        );
      } catch (err) {
        await refreshWasmPluginsUi();
        showToast(String(err), "error");
      }
    } else if (action === "remove") {
      try {
        const updated = await removeWasmPlugin(pluginId);
        renderWasmPluginsList(updated);
        showToast("Removed WebAssembly plugin from sandbox.", "info");
      } catch (err) {
        showToast(String(err), "error");
      }
    }
  });

  el("btn-settings-import-libraries")?.addEventListener("click", runImportLibrariesFlow);

  el("btn-settings-sync-ludusavi")?.addEventListener("click", async () => {
    showToast("Updating save-location database...", "info");
    try {
      const status = await syncLudusaviManifest(true);
      showToast(
        `Save-location database updated (${Number(status?.entryCount || 0).toLocaleString()} games).`,
        "success"
      );
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  // Multi-Platform Friends, Credentials & Playtime Sync
  el("btn-toggle-friends")?.addEventListener("click", () => toggleFriendsDrawer());
  el("btn-close-friends")?.addEventListener("click", () => toggleFriendsDrawer(false));
  el("btn-refresh-friends")?.addEventListener("click", () => refreshFriendsPanel(false));
  el("friends-drawer-list")?.addEventListener("click", (e) => {
    const btn = e.target.closest("button[data-friend-action]");
    if (btn) handleFriendActionClick(btn);
  });

  el("btn-sync-playtime")?.addEventListener("click", runPlaytimeSyncFlow);
  el("btn-settings-sync-playtime")?.addEventListener("click", runPlaytimeSyncFlow);

  el("btn-save-steam-api-key")?.addEventListener("click", async () => {
    const val = (el("setting-steam-api-key")?.value || "").trim();
    if (!val) {
      showToast("Paste your Steam Web API key before saving.", "error");
      return;
    }
    try {
      const updated = await updateLauncherSettings({ steamApiKey: val });
      syncSettingsUiFromBackend(updated);
      if (el("setting-steam-api-key")) el("setting-steam-api-key").value = "";
      showToast("Steam Web API key stored locally.", "success");
      if (state.friendsDrawerOpen) refreshFriendsPanel(true);
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-clear-steam-api-key")?.addEventListener("click", async () => {
    try {
      const updated = await updateLauncherSettings({ clearSteamApiKey: true });
      syncSettingsUiFromBackend(updated);
      showToast("Steam Web API key removed.", "info");
      if (state.friendsDrawerOpen) refreshFriendsPanel(true);
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-save-steam-id-override")?.addEventListener("click", async () => {
    const val = (el("setting-steam-id-override")?.value || "").trim();
    try {
      const updated = await updateLauncherSettings({ steamIdOverride: val });
      syncSettingsUiFromBackend(updated);
      showToast(val ? "SteamID64 override saved." : "SteamID64 override cleared (auto-detect active).", "success");
      if (state.friendsDrawerOpen) refreshFriendsPanel(true);
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-save-xbox-api-key")?.addEventListener("click", async () => {
    const val = (el("setting-xbox-api-key")?.value || "").trim();
    if (!val) {
      showToast("Paste your Xbox Live (xbl.io) API key before saving.", "error");
      return;
    }
    try {
      const updated = await updateLauncherSettings({ xboxApiKey: val });
      syncSettingsUiFromBackend(updated);
      if (el("setting-xbox-api-key")) el("setting-xbox-api-key").value = "";
      showToast("Xbox Live API key stored locally.", "success");
      if (state.friendsDrawerOpen) refreshFriendsPanel(true);
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-clear-xbox-api-key")?.addEventListener("click", async () => {
    try {
      const updated = await updateLauncherSettings({ clearXboxApiKey: true });
      syncSettingsUiFromBackend(updated);
      showToast("Xbox Live API key removed.", "info");
      if (state.friendsDrawerOpen) refreshFriendsPanel(true);
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-save-psn-npsso")?.addEventListener("click", async () => {
    const val = (el("setting-psn-npsso")?.value || "").trim();
    if (!val) {
      showToast("Paste your 64-character PSN NPSSO token before saving.", "error");
      return;
    }
    try {
      const updated = await updateLauncherSettings({ psnNpsso: val });
      syncSettingsUiFromBackend(updated);
      if (el("setting-psn-npsso")) el("setting-psn-npsso").value = "";
      showToast("PSN NPSSO token stored locally.", "success");
      if (state.friendsDrawerOpen) refreshFriendsPanel(true);
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-clear-psn-npsso")?.addEventListener("click", async () => {
    try {
      const updated = await updateLauncherSettings({ clearPsnNpsso: true });
      syncSettingsUiFromBackend(updated);
      showToast("PSN NPSSO token removed.", "info");
      if (state.friendsDrawerOpen) refreshFriendsPanel(true);
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-save-ra-key")?.addEventListener("click", async () => {
    const username = (el("setting-ra-username")?.value || "").trim();
    const apiKey = (el("setting-ra-api-key")?.value || "").trim();
    if (!apiKey) {
      showToast("Paste your RetroAchievements read-only Web API key first.", "error");
      return;
    }
    try {
      const st = await setVaultApiKey("retroAchievements", apiKey, username || null);
      renderPrivacyVaultStatus(st);
      if (el("setting-ra-api-key")) el("setting-ra-api-key").value = "";
      showToast("RetroAchievements API key encrypted in Windows DPAPI vault.", "success", 3200);
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-clear-ra-key")?.addEventListener("click", async () => {
    try {
      const st = await setVaultApiKey("retroAchievements", "", null);
      renderPrivacyVaultStatus(st);
      showToast("RetroAchievements key removed from DPAPI vault.", "info");
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
    } else if (action === "fix") {
      await openGameDetail(gameId);
      requestAnimationFrame(() => {
        el("detail-compat-card")?.scrollIntoView({ behavior: "smooth", block: "start" });
      });
    } else if (action === "backup") {
      try {
        const path = await invokeCommand("backup_game_saves", {
          gameId: game.id,
          title: game.title,
          installDir: game.installDir || null,
          exePath: game.exePath
        });
        showToast(`Save Backup archived: ${path}`, "success", 3400);
      } catch (err) {
        showToast(String(err), "error");
      }
    } else if (action === "settings") {
      await openGameDetail(gameId);
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
  el("btn-empty-add-game")?.addEventListener("click", async () => {
    const chosen = await pickExecutableOrShortcutFile();
    if (chosen) await handleIngestPathsFlow([chosen]);
  });
  el("btn-empty-add-folder")?.addEventListener("click", async () => {
    const folder = await pickGameFolder();
    if (folder) await handleIngestPathsFlow([folder]);
  });
  el("btn-empty-find-game")?.addEventListener("click", () => {
    switchTab("catalog");
    syncCatalogFilterControlsToDom();
    if (!state.catalog.loaded && !state.catalog.loading) {
      loadCatalogData({ query: state.catalog.searchQuery, forceRefresh: false });
    } else if (state.catalog.renderedCount === 0 && !state.catalog.loading) {
      renderCatalogView(true);
    } else {
      updateCatalogResultsSummary();
    }
    el("input-catalog-search")?.focus();
  });
  el("btn-empty-paste-link")?.addEventListener("click", async () => {
    const clip = (await readSystemClipboardText()).trim();
    if (isValidDownloadUrl(clip)) {
      await startDownloadFromUrl(clip, true);
    } else {
      switchTab("downloader");
      renderDownloadJobs();
      el("input-dl-url")?.focus();
      showToast("Copy or paste a game download link, then press Queue Download.", "info", 3200);
    }
  });
  el("card-empty-find-game")?.addEventListener("click", (e) => {
    if (!e.target.closest("button")) el("btn-empty-find-game")?.click();
  });
  el("card-empty-import-libraries")?.addEventListener("click", (e) => {
    if (!e.target.closest("button")) runImportLibrariesFlow();
  });
  el("card-empty-add-game")?.addEventListener("click", (e) => {
    if (!e.target.closest("button")) el("btn-empty-add-folder")?.click();
  });
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

  el("btn-crash-dismiss")?.addEventListener("click", () => {
    el("crash-heal-banner")?.classList.add("hidden");
    state.lastCrashHealed = null;
  });

  el("btn-crash-relaunch")?.addEventListener("click", async () => {
    const healed = state.lastCrashHealed;
    el("crash-heal-banner")?.classList.add("hidden");
    state.lastCrashHealed = null;
    if (!healed) return;
    const targetGame =
      state.library.find((g) => g.id === healed.gameId) ||
      (healed.exePath
        ? state.library.find((g) => String(g.exePath || "").toLowerCase() === String(healed.exePath).toLowerCase())
        : null);
    if (targetGame) {
      await launchGameById(targetGame);
    } else if (healed.exePath) {
      await launchGameDirectly({
        id: healed.gameId || "",
        title: healed.title || "Game",
        exePath: healed.exePath
      });
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
  el("btn-detail-autofix")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      const btn = el("btn-detail-autofix");
      const summaryEl = el("detail-autofix-summary");
      if (btn) btn.disabled = true;
      showToast(`Running Auto-Fix on ${g.title}...`, "info", 2600);
      try {
        const rep = await autoFixGame(g.id);
        state.library = await invokeCommand("get_local_library");
        const updatedGame = state.library.find((x) => x.id === g.id);
        if (updatedGame && el("detail-toggle-sandbox")) {
          el("detail-toggle-sandbox").checked = Boolean(updatedGame.sandboxEnabled);
        }
        const unresolved = Array.isArray(rep?.unresolvedDlls) ? rep.unresolvedDlls : [];
        if (summaryEl) {
          summaryEl.textContent = rep?.summary || "Auto-Fix finished.";
          summaryEl.classList.remove("hidden");
          summaryEl.classList.toggle("warn", unresolved.length > 0);
        }
        showToast(rep?.summary || "Auto-Fix finished.", unresolved.length > 0 ? "info" : "success", 4200);
      } catch (err) {
        showToast(String(err), "error");
      } finally {
        if (btn) btn.disabled = false;
      }
    })
  );
  el("btn-detail-save-restore-point")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      const btn = el("btn-detail-save-restore-point");
      if (btn) btn.disabled = true;
      showToast(`Saving 1-second restore point for ${g.title}...`, "info", 2200);
      try {
        const stamp = new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
        const created = await createGameSnapshot(g.id, `Restore Point (${stamp})`);
        showToast(
          `Restore point saved in ${Number(created?.elapsedMs || 0)} ms — uses 0 GB extra space.`,
          "success",
          3600
        );
      } catch (err) {
        showToast(String(err), "error");
      } finally {
        if (btn) btn.disabled = false;
      }
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
      showToast("Shrinking game folder size (lossless)...", "info");
      const res = await compressGameFolder(g.id);
      state.library = await invokeCommand("get_local_library");
      openGameDetailView(g.id);
      updateSidebarTelemetry();
      showToast(res?.message || "Disk compression finished.", "success");
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
  el("btn-detail-export-capsule")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      const btn = el("btn-detail-export-capsule");
      const fmt = el("select-capsule-format")?.value || "folder";
      if (btn) btn.disabled = true;
      showToast(`Packaging Portable Capsule (${fmt}) for ${g.title}...`, "info", 2800);
      try {
        const rep = await exportGameCapsule(g.id, null, fmt, true);
        showToast(rep?.summary || `Capsule exported to ${rep?.outputPath || ""}`, "success", 5200);
      } catch (err) {
        showToast(String(err), "error");
      } finally {
        if (btn) btn.disabled = false;
      }
    })
  );
  el("btn-detail-export-dkpack")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      const btn = el("btn-detail-export-dkpack");
      if (btn) btn.disabled = true;
      try {
        const rep = await exportGameDkpack(g.id, null);
        const kb = (Number(rep?.sizeBytes || 0) / 1024).toFixed(1);
        showToast(`Compiled .dkpack (${kb} KB) -> ${rep?.outputPath || ""}`, "success", 4800);
      } catch (err) {
        showToast(String(err), "error");
      } finally {
        if (btn) btn.disabled = false;
      }
    })
  );
  el("btn-detail-apply-dkpack")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      const filePath = await pickDkpackFile();
      if (!filePath) return;
      showToast(`Verifying hashes & applying .dkpack to ${g.title}...`, "info", 2600);
      try {
        const rep = await applyDkpackRecipe(filePath, g.id);
        showToast(rep?.summary || "Modpack recipe applied.", "success", 4600);
        loadDetailModGraph(g, state.activeDetailRequestId);
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

  const syncJailPolicyToggles = () =>
    withSelectedGame(async (g) => {
      const jailEnabled = Boolean(el("detail-toggle-appcontainer-jail")?.checked);
      const offlineMode = Boolean(el("detail-toggle-offline-lock")?.checked);
      try {
        const st = await setGameJailPolicy(g.id, jailEnabled, offlineMode);
        renderDetailJailStatus(st);
        showToast(st?.summary || "Updated AppContainer Jail & Offline Network policy.", "success", 3000);
      } catch (err) {
        showToast(String(err), "error");
        loadDetailJailStatus(g, state.activeDetailRequestId);
      }
    });
  el("detail-toggle-appcontainer-jail")?.addEventListener("change", syncJailPolicyToggles);
  el("detail-toggle-offline-lock")?.addEventListener("change", syncJailPolicyToggles);

  const runCompatFixApply = async (overrideShim = null) => {
    const g = getSelectedGame();
    if (!g) return;
    const shimVal = overrideShim !== null ? overrideShim : (el("select-fix-compat-shim")?.value || "");
    const compatOsShim =
      shimVal === "WIN8RTM"
        ? "win8"
        : shimVal === "WIN7RTM"
          ? "win7"
          : shimVal === ""
            ? "clear"
            : null;
    const highDpiAware = shimVal === "WIN8RTM" || shimVal === "WIN7RTM" || shimVal === "HIGHDPIAWARE";
    try {
      showToast(`Applying fixes with .duke-cas snapshot for ${g.title}...`, "info", 2400);
      const rep = await applyOneClickGameFixes(g.id, {
        createCasBackup: true,
        healRuntimeDlls: overrideShim === null,
        enable4gbLaa: overrideShim === null && Boolean(state.detailCompatStatus?.is32bit),
        toggleDxvk: null,
        highDpiAware,
        compatOsShim
      });
      showToast(rep?.summary || "1-Click repair complete.", "success", 4200);
      loadDetailCompat(g, state.activeDetailRequestId);
      loadDetailTimeMachine(g, state.activeDetailRequestId);
    } catch (err) {
      showToast(String(err), "error");
    }
  };
  el("btn-apply-auto-fixes")?.addEventListener("click", () => runCompatFixApply(null));
  el("select-fix-compat-shim")?.addEventListener("change", (e) => runCompatFixApply(String(e.target.value || "")));
  el("btn-fetch-pcgw-fixes")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      const btn = el("btn-fetch-pcgw-fixes");
      if (btn) btn.disabled = true;
      try {
        const rep = await fetchPcgwFixes(
          g.id,
          g.title,
          Number(g.steamAppId || g.appId || 0) || null,
          false
        );
        renderPcgwFixIntelligence(rep);
        showToast(`Loaded PCGamingWiki fix intelligence for ${g.title}.`, "success", 2600);
      } catch (err) {
        showToast(String(err), "error");
      } finally {
        if (btn) btn.disabled = false;
      }
    })
  );
  const openFixExternal = (url) => {
    if (isValidHttpUrl(url)) openExternalUrl(url);
  };
  el("btn-fix-link-pcgw")?.addEventListener("click", () => openFixExternal(state.detailFixLinks?.pcgamingwikiUrl));
  el("btn-fix-link-protondb")?.addEventListener("click", () => openFixExternal(state.detailFixLinks?.protondbUrl));
  el("btn-fix-link-steam")?.addEventListener("click", () => openFixExternal(state.detailFixLinks?.steamDiscussionsUrl));
  el("btn-fix-link-github")?.addEventListener("click", () => openFixExternal(state.detailFixLinks?.githubIssuesUrl));
  el("detail-prefetch-mode")?.addEventListener("change", (e) =>
    withSelectedGame(async (g) => {
      try {
        const mode = String(e.target.value || "inherit");
        state.library = await setGameLaunchProfile({
          gameId: g.id,
          nvmePrefetchOverride: mode
        });
        showToast(`Fast Startup mode set to "${mode}".`, "success");
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

  el("btn-detail-link-rom")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      try {
        const romPath = await pickRomFile();
        if (!romPath) return;
        await linkGameRom({ gameId: g.id, romPath });
        state.library = await invokeCommand("get_local_library");
        invalidateSidebarIcons([g.id]);
        renderSidebarGameList();
        renderLibraryGrid();
        updateSidebarTelemetry();
        openGameDetailView(g.id);
        showToast("Console ROM / ISO linked.", "success");
      } catch (err) {
        showToast(String(err), "error");
      }
    })
  );

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

  el("btn-refresh-ext-telemetry")?.addEventListener("click", () =>
    withSelectedGame((g) => loadDetailExtendedTelemetry(g, state.activeDetailRequestId, true))
  );

  el("btn-open-pcgw-wiki")?.addEventListener("click", () =>
    withSelectedGame((g) => {
      const targetUrl =
        state.detailPcgwWikiUrl ||
        `https://www.pcgamingwiki.com/w/index.php?search=${encodeURIComponent(g.title)}`;
      openExternalUrl(targetUrl);
    })
  );

  el("btn-refresh-achievements")?.addEventListener("click", () =>
    withSelectedGame((g) => loadDetailAchievements(g, state.activeDetailRequestId))
  );

  el("btn-save-ach-path")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      const customPath = (el("detail-ach-path")?.value || "").trim();
      try {
        state.library = await setGameAchievementsPath(g.id, customPath);
        showToast(customPath ? "Achievement telemetry path saved." : "Achievement path cleared.", "success");
        const updatedGame = state.library.find((x) => x.id === g.id) || g;
        loadDetailAchievements(updatedGame, state.activeDetailRequestId);
      } catch (err) {
        showToast(String(err), "error");
      }
    })
  );

  el("btn-save-update-config")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      const versionTag = (el("detail-version-tag")?.value || "").trim();
      const feedUrl = (el("detail-update-feed-url")?.value || "").trim();
      try {
        state.library = await setGameUpdateConfig(g.id, versionTag, feedUrl);
        const updatedGame = state.library.find((x) => x.id === g.id) || g;
        renderDetailUpdateState(updatedGame, null);
        renderLibraryGrid();
        showToast("Build version & update feed saved.", "success");
      } catch (err) {
        showToast(String(err), "error");
      }
    })
  );

  el("btn-check-game-update")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      showToast(`Checking update feed for ${g.title}...`, "info", 2000);
      try {
        const res = await checkGameUpdates(g.id);
        if (Array.isArray(res?.games)) {
          state.library = res.games;
          renderLibraryGrid();
          renderSidebarGameList();
        }
        const updatedGame = state.library.find((x) => x.id === g.id) || g;
        renderDetailUpdateState(updatedGame, res);
        showToast(
          res?.updateAvailable
            ? `Update available: ${res.latestVersion || "New build"}`
            : `No newer build found (current: ${res?.currentVersion || g.versionTag || "unknown"}).`,
          res?.updateAvailable ? "success" : "info",
          3600
        );
      } catch (err) {
        showToast(String(err), "error");
      }
    })
  );

  el("btn-apply-game-update")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      const btn = el("btn-apply-game-update");
      if (btn) btn.disabled = true;
      showToast("Saving automatic restore point & updating game...", "info", 3200);
      try {
        const res = await applyGameUpdate(g.id);
        if (Array.isArray(res?.games)) {
          state.library = res.games;
          renderLibraryGrid();
          renderSidebarGameList();
          updateSidebarTelemetry();
        }
        const updatedGame = state.library.find((x) => x.id === g.id) || g;
        renderDetailUpdateState(updatedGame, null);
        loadDetailWofStorage(updatedGame, state.activeDetailRequestId);
        showToast(res?.message || `Updated to ${res?.newVersion || "latest"}.`, "success", 4200);
      } catch (err) {
        showToast(String(err), "error");
      } finally {
        if (btn) btn.disabled = false;
      }
    })
  );

  el("btn-toggle-laa")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      const enable = !Boolean(state.detailCompatStatus?.largeAddressAware);
      const btn = el("btn-toggle-laa");
      if (btn) btn.disabled = true;
      showToast(
        enable
          ? "Saving restore point & unlocking 4 GB memory support..."
          : "Saving restore point & restoring original memory limit...",
        "info",
        2400
      );
      try {
        const report = await toggleGameLaaPatch(g.id, enable);
        renderDetailCompat(report);
        showToast(report?.statusMessage || "4 GB memory support updated.", "success", 3600);
      } catch (err) {
        showToast(String(err), "error");
      } finally {
        if (btn) btn.disabled = !state.detailCompatStatus?.is32bit;
      }
    })
  );

  el("btn-toggle-dxvk")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      const enable = !Boolean(state.detailCompatStatus?.dxvkDeployed);
      const btn = el("btn-toggle-dxvk");
      if (btn) btn.disabled = true;
      showToast(
        enable ? "Saving restore point & applying Vulkan graphics fix..." : "Removing graphics fix files...",
        "info",
        2400
      );
      try {
        const report = await toggleGameDxvkWrapper(g.id, enable);
        renderDetailCompat(report);
        showToast(report?.statusMessage || "DXVK wrapper state updated.", "success", 3600);
      } catch (err) {
        showToast(String(err), "error");
      } finally {
        if (btn) btn.disabled = false;
      }
    })
  );

  el("btn-compress-wof")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      const algorithm = el("select-wof-algorithm")?.value || "LZX";
      const btn = el("btn-compress-wof");
      if (btn) btn.disabled = true;
      if (el("detail-wof-progress")) {
        el("detail-wof-progress").textContent = `Compressing with NTFS WOF (${algorithm})...`;
        el("detail-wof-progress").classList.remove("hidden");
      }
      try {
        const res = await compressGameWof(g.id, algorithm);
        if (Array.isArray(res?.library)) {
          state.library = res.library;
          renderLibraryGrid();
          renderSidebarGameList();
          updateSidebarTelemetry();
        }
        loadDetailWofStorage(state.library.find((x) => x.id === g.id) || g, state.activeDetailRequestId);
        showToast(res?.message || "NTFS WOF compression finished.", "success", 4200);
      } catch (err) {
        showToast(String(err), "error");
      } finally {
        if (btn) btn.disabled = false;
      }
    })
  );

  el("btn-decompress-wof")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      const btn = el("btn-decompress-wof");
      if (btn) btn.disabled = true;
      if (el("detail-wof-progress")) {
        el("detail-wof-progress").textContent = "Decompressing NTFS WOF external backing...";
        el("detail-wof-progress").classList.remove("hidden");
      }
      try {
        const res = await decompressGameWof(g.id);
        if (Array.isArray(res?.library)) {
          state.library = res.library;
          renderLibraryGrid();
          renderSidebarGameList();
          updateSidebarTelemetry();
        }
        loadDetailWofStorage(state.library.find((x) => x.id === g.id) || g, state.activeDetailRequestId);
        showToast(res?.message || "WOF decompression complete.", "success", 3800);
      } catch (err) {
        showToast(String(err), "error");
      } finally {
        if (btn) btn.disabled = false;
      }
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
      state.defaultSaveDir = dir;
      localStorage.setItem("duke_launcher_dl_dir", dir);
      if (el("input-dl-dir")) el("input-dl-dir").value = dir;
      updateLauncherSettings({ defaultSaveDir: dir })
        .then((updated) => syncSettingsUiFromBackend(updated))
        .catch(() => {});
    }
  });

  el("btn-start-dl")?.addEventListener("click", () => startDownloadFromUrl());
  el("btn-paste-and-dl")?.addEventListener("click", async () => {
    const clip = (await readSystemClipboardText()).trim();
    if (!isValidDownloadUrl(clip)) {
      showToast("Clipboard does not contain a valid http://, https://, or duke:// download URL.", "error");
      return;
    }
    if (el("input-dl-url")) el("input-dl-url").value = clip;
    await startDownloadFromUrl(clip, false);
  });

  // Global Ctrl+V / Cmd+V Paste-to-Download
  document.addEventListener("keydown", async (e) => {
    if (!(e.ctrlKey || e.metaKey) || e.altKey || e.shiftKey) return;
    if (String(e.key || "").toLowerCase() !== "v") return;
    const active = document.activeElement;
    if (
      active &&
      typeof active.closest === "function" &&
      active.closest("input, textarea, select, [contenteditable], [contenteditable='true']")
    ) {
      return;
    }
    const clip = (await readSystemClipboardText()).trim();
    if (!isValidDownloadUrl(clip)) return;
    e.preventDefault();
    const ok = await startDownloadFromUrl(clip, false);
    if (ok) {
      showToast(`Pasted & queued download: ${truncateClipboardUrl(clip, 56)}`, "success", 3600);
    }
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
    } else if (btn.dataset.repAction === "threat-feed") {
      const sha = btn.dataset.repSha256 || "";
      if (!sha) return;
      try {
        showToast("Checking SHA-256 against read-only threat feeds...", "info", 2200);
        const res = await checkSha256ThreatFeed(sha);
        showToast(
          res?.summary || (res?.flagged ? "Matched known threat hash!" : "Clean across threat feeds."),
          res?.flagged ? "error" : "success",
          4200
        );
      } catch (err) {
        showToast(String(err), "error");
      }
    } else if (btn.dataset.repAction === "unblock") {
      try {
        const count = await unblockMotwPath(path);
        const targetRep = state.reports.find((r) => r.filePath === path);
        if (targetRep) targetRep.zoneUnblocked = true;
        renderSecurityReports();
        showToast(`Stripped Windows :Zone.Identifier from ${Number(count || 1)} file(s).`, "success", 3000);
      } catch (err) {
        showToast(String(err), "error");
      }
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
    if (!isValidDownloadUrl(url)) {
      showToast("Paste a valid http://, https://, or duke:// direct link first.", "error");
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
      showToast("Restored all 8 default game directories.", "info");
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
    showToast("Importing game source feed...", "info", 2400);
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
    applyTheme(card.dataset.themeId, true);
    renderThemes();
    showToast("Theme updated.", "info");
  });

  // Details & Cover Modal Events (Steam CDN & SteamGridDB + Custom .ICO)
  el("btn-close-details-modal")?.addEventListener("click", closeDetailsModal);
  el("details-modal")?.addEventListener("click", (e) => {
    if (e.target === el("details-modal")) closeDetailsModal();
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

  el("btn-search-web-covers")?.addEventListener("click", runWebCoverSearch);
  el("input-cover-query")?.addEventListener("keydown", (e) => {
    if (e.key === "Enter") runWebCoverSearch();
  });

  el("btn-pick-local-cover")?.addEventListener("click", async () => {
    if (!state.modalGameId) return;
    try {
      const imgPath = await pickCustomCoverImage();
      if (!imgPath) return;
      showToast("Applying custom local cover...", "info", 1800);
      const updated = await applyCustomArtwork({ gameId: state.modalGameId, coverUrl: imgPath });
      if (updated && updated.id) {
        const idx = state.library.findIndex((g) => g.id === updated.id);
        if (idx >= 0) state.library[idx] = updated;
      }
      invalidateSidebarIcons([state.modalGameId]);
      renderSidebarGameList();
      renderLibraryGrid();
      if (state.selectedDetailId === state.modalGameId) {
        openGameDetailView(state.modalGameId);
      }
      showToast("Applied custom local cover image.", "success");
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("web-cover-results-container")?.addEventListener("click", async (e) => {
    const card = e.target.closest(".sgdb-thumb-card[data-web-cover-url]");
    if (!card || !state.modalGameId) return;
    const coverUrl = card.dataset.webCoverUrl;
    if (!coverUrl) return;
    showToast("Downloading and caching selected web cover...", "info", 2000);
    try {
      const updated = await applyCustomArtwork({ gameId: state.modalGameId, coverUrl });
      if (updated && updated.id) {
        const idx = state.library.findIndex((g) => g.id === updated.id);
        if (idx >= 0) state.library[idx] = updated;
      }
      invalidateSidebarIcons([state.modalGameId]);
      renderSidebarGameList();
      renderLibraryGrid();
      if (state.selectedDetailId === state.modalGameId) {
        openGameDetailView(state.modalGameId);
      }
      showToast("Applied selected web cover artwork.", "success");
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
    closeDetailsModal();
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
  const closeRestoreModal = closeRestoreSaveModal;
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
    showToast("Saving 1-second restore point...", "info", 2200);
    try {
      const created = await createGameSnapshot(gid, label);
      state.snapshotModalState.snapshots = await listGameSnapshots(gid);
      if (el("input-snapshot-label")) el("input-snapshot-label").value = "";
      renderSnapshotsModalList();
      showToast(
        `Restore point "${created?.label || created?.id}" saved in ${Number(created?.elapsedMs || 0)} ms — 0 GB extra space used.`,
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
          `Restored backup "${rep?.label || snapshotId}" in ${Number(rep?.elapsedMs || 0)} ms (${Number(rep?.hardlinkedFiles || 0)} files linked, ${Number(rep?.removedFiles || 0)} cleaned).`,
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
        showToast("Restore point deleted and unused backup space reclaimed.", "info", 2400);
      } catch (err) {
        showToast(String(err), "error");
      } finally {
        delBtn.disabled = false;
      }
    }
  });

  // Pre-Flight PE32 / PE32+ DLL Auto-Healer Modal Events
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

  // Quick Resume 3-Slot Sidebar Actions
  el("sidebar-qr-list")?.addEventListener("click", async (e) => {
    const switchBtn = e.target.closest("[data-qr-switch]");
    if (switchBtn) {
      const gid = switchBtn.dataset.qrSwitch;
      switchBtn.disabled = true;
      try {
        const rep = await quickSwitchGameSlot(gid);
        showToast(rep?.summary || `Quick switched slot (${rep?.framePacingHzLabel || "0.5 ms"}).`, "success", 3200);
        await refreshQuickResumeSlots();
        await refreshActiveSessions();
      } catch (err) {
        showToast(String(err), "error");
      } finally {
        switchBtn.disabled = false;
      }
      return;
    }
    const remBtn = e.target.closest("[data-qr-remove]");
    if (remBtn) {
      remBtn.disabled = true;
      try {
        const summary = await removeQuickResumeSlot(remBtn.dataset.qrRemove, false);
        renderSidebarQuickResume(summary);
        showToast("Quick Resume slot released.", "info", 2000);
      } catch (err) {
        showToast(String(err), "error");
      } finally {
        remBtn.disabled = false;
      }
    }
  });

  // Deterministic Mod & Proxy-DLL Conflict Graph Actions
  el("btn-detail-scan-mods")?.addEventListener("click", async () => {
    const game = state.library.find((g) => g.id === state.selectedDetailId);
    if (!game) return;
    await loadDetailModGraph(game, state.activeDetailRequestId);
    showToast("Proxy-DLL conflict graph refreshed.", "info", 2000);
  });

  el("btn-detail-add-mod-pack")?.addEventListener("click", async () => {
    const game = state.library.find((g) => g.id === state.selectedDetailId);
    if (!game) return;
    const folder = await pickGameFolder();
    if (!folder) return;
    const packName = folder.split(/[\\/]/).filter(Boolean).pop() || "Mod Pack";
    try {
      const rep = await registerGameModPack({ gameId: game.id, sourceDir: folder, packName });
      renderDetailModGraph(rep);
      showToast(`Registered mod pack "${packName}" into CAS isolation store.`, "success", 3000);
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-detail-save-mod-loadout")?.addEventListener("click", async () => {
    const game = state.library.find((g) => g.id === state.selectedDetailId);
    if (!game) return;
    const loadoutName = (el("input-mod-loadout-name")?.value || "").trim() || `Profile ${new Date().toLocaleTimeString()}`;
    const enabledPackIds = Array.from(
      document.querySelectorAll("#detail-mods-packs-list input[data-mod-pack-id]:checked")
    ).map((cb) => cb.dataset.modPackId);
    try {
      const rep = await saveGameModLoadout({ gameId: game.id, loadoutName, enabledPackIds });
      if (el("input-mod-loadout-name")) el("input-mod-loadout-name").value = "";
      renderDetailModGraph(rep);
      showToast(`Saved loadout "${loadoutName}" (${enabledPackIds.length} packs).`, "success");
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-detail-revert-vanilla")?.addEventListener("click", async () => {
    const game = state.library.find((g) => g.id === state.selectedDetailId);
    if (!game) return;
    try {
      const res = await activateGameModLoadout(game.id, null);
      if (res?.report) renderDetailModGraph(res.report);
      showToast(`Reverted "${game.title}" to vanilla files in ${Number(res?.elapsedMs || 0).toFixed(1)} ms.`, "success", 3200);
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("detail-mods-loadouts-list")?.addEventListener("click", async (e) => {
    const btn = e.target.closest("[data-mod-loadout-apply]");
    if (!btn) return;
    const game = state.library.find((g) => g.id === state.selectedDetailId);
    if (!game) return;
    btn.disabled = true;
    try {
      const res = await activateGameModLoadout(game.id, btn.dataset.modLoadoutApply);
      if (res?.report) renderDetailModGraph(res.report);
      const chainCount = Array.isArray(res?.chainConfigsWritten) ? res.chainConfigsWritten.length : 0;
      showToast(
        `Activated "${res?.loadoutName || "Loadout"}" in ${Number(res?.elapsedMs || 0).toFixed(1)} ms (${Number(res?.hardlinkedFiles || 0)} CAS links, ${chainCount} proxy chains).`,
        "success",
        3800
      );
    } catch (err) {
      showToast(String(err), "error");
    } finally {
      btn.disabled = false;
    }
  });

  el("btn-detail-drop-mod-archive")?.addEventListener("click", async () => {
    const game = state.library.find((g) => g.id === state.selectedDetailId);
    if (!game) return;
    const archivePath = await pickBiosOrPatchFile([
      { name: "Mod Archives (.zip / .7z / .rar)", extensions: ["zip", "7z", "rar"] }
    ]);
    if (!archivePath) return;
    await runModDropperInstall(game, archivePath);
  });

  el("btn-detail-install-workshop-mod")?.addEventListener("click", async () => {
    const game = state.library.find((g) => g.id === state.selectedDetailId);
    if (!game) return;
    const url = (el("input-mod-workshop-url")?.value || "").trim();
    if (!url) {
      showToast("Paste a Steam Workshop URL or ID first.", "info");
      return;
    }
    await runModDropperInstall(game, url);
  });

  el("btn-detail-apply-rom-patch")?.addEventListener("click", async () => {
    const game = state.library.find((g) => g.id === state.selectedDetailId);
    if (!game) return;
    const patchPath = await pickBiosOrPatchFile([
      { name: "ROM Patches", extensions: ["ips", "bps", "ups", "xdelta", "vcdiff"] }
    ]);
    if (!patchPath) return;
    await applyDroppedOrPickedRomPatch(game, patchPath);
  });

  el("btn-sync-retro-achievements")?.addEventListener("click", () =>
    withSelectedGame((g) => loadDetailRetroAchievements(g, state.activeDetailRequestId, true))
  );

  el("btn-detail-clean-uninstall")?.addEventListener("click", () =>
    withSelectedGame((g) => openCleanUninstallModalForGame(g))
  );

  el("btn-close-clean-uninstall-modal")?.addEventListener("click", closeCleanUninstallModal);
  el("btn-cancel-clean-uninstall")?.addEventListener("click", closeCleanUninstallModal);

  el("btn-confirm-clean-uninstall")?.addEventListener("click", async () => {
    const gameId = pendingCleanUninstallGameId;
    if (!gameId) return;
    const keepSaves = Boolean(el("chk-uninstall-keep-saves")?.checked ?? true);
    const removeOrphans = Boolean(el("chk-uninstall-orphans")?.checked ?? true);
    const btn = el("btn-confirm-clean-uninstall");
    try {
      if (btn) btn.disabled = true;
      const rep = await executeCleanUninstall(gameId, keepSaves, removeOrphans);
      closeCleanUninstallModal();
      state.library = await invokeCommand("get_local_library");
      delete state.sidebarIcons[gameId];
      state.sidebarIconsAttempted.delete(gameId);
      state.selectedDetailId = null;
      switchTab("library");
      renderSidebarGameList();
      renderLibraryGrid();
      updateSidebarTelemetry();
      showToast(rep?.summary || "Game cleanly uninstalled.", "success", 4600);
    } catch (err) {
      showToast(String(err), "error");
    } finally {
      if (btn) btn.disabled = false;
    }
  });

  // Friends Drawer Steam CM Chat & Live Game Invites
  el("btn-friends-cm-connect")?.addEventListener("click", async () => {
    try {
      if (state.steamCmStatus?.connected) {
        state.steamCmStatus = await disconnectSteamCm();
        showToast("Disconnected Steam CM WebSocket client.", "info", 2200);
      } else {
        const token = (el("input-steam-cm-token")?.value || "").trim() || null;
        const persona = (el("input-steam-cm-persona")?.value || "").trim() || null;
        state.steamCmStatus = await connectSteamCm({ refreshToken: token, personaName: persona });
        showToast("Steam CM WebSocket handshake active.", "success", 2600);
      }
      renderSteamCmFriendsState();
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("friends-invites-list")?.addEventListener("click", async (e) => {
    const acceptBtn = e.target.closest("[data-steam-invite-accept]");
    const declineBtn = e.target.closest("[data-steam-invite-decline]");
    const btn = acceptBtn || declineBtn;
    if (!btn) return;
    const inviteId = btn.dataset.steamInviteAccept || btn.dataset.steamInviteDecline;
    const accept = Boolean(acceptBtn);
    btn.disabled = true;
    try {
      const launchTarget = await respondSteamGameInvite({ inviteId, accept });
      state.steamPendingInvites = await getSteamPendingInvites();
      renderSteamCmFriendsState();
      if (accept && launchTarget) {
        showToast(`Joining friend invite (${launchTarget})...`, "success", 3000);
      } else {
        showToast(accept ? "Accepted Steam invite." : "Declined Steam invite.", "info", 2000);
      }
    } catch (err) {
      showToast(String(err), "error");
    } finally {
      btn.disabled = false;
    }
  });

  const submitSteamChat = async () => {
    const peerSteamId = (el("input-friends-chat-peer")?.value || "").trim();
    const message = (el("input-friends-chat-msg")?.value || "").trim();
    if (!peerSteamId || !message) {
      showToast("Enter a target SteamID64 and chat message.", "info", 2200);
      return;
    }
    try {
      await sendSteamChatMessage({ peerSteamId, peerName: null, message });
      if (el("input-friends-chat-msg")) el("input-friends-chat-msg").value = "";
      state.steamChatMessages = await getSteamChatMessages(null);
      renderSteamCmFriendsState();
    } catch (err) {
      showToast(String(err), "error");
    }
  };
  el("btn-friends-chat-send")?.addEventListener("click", submitSteamChat);
  el("input-friends-chat-msg")?.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      submitSteamChat();
    }
  });

  // Frontier Systems Engine Settings Controls
  el("btn-frontier-refresh-diag")?.addEventListener("click", async () => {
    await refreshFrontierDiagnostics();
    showToast("Kernel IoRing, GPU HAGS & ProjFS diagnostics updated.", "info", 2200);
  });

  // Suite Autonomy Status Strip
  el("btn-refresh-suite-autonomy")?.addEventListener("click", async () => {
    await refreshSuiteAutonomyStrip();
    showToast("Suite Autonomy daemon bus refreshed.", "info", 1800);
  });

  // Duke Time-Machine: Save Timeline & Branch Scrubber
  el("detail-tm-scrubber-range")?.addEventListener("input", (e) => {
    const idx = Number(e.target.value || 0);
    state.detailTimeMachineSelectedIdx = idx;
    document.querySelectorAll("#detail-tm-timeline-list .tm-node-chip").forEach((node) => {
      node.classList.toggle("active", Number(node.dataset.tmIdx) === idx);
    });
    renderSelectedTimeMachineCheckpoint();
  });

  el("detail-tm-timeline-list")?.addEventListener("click", (e) => {
    const chip = e.target.closest("[data-tm-idx]");
    if (!chip) return;
    const idx = Number(chip.dataset.tmIdx || 0);
    state.detailTimeMachineSelectedIdx = idx;
    if (el("detail-tm-scrubber-range")) el("detail-tm-scrubber-range").value = String(idx);
    document.querySelectorAll("#detail-tm-timeline-list .tm-node-chip").forEach((node) => {
      node.classList.toggle("active", Number(node.dataset.tmIdx) === idx);
    });
    renderSelectedTimeMachineCheckpoint();
  });

  el("btn-detail-tm-checkpoint")?.addEventListener("click", async () => {
    const game = state.library.find((g) => g.id === state.selectedDetailId);
    if (!game) return;
    const btn = el("btn-detail-tm-checkpoint");
    if (btn) btn.disabled = true;
    try {
      const cp = await createTimeMachineCheckpoint(game.id, "Manual Save Point", "manual");
      await loadDetailTimeMachine(game, state.activeDetailRequestId);
      showToast(`Saved Time-Machine checkpoint "${cp?.label || "Snapshot"}" (${cp?.fileCount || 0} files).`, "success", 2600);
    } catch (err) {
      showToast(String(err), "error");
    } finally {
      if (btn) btn.disabled = false;
    }
  });

  el("btn-detail-tm-refresh")?.addEventListener("click", async () => {
    const game = state.library.find((g) => g.id === state.selectedDetailId);
    if (game) await loadDetailTimeMachine(game, state.activeDetailRequestId);
  });

  el("btn-detail-tm-create-branch")?.addEventListener("click", async () => {
    const game = state.library.find((g) => g.id === state.selectedDetailId);
    if (!game) return;
    const branchName = (el("input-detail-tm-branch")?.value || "").trim();
    if (!branchName) {
      showToast("Enter a save branch name first.", "info", 2000);
      return;
    }
    const list = state.detailTimeMachine?.checkpoints || [];
    const selectedCp = list[state.detailTimeMachineSelectedIdx || 0];
    try {
      const rep = await createTimeMachineBranch(game.id, branchName, selectedCp?.id || null);
      if (el("input-detail-tm-branch")) el("input-detail-tm-branch").value = "";
      renderDetailTimeMachine(rep);
      showToast(`Created & checked out save branch "${rep?.currentBranch || branchName}".`, "success", 2800);
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("detail-tm-branch-select")?.addEventListener("change", async (e) => {
    const game = state.library.find((g) => g.id === state.selectedDetailId);
    if (!game) return;
    const targetBranch = e.target.value;
    try {
      const rep = await switchTimeMachineBranch(game.id, targetBranch, true);
      renderDetailTimeMachine(rep);
      showToast(`Switched Time-Machine to branch "${targetBranch}".`, "success", 2400);
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("detail-tm-selected-card")?.addEventListener("click", async (e) => {
    const btn = e.target.closest("[data-tm-action][data-tm-id]");
    if (!btn) return;
    const game = state.library.find((g) => g.id === state.selectedDetailId);
    if (!game) return;
    const action = btn.dataset.tmAction;
    const cpId = btn.dataset.tmId;
    btn.disabled = true;
    try {
      if (action === "restore") {
        const rep = await restoreTimeMachineCheckpoint(game.id, cpId);
        await loadDetailTimeMachine(game, state.activeDetailRequestId);
        showToast(rep?.summary || "Restored save checkpoint.", "success", 3200);
      } else if (action === "delete") {
        const rep = await deleteTimeMachineCheckpoint(game.id, cpId);
        renderDetailTimeMachine(rep);
        showToast("Removed Time-Machine checkpoint.", "info", 2200);
      }
    } catch (err) {
      showToast(String(err), "error");
    } finally {
      btn.disabled = false;
    }
  });

  // Deterministic Input Ghost & Macro Recorder Controls
  el("btn-detail-ghost-record")?.addEventListener("click", async () => {
    const game = state.library.find((g) => g.id === state.selectedDetailId);
    if (!game) return;
    const label = (el("input-detail-ghost-label")?.value || "").trim() || null;
    const syncTm = Boolean(el("chk-detail-ghost-sync-tm")?.checked);
    try {
      const st = await startGhostRecording(game.id, label, syncTm);
      renderDetailGhosts(null, st);
      showToast(st?.statusText || "Recording ~60 Hz delta controller stream.", "info", 2800);
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-detail-ghost-stop")?.addEventListener("click", async () => {
    const game = state.library.find((g) => g.id === state.selectedDetailId);
    if (!game) return;
    const mode = String(state.detailGhostStatus?.mode || "").toLowerCase();
    try {
      if (mode === "recording") {
        const sum = await stopGhostRecording(game.id);
        await loadDetailGhosts(game, state.activeDetailRequestId);
        showToast(
          `Saved .dkghost "${sum?.label || "Ghost"}" (${sum?.fileSizeFormatted || "OK"} · ${Number(sum?.kbPerMinute || 0).toFixed(1)} KB/min).`,
          "success",
          3400
        );
      } else {
        const st = await stopGhostPlayback(game.id);
        renderDetailGhosts(null, st);
        showToast(st?.statusText || "Ghost playback stopped.", "info", 2200);
      }
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-detail-ghost-import")?.addEventListener("click", async () => {
    const game = state.library.find((g) => g.id === state.selectedDetailId);
    if (!game) return;
    try {
      const srcPath = await pickBiosOrPatchFile([
        { name: "Duke Ghost Recording", extensions: ["dkghost"] },
        { name: "All Files", extensions: ["*"] }
      ]);
      if (!srcPath) return;
      const sum = await importGhostFile(game.id, srcPath);
      await loadDetailGhosts(game, state.activeDetailRequestId);
      showToast(`Imported .dkghost "${sum?.label || "Ghost"}" (${sum?.fileSizeFormatted || ""}).`, "success", 3000);
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("detail-ghost-list")?.addEventListener("click", async (e) => {
    const btn = e.target.closest("[data-ghost-action][data-ghost-id]");
    if (!btn) return;
    const game = state.library.find((g) => g.id === state.selectedDetailId);
    if (!game) return;
    const action = btn.dataset.ghostAction;
    const ghostId = btn.dataset.ghostId;
    btn.disabled = true;
    try {
      if (action === "play") {
        const loopMode = Boolean(el("chk-detail-ghost-loop")?.checked);
        const restoreTm = Boolean(el("chk-detail-ghost-sync-tm")?.checked);
        const st = await startGhostPlayback(game.id, ghostId, loopMode, restoreTm);
        renderDetailGhosts(null, st);
        showToast(st?.statusText || "Ghost playback started (pauses automatically on focus loss).", "success", 3200);
      } else if (action === "export") {
        const rawLabel = (btn.dataset.ghostLabel || "ghost").replace(/[^a-zA-Z0-9_-]+/g, "_");
        const destPath = await pickSaveFilePath(`${rawLabel}.dkghost`, [
          { name: "Duke Ghost Recording", extensions: ["dkghost"] }
        ]);
        if (!destPath) return;
        const msg = await exportGhostFile(game.id, ghostId, destPath);
        showToast(msg || "Exported .dkghost.", "success", 3000);
      } else if (action === "delete") {
        await deleteGhostRecording(game.id, ghostId);
        await loadDetailGhosts(game, state.activeDetailRequestId);
        showToast("Deleted .dkghost recording.", "info", 2200);
      }
    } catch (err) {
      showToast(String(err), "error");
    } finally {
      btn.disabled = false;
    }
  });

  // Duke Link: Zero-Latency Couch Co-Op Stream Modal & Controls
  el("btn-open-coop-modal")?.addEventListener("click", openCoopStreamModal);
  el("btn-close-coop-modal")?.addEventListener("click", closeCoopStreamModal);
  el("coop-stream-modal")?.addEventListener("click", (e) => {
    if (e.target === el("coop-stream-modal")) closeCoopStreamModal();
  });

  el("btn-coop-start-host")?.addEventListener("click", async () => {
    const selected = state.library.find((g) => g.id === state.selectedDetailId);
    try {
      const snap = await startCoOpHost({
        gameId: selected?.id || null,
        gameTitle: selected?.title || "Active Game Session"
      });
      renderDukeLinkUi(snap);
      const uri = `dukelink://${snap.advertiseAddr}?code=${encodeURIComponent(snap.roomCode)}`;
      state.dukeLinkInviteUri = uri;
      await writeClipboardText(uri).catch(() => {});
      showToast(`Co-Op Host started (${snap.roomCode}) — link copied to clipboard.`, "success", 3200);
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-coop-join-connect")?.addEventListener("click", async () => {
    const hostAddr = (el("input-coop-join-uri")?.value || "").trim();
    const roomCode = (el("input-coop-room-code")?.value || "").trim() || null;
    if (!hostAddr) {
      showToast("Enter a dukelink:// invite URI or friend Tailscale IP:port.", "info", 2400);
      return;
    }
    try {
      const snap = await joinCoOpStream({ hostAddr, roomCode });
      renderDukeLinkUi(snap);
      startCoopP2InputPump();
      showToast(`Connecting to ${snap?.peerAddr || hostAddr}...`, "info", 2400);
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  const stopCoopHandler = async () => {
    try {
      const snap = await stopCoOpSession();
      renderDukeLinkUi(snap);
      showToast("Duke Link Couch Co-Op session ended.", "info", 2200);
    } catch (err) {
      showToast(String(err), "error");
    }
  };
  el("btn-coop-stop-stream")?.addEventListener("click", stopCoopHandler);
  el("btn-coop-stop-inline")?.addEventListener("click", stopCoopHandler);
  el("btn-coop-copy-invite")?.addEventListener("click", async () => {
    if (state.dukeLinkInviteUri) {
      await writeClipboardText(state.dukeLinkInviteUri).catch(() => {});
      showToast("Copied Duke Link invite URI to clipboard.", "success", 2000);
    }
  });

  window.addEventListener("keydown", (e) => {
    if (state.coopModalOpen && state.dukeLinkStatus?.role === "guest") {
      if (document.activeElement?.tagName !== "INPUT") {
        state.coopKeysDown.add(e.code);
      }
    }
  });
  window.addEventListener("keyup", (e) => {
    state.coopKeysDown.delete(e.code);
  });

  el("btn-frontier-dedup-cas")?.addEventListener("click", async () => {
    const btn = el("btn-frontier-dedup-cas");
    if (btn) btn.disabled = true;
    showToast("Indexing shared Unreal/Unity blocks into .duke-cas...", "info", 2400);
    try {
      const rep = await deduplicateLibraryCas();
      showToast(rep?.summary || "CAS block deduplication finished.", "success", 4200);
    } catch (err) {
      showToast(String(err), "error");
    } finally {
      if (btn) btn.disabled = false;
    }
  });

  el("btn-frontier-purge-shaders")?.addEventListener("click", async () => {
    try {
      const rep = await purgeStaleDriverShaders();
      showToast(
        rep?.driverChanged
          ? `Purged ${rep.filesRemoved} stale shaders (${formatBytesJs(rep.bytesReclaimed)}) after GPU driver update.`
          : `Shader caches clean (${rep?.filesRemoved || 0} stale files removed, ${formatBytesJs(rep?.bytesReclaimed || 0)} reclaimed).`,
        "success",
        3400
      );
      await refreshFrontierDiagnostics();
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-enable-projfs-feature")?.addEventListener("click", async () => {
    const btn = el("btn-enable-projfs-feature");
    if (btn) btn.disabled = true;
    showToast("Enabling Windows Client-ProjFS optional feature...", "info", 3200);
    try {
      const msg = await enableProjfsWindowsFeature();
      showToast(msg || "Client-ProjFS optional feature enabled.", "success", 4000);
      await refreshFrontierDiagnostics();
    } catch (err) {
      showToast(String(err), "error");
    } finally {
      if (btn) btn.disabled = false;
    }
  });

  el("setting-projfs-streaming")?.addEventListener("change", async (e) => {
    try {
      const updated = await updateLauncherSettings({ projfsStreamingEnabled: Boolean(e.target.checked) });
      syncSettingsUiFromBackend(updated);
      showToast(updated?.projfsStreamingEnabled ? "ProjFS instant-play streaming enabled." : "ProjFS instant-play streaming disabled.", "info", 2200);
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("setting-shader-swarm")?.addEventListener("change", async (e) => {
    try {
      const updated = await updateLauncherSettings({ shaderSwarmEnabled: Boolean(e.target.checked) });
      syncSettingsUiFromBackend(updated);
      showToast(updated?.shaderSwarmEnabled ? "Peer Shader Swarm enabled." : "Peer Shader Swarm disabled.", "info", 2200);
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("setting-frame-pacing")?.addEventListener("change", async (e) => {
    const checked = Boolean(e.target.checked);
    try {
      const updated = await updateLauncherSettings({ framePacingEnabled: checked });
      syncSettingsUiFromBackend(updated);
      await toggleHalfMsFramePacing(checked).catch(() => null);
      await refreshFrontierDiagnostics();
      showToast(checked ? "0.5 ms Frame Pacing Runtime enabled." : "0.5 ms Frame Pacing Runtime disabled.", "info", 2200);
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-steam-cm-connect")?.addEventListener("click", async () => {
    const refreshToken = (el("input-steam-cm-token")?.value || "").trim() || null;
    const personaName = (el("input-steam-cm-persona")?.value || "").trim() || null;
    try {
      state.steamCmStatus = await connectSteamCm({ refreshToken, personaName });
      if (el("input-steam-cm-token")) el("input-steam-cm-token").value = "";
      renderSteamCmFriendsState();
      showToast("Connected Steam CM WebSocket client.", "success", 2600);
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-steam-cm-disconnect")?.addEventListener("click", async () => {
    try {
      state.steamCmStatus = await disconnectSteamCm();
      renderSteamCmFriendsState();
      showToast("Disconnected Steam CM client.", "info", 2000);
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-steam-cloud-list")?.addEventListener("click", async () => {
    const appId = Number(el("input-steam-cloud-appid")?.value || 0);
    if (!Number.isFinite(appId) || appId <= 0) {
      showToast("Enter a valid numeric Steam AppID first.", "info");
      return;
    }
    try {
      const files = await listSteamCloudFiles(appId);
      const out = el("frontier-steam-cloud-result");
      if (out) {
        out.textContent =
          Array.isArray(files) && files.length > 0
            ? `AppID ${appId}: ${files.length} Cloud file(s) (${files.slice(0, 4).map((f) => f.filename).join(", ")})`
            : `AppID ${appId}: No public/session Steam Cloud files returned.`;
      }
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-login-gog-direct")?.addEventListener("click", async () => {
    const code = (el("input-gog-oauth-token")?.value || "").trim();
    if (!code) {
      showToast("Paste your GOG authorization code or access token.", "info");
      return;
    }
    try {
      state.storeDirectStatus = await loginGogDirect(code);
      if (el("input-gog-oauth-token")) el("input-gog-oauth-token").value = "";
      renderStoreOwnedTitles();
      showToast("GOG Direct OAuth session saved.", "success");
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-login-epic-direct")?.addEventListener("click", async () => {
    const code = (el("input-epic-oauth-code")?.value || "").trim();
    if (!code) {
      showToast("Paste your Epic authorizationCode first.", "info");
      return;
    }
    try {
      state.storeDirectStatus = await loginEpicDirect(code);
      if (el("input-epic-oauth-code")) el("input-epic-oauth-code").value = "";
      renderStoreOwnedTitles();
      showToast("Epic Direct OAuth session saved.", "success");
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-load-gog-owned")?.addEventListener("click", async () => {
    try {
      state.gogOwnedTitles = await listGogOwnedTitles();
      renderStoreOwnedTitles();
      showToast(`Loaded ${state.gogOwnedTitles.length} GOG title(s).`, "success");
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-load-epic-owned")?.addEventListener("click", async () => {
    try {
      state.epicOwnedTitles = await listEpicOwnedTitles();
      renderStoreOwnedTitles();
      showToast(`Loaded ${state.epicOwnedTitles.length} Epic title(s).`, "success");
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("frontier-owned-titles-list")?.addEventListener("click", async (e) => {
    const btn = e.target.closest("[data-store-install-idx]");
    if (!btn) return;
    const idx = Number(btn.dataset.storeInstallIdx);
    const combined = [...(state.gogOwnedTitles || []), ...(state.epicOwnedTitles || [])];
    const item = combined[idx];
    if (!item) return;
    btn.disabled = true;
    try {
      const rep = await startStoreDirectInstall({
        store: item.store,
        productId: item.productId,
        appName: item.appName || null,
        namespace: item.namespace || null,
        title: item.title,
        saveDir: state.customSaveDir || state.defaultSaveDir || null
      });
      showToast(rep?.summary || `Queued "${item.title}" in Duke AIMD Downloader.`, "success", 3600);
      await refreshDownloadJobs();
      switchTab("downloader");
    } catch (err) {
      showToast(String(err), "error");
    } finally {
      btn.disabled = false;
    }
  });

  // ============================================================================
  // Wave 2 (v0.7.0) Detail & Settings Handlers
  // ============================================================================
  el("btn-detail-xray-refresh")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      await loadDetailXrayScorecards(g, state.activeDetailRequestId);
      showToast("ETW X-Ray session scorecards refreshed.", "success", 2200);
    })
  );

  el("btn-detail-xray-clear")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      try {
        const msg = await clearGameXrayScorecards(g.id);
        if (state.detailXrayLiveTick?.gameId === g.id) state.detailXrayLiveTick = null;
        await loadDetailXrayScorecards(g, state.activeDetailRequestId);
        showToast(msg || "Cleared X-Ray session scorecards.", "success", 2600);
      } catch (err) {
        showToast(String(err), "error");
      }
    })
  );

  el("detail-xray-suggestions")?.addEventListener("click", async (e) => {
    const btn = e.target.closest("[data-xray-action]");
    if (!btn) return;
    const action = btn.dataset.xrayAction || "";
    const game = state.library.find((g) => g.id === (btn.dataset.xrayGameId || state.selectedDetailId));
    if (!game) return;
    btn.disabled = true;
    try {
      if (action === "sync_shader_swarm") {
        await syncGameShadersFromSwarm(game.id).catch(() => {});
        const msg = await purgeStaleDriverShaders().catch(() => "Purged shader cache.");
        showToast(String(msg || "Synced shaders & cleared stale PSO cache."), "success", 3200);
      } else if (action === "enable_latency_boost" && game.exePath) {
        await applyGameLatencyProfile({
          exePath: game.exePath,
          gpuPreference: 2,
          flipModelUpgrade: true,
          autoHdrEnable: Boolean(state.detailUpscaleStatus?.autoHdr),
          autoSrMode: el("detail-latency-autosr")?.value || "on"
        });
        await loadDetailDisplayTuning(game, state.activeDetailRequestId);
        showToast("Enabled high-priority GPU & Flip-Model latency profile.", "success", 3000);
      } else if (action === "enable_upscaler") {
        await saveGameUpscaleProfile({
          gameId: game.id,
          exePath: game.exePath || "",
          borderlessFlipPromote: true,
          upscaleMode: "auto_sr",
          fsrSharpness: 0.75,
          fpsCap: Number(el("detail-upscale-fps-cap")?.value || 0) || 0,
          autoHdr: Boolean(state.detailUpscaleStatus?.autoHdr),
          emulatorId: game.emulatorId || null
        });
        await loadDetailDisplayTuning(game, state.activeDetailRequestId);
        showToast("Enabled Flip-Model Borderless & Auto-SR Upscaling.", "success", 3000);
      } else if (action === "trim_ram") {
        const tele = await invokeCommand("trim_working_set");
        updateSidebarTelemetry(tele);
        showToast(`Trimmed background working set (${Number(tele?.workingSetMb || 0).toFixed(1)} MB).`, "success", 2800);
      } else if (action === "open_fix_finder") {
        const diag = await analyzeGameCrashOrPreflight(game.id, null, null);
        showToast(diag?.plainEnglishCause || "Opened Fix Finder diagnostics.", "info", 3400);
        el("detail-compat-card")?.scrollIntoView({ behavior: "smooth", block: "nearest" });
      }
    } catch (err) {
      showToast(String(err), "error");
    } finally {
      btn.disabled = false;
    }
  });

  el("btn-detail-save-display-tuning")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      const btn = el("btn-detail-save-display-tuning");
      if (btn) btn.disabled = true;
      try {
        const borderlessFlipPromote = Boolean(el("detail-upscale-borderless")?.checked);
        const upscaleMode = el("detail-upscale-mode")?.value || "off";
        const fpsCap = Math.max(0, Number(el("detail-upscale-fps-cap")?.value || 0) || 0);
        const autoHdr = Boolean(el("detail-upscale-autohdr")?.checked);
        const sharpEl = el("detail-upscale-sharpness");
        const rawSharp = Number(sharpEl?.value ?? 0.75);
        const fsrSharpness = Number(sharpEl?.max || 1) > 1.5 ? rawSharp / 100 : rawSharp;
        const gpuPreference = Number(el("detail-latency-gpu-pref")?.value ?? 2);
        const flipModelUpgrade = Boolean(el("detail-latency-flip-upgrade")?.checked);
        const autoSrMode = el("detail-latency-autosr")?.value || "off";

        await saveGameUpscaleProfile({
          gameId: g.id,
          exePath: g.exePath || "",
          borderlessFlipPromote,
          upscaleMode,
          fsrSharpness,
          fpsCap,
          autoHdr,
          emulatorId: g.emulatorId || null
        });
        if (g.exePath) {
          await applyGameLatencyProfile({
            exePath: g.exePath,
            gpuPreference,
            flipModelUpgrade,
            autoHdrEnable: autoHdr,
            autoSrMode
          });
        }
        await loadDetailDisplayTuning(g, state.activeDetailRequestId);
        refreshWave2Diagnostics().catch(() => {});
        showToast("Display upscaler, frame pacer & HKCU Auto-SR profile saved.", "success", 3200);
      } catch (err) {
        showToast(String(err), "error");
      } finally {
        if (btn) btn.disabled = false;
      }
    })
  );

  el("btn-detail-promote-borderless")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      const btn = el("btn-detail-promote-borderless");
      if (btn) btn.disabled = true;
      try {
        const pids = (state.activeSessions || [])
          .filter((s) => s.gameId === g.id || (g.exePath && s.exePath === g.exePath))
          .map((s) => Number(s.pid))
          .filter((pid) => Number.isFinite(pid) && pid > 0);
        const res = await promoteGameWindowBorderless(pids);
        showToast(res?.notes || `Promoted ${res?.promotedWindows || 0} window(s).`, "success", 3200);
      } catch (err) {
        showToast(String(err), "error");
      } finally {
        if (btn) btn.disabled = false;
      }
    })
  );

  el("btn-detail-reset-latency-reg")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      if (!g.exePath) return;
      try {
        const msg = await removeGameLatencyProfile(g.exePath);
        await loadDetailDisplayTuning(g, state.activeDetailRequestId);
        refreshWave2Diagnostics().catch(() => {});
        showToast(msg || "Reset per-exe Windows DirectX registry overrides.", "success", 3000);
      } catch (err) {
        showToast(String(err), "error");
      }
    })
  );

  el("btn-detail-ds-scan")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      const btn = el("btn-detail-ds-scan");
      if (btn) btn.disabled = true;
      try {
        const scan = await scanDirectStorageAssets(g.id, null);
        renderDetailDirectStorage(scan);
        showToast(
          `Found ${scan?.candidates?.length || 0} asset candidates (${formatBytesJs(
            scan?.eligibleUncompressedBytes || 0
          )} uncompressed).`,
          "success",
          3000
        );
      } catch (err) {
        showToast(String(err), "error");
      } finally {
        if (btn) btn.disabled = false;
      }
    })
  );

  el("btn-detail-ds-repack")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      const btn = el("btn-detail-ds-repack");
      if (btn) btn.disabled = true;
      showToast("Creating .duke-cas restore point & repacking Zstd DirectStorage streams...", "info", 3200);
      try {
        const rep = await repackGameDirectStorageAssets(g.id, 3, null, null);
        const scan = await scanDirectStorageAssets(g.id, null).catch(() => null);
        if (scan) renderDetailDirectStorage(scan);
        showToast(
          `Packed ${rep?.filesPacked || 0} stream(s), saved ${rep?.savedFormatted || "0 B"} (${rep?.activeBackend || "DirectStorage"}).`,
          "success",
          4200
        );
      } catch (err) {
        showToast(String(err), "error");
      } finally {
        if (btn) btn.disabled = false;
      }
    })
  );

  el("btn-detail-ds-verify")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      const btn = el("btn-detail-ds-verify");
      if (btn) btn.disabled = true;
      try {
        const rep = await verifyDirectStorageStreams(g.id);
        if (el("detail-ds-readout")) {
          el("detail-ds-readout").textContent = `Verified ${rep?.streamsValid || 0}/${
            rep?.streamsChecked || 0
          } streams (${rep?.chunksVerified || 0} chunks · ${rep?.elapsedMs || 0} ms)`;
        }
        showToast(
          rep?.allValid
            ? `All ${rep?.streamsChecked || 0} DirectStorage Zstd streams verified OK.`
            : `${rep?.streamsCorrupt?.length || 0} stream(s) failed verification.`,
          rep?.allValid ? "success" : "error",
          3400
        );
      } catch (err) {
        showToast(String(err), "error");
      } finally {
        if (btn) btn.disabled = false;
      }
    })
  );

  el("btn-detail-vhdx-containerize")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      const btn = el("btn-detail-vhdx-containerize");
      if (btn) btn.disabled = true;
      showToast("Packing multi-file assets into .vhdx container with IoRing...", "info", 3200);
      try {
        const rep = await containerizeGameToVhdx(g.id, null);
        const st = await getVhdxStageStatus(g.id).catch(() => null);
        if (st) renderDetailVhdxStage(st);
        showToast(
          rep?.notes || `Containerized ${rep?.smallFilesConsolidated || 0} small files into VHDX.`,
          "success",
          4200
        );
      } catch (err) {
        showToast(String(err), "error");
      } finally {
        if (btn) btn.disabled = false;
      }
    })
  );

  el("btn-detail-vhdx-stage")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      const btn = el("btn-detail-vhdx-stage");
      if (btn) btn.disabled = true;
      try {
        const autoStage = Boolean(el("detail-vhdx-autostage")?.checked);
        const rep = await stageGameHotTier(g.id, 4096, autoStage);
        const st = await getVhdxStageStatus(g.id).catch(() => null);
        if (st) renderDetailVhdxStage(st);
        showToast(
          rep?.statusMessage || `Staged ${rep?.filesTransferred || 0} files (${rep?.bytesFormatted || "0 B"}).`,
          "success",
          3800
        );
      } catch (err) {
        showToast(String(err), "error");
      } finally {
        if (btn) btn.disabled = false;
      }
    })
  );

  el("btn-detail-vhdx-unstage")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      const btn = el("btn-detail-vhdx-unstage");
      if (btn) btn.disabled = true;
      try {
        const msg = await unstageOrDetachVhdx(g.id);
        const st = await getVhdxStageStatus(g.id).catch(() => null);
        if (st) renderDetailVhdxStage(st);
        showToast(msg || "Detached VHDX & cleared hot-tier stage.", "success", 3200);
      } catch (err) {
        showToast(String(err), "error");
      } finally {
        if (btn) btn.disabled = false;
      }
    })
  );

  el("detail-vhdx-autostage")?.addEventListener("change", () =>
    withSelectedGame(async (g) => {
      if (!state.detailVhdxState?.[1]?.hotStaged) return;
      try {
        await stageGameHotTier(g.id, 4096, Boolean(el("detail-vhdx-autostage")?.checked));
      } catch (err) {
        showToast(String(err), "error");
      }
    })
  );

  el("btn-detail-shrink-scan")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      const btn = el("btn-detail-shrink-scan");
      if (btn) btn.disabled = true;
      try {
        const rep = await scanGameDeepShrink(g.id, ["english"]);
        renderDetailDeepShrink(rep);
        showToast(
          `Deep Shrink found ${rep?.totalReclaimableFormatted || "0 B"} reclaimable.`,
          "success",
          3200
        );
      } catch (err) {
        showToast(String(err), "error");
      } finally {
        if (btn) btn.disabled = false;
      }
    })
  );

  el("btn-detail-shrink-apply")?.addEventListener("click", () =>
    withSelectedGame(async (g) => {
      const btn = el("btn-detail-shrink-apply");
      if (btn) btn.disabled = true;
      const stripLanguages = Array.from(
        document.querySelectorAll('#detail-shrink-buckets input[type="checkbox"][data-shrink-lang]:checked')
      ).map((inp) => inp.dataset.shrinkLang);
      const dedupInternalMedia = Boolean(el("detail-shrink-dedup-media")?.checked);
      const conditionBler = el("detail-shrink-bler-textures")
        ? Boolean(el("detail-shrink-bler-textures").checked)
        : true;
      showToast("Saving .duke-cas restore point & running Deep Shrink + BLER pass...", "info", 3200);
      try {
        const rep = await applyGameDeepShrink(g.id, stripLanguages, dedupInternalMedia, ["english"], conditionBler);
        await refreshAllData().catch(() => {});
        const rescan = await scanGameDeepShrink(g.id, ["english"]).catch(() => null);
        if (rescan) renderDetailDeepShrink(rescan);
        loadDetailWofStorage(g, state.activeDetailRequestId);
        showToast(
          rep?.message || `Reclaimed ${rep?.reclaimedFormatted || "0 B"} cleanly.`,
          "success",
          4400
        );
      } catch (err) {
        showToast(String(err), "error");
      } finally {
        if (btn) btn.disabled = false;
      }
    })
  );

  el("btn-wave2-refresh-diag")?.addEventListener("click", async () => {
    await refreshWave2Diagnostics();
    showToast("Wave 2 engine diagnostics refreshed.", "success", 2200);
  });

  el("btn-wave2-driver-guard-sync")?.addEventListener("click", async () => {
    const btn = el("btn-wave2-driver-guard-sync");
    if (btn) btn.disabled = true;
    try {
      const rep = await runDriverProfileGuard(true);
      await refreshWave2Diagnostics();
      showToast(
        `Driver Profile Guard synced ${rep?.reappliedCount || 0} .exe profile(s) on driver ${
          rep?.currentDriver || "current"
        }.`,
        "success",
        3400
      );
    } catch (err) {
      showToast(String(err), "error");
    } finally {
      if (btn) btn.disabled = false;
    }
  });

  el("btn-wave2-enforce-shader-cap")?.addEventListener("click", async () => {
    const btn = el("btn-wave2-enforce-shader-cap");
    if (btn) btn.disabled = true;
    try {
      const capMb = Math.max(256, Number(el("input-wave2-shader-cap-mb")?.value || 4096) || 4096);
      const rep = await enforceShaderCacheCap(capMb);
      await refreshWave2Diagnostics();
      showToast(
        `Enforced ${rep?.capMb || capMb} MB shader cache cap · freed ${rep?.freedFormatted || "0 B"} (${
          rep?.filesPruned || 0
        } files).`,
        "success",
        3800
      );
    } catch (err) {
      showToast(String(err), "error");
    } finally {
      if (btn) btn.disabled = false;
    }
  });

  el("btn-wave2-library-blake3-dedup")?.addEventListener("click", async () => {
    const btn = el("btn-wave2-library-blake3-dedup");
    if (btn) btn.disabled = true;
    showToast("Scanning game folders for cross-folder BLAKE3 duplicates...", "info", 3200);
    try {
      const extraRoot = (el("input-wave2-dedup-root")?.value || "").trim() || null;
      const rep = await deduplicateLibraryBlake3(extraRoot, 512);
      if (el("wave2-dedup-status")) {
        el("wave2-dedup-status").textContent = `Scanned ${rep?.foldersScanned || 0} folders (${
          rep?.filesInspected || 0
        } files) · ${rep?.filesHardlinked || 0} hardlinks created · reclaimed ${
          rep?.reclaimedFormatted || "0 B"
        }`;
      }
      await refreshAllData().catch(() => {});
      showToast(
        `BLAKE3 hardlink deduplication reclaimed ${rep?.reclaimedFormatted || "0 B"} across ${
          rep?.filesHardlinked || 0
        } file(s).`,
        "success",
        4400
      );
    } catch (err) {
      showToast(String(err), "error");
    } finally {
      if (btn) btn.disabled = false;
    }
  });

  // -------------------------------------------------------------------------
  // Wave 3 (v0.8.0): Detail View AI Suite (Doctor, Configurator, Director)
  // -------------------------------------------------------------------------
  el("btn-detail-doctor-diagnose")?.addEventListener("click", async () => {
    const game = getSelectedGame();
    if (!game) return;
    const btn = el("btn-detail-doctor-diagnose");
    if (btn) btn.disabled = true;
    showToast("Scanning crash dumps, Unreal/Unity logs & mod load order...", "info", 2800);
    try {
      const rep = await diagnoseGameCrash(game.id, null);
      renderDoctorDiagnosisReport(rep);
      showToast(rep?.summary || "Crash diagnosis complete.", rep?.severity === "critical" ? "error" : "success", 3600);
    } catch (err) {
      showToast(String(err), "error");
    } finally {
      if (btn) btn.disabled = false;
    }
  });

  el("btn-detail-doctor-revert")?.addEventListener("click", async () => {
    const game = getSelectedGame();
    const btn = el("btn-detail-doctor-revert");
    const snapshotId = btn?.dataset.snapshotId;
    if (!game || !snapshotId) return;
    btn.disabled = true;
    try {
      const rep = await doctorRevertGameSnapshot(game.id, snapshotId);
      showToast(
        `Reverted .duke-cas snapshot (${rep?.restoredFiles || 0} files restored).`,
        "success",
        3800
      );
      await loadAllDetailPanels(game, state.activeDetailRequestId);
    } catch (err) {
      showToast(String(err), "error");
    } finally {
      btn.disabled = false;
    }
  });

  el("btn-detail-cfg-analyze")?.addEventListener("click", async () => {
    const game = getSelectedGame();
    if (!game) return;
    const hz = Number(el("select-detail-cfg-hz")?.value || 120) || 120;
    try {
      const plan = await analyzeGameConfigPreset(game.id, hz);
      renderConfiguratorPlanReport(plan);
      showToast(`Synthesized ${plan?.tierLabel || "Balanced"} (${hz} Hz) config preset.`, "info", 2600);
    } catch (err) {
      showToast(String(err), "error");
    }
  });

  el("btn-detail-cfg-apply")?.addEventListener("click", async () => {
    const game = getSelectedGame();
    if (!game) return;
    const btn = el("btn-detail-cfg-apply");
    const hz = Number(el("select-detail-cfg-hz")?.value || 120) || 120;
    if (btn) btn.disabled = true;
    try {
      const res = await applyGameConfigPreset(game.id, hz);
      showToast(res?.message || `Applied ${res?.patchesApplied || 0} INI/XML optimizations.`, "success", 3800);
      const plan = await analyzeGameConfigPreset(game.id, hz).catch(() => null);
      if (plan) renderConfiguratorPlanReport(plan);
    } catch (err) {
      showToast(String(err), "error");
    } finally {
      if (btn) btn.disabled = false;
    }
  });

  el("btn-detail-cfg-rollback")?.addEventListener("click", async () => {
    const game = getSelectedGame();
    const btn = el("btn-detail-cfg-rollback");
    if (!game) return;
    if (btn) btn.disabled = true;
    try {
      const msg = await rollbackGameConfigPreset(game.id, btn?.dataset.backupId || null);
      showToast(msg || "Original config restored cleanly.", "success", 3400);
      const hz = Number(el("select-detail-cfg-hz")?.value || 120) || 120;
      const plan = await analyzeGameConfigPreset(game.id, hz).catch(() => null);
      if (plan) renderConfiguratorPlanReport(plan);
    } catch (err) {
      showToast(String(err), "error");
    } finally {
      if (btn) btn.disabled = false;
    }
  });

  const syncDetailDirectorSettings = async () => {
    const enabled = Boolean(el("chk-detail-director-enabled")?.checked);
    const sensitivity = Number(el("range-detail-director-sens")?.value || 65) || 65;
    if (el("detail-director-sens-val")) el("detail-director-sens-val").textContent = `${sensitivity}%`;
    try {
      const rep = await toggleDukeDirector(enabled, sensitivity);
      renderDirectorStateReport(rep);
    } catch (err) {
      showToast(String(err), "error");
    }
  };
  el("chk-detail-director-enabled")?.addEventListener("change", syncDetailDirectorSettings);
  el("range-detail-director-sens")?.addEventListener("change", syncDetailDirectorSettings);
  el("range-detail-director-sens")?.addEventListener("input", () => {
    if (el("detail-director-sens-val")) {
      el("detail-director-sens-val").textContent = `${el("range-detail-director-sens")?.value || 65}%`;
    }
  });

  el("btn-detail-director-clip")?.addEventListener("click", async () => {
    const game = getSelectedGame();
    const btn = el("btn-detail-director-clip");
    if (!game) return;
    if (btn) btn.disabled = true;
    try {
      const res = await triggerDirectorHighlightNow({
        gameId: game.id,
        gameTitle: game.title,
        targetDurationSecs: 22
      });
      showToast(res?.message || "Highlight clip saved.", "success", 3400);
      const rep = await getDukeDirectorState(game.id).catch(() => null);
      if (rep) renderDirectorStateReport(rep);
    } catch (err) {
      showToast(String(err), "error");
    } finally {
      if (btn) btn.disabled = false;
    }
  });

  // -------------------------------------------------------------------------
  // Wave 3 (v0.8.0): Settings Zero-VRAM NPU & Windows ML AI Engine
  // -------------------------------------------------------------------------
  el("btn-wave3-refresh-ai")?.addEventListener("click", async () => {
    await refreshWave3AiDiagnostics();
    showToast("Probed NPU / WinML / OneOCR & Foundry Local capabilities.", "info", 2400);
  });

  el("btn-wave3-unload-workers")?.addEventListener("click", async () => {
    const btn = el("btn-wave3-unload-workers");
    if (btn) btn.disabled = true;
    try {
      await unloadAiWorkers();
      await refreshWave3AiDiagnostics();
      showToast("Unloaded idle AI workers & trimmed working set.", "success", 2800);
    } catch (err) {
      showToast(String(err), "error");
    } finally {
      if (btn) btn.disabled = false;
    }
  });

  el("btn-wave3-save-config")?.addEventListener("click", async () => {
    const btn = el("btn-wave3-save-config");
    if (btn) btn.disabled = true;
    try {
      await updateAiRuntimeConfig({
        preferredProvider: el("select-wave3-provider")?.value || "auto",
        autoUnloadEnabled: Boolean(el("chk-wave3-auto-unload")?.checked),
        idleTtlSecs: Number(el("input-wave3-idle-ttl")?.value || 45) || 45,
        foundryEndpoint: (el("input-wave3-foundry-endpoint")?.value || "").trim(),
        foundryModel: (el("input-wave3-foundry-model")?.value || "").trim(),
        targetLanguage: el("select-wave3-target-lang")?.value || "en"
      });
      await refreshWave3AiDiagnostics();
      showToast("Zero-VRAM NPU AI policy updated.", "success", 2800);
    } catch (err) {
      showToast(String(err), "error");
    } finally {
      if (btn) btn.disabled = false;
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

  const resetRepeats = () => {
    prev.a = false;
    prev.b = false;
    prev.x = false;
    prev.lb = false;
    prev.rb = false;
    lastMoveTs = 0;
  };

  const setGamepadFocus = (nextEl) => {
    if (focusedEl && focusedEl !== nextEl) {
      focusedEl.classList.remove("gamepad-focus");
    }
    focusedEl = nextEl || null;
    if (focusedEl && document.contains(focusedEl)) {
      state.hasActiveGamepadFocus = true;
      document.body?.setAttribute("data-gamepad-active", "true");
      focusedEl.classList.add("gamepad-focus");
      focusedEl.focus?.({ preventScroll: true });
      focusedEl.scrollIntoView?.({ block: "nearest", inline: "nearest" });
    } else {
      focusedEl = null;
      state.hasActiveGamepadFocus = false;
      document.body?.setAttribute("data-gamepad-active", "false");
    }
  };

  state.gamepadClearFocusFn = () => {
    if (focusedEl) {
      focusedEl.classList.remove("gamepad-focus");
    }
    focusedEl = null;
    state.hasActiveGamepadFocus = false;
  };
  state.gamepadResetRepeatsFn = resetRepeats;

  const getActiveModal = () => {
    const modals = [
      "emu-install-prompt-modal",
      "clean-uninstall-modal",
      "coop-stream-modal",
      "snapshots-modal",
      "preflight-modal",
      "restore-save-modal",
      "add-site-modal",
      "bulk-import-modal",
      "catalog-detail-modal",
      "details-modal"
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
      if (state.couchMode) {
        openVirtualKeyboard(focusedEl);
      } else {
        focusedEl.focus();
      }
      return;
    }
    focusedEl.click();
  };

  const handleBack = () => {
    const modal = getActiveModal();
    if (modal) {
      closeTopmostUiLayer();
      setGamepadFocus(null);
      return;
    }
    if (state.selectedDetailId) {
      el("btn-detail-back")?.click();
      setTimeout(() => {
        const firstCard = document.querySelector("#library-grid .game-card");
        if (firstCard && state.controllerConnected) setGamepadFocus(firstCard);
      }, 40);
      return;
    }
    const navBtn = document.querySelector(".sidebar-nav .nav-item.active");
    if (navBtn) setGamepadFocus(navBtn);
  };

  const cycleTab = (dir) => {
    const modal = getActiveModal();
    if (modal) closeTopmostUiLayer();
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

  const pollBrowserGamepad = () => {
    try {
      const pads = navigator.getGamepads?.() || [];
      for (const gp of pads) {
        if (!gp || !gp.connected) continue;
        const idLow = String(gp.id || "").toLowerCase();
        const isFpv =
          idLow.includes("edgetx") ||
          idLow.includes("opentx") ||
          idLow.includes("radiomaster") ||
          idLow.includes("1209") ||
          idLow.includes("4f54");
        const isPs =
          idLow.includes("dualsense") ||
          idLow.includes("dualshock") ||
          idLow.includes("054c") ||
          idLow.includes("sony");
        const isSwitch =
          idLow.includes("switch") || idLow.includes("057e") || idLow.includes("2dc8") || idLow.includes("8bitdo");
        const rawAxes = [
          Number(gp.axes?.[0] || 0),
          Number(gp.axes?.[1] || 0),
          Number(gp.axes?.[2] || 0),
          Number(gp.axes?.[3] || 0)
        ];
        const dz = Number(el("cal-deadzone-slider")?.value || 14) / 100;
        const axisModes = rawAxes.map((v, i) =>
          isFpv && (i === 1 || i === 2) && v < -0.68 ? "throttle_low" : "centered"
        );
        const norm = (v, mode) => {
          if (mode === "throttle_low") return Math.abs(v) < 0.72 ? v : 0;
          if (Math.abs(v) <= dz) return 0;
          return ((Math.abs(v) - dz) / (1 - dz)) * Math.sign(v);
        };
        const cal = rawAxes.map((v, i) => norm(v, axisModes[i]));
        const btn = (idx) => Boolean(gp.buttons?.[idx]?.pressed);
        return {
          connected: true,
          controllerKind: isFpv ? "edgetx" : isPs ? "dualsense" : isSwitch ? "switch" : "hid",
          deviceName: gp.id || "W3C Gamepad",
          lx: cal[0],
          ly: -cal[1],
          rx: cal[2],
          ry: -cal[3],
          a: btn(0),
          b: btn(1),
          x: btn(2),
          y: btn(3),
          lb: btn(4),
          rb: btn(5),
          up: btn(12),
          down: btn(13),
          left: btn(14),
          right: btn(15),
          rawAxes,
          calibratedAxes: cal,
          axisModes,
          deadzone: dz
        };
      }
    } catch {
      // Browser Gamepad API unavailable
    }
    return null;
  };

  const updateControllerIndicatorUi = (pad) => {
    const ind = el("couch-input-indicator");
    const connected = Boolean(pad?.connected);
    if (ind) {
      ind.classList.toggle("connected", connected);
      ind.title = connected
        ? `${pad.deviceName || pad.controllerKind || "Controller"} connected`
        : "No controller detected";
    }
    if (!state.couchMode) return;

    const cal = Array.isArray(pad?.calibratedAxes)
      ? pad.calibratedAxes
      : [Number(pad?.lx || 0), Number(pad?.ly || 0), Number(pad?.rx || 0), Number(pad?.ry || 0)];
    const raw = Array.isArray(pad?.rawAxes) ? pad.rawAxes : cal;
    const modes = Array.isArray(pad?.axisModes) ? pad.axisModes : [];

    const dotL = el("couch-stick-dot-l");
    const dotR = el("couch-stick-dot-r");
    if (dotL) {
      dotL.style.transform = `translate(calc(-50% + ${(cal[0] * 5).toFixed(1)}px), calc(-50% - ${(cal[1] * 5).toFixed(1)}px))`;
    }
    if (dotR) {
      dotR.style.transform = `translate(calc(-50% + ${(cal[2] * 5).toFixed(1)}px), calc(-50% - ${(cal[3] * 5).toFixed(1)}px))`;
    }

    const pop = el("couch-calibrator-popover");
    if (pop && !pop.classList.contains("hidden")) {
      const devLbl = el("cal-device-label");
      if (devLbl) {
        devLbl.textContent = connected
          ? `${pad.deviceName || "Controller"}${pad.vidPid ? ` (${pad.vidPid})` : ""}`
          : "No active controller";
      }
      for (let i = 0; i < 4; i++) {
        const val = Number(cal[i] ?? raw[i] ?? 0);
        const pct = Math.max(0, Math.min(100, ((val + 1) * 0.5) * 100));
        const barEl = el(`cal-axis-bar-${i}`);
        const modeEl = el(`cal-axis-mode-${i}`);
        const valEl = el(`cal-axis-val-${i}`);
        if (barEl) barEl.style.width = `${pct.toFixed(1)}%`;
        if (modeEl) modeEl.textContent = modes[i] === "throttle_low" ? "Thr Low" : "Centered";
        if (valEl) valEl.textContent = val.toFixed(2);
      }
    }
  };

  const tick = async () => {
    if (document.visibilityState !== "visible") {
      stopPolling();
      return;
    }
    if (inFlight) return;
    inFlight = true;
    try {
      const nativePad = await pollGamepadState().catch(() => null);
      const pad = nativePad?.connected ? nativePad : pollBrowserGamepad() || nativePad;
      const connected = Boolean(pad?.connected);
      state.controllerConnected = connected;
      if (connected !== lastConnected) {
        lastConnected = connected;
        if (connected) {
          const badge = el("titlebar-controller-badge");
          if (badge) badge.classList.remove("hidden");
          updateControllerGlyphLegend(pad?.controllerKind || "xbox");
        } else {
          clearControllerFocus({ disconnect: true, blurNonText: true, resetRepeats: true });
        }
        stopPolling();
        startPolling();
      } else if (connected && pad?.controllerKind && pad.controllerKind !== state.controllerKind) {
        updateControllerGlyphLegend(pad.controllerKind);
      }
      updateControllerIndicatorUi(pad);
      if (!connected) return;

      const lxRaw = Number(pad.lx ?? pad.thumbLx ?? pad.leftStickX ?? 0);
      const lyRaw = Number(pad.ly ?? pad.thumbLy ?? pad.leftStickY ?? 0);
      const rxRaw = Number(pad.rx ?? 0);
      const ryRaw = Number(pad.ry ?? 0);
      const isNormalizedFloat =
        Math.abs(lxRaw) <= 1.05 && Math.abs(lyRaw) <= 1.05 && Math.abs(rxRaw) <= 1.05 && Math.abs(ryRaw) <= 1.05;
      const lx = isNormalizedFloat ? lxRaw : lxRaw / 32768;
      const ly = isNormalizedFloat ? lyRaw : lyRaw / 32768;
      const rx = isNormalizedFloat ? rxRaw : rxRaw / 32768;
      const ry = isNormalizedFloat ? ryRaw : ryRaw / 32768;
      // On EdgeTX Mode-2 FPV transmitters, right stick (Roll/Pitch -> rx/ry) is always self-centering.
      const navX = Math.abs(lx) > 0.42 ? lx : pad.controllerKind === "edgetx" ? rx : 0;
      const navY = Math.abs(ly) > 0.42 ? ly : pad.controllerKind === "edgetx" ? ry : 0;
      const buttons = Number(pad.buttonsMask ?? pad.buttons ?? 0);

      const up = Boolean(pad.dpadUp || pad.up || navY > 0.45 || (buttons & 0x0001));
      const down = Boolean(pad.dpadDown || pad.down || navY < -0.45 || (buttons & 0x0002));
      const left = Boolean(pad.dpadLeft || pad.left || navX < -0.45 || (buttons & 0x0004));
      const right = Boolean(pad.dpadRight || pad.right || navX > 0.45 || (buttons & 0x0008));

      const a = Boolean(pad.a || (buttons & 0x1000));
      const b = Boolean(pad.b || (buttons & 0x2000));
      const x = Boolean(pad.x || (buttons & 0x4000));
      const lb = Boolean(pad.lb || pad.leftShoulder || (buttons & 0x0100));
      const rb = Boolean(pad.rb || pad.rightShoulder || (buttons & 0x0200));

      const now = performance.now();
      if (state.radialOpen) {
        const radX = Math.abs(lx) > 0.35 ? lx : rx;
        const radY = Math.abs(ly) > 0.35 ? ly : ry;
        if (Math.abs(radX) > 0.35 || Math.abs(radY) > 0.35) {
          const deg = ((Math.atan2(radX, radY) * 180) / Math.PI + 360) % 360;
          const slotIdx = Math.round(deg / 45) % 8;
          if (slotIdx !== state.radialSelectedSlot) {
            renderRadialHotbar(null, slotIdx);
          }
        }
        if (a && !prev.a) {
          const activeSlots = getActiveRadialSlots();
          const chosen = activeSlots[state.radialSelectedSlot];
          if (chosen) handleRadialActionDispatch(chosen.actionId ?? chosen.action, chosen.slot ?? chosen.slotId);
        }
        if (b && !prev.b) {
          closeRadialHotbar();
        }
        prev.a = a;
        prev.b = b;
        prev.x = x;
        prev.lb = lb;
        prev.rb = rb;
        return;
      }

      if ((up || down || left || right) && now - lastMoveTs >= (state.couchMode ? 120 : MOVE_DEBOUNCE_MS)) {
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
        toggleCouchMode(!state.couchMode, true);
        showToast(state.couchMode ? "Duke Deck 10-Foot Mode enabled." : "Couch Mode disabled.", "info", 2000);
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

  function getPollIntervalMs() {
    if (state.couchMode || state.radialOpen) return 33;
    return lastConnected ? 33 : 250;
  }

  function startPolling() {
    if (timerId !== null || document.visibilityState !== "visible") return;
    timerId = setInterval(tick, getPollIntervalMs());
  }

  function stopPolling() {
    if (timerId !== null) {
      clearInterval(timerId);
      timerId = null;
    }
  }

  state.gamepadStopPollingFn = stopPolling;
  state.gamepadPollingRateFn = () => {
    stopPolling();
    startPolling();
  };

  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") {
      startPolling();
    } else {
      clearControllerFocus({
        disconnect: !state.controllerConnected,
        blurNonText: false,
        resetRepeats: true
      });
      stopPolling();
    }
  });

  startPolling();
}

async function init() {
  if (new URLSearchParams(window.location.search).get("mode") === "overlay") {
    try {
      await initOverlayHudMode();
    } catch (err) {
      console.error("Overlay HUD init failed:", err);
    }
    return;
  }
  applyTheme(state.activeTheme);
  applyCardZoom(state.cardZoomPx, false);
  applySidebarCollapsed(state.sidebarCollapsed);
  renderThemes();
  renderSources();
  syncCatalogFilterControlsToDom();
  getAppVersion()
    .then((ver) => {
      const pill = el("app-version-pill");
      if (pill && ver) {
        pill.textContent = `v${ver}`;
      }
    })
    .catch(() => {});
  try {
    initWindowControls({
      onCtrlK: () => {
        if (state.activeTab === "catalog" && document.activeElement === el("input-catalog-search")) {
          openEverythingCommandPalette();
          return;
        }
        openEverythingCommandPalette();
      }
    });
  } catch (err) {
    console.error("Window controls init warning:", err);
  }
  wireEvents();
  switchSettingsTab("essentials");
  initDragAndDropIngestion().catch(() => {});

  getLauncherSettings()
    .then((settings) => {
      syncSettingsUiFromBackend(settings);
      applyCouchMode(Boolean(settings?.couchMode), false);
      if (state.autoDetectClipboard) {
        scheduleClipboardDetection();
      }
      refreshSwarmAndMmapTelemetry();
    })
    .catch(() => {});

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

  // Start cached library + controller loop immediately (<100 ms first paint); register all listeners in parallel.
  const initialLibraryPromise = refreshAllData().catch((err) => {
    console.error("Initial refreshAllData failed:", err);
  });

  try {
    startGamepadControllerLoop();
  } catch (err) {
    console.error("Gamepad loop init warning:", err);
  }

  void Promise.allSettled([
    listenDownloadProgress((job) => {
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
    }),
    listenEvent("download-queue-updated", (jobs) => {
      if (Array.isArray(jobs)) {
        state.downloadJobs = jobs;
        renderDownloadJobs();
        updateSidebarTelemetry();
      }
    }),
    listenEvent("download-auto-cleaned", (payload) => {
      if (payload?.downloadId) {
        state.downloadJobs = state.downloadJobs.filter((j) => j.downloadId !== payload.downloadId);
        renderDownloadJobs();
        updateSidebarTelemetry();
      }
    }),
    listenEvent("library-updated", (lib) => {
      if (!Array.isArray(lib)) return;
      const prevList = state.pendingLibraryPayload || state.library || [];
      const prevById = new Map(prevList.map((g) => [g.id, g]));
      let structural = prevById.size !== lib.length;
      for (let idx = 0; idx < lib.length; idx++) {
        const g = lib[idx];
        const prev = prevById.get(g.id);
        if (!prev || prevList[idx]?.id !== g.id) {
          structural = true;
          state.pendingChangedGameIds.add(g.id);
        } else if (
          prev.coverUrl !== g.coverUrl ||
          prev.coverDataUrl !== g.coverDataUrl ||
          prev.dukePlaytimeSecs !== g.dukePlaytimeSecs ||
          prev.importedPlaytimeSecs !== g.importedPlaytimeSecs ||
          prev.playtimeSeconds !== g.playtimeSeconds ||
          prev.lastPlayedTs !== g.lastPlayedTs ||
          prev.lastPlayed !== g.lastPlayed ||
          prev.sizeFormatted !== g.sizeFormatted ||
          prev.isFavorite !== g.isFavorite ||
          prev.title !== g.title ||
          prev.updateAvailable !== g.updateAvailable
        ) {
          state.pendingChangedGameIds.add(g.id);
        }
      }
      state.pendingLibraryStructural = state.pendingLibraryStructural || structural;
      state.pendingLibraryPayload = lib;
      scheduleLibraryCoalescedFlush();
    }),
    listenEvent("cover-updated", (payload) => {
      const gid = payload?.gameId;
      const coverUrl = payload?.coverUrl;
      if (!gid || !coverUrl) return;
      const targetList = Array.isArray(state.pendingLibraryPayload) ? state.pendingLibraryPayload : state.library;
      const match = targetList.find((g) => g.id === gid);
      if (match && match.coverUrl !== coverUrl) {
        match.coverUrl = coverUrl;
        state.pendingChangedGameIds.add(gid);
        scheduleLibraryCoalescedFlush();
      }
    }),
    listenEvent("playtime-synced", (payload) => {
      if (Number(payload?.updatedGames || 0) > 0 && state.libraryPatchTimer === null && state.activeTab === "library") {
        if (state.libraryPatchRaf !== null) cancelAnimationFrame(state.libraryPatchRaf);
        state.libraryPatchRaf = requestAnimationFrame(() => {
          state.libraryPatchRaf = null;
          patchChangedLibraryCards(null);
          updateSidebarTelemetry();
        });
      }
    }),
    listenEvent("catalog-sync-progress", (payload) => {
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
    }),
    listenEvent("memory-telemetry", (tele) => {
      if (!document.hidden) updateSidebarTelemetry(tele);
    }),
    listenEvent("game-running-state", (payload) => {
      document.body.classList.toggle("game-running-idle", Boolean(payload?.running));
      refreshActiveSessions();
    }),
    listenEvent("game-suspend-state", (payload) => {
      refreshActiveSessions();
      if (payload?.message) {
        showToast(payload.message, "info", 2400);
      }
    }),
    listenEvent("nvme-prefetch-report", (payload) => {
      if (!payload || Number(payload.warmedMb || 0) <= 0) return;
      const files = Number(payload.filesWarmed || 0);
      const warmedMb = Number(payload.warmedMb || 0);
      const ms = Number(payload.elapsedMs || 0);
      showToast(
        `Fast Startup pre-loaded ${warmedMb.toFixed(1)} MB (${files} archive${files === 1 ? "" : "s"}) in ${ms} ms.`,
        "success",
        3200
      );
    }),
    listenEvent("directory-download-intercepted", (payload) => {
      const fileLabel = payload?.fileName || "file";
      const siteLabel = payload?.siteTitle || "Safe Browser";
      showToast(`Auto-grabbed "${fileLabel}" from ${siteLabel} -> Downloads queue.`, "success", 3400);
      switchTab("downloader");
      refreshDownloadJobs();
    }),
    listenEvent("grabber-download-redirect", (payload) => {
      const fileLabel = payload?.fileName || "file";
      showToast(`Captured "${fileLabel}" in Duke Stream Grabber -> Downloads queue.`, "success", 3400);
      switchTab("downloader");
      refreshDownloadJobs();
    }),
    listenEvent("sidebar-icons-updated", () => {
      invalidateSidebarIcons();
      renderSidebarGameList();
    }),
    listenEvent("game-crash-detected", (payload) => {
      if (!payload) return;
      state.lastCrashDetected = payload;
      if (
        state.activeTab === "detail" &&
        state.selectedDetailId &&
        payload.gameId === state.selectedDetailId
      ) {
        renderCrashDiagnosisBox(payload);
      }
      showToast(
        `${payload.title || "Game"} exited early (${payload.exitCodeHex || "crash"}): ${payload.faultName || "Open Fix Game for 1-click repair"}`,
        "error",
        5500
      );
    }),
    listenEvent("game-crash-healed", (payload) => {
      if (!payload) return;
      state.lastCrashHealed = payload;
      const banner = el("crash-heal-banner");
      const titleEl = el("crash-heal-title");
      const summaryEl = el("crash-heal-summary");
      if (titleEl) {
        const exitHex = payload.exitCodeHex ? ` (${payload.exitCodeHex})` : "";
        titleEl.textContent = `Repaired ${payload.title || "Game"}${exitHex}:`;
      }
      if (summaryEl) {
        summaryEl.textContent =
          payload.summary || payload.diagnosis || "Unblocked files & repaired missing runtime dependencies.";
      }
      if (banner) {
        banner.classList.remove("hidden");
      }
      showToast(
        `Self-Healed "${payload.title || "game"}" after crash. Click "Repaired — Relaunch Game" to retry.`,
        "success",
        5000
      );
    }),
    listenEvent("duke-uri-download", (payload) => {
      const uriUrl = String(payload?.url || "").trim();
      if (uriUrl) {
        markClipboardUrlHandled(uriUrl);
        showToast(`Queued download from duke:// link: ${truncateClipboardUrl(uriUrl, 54)}`, "success", 3400);
      }
      switchTab("downloader");
      refreshDownloadJobs();
    }),
    listenEvent("achievement-unlocked", (payload) => {
      showAchievementToast(payload);
      if (
        state.activeTab === "detail" &&
        state.selectedDetailId &&
        payload?.gameId === state.selectedDetailId
      ) {
        const currentGame = state.library.find((g) => g.id === state.selectedDetailId);
        if (currentGame) {
          loadDetailAchievements(currentGame, state.activeDetailRequestId);
        }
      }
    }),
    listenEvent("wof-compress-progress", (payload) => {
      if (
        state.activeTab !== "detail" ||
        !state.selectedDetailId ||
        payload?.gameId !== state.selectedDetailId
      ) {
        return;
      }
      const prog = el("detail-wof-progress");
      if (!prog) return;
      if (payload.done) {
        prog.classList.add("hidden");
        const currentGame = state.library.find((g) => g.id === state.selectedDetailId);
        if (currentGame) {
          loadDetailWofStorage(currentGame, state.activeDetailRequestId);
        }
      } else {
        const done = Number(payload.filesProcessed || 0);
        const total = Number(payload.filesTotal || 0);
        prog.textContent = `${payload.algorithm || "WOF"}: ${done.toLocaleString()} / ${total.toLocaleString()} files processed`;
        prog.classList.remove("hidden");
      }
    }),
    listenEmulatorInstallProgress((payload) => {
      if (!payload) return;
      const wrap = el("emu-install-progress-wrap");
      const label = el("emu-install-progress-label");
      const pctEl = el("emu-install-progress-pct");
      const fill = el("emu-install-progress-fill");
      const pct = Math.max(0, Math.min(100, Math.round(Number(payload.progressPct || 0))));
      if (wrap && !el("emu-install-prompt-modal")?.classList.contains("hidden")) {
        wrap.classList.remove("hidden");
      }
      if (label) label.textContent = payload.message || `${payload.consoleName || "Emulator"}: ${payload.stage || "installing"}`;
      if (pctEl) pctEl.textContent = `${pct}%`;
      if (fill) fill.style.width = `${pct}%`;
      const statusLine = el("settings-retro-sync-status");
      if (statusLine && payload.stage !== "ready") {
        statusLine.dataset.syncBusy = "1";
        statusLine.textContent = `${payload.consoleName || "Emulator"} setup (${pct}%): ${payload.message || payload.stage}`;
      } else if (payload.stage === "ready") {
        if (statusLine) delete statusLine.dataset.syncBusy;
        loadEmulatorSettingsSection();
      }
    }),
    listenRetroCatalogSyncProgress((payload) => {
      if (!payload) return;
      const total = Number(payload.totalCatalogEntries || 0);
      if (total > 0) {
        state.catalog.totalCached = total;
        updateNavCatalogBadge();
      }
      const statusLine = el("settings-retro-sync-status");
      const synced = Number(payload.syncedPlatforms || 0);
      const totalPlatforms = Number(payload.totalPlatforms || 0);
      if (statusLine) {
        if (totalPlatforms > 0 && synced < totalPlatforms) {
          statusLine.dataset.syncBusy = "1";
          statusLine.textContent = payload.message || `Syncing ${payload.currentPlatform || "consoles"} (${synced}/${totalPlatforms})...`;
        } else {
          delete statusLine.dataset.syncBusy;
          if (payload.message) statusLine.textContent = payload.message;
        }
      }
      if (state.activeTab === "catalog" && state.catalog.loaded && !state.catalog.loading && total > 0 && synced >= totalPlatforms) {
        runCatalogQuery(true);
      }
    }),
    listenEvent("quick-resume-updated", (summary) => {
      if (summary) renderSidebarQuickResume(summary);
    }),
    listenEvent("projfs-stream-ready", (payload) => {
      if (payload?.message) showToast(payload.message, "success", 3600);
      refreshFrontierDiagnostics().catch(() => {});
    }),
    listenEvent("projfs-mount-changed", () => {
      refreshFrontierDiagnostics().catch(() => {});
    }),
    listenEvent("projfs-mount-unmounted", () => {
      refreshFrontierDiagnostics().catch(() => {});
    }),
    listenEvent("shader-swarm-report", (rep) => {
      if (rep?.summary) showToast(rep.summary, "info", 2800);
      refreshFrontierDiagnostics().catch(() => {});
    }),
    listenEvent("frame-pacing-state", () => {
      refreshFrontierDiagnostics().catch(() => {});
    }),
    listenEvent("steam-cm-state", (cm) => {
      if (cm) {
        state.steamCmStatus = cm;
        renderSteamCmFriendsState();
      }
    }),
    listenEvent("steam-chat-message", (msg) => {
      if (msg) {
        state.steamChatMessages = [...(state.steamChatMessages || []), msg].slice(-80);
        renderSteamCmFriendsState();
        if (!msg.senderIsSelf) {
          showToast(`Steam Chat (${msg.peerName || msg.peerSteamId}): ${String(msg.message || "").slice(0, 60)}`, "info", 3200);
        }
      }
    }),
    listenEvent("steam-game-invite", (inv) => {
      if (inv) {
        state.steamPendingInvites = [
          inv,
          ...(state.steamPendingInvites || []).filter((x) => x.inviteId !== inv.inviteId)
        ];
        renderSteamCmFriendsState();
        showToast(`Game Invite from ${inv.senderName || inv.senderSteamId}: ${inv.gameName || inv.appId}`, "info", 4200);
      }
    }),
    listenEvent("steam-invites-updated", (list) => {
      if (Array.isArray(list)) {
        state.steamPendingInvites = list;
        renderSteamCmFriendsState();
      }
    }),
    listenEvent("time-machine-updated", (gid) => {
      const activeGame = state.library.find((g) => g.id === state.selectedDetailId);
      if (activeGame && activeGame.id === gid) {
        loadDetailTimeMachine(activeGame, state.activeDetailRequestId);
      }
    }),
    listenEvent("duke-link-state", (snap) => {
      if (snap) renderDukeLinkUi(snap);
    }),
    listenEvent("duke-link-frame", (frameEv) => {
      if (state.coopModalOpen) renderCoopFrameOnCanvas(frameEv);
    }),
    listenEvent("etw-xray-tick", (tick) => {
      if (!tick) return;
      state.detailXrayLiveTick = tick;
      if (tick.gameId && tick.gameId === state.selectedDetailId) {
        applyWave2XrayMetrics(tick, true, null);
      }
    }),
    listenEvent("etw-xray-scorecard", (card) => {
      if (!card) return;
      if (state.detailXrayLiveTick?.gameId === card.gameId) {
        state.detailXrayLiveTick = null;
      }
      if (card.gameId && card.gameId === state.selectedDetailId) {
        const g = state.library.find((x) => x.id === card.gameId);
        if (g) loadDetailXrayScorecards(g, state.activeDetailRequestId);
      }
      showToast(
        `X-Ray Scorecard (${card.gameTitle || "Session"}): ${Number(card.avgFps || 0).toFixed(
          1
        )} FPS avg · 1% Low ${Number(card.low1pctFps || 0).toFixed(1)} FPS`,
        "info",
        4200
      );
    }),
    listenEvent("ghost-session-status", (gs) => {
      if (!gs) return;
      if (gs.gameId && gs.gameId === state.selectedDetailId) {
        renderDetailGhosts(null, gs);
      }
    }),
    listenEvent("latency-driver-guard", (rep) => {
      refreshWave2Diagnostics().catch(() => {});
      if (rep?.driverChanged) {
        showToast(
          `GPU driver changed (${rep.previousDriver || "old"} -> ${
            rep.currentDriver || "new"
          }): re-applied ${Number(rep.reappliedCount || 0)} Auto-SR/latency profile(s).`,
          "info",
          4800
        );
      }
    }),
    listenCouchStateChanged((cs) => {
      if (!cs) return;
      if (Array.isArray(cs.radialSlots)) state.radialSlots = cs.radialSlots;
      applyCouchMode(Boolean(cs.active ?? cs.enabled), false, cs.controllerKind);
      if (cs.radialVisible ?? cs.radialOpen) openRadialHotbar(cs.radialSlots);
      else closeRadialHotbar();
    }),
    listenCouchRadialToggled((cs) => {
      if (!cs) return;
      if (Array.isArray(cs.radialSlots)) state.radialSlots = cs.radialSlots;
      if (cs.controllerKind) updateControllerGlyphLegend(cs.controllerKind);
      if (cs.radialVisible ?? cs.radialOpen) openRadialHotbar(cs.radialSlots);
      else closeRadialHotbar();
    }),
    listenCouchRadialAction((evt) => {
      const act = evt?.action ?? evt?.slot?.actionId ?? evt?.slot?.action;
      const slot = evt?.slot?.slot ?? evt?.slot?.slotId ?? (typeof evt?.slot === "number" ? evt.slot : null);
      if (act) {
        handleRadialActionDispatch(act, slot);
      }
    })
  ]);

  getDukeLinkStatus().then(renderDukeLinkUi).catch(() => {});
  await initialLibraryPromise;
  refreshQuickResumeSlots().catch(() => {});
  refreshSteamCmAndInvites().catch(() => {});
  refreshFrontierDiagnostics().catch(() => {});
  refreshWave3AiDiagnostics().catch(() => {});
  listenDukeDirectorHighlight((res) => {
    if (res?.entry) {
      showToast(
        `Duke Director clipped ${res.entry.gameTitle}: ${res.entry.bossOrAchievement} (${Number(res.entry.durationSecs || 22).toFixed(0)}s)`,
        "info",
        3800
      );
    }
  }).catch(() => {});
  listenDukeCommsAction((evt) => {
    if (evt?.phrase) {
      showToast(`Voice Macro: ${evt.phrase} -> ${evt.actionKind}`, "info", 2400);
    }
  }).catch(() => {});
  getCouchModeStatus()
    .then((cs) => {
      if (cs?.radialSlots) state.radialSlots = cs.radialSlots;
      if (cs?.controllerKind) state.controllerKind = cs.controllerKind;
      if (cs?.cliBooted || cs?.active || cs?.enabled) {
        applyCouchMode(true, false, cs.controllerKind);
      } else {
        updateControllerGlyphLegend(state.controllerKind);
      }
    })
    .catch(() => {});
  getFseShellStatus()
    .then((fse) => syncFseShellUi(fse))
    .catch(() => {});
  refreshWasmPluginsUi().catch(() => {});
  listenDkpackApplied((rep) => {
    if (rep?.summary) {
      showToast(rep.summary, "success", 4400);
    }
  }).catch(() => {});
  listenPilotSemanticSearch((q) => {
    if (q) {
      switchTab("catalog");
      if (el("input-catalog-search")) {
        el("input-catalog-search").value = String(q);
        handleCatalogSearchInput();
      }
    }
  }).catch(() => {});
  listenPilotPlayTonight((mins) => {
    if (el("select-play-tonight-mins") && mins) {
      el("select-play-tonight-mins").value = String([30, 60, 120, 240].reduce((a, b) => (Math.abs(b - mins) < Math.abs(a - mins) ? b : a)));
    }
    activatePlayTonightFilter();
  }).catch(() => {});
  Promise.resolve(loadEmulatorSettingsSection()).catch((err) => {
    console.error("Initial loadEmulatorSettingsSection failed:", err);
  });
  Promise.resolve(refreshFriendsPanel(true)).catch((err) => {
    console.error("Initial refreshFriendsPanel failed:", err);
  });
  takePendingProtocolUrls()
    .then((pendingProtocolUrls) => {
      if (Array.isArray(pendingProtocolUrls) && pendingProtocolUrls.length > 0) {
        pendingProtocolUrls.forEach((u) => markClipboardUrlHandled(u));
        switchTab("downloader");
        refreshDownloadJobs();
        const suffix = pendingProtocolUrls.length === 1 ? "" : "s";
        showToast(`Queued ${pendingProtocolUrls.length} download${suffix} from browser link.`, "success", 3400);
      }
    })
    .catch(() => {});
}

init().catch((err) => {
  console.error("Duke Launcher startup failed:", err);
});

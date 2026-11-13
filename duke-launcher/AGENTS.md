# AGENTS.md — AI Coding Agent & Contributor Guide for Duke Launcher

This document is the authoritative system architecture and operational reference for AI coding agents (**Google Antigravity**, **Claude Code**, **Gemini**, **Cursor**, **GitHub Copilot**) and human contributors working on **Duke Launcher**.

---

## 1. Frontier Systems Architecture Overview

**Duke Launcher** is an ultra-low-footprint (~15–25 MB idle working set, 0% idle CPU), zero-telemetry Windows PC game launcher, multi-hoster download manager, and crack-aware security auditor built with **Tauri v2 (Rust 2021 + Tokio)** and **Vanilla ES2022 HTML/CSS/JS** (zero frontend framework or bundler overhead).

### Core Engineering Pillars
1. **Ultra-Low Memory & Windows 11 EcoQoS Hybrid Core Pinning**
   - Raw `kernel32` + `psapi` Windows FFI (`SetPriorityClass`, `SetProcessInformation`, `SetProcessWorkingSetSizeEx`, `K32EmptyWorkingSet`, `GetCompressedFileSizeW`).
   - During gameplay (`launch_game`), Duke Launcher transitions to `BELOW_NORMAL_PRIORITY_CLASS` (`0x00004000`), enables Windows 11 `PROCESS_POWER_THROTTLING_EXECUTION_SPEED` (EcoQoS) so the OS schedules the launcher strictly on Efficiency (E) cores while leaving Performance (P) cores 100% dedicated to the game, minimizes/hides the main window, pauses all frontend timers/animations, and trims its working set to disk.
   - When the game exits, `NORMAL_PRIORITY_CLASS` (`0x00000020`) is restored and session playtime telemetry is persisted.
2. **Multi-Provider DoH IP Racing & Lock-Free 8-Worker Download Matrix**
   - Encrypted DNS-over-HTTPS (DoH) concurrently races queries across **Cloudflare Primary (`1.1.1.1`)**, **Cloudflare Secondary (`1.0.0.1`)**, and **Quad9 (`9.9.9.9:5053`)** using `Accept: application/dns-json`, caching all resolved IPv4 `SocketAddr` endpoints per host for transparent CDN edge IP failover.
   - Up to 8 concurrent workers stream HTTP `Range: bytes=start-end` slices using lock-free `AtomicU64` counters (`Ordering::Relaxed`), pre-allocated 64 KB staging buffers, and 512 KB `tokio::io::BufWriter` segments.
   - **Dynamic Work-Stealing & Adaptive Range Windows**: Starts with 16 MB range slices, scales up to 32 MB on fast unthrottled sockets (`>8 MB/s`), steals remaining byte ranges from slow workers, and recycles any socket averaging `<300 KB/s` over 6 seconds while rotating cached DoH edge IPs.
3. **Zero-RAM Native Multi-Hoster Resolver**
   - Resolves direct binary streams in pure Rust (`0 MB` WebView overhead) for **Buzzheavier** (`bzzhr.to` / `buzzheavier.com` via HTMX `HX-Request: true` on `/{id}/download` reading `hx-redirect`), **GoFile**, **Pixeldrain** (`pixeldrain.com/api/file/{id}?download`), **MediaFire**, **FileDitch**, **1Fichier**, **MegaUp**, **DataNodes**, and **FuckingFast** before falling back to an ephemeral, sandboxed WebView2 sniffer window (`clear_all_browsing_data()`).
4. **Crack-Aware Zero-False-Positive Security Auditor & NTFS Unblocker**
   - `scan_file_security` streams files in 64 KB sliding windows (`Seek`/`Read`) + streaming `sha2::Sha256` (up to 64 MB) without loading multi-gigabyte archives into RAM.
   - **Crack-Aware Whitelist**: Explicitly recognizes legitimate offline game emulators, Steam wrappers, and scene releases (`steam_api.dll`, `steam_api64.dll`, `OnlineFix64.dll`, `Custom.dll`, `winmm.dll`, `Goldberg`, `SteamEmu`, `RUNE`, `TENOKE`, `CODEX`, `FLT`, `ElAmigos`, `Steamless`) as `CLEAN (VERIFIED GAME EMULATOR)` or `FALSE_POSITIVE_WARNING`.
   - **High-Precision Malware Detection**: Flags genuine threats (`DANGER`) for archive disguise attacks (`.rar`/`.7z`/`.zip` starting with `MZ` `4D 5A` or HTML/script payloads), double extensions (`.rar.exe`, `.scr`, `.hta`, `.vbs`, `.cmd`, `.bat`, `.ps1`), and small standalone `.exe` binaries (`<15 MB`) containing combined infostealer signatures (`CryptUnprotectData` + `WinHttpSendRequest`/`InternetOpenUrl` + browser `Login Data` / `Local State` / `wallet.dat` / Discord token exfiltration or hidden `-enc` PowerShell droppers).
   - **Automatic NTFS Mark-of-the-Web (`:Zone.Identifier`) Unblocker**: Automatically strips `:Zone.Identifier` alternate data streams on verified files and extracted game folders so Windows SmartScreen never blocks or slows down first game launch.
5. **Maximum Network & Process Anonymity + Portable Mode**
   - Outbound requests enforce `DNT: 1`, `Sec-GPC: 1`, stripped `Sec-CH-UA` client hints, rotated privacy User-Agents, and ephemeral WebView2 session wiping.
   - Launched games receive anti-telemetry environment variables (`DO_NOT_TRACK=1`, `SENTRY_DSN=`, `UNITY_DISABLE_ANALYTICS=1`, `UE_DISABLE_CRASH_REPORTER=1`, `STEAM_FRAME_FORCE_CLOSE=1`).
   - **Portable Mode**: If `portable.flag` or a `data/` directory exists alongside `duke-launcher.exe`, all state (`library.json`, `downloads.json`, `covers/`, and `backups/Duke_Save_Backups/`) is stored self-contained inside `data/` with zero `AppData` footprint.

---

## 2. Repository File Map

```text
game-launcher/
├── AGENTS.md                  # AI coding agent & contributor architecture guide (this file)
├── README.md                  # Open-source user & developer documentation
├── package.json               # Tauri CLI scripts (dev, build)
├── .gitignore                 # Clean git exclusions (target/, node_modules/, data/, covers/, etc.)
├── src/                       # Static Vanilla ES2022 Frontend (served directly by Tauri)
│   ├── index.html             # Single-page application shell, sidebar telemetry footer, modals
│   ├── styles.css             # GPU-composited dark UI, CSS virtualization (content-visibility: auto)
│   ├── main.js                # UI state machine, debounced search, Steam Detail View, event listeners
│   └── api.js                 # Tauri v2 IPC bridge (window.__TAURI__.core.invoke & event.listen)
└── src-tauri/                 # Rust Backend (Tauri v2 + Tokio)
    ├── Cargo.toml             # Rust dependencies & release LTO/strip profile
    ├── build.rs               # Tauri build script
    ├── tauri.conf.json        # Window configuration, CSP, WebView2 browser args
    ├── capabilities/          # Tauri v2 permission capabilities
    ├── icons/                 # Application icons (.ico, .png)
    └── src/
        └── main.rs            # Complete Rust backend: FFI, DoH, download matrix, security scanner, IPC
```

---

## 3. Tauri IPC Command Table (`#[tauri::command]` ↔ `src/api.js`)

All backend commands are defined in `src-tauri/src/main.rs` and wrapped in `src/api.js`:

| Rust `#[tauri::command]` | `src/api.js` Wrapper | Parameters | Return Type | Description |
| :--- | :--- | :--- | :--- | :--- |
| `get_local_library` | `getLocalLibrary()` | *(none)* | `Result<Vec<GameEntry>, String>` | Loads `library.json`, hydrates disk-cached covers from `covers/`, and triggers background Steam price sync if needed. |
| `add_local_game` | `addLocalGame(exePath, customTitle, customCover, genre)` | `exe_path`, `custom_title`, `custom_cover`, `genre` | `Result<GameEntry, String>` | Indexes a local `.exe`, computes physical disk size, fetches verified Steam/Bing cover art, unblocks NTFS `Zone.Identifier`, and saves to `library.json`. |
| `remove_local_game` | `removeLocalGame(id)` | `id: String` | `Result<Vec<GameEntry>, String>` | Removes a game entry from `library.json` (without deleting game files on disk). |
| `toggle_favorite_game` | `toggleFavoriteGame(id)` | `id: String` | `Result<Vec<GameEntry>, String>` | Toggles `is_favorite` for a game entry. |
| `change_game_exe` | `changeGameExe(gameId, newExePath)` | `game_id: String`, `new_exe_path: Option<String>` | `Result<Vec<GameEntry>, String>` | Opens native file picker (or applies `new_exe_path`) to switch which `.exe` a game launches (DX11/DX12/Vulkan/Mod). |
| `set_game_launch_args` | `setGameLaunchArgs(gameId, launchArgs)` | `game_id: String`, `launch_args: Option<String>` | `Result<Vec<GameEntry>, String>` | Saves custom CLI launch arguments (e.g., `-dx11 -novid -high`) to `GameEntry.launch_args`. |
| `compress_game_folder` | `compressGameFolder(gameId)` | `game_id: String` | `Result<Value, String>` | Runs `compact.exe /c /s:"<game_folder>" /a /i /exe:xpress8k` silently (`CREATE_NO_WINDOW`), recalculates physical size via `GetCompressedFileSizeW`, and updates `library.json`. |
| `launch_game` / `launch_local_game` | `launchLocalGame(exePath, gameId, launchArgs)` | `exe_path`, `game_id`, `launch_args` | `Result<(), String>` | Strips `:Zone.Identifier`, injects anti-telemetry env vars, passes CLI `launch_args`, activates EcoQoS + `BELOW_NORMAL_PRIORITY_CLASS`, and tracks playtime until exit. |
| `get_game_steam_details` | `getGameSteamDetails(appId, title, installDir, gameId)` | `app_id`, `title`, `install_dir`, `game_id` | `Result<GameSteamDetails, String>` | Queries Steam Store, ISteamUserStats (live player count), and ISteamNews in parallel via DoH. |
| `sync_library_steam_prices` | `syncLibrarySteamPrices()` | *(none)* | `Result<Vec<GameEntry>, String>` | Backfills `steam_price_cents` and `steam_price_formatted` for library games to power the `MONEY SAVED` counter. |
| `backup_game_saves` | `backupGameSaves({ gameId, title, installDir, exePath })` | `game_id`, `title`, `install_dir`, `exe_path` | `Result<String, String>` | Archives discovered save folders/files into a pure-Rust `.zip` inside `Duke_Save_Backups/` and rotates up to the last 5 snapshots per game. |
| `open_game_folder` | `openGameFolder(path)` | `path: String` | `Result<(), String>` | Opens the game installation directory in Windows Explorer. |
| `get_directory_size` | `getDirectorySize(path)` | `path: String` | `Result<u64, String>` | Calculates physical on-disk directory byte size using `GetCompressedFileSizeW`. |
| `auto_fix_all_covers` | `autoFixAllCovers()` | *(none)* | `Result<Vec<GameEntry>, String>` | Re-queries Steam/Bing box art for all games and offloads covers to `covers/`. |
| `find_and_apply_cover` | `findAndApplyCover(id, customQuery)` | `id: String`, `custom_query: Option<String>` | `Result<Vec<GameEntry>, String>` | Searches and applies cover art for a single game entry. |
| `enqueue_download` | `enqueueDownload(url, customSaveDir, threads, maxSpeedMbps)` | `url`, `custom_save_dir`, `threads`, `max_speed_mbps` | `Result<Vec<DownloadJob>, String>` | Enqueues a URL into the persistent download matrix (`downloads.json`) and starts the queue scheduler. |
| `pause_download` | `pauseDownload(downloadId)` | `download_id: String` | `Result<Vec<DownloadJob>, String>` | Signals atomic pause flag, flushes `.part.seg*` buffers to disk, and saves byte offsets. |
| `resume_download` | `resumeDownload(downloadId, maxSpeedMbps)` | `download_id`, `max_speed_mbps` | `Result<Vec<DownloadJob>, String>` | Resumes a paused/interrupted download from exact on-disk `.part.seg*` byte offsets. |
| `remove_download_job` | `removeDownloadJob(downloadId, deleteFile)` | `download_id: String`, `delete_file: bool` | `Result<Vec<DownloadJob>, String>` | Stops and removes a download job, optionally deleting `.part.seg0..31`, `.part`, and final files. |
| `get_download_jobs` | `getDownloadJobs(backupJobs)` | `backup_jobs: Option<Vec<DownloadJob>>` | `Result<Vec<DownloadJob>, String>` | Returns all download jobs reconciled with on-disk segment files. |
| `install_downloaded_job` | `installDownloadedJob(downloadId)` | `download_id: String` | `Result<GameEntry, String>` | Extracts a completed archive (with Zip-Slip/bomb protection + NTFS unblocking) and registers the game. |
| `set_speed_limit` | `setSpeedLimit(maxSpeedMbps)` | `max_speed_mbps: Option<f64>` | `Result<(), String>` | Dynamically updates `GLOBAL_SPEED_LIMIT_KBPS` atomic rate limiter across all active workers. |
| `scan_file_security` | `scanFileSecurity(path)` | `path: String` | `Result<SecurityScanReport, String>` | Runs the 64 KB sliding-window PE & heuristic scanner, Crack-Aware Whitelist, Defender check, and NTFS unblocker. |
| `delete_flagged_file` | `deleteFlaggedFile(path)` | `path: String` | `Result<(), String>` | Permanently deletes a file flagged as `DANGER`. |
| `trim_working_set` | `trimWorkingSet()` | *(none)* | `Result<Value, String>` | Invokes Windows `SetProcessWorkingSetSizeEx` + `K32EmptyWorkingSet` and emits `memory-telemetry`. |
| `get_system_telemetry` | `getSystemTelemetry()` | *(none)* | `Result<Value, String>` | Returns live working-set RAM MB, DoH shield state, cached host count, and portable mode status. |

---

## 4. JSON State Schemas

### `GameEntry` (`library.json` / IPC camelCase)
```json
{
  "id": "game_cgameseldenringgameeldenringexe",
  "title": "ELDEN RING",
  "exePath": "C:\\Games\\Elden Ring\\Game\\eldenring.exe",
  "installDir": "C:\\Games\\Elden Ring",
  "sizeBytes": 52428800000,
  "sizeFormatted": "48.83 GB",
  "coverUrl": "data:image/jpeg;base64,...",
  "genre": "Installed",
  "platform": "Windows",
  "isFavorite": true,
  "isInstalled": true,
  "addedAt": 1730000000,
  "lastPlayed": 1730003600,
  "steamAppId": 1245620,
  "appId": 1245620,
  "playtimeSeconds": 14400,
  "lastPlayedTs": 1730003600,
  "steamPriceCents": 5999,
  "steamPriceFormatted": "$59.99",
  "launchArgs": "-dx11 -novid -high"
}
```
> **Note on Cover Caching:** On disk inside `library.json`, `data:image/...` strings are automatically offloaded to `covers/<game_id>.cover` and stored as `covercache://<game_id>.cover` so `library.json` remains tiny and fast to parse. When `load_library_from_disk` runs, covers are transparently hydrated for the frontend.

### `DownloadJob` (`downloads.json` / `download-progress` event)
```json
{
  "downloadId": "dl_1730000000000",
  "originalUrl": "https://buzzheavier.com/f/example",
  "streamUrl": "https://cdn.example.com/archive.rar",
  "resolvedUrl": "https://cdn.example.com/archive.rar",
  "referer": "https://buzzheavier.com/",
  "cookie": "",
  "userAgent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) ...",
  "fileName": "Example_Game.rar",
  "saveDir": "C:\\Users\\Public\\Downloads",
  "partPath": "C:\\Users\\Public\\Downloads\\Example_Game.rar.part",
  "finalPath": "C:\\Users\\Public\\Downloads\\Example_Game.rar",
  "downloadedBytes": 1073741824,
  "totalBytes": 4294967296,
  "speedMbps": 94.5,
  "peakSpeedMbps": 118.2,
  "progressPct": 25.0,
  "elapsedSecs": 12,
  "etaSecs": 34,
  "threadCount": 8,
  "segments": [
    { "index": 0, "start": 0, "end": 16777215, "downloaded": 16777216, "completed": true }
  ],
  "status": "DOWNLOADING",
  "message": "8-Worker Adaptive Matrix • 94.50 MB/s (Peak 118.2 MB/s)",
  "verdict": "SAFE",
  "addedAt": 1730000000
}
```

### `SecurityScanReport` (`scan_file_security` response)
```json
{
  "filePath": "C:\\Games\\Example\\steam_api64.dll",
  "fileName": "steam_api64.dll",
  "sizeBytes": 312832,
  "sizeFormatted": "305.50 KB",
  "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
  "detectedFormat": "Windows PE Executable (.exe/.dll)",
  "verdict": "CLEAN (VERIFIED GAME EMULATOR)",
  "summary": "Verified legitimate offline game emulator / scene release (STEAM_API64.DLL). Whitelisted with zero false positives.",
  "details": [
    "Type: Windows PE Executable (.exe/.dll)",
    "SHA-256: e3b0c44298fc1c149afbf4c8...",
    "Crack-Aware Whitelist: STEAM_API64.DLL verified",
    "NTFS Zone.Identifier: Unblocked for instant SmartScreen-free launch"
  ]
}
```

---

## 5. Critical Engineering Rules & Invariants

1. **WebView2 Multi-Window Rule (`tauri.conf.json`)**:
   - **NEVER** pass `--renderer-process-limit=1` or `--single-process` in `additionalBrowserArgs`. Doing so causes a fatal renderer deadlock when Duke Launcher spawns an auxiliary `WebviewWindow` (`resolver_*`) for fallback hoster sniffing.
2. **Zero Full-File RAM Buffering**:
   - Never call `std::fs::read()` on game archives or `.exe` binaries. Always stream with fixed buffers (`64 KB`–`512 KB`) and `Seek`/`Read`.
3. **Top Bar Cleanliness**:
   - Keep telemetry pills (`DoH SHIELD: ACTIVE`, `RAM STATE: IDLE TRIMMED / GAME BOOST`, `MONEY SAVED`, `DISK USED`, `INSTALLED`) exclusively in the bottom-left sidebar footer (`.sidebar-stats`), never in `.topbar`.
4. **NTFS Compressed Size Accuracy**:
   - Always use `get_physical_file_size_on_disk` (`GetCompressedFileSizeW`) inside `calculate_dir_size_sync` so `COMPRESS DISK` (`compact.exe /exe:xpress8k`) accurately reflects physical SSD space reclaimed.

---

## 6. Build, Verification & Release Pipeline (PowerShell)

### Fast Type & Borrow-Checker Verification
```powershell
cd src-tauri
cargo check
```
*(Must pass with **0 errors and 0 warnings** before committing any change.)*

### Production Release Build (`LTO = true`, `strip = true`, `codegen-units = 1`)
```powershell
npm run tauri build
```
The optimized portable binary and installers are emitted to:
- `src-tauri/target/release/duke-launcher.exe`
- `src-tauri/target/release/bundle/nsis/`
- `src-tauri/target/release/bundle/msi/`

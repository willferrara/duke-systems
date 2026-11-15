# The Duke Suite — Master Engineering & Execution Rules

## 1. Mandatory Upfront Total-Process Time Estimation (EVERY MULTI-FILE PROMPT)
Before executing any file edits, searches, or shell commands on tasks touching 2+ files or crates, ALWAYS output a concise **Execution Plan & Total Time Estimate** block first:
- Break the task into numbered phases (e.g., `Phase 1: Codebase Audit`, `Phase 2: duke-core / Rust Backend`, `Phase 3: Frontend UI & SVG Polish`, `Phase 4: cargo check, cargo test & Binary Audit`).
- Estimate realistic wall-clock minutes per phase across the ENTIRE multi-file workflow (including `cargo check` / `npm` execution), and state **Estimated Total Process Time: ~X–Y min** up front.
- Before starting each phase, print a single status line: `[Phase N/Total | Elapsed: ~Xm | Est. Remaining: ~Ym]`.
- **Calibrated 20-Block Progress Bar:** Accompany phase transitions and progress updates with a calibrated 20-block Unicode progress bar (`█` / `░`) showing exact deliverable completion (`Done/N`, e.g., `Progress: [██████████░░░░░░░░░░] 50% (4/8 deliverables)`), real wall-clock elapsed time, and throughput-calibrated ETA.

## 2. Duke Suite Architecture & Zero-Bloat Standards
- **Workspace Layout:** `duke-core` (shared feature-gated library), `duke-systems` (central command hub), `duke-launcher`, `duke-monitor`, `duke-capture`, `duke-input`.
- **Feature-Gated `duke-core`:** Never enable `net-doh` on daemons like `duke-input` that do not need HTTP/TLS.
- **Release Optimization:** Every `Cargo.toml` must keep `[profile.release]` with `opt-level = 3`, `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, and `strip = true`.
- **WebView2 Multi-Window Safety:** Never set `--renderer-process-limit=1` in `tauri.conf.json`.
- **Open-Source Hygiene:** Never hardcode personal usernames, local IPs, third-party piracy URLs, or `C:\Users\<name>` paths anywhere in code, docs, or configs. Always resolve paths dynamically via environment variables or `%LOCALAPPDATA%` / `%USERPROFILE%`.

## 3. Native Windows 11 UI Standards (Zero AI Slop)
- Use Title Case with `"Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif` for UI labels and `"Cascadia Code", "Consolas", monospace` (`font-variant-numeric: tabular-nums`) strictly for live numeric telemetry and file paths.
- Use inline 1.5px `<symbol>` SVG icon sprites (`#icon-...`) instead of emojis or text-only buttons.
- Preserve all 10 zero-RAM CSS themes (`[data-theme="..."]` / `[data-preview-theme="..."]`) and never inline double-quoted SVGs inside HTML `style=""` attributes.
- When visual UI verification is requested, use the `playwright` MCP server to render `src/index.html` and verify layout alignment and contrast.

## 4. `duke-launcher` Tauri v2 IPC Command Reference

### Core Library, Launch & Utilities
- `get_local_library()` -> `Vec<GameEntry>`
- `add_local_game(exe_path, custom_title, custom_cover, genre)` -> `GameEntry`
- `link_local_game_with_steam_id(exe_path, custom_title, genre, steam_app_id)` -> `GameEntry`
- `remove_local_game(id)` -> `Vec<GameEntry>`
- `toggle_favorite_game(id)` -> `Vec<GameEntry>`
- `change_game_exe(game_id, new_exe_path)` -> `Vec<GameEntry>`
- `set_game_launch_args(game_id, launch_args)` -> `Vec<GameEntry>`
- `launch_game(exe_path, game_id, launch_args)` / `launch_local_game(...)` -> `()`
- `compress_game_folder(game_id)` -> `CompressionSummary`
- `get_game_steam_details(app_id, title, install_dir, game_id)` -> `GameSteamDetails`
- `sync_library_steam_prices()` -> `Vec<GameEntry>`
- `fetch_steam_game_catalog(force_refresh)` -> `CatalogResponse`
- `get_catalog_game_details(app_id)` -> `SteamCatalogEntry`
- `open_game_folder(path)` -> `()`
- `get_directory_size(path)` -> `u64`
- `trim_working_set()` -> `Value`
- `get_system_telemetry()` -> `Value`
- `get_launcher_settings()` -> `LauncherSettings`
- `update_launcher_settings(auto_clean_completed, steamgriddb_api_key, suspend_hotkey, couch_mode, auto_detect_clipboard, nvme_prefetch_enabled, swarm_enabled, swarm_node_name, swarm_peers)` -> `LauncherSettings`
- `set_game_launch_profile(game_id, sandbox_enabled, nvme_prefetch_override)` -> `Vec<GameEntry>`

### Copy-on-Write Snapshots, Mmap Catalog, Delta Patch & Swarm (`snapshots.rs`, `catalog_mmap.rs`, `delta_patch.rs`, `swarm.rs`, `pe_inspect.rs`)
- `create_game_snapshot(game_id, label)` -> `GameSnapshotSummary` (parallel `blake3` 64 KB hashing + `.duke-cas` + Win32 `CreateHardLinkW`)
- `list_game_snapshots(game_id)` / `restore_game_snapshot(game_id, snapshot_id)` / `delete_game_snapshot(game_id, snapshot_id)` (zero-duplication hardlink rollback & orphan blob GC)
- `get_catalog_mmap_stats()` -> `CatalogMmapStats` / `search_catalog_mmap(query, limit)` -> `Vec<CatalogGameEntry>` (`memmap2::Mmap` on `catalog.dkcat` `DKCAT001` archive + trigram index)
- `generate_game_dkmanifest(game_id, out_path)` / `diff_game_against_manifest(game_id, manifest_path)` / `create_delta_patch_file(...)` / `apply_game_delta_patch(...)` (`fastcdc::v2020::StreamCDC` 16–64 KiB + `zstd`)
- `get_swarm_status()` / `probe_swarm_peers()` / `beam_game_from_peer(peer_addr, game_key, dest_parent_dir, register_in_library)` / `sync_game_saves_mesh(game_id)` (UDP multicast `239.255.77.88:47880` + TCP `47881` + Tailscale peers + vector clocks)
- `preflight_check_exe(exe_path)` -> `PePreflightReport` / `heal_game_runtime_dlls(exe_path, dll_names)` -> `DllHealReport` (pure-Rust PE32/PE32+ `.idata` parser & local redistributable auto-heal)

### Community Hydra JSON Sources (`sources.rs` — DMCA Safe-Harbor)
- `get_imported_sources()` -> `Vec<ImportedSourceSummary>`
- `import_source_url(url)` -> `ImportedSourceSummary`
- `import_source_file(file_path)` -> `ImportedSourceSummary`
- `toggle_imported_source(source_id, enabled)` -> `Vec<ImportedSourceSummary>`
- `refresh_imported_source(source_id)` -> `ImportedSourceSummary`
- `refresh_all_imported_sources()` -> `Vec<ImportedSourceSummary>`
- `remove_imported_source(source_id)` -> `Vec<ImportedSourceSummary>`
- `search_imported_sources(query, source_id, offset, limit)` -> `HydraSearchResponse`

### Ludusavi Save-Path Discovery & Restore (`ludusavi.rs`)
- `sync_ludusavi_manifest(force)` -> `LudusaviCacheStatus` (7-day TTL cached in `ludusavi_cache.json`)
- `discover_game_save_paths(game_id, sync_if_missing)` -> `DiscoveredSaveReport` (Manifest + `%APPDATA%` / `%LOCALAPPDATA%` / `Documents\My Games` / `Saved Games` heuristics)
- `backup_game_saves(game_id, title, install_dir, exe_path)` -> `String` (rolling 5 pure-Rust ZIP snapshots in `Duke_Save_Backups`)
- `list_save_backups(game_id)` -> `Vec<SaveBackupSnapshot>`
- `restore_game_save(game_id, backup_path)` -> `String`

### 1-Click Library Importer & Artwork (`importers.rs`, `artwork.rs`)
- `import_existing_libraries()` -> `ImportLibraryResult` (Steam VDF/ACF, Epic `.item`, GOG Registry, Xbox `.GamingRoot` / `XboxGames`)
- `auto_fix_all_covers()` / `find_and_apply_cover(id, custom_query)` -> `Vec<GameEntry>`
- `search_steamgriddb_artwork(query_or_app_id)` -> `SteamGridDbSearchResult`
- `apply_custom_artwork(game_id, cover_url, hero_url, icon_url)` -> `Vec<GameEntry>`
- `create_desktop_shortcut_with_icon(game_id)` -> `String`

### Game Suspend/Resume, Gamepad & Security (`main.rs`)
- `suspend_or_resume_game(game_id, action)` -> `SuspendResult` (`ntdll!NtSuspendProcess` / `NtResumeProcess` over Win32 Job Object process tree + working-set trim)
- `get_active_game_sessions()` -> `Vec<ActiveGameSession>`
- `poll_gamepad_state()` -> `XInputPadState` (lightweight XInput polling strictly when visible)
- `enqueue_download(url, custom_save_dir, threads, max_speed_mbps)` / `pause_download` / `resume_download` / `remove_download_job` / `clear_completed_downloads` / `get_download_jobs` / `install_downloaded_job` / `set_speed_limit`
- `scan_file_security(path)` / `delete_flagged_file(path)` (`SecurityScanReport` with Shannon entropy `0.0-8.0` b/B + PE `IMAGE_NT_HEADERS` analysis)
- `get_search_sites()` / `add_search_site` / `remove_search_site` / `bulk_import_search_sites` / `reset_default_search_sites`
- `read_clipboard_text()` / `write_clipboard_text(text)` / `take_pending_protocol_urls()`

## 5. Headless CLI / MCP Server Mode (`duke-launcher.exe --mcp`)
- Checked on the very first line of `fn main()` before Tauri or WebView2 initializes (`0 MB` GUI RAM).
- Speaks line-delimited JSON-RPC 2.0 over `stdin`/`stdout` supporting `initialize`, `notifications/initialized`, `tools/list`, and `tools/call`.
- Registered MCP Tools (8):
  - `list_installed_games`
  - `search_catalog`
  - `launch_game`
  - `scan_binary`
  - `discover_saves`
  - `backup_saves`
  - `import_libraries`
  - `suspend_or_resume_game`

### Example `mcp_config.json`
```json
{
  "mcpServers": {
    "duke-launcher": {
      "command": "C:\\Program Files\\Duke Launcher\\duke-launcher.exe",
      "args": ["--mcp"]
    }
  }
}
```

## 6. Key JSON Schemas
- **App Data Root (`%LOCALAPPDATA%\com.dukelauncher.desktop\` or `./data` in Portable Mode):**
  - `library.json`: array of `GameEntry` (`id`, `title`, `exePath`, `installDir`, `sizeBytes`, `sizeFormatted`, `coverUrl`, `heroUrl`, `iconPath`, `source`, `steamAppId`, `playtimeSeconds`, `lastPlayedTs`, `launchArgs`, `sandboxEnabled`, `nvmePrefetchEnabled`).
  - `downloads.json`: AIMD pre-allocated download queue (`DownloadJob` with `SegmentChunk` byte spans flushed via positional `seek_write`, or in-flight `.tar.zst` ring-channel extraction / `.zip` HTTP Range EOCD dispatch).
  - `catalog.dkcat`: zero-copy `memmap2` binary catalog (`DKCAT001` header, packed `CatalogRecord` table, UTF-8 string pool, trigram inverted index).
  - `search_sites.json`: user-customized search directory URLs (`{id, name, searchUrl, baseUrl}`) initialized with 6 DMCA-safe preservation defaults.
  - `sources.json`: array of Hydra feeds matching `{ "name": "Source Name", "downloads": [{ "title": "...", "uris": ["https://..."], "uploadDate": "...", "fileSize": "..." }] }`.
  - `ludusavi_cache.json`: normalized save schema cache fetched via `duke-core` DoH with 7-day TTL.
  - `settings.json`: `{ "autoCleanCompleted": true, "steamgriddbApiKey": "", "suspendHotkey": "Ctrl+Alt+P", "couchMode": false, "autoDetectClipboard": true, "nvmePrefetchEnabled": true, "swarmEnabled": false, "swarmNodeName": "...", "swarmPeers": [] }`.

## 7. Spec-Locked Delegation Protocol (`Hydrogen XHigh`)
When `Hydrogen XHigh` orchestrates parallel or multi-crate work using `.agents/agents/` (`rust-engine-builder`, `win11-ui-crafter`, `static-verifier`) and `.agents/rules/duke-invariants.md`, strictly enforce:
- **Disjoint File Ownership:** Never assign two subagents to edit the same file concurrently. `rust-engine-builder` workers edit only isolated backend modules (`duke-launcher/src-tauri/src/<module>.rs` or `duke-core/src/<module>.rs`); `Hydrogen XHigh` wires thin `#[tauri::command]` wrappers and `generate_handler!` entries in `duke-launcher/src-tauri/src/main.rs` itself after workers complete their modules. `win11-ui-crafter` workers edit only assigned disjoint frontend files (`duke-launcher/src/index.html`, `styles.css`, `main.js`, `api.js`).
- **Exact Contract Handoff:** Always pass workers a locked mini-spec containing exact `struct`/`enum`/`pub fn` signatures, exact DOM IDs and `#icon-...` `<symbol>` IDs, exact Tauri `invokeCommand` parameter shapes, CSS custom properties, and exact target verification commands (`cargo test -p <crate> <module>::tests`, `node --check`).
- **Digest-Only Return:** Subagents must return concise `<= 10`-line digests (exported types/signatures, modified selectors/IDs/handlers, test pass counts, or `static-verifier` pass/fail tables with `file:line` diagnostics) rather than raw file dumps so the primary `XHigh` context window stays lean.
- **Zero-Token Pre-Check:** Run `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/verify-hygiene.ps1` (`<2s`) after edits, and delegate full `cargo clippy --workspace --all-targets -- -D warnings` + `cargo test --workspace` verification to `static-verifier`.

# GitHub Copilot Instructions — The Duke Suite

## Project Overview
The Duke Suite (`duke-core`, `duke-launcher`, `duke-systems`, `duke-monitor`, `duke-capture`, `duke-input`) builds ultra-lean Tauri v2 + Rust + Win32 FFI desktop tools on Windows 11.

## Strict Constraints
- **Open-Source Hygiene:** Never embed personal usernames, local IPs, third-party piracy URLs, or `C:\Users\<name>` paths. Community catalogs use user-supplied Hydra-compatible JSON feeds in `%LOCALAPPDATA%\com.dukelauncher.desktop\sources.json`.
- **Zero-Bloat Rust:** Reuse `duke-core` (`win32_ffi`, `net_doh`, `pipe_ipc`, `shm_bus`, `nvme_prefetch`, `hid_raw`) with pure-Rust implementations before adding crates. Preserve `opt-level = 3`, `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `strip = true`.
- **WebView2 Multi-Window:** Never configure `--renderer-process-limit=1`.
- **Native Win11 UI:** Vanilla HTML/CSS/JS only; Segoe UI Variable Text for UI, Cascadia Code (`tabular-nums`) for telemetry/paths, 1.5px `#icon-...` SVG sprites (no emojis), preserve all 10 `[data-theme]` themes.

## Key Commands, CLI & Schemas (`duke-launcher`)
- **Hydra JSON Sources:** `get_imported_sources`, `import_source_url`, `import_source_file`, `toggle_imported_source`, `refresh_imported_source`, `refresh_all_imported_sources`, `remove_imported_source`, `search_imported_sources`.
- **Ludusavi Saves:** `sync_ludusavi_manifest`, `discover_game_save_paths`, `backup_game_saves`, `list_save_backups`, `restore_game_save`.
- **Library Importer & Artwork:** `import_existing_libraries` (Steam VDF/ACF, Epic `.item`, GOG Registry, Xbox `XboxGames`/`.GamingRoot`), `search_steamgriddb_artwork`, `apply_custom_artwork`, `create_desktop_shortcut_with_icon`.
- **Snapshots, Catalog Mmap, Delta, Swarm & PE:** `create_game_snapshot`, `list_game_snapshots`, `restore_game_snapshot`, `delete_game_snapshot`, `get_catalog_mmap_stats`, `search_catalog_mmap`, `generate_game_dkmanifest`, `diff_game_against_manifest`, `create_delta_patch_file`, `apply_game_delta_patch`, `get_swarm_status`, `probe_swarm_peers`, `beam_game_from_peer`, `sync_game_saves_mesh`, `preflight_check_exe`, `heal_game_runtime_dlls`.
- **Process / Daemon / Downloader (`main.rs` & `duke-core`):** 8-worker AIMD (`4-64 MiB`) downloader + in-flight `.tar.zst` / parallel `.zip` Range extractor, `Local\duke-suite-shm-v1` SHM bus (`ShmEventFrame`), NVMe `PrefetchVirtualMemory` pre-launch warmer, `.duke-profile` sandbox (`set_game_launch_profile`), Win32 Job Object (`ProcessTreeJob`) game tree tracking, `Zero-RAM` WebView2 tray destruction (`duke_core::tray_daemon`), zero-allocation PE `IMAGE_NT_HEADERS` + Shannon-entropy scanner, `duke://download?url=...` URI protocol, `suspend_or_resume_game`, `get_active_game_sessions`, `poll_gamepad_state`, `get_launcher_settings`, `update_launcher_settings`.
- **Headless `--mcp` Stdio Server:** Invoking `duke-launcher.exe --mcp` bypasses GUI initialization and exposes all 8 tools: `list_installed_games`, `search_catalog`, `launch_game`, `scan_binary`, `discover_saves`, `backup_saves`, `import_libraries`, and `suspend_or_resume_game`.

## Verification Checklist
- `cargo check` and `cargo test` in `duke-core` (`--all-features`) and `duke-launcher/src-tauri`
- `node --check duke-launcher/src/api.js` and `node --check duke-launcher/src/main.js`
- `cargo bloat --release -n 15` in `duke-launcher/src-tauri`

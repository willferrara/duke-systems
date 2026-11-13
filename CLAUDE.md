# CLAUDE.md — Claude Code Instructions for The Duke Suite

## Repository Architecture
This repository hosts **The Duke Suite**, a multi-crate Windows 11 native performance toolkit built in **Rust (Tauri v2 + Tokio + Win32 FFI)** with a Vanilla ES2022 HTML/CSS/JS frontend:
- `duke-core`: Shared feature-gated core library (`win32_ffi`, `net_doh`, `pipe_ipc`, `shm_bus`, `nvme_prefetch`, `hid_raw`, `macro_engine`, `tray_daemon`). Lock-free 64-byte SHM ring buffer runs on `Local\duke-suite-shm-v1`; Named Pipe control bus runs on `\\.\pipe\duke-suite`.
- `duke-launcher`: PC game library manager, 8-worker AIMD (`4-64 MiB`) downloader + in-flight `.tar.zst` / parallel-range `.zip` streaming extractor (`stream_extract.rs`), NVMe `PrefetchVirtualMemory` pre-launch cache warmer (`<= 25%` free RAM), `.duke-cas` `blake3` + `CreateHardLinkW` CoW snapshots (`snapshots.rs`), zero-copy `memmap2` `.dkcat` catalog (`catalog_mmap.rs`), FastCDC v2020 + `zstd` delta patcher (`delta_patch.rs`), Duke Swarm LAN multicast (`239.255.77.88:47880`) / Tailscale (`47881`) game-beam + vector-clock save sync (`swarm.rs`), `.duke-profile` portable sandbox + pure-Rust PE32/PE32+ import auto-healer (`pe_inspect.rs`), `ProcessTreeJob` + EcoQoS + `tray_daemon` headless mode, Hydra JSON sources, Ludusavi saves, multi-store importers, `NtSuspendProcess` pause/resume, XInput Couch Mode, `duke://` URI, and `--mcp` stdio JSON-RPC server.
- `duke-systems`, `duke-monitor`, `duke-capture`, `duke-input`: Companion suite apps sharing `duke-core`.

## Engineering & Safety Standards
1. **Zero-Bloat Feature Gating:** Never enable `net-doh` on daemons like `duke-input` that do not require HTTP/TLS.
2. **Release Optimization:** Preserve `[profile.release]` (`opt-level = 3`, `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `strip = true`) across all crates.
3. **WebView2 Safety:** Never pass `--renderer-process-limit=1` in any `tauri.conf.json`.
4. **Open-Source Hygiene:** Zero personal usernames, local IPs, hardcoded third-party piracy URLs, or `C:\Users\<name>` paths anywhere in code, docs, or configs.
5. **Native Win11 UI:** Use Title Case with Segoe UI Variable Text for labels, Cascadia Code (`tabular-nums`) for telemetry/paths, 1.5px `#icon-...` SVG sprites (no emojis), and preserve all 10 CSS themes without inlining double-quoted SVGs inside `style=""`.

## `duke-launcher` Backend Commands & Headless MCP
- **JSON Sources & Directories (`sources.rs`, `sites.rs`):** `get_imported_sources`, `import_source_url`, `import_source_file`, `toggle_imported_source`, `refresh_imported_source`, `refresh_all_imported_sources`, `remove_imported_source`, `search_imported_sources`, plus `get_search_sites`, `add_search_site`, `remove_search_site`, `bulk_import_search_sites`, `reset_default_search_sites`. Schema: `{ "name": "...", "downloads": [{ "title": "...", "uris": ["https://..."], "uploadDate": "...", "fileSize": "..." }] }`.
- **Ludusavi & Saves (`ludusavi.rs`):** `sync_ludusavi_manifest`, `discover_game_save_paths`, `backup_game_saves`, `list_save_backups`, `restore_game_save`.
- **Library Importer & Artwork (`importers.rs`, `artwork.rs`):** `import_existing_libraries`, `link_local_game_with_steam_id`, `search_steamgriddb_artwork`, `apply_custom_artwork`, `create_desktop_shortcut_with_icon`.
- **Process Control & Input (`main.rs`):** `suspend_or_resume_game` (`ntdll!NtSuspendProcess`/`NtResumeProcess` across Job Object process tree + working-set trim), `get_active_game_sessions`, `poll_gamepad_state`, `get_launcher_settings`, `update_launcher_settings`, `set_game_launch_profile`, `read_clipboard_text`, `write_clipboard_text`, `take_pending_protocol_urls`.
- **Snapshots, Catalog Mmap, Delta, Swarm & PE (`snapshots.rs`, `catalog_mmap.rs`, `delta_patch.rs`, `swarm.rs`, `pe_inspect.rs`):** `create_game_snapshot`, `list_game_snapshots`, `restore_game_snapshot`, `delete_game_snapshot`, `get_catalog_mmap_stats`, `search_catalog_mmap`, `generate_game_dkmanifest`, `diff_game_against_manifest`, `create_delta_patch_file`, `apply_game_delta_patch`, `get_swarm_status`, `probe_swarm_peers`, `beam_game_from_peer`, `sync_game_saves_mesh`, `preflight_check_exe`, `heal_game_runtime_dlls`.
- **Headless CLI (`--mcp`):** `duke-launcher.exe --mcp` bypasses Tauri/WebView2 GUI startup and serves JSON-RPC 2.0 over stdio for `list_installed_games`, `search_catalog`, `launch_game`, `scan_binary`, `discover_saves`, `backup_saves`, `import_libraries`, and `suspend_or_resume_game`.

## Verification Commands
```powershell
node --check duke-launcher/src/api.js
node --check duke-launcher/src/main.js
cargo check --manifest-path duke-core/Cargo.toml --all-features
cargo clippy --manifest-path duke-core/Cargo.toml --all-features -- -D warnings
cargo test --manifest-path duke-core/Cargo.toml --all-features
cargo check --manifest-path duke-launcher/src-tauri/Cargo.toml --tests
cargo clippy --manifest-path duke-launcher/src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path duke-launcher/src-tauri/Cargo.toml
cargo deny --manifest-path duke-launcher/src-tauri/Cargo.toml check advisories bans sources
```

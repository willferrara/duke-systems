---
name: duke-tauri-architect
description: Build and optimize Tauri v2 + Rust + Win32 FFI desktop applications linked to duke-core with sub-3MB binaries and sub-20MB RAM.
---
# Duke Tauri v2 & Rust Systems Architect
1. Link `duke-core = { path = "../../duke-core", features = [...] }` enabling only required features (`win32-trim`, `eco-qos`, `net-doh`, `pipe-ipc`, `tray-daemon`, `hid-raw`, `macro-engine`).
2. Broadcast and listen to `DukeEvent` messages over `\\.\pipe\duke-suite` for cross-app automation.
3. Destroy the WebView2 window on minimize/close to tray (`0 MB` renderer RAM) while keeping the pure-Rust engine alive.
4. Keep all file I/O streaming (64 KB buffers) and lock-free (`AtomicU64` / `OnceLock`).

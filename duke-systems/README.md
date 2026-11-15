# Duke Systems

> **Status: Experimental / Paused — Active development is focused exclusively on `Duke Launcher`. Please download and use `Duke Launcher` from Releases.**

**Unified Low-RAM Windows Systems Suite Command Deck** (`Duke Systems.exe`) built with **Tauri v2 + Rust + Vanilla JS/CSS** and powered by **`duke-core`**.

Duke Systems acts as the lightweight central launcher, live process telemetry console, and persistent **`\\.\pipe\duke-suite`** Named Pipe IPC hub coordinating the four specialized Duke Suite applications:

- **Duke Launcher** (`Duke Launcher.exe`) — Unified Steam / Epic / GOG / Riot / Custom game library & one-click launcher
- **Duke Monitor** (`Duke Monitor.exe`) — Ultra-low-RAM frame-time overlay, DXGI GPU & CPU telemetry profiler
- **Duke Capture** (`Duke Capture.exe`) — Hardware-accelerated DXGI Desktop Duplication recorder & instant replay
- **Duke Input** (`Duke Input.exe`) — RawHID zero-hook mouse/controller remapper & per-game profile switcher

## Architecture & Low-Memory Design

- **No Bundler Overhead:** Pure ES2022 Vanilla JS/CSS frontend (`frontendDist: "../src"`) with 10 zero-RAM inline SVG themes.
- **Persistent Named Pipe Hub (`\\.\pipe\duke-suite`):** Uses `duke_core::pipe_ipc::DukeBus` to host and relay cross-app events (`GameStarted`, `GameStopped`, `ProfileSwitched`, `ClipSaved`, `RamPurgeRequested`) in real time to the **Suite Activity Feed**.
- **Relative Binary Discovery (Portable & Multi-Repo Friendly):** Resolves sibling executables dynamically relative to the running `Duke Systems.exe` location (checking `.` and `..\<app-dir>\` development/release paths) with zero hardcoded usernames or personal paths.
- **Zero-RAM Tray Daemon (`duke_core::tray_daemon`):** Closing or minimizing the window destroys the WebView2 renderer process (`0 MB` browser RAM) while keeping the Win32 Rust IPC hub resident at ~3–6 MB working set.
- **One-Click Suite Memory Purge (`Purge Suite RAM`):** Broadcasts `DukeEvent::RamPurgeRequested` over `\\.\pipe\duke-suite`, trims every running Duke Suite process via `duke_core::win32_ffi::trim_external_pid`, and trims its own working set.

## Development & Build

```powershell
# Install Tauri CLI dependencies
npm install

# Regenerate Windows app icons from src/dukesystems.png
npx tauri icon src\dukesystems.png

# Verify Rust backend compilation
cd src-tauri
cargo check

# Build stripped LTO release binary
cd ..
npm run tauri build
```

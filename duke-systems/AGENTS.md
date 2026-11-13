# AGENTS.md — Duke Systems Command Deck Architecture Guide

## Project Overview

**Duke Systems** (`Duke Systems.exe`) is the central command hub and persistent `\\.\pipe\duke-suite` IPC coordinator for the **Duke Suite** (`duke-launcher`, `duke-monitor`, `duke-capture`, `duke-input`). It uses **Tauri v2**, **Rust**, and a dependency-free **Vanilla ES2022 JS/CSS** frontend backed by **`duke-core`**.

## Stack & Key Invariants

- **Backend:** Rust (`src-tauri/src/main.rs`) with `duke-core` features `win32-trim`, `eco-qos`, `pipe-ipc`, and `tray-daemon`.
- **Frontend:** Vanilla HTML/CSS/ES modules (`src/index.html`, `src/styles.css`, `src/api.js`, `src/main.js`) with zero frameworks or build tools.
- **Executable Discovery:** Always resolve sibling suite executables dynamically relative to `std::env::current_exe()` (checking `.` alongside the binary and sibling workspace `target\release` / `target\debug` folders). Never hardcode user profile names or local checkout paths.
- **Renderer Stability:** `tauri.conf.json` intentionally avoids `--renderer-process-limit=1` and `--single-process`.
- **Zero-RAM Tray Behavior:** Closing or minimizing to tray destroys the WebView2 window via `duke_core::tray_daemon::destroy_window_for_tray`; frontend polling pauses whenever `document.hidden` is true.
- **XSS Safety:** Always pass dynamic data through `escapeHtml()` before rendering templates with `innerHTML`.
- **Release Profile:** `src-tauri/Cargo.toml` enforces `opt-level = 3`, `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, and `strip = true`.

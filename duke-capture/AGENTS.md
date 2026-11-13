# AGENTS.md

Engineering constraints and architecture invariants for **Duke Capture**.

## Architecture & Memory Budget
- **Backend:** Rust + Tauri v2 (`src-tauri/src/main.rs`) with Win32 DXGI Desktop Duplication, WASAPI dual capture, Windows Media Foundation `IMFSinkWriter`/`IMFSourceReader`, low-level input hooks, and LTO-stripped release profile.
- **Frontend:** Vanilla HTML5, CSS3, and ES2022 modules (`src/index.html`, `src/styles.css`, `src/main.js`, `src/api.js`) using `convertFileSrc` via Tauri `assetProtocol` for smooth HTML5 `<video>` playback.
- **WebView2 Memory Discipline:** Trim idle working sets via `K32EmptyWorkingSet` and pause live UI meters when `document.hidden === true`. Never add `--renderer-process-limit=1` or `--single-process`.

## Security & Privacy Rules
- Never log or persist captured frame pixels continuously to disk during replay buffering; keep the circular replay buffer strictly in RAM until save hotkey triggers.
- Enforce directory scoping inside `delete_clip` so arbitrary paths outside the configured Clips Root cannot be removed.
- Escape all filenames/device names before injecting HTML in `src/main.js`.
- Resolve optional FFmpeg installer fetches over Encrypted DoH (`1.1.1.1`, `1.0.0.1`, `9.9.9.9`).

## Verification
Run from `duke-capture/src-tauri`:
```bash
cargo check
```
All changes must compile cleanly with zero errors and zero warnings.

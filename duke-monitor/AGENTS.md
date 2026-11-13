# AGENTS.md

Engineering constraints and architecture invariants for **Duke Monitor**.

## Stack & Memory Budget
- **Backend:** Rust + Tauri v2 (`src-tauri/src/main.rs`) with LTO-stripped release profile (`opt-level = 3`, `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `strip = true`).
- **Frontend:** Vanilla HTML5, CSS3, and ES2022 modules (`src/index.html`, `src/styles.css`, `src/main.js`, `src/api.js`) with zero external JS frameworks.
- **WebView2 Memory Discipline:** Maintain Win32 `K32EmptyWorkingSet` idle trimming and pause sensor polling when `document.hidden === true`. Never add `--renderer-process-limit=1` or `--single-process` in `tauri.conf.json`.

## Security & Privacy Rules
- Resolve external driver APIs and installer hosts exclusively through the Rust DoH resolver (`1.1.1.1`, `1.0.0.1`, `9.9.9.9`) with SSRF host allowlisting (`nvidia.com`, `intel.com`, `amd.com`) and HTTPS enforcement.
- Validate all external URLs before `shell::open` (`http://` / `https://` only).
- Escape all untrusted WMI/registry/process strings before rendering HTML in `src/main.js`.
- Never hardcode user-specific absolute paths or personal usernames.

## Verification
Run from `duke-monitor/src-tauri`:
```bash
cargo check
```
All changes must compile cleanly with zero errors and zero warnings.

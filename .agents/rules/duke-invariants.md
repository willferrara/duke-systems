---
trigger: always_on
---

# Duke Suite Non-Negotiable Workspace Invariants

Every primary agent and subagent operating in this workspace MUST uphold these invariants without exception:

## 1. Rust `panic = "abort"` & Zero-Bloat Safety
- Every release profile uses `opt-level = 3`, `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, and `strip = true` — any panic aborts the entire process immediately.
- **Never** use `.unwrap()` or `.expect()` on runtime paths, file I/O, network/HTTP calls, Win32 registry/FFI probes, JSON parsing, or `memmap2` slices. Propagate errors cleanly with `?`, `.ok()`, `.unwrap_or*()`, or contextual `Result<T, String>` / `anyhow` errors.
- **Heap-allocate large buffers:** Never place buffers `>= 32 KiB` (`32_768` bytes) on the stack (e.g., avoid `[u8; 65536]`); allocate on the heap via `vec![0u8; len]` or `Vec::with_capacity(len)`.
- **Streaming & Lock-Free I/O:** Stream large files/archives in 64 KB–512 KB chunks (`Seek`/`Read`), never `std::fs::read()` multi-GB binaries, and prefer lock-free primitives (`AtomicU64`, `OnceLock`) for hot counters.
- **Feature-Gated `duke-core`:** Enable only strictly required features (e.g., never enable `net-doh` on daemons like `duke-input` that do not perform HTTP/TLS).

## 2. Tauri v2 & Native Windows 11 UI Parity
- **Window Configuration:** Keep `"transparent": false` on the `"main"` window in `duke-launcher/src-tauri/tauri.conf.json`, and **never** add `--renderer-process-limit=1` or `--single-process` to browser args.
- **10 Solid Themes Only:** Preserve all 10 zero-RAM CSS themes (`dark`, `light`, `minecraft`, `sunset`, `aerospace`, `cyberpunk`, `terminal`, `crimson`, `ocean`, `frost`). Never reference `frosted-obsidian` or reintroduce translucent main-window glass hacks.
- **Vector Icons & Typography:** Use inline 1.5px `<symbol id="icon-...">` SVG sprites (`<svg class="icon"><use href="#icon-..."/></svg>`); never use emojis or inline double-quoted SVGs inside HTML `style=""` attributes. Use Title Case with `"Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif` for UI labels and `"Cascadia Code", "Consolas", monospace` (`font-variant-numeric: tabular-nums`) strictly for telemetry/paths. Escape dynamic HTML with `escapeHtml()`.
- **100% Static Parity:** Maintain exact parity across:
  - DOM IDs in `index.html` and `el(...)` / `getElementById(...)` calls in `main.js`
  - `<symbol id="icon-...">` definitions in `index.html` and `#icon-...` references in HTML/JS
  - `invokeCommand(...)` / `invoke(...)` command names in `api.js` / `main.js` and `tauri::generate_handler![...]` in `main.rs`
  - Required plugin permissions in `src-tauri/capabilities/default.json`

## 3. Privacy, Standalone & Open-Source Hygiene
- **Zero Personal Leaks:** Never commit personal usernames, home directory paths (`C:\Users\<username>`), private LAN IPs, or links to copyrighted ROM/piracy hosting sites in code, docs, comments, tests, or configs. Always resolve paths dynamically via `%LOCALAPPDATA%`, `%USERPROFILE%`, or `std::env::current_exe()`.
- **Standalone Guarantee:** Keep **Duke Launcher** 100% standalone with self-contained runtime state and zero mandatory external companion dependencies.

# The Duke Suite — Master Engineering & Execution Rules

## 1. Mandatory Upfront Total-Process Time Estimation (EVERY MULTI-FILE PROMPT)
Before executing any file edits, searches, or shell commands on tasks touching 2+ files or crates, ALWAYS output a concise **Execution Plan & Total Time Estimate** block first:
- Break the task into numbered phases (e.g., `Phase 1: Codebase Audit`, `Phase 2: duke-core / Rust Backend`, `Phase 3: Frontend UI & SVG Polish`, `Phase 4: cargo check, cargo test & Binary Audit`).
- Estimate realistic wall-clock minutes per phase across the ENTIRE multi-file workflow (including `cargo check` / `npm` execution), and state **Estimated Total Process Time: ~X–Y min** up front.
- Before starting each phase, print a single status line: `[Phase N/Total | Elapsed: ~Xm | Est. Remaining: ~Ym]`.

## 2. Duke Suite Architecture & Zero-Bloat Standards
- **Workspace Layout:** `duke-core` (shared feature-gated library), `duke-systems` (central command hub), `duke-launcher`, `duke-monitor`, `duke-capture`, `duke-input`.
- **Feature-Gated `duke-core`:** Never enable `net-doh` on daemons like `duke-input` that do not need HTTP/TLS.
- **Release Optimization:** Every `Cargo.toml` must keep `[profile.release]` with `opt-level = 3`, `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, and `strip = true`.
- **WebView2 Multi-Window Safety:** Never set `--renderer-process-limit=1` in `tauri.conf.json`.
- **Open-Source Hygiene:** Never hardcode personal usernames, local IPs, or `C:\Users\<name>` paths anywhere in code, docs, or configs.

## 3. Native Windows 11 UI Standards (Zero AI Slop)
- Use Title Case with `"Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif` for UI labels and `"Cascadia Code", "Consolas", monospace` (`font-variant-numeric: tabular-nums`) strictly for live numeric telemetry and file paths.
- Use inline 1.5px `<symbol>` SVG icon sprites (`#icon-...`) instead of emojis or text-only buttons.
- Preserve all 10 zero-RAM CSS themes (`[data-theme="..."]` / `[data-preview-theme="..."]`) and never inline double-quoted SVGs inside HTML `style=""` attributes.
- When visual UI verification is requested, use the `playwright` MCP server to render `src/index.html` and verify layout alignment and contrast.

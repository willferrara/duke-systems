---
name: systematic-verification
description: Mandatory multi-crate verification, binary bloat auditing, and DOM/IPC integrity checks before completing any task.
---
# Verification & Binary Bloat Audit
1. Run `node --check` on every modified `.js` file and verify every `getElementById` / `querySelector` matches an existing ID in `index.html`.
2. Verify every frontend `invoke` call matches a registered `#[tauri::command]` handler in `main.rs` and every permission is declared in `src-tauri/capabilities/default.json`.
3. Run `cargo check` and `cargo test` inside every modified crate and confirm `0 errors` and `0 warnings`.
4. When optimizing binary size, run `cargo bloat --release -n 15` to identify and trim heavy dependencies.

#requires -Version 5.1
<#
.SYNOPSIS
  Bootstraps a standalone hardened Tauri v2 + Rust Windows 11 desktop app using The Duke Suite architecture.
.DESCRIPTION
  Copies .agents/ (rules, skills, agents), AGENTS.md, .vscode/ (or default settings), and
  scripts/verify-hygiene.ps1 into -TargetDir, rewrites crate/app identifiers in
  .agents/rules/duke-invariants.md, and generates a hardened Cargo workspace + zero-panic
  src-tauri/src/main.rs starter with std::panic::set_hook + SetUnhandledExceptionFilter crash logging.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$AppName,

    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[a-z0-9][a-z0-9_-]*$')]
    [string]$CrateSlug,

    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$TargetDir
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$SourceRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$DestRoot = [System.IO.Path]::GetFullPath($TargetDir)
$IdentifierSlug = ($CrateSlug -replace '[^a-z0-9]', '').ToLowerInvariant()
if (-not $IdentifierSlug) {
    $IdentifierSlug = 'dukeapp'
}
$BundleId = "com.$IdentifierSlug.desktop"

function Ensure-Dir {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) {
        [void](New-Item -ItemType Directory -Path $Path -Force)
    }
}

function Write-Utf8NoBom {
    param([string]$Path, [string]$Content)
    $Parent = Split-Path -Parent $Path
    if ($Parent) { Ensure-Dir $Parent }
    $Enc = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($Path, $Content, $Enc)
}

Write-Host "Scaffolding Duke Suite app '$AppName' ($CrateSlug) -> $DestRoot"
Ensure-Dir $DestRoot

# ------------------------------------------------------------------
# 1. Copy .agents/, AGENTS.md, .vscode/, and scripts/verify-hygiene.ps1
# ------------------------------------------------------------------
$SourceAgents = Join-Path $SourceRoot '.agents'
$DestAgents = Join-Path $DestRoot '.agents'
if (Test-Path -LiteralPath $SourceAgents) {
    Copy-Item -LiteralPath $SourceAgents -Destination $DestAgents -Recurse -Force
} else {
    Ensure-Dir (Join-Path $DestAgents 'rules')
    Ensure-Dir (Join-Path $DestAgents 'skills')
    Ensure-Dir (Join-Path $DestAgents 'agents')
}

$SourceAgentsMd = Join-Path $SourceRoot 'AGENTS.md'
if (Test-Path -LiteralPath $SourceAgentsMd) {
    Copy-Item -LiteralPath $SourceAgentsMd -Destination (Join-Path $DestRoot 'AGENTS.md') -Force
}

$SourceVsCode = Join-Path $SourceRoot '.vscode'
$DestVsCode = Join-Path $DestRoot '.vscode'
if (Test-Path -LiteralPath $SourceVsCode) {
    Copy-Item -LiteralPath $SourceVsCode -Destination $DestVsCode -Recurse -Force
} else {
    Ensure-Dir $DestVsCode
    $VsSettingsJson = @'
{
  "editor.formatOnSave": true,
  "files.eol": "\n",
  "rust-analyzer.check.command": "clippy",
  "rust-analyzer.linkedProjects": [
    "./Cargo.toml"
  ]
}
'@
    Write-Utf8NoBom (Join-Path $DestVsCode 'settings.json') $VsSettingsJson
}

$SourceHygiene = Join-Path $SourceRoot 'scripts\verify-hygiene.ps1'
$DestHygiene = Join-Path $DestRoot 'scripts\verify-hygiene.ps1'
if (Test-Path -LiteralPath $SourceHygiene) {
    $HygieneRaw = [System.IO.File]::ReadAllText($SourceHygiene)
    $HygieneAdapted = $HygieneRaw.Replace('duke-launcher/', '').Replace('duke-launcher\', '').Replace('duke-launcher', $CrateSlug).Replace(", 'src/virtual_grid.js'", '')
    Write-Utf8NoBom $DestHygiene $HygieneAdapted
}

# Update crate/app identifiers in .agents/rules/duke-invariants.md
$InvariantsPath = Join-Path $DestAgents 'rules\duke-invariants.md'
if (Test-Path -LiteralPath $InvariantsPath) {
    $InvRaw = [System.IO.File]::ReadAllText($InvariantsPath)
    $InvUpdated = $InvRaw.Replace('duke-launcher/', '').Replace('duke-launcher\', '').Replace('duke-launcher', $CrateSlug).Replace('Duke Launcher', $AppName)
    Write-Utf8NoBom $InvariantsPath $InvUpdated
}

# ------------------------------------------------------------------
# 2. Generate hardened workspace Cargo.toml & src-tauri crate
# ------------------------------------------------------------------
$WorkspaceCargo = @"
[workspace]
resolver = "2"
members = [
    "src-tauri",
]

[profile.dev]
incremental = true

[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "abort"
strip = true
"@
Write-Utf8NoBom (Join-Path $DestRoot 'Cargo.toml') $WorkspaceCargo

$CrateCargo = @"
[package]
name = "$CrateSlug"
version = "0.1.0"
description = "$AppName - Hardened Tauri v2 Windows 11 Desktop Application"
edition = "2021"
rust-version = "1.92.0"

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
tauri = { version = "2", features = [] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tokio = { version = "1.40", features = ["rt-multi-thread", "macros", "sync", "time"] }

[target.'cfg(target_os = "windows")'.dependencies]
windows = { version = "0.62", features = [
    "Win32_Foundation",
    "Win32_System_Threading",
] }
"@
Write-Utf8NoBom (Join-Path $DestRoot 'src-tauri\Cargo.toml') $CrateCargo

$BuildRs = @'
fn main() {
    tauri_build::build();
}
'@
Write-Utf8NoBom (Join-Path $DestRoot 'src-tauri\build.rs') $BuildRs

$TauriConf = @"
{
  "`$schema": "https://schema.tauri.app/config/2",
  "productName": "$AppName",
  "version": "0.1.0",
  "identifier": "$BundleId",
  "build": {
    "frontendDist": "../src"
  },
  "app": {
    "withGlobalTauri": true,
    "windows": [
      {
        "label": "main",
        "title": "$AppName",
        "width": 1200,
        "height": 780,
        "minWidth": 640,
        "minHeight": 540,
        "resizable": true,
        "fullscreen": false,
        "decorations": false,
        "transparent": false,
        "center": true
      }
    ],
    "security": {
      "csp": "default-src 'self' ipc: http://ipc.localhost; img-src 'self' data: blob:; style-src 'self' 'unsafe-inline'; font-src 'self' data:; connect-src 'self' ipc: http://ipc.localhost"
    }
  },
  "bundle": {
    "active": true,
    "targets": ["nsis"]
  }
}
"@
Write-Utf8NoBom (Join-Path $DestRoot 'src-tauri\tauri.conf.json') $TauriConf

$CapJson = @"
{
  "`$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Default desktop capabilities for $AppName.",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "core:window:allow-minimize",
    "core:window:allow-toggle-maximize",
    "core:window:allow-start-dragging",
    "core:window:allow-close"
  ]
}
"@
Write-Utf8NoBom (Join-Path $DestRoot 'src-tauri\capabilities\default.json') $CapJson

$PackageJson = @"
{
  "name": "$CrateSlug",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "tauri": "tauri"
  },
  "devDependencies": {
    "@tauri-apps/cli": "^2"
  },
  "dependencies": {
    "@tauri-apps/api": "^2.12.4"
  }
}
"@
Write-Utf8NoBom (Join-Path $DestRoot 'package.json') $PackageJson

# ------------------------------------------------------------------
# 3. Generate zero-panic src-tauri/src/main.rs with crash_guard
# ------------------------------------------------------------------
$MainRs = @"
// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod crash_guard {
    use core::ffi::c_void;
    use std::backtrace::Backtrace;
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    const EXCEPTION_STACK_OVERFLOW: u32 = 0xC000_00FD;
    const EXCEPTION_CONTINUE_SEARCH: i32 = 0;

    #[cfg(target_os = "windows")]
    #[repr(C)]
    #[allow(dead_code)]
    struct Win32ExceptionRecord {
        exception_code: u32,
        exception_flags: u32,
        exception_record: *const Win32ExceptionRecord,
        exception_address: *const c_void,
        number_parameters: u32,
    }

    #[cfg(target_os = "windows")]
    #[repr(C)]
    #[allow(dead_code)]
    struct Win32ExceptionPointers {
        exception_record: *const Win32ExceptionRecord,
        context_record: *const c_void,
    }

    #[cfg(target_os = "windows")]
    #[link(name = "kernel32")]
    extern "system" {
        fn SetUnhandledExceptionFilter(
            filter: Option<unsafe extern "system" fn(*const Win32ExceptionPointers) -> i32>,
        ) -> *const c_void;
    }

    fn crash_log_paths() -> Vec<PathBuf> {
        let mut paths = Vec::with_capacity(2);
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(dir) = exe_path.parent() {
                paths.push(dir.join("$CrateSlug-crash.log"));
            }
        }
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            if !local_app_data.is_empty() {
                paths.push(
                    PathBuf::from(local_app_data)
                        .join("$CrateSlug")
                        .join("crash.log"),
                );
            }
        }
        paths
    }

    fn write_crash_entry(context: &str, detail: &str, capture_backtrace: bool) {
        let epoch_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let pid = std::process::id();
        let current_thread = std::thread::current();
        let thread_name = current_thread.name().unwrap_or("<unnamed>");

        let mut report = format!(
            "=== $AppName Crash Report ===\n\
             timestamp_unix: {epoch_secs}\n\
             version: {}\n\
             pid: {pid}\n\
             thread: {thread_name}\n\
             kind: {context}\n\
             detail: {detail}\n",
            env!("CARGO_PKG_VERSION")
        );

        if capture_backtrace {
            let bt = Backtrace::force_capture();
            report.push_str(&format!("backtrace:\n{bt}\n"));
        } else {
            report.push_str("backtrace: <skipped on stack overflow>\n");
        }
        report.push('\n');

        let _ = std::io::stderr().write_all(report.as_bytes());
        let _ = std::io::stderr().flush();

        for path in crash_log_paths() {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
                let _ = file.write_all(report.as_bytes());
                let _ = file.flush();
            }
        }
    }

    pub fn record_fatal_error(context: &str, err: &str) {
        write_crash_entry(context, err, true);
    }

    #[cfg(target_os = "windows")]
    fn exception_code_name(code: u32) -> &'static str {
        match code {
            0xC000_0005 => "EXCEPTION_ACCESS_VIOLATION",
            0xC000_001D => "EXCEPTION_ILLEGAL_INSTRUCTION",
            0xC000_0094 => "EXCEPTION_INT_DIVIDE_BY_ZERO",
            0xC000_00FD => "EXCEPTION_STACK_OVERFLOW",
            0xC000_0409 => "STATUS_STACK_BUFFER_OVERRUN",
            0xE06D_7363 => "CPP_EH_EXCEPTION",
            _ => "WIN32_SEH_EXCEPTION",
        }
    }

    #[cfg(target_os = "windows")]
    unsafe extern "system" fn unhandled_exception_filter(
        info: *const Win32ExceptionPointers,
    ) -> i32 {
        let (code, flags, addr) = if !info.is_null() && !(*info).exception_record.is_null() {
            let rec = &*(*info).exception_record;
            (
                rec.exception_code,
                rec.exception_flags,
                rec.exception_address as usize,
            )
        } else {
            (0, 0, 0)
        };
        let label = exception_code_name(code);
        let detail = format!(
            "code=0x{code:08X} ({label}) flags=0x{flags:08X} address=0x{addr:016X}"
        );
        write_crash_entry(
            "unhandled_win32_exception",
            &detail,
            code != EXCEPTION_STACK_OVERFLOW,
        );
        EXCEPTION_CONTINUE_SEARCH
    }

    pub fn install() {
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            let payload = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
                s.clone()
            } else {
                "non-string panic payload".to_string()
            };
            let location = panic_info
                .location()
                .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()))
                .unwrap_or_else(|| "<unknown location>".to_string());
            let detail = format!("at {location}: {payload}");
            write_crash_entry("rust_panic", &detail, true);
            prev_hook(panic_info);
        }));

        #[cfg(target_os = "windows")]
        unsafe {
            let _ = SetUnhandledExceptionFilter(Some(unhandled_exception_filter));
        }
    }
}

#[tauri::command]
async fn get_app_version() -> Result<String, String> {
    Ok(env!("CARGO_PKG_VERSION").to_string())
}

fn main() {
    crash_guard::install();

    let app = match tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_app_version])
        .build(tauri::generate_context!())
    {
        Ok(app) => app,
        Err(err) => {
            crash_guard::record_fatal_error(
                "tauri::Builder::build",
                &format!("failed to build $AppName Tauri application: {err}"),
            );
            std::process::exit(1);
        }
    };

    app.run(|_app_handle, _event| {});
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_version_present() {
        assert!(!env!("CARGO_PKG_VERSION").is_empty());
    }
}
"@
Write-Utf8NoBom (Join-Path $DestRoot 'src-tauri\src\main.rs') $MainRs

# ------------------------------------------------------------------
# 4. Generate minimal Native Win11 UI starter (10 solid themes + SVG parity)
# ------------------------------------------------------------------
$IndexHtml = @"
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>$AppName</title>
  <link rel="stylesheet" href="styles.css" />
</head>
<body data-theme="dark">
  <svg xmlns="http://www.w3.org/2000/svg" style="display:none">
    <symbol id="icon-win-min" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.25"><line x1="3" y1="8" x2="13" y2="8"/></symbol>
    <symbol id="icon-win-max" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.25"><rect x="3.5" y="3.5" width="9" height="9" rx="0.5"/></symbol>
    <symbol id="icon-win-close" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.25"><path d="M12 4 4 12"/><path d="m4 4 8 8"/></symbol>
    <symbol id="icon-zap" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M4 14a1 1 0 0 1-.78-1.63l9.9-10.2a.5.5 0 0 1 .86.46l-1.92 6.02A1 1 0 0 0 13 10h7a1 1 0 0 1 .78 1.63l-9.9 10.2a.5.5 0 0 1-.86-.46l1.92-6.02A1 1 0 0 0 11 14z"/></symbol>
  </svg>
  <div class="window-shell">
    <header class="titlebar" id="app-titlebar">
      <div class="titlebar-left" data-tauri-drag-region>
        <svg class="icon" aria-hidden="true"><use href="#icon-zap"/></svg>
        <span class="titlebar-title">$AppName</span>
        <span class="titlebar-version mono-num" id="app-version-pill">v0.1.0</span>
      </div>
      <div class="titlebar-right">
        <div class="titlebar-drag-filler" data-tauri-drag-region></div>
        <div class="window-controls">
          <button type="button" class="win-btn" id="win-btn-min" title="Minimize" aria-label="Minimize">
            <svg class="icon-win" aria-hidden="true"><use href="#icon-win-min"/></svg>
          </button>
          <button type="button" class="win-btn" id="win-btn-max" title="Maximize" aria-label="Maximize">
            <svg class="icon-win" aria-hidden="true"><use href="#icon-win-max"/></svg>
          </button>
          <button type="button" class="win-btn win-btn-close" id="win-btn-close" title="Close" aria-label="Close">
            <svg class="icon-win" aria-hidden="true"><use href="#icon-win-close"/></svg>
          </button>
        </div>
      </div>
    </header>
    <main class="main-panel">
      <section class="panel-card">
        <h1>$AppName</h1>
        <p class="panel-sub">Native Windows 11 Tauri v2 + Rust starter bootstrapped with zero-panic crash guards and Duke Suite hygiene checks.</p>
      </section>
    </main>
  </div>
  <script type="module" src="main.js"></script>
</body>
</html>
"@
Write-Utf8NoBom (Join-Path $DestRoot 'src\index.html') $IndexHtml

$StylesCss = @'
:root {
  --bg-canvas: #0e1116;
  --bg-sidebar: #13171f;
  --bg-surface: #181d27;
  --bg-elevated: #1f2633;
  --border-subtle: rgba(255, 255, 255, 0.06);
  --text-primary: #f1f5f9;
  --text-secondary: #94a3b8;
  --accent: #0078d4;
}

body[data-theme="dark"] { --bg-canvas: #0e1116; --bg-surface: #181d27; --text-primary: #f1f5f9; }
body[data-theme="light"] { --bg-canvas: #f3f5f8; --bg-surface: #ffffff; --text-primary: #111827; --text-secondary: #4b5563; --border-subtle: rgba(0, 0, 0, 0.08); }
body[data-theme="minecraft"] { --bg-canvas: #141913; --bg-surface: #1d261b; --accent: #4ade80; }
body[data-theme="sunset"] { --bg-canvas: #181116; --bg-surface: #231820; --accent: #fb7185; }
body[data-theme="aerospace"] { --bg-canvas: #0d131a; --bg-surface: #151e29; --accent: #38bdf8; }
body[data-theme="cyberpunk"] { --bg-canvas: #130f1e; --bg-surface: #1c162c; --accent: #f472b6; }
body[data-theme="terminal"] { --bg-canvas: #0a0f0c; --bg-surface: #101914; --accent: #22c55e; }
body[data-theme="crimson"] { --bg-canvas: #160e10; --bg-surface: #221518; --accent: #ef4444; }
body[data-theme="ocean"] { --bg-canvas: #0c141c; --bg-surface: #12202d; --accent: #06b6d4; }
body[data-theme="frost"] { --bg-canvas: #eef2f7; --bg-surface: #ffffff; --text-primary: #0f172a; --text-secondary: #475569; --border-subtle: rgba(15, 23, 42, 0.08); }

* { box-sizing: border-box; margin: 0; padding: 0; }
body {
  background: var(--bg-canvas);
  color: var(--text-primary);
  font-family: "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif;
  height: 100vh;
  overflow: hidden;
}
.mono-num {
  font-family: "Cascadia Code", "Consolas", monospace;
  font-variant-numeric: tabular-nums;
}
.window-shell { display: flex; flex-direction: column; height: 100vh; }
.titlebar {
  height: 36px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding-left: 12px;
  background: var(--bg-sidebar);
  border-bottom: 1px solid var(--border-subtle);
  user-select: none;
}
.titlebar-left, .titlebar-right { display: flex; align-items: center; gap: 8px; height: 100%; }
.titlebar-drag-filler { flex: 1; min-width: 24px; height: 100%; }
.titlebar-title { font-size: 12.5px; font-weight: 600; }
.titlebar-version { font-size: 11px; color: var(--text-secondary); }
.window-controls { display: flex; height: 100%; }
.win-btn {
  width: 44px;
  height: 100%;
  border: 0;
  background: transparent;
  color: var(--text-primary);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
}
.win-btn:hover { background: rgba(255, 255, 255, 0.08); }
.win-btn-close:hover { background: #c42b1c; color: #ffffff; }
.icon { width: 16px; height: 16px; }
.icon-win { width: 12px; height: 12px; }
.main-panel { flex: 1; padding: 20px; overflow: auto; }
.panel-card {
  background: var(--bg-surface);
  border: 1px solid var(--border-subtle);
  border-radius: 8px;
  padding: 18px;
}
.panel-sub { margin-top: 6px; font-size: 13px; color: var(--text-secondary); }
'@
Write-Utf8NoBom (Join-Path $DestRoot 'src\styles.css') $StylesCss

$ApiJs = @'
export async function invokeCommand(command, args = {}) {
  const fn = window.__TAURI__?.core?.invoke;
  if (typeof fn !== "function") {
    throw new Error(`Tauri IPC unavailable for "${command}"`);
  }
  return fn(command, args);
}

export async function getAppVersion() {
  return invokeCommand("get_app_version");
}

export function initWindowControls() {
  let win = null;
  try {
    win = window.__TAURI__?.window?.getCurrentWindow?.() ?? null;
  } catch {
    win = null;
  }
  document.getElementById("win-btn-min")?.addEventListener("click", () => win?.minimize?.().catch(() => {}));
  document.getElementById("win-btn-max")?.addEventListener("click", () => win?.toggleMaximize?.().catch(() => {}));
  document.getElementById("win-btn-close")?.addEventListener("click", () => win?.close?.().catch(() => {}));
  document.getElementById("app-titlebar")?.addEventListener("mousedown", (e) => {
    if (e.button !== 0 || e.detail > 1) return;
    if (e.target.closest("button, input, select, a")) return;
    if (e.target.hasAttribute("data-tauri-drag-region")) return;
    win?.startDragging?.().catch(() => {});
  });
}
'@
Write-Utf8NoBom (Join-Path $DestRoot 'src\api.js') $ApiJs

$MainJs = @'
import { getAppVersion, initWindowControls } from "./api.js";

function el(id) {
  return document.getElementById(id);
}

initWindowControls();
getAppVersion()
  .then((ver) => {
    const pill = el("app-version-pill");
    if (pill && ver) {
      pill.textContent = `v${ver}`;
    }
  })
  .catch(() => {});
'@
Write-Utf8NoBom (Join-Path $DestRoot 'src\main.js') $MainJs

Write-Host "Scaffolded '$AppName' ($CrateSlug) successfully at $DestRoot"

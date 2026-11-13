# Duke Launcher

**Duke Launcher** is an ultra-lightweight, zero-telemetry Windows PC game library manager, 8-worker adaptive download engine, and crack-aware security auditor built with **Rust (Tauri v2 + Tokio)** and **Vanilla ES2022 HTML/CSS/JS**.

Engineered on the **Frontier Systems Architecture**, Duke Launcher idles at **~15–25 MB of RAM** and **0% CPU**—roughly **20x lighter** than Electron-based launchers—while delivering full line-rate multi-connection downloads, automatic Steam metadata/box-art matching, 1-click NTFS game compression, and Windows 11 Efficiency-Core (EcoQoS) scheduling.

---

## Key Features

### 1. Ultra-Low RAM, CPU & Windows 11 EcoQoS Hybrid Core Pinning
- **Win32 Working-Set Trimming**: Directly invokes `SetProcessWorkingSetSizeEx` and `K32EmptyWorkingSet` via `kernel32` FFI to release unused physical RAM pages immediately after startup, after cover art indexing, and while minimized.
- **Windows 11 EcoQoS Game Boost**: When you click **PLAY**, Duke Launcher transitions its own process priority to `BELOW_NORMAL_PRIORITY_CLASS` (`0x00004000`), enables Windows 11 `PROCESS_POWER_THROTTLING_EXECUTION_SPEED` (EcoQoS) so the OS schedules Duke Launcher strictly on Efficiency (E) cores while leaving Performance (P) cores 100% dedicated to your game, minimizes its window, pauses all UI timers/animations, and trims its working set. When the game exits, `NORMAL_PRIORITY_CLASS` is restored and playtime is updated automatically.
- **Disk Cover Cache (`covers/`)**: Downloaded box art is offloaded to disk inside `covers/` so `library.json` stays tiny and fast to parse.
- **CSS Viewport Virtualization**: Library and directory grids use `content-visibility: auto; contain-intrinsic-size: 240px;` with `120ms` debounced search for zero-jank scrolling across hundreds of titles.

### 2. Extreme Download Speed, Dynamic Work-Stealing & Mirror Failover
- **Multi-Provider Encrypted DoH IP Racing**: Concurrently races DNS-over-HTTPS queries across **Cloudflare Primary (`1.1.1.1`)**, **Cloudflare Secondary (`1.0.0.1`)**, and **Quad9 (`9.9.9.9`)** (`Accept: application/dns-json`), caching all resolved IPv4 `SocketAddr` endpoints per host so workers can fail over across CDN edge nodes if one IP throttles.
- **Lock-Free Atomic Progress & 512 KB Buffered Streaming**: Uses per-worker `AtomicU64` counters (`Ordering::Relaxed`), reusable 64 KB staging buffers, and 512 KB `tokio::io::BufWriter` segments for zero heap churn during high-speed bursts.
- **Dynamic Work-Stealing & Adaptive Range Windows**: Starts with 16 MB `Range: bytes=start-end` slices, scales up to 32 MB on fast unthrottled sockets (`>8 MB/s`), steals remaining byte ranges from slow workers, and recycles any socket averaging `<300 KB/s` over 6 seconds.
- **Zero-RAM Native Multi-Hoster Resolver**: Resolves direct CDN binary URLs in pure Rust (`0 MB` WebView overhead) for **Buzzheavier** (`bzzhr.to` / `buzzheavier.com` HTMX `HX-Request: true` on `/{id}/download`), **GoFile**, **Pixeldrain**, **MediaFire**, **FileDitch**, **1Fichier**, **MegaUp**, **DataNodes**, and **FuckingFast**.

### 3. Crack-Aware Zero-False-Positive Security Auditor & NTFS Unblocker
- **Streaming 64 KB PE & Heuristic Scanner**: Inspects file headers, 64 KB sliding windows (`Seek`/`Read`), and tail bytes while computing streaming `SHA-256` digests without loading multi-gigabyte archives into RAM.
- **Crack-Aware Whitelist**: Recognizes and whitelists legitimate offline game emulators, Steam wrappers, and scene releases (`steam_api.dll`, `steam_api64.dll`, `OnlineFix64.dll`, `Custom.dll`, `winmm.dll`, `Goldberg`, `SteamEmu`, `RUNE`, `TENOKE`, `CODEX`, `FLT`, `ElAmigos`, `Steamless`) as `CLEAN (VERIFIED GAME EMULATOR)` or `FALSE_POSITIVE_WARNING`.
- **High-Precision Malware Detection**: Flags genuine threats (`DANGER`) including archive disguise attacks (`.rar`/`.7z`/`.zip` starting with `MZ` `4D 5A` or HTML/script payloads), double extensions (`.rar.exe`, `.scr`, `.hta`, `.vbs`, `.cmd`, `.bat`, `.ps1`), and small standalone `.exe` binaries (`<15 MB`) containing combined infostealer signatures (`CryptUnprotectData` + `WinHttpSendRequest`/`InternetOpenUrl` + browser `Login Data` / `Local State` / `wallet.dat` / Discord token exfiltration or encoded PowerShell droppers).
- **Automatic NTFS Mark-of-the-Web (`Zone.Identifier`) Unblocker**: Automatically removes `:Zone.Identifier` alternate data streams from verified binaries and extracted game folders so Windows SmartScreen never blocks or slows down first game launch.
- **Strict Zip-Slip & Decompression Bomb Protection**: Validates all extracted paths against `..` traversal, external symlinks, and excessive file counts.

### 4. Maximum Network & Process Anonymity + Portable Mode
- **Anti-Telemetry Game Launch Scrubbing**: Automatically injects `DO_NOT_TRACK=1`, `SENTRY_DSN=`, `UNITY_DISABLE_ANALYTICS=1`, `UE_DISABLE_CRASH_REPORTER=1`, and `STEAM_FRAME_FORCE_CLOSE=1` into launched game processes.
- **Outbound Privacy Hardening**: Enforces `DNT: 1`, `Sec-GPC: 1`, stripped client hints, rotated User-Agents, and ephemeral WebView2 session wiping (`clear_all_browsing_data()`).
- **100% Portable USB Mode**: Place an empty `portable.flag` file or create a `data/` folder next to `duke-launcher.exe` to store `library.json`, `downloads.json`, `covers/`, and `backups/` inside `data/` with zero `AppData` footprint.

### 5. Power-User Single-Game Detail View
- **Per-Game CLI Launch Arguments**: Configure custom launch flags per game (e.g., `-dx11 -novid -high`) directly inside the Single-Game Detail View.
- **1-Click NTFS Game Folder Compression (`COMPRESS DISK`)**: Invokes Windows 11 native `compact.exe /c /s:"<game_folder>" /a /i /exe:xpress8k` silently, recalculates physical compressed size via `GetCompressedFileSizeW`, updates `library.json`, and displays SSD space saved.
- **Rolling Multi-Version Save Backups (`BACKUP SAVES`)**: Packages discovered save folders/files into timestamped `.zip` archives inside `Duke_Save_Backups/` and automatically rotates the latest 5 snapshots per game.
- **Switch Executable (`CHANGE EXE`)**: Switch between DX11, DX12, Vulkan, or Mod Launcher executables at any time.

---

## Architecture & Repository Layout

See [AGENTS.md](./AGENTS.md) for the complete technical architecture, Tauri IPC command table, JSON state schemas, and contributor guidelines.

```text
game-launcher/
├── AGENTS.md                  # AI Coding Agent & Contributor Reference
├── README.md                  # Project Documentation
├── package.json               # Build & Dev Scripts
├── src/                       # Vanilla ES2022 Frontend (HTML/CSS/JS)
│   ├── index.html             # Application layout, sidebar telemetry footer, modals
│   ├── styles.css             # GPU-accelerated dark styling & grid virtualization
│   ├── main.js                # State management, search, detail view, event handlers
│   └── api.js                 # Tauri v2 IPC command wrappers
└── src-tauri/                 # Rust Backend (Tauri v2 + Tokio)
    ├── Cargo.toml             # Crate dependencies & LTO release profile
    ├── tauri.conf.json        # Window configuration & CSP security policy
    └── src/
        └── main.rs            # Core Rust engine (DoH, 8-worker matrix, EcoQoS, security auditor)
```

---

## Getting Started

### Prerequisites
- **Windows 10 / 11 (64-bit)** with Microsoft Edge WebView2 Runtime (pre-installed on Windows 11)
- **Rust Toolchain** (`rustup`, `cargo` 1.75+)
- **Node.js** (18+ for running the Tauri CLI)

### Development
```powershell
npm install
npm run tauri dev
```

### Verify Rust Backend (`0 errors, 0 warnings`)
```powershell
cd src-tauri
cargo check
```

### Production Release Build
```powershell
npm run tauri build
```
The compiled standalone executable and installers are generated in:
- `src-tauri/target/release/duke-launcher.exe`
- `src-tauri/target/release/bundle/nsis/`
- `src-tauri/target/release/bundle/msi/`

---

## License

MIT License.

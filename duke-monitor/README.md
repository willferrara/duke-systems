# Duke Monitor

Zero-bloat **System, GPU, Process & Clean Driver Cockpit** built with **Tauri v2**, **Rust**, and **Vanilla ES2022 JS/CSS**. Designed as a lightweight replacement for Windows Task Manager, HWMonitor, OEM gaming hubs, and telemetry-heavy driver updaters.

## Key Capabilities

- **GPU Cockpit (Dynamic NVML FFI + DXGI/WMI Fallback):** Loads `nvml.dll` dynamically at runtime for NVIDIA RTX telemetry (core temp, GPU/memory clocks, VRAM used/total, power draw/limit, fan speed) and automatically falls back to Windows DXGI + WMI/PDH for Intel Arc/Core Ultra and AMD Radeon GPUs.
- **CPU & Physical RAM Cockpit:** Queries Win32 `GlobalMemoryStatusEx`, `sysinfo`, and logical processor efficiency classes (`GetLogicalProcessorInformationEx`) to render per-core P-Core vs E-Core bars alongside Duke Monitor's live resident working set (~15–25 MB).
- **1-Click Deep RAM Purge:** Trims working sets across active user processes via `K32EmptyWorkingSet` without killing running apps, reporting reclaimed physical memory immediately.
- **Process Controller, EcoQoS Jail & Bloat Killer:**
  - Virtualized searchable process list (`ECO JAIL`, `BOOST`, `TRIM RAM`, `END TASK`).
  - **EcoQoS Jail:** Applies `BELOW_NORMAL_PRIORITY_CLASS` + Windows 11 `PROCESS_POWER_THROTTLING_EXECUTION_SPEED` and trims RAM to keep background hogs pinned on Efficiency cores.
  - **Game Boost:** Elevates the selected game to `ABOVE_NORMAL_PRIORITY_CLASS` while purging standby RAM from background processes.
  - **OEM & Telemetry Bloat Killer:** Detects and terminates unnecessary background bloat (`OmenCommandCenterBackground`, `ArmouryCrate`, `NvTelemetryContainer`, `GameBarPresenceWriter`, `AdobeCollabSync`, and more).
- **Clean GPU & CPU Driver Auditor (Encrypted DoH):** Reads exact PCI `VEN_`/`DEV_` hardware IDs, installed driver versions, and dates via WMI + Registry, checks official NVIDIA, Intel, and AMD endpoints over **DNS-over-HTTPS (`1.1.1.1` / `9.9.9.9`)**, and downloads official installers into `Downloads\Duke_Drivers\` with clean-install instructions.
- **10 Zero-RAM Vector SVG Themes:** Obsidian Dark, Clean Daylight, Minecraft Overworld, Gulf Coast Sunset, Aerospace HUD, Cyberpunk Night, Retro CRT Amber, Redline Carbon, Abyssal Ocean, and Nordic Frost.

## Development

### Prerequisites
- Windows 10/11 (x64)
- [Rust & Cargo](https://rustup.rs/)
- [Node.js](https://nodejs.org/) (v20+)

### Commands
```bash
npm install
npm run tauri dev
```

### Release Build
```bash
npm run tauri build
```

## Portable Mode
Create an empty `portable.flag` file or a `data/` directory next to `duke-monitor.exe` to isolate application data locally without writing to `%APPDATA%`.

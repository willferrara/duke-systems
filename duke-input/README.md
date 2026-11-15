# Duke Input

> **Status: Experimental / Paused — Active development is focused exclusively on `Duke Launcher`. Please download and use `Duke Launcher` from Releases.**

Zero-bloat, sub-10 MB RAM native Windows input engine, controller remapper, mouse tuner, and FPV transmitter configurator built with Tauri v2, Rust, Win32 XInput/RawInput/SendInput, and `hidapi`.

## Features

- **1000 Hz Deterministic Polling Engine:** Dedicated `THREAD_PRIORITY_HIGHEST` loop with `timeBeginPeriod(1)`, zero-allocation stack buffers (`[u8; 64]`), and lock-free atomic telemetry.
- **Sub-10 MB System Tray Daemon:** Destroys the WebView2 renderer when closed or minimized to tray and trims the working set with `SetProcessWorkingSetSizeEx` + `K32EmptyWorkingSet`.
- **Universal Device Support:**
  - **Xbox / XInput Controllers:** Radial/Axial inner & outer deadzones, anti-deadzone, 1-click center drift calibration, Linear / Exponential / S-Curve / Dynamic Precision / 5-Point Bezier curves, hair-triggers, and independent heavy/light rumble scaling.
  - **Logitech Mice & HID++ 2.0:** Real HID++ 2.0 battery/voltage queries (`0x1000`/`0x1004`), 4 customizable DPI stages + Sniper DPI hold, live polling-rate & jitter auditor via Win32 `RegisterRawInputDevices`, and button/scroll remapping + rapid-fire burst clicker.
  - **FPV Drone Transmitters (EdgeTX / OpenTX / ExpressLRS):** Auto-detects USB HID joysticks (`CH1–CH16`), `AETR`/`TAER` channel mapping, unipolar (`0%–100%`) / bipolar (`-100%–+100%`) throttle modes, Betaflight-style `RC Rate` / `Super Rate` / `Expo` curves, and `SA–SE` switch macros.
  - **PlayStation & Switch Gyro-to-Mouse:** Parses DualSense, DualShock 4, and Switch Pro 6-axis IMU packets with sub-pixel `SendInput` mouse aiming, noise floor deadzone, smoothing, and flexible activation modes.
  - **Raw HID Packet Inspector:** Live 64-byte hex inspector, bit-level toggles, and min/max axis bounds for any USB/Bluetooth HID device.
- **Per-Game Auto-Profile Switching:** 500 ms foreground executable watcher (`GetForegroundWindow` + `QueryFullProcessImageNameW`) with sub-millisecond macro sequencer.
- **10 Zero-RAM SVG Themes:** Obsidian Dark, Clean Daylight, Minecraft Overworld, Gulf Coast Sunset, Aerospace HUD, Cyberpunk Night, Retro CRT Amber, Redline Carbon, Abyssal Ocean, and Nordic Frost.

## Development

```bash
npm install
npx tauri dev
```

## Release Build

```bash
npm run tauri build
```

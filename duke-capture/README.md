# Duke Capture

Zero-bloat **Instant Replay & Clip Studio** built with **Tauri v2**, **Rust**, and **Vanilla ES2022 JS/CSS**. Engineered as a fast, privacy-respecting replacement for Medal.tv, OBS Replay Buffer, and GeForce ShadowPlay.

## Key Capabilities

- **Zero-Wear In-RAM Circular Ring Buffer:** Captures your display via Windows **DXGI Desktop Duplication** (`IDXGIOutputDuplication` with seamless GDI fallback) into bounded compressed H.264 + AAC MP4 GOP chunks in RAM (`15s`, `30s`, `60s`, `120s`; `1080p`/`1440p`/`Native`; `8–20 Mbps`) so zero continuous writes hit your SSD until you trigger a save.
- **Built-In Windows Media Foundation HW + Optional FFmpeg Hardware Encoders:** Captures out-of-the-box using native Windows Media Foundation H.264 Hardware transforms, and automatically detects `ffmpeg.exe` (`h264_nvenc`, `h264_qsv`, `h264_amf`, `libx264`) or installs FFmpeg essentials in one click via Encrypted DoH.
- **Dual WASAPI Audio Capture (PC + Discord + Microphone):**
  - Captures system loopback (`AUDCLNT_STREAMFLAGS_LOOPBACK`) including all game audio and Discord voice chat without virtual audio cables.
  - Simultaneously mixes your chosen microphone (`IAudioCaptureClient`) with live VU meters, adjustable gains, adaptive RMS noise gate, and mute toggle into synchronized 192 kbps 48 kHz AAC stereo.
- **System-Wide Clip Keybinds & Acoustic Confirmation Chime:** Global hotkey + low-level keyboard/mouse hooks (`RegisterHotKey`, `WH_KEYBOARD_LL`, `WH_MOUSE_LL`) support keys, modifier combos, and `Mouse4` / `Mouse5` inside exclusive fullscreen games—firing an immediate synthesized harmonic two-tone WAV confirmation chime (`PlaySoundW`) with zero FPS-draining overlays.
- **Auto-Game Tagging, Fast Trimmer & 1-Click Discord Copy:** Queries the foreground window process when saving so clips organize cleanly into `Videos\Duke Clips\<Game Name>\<Game>_<Timestamp>.mp4`, includes an HTML5 studio with Start/End trimmer sliders, Win32 `CF_HDROP` clipboard copy (`Ctrl + V` in Discord), and `<10 MB` / `<25 MB` Discord size-target compression.
- **10 Zero-RAM Vector SVG Themes** & **Win32 Standby RAM Trimmer** (`K32EmptyWorkingSet`).

## Development

```bash
npm install
npm run tauri dev
```

### Release Build
```bash
npm run tauri build
```

## Portable Mode
Place an empty `portable.flag` file or a `data/` directory beside `duke-capture.exe` to isolate `config.json` locally.

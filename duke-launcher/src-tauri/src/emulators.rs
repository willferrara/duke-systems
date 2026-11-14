//! Portable Emulator Provisioner, Runtime Optimizer & Save-State Snapshot Orchestrator.
//!
//! Manages portable emulator runtimes inside `<app_data_dir>/emulators/` (`portable.txt` / `--config`)
//! across RetroArch + 9 Libretro cores and standalone engines (Dolphin, Cemu, Azahar, melonDS, mGBA,
//! DuckStation, PCSX2, PPSSPP), plus user-configured paths for Switch/PS3 dumps and custom setups.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};

use crate::platforms::{self};

#[derive(Debug, Clone, Copy)]
pub enum EmulatorSource {
    GitHubLatest {
        owner: &'static str,
        repo: &'static str,
        asset_keywords: &'static [&'static str],
    },
    DolphinStable,
    RetroArchCore {
        core_name: &'static str,
        fallback_core: Option<&'static str>,
    },
    CustomPathOnly,
}

#[derive(Debug, Clone, Copy)]
pub struct EmulatorSpec {
    pub id: &'static str,
    pub display_name: &'static str,
    pub dir_name: &'static str,
    pub exe_candidates: &'static [&'static str],
    pub portable_marker: Option<&'static str>,
    #[allow(dead_code)]
    pub core_name: Option<&'static str>,
    pub source: EmulatorSource,
    pub save_subdirs: &'static [&'static str],
    pub console_summary: &'static str,
    pub supports_auto_install: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EmulatorStatusDto {
    pub id: String,
    pub display_name: String,
    pub console_slugs: Vec<String>,
    pub console_names: Vec<String>,
    pub primary_platform_id: u16,
    pub status: String, // "ready" | "not_installed" | "custom_path"
    pub exe_path: String,
    pub custom_path: String,
    pub supports_auto_install: bool,
    pub caption: String,
    pub install_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EmulatorInstallProgress {
    pub emulator_id: String,
    pub console_name: String,
    pub stage: String,
    pub progress_pct: f64,
    pub message: String,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ResolvedEmulatorLaunch {
    pub emulator_id: String,
    pub emulator_name: String,
    pub exe_path: PathBuf,
    pub working_dir: PathBuf,
    pub args: Vec<String>,
    pub save_dirs: Vec<PathBuf>,
    pub base_emu_dir: PathBuf,
}

pub const EMULATOR_SPECS: &[EmulatorSpec] = &[
    EmulatorSpec {
        id: "retroarch_nes",
        display_name: "RetroArch (Mesen / FCEUmm NES)",
        dir_name: "retroarch",
        exe_candidates: &["retroarch.exe"],
        portable_marker: None,
        core_name: Some("mesen"),
        source: EmulatorSource::RetroArchCore {
            core_name: "mesen",
            fallback_core: Some("fceumm"),
        },
        save_subdirs: &["saves", "states"],
        console_summary: "NES",
        supports_auto_install: true,
    },
    EmulatorSpec {
        id: "retroarch_snes",
        display_name: "RetroArch (bsnes / Snes9x SNES)",
        dir_name: "retroarch",
        exe_candidates: &["retroarch.exe"],
        portable_marker: None,
        core_name: Some("bsnes"),
        source: EmulatorSource::RetroArchCore {
            core_name: "bsnes",
            fallback_core: Some("snes9x"),
        },
        save_subdirs: &["saves", "states"],
        console_summary: "SNES",
        supports_auto_install: true,
    },
    EmulatorSpec {
        id: "retroarch_n64",
        display_name: "RetroArch (Mupen64Plus-Next N64)",
        dir_name: "retroarch",
        exe_candidates: &["retroarch.exe"],
        portable_marker: None,
        core_name: Some("mupen64plus_next"),
        source: EmulatorSource::RetroArchCore {
            core_name: "mupen64plus_next",
            fallback_core: Some("parallel_n64"),
        },
        save_subdirs: &["saves", "states"],
        console_summary: "Nintendo 64",
        supports_auto_install: true,
    },
    EmulatorSpec {
        id: "dolphin",
        display_name: "Dolphin Emulator",
        dir_name: "dolphin",
        exe_candidates: &["Dolphin.exe", "Dolphin-x64/Dolphin.exe"],
        portable_marker: Some("portable.txt"),
        core_name: None,
        source: EmulatorSource::DolphinStable,
        save_subdirs: &["User/GC", "User/Wii", "User/StateSaves"],
        console_summary: "GameCube / Wii",
        supports_auto_install: true,
    },
    EmulatorSpec {
        id: "cemu",
        display_name: "Cemu",
        dir_name: "cemu",
        exe_candidates: &["Cemu.exe"],
        portable_marker: Some("portable.txt"),
        core_name: None,
        source: EmulatorSource::GitHubLatest {
            owner: "cemu-project",
            repo: "Cemu",
            asset_keywords: &["windows-x64", "win64", ".zip"],
        },
        save_subdirs: &["portable/mlc01/usr/save", "mlc01/usr/save", "saves", "states"],
        console_summary: "Wii U",
        supports_auto_install: true,
    },
    EmulatorSpec {
        id: "switch_custom",
        display_name: "Custom Switch Emulator",
        dir_name: "switch",
        exe_candidates: &["eden.exe", "ryujinx.exe", "Ryujinx.exe", "yuzu.exe", "suyu.exe"],
        portable_marker: None,
        core_name: None,
        source: EmulatorSource::CustomPathOnly,
        save_subdirs: &["user/nand/user/save", "portable/bis/user/save", "saves"],
        console_summary: "Nintendo Switch (Homebrew & User Dumps)",
        supports_auto_install: false,
    },
    EmulatorSpec {
        id: "mgba",
        display_name: "mGBA",
        dir_name: "mgba",
        exe_candidates: &["mGBA.exe", "mgba.exe", "mgba-sdl.exe"],
        portable_marker: Some("portable.ini"),
        core_name: None,
        source: EmulatorSource::GitHubLatest {
            owner: "mgba-emu",
            repo: "mgba",
            asset_keywords: &["win64", "x64", ".7z"],
        },
        save_subdirs: &["saves", "states"],
        console_summary: "Game Boy / Color / Advance",
        supports_auto_install: true,
    },
    EmulatorSpec {
        id: "melonds",
        display_name: "melonDS",
        dir_name: "melonds",
        exe_candidates: &["melonDS.exe"],
        portable_marker: Some("portable.txt"),
        core_name: None,
        source: EmulatorSource::GitHubLatest {
            owner: "melonDS-emu",
            repo: "melonDS",
            asset_keywords: &["windows", "x86_64", "win64", ".zip"],
        },
        save_subdirs: &["saves", "states"],
        console_summary: "Nintendo DS",
        supports_auto_install: true,
    },
    EmulatorSpec {
        id: "azahar",
        display_name: "Azahar",
        dir_name: "azahar",
        exe_candidates: &["azahar.exe", "azahar-qt.exe"],
        portable_marker: Some("user/portable.txt"),
        core_name: None,
        source: EmulatorSource::GitHubLatest {
            owner: "azahar-emu",
            repo: "azahar",
            asset_keywords: &["windows-msys2", "windows-msvc", "windows", ".zip"],
        },
        save_subdirs: &["user/sdmc", "user/states", "saves", "states"],
        console_summary: "Nintendo 3DS",
        supports_auto_install: true,
    },
    EmulatorSpec {
        id: "retroarch_vb",
        display_name: "RetroArch (Beetle VB)",
        dir_name: "retroarch",
        exe_candidates: &["retroarch.exe"],
        portable_marker: None,
        core_name: Some("mednafen_vb"),
        source: EmulatorSource::RetroArchCore {
            core_name: "mednafen_vb",
            fallback_core: None,
        },
        save_subdirs: &["saves", "states"],
        console_summary: "Virtual Boy",
        supports_auto_install: true,
    },
    EmulatorSpec {
        id: "duckstation",
        display_name: "DuckStation",
        dir_name: "duckstation",
        exe_candidates: &[
            "duckstation-qt-x64-ReleaseLTCG.exe",
            "duckstation-qt-x64.exe",
            "duckstation-nogui-x64-ReleaseLTCG.exe",
        ],
        portable_marker: Some("portable.txt"),
        core_name: None,
        source: EmulatorSource::GitHubLatest {
            owner: "stenzek",
            repo: "duckstation",
            asset_keywords: &["windows-x64-release", "windows-x64", ".zip"],
        },
        save_subdirs: &["memcards", "savestates", "saves", "states"],
        console_summary: "Sony PlayStation (PS1)",
        supports_auto_install: true,
    },
    EmulatorSpec {
        id: "pcsx2",
        display_name: "PCSX2",
        dir_name: "pcsx2",
        exe_candidates: &[
            "pcsx2-qt.exe",
            "pcsx2-avx2-qt.exe",
            "pcsx2x64.exe",
            "pcsx2.exe",
        ],
        portable_marker: Some("portable.ini"),
        core_name: None,
        source: EmulatorSource::GitHubLatest {
            owner: "PCSX2",
            repo: "pcsx2",
            asset_keywords: &["windows-x64-Qt", "windows-x64", ".7z"],
        },
        save_subdirs: &["memcards", "sstates", "saves", "states"],
        console_summary: "Sony PlayStation 2 (PS2)",
        supports_auto_install: true,
    },
    EmulatorSpec {
        id: "ppsspp",
        display_name: "PPSSPP",
        dir_name: "ppsspp",
        exe_candidates: &["PPSSPPWindows64.exe", "PPSSPPWindows.exe"],
        portable_marker: Some("installed.txt"),
        core_name: None,
        source: EmulatorSource::GitHubLatest {
            owner: "hrydgard",
            repo: "ppsspp",
            asset_keywords: &["PPSSPPWin", "windows", ".zip"],
        },
        save_subdirs: &["memstick/PSP/SAVEDATA", "memstick/PSP/PPSSPP_STATE", "saves", "states"],
        console_summary: "PlayStation Portable (PSP)",
        supports_auto_install: true,
    },
    EmulatorSpec {
        id: "ps3_custom",
        display_name: "RPCS3 / Custom PS3 Emulator",
        dir_name: "ps3",
        exe_candidates: &["rpcs3.exe"],
        portable_marker: None,
        core_name: None,
        source: EmulatorSource::CustomPathOnly,
        save_subdirs: &["dev_hdd0/home/00000001/savedata", "saves"],
        console_summary: "PlayStation 3 (Homebrew & User Dumps)",
        supports_auto_install: false,
    },
    EmulatorSpec {
        id: "retroarch_genesis",
        display_name: "RetroArch (Genesis Plus GX)",
        dir_name: "retroarch",
        exe_candidates: &["retroarch.exe"],
        portable_marker: None,
        core_name: Some("genesis_plus_gx"),
        source: EmulatorSource::RetroArchCore {
            core_name: "genesis_plus_gx",
            fallback_core: Some("picodrive"),
        },
        save_subdirs: &["saves", "states"],
        console_summary: "Sega Genesis / Mega Drive",
        supports_auto_install: true,
    },
    EmulatorSpec {
        id: "retroarch_sms",
        display_name: "RetroArch (Genesis Plus GX SMS)",
        dir_name: "retroarch",
        exe_candidates: &["retroarch.exe"],
        portable_marker: None,
        core_name: Some("genesis_plus_gx"),
        source: EmulatorSource::RetroArchCore {
            core_name: "genesis_plus_gx",
            fallback_core: Some("smsplus"),
        },
        save_subdirs: &["saves", "states"],
        console_summary: "Sega Master System",
        supports_auto_install: true,
    },
    EmulatorSpec {
        id: "retroarch_saturn",
        display_name: "RetroArch (Beetle Saturn)",
        dir_name: "retroarch",
        exe_candidates: &["retroarch.exe"],
        portable_marker: None,
        core_name: Some("mednafen_saturn"),
        source: EmulatorSource::RetroArchCore {
            core_name: "mednafen_saturn",
            fallback_core: Some("kronos"),
        },
        save_subdirs: &["saves", "states"],
        console_summary: "Sega Saturn",
        supports_auto_install: true,
    },
    EmulatorSpec {
        id: "retroarch_dreamcast",
        display_name: "RetroArch (Flycast Dreamcast)",
        dir_name: "retroarch",
        exe_candidates: &["retroarch.exe"],
        portable_marker: None,
        core_name: Some("flycast"),
        source: EmulatorSource::RetroArchCore {
            core_name: "flycast",
            fallback_core: None,
        },
        save_subdirs: &["saves", "states"],
        console_summary: "Sega Dreamcast",
        supports_auto_install: true,
    },
    EmulatorSpec {
        id: "retroarch_atari2600",
        display_name: "RetroArch (Stella Atari 2600)",
        dir_name: "retroarch",
        exe_candidates: &["retroarch.exe"],
        portable_marker: None,
        core_name: Some("stella"),
        source: EmulatorSource::RetroArchCore {
            core_name: "stella",
            fallback_core: Some("stella2014"),
        },
        save_subdirs: &["saves", "states"],
        console_summary: "Atari 2600",
        supports_auto_install: true,
    },
    EmulatorSpec {
        id: "retroarch_arcade",
        display_name: "RetroArch (FinalBurn Neo Arcade)",
        dir_name: "retroarch",
        exe_candidates: &["retroarch.exe"],
        portable_marker: None,
        core_name: Some("fbneo"),
        source: EmulatorSource::RetroArchCore {
            core_name: "fbneo",
            fallback_core: Some("mame2003_plus"),
        },
        save_subdirs: &["saves", "states"],
        console_summary: "Neo Geo / Arcade (MAME)",
        supports_auto_install: true,
    },
    EmulatorSpec {
        id: "retroarch_pce",
        display_name: "RetroArch (Beetle PCE Fast)",
        dir_name: "retroarch",
        exe_candidates: &["retroarch.exe"],
        portable_marker: None,
        core_name: Some("mednafen_pce_fast"),
        source: EmulatorSource::RetroArchCore {
            core_name: "mednafen_pce_fast",
            fallback_core: Some("mednafen_pce"),
        },
        save_subdirs: &["saves", "states"],
        console_summary: "PC Engine / TurboGrafx-16",
        supports_auto_install: true,
    },
];

pub fn emulator_spec_by_id(id_or_console: &str) -> Option<&'static EmulatorSpec> {
    let key = id_or_console.trim().to_ascii_lowercase();
    if key.is_empty() {
        return None;
    }
    if let Some(exact) = EMULATOR_SPECS.iter().find(|s| s.id == key) {
        return Some(exact);
    }
    if let Some(plat) = platforms::platform_from_slug_or_name(&key) {
        return EMULATOR_SPECS
            .iter()
            .find(|s| s.id == plat.default_emulator_id);
    }
    None
}

pub fn emulators_root_dir() -> Result<PathBuf, String> {
    let data_root = crate::resolve_data_root_dir_standalone()?;
    Ok(data_root.join("emulators"))
}

fn find_exe_recursive(root: &Path, candidates: &[&str], max_depth: usize) -> Option<PathBuf> {
    for &rel in candidates {
        let direct = root.join(rel);
        if direct.is_file() {
            return Some(direct);
        }
    }
    if !root.is_dir() {
        return None;
    }
    for entry in walkdir::WalkDir::new(root)
        .max_depth(max_depth)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let fname = entry.file_name().to_string_lossy();
        if candidates
            .iter()
            .any(|c| Path::new(c).file_name().and_then(|n| n.to_str()).unwrap_or(c).eq_ignore_ascii_case(&fname))
        {
            return Some(entry.path().to_path_buf());
        }
    }
    None
}

fn retroarch_core_path(ra_dir: &Path, core_name: &str, fallback_core: Option<&str>) -> Option<PathBuf> {
    let cores_dir = ra_dir.join("cores");
    let primary = cores_dir.join(format!("{}_libretro.dll", core_name));
    if primary.is_file() {
        return Some(primary);
    }
    if let Some(fb) = fallback_core {
        let fb_path = cores_dir.join(format!("{}_libretro.dll", fb));
        if fb_path.is_file() {
            return Some(fb_path);
        }
    }
    None
}

/// Builds clean fullscreen + XInput controller CLI arguments for each supported portable emulator.
pub fn build_emulator_cli_args(
    emu_id: &str,
    rom_path: &Path,
    emu_dir: &Path,
    core_dll: Option<&Path>,
) -> Vec<String> {
    let rom_str = rom_path.to_string_lossy().into_owned();
    match emu_id {
        id if id.starts_with("retroarch_") => {
            let mut args = vec![
                "--config".to_string(),
                emu_dir.join("retroarch.cfg").to_string_lossy().into_owned(),
            ];
            if let Some(core) = core_dll {
                args.push("-L".to_string());
                args.push(core.to_string_lossy().into_owned());
            }
            args.push("-f".to_string());
            args.push(rom_str);
            args
        }
        "dolphin" => vec![
            "-b".to_string(),
            "-u".to_string(),
            emu_dir.join("User").to_string_lossy().into_owned(),
            "-e".to_string(),
            rom_str,
        ],
        "cemu" => vec!["-f".to_string(), "-g".to_string(), rom_str],
        "azahar" | "melonds" | "mgba" => vec!["-f".to_string(), rom_str],
        "duckstation" | "pcsx2" => vec!["-fullscreen".to_string(), "--".to_string(), rom_str],
        "ppsspp" => vec!["--fullscreen".to_string(), rom_str],
        "switch_custom" => vec!["-f".to_string(), "-g".to_string(), rom_str],
        "ps3_custom" => vec!["--no-gui".to_string(), rom_str],
        _ => vec![rom_str],
    }
}

/// Ensures `portable.txt` / `portable.ini` and baseline fullscreen + XInput configuration exist
/// inside `<app_data_dir>/emulators/<dir_name>/` so zero settings leak into Windows Documents/Registry.
pub fn write_portable_emulator_config(spec: &EmulatorSpec, emu_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(emu_dir)
        .map_err(|e| format!("Failed creating emulator dir {}: {}", emu_dir.display(), e))?;
    for &sub in spec.save_subdirs {
        let _ = fs::create_dir_all(emu_dir.join(sub));
    }

    if let Some(marker) = spec.portable_marker {
        let marker_path = emu_dir.join(marker);
        if let Some(parent) = marker_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if !marker_path.exists() {
            let content = if marker.ends_with(".ini") {
                "[Portable]\nEnabled=1\n"
            } else {
                ""
            };
            let _ = fs::write(&marker_path, content);
        }
    }

    if spec.id.starts_with("retroarch_") {
        let ra_cfg = emu_dir.join("retroarch.cfg");
        if !ra_cfg.exists() {
            let saves = emu_dir.join("saves").to_string_lossy().replace('\\', "/");
            let states = emu_dir.join("states").to_string_lossy().replace('\\', "/");
            let cores = emu_dir.join("cores").to_string_lossy().replace('\\', "/");
            let cfg_body = format!(
                "video_fullscreen = \"true\"\n\
                 input_driver = \"dinput\"\n\
                 input_joypad_driver = \"xinput\"\n\
                 input_autodetect_enable = \"true\"\n\
                 savefile_directory = \"{}\"\n\
                 savestate_directory = \"{}\"\n\
                 libretro_directory = \"{}\"\n\
                 quit_press_twice = \"false\"\n",
                saves, states, cores
            );
            let _ = fs::write(&ra_cfg, cfg_body);
        }
    } else if spec.id == "dolphin" {
        let dolphin_ini = emu_dir.join("User").join("Config").join("Dolphin.ini");
        if !dolphin_ini.exists() {
            if let Some(parent) = dolphin_ini.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let body = "[Display]\nFullscreen = True\nKeepWindowOnTop = False\n\
                        [Interface]\nConfirmStop = False\nUsePanicHandlers = False\n";
            let _ = fs::write(&dolphin_ini, body);
        }
    }
    Ok(())
}

/// Resolves an installed or user-configured emulator + portable launch arguments for `platform_id`.
pub fn resolve_emulator_launch(
    platform_id: u16,
    emulator_id_override: Option<&str>,
    rom_path: &Path,
) -> Result<ResolvedEmulatorLaunch, String> {
    let plat = platforms::platform_by_id(platform_id);
    if plat.id == 0 {
        return Err("PC games do not use console emulation.".to_string());
    }

    let settings = crate::load_settings_standalone();
    let target_emu_id = emulator_id_override
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(plat.default_emulator_id);
    let spec = emulator_spec_by_id(target_emu_id)
        .ok_or_else(|| format!("Unsupported console '{}'.", plat.display_name))?;

    let root = emulators_root_dir()?;
    let emu_dir = root.join(spec.dir_name);

    // Check custom override by console slug, emulator id, or dir_name first
    let custom_exe = settings
        .custom_emulator_paths
        .get(plat.slug)
        .or_else(|| settings.custom_emulator_paths.get(spec.id))
        .or_else(|| settings.custom_emulator_paths.get(spec.dir_name))
        .map(|s| PathBuf::from(s.trim()))
        .filter(|p| p.is_file());

    let exe_path = if let Some(custom) = custom_exe {
        custom
    } else if let Some(found) = find_exe_recursive(&emu_dir, spec.exe_candidates, 3) {
        found
    } else {
        return Err(format!(
            "EMULATOR_NOT_INSTALLED|{}|{}|{}",
            plat.slug, plat.display_name, spec.display_name
        ));
    };

    let core_dll = if let EmulatorSource::RetroArchCore {
        core_name,
        fallback_core,
    } = spec.source
    {
        let ra_base = exe_path.parent().unwrap_or(&emu_dir);
        let found_core = retroarch_core_path(ra_base, core_name, fallback_core).or_else(|| {
            retroarch_core_path(&emu_dir, core_name, fallback_core)
        });
        if found_core.is_none() {
            return Err(format!(
                "EMULATOR_NOT_INSTALLED|{}|{}|{}",
                plat.slug, plat.display_name, spec.display_name
            ));
        }
        found_core
    } else {
        None
    };

    let working_dir = exe_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| emu_dir.clone());
    let effective_base_dir = if emu_dir.is_dir() {
        emu_dir.clone()
    } else {
        working_dir.clone()
    };

    let _ = write_portable_emulator_config(spec, &effective_base_dir);
    let args = build_emulator_cli_args(spec.id, rom_path, &effective_base_dir, core_dll.as_deref());
    let save_dirs = spec
        .save_subdirs
        .iter()
        .map(|sub| effective_base_dir.join(sub))
        .collect();

    Ok(ResolvedEmulatorLaunch {
        emulator_id: spec.id.to_string(),
        emulator_name: spec.display_name.to_string(),
        exe_path,
        working_dir,
        args,
        save_dirs,
        base_emu_dir: effective_base_dir,
    })
}

/// Returns save/state directories for a console library entry so `snapshots.rs`, `ludusavi.rs`,
/// and `swarm.rs` seamlessly back up, restore, and mesh-sync console saves & save states.
pub fn resolve_console_save_dirs_for_game(
    platform_id: u16,
    emulator_id_hint: &str,
) -> Option<(PathBuf, Vec<&'static str>, Vec<PathBuf>)> {
    if platform_id == 0 {
        return None;
    }
    let plat = platforms::platform_by_id(platform_id);
    let target_id = if emulator_id_hint.trim().is_empty() {
        plat.default_emulator_id
    } else {
        emulator_id_hint.trim()
    };
    let spec = emulator_spec_by_id(target_id)?;
    let root = emulators_root_dir().ok()?;
    let settings = crate::load_settings_standalone();
    let custom_base = settings
        .custom_emulator_paths
        .get(plat.slug)
        .or_else(|| settings.custom_emulator_paths.get(spec.id))
        .and_then(|p| Path::new(p.trim()).parent().map(Path::to_path_buf));

    let base_dir = custom_base.unwrap_or_else(|| root.join(spec.dir_name));
    let paths: Vec<PathBuf> = spec.save_subdirs.iter().map(|s| base_dir.join(s)).collect();
    Some((base_dir, spec.save_subdirs.to_vec(), paths))
}

/// Creates an automatic `.duke-cas/` BLAKE3 hardlink snapshot of emulator `saves/` and `states/`
/// right after an emulator session exits (`0 MB` extra disk space for unchanged blocks).
pub fn snapshot_console_saves_on_exit(
    game_id: &str,
    platform_id: u16,
    emulator_id: &str,
    label: Option<&str>,
) {
    let Some((base_dir, save_subdirs, _)) =
        resolve_console_save_dirs_for_game(platform_id, emulator_id)
    else {
        return;
    };
    let has_any_save = save_subdirs
        .iter()
        .any(|sub| {
            let p = base_dir.join(sub);
            p.is_dir() && walkdir::WalkDir::new(&p).max_depth(6).into_iter().filter_map(|e| e.ok()).any(|e| e.file_type().is_file())
        });
    if !has_any_save {
        return;
    }
    let cas_dir = base_dir.join(crate::snapshots::CAS_DIR_NAME);
    let _ = crate::snapshots::create_snapshot_in_dir_filtered(
        game_id,
        &base_dir,
        &cas_dir,
        label,
        Some(&save_subdirs),
    );
}

fn pick_best_github_windows_asset(assets: &[serde_json::Value], keywords: &[&str]) -> Option<(String, Option<String>)> {
    let mut best: Option<(i32, String, Option<String>)> = None;
    for asset in assets {
        let name = asset.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let url = asset
            .get("browser_download_url")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if name.is_empty() || url.is_empty() {
            continue;
        }
        let low = name.to_ascii_lowercase();
        if low.ends_with(".exe")
            || low.ends_with(".msi")
            || low.ends_with(".appimage")
            || low.ends_with(".dmg")
            || low.ends_with(".tar.gz")
            || low.ends_with(".tar.xz")
            || low.ends_with(".pdb")
            || low.contains("symbols")
            || low.contains("debug")
            || low.contains("linux")
            || low.contains("macos")
            || low.contains("android")
            || low.contains("arm64")
        {
            continue;
        }
        if !low.ends_with(".zip") && !low.ends_with(".7z") {
            continue;
        }
        let mut score = 10i32;
        for &kw in keywords {
            if low.contains(&kw.to_ascii_lowercase()) {
                score += 30;
            }
        }
        if low.contains("win") || low.contains("x64") || low.contains("x86_64") {
            score += 25;
        }
        if low.contains("qt") || low.contains("portable") || low.contains("msvc") {
            score += 10;
        }
        let sha256_digest = asset
            .get("digest")
            .and_then(|v| v.as_str())
            .and_then(|d| d.strip_prefix("sha256:"))
            .map(|h| h.trim().to_ascii_lowercase());

        match &best {
            Some((b_score, _, _)) if *b_score >= score => {}
            _ => best = Some((score, url.to_string(), sha256_digest)),
        }
    }
    best.map(|(_, u, h)| (u, h))
}

async fn resolve_github_release_asset(
    owner: &str,
    repo: &str,
    keywords: &[&str],
) -> Result<(String, Option<String>), String> {
    let api_url = format!("https://api.github.com/repos/{}/{}/releases?per_page=6", owner, repo);
    let client = duke_core::net_doh::build_doh_client_for_url(&api_url, false, true).await;
    let resp = client
        .get(&api_url)
        .header("User-Agent", "Duke-Launcher/1.0")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("Failed querying {} releases: {}", repo, e))?;
    if !resp.status().is_success() {
        return Err(format!("GitHub API returned {} for {}/{}", resp.status(), owner, repo));
    }
    let releases: Vec<serde_json::Value> = resp
        .json()
        .await
        .map_err(|e| format!("Invalid GitHub release JSON: {}", e))?;

    for rel in releases {
        if rel.get("draft").and_then(|v| v.as_bool()).unwrap_or(false) {
            continue;
        }
        if let Some(assets) = rel.get("assets").and_then(|v| v.as_array()) {
            if let Some((url, mut sha)) = pick_best_github_windows_asset(assets, keywords) {
                if sha.is_none() {
                    // Check for companion `.sha256` or `SHA256SUMS` asset in same release
                    let file_name = url.rsplit('/').next().unwrap_or("");
                    if let Some(sum_asset) = assets.iter().find(|a| {
                        let aname = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        aname.ends_with(".sha256")
                            || aname.eq_ignore_ascii_case("SHA256SUMS")
                            || aname.eq_ignore_ascii_case("sha256sums.txt")
                    }) {
                        if let Some(sum_url) = sum_asset.get("browser_download_url").and_then(|v| v.as_str()) {
                            if let Ok(sr) = client.get(sum_url).header("User-Agent", "Duke-Launcher/1.0").send().await {
                                if let Ok(txt) = sr.text().await {
                                    sha = extract_sha256_for_filename(&txt, file_name);
                                }
                            }
                        }
                    }
                }
                return Ok((url, sha));
            }
        }
    }
    Err(format!("No Windows .zip or .7z portable package found in {}/{} releases.", owner, repo))
}

fn extract_sha256_for_filename(text: &str, file_name: &str) -> Option<String> {
    for line in text.lines() {
        let tr = line.trim();
        if tr.is_empty() {
            continue;
        }
        let hex_cand = tr.split_whitespace().next().unwrap_or("");
        if hex_cand.len() == 64
            && hex_cand.chars().all(|c| c.is_ascii_hexdigit())
            && (tr.contains(file_name) || text.lines().count() == 1)
        {
            return Some(hex_cand.to_ascii_lowercase());
        }
    }
    None
}

async fn resolve_dolphin_windows_archive() -> Result<(String, Option<String>), String> {
    let page_url = "https://dolphin-emu.org/download/";
    let client = duke_core::net_doh::build_doh_client_for_url(page_url, false, true).await;
    let resp = client
        .get(page_url)
        .header("User-Agent", "Duke-Launcher/1.0")
        .send()
        .await
        .map_err(|e| format!("Failed checking Dolphin downloads: {}", e))?;
    let html = resp.text().await.map_err(|e| e.to_string())?;
    let mut best_url: Option<String> = None;
    for chunk in html.split("https://dl.dolphin-emu.org/").skip(1) {
        let end = chunk
            .find(['"', '\'', '<', '>', ' '])
            .unwrap_or(chunk.len());
        let candidate = format!("https://dl.dolphin-emu.org/{}", &chunk[..end]);
        let low = candidate.to_ascii_lowercase();
        if low.ends_with("-x64.7z") || low.ends_with("-x64.zip") {
            if candidate.contains("/releases/") {
                best_url = Some(candidate);
                break;
            } else if best_url.is_none() {
                best_url = Some(candidate);
            }
        }
    }
    best_url
        .map(|u| (u, None))
        .ok_or_else(|| "Could not locate official Dolphin Windows x64 archive on dolphin-emu.org.".to_string())
}

async fn resolve_retroarch_windows_archive() -> Result<String, String> {
    let api_url = "https://api.github.com/repos/libretro/RetroArch/releases/latest";
    let client = duke_core::net_doh::build_doh_client_for_url(api_url, false, true).await;
    if let Ok(resp) = client
        .get(api_url)
        .header("User-Agent", "Duke-Launcher/1.0")
        .send()
        .await
    {
        if let Ok(json) = resp.json::<serde_json::Value>().await {
            if let Some(tag) = json.get("tag_name").and_then(|v| v.as_str()) {
                let ver = tag.trim().trim_start_matches('v');
                if !ver.is_empty() {
                    let stable_url = format!(
                        "https://buildbot.libretro.com/stable/{}/windows/x86_64/RetroArch.7z",
                        ver
                    );
                    if let Ok(head) = client.head(&stable_url).send().await {
                        if head.status().is_success() {
                            return Ok(stable_url);
                        }
                    }
                }
            }
        }
    }
    Ok("https://buildbot.libretro.com/nightly/windows/x86_64/RetroArch.7z".to_string())
}

/// Multi-worker (up to 8 parallel AIMD-inspired range streams via DoH) downloader with streaming
/// `.part` positional `seek_write` and mandatory SHA-256 verification when an upstream digest exists.
async fn download_archive_aimd_verified(
    app: &AppHandle,
    emu_id: &str,
    console_name: &str,
    url: &str,
    dest_path: &Path,
    expected_sha256: Option<&str>,
) -> Result<String, String> {
    let client = Arc::new(duke_core::net_doh::build_doh_client_for_url(url, true, true).await);
    if let Some(parent) = dest_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let part_path = dest_path.with_extension("part");
    let _ = fs::remove_file(&part_path);

    let probe = client
        .head(url)
        .header("User-Agent", "Duke-Launcher/1.0")
        .send()
        .await;
    let (total_bytes, accept_ranges) = match probe {
        Ok(ref r) if r.status().is_success() => {
            let len = r
                .headers()
                .get(reqwest::header::CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let ranges = r
                .headers()
                .get(reqwest::header::ACCEPT_RANGES)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.eq_ignore_ascii_case("bytes"))
                .unwrap_or(false);
            (len, ranges)
        }
        _ => (0, false),
    };

    let worker_count = if accept_ranges && total_bytes > 2 * 1024 * 1024 {
        8usize
    } else {
        1usize
    };

    if worker_count > 1 {
        let file = File::create(&part_path).map_err(|e| e.to_string())?;
        file.set_len(total_bytes).map_err(|e| e.to_string())?;
        drop(file);

        let downloaded = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let chunk_sz = total_bytes / worker_count as u64;
        let mut handles = Vec::with_capacity(worker_count);

        for idx in 0..worker_count {
            let start = idx as u64 * chunk_sz;
            let end = if idx == worker_count - 1 {
                total_bytes - 1
            } else {
                (idx as u64 + 1) * chunk_sz - 1
            };
            let cl = client.clone();
            let u = url.to_string();
            let p = part_path.clone();
            let dl = downloaded.clone();
            let app_c = app.clone();
            let eid = emu_id.to_string();
            let cname = console_name.to_string();
            handles.push(tokio::spawn(async move {
                use std::os::windows::fs::FileExt;

                let mut resp = cl
                    .get(&u)
                    .header("User-Agent", "Duke-Launcher/1.0")
                    .header(reqwest::header::RANGE, format!("bytes={}-{}", start, end))
                    .send()
                    .await
                    .map_err(|e| e.to_string())?;
                if !resp.status().is_success() {
                    return Err(format!("Range GET failed with {}", resp.status()));
                }
                let f = fs::OpenOptions::new()
                    .write(true)
                    .open(&p)
                    .map_err(|e| e.to_string())?;
                let mut offset = start;
                while let Some(bytes) = resp.chunk().await.map_err(|e| e.to_string())? {
                    let mut written = 0usize;
                    while written < bytes.len() {
                        let n = f
                            .seek_write(&bytes[written..], offset + written as u64)
                            .map_err(|e| e.to_string())?;
                        written += n;
                    }
                    offset += bytes.len() as u64;
                    let acc = dl.fetch_add(bytes.len() as u64, std::sync::atomic::Ordering::Relaxed)
                        + bytes.len() as u64;
                    if idx == 0 || acc == total_bytes {
                        let pct = ((acc as f64 / total_bytes as f64) * 80.0).min(80.0);
                        let _ = app_c.emit(
                            "emulator-install-progress",
                            EmulatorInstallProgress {
                                emulator_id: eid.clone(),
                                console_name: cname.clone(),
                                stage: "downloading".to_string(),
                                progress_pct: pct,
                                message: format!(
                                    "Downloading {} ({:.0}%)...",
                                    cname,
                                    (acc as f64 / total_bytes as f64) * 100.0
                                ),
                            },
                        );
                    }
                }
                Ok::<(), String>(())
            }));
        }

        for h in handles {
            h.await.map_err(|e| e.to_string())??;
        }
    } else {
        let mut resp = client
            .get(url)
            .header("User-Agent", "Duke-Launcher/1.0")
            .send()
            .await
            .map_err(|e| format!("Download failed: {}", e))?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {} downloading emulator package", resp.status()));
        }
        let known_total = resp.content_length().unwrap_or(total_bytes);
        let mut f = File::create(&part_path).map_err(|e| e.to_string())?;
        let mut acc = 0u64;
        while let Some(bytes) = resp.chunk().await.map_err(|e| e.to_string())? {
            f.write_all(&bytes).map_err(|e| e.to_string())?;
            acc += bytes.len() as u64;
            if known_total > 0 {
                let pct = ((acc as f64 / known_total as f64) * 80.0).min(80.0);
                let _ = app.emit(
                    "emulator-install-progress",
                    EmulatorInstallProgress {
                        emulator_id: emu_id.to_string(),
                        console_name: console_name.to_string(),
                        stage: "downloading".to_string(),
                        progress_pct: pct,
                        message: format!(
                            "Downloading {} ({:.0}%)...",
                            console_name,
                            (acc as f64 / known_total as f64) * 100.0
                        ),
                    },
                );
            }
        }
        f.flush().map_err(|e| e.to_string())?;
    }

    // Verify SHA-256 checksum
    let part_c = part_path.clone();
    let expected_owned = expected_sha256.map(|s| s.to_ascii_lowercase());
    let actual_sha = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let mut f = File::open(&part_c).map_err(|e| e.to_string())?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 65536];
        loop {
            let n = f.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        let digest = hex::encode(hasher.finalize());
        if let Some(exp) = expected_owned {
            if digest != exp {
                return Err(format!(
                    "SHA-256 verification failed (expected {}, got {})",
                    exp, digest
                ));
            }
        }
        Ok(digest)
    })
    .await
    .map_err(|e| e.to_string())??;

    if dest_path.exists() {
        let _ = fs::remove_file(dest_path);
    }
    fs::rename(&part_path, dest_path).map_err(|e| e.to_string())?;
    Ok(actual_sha)
}

async fn install_spec_portable(
    app: &AppHandle,
    spec: &'static EmulatorSpec,
    console_name: &str,
) -> Result<String, String> {
    let root = emulators_root_dir()?;
    let target_dir = root.join(spec.dir_name);
    let cache_dir = root.join(".downloads");
    fs::create_dir_all(&cache_dir).map_err(|e| e.to_string())?;

    let emit_stage = |stage: &str, pct: f64, msg: &str| {
        let _ = app.emit(
            "emulator-install-progress",
            EmulatorInstallProgress {
                emulator_id: spec.id.to_string(),
                console_name: console_name.to_string(),
                stage: stage.to_string(),
                progress_pct: pct,
                message: msg.to_string(),
            },
        );
    };

    match spec.source {
        EmulatorSource::CustomPathOnly => {
            Err(format!(
                "{} uses a custom user-provided executable path. Click Browse to point Duke Launcher to your emulator .exe.",
                spec.display_name
            ))
        }
        EmulatorSource::RetroArchCore {
            core_name,
            fallback_core,
        } => {
            if find_exe_recursive(&target_dir, &["retroarch.exe"], 2).is_none() {
                emit_stage("resolving", 5.0, "Locating official RetroArch portable build...");
                let ra_url = resolve_retroarch_windows_archive().await?;
                let ra_archive = cache_dir.join("RetroArch.7z");
                download_archive_aimd_verified(
                    app,
                    spec.id,
                    "RetroArch",
                    &ra_url,
                    &ra_archive,
                    None,
                )
                .await?;
                emit_stage("extracting", 82.0, "Extracting RetroArch portable runtime...");
                let arch_c = ra_archive.clone();
                let dir_c = target_dir.clone();
                tokio::task::spawn_blocking(move || {
                    crate::extract_archive_to_folder_sync(&arch_c, &dir_c)
                })
                .await
                .map_err(|e| e.to_string())??;
                let _ = fs::remove_file(&ra_archive);
                // Flatten nested RetroArch-Win64/ subfolder if archive wrapped inside one directory
                flatten_single_subdir_if_needed(&target_dir, "retroarch.exe");
            }

            if retroarch_core_path(&target_dir, core_name, fallback_core).is_none() {
                emit_stage(
                    "downloading",
                    85.0,
                    &format!("Fetching {} Libretro core...", console_name),
                );
                let cores_dir = target_dir.join("cores");
                fs::create_dir_all(&cores_dir).map_err(|e| e.to_string())?;
                let mut core_ok = false;
                for cname in [Some(core_name), fallback_core].into_iter().flatten() {
                    let core_url = format!(
                        "https://buildbot.libretro.com/nightly/windows/x86_64/latest/{}_libretro.dll.zip",
                        cname
                    );
                    let core_zip = cache_dir.join(format!("{}_libretro.dll.zip", cname));
                    if download_archive_aimd_verified(
                        app,
                        spec.id,
                        console_name,
                        &core_url,
                        &core_zip,
                        None,
                    )
                    .await
                    .is_ok()
                    {
                        let zc = core_zip.clone();
                        let cc = cores_dir.clone();
                        let res = tokio::task::spawn_blocking(move || {
                            crate::extract_archive_to_folder_sync(&zc, &cc)
                        })
                        .await
                        .map_err(|e| e.to_string())?;
                        let _ = fs::remove_file(&core_zip);
                        if res.is_ok() {
                            core_ok = true;
                            break;
                        }
                    }
                }
                if !core_ok {
                    return Err(format!("Failed to download Libretro core for {}.", console_name));
                }
            }

            write_portable_emulator_config(spec, &target_dir)?;
            let exe = find_exe_recursive(&target_dir, spec.exe_candidates, 2)
                .ok_or_else(|| "retroarch.exe missing after extraction.".to_string())?;
            emit_stage("ready", 100.0, &format!("{} ready!", console_name));
            Ok(exe.to_string_lossy().into_owned())
        }
        EmulatorSource::DolphinStable => {
            emit_stage("resolving", 5.0, "Locating latest stable Dolphin x64 release...");
            let (url, sha) = resolve_dolphin_windows_archive().await?;
            let fname = url.rsplit('/').next().unwrap_or("dolphin.7z");
            let arch = cache_dir.join(fname);
            download_archive_aimd_verified(
                app,
                spec.id,
                spec.display_name,
                &url,
                &arch,
                sha.as_deref(),
            )
            .await?;
            emit_stage("extracting", 85.0, "Extracting Dolphin portable build...");
            let arch_c = arch.clone();
            let dir_c = target_dir.clone();
            tokio::task::spawn_blocking(move || {
                crate::extract_archive_to_folder_sync(&arch_c, &dir_c)
            })
            .await
            .map_err(|e| e.to_string())??;
            let _ = fs::remove_file(&arch);
            flatten_single_subdir_if_needed(&target_dir, "Dolphin.exe");
            write_portable_emulator_config(spec, &target_dir)?;
            let exe = find_exe_recursive(&target_dir, spec.exe_candidates, 3)
                .ok_or_else(|| "Dolphin.exe not found after extraction.".to_string())?;
            emit_stage("ready", 100.0, &format!("{} ready!", spec.display_name));
            Ok(exe.to_string_lossy().into_owned())
        }
        EmulatorSource::GitHubLatest {
            owner,
            repo,
            asset_keywords,
        } => {
            emit_stage(
                "resolving",
                5.0,
                &format!("Resolving latest {} GitHub release...", spec.display_name),
            );
            let (url, sha) = resolve_github_release_asset(owner, repo, asset_keywords).await?;
            let fname = url.rsplit('/').next().unwrap_or("emulator.zip");
            let arch = cache_dir.join(fname);
            download_archive_aimd_verified(
                app,
                spec.id,
                spec.display_name,
                &url,
                &arch,
                sha.as_deref(),
            )
            .await?;
            emit_stage("extracting", 85.0, &format!("Extracting {}...", spec.display_name));
            let arch_c = arch.clone();
            let dir_c = target_dir.clone();
            tokio::task::spawn_blocking(move || {
                crate::extract_archive_to_folder_sync(&arch_c, &dir_c)
            })
            .await
            .map_err(|e| e.to_string())??;
            let _ = fs::remove_file(&arch);
            if let Some(first_exe) = spec.exe_candidates.first() {
                flatten_single_subdir_if_needed(&target_dir, first_exe);
            }
            write_portable_emulator_config(spec, &target_dir)?;
            let exe = find_exe_recursive(&target_dir, spec.exe_candidates, 3)
                .ok_or_else(|| format!("{} executable not found after extraction.", spec.display_name))?;
            emit_stage("ready", 100.0, &format!("{} ready!", spec.display_name));
            Ok(exe.to_string_lossy().into_owned())
        }
    }
}

fn flatten_single_subdir_if_needed(dir: &Path, expected_exe: &str) {
    if dir.join(expected_exe).is_file() {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let subdirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|x| x.path()))
        .filter(|p| p.is_dir())
        .collect();
    if subdirs.len() == 1 {
        let nested = &subdirs[0];
        if nested.join(expected_exe).is_file() {
            if let Ok(inner) = fs::read_dir(nested) {
                for item in inner.flatten() {
                    let dest = dir.join(item.file_name());
                    let _ = fs::rename(item.path(), dest);
                }
            }
            let _ = fs::remove_dir(nested);
        }
    }
}

pub fn collect_emulator_statuses() -> Result<Vec<EmulatorStatusDto>, String> {
    let root = emulators_root_dir()?;
    let settings = crate::load_settings_standalone();
    let mut out = Vec::with_capacity(EMULATOR_SPECS.len());

    for spec in EMULATOR_SPECS {
        let matching_plats: Vec<&'static platforms::PlatformSpec> = platforms::PLATFORMS
            .iter()
            .filter(|p| p.id > 0 && p.default_emulator_id == spec.id)
            .collect();
        let console_slugs: Vec<String> = matching_plats.iter().map(|p| p.slug.to_string()).collect();
        let console_names: Vec<String> = matching_plats
            .iter()
            .map(|p| p.short_name.to_string())
            .collect();
        let primary_platform_id = matching_plats.first().map(|p| p.id).unwrap_or(0);

        let emu_dir = root.join(spec.dir_name);
        let custom_path = console_slugs
            .iter()
            .find_map(|slug| settings.custom_emulator_paths.get(slug))
            .or_else(|| settings.custom_emulator_paths.get(spec.id))
            .or_else(|| settings.custom_emulator_paths.get(spec.dir_name))
            .cloned()
            .unwrap_or_default();

        let (status, exe_path) = if !custom_path.trim().is_empty() && Path::new(custom_path.trim()).is_file() {
            ("custom_path".to_string(), custom_path.trim().to_string())
        } else if let Some(found_exe) = find_exe_recursive(&emu_dir, spec.exe_candidates, 3) {
            let ready = match spec.source {
                EmulatorSource::RetroArchCore {
                    core_name,
                    fallback_core,
                } => retroarch_core_path(
                    found_exe.parent().unwrap_or(&emu_dir),
                    core_name,
                    fallback_core,
                )
                .is_some(),
                _ => true,
            };
            if ready {
                ("ready".to_string(), found_exe.to_string_lossy().into_owned())
            } else {
                ("not_installed".to_string(), String::new())
            }
        } else {
            ("not_installed".to_string(), String::new())
        };

        out.push(EmulatorStatusDto {
            id: spec.id.to_string(),
            display_name: spec.display_name.to_string(),
            console_slugs,
            console_names,
            primary_platform_id,
            status,
            exe_path,
            custom_path,
            supports_auto_install: spec.supports_auto_install,
            caption: spec.console_summary.to_string(),
            install_dir: emu_dir.to_string_lossy().into_owned(),
        });
    }
    Ok(out)
}

#[tauri::command]
pub async fn get_emulator_statuses() -> Result<Vec<EmulatorStatusDto>, String> {
    tokio::task::spawn_blocking(collect_emulator_statuses)
        .await
        .map_err(|e| format!("Emulator status task failed: {}", e))?
}

#[tauri::command]
pub async fn install_emulator_for_console(
    app: AppHandle,
    console_id: String,
) -> Result<Vec<EmulatorStatusDto>, String> {
    let spec = emulator_spec_by_id(&console_id)
        .ok_or_else(|| format!("No emulator configuration matches '{}'.", console_id))?;
    let _exe = install_spec_portable(&app, spec, spec.console_summary).await?;
    collect_emulator_statuses()
}

#[tauri::command]
pub async fn set_custom_emulator_path(
    console_id: String,
    exe_path: String,
) -> Result<Vec<EmulatorStatusDto>, String> {
    let trimmed = exe_path.trim().trim_matches('"').to_string();
    let spec = emulator_spec_by_id(&console_id)
        .ok_or_else(|| format!("Unknown console or emulator '{}'.", console_id))?;
    if !trimmed.is_empty() && !Path::new(&trimmed).is_file() {
        return Err(format!("Selected executable not found: {}", trimmed));
    }

    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let mut settings = crate::load_settings_standalone();
        if trimmed.is_empty() {
            settings.custom_emulator_paths.remove(spec.id);
        } else {
            settings
                .custom_emulator_paths
                .insert(spec.id.to_string(), trimmed);
        }
        crate::save_settings_standalone(&settings)
    })
    .await
    .map_err(|e| e.to_string())??;

    collect_emulator_statuses()
}

/// Checks if a console game has its emulator installed and ready without spawning a window.
#[tauri::command]
pub async fn check_game_emulator_ready(game_id: String) -> Result<EmulatorStatusDto, String> {
    let lib = tokio::task::spawn_blocking(crate::load_library_standalone)
        .await
        .map_err(|e| e.to_string())??;
    let entry = lib
        .into_iter()
        .find(|g| g.id == game_id)
        .ok_or_else(|| "Game not found in library.".to_string())?;
    let statuses = collect_emulator_statuses()?;
    let target_id = if !entry.emulator_id.trim().is_empty() {
        entry.emulator_id
    } else {
        platforms::platform_by_id(entry.platform_id)
            .default_emulator_id
            .to_string()
    };
    statuses
        .into_iter()
        .find(|s| s.id == target_id)
        .ok_or_else(|| "No emulator configured for this game.".to_string())
}

/// Inspects a set of deduplicated ROM/ISO extensions used during directory scanning.
#[allow(dead_code)]
pub fn is_supported_retro_rom_extension(ext: &str) -> bool {
    let low = ext.to_ascii_lowercase();
    let set: HashSet<&str> = [
        "nes", "unf", "sfc", "smc", "z64", "v64", "n64", "iso", "gcm", "rvz", "wbfs", "wia", "wux",
        "wua", "wud", "rpx", "nsp", "xci", "nro", "gb", "gbc", "gba", "nds", "3ds", "cci", "cxi",
        "cia", "vb", "bin", "cue", "chd", "cso", "pbp", "md", "gen", "smd", "sms", "cdi", "gdi",
        "a26", "pce", "sgx",
    ]
    .into_iter()
    .collect();
    set.contains(low.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_portable_cli_args_correctly_per_emulator() {
        let emu_dir = PathBuf::from("C:\\DukePortable\\emulators\\dolphin");
        let rom = PathBuf::from("C:\\ROMs\\GameCube\\Metroid Prime.rvz");
        let d_args = build_emulator_cli_args("dolphin", &rom, &emu_dir, None);
        assert_eq!(d_args[0], "-b");
        assert_eq!(d_args[1], "-u");
        assert!(d_args[2].ends_with("User"));
        assert_eq!(d_args[3], "-e");
        assert!(d_args[4].ends_with("Metroid Prime.rvz"));

        let c_args = build_emulator_cli_args(
            "cemu",
            Path::new("C:\\ROMs\\WiiU\\BotW.wua"),
            Path::new("C:\\Duke\\cemu"),
            None,
        );
        assert_eq!(c_args, vec!["-f", "-g", "C:\\ROMs\\WiiU\\BotW.wua"]);

        let ra_dir = PathBuf::from("C:\\Duke\\emulators\\retroarch");
        let core = ra_dir.join("cores").join("mupen64plus_next_libretro.dll");
        let ra_args = build_emulator_cli_args(
            "retroarch_n64",
            Path::new("C:\\ROMs\\N64\\Zelda64.z64"),
            &ra_dir,
            Some(&core),
        );
        assert_eq!(ra_args[0], "--config");
        assert!(ra_args[1].ends_with("retroarch.cfg"));
        assert_eq!(ra_args[2], "-L");
        assert!(ra_args[3].ends_with("mupen64plus_next_libretro.dll"));
        assert_eq!(ra_args[4], "-f");
        assert!(ra_args[5].ends_with("Zelda64.z64"));

        let ps1_args = build_emulator_cli_args(
            "duckstation",
            Path::new("C:\\ROMs\\PS1\\MGS.chd"),
            Path::new("C:\\Duke\\duck"),
            None,
        );
        assert_eq!(ps1_args, vec!["-fullscreen", "--", "C:\\ROMs\\PS1\\MGS.chd"]);
    }

    #[test]
    fn writes_portable_marker_and_retroarch_config_without_leaking_documents() {
        let dir = std::env::temp_dir().join(format!("duke_emu_portable_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);

        let ra_spec = emulator_spec_by_id("nes").unwrap();
        let ra_dir = dir.join("retroarch");
        write_portable_emulator_config(ra_spec, &ra_dir).unwrap();
        let cfg = fs::read_to_string(ra_dir.join("retroarch.cfg")).unwrap();
        assert!(cfg.contains("video_fullscreen = \"true\""));
        assert!(cfg.contains("input_joypad_driver = \"xinput\""));
        assert!(ra_dir.join("saves").is_dir());
        assert!(ra_dir.join("states").is_dir());

        let dolphin_spec = emulator_spec_by_id("gamecube").unwrap();
        let d_dir = dir.join("dolphin");
        write_portable_emulator_config(dolphin_spec, &d_dir).unwrap();
        assert!(d_dir.join("portable.txt").is_file());
        assert!(d_dir.join("User").join("Config").join("Dolphin.ini").is_file());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn covers_all_24_consoles_in_emulator_specs() {
        for p in platforms::PLATFORMS.iter().filter(|p| p.id > 0) {
            assert!(
                emulator_spec_by_id(p.slug).is_some(),
                "Missing emulator spec for console {}",
                p.slug
            );
        }
    }
}

use crate::curves::{
    AxisRateConfig, CurveMode, DeadzoneShape, GyroActivationMode, GyroConfig, StickCurveConfig,
    TriggerConfig,
};
pub use duke_core::macro_engine::{
    MacroMode, MacroSequence, MacroStep, MacroStepKind, RemapActionType, RemapBinding,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MouseConfig {
    pub dpi_stages: [u16; 4],
    pub active_dpi_stage: usize,
    pub sniper_dpi: u16,
    pub dpi_multiplier: f32,
    pub rapid_fire_cps: f32,
    pub polling_audit_enabled: bool,
}

impl Default for MouseConfig {
    fn default() -> Self {
        Self {
            dpi_stages: [800, 1200, 1600, 3200],
            active_dpi_stage: 1,
            sniper_dpi: 400,
            dpi_multiplier: 1.0,
            rapid_fire_cps: 12.0,
            polling_audit_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FpvConfig {
    pub channel_order: String,
    pub throttle_unipolar: bool,
    pub roll_rates: AxisRateConfig,
    pub pitch_rates: AxisRateConfig,
    pub yaw_rates: AxisRateConfig,
}

impl Default for FpvConfig {
    fn default() -> Self {
        Self {
            channel_order: "AETR".to_string(),
            throttle_unipolar: true,
            roll_rates: AxisRateConfig::default(),
            pitch_rates: AxisRateConfig::default(),
            yaw_rates: AxisRateConfig {
                rc_rate: 0.95,
                super_rate: 0.65,
                expo: 0.12,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputProfile {
    pub id: String,
    pub name: String,
    pub description: String,
    pub is_default: bool,
    pub linked_exes: Vec<String>,
    pub left_stick: StickCurveConfig,
    pub right_stick: StickCurveConfig,
    pub left_trigger: TriggerConfig,
    pub right_trigger: TriggerConfig,
    pub rumble_left_scale: f32,
    pub rumble_right_scale: f32,
    pub mouse: MouseConfig,
    pub fpv: FpvConfig,
    pub gyro: GyroConfig,
    pub remappings: Vec<RemapBinding>,
    pub macros: Vec<MacroSequence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub active_profile_id: String,
    pub auto_switch_enabled: bool,
    pub engine_paused: bool,
    pub close_to_tray: bool,
    pub minimize_to_tray: bool,
    pub start_minimized: bool,
    pub autostart_windows: bool,
    pub poll_hz: u32,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            active_profile_id: "default_desktop".to_string(),
            auto_switch_enabled: true,
            engine_paused: false,
            close_to_tray: true,
            minimize_to_tray: true,
            start_minimized: false,
            autostart_windows: false,
            poll_hz: 1000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileStore {
    pub settings: AppSettings,
    pub profiles: Vec<InputProfile>,
}

impl Default for ProfileStore {
    fn default() -> Self {
        Self {
            settings: AppSettings::default(),
            profiles: build_default_profiles(),
        }
    }
}

pub fn build_default_profiles() -> Vec<InputProfile> {
    let default_macros = vec![
        MacroSequence {
            id: "macro_duke_clip".to_string(),
            name: "Save Duke Capture Clip (F9)".to_string(),
            mode: MacroMode::Sequence,
            repeat_delay_ms: 0,
            steps: vec![
                MacroStep {
                    kind: MacroStepKind::KeyDown,
                    value: "F9".to_string(),
                },
                MacroStep {
                    kind: MacroStepKind::DelayMs,
                    value: "35".to_string(),
                },
                MacroStep {
                    kind: MacroStepKind::KeyUp,
                    value: "F9".to_string(),
                },
            ],
        },
        MacroSequence {
            id: "macro_fpv_reset".to_string(),
            name: "FPV Sim Reset + Arm".to_string(),
            mode: MacroMode::Sequence,
            repeat_delay_ms: 0,
            steps: vec![
                MacroStep {
                    kind: MacroStepKind::KeyTap,
                    value: "R".to_string(),
                },
                MacroStep {
                    kind: MacroStepKind::DelayMs,
                    value: "80".to_string(),
                },
                MacroStep {
                    kind: MacroStepKind::KeyTap,
                    value: "Space".to_string(),
                },
            ],
        },
        MacroSequence {
            id: "macro_burst_3x".to_string(),
            name: "3-Round Precision Burst".to_string(),
            mode: MacroMode::RepeatWhileHeld,
            repeat_delay_ms: 90,
            steps: vec![
                MacroStep {
                    kind: MacroStepKind::MouseClick,
                    value: "MouseLeft".to_string(),
                },
                MacroStep {
                    kind: MacroStepKind::DelayMs,
                    value: "55".to_string(),
                },
                MacroStep {
                    kind: MacroStepKind::MouseClick,
                    value: "MouseLeft".to_string(),
                },
                MacroStep {
                    kind: MacroStepKind::DelayMs,
                    value: "55".to_string(),
                },
                MacroStep {
                    kind: MacroStepKind::MouseClick,
                    value: "MouseLeft".to_string(),
                },
            ],
        },
    ];

    let desktop_profile = InputProfile {
        id: "default_desktop".to_string(),
        name: "Default Desktop".to_string(),
        description: "Productivity shortcuts for Logitech mice & clean passthrough defaults."
            .to_string(),
        is_default: true,
        linked_exes: Vec::new(),
        left_stick: StickCurveConfig::default(),
        right_stick: StickCurveConfig::default(),
        left_trigger: TriggerConfig::default(),
        right_trigger: TriggerConfig::default(),
        rumble_left_scale: 1.0,
        rumble_right_scale: 1.0,
        mouse: MouseConfig::default(),
        fpv: FpvConfig::default(),
        gyro: GyroConfig {
            activation_mode: GyroActivationMode::Off,
            ..GyroConfig::default()
        },
        remappings: vec![
            RemapBinding {
                source: "mouse4".to_string(),
                action_type: RemapActionType::Native,
                target: "MouseX1".to_string(),
                note: "Browser Back / Native Side Button".to_string(),
            },
            RemapBinding {
                source: "mouse5".to_string(),
                action_type: RemapActionType::Native,
                target: "MouseX2".to_string(),
                note: "Browser Forward / Native Side Button".to_string(),
            },
            RemapBinding {
                source: "mouse_dpi".to_string(),
                action_type: RemapActionType::DpiStageCycle,
                target: String::new(),
                note: "Cycle 4 DPI stages".to_string(),
            },
        ],
        macros: default_macros.clone(),
    };

    let fps_gyro_profile = InputProfile {
        id: "fps_gyro".to_string(),
        name: "FPS Precision + Gyro".to_string(),
        description:
            "Dynamic Precision stick curve, 10% hair-triggers, Sniper DPI hold, and ADS gyro aiming."
                .to_string(),
        is_default: false,
        linked_exes: vec![
            "cs2.exe".to_string(),
            "valorant.exe".to_string(),
            "r5apex.exe".to_string(),
            "cod.exe".to_string(),
        ],
        left_stick: StickCurveConfig {
            inner_deadzone: 0.04,
            outer_deadzone: 0.96,
            anti_deadzone: 0.0,
            shape: DeadzoneShape::Radial,
            curve_mode: CurveMode::Linear,
            ..StickCurveConfig::default()
        },
        right_stick: StickCurveConfig {
            inner_deadzone: 0.03,
            outer_deadzone: 0.98,
            anti_deadzone: 0.04,
            shape: DeadzoneShape::Radial,
            curve_mode: CurveMode::DynamicPrecision,
            expo_power: 1.65,
            ..StickCurveConfig::default()
        },
        left_trigger: TriggerConfig {
            activation_threshold: 0.10,
            hair_trigger_digital: true,
            invert: false,
        },
        right_trigger: TriggerConfig {
            activation_threshold: 0.08,
            hair_trigger_digital: true,
            invert: false,
        },
        rumble_left_scale: 0.65,
        rumble_right_scale: 0.75,
        mouse: MouseConfig {
            dpi_stages: [400, 800, 1200, 1600],
            active_dpi_stage: 1,
            sniper_dpi: 400,
            dpi_multiplier: 1.0,
            rapid_fire_cps: 14.0,
            polling_audit_enabled: true,
        },
        fpv: FpvConfig::default(),
        gyro: GyroConfig {
            activation_mode: GyroActivationMode::HoldLt,
            sensitivity: 5.5,
            vertical_ratio: 0.75,
            noise_floor_dps: 1.2,
            smoothing: 0.22,
            invert_x: false,
            invert_y: false,
        },
        remappings: vec![
            RemapBinding {
                source: "mouse4".to_string(),
                action_type: RemapActionType::DpiShiftHold,
                target: "400".to_string(),
                note: "Hold Mouse4 for Sniper DPI Precision".to_string(),
            },
            RemapBinding {
                source: "mouse5".to_string(),
                action_type: RemapActionType::Macro,
                target: "macro_duke_clip".to_string(),
                note: "Save instant replay clip".to_string(),
            },
        ],
        macros: default_macros.clone(),
    };

    let fpv_profile = InputProfile {
        id: "fpv_freestyle".to_string(),
        name: "FPV Sim Rates (EdgeTX)".to_string(),
        description:
            "Betaflight RC Rate / SuperRate / Expo curves, AETR map, unipolar throttle & SA/SB switch binds."
                .to_string(),
        is_default: false,
        linked_exes: vec![
            "fpv.skydive.exe".to_string(),
            "liftoff.exe".to_string(),
            "uncrashed.exe".to_string(),
            "velocidrone.exe".to_string(),
            "trypp.exe".to_string(),
        ],
        left_stick: StickCurveConfig::default(),
        right_stick: StickCurveConfig::default(),
        left_trigger: TriggerConfig::default(),
        right_trigger: TriggerConfig::default(),
        rumble_left_scale: 0.0,
        rumble_right_scale: 0.0,
        mouse: MouseConfig::default(),
        fpv: FpvConfig {
            channel_order: "AETR".to_string(),
            throttle_unipolar: true,
            roll_rates: AxisRateConfig {
                rc_rate: 1.05,
                super_rate: 0.72,
                expo: 0.18,
            },
            pitch_rates: AxisRateConfig {
                rc_rate: 1.05,
                super_rate: 0.72,
                expo: 0.18,
            },
            yaw_rates: AxisRateConfig {
                rc_rate: 0.95,
                super_rate: 0.68,
                expo: 0.15,
            },
        },
        gyro: GyroConfig {
            activation_mode: GyroActivationMode::Off,
            ..GyroConfig::default()
        },
        remappings: vec![
            RemapBinding {
                source: "fpv_sa_high".to_string(),
                action_type: RemapActionType::KeyTap,
                target: "R".to_string(),
                note: "Reset Drone in Sim".to_string(),
            },
            RemapBinding {
                source: "fpv_sb_high".to_string(),
                action_type: RemapActionType::Macro,
                target: "macro_duke_clip".to_string(),
                note: "Trigger Clip Shortcut (F9)".to_string(),
            },
            RemapBinding {
                source: "fpv_sc_high".to_string(),
                action_type: RemapActionType::KeyCombo,
                target: "Ctrl+Shift+M".to_string(),
                note: "Toggle Discord Mic Mute".to_string(),
            },
        ],
        macros: default_macros.clone(),
    };

    let comp_pad_profile = InputProfile {
        id: "comp_pad".to_string(),
        name: "Competitive Controller (Anti-DZ)".to_string(),
        description: "4% radial deadzone, 8% anti-deadzone, S-Curve aim & 120% tuned haptics."
            .to_string(),
        is_default: false,
        linked_exes: vec![
            "rocketleague.exe".to_string(),
            "halo.exe".to_string(),
            "fortniteclient-win64-shipping.exe".to_string(),
        ],
        left_stick: StickCurveConfig {
            inner_deadzone: 0.04,
            outer_deadzone: 0.95,
            anti_deadzone: 0.06,
            shape: DeadzoneShape::Radial,
            curve_mode: CurveMode::Linear,
            ..StickCurveConfig::default()
        },
        right_stick: StickCurveConfig {
            inner_deadzone: 0.04,
            outer_deadzone: 0.98,
            anti_deadzone: 0.08,
            shape: DeadzoneShape::Radial,
            curve_mode: CurveMode::SCurve,
            expo_power: 1.8,
            ..StickCurveConfig::default()
        },
        left_trigger: TriggerConfig {
            activation_threshold: 0.05,
            hair_trigger_digital: true,
            invert: false,
        },
        right_trigger: TriggerConfig {
            activation_threshold: 0.05,
            hair_trigger_digital: true,
            invert: false,
        },
        rumble_left_scale: 1.2,
        rumble_right_scale: 1.2,
        mouse: MouseConfig::default(),
        fpv: FpvConfig::default(),
        gyro: GyroConfig {
            activation_mode: GyroActivationMode::Off,
            ..GyroConfig::default()
        },
        remappings: Vec::new(),
        macros: default_macros,
    };

    vec![
        desktop_profile,
        fps_gyro_profile,
        fpv_profile,
        comp_pad_profile,
    ]
}

fn resolve_portable_dir() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if dir.join("portable.flag").exists() || dir.join("data").is_dir() {
                return Some(dir.join("data"));
            }
        }
    }
    None
}

pub fn get_store_path(app: &AppHandle) -> PathBuf {
    if let Some(portable) = resolve_portable_dir() {
        let _ = fs::create_dir_all(&portable);
        return portable.join("profiles.json");
    }
    if let Ok(dir) = app.path().app_data_dir() {
        let _ = fs::create_dir_all(&dir);
        return dir.join("profiles.json");
    }
    std::env::temp_dir().join("DukeInput").join("profiles.json")
}

pub fn load_store(app: &AppHandle) -> ProfileStore {
    let path = get_store_path(app);
    if let Ok(raw) = fs::read_to_string(&path) {
        if let Ok(mut store) = serde_json::from_str::<ProfileStore>(&raw) {
            if store.profiles.is_empty() {
                store.profiles = build_default_profiles();
            }
            if !store
                .profiles
                .iter()
                .any(|p| p.id == store.settings.active_profile_id)
            {
                store.settings.active_profile_id = store.profiles[0].id.clone();
            }
            return store;
        }
    }
    let default_store = ProfileStore::default();
    let _ = save_store_to_path(&path, &default_store);
    default_store
}

pub fn save_store(app: &AppHandle, store: &ProfileStore) -> Result<(), String> {
    let path = get_store_path(app);
    save_store_to_path(&path, store)
}

fn save_store_to_path(path: &Path, store: &ProfileStore) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed creating profile directory: {}", e))?;
    }
    let json = serde_json::to_string_pretty(store)
        .map_err(|e| format!("Failed serializing profile store: {}", e))?;
    fs::write(path, json).map_err(|e| format!("Failed writing profiles.json: {}", e))
}

#[cfg(target_os = "windows")]
pub fn set_windows_autostart(enabled: bool) -> Result<(), String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey("Software\\Microsoft\\CurrentVersion\\Run")
        .map_err(|e| format!("Failed opening Windows Run key: {}", e))?;

    if enabled {
        let exe = std::env::current_exe()
            .map_err(|e| format!("Could not resolve executable path: {}", e))?;
        let val = format!("\"{}\" --minimized", exe.display());
        key.set_value("DukeInput", &val)
            .map_err(|e| format!("Failed setting Windows autostart: {}", e))?;
    } else {
        let _ = key.delete_value("DukeInput");
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn set_windows_autostart(_enabled: bool) -> Result<(), String> {
    Ok(())
}

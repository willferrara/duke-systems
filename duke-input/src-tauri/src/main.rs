#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod curves;
mod hid;
mod injector;
mod profiles;

use curves::{
    apply_betaflight_rate, apply_stick_transform, apply_trigger_transform, sample_betaflight_preview,
    sample_curve_preview, AxisRateConfig, GyroActivationMode, GyroFilterState, StickCurveConfig,
};
use duke_core::hid_raw::reorder_fpv_primary_axes;
use duke_core::macro_engine::{exe_matches_rule, get_foreground_exe_name};
use duke_core::pipe_ipc::{DukeBus, DukeEvent};
use duke_core::shm_bus::{DukeShmBus, ShmEventType, ShmSourceApp};
use duke_core::tray_daemon::{
    destroy_window_for_tray, reopen_main_window as core_reopen_main_window,
    schedule_delayed_tray_trims,
};
use duke_core::win32_ffi::{
    disable_realtime_input_timer, enable_idle_eco_qos_mode, enable_realtime_input_mode,
    get_current_working_set_mb, restore_normal_priority_mode, set_current_thread_high_priority,
    trim_current_working_set,
};
use hid::{
    poll_xinput, read_raw_mouse_snapshot, set_xinput_rumble, start_raw_input_listener, DeviceKind,
    FpvTelemetry, GyroImuFrame, HidDeviceManager, LiveTelemetrySnapshot, MouseTelemetry,
    RawHidInspectorData, XInputRawState, XboxTelemetry,
};
use injector::{send_mouse_move, InputActionEngine};
use profiles::{
    build_default_profiles, load_store, save_store, set_windows_autostart, AppSettings,
    InputProfile, MacroMode, MacroSequence, MacroStep, MacroStepKind, ProfileStore,
};
use std::fs;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};
use tauri::menu::{CheckMenuItemBuilder, Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WindowEvent};

static DUKE_BUS: OnceLock<DukeBus> = OnceLock::new();
static DUKE_SHM_BUS: OnceLock<Option<DukeShmBus>> = OnceLock::new();

fn query_exe_name_by_pid(pid: u32) -> Option<String> {
    if pid <= 4 {
        return None;
    }
    #[cfg(target_os = "windows")]
    unsafe {
        use windows::core::PWSTR;
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION,
        };
        if let Ok(proc_h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            let mut path_buf = [0u16; 512];
            let mut size = path_buf.len() as u32;
            let ok = QueryFullProcessImageNameW(
                proc_h,
                PROCESS_NAME_WIN32,
                PWSTR(path_buf.as_mut_ptr()),
                &mut size,
            );
            let _ = CloseHandle(proc_h);
            if ok.is_ok() && size > 0 {
                let full = String::from_utf16_lossy(&path_buf[..size as usize]);
                return std::path::Path::new(&full)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .map(str::to_string);
            }
        }
    }
    None
}

struct GameLaunchContext<'a> {
    pid: u32,
    title: &'a str,
    exe_name: &'a str,
    platform_id: Option<u16>,
    app_id: Option<u32>,
    exe_hash: Option<u64>,
}

fn handle_game_launched_auto_switch(
    app: &AppHandle,
    state: &AppState,
    ctx: GameLaunchContext<'_>,
) {
    let resolved_exe = if ctx.exe_name.trim().to_ascii_lowercase().ends_with(".exe") {
        ctx.exe_name.trim().to_string()
    } else {
        query_exe_name_by_pid(ctx.pid).unwrap_or_else(|| ctx.exe_name.trim().to_string())
    };
    {
        let mut active_game = state
            .bus_active_game
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *active_game = Some((ctx.pid, ctx.title.to_string(), resolved_exe.clone()));
    }

    if state.auto_switch_enabled.load(Ordering::Relaxed) {
        let tel_snap = state
            .latest_telemetry
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let switched = {
            let mut guard = state.store.write().unwrap_or_else(|e| e.into_inner());
            select_profile_for_game(
                &guard,
                ctx.title,
                &resolved_exe,
                ctx.platform_id,
                ctx.app_id,
                ctx.exe_hash,
                &tel_snap,
            )
            .and_then(|(target_id, target_name, curve_mode)| {
                if guard.settings.active_profile_id != target_id {
                    guard.settings.active_profile_id = target_id.clone();
                    Some((guard.clone(), target_id, target_name, curve_mode))
                } else {
                    None
                }
            })
        };

        if let Some((updated, target_id, target_name, curve_mode)) = switched {
            state.bump_revision();
            let _ = save_store(app, &updated);
            refresh_tray_menu(app, state);
            let _ = app.emit("profile-switched", &target_id);
            publish_profile_switched_ext(
                &target_name,
                ctx.app_id.unwrap_or(0),
                ctx.platform_id.unwrap_or(0),
                &curve_mode,
            );
        }
    }
}

fn handle_game_exited_auto_revert(app: &AppHandle, state: &AppState, pid: u32) {
    let cleared = {
        let mut active_game = state
            .bus_active_game
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let clear = match &*active_game {
            Some((active_pid, _, _)) => pid == 0 || *active_pid == 0 || *active_pid == pid,
            None => false,
        };
        if clear {
            *active_game = None;
        }
        clear
    };

    if cleared && state.auto_switch_enabled.load(Ordering::Relaxed) {
        let reverted = {
            let mut guard = state.store.write().unwrap_or_else(|e| e.into_inner());
            guard
                .profiles
                .iter()
                .find(|p| p.is_default)
                .or_else(|| guard.profiles.first())
                .map(|p| (p.id.clone(), p.name.clone()))
                .and_then(|(def_id, def_name)| {
                    if guard.settings.active_profile_id != def_id {
                        guard.settings.active_profile_id = def_id.clone();
                        Some((guard.clone(), def_id, def_name))
                    } else {
                        None
                    }
                })
        };

        if let Some((updated, def_id, def_name)) = reverted {
            state.bump_revision();
            let _ = save_store(app, &updated);
            refresh_tray_menu(app, state);
            let _ = app.emit("profile-switched", &def_id);
            publish_profile_switched(&def_name);
        }
    }
    if cleared {
        trim_process_working_set();
    }
}

fn poll_shm_events(app: &AppHandle, state: &AppState, shm_rx: Option<&mut DukeShmBus>) {
    let Some(bus) = shm_rx else {
        return;
    };
    while let Some(frame) = bus.try_recv() {
        match frame.kind() {
            ShmEventType::GameLaunched => {
                if let Some(ev) = frame.decode_game_launched() {
                    handle_game_launched_auto_switch(
                        app,
                        state,
                        GameLaunchContext {
                            pid: ev.pid,
                            title: &ev.tag,
                            exe_name: &ev.tag,
                            platform_id: if ev.platform_id > 0 {
                                Some(ev.platform_id)
                            } else {
                                None
                            },
                            app_id: if ev.app_id > 0 { Some(ev.app_id) } else { None },
                            exe_hash: if ev.exe_hash != 0 {
                                Some(ev.exe_hash)
                            } else {
                                None
                            },
                        },
                    );
                }
            }
            ShmEventType::GameExited => {
                handle_game_exited_auto_revert(app, state, frame.pid);
            }
            ShmEventType::CoOpInputP2 => {
                if let Some(pad) = frame.decode_coop_input_p2() {
                    let _ = injector::inject_virtual_p2_xinput(pad);
                }
            }
            ShmEventType::RamPurge => {
                trim_process_working_set();
            }
            ShmEventType::VoiceMacro => {
                if let Some((vk_code, hold_ms, tag)) = frame.decode_voice_macro() {
                    if let Some(key_name) = vk_code_to_voice_key_name(vk_code) {
                        let steps = build_voice_macro_steps(&[key_name.to_string()], hold_ms as u32);
                        let seq = MacroSequence {
                            id: format!("shm_voice_{}", tag),
                            name: format!("Voice SHM: {}", tag),
                            mode: MacroMode::Sequence,
                            repeat_delay_ms: 0,
                            steps,
                        };
                        *state
                            .pending_macro_test
                            .lock()
                            .unwrap_or_else(|e| e.into_inner()) = Some(seq);
                    }
                }
            }
            _ => {}
        }
    }
}

fn publish_profile_switched_ext(
    profile_name: &str,
    app_id: u32,
    platform_id: u16,
    curve_mode: &str,
) {
    if let Some(bus) = DUKE_BUS.get() {
        bus.publish(DukeEvent::ProfileSwitch {
            app: "duke-input".to_string(),
            profile_name: profile_name.to_string(),
            app_id,
            platform_id,
            curve_mode: curve_mode.to_string(),
        });
    }
    if let Some(Some(shm)) = DUKE_SHM_BUS.get() {
        shm.publish_frame(duke_core::shm_bus::ShmEventFrame::profile_switch(
            ShmSourceApp::DukeInput,
            std::process::id(),
            app_id,
            platform_id,
            profile_name,
        ));
    }
}

fn publish_profile_switched(profile_name: &str) {
    publish_profile_switched_ext(profile_name, 0, 0, "Linear");
}

// ============================================================================
// WIN32 WORKING-SET RAM TRIMMER & WINDOWS 11 ECOQOS SCHEDULER (VIA DUKE-CORE)
// ============================================================================

fn trim_process_working_set() {
    trim_current_working_set();
}

fn get_process_working_set_mb() -> f64 {
    get_current_working_set_mb()
}

fn update_power_policy(state: &AppState) {
    let paused = state.engine_paused.load(Ordering::Relaxed);
    let visible = state.ui_visible.load(Ordering::Relaxed);
    if paused && !visible {
        enable_idle_eco_qos_mode();
    } else if !paused {
        enable_realtime_input_mode();
    } else {
        restore_normal_priority_mode();
    }
}

fn curve_mode_label(profile: &InputProfile) -> String {
    format!("{:?}", profile.right_stick.curve_mode)
}

fn select_profile_for_game(
    store: &ProfileStore,
    title: &str,
    exe_name: &str,
    platform_id: Option<u16>,
    app_id: Option<u32>,
    exe_hash: Option<u64>,
    tel: &LiveTelemetrySnapshot,
) -> Option<(String, String, String)> {
    if let Some(aid) = app_id.filter(|&id| id > 0) {
        if let Some(matched) = store
            .profiles
            .iter()
            .find(|p| p.linked_app_ids.contains(&aid))
        {
            return Some((
                matched.id.clone(),
                matched.name.clone(),
                curve_mode_label(matched),
            ));
        }
    }

    if let Some(hash) = exe_hash.filter(|&h| h != 0) {
        if let Some(matched) = store
            .profiles
            .iter()
            .find(|p| p.linked_exe_hashes.contains(&hash))
        {
            return Some((
                matched.id.clone(),
                matched.name.clone(),
                curve_mode_label(matched),
            ));
        }
    }

    if !exe_name.trim().is_empty() {
        if let Some(matched) = store
            .profiles
            .iter()
            .find(|p| p.linked_exes.iter().any(|rule| exe_matches_rule(exe_name, rule)))
        {
            return Some((
                matched.id.clone(),
                matched.name.clone(),
                curve_mode_label(matched),
            ));
        }
    }

    if let Some(pid) = platform_id.filter(|&p| p > 0) {
        if let Some(matched) = store
            .profiles
            .iter()
            .find(|p| p.linked_platform_ids.contains(&pid))
        {
            return Some((
                matched.id.clone(),
                matched.name.clone(),
                curve_mode_label(matched),
            ));
        }
        if let Some(sys_label) = duke_core::shm_bus::console_label_for_platform_id(pid) {
            let sys_lower = sys_label.to_ascii_lowercase();
            // 1. User-created or explicitly named profile targeting this console (by name or linked rule)
            if let Some(console_prof) = store.profiles.iter().find(|p| {
                p.name.to_ascii_lowercase().contains(&sys_lower)
                    || p.id.to_ascii_lowercase().contains(&sys_lower)
                    || p.linked_exes
                        .iter()
                        .any(|r| r.to_ascii_lowercase().contains(&sys_lower))
            }) {
                return Some((
                    console_prof.id.clone(),
                    console_prof.name.clone(),
                    curve_mode_label(console_prof),
                ));
            }
            // 2. Motion/Gyro consoles (Wii, Wii U, Switch, 3DS, PS3) prefer 5-point Bézier/Gyro profile
            if duke_core::shm_bus::console_has_motion_gyro(pid) {
                if let Some(gyro_prof) = store
                    .profiles
                    .iter()
                    .find(|p| {
                        p.id == "console_gyro_bezier5"
                            || p.id == "fps_gyro"
                            || p.name.to_ascii_lowercase().contains("gyro")
                    })
                {
                    return Some((
                        gyro_prof.id.clone(),
                        gyro_prof.name.clone(),
                        curve_mode_label(gyro_prof),
                    ));
                }
            }
            // 3. Classic/Standard consoles prefer 5-point Bézier or generic controller profile
            if let Some(pad_prof) = store
                .profiles
                .iter()
                .find(|p| p.id == "console_gyro_bezier5" || p.id == "comp_pad")
                .or_else(|| {
                    store.profiles.iter().find(|p| {
                        let lower = p.name.to_ascii_lowercase();
                        lower.contains("retro")
                            || lower.contains("emulator")
                            || lower.contains("controller")
                            || lower.contains("console")
                    })
                })
            {
                return Some((
                    pad_prof.id.clone(),
                    pad_prof.name.clone(),
                    curve_mode_label(pad_prof),
                ));
            }
        }
    }

    let combined = format!("{} {}", title, exe_name).to_ascii_lowercase();
    let is_fpv_game = [
        "liftoff",
        "uncrashed",
        "velocidrone",
        "tryp",
        "drl",
        "orqa",
        "fpv",
        "dcl",
    ]
    .iter()
    .any(|kw| combined.contains(kw));

    if is_fpv_game || tel.fpv.connected {
        if let Some(fpv) = store
            .profiles
            .iter()
            .find(|p| p.id == "fpv_freestyle")
            .or_else(|| {
                store
                    .profiles
                    .iter()
                    .find(|p| !p.is_default && p.name.to_ascii_lowercase().contains("fpv"))
            })
        {
            return Some((fpv.id.clone(), fpv.name.clone(), curve_mode_label(fpv)));
        }
    }

    let controller_connected = tel.xbox.connected
        || tel.devices.iter().any(|d| {
            matches!(
                d.kind,
                DeviceKind::Xbox | DeviceKind::PlayStation | DeviceKind::SwitchPro
            )
        });

    if controller_connected {
        if let Some(pad_prof) = store.profiles.iter().find(|p| p.id == "comp_pad").or_else(|| {
            store.profiles.iter().find(|p| {
                !p.is_default
                    && (p.name.to_ascii_lowercase().contains("controller")
                        || p.id.to_ascii_lowercase().contains("pad"))
            })
        }) {
            return Some((
                pad_prof.id.clone(),
                pad_prof.name.clone(),
                curve_mode_label(pad_prof),
            ));
        }
    }

    store
        .profiles
        .iter()
        .find(|p| p.id == "fps_gyro")
        .or_else(|| store.profiles.iter().find(|p| !p.is_default))
        .map(|p| (p.id.clone(), p.name.clone(), curve_mode_label(p)))
}

// ============================================================================
// SHARED APPLICATION STATE
// ============================================================================

struct AppState {
    store: RwLock<ProfileStore>,
    latest_telemetry: RwLock<LiveTelemetrySnapshot>,
    xbox_raw: RwLock<XInputRawState>,
    bus_active_game: Mutex<Option<(u32, String, String)>>,
    inspector_device_req: Mutex<Option<String>>,
    inspector_reset_req: AtomicBool,
    pending_hidpp_dpi: AtomicU32,
    pending_macro_test: Mutex<Option<MacroSequence>>,
    profile_revision: AtomicU64,
    engine_paused: AtomicBool,
    auto_switch_enabled: AtomicBool,
    ui_visible: AtomicBool,
    inspector_active: AtomicBool,
    measured_hz: AtomicU32,
    app_exiting: AtomicBool,
}

impl AppState {
    fn new(store: ProfileStore) -> Arc<Self> {
        let paused = store.settings.engine_paused;
        let auto_switch = store.settings.auto_switch_enabled;
        Arc::new(Self {
            store: RwLock::new(store),
            latest_telemetry: RwLock::new(LiveTelemetrySnapshot::default()),
            xbox_raw: RwLock::new(XInputRawState::default()),
            bus_active_game: Mutex::new(None),
            inspector_device_req: Mutex::new(None),
            inspector_reset_req: AtomicBool::new(false),
            pending_hidpp_dpi: AtomicU32::new(0),
            pending_macro_test: Mutex::new(None),
            profile_revision: AtomicU64::new(1),
            engine_paused: AtomicBool::new(paused),
            auto_switch_enabled: AtomicBool::new(auto_switch),
            ui_visible: AtomicBool::new(true),
            inspector_active: AtomicBool::new(false),
            measured_hz: AtomicU32::new(0),
            app_exiting: AtomicBool::new(false),
        })
    }

    fn read_store(&self) -> ProfileStore {
        self.store.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn bump_revision(&self) {
        self.profile_revision.fetch_add(1, Ordering::Release);
    }

    fn active_profile_from_store(store: &ProfileStore) -> InputProfile {
        store
            .profiles
            .iter()
            .find(|p| p.id == store.settings.active_profile_id)
            .or_else(|| store.profiles.iter().find(|p| p.is_default))
            .or_else(|| store.profiles.first())
            .cloned()
            .unwrap_or_else(|| build_default_profiles().remove(0))
    }
}

// ============================================================================
// TRAY MENU & WINDOW LIFECYCLE (SUB-10 MB TRAY DAEMON MODE)
// ============================================================================

fn build_tray_menu(app: &AppHandle, state: &AppState) -> tauri::Result<Menu<tauri::Wry>> {
    let store = state.read_store();
    let active_id = store.settings.active_profile_id;
    let paused = state.engine_paused.load(Ordering::Relaxed);

    let mut profiles_sub = SubmenuBuilder::new(app, "Switch Profile");
    for prof in &store.profiles {
        let item_id = format!("profile:{}", prof.id);
        let checked = prof.id == active_id;
        let check_item = CheckMenuItemBuilder::with_id(item_id, &prof.name)
            .checked(checked)
            .build(app)?;
        profiles_sub = profiles_sub.item(&check_item);
    }
    let profile_menu = profiles_sub.build()?;

    let pause_label = if paused {
        "Resume 1000 Hz Engine"
    } else {
        "Pause Input Engine"
    };
    let toggle_pause = MenuItemBuilder::with_id("tray_toggle_pause", pause_label).build(app)?;
    let open_ui = MenuItemBuilder::with_id("tray_open", "Open Configurator").build(app)?;
    let trim_ram = MenuItemBuilder::with_id("tray_trim_ram", "Trim Memory (< 10 MB)").build(app)?;
    let exit_item = MenuItemBuilder::with_id("tray_exit", "Exit Duke Input").build(app)?;

    MenuBuilder::new(app)
        .item(&open_ui)
        .item(&profile_menu)
        .item(&toggle_pause)
        .item(&trim_ram)
        .separator()
        .item(&exit_item)
        .build()
}

fn refresh_tray_menu(app: &AppHandle, state: &AppState) {
    if let Some(tray) = app.tray_by_id("main") {
        if let Ok(menu) = build_tray_menu(app, state) {
            let _ = tray.set_menu(Some(menu));
        }
    }
}

fn reopen_main_window(app: &AppHandle, state: &Arc<AppState>) {
    state.ui_visible.store(true, Ordering::Release);
    update_power_policy(state);
    core_reopen_main_window(app, "main");
}

fn destroy_main_window_for_tray(app: &AppHandle, state: &Arc<AppState>) {
    state.ui_visible.store(false, Ordering::Release);
    state.inspector_active.store(false, Ordering::Release);
    update_power_policy(state);
    destroy_window_for_tray(app, "main");
}

// ============================================================================
// 1000 HZ ZERO-ALLOCATION DEDICATED INPUT ENGINE LOOP
// ============================================================================

fn spawn_input_engine(app: AppHandle, state: Arc<AppState>) {
    std::thread::Builder::new()
        .name("duke-input-1000hz".to_string())
        .spawn(move || {
            set_current_thread_high_priority();
            start_raw_input_listener();

            let mut hid_mgr = HidDeviceManager::new();
            let mut injector = InputActionEngine::new();
            let mut gyro_filter = GyroFilterState::default();

            // Pre-allocated stack buffer & telemetry structures to keep hot loop zero-alloc
            let mut hid_buf = [0u8; 64];
            let mut xinput_active = [false; 4];
            let mut cached_rev: u64 = 0;
            let mut cached_profile = AppState::active_profile_from_store(&state.read_store());

            let mut tick_count: u64 = 0;
            let mut hz_counter: u32 = 0;
            let mut prev_sniper_active = false;
            let mut auto_switched_active = false;
            let mut last_fg_exe = String::new();
            let mut working_set_mb = get_process_working_set_mb();

            let mut fpv_tel = FpvTelemetry::default();
            let mut gyro_frame = GyroImuFrame::default();
            let mut gyro_name = String::new();

            let mut frame_mouse_dx: i32 = 0;
            let mut frame_mouse_dy: i32 = 0;
            let mut frame_wheel_delta: i32 = 0;
            let mut frame_gyro_dx: i32 = 0;
            let mut frame_gyro_dy: i32 = 0;

            let mut next_tick = Instant::now();
            let mut prev_step = Instant::now();
            let mut last_hz_tick = Instant::now();
            let mut last_auto_tick = Instant::now();
            let mut last_ui_emit = Instant::now();
            let mut shm_rx = DukeShmBus::open_or_create().ok();

            loop {
                if state.app_exiting.load(Ordering::Acquire) {
                    injector.release_all_and_reset();
                    break;
                }

                poll_shm_events(&app, &state, shm_rx.as_mut());

                let paused = state.engine_paused.load(Ordering::Relaxed);
                if paused {
                    injector.release_all_and_reset();
                    state.measured_hz.store(0, Ordering::Relaxed);
                    prev_sniper_active = false;

                    if last_hz_tick.elapsed() >= Duration::from_secs(1) {
                        working_set_mb = get_process_working_set_mb();
                        last_hz_tick = Instant::now();
                    }

                    if state.ui_visible.load(Ordering::Relaxed)
                        && last_ui_emit.elapsed() >= Duration::from_millis(120)
                    {
                        last_ui_emit = Instant::now();
                        let store_snap = state.read_store();
                        let active_prof = AppState::active_profile_from_store(&store_snap);
                        let raw_mouse = read_raw_mouse_snapshot();
                        let (devices, batt_pct, batt_mv, hidpp_supported, mouse_name) = hid_mgr
                            .build_device_summaries(false, 0, raw_mouse.polling_hz);
                        let active_stage = active_prof.mouse.active_dpi_stage.min(3);
                        let active_dpi = active_prof.mouse.dpi_stages[active_stage];
                        let snap = LiveTelemetrySnapshot {
                            engine_hz: 0,
                            engine_paused: true,
                            working_set_mb,
                            active_profile_id: active_prof.id,
                            active_profile_name: active_prof.name,
                            foreground_exe: last_fg_exe.clone(),
                            auto_switch_enabled: state.auto_switch_enabled.load(Ordering::Relaxed),
                            devices,
                            xbox: XboxTelemetry::default(),
                            mouse: MouseTelemetry {
                                active_name: mouse_name,
                                polling_hz: raw_mouse.polling_hz,
                                jitter_us: raw_mouse.jitter_us,
                                active_dpi_stage: active_stage,
                                active_dpi,
                                sniper_active: false,
                                battery_pct: batt_pct,
                                battery_mv: batt_mv,
                                hidpp_supported,
                                buttons_mask: raw_mouse.buttons_mask,
                                last_dx: 0,
                                last_dy: 0,
                                wheel_delta: 0,
                            },
                            fpv: fpv_tel.clone(),
                            gyro: Default::default(),
                            raw_inspector: RawHidInspectorData::default(),
                        };
                        *state.latest_telemetry.write().unwrap_or_else(|e| e.into_inner()) =
                            snap.clone();
                        let _ = app.emit("input-telemetry", &snap);
                    }

                    std::thread::sleep(Duration::from_millis(50));
                    next_tick = Instant::now();
                    prev_step = next_tick;
                    continue;
                }

                // Exact 1000 Hz pacing
                let now = Instant::now();
                let dt_sec = now.saturating_duration_since(prev_step).as_secs_f32().clamp(0.0002, 0.02);
                prev_step = now;
                tick_count = tick_count.wrapping_add(1);
                hz_counter = hz_counter.wrapping_add(1);

                // Reload profile snapshot only when edited/switched
                let rev = state.profile_revision.load(Ordering::Acquire);
                if rev != cached_rev {
                    cached_rev = rev;
                    cached_profile = AppState::active_profile_from_store(&state.read_store());
                    let cur_stage = cached_profile.mouse.active_dpi_stage.min(3);
                    let active_dpi = cached_profile.mouse.dpi_stages[cur_stage];
                    let sniper_ratio = if injector.sniper_hold_active {
                        Some((cached_profile.mouse.sniper_dpi.max(100) as f32 / active_dpi.max(100) as f32).clamp(0.1, 1.0))
                    } else {
                        None
                    };
                    injector.update_software_mouse_speed(cached_profile.mouse.dpi_multiplier, sniper_ratio);
                }

                // Periodic HID hot-plug scan (throttled internally to 1.6s)
                hid_mgr.maybe_rescan();

                // Pending UI control requests processed on engine thread
                if let Some(dev_id) = state
                    .inspector_device_req
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .take()
                {
                    hid_mgr.set_inspector_target(&dev_id);
                }
                if state.inspector_reset_req.swap(false, Ordering::AcqRel) {
                    hid_mgr.reset_inspector_bounds();
                }
                let dpi_req = state.pending_hidpp_dpi.swap(0, Ordering::AcqRel);
                if dpi_req > 0 {
                    hid_mgr.apply_logitech_dpi(dpi_req as u16);
                }
                if let Some(macro_def) = state
                    .pending_macro_test
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .take()
                {
                    injector.trigger_macro(&macro_def, true, now);
                    injector.trigger_macro(&macro_def, false, now);
                }

                // 1. Poll Xbox XInput (disconnected slots probed only once every 250 ms)
                let xbox_raw = poll_xinput(tick_count % 250 == 0, &mut xinput_active);
                if tick_count % 16 == 0 || !xbox_raw.connected {
                    *state.xbox_raw.write().unwrap_or_else(|e| e.into_inner()) = xbox_raw;
                }

                // 2. Poll RawInput mouse (microsecond polling + button bitmask)
                let raw_mouse = read_raw_mouse_snapshot();
                frame_mouse_dx = frame_mouse_dx.saturating_add(raw_mouse.last_dx);
                frame_mouse_dy = frame_mouse_dy.saturating_add(raw_mouse.last_dy);
                frame_wheel_delta = frame_wheel_delta.saturating_add(raw_mouse.last_wheel);

                // 3. Poll all opened HID devices (Logitech HID++, FPV Tx, PlayStation/Switch IMU, Inspector)
                let inspect_active = state.inspector_active.load(Ordering::Relaxed);
                hid_mgr.poll_all(
                    &mut hid_buf,
                    inspect_active,
                    &mut fpv_tel,
                    &mut gyro_frame,
                    &mut gyro_name,
                );

                // 4. Stick & Trigger Curves (Xbox / Controller)
                let (out_lx, out_ly) = if xbox_raw.connected {
                    apply_stick_transform(xbox_raw.lx, xbox_raw.ly, &cached_profile.left_stick)
                } else {
                    (0.0, 0.0)
                };
                let (out_rx, out_ry) = if xbox_raw.connected {
                    apply_stick_transform(xbox_raw.rx, xbox_raw.ry, &cached_profile.right_stick)
                } else {
                    (0.0, 0.0)
                };
                let out_lt = if xbox_raw.connected {
                    apply_trigger_transform(xbox_raw.lt, &cached_profile.left_trigger)
                } else {
                    0.0
                };
                let out_rt = if xbox_raw.connected {
                    apply_trigger_transform(xbox_raw.rt, &cached_profile.right_trigger)
                } else {
                    0.0
                };

                // 5. FPV Channel Order Remapping + Throttle Range + Betaflight Rate/Expo Curves
                if fpv_tel.connected {
                    let (raw_roll, raw_pitch, raw_throttle, raw_yaw) = reorder_fpv_primary_axes(
                        &fpv_tel.channels_raw,
                        &cached_profile.fpv.channel_order,
                    );
                    let (out_roll, roll_dps) =
                        apply_betaflight_rate(raw_roll, &cached_profile.fpv.roll_rates);
                    let (out_pitch, pitch_dps) =
                        apply_betaflight_rate(raw_pitch, &cached_profile.fpv.pitch_rates);
                    let (out_yaw, yaw_dps) =
                        apply_betaflight_rate(raw_yaw, &cached_profile.fpv.yaw_rates);
                    let out_throttle = if cached_profile.fpv.throttle_unipolar {
                        ((raw_throttle + 1.0) * 0.5).clamp(0.0, 1.0)
                    } else {
                        raw_throttle.clamp(-1.0, 1.0)
                    };
                    fpv_tel.channels_out[0] = out_roll;
                    fpv_tel.channels_out[1] = out_pitch;
                    fpv_tel.channels_out[2] = out_throttle;
                    fpv_tel.channels_out[3] = out_yaw;
                    fpv_tel.channels_out[4..16].copy_from_slice(&fpv_tel.channels_raw[4..16]);
                    fpv_tel.roll_dps = roll_dps;
                    fpv_tel.pitch_dps = pitch_dps;
                    fpv_tel.yaw_dps = yaw_dps;
                }

                // 6. Button / Switch / Wheel Remappings & Sub-ms Macro Engine
                let wbtn = xbox_raw.buttons_mask;
                let source_states: [(&str, bool); 30] = [
                    ("mouse4", (raw_mouse.buttons_mask & (1 << 3)) != 0),
                    ("mouse5", (raw_mouse.buttons_mask & (1 << 4)) != 0),
                    ("mouse_middle", (raw_mouse.buttons_mask & (1 << 2)) != 0),
                    ("mouse_dpi", false),
                    ("scroll_up", raw_mouse.scroll_up_pulses > 0),
                    ("scroll_down", raw_mouse.scroll_down_pulses > 0),
                    ("pad_a", (wbtn & 0x1000) != 0),
                    ("pad_b", (wbtn & 0x2000) != 0),
                    ("pad_x", (wbtn & 0x4000) != 0),
                    ("pad_y", (wbtn & 0x8000) != 0),
                    ("pad_lb", (wbtn & 0x0100) != 0),
                    ("pad_rb", (wbtn & 0x0200) != 0),
                    (
                        "pad_lt_hair",
                        gyro_frame.l2_down
                            || (xbox_raw.connected
                                && xbox_raw.lt >= cached_profile.left_trigger.activation_threshold),
                    ),
                    (
                        "pad_rt_hair",
                        gyro_frame.r2_down
                            || (xbox_raw.connected
                                && xbox_raw.rt >= cached_profile.right_trigger.activation_threshold),
                    ),
                    ("pad_ls", (wbtn & 0x0040) != 0),
                    ("pad_rs", (wbtn & 0x0080) != 0),
                    ("pad_back", (wbtn & 0x0020) != 0),
                    ("pad_start", (wbtn & 0x0010) != 0),
                    ("pad_dpad_up", (wbtn & 0x0001) != 0),
                    ("pad_dpad_down", (wbtn & 0x0002) != 0),
                    ("pad_dpad_left", (wbtn & 0x0004) != 0),
                    ("pad_dpad_right", (wbtn & 0x0008) != 0),
                    ("pad_touchpad", gyro_frame.touchpad_click),
                    ("fpv_sa_high", fpv_tel.connected && fpv_tel.switches[0] == 2),
                    ("fpv_sb_mid", fpv_tel.connected && fpv_tel.switches[1] == 1),
                    ("fpv_sb_high", fpv_tel.connected && fpv_tel.switches[1] == 2),
                    ("fpv_sc_mid", fpv_tel.connected && fpv_tel.switches[2] == 1),
                    ("fpv_sc_high", fpv_tel.connected && fpv_tel.switches[2] == 2),
                    ("fpv_sd_high", fpv_tel.connected && fpv_tel.switches[3] == 2),
                    ("fpv_se_high", fpv_tel.connected && fpv_tel.switches[4] == 2),
                ];

                let (cycle_dpi, cycle_profile, gyro_toggle_edge) = injector.process_bindings(
                    now,
                    &cached_profile.remappings,
                    &cached_profile.macros,
                    cached_profile.mouse.rapid_fire_cps,
                    &source_states,
                );

                // Sniper DPI hold state edge sync (Hardware HID++ + Win32 fallback)
                if injector.sniper_hold_active != prev_sniper_active {
                    prev_sniper_active = injector.sniper_hold_active;
                    let stage = cached_profile.mouse.active_dpi_stage.min(3);
                    let normal_dpi = cached_profile.mouse.dpi_stages[stage].max(100);
                    let target_dpi = if prev_sniper_active {
                        cached_profile.mouse.sniper_dpi.max(100)
                    } else {
                        normal_dpi
                    };
                    let sniper_ratio = if prev_sniper_active {
                        Some((target_dpi as f32 / normal_dpi as f32).clamp(0.1, 1.0))
                    } else {
                        None
                    };
                    injector.update_software_mouse_speed(cached_profile.mouse.dpi_multiplier, sniper_ratio);
                    hid_mgr.apply_logitech_dpi(target_dpi);
                }

                if cycle_dpi {
                    let mut next_dpi = 0u16;
                    {
                        let mut guard = state.store.write().unwrap_or_else(|e| e.into_inner());
                        let active_id = guard.settings.active_profile_id.clone();
                        if let Some(prof) = guard
                            .profiles
                            .iter_mut()
                            .find(|p| p.id == active_id)
                        {
                            prof.mouse.active_dpi_stage = (prof.mouse.active_dpi_stage + 1) % 4;
                            next_dpi = prof.mouse.dpi_stages[prof.mouse.active_dpi_stage];
                        }
                    }
                    if next_dpi > 0 {
                        state.bump_revision();
                        hid_mgr.apply_logitech_dpi(next_dpi);
                    }
                }

                if cycle_profile {
                    let mut switched_id = String::new();
                    {
                        let mut guard = state.store.write().unwrap_or_else(|e| e.into_inner());
                        if !guard.profiles.is_empty() {
                            let idx = guard
                                .profiles
                                .iter()
                                .position(|p| p.id == guard.settings.active_profile_id)
                                .unwrap_or(0);
                            let next_idx = (idx + 1) % guard.profiles.len();
                            switched_id = guard.profiles[next_idx].id.clone();
                            guard.settings.active_profile_id = switched_id.clone();
                        }
                    }
                    if !switched_id.is_empty() {
                        auto_switched_active = false;
                        state.bump_revision();
                        refresh_tray_menu(&app, &state);
                        let _ = app.emit("profile-switched", &switched_id);
                    }
                }

                gyro_filter.update_toggle(gyro_toggle_edge);

                // 7. PlayStation / Switch Gyro-to-Mouse Engine
                let gyro_active = if gyro_frame.valid {
                    match cached_profile.gyro.activation_mode {
                        GyroActivationMode::Off => false,
                        GyroActivationMode::AlwaysOn => true,
                        GyroActivationMode::HoldLt => {
                            gyro_frame.l2_down
                                || (xbox_raw.connected
                                    && xbox_raw.lt >= cached_profile.left_trigger.activation_threshold)
                        }
                        GyroActivationMode::HoldRt => {
                            gyro_frame.r2_down
                                || (xbox_raw.connected
                                    && xbox_raw.rt >= cached_profile.right_trigger.activation_threshold)
                        }
                        GyroActivationMode::TouchpadTouch => gyro_frame.touchpad_down,
                        GyroActivationMode::ToggleButton => gyro_filter.toggled_active,
                    }
                } else {
                    false
                };

                if gyro_frame.valid {
                    let (mdx, mdy) = gyro_filter.process(
                        gyro_frame.yaw_dps,
                        gyro_frame.pitch_dps,
                        gyro_active,
                        dt_sec,
                        &cached_profile.gyro,
                    );
                    if mdx != 0 || mdy != 0 {
                        send_mouse_move(mdx, mdy);
                        frame_gyro_dx = frame_gyro_dx.saturating_add(mdx);
                        frame_gyro_dy = frame_gyro_dy.saturating_add(mdy);
                    }
                }

                // 8. 1000 Hz Loop Meter + Working-Set RAM Sampler (once per second)
                if last_hz_tick.elapsed() >= Duration::from_secs(1) {
                    let secs = last_hz_tick.elapsed().as_secs_f64().max(0.5);
                    let hz = (hz_counter as f64 / secs).round().min(1000.0) as u32;
                    state.measured_hz.store(hz, Ordering::Relaxed);
                    hz_counter = 0;
                    working_set_mb = get_process_working_set_mb();
                    last_hz_tick = Instant::now();
                }

                // 9. 500 ms Foreground Executable Auto-Profile Switcher
                if last_auto_tick.elapsed() >= Duration::from_millis(500) {
                    last_auto_tick = Instant::now();
                    let fg_exe = get_foreground_exe_name();
                    if !fg_exe.is_empty() {
                        last_fg_exe = fg_exe.clone();
                    }
                    if state.auto_switch_enabled.load(Ordering::Relaxed) && !fg_exe.is_empty() {
                        let lower = fg_exe.to_ascii_lowercase();
                        let is_self_or_shell = lower == "duke-input.exe"
                            || lower == "dukeinput.exe"
                            || lower == "explorer.exe";

                        if !is_self_or_shell {
                            let bus_game_running = state
                                .bus_active_game
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .is_some();
                            let mut next_profile: Option<(String, String)> = None;
                            {
                                let guard = state.store.read().unwrap_or_else(|e| e.into_inner());
                                if let Some(matched) = guard.profiles.iter().find(|p| {
                                    p.linked_exes.iter().any(|rule| exe_matches_rule(&fg_exe, rule))
                                }) {
                                    auto_switched_active = true;
                                    if matched.id != guard.settings.active_profile_id {
                                        next_profile =
                                            Some((matched.id.clone(), matched.name.clone()));
                                    }
                                } else if auto_switched_active && !bus_game_running {
                                    auto_switched_active = false;
                                    if let Some(def) = guard
                                        .profiles
                                        .iter()
                                        .find(|p| p.is_default)
                                        .or_else(|| guard.profiles.first())
                                    {
                                        if def.id != guard.settings.active_profile_id {
                                            next_profile = Some((def.id.clone(), def.name.clone()));
                                        }
                                    }
                                }
                            }

                            if let Some((pid, pname)) = next_profile {
                                {
                                    let mut guard = state.store.write().unwrap_or_else(|e| e.into_inner());
                                    guard.settings.active_profile_id = pid.clone();
                                }
                                state.bump_revision();
                                refresh_tray_menu(&app, &state);
                                let _ = app.emit("profile-switched", &pid);
                                publish_profile_switched(&pname);
                            }
                        }
                    }
                }

                // 10. 30 FPS UI Telemetry Snapshot (only emitted while window is visible!)
                if state.ui_visible.load(Ordering::Relaxed)
                    && last_ui_emit.elapsed() >= Duration::from_millis(33)
                {
                    last_ui_emit = Instant::now();
                    let (devices, batt_pct, batt_mv, hidpp_supported, mouse_name) = hid_mgr
                        .build_device_summaries(xbox_raw.connected, xbox_raw.slot, raw_mouse.polling_hz);
                    let raw_inspector = if inspect_active {
                        hid_mgr.build_inspector_snapshot()
                    } else {
                        RawHidInspectorData::default()
                    };
                    let active_stage = cached_profile.mouse.active_dpi_stage.min(3);
                    let active_dpi = if injector.sniper_hold_active {
                        cached_profile.mouse.sniper_dpi
                    } else {
                        cached_profile.mouse.dpi_stages[active_stage]
                    };

                    let snap = LiveTelemetrySnapshot {
                        engine_hz: state.measured_hz.load(Ordering::Relaxed),
                        engine_paused: false,
                        working_set_mb,
                        active_profile_id: cached_profile.id.clone(),
                        active_profile_name: cached_profile.name.clone(),
                        foreground_exe: last_fg_exe.clone(),
                        auto_switch_enabled: state.auto_switch_enabled.load(Ordering::Relaxed),
                        devices,
                        xbox: XboxTelemetry {
                            connected: xbox_raw.connected,
                            slot: xbox_raw.slot,
                            raw_lx: xbox_raw.lx,
                            raw_ly: xbox_raw.ly,
                            raw_rx: xbox_raw.rx,
                            raw_ry: xbox_raw.ry,
                            out_lx,
                            out_ly,
                            out_rx,
                            out_ry,
                            raw_lt: xbox_raw.lt,
                            raw_rt: xbox_raw.rt,
                            out_lt,
                            out_rt,
                            buttons_mask: xbox_raw.buttons_mask,
                        },
                        mouse: MouseTelemetry {
                            active_name: mouse_name,
                            polling_hz: raw_mouse.polling_hz,
                            jitter_us: raw_mouse.jitter_us,
                            active_dpi_stage: active_stage,
                            active_dpi,
                            sniper_active: injector.sniper_hold_active,
                            battery_pct: batt_pct,
                            battery_mv: batt_mv,
                            hidpp_supported,
                            buttons_mask: raw_mouse.buttons_mask,
                            last_dx: frame_mouse_dx,
                            last_dy: frame_mouse_dy,
                            wheel_delta: frame_wheel_delta,
                        },
                        fpv: fpv_tel.clone(),
                        gyro: hid::GyroTelemetry {
                            connected: gyro_frame.valid,
                            name: gyro_name.clone(),
                            active: gyro_active,
                            pitch_dps: gyro_frame.pitch_dps,
                            yaw_dps: gyro_frame.yaw_dps,
                            roll_dps: gyro_frame.roll_dps,
                            accel_x: gyro_frame.accel_x,
                            accel_y: gyro_frame.accel_y,
                            accel_z: gyro_frame.accel_z,
                            mouse_dx: frame_gyro_dx,
                            mouse_dy: frame_gyro_dy,
                            touchpad_down: gyro_frame.touchpad_down,
                            battery_pct: gyro_frame.battery_pct,
                        },
                        raw_inspector,
                    };

                    frame_mouse_dx = 0;
                    frame_mouse_dy = 0;
                    frame_wheel_delta = 0;
                    frame_gyro_dx = 0;
                    frame_gyro_dy = 0;

                    *state.latest_telemetry.write().unwrap_or_else(|e| e.into_inner()) =
                        snap.clone();
                    let _ = app.emit("input-telemetry", &snap);
                }

                // Sleep cleanly up to the next 1 ms deadline
                next_tick += Duration::from_micros(1000);
                let after = Instant::now();
                if next_tick > after {
                    let rem = next_tick - after;
                    if rem >= Duration::from_micros(250) {
                        std::thread::sleep(rem);
                    }
                } else if after.saturating_duration_since(next_tick) > Duration::from_millis(6) {
                    next_tick = after;
                }
            }

            disable_realtime_input_timer();
        })
        .expect("Failed to start Duke Input 1000 Hz engine thread");
}

// ============================================================================
// TAURI IPC COMMANDS
// ============================================================================

#[tauri::command]
fn get_store(state: State<'_, Arc<AppState>>) -> ProfileStore {
    state.read_store()
}

#[tauri::command]
fn get_live_telemetry(state: State<'_, Arc<AppState>>) -> LiveTelemetrySnapshot {
    state
        .latest_telemetry
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

#[tauri::command]
fn update_settings(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    settings: AppSettings,
) -> Result<ProfileStore, String> {
    let mut updated = {
        let mut guard = state.store.write().unwrap_or_else(|e| e.into_inner());
        let autostart_changed = guard.settings.autostart_windows != settings.autostart_windows;
        if autostart_changed {
            set_windows_autostart(settings.autostart_windows)?;
        }
        guard.settings = settings;
        guard.clone()
    };

    if updated
        .profiles
        .iter()
        .all(|p| p.id != updated.settings.active_profile_id)
    {
        if let Some(first) = updated.profiles.first() {
            updated.settings.active_profile_id = first.id.clone();
        }
    }

    state
        .auto_switch_enabled
        .store(updated.settings.auto_switch_enabled, Ordering::Release);
    state
        .engine_paused
        .store(updated.settings.engine_paused, Ordering::Release);
    update_power_policy(&state);
    state.bump_revision();
    save_store(&app, &updated)?;
    refresh_tray_menu(&app, &state);
    Ok(updated)
}

#[tauri::command]
fn save_profile(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    profile: InputProfile,
) -> Result<ProfileStore, String> {
    let updated = {
        let mut guard = state.store.write().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = guard.profiles.iter_mut().find(|p| p.id == profile.id) {
            *existing = profile;
        } else {
            guard.profiles.push(profile);
        }
        guard.clone()
    };
    state.bump_revision();
    save_store(&app, &updated)?;
    refresh_tray_menu(&app, &state);
    Ok(updated)
}

#[tauri::command]
fn delete_profile(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    profile_id: String,
) -> Result<ProfileStore, String> {
    let updated = {
        let mut guard = state.store.write().unwrap_or_else(|e| e.into_inner());
        if guard.profiles.len() <= 1 {
            return Err("At least one input profile must remain configured.".to_string());
        }
        if let Some(pos) = guard.profiles.iter().position(|p| p.id == profile_id) {
            if guard.profiles[pos].is_default {
                return Err("The default desktop fallback profile cannot be deleted.".to_string());
            }
            guard.profiles.remove(pos);
        }
        if guard.settings.active_profile_id == profile_id {
            guard.settings.active_profile_id = guard
                .profiles
                .iter()
                .find(|p| p.is_default)
                .or_else(|| guard.profiles.first())
                .map(|p| p.id.clone())
                .unwrap_or_else(|| "default_desktop".to_string());
        }
        guard.clone()
    };
    state.bump_revision();
    save_store(&app, &updated)?;
    refresh_tray_menu(&app, &state);
    Ok(updated)
}

#[tauri::command]
fn set_active_profile(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    profile_id: String,
) -> Result<ProfileStore, String> {
    let (updated, profile_name) = {
        let mut guard = state.store.write().unwrap_or_else(|e| e.into_inner());
        let name = guard
            .profiles
            .iter()
            .find(|p| p.id == profile_id)
            .map(|p| p.name.clone())
            .ok_or_else(|| format!("Profile '{}' not found.", profile_id))?;
        guard.settings.active_profile_id = profile_id.clone();
        (guard.clone(), name)
    };
    state.bump_revision();
    save_store(&app, &updated)?;
    refresh_tray_menu(&app, &state);
    let _ = app.emit("profile-switched", &profile_id);
    publish_profile_switched(&profile_name);
    Ok(updated)
}

#[tauri::command]
fn capture_stick_center_drift(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    profile_id: String,
    stick: String,
) -> Result<ProfileStore, String> {
    let raw = *state.xbox_raw.read().unwrap_or_else(|e| e.into_inner());
    if !raw.connected {
        return Err("Connect an XInput/Xbox controller first to capture rest drift.".to_string());
    }
    let (off_x, off_y) = if stick.eq_ignore_ascii_case("right") {
        (raw.rx.clamp(-0.35, 0.35), raw.ry.clamp(-0.35, 0.35))
    } else {
        (raw.lx.clamp(-0.35, 0.35), raw.ly.clamp(-0.35, 0.35))
    };

    let updated = {
        let mut guard = state.store.write().unwrap_or_else(|e| e.into_inner());
        let prof = guard
            .profiles
            .iter_mut()
            .find(|p| p.id == profile_id)
            .ok_or_else(|| "Active profile not found.".to_string())?;
        if stick.eq_ignore_ascii_case("right") {
            prof.right_stick.center_offset_x = off_x;
            prof.right_stick.center_offset_y = off_y;
        } else {
            prof.left_stick.center_offset_x = off_x;
            prof.left_stick.center_offset_y = off_y;
        }
        guard.clone()
    };
    state.bump_revision();
    save_store(&app, &updated)?;
    Ok(updated)
}

#[tauri::command]
fn test_xinput_rumble(
    state: State<'_, Arc<AppState>>,
    left_pct: f32,
    right_pct: f32,
) -> Result<(), String> {
    let raw = *state.xbox_raw.read().unwrap_or_else(|e| e.into_inner());
    if !raw.connected {
        return Err("No XInput controller connected on slots 0..3.".to_string());
    }
    let slot = raw.slot;
    set_xinput_rumble(slot, left_pct.clamp(0.0, 1.0), right_pct.clamp(0.0, 1.0))?;
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(280));
        let _ = set_xinput_rumble(slot, 0.0, 0.0);
    });
    Ok(())
}

#[tauri::command]
fn apply_dpi_stage(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    stage_idx: usize,
) -> Result<ProfileStore, String> {
    let clamped_stage = stage_idx.min(3);
    let mut applied_dpi = 0u16;
    let updated = {
        let mut guard = state.store.write().unwrap_or_else(|e| e.into_inner());
        let active_id = guard.settings.active_profile_id.clone();
        if let Some(prof) = guard.profiles.iter_mut().find(|p| p.id == active_id) {
            prof.mouse.active_dpi_stage = clamped_stage;
            applied_dpi = prof.mouse.dpi_stages[clamped_stage];
        }
        guard.clone()
    };
    if applied_dpi > 0 {
        state
            .pending_hidpp_dpi
            .store(applied_dpi as u32, Ordering::Release);
    }
    state.bump_revision();
    save_store(&app, &updated)?;
    Ok(updated)
}

#[tauri::command]
fn preview_curve_points(config: StickCurveConfig) -> Vec<[f32; 2]> {
    sample_curve_preview(&config, 48)
}

#[tauri::command]
fn preview_betaflight_points(config: AxisRateConfig) -> Vec<[f32; 2]> {
    sample_betaflight_preview(&config, 48)
}

#[tauri::command]
fn set_inspector_device(state: State<'_, Arc<AppState>>, device_id: String, active: bool) {
    state.inspector_active.store(active, Ordering::Release);
    *state
        .inspector_device_req
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(device_id);
}

#[tauri::command]
fn reset_inspector_bounds(state: State<'_, Arc<AppState>>) {
    state.inspector_reset_req.store(true, Ordering::Release);
}

#[tauri::command]
fn toggle_engine_paused(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    paused: Option<bool>,
) -> Result<bool, String> {
    let new_val = paused.unwrap_or_else(|| !state.engine_paused.load(Ordering::Relaxed));
    state.engine_paused.store(new_val, Ordering::Release);
    let updated = {
        let mut guard = state.store.write().unwrap_or_else(|e| e.into_inner());
        guard.settings.engine_paused = new_val;
        guard.clone()
    };
    update_power_policy(&state);
    state.bump_revision();
    save_store(&app, &updated)?;
    refresh_tray_menu(&app, &state);
    let _ = app.emit("engine-paused-changed", new_val);
    Ok(new_val)
}

#[tauri::command]
fn set_ui_visible(state: State<'_, Arc<AppState>>, visible: bool) {
    state.ui_visible.store(visible, Ordering::Release);
    if !visible {
        state.inspector_active.store(false, Ordering::Release);
    }
    update_power_policy(&state);
}

#[tauri::command]
fn trim_working_set() -> f64 {
    trim_process_working_set();
    get_process_working_set_mb()
}

#[tauri::command]
fn minimize_to_tray_now(app: AppHandle, state: State<'_, Arc<AppState>>) {
    destroy_main_window_for_tray(&app, state.inner());
}

#[tauri::command]
fn export_profile_json(
    state: State<'_, Arc<AppState>>,
    profile_id: String,
    path: String,
) -> Result<(), String> {
    let store = state.read_store();
    let prof = store
        .profiles
        .iter()
        .find(|p| p.id == profile_id)
        .ok_or_else(|| "Profile not found.".to_string())?;
    let json = serde_json::to_string_pretty(prof).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| format!("Failed writing {}: {}", path, e))
}

#[tauri::command]
fn import_profile_json(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    path: String,
) -> Result<ProfileStore, String> {
    let raw = fs::read_to_string(&path).map_err(|e| format!("Failed reading {}: {}", path, e))?;
    let mut prof: InputProfile =
        serde_json::from_str(&raw).map_err(|e| format!("Invalid profile JSON: {}", e))?;
    prof.is_default = false;

    let updated = {
        let mut guard = state.store.write().unwrap_or_else(|e| e.into_inner());
        if guard.profiles.iter().any(|p| p.id == prof.id) {
            let suffix = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(1);
            prof.id = format!("{}_{}", prof.id, suffix);
            prof.name = format!("{} (Imported)", prof.name);
        }
        let pid = prof.id.clone();
        guard.profiles.push(prof);
        guard.settings.active_profile_id = pid;
        guard.clone()
    };

    state.bump_revision();
    save_store(&app, &updated)?;
    refresh_tray_menu(&app, &state);
    Ok(updated)
}

#[tauri::command]
fn test_macro(state: State<'_, Arc<AppState>>, macro_def: MacroSequence) {
    *state
        .pending_macro_test
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(macro_def);
}

// ============================================================================
// TAURI APPLICATION ENTRYPOINT
// ============================================================================

fn main() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let store = load_store(app.handle());
            let start_minimized = store.settings.start_minimized;
            let state = AppState::new(store);
            app.manage(state.clone());

            enable_realtime_input_mode();
            update_power_policy(&state);

            if let Some(tray) = app.tray_by_id("main") {
                if let Ok(menu) = build_tray_menu(app.handle(), &state) {
                    let _ = tray.set_menu(Some(menu));
                }

                let tray_state = state.clone();
                tray.on_tray_icon_event(move |tray_handle, event| match event {
                    TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    }
                    | TrayIconEvent::DoubleClick {
                        button: MouseButton::Left,
                        ..
                    } => {
                        reopen_main_window(tray_handle.app_handle(), &tray_state);
                    }
                    _ => {}
                });
            }

            let menu_state = state.clone();
            app.on_menu_event(move |app_handle, event| {
                let id = event.id().as_ref();
                if id == "tray_open" {
                    reopen_main_window(app_handle, &menu_state);
                } else if id == "tray_toggle_pause" {
                    let next = !menu_state.engine_paused.load(Ordering::Relaxed);
                    menu_state.engine_paused.store(next, Ordering::Release);
                    let updated = {
                        let mut guard = menu_state.store.write().unwrap_or_else(|e| e.into_inner());
                        guard.settings.engine_paused = next;
                        guard.clone()
                    };
                    update_power_policy(&menu_state);
                    menu_state.bump_revision();
                    let _ = save_store(app_handle, &updated);
                    refresh_tray_menu(app_handle, &menu_state);
                    let _ = app_handle.emit("engine-paused-changed", next);
                } else if id == "tray_trim_ram" {
                    trim_process_working_set();
                } else if id == "tray_exit" {
                    menu_state.app_exiting.store(true, Ordering::Release);
                    if let Some(Some(bus)) = DUKE_SHM_BUS.get() {
                        bus.unregister_app();
                    }
                    app_handle.exit(0);
                } else if let Some(profile_id) = id.strip_prefix("profile:") {
                    let (updated, switched_name) = {
                        let mut guard = menu_state.store.write().unwrap_or_else(|e| e.into_inner());
                        let mut found_name = None;
                        if let Some(prof) = guard.profiles.iter().find(|p| p.id == profile_id) {
                            found_name = Some(prof.name.clone());
                            guard.settings.active_profile_id = profile_id.to_string();
                        }
                        (guard.clone(), found_name)
                    };
                    menu_state.bump_revision();
                    let _ = save_store(app_handle, &updated);
                    refresh_tray_menu(app_handle, &menu_state);
                    let _ = app_handle.emit("profile-switched", profile_id);
                    if let Some(name) = switched_name {
                        publish_profile_switched(&name);
                    }
                }
            });

            // Connect to lock-free Shared Memory Ring Buffer (`Local\duke-suite-shm-v1`)
            let _ = DUKE_SHM_BUS.get_or_init(|| {
                let bus = DukeShmBus::open_or_create().ok();
                if let Some(ref b) = bus {
                    b.register_app(ShmSourceApp::DukeInput);
                }
                bus
            });

            // Connect to leader-elected Duke Suite IPC bus for automatic GameStarted/GameStopped profile swapping
            let bus_app = app.handle().clone();
            let bus_state = state.clone();
            let _ = DUKE_BUS.get_or_init(|| {
                DukeBus::start("duke-input", move |event| match event {
                    DukeEvent::GameLaunched {
                        title,
                        exe_name,
                        pid,
                        app_id,
                        platform_id,
                        exe_hash,
                    } => {
                        handle_game_launched_auto_switch(
                            &bus_app,
                            &bus_state,
                            GameLaunchContext {
                                pid,
                                title: &title,
                                exe_name: &exe_name,
                                platform_id: if platform_id > 0 {
                                    Some(platform_id)
                                } else {
                                    None
                                },
                                app_id: if app_id > 0 { Some(app_id) } else { None },
                                exe_hash: if exe_hash != 0 { Some(exe_hash) } else { None },
                            },
                        );
                    }
                    DukeEvent::GameExited { pid, .. } => {
                        handle_game_exited_auto_revert(&bus_app, &bus_state, pid);
                    }
                    DukeEvent::GameStarted {
                        title,
                        exe_name,
                        pid,
                    } => {
                        handle_game_launched_auto_switch(
                            &bus_app,
                            &bus_state,
                            GameLaunchContext {
                                pid,
                                title: &title,
                                exe_name: &exe_name,
                                platform_id: None,
                                app_id: None,
                                exe_hash: None,
                            },
                        );
                    }
                    DukeEvent::GameStopped { pid, .. } => {
                        handle_game_exited_auto_revert(&bus_app, &bus_state, pid);
                    }
                    DukeEvent::RamPurgeRequested { .. } => {
                        trim_process_working_set();
                    }
                    DukeEvent::VoiceMacroTriggered {
                        phrase,
                        action_id,
                        keys,
                        hold_ms,
                        ..
                    } => {
                        let steps = build_voice_macro_steps(&keys, hold_ms);
                        if !steps.is_empty() {
                            let seq = MacroSequence {
                                id: action_id,
                                name: format!("Voice: {}", phrase),
                                mode: MacroMode::Sequence,
                                repeat_delay_ms: 0,
                                steps,
                            };
                            *bus_state
                                .pending_macro_test
                                .lock()
                                .unwrap_or_else(|e| e.into_inner()) = Some(seq);
                        }
                    }
                    _ => {}
                })
            });

            spawn_input_engine(app.handle().clone(), state.clone());

            if start_minimized {
                destroy_main_window_for_tray(app.handle(), &state);
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_store,
            get_live_telemetry,
            update_settings,
            save_profile,
            delete_profile,
            set_active_profile,
            capture_stick_center_drift,
            test_xinput_rumble,
            apply_dpi_stage,
            preview_curve_points,
            preview_betaflight_points,
            set_inspector_device,
            reset_inspector_bounds,
            toggle_engine_paused,
            set_ui_visible,
            trim_working_set,
            minimize_to_tray_now,
            export_profile_json,
            import_profile_json,
            test_macro,
        ])
        .build(tauri::generate_context!())
        .expect("Failed to build Duke Input Tauri application");

    let app_state_holder = app.try_state::<Arc<AppState>>().map(|s| s.inner().clone());

    app.run(move |app_handle, event| {
        let Some(state) = app_state_holder.as_ref() else {
            return;
        };
        match event {
            RunEvent::WindowEvent { label, event, .. } if label == "main" => match event {
                WindowEvent::CloseRequested { .. } => {
                    let close_to_tray = state.read_store().settings.close_to_tray;
                    if !close_to_tray {
                        state.app_exiting.store(true, Ordering::Release);
                    } else {
                        state.ui_visible.store(false, Ordering::Release);
                        state.inspector_active.store(false, Ordering::Release);
                        update_power_policy(state);
                        schedule_delayed_tray_trims();
                    }
                }
                WindowEvent::Destroyed => {
                    state.ui_visible.store(false, Ordering::Release);
                    state.inspector_active.store(false, Ordering::Release);
                    update_power_policy(state);
                    schedule_delayed_tray_trims();
                }
                WindowEvent::Resized(_) => {
                    let min_to_tray = state.read_store().settings.minimize_to_tray;
                    if min_to_tray {
                        if let Some(win) = app_handle.get_webview_window("main") {
                            if win.is_minimized().unwrap_or(false) {
                                destroy_main_window_for_tray(app_handle, state);
                            }
                        }
                    }
                }
                _ => {}
            },
            RunEvent::ExitRequested { api, .. } => {
                if !state.app_exiting.load(Ordering::Acquire) {
                    api.prevent_exit();
                }
            }
            RunEvent::Exit => {
                if let Some(Some(bus)) = DUKE_SHM_BUS.get() {
                    bus.unregister_app();
                }
            }
            _ => {}
        }
    });
}

fn vk_code_to_voice_key_name(vk: u16) -> Option<&'static str> {
    match vk {
        0x70 => Some("F1"),
        0x71 => Some("F2"),
        0x72 => Some("F3"),
        0x73 => Some("F4"),
        0x74 => Some("F5"),
        0x75 => Some("F6"),
        0x76 => Some("F7"),
        0x77 => Some("F8"),
        0x78 => Some("F9"),
        0x79 => Some("F10"),
        0x7A => Some("F11"),
        0x7B => Some("F12"),
        0x20 => Some("Space"),
        0x0D => Some("Enter"),
        0x1B => Some("Esc"),
        _ => None,
    }
}

fn build_voice_macro_steps(keys: &[String], hold_ms: u32) -> Vec<MacroStep> {
    let clean: Vec<&str> = keys
        .iter()
        .map(|k| k.trim())
        .filter(|k| !k.is_empty())
        .collect();
    if clean.is_empty() {
        return Vec::new();
    }
    let mut steps = Vec::with_capacity(clean.len() * 2 + 1);
    for k in &clean {
        steps.push(MacroStep {
            kind: MacroStepKind::KeyDown,
            value: (*k).to_string(),
        });
    }
    steps.push(MacroStep {
        kind: MacroStepKind::DelayMs,
        value: hold_ms.clamp(20, 500).to_string(),
    });
    for k in clean.iter().rev() {
        steps.push(MacroStep {
            kind: MacroStepKind::KeyUp,
            value: (*k).to_string(),
        });
    }
    steps
}

#[cfg(test)]
mod wave1_tests {
    use super::*;

    #[test]
    fn builds_balanced_voice_macro_key_steps() {
        let steps = build_voice_macro_steps(&["Ctrl".into(), "F5".into()], 45);
        assert_eq!(steps.len(), 5);
        assert_eq!(steps[0].kind, MacroStepKind::KeyDown);
        assert_eq!(steps[1].kind, MacroStepKind::KeyDown);
        assert_eq!(steps[2].kind, MacroStepKind::DelayMs);
        assert_eq!(steps[2].value, "45");
        assert_eq!(steps[3].value, "F5");
        assert_eq!(steps[4].value, "Ctrl");
    }

    #[test]
    fn selects_bezier5_console_and_linked_app_profiles() {
        let store = ProfileStore::default();
        let tel = LiveTelemetrySnapshot::default();
        let selected =
            select_profile_for_game(&store, "Zelda BotW", "", Some(7), Some(999), None, &tel)
                .expect("Switch game should match 5-point Bezier console profile");
        assert_eq!(selected.0, "console_gyro_bezier5");
        assert_eq!(selected.2, "Bezier5Point");
    }
}

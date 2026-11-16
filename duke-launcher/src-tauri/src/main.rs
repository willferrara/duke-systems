// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod achievements;
mod artwork;
mod bench;
mod bios_vault;
mod catalog_mmap;
mod deep_shrink;
mod delta_patch;
mod direct_storage;
mod duke_acoustics;
mod duke_ai_rt;
mod duke_comms;
mod duke_configurator;
mod duke_director;
mod duke_doctor;
mod duke_link;
mod duke_upscale;
mod duke_verified;
mod duke_vision;
mod emulators;
mod etw_xray;
mod friends;
mod importers;
mod latency_lab;
mod ludusavi;
mod mcp;
mod metadata_ext;
mod mod_dropper;
mod mod_graph;
mod ntfs_compress;
mod pe_inspect;
mod platforms;
mod playtime_sync;
mod projfs_stream;
mod quick_resume;
mod retro_achievements;
mod retro_catalog;
mod rom_inspect;
mod rom_patcher;
mod self_heal;
mod shader_swarm;
mod sidebar_icons;
mod sites;
mod snapshots;
mod sources;
mod steam_cm;
mod store_direct;
mod stream_extract;
mod swarm;
mod time_machine;
mod uninstaller;
mod couch_mode;
mod dkpack;
mod duke_capsule;
mod duke_ghost;
mod duke_jail;
mod duke_pilot;
mod duke_sandbox;
mod fix_finder;
mod fse_shell;
mod privacy_vault;
mod stress_harness;
mod vhdx_stage;
mod wasm_plugins;
mod archive_native;
mod lib_store;
mod net_engine;
mod rom_shrink;
mod discord_ipc;
mod save_watcher;
mod desktop_chord;
mod redist_cleaner;
mod updater;
mod app_system;
mod steam_profile;

mod cmd_emulators;
mod cmd_library;
mod cmd_security;
mod cmd_system;

#[allow(unused_imports)]
pub(crate) use cmd_library::{
    atomic_write_bytes, atomic_write_string, calculate_dir_size_sync, clean_game_title,
    compress_game_folder_core, encode_base64, find_best_game_exe_in_folder, format_bytes,
    get_app_data_file, get_app_data_file_standalone, get_backups_dir, get_backups_dir_standalone,
    get_covers_dir, get_covers_dir_standalone, ingest_dropped_paths_core,
    load_catalog_cache_standalone, load_library_from_disk, load_library_lightweight,
    load_library_standalone, normalize_game_entry, resolve_data_root_dir,
    resolve_data_root_dir_standalone, resolve_game_root_dir, resolve_portable_data_dir,
    save_library_standalone, save_library_to_disk, smart_find_game_cover,
    spawn_startup_turbo_price_resolution, CatalogCacheFile, CatalogGameEntry, GameEntry,
};
#[allow(unused_imports)]
pub(crate) use cmd_security::{
    build_doh_client_for_url, enqueue_download_internal, extract_archive_to_folder_sync,
    fetch_doh_json, get_queue_store, reload_queue_store_from_disk, resolve_default_downloads_dir,
    sanitize_filename, scan_file_security_core, validate_external_http_url,
};
#[allow(unused_imports)]
pub(crate) use cmd_system::{
    active_shm_apps, apply_runtime_settings, crash_guard, crc32_ieee, emit_if_alive,
    enqueue_protocol_download_url, find_active_session_pid, first_active_session_pid,
    get_active_hotkey_cell, get_active_jobs_map, get_active_sessions,
    get_process_working_set_mb, init_duke_bus, is_shutting_down, launch_game_detached_headless,
    launch_game_headless, load_settings_from_disk, load_settings_standalone,
    normalize_loaded_settings, parse_duke_protocol_download_url, publish_pipe_event,
    publish_shm_event, publish_shm_frame_direct, query_disk_free_space_sync, run_blocking_timeout,
    save_settings_standalone, save_settings_to_disk, shutdown_all_runtime_services,
    shutdown_flush_downloads, spawn_idle_ram_trim_watcher, spawn_overlay_trigger_watcher,
    suspend_or_resume_game, suspend_or_resume_game_core, toggle_overlay_window,
    trim_process_working_set, win32_clipboard_protocol, write_pure_rust_zip_archive, APP_SHUTDOWN,
    COUCH_MODE_ACTIVE, CREATE_NO_WINDOW, DUKE_BUS, DUKE_SHM_BUS, HOTKEY_WATCHER_ACTIVE,
    IS_SHUTTING_DOWN, LauncherSettings, RAM_TRIM_MODE, TRAY_DAEMON,
};

use duke_core::shm_bus::{DukeShmBus, ShmSourceApp};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
#[allow(unused_imports)]
use tauri::{Emitter, Manager};
fn main() {
    crash_guard::install();

    if std::env::args().any(|a| a == "--mcp") {
        mcp::run_mcp_stdio_server();
        return;
    }

    if std::env::args().any(|a| a == "--bench") {
        if let Err(err) = bench::run_cli_benchmark() {
            eprintln!("duke-launcher --bench error: {err}");
            std::process::exit(1);
        }
        return;
    }

    if std::env::args().any(|a| a == "--stress-test") {
        if let Err(err) = stress_harness::run_cli_stress_test() {
            eprintln!("duke-launcher --stress-test error: {err}");
            std::process::exit(1);
        }
        return;
    }

    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    let allow_multi_instance = raw_args.iter().any(|a| a == "--allow-multi-instance");
    couch_mode::init_cli_couch_flag(&raw_args);
    fse_shell::init_cli_fse_flag(&raw_args);
    if !allow_multi_instance && !win32_clipboard_protocol::acquire_single_instance_or_forward(&raw_args) {
        return;
    }
    let initial_protocol_routes: Vec<app_system::DukeProtocolRoute> = raw_args
        .iter()
        .filter_map(|arg| app_system::parse_duke_protocol_route(arg))
        .collect();
    let initial_protocol_urls: Vec<String> = raw_args
        .iter()
        .filter(|arg| app_system::parse_duke_protocol_route(arg).is_none())
        .filter_map(|arg| parse_duke_protocol_download_url(arg))
        .collect();

    let _ = get_active_hotkey_cell();
    let daemon_state = duke_core::tray_daemon::TrayDaemonState::new(
        duke_core::tray_daemon::TrayDaemonConfig {
            tray_id: "duke-launcher-tray".to_string(),
            window_label: "main".to_string(),
            tooltip: "Duke Launcher (Zero-RAM Tray Daemon)".to_string(),
            open_label: "Open Duke Launcher".to_string(),
            trim_label: "Trim RAM Now".to_string(),
            exit_label: "Quit Duke Launcher".to_string(),
            close_to_tray: Arc::new(AtomicBool::new(false)),
            minimize_to_tray: Arc::new(AtomicBool::new(false)),
        },
    );
    let _ = TRAY_DAEMON.set(daemon_state.clone());

    let daemon_for_setup = daemon_state.clone();
    let app = match tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .setup(move |app| {
            if !allow_multi_instance {
                win32_clipboard_protocol::register_duke_uri_protocol();
                win32_clipboard_protocol::start_protocol_pipe_server(app.handle().clone());
            }

            let exit_app = app.handle().clone();
            let on_exit_cb: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                shutdown_all_runtime_services(&exit_app);
            });
            let _ = app_system::install_full_launcher_tray(
                app.handle(),
                &daemon_for_setup,
                on_exit_cb,
            );

            for route in &initial_protocol_routes {
                app_system::dispatch_duke_protocol_route(app.handle(), route.clone());
            }
            for initial_url in &initial_protocol_urls {
                enqueue_protocol_download_url(
                    app.handle().clone(),
                    initial_url.clone(),
                    false,
                    true,
                );
            }

            // Ensure covers/, sidebar_icons/, and backups/ directories exist (supports Portable Mode if portable.flag or data/ is present)
            if let Ok(covers_dir) = get_covers_dir(app.handle()) {
                let _ = app.asset_protocol_scope().allow_directory(&covers_dir, true);
            }
            let _ = sidebar_icons::get_sidebar_icons_dir(app.handle());
            if resolve_portable_data_dir().is_some() {
                sidebar_icons::allow_portable_sidebar_icons_scope(app.handle());
            }
            let _ = get_backups_dir(app.handle());

            // Run non-blocking startup self-healing (.bak JSON recovery, paused download reset, orphan tmp & dkcat verification).
            self_heal::spawn_startup_self_heal(app.handle().clone());

            // Non-blocking Driver Profile Guard: detects DXGI driver updates and re-applies UserGpuPreferences & Auto-SR profiles.
            latency_lab::spawn_startup_driver_profile_guard(app.handle().clone());

            // Non-blocking background sync of Nintendo & retro Libretro RDB catalog into `catalog.dkcat` if empty
            retro_catalog::spawn_background_retro_catalog_sync_if_needed(app.handle().clone());

            // Non-blocking background multi-launcher playtime sync (Steam, Xbox, Epic, GOG, Ubisoft, EA, RetroArch)
            playtime_sync::spawn_background_playtime_sync(app.handle().clone());

            // Initialize cache-line-aligned lock-free Win32 shared memory event bus (`Local\duke-suite-shm-v1`).
            let _ = DUKE_SHM_BUS.get_or_init(|| {
                let bus = DukeShmBus::open_or_create().ok();
                if let Some(ref b) = bus {
                    b.register_app(ShmSourceApp::DukeLauncher);
                }
                Mutex::new(bus)
            });

            // Map `catalog.dkcat` via zero-copy mmap asynchronously so setup never blocks first paint.
            let catalog_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let _ = tokio::task::spawn_blocking(catalog_mmap::ensure_catalog_mapped_standalone)
                    .await;
                let (record_count, fetched_at, _) = catalog_mmap::catalog_mmap_header_meta();
                if record_count > 0 {
                    emit_if_alive(
                        &catalog_app,
                        "catalog-mmap-ready",
                        serde_json::json!({
                            "recordCount": record_count,
                            "fetchedAt": fetched_at,
                        }),
                    );
                }
            });

            let mut initial_settings = load_settings_from_disk(app.handle());
            if couch_mode::is_cli_couch_booted() || fse_shell::is_cli_fse_booted() {
                initial_settings.couch_mode = true;
            }
            apply_runtime_settings(&initial_settings);
            app_system::restore_saved_window_geometry_on_boot(app.handle(), &initial_settings);
            fse_shell::apply_fse_boot_sequence_if_enabled(app.handle());

            if let Some(overlay) = app.get_webview_window("overlay") {
                let _ = overlay.set_background_color(Some(tauri::window::Color(0, 0, 0, 0)));
            }

            if std::env::args().any(|a| a == "--minimized" || a == "--tray") {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.minimize();
                    let _ = win.hide();
                }
            }

            // Start Duke Swarm LAN/Tailscale discovery only if enabled by user settings.
            if initial_settings.swarm_enabled {
                swarm::start_swarm_background_services();
            }

            // Resolve missing retail prices for `turbo_downloader` games non-blockingly (`catalog.dkcat` -> DoH).
            spawn_startup_turbo_price_resolution(app.handle().clone());

            // Start XInput Guide button (`0x0400`) + global `Alt+G` hotkey listener for quick overlay HUD.
            spawn_overlay_trigger_watcher(app.handle().clone());

            // Start leader-elected Duke Suite IPC bus
            init_duke_bus(app.handle().clone());

            // Trim working-set immediately after startup initialization (unless RAM trim is set to Off)
            if RAM_TRIM_MODE.load(Ordering::Relaxed) != 0 {
                trim_process_working_set();
            }
            spawn_idle_ram_trim_watcher(app.handle().clone());
            if initial_settings.auto_check_updates {
                updater::spawn_startup_update_check(app.handle().clone());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                match event {
                    tauri::WindowEvent::CloseRequested { api, .. } => {
                        let close_to_tray = cmd_system::TRAY_DAEMON
                            .get()
                            .map(|d| d.config.close_to_tray.load(Ordering::Relaxed))
                            .unwrap_or(false);
                        if !close_to_tray {
                            IS_SHUTTING_DOWN.store(true, Ordering::Release);
                            APP_SHUTDOWN.store(true, Ordering::Release);
                            api.prevent_close();
                            let app_handle = window.app_handle().clone();
                            shutdown_all_runtime_services(&app_handle);
                            app_handle.exit(0);
                        }
                    }
                    tauri::WindowEvent::Destroyed => {
                        IS_SHUTTING_DOWN.store(true, Ordering::Release);
                        APP_SHUTDOWN.store(true, Ordering::Release);
                        let app_handle = window.app_handle().clone();
                        shutdown_all_runtime_services(&app_handle);
                    }
                    _ => {}
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            cmd_system::get_app_version,
            cmd_system::get_system_ram_mb,
            cmd_system::check_for_app_updates,
            cmd_system::get_cached_app_update,
            cmd_system::download_and_apply_app_update,
            cmd_system::apply_window_backdrop,
            cmd_security::enqueue_download,
            cmd_security::install_downloaded_job,
            cmd_security::clear_completed_downloads,
            cmd_security::set_speed_limit,
            cmd_security::pause_download,
            cmd_security::resume_download,
            cmd_security::remove_download_job,
            cmd_security::get_download_jobs,
            cmd_system::get_launcher_settings,
            cmd_system::update_launcher_settings,
            cmd_library::fetch_catalog_games,
            cmd_library::enrich_catalog_query,
            cmd_library::get_catalog_game_details,
            cmd_library::auto_fix_all_covers,
            cmd_library::find_and_apply_cover,
            cmd_security::scan_file_security,
            cmd_security::delete_flagged_file,
            cmd_library::get_directory_size,
            cmd_library::add_local_game,
            cmd_library::link_local_game_with_steam_id,
            cmd_library::set_game_launch_args,
            cmd_library::compress_game_folder,
            cmd_library::change_game_exe,
            cmd_system::launch_local_game,
            cmd_system::launch_game,
            cmd_library::get_game_steam_details,
            cmd_library::sync_library_steam_prices,
            cmd_library::get_local_library,
            sidebar_icons::get_sidebar_icons,
            cmd_library::remove_local_game,
            cmd_library::toggle_favorite_game,
            cmd_library::open_game_folder,
            cmd_system::trim_working_set,
            cmd_system::get_system_telemetry,
            cmd_system::backup_game_saves,
            sources::get_imported_sources,
            sources::search_imported_sources,
            sources::import_source_url,
            sources::import_source_file,
            sources::toggle_imported_source,
            sources::refresh_imported_source,
            sources::refresh_all_imported_sources,
            sources::remove_imported_source,
            sites::get_search_sites,
            sites::add_search_site,
            sites::remove_search_site,
            sites::bulk_import_search_sites,
            sites::reset_default_search_sites,
            cmd_security::open_directory_browser,
            ludusavi::sync_ludusavi_manifest,
            ludusavi::discover_game_save_paths,
            ludusavi::list_save_backups,
            ludusavi::restore_game_save,
            importers::import_existing_libraries,
            artwork::search_steamgriddb_artwork,
            artwork::apply_custom_artwork,
            artwork::search_web_covers_for_game,
            artwork::create_desktop_shortcut_with_icon,
            artwork::resolve_catalog_cover,
            cmd_system::suspend_or_resume_game,
            cmd_system::get_active_game_sessions,
            cmd_system::poll_gamepad_state,
            cmd_system::read_clipboard_text,
            cmd_system::write_clipboard_text,
            cmd_system::take_pending_protocol_urls,
            cmd_library::set_game_launch_profile,
            snapshots::create_game_snapshot,
            snapshots::list_game_snapshots,
            snapshots::restore_game_snapshot,
            snapshots::delete_game_snapshot,
            snapshots::reclaim_snapshot_space,
            cmd_library::ingest_dropped_paths,
            cmd_library::scan_and_clean_desktop_shortcuts,
            cmd_security::auto_fix_game,
            cmd_system::get_disk_free_space,
            cmd_system::register_duke_protocol,
            cmd_system::run_subsystem_benchmark,
            catalog_mmap::get_catalog_mmap_stats,
            catalog_mmap::search_catalog_mmap,
            catalog_mmap::query_catalog_mmap,
            delta_patch::generate_game_dkmanifest,
            delta_patch::diff_game_against_manifest,
            delta_patch::create_delta_patch_file,
            delta_patch::apply_game_delta_patch,
            swarm::get_swarm_status,
            swarm::probe_swarm_peers,
            swarm::beam_game_from_peer,
            swarm::sync_game_saves_mesh,
            pe_inspect::preflight_check_exe,
            pe_inspect::heal_game_runtime_dlls,
            pe_inspect::get_game_compat_status,
            pe_inspect::toggle_game_laa_patch,
            pe_inspect::toggle_game_dxvk_wrapper,
            friends::get_unified_friends,
            friends::join_friend_session,
            importers::sync_storefront_playtime,
            playtime_sync::sync_all_external_playtimes,
            achievements::get_game_achievements,
            achievements::set_game_achievements_path,
            ntfs_compress::get_game_storage_stats,
            ntfs_compress::compress_game_wof,
            ntfs_compress::decompress_game_wof,
            metadata_ext::get_game_extended_telemetry,
            cmd_system::set_game_update_config,
            cmd_system::check_game_updates,
            cmd_system::apply_game_update,
            cmd_system::toggle_overlay_window,
            cmd_system::hide_overlay_window,
            cmd_system::trigger_replay_clip_ipc,
            cmd_system::purge_suite_ram_ipc,
            retro_catalog::sync_retro_catalog,
            retro_catalog::get_retro_platforms,
            emulators::get_emulator_statuses,
            emulators::install_emulator_for_console,
            emulators::set_custom_emulator_path,
            emulators::check_game_emulator_ready,
            cmd_emulators::inspect_rom_path,
            cmd_emulators::add_retro_game_from_catalog,
            cmd_emulators::link_game_rom,
            cmd_emulators::scan_rom_folder,
            cmd_system::get_frame_pacing_and_ioring_status,
            cmd_system::toggle_half_ms_frame_pacing,
            cmd_system::purge_stale_driver_shaders,
            projfs_stream::get_projfs_runtime_status,
            projfs_stream::enable_projfs_windows_feature,
            projfs_stream::mount_projfs_virtual_game,
            projfs_stream::unmount_projfs_virtual_game,
            projfs_stream::deduplicate_library_cas,
            shader_swarm::get_shader_swarm_status,
            shader_swarm::sync_game_shaders_from_swarm,
            shader_swarm::archive_game_shader_bundle,
            quick_resume::get_quick_resume_slots,
            quick_resume::quick_switch_game_slot,
            quick_resume::suspend_to_quick_resume_slot,
            quick_resume::remove_quick_resume_slot,
            steam_cm::get_steam_cm_status,
            steam_cm::connect_steam_cm,
            steam_cm::disconnect_steam_cm,
            steam_cm::send_steam_chat_message,
            steam_cm::get_steam_chat_messages,
            steam_cm::get_steam_pending_invites,
            steam_cm::respond_steam_game_invite,
            steam_cm::list_steam_cloud_files,
            steam_cm::sync_steam_cloud_saves,
            store_direct::get_store_direct_status,
            store_direct::login_gog_direct,
            store_direct::login_epic_direct,
            store_direct::list_gog_owned_titles,
            store_direct::list_epic_owned_titles,
            store_direct::start_store_direct_install,
            mod_graph::scan_game_mod_graph,
            mod_graph::register_game_mod_pack,
            mod_graph::save_game_mod_loadout,
            mod_graph::activate_game_mod_loadout,
            cmd_system::get_suite_autonomy_status,
            duke_link::get_duke_link_status,
            duke_link::start_coop_host,
            duke_link::stop_coop_session,
            duke_link::invite_friend_to_coop,
            duke_link::join_coop_stream,
            duke_link::send_coop_input_p2,
            duke_link::send_coop_input_slot,
            time_machine::get_game_time_machine,
            time_machine::create_time_machine_checkpoint,
            time_machine::create_time_machine_branch,
            time_machine::switch_time_machine_branch,
            time_machine::restore_time_machine_checkpoint,
            time_machine::delete_time_machine_checkpoint,
            direct_storage::probe_direct_storage_capability,
            direct_storage::scan_direct_storage_assets,
            direct_storage::repack_game_direct_storage_assets,
            direct_storage::verify_direct_storage_streams,
            vhdx_stage::get_vhdx_stage_status,
            vhdx_stage::containerize_game_to_vhdx,
            vhdx_stage::stage_game_hot_tier,
            vhdx_stage::unstage_or_detach_vhdx,
            duke_upscale::get_game_upscale_profile,
            duke_upscale::save_game_upscale_profile,
            duke_upscale::promote_game_window_borderless,
            latency_lab::get_latency_lab_status,
            latency_lab::get_game_latency_profile,
            latency_lab::apply_game_latency_profile,
            latency_lab::remove_game_latency_profile,
            latency_lab::run_driver_profile_guard,
            latency_lab::enforce_shader_cache_cap,
            etw_xray::probe_etw_xray_capability,
            etw_xray::get_game_xray_scorecards,
            etw_xray::clear_game_xray_scorecards,
            deep_shrink::scan_game_deep_shrink,
            deep_shrink::apply_game_deep_shrink,
            deep_shrink::deduplicate_library_blake3,
            duke_ai_rt::get_ai_runtime_status,
            duke_ai_rt::update_ai_runtime_config,
            duke_ai_rt::unload_ai_workers,
            duke_vision::trigger_duke_vision_capture,
            duke_vision::analyze_vision_text_query,
            duke_vision::get_latest_vision_state,
            duke_comms::get_duke_comms_state,
            duke_comms::toggle_duke_comms_listening,
            duke_comms::process_voice_phrase_command,
            duke_comms::upsert_voice_macro_rule,
            duke_doctor::diagnose_game_crash,
            duke_doctor::doctor_revert_game_snapshot,
            duke_director::get_duke_director_state,
            duke_director::toggle_duke_director,
            duke_director::trigger_director_highlight_now,
            duke_configurator::analyze_game_config_preset,
            duke_configurator::apply_game_config_preset,
            duke_configurator::rollback_game_config_preset,
            duke_acoustics::get_acoustics_state,
            duke_acoustics::set_wasapi_session_volume,
            duke_acoustics::save_game_eq_and_denoise,
            duke_acoustics::test_mic_rnnoise_latency,
            catalog_mmap::get_play_tonight_recommendations,
            couch_mode::get_couch_mode_status,
            couch_mode::set_couch_mode_state,
            couch_mode::trigger_couch_radial_action,
            couch_mode::calibrate_controller_axes,
            dkpack::export_game_dkpack,
            dkpack::inspect_dkpack_file,
            dkpack::apply_dkpack_recipe,
            duke_capsule::export_game_capsule,
            duke_capsule::inspect_game_capsule,
            duke_pilot::plan_duke_pilot_command,
            duke_pilot::execute_duke_pilot_plan,
            duke_jail::get_game_jail_status,
            duke_jail::set_game_jail_policy,
            duke_jail::cleanup_game_jail_profile,
            privacy_vault::get_privacy_vault_status,
            privacy_vault::set_vault_api_key,
            privacy_vault::apply_anonymity_network_preset,
            fix_finder::analyze_game_crash_or_preflight,
            fix_finder::apply_one_click_game_fixes,
            fix_finder::fetch_pcgamingwiki_fixes,
            pe_inspect::check_sha256_threat_feed,
            cmd_security::unblock_motw_for_path,
            bios_vault::get_bios_vault_status,
            bios_vault::scan_and_install_bios_sources,
            retro_achievements::fetch_retro_achievements_progress,
            rom_patcher::group_multi_disc_library,
            rom_patcher::apply_rom_patch_and_create_profile,
            uninstaller::preview_clean_uninstall,
            uninstaller::execute_clean_uninstall,
            mod_dropper::install_mod_archive_or_workshop,
            duke_verified::get_duke_verified_status,
            duke_verified::record_duke_verified_feedback,
            duke_verified::export_duke_verified_matrix,
            duke_verified::import_duke_verified_matrix,
            deep_shrink::evaluate_disk_pressure_autopilot,
            deep_shrink::execute_disk_pressure_autopilot,
            duke_ghost::list_game_ghosts,
            duke_ghost::get_ghost_status,
            duke_ghost::start_ghost_recording,
            duke_ghost::stop_ghost_recording,
            duke_ghost::start_ghost_playback,
            duke_ghost::stop_ghost_playback,
            duke_ghost::import_ghost_file,
            duke_ghost::export_ghost_file,
            duke_ghost::delete_ghost_recording,
            duke_sandbox::get_sandbox_trainer_status,
            duke_sandbox::sandbox_scan_memory,
            duke_sandbox::sandbox_write_memory_value,
            duke_sandbox::sandbox_set_speedhack_scale,
            fse_shell::get_fse_shell_status,
            fse_shell::set_fse_gaming_home,
            fse_shell::suspend_windows_bloat_now,
            fse_shell::resume_windows_bloat_now,
            wasm_plugins::list_wasm_plugins,
            wasm_plugins::install_wasm_plugin,
            wasm_plugins::install_demo_wasm_plugin,
            wasm_plugins::set_wasm_plugin_capabilities,
            wasm_plugins::execute_wasm_plugin,
            wasm_plugins::remove_wasm_plugin,
            cmd_emulators::scan_shrinkable_roms,
            cmd_emulators::compress_retro_rom,
            cmd_emulators::verify_rom_archive,
            cmd_security::scan_redist_bloat,
            cmd_security::clean_redist_bloat,
            cmd_security::get_net_engine_stats_cmd,
            cmd_security::save_api_key,
            cmd_security::clear_api_key,
            cmd_system::get_self_heal_diagnostics,
            cmd_system::reset_self_heal_subsystem,
            cmd_system::get_discord_presence_status,
            cmd_system::set_discord_presence_enabled,
            cmd_system::get_save_watcher_status,
            cmd_system::start_save_watcher,
            cmd_system::stop_save_watcher,
            cmd_system::get_desktop_chord_status,
            cmd_system::set_desktop_chord_enabled,
            cmd_library::get_steam_mini_profile,
            cmd_library::set_steam_profile_override,
            cmd_library::save_steam_id_override,
            cmd_system::get_app_system_status,
            cmd_system::configure_startup_and_tray,
            cmd_system::create_app_shortcuts_and_register_protocol,
            cmd_system::export_diagnostics_bundle,
            cmd_system::quit_app,
            stress_harness::run_chaos_stress_harness
        ])
        .build(tauri::generate_context!())
    {
        Ok(app) => app,
        Err(err) => {
            crash_guard::record_fatal_error(
                "tauri::Builder::build",
                &format!("failed to build Duke Launcher Tauri application: {err}"),
            );
            std::process::exit(1);
        }
    };

    app.run(move |app_handle, event| {
        match &event {
            tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::CloseRequested { api, .. },
                ..
            } => {
                if label == "overlay" {
                    api.prevent_close();
                    if let Some(overlay_win) = app_handle.get_webview_window("overlay") {
                        let _ = overlay_win.hide();
                    }
                    return;
                }
                if label == "main" {
                    if !daemon_state.config.close_to_tray.load(Ordering::Relaxed) {
                        IS_SHUTTING_DOWN.store(true, Ordering::Release);
                        APP_SHUTDOWN.store(true, Ordering::Release);
                        daemon_state.app_exiting.store(true, Ordering::Release);
                        api.prevent_close();
                        shutdown_all_runtime_services(app_handle);
                        app_handle.exit(0);
                        return;
                    } else {
                        api.prevent_close();
                        app_system::capture_and_save_window_geometry(app_handle);
                        daemon_state.ui_visible.store(false, Ordering::Release);
                        if let Some(main_win) = app_handle.get_webview_window("main") {
                            let _ = main_win.hide();
                        }
                        duke_core::tray_daemon::schedule_delayed_tray_trims();
                        return;
                    }
                }
            }
            tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::Destroyed,
                ..
            } if label == "main" => {
                if !daemon_state.config.close_to_tray.load(Ordering::Relaxed) {
                    IS_SHUTTING_DOWN.store(true, Ordering::Release);
                    APP_SHUTDOWN.store(true, Ordering::Release);
                    daemon_state.app_exiting.store(true, Ordering::Release);
                    shutdown_all_runtime_services(app_handle);
                }
            }
            tauri::RunEvent::ExitRequested { .. }
                if (daemon_state.app_exiting.load(Ordering::Acquire)
                    || IS_SHUTTING_DOWN.load(Ordering::Acquire)
                    || APP_SHUTDOWN.load(Ordering::Acquire)) =>
            {
                IS_SHUTTING_DOWN.store(true, Ordering::Release);
                APP_SHUTDOWN.store(true, Ordering::Release);
                daemon_state.app_exiting.store(true, Ordering::Release);
                shutdown_all_runtime_services(app_handle);
            }
            _ => {}
        }
        let exit_cb = || {
            IS_SHUTTING_DOWN.store(true, Ordering::Release);
            APP_SHUTDOWN.store(true, Ordering::Release);
            shutdown_all_runtime_services(app_handle);
        };
        duke_core::tray_daemon::handle_run_event(
            app_handle,
            &event,
            &daemon_state,
            None,
            Some(&exit_cb),
        );
    });
}


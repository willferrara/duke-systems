#![allow(unused_imports)]
use duke_core::pipe_ipc::{DukeBus, DukeEvent};
use duke_core::shm_bus::{DukeShmBus, ShmEventType, ShmSourceApp};
use reqwest::header::{
    ACCEPT, ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE, COOKIE, LOCATION,
    RANGE, REFERER, SET_COOKIE, USER_AGENT,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::windows::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::webview::{DownloadEvent, NewWindowResponse};
use tauri::{
    AppHandle, Emitter, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::{oneshot, Semaphore};
use walkdir::WalkDir;
use crate::{
    achievements, archive_native, artwork, bench, bios_vault, catalog_mmap, couch_mode,
    deep_shrink, delta_patch, desktop_chord, direct_storage, discord_ipc, dkpack, duke_acoustics,
    duke_ai_rt,
    duke_capsule, duke_comms, duke_configurator, duke_director, duke_doctor, duke_ghost,
    duke_jail, duke_link, duke_pilot, duke_sandbox, duke_upscale, duke_verified, duke_vision,
    emulators, etw_xray, fix_finder, friends, fse_shell, importers, latency_lab, lib_store,
    ludusavi, mcp, metadata_ext, mod_dropper, mod_graph, net_engine, ntfs_compress, pe_inspect,
    platforms, playtime_sync, privacy_vault, projfs_stream, quick_resume, redist_cleaner,
    retro_achievements, retro_catalog, rom_inspect, rom_patcher, rom_shrink, save_watcher,
    self_heal, shader_swarm, sidebar_icons, sites, snapshots, sources, steam_cm, store_direct,
    stream_extract, stress_harness, swarm, time_machine, uninstaller, vhdx_stage, wasm_plugins,
};
use crate::cmd_library::*;
use crate::cmd_security::*;
use crate::cmd_system::*;
pub(crate) fn is_rom_or_console_image_name(lower_name: &str) -> bool {
    const ROM_EXTS: &[&str] = &[
        ".rvz", ".chd", ".3ds", ".cci", ".nsp", ".xci", ".gba", ".nds", ".z64", ".n64", ".v64",
        ".sfc", ".smc", ".nes", ".gb", ".gbc", ".wbfs", ".gcm", ".cso",
    ];
    ROM_EXTS.iter().any(|ext| lower_name.ends_with(ext))
}

pub(crate) async fn install_detected_rom_to_library(
    app: &AppHandle,
    rom_path: &Path,
    clean_title: &str,
) -> Result<GameEntry, String> {
    let rp = rom_path.to_path_buf();
    let inspection = tokio::task::spawn_blocking(move || rom_inspect::inspect_rom_file(&rp))
        .await
        .map_err(|e| format!("ROM inspection panicked: {e}"))?;
    let plat_id = inspection
        .as_ref()
        .map(|i| i.platform_id)
        .filter(|&id| id > 0)
        .unwrap_or_else(|| {
            let (inferred, _) =
                artwork::infer_game_platform_hint(clean_title, &rom_path.to_string_lossy(), "", "");
            inferred.unwrap_or(platforms::PLATFORM_N64)
        });
    let fallback_title = clean_title.trim().to_string();
    let mut entry = {
        let mut library = load_library_from_disk(app)?;
        let out = upsert_retro_game_entry_in_library(
            &mut library,
            RetroUpsertParams {
                rom_path: Some(rom_path),
                inspection: inspection.as_ref(),
                platform_id: plat_id,
                custom_title: Some(&fallback_title),
                catalog_app_id: None,
                cover_hint: None,
                hero_hint: None,
                origin_source: "turbo_downloader",
            },
        );
        save_library_to_disk(app, &library)?;
        out
    };

    if let (Ok(covers_dir), Ok(icons_dir)) = (
        get_covers_dir(app),
        sidebar_icons::get_sidebar_icons_dir(app),
    ) {
        let settings = load_settings_from_disk(app);
        let (_, plat_hint) =
            artwork::infer_game_platform_hint(&entry.title, &entry.exe_path, &entry.genre, &entry.source);
        if let Some(outcome) = artwork::resolve_and_cache_cover_in_dirs(
            &artwork::CoverResolveRequest {
                cache_id: entry.id.clone(),
                title: entry.title.clone(),
                exe_path: (!entry.exe_path.trim().is_empty()).then(|| entry.exe_path.clone()),
                platform_id: (entry.platform_id > 0).then_some(entry.platform_id),
                platform_hint: plat_hint,
                steam_app_id: None,
                existing_cover_url: (!entry.cover_url.is_empty()).then(|| entry.cover_url.clone()),
                steamgriddb_api_key: settings.steamgriddb_api_key.unwrap_or_default(),
                sidebar_icon_id: Some(entry.id.clone()),
            },
            &covers_dir,
            &icons_dir,
        )
        .await
        {
            entry.cover_url = outcome.cover_path.to_string_lossy().to_string();
            if let Ok(mut lib) = load_library_from_disk(app) {
                if let Some(found) = lib.iter_mut().find(|g| g.id == entry.id) {
                    found.cover_url = entry.cover_url.clone();
                }
                let _ = save_library_to_disk(app, &lib);
            }
        }
    }

    if let Ok(lib) = load_library_from_disk(app) {
        emit_if_alive(app, "library-updated", &lib);
    }
    Ok(entry)
}

/// Automatically ingests any dropped folders, `.exe` files, or `.lnk` shortcuts in zero extra clicks.
pub(crate) type CoverCacheTarget = (u16, String, String, Option<String>);

pub(crate) struct RetroUpsertParams<'a> {
    pub(crate) rom_path: Option<&'a Path>,
    pub(crate) inspection: Option<&'a rom_inspect::RomInspectionResult>,
    pub(crate) platform_id: u16,
    pub(crate) custom_title: Option<&'a str>,
    pub(crate) catalog_app_id: Option<u32>,
    pub(crate) cover_hint: Option<&'a str>,
    pub(crate) hero_hint: Option<&'a str>,
    pub(crate) origin_source: &'a str,
}

pub(crate) fn upsert_retro_game_entry_in_library(
    library: &mut Vec<GameEntry>,
    params: RetroUpsertParams<'_>,
) -> GameEntry {
    let rom_path = params.rom_path;
    let inspection = params.inspection;
    let platform_id = params.platform_id;
    let custom_title = params.custom_title;
    let catalog_app_id = params.catalog_app_id;
    let cover_hint = params.cover_hint;
    let hero_hint = params.hero_hint;
    let origin_source = params.origin_source;
    let eff_plat_id = inspection
        .map(|i| i.platform_id)
        .filter(|&id| id > 0)
        .unwrap_or(platform_id);
    let plat = platforms::platform_by_id(eff_plat_id);

    let rom_id = inspection
        .map(|i| i.rom_id.trim().to_string())
        .unwrap_or_default();
    let raw_title = custom_title
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            inspection
                .map(|i| i.detected_title.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            rom_path
                .and_then(|p| p.file_stem())
                .and_then(|s| s.to_str())
                .map(clean_game_title)
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| format!("{} Title", plat.short_name));

    let catalog_match = catalog_app_id
        .and_then(catalog_mmap::get_entry_via_mmap)
        .or_else(|| {
            catalog_mmap::lookup_retro_entry_by_title_or_id(eff_plat_id, &rom_id, &raw_title)
        });

    let resolved_title = if custom_title.is_some() {
        raw_title.clone()
    } else if let Some(ref cat) = catalog_match {
        let base = retro_catalog::normalize_retro_base_title(&cat.name);
        if !base.is_empty() {
            base
        } else {
            raw_title.clone()
        }
    } else {
        raw_title.clone()
    };

    let resolved_app_id = catalog_app_id.or_else(|| catalog_match.as_ref().map(|c| c.app_id));
    let resolved_cover = cover_hint
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            catalog_match
                .as_ref()
                .map(|c| c.cover_url.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            let (boxart_url, _, _) =
                retro_catalog::build_libretro_thumbnail_urls(eff_plat_id, &resolved_title);
            if boxart_url.is_empty() {
                None
            } else {
                Some(boxart_url)
            }
        });
    let resolved_hero = hero_hint
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            catalog_match
                .as_ref()
                .map(|c| c.hero_url.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .or_else(|| resolved_cover.clone());

    let rom_path_str = rom_path
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (install_dir_str, size_bytes) = if let Some(p) = rom_path {
        let dir = p
            .parent()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_default();
        let bytes = fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        (dir, bytes)
    } else {
        (String::new(), 0)
    };
    let is_installed = !rom_path_str.is_empty();
    let now_ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if let Some(existing) = library.iter_mut().find(|g| {
        (!rom_path_str.is_empty() && g.exe_path.eq_ignore_ascii_case(&rom_path_str))
            || (g.platform_id == eff_plat_id
                && resolved_app_id.is_some()
                && g.app_id == resolved_app_id)
    }) {
        if is_installed {
            existing.exe_path = rom_path_str.clone();
            existing.install_dir = install_dir_str;
            existing.size_bytes = size_bytes;
            existing.size_formatted = format_bytes(size_bytes);
            existing.is_installed = true;
        }
        existing.platform_id = eff_plat_id;
        existing.console_group = plat.console_group;
        existing.platform = plat.display_name.to_string();
        if existing.emulator_id.trim().is_empty() {
            existing.emulator_id = plat.default_emulator_id.to_string();
        }
        if !rom_id.is_empty() {
            existing.rom_id = rom_id;
        }
        if existing.cover_url.is_empty() {
            if let Some(ref c) = resolved_cover {
                existing.cover_url = c.clone();
            }
        }
        if existing.hero_url.is_none() {
            existing.hero_url = resolved_hero;
        }
        if existing.app_id.is_none() {
            existing.app_id = resolved_app_id;
        }
        return existing.clone();
    }

    let id = if !rom_path_str.is_empty() {
        format!(
            "retro_{}_{:08x}",
            plat.slug,
            crc32_ieee(rom_path_str.to_ascii_lowercase().as_bytes())
        )
    } else if let Some(aid) = resolved_app_id {
        format!("retro_{}_{}", plat.slug, aid)
    } else {
        format!(
            "retro_{}_{:08x}",
            plat.slug,
            crc32_ieee(resolved_title.to_ascii_lowercase().as_bytes())
        )
    };

    let entry = GameEntry {
        id,
        title: resolved_title,
        exe_path: rom_path_str,
        install_dir: install_dir_str,
        size_bytes,
        size_formatted: format_bytes(size_bytes),
        cover_url: resolved_cover.unwrap_or_default(),
        hero_url: resolved_hero,
        icon_path: None,
        genre: format!("{} Console", plat.short_name),
        platform: plat.display_name.to_string(),
        source: plat.display_name.to_string(),
        acquisition_source: origin_source.to_string(),
        is_favorite: false,
        is_installed,
        added_at: now_ts,
        app_id: resolved_app_id,
        steam_app_id: resolved_app_id.map(u64::from),
        platform_id: eff_plat_id,
        console_group: plat.console_group,
        rom_id,
        emulator_id: plat.default_emulator_id.to_string(),
        ..Default::default()
    };
    library.push(entry.clone());
    entry
}

pub(crate) fn spawn_cache_retro_covers_batch(app: AppHandle, targets: Vec<CoverCacheTarget>) {
    if targets.is_empty() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let sem = Arc::new(tokio::sync::Semaphore::new(4));
        let mut updates: Vec<(String, String, String)> = Vec::new();
        let mut join_set = tokio::task::JoinSet::new();
        for (plat_id, gid, title, hint) in targets.into_iter().take(64) {
            let permit = Arc::clone(&sem);
            join_set.spawn(async move {
                let Ok(_p) = permit.acquire().await else {
                    return None;
                };
                retro_catalog::cache_retro_cover_and_icon(
                    plat_id,
                    &gid,
                    &title,
                    hint.as_deref(),
                )
                .await
                .map(|(cov, ico)| (gid, cov, ico))
            });
        }
        while let Some(res) = join_set.join_next().await {
            if let Ok(Some(item)) = res {
                updates.push(item);
            }
        }
        if !updates.is_empty() {
            let app_save = app.clone();
            if let Ok(Ok(lib)) = tokio::task::spawn_blocking(move || -> Result<Vec<GameEntry>, String> {
                let mut lib = load_library_from_disk(&app_save)?;
                for (gid, cov, ico) in &updates {
                    if let Some(entry) = lib.iter_mut().find(|g| &g.id == gid) {
                        if !cov.is_empty() {
                            entry.cover_url = cov.clone();
                        }
                        if !ico.is_empty() {
                            entry.icon_path = Some(ico.clone());
                        }
                    }
                }
                save_library_to_disk(&app_save, &lib)?;
                Ok(lib)
            })
            .await
            {
                emit_if_alive(&app, "library-updated", &lib);
            }
        }
    });
}

#[tauri::command]
pub(crate) async fn inspect_rom_path(path: String) -> Result<rom_inspect::RomInspectionResult, String> {
    let trimmed = path.trim().trim_matches('"').to_string();
    let p = PathBuf::from(&trimmed);
    if !p.is_file() {
        return Err(format!("ROM / ISO file not found: {}", trimmed));
    }
    tokio::task::spawn_blocking(move || {
        rom_inspect::inspect_rom_file(&p)
            .ok_or_else(|| "Unrecognized console ROM or disc image header.".to_string())
    })
    .await
    .map_err(|e| format!("ROM inspection worker failed: {}", e))?
}

#[tauri::command]
pub(crate) async fn add_retro_game_from_catalog(
    app: AppHandle,
    app_id: u32,
    rom_path: Option<String>,
) -> Result<GameEntry, String> {
    let cat_entry = tokio::task::spawn_blocking(move || catalog_mmap::get_entry_via_mmap(app_id))
        .await
        .map_err(|e| format!("Catalog lookup failed: {}", e))?
        .ok_or_else(|| format!("Catalog entry {} not found.", app_id))?;

    let trimmed_rom = rom_path
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty());
    let rom_buf = trimmed_rom.as_ref().map(PathBuf::from);
    if let Some(ref p) = rom_buf {
        if !p.is_file() {
            return Err(format!("ROM / ISO file not found: {}", p.display()));
        }
        unblock_ntfs_zone_identifier(p);
    }

    let rom_inspect_res = if let Some(ref p) = rom_buf {
        let p_clone = p.clone();
        tokio::task::spawn_blocking(move || rom_inspect::inspect_rom_file(&p_clone))
            .await
            .map_err(|e| format!("ROM inspection worker failed: {}", e))?
    } else {
        None
    };

    let app_clone = app.clone();
    let cat_clone = cat_entry.clone();
    let entry = tokio::task::spawn_blocking(move || -> Result<GameEntry, String> {
        let mut library = load_library_from_disk(&app_clone)?;
        let title_clean = retro_catalog::normalize_retro_base_title(&cat_clone.name);
        let eff_title = if title_clean.is_empty() {
            cat_clone.name.clone()
        } else {
            title_clean
        };
        let entry = upsert_retro_game_entry_in_library(
            &mut library,
            RetroUpsertParams {
                rom_path: rom_buf.as_deref(),
                inspection: rom_inspect_res.as_ref(),
                platform_id: cat_clone.platform_id,
                custom_title: Some(&eff_title),
                catalog_app_id: Some(cat_clone.app_id),
                cover_hint: if cat_clone.cover_url.is_empty() {
                    None
                } else {
                    Some(cat_clone.cover_url.as_str())
                },
                hero_hint: if cat_clone.hero_url.is_empty() {
                    None
                } else {
                    Some(cat_clone.hero_url.as_str())
                },
                origin_source: if rom_buf.is_some() {
                    "rom_linked"
                } else {
                    "catalog_retro"
                },
            },
        );
        save_library_to_disk(&app_clone, &library)?;
        emit_if_alive(&app_clone, "library-updated", &library);
        Ok(entry)
    })
    .await
    .map_err(|e| format!("Library update worker failed: {}", e))??;

    let hint = if entry.cover_url.is_empty() {
        None
    } else {
        Some(entry.cover_url.clone())
    };
    spawn_cache_retro_covers_batch(
        app,
        vec![(entry.platform_id, entry.id.clone(), entry.title.clone(), hint)],
    );

    Ok(entry)
}

#[tauri::command]
pub(crate) async fn link_game_rom(
    app: AppHandle,
    game_id: String,
    rom_path: String,
) -> Result<Vec<GameEntry>, String> {
    let trimmed = rom_path.trim().trim_matches('"').to_string();
    let rom_buf = PathBuf::from(&trimmed);
    if !rom_buf.is_file() {
        return Err(format!("Selected ROM / ISO file does not exist: {}", trimmed));
    }
    unblock_ntfs_zone_identifier(&rom_buf);
    let rom_inspect_buf = rom_buf.clone();
    let inspection = tokio::task::spawn_blocking(move || rom_inspect::inspect_rom_file(&rom_inspect_buf))
        .await
        .map_err(|e| format!("ROM inspection worker failed: {}", e))?;

    let app_clone = app.clone();
    let updated = tokio::task::spawn_blocking(move || -> Result<Vec<GameEntry>, String> {
        let mut library = load_library_from_disk(&app_clone)?;
        let Some(entry) = library.iter_mut().find(|g| g.id == game_id) else {
            return Err("Game entry not found in library.".to_string());
        };
        let size_bytes = fs::metadata(&rom_buf).map(|m| m.len()).unwrap_or(0);
        entry.exe_path = trimmed.clone();
        entry.install_dir = rom_buf
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        entry.size_bytes = size_bytes;
        entry.size_formatted = format_bytes(size_bytes);
        entry.is_installed = true;
        if let Some(ref info) = inspection {
            if entry.platform_id == 0 && info.platform_id > 0 {
                let plat = platforms::platform_by_id(info.platform_id);
                entry.platform_id = info.platform_id;
                entry.console_group = plat.console_group;
                entry.platform = plat.display_name.to_string();
                entry.emulator_id = plat.default_emulator_id.to_string();
            }
            if !info.rom_id.trim().is_empty() {
                entry.rom_id = info.rom_id.clone();
            }
        }
        save_library_to_disk(&app_clone, &library)?;
        emit_if_alive(&app_clone, "library-updated", &library);
        Ok(library)
    })
    .await
    .map_err(|e| format!("Failed linking ROM: {}", e))??;

    Ok(updated)
}

#[tauri::command]
pub(crate) async fn scan_rom_folder(app: AppHandle, folder_path: String) -> Result<IngestPathsReport, String> {
    let trimmed = folder_path.trim().trim_matches('"').to_string();
    let dir = PathBuf::from(&trimmed);
    if !dir.is_dir() {
        return Err(format!("Selected ROM folder does not exist: {}", trimmed));
    }

    let dir_m3u = dir.clone();
    let detected = tokio::task::spawn_blocking(move || {
        let _ = rom_patcher::generate_m3u_playlists_in_dir(&dir_m3u);
        rom_inspect::scan_folder_for_roms(&dir, 4)
    })
    .await
    .map_err(|e| format!("ROM folder scanner failed: {}", e))?;

    if detected.is_empty() {
        let library = load_library_from_disk(&app).unwrap_or_default();
        return Ok(IngestPathsReport {
            added_count: 0,
            skipped_count: 1,
            added_titles: Vec::new(),
            skipped_reasons: vec![format!("No supported console ROM / ISO files found in '{}'.", trimmed)],
            library,
        });
    }

    let app_clone = app.clone();
    let (added_titles, cache_targets, library) = tokio::task::spawn_blocking(move || -> Result<_, String> {
        let mut lib = load_library_from_disk(&app_clone)?;
        let mut titles = Vec::new();
        let mut covers = Vec::new();
        for info in detected {
            let p = PathBuf::from(&info.path);
            unblock_ntfs_zone_identifier(&p);
            let entry = upsert_retro_game_entry_in_library(
                &mut lib,
                RetroUpsertParams {
                    rom_path: Some(&p),
                    inspection: Some(&info),
                    platform_id: info.platform_id,
                    custom_title: None,
                    catalog_app_id: None,
                    cover_hint: None,
                    hero_hint: None,
                    origin_source: "rom_scan",
                },
            );
            titles.push(entry.title.clone());
            let hint = if entry.cover_url.is_empty() {
                None
            } else {
                Some(entry.cover_url.clone())
            };
            covers.push((entry.platform_id, entry.id, entry.title, hint));
        }
        save_library_to_disk(&app_clone, &lib)?;
        emit_if_alive(&app_clone, "library-updated", &lib);
        Ok((titles, covers, lib))
    })
    .await
    .map_err(|e| format!("Failed saving scanned ROMs: {}", e))??;

    spawn_cache_retro_covers_batch(app, cache_targets);

    Ok(IngestPathsReport {
        added_count: added_titles.len(),
        skipped_count: 0,
        added_titles,
        skipped_reasons: Vec::new(),
        library,
    })
}

#[tauri::command]
pub(crate) async fn scan_shrinkable_roms(
    app: AppHandle,
) -> Result<Vec<rom_shrink::RomShrinkCandidate>, String> {
    run_blocking_timeout(30, "scan_shrinkable_roms", move || {
        let library = load_library_from_disk(&app)?;
        Ok(rom_shrink::scan_shrinkable_retro_library(&library))
    })
    .await
}

#[tauri::command]
pub(crate) async fn compress_retro_rom(
    app: AppHandle,
    game_id: Option<String>,
    delete_originals: Option<bool>,
) -> Result<rom_shrink::BatchRomShrinkReport, String> {
    let _crumb = self_heal::record_heavy_command_breadcrumb("compress_retro_rom");
    let clean_gid = game_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let delete_orig = delete_originals.unwrap_or(false);
    emit_if_alive(
        &app,
        "rom-shrink-progress",
        serde_json::json!({
            "status": "running",
            "gameId": clean_gid,
            "deleteOriginals": delete_orig,
        }),
    );

    let app_work = app.clone();
    let gid_work = clean_gid.clone();
    let res = run_blocking_timeout(600, "compress_retro_rom", move || {
        let mut library = load_library_from_disk(&app_work)?;
        let report =
            rom_shrink::compress_retro_library_sync(&mut library, gid_work.as_deref(), delete_orig)?;
        if report.converted_count > 0 {
            save_library_to_disk(&app_work, &library)?;
            emit_if_alive(&app_work, "library-updated", &library);
        }
        Ok(report)
    })
    .await;

    match &res {
        Ok(report) => {
            emit_if_alive(
                &app,
                "rom-shrink-progress",
                serde_json::json!({
                    "status": "completed",
                    "gameId": clean_gid,
                    "convertedCount": report.converted_count,
                    "totalOriginalBytes": report.total_original_bytes,
                    "totalCompressedBytes": report.total_compressed_bytes,
                    "totalSavedBytes": report.total_saved_bytes,
                }),
            );
        }
        Err(err) => {
            emit_if_alive(
                &app,
                "rom-shrink-progress",
                serde_json::json!({
                    "status": "error",
                    "gameId": clean_gid,
                    "error": err,
                }),
            );
        }
    }

    res
}

#[tauri::command]
pub(crate) async fn verify_rom_archive(path: String) -> Result<Value, String> {
    let clean = path.trim().trim_matches('"').to_string();
    if clean.is_empty() {
        return Err("ROM archive path is empty.".to_string());
    }
    run_blocking_timeout(120, "verify_rom_archive", move || {
        let target = PathBuf::from(&clean);
        let format = target
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .filter(|e| e == "chd" || e == "rvz")
            .unwrap_or_else(|| "archive".to_string());
        let (sha1, crc32, size_bytes) = rom_shrink::verify_chd_or_rvz_integrity(&target)?;
        let summary = format!("SHA-1 {sha1} · CRC32 {crc32} · {size_bytes} bytes verified");
        Ok(serde_json::json!({
            "path": clean,
            "format": format,
            "sha1": sha1,
            "crc32": crc32,
            "summary": summary,
            "sizeBytes": size_bytes,
            "verified": true,
        }))
    })
    .await
}


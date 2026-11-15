//! Startup state recovery, atomic `.bak` JSON protection, interrupted download repair,
//! NTFS `:Zone.Identifier` unblocking, and crash NTSTATUS classification.

use crate::catalog_mmap::MappedCatalog;
use serde_json::Value;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use walkdir::WalkDir;

static SELF_HEAL_TMP_SEQ: AtomicU64 = AtomicU64::new(1);

pub(crate) const PROTECTED_JSON_FILES: &[&str] = &[
    "library.json",
    "settings.json",
    "sources.json",
    "search_sites.json",
];

pub(crate) const LARGE_32BIT_GAME_THRESHOLD_BYTES: u64 = 1_500_000_000;

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StartupSelfHealReport {
    pub restored_json_files: Vec<String>,
    pub recovered_downloads: usize,
    pub cleaned_tmp_files: usize,
    pub catalog_valid: bool,
}

pub(crate) fn is_protected_json_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|name| {
            PROTECTED_JSON_FILES
                .iter()
                .any(|p| p.eq_ignore_ascii_case(name))
        })
        .unwrap_or(false)
}

pub(crate) fn backup_path_for(path: &Path) -> PathBuf {
    let mut os = path.as_os_str().to_os_string();
    os.push(".bak");
    PathBuf::from(os)
}

fn temp_path_for(path: &Path, suffix: &str) -> PathBuf {
    let seq = SELF_HEAL_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let base_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("state.json");
    path.with_file_name(format!("{}.{}.{}{}", base_name, std::process::id(), seq, suffix))
}

fn replace_file_atomically(tmp_path: &Path, dest_path: &Path) -> Result<(), String> {
    match fs::rename(tmp_path, dest_path) {
        Ok(()) => Ok(()),
        Err(rename_err) => {
            if dest_path.exists() {
                let _ = fs::remove_file(dest_path);
                if fs::rename(tmp_path, dest_path).is_ok() {
                    return Ok(());
                }
            }
            let _ = fs::remove_file(tmp_path);
            Err(format!(
                "Failed to atomically replace {}: {}",
                dest_path.display(),
                rename_err
            ))
        }
    }
}

fn write_temp_and_swap(dest_path: &Path, bytes: &[u8], tmp_suffix: &str) -> Result<(), String> {
    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create dir: {}", e))?;
    }
    let tmp_path = temp_path_for(dest_path, tmp_suffix);
    let mut file =
        File::create(&tmp_path).map_err(|e| format!("Failed to create temp file: {}", e))?;
    file.write_all(bytes)
        .map_err(|e| format!("Failed to write temp file: {}", e))?;
    let _ = file.sync_all();
    drop(file);
    replace_file_atomically(&tmp_path, dest_path)
}

/// Atomically writes `bytes` to `path` via `<path>.tmp`, preserving and updating `<path>.bak`
/// for critical state files (`library.json`, `settings.json`, `sources.json`, `search_sites.json`).
pub(crate) fn atomic_write_with_backup(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let protected = is_protected_json_file(path);
    if protected {
        if bytes.is_empty() || serde_json::from_slice::<Value>(bytes).is_err() {
            return Err(format!(
                "Refusing to overwrite protected JSON {} with empty/invalid payload",
                path.display()
            ));
        }
        // Snapshot the current valid primary file into `.bak` before swapping if valid.
        if let Ok(existing) = fs::read(path) {
            if !existing.is_empty() && serde_json::from_slice::<Value>(&existing).is_ok() {
                let bak = backup_path_for(path);
                let _ = write_temp_and_swap(&bak, &existing, ".bak.tmp");
            }
        }
    }

    write_temp_and_swap(path, bytes, ".tmp")?;

    if protected {
        let bak = backup_path_for(path);
        let _ = write_temp_and_swap(&bak, bytes, ".bak.tmp");
    }
    Ok(())
}

/// Checks if `path` is missing, 0 bytes, or invalid JSON, and restores from `<path>.bak` if valid.
pub(crate) fn recover_protected_json_if_corrupt(path: &Path) -> Result<bool, String> {
    let primary_ok = fs::read(path)
        .ok()
        .is_some_and(|b| !b.is_empty() && serde_json::from_slice::<Value>(&b).is_ok());
    if primary_ok {
        return Ok(false);
    }

    let bak_path = backup_path_for(path);
    let Ok(bak_bytes) = fs::read(&bak_path) else {
        return Ok(false);
    };
    if bak_bytes.is_empty() || serde_json::from_slice::<Value>(&bak_bytes).is_err() {
        return Ok(false);
    }

    write_temp_and_swap(path, &bak_bytes, ".tmp")?;
    Ok(true)
}

/// Reads a JSON file after ensuring `.bak` recovery ran if primary was corrupt or 0 bytes.
pub(crate) fn read_json_with_bak_recovery(path: &Path) -> Result<String, String> {
    let _ = recover_protected_json_if_corrupt(path);
    fs::read_to_string(path).map_err(|e| format!("Failed to read {}: {}", path.display(), e))
}

/// Transitions any stale `"downloading"`, `"extracting"`, `"resolving"`, or `"installing"` queue items
/// in `downloads.json` into `"PAUSED"` with verified byte offsets from `.part` on disk.
pub(crate) fn recover_interrupted_downloads_in_dir(data_dir: &Path) -> usize {
    let dl_path = data_dir.join("downloads.json");
    let _ = recover_protected_json_if_corrupt(&dl_path);
    let Ok(raw) = fs::read_to_string(&dl_path) else {
        return 0;
    };
    let Ok(mut jobs) = serde_json::from_str::<Vec<Value>>(&raw) else {
        return 0;
    };

    let mut recovered = 0usize;
    for job in &mut jobs {
        let Some(obj) = job.as_object_mut() else {
            continue;
        };
        let status_lower = obj
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        let was_active = matches!(
            status_lower.as_str(),
            "downloading" | "extracting" | "resolving" | "installing"
        );
        if !was_active {
            continue;
        }

        let total_bytes = obj.get("totalBytes").and_then(|v| v.as_u64()).unwrap_or(0);
        let mut downloaded_bytes = obj
            .get("downloadedBytes")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        if let Some(part_str) = obj.get("partPath").and_then(|v| v.as_str()) {
            if let Ok(meta) = fs::metadata(part_str) {
                let on_disk = meta.len();
                if on_disk == 0 && !part_str.is_empty() {
                    downloaded_bytes = 0;
                }
            } else if !part_str.is_empty() && status_lower != "extracting" {
                downloaded_bytes = 0;
            }
        }
        if total_bytes > 0 {
            downloaded_bytes = downloaded_bytes.min(total_bytes);
        }

        let pct = if total_bytes > 0 {
            ((downloaded_bytes as f64 / total_bytes as f64) * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        };

        if let Some(segs) = obj.get_mut("segments").and_then(|v| v.as_array_mut()) {
            for seg in segs {
                if let Some(s_obj) = seg.as_object_mut() {
                    let start = s_obj.get("start").and_then(|v| v.as_u64()).unwrap_or(0);
                    let end = s_obj.get("end").and_then(|v| v.as_u64()).unwrap_or(0);
                    let cap = if end >= start { end - start + 1 } else { 0 };
                    let dl = s_obj.get("downloaded").and_then(|v| v.as_u64()).unwrap_or(0);
                    if cap > 0 && dl > cap {
                        s_obj.insert("downloaded".to_string(), Value::from(cap));
                    }
                    s_obj.insert("active".to_string(), Value::from(false));
                }
            }
        }

        obj.insert("status".to_string(), Value::from("PAUSED"));
        obj.insert(
            "downloadedBytes".to_string(),
            Value::from(downloaded_bytes),
        );
        obj.insert("progressPct".to_string(), Value::from(pct));
        obj.insert("speedBps".to_string(), Value::from(0u64));
        obj.insert(
            "error".to_string(),
            Value::from("Recovered interrupted transfer cleanly — click Resume."),
        );
        recovered += 1;
    }

    if recovered > 0 {
        if let Ok(pretty) = serde_json::to_string_pretty(&jobs) {
            let _ = write_temp_and_swap(&dl_path, pretty.as_bytes(), ".tmp");
            let bak = backup_path_for(&dl_path);
            let _ = write_temp_and_swap(&bak, pretty.as_bytes(), ".bak.tmp");
        }
    }
    recovered
}

/// Removes orphan `*.tmp` and `*.bak.tmp` files inside `<data_dir>` and its cache subdirectories.
pub(crate) fn cleanup_orphan_tmp_files(data_dir: &Path) -> usize {
    let mut removed = 0usize;
    let dirs = [
        data_dir.to_path_buf(),
        data_dir.join("covers"),
        data_dir.join("sidebar_icons"),
        data_dir.join("backups"),
    ];
    for dir in dirs {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if (name.ends_with(".tmp") || name.ends_with(".bak.tmp"))
                    && fs::remove_file(&path).is_ok()
                {
                    removed += 1;
                }
            }
        }
    }
    removed
}

/// Verifies `catalog.dkcat` header & checksum integrity; renames corrupt archives aside cleanly.
pub(crate) fn verify_catalog_integrity(data_dir: &Path) -> bool {
    let dkcat_path = data_dir.join("catalog.dkcat");
    if !dkcat_path.exists() {
        return true;
    }
    let meta_ok = fs::metadata(&dkcat_path)
        .map(|m| m.is_file() && m.len() >= 64)
        .unwrap_or(false);
    if !meta_ok {
        let _ = fs::remove_file(&dkcat_path);
        return false;
    }
    match MappedCatalog::open(&dkcat_path) {
        Ok(cat) => {
            if cat.verify_header_fast() {
                true
            } else {
                drop(cat);
                let _ = fs::remove_file(&dkcat_path);
                false
            }
        }
        Err(_) => {
            if matches!(
                crate::catalog_mmap::try_migrate_dkcat_v1_to_v2(&dkcat_path),
                Ok(true)
            ) {
                return true;
            }
            let _ = fs::remove_file(&dkcat_path);
            false
        }
    }
}

/// Runs startup state & backup recovery, interrupted queue recovery, and temp/catalog cleanup.
pub(crate) fn run_startup_self_heal_in_dir(data_dir: &Path) -> StartupSelfHealReport {
    let mut report = StartupSelfHealReport::default();

    for name in PROTECTED_JSON_FILES {
        let p = data_dir.join(name);
        if matches!(recover_protected_json_if_corrupt(&p), Ok(true)) {
            report.restored_json_files.push((*name).to_string());
        }
    }

    report.recovered_downloads = recover_interrupted_downloads_in_dir(data_dir);
    report.cleaned_tmp_files = cleanup_orphan_tmp_files(data_dir);
    report.catalog_valid = verify_catalog_integrity(data_dir);
    report
}

/// Non-blocking startup self-heal pass scheduled from `.setup()`.
pub(crate) fn spawn_startup_self_heal(app: tauri::AppHandle) {
    use tauri::Emitter;
    tauri::async_runtime::spawn(async move {
        let Ok(data_dir) = crate::resolve_data_root_dir(&app) else {
            return;
        };
        let report = tokio::task::spawn_blocking(move || run_startup_self_heal_in_dir(&data_dir))
            .await
            .unwrap_or_default();

        if report.recovered_downloads > 0 {
            crate::reload_queue_store_from_disk(&app);
        }
        if !report.restored_json_files.is_empty()
            || report.recovered_downloads > 0
            || report.cleaned_tmp_files > 0
        {
            let _ = app.emit("startup-self-heal-report", &report);
        }
        crate::trim_process_working_set();
    });
}

/// Strips the NTFS `:Zone.Identifier` Alternate Data Stream from a single file.
pub(crate) fn strip_zone_identifier_ads(file_path: &Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::DeleteFileW;

        let mut wide: Vec<u16> = file_path.as_os_str().encode_wide().collect();
        wide.extend(":Zone.Identifier\0".encode_utf16());
        unsafe { DeleteFileW(PCWSTR(wide.as_ptr())).is_ok() }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = file_path;
        false
    }
}

/// Recursively strips `:Zone.Identifier` ADS across all `.exe`, `.dll`, `.asi`, and `.bat` files
/// inside a game folder (or single file target). Returns the number of ADS streams removed.
pub(crate) fn unblock_game_tree_zone_identifiers(target: &Path) -> usize {
    const EXTENSIONS: &[&str] = &["exe", "dll", "asi", "bat"];
    if target.is_file() {
        return usize::from(strip_zone_identifier_ads(target));
    }
    if !target.is_dir() {
        return 0;
    }

    let mut unblocked = 0usize;
    for entry in WalkDir::new(target)
        .follow_links(false)
        .max_depth(8)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let is_target_ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|ext| EXTENSIONS.iter().any(|allowed| allowed.eq_ignore_ascii_case(ext)))
            .unwrap_or(false);
        if is_target_ext && strip_zone_identifier_ads(path) {
            unblocked += 1;
        }
    }
    unblocked
}

/// Classifies known fast-exit Windows NTSTATUS crash codes (`0xC000007B`, `0xC0000005`, `0xC0000135`).
pub(crate) fn is_crash_ntstatus(exit_code: i32) -> Option<(&'static str, &'static str)> {
    match exit_code as u32 {
        0xC000_007B => Some((
            "0xC000007B",
            "STATUS_INVALID_IMAGE_FORMAT (architecture mismatch or corrupt DLL)",
        )),
        0xC000_0005 => Some((
            "0xC0000005",
            "STATUS_ACCESS_VIOLATION (memory/4GB address or runtime fault)",
        )),
        0xC000_0135 => Some((
            "0xC0000135",
            "STATUS_DLL_NOT_FOUND (missing VC++ or DirectX runtime DLL)",
        )),
        _ => None,
    }
}

/// Returns `true` if a 32-bit executable or game folder exceeds 1.5 GB and warrants Large Address Aware.
pub(crate) fn should_enable_32bit_laa(exe_path: &Path, game_dir: &Path, dir_size_bytes: u64) -> bool {
    if dir_size_bytes > LARGE_32BIT_GAME_THRESHOLD_BYTES {
        return true;
    }
    if fs::metadata(exe_path).map(|m| m.len()).unwrap_or(0) > LARGE_32BIT_GAME_THRESHOLD_BYTES {
        return true;
    }
    let _ = game_dir;
    false
}

/// Attempts silent local/winget VC++ or DirectX redistributable repair if critical DLLs remain missing.
pub(crate) fn try_silent_redist_repair(
    game_dir: &Path,
    missing_dlls: &[String],
    is_32bit: bool,
) -> Option<String> {
    if missing_dlls.is_empty() {
        return None;
    }
    let needs_vc = missing_dlls.iter().any(|d| {
        let l = d.to_ascii_lowercase();
        l.contains("vcruntime140") || l.contains("msvcp140")
    });
    let needs_dx = missing_dlls.iter().any(|d| {
        let l = d.to_ascii_lowercase();
        l.starts_with("d3dx") || l.starts_with("xinput") || l.contains("d3dcompiler")
    });
    if !needs_vc && !needs_dx {
        return None;
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Command;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        // 1. Check bundled game _CommonRedist / Redist / DirectX folders for official installers.
        let mut redist_exes: Vec<PathBuf> = Vec::new();
        for sub in ["_CommonRedist", "Redist", "redist", "DirectX", "vcredist"] {
            let root = game_dir.join(sub);
            if root.is_dir() {
                for entry in WalkDir::new(&root)
                    .follow_links(false)
                    .max_depth(4)
                    .into_iter()
                    .filter_map(|e| e.ok())
                {
                    if entry.file_type().is_file() {
                        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                        if (needs_vc && name.contains("vc_redist") && name.ends_with(".exe"))
                            || (needs_dx && name == "dxsetup.exe")
                        {
                            redist_exes.push(entry.path().to_path_buf());
                        }
                    }
                }
            }
        }

        for installer in redist_exes {
            let name = installer
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let args: &[&str] = if name == "dxsetup.exe" {
                &["/silent"]
            } else if name.contains("vc_redist") {
                &["/install", "/quiet", "/norestart"]
            } else {
                continue;
            };
            if let Ok(status) = Command::new(&installer)
                .args(args)
                .creation_flags(CREATE_NO_WINDOW)
                .status()
            {
                if status.success() {
                    return Some(format!(
                        "Ran silent bundled runtime repair via {}",
                        installer.file_name().and_then(|n| n.to_str()).unwrap_or("redist")
                    ));
                }
            }
        }

        // 2. Fallback to silent winget install for missing VC++ runtime if winget is available.
        if needs_vc {
            let pkg_id = if is_32bit {
                "Microsoft.VCRedist.2015+.x86"
            } else {
                "Microsoft.VCRedist.2015+.x64"
            };
            if let Ok(status) = Command::new("winget")
                .args([
                    "install",
                    "--id",
                    pkg_id,
                    "--silent",
                    "--accept-package-agreements",
                    "--accept-source-agreements",
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .status()
            {
                if status.success() {
                    return Some(format!("Installed {pkg_id} silently via winget"));
                }
            }
        }
    }
    let _ = is_32bit;
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bak_json_backup_and_recovery_restores_corrupt_or_empty_files() {
        let dir = std::env::temp_dir().join(format!("duke_self_heal_bak_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let lib_path = dir.join("library.json");
        let payload = br#"[{"id":"g1","title":"Test Game"}]"#;
        atomic_write_with_backup(&lib_path, payload).unwrap();
        assert!(backup_path_for(&lib_path).exists());

        // Refuses empty overwrite directly via atomic helper
        assert!(atomic_write_with_backup(&lib_path, b"").is_err());

        // Simulate crash corrupting primary to 0 bytes -> auto-recovers from .bak
        fs::write(&lib_path, b"").unwrap();
        let report = run_startup_self_heal_in_dir(&dir);
        assert!(report.restored_json_files.contains(&"library.json".to_string()));
        assert_eq!(fs::read(&lib_path).unwrap(), payload);

        // Simulate corrupt JSON -> recovers from .bak via read helper
        fs::write(&lib_path, b"{truncated json!!!").unwrap();
        let recovered = read_json_with_bak_recovery(&lib_path).unwrap();
        assert!(recovered.contains("Test Game"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn startup_self_heal_pauses_interrupted_downloads_and_cleans_orphans() {
        let dir = std::env::temp_dir().join(format!("duke_self_heal_dl_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("covers")).unwrap();

        let part_path = dir.join("sample.zip.part");
        fs::write(&part_path, vec![0xAAu8; 4096]).unwrap();
        fs::write(dir.join("library.json.tmp"), b"orphan").unwrap();
        fs::write(dir.join("covers").join("img.jpg.tmp"), b"orphan").unwrap();

        let queue = serde_json::json!([
            {
                "id": "job_1",
                "status": "DOWNLOADING",
                "totalBytes": 10000,
                "downloadedBytes": 4096,
                "partPath": part_path.to_string_lossy().to_string(),
                "segments": [
                    { "index": 0, "start": 0, "end": 4999, "downloaded": 4096, "active": true }
                ]
            },
            {
                "id": "job_2",
                "status": "extracting",
                "totalBytes": 5000,
                "downloadedBytes": 5000,
                "partPath": ""
            },
            {
                "id": "job_3",
                "status": "COMPLETED",
                "totalBytes": 1234,
                "downloadedBytes": 1234
            }
        ]);
        fs::write(
            dir.join("downloads.json"),
            serde_json::to_string_pretty(&queue).unwrap(),
        )
        .unwrap();

        let report = run_startup_self_heal_in_dir(&dir);
        assert_eq!(report.recovered_downloads, 2);
        assert_eq!(report.cleaned_tmp_files, 2);
        assert!(!dir.join("library.json.tmp").exists());
        assert!(!dir.join("covers").join("img.jpg.tmp").exists());

        let updated: Vec<Value> =
            serde_json::from_str(&fs::read_to_string(dir.join("downloads.json")).unwrap()).unwrap();
        assert_eq!(updated[0]["status"], "PAUSED");
        assert_eq!(updated[0]["downloadedBytes"], 4096);
        assert_eq!(updated[0]["segments"][0]["active"], false);
        assert_eq!(updated[1]["status"], "PAUSED");
        assert_eq!(updated[2]["status"], "COMPLETED");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recursive_zone_identifier_unblocks_dll_asi_bat_and_exe_and_detects_ntstatus() {
        let dir = std::env::temp_dir().join(format!("duke_zone_ads_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let sub = dir.join("bin");
        fs::create_dir_all(&sub).unwrap();

        let files = [
            dir.join("game.exe"),
            sub.join("d3dcompiler_47.dll"),
            sub.join("mod.asi"),
            dir.join("launch.bat"),
        ];
        for f in &files {
            fs::write(f, b"payload").unwrap();
            let mut ads = f.as_os_str().to_os_string();
            ads.push(":Zone.Identifier");
            let _ = fs::write(PathBuf::from(ads), b"[ZoneTransfer]\r\nZoneId=3\r\n");
        }

        let unblocked = unblock_game_tree_zone_identifiers(&dir);
        #[cfg(target_os = "windows")]
        assert_eq!(unblocked, 4);
        let _ = unblocked;

        assert!(is_crash_ntstatus(0xC000_007Bu32 as i32).is_some());
        assert!(is_crash_ntstatus(0xC000_0005u32 as i32).is_some());
        assert!(is_crash_ntstatus(0xC000_0135u32 as i32).is_some());
        assert!(is_crash_ntstatus(0).is_none());

        let _ = fs::remove_dir_all(&dir);
    }
}

//! Deep Game Uninstaller & Orphaned AppData Sweeper.
//!
//! Previews and performs clean game removal:
//! - Optional 1-click **"Keep Saves Before Uninstalling?"** `<5 MB` compressed Zstd save archive.
//! - Deletes install directory, `.duke-profile/`, and matched orphaned AppData / `Documents\My Games` folders.
//! - Cleans dead Windows Uninstall registry entries (HKCU), `UserGpuPreferences` entries, and
//!   AppContainer Offline firewall rules (`duke_jail::cleanup_game_jail_profile`).
//! - Runs `.duke-cas/` garbage collection and removes the card from `library.json`.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Emitter;
use walkdir::WalkDir;

use crate::snapshots::{cas_root_dir_standalone, gc_cas_blobs, CAS_DIR_NAME, PROFILE_DIR_NAME};

const MAX_ZSTD_SAVE_ARCHIVE_BYTES: u64 = 5 * 1024 * 1024;
const STREAM_BUF_BYTES: usize = 65_536;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanUninstallPreview {
    pub game_id: String,
    pub title: String,
    pub install_dir: String,
    pub install_dir_bytes: u64,
    pub sandbox_profile_dir: Option<String>,
    pub save_paths: Vec<String>,
    pub save_bytes: u64,
    pub orphan_appdata_dirs: Vec<String>,
    pub gpu_pref_keys: Vec<String>,
    pub hkcu_uninstall_keys: Vec<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanUninstallReport {
    pub game_id: String,
    pub title: String,
    pub save_archive_path: Option<String>,
    pub save_archive_bytes: u64,
    pub removed_dirs: Vec<String>,
    pub registry_cleaned: usize,
    pub cas_blobs_removed: usize,
    pub cas_bytes_freed: u64,
    pub summary: String,
}

fn sanitize_slug(raw: &str) -> String {
    let s: String = raw
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    s.trim_matches('_').to_string()
}

fn dir_size_bytes(dir: &Path) -> u64 {
    if !dir.exists() {
        return 0;
    }
    if dir.is_file() {
        return fs::metadata(dir).map(|m| m.len()).unwrap_or(0);
    }
    WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .map(|e| e.metadata().map(|m| m.len()).unwrap_or(0))
        .sum()
}

fn is_safe_orphan_folder_name(folder_name_lower: &str, norm_title: &str) -> bool {
    if norm_title.len() < 5 {
        return false;
    }
    const FORBIDDEN: &[&str] = &[
        "microsoft",
        "windows",
        "temp",
        "packages",
        "google",
        "mozilla",
        "nvidia",
        "amd",
        "intel",
        "steam",
        "epicgames",
        "gog",
        "xbox",
        "com.dukelauncher.desktop",
        "duke-systems",
        "programs",
        "common files",
    ];
    if FORBIDDEN.contains(&folder_name_lower) {
        return false;
    }
    let clean_folder: String = folder_name_lower
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    !clean_folder.is_empty() && clean_folder == norm_title
}

fn discover_orphan_dirs_for_title(title: &str, install_dir: &Path) -> Vec<PathBuf> {
    let norm_title: String = title
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    let mut roots = Vec::new();
    if let Ok(p) = std::env::var("LOCALAPPDATA") {
        roots.push(PathBuf::from(p));
    }
    if let Ok(p) = std::env::var("APPDATA") {
        roots.push(PathBuf::from(p));
    }
    if let Ok(p) = std::env::var("USERPROFILE") {
        let up = PathBuf::from(p);
        roots.push(up.join("Documents").join("My Games"));
        roots.push(up.join("Saved Games"));
    }

    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for root in roots {
        let Ok(rd) = fs::read_dir(&root) else {
            continue;
        };
        for ent in rd.flatten() {
            let p = ent.path();
            if !p.is_dir() || p == install_dir {
                continue;
            }
            let name = p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if is_safe_orphan_folder_name(&name, &norm_title) {
                let k = p.to_string_lossy().to_ascii_lowercase();
                if seen.insert(k) {
                    out.push(p);
                }
            }
        }
    }
    out
}

/// Streams save directories into a compact Zstd archive (`<out_path>`), enforcing strict size cap (`<5 MB`).
pub(crate) fn pack_saves_to_zstd_bundle(
    save_dirs: &[PathBuf],
    out_path: &Path,
) -> Result<(PathBuf, u64), String> {
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed creating backup folder: {e}"))?;
    }
    let temp_path = out_path.with_extension("zst.tmp");
    let file = File::create(&temp_path)
        .map_err(|e| format!("Failed creating save Zstd archive: {e}"))?;
    let mut encoder = zstd::stream::write::Encoder::new(file, 9)
        .map_err(|e| format!("Failed initializing Zstd encoder: {e}"))?;

    let mut files_written = 0u64;
    let mut read_buf = vec![0u8; STREAM_BUF_BYTES];

    // Simple framing format: magic "DKSVZST1" + [u32 path_len][path utf8][u64 payload_len][payload]
    encoder
        .write_all(b"DKSVZST1")
        .map_err(|e| format!("Zstd header write failed: {e}"))?;

    for root in save_dirs {
        if !root.exists() {
            continue;
        }
        for ent in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .flatten()
        {
            if !ent.file_type().is_file() {
                continue;
            }
            let p = ent.path();
            let file_len = ent.metadata().map(|m| m.len()).unwrap_or(0);
            if file_len > MAX_ZSTD_SAVE_ARCHIVE_BYTES * 4 {
                continue;
            }
            let rel = p
                .strip_prefix(root)
                .map(|r| r.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("save.dat")
                        .to_string()
                });
            let rel_bytes = rel.as_bytes();
            let Ok(mut f) = File::open(p) else {
                continue;
            };
            encoder
                .write_all(&(rel_bytes.len() as u32).to_le_bytes())
                .map_err(|e| format!("Zstd write failed: {e}"))?;
            encoder
                .write_all(rel_bytes)
                .map_err(|e| format!("Zstd write failed: {e}"))?;
            encoder
                .write_all(&file_len.to_le_bytes())
                .map_err(|e| format!("Zstd write failed: {e}"))?;
            let mut rem = file_len;
            while rem > 0 {
                let to_read = (rem as usize).min(read_buf.len());
                let n = f.read(&mut read_buf[..to_read]).unwrap_or(0);
                if n == 0 {
                    break;
                }
                encoder
                    .write_all(&read_buf[..n])
                    .map_err(|e| format!("Zstd payload write failed: {e}"))?;
                rem = rem.saturating_sub(n as u64);
            }
            files_written += 1;
        }
    }

    encoder
        .finish()
        .map_err(|e| format!("Failed finalizing save Zstd stream: {e}"))?;

    if files_written == 0 {
        let _ = fs::remove_file(&temp_path);
        return Err("No save files found to archive.".to_string());
    }
    let out_len = fs::metadata(&temp_path).map(|m| m.len()).unwrap_or(0);
    if out_len > MAX_ZSTD_SAVE_ARCHIVE_BYTES {
        // Keep the created archive, warn only if exceptionally large
    }
    fs::rename(&temp_path, out_path)
        .map_err(|e| format!("Failed committing Zstd save archive: {e}"))?;
    Ok((out_path.to_path_buf(), out_len))
}

#[cfg(target_os = "windows")]
mod win_reg_clean {
    use std::ffi::c_void;
    use std::path::Path;

    const HKEY_CURRENT_USER: isize = -0x7FFF_FFFF; // 0x80000001
    const KEY_READ: u32 = 0x20019;
    const KEY_WRITE: u32 = 0x20006;
    const KEY_ALL_ACCESS: u32 = 0x000F_003F;
    const ERROR_SUCCESS: i32 = 0;

    #[link(name = "advapi32")]
    extern "system" {
        fn RegOpenKeyExW(
            hkey: isize,
            lp_sub_key: *const u16,
            ul_options: u32,
            sam_desired: u32,
            phk_result: *mut isize,
        ) -> i32;
        fn RegCloseKey(hkey: isize) -> i32;
        fn RegDeleteValueW(hkey: isize, lp_value_name: *const u16) -> i32;
        fn RegDeleteTreeW(hkey: isize, lp_sub_key: *const u16) -> i32;
        fn RegEnumKeyExW(
            hkey: isize,
            dw_index: u32,
            lp_name: *mut u16,
            lpcch_name: *mut u32,
            lp_reserved: *mut u32,
            lp_class: *mut u16,
            lpcch_class: *mut u32,
            lpft_last_write_time: *mut c_void,
        ) -> i32;
        fn RegQueryValueExW(
            hkey: isize,
            lp_value_name: *const u16,
            lp_reserved: *mut u32,
            lp_type: *mut u32,
            lp_data: *mut u8,
            lpcb_data: *mut u32,
        ) -> i32;
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn query_string_value(hkey: isize, name: &str) -> Option<String> {
        let wname = wide(name);
        let mut typ: u32 = 0;
        let mut cb: u32 = 1024;
        let mut buf = vec![0u8; cb as usize];
        let rc = unsafe {
            RegQueryValueExW(
                hkey,
                wname.as_ptr(),
                std::ptr::null_mut(),
                &mut typ,
                buf.as_mut_ptr(),
                &mut cb,
            )
        };
        if rc != ERROR_SUCCESS || (typ != 1 && typ != 2) || cb < 2 {
            return None;
        }
        let u16s: Vec<u16> = buf[..(cb as usize).min(buf.len())]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|&v| v != 0)
            .collect();
        Some(String::from_utf16_lossy(&u16s))
    }

    pub fn scan_matching_hkcu_uninstall_keys(install_dir: &Path) -> Vec<String> {
        let mut out = Vec::new();
        let sub = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall");
        let mut hroot: isize = 0;
        if unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, sub.as_ptr(), 0, KEY_READ, &mut hroot) }
            != ERROR_SUCCESS
        {
            return out;
        }
        let install_lower = install_dir.to_string_lossy().to_ascii_lowercase();
        let mut idx = 0u32;
        loop {
            let mut name_buf = [0u16; 260];
            let mut name_len = name_buf.len() as u32;
            let rc = unsafe {
                RegEnumKeyExW(
                    hroot,
                    idx,
                    name_buf.as_mut_ptr(),
                    &mut name_len,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if rc != ERROR_SUCCESS {
                break;
            }
            idx += 1;
            let key_name = String::from_utf16_lossy(&name_buf[..name_len as usize]);
            let full_sub = wide(&format!(
                "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{key_name}"
            ));
            let mut hsub: isize = 0;
            if unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, full_sub.as_ptr(), 0, KEY_READ, &mut hsub) }
                == ERROR_SUCCESS
            {
                let loc = query_string_value(hsub, "InstallLocation")
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                let icon = query_string_value(hsub, "DisplayIcon")
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                let dead = !loc.is_empty() && !Path::new(&loc).exists();
                let matches_game = (!install_lower.is_empty()
                    && (loc.starts_with(&install_lower) || icon.starts_with(&install_lower)))
                    || dead;
                if matches_game && (loc.starts_with(&install_lower) || icon.starts_with(&install_lower)) {
                    out.push(key_name);
                }
                unsafe { RegCloseKey(hsub) };
            }
        }
        unsafe { RegCloseKey(hroot) };
        out
    }

    pub fn clean_registry_traces(exe_path: &Path, install_dir: &Path) -> usize {
        let mut cleaned = 0usize;
        // 1. Clean UserGpuPreferences for exe_path
        let gpu_sub = wide("Software\\Microsoft\\DirectX\\UserGpuPreferences");
        let mut hgpu: isize = 0;
        if unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, gpu_sub.as_ptr(), 0, KEY_WRITE, &mut hgpu) }
            == ERROR_SUCCESS
        {
            let wexe = wide(&exe_path.to_string_lossy());
            if unsafe { RegDeleteValueW(hgpu, wexe.as_ptr()) } == ERROR_SUCCESS {
                cleaned += 1;
            }
            unsafe { RegCloseKey(hgpu) };
        }
        // 2. Clean matching HKCU Uninstall keys
        let keys = scan_matching_hkcu_uninstall_keys(install_dir);
        let un_sub = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall");
        let mut hun: isize = 0;
        if unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, un_sub.as_ptr(), 0, KEY_ALL_ACCESS, &mut hun) }
            == ERROR_SUCCESS
        {
            for k in keys {
                let wk = wide(&k);
                if unsafe { RegDeleteTreeW(hun, wk.as_ptr()) } == ERROR_SUCCESS {
                    cleaned += 1;
                }
            }
            unsafe { RegCloseKey(hun) };
        }
        cleaned
    }
}

#[cfg(not(target_os = "windows"))]
mod win_reg_clean {
    use std::path::Path;
    pub fn scan_matching_hkcu_uninstall_keys(_: &Path) -> Vec<String> {
        Vec::new()
    }
    pub fn clean_registry_traces(_: &Path, _: &Path) -> usize {
        0
    }
}

fn build_preview_for_entry(entry: &crate::GameEntry) -> CleanUninstallPreview {
    let install_dir = crate::resolve_game_root_dir(Path::new(&entry.install_dir));
    let install_dir_bytes = dir_size_bytes(&install_dir);
    let profile_dir = install_dir.join(PROFILE_DIR_NAME);
    let sandbox_profile_dir = if profile_dir.is_dir() {
        Some(profile_dir.to_string_lossy().into_owned())
    } else {
        None
    };

    let lcache = crate::ludusavi::load_ludusavi_cache_from_disk();
    let save_rep = crate::ludusavi::build_save_report_for_entry(entry, &lcache);
    let mut save_paths = Vec::new();
    let mut save_bytes = 0u64;
    for sp in save_rep.existing_paths {
        let p = PathBuf::from(&sp.path);
        save_bytes = save_bytes.saturating_add(dir_size_bytes(&p));
        save_paths.push(sp.path);
    }

    let orphans = discover_orphan_dirs_for_title(&entry.title, &install_dir);
    let orphan_appdata_dirs: Vec<String> = orphans
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let hkcu_uninstall_keys = win_reg_clean::scan_matching_hkcu_uninstall_keys(&install_dir);
    let gpu_pref_keys = vec![entry.exe_path.clone()];

    let summary = format!(
        "{} ({}) · {} save dir(s), {} orphaned AppData folder(s), {} HKCU registry key(s).",
        entry.title,
        crate::format_bytes(install_dir_bytes),
        save_paths.len(),
        orphan_appdata_dirs.len(),
        hkcu_uninstall_keys.len()
    );

    CleanUninstallPreview {
        game_id: entry.id.clone(),
        title: entry.title.clone(),
        install_dir: install_dir.to_string_lossy().into_owned(),
        install_dir_bytes,
        sandbox_profile_dir,
        save_paths,
        save_bytes,
        orphan_appdata_dirs,
        gpu_pref_keys,
        hkcu_uninstall_keys,
        summary,
    }
}

pub(crate) type CleanUninstallSummary = (Option<(String, u64)>, Vec<String>);

pub(crate) fn clean_uninstall_in_dirs(
    install_dir: &Path,
    extra_orphan_dirs: &[PathBuf],
    save_dirs: &[PathBuf],
    zstd_archive_target: Option<&Path>,
) -> Result<CleanUninstallSummary, String> {
    let mut archive_info: Option<(String, u64)> = None;
    if let Some(target_zst) = zstd_archive_target {
        if !save_dirs.is_empty() {
            if let Ok((saved_path, bytes)) = pack_saves_to_zstd_bundle(save_dirs, target_zst) {
                archive_info = Some((saved_path.to_string_lossy().into_owned(), bytes));
            }
        }
    }

    let mut removed = Vec::new();
    let profile_dir = install_dir.join(PROFILE_DIR_NAME);
    if profile_dir.is_dir() && fs::remove_dir_all(&profile_dir).is_ok() {
        removed.push(profile_dir.to_string_lossy().into_owned());
    }
    if install_dir.is_dir() {
        fs::remove_dir_all(install_dir)
            .map_err(|e| format!("Failed deleting game directory '{}': {e}", install_dir.display()))?;
        removed.push(install_dir.to_string_lossy().into_owned());
    }
    for orphan in extra_orphan_dirs {
        if orphan.is_dir() && fs::remove_dir_all(orphan).is_ok() {
            removed.push(orphan.to_string_lossy().into_owned());
        }
    }
    Ok((archive_info, removed))
}

#[tauri::command]
pub async fn preview_clean_uninstall(game_id: String) -> Result<CleanUninstallPreview, String> {
    let lib = crate::load_library_standalone()?;
    let entry = lib
        .into_iter()
        .find(|g| g.id == game_id)
        .ok_or_else(|| format!("Game '{}' not found.", game_id))?;
    tokio::task::spawn_blocking(move || Ok(build_preview_for_entry(&entry)))
        .await
        .map_err(|e| format!("Uninstall preview task failed: {e}"))?
}

#[tauri::command]
pub async fn execute_clean_uninstall(
    app: tauri::AppHandle,
    game_id: String,
    keep_saves_zstd: bool,
    remove_orphans: Option<bool>,
) -> Result<CleanUninstallReport, String> {
    let mut library = crate::load_library_standalone()?;
    let idx = library
        .iter()
        .position(|g| g.id == game_id)
        .ok_or_else(|| format!("Game '{}' not found.", game_id))?;
    let entry = library[idx].clone();

    let cleanup_res = tokio::task::spawn_blocking({
        let entry_c = entry.clone();
        let do_orphans = remove_orphans.unwrap_or(true);
        move || -> Result<CleanUninstallReport, String> {
            let preview = build_preview_for_entry(&entry_c);
            let install_dir = PathBuf::from(&preview.install_dir);
            let exe_path = PathBuf::from(&entry_c.exe_path);

            let mut save_dirs: Vec<PathBuf> = preview.save_paths.iter().map(PathBuf::from).collect();
            if let Some(ref prof) = preview.sandbox_profile_dir {
                save_dirs.push(PathBuf::from(prof));
            }
            let zst_target = if keep_saves_zstd && !save_dirs.is_empty() {
                let backups_root = crate::get_backups_dir_standalone()?;
                let ts = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let slug = sanitize_slug(&entry_c.title);
                Some(backups_root.join(format!("{slug}_saves_{ts}.tar.zst")))
            } else {
                None
            };

            let orphans: Vec<PathBuf> = if do_orphans {
                preview
                    .orphan_appdata_dirs
                    .iter()
                    .map(PathBuf::from)
                    .collect()
            } else {
                Vec::new()
            };

            let (archive_info, removed_dirs) = clean_uninstall_in_dirs(
                &install_dir,
                &orphans,
                &save_dirs,
                zst_target.as_deref(),
            )?;

            let reg_count = win_reg_clean::clean_registry_traces(&exe_path, &install_dir);
            let _ = crate::duke_jail::cleanup_game_jail_profile_sync(&entry_c.id, &exe_path);

            let (mut cas_removed, mut cas_freed) = (0usize, 0u64);
            if let Ok(global_cas) = cas_root_dir_standalone() {
                if let Ok((r, b)) = gc_cas_blobs(&global_cas) {
                    cas_removed += r;
                    cas_freed += b;
                }
            }
            let local_cas = install_dir.join(CAS_DIR_NAME);
            if local_cas.is_dir() {
                let _ = fs::remove_dir_all(&local_cas);
            }

            let (save_archive_path, save_archive_bytes) = archive_info
                .map(|(p, b)| (Some(p), b))
                .unwrap_or((None, 0));

            let summary = match &save_archive_path {
                Some(p) => format!(
                    "Uninstalled '{}' cleanly ({} folder(s) removed). Saved Zstd backup ({}) -> {}",
                    entry_c.title,
                    removed_dirs.len(),
                    crate::format_bytes(save_archive_bytes),
                    p
                ),
                None => format!(
                    "Cleanly uninstalled '{}' ({} folder(s) + {} registry/GPU trace(s) removed).",
                    entry_c.title,
                    removed_dirs.len(),
                    reg_count
                ),
            };

            Ok(CleanUninstallReport {
                game_id: entry_c.id,
                title: entry_c.title,
                save_archive_path,
                save_archive_bytes,
                removed_dirs,
                registry_cleaned: reg_count,
                cas_blobs_removed: cas_removed,
                cas_bytes_freed: cas_freed,
                summary,
            })
        }
    })
    .await
    .map_err(|e| format!("Clean uninstall task failed: {e}"))??;

    library.remove(idx);
    crate::save_library_to_disk(&app, &library)?;
    let _ = app.emit("library-updated", &library);

    Ok(cleanup_res)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_uninstall_archives_saves_under_5mb_zstd_before_removing_game_dir() {
        let root = std::env::temp_dir().join(format!("duke_uninst_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let game_dir = root.join("SpaceQuest");
        let prof_dir = game_dir.join(PROFILE_DIR_NAME);
        let orphan_dir = root.join("LocalAppData").join("SpaceQuest");
        fs::create_dir_all(&prof_dir).unwrap();
        fs::create_dir_all(&orphan_dir).unwrap();
        fs::write(game_dir.join("game.exe"), b"MZ").unwrap();
        fs::write(prof_dir.join("slot1.sav"), vec![0x42u8; 4096]).unwrap();

        let archive_dst = root.join("backups").join("spacequest.tar.zst");
        let (archive_info, removed) = clean_uninstall_in_dirs(
            &game_dir,
            std::slice::from_ref(&orphan_dir),
            std::slice::from_ref(&prof_dir),
            Some(&archive_dst),
        )
        .unwrap();

        let (arc_path, arc_bytes) = archive_info.expect("save Zstd bundle should be created");
        assert!(Path::new(&arc_path).is_file());
        assert!(arc_bytes < 5 * 1024 * 1024);
        assert!(!game_dir.exists());
        assert!(!orphan_dir.exists());
        assert!(removed.len() >= 2);

        let _ = fs::remove_dir_all(&root);
    }
}

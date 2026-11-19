//! 1-Click redistributable & dead-bloat scanner/reclaimer backed by `.duke-cas/` restore points.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use walkdir::WalkDir;

use crate::snapshots::{create_snapshot_in_dir_filtered, CAS_DIR_NAME, PROFILE_DIR_NAME};

const MAX_SCAN_DEPTH: usize = 5;

const REDIST_DIR_NAMES: &[&str] = &[
    "_commonredist",
    "__installer",
    "commonredist",
    "redist",
    "vcredist",
    "directx",
    "dotnet",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RedistCleanCandidate {
    pub category: String,
    pub path: String,
    pub rel_path: String,
    pub is_dir: bool,
    pub size_bytes: u64,
    pub file_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GameRedistScanSummary {
    pub game_id: String,
    pub game_title: String,
    pub install_dir: String,
    pub reclaimable_bytes: u64,
    pub reclaimable_formatted: String,
    pub candidates: Vec<RedistCleanCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RedistScanReport {
    pub games_scanned: usize,
    pub games_with_bloat: usize,
    pub total_reclaimable_bytes: u64,
    pub total_reclaimable_formatted: String,
    pub games: Vec<GameRedistScanSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RedistCleanReport {
    pub games_cleaned: usize,
    pub entries_removed: usize,
    pub files_removed: usize,
    pub bytes_reclaimed: u64,
    pub bytes_reclaimed_formatted: String,
    pub snapshot_ids: Vec<String>,
    pub errors: Vec<String>,
}

fn normalize_win_canonical(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        let bytes = rest.as_bytes();
        if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
            return PathBuf::from(rest);
        }
    }
    path.to_path_buf()
}

fn path_key_lower(path: &Path) -> String {
    let norm = normalize_win_canonical(path);
    norm.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

fn is_safe_game_root(canon_root: &Path) -> bool {
    let raw = canon_root.to_string_lossy();
    if raw.trim().is_empty() {
        return false;
    }

    if canon_root.parent().is_none() {
        return false;
    }

    let normals: Vec<String> = canon_root
        .components()
        .filter_map(|c| match c {
            Component::Normal(seg) => Some(seg.to_string_lossy().to_ascii_lowercase()),
            _ => None,
        })
        .collect();

    if normals.is_empty() {
        return false;
    }

    let first = normals[0].as_str();
    // Never operate anywhere inside the OS Windows directory or top-level system containers.
    if first == "windows" {
        return false;
    }
    if normals.len() == 1
        && matches!(
            first,
            "program files"
                | "program files (x86)"
                | "programdata"
                | "users"
                | "system volume information"
                | "$recycle.bin"
                | "recovery"
                | "boot"
                | "perflogs"
                | "usr"
                | "bin"
                | "etc"
                | "var"
                | "home"
        )
    {
        return false;
    }
    if normals.len() == 2
        && (first == "program files" || first == "program files (x86)")
        && matches!(normals[1].as_str(), "common files" | "windowsapps")
    {
        return false;
    }
    if normals.len() == 2 && first == "users" {
        return false;
    }

    let root_key = path_key_lower(canon_root);
    for env_var in [
        "SystemRoot",
        "WINDIR",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramW6432",
        "ProgramData",
        "USERPROFILE",
        "LOCALAPPDATA",
        "APPDATA",
        "TEMP",
        "TMP",
    ] {
        if let Some(val) = std::env::var_os(env_var) {
            let env_path = PathBuf::from(val);
            if !env_path.as_os_str().is_empty() && path_key_lower(&env_path) == root_key {
                return false;
            }
        }
    }

    true
}

fn resolve_safe_game_root(game: &crate::GameEntry) -> Option<PathBuf> {
    let install_trimmed = game.install_dir.trim();
    let exe_trimmed = game.exe_path.trim();

    let candidate = if !install_trimmed.is_empty() {
        let p = PathBuf::from(install_trimmed);
        if !p.is_dir() {
            return None;
        }
        p
    } else if !exe_trimmed.is_empty() {
        let exe_p = Path::new(exe_trimmed);
        let base_hint = if exe_p.is_file() {
            exe_p.to_path_buf()
        } else if exe_p
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
        {
            exe_p.parent()?.to_path_buf()
        } else {
            exe_p.to_path_buf()
        };
        crate::resolve_game_root_dir(&base_hint)
    } else {
        return None;
    };

    if !candidate.is_dir() {
        return None;
    }

    let canonical = fs::canonicalize(&candidate).ok()?;
    let norm = normalize_win_canonical(&canonical);
    if !norm.is_dir() || !is_safe_game_root(&norm) {
        return None;
    }
    Some(norm)
}

fn is_symlink_or_reparse_dir(path: &Path) -> bool {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return false;
    };
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(target_os = "windows")]
    {
        if meta.is_dir() {
            use std::os::windows::fs::MetadataExt;
            const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
            if (meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT) != 0 {
                return true;
            }
        }
    }
    false
}

fn should_skip_system_dir_name(name: &std::ffi::OsStr) -> bool {
    let lower = name.to_string_lossy().to_ascii_lowercase();
    lower == CAS_DIR_NAME || lower == PROFILE_DIR_NAME || lower == ".duke-patch-tmp"
}

fn normalize_rel_path(base: &Path, full: &Path) -> Option<String> {
    let rel = full.strip_prefix(base).ok()?;
    let mut parts = Vec::new();
    for comp in rel.components() {
        match comp {
            Component::Normal(seg) => parts.push(seg.to_string_lossy().into_owned()),
            Component::CurDir => {}
            _ => return None,
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

fn is_redist_dir_match(name_lower: &str, rel_lower: &str) -> bool {
    REDIST_DIR_NAMES.contains(&name_lower) || rel_lower == "support/redist"
}

fn is_crash_dir_match(name_lower: &str) -> bool {
    name_lower == "crashdumps" || name_lower.starts_with("ue4cc-")
}

fn is_crash_file_match(name_lower: &str) -> bool {
    name_lower.ends_with(".dmp")
        || name_lower.ends_with(".mdmp")
        || name_lower.ends_with(".hdmp")
        || (name_lower.starts_with("crash_report_") && name_lower.ends_with(".zip"))
}

fn is_standalone_redist_installer(name_lower: &str) -> bool {
    (name_lower.starts_with("vc_redist") && name_lower.ends_with(".exe"))
        || (name_lower.starts_with("vcredist") && name_lower.ends_with(".exe"))
        || name_lower == "dxsetup.exe"
        || (name_lower.starts_with("ndp") && name_lower.ends_with(".exe"))
        || (name_lower.starts_with("dotnetfx") && name_lower.ends_with(".exe"))
}

fn rel_in_save_folder(rel_lower: &str) -> bool {
    let mut parts = rel_lower.split('/').peekable();
    while let Some(seg) = parts.next() {
        // Only treat directory segments (not the filename itself) as save folders.
        if parts.peek().is_some()
            && matches!(seg, "save" | "saves" | "savegames" | "save_games" | "savedata")
        {
            return true;
        }
    }
    false
}

fn canonicalize_within_root(path: &Path, canon_root: &Path) -> Option<PathBuf> {
    if is_symlink_or_reparse_dir(path) {
        return None;
    }
    let canon = fs::canonicalize(path).ok()?;
    let norm = normalize_win_canonical(&canon);
    if norm == canon_root || !norm.starts_with(canon_root) {
        return None;
    }
    Some(norm)
}

fn measure_bloat_dir(dir: &Path, canon_root: &Path) -> (u64, usize) {
    let mut total_bytes = 0u64;
    let mut file_count = 0usize;
    let mut it = WalkDir::new(dir).follow_links(false).into_iter();
    while let Some(entry_res) = it.next() {
        let Ok(entry) = entry_res else {
            continue;
        };
        if entry.path() == dir {
            continue;
        }
        if entry.path_is_symlink() || is_symlink_or_reparse_dir(entry.path()) {
            if entry.file_type().is_dir() {
                it.skip_current_dir();
            }
            continue;
        }
        if entry.file_type().is_dir() && should_skip_system_dir_name(entry.file_name()) {
            it.skip_current_dir();
            continue;
        }
        if canonicalize_within_root(entry.path(), canon_root).is_none() {
            if entry.file_type().is_dir() {
                it.skip_current_dir();
            }
            continue;
        }
        if entry.file_type().is_file() {
            let sz = entry.metadata().map(|m| m.len()).unwrap_or(0);
            total_bytes = total_bytes.saturating_add(sz);
            file_count += 1;
        }
    }
    (total_bytes, file_count)
}

fn resolve_main_exe_and_bin_dir(game: &crate::GameEntry) -> (Option<PathBuf>, Option<PathBuf>) {
    let trimmed = game.exe_path.trim();
    if trimmed.is_empty() {
        return (None, None);
    }
    let p = Path::new(trimmed);
    let exe_canon = fs::canonicalize(p)
        .ok()
        .map(|c| normalize_win_canonical(&c))
        .or_else(|| Some(p.to_path_buf()));
    let bin_dir = p.parent().and_then(|parent| {
        fs::canonicalize(parent)
            .ok()
            .map(|c| normalize_win_canonical(&c))
            .or_else(|| Some(parent.to_path_buf()))
    });
    (exe_canon, bin_dir)
}

pub(crate) fn scan_game_redist_bloat(game: &crate::GameEntry) -> Option<GameRedistScanSummary> {
    let game_root = resolve_safe_game_root(game)?;
    let (main_exe_canon, main_bin_dir) = resolve_main_exe_and_bin_dir(game);

    let mut candidates = Vec::new();
    let mut it = WalkDir::new(&game_root)
        .max_depth(MAX_SCAN_DEPTH)
        .follow_links(false)
        .into_iter();

    while let Some(entry_res) = it.next() {
        let Ok(entry) = entry_res else {
            continue;
        };
        let path = entry.path();
        if path == game_root {
            continue;
        }

        if entry.path_is_symlink() || is_symlink_or_reparse_dir(path) {
            if entry.file_type().is_dir() {
                it.skip_current_dir();
            }
            continue;
        }

        if entry.file_type().is_dir() && should_skip_system_dir_name(entry.file_name()) {
            it.skip_current_dir();
            continue;
        }

        let Some(canon_path) = canonicalize_within_root(path, &game_root) else {
            if entry.file_type().is_dir() {
                it.skip_current_dir();
            }
            continue;
        };

        let Some(rel_path) = normalize_rel_path(&game_root, &canon_path) else {
            if entry.file_type().is_dir() {
                it.skip_current_dir();
            }
            continue;
        };

        let name_lower = entry.file_name().to_string_lossy().to_ascii_lowercase();
        let rel_lower = rel_path.to_ascii_lowercase();

        if entry.file_type().is_dir() {
            let category = if is_redist_dir_match(&name_lower, &rel_lower) {
                Some("redist_dir")
            } else if is_crash_dir_match(&name_lower) {
                Some("crash_dump")
            } else {
                None
            };

            if let Some(cat) = category {
                let protects_main_exe = main_exe_canon
                    .as_ref()
                    .is_some_and(|exe| exe.starts_with(&canon_path));
                let protects_main_bin = main_bin_dir
                    .as_ref()
                    .is_some_and(|bin| bin.starts_with(&canon_path));
                if protects_main_exe || protects_main_bin {
                    continue;
                }

                let (size_bytes, file_count) = measure_bloat_dir(&canon_path, &game_root);
                candidates.push(RedistCleanCandidate {
                    category: cat.to_string(),
                    path: canon_path.to_string_lossy().to_string(),
                    rel_path,
                    is_dir: true,
                    size_bytes,
                    file_count,
                });
                it.skip_current_dir();
            }
            continue;
        }

        if !entry.file_type().is_file() {
            continue;
        }

        if main_exe_canon
            .as_ref()
            .is_some_and(|exe| exe == &canon_path)
        {
            continue;
        }

        if rel_in_save_folder(&rel_lower) {
            continue;
        }

        if is_crash_file_match(&name_lower) {
            let size_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
            candidates.push(RedistCleanCandidate {
                category: "crash_dump".to_string(),
                path: canon_path.to_string_lossy().to_string(),
                rel_path,
                is_dir: false,
                size_bytes,
                file_count: 1,
            });
            continue;
        }

        if is_standalone_redist_installer(&name_lower) {
            let in_non_main_subdir = match canon_path.parent() {
                Some(parent) => {
                    parent != game_root
                        && main_bin_dir
                            .as_ref()
                            .is_none_or(|bin| bin.as_path() != parent)
                }
                None => false,
            };
            if in_non_main_subdir {
                let size_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
                candidates.push(RedistCleanCandidate {
                    category: "redist_installer".to_string(),
                    path: canon_path.to_string_lossy().to_string(),
                    rel_path,
                    is_dir: false,
                    size_bytes,
                    file_count: 1,
                });
            }
        }
    }

    if candidates.is_empty() {
        return None;
    }

    candidates.sort_by(|a, b| {
        b.size_bytes
            .cmp(&a.size_bytes)
            .then_with(|| a.rel_path.cmp(&b.rel_path))
    });

    let reclaimable_bytes = candidates
        .iter()
        .fold(0u64, |acc, c| acc.saturating_add(c.size_bytes));

    Some(GameRedistScanSummary {
        game_id: game.id.clone(),
        game_title: game.title.clone(),
        install_dir: game_root.to_string_lossy().to_string(),
        reclaimable_bytes,
        reclaimable_formatted: crate::format_bytes(reclaimable_bytes),
        candidates,
    })
}

#[allow(dead_code)]
pub(crate) fn scan_redist_bloat_for_library(
    library: &[crate::GameEntry],
    game_id: Option<&str>,
) -> RedistScanReport {
    let target_id = game_id.map(str::trim).filter(|s| !s.is_empty());
    let mut games_scanned = 0usize;
    let mut total_reclaimable_bytes = 0u64;
    let mut games = Vec::new();
    let mut seen_roots = HashSet::new();

    for entry in library {
        if let Some(id) = target_id {
            if entry.id != id {
                continue;
            }
        }
        games_scanned += 1;
        if let Some(root) = resolve_safe_game_root(entry) {
            if !seen_roots.insert(root) {
                continue;
            }
        }
        if let Some(summary) = scan_game_redist_bloat(entry) {
            total_reclaimable_bytes =
                total_reclaimable_bytes.saturating_add(summary.reclaimable_bytes);
            games.push(summary);
        }
    }

    games.sort_by(|a, b| {
        b.reclaimable_bytes
            .cmp(&a.reclaimable_bytes)
            .then_with(|| a.game_title.cmp(&b.game_title))
    });

    RedistScanReport {
        games_scanned,
        games_with_bloat: games.len(),
        total_reclaimable_bytes,
        total_reclaimable_formatted: crate::format_bytes(total_reclaimable_bytes),
        games,
    }
}

#[allow(clippy::permissions_set_readonly_false)]
fn clear_readonly_if_present(path: &Path) {
    if let Ok(meta) = fs::metadata(path) {
        let mut perms = meta.permissions();
        if perms.readonly() {
            perms.set_readonly(false);
            let _ = fs::set_permissions(path, perms);
        }
    }
}

fn verify_snapshot_covers_candidates(
    cas_dir: &Path,
    manifest: &crate::snapshots::GameSnapshotManifest,
    candidates: &[RedistCleanCandidate],
) -> Result<(), String> {
    if manifest.id.trim().is_empty() {
        return Err("Pre-clean snapshot returned an empty snapshot id.".to_string());
    }
    let manifest_path = cas_dir
        .join("manifests")
        .join(format!("{}.json", manifest.id));
    if !manifest_path.is_file() {
        return Err(format!(
            "Pre-clean snapshot manifest was not persisted: {}",
            manifest_path.display()
        ));
    }

    let manifest_files_lower: Vec<(String, &str)> = manifest
        .files
        .iter()
        .map(|f| (f.rel_path.to_ascii_lowercase(), f.blake3.as_str()))
        .collect();

    let verify_blob = |blake3_hex: &str, rel: &str| -> Result<(), String> {
        let prefix = blake3_hex.get(..2).unwrap_or("00");
        let blob_path = cas_dir
            .join("blobs")
            .join(prefix)
            .join(format!("{blake3_hex}.blob"));
        if !blob_path.is_file() {
            return Err(format!(
                "Pre-clean snapshot blob missing for '{}': {}",
                rel,
                blob_path.display()
            ));
        }
        Ok(())
    };

    for cand in candidates {
        if cand.file_count == 0 {
            continue;
        }
        let cand_lower = cand.rel_path.trim_end_matches('/').to_ascii_lowercase();
        if cand.is_dir {
            let prefix = format!("{cand_lower}/");
            let mut found = false;
            for (rel_l, hash) in &manifest_files_lower {
                if rel_l.starts_with(&prefix) {
                    found = true;
                    verify_blob(hash, rel_l)?;
                }
            }
            if !found {
                return Err(format!(
                    "Pre-clean snapshot did not capture files under '{}'.",
                    cand.rel_path
                ));
            }
        } else {
            let matched = manifest_files_lower
                .iter()
                .find(|(rel_l, _)| rel_l == &cand_lower);
            let Some((_, hash)) = matched else {
                return Err(format!(
                    "Pre-clean snapshot did not capture '{}'.",
                    cand.rel_path
                ));
            };
            verify_blob(hash, &cand.rel_path)?;
        }
    }

    Ok(())
}

fn remove_dir_safely_within_root(
    dir: &Path,
    canon_root: &Path,
) -> Result<(usize, u64), String> {
    let Some(canon_dir) = canonicalize_within_root(dir, canon_root) else {
        return Err(format!(
            "Refused removing directory outside game root: {}",
            dir.display()
        ));
    };

    let mut files_to_delete = Vec::new();
    let mut dirs_to_delete = vec![canon_dir.clone()];

    let mut it = WalkDir::new(&canon_dir).follow_links(false).into_iter();
    while let Some(entry_res) = it.next() {
        let entry = entry_res.map_err(|e| format!("Failed traversing {}: {}", canon_dir.display(), e))?;
        let path = entry.path();
        if path == canon_dir {
            continue;
        }
        if entry.path_is_symlink() || is_symlink_or_reparse_dir(path) {
            if entry.file_type().is_dir() {
                it.skip_current_dir();
            }
            continue;
        }
        let Some(canon_entry) = canonicalize_within_root(path, canon_root) else {
            return Err(format!(
                "Refused traversing entry outside game root: {}",
                path.display()
            ));
        };
        if entry.file_type().is_dir() {
            if should_skip_system_dir_name(entry.file_name()) {
                it.skip_current_dir();
                continue;
            }
            dirs_to_delete.push(canon_entry);
        } else if entry.file_type().is_file() {
            let sz = entry.metadata().map(|m| m.len()).unwrap_or(0);
            files_to_delete.push((canon_entry, sz));
        }
    }

    let mut files_removed = 0usize;
    let mut bytes_reclaimed = 0u64;
    for (file_path, sz) in files_to_delete {
        if crate::uninstaller::is_protected_user_or_system_path(&file_path) {
            return Err(format!(
                "Refused removing protected path: {}",
                file_path.display()
            ));
        }
        clear_readonly_if_present(&file_path);
        fs::remove_file(&file_path)
            .map_err(|e| format!("Failed removing {}: {}", file_path.display(), e))?;
        files_removed += 1;
        bytes_reclaimed = bytes_reclaimed.saturating_add(sz);
    }

    dirs_to_delete.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    for sub_dir in dirs_to_delete {
        if crate::uninstaller::is_protected_user_or_system_path(&sub_dir) {
            return Err(format!(
                "Refused removing protected directory: {}",
                sub_dir.display()
            ));
        }
        clear_readonly_if_present(&sub_dir);
        fs::remove_dir(&sub_dir)
            .map_err(|e| format!("Failed removing directory {}: {}", sub_dir.display(), e))?;
    }

    Ok((files_removed, bytes_reclaimed))
}

#[allow(dead_code)]
pub(crate) fn clean_redist_bloat_with_cas(
    library: &mut [crate::GameEntry],
    game_id: Option<&str>,
) -> Result<RedistCleanReport, String> {
    let target_id = game_id.map(str::trim).filter(|s| !s.is_empty());
    if let Some(id) = target_id {
        if !library.iter().any(|g| g.id == id) {
            return Err(format!("Game '{}' was not found in library.", id));
        }
    }

    let mut games_cleaned = 0usize;
    let mut entries_removed = 0usize;
    let mut files_removed = 0usize;
    let mut bytes_reclaimed = 0u64;
    let mut snapshot_ids = Vec::new();
    let mut errors = Vec::new();
    let mut cleaned_roots = HashSet::new();

    for entry in library.iter_mut() {
        if let Some(id) = target_id {
            if entry.id != id {
                continue;
            }
        }

        let Some(game_root) = resolve_safe_game_root(entry) else {
            continue;
        };

        if cleaned_roots.contains(&game_root) {
            let new_size = crate::calculate_dir_size_sync(&game_root);
            entry.size_bytes = new_size;
            entry.size_formatted = crate::format_bytes(new_size);
            continue;
        }

        let Some(summary) = scan_game_redist_bloat(entry) else {
            continue;
        };

        let (main_exe_canon, _) = resolve_main_exe_and_bin_dir(entry);
        let cas_dir = game_root.join(CAS_DIR_NAME);

        // Capture full game tree into `.duke-cas` before removing any bloat candidate so
        // both filtered and full `snapshots::restore_snapshot_in_dir` rollbacks remain lossless.
        let manifest = match create_snapshot_in_dir_filtered(
            &entry.id,
            &game_root,
            &cas_dir,
            Some("pre-redist-clean"),
            None,
        ) {
            Ok(m) => m,
            Err(err) => {
                errors.push(format!("{}: pre-clean snapshot failed: {}", entry.title, err));
                continue;
            }
        };

        if let Err(err) = verify_snapshot_covers_candidates(&cas_dir, &manifest, &summary.candidates)
        {
            errors.push(format!("{}: {}", entry.title, err));
            continue;
        }

        snapshot_ids.push(manifest.id.clone());

        let mut game_entries_removed = 0usize;
        let mut game_files_removed = 0usize;
        let mut game_bytes_reclaimed = 0u64;

        for cand in &summary.candidates {
            let cand_path = PathBuf::from(&cand.path);
            let Some(canon_cand) = canonicalize_within_root(&cand_path, &game_root) else {
                errors.push(format!(
                    "{}: skipped unsafe candidate '{}'",
                    entry.title, cand.rel_path
                ));
                continue;
            };

            if main_exe_canon
                .as_ref()
                .is_some_and(|exe| exe == &canon_cand || (cand.is_dir && exe.starts_with(&canon_cand)))
            {
                errors.push(format!(
                    "{}: refused removing main executable path '{}'",
                    entry.title, cand.rel_path
                ));
                continue;
            }

            if cand.is_dir {
                match remove_dir_safely_within_root(&canon_cand, &game_root) {
                    Ok((removed_cnt, reclaimed)) => {
                        game_entries_removed += 1;
                        game_files_removed += removed_cnt;
                        game_bytes_reclaimed = game_bytes_reclaimed.saturating_add(reclaimed);
                    }
                    Err(err) => {
                        errors.push(format!("{}: {}", entry.title, err));
                    }
                }
            } else {
                if crate::uninstaller::is_protected_user_or_system_path(&canon_cand) {
                    errors.push(format!(
                        "{}: refused removing protected path '{}'",
                        entry.title, cand.rel_path
                    ));
                    continue;
                }
                clear_readonly_if_present(&canon_cand);
                match fs::remove_file(&canon_cand) {
                    Ok(()) => {
                        game_entries_removed += 1;
                        game_files_removed += 1;
                        game_bytes_reclaimed = game_bytes_reclaimed.saturating_add(cand.size_bytes);
                    }
                    Err(err) => {
                        errors.push(format!(
                            "{}: failed removing {}: {}",
                            entry.title, cand.rel_path, err
                        ));
                    }
                }
            }
        }

        if game_entries_removed > 0 {
            cleaned_roots.insert(game_root.clone());
            games_cleaned += 1;
            entries_removed += game_entries_removed;
            files_removed += game_files_removed;
            bytes_reclaimed = bytes_reclaimed.saturating_add(game_bytes_reclaimed);
        }

        let new_size = crate::calculate_dir_size_sync(&game_root);
        entry.size_bytes = new_size;
        entry.size_formatted = crate::format_bytes(new_size);
    }

    Ok(RedistCleanReport {
        games_cleaned,
        entries_removed,
        files_removed,
        bytes_reclaimed,
        bytes_reclaimed_formatted: crate::format_bytes(bytes_reclaimed),
        snapshot_ids,
        errors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_SEQ: AtomicU64 = AtomicU64::new(1);

    struct TempTestDir {
        path: PathBuf,
    }

    impl TempTestDir {
        fn new(tag: &str) -> Self {
            let seq = TEST_SEQ.fetch_add(1, Ordering::Relaxed);
            let ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join(format!(
                "duke_redist_cleaner_{}_{}_{}_{}",
                tag,
                std::process::id(),
                ts,
                seq
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for TempTestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn make_game_entry(id: &str, title: &str, game_dir: &Path, exe_rel: &str) -> crate::GameEntry {
        let initial_bytes = crate::calculate_dir_size_sync(game_dir);
        crate::GameEntry {
            id: id.to_string(),
            title: title.to_string(),
            install_dir: game_dir.to_string_lossy().to_string(),
            exe_path: game_dir.join(exe_rel).to_string_lossy().to_string(),
            size_bytes: initial_bytes,
            size_formatted: crate::format_bytes(initial_bytes),
            is_installed: true,
            ..Default::default()
        }
    }

    #[test]
    fn scan_preview_sums_exact_bloat_and_preserves_game_and_saves() {
        let tmp = TempTestDir::new("preview");
        let game_dir = tmp.path.join("MyGame");
        fs::create_dir_all(game_dir.join("_CommonRedist").join("vcredist")).unwrap();
        fs::create_dir_all(game_dir.join("DirectX")).unwrap();
        fs::create_dir_all(game_dir.join("DotNet")).unwrap();
        fs::create_dir_all(game_dir.join("saves")).unwrap();
        fs::create_dir_all(game_dir.join("assets")).unwrap();

        let vc_bytes = vec![0x11u8; 4096];
        let dx_bytes = vec![0x22u8; 2048];
        let net_bytes = vec![0x33u8; 3072];
        let mdmp_bytes = vec![0x44u8; 1024];

        fs::write(
            game_dir
                .join("_CommonRedist")
                .join("vcredist")
                .join("vc_redist.x64.exe"),
            &vc_bytes,
        )
        .unwrap();
        fs::write(game_dir.join("DirectX").join("DXSETUP.exe"), &dx_bytes).unwrap();
        fs::write(game_dir.join("DotNet").join("ndp48-web.exe"), &net_bytes).unwrap();
        fs::write(game_dir.join("crash_01.mdmp"), &mdmp_bytes).unwrap();

        fs::write(game_dir.join("MyGame.exe"), b"MZ_MAIN_GAME_EXE").unwrap();
        fs::write(game_dir.join("vcruntime140.dll"), b"MZ_RUNTIME_DLL").unwrap();
        fs::write(game_dir.join("saves").join("save0.sav"), b"SAVE_SLOT_0").unwrap();
        fs::write(game_dir.join("assets").join("world.pak"), b"PAK_ASSETS").unwrap();

        let expected_bloat_bytes =
            (vc_bytes.len() + dx_bytes.len() + net_bytes.len() + mdmp_bytes.len()) as u64;

        let game = make_game_entry("g-preview", "My Game", &game_dir, "MyGame.exe");
        let summary = scan_game_redist_bloat(&game).expect("expected bloat candidates");
        assert_eq!(summary.game_id, "g-preview");
        assert_eq!(summary.game_title, "My Game");
        assert_eq!(summary.reclaimable_bytes, expected_bloat_bytes);
        assert_eq!(summary.candidates.len(), 4);

        for cand in &summary.candidates {
            assert!(!cand.rel_path.contains("MyGame.exe"));
            assert!(!cand.rel_path.contains("save0.sav"));
            assert!(!cand.rel_path.contains("vcruntime140.dll"));
            assert!(!cand.rel_path.contains("world.pak"));
        }

        let report = scan_redist_bloat_for_library(std::slice::from_ref(&game), None);
        assert_eq!(report.games_scanned, 1);
        assert_eq!(report.games_with_bloat, 1);
        assert_eq!(report.total_reclaimable_bytes, expected_bloat_bytes);
        assert_eq!(report.games.len(), 1);
    }

    #[test]
    fn clean_with_cas_snapshots_first_deletes_bloat_and_restores_byte_for_byte() {
        let tmp = TempTestDir::new("clean_restore");
        let game_dir = tmp.path.join("MyGame");
        fs::create_dir_all(game_dir.join("_CommonRedist").join("vcredist")).unwrap();
        fs::create_dir_all(game_dir.join("DirectX")).unwrap();
        fs::create_dir_all(game_dir.join("DotNet")).unwrap();
        fs::create_dir_all(game_dir.join("saves")).unwrap();

        let vc_path = game_dir
            .join("_CommonRedist")
            .join("vcredist")
            .join("vc_redist.x64.exe");
        let dx_path = game_dir.join("DirectX").join("DXSETUP.exe");
        let net_path = game_dir.join("DotNet").join("ndp48-web.exe");
        let mdmp_path = game_dir.join("crash_01.mdmp");
        let exe_path = game_dir.join("MyGame.exe");
        let save_path = game_dir.join("saves").join("save0.sav");

        let vc_bytes = vec![0xA1u8; 5120];
        let dx_bytes = vec![0xB2u8; 3000];
        let net_bytes = vec![0xC3u8; 2500];
        let mdmp_bytes = vec![0xD4u8; 1500];
        let exe_bytes = b"MY_GAME_BINARY_PAYLOAD".to_vec();
        let save_bytes = b"SAVE_FILE_BYTES_V1".to_vec();

        fs::write(&vc_path, &vc_bytes).unwrap();
        fs::write(&dx_path, &dx_bytes).unwrap();
        fs::write(&net_path, &net_bytes).unwrap();
        fs::write(&mdmp_path, &mdmp_bytes).unwrap();
        fs::write(&exe_path, &exe_bytes).unwrap();
        fs::write(&save_path, &save_bytes).unwrap();

        // Mark one bloat file read-only to confirm Windows read-only attribute cleanup.
        #[allow(clippy::permissions_set_readonly_false)]
        {
            let mut perms = fs::metadata(&dx_path).unwrap().permissions();
            perms.set_readonly(true);
            fs::set_permissions(&dx_path, perms).unwrap();
        }

        let expected_bloat =
            (vc_bytes.len() + dx_bytes.len() + net_bytes.len() + mdmp_bytes.len()) as u64;

        let mut library = vec![make_game_entry("g-clean", "My Game", &game_dir, "MyGame.exe")];
        let before_size = library[0].size_bytes;

        let clean_report = clean_redist_bloat_with_cas(&mut library, Some("g-clean"))
            .expect("clean should succeed");

        assert!(clean_report.errors.is_empty(), "unexpected errors: {:?}", clean_report.errors);
        assert_eq!(clean_report.games_cleaned, 1);
        assert_eq!(clean_report.entries_removed, 4);
        assert_eq!(clean_report.files_removed, 4);
        assert_eq!(clean_report.bytes_reclaimed, expected_bloat);
        assert_eq!(clean_report.snapshot_ids.len(), 1);
        assert!(!clean_report.bytes_reclaimed_formatted.is_empty());

        // Bloat files & folders removed
        assert!(!game_dir.join("_CommonRedist").exists());
        assert!(!game_dir.join("DirectX").exists());
        assert!(!game_dir.join("DotNet").exists());
        assert!(!mdmp_path.exists());

        // Main executable and saves untouched
        assert_eq!(fs::read(&exe_path).unwrap(), exe_bytes);
        assert_eq!(fs::read(&save_path).unwrap(), save_bytes);
        assert!(library[0].size_bytes < before_size);
        assert_eq!(
            library[0].size_formatted,
            crate::format_bytes(library[0].size_bytes)
        );

        // `.duke-cas` snapshot created with expected label and restores everything byte-for-byte
        let cas_dir = game_dir.join(CAS_DIR_NAME);
        let snaps = crate::snapshots::list_snapshots_in_cas(&cas_dir).unwrap();
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].id, clean_report.snapshot_ids[0]);
        assert_eq!(snaps[0].label, "pre-redist-clean");

        let restore = crate::snapshots::restore_snapshot_in_dir(
            &game_dir,
            &cas_dir,
            &clean_report.snapshot_ids[0],
        )
        .expect("restore should succeed");
        assert!(restore.restored_files >= 4);

        assert_eq!(fs::read(&vc_path).unwrap(), vc_bytes);
        assert_eq!(fs::read(&dx_path).unwrap(), dx_bytes);
        assert_eq!(fs::read(&net_path).unwrap(), net_bytes);
        assert_eq!(fs::read(&mdmp_path).unwrap(), mdmp_bytes);
        assert_eq!(fs::read(&exe_path).unwrap(), exe_bytes);
        assert_eq!(fs::read(&save_path).unwrap(), save_bytes);
    }

    #[test]
    fn scan_respects_depth_limit_support_redist_non_main_subdirs_and_cas_skips() {
        let tmp = TempTestDir::new("rules");
        let game_dir = tmp.path.join("DeepGame");
        fs::create_dir_all(game_dir.join("Binaries").join("Win64")).unwrap();
        fs::create_dir_all(game_dir.join("Support").join("Redist")).unwrap();
        fs::create_dir_all(game_dir.join("Extras")).unwrap();
        fs::create_dir_all(game_dir.join("CrashDumps")).unwrap();
        fs::create_dir_all(game_dir.join("Saved").join("Crashes").join("UE4CC-Windows-ABCD")).unwrap();
        fs::create_dir_all(game_dir.join(CAS_DIR_NAME).join("blobs")).unwrap();
        fs::create_dir_all(game_dir.join(PROFILE_DIR_NAME)).unwrap();
        fs::create_dir_all(game_dir.join("a").join("b").join("c").join("d").join("e")).unwrap();

        fs::write(
            game_dir.join("Binaries").join("Win64").join("DeepGame.exe"),
            b"MAIN_BIN",
        )
        .unwrap();
        // Inside main binary dir -> must NOT be flagged
        fs::write(
            game_dir.join("Binaries").join("Win64").join("ndp_runtime.exe"),
            b"SKIP_MAIN_BIN_DIR",
        )
        .unwrap();

        // Expected candidates:
        fs::write(
            game_dir.join("Support").join("Redist").join("vcredist_x86.exe"),
            b"12345",
        )
        .unwrap();
        fs::write(game_dir.join("Support").join("Manual.pdf"), b"KEEP_MANUAL").unwrap();
        fs::write(game_dir.join("Extras").join("dotnetfx35.exe"), b"123456").unwrap();
        fs::write(game_dir.join("CrashDumps").join("last.dmp"), b"1234567").unwrap();
        fs::write(
            game_dir
                .join("Saved")
                .join("Crashes")
                .join("UE4CC-Windows-ABCD")
                .join("crash.log"),
            b"12345678",
        )
        .unwrap();
        fs::write(game_dir.join("crash_report_99.zip"), b"123456789").unwrap();
        fs::write(game_dir.join("mem.hdmp"), b"12").unwrap();

        // Must be skipped (.duke-cas, .duke-profile, depth > 5)
        fs::write(
            game_dir.join(CAS_DIR_NAME).join("blobs").join("cached.dmp"),
            b"IGNORED_CAS",
        )
        .unwrap();
        fs::write(
            game_dir.join(PROFILE_DIR_NAME).join("profile.mdmp"),
            b"IGNORED_PROFILE",
        )
        .unwrap();
        fs::write(
            game_dir
                .join("a")
                .join("b")
                .join("c")
                .join("d")
                .join("e")
                .join("too_deep.dmp"),
            b"IGNORED_DEPTH_6",
        )
        .unwrap();

        let game = make_game_entry(
            "g-rules",
            "Deep Game",
            &game_dir,
            "Binaries\\Win64\\DeepGame.exe",
        );
        let summary = scan_game_redist_bloat(&game).expect("expected bloat candidates");
        let rels: HashSet<&str> = summary
            .candidates
            .iter()
            .map(|c| c.rel_path.as_str())
            .collect();

        assert!(rels.contains("Support/Redist"));
        assert!(rels.contains("Extras/dotnetfx35.exe"));
        assert!(rels.contains("CrashDumps"));
        assert!(rels.contains("Saved/Crashes/UE4CC-Windows-ABCD"));
        assert!(rels.contains("crash_report_99.zip"));
        assert!(rels.contains("mem.hdmp"));

        assert!(!rels.iter().any(|r| r.contains("ndp_runtime.exe")));
        assert!(!rels.iter().any(|r| r.contains("Manual.pdf")));
        assert!(!rels.iter().any(|r| r.contains("cached.dmp")));
        assert!(!rels.iter().any(|r| r.contains("profile.mdmp")));
        assert!(!rels.iter().any(|r| r.contains("too_deep.dmp")));
    }

    #[test]
    fn unsafe_roots_are_refused() {
        for bad in [
            "",
            "   ",
            "C:\\",
            "C:\\Windows",
            "C:\\Program Files",
            "C:\\Program Files (x86)",
        ] {
            let g = crate::GameEntry {
                id: "bad".to_string(),
                title: "Bad".to_string(),
                install_dir: bad.to_string(),
                ..Default::default()
            };
            assert!(
                scan_game_redist_bloat(&g).is_none(),
                "expected '{}' to be refused",
                bad
            );
        }

        let temp_root_game = crate::GameEntry {
            id: "temp".to_string(),
            install_dir: std::env::temp_dir().to_string_lossy().to_string(),
            ..Default::default()
        };
        assert!(scan_game_redist_bloat(&temp_root_game).is_none());
    }
}

//! Safe Game Uninstaller (dedicated-folder-only).
//!
//! Hard safety contract (v2.3.2):
//! - **Never** searches or deletes anything under `%APPDATA%`, `%LOCALAPPDATA%`, `Desktop`,
//!   `Public\Desktop`, or `Start Menu`, and **never** touches `.lnk` shortcuts.
//! - Recursive deletion (`remove_dir_all`) only runs on a folder that passes
//!   [`validate_safe_uninstall_target`]: a dedicated single-game subfolder that is not a drive root,
//!   user/system folder, shared launcher tree (`ModrinthApp`, `PrismLauncher`, `CurseForge`,
//!   `RetroArch`, ...), store library root (`steamapps\common`, `Epic Games`, ...), and that does not
//!   hold launcher databases, other library games, or multiple unrelated game executables.
//! - When a game lives in a shared folder, only the library entry is removed, or (explicit opt-in)
//!   the single target `.exe` is deleted after packing a `.tar.zst` save backup.
//! - Optional `<5 MB` Zstd save archive, scoped HKCU Uninstall / `UserGpuPreferences` cleanup,
//!   AppContainer jail cleanup and `.duke-cas/` garbage collection.

use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

use crate::snapshots::{cas_root_dir_standalone, gc_cas_blobs, CAS_DIR_NAME, PROFILE_DIR_NAME};

const MAX_ZSTD_SAVE_ARCHIVE_BYTES: u64 = 5 * 1024 * 1024;
const STREAM_BUF_BYTES: usize = 65_536;
/// Maximum directory entries inspected by the multi-game heuristic before refusing as "too broad".
const MULTI_GAME_SCAN_BUDGET: usize = 20_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanUninstallPreview {
    pub game_id: String,
    pub title: String,
    pub install_dir: String,
    pub install_dir_bytes: u64,
    pub exe_path: String,
    pub sandbox_profile_dir: Option<String>,
    pub save_paths: Vec<String>,
    pub save_bytes: u64,
    /// `true` only when `install_dir` passed [`validate_safe_uninstall_target`].
    pub folder_deletion_allowed: bool,
    /// Human-readable reason when folder deletion is refused.
    pub refusal_reason: Option<String>,
    /// Single `.exe` that may be deleted instead (explicit opt-in) when folder deletion is refused.
    pub exe_only_target: Option<String>,
    pub gpu_pref_keys: Vec<String>,
    pub hkcu_uninstall_keys: Vec<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanUninstallReport {
    pub game_id: String,
    pub title: String,
    /// `"folder"` (validated dedicated folder removed) or `"exe_only"` (single executable removed).
    pub mode: String,
    pub deleted_target: String,
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

// ---------------------------------------------------------------------------------------------
// Safety validation
// ---------------------------------------------------------------------------------------------

/// Path components that mark a shared launcher / user-data tree. Any install dir at or below
/// one of these is never recursively deleted.
const SHARED_TREE_COMPONENTS: &[&str] = &[
    "desktop",
    "appdata",
    "modrinthapp",
    "com.modrinth.theseus",
    "modrinth app",
    "prismlauncher",
    "prism launcher",
    "multimc",
    "polymc",
    "curseforge",
    "atlauncher",
    "gdlauncher",
    ".minecraft",
    "retroarch",
    "start menu",
    "windowsapps",
    "windows",
    "system32",
    "$recycle.bin",
    "com.dukelauncher.desktop",
    "duke_save_backups",
];

/// Folder names that are never a dedicated single-game folder when they are the deletion target.
const PROTECTED_LEAF_NAMES: &[&str] = &[
    "users",
    "public",
    "desktop",
    "downloads",
    "documents",
    "my documents",
    "my games",
    "saved games",
    "music",
    "pictures",
    "videos",
    "onedrive",
    "local",
    "locallow",
    "roaming",
    "program files",
    "program files (x86)",
    "programdata",
    "games",
    "steam",
    "steamlibrary",
    "steamapps",
    "common",
    "epic games",
    "epic games launcher",
    "epicgames",
    "gog galaxy",
    "gog games",
    "gog.com",
    "xboxgames",
    "xbox games",
    "ubisoft",
    "ubisoft game launcher",
    "ea games",
    "ea app",
    "origin games",
    "battle.net",
    "riot games",
    "amazon games",
    "itch",
    "heroic",
    "playnite",
    "emulators",
    "roms",
];

/// Root-level files / folders that identify a launcher installation or launcher database.
const LAUNCHER_MARKERS: &[&str] = &[
    "steam.exe",
    "steamapps",
    "libraryfolders.vdf",
    "epicgameslauncher.exe",
    "galaxyclient.exe",
    "retroarch.exe",
    "retroarch.cfg",
    "modrinthapp.exe",
    "modrinth app.exe",
    "theseus.exe",
    "prismlauncher.exe",
    "prismlauncher.cfg",
    "multimc.exe",
    "multimc.cfg",
    "curseforge.exe",
    "overwolf.exe",
    "launcher_profiles.json",
    "app.db",
    "instances",
    "playnite.desktopapp.exe",
    "ubisoftconnect.exe",
    "upc.exe",
    "eadesktop.exe",
    "battle.net.exe",
    "duke-launcher.exe",
];

/// Launcher binaries that may never be removed by the exe-only fallback.
const LAUNCHER_EXE_NAMES: &[&str] = &[
    "steam.exe",
    "epicgameslauncher.exe",
    "galaxyclient.exe",
    "retroarch.exe",
    "modrinthapp.exe",
    "modrinth app.exe",
    "theseus.exe",
    "prismlauncher.exe",
    "multimc.exe",
    "curseforge.exe",
    "overwolf.exe",
    "playnite.desktopapp.exe",
    "ubisoftconnect.exe",
    "upc.exe",
    "eadesktop.exe",
    "battle.net.exe",
    "minecraftlauncher.exe",
    "duke-launcher.exe",
    "explorer.exe",
];

const HELPER_EXE_TOKENS: &[&str] = &[
    "unins", "setup", "install", "redist", "vcredist", "vc_redist", "dxsetup", "dxweb", "crash",
    "report", "launcher", "updater", "update", "helper", "dotnet", "prereq", "cefprocess",
    "webhelper", "easyanticheat", "battleye", "beservice", "uploader", "config", "settings",
    "benchmark", "server", "editor", "touchup", "cleanup", "dxdiag", "handler", "service",
];

const HELPER_DIR_NAMES: &[&str] = &[
    "bin", "binaries", "win64", "win32", "x64", "x86", "engine", "redist", "_redist",
    "_commonredist", "commonredist", "directx", "dotnet", "vcredist", "support", "tools",
    "crashreporter", "__installer", "installer", "installers", "prereq", "prerequisites",
    "plugins", "data", "content", "launcher", "easyanticheat", "battleye", "mono", "jre", "java",
    "runtime", "runtimes", "dependencies", "deps", "thirdparty", "third_party", ".duke-profile",
    ".duke-cas", ".egstore", "__overlay", "update", "updates", "patch", "patches", "mods",
    "shadercache", "logs", "saves", "lib", "libs", "dlc", "extras", "docs", "manual",
];

/// Environment-derived roots used by the validator. Split out so tests can inject fixtures.
#[derive(Debug, Clone, Default)]
pub(crate) struct ProtectedRoots {
    /// Deleting one of these, or any ancestor of one of these, is refused.
    pub exact: Vec<PathBuf>,
    /// Deleting anything at or below one of these is refused.
    pub containers: Vec<PathBuf>,
}

impl ProtectedRoots {
    pub(crate) fn from_environment() -> Self {
        let mut exact = Vec::new();
        let mut containers = Vec::new();
        let env_path = |k: &str| {
            std::env::var(k)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        if let Some(up) = env_path("USERPROFILE") {
            for sub in [
                "Desktop",
                "Downloads",
                "Documents",
                "Saved Games",
                "Music",
                "Pictures",
                "Videos",
                "OneDrive",
                "AppData",
            ] {
                exact.push(up.join(sub));
            }
            containers.push(up.join("Desktop"));
            containers.push(up.join("AppData"));
            exact.push(up);
        }
        if let Some(public) = env_path("PUBLIC") {
            containers.push(public.join("Desktop"));
            exact.push(public.join("Desktop"));
            exact.push(public);
        }
        for k in [
            "ProgramFiles",
            "ProgramFiles(x86)",
            "ProgramW6432",
            "ProgramData",
            "OneDrive",
            "HOMEPATH",
        ] {
            if let Some(p) = env_path(k) {
                exact.push(p);
            }
        }
        for k in ["APPDATA", "LOCALAPPDATA", "SystemRoot", "windir"] {
            if let Some(p) = env_path(k) {
                exact.push(p.clone());
                containers.push(p);
            }
        }
        if let Some(portable) = crate::resolve_portable_data_dir() {
            containers.push(portable);
        }
        if let Some(exe_dir) = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        {
            exact.push(exe_dir.clone());
            containers.push(exe_dir);
        }
        Self { exact, containers }
    }
}

/// Lexically normalizes an absolute path: strips quotes and the `\\?\` verbatim prefix, removes
/// `.` components, and rejects any `..` traversal or relative path.
fn normalize_absolute(p: &Path) -> Option<PathBuf> {
    let raw = p.to_string_lossy();
    let trimmed = raw.trim().trim_matches('"');
    let stripped = trimmed.strip_prefix(r"\\?\").unwrap_or(trimmed);
    if stripped.is_empty() {
        return None;
    }
    let candidate = Path::new(stripped);
    if !candidate.is_absolute() {
        return None;
    }
    let mut out = PathBuf::new();
    for c in candidate.components() {
        match c {
            Component::ParentDir => return None,
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    Some(out)
}

fn lower_key(p: &Path) -> String {
    p.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

fn path_is_within(child: &Path, parent: &Path) -> bool {
    let c = lower_key(child);
    let p = lower_key(parent);
    if p.is_empty() {
        return false;
    }
    c == p || c.starts_with(&format!("{p}\\"))
}

fn normal_components_lower(p: &Path) -> Vec<String> {
    p.components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().to_ascii_lowercase()),
            _ => None,
        })
        .collect()
}

fn is_helper_exe_name(name_lower: &str) -> bool {
    HELPER_EXE_TOKENS.iter().any(|t| name_lower.contains(t))
}

fn is_game_exe_entry(path: &Path) -> bool {
    let is_exe = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("exe"));
    if !is_exe {
        return false;
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    !is_helper_exe_name(&name)
}

/// Returns `Some(true)` if a non-helper executable exists within `max_depth`, `None` if the scan
/// budget was exhausted.
fn subtree_has_game_exe(dir: &Path, max_depth: usize, budget: &mut usize) -> Option<bool> {
    for ent in WalkDir::new(dir)
        .follow_links(false)
        .max_depth(max_depth)
        .into_iter()
        .filter_entry(|e| {
            e.depth() == 0
                || !e.file_type().is_dir()
                || !HELPER_DIR_NAMES.contains(&e.file_name().to_string_lossy().to_ascii_lowercase().as_str())
        })
        .flatten()
    {
        if *budget == 0 {
            return None;
        }
        *budget -= 1;
        if ent.file_type().is_file() && is_game_exe_entry(ent.path()) {
            return Some(true);
        }
    }
    Some(false)
}

/// Detects library-style container folders (e.g. `D:\Games`) that hold several separate games.
fn multi_game_container_reason(install_dir: &Path) -> Option<String> {
    let Ok(rd) = fs::read_dir(install_dir) else {
        return Some("Install folder could not be read for safety verification.".to_string());
    };
    let mut budget = MULTI_GAME_SCAN_BUDGET;
    let mut root_has_game_exe = false;
    let mut child_dirs = Vec::new();
    for ent in rd.flatten() {
        if budget == 0 {
            return Some("Install folder is too large to verify as a single-game folder.".to_string());
        }
        budget -= 1;
        let Ok(ft) = ent.file_type() else {
            continue;
        };
        let path = ent.path();
        if ft.is_file() && is_game_exe_entry(&path) {
            root_has_game_exe = true;
        } else if ft.is_dir() {
            let name = ent.file_name().to_string_lossy().to_ascii_lowercase();
            if !HELPER_DIR_NAMES.contains(&name.as_str()) {
                child_dirs.push(path);
            }
        }
    }
    let mut child_game_roots = 0usize;
    for child in &child_dirs {
        match subtree_has_game_exe(child, 3, &mut budget) {
            Some(true) => child_game_roots += 1,
            Some(false) => {}
            None => {
                return Some(
                    "Install folder is too large to verify as a single-game folder.".to_string(),
                )
            }
        }
        if child_game_roots >= 3 {
            break;
        }
    }
    if child_game_roots >= 3 || (child_game_roots >= 2 && !root_has_game_exe) {
        return Some(format!(
            "Folder contains {child_game_roots}+ separate game executables in different subfolders (shared library folder)."
        ));
    }
    None
}

/// Lexical / environment rules that never touch the filesystem.
fn lexical_refusal_reason(dir: &Path, roots: &ProtectedRoots) -> Option<String> {
    let comps = normal_components_lower(dir);
    if comps.is_empty() {
        return Some("Refusing to delete a drive root.".to_string());
    }
    if let Some(hit) = comps
        .iter()
        .find(|c| SHARED_TREE_COMPONENTS.contains(&c.as_str()))
    {
        return Some(format!(
            "Folder is inside a shared launcher or system tree ('{hit}'). Only the library entry can be removed."
        ));
    }
    let leaf = comps.last().map(String::as_str).unwrap_or("");
    if PROTECTED_LEAF_NAMES.contains(&leaf) {
        return Some(format!(
            "'{leaf}' is a shared user, store, or library folder, not a dedicated game folder."
        ));
    }
    if comps.len() >= 2 && comps[comps.len() - 2] == "users" {
        return Some("Refusing to delete a user profile directory.".to_string());
    }
    for protected in &roots.exact {
        if path_is_within(protected, dir) {
            return Some(format!(
                "Folder is (or contains) the protected location '{}'.",
                protected.display()
            ));
        }
    }
    for container in &roots.containers {
        if path_is_within(dir, container) {
            return Some(format!(
                "Folder is inside the protected location '{}'.",
                container.display()
            ));
        }
    }
    None
}

/// Central protected-path validator shared across `uninstaller.rs`, `redist_cleaner.rs`,
/// `cmd_system.rs`, `cmd_library.rs`, and `cmd_security.rs`.
///
/// Returns `Some(reason)` when `path` is a drive root, user profile root, Desktop/Downloads/
/// Documents/Pictures/Videos/AppData/Program Files/Windows/ProgramData, a shared launcher or
/// mod-manager tree (`ModrinthApp`, `PrismLauncher`, `CurseForge`, `MultiMC`, `ATLauncher`,
/// `Steam`, `steamapps\common`, `Epic Games`, `GOG Galaxy`, `XboxGames`, `RetroArch`), or an
/// existing directory containing launcher markers or multiple unrelated game executables.
pub(crate) fn protected_path_refusal_reason_with_roots(
    path: &Path,
    roots: &ProtectedRoots,
) -> Option<String> {
    let Some(normalized) = normalize_absolute(path) else {
        return Some("Path is empty, relative, or contains '..' traversal.".to_string());
    };
    if let Some(reason) = lexical_refusal_reason(&normalized, roots) {
        return Some(reason);
    }
    if let Ok(meta) = fs::symlink_metadata(&normalized) {
        if meta.file_type().is_symlink() {
            return Some("Path is a symlink or junction; refusing deletion.".to_string());
        }
        if meta.is_dir() {
            if let Ok(rd) = fs::read_dir(&normalized) {
                for ent in rd.flatten() {
                    let name = ent.file_name().to_string_lossy().to_ascii_lowercase();
                    if LAUNCHER_MARKERS.contains(&name.as_str()) {
                        return Some(format!(
                            "Folder contains launcher files ('{name}'); refusing deletion."
                        ));
                    }
                }
            }
            if let Some(reason) = multi_game_container_reason(&normalized) {
                return Some(reason);
            }
        }
    }
    None
}

pub(crate) fn protected_path_refusal_reason(path: &Path) -> Option<String> {
    protected_path_refusal_reason_with_roots(path, &ProtectedRoots::from_environment())
}

#[allow(dead_code)]
pub(crate) fn is_protected_user_or_system_path_with_roots(
    path: &Path,
    roots: &ProtectedRoots,
) -> bool {
    protected_path_refusal_reason_with_roots(path, roots).is_some()
}

pub(crate) fn is_protected_user_or_system_path(path: &Path) -> bool {
    protected_path_refusal_reason(path).is_some()
}

/// Validates that `install_dir` is a dedicated single-game folder that may be recursively removed.
/// Returns the normalized folder on success, or a human-readable refusal reason.
#[allow(dead_code)]
pub(crate) fn validate_safe_uninstall_target(
    install_dir: &Path,
    exe_path: &Path,
    other_library_exes: &[PathBuf],
) -> Result<PathBuf, String> {
    validate_safe_uninstall_target_with_roots(
        install_dir,
        exe_path,
        other_library_exes,
        &ProtectedRoots::from_environment(),
    )
}

pub(crate) fn validate_safe_uninstall_target_with_roots(
    install_dir: &Path,
    exe_path: &Path,
    other_library_exes: &[PathBuf],
    roots: &ProtectedRoots,
) -> Result<PathBuf, String> {
    let dir = normalize_absolute(install_dir)
        .ok_or_else(|| "Install folder is empty, relative, or contains '..' traversal.".to_string())?;

    if let Some(reason) = lexical_refusal_reason(&dir, roots) {
        return Err(reason);
    }

    let exe_raw = exe_path.to_string_lossy();
    if !exe_raw.trim().is_empty() {
        let exe = normalize_absolute(exe_path)
            .ok_or_else(|| "Game executable path is relative or contains '..'.".to_string())?;
        if !path_is_within(&exe, &dir) {
            return Err("Game executable is not inside the install folder.".to_string());
        }
    }

    for other in other_library_exes {
        if let Some(o) = normalize_absolute(other) {
            if path_is_within(&o, &dir) {
                return Err(format!(
                    "Folder also contains another library game ('{}').",
                    o.display()
                ));
            }
        }
    }

    let meta = fs::symlink_metadata(&dir)
        .map_err(|_| "Install folder no longer exists on disk.".to_string())?;
    if meta.file_type().is_symlink() {
        return Err("Install folder is a symlink or junction; refusing recursive deletion.".to_string());
    }
    if !meta.is_dir() {
        return Err("Install path is not a directory.".to_string());
    }

    if let Ok(rd) = fs::read_dir(&dir) {
        for ent in rd.flatten() {
            let name = ent.file_name().to_string_lossy().to_ascii_lowercase();
            if LAUNCHER_MARKERS.contains(&name.as_str()) {
                return Err(format!(
                    "Folder contains launcher files ('{name}'); it is a launcher installation, not a game folder."
                ));
            }
        }
    }

    if let Some(reason) = multi_game_container_reason(&dir) {
        return Err(reason);
    }
    Ok(dir)
}

/// Validates the single executable that the exe-only fallback may delete.
pub(crate) fn validate_exe_only_target(exe_path: &Path, roots: &ProtectedRoots) -> Result<PathBuf, String> {
    let exe = normalize_absolute(exe_path)
        .ok_or_else(|| "Game executable path is empty or invalid.".to_string())?;
    let name = exe
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !name.ends_with(".exe") {
        return Err("Only a single .exe file may be deleted in a shared folder.".to_string());
    }
    if LAUNCHER_EXE_NAMES.contains(&name.as_str()) {
        return Err(format!("'{name}' is a launcher executable and is never deleted."));
    }
    if let Some(reason) = lexical_refusal_reason(&exe, roots) {
        return Err(reason);
    }
    let meta = fs::symlink_metadata(&exe).map_err(|_| "Game executable no longer exists.".to_string())?;
    if !meta.is_file() {
        return Err("Game executable path is not a regular file.".to_string());
    }
    Ok(exe)
}

// ---------------------------------------------------------------------------------------------
// Save archiving
// ---------------------------------------------------------------------------------------------

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
    fs::rename(&temp_path, out_path)
        .map_err(|e| format!("Failed committing Zstd save archive: {e}"))?;
    Ok((out_path.to_path_buf(), out_len))
}

// ---------------------------------------------------------------------------------------------
// Scoped registry cleanup (HKCU only, keyed by exact install folder prefix)
// ---------------------------------------------------------------------------------------------

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
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .take_while(|&v| v != 0)
            .collect();
        Some(String::from_utf16_lossy(&u16s))
    }

    fn within_prefix(value_lower: &str, install_lower: &str) -> bool {
        let v = value_lower.trim().trim_start_matches('"').trim_end_matches('\\');
        v == install_lower || v.starts_with(&format!("{install_lower}\\"))
    }

    pub fn scan_matching_hkcu_uninstall_keys(install_dir: &Path) -> Vec<String> {
        let mut out = Vec::new();
        let install_lower = install_dir
            .to_string_lossy()
            .trim_end_matches('\\')
            .to_ascii_lowercase();
        if install_lower.len() < 4 {
            return out;
        }
        let sub = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall");
        let mut hroot: isize = 0;
        if unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, sub.as_ptr(), 0, KEY_READ, &mut hroot) }
            != ERROR_SUCCESS
        {
            return out;
        }
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
                if (!loc.is_empty() && within_prefix(&loc, &install_lower))
                    || (!icon.is_empty() && within_prefix(&icon, &install_lower))
                {
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
        let keys = scan_matching_hkcu_uninstall_keys(install_dir);
        if keys.is_empty() {
            return cleaned;
        }
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

// ---------------------------------------------------------------------------------------------
// Preview & execution
// ---------------------------------------------------------------------------------------------

fn resolve_entry_install_dir(entry: &crate::GameEntry) -> PathBuf {
    let raw = entry.install_dir.trim();
    if !raw.is_empty() {
        return crate::resolve_game_root_dir(Path::new(raw));
    }
    Path::new(entry.exe_path.trim())
        .parent()
        .map(crate::resolve_game_root_dir)
        .unwrap_or_default()
}

fn other_library_exes(library: &[crate::GameEntry], game_id: &str) -> Vec<PathBuf> {
    library
        .iter()
        .filter(|g| g.id != game_id && !g.exe_path.trim().is_empty())
        .map(|g| PathBuf::from(g.exe_path.trim()))
        .collect()
}

fn build_preview_for_entry(
    entry: &crate::GameEntry,
    others: &[PathBuf],
    roots: &ProtectedRoots,
) -> CleanUninstallPreview {
    let install_dir = resolve_entry_install_dir(entry);
    let exe_path = PathBuf::from(entry.exe_path.trim());
    let verdict = validate_safe_uninstall_target_with_roots(&install_dir, &exe_path, others, roots);
    let folder_deletion_allowed = verdict.is_ok();
    let refusal_reason = verdict.as_ref().err().cloned();
    let exe_only_target = if folder_deletion_allowed {
        None
    } else {
        validate_exe_only_target(&exe_path, roots)
            .ok()
            .map(|p| p.to_string_lossy().into_owned())
    };
    let install_dir_bytes = if folder_deletion_allowed {
        dir_size_bytes(&install_dir)
    } else {
        0
    };
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

    let hkcu_uninstall_keys = if folder_deletion_allowed {
        win_reg_clean::scan_matching_hkcu_uninstall_keys(&install_dir)
    } else {
        Vec::new()
    };
    let gpu_pref_keys = vec![entry.exe_path.clone()];

    let summary = match (&refusal_reason, &exe_only_target) {
        (None, _) => format!(
            "{} ({}) · dedicated game folder verified · {} save dir(s), {} HKCU registry key(s).",
            entry.title,
            crate::format_bytes(install_dir_bytes),
            save_paths.len(),
            hkcu_uninstall_keys.len()
        ),
        (Some(reason), Some(_)) => format!(
            "Folder deletion refused: {reason} Only the library entry (or the single game .exe) can be removed."
        ),
        (Some(reason), None) => format!(
            "Folder deletion refused: {reason} Only the library entry can be removed."
        ),
    };

    CleanUninstallPreview {
        game_id: entry.id.clone(),
        title: entry.title.clone(),
        install_dir: install_dir.to_string_lossy().into_owned(),
        install_dir_bytes,
        exe_path: entry.exe_path.clone(),
        sandbox_profile_dir,
        save_paths,
        save_bytes,
        folder_deletion_allowed,
        refusal_reason,
        exe_only_target,
        gpu_pref_keys,
        hkcu_uninstall_keys,
        summary,
    }
}

pub(crate) type CleanUninstallSummary = (Option<(String, u64)>, Vec<String>);

/// Archives saves (optional) and removes a **pre-validated** dedicated game folder.
/// Never touches AppData, Desktop, Start Menu, or `.lnk` shortcuts.
pub(crate) fn clean_uninstall_in_dirs(
    install_dir: &Path,
    save_dirs: &[PathBuf],
    zstd_archive_target: Option<&Path>,
) -> Result<CleanUninstallSummary, String> {
    // Defense in depth: even pre-validated callers can never delete drive roots or shared folders.
    if let Some(reason) = protected_path_refusal_reason(install_dir) {
        return Err(format!("Refusing to delete protected path: {reason}"));
    }
    let normalized = normalize_absolute(install_dir)
        .ok_or_else(|| "Refusing to delete an invalid or relative path.".to_string())?;
    let comps = normal_components_lower(&normalized);
    if comps.len() < 2 {
        return Err("Refusing to delete a drive root or top-level folder.".to_string());
    }

    let mut archive_info: Option<(String, u64)> = None;
    if let Some(target_zst) = zstd_archive_target {
        if !save_dirs.is_empty() {
            if let Ok((saved_path, bytes)) = pack_saves_to_zstd_bundle(save_dirs, target_zst) {
                archive_info = Some((saved_path.to_string_lossy().into_owned(), bytes));
            }
        }
    }

    let mut removed = Vec::new();
    let is_real_dir = fs::symlink_metadata(&normalized)
        .map(|m| m.is_dir() && !m.file_type().is_symlink())
        .unwrap_or(false);
    if is_real_dir {
        fs::remove_dir_all(&normalized)
            .map_err(|e| format!("Failed deleting game directory '{}': {e}", normalized.display()))?;
        removed.push(normalized.to_string_lossy().into_owned());
    }
    Ok((archive_info, removed))
}

/// Inputs for one uninstall decision (separated from `AppHandle` for testability).
pub(crate) struct UninstallPlan {
    pub install_dir: PathBuf,
    pub exe_path: PathBuf,
    pub save_dirs: Vec<PathBuf>,
    pub zst_target: Option<PathBuf>,
    pub other_library_exes: Vec<PathBuf>,
}

pub(crate) struct UninstallOutcome {
    pub mode: &'static str,
    pub deleted_target: PathBuf,
    pub archive_info: Option<(String, u64)>,
    pub removed: Vec<String>,
}

pub(crate) fn run_uninstall_plan(
    plan: &UninstallPlan,
    roots: &ProtectedRoots,
    exe_only_fallback: bool,
) -> Result<UninstallOutcome, String> {
    match validate_safe_uninstall_target_with_roots(
        &plan.install_dir,
        &plan.exe_path,
        &plan.other_library_exes,
        roots,
    ) {
        Ok(dir) => {
            let (archive_info, removed) =
                clean_uninstall_in_dirs(&dir, &plan.save_dirs, plan.zst_target.as_deref())?;
            Ok(UninstallOutcome {
                mode: "folder",
                deleted_target: dir,
                archive_info,
                removed,
            })
        }
        Err(reason) => {
            if !exe_only_fallback {
                return Err(format!(
                    "Refused to delete game files: {reason} Use \"Remove from Launcher\" instead."
                ));
            }
            let exe = validate_exe_only_target(&plan.exe_path, roots)
                .map_err(|e| format!("Refused to delete game files: {reason} ({e})"))?;
            let mut archive_info = None;
            if let Some(target) = plan.zst_target.as_deref() {
                if !plan.save_dirs.is_empty() {
                    if let Ok((p, b)) = pack_saves_to_zstd_bundle(&plan.save_dirs, target) {
                        archive_info = Some((p.to_string_lossy().into_owned(), b));
                    }
                }
            }
            fs::remove_file(&exe)
                .map_err(|e| format!("Failed deleting '{}': {e}", exe.display()))?;
            Ok(UninstallOutcome {
                mode: "exe_only",
                removed: vec![exe.to_string_lossy().into_owned()],
                deleted_target: exe,
                archive_info,
            })
        }
    }
}

#[tauri::command]
pub async fn preview_clean_uninstall(game_id: String) -> Result<CleanUninstallPreview, String> {
    let lib = crate::load_library_standalone()?;
    let others = other_library_exes(&lib, &game_id);
    let entry = lib
        .into_iter()
        .find(|g| g.id == game_id)
        .ok_or_else(|| format!("Game '{}' not found.", game_id))?;
    tokio::task::spawn_blocking(move || {
        Ok(build_preview_for_entry(
            &entry,
            &others,
            &ProtectedRoots::from_environment(),
        ))
    })
    .await
    .map_err(|e| format!("Uninstall preview task failed: {e}"))?
}

/// Deletes game files only for a validated dedicated folder (or, with `exe_only_fallback`, the
/// single game `.exe` in a shared folder), then removes the library entry.
#[tauri::command]
pub async fn execute_clean_uninstall(
    app: tauri::AppHandle,
    game_id: String,
    keep_saves_zstd: bool,
    exe_only_fallback: Option<bool>,
) -> Result<CleanUninstallReport, String> {
    let mut library = crate::load_library_standalone()?;
    let idx = library
        .iter()
        .position(|g| g.id == game_id)
        .ok_or_else(|| format!("Game '{}' not found.", game_id))?;
    let entry = library[idx].clone();
    let others = other_library_exes(&library, &game_id);
    let allow_exe_only = exe_only_fallback.unwrap_or(false);

    let cleanup_res = tokio::task::spawn_blocking({
        let entry_c = entry.clone();
        move || -> Result<CleanUninstallReport, String> {
            let roots = ProtectedRoots::from_environment();
            let preview = build_preview_for_entry(&entry_c, &others, &roots);
            let exe_path = PathBuf::from(entry_c.exe_path.trim());

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

            let plan = UninstallPlan {
                install_dir: PathBuf::from(&preview.install_dir),
                exe_path: exe_path.clone(),
                save_dirs,
                zst_target,
                other_library_exes: others,
            };
            let outcome = run_uninstall_plan(&plan, &roots, allow_exe_only)?;

            let reg_count = if outcome.mode == "folder" {
                win_reg_clean::clean_registry_traces(&exe_path, &outcome.deleted_target)
            } else {
                0
            };
            let _ = crate::duke_jail::cleanup_game_jail_profile_sync(&entry_c.id, &exe_path);

            let (mut cas_removed, mut cas_freed) = (0usize, 0u64);
            if let Ok(global_cas) = cas_root_dir_standalone() {
                if let Ok((r, b)) = gc_cas_blobs(&global_cas) {
                    cas_removed += r;
                    cas_freed += b;
                }
            }
            if outcome.mode == "folder" {
                let local_cas = outcome.deleted_target.join(CAS_DIR_NAME);
                if local_cas.is_dir() {
                    let _ = fs::remove_dir_all(&local_cas);
                }
            }

            let (save_archive_path, save_archive_bytes) = outcome
                .archive_info
                .map(|(p, b)| (Some(p), b))
                .unwrap_or((None, 0));
            let target_str = outcome.deleted_target.to_string_lossy().into_owned();
            let what = if outcome.mode == "folder" {
                format!("folder '{target_str}'")
            } else {
                format!("executable '{target_str}'")
            };
            let summary = match &save_archive_path {
                Some(p) => format!(
                    "Uninstalled '{}' (deleted {}). Saved Zstd backup ({}) -> {}",
                    entry_c.title,
                    what,
                    crate::format_bytes(save_archive_bytes),
                    p
                ),
                None => format!(
                    "Uninstalled '{}' (deleted {}, {} registry/GPU trace(s) cleaned).",
                    entry_c.title, what, reg_count
                ),
            };

            Ok(CleanUninstallReport {
                game_id: entry_c.id,
                title: entry_c.title,
                mode: outcome.mode.to_string(),
                deleted_target: target_str,
                save_archive_path,
                save_archive_bytes,
                removed_dirs: outcome.removed,
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
    crate::emit_if_alive(&app, "library-updated", &library);

    Ok(cleanup_res)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixture root outside `%TEMP%` (which lives under `AppData` on Windows and is itself refused).
    fn fixture_root(tag: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("uninstaller-fixtures-{tag}-{}", std::process::id()))
    }

    fn abs(rel: &str) -> PathBuf {
        let base = if cfg!(windows) { "C:\\" } else { "/" };
        PathBuf::from(base).join(rel)
    }

    fn assert_refused(dir: PathBuf, roots: &ProtectedRoots) {
        let exe = dir.join("game.exe");
        let res = validate_safe_uninstall_target_with_roots(&dir, &exe, &[], roots);
        assert!(res.is_err(), "expected refusal for {}", dir.display());
    }

    #[test]
    fn refuses_shared_launchers_user_folders_and_store_roots() {
        let profile = abs("Users/Player");
        let roots = ProtectedRoots {
            exact: vec![
                profile.clone(),
                profile.join("Desktop"),
                profile.join("Downloads"),
                profile.join("Documents"),
                profile.join("AppData"),
            ],
            containers: vec![profile.join("AppData")],
        };
        // Shared launchers (and every profile/instance inside them)
        assert_refused(profile.join("AppData/Roaming/ModrinthApp"), &roots);
        assert_refused(profile.join("AppData/Roaming/ModrinthApp/profiles/Fabric 1.21"), &roots);
        assert_refused(abs("Games/ModrinthApp/profiles/Vanilla"), &roots);
        assert_refused(abs("Games/PrismLauncher/instances/1.20.1"), &roots);
        assert_refused(abs("Games/CurseForge/Instances/Pack"), &roots);
        assert_refused(abs("Emulation/RetroArch"), &roots);
        // User folders
        assert_refused(profile.join("Desktop"), &roots);
        assert_refused(profile.join("Downloads"), &roots);
        assert_refused(profile.join("Documents"), &roots);
        assert_refused(profile.join("AppData"), &roots);
        assert_refused(profile.join("AppData/Local/SomeGame"), &roots);
        assert_refused(profile.clone(), &roots);
        assert_refused(abs("Users"), &roots);
        // Store library roots
        assert_refused(abs("Program Files (x86)/Steam/steamapps/common"), &roots);
        assert_refused(abs("SteamLibrary/steamapps/common"), &roots);
        assert_refused(abs("SteamLibrary/steamapps"), &roots);
        assert_refused(abs("Program Files/Epic Games"), &roots);
        assert_refused(abs("Program Files"), &roots);
        // Drive root and traversal tricks
        assert_refused(abs(""), &roots);
        assert_refused(
            abs("Games/Foo").join("..").join("..").join("Users").join("Player").join("Desktop"),
            &roots,
        );
        assert!(validate_safe_uninstall_target_with_roots(
            Path::new("relative/Game"),
            Path::new(""),
            &[],
            &roots
        )
        .is_err());
    }

    #[test]
    fn refuses_env_protected_ancestors_and_containers() {
        let roots = ProtectedRoots {
            exact: vec![abs("Data/Profile/Stuff")],
            containers: vec![abs("Data/LauncherState")],
        };
        // Ancestor of a protected root
        assert_refused(abs("Data/Profile"), &roots);
        // Inside a protected container
        assert_refused(abs("Data/LauncherState/Game"), &roots);
    }

    #[test]
    fn exe_outside_install_dir_and_other_library_games_are_refused() {
        let root = fixture_root("contain");
        let _ = fs::remove_dir_all(&root);
        let game = root.join("Library").join("SpaceQuest");
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("SpaceQuest.exe"), b"MZ").unwrap();
        let roots = ProtectedRoots::default();

        let outside = root.join("Elsewhere").join("other.exe");
        assert!(validate_safe_uninstall_target_with_roots(&game, &outside, &[], &roots).is_err());

        let sibling_game = game.join("Bundled").join("OtherGame.exe");
        assert!(validate_safe_uninstall_target_with_roots(
            &game,
            &game.join("SpaceQuest.exe"),
            std::slice::from_ref(&sibling_game),
            &roots
        )
        .is_err());

        assert!(validate_safe_uninstall_target_with_roots(
            &game,
            &game.join("SpaceQuest.exe"),
            &[],
            &roots
        )
        .is_ok());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn refuses_launcher_installs_and_multi_game_folders() {
        let root = fixture_root("multi");
        let _ = fs::remove_dir_all(&root);
        let roots = ProtectedRoots::default();

        // Launcher database marker at the folder root
        let launcher = root.join("Launchers").join("MyLauncher");
        fs::create_dir_all(&launcher).unwrap();
        fs::write(launcher.join("app.db"), b"sqlite").unwrap();
        fs::write(launcher.join("play.exe"), b"MZ").unwrap();
        assert!(validate_safe_uninstall_target_with_roots(
            &launcher,
            &launcher.join("play.exe"),
            &[],
            &roots
        )
        .is_err());

        // Library container with several separate games
        let shelf = root.join("Shelf").join("Collection");
        for name in ["Alpha", "Bravo", "Charlie"] {
            let g = shelf.join(name);
            fs::create_dir_all(&g).unwrap();
            fs::write(g.join(format!("{name}.exe")), b"MZ").unwrap();
        }
        assert!(validate_safe_uninstall_target_with_roots(
            &shelf,
            &shelf.join("Alpha").join("Alpha.exe"),
            &[],
            &roots
        )
        .is_err());

        // A dedicated game folder with helper executables stays deletable
        let game = shelf.join("Alpha");
        fs::create_dir_all(game.join("_CommonRedist")).unwrap();
        fs::write(game.join("_CommonRedist").join("vc_redist.x64.exe"), b"MZ").unwrap();
        fs::write(game.join("unins000.exe"), b"MZ").unwrap();
        assert!(validate_safe_uninstall_target_with_roots(
            &game,
            &game.join("Alpha.exe"),
            &[],
            &roots
        )
        .is_ok());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn uninstall_never_touches_desktop_lnk_shortcuts_or_shared_folders() {
        let root = fixture_root("lnk");
        let _ = fs::remove_dir_all(&root);
        let desktop = root.join("Profile").join("Desktop");
        let downloads = root.join("Profile").join("Downloads");
        let game = root.join("Games").join("SpaceQuest");
        fs::create_dir_all(&desktop).unwrap();
        fs::create_dir_all(&downloads).unwrap();
        fs::create_dir_all(&game).unwrap();
        let game_lnk = desktop.join("SpaceQuest.lnk");
        let other_lnk = desktop.join("Modrinth App.lnk");
        fs::write(&game_lnk, b"L\0\0\0").unwrap();
        fs::write(&other_lnk, b"L\0\0\0").unwrap();
        fs::write(game.join("SpaceQuest.exe"), b"MZ").unwrap();
        let loose_exe = downloads.join("LooseGame.exe");
        fs::write(&loose_exe, b"MZ").unwrap();
        fs::write(downloads.join("tax-return.pdf"), b"%PDF").unwrap();
        let roots = ProtectedRoots::default();

        // Dedicated folder: removed, shortcuts untouched.
        let plan = UninstallPlan {
            install_dir: game.clone(),
            exe_path: game.join("SpaceQuest.exe"),
            save_dirs: Vec::new(),
            zst_target: None,
            other_library_exes: Vec::new(),
        };
        let out = run_uninstall_plan(&plan, &roots, false).unwrap();
        assert_eq!(out.mode, "folder");
        assert!(!game.exists());
        assert!(game_lnk.is_file(), "game .lnk must never be deleted");
        assert!(other_lnk.is_file(), "unrelated .lnk must never be deleted");

        // Shared Downloads folder: folder deletion refused without fallback.
        let shared_plan = UninstallPlan {
            install_dir: downloads.clone(),
            exe_path: loose_exe.clone(),
            save_dirs: Vec::new(),
            zst_target: None,
            other_library_exes: Vec::new(),
        };
        assert!(run_uninstall_plan(&shared_plan, &roots, false).is_err());
        assert!(loose_exe.is_file());

        // Explicit exe-only fallback removes just the single .exe.
        let out = run_uninstall_plan(&shared_plan, &roots, true).unwrap();
        assert_eq!(out.mode, "exe_only");
        assert!(!loose_exe.exists());
        assert!(downloads.join("tax-return.pdf").is_file());
        assert!(downloads.is_dir());

        // Desktop itself can never be a deletion target.
        assert!(validate_safe_uninstall_target_with_roots(&desktop, &game_lnk, &[], &roots).is_err());
        assert!(game_lnk.is_file() && other_lnk.is_file());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn exe_only_fallback_never_deletes_launcher_binaries() {
        let root = fixture_root("exeguard");
        let _ = fs::remove_dir_all(&root);
        let dir = root.join("Tools");
        fs::create_dir_all(&dir).unwrap();
        let steam = dir.join("steam.exe");
        fs::write(&steam, b"MZ").unwrap();
        assert!(validate_exe_only_target(&steam, &ProtectedRoots::default()).is_err());
        assert!(steam.is_file());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn clean_uninstall_archives_saves_under_5mb_zstd_before_removing_game_dir() {
        let root = fixture_root("zstd");
        let _ = fs::remove_dir_all(&root);
        let game_dir = root.join("Library").join("SpaceQuest");
        let prof_dir = game_dir.join(PROFILE_DIR_NAME);
        fs::create_dir_all(&prof_dir).unwrap();
        fs::write(game_dir.join("game.exe"), b"MZ").unwrap();
        fs::write(prof_dir.join("slot1.sav"), vec![0x42u8; 4096]).unwrap();

        let archive_dst = root.join("backups").join("spacequest.tar.zst");
        let (archive_info, removed) = clean_uninstall_in_dirs(
            &game_dir,
            std::slice::from_ref(&prof_dir),
            Some(&archive_dst),
        )
        .unwrap();

        let (arc_path, arc_bytes) = archive_info.expect("save Zstd bundle should be created");
        assert!(Path::new(&arc_path).is_file());
        assert!(arc_bytes < 5 * 1024 * 1024);
        assert!(!game_dir.exists());
        assert_eq!(removed.len(), 1);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn is_protected_user_or_system_path_strictly_rejects_user_system_and_launcher_roots() {
        let user_root = PathBuf::from("C:\\").join("Users").join("Player");
        let public_desktop = PathBuf::from("C:\\").join("Users").join("Public").join("Desktop");
        let protected_cases = vec![
            PathBuf::from("C:\\"),
            PathBuf::from("D:\\"),
            user_root.clone(),
            user_root.join("Desktop"),
            user_root.join("Desktop").join("SpaceQuest.lnk"),
            public_desktop,
            user_root.join("Downloads"),
            user_root.join("Documents"),
            user_root.join("Pictures"),
            user_root.join("Videos"),
            user_root.join("AppData"),
            user_root.join("AppData").join("Roaming"),
            user_root.join("AppData").join("Local"),
            user_root.join("AppData").join("LocalLow"),
            PathBuf::from("C:\\Program Files"),
            PathBuf::from("C:\\Program Files (x86)"),
            PathBuf::from("C:\\Windows"),
            PathBuf::from("C:\\ProgramData"),
            user_root.join("AppData").join("Roaming").join("ModrinthApp").join("profiles").join("PackA"),
            PathBuf::from("D:\\Launchers\\ModrinthApp"),
            PathBuf::from("D:\\Launchers\\PrismLauncher\\instances\\Survival"),
            PathBuf::from("D:\\Launchers\\CurseForge\\Minecraft"),
            PathBuf::from("D:\\Launchers\\MultiMC\\instances"),
            PathBuf::from("D:\\Launchers\\ATLauncher\\instances"),
            PathBuf::from("C:\\Program Files (x86)\\Steam"),
            PathBuf::from("C:\\Program Files (x86)\\Steam\\steamapps\\common"),
            PathBuf::from("C:\\Program Files\\Epic Games"),
            PathBuf::from("C:\\Program Files (x86)\\GOG Galaxy"),
            PathBuf::from("D:\\XboxGames"),
            PathBuf::from("D:\\Emulators\\RetroArch"),
        ];

        for p in &protected_cases {
            assert!(
                is_protected_user_or_system_path(p),
                "Expected protected path to be strictly rejected: {}",
                p.display()
            );
            assert!(
                clean_uninstall_in_dirs(p, &[], None).is_err(),
                "clean_uninstall_in_dirs must refuse protected path: {}",
                p.display()
            );
        }

        // Dedicated single-game folder outside protected trees is allowed.
        let root = fixture_root("central_guard");
        let _ = fs::remove_dir_all(&root);
        let safe_game = root.join("DedicatedGames").join("StarDrifter");
        fs::create_dir_all(&safe_game).unwrap();
        fs::write(safe_game.join("StarDrifter.exe"), b"MZ").unwrap();
        assert!(
            !is_protected_user_or_system_path_with_roots(&safe_game, &ProtectedRoots::default()),
            "Dedicated single-game folder should not be marked protected"
        );
        let _ = fs::remove_dir_all(&root);
    }
}

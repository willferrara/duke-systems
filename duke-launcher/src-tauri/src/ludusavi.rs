use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

pub const LUDUSAVI_MANIFEST_PRIMARY_URL: &str =
    "https://raw.githubusercontent.com/mtkennerly/ludusavi-manifest/refs/heads/data/data/manifest.yaml";
pub const LUDUSAVI_MANIFEST_FALLBACK_URL: &str =
    "https://raw.githubusercontent.com/mtkennerly/ludusavi-manifest/master/data/manifest.yaml";
const LUDUSAVI_TTL_SECS: u64 = 7 * 24 * 60 * 60;

static LUDUSAVI_IO_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn get_ludusavi_io_lock() -> &'static Mutex<()> {
    LUDUSAVI_IO_LOCK.get_or_init(|| Mutex::new(()))
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct LudusaviGameRecord {
    pub title: String,
    pub norm_title: String,
    pub steam_id: Option<u32>,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct LudusaviCacheFile {
    pub fetched_at: u64,
    pub source_url: String,
    pub entries: Vec<LudusaviGameRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct LudusaviCacheStatus {
    pub synced: bool,
    pub entry_count: usize,
    pub fetched_at: u64,
    pub source_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SavePathCandidate {
    pub path: String,
    pub exists: bool,
    pub source: String,
    pub pattern: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct DiscoveredSaveReport {
    pub game_id: String,
    pub game_title: String,
    pub matched_manifest_title: Option<String>,
    pub cache_age_secs: Option<u64>,
    pub existing_paths: Vec<SavePathCandidate>,
    pub candidate_paths: Vec<SavePathCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SaveBackupSnapshot {
    pub filename: String,
    pub path: String,
    pub created_ts: u64,
    pub size_bytes: u64,
    pub size_formatted: String,
    pub root_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SaveRestoreResult {
    pub backup_path: String,
    pub restored_files: usize,
    pub target_dirs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct BackupArchiveManifest {
    game_id: String,
    game_title: String,
    install_dir: String,
    created_at: u64,
    roots: HashMap<String, String>,
}

#[derive(Debug, Clone, Default)]
pub struct PathEnvVars {
    pub win_app_data: String,
    pub win_local_app_data: String,
    pub win_local_app_data_low: String,
    pub win_documents: String,
    pub win_public: String,
    pub win_program_data: String,
    pub home: String,
    pub os_user_name: String,
    pub base_dir: String,
    pub store_user_ids: Vec<String>,
}

impl PathEnvVars {
    pub fn detect(base_dir: &str) -> Self {
        let user_profile = std::env::var("USERPROFILE").unwrap_or_default();
        let win_app_data = std::env::var("APPDATA").unwrap_or_else(|_| {
            if user_profile.is_empty() {
                String::new()
            } else {
                format!("{}\\AppData\\Roaming", user_profile)
            }
        });
        let win_local_app_data = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| {
            if user_profile.is_empty() {
                String::new()
            } else {
                format!("{}\\AppData\\Local", user_profile)
            }
        });
        let win_local_app_data_low = if user_profile.is_empty() {
            String::new()
        } else {
            format!("{}\\AppData\\LocalLow", user_profile)
        };
        let win_documents = resolve_windows_documents_dir(&user_profile);
        let win_public = std::env::var("PUBLIC")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "C:\\Users\\Public".to_string());
        let win_program_data = std::env::var("PROGRAMDATA")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "C:\\ProgramData".to_string());
        let os_user_name = std::env::var("USERNAME").unwrap_or_default();
        let store_user_ids = detect_steam_user_ids();

        Self {
            win_app_data,
            win_local_app_data,
            win_local_app_data_low,
            win_documents,
            win_public,
            win_program_data,
            home: user_profile,
            os_user_name,
            base_dir: base_dir.trim_end_matches(['\\', '/']).to_string(),
            store_user_ids,
        }
    }
}

fn resolve_windows_documents_dir(user_profile: &str) -> String {
    if let Some(shell_docs) = duke_core::win32_ffi::reg_read_string(
        false,
        "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\User Shell Folders",
        "Personal",
    ) {
        let expanded = if !user_profile.is_empty() {
            shell_docs.replace("%USERPROFILE%", user_profile)
        } else {
            shell_docs
        };
        if !expanded.trim().is_empty() && !expanded.contains('%') {
            return expanded;
        }
    }
    if !user_profile.is_empty() {
        format!("{}\\Documents", user_profile)
    } else {
        String::new()
    }
}

fn detect_steam_user_ids() -> Vec<String> {
    let mut ids = Vec::new();
    let Some(steam_path) = duke_core::win32_ffi::reg_read_string(
        false,
        "Software\\Valve\\Steam",
        "SteamPath",
    ) else {
        return ids;
    };
    let userdata = PathBuf::from(steam_path.replace('/', "\\")).join("userdata");
    if let Ok(entries) = fs::read_dir(&userdata) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.chars().all(|c| c.is_ascii_digit()) {
                    ids.push(name);
                }
            }
        }
    }
    ids
}

pub fn normalize_title_key(raw: &str) -> String {
    let stripped = crate::clean_game_title(raw);
    let source = if stripped.is_empty() {
        raw.to_string()
    } else {
        stripped
    };
    source
        .to_lowercase()
        .replace('&', "and")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

fn unquote_yaml_scalar(raw: &str) -> String {
    let s = raw.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        let inner = &s[1..s.len() - 1];
        inner
            .replace("\\\"", "\"")
            .replace("\\\\", "\\")
            .replace("\\/", "/")
    } else if s.len() >= 2 && s.starts_with('\'') && s.ends_with('\'') {
        s[1..s.len() - 1].replace("''", "'")
    } else {
        s.to_string()
    }
}

/// Zero-dependency parser for `ludusavi-manifest/data/manifest.yaml`.
pub fn parse_ludusavi_manifest_yaml(yaml_text: &str) -> Vec<LudusaviGameRecord> {
    #[derive(Default)]
    struct RawGame {
        title: String,
        steam_id: Option<u32>,
        alias: Option<String>,
        files: Vec<String>,
    }

    #[derive(Default)]
    struct PendingFile {
        pattern: String,
        has_when_os: bool,
        matches_windows: bool,
    }

    #[derive(PartialEq, Eq)]
    enum Section {
        None,
        Files,
        Steam,
        Other,
    }

    let mut parsed_games: Vec<RawGame> = Vec::with_capacity(8192);
    let mut current: Option<RawGame> = None;
    let mut current_section = Section::None;
    let mut pending_file: Option<PendingFile> = None;
    let mut inside_when = false;

    let flush_pending_file = |pend_opt: &mut Option<PendingFile>, game: &mut RawGame| {
        if let Some(pf) = pend_opt.take() {
            let allow = !pf.has_when_os || pf.matches_windows;
            let trimmed_pat = pf.pattern.trim();
            let linux_only = (trimmed_pat.starts_with("<xdg")
                || trimmed_pat.starts_with("/home")
                || trimmed_pat.starts_with("/Users"))
                && !trimmed_pat.contains("<win");
            if allow && !linux_only && !trimmed_pat.is_empty() {
                game.files.push(trimmed_pat.to_string());
            }
        }
    };

    for raw_line in yaml_text.lines() {
        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() || line == "---" || line.trim_start().starts_with('#') {
            continue;
        }

        let indent = line.bytes().take_while(|&b| b == b' ').count();
        let trimmed = line.trim();

        // Top-level game title (`indent == 0`)
        if indent == 0 {
            if let Some(mut g) = current.take() {
                flush_pending_file(&mut pending_file, &mut g);
                parsed_games.push(g);
            }
            if let Some(key_part) = trimmed.strip_suffix(':') {
                let title = unquote_yaml_scalar(key_part);
                if !title.is_empty() {
                    current = Some(RawGame {
                        title,
                        ..Default::default()
                    });
                }
            }
            current_section = Section::None;
            inside_when = false;
            continue;
        }

        let Some(ref mut game) = current else {
            continue;
        };

        // Top section under game (`indent == 2`)
        if indent == 2 {
            flush_pending_file(&mut pending_file, game);
            inside_when = false;

            if trimmed == "files:" || trimmed == "files: {}" {
                current_section = if trimmed == "files:" {
                    Section::Files
                } else {
                    Section::None
                };
            } else if trimmed == "steam:" {
                current_section = Section::Steam;
            } else if let Some(rest) = trimmed.strip_prefix("alias:") {
                let target_alias = unquote_yaml_scalar(rest);
                if !target_alias.is_empty() {
                    game.alias = Some(target_alias);
                }
                current_section = Section::Other;
            } else if trimmed.ends_with(':') || trimmed.contains(": ") {
                current_section = Section::Other;
            }
            continue;
        }

        match current_section {
            Section::Steam => {
                if let Some(id_val) = trimmed.strip_prefix("id:") {
                    if let Ok(parsed_id) = id_val.trim().parse::<u32>() {
                        game.steam_id = Some(parsed_id);
                    }
                }
            }
            Section::Files => {
                if indent == 4 && trimmed.ends_with(':') {
                    flush_pending_file(&mut pending_file, game);
                    inside_when = false;
                    let key_part = trimmed.strip_suffix(':').unwrap_or("");
                    let pat = unquote_yaml_scalar(key_part);
                    pending_file = Some(PendingFile {
                        pattern: pat,
                        has_when_os: false,
                        matches_windows: false,
                    });
                } else if let Some(ref mut pf) = pending_file {
                    if indent == 6 && trimmed == "when:" {
                        inside_when = true;
                    } else if indent == 6 && trimmed.ends_with(':') {
                        inside_when = false;
                    } else if inside_when {
                        if let Some(os_str) = trimmed
                            .strip_prefix("- os:")
                            .or_else(|| trimmed.strip_prefix("os:"))
                        {
                            pf.has_when_os = true;
                            if unquote_yaml_scalar(os_str).eq_ignore_ascii_case("windows") {
                                pf.matches_windows = true;
                            }
                        }
                    }
                }
            }
            Section::None | Section::Other => {}
        }
    }

    if let Some(mut g) = current.take() {
        flush_pending_file(&mut pending_file, &mut g);
        parsed_games.push(g);
    }

    let mut files_by_title: HashMap<String, Vec<String>> = HashMap::with_capacity(parsed_games.len());
    for g in &parsed_games {
        if !g.files.is_empty() {
            files_by_title.insert(g.title.clone(), g.files.clone());
        }
    }

    let mut out = Vec::with_capacity(parsed_games.len());
    for mut g in parsed_games {
        if g.files.is_empty() {
            if let Some(ref alias_target) = g.alias {
                if let Some(aliased_files) = files_by_title.get(alias_target) {
                    g.files = aliased_files.clone();
                }
            }
        }
        if g.files.is_empty() && g.steam_id.is_none() {
            continue;
        }
        let norm_title = normalize_title_key(&g.title);
        out.push(LudusaviGameRecord {
            title: g.title,
            norm_title,
            steam_id: g.steam_id,
            files: g.files,
        });
    }
    out
}

fn clean_expanded_pattern_to_dir(raw_path: &str) -> PathBuf {
    let mut normalized = raw_path.replace('/', "\\");
    while normalized.contains("\\\\") {
        normalized = normalized.replace("\\\\", "\\");
    }
    let trimmed = normalized.trim().trim_end_matches('\\');

    // Trim wildcard segments (`\*.sav`, `\*`, `\**`) to the enclosing directory
    if let Some(star_idx) = trimmed.find('*') {
        let prefix = &trimmed[..star_idx];
        let dir_part = prefix
            .rfind('\\')
            .map(|idx| &prefix[..idx])
            .unwrap_or(prefix)
            .trim_end_matches('\\');
        return PathBuf::from(dir_part);
    }

    let p = PathBuf::from(trimmed);
    if p.is_file() {
        return p.parent().unwrap_or(p.as_path()).to_path_buf();
    }
    // If leaf looks like an explicit file extension (e.g. `settings.sav`, `save.dat`) rather than folder
    if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
        if matches!(
            ext.to_ascii_lowercase().as_str(),
            "sav" | "save" | "dat" | "sl2" | "ini" | "cfg" | "json" | "xml" | "bin" | "txt"
        ) && !p.is_dir()
        {
            if let Some(parent) = p.parent() {
                return parent.to_path_buf();
            }
        }
    }
    p
}

/// Expands a Ludusavi placeholder path template into concrete Windows directory path(s).
pub fn expand_ludusavi_template(pattern: &str, env: &PathEnvVars) -> Vec<PathBuf> {
    let mut base = pattern.trim().to_string();
    let pairs = [
        ("<winAppData>", &env.win_app_data),
        ("<winLocalAppData>", &env.win_local_app_data),
        ("<winLocalAppDataLow>", &env.win_local_app_data_low),
        ("<winDocuments>", &env.win_documents),
        ("<winPublic>", &env.win_public),
        ("<winProgramData>", &env.win_program_data),
        ("<home>", &env.home),
        ("<osUserName>", &env.os_user_name),
        ("<base>", &env.base_dir),
        ("<game>", &env.base_dir),
    ];

    for (token, val) in pairs {
        if base.contains(token) {
            if val.trim().is_empty() {
                return Vec::new();
            }
            base = base.replace(token, val.trim_end_matches(['\\', '/']));
        }
    }

    // Reject unexpanded non-Windows placeholders (`<xdgData>`, `<root>`, etc.)
    if base.contains('<') && !base.contains("<storeUserId>") {
        return Vec::new();
    }

    if !base.contains("<storeUserId>") {
        let dir = clean_expanded_pattern_to_dir(&base);
        return if dir.as_os_str().is_empty() {
            Vec::new()
        } else {
            vec![dir]
        };
    }

    let mut results = Vec::new();
    // 1. Expand known Steam user IDs
    for uid in &env.store_user_ids {
        let replaced = base.replace("<storeUserId>", uid);
        let dir = clean_expanded_pattern_to_dir(&replaced);
        if !dir.as_os_str().is_empty() && !results.contains(&dir) {
            results.push(dir);
        }
    }

    // 2. Also probe actual subdirectories at `<storeUserId>` parent on disk (handles Epic/GOG/custom IDs)
    if let Some((before, after)) = base.split_once("<storeUserId>") {
        let parent_dir = clean_expanded_pattern_to_dir(before);
        if parent_dir.is_dir() {
            if let Ok(read_dir) = fs::read_dir(&parent_dir) {
                for entry in read_dir.flatten() {
                    if entry.path().is_dir() {
                        let candidate_raw = format!(
                            "{}{}",
                            entry.path().to_string_lossy(),
                            after.replace('/', "\\")
                        );
                        let dir = clean_expanded_pattern_to_dir(&candidate_raw);
                        if !dir.as_os_str().is_empty() && !results.contains(&dir) {
                            results.push(dir);
                        }
                    }
                }
            }
        }
    }

    results
}

fn load_ludusavi_cache_from_disk() -> LudusaviCacheFile {
    let _guard = get_ludusavi_io_lock().lock().unwrap();
    let Ok(path) = crate::get_app_data_file_standalone("ludusavi_cache.json") else {
        return LudusaviCacheFile::default();
    };
    if !path.exists() {
        return LudusaviCacheFile::default();
    }
    let Ok(raw) = fs::read_to_string(&path) else {
        return LudusaviCacheFile::default();
    };
    if raw.trim().is_empty() {
        return LudusaviCacheFile::default();
    }
    serde_json::from_str::<LudusaviCacheFile>(&raw).unwrap_or_default()
}

fn save_ludusavi_cache_to_disk(cache: &LudusaviCacheFile) -> Result<(), String> {
    let _guard = get_ludusavi_io_lock().lock().unwrap();
    let path = crate::get_app_data_file_standalone("ludusavi_cache.json")?;
    let serialized = serde_json::to_string(cache)
        .map_err(|e| format!("Failed serializing ludusavi_cache.json: {}", e))?;
    crate::atomic_write_string(&path, &serialized)
}

pub(crate) async fn ensure_ludusavi_cache_fresh(
    force: bool,
) -> Result<LudusaviCacheFile, String> {
    let existing = tokio::task::spawn_blocking(load_ludusavi_cache_from_disk)
        .await
        .map_err(|e| format!("Failed loading ludusavi cache: {}", e))?;

    let now = now_unix_secs();
    let fresh = !existing.entries.is_empty()
        && existing.fetched_at > 0
        && now.saturating_sub(existing.fetched_at) < LUDUSAVI_TTL_SECS;

    if fresh && !force {
        return Ok(existing);
    }

    let mut fetched_yaml: Option<(String, String)> = None;
    let mut last_err = String::new();
    for url in [LUDUSAVI_MANIFEST_PRIMARY_URL, LUDUSAVI_MANIFEST_FALLBACK_URL] {
        let client = duke_core::net_doh::build_doh_client_with_timeouts(
            url,
            Duration::from_secs(15),
            Duration::from_secs(45),
            true,
        )
        .await;
        match client.get(url).send().await {
            Ok(resp) if resp.status().is_success() => match resp.text().await {
                Ok(text) if !text.trim().is_empty() => {
                    fetched_yaml = Some((url.to_string(), text));
                    break;
                }
                Ok(_) => last_err = format!("Empty response from {}", url),
                Err(e) => last_err = format!("Failed reading body from {}: {}", url, e),
            },
            Ok(resp) => last_err = format!("HTTP {} from {}", resp.status().as_u16(), url),
            Err(e) => last_err = format!("Request failed for {}: {}", url, e),
        }
    }

    let Some((source_url, yaml_text)) = fetched_yaml else {
        if !existing.entries.is_empty() {
            return Ok(existing);
        }
        return Err(format!("Failed syncing Ludusavi manifest: {}", last_err));
    };

    let parsed_cache = tokio::task::spawn_blocking(move || -> Result<LudusaviCacheFile, String> {
        let entries = parse_ludusavi_manifest_yaml(&yaml_text);
        if entries.is_empty() {
            return Err("Parsed Ludusavi manifest yielded 0 entries.".to_string());
        }
        let cache = LudusaviCacheFile {
            fetched_at: now,
            source_url,
            entries,
        };
        save_ludusavi_cache_to_disk(&cache)?;
        Ok(cache)
    })
    .await
    .map_err(|e| format!("Failed parsing Ludusavi manifest: {}", e))??;

    crate::trim_process_working_set();
    Ok(parsed_cache)
}

fn generate_heuristic_save_candidates(
    title: &str,
    install_dir: &str,
    steam_app_id: Option<u32>,
    env: &PathEnvVars,
) -> Vec<(String, PathBuf)> {
    let mut candidates: Vec<(String, PathBuf)> = Vec::new();
    let clean_title = crate::clean_game_title(title);
    let safe_fs_title: String = clean_title
        .chars()
        .filter(|&c| !matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .collect();
    let compact_title: String = safe_fs_title.replace(' ', "");

    let mut name_variants: Vec<String> = Vec::new();
    for v in [
        title.trim().to_string(),
        clean_title.trim().to_string(),
        safe_fs_title.trim().to_string(),
        compact_title.trim().to_string(),
    ] {
        if !v.is_empty() && !name_variants.iter().any(|x| x.eq_ignore_ascii_case(&v)) {
            name_variants.push(v);
        }
    }

    let bases = [
        ("APPDATA", &env.win_app_data),
        ("LOCALAPPDATA", &env.win_local_app_data),
        ("LocalLow", &env.win_local_app_data_low),
    ];

    for (label, base) in bases {
        if base.is_empty() {
            continue;
        }
        for name in &name_variants {
            candidates.push((format!("%{}%\\{}", label, name), PathBuf::from(base).join(name)));
        }
    }

    if !env.home.is_empty() {
        for name in &name_variants {
            candidates.push((
                format!("%USERPROFILE%\\Saved Games\\{}", name),
                PathBuf::from(&env.home).join("Saved Games").join(name),
            ));
        }
    }

    if !env.win_documents.is_empty() {
        for name in &name_variants {
            candidates.push((
                format!("%DOCUMENTS%\\My Games\\{}", name),
                PathBuf::from(&env.win_documents).join("My Games").join(name),
            ));
            candidates.push((
                format!("%DOCUMENTS%\\{}", name),
                PathBuf::from(&env.win_documents).join(name),
            ));
        }
    }

    if let Some(aid) = steam_app_id {
        if let Some(steam_path) = duke_core::win32_ffi::reg_read_string(
            false,
            "Software\\Valve\\Steam",
            "SteamPath",
        ) {
            let userdata_root = PathBuf::from(steam_path.replace('/', "\\")).join("userdata");
            for uid in &env.store_user_ids {
                candidates.push((
                    format!("Steam userdata/{}/{}/remote", uid, aid),
                    userdata_root.join(uid).join(aid.to_string()).join("remote"),
                ));
            }
        }
    }

    if !install_dir.trim().is_empty() {
        let root = Path::new(install_dir);
        if root.is_dir() {
            const SAVE_KEYWORDS: &[&str] = &[
                "save",
                "saves",
                "savegame",
                "savegames",
                "savedata",
                "save_data",
                "userdata",
                "profile",
                "profiles",
                "remote",
                "steam_settings",
            ];
            for entry in WalkDir::new(root).max_depth(3).into_iter().filter_map(|e| e.ok()) {
                if entry.file_type().is_dir() && entry.path() != root {
                    let fname = entry.file_name().to_string_lossy().to_lowercase();
                    if SAVE_KEYWORDS.iter().any(|&kw| fname == kw || fname.contains(kw)) {
                        candidates.push((
                            format!("<base>\\{}", entry.file_name().to_string_lossy()),
                            entry.path().to_path_buf(),
                        ));
                    }
                }
            }
        }
    }

    candidates
}

pub(crate) async fn discover_game_save_paths_core(
    game_id: &str,
    sync_if_missing: bool,
) -> Result<DiscoveredSaveReport, String> {
    let library = tokio::task::spawn_blocking(crate::load_library_standalone)
        .await
        .map_err(|e| format!("Failed reading library: {}", e))??;

    let entry = library
        .iter()
        .find(|g| g.id == game_id || g.exe_path.eq_ignore_ascii_case(game_id))
        .ok_or_else(|| format!("Game '{}' not found in library.", game_id))?
        .clone();

    let cache = if sync_if_missing {
        ensure_ludusavi_cache_fresh(false).await.unwrap_or_else(|_| load_ludusavi_cache_from_disk())
    } else {
        tokio::task::spawn_blocking(load_ludusavi_cache_from_disk)
            .await
            .unwrap_or_default()
    };

    tokio::task::spawn_blocking(move || build_save_report_for_entry(&entry, &cache))
        .await
        .map_err(|e| format!("Save discovery failed: {}", e))
}

pub(crate) fn build_save_report_for_entry(
    entry: &crate::GameEntry,
    cache: &LudusaviCacheFile,
) -> DiscoveredSaveReport {
    let steam_app_id = entry
        .app_id
        .or_else(|| entry.steam_app_id.and_then(|v| u32::try_from(v).ok()));
    let norm_game_title = normalize_title_key(&entry.title);

    let matched = steam_app_id
        .and_then(|sid| cache.entries.iter().find(|r| r.steam_id == Some(sid)))
        .or_else(|| {
            cache
                .entries
                .iter()
                .find(|r| !norm_game_title.is_empty() && r.norm_title == norm_game_title)
        });

    let env = PathEnvVars::detect(&entry.install_dir);
    let mut seen: HashSet<String> = HashSet::new();
    let mut existing_paths: Vec<SavePathCandidate> = Vec::new();
    let mut candidate_paths: Vec<SavePathCandidate> = Vec::new();

    if let Some(rec) = matched {
        for pat in &rec.files {
            for expanded_dir in expand_ludusavi_template(pat, &env) {
                let norm_key = expanded_dir
                    .to_string_lossy()
                    .trim_end_matches('\\')
                    .to_lowercase();
                if norm_key.is_empty() || !seen.insert(norm_key) {
                    continue;
                }
                let exists = expanded_dir.exists();
                let item = SavePathCandidate {
                    path: expanded_dir.to_string_lossy().to_string(),
                    exists,
                    source: "ludusavi".to_string(),
                    pattern: Some(pat.clone()),
                };
                if exists {
                    existing_paths.push(item);
                } else {
                    candidate_paths.push(item);
                }
            }
        }
    }

    for (label, path) in
        generate_heuristic_save_candidates(&entry.title, &entry.install_dir, steam_app_id, &env)
    {
        let norm_key = path.to_string_lossy().trim_end_matches('\\').to_lowercase();
        if norm_key.is_empty() || !seen.insert(norm_key) {
            continue;
        }
        let exists = path.exists();
        let src = if label.starts_with("<base>") {
            "install_dir"
        } else {
            "heuristic"
        };
        let item = SavePathCandidate {
            path: path.to_string_lossy().to_string(),
            exists,
            source: src.to_string(),
            pattern: Some(label),
        };
        if exists {
            existing_paths.push(item);
        } else if candidate_paths.len() < 8 {
            candidate_paths.push(item);
        }
    }

    let now = now_unix_secs();
    let cache_age_secs = if cache.fetched_at > 0 {
        Some(now.saturating_sub(cache.fetched_at))
    } else {
        None
    };

    DiscoveredSaveReport {
        game_id: entry.id.clone(),
        game_title: entry.title.clone(),
        matched_manifest_title: matched.map(|m| m.title.clone()),
        cache_age_secs,
        existing_paths,
        candidate_paths,
    }
}

pub(crate) async fn backup_game_saves_core(
    game_id: Option<String>,
    title: Option<String>,
    install_dir: Option<String>,
    exe_path: Option<String>,
) -> Result<String, String> {
    let mut resolved_id = game_id.unwrap_or_default().trim().to_string();
    let mut resolved_title = title.unwrap_or_default().trim().to_string();
    let mut resolved_install_dir = install_dir.unwrap_or_default().trim().to_string();
    let mut resolved_exe_path = exe_path.unwrap_or_default().trim().to_string();
    let mut matched_entry: Option<crate::GameEntry> = None;

    if let Ok(library) = load_library_blocking().await {
        if let Some(entry) = library.iter().find(|g| {
            (!resolved_id.is_empty() && g.id == resolved_id)
                || (!resolved_exe_path.is_empty()
                    && g.exe_path.eq_ignore_ascii_case(&resolved_exe_path))
                || (!resolved_title.is_empty() && g.title.eq_ignore_ascii_case(&resolved_title))
        }) {
            matched_entry = Some(entry.clone());
            if resolved_id.is_empty() {
                resolved_id = entry.id.clone();
            }
            if resolved_title.is_empty() {
                resolved_title = entry.title.clone();
            }
            if resolved_install_dir.is_empty() {
                resolved_install_dir = entry.install_dir.clone();
            }
            if resolved_exe_path.is_empty() {
                resolved_exe_path = entry.exe_path.clone();
            }
        }
    }

    let game_root = if !resolved_install_dir.is_empty() && Path::new(&resolved_install_dir).exists()
    {
        PathBuf::from(&resolved_install_dir)
    } else if !resolved_exe_path.is_empty() && Path::new(&resolved_exe_path).exists() {
        crate::resolve_game_root_dir(Path::new(&resolved_exe_path))
    } else {
        PathBuf::new()
    };

    if resolved_title.is_empty() && !game_root.as_os_str().is_empty() {
        resolved_title = game_root
            .file_name()
            .and_then(|n| n.to_str())
            .map(crate::clean_game_title)
            .unwrap_or_else(|| "Game".to_string());
    }

    let discovered_report = if let Some(ref ent) = matched_entry {
        let cache = ensure_ludusavi_cache_fresh(false)
            .await
            .unwrap_or_else(|_| load_ludusavi_cache_from_disk());
        Some(build_save_report_for_entry(ent, &cache))
    } else {
        None
    };

    let backups_dir = crate::get_backups_dir_standalone()?;
    if !backups_dir.exists() {
        fs::create_dir_all(&backups_dir)
            .map_err(|e| format!("Failed to create Duke_Save_Backups directory: {}", e))?;
    }

    let safe_game_title = sanitize_backup_prefix(&resolved_title);
    let timestamp = now_unix_secs();
    let zip_filename = format!("{}_{}.zip", safe_game_title, timestamp);
    let zip_path = backups_dir.join(&zip_filename);
    let prefix_for_rotate = format!("{}_", safe_game_title);

    tokio::task::spawn_blocking(move || -> Result<String, String> {
        const EXCLUDED_EXTENSIONS: &[&str] = &[
            "exe", "dll", "pak", "ucas", "utoc", "pck", "rpf", "ba2", "bsa", "assets", "ress",
            "resource", "mp4", "bk2", "webm", "ogg", "wav", "mp3", "pdb", "lib", "exp", "so",
        ];
        const SAVE_EXTENSIONS: &[&str] = &[
            "sav", "save", "dat", "sl2", "prf", "slot", "bak", "ini", "cfg", "json", "xml",
        ];

        let mut collected_files: HashMap<String, Vec<u8>> = HashMap::new();
        let mut root_map: HashMap<String, String> = HashMap::new();
        let max_single_file_bytes: u64 = 64 * 1024 * 1024;
        let max_total_backup_bytes: u64 = 256 * 1024 * 1024;
        let mut total_collected_bytes: u64 = 0;

        // 1. Collect files from Ludusavi & heuristic existing directories
        if let Some(ref report) = discovered_report {
            for (idx, cand) in report.existing_paths.iter().enumerate() {
                let save_dir = PathBuf::from(&cand.path);
                if !save_dir.exists() {
                    continue;
                }
                let root_key = idx.to_string();
                let mut added_any = false;

                if save_dir.is_file() {
                    if let Ok(meta) = fs::metadata(&save_dir) {
                        if meta.len() <= max_single_file_bytes
                            && total_collected_bytes + meta.len() <= max_total_backup_bytes
                        {
                            if let Ok(bytes) = fs::read(&save_dir) {
                                let fname = save_dir
                                    .file_name()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("save.dat");
                                total_collected_bytes += bytes.len() as u64;
                                collected_files.insert(format!("roots/{}/{}", root_key, fname), bytes);
                                added_any = true;
                            }
                        }
                    }
                    if added_any {
                        if let Some(parent) = save_dir.parent() {
                            root_map.insert(root_key, parent.to_string_lossy().to_string());
                        }
                    }
                } else {
                    for entry in WalkDir::new(&save_dir)
                        .max_depth(6)
                        .into_iter()
                        .filter_map(|e| e.ok())
                    {
                        if entry.file_type().is_file() {
                            let path = entry.path();
                            let ext = path
                                .extension()
                                .and_then(|s| s.to_str())
                                .unwrap_or("")
                                .to_lowercase();
                            if EXCLUDED_EXTENSIONS.contains(&ext.as_str()) {
                                continue;
                            }
                            if let Ok(meta) = entry.metadata() {
                                if meta.len() <= max_single_file_bytes
                                    && total_collected_bytes + meta.len() <= max_total_backup_bytes
                                {
                                    if let Ok(bytes) = fs::read(path) {
                                        let rel = path
                                            .strip_prefix(&save_dir)
                                            .unwrap_or(path)
                                            .to_string_lossy()
                                            .replace('\\', "/");
                                        total_collected_bytes += bytes.len() as u64;
                                        collected_files
                                            .insert(format!("roots/{}/{}", root_key, rel), bytes);
                                        added_any = true;
                                    }
                                }
                            }
                        }
                    }
                    if added_any {
                        root_map.insert(root_key, save_dir.to_string_lossy().to_string());
                    }
                }
            }
        }

        // 2. Also scan install root (depth <= 3) for standalone save/config files if not already archived
        if !game_root.as_os_str().is_empty() && game_root.is_dir() {
            let mut install_added = false;
            for entry in WalkDir::new(&game_root)
                .max_depth(3)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                if entry.file_type().is_file() {
                    let path = entry.path();
                    let ext = path
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    let fname_lower = entry.file_name().to_string_lossy().to_lowercase();
                    let is_save_file = SAVE_EXTENSIONS.contains(&ext.as_str())
                        || fname_lower.contains("save")
                        || fname_lower.contains("profile")
                        || fname_lower.contains("user");

                    if is_save_file && !EXCLUDED_EXTENSIONS.contains(&ext.as_str()) {
                        if let Ok(meta) = entry.metadata() {
                            if meta.len() <= 25 * 1024 * 1024
                                && total_collected_bytes + meta.len() <= max_total_backup_bytes
                            {
                                let rel = path
                                    .strip_prefix(&game_root)
                                    .unwrap_or(path)
                                    .to_string_lossy()
                                    .replace('\\', "/");
                                let zip_key = format!("roots/install/{}", rel);
                                if let std::collections::hash_map::Entry::Vacant(e) =
                                    collected_files.entry(zip_key)
                                {
                                    if let Ok(bytes) = fs::read(path) {
                                        total_collected_bytes += bytes.len() as u64;
                                        e.insert(bytes);
                                        install_added = true;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if install_added {
                root_map.insert(
                    "install".to_string(),
                    game_root.to_string_lossy().to_string(),
                );
            }
        }

        let archive_manifest = BackupArchiveManifest {
            game_id: resolved_id.clone(),
            game_title: resolved_title.clone(),
            install_dir: game_root.to_string_lossy().to_string(),
            created_at: timestamp,
            roots: root_map.clone(),
        };
        if let Ok(manifest_json) = serde_json::to_vec_pretty(&archive_manifest) {
            collected_files.insert("duke_save_backup_manifest.json".to_string(), manifest_json);
        }

        let roots_desc: Vec<String> = root_map
            .iter()
            .map(|(k, v)| format!("  [{}] {}", k, v))
            .collect();
        let manifest_text = format!(
            "Duke Launcher - Save Backup\r\nGame Title: {}\r\nTimestamp (UTC Unix): {}\r\nFiles Archived: {}\r\nTotal Size: {}\r\nSave Roots:\r\n{}\r\n",
            resolved_title,
            timestamp,
            collected_files.len(),
            crate::format_bytes(total_collected_bytes),
            roots_desc.join("\r\n")
        );
        collected_files.insert(
            "duke_save_backup_manifest.txt".to_string(),
            manifest_text.into_bytes(),
        );

        let entries_vec: Vec<(String, Vec<u8>)> = collected_files.into_iter().collect();
        crate::write_pure_rust_zip_archive(&zip_path, &entries_vec)?;

        // Configurable rolling save snapshot retention per game (defaults to 5)
        let max_keep = crate::load_settings_standalone()
            .max_restore_points
            .clamp(1, 50) as usize;
        if let Ok(read_dir) = fs::read_dir(&backups_dir) {
            let mut game_snapshots: Vec<(SystemTime, PathBuf)> = read_dir
                .filter_map(|e| e.ok())
                .filter_map(|entry| {
                    let path = entry.path();
                    let fname = entry.file_name().to_string_lossy().to_string();
                    if path.is_file()
                        && fname.starts_with(&prefix_for_rotate)
                        && fname.to_lowercase().ends_with(".zip")
                    {
                        let mtime = entry
                            .metadata()
                            .and_then(|m| m.modified())
                            .unwrap_or(UNIX_EPOCH);
                        Some((mtime, path))
                    } else {
                        None
                    }
                })
                .collect();

            game_snapshots.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
            for (_, old_path) in game_snapshots.into_iter().skip(max_keep) {
                let _ = fs::remove_file(old_path);
            }
        }

        crate::trim_process_working_set();
        Ok(zip_path.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| format!("Save backup task failed: {}", e))?
}

async fn load_library_blocking() -> Result<Vec<crate::GameEntry>, String> {
    tokio::task::spawn_blocking(crate::load_library_standalone)
        .await
        .map_err(|e| format!("Failed reading library: {}", e))?
}

fn sanitize_backup_prefix(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    if cleaned.is_empty() {
        "DukeGame".to_string()
    } else {
        cleaned
    }
}

fn is_safe_archive_rel_path(rel: &str) -> Option<PathBuf> {
    let trimmed = rel.trim();
    if trimmed.is_empty() || trimmed.contains('\0') {
        return None;
    }
    let normalized = trimmed.replace('\\', "/");
    let p = Path::new(&normalized);
    let mut safe_buf = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::Normal(part) => safe_buf.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if safe_buf.as_os_str().is_empty() {
        None
    } else {
        Some(safe_buf)
    }
}

/// Reads STORE (method 0) ZIP archives written by `write_pure_rust_zip_archive` with zero external crates.
pub fn read_pure_rust_zip_archive(zip_path: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut f = File::open(zip_path)
        .map_err(|e| format!("Failed opening backup archive '{}': {}", zip_path.display(), e))?;
    let mut data = Vec::new();
    f.read_to_end(&mut data)
        .map_err(|e| format!("Failed reading backup archive '{}': {}", zip_path.display(), e))?;

    let mut pos = 0usize;
    let mut entries = Vec::new();
    while pos + 30 <= data.len() {
        let sig = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
        if sig == 0x0201_4B50 || sig == 0x0605_4B50 {
            break;
        }
        if sig != 0x0403_4B50 {
            return Err("Unsupported or corrupted ZIP local header signature.".to_string());
        }

        let method = u16::from_le_bytes([data[pos + 8], data[pos + 9]]);
        let expected_crc =
            u32::from_le_bytes([data[pos + 14], data[pos + 15], data[pos + 16], data[pos + 17]]);
        let comp_size =
            u32::from_le_bytes([data[pos + 18], data[pos + 19], data[pos + 20], data[pos + 21]])
                as usize;
        let name_len = u16::from_le_bytes([data[pos + 26], data[pos + 27]]) as usize;
        let extra_len = u16::from_le_bytes([data[pos + 28], data[pos + 29]]) as usize;

        let name_start = pos + 30;
        let data_start = name_start + name_len + extra_len;
        let data_end = data_start.saturating_add(comp_size);
        if data_end > data.len() {
            return Err("Truncated ZIP archive entry payload.".to_string());
        }
        if method != 0 {
            return Err(format!(
                "ZIP entry uses unsupported compression method {} (expected STORE).",
                method
            ));
        }

        let name = String::from_utf8_lossy(&data[name_start..name_start + name_len]).to_string();
        let payload = data[data_start..data_end].to_vec();
        if crate::crc32_ieee(&payload) != expected_crc {
            return Err(format!("CRC32 mismatch for archived file '{}'.", name));
        }
        entries.push((name, payload));
        pos = data_end;
    }

    Ok(entries)
}

fn is_allowed_restore_root(target: &Path, install_dir: &str) -> bool {
    let norm_target = target.to_string_lossy().replace('/', "\\").to_lowercase();
    let env = PathEnvVars::detect(install_dir);
    let mut allowed_bases = vec![
        env.win_app_data,
        env.win_local_app_data,
        env.win_local_app_data_low,
        env.win_documents,
        env.win_public,
        env.win_program_data,
        env.home,
        env.base_dir,
    ];
    if let Some(steam_path) =
        duke_core::win32_ffi::reg_read_string(false, "Software\\Valve\\Steam", "SteamPath")
    {
        allowed_bases.push(format!("{}\\userdata", steam_path.replace('/', "\\")));
    }

    allowed_bases
        .into_iter()
        .map(|b| b.trim().replace('/', "\\").to_lowercase())
        .filter(|b| !b.is_empty())
        .any(|base| norm_target == base || norm_target.starts_with(&format!("{}\\", base)))
}

#[tauri::command]
pub(crate) async fn sync_ludusavi_manifest(
    force: Option<bool>,
) -> Result<LudusaviCacheStatus, String> {
    let cache = ensure_ludusavi_cache_fresh(force.unwrap_or(false)).await?;
    Ok(LudusaviCacheStatus {
        synced: true,
        entry_count: cache.entries.len(),
        fetched_at: cache.fetched_at,
        source_url: cache.source_url,
    })
}

#[tauri::command]
pub(crate) async fn discover_game_save_paths(
    game_id: String,
    sync_if_missing: Option<bool>,
) -> Result<DiscoveredSaveReport, String> {
    discover_game_save_paths_core(&game_id, sync_if_missing.unwrap_or(true)).await
}

#[tauri::command]
pub(crate) async fn list_save_backups(game_id: String) -> Result<Vec<SaveBackupSnapshot>, String> {
    let library = load_library_blocking().await?;
    let entry = library
        .iter()
        .find(|g| g.id == game_id || g.exe_path.eq_ignore_ascii_case(&game_id))
        .ok_or_else(|| format!("Game '{}' not found in library.", game_id))?
        .clone();

    let safe_prefix = format!("{}_", sanitize_backup_prefix(&entry.title));
    let backups_dir = crate::get_backups_dir_standalone()?;

    tokio::task::spawn_blocking(move || -> Result<Vec<SaveBackupSnapshot>, String> {
        if !backups_dir.exists() {
            return Ok(Vec::new());
        }
        let mut items = Vec::new();
        for dir_entry in fs::read_dir(&backups_dir)
            .map_err(|e| format!("Failed reading backups directory: {}", e))?
            .flatten()
        {
            let p = dir_entry.path();
            let fname = dir_entry.file_name().to_string_lossy().to_string();
            if !p.is_file()
                || !fname.starts_with(&safe_prefix)
                || !fname.to_lowercase().ends_with(".zip")
            {
                continue;
            }
            let meta = dir_entry.metadata().ok();
            let size_bytes = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let created_ts = fname
                .trim_start_matches(&safe_prefix)
                .trim_end_matches(".zip")
                .parse::<u64>()
                .ok()
                .or_else(|| {
                    meta.as_ref()
                        .and_then(|m| m.modified().ok())
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                })
                .unwrap_or(0);

            let root_count = read_pure_rust_zip_archive(&p)
                .ok()
                .and_then(|entries| {
                    entries
                        .into_iter()
                        .find(|(k, _)| k == "duke_save_backup_manifest.json")
                        .and_then(|(_, bytes)| serde_json::from_slice::<BackupArchiveManifest>(&bytes).ok())
                })
                .map(|m| m.roots.len())
                .unwrap_or(1);

            items.push(SaveBackupSnapshot {
                filename: fname,
                path: p.to_string_lossy().to_string(),
                created_ts,
                size_bytes,
                size_formatted: crate::format_bytes(size_bytes),
                root_count,
            });
        }
        items.sort_by_key(|a| std::cmp::Reverse(a.created_ts));
        Ok(items)
    })
    .await
    .map_err(|e| format!("Failed listing save backups: {}", e))?
}

#[tauri::command]
pub(crate) async fn restore_game_save(
    game_id: String,
    backup_path: String,
) -> Result<SaveRestoreResult, String> {
    let library = load_library_blocking().await?;
    let entry = library
        .iter()
        .find(|g| g.id == game_id || g.exe_path.eq_ignore_ascii_case(&game_id))
        .ok_or_else(|| format!("Game '{}' not found in library.", game_id))?
        .clone();

    let zip_path = PathBuf::from(backup_path.trim().trim_matches('"'));
    let backups_dir = crate::get_backups_dir_standalone()?;

    tokio::task::spawn_blocking(move || -> Result<SaveRestoreResult, String> {
        let canon_zip = fs::canonicalize(&zip_path)
            .map_err(|e| format!("Backup archive not found '{}': {}", zip_path.display(), e))?;
        let canon_backups = fs::canonicalize(&backups_dir)
            .map_err(|e| format!("Backups folder unavailable: {}", e))?;
        if !canon_zip.starts_with(&canon_backups) {
            return Err("Refusing to restore backup outside Duke_Save_Backups.".to_string());
        }

        let entries = read_pure_rust_zip_archive(&canon_zip)?;
        let manifest = entries
            .iter()
            .find(|(name, _)| name == "duke_save_backup_manifest.json")
            .and_then(|(_, bytes)| serde_json::from_slice::<BackupArchiveManifest>(bytes).ok());

        let mut restored_files = 0usize;
        let mut target_dirs: HashSet<String> = HashSet::new();

        for (raw_name, content) in entries {
            if raw_name == "duke_save_backup_manifest.json"
                || raw_name == "duke_save_backup_manifest.txt"
            {
                continue;
            }
            let normalized = raw_name.replace('\\', "/");

            let (base_dir, rel_str) = if let Some(after_roots) = normalized.strip_prefix("roots/") {
                let Some((root_key, rel_part)) = after_roots.split_once('/') else {
                    continue;
                };
                let target_root = manifest
                    .as_ref()
                    .and_then(|m| m.roots.get(root_key))
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from(&entry.install_dir));
                (target_root, rel_part.to_string())
            } else {
                (PathBuf::from(&entry.install_dir), normalized)
            };

            if base_dir.as_os_str().is_empty() || !is_allowed_restore_root(&base_dir, &entry.install_dir) {
                continue;
            }
            let Some(safe_rel) = is_safe_archive_rel_path(&rel_str) else {
                continue;
            };
            let dest_path = base_dir.join(&safe_rel);
            crate::atomic_write_bytes(&dest_path, &content)?;
            target_dirs.insert(base_dir.to_string_lossy().to_string());
            restored_files += 1;
        }

        crate::trim_process_working_set();
        let mut dirs_vec: Vec<String> = target_dirs.into_iter().collect();
        dirs_vec.sort();
        Ok(SaveRestoreResult {
            backup_path: canon_zip.to_string_lossy().to_string(),
            restored_files,
            target_dirs: dirs_vec,
        })
    })
    .await
    .map_err(|e| format!("Save restore task failed: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ludusavi_yaml_and_filters_linux_only_entries() {
        let yaml = r#"---
"Hades II":
  files:
    "<winDocuments>/Saved Games/Hades II":
      tags:
        - save
      when:
        - os: windows
    "<xdgData>/Supergiant Games/Hades II":
      tags:
        - save
      when:
        - os: linux
  steam:
    id: 1145350
"Alias Edition":
  alias: "Hades II"
  steam:
    id: 999999
"#;

        let records = parse_ludusavi_manifest_yaml(yaml);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].title, "Hades II");
        assert_eq!(records[0].steam_id, Some(1145350));
        assert_eq!(
            records[0].files,
            vec!["<winDocuments>/Saved Games/Hades II"]
        );
        assert_eq!(records[1].title, "Alias Edition");
        assert_eq!(records[1].files, records[0].files);
    }

    #[test]
    fn expands_ludusavi_placeholders_and_wildcards() {
        let env = PathEnvVars {
            win_app_data: "C:\\Users\\Player\\AppData\\Roaming".to_string(),
            win_local_app_data: "C:\\Users\\Player\\AppData\\Local".to_string(),
            win_local_app_data_low: "C:\\Users\\Player\\AppData\\LocalLow".to_string(),
            win_documents: "C:\\Users\\Player\\Documents".to_string(),
            win_public: "C:\\Users\\Public".to_string(),
            win_program_data: "C:\\ProgramData".to_string(),
            home: "C:\\Users\\Player".to_string(),
            os_user_name: "Player".to_string(),
            base_dir: "D:\\Games\\TestGame".to_string(),
            store_user_ids: vec!["76561198000000001".to_string()],
        };

        let expanded = expand_ludusavi_template("<winAppData>/EldenRing/<storeUserId>/*.sl2", &env);
        assert_eq!(
            expanded,
            vec![PathBuf::from("C:\\Users\\Player\\AppData\\Roaming\\EldenRing\\76561198000000001")]
        );

        let base_expanded = expand_ludusavi_template("<base>/userdata/*", &env);
        assert_eq!(base_expanded, vec![PathBuf::from("D:\\Games\\TestGame\\userdata")]);
    }
}

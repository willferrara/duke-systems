use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UnifiedAchievementItem {
    pub api_name: String,
    pub display_name: String,
    pub description: String,
    pub unlocked: bool,
    pub unlocked_at: Option<u64>,
    pub icon_url: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UnifiedAchievementsReport {
    pub game_id: String,
    pub game_title: String,
    pub total_count: usize,
    pub unlocked_count: usize,
    pub completion_percent: f64,
    pub sources: Vec<String>,
    pub custom_path: String,
    pub achievements: Vec<UnifiedAchievementItem>,
    pub recent_unlocks: Vec<UnifiedAchievementItem>,
    pub status_message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AchievementUnlockEvent {
    pub game_id: String,
    pub game_title: String,
    pub api_name: String,
    pub display_name: String,
    pub description: String,
    pub icon_url: Option<String>,
    pub unlocked_at: u64,
    pub source: String,
}

struct WatcherControl {
    stop_tx: std::sync::mpsc::Sender<()>,
    #[cfg(target_os = "windows")]
    stop_event_raw: usize,
}

static ACTIVE_WATCHERS: OnceLock<Mutex<HashMap<String, WatcherControl>>> = OnceLock::new();

fn watchers_map() -> &'static Mutex<HashMap<String, WatcherControl>> {
    ACTIVE_WATCHERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn normalize_ach_key(raw: &str) -> String {
    raw.trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect::<String>()
        .to_ascii_lowercase()
}

fn truthy_flag(val: Option<&Value>) -> bool {
    match val {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_u64().unwrap_or(0) > 0 || n.as_f64().unwrap_or(0.0) > 0.0,
        Some(Value::String(s)) => matches!(
            s.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "unlocked" | "earned"
        ),
        _ => false,
    }
}

fn truthy_ini_value(raw: &str) -> bool {
    matches!(
        raw.trim().trim_matches('"').to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "unlocked" | "earned"
    )
}

fn parse_u64_flex(val: Option<&Value>) -> Option<u64> {
    match val {
        Some(Value::Number(n)) => n.as_u64().filter(|&v| v > 0),
        Some(Value::String(s)) => s.trim().parse::<u64>().ok().filter(|&v| v > 0),
        _ => None,
    }
}

pub fn parse_local_achievements_json(raw: &str) -> Vec<UnifiedAchievementItem> {
    let Ok(val) = serde_json::from_str::<Value>(raw) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let parse_obj = |api_hint: &str, obj: &serde_json::Map<String, Value>| -> Option<UnifiedAchievementItem> {
        let api_name = obj
            .get("apiname")
            .or_else(|| obj.get("api_name"))
            .or_else(|| obj.get("id"))
            .or_else(|| obj.get("name"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(api_hint)
            .to_string();
        if api_name.is_empty() {
            return None;
        }
        let display_name = obj
            .get("displayName")
            .or_else(|| obj.get("display_name"))
            .or_else(|| obj.get("title"))
            .or_else(|| obj.get("name"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(&api_name)
            .to_string();
        let description = obj
            .get("description")
            .or_else(|| obj.get("desc"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .trim()
            .to_string();
        let unlocked = truthy_flag(
            obj.get("earned")
                .or_else(|| obj.get("achieved"))
                .or_else(|| obj.get("unlocked"))
                .or_else(|| obj.get("achieved_state"))
                .or_else(|| obj.get("state")),
        );
        let unlocked_at = parse_u64_flex(
            obj.get("earned_time")
                .or_else(|| obj.get("earnedTime"))
                .or_else(|| obj.get("unlock_time"))
                .or_else(|| obj.get("unlocktime"))
                .or_else(|| obj.get("timestamp"))
                .or_else(|| obj.get("time")),
        );
        let icon_url = obj
            .get("icon")
            .or_else(|| obj.get("icon_url"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| s.starts_with("http"))
            .map(ToString::to_string);
        Some(UnifiedAchievementItem {
            api_name,
            display_name,
            description,
            unlocked,
            unlocked_at,
            icon_url,
            source: "Local".to_string(),
        })
    };

    if let Some(arr) = val.as_array() {
        for item in arr {
            if let Some(obj) = item.as_object() {
                if let Some(parsed) = parse_obj("", obj) {
                    out.push(parsed);
                }
            }
        }
    } else if let Some(obj) = val.as_object() {
        if let Some(arr) = obj.get("achievements").and_then(|v| v.as_array()) {
            for item in arr {
                if let Some(sub) = item.as_object() {
                    if let Some(parsed) = parse_obj("", sub) {
                        out.push(parsed);
                    }
                }
            }
        } else {
            for (k, v) in obj {
                if let Some(sub) = v.as_object() {
                    if let Some(parsed) = parse_obj(k, sub) {
                        out.push(parsed);
                    }
                } else if let Some(b) = v.as_bool() {
                    out.push(UnifiedAchievementItem {
                        api_name: k.clone(),
                        display_name: k.clone(),
                        description: String::new(),
                        unlocked: b,
                        unlocked_at: None,
                        icon_url: None,
                        source: "Local".to_string(),
                    });
                }
            }
        }
    }
    out
}

pub fn parse_local_achievements_ini(raw: &str) -> Vec<UnifiedAchievementItem> {
    let mut out = Vec::new();
    let mut section = String::new();
    let mut kv: HashMap<String, String> = HashMap::new();
    let mut flat_unlocked: Vec<(String, bool, Option<u64>)> = Vec::new();

    let flush_section = |sec_name: &str,
                         fields: &HashMap<String, String>,
                         acc: &mut Vec<UnifiedAchievementItem>| {
        let s_clean = sec_name.trim();
        if s_clean.is_empty()
            || s_clean.eq_ignore_ascii_case("SteamAchievements")
            || s_clean.eq_ignore_ascii_case("Achievements")
            || s_clean.eq_ignore_ascii_case("General")
        {
            return;
        }
        let unlocked = fields
            .get("achieved")
            .or_else(|| fields.get("unlocked"))
            .or_else(|| fields.get("earned"))
            .or_else(|| fields.get("state"))
            .map(|v| truthy_ini_value(v))
            .unwrap_or(false);
        let unlocked_at = fields
            .get("unlocktime")
            .or_else(|| fields.get("unlock_time"))
            .or_else(|| fields.get("timestamp"))
            .or_else(|| fields.get("time"))
            .and_then(|v| v.trim().parse::<u64>().ok())
            .filter(|&ts| ts > 0);
        let display_name = fields
            .get("name")
            .or_else(|| fields.get("displayname"))
            .filter(|v| !v.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| s_clean.to_string());
        let description = fields.get("description").cloned().unwrap_or_default();
        acc.push(UnifiedAchievementItem {
            api_name: s_clean.to_string(),
            display_name,
            description,
            unlocked,
            unlocked_at,
            icon_url: None,
            source: "Local".to_string(),
        });
    };

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') && trimmed.len() > 2 {
            flush_section(&section, &kv, &mut out);
            kv.clear();
            section = trimmed[1..trimmed.len() - 1].trim().to_string();
            continue;
        }
        if let Some((k, v)) = trimmed.split_once('=') {
            let key = k.trim();
            let val = v.trim().trim_matches('"');
            let key_lower = key.to_ascii_lowercase();
            if section.eq_ignore_ascii_case("SteamAchievements")
                || section.eq_ignore_ascii_case("Achievements")
            {
                if key_lower == "count" {
                    continue;
                }
                if key_lower == "unlocked" || key_lower == "unlock" {
                    for part in val.split([',', ';']) {
                        let id = part.trim();
                        if !id.is_empty() {
                            flat_unlocked.push((id.to_string(), true, None));
                        }
                    }
                } else {
                    flat_unlocked.push((key.to_string(), truthy_ini_value(val), None));
                }
            } else {
                kv.insert(key_lower, val.to_string());
            }
        }
    }
    flush_section(&section, &kv, &mut out);

    for (api_name, unlocked, unlocked_at) in flat_unlocked {
        if !out
            .iter()
            .any(|item| item.api_name.eq_ignore_ascii_case(&api_name))
        {
            out.push(UnifiedAchievementItem {
                display_name: api_name.clone(),
                api_name,
                description: String::new(),
                unlocked,
                unlocked_at,
                icon_url: None,
                source: "Local".to_string(),
            });
        }
    }
    out
}

pub fn parse_achievement_file(path: &Path) -> Vec<UnifiedAchievementItem> {
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if ext == "json" || raw.trim_start().starts_with('{') || raw.trim_start().starts_with('[') {
        parse_local_achievements_json(&raw)
    } else {
        parse_local_achievements_ini(&raw)
    }
}

fn collect_candidate_achievement_files(game: &crate::GameEntry) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut watch_dirs: Vec<PathBuf> = Vec::new();
    let mut candidate_files: Vec<PathBuf> = Vec::new();

    let custom = game.achievements_path.trim();
    if !custom.is_empty() {
        let p = PathBuf::from(custom);
        if p.is_file() {
            candidate_files.push(p.clone());
            if let Some(parent) = p.parent() {
                watch_dirs.push(parent.to_path_buf());
            }
        } else if p.is_dir() {
            watch_dirs.push(p);
        }
    }

    let game_root = if !game.install_dir.trim().is_empty() {
        PathBuf::from(game.install_dir.trim())
    } else {
        crate::resolve_game_root_dir(Path::new(game.exe_path.trim()))
    };

    if game_root.is_dir() {
        watch_dirs.push(game_root.clone());
        let profile_dir = game_root.join(".duke-profile");
        if profile_dir.is_dir() {
            watch_dirs.push(profile_dir);
        }
        let steam_settings = game_root.join("steam_settings");
        if steam_settings.is_dir() {
            watch_dirs.push(steam_settings);
        }
    }

    if let Some(aid) = game.steam_app_id.filter(|&v| v > 0) {
        let aid_str = aid.to_string();
        for env_key in ["APPDATA", "LOCALAPPDATA", "PUBLIC"] {
            if let Ok(base) = std::env::var(env_key) {
                let b = PathBuf::from(base);
                for rel in [
                    format!("Goldberg SteamEmu Saves\\{aid_str}"),
                    format!("GSE Saves\\{aid_str}"),
                    format!("Steam\\CODEX\\{aid_str}"),
                    format!("Documents\\Steam\\CODEX\\{aid_str}"),
                    format!("Documents\\Steam\\RUNE\\{aid_str}"),
                ] {
                    let dir = b.join(rel);
                    if dir.is_dir() {
                        watch_dirs.push(dir);
                    }
                }
            }
        }
    }

    let mut seen_dirs = HashSet::new();
    watch_dirs.retain(|d| {
        let key = d.to_string_lossy().to_ascii_lowercase();
        d.is_dir() && seen_dirs.insert(key)
    });

    for dir in &watch_dirs {
        for fname in [
            "achievements.json",
            "achievements.ini",
            "stats.ini",
            "steam_achievements.json",
            "steam_achievements.ini",
            "achiev.ini",
        ] {
            let candidate = dir.join(fname);
            if candidate.is_file() {
                candidate_files.push(candidate);
            }
        }
        let sub_stats = dir.join("remote").join("stats.ini");
        if sub_stats.is_file() {
            candidate_files.push(sub_stats);
        }
        let sub_ach = dir.join("steam_settings").join("achievements.json");
        if sub_ach.is_file() {
            candidate_files.push(sub_ach);
        }
    }

    let mut seen_files = HashSet::new();
    candidate_files.retain(|f| {
        let key = f.to_string_lossy().to_ascii_lowercase();
        f.is_file() && seen_files.insert(key)
    });

    (watch_dirs, candidate_files)
}

fn scan_local_achievements(game: &crate::GameEntry) -> Vec<UnifiedAchievementItem> {
    let (watch_dirs, mut files) = collect_candidate_achievement_files(game);
    // Catch any achievements.json / stats.ini placed 1-2 levels under watch dirs
    for dir in &watch_dirs {
        for entry in walkdir::WalkDir::new(dir)
            .max_depth(3)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if matches!(
                name.as_str(),
                "achievements.json"
                    | "achievements.ini"
                    | "stats.ini"
                    | "steam_achievements.json"
                    | "steam_achievements.ini"
                    | "achiev.ini"
            ) {
                files.push(entry.into_path());
            }
        }
    }

    let mut seen_files = HashSet::new();
    let mut merged: HashMap<String, UnifiedAchievementItem> = HashMap::new();
    for path in files {
        let pkey = path.to_string_lossy().to_ascii_lowercase();
        if !seen_files.insert(pkey) {
            continue;
        }
        for item in parse_achievement_file(&path) {
            let key = normalize_ach_key(&item.api_name);
            if key.is_empty() {
                continue;
            }
            merged
                .entry(key)
                .and_modify(|existing| {
                    if item.unlocked && !existing.unlocked {
                        existing.unlocked = true;
                    }
                    if existing.unlocked_at.is_none() {
                        existing.unlocked_at = item.unlocked_at;
                    }
                    if existing.display_name == existing.api_name && !item.display_name.is_empty() {
                        existing.display_name = item.display_name.clone();
                    }
                    if existing.description.is_empty() && !item.description.is_empty() {
                        existing.description = item.description.clone();
                    }
                })
                .or_insert(item);
        }
    }
    merged.into_values().collect()
}

pub fn diff_new_unlocks(
    baseline: &mut HashSet<String>,
    current: &[UnifiedAchievementItem],
) -> Vec<UnifiedAchievementItem> {
    let mut newly_unlocked = Vec::new();
    for item in current {
        if !item.unlocked {
            continue;
        }
        let key = normalize_ach_key(&item.api_name);
        if !key.is_empty() && baseline.insert(key) {
            newly_unlocked.push(item.clone());
        }
    }
    newly_unlocked
}

fn emit_new_unlocks(
    app: &AppHandle,
    game_id: &str,
    game_title: &str,
    items: Vec<UnifiedAchievementItem>,
) {
    let fallback_ts = now_unix_secs();
    for item in items {
        let payload = AchievementUnlockEvent {
            game_id: game_id.to_string(),
            game_title: game_title.to_string(),
            api_name: item.api_name,
            display_name: item.display_name,
            description: item.description,
            icon_url: item.icon_url,
            unlocked_at: item.unlocked_at.unwrap_or(fallback_ts),
            source: item.source,
        };
        let _ = app.emit("achievement-unlocked", &payload);
    }
}

pub fn start_achievement_watcher(app: AppHandle, game: &crate::GameEntry) {
    stop_achievement_watcher(&game.id);

    let (watch_dirs, _) = collect_candidate_achievement_files(game);
    if watch_dirs.is_empty() {
        return;
    }

    let mut baseline: HashSet<String> = scan_local_achievements(game)
        .into_iter()
        .filter(|a| a.unlocked)
        .map(|a| normalize_ach_key(&a.api_name))
        .filter(|k| !k.is_empty())
        .collect();

    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
    let game_clone = game.clone();

    #[cfg(target_os = "windows")]
    let stop_event_raw: usize = unsafe {
        use windows::Win32::System::Threading::CreateEventW;
        CreateEventW(None, true, false, None)
            .map(|h| h.0 as usize)
            .unwrap_or(0)
    };
    #[cfg(not(target_os = "windows"))]
    let _stop_event_raw = 0usize;

    {
        let mut map = watchers_map().lock().unwrap();
        map.insert(
            game.id.clone(),
            WatcherControl {
                stop_tx,
                #[cfg(target_os = "windows")]
                stop_event_raw,
            },
        );
    }

    std::thread::Builder::new()
        .name(format!("duke-ach-watch-{}", game.id))
        .spawn(move || {
            #[cfg(target_os = "windows")]
            {
                run_win32_dir_watcher(
                    &app,
                    &game_clone,
                    &watch_dirs,
                    &mut baseline,
                    stop_event_raw,
                    stop_rx,
                );
                if stop_event_raw != 0 {
                    unsafe {
                        use windows::Win32::Foundation::{CloseHandle, HANDLE};
                        let _ = CloseHandle(HANDLE(stop_event_raw as *mut std::ffi::c_void));
                    }
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = (app, game_clone, watch_dirs, baseline, stop_rx);
            }
        })
        .ok();
}

pub fn stop_achievement_watcher(game_id: &str) {
    let control = watchers_map().lock().unwrap().remove(game_id);
    if let Some(ctrl) = control {
        let _ = ctrl.stop_tx.send(());
        #[cfg(target_os = "windows")]
        if ctrl.stop_event_raw != 0 {
            unsafe {
                use windows::Win32::Foundation::HANDLE;
                use windows::Win32::System::Threading::SetEvent;
                let _ = SetEvent(HANDLE(ctrl.stop_event_raw as *mut std::ffi::c_void));
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn run_win32_dir_watcher(
    app: &AppHandle,
    game: &crate::GameEntry,
    watch_dirs: &[PathBuf],
    baseline: &mut HashSet<String>,
    stop_event_raw: usize,
    stop_rx: std::sync::mpsc::Receiver<()>,
) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, ReadDirectoryChangesW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED,
        FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE,
        FILE_NOTIFY_INFORMATION, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        OPEN_EXISTING,
    };
    use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
    use windows::Win32::System::Threading::{CreateEventW, ResetEvent, WaitForMultipleObjects};

    struct DirWatchHandle {
        dir_handle: HANDLE,
        io_event: HANDLE,
        overlapped: Box<OVERLAPPED>,
        buffer: Vec<u8>,
    }

    let mut handles: Vec<DirWatchHandle> = Vec::new();
    for dir in watch_dirs.iter().take(12) {
        let mut wide: Vec<u16> = OsStr::new(dir).encode_wide().collect();
        wide.push(0);
        let dir_handle = unsafe {
            CreateFileW(
                PCWSTR(wide.as_ptr()),
                FILE_LIST_DIRECTORY.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
                None,
            )
        };
        let Ok(dh) = dir_handle else {
            continue;
        };
        let Ok(ev) = (unsafe { CreateEventW(None, true, false, None) }) else {
            unsafe {
                let _ = CloseHandle(dh);
            }
            continue;
        };
        let mut overlapped = Box::new(OVERLAPPED::default());
        overlapped.hEvent = ev;
        let mut buffer = vec![0u8; 16 * 1024];
        let issue_ok = unsafe {
            ReadDirectoryChangesW(
                dh,
                buffer.as_mut_ptr() as *mut std::ffi::c_void,
                buffer.len() as u32,
                true,
                FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE,
                None,
                Some(&mut *overlapped),
                None,
            )
            .is_ok()
        };
        if !issue_ok {
            unsafe {
                let _ = CloseHandle(ev);
                let _ = CloseHandle(dh);
            }
            continue;
        }
        handles.push(DirWatchHandle {
            dir_handle: dh,
            io_event: ev,
            overlapped,
            buffer,
        });
    }

    if handles.is_empty() || stop_event_raw == 0 {
        while stop_rx.recv_timeout(Duration::from_secs(3)).is_err() {
            let current = scan_local_achievements(game);
            let unlocked = diff_new_unlocks(baseline, &current);
            if !unlocked.is_empty() {
                emit_new_unlocks(app, &game.id, &game.title, unlocked);
            }
        }
        return;
    }

    let stop_handle = HANDLE(stop_event_raw as *mut std::ffi::c_void);
    let notify_filter = FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE;

    loop {
        if stop_rx.try_recv().is_ok() {
            break;
        }
        let mut wait_list = Vec::with_capacity(handles.len() + 1);
        wait_list.push(stop_handle);
        for item in &handles {
            wait_list.push(item.io_event);
        }
        let wait_res = unsafe { WaitForMultipleObjects(&wait_list, false, 3000) };
        if wait_res.0 == WAIT_OBJECT_0.0 {
            break;
        }
        let idx_offset = wait_res.0.wrapping_sub(WAIT_OBJECT_0.0 + 1) as usize;
        if idx_offset < handles.len() {
            let item = &mut handles[idx_offset];
            let mut bytes_transferred = 0u32;
            let got = unsafe {
                GetOverlappedResult(
                    item.dir_handle,
                    &*item.overlapped,
                    &mut bytes_transferred,
                    false,
                )
            };
            let mut should_rescan = false;
            if got.is_ok() && bytes_transferred as usize >= std::mem::size_of::<FILE_NOTIFY_INFORMATION>() {
                let mut offset = 0usize;
                let max_bytes = (bytes_transferred as usize).min(item.buffer.len());
                while offset + std::mem::size_of::<FILE_NOTIFY_INFORMATION>() <= max_bytes {
                    let info = unsafe {
                        &*(item.buffer.as_ptr().add(offset) as *const FILE_NOTIFY_INFORMATION)
                    };
                    let name_bytes = info.FileNameLength as usize;
                    let name_start = offset + std::mem::size_of::<FILE_NOTIFY_INFORMATION>() - 2;
                    if name_bytes > 0 && name_start + name_bytes <= max_bytes {
                        let units = name_bytes / 2;
                        let mut chars = Vec::with_capacity(units);
                        for u in 0..units {
                            let lo = item.buffer[name_start + u * 2];
                            let hi = item.buffer[name_start + u * 2 + 1];
                            chars.push(u16::from_le_bytes([lo, hi]));
                        }
                        let fname = String::from_utf16_lossy(&chars).to_ascii_lowercase();
                        if fname.ends_with(".json") || fname.ends_with(".ini") {
                            should_rescan = true;
                        }
                    }
                    if info.NextEntryOffset == 0 {
                        break;
                    }
                    offset += info.NextEntryOffset as usize;
                }
            } else {
                should_rescan = true;
            }

            unsafe {
                let _ = ResetEvent(item.io_event);
                let _ = ReadDirectoryChangesW(
                    item.dir_handle,
                    item.buffer.as_mut_ptr() as *mut std::ffi::c_void,
                    item.buffer.len() as u32,
                    true,
                    notify_filter,
                    None,
                    Some(&mut *item.overlapped),
                    None,
                );
            }

            if should_rescan {
                std::thread::sleep(Duration::from_millis(80));
                let current = scan_local_achievements(game);
                let unlocked = diff_new_unlocks(baseline, &current);
                if !unlocked.is_empty() {
                    emit_new_unlocks(app, &game.id, &game.title, unlocked);
                }
            }
        }
    }

    // Cancel pending async directory I/O and drain OVERLAPPED before dropping heap buffers.
    for item in &mut handles {
        unsafe {
            let _ = CancelIoEx(item.dir_handle, Some(&*item.overlapped));
            let mut drained = 0u32;
            let _ = GetOverlappedResult(item.dir_handle, &*item.overlapped, &mut drained, true);
            let _ = CloseHandle(item.io_event);
            let _ = CloseHandle(item.dir_handle);
        }
    }
}

async fn fetch_steam_achievements(
    steam_key: &str,
    steam_id: &str,
    app_id: u32,
) -> Result<Vec<UnifiedAchievementItem>, String> {
    let schema_url = format!(
        "https://api.steampowered.com/ISteamUserStats/GetSchemaForGame/v2/?key={}&appid={}",
        urlencoding::encode(steam_key),
        app_id
    );
    let player_url = format!(
        "https://api.steampowered.com/ISteamUserStats/GetPlayerAchievements/v1/?key={}&steamid={}&appid={}&l=english",
        urlencoding::encode(steam_key),
        urlencoding::encode(steam_id),
        app_id
    );

    let (schema_res, player_res) = tokio::join!(
        crate::friends::fetch_doh_json_redacted(&schema_url, &[], "Steam Schema"),
        crate::friends::fetch_doh_json_redacted(&player_url, &[], "Steam PlayerAchievements")
    );

    let mut schema_map: HashMap<String, (String, String, Option<String>)> = HashMap::new();
    if let Ok(schema_json) = schema_res {
        if let Some(arr) = schema_json
            .pointer("/game/availableGameStats/achievements")
            .and_then(|v| v.as_array())
        {
            for item in arr {
                let api = item
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .trim();
                if api.is_empty() {
                    continue;
                }
                let disp = item
                    .get("displayName")
                    .and_then(|v| v.as_str())
                    .unwrap_or(api)
                    .trim()
                    .to_string();
                let desc = item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                let icon = item
                    .get("icon")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| s.starts_with("http"))
                    .map(ToString::to_string);
                schema_map.insert(normalize_ach_key(api), (disp, desc, icon));
            }
        }
    }

    let player_json = player_res?;
    let mut items = Vec::new();
    if let Some(arr) = player_json
        .pointer("/playerstats/achievements")
        .and_then(|v| v.as_array())
    {
        for entry in arr {
            let api = entry
                .get("apiname")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .trim();
            if api.is_empty() {
                continue;
            }
            let unlocked = entry.get("achieved").and_then(|v| v.as_u64()).unwrap_or(0) == 1;
            let unlocked_at = entry
                .get("unlocktime")
                .and_then(|v| v.as_u64())
                .filter(|&ts| ts > 0);
            let meta = schema_map.get(&normalize_ach_key(api));
            let display_name = entry
                .get("name")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(ToString::to_string)
                .or_else(|| meta.map(|m| m.0.clone()))
                .unwrap_or_else(|| api.to_string());
            let description = entry
                .get("description")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(ToString::to_string)
                .or_else(|| meta.map(|m| m.1.clone()))
                .unwrap_or_default();
            let icon_url = meta.and_then(|m| m.2.clone());
            items.push(UnifiedAchievementItem {
                api_name: api.to_string(),
                display_name,
                description,
                unlocked,
                unlocked_at,
                icon_url,
                source: "Steam".to_string(),
            });
        }
    }
    Ok(items)
}

async fn fetch_xbox_achievements_for_title(
    xbox_key: &str,
    game_title: &str,
) -> Result<Vec<UnifiedAchievementItem>, String> {
    let titles_json = crate::friends::fetch_doh_json_redacted(
        "https://xbl.io/api/v2/achievements",
        &[("X-Authorization", xbox_key)],
        "Xbox Achievements",
    )
    .await?;

    let target = normalize_ach_key(&crate::clean_game_title(game_title));
    let titles = titles_json
        .get("titles")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let title_id = titles.into_iter().find_map(|t| {
        let name = t.get("name").and_then(|v| v.as_str()).unwrap_or_default();
        let nkey = normalize_ach_key(&crate::clean_game_title(name));
        if !target.is_empty() && (nkey == target || nkey.contains(&target) || target.contains(&nkey)) {
            t.get("titleId")
                .and_then(|v| v.as_str().map(ToString::to_string).or_else(|| v.as_u64().map(|n| n.to_string())))
        } else {
            None
        }
    });

    let Some(tid) = title_id else {
        return Ok(Vec::new());
    };
    let detail_url = format!("https://xbl.io/api/v2/achievements/title/{}", urlencoding::encode(&tid));
    let detail_json = crate::friends::fetch_doh_json_redacted(
        &detail_url,
        &[("X-Authorization", xbox_key)],
        "Xbox Title Achievements",
    )
    .await?;

    let mut out = Vec::new();
    if let Some(arr) = detail_json.get("achievements").and_then(|v| v.as_array()) {
        for item in arr {
            let api_name = item
                .get("id")
                .and_then(|v| v.as_str().map(ToString::to_string).or_else(|| v.as_u64().map(|n| n.to_string())))
                .unwrap_or_default();
            let display_name = item
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or(&api_name)
                .to_string();
            let description = item
                .get("description")
                .or_else(|| item.get("lockedDescription"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let progress_state = item
                .get("progressState")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let unlocked = progress_state.eq_ignore_ascii_case("Achieved");
            let icon_url = item
                .get("mediaAssets")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|m| m.get("url"))
                .and_then(|v| v.as_str())
                .filter(|s| s.starts_with("http"))
                .map(ToString::to_string);
            out.push(UnifiedAchievementItem {
                api_name,
                display_name,
                description,
                unlocked,
                unlocked_at: None,
                icon_url,
                source: "Xbox".to_string(),
            });
        }
    }
    Ok(out)
}

async fn fetch_psn_trophies_for_title(
    npsso: &str,
    game_title: &str,
) -> Result<Vec<UnifiedAchievementItem>, String> {
    let bearer = crate::friends::fetch_psn_access_token(npsso).await?;
    let auth_header = format!("Bearer {bearer}");
    let titles_json = crate::friends::fetch_doh_json_redacted(
        "https://m.np.playstation.com/api/trophy/v1/users/me/trophyTitles?limit=80",
        &[("Authorization", auth_header.as_str())],
        "PSN Trophy Titles",
    )
    .await?;

    let target = normalize_ach_key(&crate::clean_game_title(game_title));
    let match_meta = titles_json
        .get("trophyTitles")
        .and_then(|v| v.as_array())
        .and_then(|arr| {
            arr.iter().find_map(|entry| {
                let name = entry
                    .get("trophyTitleName")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let key = normalize_ach_key(&crate::clean_game_title(name));
                if !target.is_empty() && (key == target || key.contains(&target) || target.contains(&key)) {
                    let np_id = entry.get("npCommunicationId").and_then(|v| v.as_str())?;
                    let service = entry
                        .get("npServiceName")
                        .and_then(|v| v.as_str())
                        .unwrap_or("trophy");
                    Some((np_id.to_string(), service.to_string()))
                } else {
                    None
                }
            })
        });

    let Some((np_id, service)) = match_meta else {
        return Ok(Vec::new());
    };

    let def_url = format!(
        "https://m.np.playstation.com/api/trophy/v1/npCommunicationIds/{}/trophyGroups/all/trophies?npServiceName={}",
        urlencoding::encode(&np_id),
        urlencoding::encode(&service)
    );
    let earned_url = format!(
        "https://m.np.playstation.com/api/trophy/v1/users/me/npCommunicationIds/{}/trophyGroups/all/trophies?npServiceName={}",
        urlencoding::encode(&np_id),
        urlencoding::encode(&service)
    );
    let hdrs = [("Authorization", auth_header.as_str())];
    let (def_res, earned_res) = tokio::join!(
        crate::friends::fetch_doh_json_redacted(&def_url, &hdrs, "PSN Trophy Meta"),
        crate::friends::fetch_doh_json_redacted(&earned_url, &hdrs, "PSN Trophies Earned")
    );

    let mut meta_map: HashMap<u64, (String, String, Option<String>)> = HashMap::new();
    if let Ok(def_json) = def_res {
        if let Some(arr) = def_json.get("trophies").and_then(|v| v.as_array()) {
            for t in arr {
                if let Some(tid) = t.get("trophyId").and_then(|v| v.as_u64()) {
                    let name = t
                        .get("trophyName")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let desc = t
                        .get("trophyDetail")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let icon = t
                        .get("trophyIconUrl")
                        .and_then(|v| v.as_str())
                        .filter(|s| s.starts_with("http"))
                        .map(ToString::to_string);
                    meta_map.insert(tid, (name, desc, icon));
                }
            }
        }
    }

    let earned_json = earned_res?;
    let mut out = Vec::new();
    if let Some(arr) = earned_json.get("trophies").and_then(|v| v.as_array()) {
        for t in arr {
            let tid = t.get("trophyId").and_then(|v| v.as_u64()).unwrap_or(0);
            let unlocked = t.get("earned").and_then(|v| v.as_bool()).unwrap_or(false);
            let meta = meta_map.get(&tid);
            out.push(UnifiedAchievementItem {
                api_name: format!("psn_trophy_{tid}"),
                display_name: meta
                    .map(|m| m.0.clone())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| format!("Trophy #{tid}")),
                description: meta.map(|m| m.1.clone()).unwrap_or_default(),
                unlocked,
                unlocked_at: None,
                icon_url: meta.and_then(|m| m.2.clone()),
                source: "PSN".to_string(),
            });
        }
    }
    Ok(out)
}

#[tauri::command]
pub async fn get_game_achievements(
    app: AppHandle,
    game_id: String,
) -> Result<UnifiedAchievementsReport, String> {
    let library = crate::load_library_from_disk(&app)?;
    let game = library
        .iter()
        .find(|g| g.id == game_id)
        .cloned()
        .ok_or_else(|| "Game not found in library".to_string())?;
    let settings = crate::load_settings_standalone();

    let local_items = tokio::task::spawn_blocking({
        let g = game.clone();
        move || scan_local_achievements(&g)
    })
    .await
    .unwrap_or_default();

    let steam_key = settings
        .steam_api_key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    let steam_id = crate::friends::resolve_effective_steam_id64(&settings);
    let xbox_key = settings
        .xbox_api_key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    let psn_npsso = settings
        .psn_npsso
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);

    let steam_fut = async {
        match (steam_key, steam_id, game.steam_app_id) {
            (Some(k), Some(sid), Some(aid)) if aid > 0 && aid <= u32::MAX as u64 => {
                fetch_steam_achievements(&k, &sid, aid as u32).await.ok()
            }
            _ => None,
        }
    };
    let xbox_fut = async {
        match xbox_key {
            Some(k) => fetch_xbox_achievements_for_title(&k, &game.title).await.ok(),
            None => None,
        }
    };
    let psn_fut = async {
        match psn_npsso {
            Some(tok) => fetch_psn_trophies_for_title(&tok, &game.title).await.ok(),
            None => None,
        }
    };

    let (steam_opt, xbox_opt, psn_opt) = tokio::join!(steam_fut, xbox_fut, psn_fut);

    let mut sources: Vec<String> = Vec::new();
    let mut unified: HashMap<String, UnifiedAchievementItem> = HashMap::new();

    if !local_items.is_empty() {
        sources.push("Local".to_string());
    }

    let merge_batch = |batch: Vec<UnifiedAchievementItem>,
                       target: &mut HashMap<String, UnifiedAchievementItem>| {
        for item in batch {
            let key = normalize_ach_key(&item.api_name);
            let alt_key = normalize_ach_key(&item.display_name);
            let existing_key = if target.contains_key(&key) {
                key.clone()
            } else if !alt_key.is_empty() && target.contains_key(&alt_key) {
                alt_key
            } else {
                key
            };
            target
                .entry(existing_key)
                .and_modify(|cur| {
                    cur.unlocked |= item.unlocked;
                    if cur.unlocked_at.is_none() {
                        cur.unlocked_at = item.unlocked_at;
                    }
                    if cur.icon_url.is_none() {
                        cur.icon_url = item.icon_url.clone();
                    }
                    if cur.description.is_empty() {
                        cur.description = item.description.clone();
                    }
                    if !item.display_name.is_empty() && cur.display_name == cur.api_name {
                        cur.display_name = item.display_name.clone();
                    }
                })
                .or_insert(item);
        }
    };

    if let Some(steam_items) = steam_opt {
        if !steam_items.is_empty() {
            sources.push("Steam".to_string());
            merge_batch(steam_items, &mut unified);
        }
    }
    merge_batch(local_items, &mut unified);
    if let Some(xbox_items) = xbox_opt {
        if !xbox_items.is_empty() {
            sources.push("Xbox".to_string());
            merge_batch(xbox_items, &mut unified);
        }
    }
    if let Some(psn_items) = psn_opt {
        if !psn_items.is_empty() {
            sources.push("PSN".to_string());
            merge_batch(psn_items, &mut unified);
        }
    }

    let mut list: Vec<UnifiedAchievementItem> = unified.into_values().collect();
    list.sort_by(|a, b| {
        b.unlocked
            .cmp(&a.unlocked)
            .then_with(|| b.unlocked_at.unwrap_or(0).cmp(&a.unlocked_at.unwrap_or(0)))
            .then_with(|| a.display_name.cmp(&b.display_name))
    });

    let total_count = list.len();
    let unlocked_count = list.iter().filter(|a| a.unlocked).count();
    let completion_percent = if total_count > 0 {
        ((unlocked_count as f64 / total_count as f64) * 1000.0).round() / 10.0
    } else {
        0.0
    };

    let recent_unlocks: Vec<UnifiedAchievementItem> = list
        .iter()
        .filter(|a| a.unlocked)
        .take(6)
        .cloned()
        .collect();

    let status_message = if total_count == 0 {
        "No local achievements.json / stats.ini or linked storefront achievement data found."
            .to_string()
    } else {
        format!(
            "{unlocked_count} / {total_count} unlocked ({:.1}%) via {}",
            completion_percent,
            sources.join(" + ")
        )
    };

    Ok(UnifiedAchievementsReport {
        game_id: game.id,
        game_title: game.title,
        total_count,
        unlocked_count,
        completion_percent,
        sources,
        custom_path: game.achievements_path,
        achievements: list,
        recent_unlocks,
        status_message,
    })
}

#[tauri::command]
pub async fn set_game_achievements_path(
    app: AppHandle,
    game_id: String,
    achievements_path: String,
) -> Result<Vec<crate::GameEntry>, String> {
    let mut library = crate::load_library_from_disk(&app)?;
    let entry = library
        .iter_mut()
        .find(|g| g.id == game_id)
        .ok_or_else(|| "Game not found".to_string())?;
    entry.achievements_path = achievements_path.trim().to_string();
    crate::save_library_to_disk(&app, &library)?;
    Ok(library)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_goldberg_style_achievements_json_and_diffs() {
        let raw = r#"{
            "ACH_FIRST_BLOOD": {
                "earned": true,
                "earned_time": 1700001234,
                "displayName": "First Steps",
                "description": "Complete the tutorial"
            },
            "ACH_MASTER": {
                "earned": false,
                "earned_time": 0,
                "displayName": "Grand Master"
            }
        }"#;
        let items = parse_local_achievements_json(raw);
        assert_eq!(items.len(), 2);
        let first = items.iter().find(|i| i.api_name == "ACH_FIRST_BLOOD").unwrap();
        assert!(first.unlocked);
        assert_eq!(first.unlocked_at, Some(1700001234));
        assert_eq!(first.display_name, "First Steps");

        let mut baseline = HashSet::new();
        let initial_new = diff_new_unlocks(&mut baseline, &items);
        assert_eq!(initial_new.len(), 1);
        assert_eq!(diff_new_unlocks(&mut baseline, &items).len(), 0);

        let updated = vec![
            first.clone(),
            UnifiedAchievementItem {
                api_name: "ACH_MASTER".to_string(),
                display_name: "Grand Master".to_string(),
                description: String::new(),
                unlocked: true,
                unlocked_at: Some(1700009999),
                icon_url: None,
                source: "Local".to_string(),
            },
        ];
        let unlocked_now = diff_new_unlocks(&mut baseline, &updated);
        assert_eq!(unlocked_now.len(), 1);
        assert_eq!(unlocked_now[0].api_name, "ACH_MASTER");
    }

    #[test]
    fn parses_ini_achievement_sections_and_flat_list() {
        let ini_sec = r#"
[ACH_ALPHA]
Achieved=1
UnlockTime=1711111111
Name=Alpha Explorer
Description=Discover zone A

[ACH_BETA]
Achieved=0
"#;
        let parsed = parse_local_achievements_ini(ini_sec);
        assert_eq!(parsed.len(), 2);
        assert!(parsed.iter().any(|i| i.api_name == "ACH_ALPHA" && i.unlocked && i.unlocked_at == Some(1711111111)));
        assert!(parsed.iter().any(|i| i.api_name == "ACH_BETA" && !i.unlocked));

        let ini_flat = r#"
[SteamAchievements]
Count=2
Unlocked=ACH_ONE,ACH_TWO
"#;
        let parsed_flat = parse_local_achievements_ini(ini_flat);
        assert_eq!(parsed_flat.len(), 2);
        assert!(parsed_flat.iter().all(|i| i.unlocked));
    }
}

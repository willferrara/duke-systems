//! Zero-login background multi-launcher playtime synchronization.
//!
//! Scans local metadata stores for Steam, Xbox App / PC Game Pass, Epic Games,
//! GOG Galaxy, Ubisoft Connect, EA App, and RetroArch on startup and game exit,
//! merging `max(imported_playtime_secs, synced_external_secs) + duke_playtime_secs`
//! non-destructively so Duke session hours are preserved and re-syncing never
//! double-counts.

use crate::importers::{
    collect_epic_local_playtimes, collect_gog_local_playtimes, collect_steam_local_playtimes,
    extract_steam_library_paths_from_vdf, find_steam_root_dirs, normalize_title_playtime_key,
    normalize_win_path_key, parse_steam_appmanifest_acf,
};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use tauri::AppHandle;

const MAX_SCAN_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_PLAYTIME_SECS_SANE: u64 = 100_000 * 3600;
const MIN_VALID_UNIX_TS: u64 = 1_100_000_000;
const MAX_VALID_UNIX_TS: u64 = 4_102_444_800;

static PLAYTIME_SYNC_IN_FLIGHT: OnceLock<AtomicBool> = OnceLock::new();

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExternalPlaytimeRecord {
    pub seconds: u64,
    pub last_played_ts: Option<u64>,
}

impl ExternalPlaytimeRecord {
    fn merge(&mut self, seconds: u64, last_played_ts: Option<u64>) {
        if seconds <= MAX_PLAYTIME_SECS_SANE {
            self.seconds = self.seconds.max(seconds);
        }
        if let Some(lp) = last_played_ts.filter(|&ts| (MIN_VALID_UNIX_TS..=MAX_VALID_UNIX_TS).contains(&ts))
        {
            self.last_played_ts = Some(self.last_played_ts.map_or(lp, |prev| prev.max(lp)));
        }
    }
}

#[derive(Debug, Default)]
pub struct ExternalPlaytimeIndex {
    pub by_steam_app_id: HashMap<u32, ExternalPlaytimeRecord>,
    pub by_title: HashMap<String, ExternalPlaytimeRecord>,
    pub by_exe_path: HashMap<String, ExternalPlaytimeRecord>,
    pub by_install_dir: HashMap<String, ExternalPlaytimeRecord>,
    pub sources_scanned: Vec<String>,
}

impl ExternalPlaytimeIndex {
    fn record_source(&mut self, name: &str) {
        if !self.sources_scanned.iter().any(|s| s == name) {
            self.sources_scanned.push(name.to_string());
        }
    }

    fn insert_steam(&mut self, app_id: u32, secs: u64, lp: Option<u64>) {
        if secs == 0 && lp.is_none() {
            return;
        }
        self.by_steam_app_id
            .entry(app_id)
            .or_default()
            .merge(secs, lp);
    }

    fn insert_title(&mut self, raw_title: &str, secs: u64, lp: Option<u64>) {
        if secs == 0 && lp.is_none() {
            return;
        }
        let raw_key = normalize_title_playtime_key(raw_title);
        if raw_key.len() >= 2 {
            self.by_title.entry(raw_key.clone()).or_default().merge(secs, lp);
        }
        let clean = crate::artwork::clean_cover_query(raw_title);
        let clean_key = normalize_title_playtime_key(&clean);
        if clean_key.len() >= 2 && clean_key != raw_key {
            self.by_title.entry(clean_key).or_default().merge(secs, lp);
        }
    }

    fn insert_exe_path(&mut self, raw_path: &str, secs: u64, lp: Option<u64>) {
        if secs == 0 && lp.is_none() {
            return;
        }
        let key = normalize_win_path_key(raw_path);
        if !key.is_empty() {
            self.by_exe_path.entry(key).or_default().merge(secs, lp);
        }
    }

    fn insert_install_dir(&mut self, raw_dir: &str, secs: u64, lp: Option<u64>) {
        if secs == 0 && lp.is_none() {
            return;
        }
        let key = normalize_win_path_key(raw_dir);
        if !key.is_empty() {
            self.by_install_dir.entry(key).or_default().merge(secs, lp);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlaytimeSyncSummary {
    pub updated_games: usize,
    pub total_imported_secs: u64,
    pub total_duke_secs: u64,
    pub sources_scanned: Vec<String>,
}

fn sanitize_sane_secs(raw: u64) -> Option<u64> {
    if raw > 0 && raw <= MAX_PLAYTIME_SECS_SANE {
        Some(raw)
    } else {
        None
    }
}

fn sanitize_unix_ts(raw: u64) -> Option<u64> {
    if (MIN_VALID_UNIX_TS..=MAX_VALID_UNIX_TS).contains(&raw) {
        Some(raw)
    } else {
        None
    }
}

fn windows_filetime_to_unix_secs(filetime: u64) -> Option<u64> {
    // 100-ns intervals from 1601-01-01 to 1970-01-01 = 116_444_736_000_000_000
    const EPOCH_DIFF_100NS: u64 = 116_444_736_000_000_000;
    if filetime <= EPOCH_DIFF_100NS {
        return None;
    }
    sanitize_unix_ts((filetime - EPOCH_DIFF_100NS) / 10_000_000)
}

/// Converts RetroArch `"H:MM:SS"` or `"HH:MM:SS"` runtime strings to total seconds.
pub fn parse_hms_duration_secs(raw: &str) -> Option<u64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let parts: Vec<&str> = trimmed.split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    let hours: u64 = parts[0].trim().parse().ok()?;
    let mins: u64 = parts[1].trim().parse().ok()?;
    let secs: u64 = parts[2].trim().parse().ok()?;
    if mins >= 60 || secs >= 60 {
        return None;
    }
    sanitize_sane_secs(
        hours
            .saturating_mul(3600)
            .saturating_add(mins.saturating_mul(60))
            .saturating_add(secs),
    )
}

/// Converts `"YYYY-MM-DD HH:MM:SS"` or ISO timestamps into approximate UTC Unix seconds.
pub fn parse_timestamp_utc_secs(raw: &str) -> Option<u64> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(num) = s.parse::<u64>() {
        if num > 100_000_000_000_000 {
            return windows_filetime_to_unix_secs(num);
        }
        if num > 1_000_000_000_000 {
            return sanitize_unix_ts(num / 1000);
        }
        return sanitize_unix_ts(num);
    }
    // Parse "YYYY-MM-DD HH:MM:SS" or "YYYY-MM-DDTHH:MM:SS..."
    let bytes = s.as_bytes();
    if bytes.len() < 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let year: i64 = s[0..4].parse().ok()?;
    let month: u32 = s[5..7].parse().ok()?;
    let day: u32 = s[8..10].parse().ok()?;
    if !(1970..=2100).contains(&year) || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let (hour, min, sec) = if s.len() >= 19 && bytes[13] == b':' && bytes[16] == b':' {
        (
            s[11..13].parse::<u64>().ok().filter(|&v| v < 24)?,
            s[14..16].parse::<u64>().ok().filter(|&v| v < 60)?,
            s[17..19].parse::<u64>().ok().filter(|&v| v < 60)?,
        )
    } else {
        (0, 0, 0)
    };
    let days = civil_days_since_unix_epoch(year, month, day)?;
    let secs = u64::try_from(days)
        .ok()?
        .saturating_mul(86400)
        .saturating_add(hour * 3600 + min * 60 + sec);
    sanitize_unix_ts(secs)
}

fn civil_days_since_unix_epoch(y: i64, m: u32, d: u32) -> Option<i64> {
    // Howard Hinnant's civil-from-days algorithm inverted cleanly in integer math.
    let mut y = y;
    if m <= 2 {
        y -= 1;
    }
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m_i64 = i64::from(m);
    let d_i64 = i64::from(d);
    let doy = (153 * (m_i64 + if m > 2 { -3 } else { 9 }) + 2) / 5 + d_i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    (days >= 0).then_some(days)
}

fn collect_files_bounded(root: &Path, max_depth: usize, max_files: usize, out: &mut Vec<PathBuf>) {
    if !root.is_dir() || out.len() >= max_files {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= max_files {
            break;
        }
        let path = entry.path();
        if path.is_dir() {
            if max_depth > 0 {
                collect_files_bounded(&path, max_depth - 1, max_files, out);
            }
        } else if path.is_file() {
            if let Ok(meta) = entry.metadata() {
                if meta.len() > 0 && meta.len() <= MAX_SCAN_FILE_BYTES {
                    out.push(path);
                }
            }
        }
    }
}

// ============================================================================
// 1. Steam (`userdata/<id>/config/localconfig.vdf` + `appmanifest_*.acf`)
// ============================================================================

fn collect_steam_playtime_index(index: &mut ExternalPlaytimeIndex) {
    let steam_times = collect_steam_local_playtimes();
    if !steam_times.is_empty() {
        index.record_source("Steam localconfig.vdf");
    }
    for (aid, (secs, lp)) in &steam_times {
        index.insert_steam(*aid, *secs, *lp);
    }

    for steam_root in find_steam_root_dirs() {
        index.record_source("Steam localconfig.vdf");
        let mut lib_roots = vec![steam_root.clone()];
        let vdf_path = steam_root.join("steamapps").join("libraryfolders.vdf");
        if let Ok(vdf_text) = fs::read_to_string(&vdf_path) {
            lib_roots.extend(extract_steam_library_paths_from_vdf(&vdf_text));
        }
        for root in lib_roots {
            let steamapps = root.join("steamapps");
            let Ok(rd) = fs::read_dir(&steamapps) else {
                continue;
            };
            for fentry in rd.flatten() {
                let name = fentry.file_name().to_string_lossy().to_ascii_lowercase();
                if !(name.starts_with("appmanifest_") && name.ends_with(".acf")) {
                    continue;
                }
                let Ok(raw) = fs::read_to_string(fentry.path()) else {
                    continue;
                };
                let Some(acf) = parse_steam_appmanifest_acf(&raw) else {
                    continue;
                };
                let (secs, lp) = steam_times
                    .get(&acf.app_id)
                    .copied()
                    .unwrap_or((0, acf.last_played));
                let lp = lp.or(acf.last_played);
                if secs > 0 || lp.is_some() {
                    index.insert_title(&acf.name, secs, lp);
                    let install_dir = steamapps.join("common").join(&acf.install_dir_name);
                    if let Some(dir_str) = install_dir.to_str() {
                        index.insert_install_dir(dir_str, secs, lp);
                    }
                }
            }
        }
    }
}

// ============================================================================
// 2. Xbox App / PC Game Pass (`Microsoft.GamingApp_*` + `GameConfigStore`)
// ============================================================================

fn extract_json_playtime_records(val: &Value, index: &mut ExternalPlaytimeIndex) {
    match val {
        Value::Array(items) => {
            for item in items {
                extract_json_playtime_records(item, index);
            }
        }
        Value::Object(map) => {
            let title = [
                "title",
                "Title",
                "name",
                "Name",
                "displayName",
                "DisplayName",
                "productTitle",
                "ProductTitle",
                "gameTitle",
                "GameTitle",
            ]
            .iter()
            .find_map(|k| map.get(*k).and_then(|v| v.as_str()))
            .map(str::trim)
            .filter(|s| !s.is_empty());

            let exe_path = ["exePath", "executablePath", "matchedExeFullPath"]
                .iter()
                .find_map(|k| map.get(*k).and_then(|v| v.as_str()))
                .map(str::trim)
                .filter(|s| !s.is_empty());

            let secs = [
                ("playtimeSeconds", 1u64),
                ("PlaytimeSeconds", 1),
                ("secondsPlayed", 1),
                ("SecondsPlayed", 1),
                ("totalSecondsPlayed", 1),
                ("timePlayedSeconds", 1),
                ("durationSeconds", 1),
                ("totalTimePlayed", 1),
                ("minutesPlayed", 60),
                ("MinutesPlayed", 60),
                ("timePlayedMinutes", 60),
                ("TimePlayedMinutes", 60),
                ("playtimeMinutes", 60),
                ("PlaytimeMinutes", 60),
                ("totalMinutesPlayed", 60),
                ("totalPlaytimeMinutes", 60),
                ("minutesInGame", 60),
            ]
            .iter()
            .find_map(|(k, mult)| {
                map.get(*k).and_then(|v| {
                    v.as_u64()
                        .or_else(|| v.as_f64().filter(|f| *f > 0.0).map(|f| f.round() as u64))
                        .or_else(|| v.as_str().and_then(|s| s.trim().parse::<u64>().ok()))
                        .and_then(|n| sanitize_sane_secs(n.saturating_mul(*mult)))
                })
            })
            .unwrap_or(0);

            let lp = [
                "lastPlayed",
                "LastPlayed",
                "lastPlayedTime",
                "LastPlayedTime",
                "lastPlayedTimestamp",
                "lastPlayedDate",
            ]
            .iter()
            .find_map(|k| {
                map.get(*k).and_then(|v| {
                    v.as_u64()
                        .and_then(sanitize_unix_ts)
                        .or_else(|| v.as_str().and_then(parse_timestamp_utc_secs))
                })
            });

            if secs > 0 || lp.is_some() {
                if let Some(t) = title {
                    index.insert_title(t, secs, lp);
                }
                if let Some(ep) = exe_path {
                    index.insert_exe_path(ep, secs, lp);
                }
            }

            for child in map.values() {
                if matches!(child, Value::Object(_) | Value::Array(_)) {
                    extract_json_playtime_records(child, index);
                }
            }
        }
        _ => {}
    }
}

fn try_scan_sqlite_playtimes(db_path: &Path, index: &mut ExternalPlaytimeIndex) {
    let Ok(conn) = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY) else {
        return;
    };
    let Ok(mut tbl_stmt) =
        conn.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")
    else {
        return;
    };
    let Ok(tables) = tbl_stmt.query_map([], |row| row.get::<_, String>(0)) else {
        return;
    };

    for table_res in tables.flatten() {
        let safe_table: String = table_res
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if safe_table.is_empty() {
            continue;
        }
        let pragma = format!("PRAGMA table_info(\"{safe_table}\")");
        let Ok(mut col_stmt) = conn.prepare(&pragma) else {
            continue;
        };
        let cols: Vec<String> = col_stmt
            .query_map([], |r| r.get::<_, String>(1))
            .ok()
            .map(|iter| iter.flatten().collect())
            .unwrap_or_default();

        let title_col = cols.iter().find(|c| {
            let l = c.to_ascii_lowercase();
            l.contains("title") || l == "name" || l == "displayname" || l == "producttitle"
        });
        let pt_col = cols.iter().find(|c| {
            let l = c.to_ascii_lowercase();
            l.contains("minutes") || l.contains("playtime") || l.contains("seconds")
        });

        let (Some(tc), Some(pc)) = (title_col, pt_col) else {
            continue;
        };
        let is_secs = pc.to_ascii_lowercase().contains("sec");
        let sql = format!(
            "SELECT \"{tc}\", \"{pc}\" FROM \"{safe_table}\" WHERE \"{tc}\" IS NOT NULL AND \"{pc}\" IS NOT NULL"
        );
        let Ok(mut stmt) = conn.prepare(&sql) else {
            continue;
        };
        let Ok(rows) = stmt.query_map([], |row| {
            let title: String = row.get(0)?;
            let val: i64 = row.get(1)?;
            Ok((title, val))
        }) else {
            continue;
        };
        for row in rows.flatten() {
            if let Ok(raw_num) = u64::try_from(row.1) {
                let mult = if is_secs { 1 } else { 60 };
                if let Some(secs) = sanitize_sane_secs(raw_num.saturating_mul(mult)) {
                    index.insert_title(&row.0, secs, None);
                }
            }
        }
    }
}

/// Parses a single `Microsoft.GamingApp_*` LocalState JSON text into `index`.
pub fn parse_xbox_localstate_json(raw_json: &str, index: &mut ExternalPlaytimeIndex) {
    if let Ok(val) = serde_json::from_str::<Value>(raw_json.trim_start_matches('\u{feff}')) {
        extract_json_playtime_records(&val, index);
    }
}

fn collect_xbox_playtime_index(index: &mut ExternalPlaytimeIndex) {
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let packages_dir = PathBuf::from(local_app_data).join("Packages");
        if let Ok(pkgs) = fs::read_dir(&packages_dir) {
            for pkg in pkgs.flatten() {
                let pkg_name = pkg.file_name().to_string_lossy().to_string();
                if !pkg_name
                    .to_ascii_lowercase()
                    .starts_with("microsoft.gamingapp_")
                {
                    continue;
                }
                index.record_source("Xbox Microsoft.GamingApp");
                let local_state = pkg.path().join("LocalState");
                let mut files = Vec::new();
                collect_files_bounded(&local_state, 2, 160, &mut files);
                for f in files {
                    let ext = f
                        .extension()
                        .and_then(|s| s.to_str())
                        .map(|s| s.to_ascii_lowercase())
                        .unwrap_or_default();
                    if matches!(ext.as_str(), "json" | "txt") {
                        if let Ok(raw) = fs::read_to_string(&f) {
                            parse_xbox_localstate_json(&raw, index);
                        }
                    } else if matches!(ext.as_str(), "db" | "sqlite" | "sqlite3") {
                        try_scan_sqlite_playtimes(&f, index);
                    }
                }
            }
        }
    }

    // Windows GameConfigStore local registry fallback (`HKCU\System\GameConfigStore\Children\*`)
    let subkeys =
        duke_core::win32_ffi::reg_enum_subkeys(false, "System\\GameConfigStore\\Children");
    if !subkeys.is_empty() {
        index.record_source("Windows GameConfigStore");
    }
    for child in subkeys.iter().take(512) {
        let subkey = format!("System\\GameConfigStore\\Children\\{child}");
        let exe_path = duke_core::win32_ffi::reg_read_string(false, &subkey, "MatchedExeFullPath");
        let last_played = duke_core::win32_ffi::reg_read_string(false, &subkey, "LastPlayed")
            .and_then(|s| parse_timestamp_utc_secs(&s));
        let duration_secs = duke_core::win32_ffi::reg_read_dword(false, &subkey, "PlaytimeSeconds")
            .or_else(|| duke_core::win32_ffi::reg_read_dword(false, &subkey, "ActiveTimeSeconds"))
            .map(u64::from)
            .or_else(|| {
                duke_core::win32_ffi::reg_read_dword(false, &subkey, "PlaytimeMinutes")
                    .map(|m| u64::from(m).saturating_mul(60))
            })
            .and_then(sanitize_sane_secs)
            .unwrap_or(0);

        if duration_secs > 0 || last_played.is_some() {
            if let Some(ref ep) = exe_path {
                index.insert_exe_path(ep, duration_secs, last_played);
                if let Some(stem) = Path::new(ep).file_stem().and_then(|s| s.to_str()) {
                    index.insert_title(stem, duration_secs, last_played);
                }
            }
        }
    }
}

// ============================================================================
// 3. Epic Games (`Manifests\*.item` + `Saved\Data\*.json` + Logs)
// ============================================================================

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
struct EpicManifestBasic {
    display_name: String,
    app_name: String,
    install_location: String,
}

fn collect_epic_playtime_index(index: &mut ExternalPlaytimeIndex) {
    let epic_titles = collect_epic_local_playtimes();
    if !epic_titles.is_empty() {
        index.record_source("Epic Games");
    }
    for (tkey, secs) in &epic_titles {
        if let Some(s) = sanitize_sane_secs(*secs) {
            index.by_title
                .entry(tkey.clone())
                .or_default()
                .merge(s, None);
        }
    }

    let program_data = std::env::var("PROGRAMDATA")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "C:\\ProgramData".to_string());
    let manifests_dir = PathBuf::from(&program_data)
        .join("Epic")
        .join("EpicGamesLauncher")
        .join("Data")
        .join("Manifests");

    let mut app_name_to_dir: HashMap<String, (String, PathBuf)> = HashMap::new();
    if let Ok(rd) = fs::read_dir(&manifests_dir) {
        index.record_source("Epic Games");
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension()
                .and_then(|s| s.to_str())
                .map(|s| s.eq_ignore_ascii_case("item"))
                .unwrap_or(false)
            {
                if let Ok(raw) = fs::read_to_string(&p) {
                    if let Ok(m) =
                        serde_json::from_str::<EpicManifestBasic>(raw.trim_start_matches('\u{feff}'))
                    {
                        let tkey = normalize_title_playtime_key(&m.display_name);
                        let secs = epic_titles.get(&tkey).copied().unwrap_or(0);
                        let dir = PathBuf::from(m.install_location.replace('/', "\\"));
                        if secs > 0 && !m.install_location.trim().is_empty() {
                            index.insert_install_dir(&m.install_location, secs, None);
                        }
                        if !m.app_name.trim().is_empty() {
                            app_name_to_dir.insert(
                                m.app_name.trim().to_ascii_lowercase(),
                                (m.display_name, dir),
                            );
                        }
                    }
                }
            }
        }
    }

    // Also parse EpicGamesLauncher session logs for cumulative elapsed session seconds if present.
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let logs_dir = PathBuf::from(local_app_data)
            .join("EpicGamesLauncher")
            .join("Saved")
            .join("Logs");
        let mut files = Vec::new();
        collect_files_bounded(&logs_dir, 0, 24, &mut files);
        for f in files {
            let fname = f.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if !fname.to_ascii_lowercase().starts_with("epicgameslauncher") {
                continue;
            }
            if let Ok(file) = fs::File::open(&f) {
                let reader = BufReader::with_capacity(32 * 1024, file);
                for line in reader.lines().map_while(Result::ok) {
                    let lower = line.to_ascii_lowercase();
                    if !(lower.contains("elapsed=") || lower.contains("playtime")) {
                        continue;
                    }
                    for (app_key, (title, dir)) in &app_name_to_dir {
                        if lower.contains(app_key) {
                            if let Some(secs) = extract_elapsed_seconds_token(&lower) {
                                index.insert_title(title, secs, None);
                                if let Some(ds) = dir.to_str() {
                                    index.insert_install_dir(ds, secs, None);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn extract_elapsed_seconds_token(lower_line: &str) -> Option<u64> {
    for key in ["elapsed=", "playtimeseconds=", "seconds="] {
        if let Some(idx) = lower_line.find(key) {
            let rest = &lower_line[idx + key.len()..];
            let num_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(val) = num_str.parse::<u64>() {
                return sanitize_sane_secs(val);
            }
        }
    }
    None
}

// ============================================================================
// 4. GOG Galaxy (`galaxy-2.0.db` `GameTimes`)
// ============================================================================

fn collect_gog_playtime_index(index: &mut ExternalPlaytimeIndex) {
    let (by_release_key, by_title) = collect_gog_local_playtimes();
    if !by_release_key.is_empty() || !by_title.is_empty() {
        index.record_source("GOG galaxy-2.0.db");
    }
    for (tkey, secs) in by_title {
        if let Some(s) = sanitize_sane_secs(secs) {
            index.by_title.entry(tkey).or_default().merge(s, None);
        }
    }

    let program_data = std::env::var("PROGRAMDATA")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "C:\\ProgramData".to_string());
    let db_path = PathBuf::from(program_data)
        .join("GOG.com")
        .join("Galaxy")
        .join("storage")
        .join("galaxy-2.0.db");
    if db_path.is_file() {
        index.record_source("GOG galaxy-2.0.db");
        if let Ok(conn) = Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
            if let Ok(mut stmt) = conn.prepare(
                "SELECT ibp.productId, ibp.installationPath FROM InstalledBaseProducts ibp",
            ) {
                if let Ok(rows) = stmt.query_map([], |row| {
                    let pid: String = row.get(0)?;
                    let path: String = row.get(1)?;
                    Ok((pid, path))
                }) {
                    for (pid, path) in rows.flatten() {
                        let key = format!("gog_{}", pid.trim().to_ascii_lowercase());
                        if let Some(secs) = by_release_key
                            .get(&key)
                            .copied()
                            .or_else(|| by_release_key.get(&pid.trim().to_ascii_lowercase()).copied())
                        {
                            index.insert_install_dir(&path, secs, None);
                        }
                    }
                }
            }
        }
    }
}

// ============================================================================
// 5. Ubisoft Connect & EA App local caches
// ============================================================================

pub fn parse_ubisoft_ea_text_playtime(raw: &str, index: &mut ExternalPlaytimeIndex) {
    let trimmed = raw.trim_start_matches('\u{feff}').trim();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
            extract_json_playtime_records(&val, index);
            return;
        }
    }
    // Simple line-oriented YAML/INI fallback (`title: ...` paired near `playtime_seconds:` / `time_played:`)
    let mut current_title: Option<String> = None;
    for line in trimmed.lines() {
        let l = line.trim();
        let lower = l.to_ascii_lowercase();
        if l.starts_with('-') {
            current_title = None;
        }
        if let Some((k, v)) = l.split_once(':').or_else(|| l.split_once('=')) {
            let kl = k.trim().trim_start_matches('-').trim().to_ascii_lowercase();
            let val = v.trim().trim_matches('"').trim_matches('\'');
            if matches!(kl.as_str(), "title" | "name" | "gametitle" | "displayname") && !val.is_empty()
            {
                current_title = Some(val.to_string());
            } else if matches!(
                kl.as_str(),
                "playtime" | "playtime_seconds" | "playtimeseconds" | "time_played" | "secondsplayed"
            ) {
                if let Some(secs) = val.parse::<u64>().ok().and_then(sanitize_sane_secs) {
                    if let Some(ref t) = current_title {
                        index.insert_title(t, secs, None);
                    }
                }
            } else if matches!(kl.as_str(), "minutesplayed" | "playtime_minutes" | "minutes") {
                if let Some(secs) = val
                    .parse::<u64>()
                    .ok()
                    .and_then(|m| sanitize_sane_secs(m.saturating_mul(60)))
                {
                    if let Some(ref t) = current_title {
                        index.insert_title(t, secs, None);
                    }
                }
            }
        } else if lower.is_empty() {
            current_title = None;
        }
    }
}

fn collect_ubisoft_and_ea_playtime_index(index: &mut ExternalPlaytimeIndex) {
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let base = PathBuf::from(&local_app_data);
        let ubisoft_dir = base.join("Ubisoft Game Launcher");
        if ubisoft_dir.is_dir() {
            index.record_source("Ubisoft Connect");
            let mut files = Vec::new();
            collect_files_bounded(&ubisoft_dir, 2, 100, &mut files);
            for f in files {
                let ext = f
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_ascii_lowercase())
                    .unwrap_or_default();
                if matches!(ext.as_str(), "json" | "yml" | "yaml" | "txt") {
                    if let Ok(raw) = fs::read_to_string(&f) {
                        parse_ubisoft_ea_text_playtime(&raw, index);
                    }
                }
            }
        }

        let ea_dir = base.join("Electronic Arts").join("EA Desktop");
        let origin_dir = base.join("Origin").join("LocalContent");
        for dir in [&ea_dir, &origin_dir] {
            if dir.is_dir() {
                index.record_source("EA App");
                let mut files = Vec::new();
                collect_files_bounded(dir, 2, 120, &mut files);
                for f in files {
                    let ext = f
                        .extension()
                        .and_then(|s| s.to_str())
                        .map(|s| s.to_ascii_lowercase())
                        .unwrap_or_default();
                    if matches!(ext.as_str(), "json" | "ini" | "txt") {
                        if let Ok(raw) = fs::read_to_string(&f) {
                            parse_ubisoft_ea_text_playtime(&raw, index);
                        }
                    }
                }
            }
        }
    }
}

// ============================================================================
// 6. RetroArch (`.lpl` / `content_history.lpl` + `.lrtl` runtime logs)
// ============================================================================

#[derive(Debug, Deserialize, Default)]
struct RetroArchLplFile {
    #[serde(default)]
    items: Vec<RetroArchLplItem>,
}

#[derive(Debug, Deserialize, Default)]
struct RetroArchLplItem {
    #[serde(default)]
    path: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    runtime: String,
    #[serde(default)]
    last_played: String,
}

#[derive(Debug, Deserialize, Default)]
struct RetroArchLrtlFile {
    #[serde(default)]
    runtime: String,
    #[serde(default)]
    last_played: String,
}

/// Parses RetroArch `.lpl` / `content_history.lpl` JSON into `index`.
pub fn parse_retroarch_lpl_str(raw: &str, index: &mut ExternalPlaytimeIndex) {
    let Ok(parsed) = serde_json::from_str::<RetroArchLplFile>(raw.trim_start_matches('\u{feff}'))
    else {
        return;
    };
    for item in parsed.items {
        let secs = parse_hms_duration_secs(&item.runtime).unwrap_or(0);
        let lp = parse_timestamp_utc_secs(&item.last_played);
        if secs == 0 && lp.is_none() {
            continue;
        }
        if !item.label.trim().is_empty() {
            index.insert_title(&item.label, secs, lp);
        }
        if !item.path.trim().is_empty() {
            let rom_path_part = item.path.split('#').next().unwrap_or(&item.path);
            index.insert_exe_path(rom_path_part, secs, lp);
            if let Some(stem) = Path::new(rom_path_part).file_stem().and_then(|s| s.to_str()) {
                index.insert_title(stem, secs, lp);
            }
        }
    }
}

/// Parses a single RetroArch `.lrtl` runtime file paired with its ROM stem into `index`.
pub fn parse_retroarch_lrtl_str(rom_stem: &str, raw: &str, index: &mut ExternalPlaytimeIndex) {
    let Ok(parsed) = serde_json::from_str::<RetroArchLrtlFile>(raw.trim_start_matches('\u{feff}'))
    else {
        return;
    };
    let secs = parse_hms_duration_secs(&parsed.runtime).unwrap_or(0);
    let lp = parse_timestamp_utc_secs(&parsed.last_played);
    if secs > 0 || lp.is_some() {
        index.insert_title(rom_stem, secs, lp);
    }
}

fn collect_retroarch_playtime_index(index: &mut ExternalPlaytimeIndex, extra_roots: &[PathBuf]) {
    let mut roots: Vec<PathBuf> = Vec::new();
    let mut push_unique = |p: PathBuf| {
        if p.is_dir()
            && !roots.iter().any(|existing| {
                existing
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&p.to_string_lossy())
            })
        {
            roots.push(p);
        }
    };

    for env_key in ["APPDATA", "LOCALAPPDATA"] {
        if let Ok(base) = std::env::var(env_key) {
            push_unique(PathBuf::from(base).join("RetroArch"));
        }
    }
    if let Ok(user_profile) = std::env::var("USERPROFILE") {
        push_unique(PathBuf::from(user_profile).join("RetroArch"));
    }
    for er in extra_roots {
        push_unique(er.clone());
    }
    for drive in ['C', 'D'] {
        push_unique(PathBuf::from(format!("{drive}:\\RetroArch-Win64")));
        push_unique(PathBuf::from(format!("{drive}:\\RetroArch")));
    }

    for root in roots {
        index.record_source("RetroArch");

        let history_lpl = root.join("content_history.lpl");
        if history_lpl.is_file() {
            if let Ok(raw) = fs::read_to_string(&history_lpl) {
                parse_retroarch_lpl_str(&raw, index);
            }
        }

        let playlists_dir = root.join("playlists");
        let mut lpl_and_lrtl_files = Vec::new();
        collect_files_bounded(&playlists_dir, 3, 512, &mut lpl_and_lrtl_files);
        collect_files_bounded(&root.join("logs"), 2, 256, &mut lpl_and_lrtl_files);

        for file_path in lpl_and_lrtl_files {
            let ext = file_path
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_default();
            if ext == "lpl" {
                if let Ok(raw) = fs::read_to_string(&file_path) {
                    parse_retroarch_lpl_str(&raw, index);
                }
            } else if ext == "lrtl" {
                if let Some(stem) = file_path.file_stem().and_then(|s| s.to_str()) {
                    if let Ok(raw) = fs::read_to_string(&file_path) {
                        parse_retroarch_lrtl_str(stem, &raw, index);
                    }
                }
            }
        }
    }
}

// ============================================================================
// Aggregator & Non-Destructive Merger
// ============================================================================

pub fn collect_all_external_playtimes(retroarch_roots: &[PathBuf]) -> ExternalPlaytimeIndex {
    let mut index = ExternalPlaytimeIndex::default();
    collect_steam_playtime_index(&mut index);
    collect_xbox_playtime_index(&mut index);
    collect_epic_playtime_index(&mut index);
    collect_gog_playtime_index(&mut index);
    collect_ubisoft_and_ea_playtime_index(&mut index);
    collect_retroarch_playtime_index(&mut index, retroarch_roots);
    index
}

fn find_best_external_match(
    g: &crate::GameEntry,
    index: &ExternalPlaytimeIndex,
) -> (u64, Option<u64>) {
    let mut synced_secs = 0u64;
    let mut synced_lp: Option<u64> = None;
    let mut absorb = |rec: &ExternalPlaytimeRecord| {
        synced_secs = synced_secs.max(rec.seconds);
        if let Some(lp) = rec.last_played_ts {
            synced_lp = Some(synced_lp.map_or(lp, |cur| cur.max(lp)));
        }
    };

    if let Some(aid) = g
        .app_id
        .or_else(|| g.steam_app_id.and_then(|v| u32::try_from(v).ok()))
    {
        if let Some(rec) = index.by_steam_app_id.get(&aid) {
            absorb(rec);
        }
    }

    let exe_key = normalize_win_path_key(&g.exe_path);
    if !exe_key.is_empty() {
        if let Some(rec) = index.by_exe_path.get(&exe_key) {
            absorb(rec);
        }
        if let Some(stem) = Path::new(&g.exe_path).file_stem().and_then(|s| s.to_str()) {
            let stem_key = normalize_title_playtime_key(stem);
            if stem_key.len() >= 2 {
                if let Some(rec) = index.by_title.get(&stem_key) {
                    absorb(rec);
                }
            }
        }
    }

    let dir_key = normalize_win_path_key(&g.install_dir);
    if !dir_key.is_empty() {
        if let Some(rec) = index.by_install_dir.get(&dir_key) {
            absorb(rec);
        }
    }

    let tkey = normalize_title_playtime_key(&g.title);
    if tkey.len() >= 2 {
        if let Some(rec) = index.by_title.get(&tkey) {
            absorb(rec);
        }
    }

    (synced_secs, synced_lp)
}

/// Applies external playtime non-destructively:
/// `new_imported = max(imported_playtime_secs, synced_external_secs)`
/// `total = new_imported + duke_playtime_secs`
pub fn apply_external_playtimes_non_destructive(
    library: &mut [crate::GameEntry],
    index: &ExternalPlaytimeIndex,
) -> usize {
    let mut updated = 0usize;
    for g in library.iter_mut() {
        let mut changed = false;
        if g.imported_playtime_secs == 0 && g.duke_playtime_secs == 0 && g.playtime_seconds > 0 {
            g.duke_playtime_secs = g.playtime_seconds;
            changed = true;
        }

        let (synced_secs, synced_lp) = find_best_external_match(g, index);
        let target_imported = g.imported_playtime_secs.max(synced_secs);
        if target_imported > g.imported_playtime_secs {
            g.imported_playtime_secs = target_imported;
            changed = true;
        }

        if let Some(lp) = synced_lp.filter(|&ts| ts > g.last_played_ts.unwrap_or(0)) {
            g.last_played_ts = Some(lp);
            g.last_played = Some(lp);
            changed = true;
        }

        let target_total = g.imported_playtime_secs.saturating_add(g.duke_playtime_secs);
        if target_total != g.playtime_seconds {
            g.playtime_seconds = target_total;
            changed = true;
        }

        if changed {
            updated += 1;
        }
    }
    updated
}

fn infer_retroarch_roots_from_library(library: &[crate::GameEntry]) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut seen = HashSet::new();
    for g in library {
        let exe_lower = g.exe_path.to_ascii_lowercase();
        let emu_lower = g.emulator_id.to_ascii_lowercase();
        if exe_lower.contains("retroarch") || emu_lower.contains("retroarch") {
            if let Some(parent) = Path::new(&g.exe_path).parent() {
                let key = parent.to_string_lossy().to_ascii_lowercase();
                if seen.insert(key) {
                    roots.push(parent.to_path_buf());
                }
            }
        }
    }
    roots
}

pub async fn run_external_playtime_sync_once(
    app: &AppHandle,
) -> Result<PlaytimeSyncSummary, String> {
    let snapshot_lib = crate::load_library_lightweight(app).unwrap_or_default();
    let retro_roots = infer_retroarch_roots_from_library(&snapshot_lib);

    let mut index = tokio::task::spawn_blocking(move || collect_all_external_playtimes(&retro_roots))
        .await
        .map_err(|e| format!("Playtime sync task failed: {e}"))?;

    let mut library = crate::load_library_from_disk(app)?;
    for entry in &mut library {
        crate::normalize_game_entry(entry);
    }

    let updated_games = apply_external_playtimes_non_destructive(&mut library, &index);
    let total_imported_secs: u64 = library.iter().map(|g| g.imported_playtime_secs).sum();
    let total_duke_secs: u64 = library.iter().map(|g| g.duke_playtime_secs).sum();

    if updated_games > 0 {
        crate::save_library_to_disk(app, &library)?;
        crate::emit_if_alive(app, "library-updated", &library);
        crate::emit_if_alive(
            app,
            "playtime-synced",
            serde_json::json!({
                "updatedGames": updated_games,
                "totalImportedSecs": total_imported_secs,
                "totalDukeSecs": total_duke_secs,
            }),
        );
    }

    let sources_scanned = std::mem::take(&mut index.sources_scanned);
    drop(index);
    drop(library);
    drop(snapshot_lib);
    duke_core::win32_ffi::trim_current_working_set();

    Ok(PlaytimeSyncSummary {
        updated_games,
        total_imported_secs,
        total_duke_secs,
        sources_scanned,
    })
}

/// Spawns non-blocking multi-launcher playtime sync (guarded against overlapping runs).
pub fn spawn_background_playtime_sync(app: AppHandle) {
    let flag = PLAYTIME_SYNC_IN_FLIGHT.get_or_init(|| AtomicBool::new(false));
    if flag.swap(true, Ordering::AcqRel) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let _ = run_external_playtime_sync_once(&app).await;
        if let Some(f) = PLAYTIME_SYNC_IN_FLIGHT.get() {
            f.store(false, Ordering::Release);
        }
    });
}

#[tauri::command]
pub async fn sync_all_external_playtimes(app: AppHandle) -> Result<PlaytimeSyncSummary, String> {
    run_external_playtime_sync_once(&app).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_retroarch_lrtl_hms_and_lpl_playlist_safely() {
        let mut idx = ExternalPlaytimeIndex::default();
        let lrtl = r#"{
            "version": "1.0",
            "runtime": "2:15:30",
            "last_played": "2025-11-10 14:20:05"
        }"#;
        parse_retroarch_lrtl_str("Super Metroid (USA)", lrtl, &mut idx);
        let rec = idx
            .by_title
            .get(&normalize_title_playtime_key("Super Metroid"))
            .expect("retroarch title mapped");
        assert_eq!(rec.seconds, 2 * 3600 + 15 * 60 + 30);
        assert!(rec.last_played_ts.is_some());

        let lpl = r#"{
            "version": "1.5",
            "items": [
                {
                    "path": "D:\\ROMs\\GBA\\Metroid Fusion (USA).gba",
                    "label": "Metroid Fusion",
                    "runtime": "01:05:10",
                    "last_played": "2025-12-01 09:00:00"
                }
            ]
        }"#;
        parse_retroarch_lpl_str(lpl, &mut idx);
        let gba_rec = idx
            .by_title
            .get(&normalize_title_playtime_key("Metroid Fusion"))
            .expect("lpl label mapped");
        assert_eq!(gba_rec.seconds, 3910);
        assert!(idx.by_exe_path.contains_key("d:\\roms\\gba\\metroid fusion (usa).gba"));
    }

    #[test]
    fn parses_xbox_localstate_and_ubisoft_ea_playtime_records() {
        let mut idx = ExternalPlaytimeIndex::default();
        let xbox_json = r#"{
            "recentGames": [
                {
                    "title": "Forza Horizon 5",
                    "minutesPlayed": 185,
                    "lastPlayed": "2026-01-12T18:00:00Z"
                }
            ]
        }"#;
        parse_xbox_localstate_json(xbox_json, &mut idx);
        let forza = idx
            .by_title
            .get(&normalize_title_playtime_key("Forza Horizon 5"))
            .expect("xbox title mapped");
        assert_eq!(forza.seconds, 185 * 60);
        assert!(forza.last_played_ts.is_some());

        let ea_yaml = "title: Titanfall 2\nplaytime_seconds: 5400\n";
        parse_ubisoft_ea_text_playtime(ea_yaml, &mut idx);
        let tf2 = idx
            .by_title
            .get(&normalize_title_playtime_key("Titanfall 2"))
            .expect("ea title mapped");
        assert_eq!(tf2.seconds, 5400);
    }

    #[test]
    fn non_destructive_merge_takes_max_imported_and_never_double_counts() {
        let mut idx = ExternalPlaytimeIndex::default();
        idx.insert_title("Hades", 3600, Some(1_730_000_000));

        let mut library = vec![crate::GameEntry {
            id: "g_hades".to_string(),
            title: "Hades".to_string(),
            imported_playtime_secs: 7200,
            duke_playtime_secs: 1800,
            playtime_seconds: 9000,
            ..Default::default()
        }];

        // Smaller external number must NEVER shrink existing imported_playtime_secs.
        let changed = apply_external_playtimes_non_destructive(&mut library, &idx);
        assert_eq!(changed, 1); // last_played_ts updated
        assert_eq!(library[0].imported_playtime_secs, 7200);
        assert_eq!(library[0].duke_playtime_secs, 1800);
        assert_eq!(library[0].playtime_seconds, 9000);

        // Larger external number updates imported_playtime_secs while preserving duke_playtime_secs.
        idx.insert_title("Hades", 10800, None);
        let changed2 = apply_external_playtimes_non_destructive(&mut library, &idx);
        assert_eq!(changed2, 1);
        assert_eq!(library[0].imported_playtime_secs, 10800);
        assert_eq!(library[0].duke_playtime_secs, 1800);
        assert_eq!(library[0].playtime_seconds, 12600);

        // Re-running with identical data is strictly idempotent (zero changes, zero double-count).
        let changed3 = apply_external_playtimes_non_destructive(&mut library, &idx);
        assert_eq!(changed3, 0);
        assert_eq!(library[0].playtime_seconds, 12600);
    }
}

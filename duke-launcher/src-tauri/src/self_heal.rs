//! Self-Healing Engine v2: startup state recovery, atomic `.bak` JSON protection,
//! `session.lock` clean-shutdown heartbeat, IPC freeze watchdog + `freeze-breadcrumb.log`,
//! `disabled_jobs.json` repeat-stall circuit breaker, `library.dklib` / `catalog.dkcat` /
//! `.duke-cas` blob verification & quarantine, WebView2 stale lock reset, 8 default
//! directory verification, NTFS `:Zone.Identifier` unblocking, and crash NTSTATUS classification.

use crate::catalog_mmap::MappedCatalog;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

static SELF_HEAL_TMP_SEQ: AtomicU64 = AtomicU64::new(1);
static SESSION_SEQ: AtomicU64 = AtomicU64::new(1);
static SPAN_SEQ: AtomicU64 = AtomicU64::new(1);
static WATCHDOG_STARTED: AtomicBool = AtomicBool::new(false);

static WATCHDOG_DATA_DIR: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
static SESSION_LOCK_IO: OnceLock<Mutex<()>> = OnceLock::new();
static DISABLED_JOBS_IO: OnceLock<Mutex<()>> = OnceLock::new();
static ACTIVE_SPANS: OnceLock<Mutex<Vec<ActiveCommandSpan>>> = OnceLock::new();
type IssuedSessionMap = HashMap<PathBuf, (u64, SessionRecoveryState, bool)>;

static ISSUED_SESSIONS: OnceLock<Mutex<IssuedSessionMap>> = OnceLock::new();
static LAST_HEAL_REPORTS: OnceLock<Mutex<HashMap<PathBuf, StartupSelfHealReport>>> =
    OnceLock::new();

pub(crate) const PROTECTED_JSON_FILES: &[&str] = &[
    "library.json",
    "settings.json",
    "sources.json",
    "search_sites.json",
];

pub(crate) const LARGE_32BIT_GAME_THRESHOLD_BYTES: u64 = 1_500_000_000;

const SESSION_LOCK_FILENAME: &str = "session.lock";
const DISABLED_JOBS_FILENAME: &str = "disabled_jobs.json";
const FREEZE_LOG_FILENAME: &str = "freeze-breadcrumb.log";
const FREEZE_LOG_MAX_BYTES: usize = 256 * 1024;
const WATCHDOG_POLL_MS: u64 = 750;
const WATCHDOG_STALL_THRESHOLD_MS: u64 = 3_000;
const REPEAT_CRASH_DISABLE_THRESHOLD: u32 = 2;
const CAS_HASH_CHUNK_BYTES: usize = 64 * 1024;
const CAS_STARTUP_BYTE_BUDGET: u64 = 512 * 1024 * 1024;
const CAS_STARTUP_TIME_BUDGET_MS: u64 = 2_000;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct StartupSelfHealReport {
    pub restored_json_files: Vec<String>,
    pub recovered_downloads: usize,
    pub cleaned_tmp_files: usize,
    pub catalog_valid: bool,
    #[serde(default)]
    pub unclean_previous_exit: bool,
    #[serde(default)]
    pub last_stalled_command: Option<String>,
    #[serde(default)]
    pub quarantined_files: Vec<String>,
    #[serde(default)]
    pub disabled_subsystems: Vec<String>,
    #[serde(default)]
    pub dklib_repaired: bool,
    #[serde(default)]
    pub settings_repaired: bool,
    #[serde(default)]
    pub catalog_rebuilt: bool,
    #[serde(default)]
    pub cas_blobs_checked: usize,
    #[serde(default)]
    pub cas_blobs_quarantined: usize,
    #[serde(default)]
    pub webview_locks_cleared: usize,
    #[serde(default)]
    pub sites_verified: bool,
    #[serde(default)]
    pub toast_summary: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct SessionRecoveryState {
    pub unclean_previous_exit: bool,
    pub previous_pid: Option<u32>,
    pub active_command: Option<String>,
    pub stalled_at_ts: Option<u64>,
    pub crash_streak: u32,
    pub disabled_subsystem: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct DisabledSubsystemEntry {
    pub name: String,
    pub reason: String,
    pub crash_streak: u32,
    pub disabled_at_ts: u64,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct SelfHealDiagnostics {
    pub session_lock_active: bool,
    pub current_pid: Option<u32>,
    pub active_command: Option<String>,
    pub stalled_at_ts: Option<u64>,
    pub crash_streak: u32,
    pub disabled_subsystems: Vec<DisabledSubsystemEntry>,
    pub active_in_flight_commands: Vec<String>,
    pub freeze_log_bytes: u64,
    pub freeze_log_tail: Vec<String>,
    pub quarantine_count: usize,
    pub catalog_valid: bool,
    pub last_report: Option<StartupSelfHealReport>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct SessionLockPayload {
    pid: u32,
    session_id: u64,
    started_at_ts: u64,
    heartbeat_ts: u64,
    #[serde(alias = "active_command")]
    active_command: Option<String>,
    #[serde(alias = "stalled_at_ts")]
    stalled_at_ts: Option<u64>,
    #[serde(alias = "last_crashed_command")]
    last_crashed_command: Option<String>,
    #[serde(alias = "crash_streak")]
    crash_streak: u32,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct ActiveCommandSpan {
    id: u64,
    command: String,
    started_at: Instant,
    stalled_logged: bool,
}

#[allow(dead_code)]
pub(crate) struct CommandBreadcrumbGuard {
    id: u64,
}

impl CommandBreadcrumbGuard {
    #[allow(dead_code)]
    pub(crate) fn new(cmd: &str) -> Self {
        record_heavy_command_breadcrumb(cmd)
    }
}

impl Drop for CommandBreadcrumbGuard {
    fn drop(&mut self) {
        let next_active = {
            let mut guard = active_spans_lock()
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            guard.retain(|s| s.id != self.id);
            guard.last().map(|s| s.command.clone())
        };
        if let Some(dir) = watchdog_dir_clone() {
            let clear_stalled = next_active.is_none();
            update_session_active_command(&dir, next_active, clear_stalled, None);
        }
    }
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn normalize_dir_key(dir: &Path) -> PathBuf {
    fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf())
}

fn session_lock_io() -> &'static Mutex<()> {
    SESSION_LOCK_IO.get_or_init(|| Mutex::new(()))
}

fn disabled_jobs_io() -> &'static Mutex<()> {
    DISABLED_JOBS_IO.get_or_init(|| Mutex::new(()))
}

fn active_spans_lock() -> &'static Mutex<Vec<ActiveCommandSpan>> {
    ACTIVE_SPANS.get_or_init(|| Mutex::new(Vec::new()))
}

fn watchdog_data_dir_lock() -> &'static Mutex<Option<PathBuf>> {
    WATCHDOG_DATA_DIR.get_or_init(|| Mutex::new(None))
}

fn watchdog_dir_clone() -> Option<PathBuf> {
    watchdog_data_dir_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
}

fn issued_sessions_lock() -> &'static Mutex<HashMap<PathBuf, (u64, SessionRecoveryState, bool)>> {
    ISSUED_SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn last_heal_reports_lock() -> &'static Mutex<HashMap<PathBuf, StartupSelfHealReport>> {
    LAST_HEAL_REPORTS.get_or_init(|| Mutex::new(HashMap::new()))
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

fn load_disabled_jobs_unlocked(data_dir: &Path) -> Vec<DisabledSubsystemEntry> {
    let path = data_dir.join(DISABLED_JOBS_FILENAME);
    let Ok(raw) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    serde_json::from_str::<Vec<DisabledSubsystemEntry>>(trimmed).unwrap_or_default()
}

fn save_disabled_jobs_unlocked(
    data_dir: &Path,
    entries: &[DisabledSubsystemEntry],
) -> Result<(), String> {
    let path = data_dir.join(DISABLED_JOBS_FILENAME);
    let json = serde_json::to_string_pretty(entries)
        .map_err(|e| format!("Failed to serialize disabled_jobs.json: {e}"))?;
    write_temp_and_swap(&path, json.as_bytes(), ".tmp")
}

fn mark_subsystem_disabled_in_dir(
    data_dir: &Path,
    name: &str,
    reason: &str,
    crash_streak: u32,
) -> Result<(), String> {
    let clean = name.trim();
    if clean.is_empty() {
        return Ok(());
    }
    let _guard = disabled_jobs_io()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let mut entries = load_disabled_jobs_unlocked(data_dir);
    let ts = now_unix_secs();
    if let Some(existing) = entries
        .iter_mut()
        .find(|e| e.name.eq_ignore_ascii_case(clean))
    {
        existing.reason = reason.to_string();
        existing.crash_streak = existing.crash_streak.max(crash_streak);
        existing.disabled_at_ts = ts;
    } else {
        entries.push(DisabledSubsystemEntry {
            name: clean.to_string(),
            reason: reason.to_string(),
            crash_streak,
            disabled_at_ts: ts,
        });
    }
    save_disabled_jobs_unlocked(data_dir, &entries)
}

#[allow(dead_code)]
pub(crate) fn is_subsystem_safe_disabled(data_dir: &Path, name: &str) -> bool {
    let clean = name.trim();
    if clean.is_empty() {
        return false;
    }
    let _guard = disabled_jobs_io()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    load_disabled_jobs_unlocked(data_dir)
        .iter()
        .any(|e| e.name.eq_ignore_ascii_case(clean))
}

#[allow(dead_code)]
pub(crate) fn reset_disabled_subsystem(data_dir: &Path, name: &str) -> Result<bool, String> {
    let clean = name.trim();
    if clean.is_empty() {
        return Ok(false);
    }
    let removed = {
        let _guard = disabled_jobs_io()
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let mut entries = load_disabled_jobs_unlocked(data_dir);
        let before = entries.len();
        entries.retain(|e| !e.name.eq_ignore_ascii_case(clean));
        let changed = entries.len() != before;
        if changed {
            save_disabled_jobs_unlocked(data_dir, &entries)?;
        }
        changed
    };

    if removed {
        let _lock = session_lock_io()
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let lock_path = data_dir.join(SESSION_LOCK_FILENAME);
        if let Ok(raw) = fs::read_to_string(&lock_path) {
            if let Ok(mut payload) = serde_json::from_str::<SessionLockPayload>(&raw) {
                if payload
                    .last_crashed_command
                    .as_deref()
                    .is_some_and(|c| c.eq_ignore_ascii_case(clean))
                {
                    payload.last_crashed_command = None;
                    payload.crash_streak = 0;
                    if let Ok(json) = serde_json::to_string_pretty(&payload) {
                        let _ = write_temp_and_swap(&lock_path, json.as_bytes(), ".tmp");
                    }
                }
            }
        }
    }

    Ok(removed)
}

fn read_session_lock_payload(lock_path: &Path) -> Option<SessionLockPayload> {
    let raw = fs::read_to_string(lock_path).ok()?;
    serde_json::from_str::<SessionLockPayload>(raw.trim()).ok()
}

fn write_session_lock_payload(lock_path: &Path, payload: &SessionLockPayload) {
    if let Ok(json) = serde_json::to_string_pretty(payload) {
        let _ = write_temp_and_swap(lock_path, json.as_bytes(), ".tmp");
    }
}

fn begin_session_lock_inner(data_dir: &Path) -> (u64, SessionRecoveryState) {
    let _guard = session_lock_io()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let lock_path = data_dir.join(SESSION_LOCK_FILENAME);
    let ts = now_unix_secs();
    let mut state = SessionRecoveryState::default();
    let mut next_last_crashed: Option<String> = None;
    let mut next_streak: u32 = 0;

    if lock_path.exists() {
        state.unclean_previous_exit = true;
        if let Some(prev) = read_session_lock_payload(&lock_path) {
            state.previous_pid = (prev.pid > 0).then_some(prev.pid);
            state.stalled_at_ts = prev.stalled_at_ts;
            let prev_active = prev
                .active_command
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToString::to_string);
            let prev_last = prev
                .last_crashed_command
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToString::to_string);

            if let Some(cmd) = prev_active {
                state.active_command = Some(cmd.clone());
                let same_as_prev = prev_last
                    .as_deref()
                    .is_some_and(|p| p.eq_ignore_ascii_case(&cmd))
                    || (prev_last.is_none() && prev.crash_streak > 0);
                next_streak = if same_as_prev {
                    prev.crash_streak.saturating_add(1)
                } else {
                    1
                };
                next_last_crashed = Some(cmd.clone());
                state.crash_streak = next_streak;

                if next_streak >= REPEAT_CRASH_DISABLE_THRESHOLD {
                    let reason = format!(
                        "Auto-disabled after {} consecutive unclean exits/stalls during '{}'",
                        next_streak, cmd
                    );
                    if mark_subsystem_disabled_in_dir(data_dir, &cmd, &reason, next_streak).is_ok() {
                        state.disabled_subsystem = Some(cmd);
                    }
                }
            }
        }
    }

    let session_id = SESSION_SEQ.fetch_add(1, Ordering::Relaxed);
    let fresh = SessionLockPayload {
        pid: std::process::id(),
        session_id,
        started_at_ts: ts,
        heartbeat_ts: ts,
        active_command: None,
        stalled_at_ts: None,
        last_crashed_command: next_last_crashed,
        crash_streak: next_streak,
    };
    write_session_lock_payload(&lock_path, &fresh);

    let key = normalize_dir_key(data_dir);
    let mut issued = issued_sessions_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    issued.insert(key, (session_id, state.clone(), false));

    (session_id, state)
}

/// Checks `session.lock` for an unclean prior exit, updates `crash_streak` on repeat crashes,
/// auto-disables repeat-stalling plugins/jobs, and writes a fresh `session.lock` for this PID.
#[allow(dead_code)]
pub(crate) fn begin_session_lock_in_dir(data_dir: &Path) -> SessionRecoveryState {
    let (_, state) = begin_session_lock_inner(data_dir);
    state
}

/// Consumes a `SessionRecoveryState` previously opened by `begin_session_lock_in_dir` in the same
/// live process if `session.lock` on disk still carries that unhealed `session_id`, or inspects
/// `session.lock` freshly when `run_startup_self_heal_in_dir` is invoked directly.
fn resolve_session_recovery_for_startup(data_dir: &Path) -> SessionRecoveryState {
    let lock_path = data_dir.join(SESSION_LOCK_FILENAME);
    let disk_payload = if lock_path.exists() {
        read_session_lock_payload(&lock_path)
    } else {
        None
    };

    if let Some(ref payload) = disk_payload {
        if payload.pid == std::process::id() && payload.session_id > 0 {
            let key = normalize_dir_key(data_dir);
            let mut issued = issued_sessions_lock()
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if let Some((sid, cached_state, consumed)) = issued.get_mut(&key) {
                if *sid == payload.session_id {
                    if !*consumed {
                        *consumed = true;
                        return cached_state.clone();
                    }
                    return SessionRecoveryState::default();
                }
            }
        }
    }

    let (session_id, state) = begin_session_lock_inner(data_dir);
    let key = normalize_dir_key(data_dir);
    let mut issued = issued_sessions_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    if let Some((sid, _, consumed)) = issued.get_mut(&key) {
        if *sid == session_id {
            *consumed = true;
        }
    }
    state
}

#[allow(dead_code)]
pub(crate) fn clear_session_lock_in_dir(data_dir: &Path) {
    let _guard = session_lock_io()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let lock_path = data_dir.join(SESSION_LOCK_FILENAME);
    if lock_path.exists() {
        let _ = fs::remove_file(&lock_path);
    }
    let key = normalize_dir_key(data_dir);
    let mut issued = issued_sessions_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    issued.remove(&key);
}

fn update_session_active_command(
    data_dir: &Path,
    active_command: Option<String>,
    clear_stalled_ts: bool,
    stalled_at_ts: Option<u64>,
) {
    let _guard = session_lock_io()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let lock_path = data_dir.join(SESSION_LOCK_FILENAME);
    let ts = now_unix_secs();
    let mut payload = read_session_lock_payload(&lock_path).unwrap_or_else(|| SessionLockPayload {
        pid: std::process::id(),
        session_id: SESSION_SEQ.load(Ordering::Relaxed),
        started_at_ts: ts,
        heartbeat_ts: ts,
        active_command: None,
        stalled_at_ts: None,
        last_crashed_command: None,
        crash_streak: 0,
    });
    payload.pid = std::process::id();
    payload.heartbeat_ts = ts;
    payload.active_command = active_command;
    if let Some(stalled_ts) = stalled_at_ts {
        payload.stalled_at_ts = Some(stalled_ts);
    } else if clear_stalled_ts {
        payload.stalled_at_ts = None;
    }
    write_session_lock_payload(&lock_path, &payload);
}

#[allow(dead_code)]
pub(crate) fn record_heavy_command_breadcrumb(cmd: &str) -> CommandBreadcrumbGuard {
    let id = SPAN_SEQ.fetch_add(1, Ordering::Relaxed);
    let clean = cmd.trim().to_string();
    {
        let mut guard = active_spans_lock()
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        guard.push(ActiveCommandSpan {
            id,
            command: clean.clone(),
            started_at: Instant::now(),
            stalled_logged: false,
        });
    }
    if !clean.is_empty() {
        if let Some(dir) = watchdog_dir_clone() {
            update_session_active_command(&dir, Some(clean), false, None);
        }
    }
    CommandBreadcrumbGuard { id }
}

fn append_freeze_log_capped(data_dir: &Path, lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    if fs::create_dir_all(data_dir).is_err() {
        return;
    }
    let log_path = data_dir.join(FREEZE_LOG_FILENAME);
    let mut combined = Vec::<u8>::new();
    if let Ok(existing) = fs::read(&log_path) {
        combined.extend_from_slice(&existing);
    }
    for line in lines {
        combined.extend_from_slice(line.as_bytes());
        if !line.ends_with('\n') {
            combined.push(b'\n');
        }
    }
    if combined.len() > FREEZE_LOG_MAX_BYTES {
        let excess = combined.len() - FREEZE_LOG_MAX_BYTES;
        let mut cut = excess;
        while cut < combined.len() && combined[cut] != b'\n' {
            cut += 1;
        }
        if cut < combined.len() && combined[cut] == b'\n' {
            cut += 1;
        } else {
            cut = excess;
        }
        let tail = combined[cut..].to_vec();
        let _ = write_temp_and_swap(&log_path, &tail, ".tmp");
    } else {
        let _ = write_temp_and_swap(&log_path, &combined, ".tmp");
    }
}

fn record_freeze_breadcrumbs_in_dir(
    data_dir: &Path,
    stalled: &[(String, u64)],
    active_stalled_cmd: Option<String>,
) {
    if stalled.is_empty() {
        return;
    }
    let ts = now_unix_secs();
    let pid = std::process::id();
    let lines: Vec<String> = stalled
        .iter()
        .map(|(cmd, ms)| format!("[ts={ts}] pid={pid} stalled_ms={ms} command={cmd}"))
        .collect();
    append_freeze_log_capped(data_dir, &lines);
    if let Some(cmd) = active_stalled_cmd {
        update_session_active_command(data_dir, Some(cmd), false, Some(ts));
    }
}

fn evaluate_active_spans_in_dir(data_dir: &Path, threshold: Duration) -> usize {
    let mut newly_stalled: Vec<(String, u64)> = Vec::new();
    let mut active_stalled_cmd: Option<String> = None;

    {
        let mut spans = active_spans_lock()
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        for span in spans.iter_mut() {
            let elapsed = span.started_at.elapsed();
            if elapsed > threshold {
                if active_stalled_cmd.is_none() {
                    active_stalled_cmd = Some(span.command.clone());
                }
                if !span.stalled_logged {
                    span.stalled_logged = true;
                    newly_stalled.push((span.command.clone(), elapsed.as_millis() as u64));
                }
            }
        }
    }

    let count = newly_stalled.len();
    if count > 0 {
        record_freeze_breadcrumbs_in_dir(data_dir, &newly_stalled, active_stalled_cmd);
    }
    count
}

#[cfg(test)]
fn evaluate_span_slice_in_dir(
    data_dir: &Path,
    spans: &mut [ActiveCommandSpan],
    threshold: Duration,
) -> usize {
    let mut newly_stalled: Vec<(String, u64)> = Vec::new();
    let mut active_stalled_cmd: Option<String> = None;
    for span in spans.iter_mut() {
        let elapsed = span.started_at.elapsed();
        if elapsed > threshold {
            if active_stalled_cmd.is_none() {
                active_stalled_cmd = Some(span.command.clone());
            }
            if !span.stalled_logged {
                span.stalled_logged = true;
                newly_stalled.push((span.command.clone(), elapsed.as_millis() as u64));
            }
        }
    }
    let count = newly_stalled.len();
    if count > 0 {
        record_freeze_breadcrumbs_in_dir(data_dir, &newly_stalled, active_stalled_cmd);
    }
    count
}

pub(crate) fn start_ipc_watchdog(data_dir: PathBuf) {
    {
        let mut dir_slot = watchdog_data_dir_lock()
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        *dir_slot = Some(data_dir.clone());
    }
    if WATCHDOG_STARTED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    let _ = std::thread::Builder::new()
        .name("duke-ipc-watchdog".to_string())
        .spawn(move || {
            let threshold = Duration::from_millis(WATCHDOG_STALL_THRESHOLD_MS);
            let poll = Duration::from_millis(WATCHDOG_POLL_MS);
            loop {
                std::thread::sleep(poll);
                let target_dir = watchdog_dir_clone().unwrap_or_else(|| data_dir.clone());
                let _ = evaluate_active_spans_in_dir(&target_dir, threshold);
            }
        });
}

#[allow(dead_code)]
pub(crate) fn get_self_heal_status(data_dir: &Path) -> SelfHealDiagnostics {
    let lock_payload = read_session_lock_payload(&data_dir.join(SESSION_LOCK_FILENAME));
    let disabled_subsystems = {
        let _guard = disabled_jobs_io()
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        load_disabled_jobs_unlocked(data_dir)
    };
    let active_in_flight_commands = {
        let guard = active_spans_lock()
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        guard.iter().map(|s| s.command.clone()).collect()
    };

    let freeze_path = data_dir.join(FREEZE_LOG_FILENAME);
    let freeze_log_bytes = fs::metadata(&freeze_path).map(|m| m.len()).unwrap_or(0);
    let freeze_log_tail = read_log_tail_lines(&freeze_path, 10);

    let quarantine_dir = data_dir.join("quarantine");
    let quarantine_count = if quarantine_dir.is_dir() {
        WalkDir::new(&quarantine_dir)
            .follow_links(false)
            .max_depth(6)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .count()
    } else {
        0
    };

    let dkcat_path = data_dir.join("catalog.dkcat");
    let catalog_valid = if !dkcat_path.exists() {
        true
    } else {
        MappedCatalog::open(&dkcat_path)
            .map(|c| c.verify_header_fast())
            .unwrap_or(false)
    };

    let key = normalize_dir_key(data_dir);
    let last_report = last_heal_reports_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(&key)
        .cloned();

    SelfHealDiagnostics {
        session_lock_active: lock_payload.is_some(),
        current_pid: lock_payload.as_ref().map(|p| p.pid),
        active_command: lock_payload.as_ref().and_then(|p| p.active_command.clone()),
        stalled_at_ts: lock_payload.as_ref().and_then(|p| p.stalled_at_ts),
        crash_streak: lock_payload.as_ref().map(|p| p.crash_streak).unwrap_or(0),
        disabled_subsystems,
        active_in_flight_commands,
        freeze_log_bytes,
        freeze_log_tail,
        quarantine_count,
        catalog_valid,
        last_report,
    }
}

fn read_log_tail_lines(path: &Path, max_lines: usize) -> Vec<String> {
    let Ok(mut file) = File::open(path) else {
        return Vec::new();
    };
    let Ok(meta) = file.metadata() else {
        return Vec::new();
    };
    let len = meta.len();
    if len == 0 {
        return Vec::new();
    }
    let window = len.min(8 * 1024) as usize;
    let start = len.saturating_sub(window as u64);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return Vec::new();
    }
    let mut buf = vec![0u8; window];
    let mut read_total = 0usize;
    while read_total < window {
        match file.read(&mut buf[read_total..]) {
            Ok(0) => break,
            Ok(n) => read_total += n,
            Err(_) => break,
        }
    }
    let text = String::from_utf8_lossy(&buf[..read_total]);
    let mut lines: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(ToString::to_string)
        .collect();
    if lines.len() > max_lines {
        let drop_cnt = lines.len() - max_lines;
        lines.drain(0..drop_cnt);
    }
    lines
}

fn quarantine_file_in_dir(data_dir: &Path, src: &Path, ts: u64) -> Option<String> {
    if !src.is_file() {
        return None;
    }
    let q_base = data_dir.join("quarantine").join(ts.to_string());
    let rel = src
        .strip_prefix(data_dir)
        .ok()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| {
            PathBuf::from(
                src.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("corrupt.bin"),
            )
        });
    let mut dest = q_base.join(&rel);
    if dest.exists() {
        let seq = SELF_HEAL_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let name = dest
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("corrupt.bin")
            .to_string();
        dest = dest.with_file_name(format!("{name}.{seq}"));
    }
    if let Some(parent) = dest.parent() {
        if fs::create_dir_all(parent).is_err() {
            return None;
        }
    }

    let moved = match fs::rename(src, &dest) {
        Ok(()) => true,
        Err(_) => {
            if fs::copy(src, &dest).is_ok() {
                let _ = fs::remove_file(src);
                true
            } else {
                false
            }
        }
    };
    if !moved {
        return None;
    }

    let display = dest
        .strip_prefix(data_dir)
        .ok()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|| dest.to_string_lossy().into_owned());
    Some(display)
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

/// Verifies `catalog.dkcat` header & checksum integrity; removes corrupt archives cleanly.
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

fn write_minimal_empty_dkcat(dkcat_path: &Path) -> bool {
    let Ok(bytes) = crate::catalog_mmap::serialize_dkcat(&crate::CatalogCacheFile::default()) else {
        return false;
    };
    write_temp_and_swap(dkcat_path, &bytes, ".tmp").is_ok()
}

fn repair_or_rebuild_catalog_in_dir(
    data_dir: &Path,
    heal_ts: u64,
    report: &mut StartupSelfHealReport,
) {
    let dkcat_path = data_dir.join("catalog.dkcat");
    if dkcat_path.exists() {
        let header_ok = {
            let meta_ok = fs::metadata(&dkcat_path)
                .map(|m| m.is_file() && m.len() >= 64)
                .unwrap_or(false);
            if meta_ok {
                match MappedCatalog::open(&dkcat_path) {
                    Ok(cat) => cat.verify_header_fast(),
                    Err(_) => matches!(
                        crate::catalog_mmap::try_migrate_dkcat_v1_to_v2(&dkcat_path),
                        Ok(true)
                    ),
                }
            } else {
                false
            }
        };
        if header_ok {
            return;
        }
        if let Some(q_path) = quarantine_file_in_dir(data_dir, &dkcat_path, heal_ts) {
            report.quarantined_files.push(q_path);
        } else {
            let _ = fs::remove_file(&dkcat_path);
        }
    }

    if write_minimal_empty_dkcat(&dkcat_path) {
        report.catalog_rebuilt = true;
    }
}

fn repair_json_files_and_sites(
    data_dir: &Path,
    heal_ts: u64,
    report: &mut StartupSelfHealReport,
) {
    for name in PROTECTED_JSON_FILES {
        let p = data_dir.join(name);
        if matches!(recover_protected_json_if_corrupt(&p), Ok(true)) {
            report.restored_json_files.push((*name).to_string());
        }
    }

    // Verify settings.json parses into a valid JSON object & LauncherSettings; otherwise quarantine + reset `{}`.
    let settings_path = data_dir.join("settings.json");
    if settings_path.exists() {
        let settings_ok = fs::read(&settings_path).ok().is_some_and(|bytes| {
            !bytes.is_empty()
                && serde_json::from_slice::<Value>(&bytes)
                    .ok()
                    .is_some_and(|v| v.is_object())
                && serde_json::from_slice::<crate::LauncherSettings>(&bytes).is_ok()
        });
        if !settings_ok {
            if let Some(q_path) = quarantine_file_in_dir(data_dir, &settings_path, heal_ts) {
                report.quarantined_files.push(q_path);
            }
            if atomic_write_with_backup(&settings_path, b"{}\n").is_ok() {
                report.settings_repaired = true;
            }
        }
    }

    // Quarantine unrecoverable corrupt library.json / sources.json so UI/CLI startup never stalls.
    for array_json_name in ["library.json", "sources.json"] {
        let p = data_dir.join(array_json_name);
        if p.exists() {
            let valid_array = fs::read(&p).ok().is_some_and(|bytes| {
                !bytes.is_empty() && serde_json::from_slice::<Vec<Value>>(&bytes).is_ok()
            });
            if !valid_array {
                if let Some(q_path) = quarantine_file_in_dir(data_dir, &p, heal_ts) {
                    report.quarantined_files.push(q_path);
                }
                let _ = atomic_write_with_backup(&p, b"[]\n");
            }
        }
    }

    // Verify search_sites.json and restore any missing entries from the 8 default directories.
    let sites_path = data_dir.join("search_sites.json");
    if sites_path.exists() {
        let valid_sites = fs::read_to_string(&sites_path).ok().is_some_and(|raw| {
            let trimmed = raw.trim();
            trimmed.is_empty()
                || serde_json::from_str::<Vec<crate::sites::SearchSiteEntry>>(trimmed).is_ok()
        });
        if !valid_sites {
            if let Some(q_path) = quarantine_file_in_dir(data_dir, &sites_path, heal_ts) {
                report.quarantined_files.push(q_path);
            }
        }
    }
    if let Ok(loaded_sites) = crate::sites::load_search_sites_from_path(&sites_path) {
        let defaults = crate::sites::default_search_sites();
        let all_defaults_present = defaults.iter().all(|def| {
            loaded_sites
                .iter()
                .any(|entry| entry.id.eq_ignore_ascii_case(&def.id) || entry.home_url == def.home_url)
        });
        report.sites_verified = all_defaults_present && loaded_sites.len() >= defaults.len();
    }
}

fn extract_blake3_hex_from_blob_path(path: &Path) -> Option<String> {
    let ext = path.extension().and_then(|e| e.to_str());
    let candidate = match ext {
        Some(e) if e.eq_ignore_ascii_case("blob") => path.file_stem()?.to_str()?,
        None => path.file_name()?.to_str()?,
        Some(_) => return None,
    };
    if candidate.len() == 64 && candidate.bytes().all(|b| b.is_ascii_hexdigit()) {
        Some(candidate.to_ascii_lowercase())
    } else {
        None
    }
}

fn hash_file_blake3_heap_chunked(
    path: &Path,
    buf: &mut [u8],
) -> Result<(String, u64), std::io::Error> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut total = 0u64;
    loop {
        let n = file.read(buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total = total.saturating_add(n as u64);
    }
    Ok((hasher.finalize().to_hex().to_string(), total))
}

fn verify_cas_blobs_in_dir(data_dir: &Path, heal_ts: u64, report: &mut StartupSelfHealReport) {
    let candidate_roots = [
        data_dir.join(".duke-cas").join("blobs"),
        data_dir.join("snapshots").join(".duke-cas").join("blobs"),
        data_dir.join("snapshots").join("blobs"),
    ];

    let mut seen_dirs = HashSet::<PathBuf>::new();
    let mut buf = vec![0u8; CAS_HASH_CHUNK_BYTES];
    let started = Instant::now();
    let mut bytes_hashed = 0u64;

    for root in candidate_roots {
        if !root.is_dir() {
            continue;
        }
        let canon = normalize_dir_key(&root);
        if !seen_dirs.insert(canon) {
            continue;
        }
        for entry in WalkDir::new(&root)
            .follow_links(false)
            .max_depth(4)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            if bytes_hashed >= CAS_STARTUP_BYTE_BUDGET
                || started.elapsed().as_millis() as u64 >= CAS_STARTUP_TIME_BUDGET_MS
            {
                return;
            }
            let path = entry.path();
            let Some(expected_hex) = extract_blake3_hex_from_blob_path(path) else {
                continue;
            };
            let file_len = entry.metadata().map(|m| m.len()).unwrap_or(0);
            if bytes_hashed > 0 && bytes_hashed.saturating_add(file_len) > CAS_STARTUP_BYTE_BUDGET {
                continue;
            }
            report.cas_blobs_checked += 1;
            match hash_file_blake3_heap_chunked(path, &mut buf) {
                Ok((actual_hex, read_bytes)) => {
                    bytes_hashed = bytes_hashed.saturating_add(read_bytes);
                    if !actual_hex.eq_ignore_ascii_case(&expected_hex) {
                        if let Some(q_path) = quarantine_file_in_dir(data_dir, path, heal_ts) {
                            report.quarantined_files.push(q_path);
                            report.cas_blobs_quarantined += 1;
                        }
                    }
                }
                Err(_) => {
                    if let Some(q_path) = quarantine_file_in_dir(data_dir, path, heal_ts) {
                        report.quarantined_files.push(q_path);
                        report.cas_blobs_quarantined += 1;
                    }
                }
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn is_pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    use windows::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED};
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    const STILL_ACTIVE_CODE: u32 = 259;

    unsafe {
        match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(handle) => {
                let mut exit_code: u32 = 0;
                let ok = GetExitCodeProcess(handle, &mut exit_code).is_ok();
                let _ = CloseHandle(handle);
                ok && exit_code == STILL_ACTIVE_CODE
            }
            Err(err) => err.code() == ERROR_ACCESS_DENIED.to_hresult(),
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn is_pid_alive(pid: u32) -> bool {
    pid == std::process::id()
}

fn parse_singleton_lock_pid(raw: &str) -> Option<u32> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(pid) = trimmed.parse::<u32>() {
        return (pid > 0).then_some(pid);
    }
    if let Some((_, tail)) = trimmed.rsplit_once('-') {
        if let Ok(pid) = tail.trim().parse::<u32>() {
            return (pid > 0).then_some(pid);
        }
    }
    if let Some(rest) = trimmed.strip_prefix("pid=") {
        if let Ok(pid) = rest.trim().parse::<u32>() {
            return (pid > 0).then_some(pid);
        }
    }
    None
}

fn is_lock_file_in_use(lock_path: &Path, check_pid_content: bool) -> bool {
    let Ok(meta) = fs::symlink_metadata(lock_path) else {
        return false;
    };
    if meta.file_type().is_symlink() {
        if let Ok(target) = fs::read_link(lock_path) {
            if let Some(pid) = parse_singleton_lock_pid(&target.to_string_lossy()) {
                if is_pid_alive(pid) {
                    return true;
                }
            }
        }
        return false;
    }
    if !meta.file_type().is_file() {
        return false;
    }
    if check_pid_content && meta.len() > 0 && meta.len() <= 256 {
        if let Ok(raw) = fs::read_to_string(lock_path) {
            if let Some(pid) = parse_singleton_lock_pid(&raw) {
                if is_pid_alive(pid) {
                    return true;
                }
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::OpenOptionsExt;
        match OpenOptions::new().read(true).share_mode(0).open(lock_path) {
            Ok(file) => {
                drop(file);
                false
            }
            Err(_) => true,
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

/// Removes stale WebView2 `SingletonLock`, `SingletonCookie`, and `SingletonSocket` files in
/// `<data_dir>/EBWebView` (and `WEBVIEW2_USER_DATA_FOLDER` when configured), skipping any
/// directory whose lock files are still held open or owned by a live process.
pub(crate) fn reset_stale_webview2_locks(data_dir: &Path) -> usize {
    const LOCK_FILES: &[&str] = &["SingletonLock", "SingletonCookie", "SingletonSocket"];

    let mut dirs = Vec::<PathBuf>::new();
    dirs.push(data_dir.join("EBWebView"));
    if let Some(custom) = std::env::var_os("WEBVIEW2_USER_DATA_FOLDER") {
        let custom_path = PathBuf::from(custom);
        if !custom_path.as_os_str().is_empty() {
            dirs.push(custom_path.clone());
            dirs.push(custom_path.join("EBWebView"));
        }
    }

    let mut seen = HashSet::<PathBuf>::new();
    let mut cleared = 0usize;

    for dir in dirs {
        if !dir.is_dir() {
            continue;
        }
        if !seen.insert(normalize_dir_key(&dir)) {
            continue;
        }

        let existing_locks: Vec<(PathBuf, bool)> = LOCK_FILES
            .iter()
            .map(|name| (dir.join(name), *name == "SingletonLock"))
            .filter(|(p, _)| fs::symlink_metadata(p).is_ok())
            .collect();

        if existing_locks.is_empty() {
            continue;
        }

        let dir_is_live = existing_locks
            .iter()
            .any(|(path, check_pid)| is_lock_file_in_use(path, *check_pid));
        if dir_is_live {
            continue;
        }

        for (lock_path, _) in existing_locks {
            if fs::remove_file(&lock_path).is_ok() {
                cleared += 1;
            }
        }
    }

    cleared
}

fn build_toast_summary(report: &StartupSelfHealReport) -> String {
    let mut parts = Vec::<String>::new();
    if report.unclean_previous_exit {
        if let Some(ref cmd) = report.last_stalled_command {
            parts.push(format!(
                "Recovered from unexpected exit (last task: {cmd})"
            ));
        } else {
            parts.push("Recovered from unexpected exit".to_string());
        }
    }
    if !report.restored_json_files.is_empty() {
        parts.push(format!(
            "Restored {} state file(s)",
            report.restored_json_files.len()
        ));
    }
    if report.dklib_repaired {
        parts.push("Repaired library.dklib".to_string());
    }
    if report.settings_repaired {
        parts.push("Reset corrupt settings.json".to_string());
    }
    if report.catalog_rebuilt {
        parts.push("Rebuilt catalog.dkcat index".to_string());
    }
    if report.recovered_downloads > 0 {
        parts.push(format!(
            "Paused {} interrupted download(s)",
            report.recovered_downloads
        ));
    }
    if report.cleaned_tmp_files > 0 {
        parts.push(format!(
            "Cleaned {} temp file(s)",
            report.cleaned_tmp_files
        ));
    }
    if report.cas_blobs_quarantined > 0 {
        parts.push(format!(
            "Quarantined {} corrupt CAS blob(s)",
            report.cas_blobs_quarantined
        ));
    } else if !report.quarantined_files.is_empty() {
        parts.push(format!(
            "Quarantined {} corrupt file(s)",
            report.quarantined_files.len()
        ));
    }
    if report.unclean_previous_exit && !report.disabled_subsystems.is_empty() {
        parts.push(format!(
            "Safe-disabled {} unstable subsystem(s)",
            report.disabled_subsystems.len()
        ));
    }
    if report.webview_locks_cleared > 0 {
        parts.push(format!(
            "Cleared {} stale WebView2 lock(s)",
            report.webview_locks_cleared
        ));
    }
    parts.join(" · ")
}

/// Runs startup state & backup recovery, interrupted queue recovery, `library.dklib` /
/// `catalog.dkcat` / `search_sites.json` / `.duke-cas` repair, WebView2 lock cleanup,
/// and unclean-exit quarantine.
pub(crate) fn run_startup_self_heal_in_dir(data_dir: &Path) -> StartupSelfHealReport {
    let mut report = StartupSelfHealReport::default();
    let heal_ts = now_unix_secs();

    report.cleaned_tmp_files = cleanup_orphan_tmp_files(data_dir);

    let session_recovery = resolve_session_recovery_for_startup(data_dir);
    report.unclean_previous_exit = session_recovery.unclean_previous_exit;
    report.last_stalled_command = session_recovery.active_command.clone();

    if let Ok(outcome) = crate::lib_store::verify_and_repair_dklib(data_dir) {
        report.dklib_repaired = outcome.repaired || outcome.recovered_from_backup_or_json;
    }

    repair_json_files_and_sites(data_dir, heal_ts, &mut report);

    // If downloads.json itself is corrupt JSON and had no valid .bak, quarantine & reset cleanly.
    let dl_path = data_dir.join("downloads.json");
    if dl_path.exists() {
        let _ = recover_protected_json_if_corrupt(&dl_path);
        let dl_ok = fs::read(&dl_path).ok().is_some_and(|b| {
            !b.is_empty() && serde_json::from_slice::<Vec<Value>>(&b).is_ok()
        });
        if !dl_ok {
            if let Some(q_path) = quarantine_file_in_dir(data_dir, &dl_path, heal_ts) {
                report.quarantined_files.push(q_path);
            }
            let _ = write_temp_and_swap(&dl_path, b"[]\n", ".tmp");
        }
    }
    report.recovered_downloads = recover_interrupted_downloads_in_dir(data_dir);

    repair_or_rebuild_catalog_in_dir(data_dir, heal_ts, &mut report);
    report.catalog_valid = verify_catalog_integrity(data_dir);

    verify_cas_blobs_in_dir(data_dir, heal_ts, &mut report);
    report.webview_locks_cleared = reset_stale_webview2_locks(data_dir);

    {
        let _guard = disabled_jobs_io()
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        report.disabled_subsystems = load_disabled_jobs_unlocked(data_dir)
            .into_iter()
            .map(|e| e.name)
            .collect();
    }

    report.toast_summary = build_toast_summary(&report);

    let key = normalize_dir_key(data_dir);
    let mut reports = last_heal_reports_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    reports.insert(key, report.clone());

    report
}

/// Non-blocking startup self-heal pass scheduled from `.setup()`.
pub(crate) fn spawn_startup_self_heal(app: tauri::AppHandle) {
    use tauri::Emitter;
    tauri::async_runtime::spawn(async move {
        let Ok(data_dir) = crate::resolve_data_root_dir(&app) else {
            return;
        };
        let watch_dir = data_dir.clone();
        let report = tokio::task::spawn_blocking(move || run_startup_self_heal_in_dir(&data_dir))
            .await
            .unwrap_or_default();

        start_ipc_watchdog(watch_dir);

        if report.recovered_downloads > 0 {
            crate::reload_queue_store_from_disk(&app);
        }
        if report.unclean_previous_exit
            || !report.restored_json_files.is_empty()
            || report.recovered_downloads > 0
            || report.cleaned_tmp_files > 0
            || !report.quarantined_files.is_empty()
            || report.dklib_repaired
            || report.settings_repaired
            || report.catalog_rebuilt
            || report.cas_blobs_quarantined > 0
            || report.webview_locks_cleared > 0
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

    fn unique_test_dir(tag: &str) -> PathBuf {
        static TEST_DIR_SEQ: AtomicU64 = AtomicU64::new(1);
        let seq = TEST_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "duke_self_heal_{tag}_{}_{}",
            std::process::id(),
            seq
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

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

    #[test]
    fn dirty_session_lock_tracks_crash_streak_and_auto_disables_repeat_stalls() {
        let dir = unique_test_dir("session_lock");

        // 1. Clean start: no session.lock
        let clean = begin_session_lock_in_dir(&dir);
        assert!(!clean.unclean_previous_exit);
        assert_eq!(clean.crash_streak, 0);
        assert!(dir.join(SESSION_LOCK_FILENAME).exists());

        // Graceful exit clears session.lock
        clear_session_lock_in_dir(&dir);
        assert!(!dir.join(SESSION_LOCK_FILENAME).exists());

        // 2. Crash #1 while running plugin:bad_meta
        let dirty_1 = serde_json::json!({
            "pid": 999999u32,
            "activeCommand": "plugin:bad_meta",
            "stalledAtTs": 1_700_000_001u64,
            "crashStreak": 0u32
        });
        fs::write(
            dir.join(SESSION_LOCK_FILENAME),
            serde_json::to_string_pretty(&dirty_1).unwrap(),
        )
        .unwrap();

        let rec_1 = begin_session_lock_in_dir(&dir);
        assert!(rec_1.unclean_previous_exit);
        assert_eq!(rec_1.active_command.as_deref(), Some("plugin:bad_meta"));
        assert_eq!(rec_1.crash_streak, 1);
        assert!(rec_1.disabled_subsystem.is_none());
        assert!(!is_subsystem_safe_disabled(&dir, "plugin:bad_meta"));

        // 3. Crash #2 under the SAME command -> streak == 2 -> auto-disabled!
        let mut lock_payload: SessionLockPayload =
            serde_json::from_str(&fs::read_to_string(dir.join(SESSION_LOCK_FILENAME)).unwrap())
                .unwrap();
        lock_payload.active_command = Some("plugin:bad_meta".to_string());
        write_session_lock_payload(&dir.join(SESSION_LOCK_FILENAME), &lock_payload);

        let rec_2 = begin_session_lock_in_dir(&dir);
        assert!(rec_2.unclean_previous_exit);
        assert_eq!(rec_2.crash_streak, 2);
        assert_eq!(
            rec_2.disabled_subsystem.as_deref(),
            Some("plugin:bad_meta")
        );
        assert!(is_subsystem_safe_disabled(&dir, "plugin:bad_meta"));

        // Startup heal consumes rec_2 and populates report + diagnostics
        let report = run_startup_self_heal_in_dir(&dir);
        assert!(report.unclean_previous_exit);
        assert_eq!(
            report.last_stalled_command.as_deref(),
            Some("plugin:bad_meta")
        );
        assert!(report
            .disabled_subsystems
            .contains(&"plugin:bad_meta".to_string()));

        let diag = get_self_heal_status(&dir);
        assert!(diag.session_lock_active);
        assert_eq!(diag.crash_streak, 2);
        assert_eq!(diag.disabled_subsystems.len(), 1);
        assert!(diag.last_report.is_some());

        // Reset clears disabled_jobs.json entry
        assert!(reset_disabled_subsystem(&dir, "plugin:bad_meta").unwrap());
        assert!(!is_subsystem_safe_disabled(&dir, "plugin:bad_meta"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn watchdog_stall_logs_freeze_breadcrumb_capped_at_256kb_and_updates_session_lock() {
        let dir = unique_test_dir("watchdog_freeze");
        let _ = begin_session_lock_in_dir(&dir);

        // Guard adds and drops span cleanly
        {
            let _guard = record_heavy_command_breadcrumb("job:quick_test");
            let active = active_spans_lock()
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .iter()
                .any(|s| s.command == "job:quick_test");
            assert!(active);
        }
        let still_there = active_spans_lock()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .any(|s| s.command == "job:quick_test");
        assert!(!still_there);

        // Synthetic stalled span > 3000ms logs once and flushes to session.lock
        let stalled_start = Instant::now()
            .checked_sub(Duration::from_millis(3_500))
            .unwrap_or_else(Instant::now);
        let mut spans = vec![ActiveCommandSpan {
            id: 9001,
            command: "job:retro_sync".to_string(),
            started_at: stalled_start,
            stalled_logged: false,
        }];
        let logged = evaluate_span_slice_in_dir(&dir, &mut spans, Duration::from_millis(3_000));
        if Instant::now().checked_sub(Duration::from_millis(3_500)).is_some() {
            assert_eq!(logged, 1);
            assert!(spans[0].stalled_logged);
            // Second pass must NOT duplicate-log the same span
            assert_eq!(
                evaluate_span_slice_in_dir(&dir, &mut spans, Duration::from_millis(3_000)),
                0
            );
        } else {
            record_freeze_breadcrumbs_in_dir(
                &dir,
                &[("job:retro_sync".to_string(), 3_500)],
                Some("job:retro_sync".to_string()),
            );
        }

        let log_raw = fs::read_to_string(dir.join(FREEZE_LOG_FILENAME)).unwrap();
        assert!(log_raw.contains("command=job:retro_sync"));

        let lock_after: SessionLockPayload =
            serde_json::from_str(&fs::read_to_string(dir.join(SESSION_LOCK_FILENAME)).unwrap())
                .unwrap();
        assert_eq!(lock_after.active_command.as_deref(), Some("job:retro_sync"));
        assert!(lock_after.stalled_at_ts.is_some());

        // Verify 256 KB log cap when appending past 262,144 bytes
        let filler_line = format!("[ts=1] pid=1 stalled_ms=3100 command={}\n", "x".repeat(200));
        let mut big_lines = Vec::new();
        for _ in 0..1_200 {
            big_lines.push(filler_line.clone());
        }
        append_freeze_log_capped(&dir, &big_lines);
        append_freeze_log_capped(
            &dir,
            &["[ts=999] pid=1 stalled_ms=4200 command=job:newest_tail".to_string()],
        );
        let log_len = fs::metadata(dir.join(FREEZE_LOG_FILENAME)).unwrap().len();
        assert!(log_len <= FREEZE_LOG_MAX_BYTES as u64);
        let tail_text = fs::read_to_string(dir.join(FREEZE_LOG_FILENAME)).unwrap();
        assert!(tail_text.contains("command=job:newest_tail"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_catalog_and_settings_are_quarantined_and_repaired() {
        let isolated_verify_dir = unique_test_dir("verify_only");
        fs::write(isolated_verify_dir.join("catalog.dkcat"), b"BADCAT00").unwrap();
        assert!(!verify_catalog_integrity(&isolated_verify_dir));
        let _ = fs::remove_dir_all(&isolated_verify_dir);

        let dir = unique_test_dir("catalog_settings_repair");
        fs::write(dir.join("catalog.dkcat"), b"CORRUPT_DKCAT_HEADER").unwrap();
        fs::write(dir.join("settings.json"), b"{corrupt_settings_json").unwrap();

        let report = run_startup_self_heal_in_dir(&dir);
        assert!(report.catalog_rebuilt);
        assert!(report.catalog_valid);
        assert!(report.settings_repaired);
        assert!(report.quarantined_files.len() >= 2);
        assert!(verify_catalog_integrity(&dir));
        assert!(MappedCatalog::open(&dir.join("catalog.dkcat")).is_ok());
        assert!(!report.toast_summary.is_empty());

        let repaired_settings: Value =
            serde_json::from_str(&fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
        assert!(repaired_settings.is_object());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_cas_blobs_are_quarantined_while_valid_blobs_remain() {
        let dir = unique_test_dir("cas_verify");
        let good_payload = b"duke-valid-cas-blob-bytes";
        let good_hash = blake3::hash(good_payload).to_hex().to_string();
        let bad_payload_hash = blake3::hash(b"expected-content").to_hex().to_string();
        let flat_bad_hash = blake3::hash(b"expected-flat-content").to_hex().to_string();

        let bucket_a = dir.join(".duke-cas").join("blobs").join(&good_hash[..2]);
        let bucket_b = dir
            .join(".duke-cas")
            .join("blobs")
            .join(&bad_payload_hash[..2]);
        let snap_blobs = dir
            .join("snapshots")
            .join(".duke-cas")
            .join("blobs");
        fs::create_dir_all(&bucket_a).unwrap();
        fs::create_dir_all(&bucket_b).unwrap();
        fs::create_dir_all(&snap_blobs).unwrap();

        let good_path = bucket_a.join(format!("{good_hash}.blob"));
        let bad_path = bucket_b.join(format!("{bad_payload_hash}.blob"));
        let flat_bad_path = snap_blobs.join(&flat_bad_hash);

        fs::write(&good_path, good_payload).unwrap();
        fs::write(&bad_path, b"tampered-corrupt-bytes").unwrap();
        fs::write(&flat_bad_path, b"tampered-flat-bytes").unwrap();

        let report = run_startup_self_heal_in_dir(&dir);
        assert_eq!(report.cas_blobs_checked, 3);
        assert_eq!(report.cas_blobs_quarantined, 2);
        assert!(good_path.exists());
        assert!(!bad_path.exists());
        assert!(!flat_bad_path.exists());
        assert_eq!(
            report
                .quarantined_files
                .iter()
                .filter(|p| p.contains("blobs"))
                .count(),
            2
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn search_sites_restores_eight_defaults_and_clears_only_stale_webview_locks() {
        let dir = unique_test_dir("sites_and_webview");
        let defaults = crate::sites::default_search_sites();
        assert_eq!(defaults.len(), 8);

        // Seed only 1 default + 1 custom entry -> self-heal must restore all 8 defaults + preserve custom.
        let seeded = serde_json::json!([
            defaults[0],
            {
                "id": "custom_test_dir",
                "name": "Custom Test Directory",
                "searchUrl": "https://example.org/search?q={query}",
                "homeUrl": "https://example.org/",
                "tag": "Custom",
                "note": "Test",
                "isDefault": false
            }
        ]);
        fs::write(
            dir.join("search_sites.json"),
            serde_json::to_string_pretty(&seeded).unwrap(),
        )
        .unwrap();

        // Stale WebView2 locks (dead PID + unheld files)
        let webview_dir = dir.join("EBWebView");
        fs::create_dir_all(&webview_dir).unwrap();
        fs::write(webview_dir.join("SingletonLock"), b"HOST-999999999").unwrap();
        fs::write(webview_dir.join("SingletonCookie"), b"123456789").unwrap();
        fs::write(webview_dir.join("SingletonSocket"), b"").unwrap();

        let report = run_startup_self_heal_in_dir(&dir);
        assert!(report.sites_verified);
        assert_eq!(report.webview_locks_cleared, 3);
        assert!(!webview_dir.join("SingletonLock").exists());
        assert!(!webview_dir.join("SingletonCookie").exists());
        assert!(!webview_dir.join("SingletonSocket").exists());

        let repaired_sites: Vec<crate::sites::SearchSiteEntry> =
            serde_json::from_str(&fs::read_to_string(dir.join("search_sites.json")).unwrap())
                .unwrap();
        assert_eq!(repaired_sites.len(), 9);
        for def in &defaults {
            assert!(
                repaired_sites.iter().any(|s| s.id == def.id),
                "Missing default directory {}",
                def.id
            );
        }
        assert!(repaired_sites.iter().any(|s| s.id == "custom_test_dir"));

        // Live-PID SingletonLock must NOT be cleared
        let live_dir = unique_test_dir("webview_live_pid");
        let live_wv = live_dir.join("EBWebView");
        fs::create_dir_all(&live_wv).unwrap();
        fs::write(
            live_wv.join("SingletonLock"),
            format!("LOCALHOST-{}", std::process::id()),
        )
        .unwrap();
        fs::write(live_wv.join("SingletonCookie"), b"987654321").unwrap();
        assert_eq!(reset_stale_webview2_locks(&live_dir), 0);
        assert!(live_wv.join("SingletonLock").exists());
        assert!(live_wv.join("SingletonCookie").exists());

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&live_dir);
    }
}

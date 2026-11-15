//! Binary `library.dklib` (`postcard` + CRC32) store with append-only WAL journal,
//! atomic `.bak` snapshots, and lossless `library.json` import/export.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

const DKLIB_MAGIC: &[u8; 8] = b"DKLIB001";
const DKLIB_VERSION: u32 = 1;
const DKLIB_HEADER_LEN: usize = 20;
const DKLIB_MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;

const DKWL_MAGIC: &[u8; 4] = b"DKWL";
const DKWL_HEADER_LEN: usize = 12;
const DKWL_MAX_RECORD_BYTES: u32 = 16 * 1024 * 1024;

const WAL_STREAM_BUF_BYTES: usize = 64 * 1024;
const WAL_COMPACT_THRESHOLD: usize = 64;

static LIB_STORE_TMP_SEQ: AtomicU64 = AtomicU64::new(1);
static DIR_STATES: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<DirState>>>>> = OnceLock::new();

struct DirState {
    wal_count: Option<usize>,
    wal_file: Option<File>,
}

impl DirState {
    fn new() -> Self {
        Self {
            wal_count: None,
            wal_file: None,
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DklibHealOutcome {
    pub repaired: bool,
    pub replayed_wal: usize,
    pub recovered_from_backup_or_json: bool,
    pub detail: String,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum WalOp {
    Upsert(Box<crate::GameEntry>),
    Delete {
        id: String,
    },
    PatchPlaytime {
        id: String,
        playtime_seconds: u64,
        last_played_ts: u64,
    },
    PatchCover {
        id: String,
        cover_url: String,
        hero_url: Option<String>,
        icon_path: Option<String>,
    },
    PatchFavorite {
        id: String,
        is_favorite: bool,
    },
    PatchLaunchArgs {
        id: String,
        launch_args: String,
    },
    PatchLaunchProfile {
        id: String,
        sandbox_enabled: bool,
        nvme_prefetch_override: Option<bool>,
    },
}

struct ParsedWal {
    ops: Vec<WalOp>,
    valid_bytes: u64,
    truncated_tail: bool,
}

fn dir_state_lock(data_dir: &Path) -> Arc<Mutex<DirState>> {
    let map = DIR_STATES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = map.lock().unwrap_or_else(|p| p.into_inner());
    guard
        .entry(data_dir.to_path_buf())
        .or_insert_with(|| Arc::new(Mutex::new(DirState::new())))
        .clone()
}

fn dklib_path(data_dir: &Path) -> PathBuf {
    data_dir.join("library.dklib")
}

fn dklib_bak_path(data_dir: &Path) -> PathBuf {
    data_dir.join("library.dklib.bak")
}

fn dklib_wal_path(data_dir: &Path) -> PathBuf {
    data_dir.join("library.dklib.wal")
}

fn library_json_path(data_dir: &Path) -> PathBuf {
    data_dir.join("library.json")
}

fn library_json_bak_path(data_dir: &Path) -> PathBuf {
    data_dir.join("library.json.bak")
}

fn temp_path_for(dest_path: &Path, suffix: &str) -> PathBuf {
    let seq = LIB_STORE_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let base_name = dest_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("library.dklib");
    dest_path.with_file_name(format!(
        "{}.{}.{}{}",
        base_name,
        std::process::id(),
        seq,
        suffix
    ))
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
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory {}: {}", parent.display(), e))?;
        }
    }
    let tmp_path = temp_path_for(dest_path, tmp_suffix);
    let write_res = (|| -> Result<(), String> {
        let mut file = File::create(&tmp_path)
            .map_err(|e| format!("Failed to create temp file {}: {}", tmp_path.display(), e))?;
        file.write_all(bytes)
            .map_err(|e| format!("Failed to write temp file {}: {}", tmp_path.display(), e))?;
        file.sync_all()
            .map_err(|e| format!("Failed to sync temp file {}: {}", tmp_path.display(), e))?;
        Ok(())
    })();
    if let Err(err) = write_res {
        let _ = fs::remove_file(&tmp_path);
        return Err(err);
    }
    replace_file_atomically(&tmp_path, dest_path)
}

fn encode_dklib(entries: &[crate::GameEntry]) -> Result<Vec<u8>, String> {
    let payload = postcard::to_stdvec(entries)
        .map_err(|e| format!("Failed to serialize library.dklib payload: {}", e))?;
    let payload_len = u32::try_from(payload.len())
        .map_err(|_| "library.dklib payload exceeds 4 GiB limit".to_string())?;
    let crc = crc32fast::hash(&payload);

    let mut buf = Vec::with_capacity(DKLIB_HEADER_LEN + payload.len());
    buf.extend_from_slice(DKLIB_MAGIC);
    buf.extend_from_slice(&DKLIB_VERSION.to_le_bytes());
    buf.extend_from_slice(&payload_len.to_le_bytes());
    buf.extend_from_slice(&crc.to_le_bytes());
    buf.extend_from_slice(&payload);
    Ok(buf)
}

fn decode_dklib_raw_from_file(path: &Path) -> Result<(Vec<crate::GameEntry>, Vec<u8>), String> {
    let mut file =
        File::open(path).map_err(|e| format!("Failed to open {}: {}", path.display(), e))?;
    let file_len = file
        .metadata()
        .map_err(|e| format!("Failed to stat {}: {}", path.display(), e))?
        .len();
    if file_len < DKLIB_HEADER_LEN as u64 {
        return Err(format!(
            "{} is truncated ({} bytes < 20-byte header)",
            path.display(),
            file_len
        ));
    }
    if file_len > DKLIB_MAX_FILE_BYTES {
        return Err(format!(
            "{} exceeds maximum allowed size ({} bytes)",
            path.display(),
            file_len
        ));
    }

    let mut hdr = [0u8; DKLIB_HEADER_LEN];
    file.read_exact(&mut hdr)
        .map_err(|e| format!("Failed reading header of {}: {}", path.display(), e))?;

    if &hdr[0..8] != DKLIB_MAGIC {
        return Err(format!("{} has invalid magic header", path.display()));
    }
    let version = u32::from_le_bytes([hdr[8], hdr[9], hdr[10], hdr[11]]);
    if version != DKLIB_VERSION {
        return Err(format!(
            "{} has unsupported version {}",
            path.display(),
            version
        ));
    }
    let payload_len = u32::from_le_bytes([hdr[12], hdr[13], hdr[14], hdr[15]]) as usize;
    let expected_crc = u32::from_le_bytes([hdr[16], hdr[17], hdr[18], hdr[19]]);

    let expected_file_len = (DKLIB_HEADER_LEN as u64)
        .checked_add(payload_len as u64)
        .ok_or_else(|| "Header payload_len overflow".to_string())?;
    if file_len != expected_file_len {
        return Err(format!(
            "{} length mismatch (disk {} bytes, header expects {} bytes)",
            path.display(),
            file_len,
            expected_file_len
        ));
    }

    let mut full_raw = vec![0u8; DKLIB_HEADER_LEN + payload_len];
    full_raw[..DKLIB_HEADER_LEN].copy_from_slice(&hdr);
    file.read_exact(&mut full_raw[DKLIB_HEADER_LEN..])
        .map_err(|e| format!("Failed reading payload of {}: {}", path.display(), e))?;

    let payload = &full_raw[DKLIB_HEADER_LEN..];
    let actual_crc = crc32fast::hash(payload);
    if actual_crc != expected_crc {
        return Err(format!(
            "{} CRC32 mismatch (expected {:#010x}, got {:#010x})",
            path.display(),
            expected_crc,
            actual_crc
        ));
    }

    let entries = postcard::from_bytes::<Vec<crate::GameEntry>>(payload)
        .map_err(|e| format!("Failed decoding {} payload: {}", path.display(), e))?;
    Ok((entries, full_raw))
}

fn read_json_entries_from_path(path: &Path) -> Result<Vec<crate::GameEntry>, String> {
    let raw = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str::<Vec<crate::GameEntry>>(&raw)
        .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))
}

fn try_recover_from_json(data_dir: &Path) -> Option<Result<Vec<crate::GameEntry>, String>> {
    let json_path = library_json_path(data_dir);
    if json_path.is_file() {
        match read_json_entries_from_path(&json_path) {
            Ok(entries) => return Some(Ok(entries)),
            Err(primary_err) => {
                let json_bak = library_json_bak_path(data_dir);
                if json_bak.is_file() {
                    if let Ok(entries) = read_json_entries_from_path(&json_bak) {
                        return Some(Ok(entries));
                    }
                }
                return Some(Err(primary_err));
            }
        }
    }
    let json_bak = library_json_bak_path(data_dir);
    if json_bak.is_file() {
        return Some(read_json_entries_from_path(&json_bak));
    }
    None
}

fn parse_wal_file(wal_path: &Path) -> Result<ParsedWal, String> {
    if !wal_path.is_file() {
        return Ok(ParsedWal {
            ops: Vec::new(),
            valid_bytes: 0,
            truncated_tail: false,
        });
    }
    let file = File::open(wal_path)
        .map_err(|e| format!("Failed to open {}: {}", wal_path.display(), e))?;
    let file_len = file
        .metadata()
        .map_err(|e| format!("Failed to stat {}: {}", wal_path.display(), e))?
        .len();
    if file_len == 0 {
        return Ok(ParsedWal {
            ops: Vec::new(),
            valid_bytes: 0,
            truncated_tail: false,
        });
    }

    let mut reader = BufReader::with_capacity(WAL_STREAM_BUF_BYTES, file);
    let mut ops = Vec::new();
    let mut valid_bytes = 0u64;
    let mut truncated_tail = false;

    loop {
        let mut hdr = [0u8; DKWL_HEADER_LEN];
        let mut hdr_read = 0usize;
        while hdr_read < DKWL_HEADER_LEN {
            match reader.read(&mut hdr[hdr_read..]) {
                Ok(0) => break,
                Ok(n) => hdr_read += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    truncated_tail = true;
                    break;
                }
            }
        }

        if hdr_read == 0 && !truncated_tail {
            break;
        }
        if hdr_read < DKWL_HEADER_LEN {
            truncated_tail = true;
            break;
        }

        if &hdr[0..4] != DKWL_MAGIC {
            truncated_tail = true;
            break;
        }

        let payload_len = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]);
        let expected_crc = u32::from_le_bytes([hdr[8], hdr[9], hdr[10], hdr[11]]);

        if payload_len == 0 || payload_len > DKWL_MAX_RECORD_BYTES {
            truncated_tail = true;
            break;
        }
        let next_end = valid_bytes.saturating_add(DKWL_HEADER_LEN as u64 + payload_len as u64);
        if next_end > file_len {
            truncated_tail = true;
            break;
        }

        let mut payload = vec![0u8; payload_len as usize];
        if reader.read_exact(&mut payload).is_err() {
            truncated_tail = true;
            break;
        }

        if crc32fast::hash(&payload) != expected_crc {
            truncated_tail = true;
            break;
        }

        let Ok(op) = postcard::from_bytes::<WalOp>(&payload) else {
            truncated_tail = true;
            break;
        };

        ops.push(op);
        valid_bytes = next_end;
    }

    Ok(ParsedWal {
        ops,
        valid_bytes,
        truncated_tail,
    })
}

fn truncate_wal_to_valid_prefix(wal_path: &Path, valid_bytes: u64) -> Result<(), String> {
    if !wal_path.is_file() {
        return Ok(());
    }
    if valid_bytes == 0 {
        clear_wal_file(wal_path);
        return Ok(());
    }
    let file = OpenOptions::new()
        .write(true)
        .open(wal_path)
        .map_err(|e| format!("Failed opening {} for tail truncation: {}", wal_path.display(), e))?;
    file.set_len(valid_bytes)
        .map_err(|e| format!("Failed truncating {}: {}", wal_path.display(), e))?;
    let _ = file.sync_all();
    Ok(())
}

fn clear_wal_file(wal_path: &Path) {
    if wal_path.exists() && fs::remove_file(wal_path).is_err() {
        let _ = File::create(wal_path);
    }
}

fn apply_wal_op(entries: &mut Vec<crate::GameEntry>, op: &WalOp) {
    match op {
        WalOp::Upsert(entry) => {
            if !entry.id.is_empty() {
                if let Some(existing) = entries.iter_mut().find(|g| g.id == entry.id) {
                    *existing = (**entry).clone();
                    return;
                }
            }
            entries.push((**entry).clone());
        }
        WalOp::Delete { id } => {
            entries.retain(|g| g.id != *id);
        }
        WalOp::PatchPlaytime {
            id,
            playtime_seconds,
            last_played_ts,
        } => {
            if let Some(g) = entries.iter_mut().find(|g| g.id == *id) {
                g.playtime_seconds = *playtime_seconds;
                if *playtime_seconds >= g.imported_playtime_secs {
                    g.duke_playtime_secs = *playtime_seconds - g.imported_playtime_secs;
                } else {
                    g.imported_playtime_secs = *playtime_seconds;
                    g.duke_playtime_secs = 0;
                }
                if *last_played_ts > 0 {
                    g.last_played_ts = Some(*last_played_ts);
                    g.last_played = Some(*last_played_ts);
                }
            }
        }
        WalOp::PatchCover {
            id,
            cover_url,
            hero_url,
            icon_path,
        } => {
            if let Some(g) = entries.iter_mut().find(|g| g.id == *id) {
                g.cover_url = cover_url.clone();
                if hero_url.is_some() {
                    g.hero_url = hero_url.clone();
                }
                if icon_path.is_some() {
                    g.icon_path = icon_path.clone();
                }
            }
        }
        WalOp::PatchFavorite { id, is_favorite } => {
            if let Some(g) = entries.iter_mut().find(|g| g.id == *id) {
                g.is_favorite = *is_favorite;
            }
        }
        WalOp::PatchLaunchArgs { id, launch_args } => {
            if let Some(g) = entries.iter_mut().find(|g| g.id == *id) {
                let trimmed = launch_args.trim();
                g.launch_args = if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                };
            }
        }
        WalOp::PatchLaunchProfile {
            id,
            sandbox_enabled,
            nvme_prefetch_override,
        } => {
            if let Some(g) = entries.iter_mut().find(|g| g.id == *id) {
                g.sandbox_enabled = *sandbox_enabled;
                g.nvme_prefetch_enabled = *nvme_prefetch_override;
            }
        }
    }
}

fn write_primary_and_backup(data_dir: &Path, encoded: &[u8]) -> Result<(), String> {
    fs::create_dir_all(data_dir)
        .map_err(|e| format!("Failed creating data dir {}: {}", data_dir.display(), e))?;
    let primary_path = dklib_path(data_dir);
    let bak_path = dklib_bak_path(data_dir);
    let wal_path = dklib_wal_path(data_dir);

    // Snapshot the currently valid primary into `.bak` before swapping.
    if primary_path.is_file() {
        if let Ok((_, existing_raw)) = decode_dklib_raw_from_file(&primary_path) {
            let _ = write_temp_and_swap(&bak_path, &existing_raw, ".bak.tmp");
        }
    }

    write_temp_and_swap(&primary_path, encoded, ".tmp")?;
    let _ = write_temp_and_swap(&bak_path, encoded, ".bak.tmp");
    clear_wal_file(&wal_path);
    Ok(())
}

enum BaseSource {
    Primary,
    Backup,
    Json,
    Empty,
}

fn resolve_base_entries(data_dir: &Path) -> Result<(Vec<crate::GameEntry>, BaseSource), String> {
    let primary_path = dklib_path(data_dir);
    let bak_path = dklib_bak_path(data_dir);

    if primary_path.is_file() {
        match decode_dklib_raw_from_file(&primary_path) {
            Ok((entries, _)) => return Ok((entries, BaseSource::Primary)),
            Err(primary_err) => {
                if bak_path.is_file() {
                    if let Ok((entries, _)) = decode_dklib_raw_from_file(&bak_path) {
                        return Ok((entries, BaseSource::Backup));
                    }
                }
                if let Some(Ok(entries)) = try_recover_from_json(data_dir) {
                    return Ok((entries, BaseSource::Json));
                }
                return Err(format!(
                    "Corrupt library.dklib and no valid backup or library.json available: {}",
                    primary_err
                ));
            }
        }
    }

    if bak_path.is_file() {
        if let Ok((entries, _)) = decode_dklib_raw_from_file(&bak_path) {
            return Ok((entries, BaseSource::Backup));
        }
    }

    if let Some(json_res) = try_recover_from_json(data_dir) {
        let entries = json_res?;
        return Ok((entries, BaseSource::Json));
    }

    Ok((Vec::new(), BaseSource::Empty))
}

fn open_wal_append_file(wal_path: &Path) -> std::io::Result<File> {
    let mut opts = OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
        opts.share_mode(0x0000_0001 | 0x0000_0002 | 0x0000_0004);
    }
    opts.open(wal_path)
}

fn compact_locked(
    data_dir: &Path,
    state: &mut DirState,
    base_entries: Option<Vec<crate::GameEntry>>,
) -> Result<(), String> {
    state.wal_file = None;
    let entries = match base_entries {
        Some(e) => e,
        None => {
            let (mut base, _) = resolve_base_entries(data_dir)?;
            let wal = parse_wal_file(&dklib_wal_path(data_dir))?;
            for op in &wal.ops {
                apply_wal_op(&mut base, op);
            }
            base
        }
    };
    let encoded = encode_dklib(&entries)?;
    write_primary_and_backup(data_dir, &encoded)?;
    state.wal_count = Some(0);
    Ok(())
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn verify_and_repair_dklib(data_dir: &Path) -> Result<DklibHealOutcome, String> {
    let dir_lock = dir_state_lock(data_dir);
    let mut state = dir_lock.lock().unwrap_or_else(|p| p.into_inner());
    state.wal_file = None;

    let primary_path = dklib_path(data_dir);
    let bak_path = dklib_bak_path(data_dir);
    let wal_path = dklib_wal_path(data_dir);

    let (mut entries, source) = resolve_base_entries(data_dir)?;
    let wal = parse_wal_file(&wal_path)?;

    if wal.truncated_tail {
        truncate_wal_to_valid_prefix(&wal_path, wal.valid_bytes)?;
    }

    let recovered = matches!(source, BaseSource::Backup | BaseSource::Json);
    let wal_replayed_count = wal.ops.len();
    let needs_wal_compact = wal.truncated_tail
        || wal_replayed_count >= WAL_COMPACT_THRESHOLD
        || (recovered && wal_replayed_count > 0)
        || (matches!(source, BaseSource::Empty) && wal_replayed_count > 0);

    if !recovered && !needs_wal_compact {
        if primary_path.is_file() && !bak_path.is_file() {
            if let Ok((_, raw)) = decode_dklib_raw_from_file(&primary_path) {
                let _ = write_temp_and_swap(&bak_path, &raw, ".bak.tmp");
            }
        }
        state.wal_count = Some(wal_replayed_count);
        let detail = if primary_path.is_file() {
            if wal_replayed_count > 0 {
                format!("library.dklib intact ({} pending WAL ops)", wal_replayed_count)
            } else {
                "library.dklib intact".to_string()
            }
        } else {
            "No library store present".to_string()
        };
        return Ok(DklibHealOutcome {
            repaired: false,
            replayed_wal: 0,
            recovered_from_backup_or_json: false,
            detail,
        });
    }

    for op in &wal.ops {
        apply_wal_op(&mut entries, op);
    }

    let encoded = encode_dklib(&entries)?;
    write_primary_and_backup(data_dir, &encoded)?;
    state.wal_count = Some(0);

    let source_label = match source {
        BaseSource::Primary => "primary",
        BaseSource::Backup => "library.dklib.bak",
        BaseSource::Json => "library.json",
        BaseSource::Empty => "WAL",
    };
    let mut notes = Vec::new();
    if recovered {
        notes.push(format!("recovered base from {}", source_label));
    }
    if wal.truncated_tail {
        notes.push("truncated corrupt WAL tail".to_string());
    }
    if wal_replayed_count > 0 {
        notes.push(format!("replayed {} WAL frame(s)", wal_replayed_count));
    }
    if notes.is_empty() {
        notes.push("compacted library.dklib".to_string());
    }

    Ok(DklibHealOutcome {
        repaired: true,
        replayed_wal: wal_replayed_count,
        recovered_from_backup_or_json: recovered,
        detail: notes.join("; "),
    })
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn load_library_dklib(data_dir: &Path) -> Result<Vec<crate::GameEntry>, String> {
    let dir_lock = dir_state_lock(data_dir);
    let mut state = dir_lock.lock().unwrap_or_else(|p| p.into_inner());
    state.wal_file = None;

    let (mut entries, source) = resolve_base_entries(data_dir)?;
    let wal_path = dklib_wal_path(data_dir);
    let wal = parse_wal_file(&wal_path)?;
    let wal_count = wal.ops.len();

    if wal.truncated_tail {
        truncate_wal_to_valid_prefix(&wal_path, wal.valid_bytes)?;
    }

    for op in &wal.ops {
        apply_wal_op(&mut entries, op);
    }

    let should_compact = matches!(source, BaseSource::Backup | BaseSource::Json)
        || wal.truncated_tail
        || wal_count >= WAL_COMPACT_THRESHOLD
        || (matches!(source, BaseSource::Empty) && wal_count > 0);

    if should_compact {
        compact_locked(data_dir, &mut state, Some(entries.clone()))?;
    } else {
        state.wal_count = Some(wal_count);
    }

    Ok(entries)
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn save_library_dklib_full(
    data_dir: &Path,
    entries: &[crate::GameEntry],
) -> Result<(), String> {
    let dir_lock = dir_state_lock(data_dir);
    let mut state = dir_lock.lock().unwrap_or_else(|p| p.into_inner());
    state.wal_file = None;
    let encoded = encode_dklib(entries)?;
    write_primary_and_backup(data_dir, &encoded)?;
    state.wal_count = Some(0);
    Ok(())
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn append_wal_op(data_dir: &Path, op: &WalOp) -> Result<(), String> {
    let payload = postcard::to_stdvec(op)
        .map_err(|e| format!("Failed to serialize WAL op: {}", e))?;
    let payload_len = u32::try_from(payload.len())
        .map_err(|_| "WAL op exceeds 4 GiB limit".to_string())?;
    if payload_len == 0 || payload_len > DKWL_MAX_RECORD_BYTES {
        return Err(format!("WAL op size {} out of bounds", payload_len));
    }
    let crc = crc32fast::hash(&payload);

    let mut frame = Vec::with_capacity(DKWL_HEADER_LEN + payload.len());
    frame.extend_from_slice(DKWL_MAGIC);
    frame.extend_from_slice(&payload_len.to_le_bytes());
    frame.extend_from_slice(&crc.to_le_bytes());
    frame.extend_from_slice(&payload);

    let dir_lock = dir_state_lock(data_dir);
    let mut state = dir_lock.lock().unwrap_or_else(|p| p.into_inner());
    let wal_path = dklib_wal_path(data_dir);

    let current_count = match state.wal_count {
        Some(n) => n,
        None => {
            fs::create_dir_all(data_dir)
                .map_err(|e| format!("Failed creating data dir {}: {}", data_dir.display(), e))?;
            let existing = if wal_path.is_file() {
                parse_wal_file(&wal_path)?.ops.len()
            } else {
                0
            };
            state.wal_count = Some(existing);
            existing
        }
    };

    if state.wal_file.is_none() {
        let f = match open_wal_append_file(&wal_path) {
            Ok(f) => f,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir_all(data_dir).map_err(|e| {
                    format!("Failed creating data dir {}: {}", data_dir.display(), e)
                })?;
                open_wal_append_file(&wal_path)
                    .map_err(|e| format!("Failed opening {}: {}", wal_path.display(), e))?
            }
            Err(err) => return Err(format!("Failed opening {}: {}", wal_path.display(), err)),
        };
        state.wal_file = Some(f);
    }

    if let Some(ref mut file) = state.wal_file {
        if let Err(err) = file.write_all(&frame) {
            state.wal_file = None;
            return Err(format!(
                "Failed appending WAL frame to {}: {}",
                wal_path.display(),
                err
            ));
        }
    }

    let next_count = current_count.saturating_add(1);
    state.wal_count = Some(next_count);

    if next_count >= WAL_COMPACT_THRESHOLD {
        compact_locked(data_dir, &mut state, None)?;
    }

    Ok(())
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn append_playtime_wal(
    data_dir: &Path,
    id: &str,
    playtime_seconds: u64,
    last_played_ts: u64,
) -> Result<(), String> {
    append_wal_op(
        data_dir,
        &WalOp::PatchPlaytime {
            id: id.to_string(),
            playtime_seconds,
            last_played_ts,
        },
    )
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn append_cover_wal(
    data_dir: &Path,
    id: &str,
    cover_url: &str,
    hero_url: Option<&str>,
    icon_path: Option<&str>,
) -> Result<(), String> {
    append_wal_op(
        data_dir,
        &WalOp::PatchCover {
            id: id.to_string(),
            cover_url: cover_url.to_string(),
            hero_url: hero_url.map(|s| s.to_string()),
            icon_path: icon_path.map(|s| s.to_string()),
        },
    )
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn append_upsert_wal(data_dir: &Path, entry: &crate::GameEntry) -> Result<(), String> {
    append_wal_op(data_dir, &WalOp::Upsert(Box::new(entry.clone())))
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn append_delete_wal(data_dir: &Path, id: &str) -> Result<(), String> {
    append_wal_op(data_dir, &WalOp::Delete { id: id.to_string() })
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn append_favorite_wal(
    data_dir: &Path,
    id: &str,
    is_favorite: bool,
) -> Result<(), String> {
    append_wal_op(
        data_dir,
        &WalOp::PatchFavorite {
            id: id.to_string(),
            is_favorite,
        },
    )
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn append_launch_args_wal(
    data_dir: &Path,
    id: &str,
    launch_args: &str,
) -> Result<(), String> {
    append_wal_op(
        data_dir,
        &WalOp::PatchLaunchArgs {
            id: id.to_string(),
            launch_args: launch_args.to_string(),
        },
    )
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn append_launch_profile_wal(
    data_dir: &Path,
    id: &str,
    sandbox_enabled: bool,
    nvme_prefetch_override: Option<bool>,
) -> Result<(), String> {
    append_wal_op(
        data_dir,
        &WalOp::PatchLaunchProfile {
            id: id.to_string(),
            sandbox_enabled,
            nvme_prefetch_override,
        },
    )
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn compact_dklib_if_needed(data_dir: &Path, force: bool) -> Result<bool, String> {
    let dir_lock = dir_state_lock(data_dir);
    let mut state = dir_lock.lock().unwrap_or_else(|p| p.into_inner());
    state.wal_file = None;

    if !force {
        let primary_path = dklib_path(data_dir);
        let wal_path = dklib_wal_path(data_dir);
        let primary_valid =
            primary_path.is_file() && decode_dklib_raw_from_file(&primary_path).is_ok();
        let wal = parse_wal_file(&wal_path)?;
        let wal_count = wal.ops.len();

        let has_recovery_source = dklib_bak_path(data_dir).is_file()
            || library_json_path(data_dir).is_file()
            || library_json_bak_path(data_dir).is_file();

        let needs_compact = (!primary_valid && (has_recovery_source || wal_count > 0 || wal.truncated_tail))
            || wal.truncated_tail
            || wal_count >= WAL_COMPACT_THRESHOLD;

        if !needs_compact {
            state.wal_count = Some(wal_count);
            return Ok(false);
        }
    }

    compact_locked(data_dir, &mut state, None)?;
    Ok(true)
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn export_library_json(data_dir: &Path, out_path: &Path) -> Result<usize, String> {
    let entries = load_library_dklib(data_dir)?;
    let count = entries.len();
    let serialized = serde_json::to_string_pretty(&entries)
        .map_err(|e| format!("Failed to serialize library JSON: {}", e))?;
    write_temp_and_swap(out_path, serialized.as_bytes(), ".tmp")?;
    Ok(count)
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn import_library_json(data_dir: &Path, json_path: &Path) -> Result<usize, String> {
    let entries = read_json_entries_from_path(json_path)?;
    let count = entries.len();
    save_library_dklib_full(data_dir, &entries)?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn make_temp_dir(name: &str) -> PathBuf {
        let seq = LIB_STORE_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "duke_lib_store_{}_{}_{}",
            name,
            std::process::id(),
            seq
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_game(id: &str, title: &str) -> crate::GameEntry {
        crate::GameEntry {
            id: id.to_string(),
            title: title.to_string(),
            exe_path: format!("C:\\Games\\{}\\{}.exe", id, id),
            install_dir: format!("C:\\Games\\{}", id),
            size_bytes: 12_345_678_901,
            size_formatted: "11.5 GB".to_string(),
            cover_url: format!("https://example.com/{}/cover.jpg", id),
            hero_url: Some(format!("https://example.com/{}/hero.jpg", id)),
            icon_path: Some(format!("C:\\Games\\{}\\icon.ico", id)),
            genre: "Action RPG".to_string(),
            platform: "PC".to_string(),
            source: "Steam".to_string(),
            acquisition_source: "steam".to_string(),
            is_favorite: true,
            is_installed: true,
            added_at: 1_700_000_100,
            last_played: Some(1_700_000_200),
            steam_app_id: Some(1245620),
            app_id: Some(1245620),
            playtime_seconds: 3_600,
            imported_playtime_secs: 1_200,
            duke_playtime_secs: 2_400,
            last_played_ts: Some(1_700_000_200),
            steam_price_cents: Some(5999),
            steam_price_formatted: Some("$59.99".to_string()),
            launch_args: Some("-dx12 -novid".to_string()),
            sandbox_enabled: true,
            nvme_prefetch_enabled: Some(true),
            version_tag: "v1.12.1".to_string(),
            update_feed_url: "https://example.com/feed.json".to_string(),
            latest_version_tag: "v1.12.2".to_string(),
            update_available: true,
            achievements_path: format!("C:\\Games\\{}\\ach.json", id),
            platform_id: 42,
            console_group: 1,
            rom_id: "ROM-0042".to_string(),
            emulator_id: "emu_native".to_string(),
        }
    }

    #[test]
    fn test_dklib_roundtrip_populated_entries() {
        let dir = make_temp_dir("roundtrip");
        let entries = vec![
            sample_game("g1", "Sample Quest"),
            sample_game("g2", "Cosmic Drift"),
        ];

        save_library_dklib_full(&dir, &entries).unwrap();
        let raw = fs::read(dklib_path(&dir)).unwrap();
        assert_eq!(&raw[0..8], b"DKLIB001");
        assert_eq!(u32::from_le_bytes([raw[8], raw[9], raw[10], raw[11]]), 1);
        assert!(dklib_bak_path(&dir).is_file());
        assert!(!dklib_wal_path(&dir).exists());

        let loaded = load_library_dklib(&dir).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].id, "g1");
        assert_eq!(loaded[0].title, "Sample Quest");
        assert_eq!(loaded[0].steam_app_id, Some(1245620));
        assert_eq!(loaded[0].imported_playtime_secs, 1_200);
        assert_eq!(loaded[0].duke_playtime_secs, 2_400);
        assert_eq!(loaded[0].launch_args.as_deref(), Some("-dx12 -novid"));
        assert_eq!(loaded[0].nvme_prefetch_enabled, Some(true));
        assert_eq!(loaded[0].platform_id, 42);
        assert_eq!(loaded[0].rom_id, "ROM-0042");
        assert_eq!(loaded[1].id, "g2");
        assert!(loaded[1].is_installed);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_submillisecond_wal_append_and_replay() {
        let dir = make_temp_dir("wal_replay");
        let initial = vec![
            sample_game("g1", "First Game"),
            sample_game("g2", "Second Game"),
        ];
        save_library_dklib_full(&dir, &initial).unwrap();

        // Warm file metadata & verify sustained sub-millisecond WAL appends.
        append_playtime_wal(&dir, "g1", 4_000, 1_700_001_000).unwrap();

        let bench_start = Instant::now();
        for i in 1..=20u64 {
            append_playtime_wal(&dir, "g1", 4_000 + i, 1_700_001_000 + i).unwrap();
        }
        let elapsed = bench_start.elapsed();
        let avg_us = elapsed.as_micros() as f64 / 20.0;
        println!("Measured avg WAL append: {:.2} us", avg_us);
        assert!(
            avg_us < 1_000.0,
            "Expected sub-millisecond average WAL append, measured {:.2} us",
            avg_us
        );

        append_cover_wal(
            &dir,
            "g1",
            "https://example.com/new_cover.png",
            Some("https://example.com/new_hero.png"),
            None,
        )
        .unwrap();
        append_favorite_wal(&dir, "g1", false).unwrap();
        append_launch_args_wal(&dir, "g1", "-vulkan").unwrap();
        append_launch_profile_wal(&dir, "g1", false, Some(false)).unwrap();
        append_delete_wal(&dir, "g2").unwrap();

        let mut g3 = sample_game("g3", "Third Game");
        g3.playtime_seconds = 900;
        append_upsert_wal(&dir, &g3).unwrap();

        assert!(dklib_wal_path(&dir).is_file());

        let loaded = load_library_dklib(&dir).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].id, "g1");
        assert_eq!(loaded[0].playtime_seconds, 4_020);
        assert_eq!(loaded[0].duke_playtime_secs, 2_820);
        assert_eq!(loaded[0].last_played_ts, Some(1_700_001_020));
        assert_eq!(loaded[0].cover_url, "https://example.com/new_cover.png");
        assert_eq!(
            loaded[0].hero_url.as_deref(),
            Some("https://example.com/new_hero.png")
        );
        assert_eq!(
            loaded[0].icon_path.as_deref(),
            Some("C:\\Games\\g1\\icon.ico")
        );
        assert!(!loaded[0].is_favorite);
        assert_eq!(loaded[0].launch_args.as_deref(), Some("-vulkan"));
        assert!(!loaded[0].sandbox_enabled);
        assert_eq!(loaded[0].nvme_prefetch_enabled, Some(false));
        assert_eq!(loaded[1].id, "g3");

        assert!(!compact_dklib_if_needed(&dir, false).unwrap());
        assert!(compact_dklib_if_needed(&dir, true).unwrap());
        assert!(!dklib_wal_path(&dir).exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_wal_auto_compacts_at_threshold() {
        let dir = make_temp_dir("wal_compact_threshold");
        save_library_dklib_full(&dir, &[sample_game("g1", "Auto Compact")]).unwrap();

        for i in 1..=WAL_COMPACT_THRESHOLD {
            append_playtime_wal(&dir, "g1", 5_000 + i as u64, 1_700_005_000 + i as u64).unwrap();
        }

        assert!(
            !dklib_wal_path(&dir).exists(),
            "WAL should auto-compact once record count reaches threshold"
        );
        let loaded = load_library_dklib(&dir).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(
            loaded[0].playtime_seconds,
            5_000 + WAL_COMPACT_THRESHOLD as u64
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_verify_and_repair_corrupt_primary_and_truncated_wal() {
        let dir = make_temp_dir("repair_corrupt");
        let initial = vec![
            sample_game("g1", "Alpha"),
            sample_game("g2", "Beta"),
        ];
        save_library_dklib_full(&dir, &initial).unwrap();

        append_playtime_wal(&dir, "g1", 9_999, 1_700_009_999).unwrap();
        append_cover_wal(&dir, "g2", "https://example.com/beta_new.jpg", None, None).unwrap();

        // Corrupt primary library.dklib payload bytes.
        let mut primary_bytes = fs::read(dklib_path(&dir)).unwrap();
        let last_idx = primary_bytes.len() - 1;
        primary_bytes[last_idx] ^= 0xFF;
        fs::write(dklib_path(&dir), &primary_bytes).unwrap();

        // Append a truncated/corrupt trailing WAL frame.
        {
            let mut wal_file = OpenOptions::new()
                .append(true)
                .open(dklib_wal_path(&dir))
                .unwrap();
            wal_file.write_all(b"DKWL\x40\x00\x00\x00\x11\x22\x33\x44partial_frame").unwrap();
        }

        let outcome = verify_and_repair_dklib(&dir).unwrap();
        assert!(outcome.repaired);
        assert!(outcome.recovered_from_backup_or_json);
        assert_eq!(outcome.replayed_wal, 2);
        assert!(!dklib_wal_path(&dir).exists());

        let loaded = load_library_dklib(&dir).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].playtime_seconds, 9_999);
        assert_eq!(loaded[0].last_played_ts, Some(1_700_009_999));
        assert_eq!(loaded[1].cover_url, "https://example.com/beta_new.jpg");

        let second_check = verify_and_repair_dklib(&dir).unwrap();
        assert!(!second_check.repaired);
        assert_eq!(second_check.replayed_wal, 0);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_lossless_json_migration_and_export_import_roundtrip() {
        let dir = make_temp_dir("json_migration");
        let dir2 = make_temp_dir("json_import");
        let original = vec![
            sample_game("m1", "Migrated One"),
            sample_game("m2", "Migrated Two"),
        ];
        let json_raw = serde_json::to_string_pretty(&original).unwrap();
        fs::write(library_json_path(&dir), &json_raw).unwrap();

        // `.dklib` is absent -> automatically migrates from `library.json`.
        let migrated = load_library_dklib(&dir).unwrap();
        assert_eq!(migrated.len(), 2);
        assert!(dklib_path(&dir).is_file());
        assert!(dklib_bak_path(&dir).is_file());
        assert_eq!(migrated[0].rom_id, "ROM-0042");
        assert_eq!(migrated[1].steam_price_cents, Some(5999));

        // Append WAL mutation and export cleanly to JSON.
        append_playtime_wal(&dir, "m1", 8_888, 1_700_008_888).unwrap();
        let exported_path = dir.join("exported_library.json");
        let exported_count = export_library_json(&dir, &exported_path).unwrap();
        assert_eq!(exported_count, 2);

        // Import exported JSON into a clean store and verify lossless parity.
        let imported_count = import_library_json(&dir2, &exported_path).unwrap();
        assert_eq!(imported_count, 2);

        let roundtripped = load_library_dklib(&dir2).unwrap();
        assert_eq!(roundtripped.len(), 2);
        assert_eq!(roundtripped[0].id, "m1");
        assert_eq!(roundtripped[0].playtime_seconds, 8_888);
        assert_eq!(roundtripped[0].last_played_ts, Some(1_700_008_888));
        assert_eq!(roundtripped[0].nvme_prefetch_enabled, Some(true));
        assert_eq!(roundtripped[1].id, "m2");
        assert_eq!(roundtripped[1].launch_args.as_deref(), Some("-dx12 -novid"));

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&dir2);
    }
}

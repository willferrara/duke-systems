use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

use duke_core::win32_ffi::create_hard_link;

use crate::ntfs_compress::measure_game_storage_stats;
use crate::snapshots::{create_snapshot_in_dir, CAS_DIR_NAME};
use crate::{
    format_bytes, load_library_from_disk, load_library_standalone, resolve_game_root_dir,
    save_library_to_disk,
};

const MIN_MEDIA_DUPLICATE_BYTES: u64 = 128 * 1024;
const MIN_LIBRARY_DEDUP_BYTES: u64 = 512 * 1024;
const HASH_CHUNK_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepShrinkLanguageBucket {
    pub language: String,
    pub file_count: usize,
    pub total_bytes: u64,
    pub total_formatted: String,
    pub sample_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepShrinkCandidateFile {
    pub rel_path: String,
    pub language: String,
    pub size_bytes: u64,
    pub kind: String, // "foreign_voice" | "foreign_cutscene" | "duplicate_media"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepShrinkScanReport {
    pub game_id: String,
    pub game_root: String,
    pub keep_languages: Vec<String>,
    pub language_buckets: Vec<DeepShrinkLanguageBucket>,
    pub duplicate_media_files: usize,
    pub duplicate_media_bytes: u64,
    pub duplicate_media_formatted: String,
    pub total_reclaimable_bytes: u64,
    pub total_reclaimable_formatted: String,
    pub candidates: Vec<DeepShrinkCandidateFile>,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepShrinkApplyReport {
    pub game_id: String,
    pub restore_snapshot_id: Option<String>,
    pub files_removed: usize,
    pub duplicates_hardlinked: usize,
    pub reclaimed_bytes: u64,
    pub reclaimed_formatted: String,
    pub new_game_bytes: u64,
    pub new_game_formatted: String,
    pub elapsed_ms: u64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrossFolderDedupMatch {
    pub canonical_path: String,
    pub duplicate_path: String,
    pub bytes_saved: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryDedupReport {
    pub folders_scanned: usize,
    pub files_inspected: usize,
    pub candidate_clusters: usize,
    pub files_hardlinked: usize,
    pub already_hardlinked: usize,
    pub bytes_reclaimed: u64,
    pub reclaimed_formatted: String,
    pub elapsed_ms: u64,
    pub sample_matches: Vec<CrossFolderDedupMatch>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct FileIdKey {
    volume_serial: u32,
    file_index: u64,
}

#[cfg(windows)]
fn query_file_identity(path: &Path) -> Option<FileIdKey> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        FILE_ATTRIBUTE_NORMAL, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, OPEN_EXISTING,
    };

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
    }
    .ok()?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    let ok = unsafe { GetFileInformationByHandle(handle, &mut info) }.is_ok();
    unsafe {
        let _ = CloseHandle(handle);
    }
    if !ok {
        return None;
    }
    Some(FileIdKey {
        volume_serial: info.dwVolumeSerialNumber,
        file_index: ((info.nFileIndexHigh as u64) << 32) | (info.nFileIndexLow as u64),
    })
}

#[cfg(not(windows))]
fn query_file_identity(_path: &Path) -> Option<FileIdKey> {
    None
}

fn volume_bucket_key(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let bytes = raw.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' {
        format!("{}:\\", (bytes[0] as char).to_ascii_uppercase())
    } else {
        "DEFAULT".to_string()
    }
}

fn canonical_language_for_long_token(tok: &str) -> Option<&'static str> {
    match tok {
        "french" | "francais" | "fra" | "fre" | "fr_fr" => Some("french"),
        "german" | "deutsch" | "deu" | "ger" | "de_de" => Some("german"),
        "japanese" | "nihongo" | "jpn" | "ja_jp" => Some("japanese"),
        "spanish" | "espanol" | "spa" | "latam" | "es_es" | "es_mx" | "mexican" | "castilian" => {
            Some("spanish")
        }
        "russian" | "rus" | "ru_ru" => Some("russian"),
        "italian" | "ita" | "it_it" => Some("italian"),
        "portuguese" | "por" | "brazilian" | "pt_br" | "ptbr" => Some("portuguese"),
        "korean" | "kor" | "ko_kr" => Some("korean"),
        "chinese" | "mandarin" | "zho" | "chs" | "cht" | "zh_cn" | "zh_tw" | "schinese"
        | "tchinese" => Some("chinese"),
        "polish" | "pol" | "pl_pl" => Some("polish"),
        _ => None,
    }
}

fn canonical_language_for_short_iso(tok: &str) -> Option<&'static str> {
    match tok {
        "fr" => Some("french"),
        "de" => Some("german"),
        "jp" | "ja" => Some("japanese"),
        "es" => Some("spanish"),
        "ru" => Some("russian"),
        "it" => Some("italian"),
        "pt" => Some("portuguese"),
        "ko" | "kr" => Some("korean"),
        "zh" => Some("chinese"),
        "pl" => Some("polish"),
        _ => None,
    }
}

fn canonical_keep_set(keep_languages: &[String]) -> HashSet<String> {
    let mut keep = HashSet::new();
    keep.insert("english".to_string());
    for raw in keep_languages {
        let lower = raw.trim().to_ascii_lowercase();
        if let Some(canon) = canonical_language_for_long_token(&lower)
            .or_else(|| canonical_language_for_short_iso(&lower))
        {
            keep.insert(canon.to_string());
        } else if !lower.is_empty() {
            keep.insert(lower);
        }
    }
    keep
}

fn is_media_or_localization_file(ext: &str) -> bool {
    matches!(
        ext,
        "wem"
            | "bnk"
            | "pck"
            | "bik"
            | "bk2"
            | "usm"
            | "mp4"
            | "webm"
            | "ogg"
            | "wav"
            | "fsb"
            | "xwm"
            | "fuz"
            | "bsa"
            | "ba2"
            | "pak"
            | "ucas"
            | "utoc"
            | "cpk"
            | "afs"
            | "awb"
            | "acb"
            | "sdat"
            | "sab"
            | "mab"
    )
}

fn is_cutscene_ext(ext: &str) -> bool {
    matches!(ext, "bik" | "bk2" | "usm" | "mp4" | "webm")
}

fn is_localization_folder_segment(seg: &str) -> bool {
    matches!(
        seg,
        "localized"
            | "localizations"
            | "l10n"
            | "lang"
            | "langs"
            | "languages"
            | "voice"
            | "voices"
            | "vo"
            | "audio"
            | "speech"
            | "dialog"
            | "dialogue"
            | "sound"
            | "sounds"
            | "movies"
            | "video"
            | "videos"
            | "cutscenes"
            | "fmv"
    )
}

/// Identifies unused foreign voiceover / cutscene packs using delimiter-safe token matching
/// so normal English words like `frame`, `destroy`, `escape`, `sprite`, or `rust` never match.
pub fn classify_foreign_media_path(rel_path: &str, keep_languages: &[String]) -> Option<(String, &'static str)> {
    let norm = rel_path.replace('\\', "/").to_ascii_lowercase();
    let keep = canonical_keep_set(keep_languages);
    let segments: Vec<&str> = norm.split('/').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return None;
    }

    let file_name = *segments.last().unwrap_or(&"");
    let ext = Path::new(file_name)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if !is_media_or_localization_file(ext) {
        return None;
    }
    let kind = if is_cutscene_ext(ext) {
        "foreign_cutscene"
    } else {
        "foreign_voice"
    };

    let has_loc_parent = segments[..segments.len().saturating_sub(1)]
        .iter()
        .any(|seg| {
            seg.split(|c: char| !c.is_ascii_alphanumeric())
                .any(is_localization_folder_segment)
        });

    // 1. Check directory path segments for explicit language folders or `_french` / `/ja/` folders.
    for dir_seg in &segments[..segments.len().saturating_sub(1)] {
        if let Some(canon) = canonical_language_for_long_token(dir_seg) {
            if !keep.contains(canon) {
                return Some((canon.to_string(), kind));
            }
        }
        let tokens: Vec<&str> = dir_seg
            .split(['_', '-', '.', ' '])
            .filter(|t| !t.is_empty())
            .collect();
        for tok in &tokens {
            if let Some(canon) = canonical_language_for_long_token(tok) {
                if !keep.contains(canon) {
                    return Some((canon.to_string(), kind));
                }
            }
            if has_loc_parent {
                if let Some(canon) = canonical_language_for_short_iso(tok) {
                    if !keep.contains(canon) {
                        return Some((canon.to_string(), kind));
                    }
                }
            }
        }
    }

    // 2. Check filename stem tokens strictly at delimiter boundaries (`_french.`, `_german_`, `_ja.`).
    let stem = file_name
        .strip_suffix(&format!(".{}", ext))
        .unwrap_or(file_name);
    let stem_tokens: Vec<&str> = stem
        .split(['_', '-', '.', ' '])
        .filter(|t| !t.is_empty())
        .collect();

    for (idx, tok) in stem_tokens.iter().enumerate() {
        if let Some(canon) = canonical_language_for_long_token(tok) {
            if !keep.contains(canon) {
                return Some((canon.to_string(), kind));
            }
        }
        // Short 2-letter ISO code matches strictly when it sits as trailing language suffix,
        // leading prefix (`fr_vo.wem`), or inside a voice/localized parent folder (`idx > 0`).
        let at_tail = idx + 1 == stem_tokens.len() && stem_tokens.len() >= 2;
        if at_tail || (has_loc_parent && stem_tokens.len() >= 2) {
            if let Some(canon) = canonical_language_for_short_iso(tok) {
                if !keep.contains(canon) {
                    return Some((canon.to_string(), kind));
                }
            }
        }
    }

    None
}

fn should_skip_shrink_walk(entry: &walkdir::DirEntry, root: &Path) -> bool {
    if entry.path() == root {
        return false;
    }
    let name = entry.file_name().to_string_lossy();
    entry.file_type().is_dir()
        && (name.eq_ignore_ascii_case(CAS_DIR_NAME)
            || name.eq_ignore_ascii_case(".duke-ds")
            || name.eq_ignore_ascii_case(".duke-vhdx")
            || name.eq_ignore_ascii_case(".duke-profile")
            || name.eq_ignore_ascii_case("$RECYCLE.BIN")
            || name.eq_ignore_ascii_case("System Volume Information"))
}

fn hash_prefix_8k(path: &Path) -> Result<[u8; 32], String> {
    let mut f = File::open(path)
        .map_err(|e| format!("Failed opening {} for prefix hash: {}", path.display(), e))?;
    let mut buf = [0u8; 8192];
    let n = f
        .read(&mut buf)
        .map_err(|e| format!("Failed reading {}: {}", path.display(), e))?;
    Ok(*blake3::hash(&buf[..n]).as_bytes())
}

fn hash_full_streaming_64k(path: &Path) -> Result<String, String> {
    let mut f = File::open(path)
        .map_err(|e| format!("Failed opening {} for BLAKE3 hash: {}", path.display(), e))?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; HASH_CHUNK_BYTES];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| format!("Failed hashing {}: {}", path.display(), e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn find_duplicate_media_in_dir(
    root: &Path,
    excluded_rel_paths: &HashSet<String>,
) -> Vec<(PathBuf, String, u64, PathBuf)> {
    let mut size_groups: HashMap<u64, Vec<(PathBuf, String)>> = HashMap::new();
    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !should_skip_shrink_walk(e, root))
        .flatten()
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        if !matches!(ext.as_str(), "wem" | "bik" | "bk2" | "usm" | "bnk" | "pck" | "fsb") {
            continue;
        }
        let len = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if len < MIN_MEDIA_DUPLICATE_BYTES {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        if excluded_rel_paths.contains(&rel) {
            continue;
        }
        size_groups.entry(len).or_default().push((path.to_path_buf(), rel));
    }

    let mut duplicates = Vec::new();
    for (len, items) in size_groups {
        if items.len() < 2 {
            continue;
        }
        let mut by_prefix: HashMap<[u8; 32], Vec<(PathBuf, String)>> = HashMap::new();
        for item in items {
            if let Ok(pref) = hash_prefix_8k(&item.0) {
                by_prefix.entry(pref).or_default().push(item);
            }
        }
        for group in by_prefix.into_values() {
            if group.len() < 2 {
                continue;
            }
            let mut by_hash: HashMap<String, Vec<(PathBuf, String)>> = HashMap::new();
            for item in group {
                if let Ok(h) = hash_full_streaming_64k(&item.0) {
                    by_hash.entry(h).or_default().push(item);
                }
            }
            for mut cluster in by_hash.into_values() {
                if cluster.len() < 2 {
                    continue;
                }
                cluster.sort_by(|a, b| a.1.cmp(&b.1));
                let canonical = cluster.remove(0);
                let canon_id = query_file_identity(&canonical.0);
                for (dup_path, dup_rel) in cluster {
                    if let (Some(cid), Some(did)) = (canon_id, query_file_identity(&dup_path)) {
                        if cid == did {
                            continue;
                        }
                    }
                    duplicates.push((dup_path, dup_rel, len, canonical.0.clone()));
                }
            }
        }
    }

    duplicates
}

/// Core deduplication engine: groups `>= min_bytes` files by `(volume, size)`, checks NTFS
/// `FileIndex` so already-hardlinked clusters aren't double-counted, hashes streaming 64 KiB BLAKE3,
/// and links duplicate paths to the canonical file via Win32 `CreateHardLinkW`.
pub fn deduplicate_roots_blake3_internal(
    roots: &[PathBuf],
    min_bytes: u64,
) -> Result<LibraryDedupReport, String> {
    let started = Instant::now();
    let mut seen_paths: HashSet<String> = HashSet::new();
    let mut buckets: HashMap<(String, u64), Vec<PathBuf>> = HashMap::new();
    let mut files_inspected = 0usize;

    for root in roots {
        if !root.is_dir() {
            continue;
        }
        for entry in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !should_skip_shrink_walk(e, root))
            .flatten()
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path().to_path_buf();
            let norm_key = path.to_string_lossy().to_ascii_lowercase();
            if !seen_paths.insert(norm_key) {
                continue;
            }
            let len = entry.metadata().map(|m| m.len()).unwrap_or(0);
            if len < min_bytes {
                continue;
            }
            files_inspected += 1;
            let vol = volume_bucket_key(&path);
            buckets.entry((vol, len)).or_default().push(path);
        }
    }

    let mut candidate_clusters = 0usize;
    let mut files_hardlinked = 0usize;
    let mut already_hardlinked = 0usize;
    let mut bytes_reclaimed = 0u64;
    let mut sample_matches = Vec::new();

    for ((_, len), paths) in buckets {
        if paths.len() < 2 {
            continue;
        }
        candidate_clusters += 1;

        let mut prefix_groups: HashMap<[u8; 32], Vec<PathBuf>> = HashMap::new();
        for p in paths {
            if let Ok(pref) = hash_prefix_8k(&p) {
                prefix_groups.entry(pref).or_default().push(p);
            }
        }

        for pref_group in prefix_groups.into_values() {
            if pref_group.len() < 2 {
                continue;
            }
            let mut full_groups: HashMap<String, Vec<PathBuf>> = HashMap::new();
            for p in pref_group {
                if let Ok(h) = hash_full_streaming_64k(&p) {
                    full_groups.entry(h).or_default().push(p);
                }
            }

            for mut identical in full_groups.into_values() {
                if identical.len() < 2 {
                    continue;
                }
                identical.sort();
                let canonical = identical.remove(0);
                let canon_id = query_file_identity(&canonical);

                for dup in identical {
                    let dup_id = query_file_identity(&dup);
                    if let (Some(c), Some(d)) = (canon_id, dup_id) {
                        if c == d {
                            already_hardlinked += 1;
                            continue;
                        }
                    }
                    // Replace duplicate file with NTFS hardlink to canonical file cleanly.
                    let tmp_bak = dup.with_extension("duke_dedup_tmp");
                    if fs::rename(&dup, &tmp_bak).is_err() {
                        continue;
                    }
                    match create_hard_link(&dup, &canonical) {
                        Ok(()) => {
                            let _ = fs::remove_file(&tmp_bak);
                            files_hardlinked += 1;
                            bytes_reclaimed = bytes_reclaimed.saturating_add(len);
                            if sample_matches.len() < 24 {
                                sample_matches.push(CrossFolderDedupMatch {
                                    canonical_path: canonical.to_string_lossy().to_string(),
                                    duplicate_path: dup.to_string_lossy().to_string(),
                                    bytes_saved: len,
                                });
                            }
                        }
                        Err(_) => {
                            // Restore original untouched if volume doesn't support linking.
                            let _ = fs::rename(&tmp_bak, &dup);
                        }
                    }
                }
            }
        }
    }

    Ok(LibraryDedupReport {
        folders_scanned: roots.len(),
        files_inspected,
        candidate_clusters,
        files_hardlinked,
        already_hardlinked,
        bytes_reclaimed,
        reclaimed_formatted: format_bytes(bytes_reclaimed),
        elapsed_ms: started.elapsed().as_millis() as u64,
        sample_matches,
    })
}

fn scan_game_deep_shrink_internal(
    game_id: &str,
    game_root: &Path,
    keep_languages: &[String],
) -> DeepShrinkScanReport {
    let started = Instant::now();
    let mut bucket_map: HashMap<String, (usize, u64, Vec<String>)> = HashMap::new();
    let mut candidates = Vec::new();
    let mut foreign_rel_set = HashSet::new();

    for entry in WalkDir::new(game_root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !should_skip_shrink_walk(e, game_root))
        .flatten()
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let rel = path
            .strip_prefix(game_root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        if let Some((lang, kind)) = classify_foreign_media_path(&rel, keep_languages) {
            let len = entry.metadata().map(|m| m.len()).unwrap_or(0);
            if len == 0 {
                continue;
            }
            foreign_rel_set.insert(rel.clone());
            let entry_b = bucket_map
                .entry(lang.clone())
                .or_insert_with(|| (0, 0, Vec::new()));
            entry_b.0 += 1;
            entry_b.1 = entry_b.1.saturating_add(len);
            if entry_b.2.len() < 4 {
                entry_b.2.push(rel.clone());
            }
            candidates.push(DeepShrinkCandidateFile {
                rel_path: rel,
                language: lang,
                size_bytes: len,
                kind: kind.to_string(),
            });
        }
    }

    let duplicates = find_duplicate_media_in_dir(game_root, &foreign_rel_set);
    let mut duplicate_media_bytes = 0u64;
    let duplicate_media_files = duplicates.len();
    for (_path, dup_rel, len, _canon) in duplicates {
        duplicate_media_bytes = duplicate_media_bytes.saturating_add(len);
        candidates.push(DeepShrinkCandidateFile {
            rel_path: dup_rel,
            language: "duplicate_media".to_string(),
            size_bytes: len,
            kind: "duplicate_media".to_string(),
        });
    }

    let mut language_buckets: Vec<DeepShrinkLanguageBucket> = bucket_map
        .into_iter()
        .map(|(language, (file_count, total_bytes, sample_files))| DeepShrinkLanguageBucket {
            language,
            file_count,
            total_bytes,
            total_formatted: format_bytes(total_bytes),
            sample_files,
        })
        .collect();
    language_buckets.sort_by_key(|a| std::cmp::Reverse(a.total_bytes));
    candidates.sort_by_key(|a| std::cmp::Reverse(a.size_bytes));
    candidates.truncate(500);

    let foreign_total: u64 = language_buckets.iter().map(|b| b.total_bytes).sum();
    let total_reclaimable_bytes = foreign_total.saturating_add(duplicate_media_bytes);

    DeepShrinkScanReport {
        game_id: game_id.to_string(),
        game_root: game_root.to_string_lossy().to_string(),
        keep_languages: keep_languages.to_vec(),
        language_buckets,
        duplicate_media_files,
        duplicate_media_bytes,
        duplicate_media_formatted: format_bytes(duplicate_media_bytes),
        total_reclaimable_bytes,
        total_reclaimable_formatted: format_bytes(total_reclaimable_bytes),
        candidates,
        elapsed_ms: started.elapsed().as_millis() as u64,
    }
}

#[tauri::command]
pub async fn scan_game_deep_shrink(
    game_id: String,
    keep_languages: Option<Vec<String>>,
) -> Result<DeepShrinkScanReport, String> {
    tokio::task::spawn_blocking(move || {
        let lib = load_library_standalone()?;
        let game = lib
            .iter()
            .find(|g| g.id == game_id)
            .ok_or_else(|| format!("Game not found: {}", game_id))?;
        let root = resolve_game_root_dir(Path::new(&game.exe_path));
        if !root.is_dir() {
            return Err(format!("Game install folder missing: {}", root.display()));
        }
        let keeps = keep_languages.unwrap_or_else(|| vec!["english".to_string()]);
        Ok(scan_game_deep_shrink_internal(&game.id, &root, &keeps))
    })
    .await
    .map_err(|e| format!("Deep shrink scan task failed: {}", e))?
}

#[tauri::command]
pub async fn apply_game_deep_shrink(
    app: tauri::AppHandle,
    game_id: String,
    strip_languages: Vec<String>,
    dedup_internal_media: bool,
    keep_languages: Option<Vec<String>>,
) -> Result<DeepShrinkApplyReport, String> {
    tokio::task::spawn_blocking(move || {
        let started = Instant::now();
        let lib = load_library_standalone()?;
        let game = lib
            .iter()
            .find(|g| g.id == game_id)
            .ok_or_else(|| format!("Game not found: {}", game_id))?;
        let root = resolve_game_root_dir(Path::new(&game.exe_path));
        if !root.is_dir() {
            return Err(format!("Game install folder missing: {}", root.display()));
        }

        let cas_dir = root.join(CAS_DIR_NAME);
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let snap = create_snapshot_in_dir(
            &game.id,
            &root,
            &cas_dir,
            Some(&format!("Pre-Deep Shrink ({})", ts)),
        )?;

        let keep_set = canonical_keep_set(&keep_languages.unwrap_or_else(|| vec!["english".to_string()]));
        let strip_set: HashSet<String> = strip_languages
            .iter()
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty() && !keep_set.contains(s))
            .collect();

        let mut files_removed = 0usize;
        let mut reclaimed_bytes = 0u64;
        let mut removed_rels = HashSet::new();

        if !strip_set.is_empty() {
            let mut to_delete = Vec::new();
            for entry in WalkDir::new(&root)
                .follow_links(false)
                .into_iter()
                .filter_entry(|e| !should_skip_shrink_walk(e, &root))
                .flatten()
            {
                if !entry.file_type().is_file() {
                    continue;
                }
                let path = entry.path();
                let rel = path
                    .strip_prefix(&root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .replace('\\', "/");
                if let Some((lang, _)) = classify_foreign_media_path(&rel, &[]) {
                    if strip_set.contains(&lang) {
                        let len = entry.metadata().map(|m| m.len()).unwrap_or(0);
                        to_delete.push((path.to_path_buf(), rel, len));
                    }
                }
            }
            for (path, rel, len) in to_delete {
                if fs::remove_file(&path).is_ok() {
                    files_removed += 1;
                    reclaimed_bytes = reclaimed_bytes.saturating_add(len);
                    removed_rels.insert(rel);
                }
            }
        }

        let mut duplicates_hardlinked = 0usize;
        if dedup_internal_media {
            let dups = find_duplicate_media_in_dir(&root, &removed_rels);
            for (dup_path, _rel, len, canonical_path) in dups {
                let bak = dup_path.with_extension("duke_dedup_tmp");
                if fs::rename(&dup_path, &bak).is_err() {
                    continue;
                }
                match create_hard_link(&dup_path, &canonical_path) {
                    Ok(()) => {
                        let _ = fs::remove_file(&bak);
                        duplicates_hardlinked += 1;
                        reclaimed_bytes = reclaimed_bytes.saturating_add(len);
                    }
                    Err(_) => {
                        let _ = fs::rename(&bak, &dup_path);
                    }
                }
            }
        }

        let stats = measure_game_storage_stats(&game.id, &root);
        let new_game_bytes = stats.compressed_bytes;
        if let Ok(mut mut_lib) = load_library_from_disk(&app) {
            if let Some(entry) = mut_lib.iter_mut().find(|g| g.id == game_id) {
                entry.size_bytes = new_game_bytes;
                entry.size_formatted = format_bytes(new_game_bytes);
                let _ = save_library_to_disk(&app, &mut_lib);
            }
        }

        Ok(DeepShrinkApplyReport {
            game_id,
            restore_snapshot_id: Some(snap.id),
            files_removed,
            duplicates_hardlinked,
            reclaimed_bytes,
            reclaimed_formatted: format_bytes(reclaimed_bytes),
            new_game_bytes,
            new_game_formatted: format_bytes(new_game_bytes),
            elapsed_ms: started.elapsed().as_millis() as u64,
            message: format!(
                "Deep Shrink reclaimed {} ({} language files stripped, {} duplicate .wem/.bik linked; .duke-cas backup saved).",
                format_bytes(reclaimed_bytes),
                files_removed,
                duplicates_hardlinked
            ),
        })
    })
    .await
    .map_err(|e| format!("Deep shrink apply task failed: {}", e))?
}

#[tauri::command]
pub async fn deduplicate_library_blake3(
    extra_library_root: Option<String>,
    min_file_kb: Option<u64>,
) -> Result<LibraryDedupReport, String> {
    tokio::task::spawn_blocking(move || {
        let mut roots: Vec<PathBuf> = Vec::new();
        let mut seen_dirs: HashSet<String> = HashSet::new();

        if let Ok(lib) = load_library_standalone() {
            for game in lib {
                let root = resolve_game_root_dir(Path::new(&game.exe_path));
                if root.is_dir() {
                    let key = root.to_string_lossy().to_ascii_lowercase();
                    if seen_dirs.insert(key) {
                        roots.push(root);
                    }
                }
            }
        }
        if let Some(extra) = extra_library_root {
            let trimmed = extra.trim();
            if !trimmed.is_empty() {
                let p = PathBuf::from(trimmed);
                if p.is_dir() {
                    let key = p.to_string_lossy().to_ascii_lowercase();
                    if seen_dirs.insert(key) {
                        roots.push(p);
                    }
                }
            }
        }

        let min_bytes = min_file_kb
            .map(|kb| kb.max(64).saturating_mul(1024))
            .unwrap_or(MIN_LIBRARY_DEDUP_BYTES);
        deduplicate_roots_blake3_internal(&roots, min_bytes)
    })
    .await
    .map_err(|e| format!("Library BLAKE3 deduplication task failed: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("duke_shrink_test_{}_{}", name, nonce));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn foreign_language_pack_pattern_matcher_avoids_false_positives() {
        let keep = vec!["english".to_string()];

        assert_eq!(
            classify_foreign_media_path("Audio/vo_french.wem", &keep),
            Some(("french".to_string(), "foreign_voice"))
        );
        assert_eq!(
            classify_foreign_media_path("Movies/intro_german.bik", &keep),
            Some(("german".to_string(), "foreign_cutscene"))
        );
        assert_eq!(
            classify_foreign_media_path("Sound/dialog_japanese.pck", &keep),
            Some(("japanese".to_string(), "foreign_voice"))
        );
        assert_eq!(
            classify_foreign_media_path("Video/ending_spanish.bk2", &keep),
            Some(("spanish".to_string(), "foreign_cutscene"))
        );
        assert_eq!(
            classify_foreign_media_path("Voice/briefing_russian.wem", &keep),
            Some(("russian".to_string(), "foreign_voice"))
        );
        assert_eq!(
            classify_foreign_media_path("Audio/Localized/ja/line_01.wem", &keep),
            Some(("japanese".to_string(), "foreign_voice"))
        );
        assert_eq!(
            classify_foreign_media_path("Audio/sfx_fr.wem", &keep),
            Some(("french".to_string(), "foreign_voice"))
        );

        // Kept language & everyday English stems must NEVER be flagged as foreign packs.
        assert_eq!(classify_foreign_media_path("Audio/vo_english.wem", &keep), None);
        assert_eq!(classify_foreign_media_path("Audio/frame_tick.wem", &keep), None);
        assert_eq!(classify_foreign_media_path("Movies/escape_sequence.bik", &keep), None);
        assert_eq!(classify_foreign_media_path("Audio/destroy_crate.wem", &keep), None);
        assert_eq!(classify_foreign_media_path("Audio/rust_pipes.wem", &keep), None);
        assert_eq!(classify_foreign_media_path("Audio/sprite_click.wem", &keep), None);
    }

    #[test]
    fn cross_folder_blake3_hardlink_deduplication_links_identical_files_across_games() {
        let root = tmp_dir("dedup");
        let game_a = root.join("GameA");
        let game_b = root.join("GameB");
        fs::create_dir_all(&game_a).unwrap();
        fs::create_dir_all(&game_b).unwrap();

        let payload: Vec<u8> = (0..(128 * 1024)).map(|i| (i % 251) as u8).collect();
        let file_a = game_a.join("shared_cinema.bik");
        let file_b = game_b.join("shared_cinema.bik");
        let file_unique = game_b.join("unique_audio.wem");

        fs::File::create(&file_a).unwrap().write_all(&payload).unwrap();
        fs::File::create(&file_b).unwrap().write_all(&payload).unwrap();
        let mut diff_payload = payload.clone();
        diff_payload[42] ^= 0xFF;
        fs::File::create(&file_unique)
            .unwrap()
            .write_all(&diff_payload)
            .unwrap();

        let report1 = deduplicate_roots_blake3_internal(&[game_a.clone(), game_b.clone()], 64 * 1024).unwrap();
        assert_eq!(report1.files_hardlinked, 1);
        assert_eq!(report1.bytes_reclaimed, payload.len() as u64);

        // Running deduplication again must detect the shared NTFS FileIndex and not double-count!
        let report2 = deduplicate_roots_blake3_internal(&[game_a.clone(), game_b.clone()], 64 * 1024).unwrap();
        assert_eq!(report2.files_hardlinked, 0);
        #[cfg(windows)]
        {
            assert_eq!(report2.already_hardlinked, 1);
            assert_eq!(query_file_identity(&file_a), query_file_identity(&file_b));
            assert_ne!(query_file_identity(&file_a), query_file_identity(&file_unique));
        }

        let _ = fs::remove_dir_all(root);
    }
}

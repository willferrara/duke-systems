//! Pure-Rust archive extraction engine (`.zip`, `.7z`, `.tar.zst`, `.tar.gz`, `.tar`)
//! replacing external `7z.exe` and PowerShell `Expand-Archive` fallbacks.

use flate2::read::GzDecoder;
use serde::Serialize;
use sevenz_rust::{Password, SevenZReader};
use std::cell::Cell;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use zip::ZipArchive;
use zstd::stream::read::Decoder as ZstdDecoder;

/// Streaming copy buffer size (64 KiB, always heap-allocated).
const COPY_BUF_SIZE: usize = 64 * 1024;
/// Upper bound on entry count to guard against corrupt header bombs.
const MAX_ARCHIVE_ENTRIES: usize = 500_000;
/// Upper bound on a single extracted entry (128 GiB).
const MAX_SINGLE_ENTRY_BYTES: u64 = 128 * 1024 * 1024 * 1024;
/// Upper bound on cumulative extracted output per archive (512 GiB).
const MAX_TOTAL_EXTRACT_BYTES: u64 = 512 * 1024 * 1024 * 1024;
/// Upper bound on GNU `@LongLink` / PAX metadata payload allocation (1 MiB).
const MAX_TAR_META_BYTES: u64 = 1024 * 1024;
/// Minimum extracted output before checking extreme zip-bomb expansion ratios (256 MiB).
const RATIO_CHECK_MIN_WRITTEN: u64 = 256 * 1024 * 1024;
/// Maximum allowed expansion ratio once `RATIO_CHECK_MIN_WRITTEN` is exceeded.
const MAX_EXPANSION_RATIO: u64 = 1_000;

#[allow(dead_code)]
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ArchiveExtractReport {
    pub format: String,
    pub files_extracted: usize,
    pub dirs_created: usize,
    pub bytes_written: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArchiveFormat {
    Zip,
    SevenZ,
    TarZst,
    TarGz,
    Tar,
}

impl ArchiveFormat {
    fn as_str(self) -> &'static str {
        match self {
            ArchiveFormat::Zip => "zip",
            ArchiveFormat::SevenZ => "7z",
            ArchiveFormat::TarZst => "tar.zst",
            ArchiveFormat::TarGz => "tar.gz",
            ArchiveFormat::Tar => "tar",
        }
    }
}

/// Validates and normalizes an archive-relative entry path, rejecting empty paths,
/// leading `/` or `\`, Windows drive prefixes (`C:`), UNC paths, `.` / `..` traversal
/// components, NUL bytes, and NTFS Alternate Data Streams (`:`).
#[allow(dead_code)]
pub(crate) fn sanitize_archive_relative_path(raw: &str) -> Result<PathBuf, String> {
    if raw.is_empty() || raw.trim().is_empty() {
        return Err("Archive entry path is empty".to_string());
    }
    if raw.contains('\0') {
        return Err("Archive entry path contains NUL byte".to_string());
    }
    if raw.starts_with('/') || raw.starts_with('\\') {
        return Err(format!("Absolute or UNC archive entry path blocked: {raw}"));
    }
    if raw.contains(':') {
        return Err(format!(
            "Drive prefix or NTFS ADS ':' blocked in archive entry path: {raw}"
        ));
    }

    let trimmed = raw.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return Err(format!("Archive entry path resolves to empty: {raw}"));
    }

    let mut rel = PathBuf::new();
    for seg in trimmed.split(['/', '\\']) {
        if seg.is_empty() {
            continue;
        }
        if seg == "." || seg == ".." {
            return Err(format!(
                "Path traversal component '{seg}' blocked in archive entry: {raw}"
            ));
        }

        let mut comps = Path::new(seg).components();
        match (comps.next(), comps.next()) {
            (Some(Component::Normal(_)), None) => {}
            _ => {
                return Err(format!(
                    "Unsafe path component '{seg}' blocked in archive entry: {raw}"
                ));
            }
        }
        rel.push(seg);
    }

    if rel.as_os_str().is_empty() {
        return Err(format!("Archive entry path resolved to empty: {raw}"));
    }
    for comp in rel.components() {
        if !matches!(comp, Component::Normal(_)) {
            return Err(format!("Unsafe component in archive entry path: {raw}"));
        }
    }

    Ok(rel)
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Prefix(p) => out.push(p.as_os_str()),
            Component::RootDir => out.push(comp.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                let can_pop = matches!(out.components().next_back(), Some(Component::Normal(_)));
                if can_pop {
                    out.pop();
                } else {
                    out.push(comp.as_os_str());
                }
            }
            Component::Normal(s) => out.push(s),
        }
    }
    out
}

fn resolve_safe_dest_path(
    dest_dir: &Path,
    canon_dest: &Path,
    raw: &str,
) -> Result<(PathBuf, PathBuf), String> {
    let rel = sanitize_archive_relative_path(raw)?;
    let joined = dest_dir.join(&rel);

    let norm_dest = lexical_normalize(dest_dir);
    let norm_joined = lexical_normalize(&joined);
    if norm_joined == norm_dest || !norm_joined.starts_with(&norm_dest) {
        return Err(format!(
            "Archive entry '{raw}' escapes destination '{}'",
            dest_dir.display()
        ));
    }

    let mut cursor = dest_dir.to_path_buf();
    let mut deepest_existing = canon_dest.to_path_buf();
    for comp in rel.components() {
        cursor.push(comp);
        match fs::symlink_metadata(&cursor) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    return Err(format!(
                        "Refusing to traverse or overwrite symlink inside destination: {}",
                        cursor.display()
                    ));
                }
                if let Ok(canon) = fs::canonicalize(&cursor) {
                    if !canon.starts_with(canon_dest) {
                        return Err(format!(
                            "Canonical path '{}' escapes destination '{}'",
                            canon.display(),
                            canon_dest.display()
                        ));
                    }
                    deepest_existing = canon;
                }
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => break,
            Err(err) => {
                return Err(format!(
                    "Failed inspecting extraction target '{}': {}",
                    cursor.display(),
                    err
                ));
            }
        }
    }

    if !deepest_existing.starts_with(canon_dest) {
        return Err(format!(
            "Destination path '{}' escapes canonical root '{}'",
            joined.display(),
            canon_dest.display()
        ));
    }

    Ok((rel, joined))
}

fn ensure_dir_tracked(
    target_dir: &Path,
    dest_dir: &Path,
    canon_dest: &Path,
    created_dirs: &mut HashSet<PathBuf>,
    verified_dirs: &mut HashSet<PathBuf>,
) -> Result<(), String> {
    if target_dir == dest_dir || verified_dirs.contains(target_dir) {
        return Ok(());
    }

    let rel = target_dir.strip_prefix(dest_dir).map_err(|_| {
        format!(
            "Target directory '{}' is outside destination '{}'",
            target_dir.display(),
            dest_dir.display()
        )
    })?;

    let mut cursor = dest_dir.to_path_buf();
    for comp in rel.components() {
        cursor.push(comp);
        if verified_dirs.contains(&cursor) {
            continue;
        }
        match fs::symlink_metadata(&cursor) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    return Err(format!(
                        "Refusing to traverse symlink directory '{}'",
                        cursor.display()
                    ));
                }
                if !meta.is_dir() {
                    return Err(format!(
                        "Path '{}' already exists as a non-directory",
                        cursor.display()
                    ));
                }
                let canon = fs::canonicalize(&cursor).map_err(|e| {
                    format!("Failed canonicalizing dir '{}': {}", cursor.display(), e)
                })?;
                if !canon.starts_with(canon_dest) {
                    return Err(format!(
                        "Directory '{}' escapes destination '{}'",
                        canon.display(),
                        canon_dest.display()
                    ));
                }
                verified_dirs.insert(cursor.clone());
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&cursor).map_err(|e| {
                    format!("Failed creating directory '{}': {}", cursor.display(), e)
                })?;
                created_dirs.insert(cursor.clone());
                verified_dirs.insert(cursor.clone());
            }
            Err(err) => {
                return Err(format!(
                    "Failed inspecting directory '{}': {}",
                    cursor.display(),
                    err
                ));
            }
        }
    }

    Ok(())
}

fn ensure_output_file_not_symlink(target_path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(target_path) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(format!(
                    "Refusing to overwrite symlink at '{}'",
                    target_path.display()
                ));
            }
            if meta.is_dir() {
                return Err(format!(
                    "Cannot overwrite existing directory '{}' with file",
                    target_path.display()
                ));
            }
            Ok(())
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!(
            "Failed inspecting output file '{}': {}",
            target_path.display(),
            err
        )),
    }
}

fn check_write_bounds(bytes_written: u64, archive_file_len: u64) -> Result<(), String> {
    if bytes_written > MAX_TOTAL_EXTRACT_BYTES {
        return Err(format!(
            "Archive extraction aborted: output exceeded {} GiB limit",
            MAX_TOTAL_EXTRACT_BYTES / (1024 * 1024 * 1024)
        ));
    }
    if bytes_written >= RATIO_CHECK_MIN_WRITTEN && archive_file_len > 0 {
        let ratio = bytes_written / archive_file_len;
        if ratio > MAX_EXPANSION_RATIO {
            return Err(format!(
                "Archive expansion ratio ({ratio}:1) exceeded safe limit; possible zip-bomb"
            ));
        }
    }
    Ok(())
}

fn rel_label(rel: &Path) -> String {
    rel.to_string_lossy().replace('\\', "/")
}

fn detect_archive_format(archive_path: &Path) -> Result<ArchiveFormat, String> {
    let mut file = File::open(archive_path).map_err(|e| {
        format!(
            "Failed opening archive '{}' for format detection: {}",
            archive_path.display(),
            e
        )
    })?;

    let mut head = [0u8; 512];
    let mut read_len = 0usize;
    while read_len < head.len() {
        let n = file.read(&mut head[read_len..]).map_err(|e| {
            format!(
                "Failed reading header of '{}': {}",
                archive_path.display(),
                e
            )
        })?;
        if n == 0 {
            break;
        }
        read_len += n;
    }

    if read_len >= 4 {
        let sig4 = &head[..4];
        if sig4 == [0x50, 0x4B, 0x03, 0x04]
            || sig4 == [0x50, 0x4B, 0x05, 0x06]
            || sig4 == [0x50, 0x4B, 0x07, 0x08]
        {
            return Ok(ArchiveFormat::Zip);
        }
        if sig4 == [0x28, 0xB5, 0x2F, 0xFD]
            || ((sig4[0] & 0xF0) == 0x50 && sig4[1..4] == [0x2A, 0x4D, 0x18])
        {
            return Ok(ArchiveFormat::TarZst);
        }
    }
    if read_len >= 6 && head[..6] == [b'7', b'z', 0xBC, 0xAF, 0x27, 0x1C] {
        return Ok(ArchiveFormat::SevenZ);
    }
    if read_len >= 2 && head[..2] == [0x1F, 0x8B] {
        return Ok(ArchiveFormat::TarGz);
    }
    if read_len == 512 && looks_like_tar_header(&head) {
        return Ok(ArchiveFormat::Tar);
    }

    let lower_name = archive_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if lower_name.ends_with(".tar.zst") || lower_name.ends_with(".tzst") {
        return Ok(ArchiveFormat::TarZst);
    }
    if lower_name.ends_with(".tar.gz") || lower_name.ends_with(".tgz") {
        return Ok(ArchiveFormat::TarGz);
    }
    if lower_name.ends_with(".tar") {
        return Ok(ArchiveFormat::Tar);
    }
    if lower_name.ends_with(".zip") {
        return Ok(ArchiveFormat::Zip);
    }
    if lower_name.ends_with(".7z") {
        return Ok(ArchiveFormat::SevenZ);
    }

    Err(format!(
        "Unsupported or unrecognized archive format: {}",
        archive_path.display()
    ))
}

/// Pure-Rust extraction with progress callback `(current, total, entry_name)`.
#[allow(dead_code)]
pub(crate) fn extract_archive_with_progress<F>(
    archive_path: &Path,
    dest_dir: &Path,
    mut on_progress: F,
) -> Result<ArchiveExtractReport, String>
where
    F: FnMut(u64, u64, &str),
{
    let meta = fs::metadata(archive_path).map_err(|e| {
        format!(
            "Failed reading metadata for '{}': {}",
            archive_path.display(),
            e
        )
    })?;
    if !meta.is_file() {
        return Err(format!(
            "Archive path '{}' is not a regular file",
            archive_path.display()
        ));
    }
    let archive_file_len = meta.len();
    if archive_file_len == 0 {
        return Err(format!(
            "Archive '{}' is empty (0 bytes)",
            archive_path.display()
        ));
    }

    let format = detect_archive_format(archive_path)?;

    fs::create_dir_all(dest_dir).map_err(|e| {
        format!(
            "Failed creating destination directory '{}': {}",
            dest_dir.display(),
            e
        )
    })?;
    let canon_dest = fs::canonicalize(dest_dir).map_err(|e| {
        format!(
            "Failed canonicalizing destination '{}': {}",
            dest_dir.display(),
            e
        )
    })?;

    match format {
        ArchiveFormat::Zip => extract_zip(
            archive_path,
            archive_file_len,
            dest_dir,
            &canon_dest,
            &mut on_progress,
        ),
        ArchiveFormat::SevenZ => extract_sevenz(
            archive_path,
            archive_file_len,
            dest_dir,
            &canon_dest,
            &mut on_progress,
        ),
        ArchiveFormat::TarZst => extract_tar_zst(
            archive_path,
            archive_file_len,
            dest_dir,
            &canon_dest,
            &mut on_progress,
        ),
        ArchiveFormat::TarGz => extract_tar_gz(
            archive_path,
            archive_file_len,
            dest_dir,
            &canon_dest,
            &mut on_progress,
        ),
        ArchiveFormat::Tar => extract_tar_plain(
            archive_path,
            archive_file_len,
            dest_dir,
            &canon_dest,
            &mut on_progress,
        ),
    }
}

/// Pure-Rust extraction without progress reporting.
#[allow(dead_code)]
pub(crate) fn extract_archive_pure_rust(
    archive_path: &Path,
    dest_dir: &Path,
) -> Result<ArchiveExtractReport, String> {
    extract_archive_with_progress(archive_path, dest_dir, |_, _, _| {})
}

fn extract_zip<F>(
    archive_path: &Path,
    archive_file_len: u64,
    dest_dir: &Path,
    canon_dest: &Path,
    on_progress: &mut F,
) -> Result<ArchiveExtractReport, String>
where
    F: FnMut(u64, u64, &str),
{
    let file = File::open(archive_path)
        .map_err(|e| format!("Failed opening ZIP '{}': {}", archive_path.display(), e))?;
    let reader = BufReader::with_capacity(COPY_BUF_SIZE, file);
    let mut archive = ZipArchive::new(reader)
        .map_err(|e| format!("Invalid ZIP archive '{}': {}", archive_path.display(), e))?;

    let entry_count = archive.len();
    if entry_count > MAX_ARCHIVE_ENTRIES {
        return Err(format!(
            "ZIP archive '{}' exceeds maximum entry limit ({})",
            archive_path.display(),
            MAX_ARCHIVE_ENTRIES
        ));
    }

    let mut total_bytes: u64 = 0;
    for idx in 0..entry_count {
        let entry = archive.by_index(idx).map_err(|e| {
            format!(
                "Failed inspecting ZIP entry #{idx} in '{}': {}",
                archive_path.display(),
                e
            )
        })?;
        let _ = resolve_safe_dest_path(dest_dir, canon_dest, entry.name())?;
        if !entry.is_dir() && !entry.is_symlink() {
            let sz = entry.size();
            if sz > MAX_SINGLE_ENTRY_BYTES {
                return Err(format!(
                    "ZIP entry '{}' declared size {} exceeds single-file bound",
                    entry.name(),
                    sz
                ));
            }
            total_bytes = total_bytes
                .checked_add(sz)
                .ok_or_else(|| "ZIP uncompressed byte total overflow".to_string())?;
            if total_bytes > MAX_TOTAL_EXTRACT_BYTES {
                return Err("ZIP uncompressed total exceeds safe limit".to_string());
            }
        }
    }

    let mut buf = vec![0u8; COPY_BUF_SIZE];
    let mut created_dirs: HashSet<PathBuf> = HashSet::new();
    let mut verified_dirs: HashSet<PathBuf> = HashSet::new();
    let mut files_extracted: usize = 0;
    let mut bytes_written: u64 = 0;

    for idx in 0..entry_count {
        let mut entry = archive.by_index(idx).map_err(|e| {
            format!(
                "Failed opening ZIP entry #{idx} in '{}': {}",
                archive_path.display(),
                e
            )
        })?;
        let raw_name = entry.name().to_string();
        let is_dir = entry.is_dir();
        let is_symlink = entry.is_symlink();
        let declared_size = entry.size();

        let (rel, target_path) = resolve_safe_dest_path(dest_dir, canon_dest, &raw_name)?;
        let label = rel_label(&rel);

        if is_dir {
            ensure_dir_tracked(
                &target_path,
                dest_dir,
                canon_dest,
                &mut created_dirs,
                &mut verified_dirs,
            )?;
            on_progress(bytes_written, total_bytes.max(bytes_written), &label);
            continue;
        }
        if is_symlink {
            continue;
        }

        if let Some(parent) = target_path.parent() {
            ensure_dir_tracked(
                parent,
                dest_dir,
                canon_dest,
                &mut created_dirs,
                &mut verified_dirs,
            )?;
        }
        ensure_output_file_not_symlink(&target_path)?;

        let out_file = File::create(&target_path).map_err(|e| {
            format!(
                "Failed creating extracted file '{}': {}",
                target_path.display(),
                e
            )
        })?;
        let mut writer = BufWriter::with_capacity(COPY_BUF_SIZE, out_file);
        let mut entry_written: u64 = 0;

        loop {
            let n = entry
                .read(&mut buf)
                .map_err(|e| format!("Failed decompressing ZIP entry '{}': {}", label, e))?;
            if n == 0 {
                break;
            }
            entry_written = entry_written
                .checked_add(n as u64)
                .ok_or_else(|| "ZIP entry size overflow".to_string())?;
            if entry_written > declared_size {
                return Err(format!(
                    "ZIP entry '{}' exceeded declared header size ({} > {})",
                    label, entry_written, declared_size
                ));
            }
            writer.write_all(&buf[..n]).map_err(|e| {
                format!(
                    "Failed writing extracted file '{}': {}",
                    target_path.display(),
                    e
                )
            })?;
            bytes_written = bytes_written
                .checked_add(n as u64)
                .ok_or_else(|| "ZIP total extracted size overflow".to_string())?;
            check_write_bounds(bytes_written, archive_file_len)?;
            on_progress(bytes_written, total_bytes.max(bytes_written), &label);
        }

        writer.flush().map_err(|e| {
            format!(
                "Failed flushing extracted file '{}': {}",
                target_path.display(),
                e
            )
        })?;
        if declared_size == 0 {
            on_progress(bytes_written, total_bytes.max(bytes_written), &label);
        }
        files_extracted += 1;
    }

    on_progress(bytes_written, total_bytes.max(bytes_written), "");

    Ok(ArchiveExtractReport {
        format: ArchiveFormat::Zip.as_str().to_string(),
        files_extracted,
        dirs_created: created_dirs.len(),
        bytes_written,
    })
}

fn extract_sevenz<F>(
    archive_path: &Path,
    archive_file_len: u64,
    dest_dir: &Path,
    canon_dest: &Path,
    on_progress: &mut F,
) -> Result<ArchiveExtractReport, String>
where
    F: FnMut(u64, u64, &str),
{
    let file = File::open(archive_path)
        .map_err(|e| format!("Failed opening 7z '{}': {}", archive_path.display(), e))?;
    let mut sz_reader = SevenZReader::new(file, archive_file_len, Password::empty())
        .map_err(|e| format!("Invalid 7z archive '{}': {}", archive_path.display(), e))?;

    let entry_count = sz_reader.archive().files.len();
    if entry_count > MAX_ARCHIVE_ENTRIES {
        return Err(format!(
            "7z archive '{}' exceeds maximum entry limit ({})",
            archive_path.display(),
            MAX_ARCHIVE_ENTRIES
        ));
    }

    let mut total_bytes: u64 = 0;
    for entry in &sz_reader.archive().files {
        if entry.is_anti_item() {
            continue;
        }
        let _ = resolve_safe_dest_path(dest_dir, canon_dest, entry.name())?;
        if !entry.is_directory() {
            let sz = entry.size();
            if sz > MAX_SINGLE_ENTRY_BYTES {
                return Err(format!(
                    "7z entry '{}' declared size {} exceeds single-file bound",
                    entry.name(),
                    sz
                ));
            }
            total_bytes = total_bytes
                .checked_add(sz)
                .ok_or_else(|| "7z uncompressed byte total overflow".to_string())?;
            if total_bytes > MAX_TOTAL_EXTRACT_BYTES {
                return Err("7z uncompressed total exceeds safe limit".to_string());
            }
        }
    }

    let mut buf = vec![0u8; COPY_BUF_SIZE];
    let mut created_dirs: HashSet<PathBuf> = HashSet::new();
    let mut verified_dirs: HashSet<PathBuf> = HashSet::new();
    let mut files_extracted: usize = 0;
    let mut bytes_written: u64 = 0;
    let mut callback_err: Option<String> = None;

    let res = sz_reader.for_each_entries(|entry, entry_reader| {
        if entry.is_anti_item() {
            return Ok(true);
        }

        let (rel, target_path) = match resolve_safe_dest_path(dest_dir, canon_dest, entry.name()) {
            Ok(v) => v,
            Err(err) => {
                callback_err = Some(err.clone());
                return Err(sevenz_rust::Error::other(err));
            }
        };
        let label = rel_label(&rel);

        if entry.is_directory() {
            if let Err(err) = ensure_dir_tracked(
                &target_path,
                dest_dir,
                canon_dest,
                &mut created_dirs,
                &mut verified_dirs,
            ) {
                callback_err = Some(err.clone());
                return Err(sevenz_rust::Error::other(err));
            }
            on_progress(bytes_written, total_bytes.max(bytes_written), &label);
            return Ok(true);
        }

        if let Some(parent) = target_path.parent() {
            if let Err(err) = ensure_dir_tracked(
                parent,
                dest_dir,
                canon_dest,
                &mut created_dirs,
                &mut verified_dirs,
            ) {
                callback_err = Some(err.clone());
                return Err(sevenz_rust::Error::other(err));
            }
        }

        if let Err(err) = ensure_output_file_not_symlink(&target_path) {
            callback_err = Some(err.clone());
            return Err(sevenz_rust::Error::other(err));
        }

        let out_file = match File::create(&target_path) {
            Ok(f) => f,
            Err(e) => {
                let err = format!(
                    "Failed creating extracted 7z file '{}': {}",
                    target_path.display(),
                    e
                );
                callback_err = Some(err.clone());
                return Err(sevenz_rust::Error::other(err));
            }
        };
        let mut writer = BufWriter::with_capacity(COPY_BUF_SIZE, out_file);
        let declared_size = entry.size();
        let mut entry_written: u64 = 0;

        loop {
            let n = match entry_reader.read(&mut buf) {
                Ok(n) => n,
                Err(e) => {
                    let err = format!("Failed decompressing 7z entry '{}': {}", label, e);
                    callback_err = Some(err.clone());
                    return Err(sevenz_rust::Error::other(err));
                }
            };
            if n == 0 {
                break;
            }
            entry_written = match entry_written.checked_add(n as u64) {
                Some(v) => v,
                None => {
                    let err = "7z entry size overflow".to_string();
                    callback_err = Some(err.clone());
                    return Err(sevenz_rust::Error::other(err));
                }
            };
            if entry_written > declared_size {
                let err = format!(
                    "7z entry '{}' exceeded declared header size ({} > {})",
                    label, entry_written, declared_size
                );
                callback_err = Some(err.clone());
                return Err(sevenz_rust::Error::other(err));
            }
            if let Err(e) = writer.write_all(&buf[..n]) {
                let err = format!(
                    "Failed writing extracted 7z file '{}': {}",
                    target_path.display(),
                    e
                );
                callback_err = Some(err.clone());
                return Err(sevenz_rust::Error::other(err));
            }
            bytes_written = match bytes_written.checked_add(n as u64) {
                Some(v) => v,
                None => {
                    let err = "7z total extracted size overflow".to_string();
                    callback_err = Some(err.clone());
                    return Err(sevenz_rust::Error::other(err));
                }
            };
            if let Err(err) = check_write_bounds(bytes_written, archive_file_len) {
                callback_err = Some(err.clone());
                return Err(sevenz_rust::Error::other(err));
            }
            on_progress(bytes_written, total_bytes.max(bytes_written), &label);
        }

        if let Err(e) = writer.flush() {
            let err = format!(
                "Failed flushing extracted 7z file '{}': {}",
                target_path.display(),
                e
            );
            callback_err = Some(err.clone());
            return Err(sevenz_rust::Error::other(err));
        }

        if declared_size == 0 {
            on_progress(bytes_written, total_bytes.max(bytes_written), &label);
        }
        files_extracted += 1;
        Ok(true)
    });

    if let Some(err) = callback_err {
        return Err(err);
    }
    res.map_err(|e| {
        format!(
            "Failed extracting 7z archive '{}': {}",
            archive_path.display(),
            e
        )
    })?;

    on_progress(bytes_written, total_bytes.max(bytes_written), "");

    Ok(ArchiveExtractReport {
        format: ArchiveFormat::SevenZ.as_str().to_string(),
        files_extracted,
        dirs_created: created_dirs.len(),
        bytes_written,
    })
}

struct CountingReader<R> {
    inner: R,
    bytes_read: Rc<Cell<u64>>,
}

impl<R: Read> Read for CountingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.bytes_read
            .set(self.bytes_read.get().saturating_add(n as u64));
        Ok(n)
    }
}

type CountingFileStream = (CountingReader<BufReader<File>>, Rc<Cell<u64>>);

fn open_counting_file(archive_path: &Path) -> Result<CountingFileStream, String> {
    let file = File::open(archive_path)
        .map_err(|e| format!("Failed opening archive '{}': {}", archive_path.display(), e))?;
    let counter = Rc::new(Cell::new(0u64));
    let reader = CountingReader {
        inner: BufReader::with_capacity(COPY_BUF_SIZE, file),
        bytes_read: Rc::clone(&counter),
    };
    Ok((reader, counter))
}

fn extract_tar_zst<F>(
    archive_path: &Path,
    archive_file_len: u64,
    dest_dir: &Path,
    canon_dest: &Path,
    on_progress: &mut F,
) -> Result<ArchiveExtractReport, String>
where
    F: FnMut(u64, u64, &str),
{
    let (counting, bytes_read) = open_counting_file(archive_path)?;
    let mut decoder = ZstdDecoder::new(counting).map_err(|e| {
        format!(
            "Failed initializing zstd decoder for '{}': {}",
            archive_path.display(),
            e
        )
    })?;

    let mut report = extract_tar_stream(
        &mut decoder,
        ArchiveFormat::TarZst,
        archive_file_len,
        &bytes_read,
        dest_dir,
        canon_dest,
        on_progress,
    )?;

    drain_compressed_trailer(&mut decoder, "zstd")?;
    on_progress(archive_file_len, archive_file_len, "");
    report.format = ArchiveFormat::TarZst.as_str().to_string();
    Ok(report)
}

fn extract_tar_gz<F>(
    archive_path: &Path,
    archive_file_len: u64,
    dest_dir: &Path,
    canon_dest: &Path,
    on_progress: &mut F,
) -> Result<ArchiveExtractReport, String>
where
    F: FnMut(u64, u64, &str),
{
    let (counting, bytes_read) = open_counting_file(archive_path)?;
    let mut decoder = GzDecoder::new(counting);

    let mut report = extract_tar_stream(
        &mut decoder,
        ArchiveFormat::TarGz,
        archive_file_len,
        &bytes_read,
        dest_dir,
        canon_dest,
        on_progress,
    )?;

    drain_compressed_trailer(&mut decoder, "gzip")?;
    on_progress(archive_file_len, archive_file_len, "");
    report.format = ArchiveFormat::TarGz.as_str().to_string();
    Ok(report)
}

fn extract_tar_plain<F>(
    archive_path: &Path,
    archive_file_len: u64,
    dest_dir: &Path,
    canon_dest: &Path,
    on_progress: &mut F,
) -> Result<ArchiveExtractReport, String>
where
    F: FnMut(u64, u64, &str),
{
    let (mut counting, bytes_read) = open_counting_file(archive_path)?;
    let mut report = extract_tar_stream(
        &mut counting,
        ArchiveFormat::Tar,
        archive_file_len,
        &bytes_read,
        dest_dir,
        canon_dest,
        on_progress,
    )?;

    on_progress(archive_file_len, archive_file_len, "");
    report.format = ArchiveFormat::Tar.as_str().to_string();
    Ok(report)
}

fn drain_compressed_trailer<R: Read>(reader: &mut R, codec_name: &str) -> Result<(), String> {
    let mut drain_buf = vec![0u8; 8 * 1024];
    let mut drained: u64 = 0;
    loop {
        let n = reader
            .read(&mut drain_buf)
            .map_err(|e| format!("Corrupt {codec_name} stream trailer: {e}"))?;
        if n == 0 {
            break;
        }
        drained = drained.saturating_add(n as u64);
        if drained > 16 * 1024 * 1024 {
            return Err(format!(
                "Unexpected excessive trailing data after tar EOF in {codec_name} stream"
            ));
        }
    }
    Ok(())
}

fn parse_tar_checksum(field: &[u8]) -> Option<u64> {
    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    let s = std::str::from_utf8(&field[..end]).ok()?.trim();
    if s.is_empty() {
        return None;
    }
    u64::from_str_radix(s, 8).ok()
}

fn parse_tar_numeric(field: &[u8]) -> Result<u64, String> {
    if !field.is_empty() && (field[0] & 0x80) != 0 {
        if (field[0] & 0x40) != 0 {
            return Err("Negative base-256 numeric field in tar header".to_string());
        }
        let mut val: u64 = (field[0] & 0x3F) as u64;
        for &b in &field[1..] {
            val = val
                .checked_shl(8)
                .ok_or_else(|| "Overflow in tar base-256 numeric field".to_string())?
                | (b as u64);
        }
        return Ok(val);
    }

    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    let s = std::str::from_utf8(&field[..end])
        .map_err(|_| "Non-UTF-8 octal field in tar header".to_string())?
        .trim();
    if s.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(s, 8).map_err(|_| format!("Invalid octal field in tar header: '{s}'"))
}

fn looks_like_tar_header(hdr: &[u8; 512]) -> bool {
    if hdr.iter().all(|&b| b == 0) {
        return false;
    }
    let Some(expected) = parse_tar_checksum(&hdr[148..156]) else {
        return false;
    };

    let mut unsigned_sum: u64 = 0;
    let mut signed_sum: i64 = 0;
    for (idx, &b) in hdr.iter().enumerate() {
        if (148..156).contains(&idx) {
            unsigned_sum += b' ' as u64;
            signed_sum += b' ' as i64;
        } else {
            unsigned_sum += b as u64;
            signed_sum += (b as i8) as i64;
        }
    }

    unsigned_sum == expected || (signed_sum >= 0 && signed_sum as u64 == expected)
}

fn extract_tar_cstr(field: &[u8]) -> Result<String, String> {
    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    std::str::from_utf8(&field[..end])
        .map(|s| s.to_string())
        .map_err(|_| "Tar header path field contains invalid UTF-8".to_string())
}

fn read_exact_or_eof<R: Read>(reader: &mut R, buf: &mut [u8]) -> Result<bool, String> {
    let mut filled = 0usize;
    while filled < buf.len() {
        let n = reader
            .read(&mut buf[filled..])
            .map_err(|e| format!("Archive stream read failed: {e}"))?;
        if n == 0 {
            if filled == 0 {
                return Ok(false);
            }
            return Err("Unexpected EOF mid-record in tar stream".to_string());
        }
        filled += n;
    }
    Ok(true)
}

fn read_exact_bytes<R: Read>(reader: &mut R, buf: &mut [u8], context: &str) -> Result<(), String> {
    if !read_exact_or_eof(reader, buf)? {
        return Err(format!("Unexpected EOF reading {context} in tar stream"));
    }
    Ok(())
}

fn drain_tar_bytes<R: Read>(reader: &mut R, mut size: u64, buf: &mut [u8]) -> Result<(), String> {
    while size > 0 {
        let want = (size as usize).min(buf.len());
        read_exact_bytes(reader, &mut buf[..want], "tar entry payload")?;
        size -= want as u64;
    }
    Ok(())
}

fn parse_pax_records(pax_bytes: &[u8]) -> Result<(Option<String>, Option<u64>), String> {
    let mut path_out: Option<String> = None;
    let mut size_out: Option<u64> = None;
    let mut pos = 0usize;

    while pos < pax_bytes.len() {
        let slice = &pax_bytes[pos..];
        let Some(sp_rel) = slice.iter().position(|&b| b == b' ') else {
            return Err("Malformed PAX header record (missing length delimiter)".to_string());
        };
        let len_str = std::str::from_utf8(&slice[..sp_rel])
            .map_err(|_| "Non-UTF-8 length in PAX header record".to_string())?;
        let rec_len: usize = len_str
            .parse()
            .map_err(|_| format!("Invalid PAX record length: '{len_str}'"))?;
        if rec_len < sp_rel + 2 || pos + rec_len > pax_bytes.len() {
            return Err("Out-of-bounds PAX record length".to_string());
        }

        let kv_slice = &slice[sp_rel + 1..rec_len];
        let kv_trimmed = kv_slice.strip_suffix(b"\n").unwrap_or(kv_slice);
        if let Some(eq_pos) = kv_trimmed.iter().position(|&b| b == b'=') {
            let key = &kv_trimmed[..eq_pos];
            let val = &kv_trimmed[eq_pos + 1..];
            if key == b"path" {
                let path_str = std::str::from_utf8(val)
                    .map_err(|_| "Non-UTF-8 path value in PAX extended header".to_string())?;
                path_out = Some(path_str.to_string());
            } else if key == b"size" {
                let sz_str = std::str::from_utf8(val)
                    .map_err(|_| "Non-UTF-8 size value in PAX extended header".to_string())?;
                let parsed = sz_str
                    .parse::<u64>()
                    .map_err(|_| format!("Invalid PAX size value '{sz_str}'"))?;
                size_out = Some(parsed);
            }
        }

        pos += rec_len;
    }

    Ok((path_out, size_out))
}

fn extract_tar_stream<R: Read, F>(
    reader: &mut R,
    format: ArchiveFormat,
    archive_file_len: u64,
    compressed_read: &Cell<u64>,
    dest_dir: &Path,
    canon_dest: &Path,
    on_progress: &mut F,
) -> Result<ArchiveExtractReport, String>
where
    F: FnMut(u64, u64, &str),
{
    let mut copy_buf = vec![0u8; COPY_BUF_SIZE];
    let mut created_dirs: HashSet<PathBuf> = HashSet::new();
    let mut verified_dirs: HashSet<PathBuf> = HashSet::new();
    let mut files_extracted: usize = 0;
    let mut bytes_written: u64 = 0;
    let mut entry_count: usize = 0;
    let mut saw_any_block = false;
    let mut gnu_long_name: Option<String> = None;
    let mut pax_next_path: Option<String> = None;
    let mut pax_next_size: Option<u64> = None;

    loop {
        let mut hdr = [0u8; 512];
        if !read_exact_or_eof(reader, &mut hdr)? {
            break;
        }
        saw_any_block = true;

        if hdr.iter().all(|&b| b == 0) {
            break;
        }

        if !looks_like_tar_header(&hdr) {
            return Err("Invalid tar header checksum in archive stream".to_string());
        }

        entry_count += 1;
        if entry_count > MAX_ARCHIVE_ENTRIES {
            return Err(format!(
                "Tar archive exceeds maximum entry limit ({MAX_ARCHIVE_ENTRIES})"
            ));
        }

        let header_size = parse_tar_numeric(&hdr[124..136])?;
        let typeflag = hdr[156];

        let raw_name = extract_tar_cstr(&hdr[..100])?;
        let is_ustar = hdr.len() >= 263 && &hdr[257..262] == b"ustar";
        let raw_prefix = if is_ustar {
            extract_tar_cstr(&hdr[345..500])?
        } else {
            String::new()
        };
        let fallback_name = if raw_prefix.is_empty() {
            raw_name
        } else if raw_name.is_empty() {
            raw_prefix
        } else {
            format!("{raw_prefix}/{raw_name}")
        };

        let is_meta_block = matches!(typeflag, b'L' | b'K' | b'x' | b'g' | b'X');
        let size = if is_meta_block {
            header_size
        } else {
            pax_next_size.take().unwrap_or(header_size)
        };

        if size > MAX_SINGLE_ENTRY_BYTES {
            return Err(format!(
                "Tar entry size ({size} bytes) exceeds single-file safety limit"
            ));
        }

        match typeflag {
            b'L' => {
                if size > MAX_TAR_META_BYTES {
                    return Err(format!(
                        "GNU @LongLink path metadata ({size} bytes) exceeds 1 MiB safety limit"
                    ));
                }
                let mut name_bytes = vec![0u8; size as usize];
                read_exact_bytes(reader, &mut name_bytes, "GNU @LongLink name")?;
                let end = name_bytes
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(name_bytes.len());
                let long_name = std::str::from_utf8(&name_bytes[..end])
                    .map_err(|_| "GNU @LongLink name contains invalid UTF-8".to_string())?
                    .to_string();
                gnu_long_name = Some(long_name);
            }
            b'K' => {
                if size > MAX_TAR_META_BYTES {
                    return Err(format!(
                        "GNU long linkname metadata ({size} bytes) exceeds safety limit"
                    ));
                }
                drain_tar_bytes(reader, size, &mut copy_buf)?;
            }
            b'x' | b'X' => {
                if size > MAX_TAR_META_BYTES {
                    return Err(format!(
                        "PAX extended header ({size} bytes) exceeds 1 MiB safety limit"
                    ));
                }
                let mut pax_bytes = vec![0u8; size as usize];
                read_exact_bytes(reader, &mut pax_bytes, "PAX extended header")?;
                let (p_path, p_size) = parse_pax_records(&pax_bytes)?;
                if p_path.is_some() {
                    pax_next_path = p_path;
                }
                if p_size.is_some() {
                    pax_next_size = p_size;
                }
            }
            b'g' => {
                if size > MAX_TAR_META_BYTES {
                    return Err(format!(
                        "PAX global header ({size} bytes) exceeds 1 MiB safety limit"
                    ));
                }
                drain_tar_bytes(reader, size, &mut copy_buf)?;
            }
            b'5' => {
                let entry_name = pax_next_path
                    .take()
                    .or_else(|| gnu_long_name.take())
                    .unwrap_or(fallback_name);
                let (rel, target_dir) = resolve_safe_dest_path(dest_dir, canon_dest, &entry_name)?;
                ensure_dir_tracked(
                    &target_dir,
                    dest_dir,
                    canon_dest,
                    &mut created_dirs,
                    &mut verified_dirs,
                )?;
                drain_tar_bytes(reader, size, &mut copy_buf)?;
                on_progress(
                    compressed_read.get().min(archive_file_len),
                    archive_file_len,
                    &rel_label(&rel),
                );
            }
            b'0' | 0 => {
                let entry_name = pax_next_path
                    .take()
                    .or_else(|| gnu_long_name.take())
                    .unwrap_or(fallback_name);
                let is_legacy_dir = typeflag == 0
                    && size == 0
                    && (entry_name.ends_with('/') || entry_name.ends_with('\\'));
                let (rel, target_path) = resolve_safe_dest_path(dest_dir, canon_dest, &entry_name)?;
                let label = rel_label(&rel);

                if is_legacy_dir {
                    ensure_dir_tracked(
                        &target_path,
                        dest_dir,
                        canon_dest,
                        &mut created_dirs,
                        &mut verified_dirs,
                    )?;
                    on_progress(
                        compressed_read.get().min(archive_file_len),
                        archive_file_len,
                        &label,
                    );
                } else {
                    if let Some(parent) = target_path.parent() {
                        ensure_dir_tracked(
                            parent,
                            dest_dir,
                            canon_dest,
                            &mut created_dirs,
                            &mut verified_dirs,
                        )?;
                    }
                    ensure_output_file_not_symlink(&target_path)?;

                    let out_f = File::create(&target_path).map_err(|e| {
                        format!(
                            "Failed creating extracted tar file '{}': {}",
                            target_path.display(),
                            e
                        )
                    })?;
                    let mut writer = BufWriter::with_capacity(COPY_BUF_SIZE, out_f);
                    let mut left = size;

                    while left > 0 {
                        let want = (left as usize).min(copy_buf.len());
                        read_exact_bytes(reader, &mut copy_buf[..want], &label)?;
                        writer.write_all(&copy_buf[..want]).map_err(|e| {
                            format!(
                                "Failed writing extracted tar file '{}': {}",
                                target_path.display(),
                                e
                            )
                        })?;
                        left -= want as u64;
                        bytes_written = bytes_written
                            .checked_add(want as u64)
                            .ok_or_else(|| "Tar total extracted size overflow".to_string())?;
                        check_write_bounds(bytes_written, archive_file_len)?;
                        on_progress(
                            compressed_read.get().min(archive_file_len),
                            archive_file_len,
                            &label,
                        );
                    }

                    writer.flush().map_err(|e| {
                        format!(
                            "Failed flushing extracted tar file '{}': {}",
                            target_path.display(),
                            e
                        )
                    })?;
                    if size == 0 {
                        on_progress(
                            compressed_read.get().min(archive_file_len),
                            archive_file_len,
                            &label,
                        );
                    }
                    files_extracted += 1;
                }
            }
            _ => {
                // Skip non-regular metadata/links without creating symlinks or hardlinks on disk.
                pax_next_path.take();
                gnu_long_name.take();
                drain_tar_bytes(reader, size, &mut copy_buf)?;
            }
        }

        let padded_rem = ((512 - (size % 512)) % 512) as usize;
        if padded_rem > 0 {
            let mut pad = [0u8; 512];
            read_exact_bytes(reader, &mut pad[..padded_rem], "tar block padding")?;
        }
    }

    if !saw_any_block {
        return Err("Archive stream is empty (no tar headers found)".to_string());
    }

    Ok(ArchiveExtractReport {
        format: format.as_str().to_string(),
        files_extracted,
        dirs_created: created_dirs.len(),
        bytes_written,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crc32fast::Hasher as Crc32Hasher;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::sync::atomic::{AtomicU64, Ordering};
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};
    use zstd::stream::write::Encoder as ZstdEncoder;

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(1);

    struct TempDirGuard {
        path: PathBuf,
    }

    impl TempDirGuard {
        fn new(label: &str) -> Self {
            let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "duke_archive_native_test_{}_{}_{}",
                label,
                std::process::id(),
                id
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create test temp dir");
            Self { path }
        }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn write_octal(dst: &mut [u8], val: u64) {
        let width = dst.len() - 1;
        let formatted = format!("{:0width$o}", val, width = width);
        let bytes = formatted.as_bytes();
        let copy_len = bytes.len().min(width);
        dst[..copy_len].copy_from_slice(&bytes[..copy_len]);
        dst[width] = 0;
    }

    fn make_tar_header(name: &str, prefix: &str, size: u64, typeflag: u8) -> [u8; 512] {
        let mut hdr = [0u8; 512];
        let name_bytes = name.as_bytes();
        let nlen = name_bytes.len().min(100);
        hdr[..nlen].copy_from_slice(&name_bytes[..nlen]);
        write_octal(&mut hdr[100..108], if typeflag == b'5' { 0o755 } else { 0o644 });
        write_octal(&mut hdr[108..116], 0);
        write_octal(&mut hdr[116..124], 0);
        write_octal(&mut hdr[124..136], size);
        write_octal(&mut hdr[136..148], 1_700_000_000);
        hdr[148..156].fill(b' ');
        hdr[156] = typeflag;
        hdr[257..263].copy_from_slice(b"ustar\0");
        hdr[263..265].copy_from_slice(b"00");
        if !prefix.is_empty() {
            let p_bytes = prefix.as_bytes();
            let plen = p_bytes.len().min(155);
            hdr[345..345 + plen].copy_from_slice(&p_bytes[..plen]);
        }
        let sum: u64 = hdr.iter().map(|&b| b as u64).sum();
        let cksum_str = format!("{:06o}\0 ", sum);
        hdr[148..156].copy_from_slice(cksum_str.as_bytes());
        hdr
    }

    fn push_tar_entry(out: &mut Vec<u8>, name: &str, prefix: &str, typeflag: u8, data: &[u8]) {
        let hdr = make_tar_header(name, prefix, data.len() as u64, typeflag);
        out.extend_from_slice(&hdr);
        out.extend_from_slice(data);
        let rem = (512 - (data.len() % 512)) % 512;
        if rem > 0 {
            out.resize(out.len() + rem, 0);
        }
    }

    fn push_gnu_long_name_entry(out: &mut Vec<u8>, full_path: &str, typeflag: u8, data: &[u8]) {
        let mut long_payload = full_path.as_bytes().to_vec();
        long_payload.push(0);
        push_tar_entry(out, "././@LongLink", "", b'L', &long_payload);
        let short_name: String = full_path.chars().take(90).collect();
        push_tar_entry(out, &short_name, "", typeflag, data);
    }

    fn push_pax_long_name_entry(out: &mut Vec<u8>, full_path: &str, typeflag: u8, data: &[u8]) {
        let body = format!("path={full_path}\n");
        let mut total_len = body.len() + 2;
        loop {
            let candidate = format!("{total_len} {body}");
            if candidate.len() == total_len {
                push_tar_entry(
                    out,
                    "PaxHeaders.0/entry",
                    "",
                    b'x',
                    candidate.as_bytes(),
                );
                break;
            }
            total_len = candidate.len();
        }
        let short_name: String = full_path.chars().take(90).collect();
        push_tar_entry(out, &short_name, "", typeflag, data);
    }

    fn encode_7z_u64(v: u64, out: &mut Vec<u8>) {
        if v < 0x80 {
            out.push(v as u8);
            return;
        }
        let mut extra = 1u8;
        while extra < 8 {
            let cap = 1u64 << (7 - extra + 8 * extra);
            if v < cap {
                let first = (0xFFu8 << (8 - extra)) | ((v >> (8 * extra)) as u8);
                out.push(first);
                for i in 0..extra {
                    out.push(((v >> (8 * i)) & 0xFF) as u8);
                }
                return;
            }
            extra += 1;
        }
        out.push(0xFF);
        out.extend_from_slice(&v.to_le_bytes());
    }

    /// Synthesizes a valid 7z archive using COPY coder so `SevenZReader` tests run
    /// end-to-end without requiring `sevenz-rust/compress`.
    fn build_minimal_7z(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut packed_data = Vec::new();
        for (_, payload) in entries {
            packed_data.extend_from_slice(payload);
        }

        let mut header = Vec::new();
        header.push(0x01); // kHeader
        if !entries.is_empty() {
            header.push(0x04); // kMainStreamsInfo

            // kPackInfo
            header.push(0x06);
            encode_7z_u64(0, &mut header); // pack_pos
            encode_7z_u64(1, &mut header); // num_pack_streams
            header.push(0x09); // kSize
            encode_7z_u64(packed_data.len() as u64, &mut header);
            header.push(0x00); // kEnd

            // kUnpackInfo
            header.push(0x07);
            header.push(0x0B); // kFolder
            encode_7z_u64(1, &mut header); // num_folders
            header.push(0x00); // external = 0
            encode_7z_u64(1, &mut header); // num_coders
            header.push(0x01); // id_size=1, simple=1 in/1 out, no attributes
            header.push(0x00); // ID_COPY
            header.push(0x0C); // kCodersUnpackSize
            encode_7z_u64(packed_data.len() as u64, &mut header);
            header.push(0x00); // kEnd

            // kSubStreamsInfo
            header.push(0x08);
            header.push(0x0D); // kNumUnpackStream
            encode_7z_u64(entries.len() as u64, &mut header);
            if entries.len() > 1 {
                header.push(0x09); // kSize
                for (_, payload) in &entries[..entries.len() - 1] {
                    encode_7z_u64(payload.len() as u64, &mut header);
                }
            }
            header.push(0x00); // kEnd

            header.push(0x00); // kEnd StreamsInfo
        }

        // kFilesInfo
        header.push(0x05);
        encode_7z_u64(entries.len() as u64, &mut header);
        header.push(0x11); // kName
        let mut names_payload = vec![0x00u8]; // external = 0
        for (name, _) in entries {
            for code_unit in name.encode_utf16() {
                names_payload.extend_from_slice(&code_unit.to_le_bytes());
            }
            names_payload.extend_from_slice(&[0x00, 0x00]);
        }
        encode_7z_u64(names_payload.len() as u64, &mut header);
        header.extend_from_slice(&names_payload);
        header.push(0x00); // kEnd FilesInfo
        header.push(0x00); // kEnd Header

        let mut hdr_hasher = Crc32Hasher::new();
        hdr_hasher.update(&header);
        let next_header_crc = hdr_hasher.finalize();

        let mut start_info = Vec::with_capacity(20);
        start_info.extend_from_slice(&(packed_data.len() as u64).to_le_bytes());
        start_info.extend_from_slice(&(header.len() as u64).to_le_bytes());
        start_info.extend_from_slice(&next_header_crc.to_le_bytes());

        let mut start_hasher = Crc32Hasher::new();
        start_hasher.update(&start_info);
        let start_header_crc = start_hasher.finalize();

        let mut out = Vec::with_capacity(32 + packed_data.len() + header.len());
        out.extend_from_slice(&[b'7', b'z', 0xBC, 0xAF, 0x27, 0x1C]);
        out.extend_from_slice(&[0x00, 0x04]);
        out.extend_from_slice(&start_header_crc.to_le_bytes());
        out.extend_from_slice(&start_info);
        out.extend_from_slice(&packed_data);
        out.extend_from_slice(&header);
        out
    }

    #[test]
    fn test_sanitize_archive_relative_path_rejects_traversal_and_special_vectors() {
        let bad_cases = [
            "",
            "   ",
            "/",
            "\\",
            "/etc/passwd",
            "\\Windows\\System32\\cmd.exe",
            "//server/share/payload.dll",
            "\\\\server\\share\\payload.dll",
            "\\\\?\\C:\\evil.txt",
            "C:\\Windows\\evil.txt",
            "C:evil.txt",
            "readme.txt:Zone.Identifier",
            "../../outside.txt",
            "nested/../../outside.txt",
            "nested\\..\\..\\outside.txt",
            ".",
            "./",
            "./secret.txt",
            "a/./b.txt",
            "bad\0name.txt",
        ];
        for bad in bad_cases {
            assert!(
                sanitize_archive_relative_path(bad).is_err(),
                "Expected path '{bad}' to be rejected"
            );
        }

        assert_eq!(
            sanitize_archive_relative_path("bin/game.exe").expect("valid rel"),
            PathBuf::from("bin").join("game.exe")
        );
        assert_eq!(
            sanitize_archive_relative_path("assets\\textures/").expect("valid dir"),
            PathBuf::from("assets").join("textures")
        );
        assert_eq!(
            sanitize_archive_relative_path(".gitignore").expect("dotfile valid"),
            PathBuf::from(".gitignore")
        );
    }

    #[test]
    fn test_zip_deflate_and_zstd_roundtrip_with_monotonic_progress() {
        let tmp = TempDirGuard::new("zip_roundtrip");
        let zip_path = tmp.path.join("bundle.zip");
        let out_dir = tmp.path.join("extracted");

        let payload_a = vec![b'A'; 90_000];
        let payload_b = b"Zstd compressed nested payload inside ZIP!";
        let payload_empty: &[u8] = b"";

        {
            let file = File::create(&zip_path).expect("create zip");
            let mut zw = ZipWriter::new(file);
            let deflate_opts =
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            let zstd_opts =
                SimpleFileOptions::default().compression_method(CompressionMethod::Zstd);

            zw.add_directory("assets/shaders/", deflate_opts)
                .expect("add dir");
            zw.start_file("assets/big_deflate.bin", deflate_opts)
                .expect("start deflate");
            zw.write_all(&payload_a).expect("write deflate");

            zw.start_file("assets/shaders/main.frag", zstd_opts)
                .expect("start zstd");
            zw.write_all(payload_b).expect("write zstd");

            zw.start_file("empty.marker", deflate_opts)
                .expect("start empty");
            zw.write_all(payload_empty).expect("write empty");

            zw.finish().expect("finish zip");
        }

        let mut progress_events: Vec<(u64, u64, String)> = Vec::new();
        let report = extract_archive_with_progress(&zip_path, &out_dir, |cur, total, entry| {
            progress_events.push((cur, total, entry.to_string()));
        })
        .expect("extract zip");

        assert_eq!(report.format, "zip");
        assert_eq!(report.files_extracted, 3);
        assert_eq!(report.dirs_created, 2);
        assert_eq!(
            report.bytes_written,
            (payload_a.len() + payload_b.len()) as u64
        );

        assert_eq!(
            fs::read(out_dir.join("assets/big_deflate.bin")).expect("read a"),
            payload_a
        );
        assert_eq!(
            fs::read(out_dir.join("assets/shaders/main.frag")).expect("read b"),
            payload_b
        );
        assert_eq!(
            fs::read(out_dir.join("empty.marker")).expect("read empty"),
            payload_empty
        );

        assert!(!progress_events.is_empty());
        let mut prev_cur = 0u64;
        for (cur, total, _) in &progress_events {
            assert!(*cur >= prev_cur, "progress decreased: {cur} < {prev_cur}");
            assert!(*cur <= *total, "progress exceeded total: {cur} > {total}");
            prev_cur = *cur;
        }
        let last = progress_events.last().expect("final progress event");
        assert_eq!(last.0, report.bytes_written);
        assert_eq!(last.1, report.bytes_written);
    }

    #[test]
    fn test_tar_zst_and_tar_gz_roundtrip_with_long_paths() {
        let tmp = TempDirGuard::new("tar_long_paths");

        let long_dir = "deep_hierarchy/segment_alpha_0123456789/segment_beta_0123456789";
        let ustar_name = "ustar_file_entry.txt";
        let gnu_long_path = format!(
            "{}/gnu_long_link_{}/payload_gnu.bin",
            long_dir,
            "x".repeat(75)
        );
        let pax_long_path = format!(
            "{}/pax_extended_{}/payload_pax.txt",
            long_dir,
            "y".repeat(95)
        );
        assert!(gnu_long_path.len() > 100);
        assert!(pax_long_path.len() > 150);

        let mut tar_bytes = Vec::new();
        push_tar_entry(&mut tar_bytes, "explicit_dir", "", b'5', &[]);
        push_tar_entry(
            &mut tar_bytes,
            ustar_name,
            long_dir,
            b'0',
            b"ustar-prefix-content",
        );
        push_gnu_long_name_entry(
            &mut tar_bytes,
            &gnu_long_path,
            b'0',
            b"gnu-longlink-content-12345",
        );
        push_pax_long_name_entry(
            &mut tar_bytes,
            &pax_long_path,
            b'0',
            b"pax-extended-content-98765",
        );
        tar_bytes.resize(tar_bytes.len() + 1024, 0);

        // 1. .tar.zst (test magic detection even with opaque `.bin` suffix)
        let zst_path = tmp.path.join("archive_magic_probe.bin");
        {
            let f = File::create(&zst_path).expect("create zst");
            let mut enc = ZstdEncoder::new(f, 3).expect("init zstd");
            enc.write_all(&tar_bytes).expect("write zstd");
            enc.finish().expect("finish zstd");
        }
        let zst_out = tmp.path.join("out_zst");
        let zst_report = extract_archive_pure_rust(&zst_path, &zst_out).expect("extract tar.zst");
        assert_eq!(zst_report.format, "tar.zst");
        assert_eq!(zst_report.files_extracted, 3);
        assert!(zst_report.dirs_created >= 4);
        assert_eq!(
            fs::read(zst_out.join(long_dir).join(ustar_name)).expect("read ustar"),
            b"ustar-prefix-content"
        );
        assert_eq!(
            fs::read(zst_out.join(&gnu_long_path)).expect("read gnu"),
            b"gnu-longlink-content-12345"
        );
        assert_eq!(
            fs::read(zst_out.join(&pax_long_path)).expect("read pax"),
            b"pax-extended-content-98765"
        );

        // 2. .tar.gz
        let gz_path = tmp.path.join("bundle.tar.gz");
        {
            let f = File::create(&gz_path).expect("create gz");
            let mut enc = GzEncoder::new(f, Compression::default());
            enc.write_all(&tar_bytes).expect("write gz");
            enc.finish().expect("finish gz");
        }
        let gz_out = tmp.path.join("out_gz");
        let gz_report = extract_archive_pure_rust(&gz_path, &gz_out).expect("extract tar.gz");
        assert_eq!(gz_report.format, "tar.gz");
        assert_eq!(gz_report.files_extracted, 3);
        assert_eq!(gz_report.bytes_written, zst_report.bytes_written);
        assert_eq!(
            fs::read(gz_out.join(&pax_long_path)).expect("read pax gz"),
            b"pax-extended-content-98765"
        );

        // 3. Plain .tar
        let tar_path = tmp.path.join("plain.tar");
        fs::write(&tar_path, &tar_bytes).expect("write tar");
        let tar_out = tmp.path.join("out_tar");
        let tar_report = extract_archive_pure_rust(&tar_path, &tar_out).expect("extract tar");
        assert_eq!(tar_report.format, "tar");
        assert_eq!(tar_report.files_extracted, 3);
    }

    #[test]
    fn test_zip_slip_rejected_across_zip_tar_and_7z() {
        let tmp = TempDirGuard::new("zip_slip");
        let escaped_target = tmp.path.join("outside.txt");

        // 1. ZIP with `../../outside.txt`
        let evil_zip = tmp.path.join("evil_traversal.zip");
        {
            let f = File::create(&evil_zip).expect("create evil zip");
            let mut zw = ZipWriter::new(f);
            let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zw.start_file("../../outside.txt", opts).expect("start evil");
            zw.write_all(b"pwned").expect("write evil");
            zw.finish().expect("finish evil zip");
        }
        let dest_zip = tmp.path.join("nested").join("dest_zip");
        assert!(extract_archive_pure_rust(&evil_zip, &dest_zip).is_err());
        assert!(!escaped_target.exists());

        // 2. ZIP with `C:\Windows\evil.txt`
        let evil_drive_zip = tmp.path.join("evil_drive.zip");
        {
            let f = File::create(&evil_drive_zip).expect("create drive zip");
            let mut zw = ZipWriter::new(f);
            let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zw.start_file("C:\\Windows\\evil.txt", opts)
                .expect("start drive");
            zw.write_all(b"pwned").expect("write drive");
            zw.finish().expect("finish drive zip");
        }
        assert!(extract_archive_pure_rust(&evil_drive_zip, &dest_zip).is_err());

        // 3. TAR with `../../outside.txt` and TAR.GZ with `C:\Windows\evil.txt`
        let mut evil_tar = Vec::new();
        push_tar_entry(&mut evil_tar, "../../outside.txt", "", b'0', b"pwned");
        evil_tar.resize(evil_tar.len() + 1024, 0);
        let evil_tar_path = tmp.path.join("evil.tar");
        fs::write(&evil_tar_path, &evil_tar).expect("write evil tar");
        let dest_tar = tmp.path.join("nested").join("dest_tar");
        assert!(extract_archive_pure_rust(&evil_tar_path, &dest_tar).is_err());
        assert!(!escaped_target.exists());

        let mut evil_drive_tar = Vec::new();
        push_tar_entry(
            &mut evil_drive_tar,
            "C:\\Windows\\evil.txt",
            "",
            b'0',
            b"pwned",
        );
        evil_drive_tar.resize(evil_drive_tar.len() + 1024, 0);
        let evil_tgz_path = tmp.path.join("evil_drive.tar.gz");
        {
            let f = File::create(&evil_tgz_path).expect("create evil tgz");
            let mut enc = GzEncoder::new(f, Compression::default());
            enc.write_all(&evil_drive_tar).expect("write evil tgz");
            enc.finish().expect("finish evil tgz");
        }
        assert!(extract_archive_pure_rust(&evil_tgz_path, &dest_tar).is_err());

        // 4. 7z with `../../outside.txt`
        let evil_7z_bytes = build_minimal_7z(&[("../../outside.txt", b"pwned")]);
        let evil_7z_path = tmp.path.join("evil.7z");
        fs::write(&evil_7z_path, &evil_7z_bytes).expect("write evil 7z");
        let dest_7z = tmp.path.join("nested").join("dest_7z");
        assert!(extract_archive_pure_rust(&evil_7z_path, &dest_7z).is_err());
        assert!(!escaped_target.exists());
    }

    #[test]
    fn test_sevenz_roundtrip_and_corrupt_header_error() {
        let tmp = TempDirGuard::new("sevenz");
        let archive_path = tmp.path.join("game_patch.7z");
        let out_dir = tmp.path.join("out_7z");

        let file_a = b"Duke Launcher 7z extraction payload alpha";
        let file_b = b"Nested binary config data \x00\x01\x02\x03";
        let archive_bytes = build_minimal_7z(&[
            ("readme.txt", file_a),
            ("config/settings.bin", file_b),
        ]);
        fs::write(&archive_path, &archive_bytes).expect("write 7z");

        let report = extract_archive_pure_rust(&archive_path, &out_dir).expect("extract 7z");
        assert_eq!(report.format, "7z");
        assert_eq!(report.files_extracted, 2);
        assert_eq!(report.dirs_created, 1);
        assert_eq!(report.bytes_written, (file_a.len() + file_b.len()) as u64);
        assert_eq!(fs::read(out_dir.join("readme.txt")).expect("read a"), file_a);
        assert_eq!(
            fs::read(out_dir.join("config/settings.bin")).expect("read b"),
            file_b
        );

        let corrupt_7z = tmp.path.join("corrupt.7z");
        fs::write(&corrupt_7z, b"7z\xBC\xAF\x27\x1Ccorrupt-header-data").expect("write corrupt 7z");
        let corrupt_out = tmp.path.join("out_corrupt");
        let err = extract_archive_pure_rust(&corrupt_7z, &corrupt_out)
            .expect_err("corrupt 7z must fail cleanly");
        assert!(!err.is_empty());

        let unknown_bin = tmp.path.join("mystery.bin");
        fs::write(&unknown_bin, b"not an archive at all").expect("write mystery");
        assert!(extract_archive_pure_rust(&unknown_bin, &corrupt_out).is_err());
    }
}

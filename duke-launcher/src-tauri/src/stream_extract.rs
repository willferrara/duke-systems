use flate2::read::DeflateDecoder;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

const BOUNDED_RING_SLOTS: usize = 32;
const RING_CHUNK_SIZE: usize = 128 * 1024;
const MAX_OOO_BUFFER_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StreamArchiveFormat {
    None,
    TarZst,
    ZstSingleFile,
    Zip,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InFlightExtractStats {
    pub format: String,
    pub files_extracted: usize,
    pub extracted_bytes: u64,
    pub compressed_bytes_streamed: u64,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone)]
pub struct ZipRemoteEntry {
    pub rel_path: String,
    pub compression_method: u16,
    pub expected_crc32: u32,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    pub local_header_offset: u64,
    pub range_end_exclusive: u64,
    pub is_dir: bool,
}

#[derive(Debug, Clone)]
pub struct ZipRemotePlan {
    pub entries: Vec<ZipRemoteEntry>,
}

pub fn detect_stream_archive_format(
    file_name: &str,
    content_type: Option<&str>,
    magic_prefix: Option<&[u8]>,
) -> StreamArchiveFormat {
    let lower_name = file_name.trim().to_ascii_lowercase();
    let lower_ct = content_type
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();

    if let Some(bytes) = magic_prefix {
        if bytes.len() >= 4 && bytes[0..4] == [0x50, 0x4B, 0x03, 0x04] {
            return StreamArchiveFormat::Zip;
        }
        if bytes.len() >= 4 && bytes[0..4] == [0x28, 0xB5, 0x2F, 0xFD] {
            if lower_name.ends_with(".tar.zst") || lower_name.ends_with(".tzst") {
                return StreamArchiveFormat::TarZst;
            }
            return StreamArchiveFormat::ZstSingleFile;
        }
    }

    if lower_name.ends_with(".tar.zst") || lower_name.ends_with(".tzst") {
        return StreamArchiveFormat::TarZst;
    }
    if lower_name.ends_with(".zst") || lower_ct.contains("application/zstd") {
        return StreamArchiveFormat::ZstSingleFile;
    }
    if lower_name.ends_with(".zip")
        || lower_ct.contains("application/zip")
        || lower_ct.contains("application/x-zip")
    {
        return StreamArchiveFormat::Zip;
    }

    StreamArchiveFormat::None
}

fn sanitize_archive_rel_path(dest_root: &Path, entry_name: &str) -> Result<Option<PathBuf>, String> {
    let cleaned = entry_name.replace('\\', "/");
    let trimmed = cleaned.trim_start_matches("./").trim_matches('/');
    if trimmed.is_empty() {
        return Ok(None);
    }
    let rel = Path::new(trimmed);
    let mut out = dest_root.to_path_buf();
    for comp in rel.components() {
        match comp {
            Component::Normal(seg) => {
                let s = seg.to_string_lossy();
                if s.contains('\0') || s.contains(':') {
                    return Err(format!("Unsafe archive entry path: {entry_name}"));
                }
                out.push(seg);
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!("Zip-Slip blocked for entry: {entry_name}"));
            }
        }
    }
    Ok(Some(out))
}

struct StreamingCrc32 {
    state: u32,
}

impl StreamingCrc32 {
    fn new() -> Self {
        Self { state: 0xFFFF_FFFF }
    }

    fn update(&mut self, data: &[u8]) {
        for &b in data {
            self.state ^= b as u32;
            for _ in 0..8 {
                let mask = (self.state & 1).wrapping_neg() & 0xEDB8_8320;
                self.state = (self.state >> 1) ^ mask;
            }
        }
    }

    fn finalize(self) -> u32 {
        !self.state
    }
}

struct ChannelReader {
    rx: mpsc::Receiver<Vec<u8>>,
    cur: Vec<u8>,
    pos: usize,
}

impl ChannelReader {
    fn new(rx: mpsc::Receiver<Vec<u8>>) -> Self {
        Self {
            rx,
            cur: Vec::new(),
            pos: 0,
        }
    }
}

impl Read for ChannelReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        while self.pos >= self.cur.len() {
            match self.rx.recv() {
                Ok(buf) => {
                    if !buf.is_empty() {
                        self.cur = buf;
                        self.pos = 0;
                    }
                }
                Err(_) => return Ok(0),
            }
        }
        let avail = &self.cur[self.pos..];
        let to_copy = avail.len().min(out.len());
        out[..to_copy].copy_from_slice(&avail[..to_copy]);
        self.pos += to_copy;
        Ok(to_copy)
    }
}

fn parse_tar_octal(field: &[u8]) -> u64 {
    let trimmed = std::str::from_utf8(field)
        .unwrap_or("")
        .trim_matches(|c: char| c == '\0' || c == ' ');
    if trimmed.is_empty() {
        return 0;
    }
    u64::from_str_radix(trimmed, 8).unwrap_or(0)
}

fn looks_like_tar_header(hdr: &[u8; 512]) -> bool {
    if hdr.iter().all(|&b| b == 0) {
        return false;
    }
    let expected = parse_tar_octal(&hdr[148..156]);
    if expected == 0 {
        return false;
    }
    let mut sum: u64 = 0;
    for (idx, &b) in hdr.iter().enumerate() {
        if (148..156).contains(&idx) {
            sum += b' ' as u64;
        } else {
            sum += b as u64;
        }
    }
    sum == expected
}

fn read_exact_or_eof<R: Read>(reader: &mut R, buf: &mut [u8]) -> Result<bool, String> {
    let mut filled = 0usize;
    while filled < buf.len() {
        let n = reader
            .read(&mut buf[filled..])
            .map_err(|e| format!("Stream read failed: {}", e))?;
        if n == 0 {
            if filled == 0 {
                return Ok(false);
            }
            return Err("Unexpected EOF mid-record in tar.zst stream".to_string());
        }
        filled += n;
    }
    Ok(true)
}

fn parse_pax_path(pax_bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(pax_bytes).ok()?;
    for line in text.lines() {
        if let Some((_, kv)) = line.split_once(' ') {
            if let Some(val) = kv.strip_prefix("path=") {
                return Some(val.to_string());
            }
        }
    }
    None
}

fn extract_tar_stream<R: Read>(
    reader: &mut R,
    first_block: Option<[u8; 512]>,
    dest_dir: &Path,
) -> Result<(usize, u64), String> {
    fs::create_dir_all(dest_dir)
        .map_err(|e| format!("Failed creating target dir {}: {}", dest_dir.display(), e))?;

    let mut files_extracted = 0usize;
    let mut extracted_bytes = 0u64;
    let mut pending_first = first_block;
    let mut gnu_long_name: Option<String> = None;
    let mut pax_next_path: Option<String> = None;
    let mut copy_buf = vec![0u8; 64 * 1024];

    loop {
        let mut hdr = [0u8; 512];
        if let Some(first) = pending_first.take() {
            hdr = first;
        } else if !read_exact_or_eof(reader, &mut hdr)? {
            break;
        }

        if hdr.iter().all(|&b| b == 0) {
            break;
        }

        if !looks_like_tar_header(&hdr) {
            return Err("Invalid tar checksum in streaming archive".to_string());
        }

        let size = parse_tar_octal(&hdr[124..136]);
        let typeflag = hdr[156];
        let raw_name = std::str::from_utf8(&hdr[..100])
            .unwrap_or("")
            .trim_end_matches('\0');
        let raw_prefix = std::str::from_utf8(&hdr[345..500])
            .unwrap_or("")
            .trim_end_matches('\0');
        let fallback_name = if raw_prefix.is_empty() {
            raw_name.to_string()
        } else {
            format!("{}/{}", raw_prefix, raw_name)
        };

        let entry_name = pax_next_path
            .take()
            .or_else(|| gnu_long_name.take())
            .unwrap_or(fallback_name);

        let padded_rem = ((512 - (size % 512)) % 512) as usize;

        match typeflag {
            b'L' => {
                let mut name_bytes = vec![0u8; size as usize];
                read_exact_or_eof(reader, &mut name_bytes)?;
                gnu_long_name = Some(
                    String::from_utf8_lossy(&name_bytes)
                        .trim_end_matches('\0')
                        .to_string(),
                );
            }
            b'x' => {
                let mut pax_bytes = vec![0u8; size as usize];
                read_exact_or_eof(reader, &mut pax_bytes)?;
                pax_next_path = parse_pax_path(&pax_bytes);
            }
            b'g' => {
                io::copy(&mut reader.take(size), &mut io::sink())
                    .map_err(|e| format!("Failed skipping PAX global header: {}", e))?;
            }
            b'5' => {
                if let Some(target_dir) = sanitize_archive_rel_path(dest_dir, &entry_name)? {
                    fs::create_dir_all(&target_dir).map_err(|e| {
                        format!(
                            "Failed creating tar dir {}: {}",
                            target_dir.display(),
                            e
                        )
                    })?;
                }
                io::copy(&mut reader.take(size), &mut io::sink())
                    .map_err(|e| format!("Failed skipping tar dir payload: {}", e))?;
            }
            b'0' | 0 => {
                if let Some(target_file) = sanitize_archive_rel_path(dest_dir, &entry_name)? {
                    if let Some(parent) = target_file.parent() {
                        fs::create_dir_all(parent).map_err(|e| {
                            format!("Failed creating parent dir {}: {}", parent.display(), e)
                        })?;
                    }
                    let out_f = File::create(&target_file).map_err(|e| {
                        format!(
                            "Failed creating extracted file {}: {}",
                            target_file.display(),
                            e
                        )
                    })?;
                    let mut writer = BufWriter::with_capacity(64 * 1024, out_f);
                    let mut left = size;
                    while left > 0 {
                        let want = (left as usize).min(copy_buf.len());
                        if !read_exact_or_eof(reader, &mut copy_buf[..want])? {
                            return Err("Truncated file entry in tar stream".to_string());
                        }
                        writer
                            .write_all(&copy_buf[..want])
                            .map_err(|e| format!("Failed writing extracted file: {}", e))?;
                        left -= want as u64;
                        extracted_bytes += want as u64;
                    }
                    writer
                        .flush()
                        .map_err(|e| format!("Failed flushing extracted file: {}", e))?;
                    files_extracted += 1;
                } else {
                    io::copy(&mut reader.take(size), &mut io::sink())
                        .map_err(|e| format!("Failed draining anonymous tar entry: {}", e))?;
                }
            }
            _ => {
                io::copy(&mut reader.take(size), &mut io::sink())
                    .map_err(|e| format!("Failed draining unsupported tar entry: {}", e))?;
            }
        }

        if padded_rem > 0 {
            let mut pad = [0u8; 512];
            read_exact_or_eof(reader, &mut pad[..padded_rem])?;
        }
    }

    Ok((files_extracted, extracted_bytes))
}

struct ContiguousReorderState {
    next_offset: u64,
    pending: BTreeMap<u64, Vec<u8>>,
    buffered_bytes: usize,
    total_streamed: u64,
}

/// Bounded ring-channel pipeline feeding contiguous byte ranges straight into a streaming `zstd` decoder.
pub struct InFlightZstPipeline {
    sender: Option<SyncSender<Vec<u8>>>,
    reorder: Mutex<ContiguousReorderState>,
    handle: Option<JoinHandle<Result<InFlightExtractStats, String>>>,
    failed: Arc<Mutex<Option<String>>>,
}

impl InFlightZstPipeline {
    pub fn start(
        format: StreamArchiveFormat,
        dest_dir: PathBuf,
        single_file_target: PathBuf,
    ) -> Self {
        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(BOUNDED_RING_SLOTS);
        let failed = Arc::new(Mutex::new(None::<String>));
        let failed_clone = Arc::clone(&failed);

        let handle = std::thread::Builder::new()
            .name("duke-zstd-inflight".to_string())
            .spawn(move || {
                let started = Instant::now();
                let res = (|| -> Result<InFlightExtractStats, String> {
                    let ring_reader = ChannelReader::new(rx);
                    let mut decoder = zstd::stream::read::Decoder::new(ring_reader)
                        .map_err(|e| format!("Failed initializing zstd decoder: {}", e))?;

                    let is_tar = match format {
                        StreamArchiveFormat::TarZst => true,
                        StreamArchiveFormat::ZstSingleFile => {
                            let mut probe = [0u8; 512];
                            let mut read_len = 0usize;
                            while read_len < probe.len() {
                                match decoder.read(&mut probe[read_len..]) {
                                    Ok(0) => break,
                                    Ok(n) => read_len += n,
                                    Err(e) => {
                                        return Err(format!("Zstd decompression error: {}", e));
                                    }
                                }
                            }
                            if read_len == 512 && looks_like_tar_header(&probe) {
                                let (files_extracted, extracted_bytes) =
                                    extract_tar_stream(&mut decoder, Some(probe), &dest_dir)?;
                                return Ok(InFlightExtractStats {
                                    format: "tar.zst".to_string(),
                                    files_extracted,
                                    extracted_bytes,
                                    compressed_bytes_streamed: 0,
                                    elapsed_ms: started.elapsed().as_millis() as u64,
                                });
                            }
                            if let Some(parent) = single_file_target.parent() {
                                fs::create_dir_all(parent).map_err(|e| {
                                    format!("Failed creating output dir {}: {}", parent.display(), e)
                                })?;
                            }
                            let out_f = File::create(&single_file_target).map_err(|e| {
                                format!(
                                    "Failed creating .zst output {}: {}",
                                    single_file_target.display(),
                                    e
                                )
                            })?;
                            let mut writer = BufWriter::with_capacity(64 * 1024, out_f);
                            writer
                                .write_all(&probe[..read_len])
                                .map_err(|e| format!("Failed writing .zst prefix: {}", e))?;
                            let mut copied = read_len as u64;
                            let mut buf = vec![0u8; 64 * 1024];
                            loop {
                                let n = decoder
                                    .read(&mut buf)
                                    .map_err(|e| format!("Zstd stream read failed: {}", e))?;
                                if n == 0 {
                                    break;
                                }
                                writer
                                    .write_all(&buf[..n])
                                    .map_err(|e| format!("Zstd output write failed: {}", e))?;
                                copied += n as u64;
                            }
                            writer
                                .flush()
                                .map_err(|e| format!("Zstd output flush failed: {}", e))?;
                            return Ok(InFlightExtractStats {
                                format: "zst".to_string(),
                                files_extracted: 1,
                                extracted_bytes: copied,
                                compressed_bytes_streamed: 0,
                                elapsed_ms: started.elapsed().as_millis() as u64,
                            });
                        }
                        _ => false,
                    };

                    if is_tar {
                        let (files_extracted, extracted_bytes) =
                            extract_tar_stream(&mut decoder, None, &dest_dir)?;
                        Ok(InFlightExtractStats {
                            format: "tar.zst".to_string(),
                            files_extracted,
                            extracted_bytes,
                            compressed_bytes_streamed: 0,
                            elapsed_ms: started.elapsed().as_millis() as u64,
                        })
                    } else {
                        Err("Unsupported format passed to zstd pipeline".to_string())
                    }
                })();

                if let Err(ref err) = res {
                    *failed_clone.lock().unwrap() = Some(err.clone());
                }
                res
            })
            .expect("spawn in-flight zstd decoder thread");

        Self {
            sender: Some(tx),
            reorder: Mutex::new(ContiguousReorderState {
                next_offset: 0,
                pending: BTreeMap::new(),
                buffered_bytes: 0,
                total_streamed: 0,
            }),
            handle: Some(handle),
            failed,
        }
    }

    pub fn next_contiguous_offset(&self) -> u64 {
        self.reorder.lock().unwrap().next_offset
    }

    pub fn submit_range(&self, start_offset: u64, data: &[u8]) -> Result<(), String> {
        if let Some(err) = self.failed.lock().unwrap().clone() {
            return Err(err);
        }
        if data.is_empty() {
            return Ok(());
        }
        let sender = self
            .sender
            .as_ref()
            .ok_or_else(|| "Streaming pipeline already closed".to_string())?
            .clone();

        let mut ready_to_send = Vec::<Vec<u8>>::new();
        {
            let mut state = self.reorder.lock().unwrap();
            let end_offset = start_offset + data.len() as u64;
            if end_offset <= state.next_offset {
                return Ok(());
            }

            let slice = if start_offset < state.next_offset {
                let trim = (state.next_offset - start_offset) as usize;
                data[trim..].to_vec()
            } else {
                data.to_vec()
            };
            let insert_off = start_offset.max(state.next_offset);
            let slice_len = slice.len();

            if insert_off != state.next_offset
                && state.buffered_bytes.saturating_add(slice_len) > MAX_OOO_BUFFER_BYTES
            {
                return Err("Out-of-order streaming buffer budget exceeded; falling back to disk staging".to_string());
            }

            state.buffered_bytes += slice_len;
            state.pending.insert(insert_off, slice);

            loop {
                let next = state.next_offset;
                let Some(block) = state.pending.remove(&next) else {
                    break;
                };
                state.buffered_bytes = state.buffered_bytes.saturating_sub(block.len());
                state.next_offset += block.len() as u64;
                state.total_streamed += block.len() as u64;
                for chunk in block.chunks(RING_CHUNK_SIZE) {
                    ready_to_send.push(chunk.to_vec());
                }
            }
        }

        for chunk in ready_to_send {
            sender
                .send(chunk)
                .map_err(|_| "In-flight zstd decoder exited prematurely".to_string())?;
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<InFlightExtractStats, String> {
        let remaining_unflushed = !self.reorder.lock().unwrap().pending.is_empty();
        drop(self.sender.take());
        let handle = self
            .handle
            .take()
            .ok_or_else(|| "Missing decoder thread handle".to_string())?;
        let mut stats = handle
            .join()
            .map_err(|_| "In-flight decoder thread panicked".to_string())??;
        if remaining_unflushed {
            return Err("Archive stream ended with unjoined byte gaps".to_string());
        }
        stats.compressed_bytes_streamed = self.reorder.lock().unwrap().total_streamed;
        Ok(stats)
    }
}

/// Parses EOCD + Central Directory from tail buffer of a remote `.zip` archive so every entry's byte span is known up front.
pub fn parse_zip_central_directory_from_tail(
    tail_bytes: &[u8],
    tail_start_offset: u64,
    total_size: u64,
) -> Result<Option<(u64, u64, Vec<u8>)>, String> {
    if tail_bytes.len() < 22 {
        return Ok(None);
    }

    let min_pos = tail_bytes.len().saturating_sub(65_557);
    let mut eocd_idx = None;
    let mut idx = tail_bytes.len() - 22;
    loop {
        if tail_bytes[idx..idx + 4] == [0x50, 0x4B, 0x05, 0x06] {
            let comment_len = u16::from_le_bytes([tail_bytes[idx + 20], tail_bytes[idx + 21]]) as usize;
            if idx + 22 + comment_len == tail_bytes.len() {
                eocd_idx = Some(idx);
                break;
            }
        }
        if idx == min_pos {
            break;
        }
        idx -= 1;
    }

    let Some(eocd_pos) = eocd_idx else {
        return Ok(None);
    };

    let mut cd_size = u32::from_le_bytes([
        tail_bytes[eocd_pos + 12],
        tail_bytes[eocd_pos + 13],
        tail_bytes[eocd_pos + 14],
        tail_bytes[eocd_pos + 15],
    ]) as u64;
    let mut cd_offset = u32::from_le_bytes([
        tail_bytes[eocd_pos + 16],
        tail_bytes[eocd_pos + 17],
        tail_bytes[eocd_pos + 18],
        tail_bytes[eocd_pos + 19],
    ]) as u64;

    if (cd_offset == 0xFFFF_FFFF || cd_size == 0xFFFF_FFFF) && eocd_pos >= 20 {
        let loc = eocd_pos - 20;
        if tail_bytes[loc..loc + 4] == [0x50, 0x4B, 0x06, 0x07] {
            let zip64_eocd_off = u64::from_le_bytes(tail_bytes[loc + 8..loc + 16].try_into().unwrap());
            if zip64_eocd_off >= tail_start_offset {
                let rel = (zip64_eocd_off - tail_start_offset) as usize;
                if rel + 56 <= tail_bytes.len() && tail_bytes[rel..rel + 4] == [0x50, 0x4B, 0x06, 0x06] {
                    cd_size = u64::from_le_bytes(tail_bytes[rel + 40..rel + 48].try_into().unwrap());
                    cd_offset = u64::from_le_bytes(tail_bytes[rel + 48..rel + 56].try_into().unwrap());
                }
            }
        }
    }

    if cd_offset.saturating_add(cd_size) > total_size {
        return Err("Invalid ZIP Central Directory offsets in EOCD".to_string());
    }

    let inline_cd = if cd_offset >= tail_start_offset {
        let rel_start = (cd_offset - tail_start_offset) as usize;
        let rel_end = rel_start.saturating_add(cd_size as usize);
        if rel_end <= tail_bytes.len() {
            tail_bytes[rel_start..rel_end].to_vec()
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    Ok(Some((cd_offset, cd_size, inline_cd)))
}

pub fn parse_zip_central_directory_entries(cd_bytes: &[u8], cd_offset: u64) -> Result<ZipRemotePlan, String> {
    let mut entries = Vec::new();
    let mut cursor = 0usize;
    while cursor + 46 <= cd_bytes.len() {
        if cd_bytes[cursor..cursor + 4] != [0x50, 0x4B, 0x01, 0x02] {
            break;
        }
        let method = u16::from_le_bytes([cd_bytes[cursor + 10], cd_bytes[cursor + 11]]);
        let crc32 = u32::from_le_bytes(cd_bytes[cursor + 16..cursor + 20].try_into().unwrap());
        let mut comp_size =
            u32::from_le_bytes(cd_bytes[cursor + 20..cursor + 24].try_into().unwrap()) as u64;
        let mut uncomp_size =
            u32::from_le_bytes(cd_bytes[cursor + 24..cursor + 28].try_into().unwrap()) as u64;
        let fname_len = u16::from_le_bytes([cd_bytes[cursor + 28], cd_bytes[cursor + 29]]) as usize;
        let extra_len = u16::from_le_bytes([cd_bytes[cursor + 30], cd_bytes[cursor + 31]]) as usize;
        let comment_len =
            u16::from_le_bytes([cd_bytes[cursor + 32], cd_bytes[cursor + 33]]) as usize;
        let mut local_off =
            u32::from_le_bytes(cd_bytes[cursor + 42..cursor + 46].try_into().unwrap()) as u64;

        let rec_end = cursor + 46 + fname_len + extra_len + comment_len;
        if rec_end > cd_bytes.len() {
            return Err("Truncated ZIP Central Directory entry".to_string());
        }
        let name = String::from_utf8_lossy(&cd_bytes[cursor + 46..cursor + 46 + fname_len])
            .into_owned();

        // Parse Zip64 extended information field (tag 0x0001) when 32-bit fields use 0xFFFF_FFFF.
        let mut ex = cursor + 46 + fname_len;
        let ex_end = ex + extra_len;
        while ex + 4 <= ex_end {
            let tag = u16::from_le_bytes([cd_bytes[ex], cd_bytes[ex + 1]]);
            let sz = u16::from_le_bytes([cd_bytes[ex + 2], cd_bytes[ex + 3]]) as usize;
            let val_start = ex + 4;
            let val_end = (val_start + sz).min(ex_end);
            if tag == 0x0001 {
                let mut vp = val_start;
                if uncomp_size == 0xFFFF_FFFF && vp + 8 <= val_end {
                    uncomp_size = u64::from_le_bytes(cd_bytes[vp..vp + 8].try_into().unwrap());
                    vp += 8;
                }
                if comp_size == 0xFFFF_FFFF && vp + 8 <= val_end {
                    comp_size = u64::from_le_bytes(cd_bytes[vp..vp + 8].try_into().unwrap());
                    vp += 8;
                }
                if local_off == 0xFFFF_FFFF && vp + 8 <= val_end {
                    local_off = u64::from_le_bytes(cd_bytes[vp..vp + 8].try_into().unwrap());
                }
            }
            ex = ex + 4 + sz;
        }

        let is_dir = name.ends_with('/') || name.ends_with('\\');
        entries.push(ZipRemoteEntry {
            rel_path: name,
            compression_method: method,
            expected_crc32: crc32,
            compressed_size: comp_size,
            uncompressed_size: uncomp_size,
            local_header_offset: local_off,
            range_end_exclusive: local_off + 30 + comp_size + 256,
            is_dir,
        });
        cursor = rec_end;
    }

    // Tighten range_end_exclusive using adjacent local_header_offsets bounded by cd_offset.
    let mut offsets: Vec<u64> = entries.iter().map(|e| e.local_header_offset).collect();
    offsets.push(cd_offset);
    offsets.sort_unstable();
    offsets.dedup();
    for entry in &mut entries {
        if let Some(pos) = offsets.iter().position(|&o| o == entry.local_header_offset) {
            if let Some(&next_off) = offsets.get(pos + 1) {
                entry.range_end_exclusive = next_off;
            }
        }
    }

    Ok(ZipRemotePlan { entries })
}

pub async fn prefetch_zip_remote_plan(
    client: &reqwest::Client,
    url: &str,
    total_size: u64,
) -> Option<ZipRemotePlan> {
    if total_size < 22 {
        return None;
    }
    let tail_window = 65_557u64.min(total_size);
    let tail_start = total_size - tail_window;
    let tail_range = format!("bytes={}-{}", tail_start, total_size - 1);
    let resp = client
        .get(url)
        .header(reqwest::header::RANGE, tail_range)
        .send()
        .await
        .ok()?;
    if resp.status() != reqwest::StatusCode::PARTIAL_CONTENT && !resp.status().is_success() {
        return None;
    }
    let tail_bytes = resp.bytes().await.ok()?;
    let (cd_offset, cd_size, inline_cd) =
        parse_zip_central_directory_from_tail(&tail_bytes, tail_start, total_size).ok()??;

    let cd_bytes = if !inline_cd.is_empty() || cd_size == 0 {
        inline_cd
    } else if cd_size <= 64 * 1024 * 1024 {
        let cd_range = format!("bytes={}-{}", cd_offset, cd_offset + cd_size - 1);
        let cd_resp = client
            .get(url)
            .header(reqwest::header::RANGE, cd_range)
            .send()
            .await
            .ok()?;
        if !cd_resp.status().is_success() {
            return None;
        }
        cd_resp.bytes().await.ok()?.to_vec()
    } else {
        return None;
    };

    parse_zip_central_directory_entries(&cd_bytes, cd_offset).ok()
}

pub fn extract_ready_zip_entry_from_file(
    archive_file: &Path,
    entry: &ZipRemoteEntry,
    dest_dir: &Path,
) -> Result<u64, String> {
    if entry.is_dir {
        if let Some(dir_path) = sanitize_archive_rel_path(dest_dir, &entry.rel_path)? {
            fs::create_dir_all(&dir_path)
                .map_err(|e| format!("Failed creating ZIP dir {}: {}", dir_path.display(), e))?;
        }
        return Ok(0);
    }

    let Some(target_file) = sanitize_archive_rel_path(dest_dir, &entry.rel_path)? else {
        return Ok(0);
    };
    if let Some(parent) = target_file.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed creating ZIP parent {}: {}", parent.display(), e))?;
    }

    let mut file = File::open(archive_file)
        .map_err(|e| format!("Failed opening staged ZIP {}: {}", archive_file.display(), e))?;
    file.seek(SeekFrom::Start(entry.local_header_offset))
        .map_err(|e| format!("Failed seeking ZIP local header: {}", e))?;

    let mut lh = [0u8; 30];
    file.read_exact(&mut lh)
        .map_err(|e| format!("Failed reading ZIP local header: {}", e))?;
    if lh[0..4] != [0x50, 0x4B, 0x03, 0x04] {
        return Err(format!("Invalid ZIP local header for '{}'", entry.rel_path));
    }
    let fn_len = u16::from_le_bytes([lh[26], lh[27]]) as u64;
    let ex_len = u16::from_le_bytes([lh[28], lh[29]]) as u64;
    let payload_offset = entry.local_header_offset + 30 + fn_len + ex_len;
    file.seek(SeekFrom::Start(payload_offset))
        .map_err(|e| format!("Failed seeking ZIP payload: {}", e))?;

    let out_f = File::create(&target_file)
        .map_err(|e| format!("Failed creating ZIP output {}: {}", target_file.display(), e))?;
    let mut writer = BufWriter::with_capacity(64 * 1024, out_f);
    let mut hasher = StreamingCrc32::new();
    let mut written = 0u64;
    let mut buf = vec![0u8; 64 * 1024];

    match entry.compression_method {
        0 => {
            let mut bounded = file.take(entry.compressed_size);
            loop {
                let n = bounded
                    .read(&mut buf)
                    .map_err(|e| format!("Failed reading stored ZIP entry: {}", e))?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
                writer
                    .write_all(&buf[..n])
                    .map_err(|e| format!("Failed writing stored ZIP entry: {}", e))?;
                written += n as u64;
            }
        }
        8 => {
            let bounded = file.take(entry.compressed_size);
            let mut decoder = DeflateDecoder::new(bounded);
            loop {
                let n = decoder
                    .read(&mut buf)
                    .map_err(|e| format!("DEFLATE error in '{}': {}", entry.rel_path, e))?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
                writer
                    .write_all(&buf[..n])
                    .map_err(|e| format!("Failed writing deflated ZIP entry: {}", e))?;
                written += n as u64;
            }
        }
        other => {
            return Err(format!(
                "Unsupported ZIP compression method {} for '{}'",
                other, entry.rel_path
            ));
        }
    }

    writer
        .flush()
        .map_err(|e| format!("Failed flushing ZIP output {}: {}", target_file.display(), e))?;
    if written != entry.uncompressed_size {
        return Err(format!(
            "Uncompressed size mismatch for '{}': got {} expected {}",
            entry.rel_path, written, entry.uncompressed_size
        ));
    }
    let actual_crc = hasher.finalize();
    if actual_crc != entry.expected_crc32 {
        return Err(format!(
            "CRC32 mismatch for '{}': got {:08x} expected {:08x}",
            entry.rel_path, actual_crc, entry.expected_crc32
        ));
    }
    Ok(written)
}

/// Checks if any queued ZIP entries now have all required bytes finished within completed contiguous segments.
pub fn dispatch_ready_zip_entries(
    archive_file: &Path,
    dest_dir: &Path,
    plan: &ZipRemotePlan,
    completed_ranges: &[(u64, u64)],
    extracted_set: &mut HashSet<String>,
) -> Result<(usize, u64), String> {
    let mut ready = Vec::new();
    for entry in &plan.entries {
        if extracted_set.contains(&entry.rel_path) {
            continue;
        }
        let covered = completed_ranges.iter().any(|&(s, e)| {
            s <= entry.local_header_offset && e >= entry.range_end_exclusive
        });
        if covered {
            ready.push(entry.clone());
        }
    }
    if ready.is_empty() {
        return Ok((0, 0));
    }
    for e in &ready {
        extracted_set.insert(e.rel_path.clone());
    }

    let count = Arc::new(Mutex::new(0usize));
    let bytes = Arc::new(Mutex::new(0u64));
    let queue = Arc::new(Mutex::new(ready));
    let err_slot = Arc::new(Mutex::new(None::<String>));
    let workers = std::thread::available_parallelism()
        .map(|n| n.get().clamp(2, 6))
        .unwrap_or(4);

    std::thread::scope(|scope| {
        for _ in 0..workers {
            let queue = Arc::clone(&queue);
            let count = Arc::clone(&count);
            let bytes = Arc::clone(&bytes);
            let err_slot = Arc::clone(&err_slot);
            scope.spawn(move || loop {
                if err_slot.lock().unwrap().is_some() {
                    break;
                }
                let next = { queue.lock().unwrap().pop() };
                let Some(entry) = next else {
                    break;
                };
                match extract_ready_zip_entry_from_file(archive_file, &entry, dest_dir) {
                    Ok(sz) => {
                        *count.lock().unwrap() += 1;
                        *bytes.lock().unwrap() += sz;
                    }
                    Err(e) => {
                        *err_slot.lock().unwrap() = Some(e);
                        break;
                    }
                }
            });
        }
    });

    if let Some(err) = err_slot.lock().unwrap().take() {
        return Err(err);
    }
    let out_count = *count.lock().unwrap();
    let out_bytes = *bytes.lock().unwrap();
    Ok((out_count, out_bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::DeflateEncoder;
    use flate2::Compression;

    fn append_tar_entry(tar_out: &mut Vec<u8>, name: &str, data: &[u8]) {
        let mut hdr = [0u8; 512];
        let name_bytes = name.as_bytes();
        hdr[..name_bytes.len()].copy_from_slice(name_bytes);
        hdr[100..108].copy_from_slice(b"0000644\0");
        hdr[108..116].copy_from_slice(b"0000000\0");
        hdr[116..124].copy_from_slice(b"0000000\0");
        let sz_oct = format!("{:011o}\0", data.len());
        hdr[124..136].copy_from_slice(sz_oct.as_bytes());
        hdr[136..148].copy_from_slice(b"00000000000\0");
        hdr[148..156].copy_from_slice(b"        ");
        hdr[156] = b'0';
        hdr[257..263].copy_from_slice(b"ustar\0");
        hdr[263..265].copy_from_slice(b"00");
        let sum: u64 = hdr.iter().map(|&b| b as u64).sum();
        let chk = format!("{:06o}\0 ", sum);
        hdr[148..156].copy_from_slice(chk.as_bytes());

        tar_out.extend_from_slice(&hdr);
        tar_out.extend_from_slice(data);
        let pad = (512 - (data.len() % 512)) % 512;
        tar_out.resize(tar_out.len() + pad, 0);
    }

    #[test]
    fn tar_zst_ring_channel_extracts_out_of_order_chunks_in_flight() {
        let dir = std::env::temp_dir().join(format!("duke_tarzst_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);

        let mut tar_raw = Vec::new();
        append_tar_entry(&mut tar_raw, "bin/game.txt", b"launch-me");
        let large_asset = vec![0x42u8; 256_000];
        append_tar_entry(&mut tar_raw, "data/asset.bin", &large_asset);
        tar_raw.resize(tar_raw.len() + 1024, 0);

        let compressed = zstd::bulk::compress(&tar_raw, 3).unwrap();
        let pipe = InFlightZstPipeline::start(
            StreamArchiveFormat::TarZst,
            dir.clone(),
            dir.join("unused.bin"),
        );

        let mid = compressed.len() / 2;
        // Feed range 1 BEFORE range 0 to prove contiguous ring reordering handles out-of-order AIMD workers.
        pipe.submit_range(mid as u64, &compressed[mid..]).unwrap();
        pipe.submit_range(0, &compressed[..mid]).unwrap();
        let stats = pipe.finish().unwrap();

        assert_eq!(stats.files_extracted, 2);
        assert_eq!(fs::read(dir.join("bin/game.txt")).unwrap(), b"launch-me");
        assert_eq!(fs::read(dir.join("data/asset.bin")).unwrap(), large_asset);

        let _ = fs::remove_dir_all(&dir);
    }

    fn build_synthetic_zip(entries: &[(&str, &[u8], bool)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut cd = Vec::new();
        for &(name, payload, deflate) in entries {
            let local_off = out.len() as u32;
            let mut crc = StreamingCrc32::new();
            crc.update(payload);
            let crc32 = crc.finalize();
            let (method, comp_bytes) = if deflate {
                let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
                enc.write_all(payload).unwrap();
                (8u16, enc.finish().unwrap())
            } else {
                (0u16, payload.to_vec())
            };

            out.extend_from_slice(&[0x50, 0x4B, 0x03, 0x04]);
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&crc32.to_le_bytes());
            out.extend_from_slice(&(comp_bytes.len() as u32).to_le_bytes());
            out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&comp_bytes);

            cd.extend_from_slice(&[0x50, 0x4B, 0x01, 0x02]);
            cd.extend_from_slice(&20u16.to_le_bytes());
            cd.extend_from_slice(&20u16.to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&method.to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&crc32.to_le_bytes());
            cd.extend_from_slice(&(comp_bytes.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(name.len() as u16).to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&0u32.to_le_bytes());
            cd.extend_from_slice(&local_off.to_le_bytes());
            cd.extend_from_slice(name.as_bytes());
        }
        let cd_off = out.len() as u32;
        let cd_sz = cd.len() as u32;
        out.extend_from_slice(&cd);
        out.extend_from_slice(&[0x50, 0x4B, 0x05, 0x06]);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&cd_sz.to_le_bytes());
        out.extend_from_slice(&cd_off.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    #[test]
    fn zip_eocd_tail_parser_and_parallel_entry_dispatch_work() {
        let dir = std::env::temp_dir().join(format!("duke_zip_eocd_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let zip_bytes = build_synthetic_zip(&[
            ("readme.txt", b"stored entry", false),
            ("sub/config.json", b"{\"ok\":true,\"speed\":100}", true),
        ]);
        let zip_path = dir.join("game.zip");
        fs::write(&zip_path, &zip_bytes).unwrap();

        let total = zip_bytes.len() as u64;
        let tail_start = total.saturating_sub(256);
        let (_, _, cd_bytes) =
            parse_zip_central_directory_from_tail(&zip_bytes[tail_start as usize..], tail_start, total)
                .unwrap()
                .unwrap();
        let plan = parse_zip_central_directory_entries(&cd_bytes, total - 22 - cd_bytes.len() as u64)
            .unwrap();
        assert_eq!(plan.entries.len(), 2);

        let extract_dir = dir.join("out");
        let mut extracted = HashSet::new();
        let first_end = plan.entries[0].range_end_exclusive;
        // Only entry 0's range is complete first!
        let (cnt1, _) = dispatch_ready_zip_entries(
            &zip_path,
            &extract_dir,
            &plan,
            &[(0, first_end)],
            &mut extracted,
        )
        .unwrap();
        assert_eq!(cnt1, 1);
        assert!(extract_dir.join("readme.txt").is_file());
        assert!(!extract_dir.join("sub/config.json").exists());

        // Now complete the rest of the archive and extract entry 1 in parallel.
        let (cnt2, _) = dispatch_ready_zip_entries(
            &zip_path,
            &extract_dir,
            &plan,
            &[(0, total)],
            &mut extracted,
        )
        .unwrap();
        assert_eq!(cnt2, 1);
        assert_eq!(
            fs::read_to_string(extract_dir.join("sub/config.json")).unwrap(),
            "{\"ok\":true,\"speed\":100}"
        );

        let _ = fs::remove_dir_all(&dir);
    }
}

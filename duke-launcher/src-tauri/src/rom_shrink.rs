//! Pure-Rust retro ROM lossless compressor (`.chd` v5 & `.rvz`) with SHA-1/CRC32 integrity verification.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};

use crc32fast::Hasher as Crc32Hasher;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use crate::platforms;
use crate::rom_inspect;

const CHD_MAGIC: &[u8; 8] = b"MComprHD";
const CHD_V5_HEADER_LEN: u32 = 124;
const CHD_V5_VERSION: u32 = 5;
const CHD_CODEC_ZSTD: [u8; 4] = *b"zstd";
const CHD_CODEC_NONE: [u8; 4] = [0u8; 4];

const CHD_COMP_TYPE_0: u8 = 0;
const CHD_COMP_TYPE_3: u8 = 3;
const CHD_COMP_NONE: u8 = 4;
const CHD_COMP_SELF: u8 = 5;
const CHD_COMP_PARENT: u8 = 6;
const CHD_COMP_RLE_SMALL: u8 = 7;
const CHD_COMP_RLE_LARGE: u8 = 8;

const CHD_META_TAG_CHT2: [u8; 4] = *b"CHT2";
const CHD_MDFLAGS_CHECKSUM: u8 = 0x01;
const CHD_MAX_HUNK_BYTES: usize = 16 * 1024 * 1024;
const CHD_MAX_MAP_BYTES: usize = 64 * 1024 * 1024;

const RVZ_MAGIC: &[u8; 4] = b"RVZ\x01";
const RVZ_VERSION: u32 = 0x0100_0000;
const RVZ_VERSION_COMPATIBLE: u32 = 0x0100_0000;
const RVZ_FILE_HEAD_LEN: usize = 72;
const RVZ_DISC_STRUCT_LEN: usize = 192;
const RVZ_DISC_HEADER_COPY_LEN: usize = 128;
const RVZ_DISC_TYPE_GAMECUBE: u32 = 1;
const RVZ_DISC_TYPE_WII: u32 = 2;
const RVZ_COMPRESSION_ZSTD: u32 = 5;
const RVZ_CHUNK_SIZE: usize = 128 * 1024;
const RVZ_MAX_CHUNK_SIZE: usize = 16 * 1024 * 1024;
const RVZ_RAW_DATA_ENTRY_LEN: usize = 24;
const RVZ_GROUP_ENTRY_LEN: usize = 16;

const GC_MAGIC: [u8; 4] = [0xC2, 0x33, 0x9F, 0x3D];
const WII_MAGIC: [u8; 4] = [0x5D, 0x1C, 0x9E, 0xA3];

const ZSTD_COMPRESSION_LEVEL: i32 = 5;
const MAX_CUE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_M3U_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RomShrinkCandidate {
    pub game_id: String,
    pub title: String,
    pub platform_slug: String,
    pub source_path: String,
    pub target_format: String,
    pub source_bytes: u64,
    pub tracked_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RomShrinkReport {
    pub game_id: String,
    pub source_path: String,
    pub output_path: String,
    pub format: String,
    pub original_bytes: u64,
    pub compressed_bytes: u64,
    pub saved_bytes: u64,
    pub ratio_pct: f64,
    pub sha1_hex: String,
    pub crc32_hex: String,
    pub verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BatchRomShrinkReport {
    pub converted_count: usize,
    pub skipped_count: usize,
    pub total_original_bytes: u64,
    pub total_compressed_bytes: u64,
    pub total_saved_bytes: u64,
    pub items: Vec<RomShrinkReport>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone)]
enum TargetKind {
    Chd,
    Rvz,
}

impl TargetKind {
    fn as_str(&self) -> &'static str {
        match self {
            TargetKind::Chd => "chd",
            TargetKind::Rvz => "rvz",
        }
    }
}

#[derive(Debug, Clone)]
struct CueFileSpec {
    path: PathBuf,
    size_bytes: u64,
}

#[derive(Debug, Clone)]
struct CueTrackSpec {
    track_num: u32,
    chd_type: &'static str,
    pg_type: &'static str,
    sector_bytes: u32,
    file_index: usize,
    pregap_frames: u32,
    postgap_frames: u32,
    index00_frames: Option<u32>,
    index01_frames: Option<u32>,
    frames: u32,
}

#[derive(Debug, Clone)]
struct CueSheet {
    files: Vec<CueFileSpec>,
    tracks: Vec<CueTrackSpec>,
    unit_bytes: u32,
    total_bytes: u64,
}

#[derive(Debug, Clone, Copy)]
struct ChdMapEntry {
    compression: u8,
    complength: u32,
    offset: u64,
    crc16: u16,
}

#[derive(Debug, Clone)]
struct ChdMetadataItem {
    tag: [u8; 4],
    flags: u8,
    data: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
struct RvzRawDataEntry {
    data_offset: u64,
    data_size: u64,
    group_index: u32,
    n_groups: u32,
}

#[derive(Debug, Clone, Copy)]
struct RvzGroupEntry {
    data_offset: u32,
    data_size: u32,
    n_exceptions: u32,
    rvz_packed_size: u32,
}

// ============================================================================
// CRC-16/CCITT-FALSE (MAME CHD v5 hunk & map checksum)
// ============================================================================

const fn build_crc16_table() -> [u16; 256] {
    let mut table = [0u16; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut crc = (i as u16) << 8;
        let mut bit = 0usize;
        while bit < 8 {
            if (crc & 0x8000) != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

const CHD_CRC16_TABLE: [u16; 256] = build_crc16_table();

fn chd_crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        let idx = (((crc >> 8) as u8) ^ b) as usize;
        crc = (crc << 8) ^ CHD_CRC16_TABLE[idx];
    }
    crc
}

fn bit_width_u32(val: u32) -> u8 {
    if val == 0 {
        0
    } else {
        (32 - val.leading_zeros()) as u8
    }
}

fn round_ratio_pct(compressed: u64, original: u64) -> f64 {
    if original == 0 {
        100.0
    } else {
        ((compressed as f64 / original as f64) * 1000.0).round() / 10.0
    }
}

fn zstd_compress_with_checksum(data: &[u8], level: i32) -> Result<Vec<u8>, String> {
    let mut compressor = zstd::bulk::Compressor::new(level)
        .map_err(|e| format!("Failed creating Zstd compressor: {e}"))?;
    compressor
        .include_checksum(true)
        .map_err(|e| format!("Failed enabling Zstd checksum: {e}"))?;
    compressor
        .compress(data)
        .map_err(|e| format!("Zstd compression failed: {e}"))
}

// ============================================================================
// Safe Path Resolution & .cue / .m3u Parsers
// ============================================================================

fn resolve_safe_child_path(base_dir: &Path, raw_ref: &str) -> Result<PathBuf, String> {
    let cleaned = raw_ref.trim().trim_matches('"').trim();
    if cleaned.is_empty() || cleaned.contains('\0') {
        return Err("Empty or invalid file reference".to_string());
    }
    if cleaned.starts_with('/') || cleaned.starts_with('\\') {
        return Err(format!("Unsafe rooted path reference rejected: {cleaned}"));
    }
    let bytes = cleaned.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' {
        return Err(format!("Unsafe drive-prefixed reference rejected: {cleaned}"));
    }

    let normalized = cleaned.replace('\\', "/");
    let mut rel = PathBuf::new();
    for comp in Path::new(&normalized).components() {
        match comp {
            Component::Normal(seg) => rel.push(seg),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!("Path traversal reference rejected: {cleaned}"));
            }
        }
    }
    if rel.as_os_str().is_empty() {
        return Err(format!("Invalid relative path reference: {cleaned}"));
    }

    let candidate = base_dir.join(&rel);
    if candidate.exists() {
        let canon_base = fs::canonicalize(base_dir)
            .map_err(|e| format!("Failed canonicalizing base directory: {e}"))?;
        let canon_target = fs::canonicalize(&candidate)
            .map_err(|e| format!("Failed canonicalizing referenced file {}: {e}", candidate.display()))?;
        if !canon_target.starts_with(&canon_base) {
            return Err(format!("Path escapes source directory: {cleaned}"));
        }
    }
    Ok(candidate)
}

fn parse_cue_msf_frames(msf: &str) -> Result<u32, String> {
    let mut parts = msf.trim().split(':');
    let m: u32 = parts
        .next()
        .ok_or_else(|| format!("Invalid MSF timestamp '{msf}'"))?
        .parse()
        .map_err(|_| format!("Invalid MSF minutes in '{msf}'"))?;
    let s: u32 = parts
        .next()
        .ok_or_else(|| format!("Invalid MSF seconds in '{msf}'"))?
        .parse()
        .map_err(|_| format!("Invalid MSF seconds in '{msf}'"))?;
    let f: u32 = parts
        .next()
        .ok_or_else(|| format!("Invalid MSF frames in '{msf}'"))?
        .parse()
        .map_err(|_| format!("Invalid MSF frames in '{msf}'"))?;
    if parts.next().is_some() || s >= 60 || f >= 75 {
        return Err(format!("Out-of-range MSF timestamp '{msf}'"));
    }
    m.checked_mul(60)
        .and_then(|sec| sec.checked_add(s))
        .and_then(|sec| sec.checked_mul(75))
        .and_then(|frm| frm.checked_add(f))
        .ok_or_else(|| format!("MSF frame overflow in '{msf}'"))
}

fn parse_cue_file_directive(line: &str) -> Result<(String, String), String> {
    let rest = line[4..].trim();
    if let Some(after_q) = rest.strip_prefix('"') {
        let end_q = after_q
            .find('"')
            .ok_or_else(|| format!("Unterminated quote in CUE FILE line: {line}"))?;
        let fname = after_q[..end_q].to_string();
        let ftype = after_q[end_q + 1..].trim().to_ascii_uppercase();
        Ok((fname, ftype))
    } else {
        let mut parts = rest.split_whitespace();
        let fname = parts
            .next()
            .ok_or_else(|| format!("Missing filename in CUE FILE line: {line}"))?
            .to_string();
        let ftype = parts.next().unwrap_or("BINARY").to_ascii_uppercase();
        Ok((fname, ftype))
    }
}

fn map_cue_track_mode(mode_raw: &str) -> Result<(&'static str, &'static str, u32), String> {
    let mode = mode_raw.trim().to_ascii_uppercase();
    match mode.as_str() {
        "MODE1/2048" => Ok(("MODE1", "MODE1", 2048)),
        "MODE1/2352" => Ok(("MODE1_RAW", "MODE1", 2352)),
        "MODE2/2336" => Ok(("MODE2", "MODE1", 2336)),
        "MODE2/2352" | "CDI/2352" => Ok(("MODE2_RAW", "MODE1", 2352)),
        "CDI/2336" => Ok(("MODE2", "MODE1", 2336)),
        "AUDIO" => Ok(("AUDIO", "VAUDIO", 2352)),
        _ => Err(format!("Unsupported CUE track mode '{mode_raw}'")),
    }
}

fn parse_cue_sheet(cue_path: &Path) -> Result<CueSheet, String> {
    let meta = fs::metadata(cue_path)
        .map_err(|e| format!("Cannot read CUE metadata {}: {e}", cue_path.display()))?;
    if meta.len() > MAX_CUE_BYTES {
        return Err("CUE sheet exceeds maximum allowed size".to_string());
    }
    let raw = fs::read_to_string(cue_path)
        .map_err(|e| format!("Failed reading CUE sheet {}: {e}", cue_path.display()))?;
    let text = raw.strip_prefix('\u{FEFF}').unwrap_or(&raw);
    let base_dir = cue_path
        .parent()
        .ok_or_else(|| "CUE file has no parent directory".to_string())?;

    let mut files: Vec<CueFileSpec> = Vec::new();
    let mut tracks: Vec<CueTrackSpec> = Vec::new();
    let mut current_file_idx: Option<usize> = None;

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        let upper = line.to_ascii_uppercase();
        if upper.starts_with("FILE ") {
            let (fname, ftype) = parse_cue_file_directive(line)?;
            if !matches!(ftype.as_str(), "BINARY" | "MOTOROLA" | "") {
                return Err(format!(
                    "Unsupported CUE FILE type '{ftype}' in {}; only raw BINARY tracks can be losslessly compressed",
                    cue_path.display()
                ));
            }
            let resolved = resolve_safe_child_path(base_dir, &fname)?;
            let fmeta = fs::metadata(&resolved).map_err(|e| {
                format!("Missing CUE track file {}: {e}", resolved.display())
            })?;
            if !fmeta.is_file() || fmeta.len() == 0 {
                return Err(format!("CUE track file is empty or invalid: {}", resolved.display()));
            }
            files.push(CueFileSpec {
                path: resolved,
                size_bytes: fmeta.len(),
            });
            current_file_idx = Some(files.len() - 1);
        } else if upper.starts_with("TRACK ") {
            let file_idx = current_file_idx
                .ok_or_else(|| "CUE TRACK appears before any FILE directive".to_string())?;
            let tokens: Vec<&str> = line.split_whitespace().collect();
            if tokens.len() < 3 {
                return Err(format!("Malformed CUE TRACK line: {line}"));
            }
            let track_num: u32 = tokens[1]
                .parse()
                .map_err(|_| format!("Invalid CUE track number '{}'", tokens[1]))?;
            let (chd_type, pg_type, sector_bytes) = map_cue_track_mode(tokens[2])?;
            tracks.push(CueTrackSpec {
                track_num,
                chd_type,
                pg_type,
                sector_bytes,
                file_index: file_idx,
                pregap_frames: 0,
                postgap_frames: 0,
                index00_frames: None,
                index01_frames: None,
                frames: 0,
            });
        } else if upper.starts_with("PREGAP ") {
            if let Some(tr) = tracks.last_mut() {
                let arg = line[6..].trim();
                tr.pregap_frames = parse_cue_msf_frames(arg)?;
            }
        } else if upper.starts_with("POSTGAP ") {
            if let Some(tr) = tracks.last_mut() {
                let arg = line[7..].trim();
                tr.postgap_frames = parse_cue_msf_frames(arg)?;
            }
        } else if upper.starts_with("INDEX ") {
            if let Some(tr) = tracks.last_mut() {
                let tokens: Vec<&str> = line.split_whitespace().collect();
                if tokens.len() >= 3 {
                    let idx_num: u32 = tokens[1]
                        .parse()
                        .map_err(|_| format!("Invalid CUE INDEX number '{}'", tokens[1]))?;
                    let frames = parse_cue_msf_frames(tokens[2])?;
                    if idx_num == 0 && tr.index00_frames.is_none() {
                        tr.index00_frames = Some(frames);
                    } else if idx_num == 1 && tr.index01_frames.is_none() {
                        tr.index01_frames = Some(frames);
                    }
                }
            }
        }
    }

    if files.is_empty() || tracks.is_empty() {
        return Err(format!("CUE sheet {} contains no valid tracks", cue_path.display()));
    }

    // Compute frame counts and pregap for every track within each FILE
    for (file_idx, cue_file) in files.iter().enumerate() {
        let track_indices: Vec<usize> = tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.file_index == file_idx)
            .map(|(idx, _)| idx)
            .collect();
        if track_indices.is_empty() {
            return Err(format!("CUE FILE #{} has no associated TRACK", file_idx + 1));
        }

        let file_bytes = cue_file.size_bytes;
        let first_sector = u64::from(tracks[track_indices[0]].sector_bytes);
        let file_total_frames = (file_bytes / first_sector) as u32;

        for pos in 0..track_indices.len() {
            let cur_i = track_indices[pos];
            let start_frame = tracks[cur_i]
                .index00_frames
                .or(tracks[cur_i].index01_frames)
                .unwrap_or(0);
            if let (Some(i00), Some(i01)) = (tracks[cur_i].index00_frames, tracks[cur_i].index01_frames) {
                if i01 > i00 && tracks[cur_i].pregap_frames == 0 {
                    tracks[cur_i].pregap_frames = i01 - i00;
                }
            }
            let end_frame = if pos + 1 < track_indices.len() {
                let next_i = track_indices[pos + 1];
                tracks[next_i]
                    .index00_frames
                    .or(tracks[next_i].index01_frames)
                    .unwrap_or(file_total_frames)
            } else {
                file_total_frames
            };
            tracks[cur_i].frames = end_frame.saturating_sub(start_frame).max(1);
        }
    }

    let first_sector = tracks[0].sector_bytes;
    let all_same_sector = tracks.iter().all(|t| t.sector_bytes == first_sector);
    let unit_bytes = if all_same_sector { first_sector } else { 2048 };
    let total_bytes = files.iter().try_fold(0u64, |acc, f| {
        acc.checked_add(f.size_bytes)
            .ok_or_else(|| "Total CUE stream size overflow".to_string())
    })?;

    Ok(CueSheet {
        files,
        tracks,
        unit_bytes,
        total_bytes,
    })
}

fn resolve_m3u_entries(m3u_path: &Path) -> Result<Vec<PathBuf>, String> {
    let meta = fs::metadata(m3u_path)
        .map_err(|e| format!("Cannot read M3U metadata {}: {e}", m3u_path.display()))?;
    if meta.len() > MAX_M3U_BYTES {
        return Err("M3U playlist exceeds maximum allowed size".to_string());
    }
    let raw = fs::read_to_string(m3u_path)
        .map_err(|e| format!("Failed reading M3U playlist {}: {e}", m3u_path.display()))?;
    let text = raw.strip_prefix('\u{FEFF}').unwrap_or(&raw);
    let base_dir = m3u_path
        .parent()
        .ok_or_else(|| "M3U file has no parent directory".to_string())?;

    let mut targets = Vec::new();
    for line in text.lines() {
        let tr = line.trim();
        if tr.is_empty() || tr.starts_with('#') {
            continue;
        }
        let target = resolve_safe_child_path(base_dir, tr)?;
        if !target.is_file() {
            return Err(format!("M3U target does not exist: {}", target.display()));
        }
        targets.push(target);
    }
    if targets.is_empty() {
        return Err(format!("M3U playlist {} has no disc entries", m3u_path.display()));
    }
    Ok(targets)
}

// ============================================================================
// Streaming Multi-File Input Reader
// ============================================================================

struct MultiFileReader {
    files: Vec<PathBuf>,
    current_idx: usize,
    current_file: Option<BufReader<File>>,
}

impl MultiFileReader {
    fn new(files: Vec<PathBuf>) -> Result<Self, String> {
        if files.is_empty() {
            return Err("No input files to stream".to_string());
        }
        let first = File::open(&files[0])
            .map_err(|e| format!("Failed opening {}: {e}", files[0].display()))?;
        Ok(Self {
            files,
            current_idx: 0,
            current_file: Some(BufReader::with_capacity(64 * 1024, first)),
        })
    }

    fn read_chunk(&mut self, buf: &mut [u8]) -> Result<usize, String> {
        let mut filled = 0usize;
        while filled < buf.len() {
            let reader = match self.current_file.as_mut() {
                Some(r) => r,
                None => break,
            };
            let n = reader
                .read(&mut buf[filled..])
                .map_err(|e| format!("Input stream read error: {e}"))?;
            if n > 0 {
                filled += n;
                continue;
            }
            self.current_idx += 1;
            if self.current_idx >= self.files.len() {
                self.current_file = None;
                break;
            }
            let next_path = &self.files[self.current_idx];
            let next_f = File::open(next_path)
                .map_err(|e| format!("Failed opening {}: {e}", next_path.display()))?;
            self.current_file = Some(BufReader::with_capacity(64 * 1024, next_f));
        }
        Ok(filled)
    }
}

// ============================================================================
// MSB-first Bitstream + Canonical Huffman Codec (MAME CHD v5 Map)
// ============================================================================

struct BitWriter {
    out: Vec<u8>,
    buffer: u32,
    bits: u8,
}

impl BitWriter {
    fn new() -> Self {
        Self {
            out: Vec::new(),
            buffer: 0,
            bits: 0,
        }
    }

    fn write(&mut self, value: u32, num_bits: u8) {
        if num_bits == 0 {
            return;
        }
        let mask = if num_bits >= 32 {
            u32::MAX
        } else {
            (1u32 << num_bits) - 1
        };
        let val = value & mask;
        let shift = 32 - u32::from(self.bits) - u32::from(num_bits);
        if u32::from(self.bits) + u32::from(num_bits) <= 32 {
            self.buffer |= val << shift;
            self.bits += num_bits;
            while self.bits >= 8 {
                self.out.push((self.buffer >> 24) as u8);
                self.buffer <<= 8;
                self.bits -= 8;
            }
        } else {
            let first_bits = 32 - self.bits;
            let rem_bits = num_bits - first_bits;
            self.write(val >> rem_bits, first_bits);
            self.write(val & ((1u32 << rem_bits) - 1), rem_bits);
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.bits > 0 {
            self.out.push((self.buffer >> 24) as u8);
        }
        self.out
    }
}

struct BitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    buffer: u32,
    bits: u8,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte_pos: 0,
            buffer: 0,
            bits: 0,
        }
    }

    fn refill(&mut self) {
        while self.bits <= 24 {
            let b = if self.byte_pos < self.data.len() {
                let v = self.data[self.byte_pos];
                self.byte_pos += 1;
                v
            } else {
                0
            };
            self.buffer |= u32::from(b) << (24 - self.bits);
            self.bits += 8;
        }
    }

    fn peek(&mut self, num_bits: u8) -> u32 {
        if num_bits == 0 {
            return 0;
        }
        self.refill();
        self.buffer >> (32 - num_bits)
    }

    fn advance(&mut self, num_bits: u8) {
        if num_bits == 0 {
            return;
        }
        self.buffer <<= num_bits;
        self.bits = self.bits.saturating_sub(num_bits);
    }

    fn read(&mut self, num_bits: u8) -> Result<u32, String> {
        if num_bits == 0 {
            return Ok(0);
        }
        if num_bits > 24 {
            let hi_bits = num_bits - 16;
            let hi = self.read(hi_bits)?;
            let lo = self.read(16)?;
            return Ok((hi << 16) | lo);
        }
        let val = self.peek(num_bits);
        self.advance(num_bits);
        Ok(val)
    }
}

const HUFF_SYMBOLS: usize = 16;
const HUFF_MAX_BITS: u8 = 8;

#[derive(Clone)]
enum HuffTreeNode {
    Leaf(usize),
    Branch(Box<HuffTreeNode>, Box<HuffTreeNode>),
}

fn collect_huff_depths(node: &HuffTreeNode, depth: u8, depths: &mut [u8; HUFF_SYMBOLS]) {
    match node {
        HuffTreeNode::Leaf(sym) => {
            depths[*sym] = depth.max(1);
        }
        HuffTreeNode::Branch(l, r) => {
            collect_huff_depths(l, depth + 1, depths);
            collect_huff_depths(r, depth + 1, depths);
        }
    }
}

fn build_huffman_lengths(freqs: &[u64; HUFF_SYMBOLS]) -> Result<[u8; HUFF_SYMBOLS], String> {
    let active: Vec<(usize, u64)> = freqs
        .iter()
        .enumerate()
        .filter(|(_, &w)| w > 0)
        .map(|(i, &w)| (i, w))
        .collect();
    if active.is_empty() {
        return Err("Empty Huffman frequency table".to_string());
    }

    let mut lengths = [0u8; HUFF_SYMBOLS];
    if active.len() == 1 {
        let s0 = active[0].0;
        let s1 = if s0 == usize::from(CHD_COMP_TYPE_0) {
            usize::from(CHD_COMP_NONE)
        } else {
            usize::from(CHD_COMP_TYPE_0)
        };
        lengths[s0] = 1;
        lengths[s1] = 1;
        return Ok(lengths);
    }

    let mut heap: BinaryHeap<Reverse<(u64, usize, usize)>> = BinaryHeap::new();
    let mut nodes: Vec<HuffTreeNode> = Vec::with_capacity(active.len() * 2);
    for (sym, w) in &active {
        let idx = nodes.len();
        nodes.push(HuffTreeNode::Leaf(*sym));
        heap.push(Reverse((*w, *sym, idx)));
    }

    let mut tie = HUFF_SYMBOLS;
    while heap.len() > 1 {
        let Reverse((w1, _, n1)) = heap
            .pop()
            .ok_or_else(|| "Huffman heap underflow".to_string())?;
        let Reverse((w2, _, n2)) = heap
            .pop()
            .ok_or_else(|| "Huffman heap underflow".to_string())?;
        let combined = HuffTreeNode::Branch(Box::new(nodes[n1].clone()), Box::new(nodes[n2].clone()));
        let idx = nodes.len();
        nodes.push(combined);
        heap.push(Reverse((w1.saturating_add(w2), tie, idx)));
        tie += 1;
    }

    let Reverse((_, _, root_idx)) = heap
        .pop()
        .ok_or_else(|| "Huffman root missing".to_string())?;
    collect_huff_depths(&nodes[root_idx], 0, &mut lengths);

    // Clamp to HUFF_MAX_BITS and ensure exact Kraft equality (2^HUFF_MAX_BITS)
    for len in &mut lengths {
        if *len > HUFF_MAX_BITS {
            *len = HUFF_MAX_BITS;
        }
    }
    let target = 1u32 << HUFF_MAX_BITS;
    let kraft_sum = |lens: &[u8; HUFF_SYMBOLS]| -> u32 {
        lens.iter()
            .filter(|&&l| l > 0)
            .map(|&l| 1u32 << (HUFF_MAX_BITS - l))
            .sum()
    };

    let mut guard = 0usize;
    while kraft_sum(&lengths) > target && guard < 256 {
        guard += 1;
        if let Some(pos) = (0..HUFF_SYMBOLS).filter(|&i| lengths[i] > 0 && lengths[i] < HUFF_MAX_BITS).max_by_key(|&i| lengths[i]) {
            lengths[pos] += 1;
        } else {
            break;
        }
    }
    while kraft_sum(&lengths) < target && guard < 512 {
        guard += 1;
        let deficit = target - kraft_sum(&lengths);
        if let Some(pos) = (0..HUFF_SYMBOLS)
            .filter(|&i| lengths[i] > 1 && (1u32 << (HUFF_MAX_BITS - lengths[i])) <= deficit)
            .min_by_key(|&i| lengths[i])
        {
            lengths[pos] -= 1;
        } else {
            break;
        }
    }

    if kraft_sum(&lengths) != target {
        return Err("Failed constructing canonical Huffman code lengths".to_string());
    }
    Ok(lengths)
}

fn assign_canonical_codes(lengths: &[u8; HUFF_SYMBOLS]) -> Result<([u32; HUFF_SYMBOLS], u8), String> {
    let max_bits = *lengths.iter().max().unwrap_or(&0);
    if max_bits == 0 || max_bits > HUFF_MAX_BITS {
        return Err(format!("Invalid Huffman max_bits {max_bits}"));
    }

    let mut codes = [0u32; HUFF_SYMBOLS];
    let mut curstart = 0u32;
    for len in 1..=max_bits {
        for sym in 0..HUFF_SYMBOLS {
            if lengths[sym] == len {
                let shift = max_bits - len;
                codes[sym] = curstart >> shift;
                curstart += 1u32 << shift;
            }
        }
    }
    if curstart != (1u32 << max_bits) {
        return Err(format!(
            "Canonical Huffman Kraft sum mismatch: {curstart} != {}",
            1u32 << max_bits
        ));
    }
    Ok((codes, max_bits))
}

fn export_huffman_tree_rle(bw: &mut BitWriter, lengths: &[u8; HUFF_SYMBOLS]) {
    let max_len = *lengths.iter().max().unwrap_or(&1);
    let mut numbits: u8 = 2;
    while (1u8 << numbits) <= max_len {
        numbits += 1;
    }
    bw.write(u32::from(numbits), 3);

    let max_rep_extra = (1usize << numbits) - 1;
    let mut i = 0usize;
    while i < HUFF_SYMBOLS {
        let val = lengths[i];
        if val == 1 {
            bw.write(1, numbits);
            bw.write(1, numbits);
            i += 1;
            continue;
        }
        let mut run = 1usize;
        while i + run < HUFF_SYMBOLS && lengths[i + run] == val && run < 3 + max_rep_extra {
            run += 1;
        }
        if run >= 3 {
            bw.write(1, numbits);
            bw.write(u32::from(val), numbits);
            bw.write((run - 3) as u32, numbits);
            i += run;
        } else {
            bw.write(u32::from(val), numbits);
            i += 1;
        }
    }
}

fn import_huffman_tree_rle(br: &mut BitReader<'_>) -> Result<[u8; HUFF_SYMBOLS], String> {
    let numbits = br.read(3)? as u8;
    if numbits == 0 || numbits >= 6 {
        return Err(format!("Invalid CHD Huffman RLE numbits={numbits}"));
    }
    let mut lengths = [0u8; HUFF_SYMBOLS];
    let mut cur = 0usize;
    while cur < HUFF_SYMBOLS {
        let nodebits = br.read(numbits)? as u8;
        if nodebits != 1 {
            lengths[cur] = nodebits;
            cur += 1;
        } else {
            let nextbits = br.read(numbits)? as u8;
            if nextbits == 1 {
                lengths[cur] = 1;
                cur += 1;
            } else {
                let rep = (br.read(numbits)? as usize) + 3;
                if cur + rep > HUFF_SYMBOLS {
                    return Err("CHD Huffman RLE tree overflows 16 symbols".to_string());
                }
                for _ in 0..rep {
                    lengths[cur] = nextbits;
                    cur += 1;
                }
            }
        }
    }
    Ok(lengths)
}

fn build_huffman_lookup(lengths: &[u8; HUFF_SYMBOLS]) -> Result<(Vec<(u8, u8)>, u8), String> {
    let (codes, max_bits) = assign_canonical_codes(lengths)?;
    let table_len = 1usize << max_bits;
    let mut lookup = vec![(0u8, 0u8); table_len];
    for sym in 0..HUFF_SYMBOLS {
        let len = lengths[sym];
        if len == 0 {
            continue;
        }
        let shift = max_bits - len;
        let base = (codes[sym] as usize) << shift;
        let span = 1usize << shift;
        for slot in &mut lookup[base..base + span] {
            *slot = (sym as u8, len);
        }
    }
    Ok((lookup, max_bits))
}

fn encode_raw_map_entry(entry: &ChdMapEntry, dst: &mut [u8]) {
    dst[0] = entry.compression;
    let cl = entry.complength.to_be_bytes();
    dst[1..4].copy_from_slice(&cl[1..4]);
    let off = entry.offset.to_be_bytes();
    dst[4..10].copy_from_slice(&off[2..8]);
    dst[10..12].copy_from_slice(&entry.crc16.to_be_bytes());
}

fn encode_chd_v5_compressed_map(
    entries: &[ChdMapEntry],
    firstoffs: u64,
) -> Result<Vec<u8>, String> {
    let mut rawmap = vec![0u8; entries.len() * 12];
    let mut freqs = [0u64; HUFF_SYMBOLS];
    let mut max_complength = 0u32;
    let mut max_self_hunk = 0u32;
    let mut has_self = false;

    for (idx, entry) in entries.iter().enumerate() {
        encode_raw_map_entry(entry, &mut rawmap[idx * 12..(idx + 1) * 12]);
        let sym = usize::from(entry.compression);
        if sym >= HUFF_SYMBOLS {
            return Err(format!("Unsupported CHD compression code {}", entry.compression));
        }
        freqs[sym] += 1;
        if entry.compression <= CHD_COMP_TYPE_3 {
            max_complength = max_complength.max(entry.complength);
        } else if entry.compression == CHD_COMP_SELF {
            has_self = true;
            let self_idx = u32::try_from(entry.offset)
                .map_err(|_| "CHD SELF hunk index overflow".to_string())?;
            max_self_hunk = max_self_hunk.max(self_idx);
        }
    }

    let mapcrc = chd_crc16(&rawmap);
    let lengthbits = bit_width_u32(max_complength).max(1);
    let selfbits = if has_self {
        bit_width_u32(max_self_hunk).max(1)
    } else {
        0
    };
    let parentbits = 0u8;

    let lengths = build_huffman_lengths(&freqs)?;
    let (codes, _) = assign_canonical_codes(&lengths)?;

    let mut bw = BitWriter::new();
    export_huffman_tree_rle(&mut bw, &lengths);
    for entry in entries {
        let sym = usize::from(entry.compression);
        bw.write(codes[sym], lengths[sym]);
        if entry.compression <= CHD_COMP_TYPE_3 {
            bw.write(entry.complength, lengthbits);
            bw.write(u32::from(entry.crc16), 16);
        } else if entry.compression == CHD_COMP_NONE {
            bw.write(u32::from(entry.crc16), 16);
        } else if entry.compression == CHD_COMP_SELF {
            bw.write(entry.offset as u32, selfbits);
        }
    }
    let bitstream = bw.finish();
    let mapbytes = u32::try_from(bitstream.len())
        .map_err(|_| "Compressed CHD v5 map exceeds 4 GiB".to_string())?;

    let mut out = Vec::with_capacity(16 + bitstream.len());
    out.extend_from_slice(&mapbytes.to_be_bytes());
    let first_be = firstoffs.to_be_bytes();
    out.extend_from_slice(&first_be[2..8]);
    out.extend_from_slice(&mapcrc.to_be_bytes());
    out.push(lengthbits);
    out.push(selfbits);
    out.push(parentbits);
    out.push(0);
    out.extend_from_slice(&bitstream);
    Ok(out)
}

fn decode_chd_v5_compressed_map(
    map_header: &[u8; 16],
    bitstream: &[u8],
    total_hunks: usize,
    hunk_bytes: u32,
) -> Result<Vec<ChdMapEntry>, String> {
    let first_bytes = [
        0,
        0,
        map_header[4],
        map_header[5],
        map_header[6],
        map_header[7],
        map_header[8],
        map_header[9],
    ];
    let firstoffs = u64::from_be_bytes(first_bytes);
    let expected_mapcrc = u16::from_be_bytes([map_header[10], map_header[11]]);
    let lengthbits = map_header[12];
    let selfbits = map_header[13];
    let parentbits = map_header[14];
    if lengthbits > 31 || selfbits > 31 || parentbits > 31 {
        return Err("Invalid CHD v5 map bit-width header".to_string());
    }

    let mut br = BitReader::new(bitstream);
    let lengths = import_huffman_tree_rle(&mut br)?;
    let (lookup, max_bits) = build_huffman_lookup(&lengths)?;

    let mut entries = Vec::with_capacity(total_hunks);
    let mut last_comp = 0u8;
    let mut rep_count = 0usize;

    for _ in 0..total_hunks {
        let compression = if rep_count > 0 {
            rep_count -= 1;
            last_comp
        } else {
            let peek_idx = br.peek(max_bits) as usize;
            let (sym, code_len) = lookup
                .get(peek_idx)
                .copied()
                .ok_or_else(|| "CHD Huffman lookup out of bounds".to_string())?;
            if code_len == 0 {
                return Err("Encountered unused Huffman code in CHD map".to_string());
            }
            br.advance(code_len);
            if sym == CHD_COMP_RLE_SMALL {
                rep_count = 2 + (br.read(3)? as usize);
                last_comp
            } else if sym == CHD_COMP_RLE_LARGE {
                rep_count = 2 + 8 + (br.read(8)? as usize);
                last_comp
            } else {
                last_comp = sym;
                sym
            }
        };

        let mut complength = 0u32;
        let mut offset = 0u64;
        let mut crc16 = 0u16;

        if compression <= CHD_COMP_TYPE_3 {
            complength = br.read(lengthbits)?;
            crc16 = br.read(16)? as u16;
        } else if compression == CHD_COMP_NONE {
            complength = hunk_bytes;
            crc16 = br.read(16)? as u16;
        } else if compression == CHD_COMP_SELF {
            offset = u64::from(br.read(selfbits)?);
        } else if compression == CHD_COMP_PARENT {
            offset = u64::from(br.read(parentbits)?);
        } else {
            return Err(format!("Unsupported CHD v5 map compression type {compression}"));
        }

        entries.push(ChdMapEntry {
            compression,
            complength,
            offset,
            crc16,
        });
    }

    let mut cur_offs = firstoffs;
    let mut rawmap = vec![0u8; total_hunks * 12];
    for idx in 0..total_hunks {
        let comp = entries[idx].compression;
        if comp <= CHD_COMP_TYPE_3 || comp == CHD_COMP_NONE {
            entries[idx].offset = cur_offs;
            cur_offs = cur_offs
                .checked_add(u64::from(entries[idx].complength))
                .ok_or_else(|| "CHD hunk offset overflow".to_string())?;
        } else if comp == CHD_COMP_SELF {
            let target = usize::try_from(entries[idx].offset)
                .map_err(|_| "CHD SELF hunk index overflow".to_string())?;
            if target >= idx {
                return Err(format!("Invalid CHD SELF hunk reference {target} >= {idx}"));
            }
            entries[idx].crc16 = entries[target].crc16;
        } else if comp == CHD_COMP_PARENT {
            return Err("Parented delta CHDs are not standalone".to_string());
        }
        encode_raw_map_entry(&entries[idx], &mut rawmap[idx * 12..(idx + 1) * 12]);
    }

    let actual_mapcrc = chd_crc16(&rawmap);
    if actual_mapcrc != expected_mapcrc {
        return Err(format!(
            "CHD v5 map CRC-16 mismatch (expected 0x{expected_mapcrc:04X}, got 0x{actual_mapcrc:04X})"
        ));
    }

    Ok(entries)
}

fn compute_chd_overall_sha1(raw_sha1: &[u8; 20], metadata: &[ChdMetadataItem]) -> [u8; 20] {
    let mut items: Vec<&ChdMetadataItem> = metadata
        .iter()
        .filter(|m| (m.flags & CHD_MDFLAGS_CHECKSUM) != 0)
        .collect();
    if items.is_empty() {
        return *raw_sha1;
    }
    items.sort_by(|a, b| a.tag.cmp(&b.tag).then_with(|| a.data.cmp(&b.data)));

    let mut hasher = Sha1::new();
    hasher.update(raw_sha1);
    for item in items {
        hasher.update(item.tag);
        hasher.update(&item.data);
    }
    hasher.finalize().into()
}

fn build_cht2_metadata(cue: &CueSheet) -> Vec<ChdMetadataItem> {
    cue.tracks
        .iter()
        .map(|tr| {
            let s = format!(
                "TRACK:{} TYPE:{} SUBTYPE:NONE FRAMES:{} PREGAP:{} PGTYPE:{} PGSUB:NONE POSTGAP:{}\0",
                tr.track_num,
                tr.chd_type,
                tr.frames,
                tr.pregap_frames,
                tr.pg_type,
                tr.postgap_frames
            );
            ChdMetadataItem {
                tag: CHD_META_TAG_CHT2,
                flags: CHD_MDFLAGS_CHECKSUM,
                data: s.into_bytes(),
            }
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn encode_chd_v5_header(
    compressors: &[[u8; 4]; 4],
    logical_bytes: u64,
    map_offset: u64,
    meta_offset: u64,
    hunk_bytes: u32,
    unit_bytes: u32,
    raw_sha1: &[u8; 20],
    overall_sha1: &[u8; 20],
    parent_sha1: &[u8; 20],
) -> [u8; 124] {
    let mut hdr = [0u8; 124];
    hdr[0x00..0x08].copy_from_slice(CHD_MAGIC);
    hdr[0x08..0x0C].copy_from_slice(&CHD_V5_HEADER_LEN.to_be_bytes());
    hdr[0x0C..0x10].copy_from_slice(&CHD_V5_VERSION.to_be_bytes());
    for (idx, codec) in compressors.iter().enumerate() {
        let start = 0x10 + idx * 4;
        hdr[start..start + 4].copy_from_slice(codec);
    }
    hdr[0x20..0x28].copy_from_slice(&logical_bytes.to_be_bytes());
    hdr[0x28..0x30].copy_from_slice(&map_offset.to_be_bytes());
    hdr[0x30..0x38].copy_from_slice(&meta_offset.to_be_bytes());
    hdr[0x38..0x3C].copy_from_slice(&hunk_bytes.to_be_bytes());
    hdr[0x3C..0x40].copy_from_slice(&unit_bytes.to_be_bytes());
    hdr[0x40..0x54].copy_from_slice(raw_sha1);
    hdr[0x54..0x68].copy_from_slice(overall_sha1);
    hdr[0x68..0x7C].copy_from_slice(parent_sha1);
    hdr
}

// ============================================================================
// Pure-Rust CHD v5 Writer & Verifier
// ============================================================================

fn write_chd_v5_from_stream(
    input_files: &[PathBuf],
    total_bytes: u64,
    unit_bytes: u32,
    metadata: &[ChdMetadataItem],
    out_chd: &Path,
) -> Result<(String, String, u64), String> {
    if total_bytes == 0 {
        return Err("Cannot create CHD from empty stream".to_string());
    }
    let hunk_bytes = unit_bytes
        .checked_mul(8)
        .ok_or_else(|| "CHD hunk_bytes overflow".to_string())?;
    let hunk_usize = hunk_bytes as usize;
    if hunk_usize == 0 || hunk_usize > CHD_MAX_HUNK_BYTES {
        return Err(format!("Invalid CHD hunk size {hunk_usize}"));
    }

    let tmp_path = out_chd.with_extension("chd.tmp");
    let _ = fs::remove_file(&tmp_path);

    let write_res: Result<(String, String, u64), String> = (|| {
        let f = File::create(&tmp_path)
            .map_err(|e| format!("Failed creating {}: {e}", tmp_path.display()))?;
        let mut writer = BufWriter::with_capacity(256 * 1024, f);

        // 1. Placeholder 124-byte header
        writer
            .write_all(&[0u8; 124])
            .map_err(|e| format!("Failed writing CHD placeholder header: {e}"))?;
        let mut cur_offset = 124u64;

        // 2. Metadata chain (immediately after header for fast emulator probing)
        let meta_offset = if metadata.is_empty() { 0u64 } else { cur_offset };
        for (idx, item) in metadata.iter().enumerate() {
            let len_u32 = u32::try_from(item.data.len())
                .map_err(|_| "CHD metadata payload exceeds limit".to_string())?;
            if len_u32 > 0x00FF_FFFF {
                return Err("CHD metadata payload exceeds 24-bit length".to_string());
            }
            let flags_len = (u32::from(item.flags) << 24) | (len_u32 & 0x00FF_FFFF);
            let next_off = if idx + 1 < metadata.len() {
                cur_offset + 16 + u64::from(len_u32)
            } else {
                0u64
            };
            writer
                .write_all(&item.tag)
                .map_err(|e| format!("Failed writing CHD metadata tag: {e}"))?;
            writer
                .write_all(&flags_len.to_be_bytes())
                .map_err(|e| format!("Failed writing CHD metadata flags: {e}"))?;
            writer
                .write_all(&next_off.to_be_bytes())
                .map_err(|e| format!("Failed writing CHD metadata next offset: {e}"))?;
            writer
                .write_all(&item.data)
                .map_err(|e| format!("Failed writing CHD metadata payload: {e}"))?;
            cur_offset += 16 + u64::from(len_u32);
        }

        let firstoffs = cur_offset;
        let total_hunks_u64 = total_bytes.div_ceil(u64::from(hunk_bytes));
        let total_hunks = usize::try_from(total_hunks_u64)
            .map_err(|_| "CHD total hunk count overflow".to_string())?;

        // 3. Stream hunks using ONE heap buffer
        let mut stream = MultiFileReader::new(input_files.to_vec())?;
        let mut hunk_buf = vec![0u8; hunk_usize];
        let mut src_sha1 = Sha1::new();
        let mut src_crc32 = Crc32Hasher::new();
        let mut map_entries = Vec::with_capacity(total_hunks);
        let mut first_zero_hunk: Option<usize> = None;
        let mut remaining = total_bytes;

        for hunk_idx in 0..total_hunks {
            hunk_buf.fill(0);
            let want = remaining.min(hunk_usize as u64) as usize;
            let got = stream.read_chunk(&mut hunk_buf[..want])?;
            if got != want {
                return Err(format!(
                    "Unexpected EOF reading input stream at hunk {hunk_idx}: expected {want} bytes, got {got}"
                ));
            }
            remaining -= got as u64;

            src_sha1.update(&hunk_buf[..got]);
            src_crc32.update(&hunk_buf[..got]);

            let hunk_crc16 = chd_crc16(&hunk_buf);
            let is_zero = hunk_buf.iter().all(|&b| b == 0);
            if is_zero {
                if let Some(zero_idx) = first_zero_hunk {
                    map_entries.push(ChdMapEntry {
                        compression: CHD_COMP_SELF,
                        complength: 0,
                        offset: zero_idx as u64,
                        crc16: hunk_crc16,
                    });
                    continue;
                }
                first_zero_hunk = Some(hunk_idx);
            }

            let compressed = zstd_compress_with_checksum(&hunk_buf, ZSTD_COMPRESSION_LEVEL)?;
            if compressed.len() < hunk_usize {
                writer
                    .write_all(&compressed)
                    .map_err(|e| format!("Failed writing CHD compressed hunk: {e}"))?;
                map_entries.push(ChdMapEntry {
                    compression: CHD_COMP_TYPE_0,
                    complength: compressed.len() as u32,
                    offset: cur_offset,
                    crc16: hunk_crc16,
                });
                cur_offset += compressed.len() as u64;
            } else {
                writer
                    .write_all(&hunk_buf)
                    .map_err(|e| format!("Failed writing CHD raw hunk: {e}"))?;
                map_entries.push(ChdMapEntry {
                    compression: CHD_COMP_NONE,
                    complength: hunk_bytes,
                    offset: cur_offset,
                    crc16: hunk_crc16,
                });
                cur_offset += hunk_usize as u64;
            }
        }

        // 4. Write CHD v5 compressed map after hunks
        let map_offset = cur_offset;
        let encoded_map = encode_chd_v5_compressed_map(&map_entries, firstoffs)?;
        writer
            .write_all(&encoded_map)
            .map_err(|e| format!("Failed writing CHD v5 map: {e}"))?;
        writer
            .flush()
            .map_err(|e| format!("Failed flushing CHD writer: {e}"))?;

        let raw_sha1: [u8; 20] = src_sha1.finalize().into();
        let overall_sha1 = compute_chd_overall_sha1(&raw_sha1, metadata);
        let codecs = [CHD_CODEC_ZSTD, CHD_CODEC_NONE, CHD_CODEC_NONE, CHD_CODEC_NONE];
        let hdr = encode_chd_v5_header(
            &codecs,
            total_bytes,
            map_offset,
            meta_offset,
            hunk_bytes,
            unit_bytes,
            &raw_sha1,
            &overall_sha1,
            &[0u8; 20],
        );

        let mut file = writer
            .into_inner()
            .map_err(|e| format!("Failed retrieving CHD file handle: {e}"))?;
        file.seek(SeekFrom::Start(0))
            .map_err(|e| format!("Failed seeking CHD header: {e}"))?;
        file.write_all(&hdr)
            .map_err(|e| format!("Failed writing CHD header: {e}"))?;
        file.sync_all()
            .map_err(|e| format!("Failed syncing CHD temp file: {e}"))?;
        drop(file);

        // 5. Pure-Rust roundtrip verification pass before atomic promotion
        let expected_sha1 = hex::encode(raw_sha1);
        let expected_crc32 = format!("{:08x}", src_crc32.finalize());
        let (ver_sha1, ver_crc32, ver_bytes) = verify_chd_v5(&tmp_path)?;
        if ver_sha1 != expected_sha1 || ver_crc32 != expected_crc32 || ver_bytes != total_bytes {
            return Err(format!(
                "CHD roundtrip verification mismatch (sha1 {ver_sha1} vs {expected_sha1}, crc32 {ver_crc32} vs {expected_crc32})"
            ));
        }
        let archive_len = fs::metadata(&tmp_path)
            .map_err(|e| format!("Failed reading CHD size: {e}"))?
            .len();

        Ok((expected_sha1, expected_crc32, archive_len))
    })();

    match write_res {
        Ok(res) => {
            if let Err(e) = fs::rename(&tmp_path, out_chd) {
                let _ = fs::remove_file(&tmp_path);
                return Err(format!("Failed promoting {} to {}: {e}", tmp_path.display(), out_chd.display()));
            }
            Ok(res)
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp_path);
            Err(e)
        }
    }
}

fn read_exact_at(file: &mut File, offset: u64, dst: &mut [u8]) -> Result<(), String> {
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| format!("Seek error at 0x{offset:X}: {e}"))?;
    file.read_exact(dst)
        .map_err(|e| format!("Read error at 0x{offset:X} ({} bytes): {e}", dst.len()))
}

fn verify_chd_v5(path: &Path) -> Result<(String, String, u64), String> {
    let mut file = File::open(path)
        .map_err(|e| format!("Failed opening CHD {}: {e}", path.display()))?;
    let file_len = file
        .metadata()
        .map_err(|e| format!("Failed reading CHD metadata: {e}"))?
        .len();
    if file_len < 124 {
        return Err("File too small to be a valid CHD v5 container".to_string());
    }

    let mut hdr = [0u8; 124];
    file.read_exact(&mut hdr)
        .map_err(|e| format!("Failed reading CHD header: {e}"))?;
    if &hdr[0..8] != CHD_MAGIC {
        return Err("Invalid CHD magic header".to_string());
    }
    let header_len = u32::from_be_bytes([hdr[8], hdr[9], hdr[10], hdr[11]]);
    let version = u32::from_be_bytes([hdr[12], hdr[13], hdr[14], hdr[15]]);
    if header_len != CHD_V5_HEADER_LEN || version != CHD_V5_VERSION {
        return Err(format!("Unsupported CHD header (len={header_len}, version={version})"));
    }

    let mut codecs = [[0u8; 4]; 4];
    for (i, slot) in codecs.iter_mut().enumerate() {
        slot.copy_from_slice(&hdr[0x10 + i * 4..0x14 + i * 4]);
    }
    let logical_bytes = u64::from_be_bytes([
        hdr[0x20], hdr[0x21], hdr[0x22], hdr[0x23], hdr[0x24], hdr[0x25], hdr[0x26], hdr[0x27],
    ]);
    let map_offset = u64::from_be_bytes([
        hdr[0x28], hdr[0x29], hdr[0x2A], hdr[0x2B], hdr[0x2C], hdr[0x2D], hdr[0x2E], hdr[0x2F],
    ]);
    let meta_offset = u64::from_be_bytes([
        hdr[0x30], hdr[0x31], hdr[0x32], hdr[0x33], hdr[0x34], hdr[0x35], hdr[0x36], hdr[0x37],
    ]);
    let hunk_bytes = u32::from_be_bytes([hdr[0x38], hdr[0x39], hdr[0x3A], hdr[0x3B]]);
    let unit_bytes = u32::from_be_bytes([hdr[0x3C], hdr[0x3D], hdr[0x3E], hdr[0x3F]]);
    let mut raw_sha1 = [0u8; 20];
    raw_sha1.copy_from_slice(&hdr[0x40..0x54]);
    let mut overall_sha1 = [0u8; 20];
    overall_sha1.copy_from_slice(&hdr[0x54..0x68]);
    if hdr[0x68..0x7C].iter().any(|&b| b != 0) {
        return Err("Delta/parent CHDs require parent image for verification".to_string());
    }
    if unit_bytes == 0 || hunk_bytes == 0 || hunk_bytes % unit_bytes != 0 {
        return Err("Invalid CHD unit_bytes / hunk_bytes geometry".to_string());
    }
    let hunk_usize = hunk_bytes as usize;
    if hunk_usize > CHD_MAX_HUNK_BYTES {
        return Err(format!("CHD hunk_bytes {hunk_usize} exceeds safety bound"));
    }

    // Parse metadata chain and validate overall SHA-1
    let mut metadata_items = Vec::new();
    let mut cur_meta = meta_offset;
    let mut visited_meta = HashSet::new();
    while cur_meta != 0 {
        if !visited_meta.insert(cur_meta) || metadata_items.len() >= 1024 {
            return Err("Cyclic or excessive CHD metadata chain".to_string());
        }
        if cur_meta + 16 > file_len {
            return Err("CHD metadata header out of file bounds".to_string());
        }
        let mut mhdr = [0u8; 16];
        read_exact_at(&mut file, cur_meta, &mut mhdr)?;
        let mut tag = [0u8; 4];
        tag.copy_from_slice(&mhdr[0..4]);
        let flags_len = u32::from_be_bytes([mhdr[4], mhdr[5], mhdr[6], mhdr[7]]);
        let flags = (flags_len >> 24) as u8;
        let data_len = (flags_len & 0x00FF_FFFF) as usize;
        let next_off = u64::from_be_bytes([
            mhdr[8], mhdr[9], mhdr[10], mhdr[11], mhdr[12], mhdr[13], mhdr[14], mhdr[15],
        ]);
        if data_len > 1024 * 1024 || cur_meta + 16 + (data_len as u64) > file_len {
            return Err("CHD metadata payload out of bounds".to_string());
        }
        let mut data = vec![0u8; data_len];
        read_exact_at(&mut file, cur_meta + 16, &mut data)?;
        metadata_items.push(ChdMetadataItem { tag, flags, data });
        cur_meta = next_off;
    }

    let computed_overall = compute_chd_overall_sha1(&raw_sha1, &metadata_items);
    if computed_overall != overall_sha1 {
        return Err("CHD overall SHA-1 metadata digest mismatch".to_string());
    }

    // Decode CHD v5 map
    let total_hunks_u64 = logical_bytes.div_ceil(u64::from(hunk_bytes));
    let total_hunks = usize::try_from(total_hunks_u64)
        .map_err(|_| "CHD hunk count overflow".to_string())?;
    let is_uncompressed = codecs[0] == CHD_CODEC_NONE;
    let map_entries = if is_uncompressed {
        let map_bytes = total_hunks
            .checked_mul(4)
            .ok_or_else(|| "CHD uncompressed map size overflow".to_string())?;
        if map_offset + (map_bytes as u64) > file_len {
            return Err("CHD uncompressed map extends past EOF".to_string());
        }
        let mut raw_u32s = vec![0u8; map_bytes];
        read_exact_at(&mut file, map_offset, &mut raw_u32s)?;
        let mut out = Vec::with_capacity(total_hunks);
        for chunk in raw_u32s.as_chunks::<4>().0 {
            let block_idx = u32::from_be_bytes(*chunk);
            out.push(ChdMapEntry {
                compression: CHD_COMP_NONE,
                complength: hunk_bytes,
                offset: u64::from(block_idx) * u64::from(hunk_bytes),
                crc16: 0,
            });
        }
        out
    } else {
        if map_offset + 16 > file_len {
            return Err("CHD compressed map header extends past EOF".to_string());
        }
        let mut mhdr = [0u8; 16];
        read_exact_at(&mut file, map_offset, &mut mhdr)?;
        let mapbytes = u32::from_be_bytes([mhdr[0], mhdr[1], mhdr[2], mhdr[3]]) as usize;
        if mapbytes == 0 || mapbytes > CHD_MAX_MAP_BYTES || map_offset + 16 + (mapbytes as u64) > file_len {
            return Err("Invalid CHD compressed map byte length".to_string());
        }
        let mut bitstream = vec![0u8; mapbytes];
        read_exact_at(&mut file, map_offset + 16, &mut bitstream)?;
        decode_chd_v5_compressed_map(&mhdr, &bitstream, total_hunks, hunk_bytes)?
    };

    // Stream-decompress and verify every hunk + CRC-16 + SHA-1 + CRC-32
    let mut sha1 = Sha1::new();
    let mut crc32 = Crc32Hasher::new();
    let mut hunk_buf = vec![0u8; hunk_usize];
    let mut comp_buf = Vec::with_capacity(hunk_usize);
    let mut remaining = logical_bytes;

    for idx in 0..total_hunks {
        let mut cur_idx = idx;
        let mut hops = 0usize;
        while map_entries[cur_idx].compression == CHD_COMP_SELF {
            let next = usize::try_from(map_entries[cur_idx].offset)
                .map_err(|_| "CHD SELF reference overflow".to_string())?;
            if next >= cur_idx || hops >= 16 {
                return Err("Invalid or cyclic CHD SELF hunk chain".to_string());
            }
            cur_idx = next;
            hops += 1;
        }

        let entry = map_entries[cur_idx];
        if entry.compression == CHD_COMP_NONE {
            if entry.offset + u64::from(hunk_bytes) > file_len {
                return Err(format!("CHD raw hunk {cur_idx} out of bounds"));
            }
            read_exact_at(&mut file, entry.offset, &mut hunk_buf)?;
        } else if entry.compression <= CHD_COMP_TYPE_3 {
            let codec = codecs[usize::from(entry.compression)];
            if codec != CHD_CODEC_ZSTD {
                return Err(format!(
                    "Unsupported CHD hunk codec '{}' in pure-Rust verifier",
                    String::from_utf8_lossy(&codec)
                ));
            }
            let clen = entry.complength as usize;
            if clen == 0 || clen > hunk_usize * 2 || entry.offset + (clen as u64) > file_len {
                return Err(format!("CHD compressed hunk {cur_idx} out of bounds"));
            }
            comp_buf.resize(clen, 0);
            read_exact_at(&mut file, entry.offset, &mut comp_buf)?;
            let decompressed = zstd::bulk::decompress(&comp_buf, hunk_usize)
                .map_err(|e| format!("CHD hunk {cur_idx} Zstd decompression failed: {e}"))?;
            if decompressed.len() != hunk_usize {
                return Err(format!(
                    "CHD hunk {cur_idx} decoded length {} != expected {hunk_usize}",
                    decompressed.len()
                ));
            }
            hunk_buf.copy_from_slice(&decompressed);
        } else {
            return Err(format!("Unsupported CHD hunk compression {}", entry.compression));
        }

        if !is_uncompressed {
            let actual_crc16 = chd_crc16(&hunk_buf);
            if actual_crc16 != map_entries[idx].crc16 {
                return Err(format!(
                    "CHD hunk {idx} CRC-16 mismatch (expected 0x{:04X}, got 0x{actual_crc16:04X})",
                    map_entries[idx].crc16
                ));
            }
        }

        let valid = remaining.min(hunk_usize as u64) as usize;
        sha1.update(&hunk_buf[..valid]);
        crc32.update(&hunk_buf[..valid]);
        remaining -= valid as u64;
    }

    if remaining != 0 {
        return Err("CHD stream ended before logical_bytes satisfied".to_string());
    }

    let computed_raw_sha1: [u8; 20] = sha1.finalize().into();
    if computed_raw_sha1 != raw_sha1 {
        return Err(format!(
            "CHD SHA-1 mismatch (expected {}, got {})",
            hex::encode(raw_sha1),
            hex::encode(computed_raw_sha1)
        ));
    }

    Ok((
        hex::encode(computed_raw_sha1),
        format!("{:08x}", crc32.finalize()),
        logical_bytes,
    ))
}

// ============================================================================
// Pure-Rust RVZ (`RVZ\x01`) Writer & Verifier
// ============================================================================

fn serialize_rvz_raw_data(entries: &[RvzRawDataEntry]) -> Vec<u8> {
    let mut out = Vec::with_capacity(entries.len() * RVZ_RAW_DATA_ENTRY_LEN);
    for e in entries {
        out.extend_from_slice(&e.data_offset.to_be_bytes());
        out.extend_from_slice(&e.data_size.to_be_bytes());
        out.extend_from_slice(&e.group_index.to_be_bytes());
        out.extend_from_slice(&e.n_groups.to_be_bytes());
    }
    out
}

fn parse_rvz_raw_data(bytes: &[u8]) -> Result<Vec<RvzRawDataEntry>, String> {
    if !bytes.len().is_multiple_of(RVZ_RAW_DATA_ENTRY_LEN) {
        return Err("Malformed RVZ raw_data table size".to_string());
    }
    let mut out = Vec::with_capacity(bytes.len() / RVZ_RAW_DATA_ENTRY_LEN);
    for chunk in bytes.as_chunks::<RVZ_RAW_DATA_ENTRY_LEN>().0 {
        out.push(RvzRawDataEntry {
            data_offset: u64::from_be_bytes([
                chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
            ]),
            data_size: u64::from_be_bytes([
                chunk[8], chunk[9], chunk[10], chunk[11], chunk[12], chunk[13], chunk[14], chunk[15],
            ]),
            group_index: u32::from_be_bytes([chunk[16], chunk[17], chunk[18], chunk[19]]),
            n_groups: u32::from_be_bytes([chunk[20], chunk[21], chunk[22], chunk[23]]),
        });
    }
    Ok(out)
}

fn serialize_rvz_groups(entries: &[RvzGroupEntry]) -> Vec<u8> {
    let mut out = Vec::with_capacity(entries.len() * RVZ_GROUP_ENTRY_LEN);
    for e in entries {
        out.extend_from_slice(&e.data_offset.to_be_bytes());
        out.extend_from_slice(&e.data_size.to_be_bytes());
        out.extend_from_slice(&e.n_exceptions.to_be_bytes());
        out.extend_from_slice(&e.rvz_packed_size.to_be_bytes());
    }
    out
}

fn parse_rvz_groups(bytes: &[u8]) -> Result<Vec<RvzGroupEntry>, String> {
    if !bytes.len().is_multiple_of(RVZ_GROUP_ENTRY_LEN) {
        return Err("Malformed RVZ group table size".to_string());
    }
    let mut out = Vec::with_capacity(bytes.len() / RVZ_GROUP_ENTRY_LEN);
    for chunk in bytes.as_chunks::<RVZ_GROUP_ENTRY_LEN>().0 {
        out.push(RvzGroupEntry {
            data_offset: u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
            data_size: u32::from_be_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]),
            n_exceptions: u32::from_be_bytes([chunk[8], chunk[9], chunk[10], chunk[11]]),
            rvz_packed_size: u32::from_be_bytes([chunk[12], chunk[13], chunk[14], chunk[15]]),
        });
    }
    Ok(out)
}

fn write_rvz_from_iso(iso_path: &Path, out_rvz: &Path) -> Result<(String, String, u64, u64), String> {
    let mut src_file = File::open(iso_path)
        .map_err(|e| format!("Failed opening ISO {}: {e}", iso_path.display()))?;
    let iso_file_size = src_file
        .metadata()
        .map_err(|e| format!("Failed reading ISO metadata: {e}"))?
        .len();
    if iso_file_size < RVZ_DISC_HEADER_COPY_LEN as u64 {
        return Err(format!(
            "ISO file {} is too small ({iso_file_size} bytes) for RVZ container",
            iso_path.display()
        ));
    }

    let mut dhead = [0u8; RVZ_DISC_HEADER_COPY_LEN];
    src_file
        .read_exact(&mut dhead)
        .map_err(|e| format!("Failed reading disc header from {}: {e}", iso_path.display()))?;

    let disc_type = if dhead[0x18..0x1C] == WII_MAGIC {
        RVZ_DISC_TYPE_WII
    } else {
        RVZ_DISC_TYPE_GAMECUBE
    };

    let tmp_path = out_rvz.with_extension("rvz.tmp");
    let _ = fs::remove_file(&tmp_path);

    let write_res: Result<(String, String, u64, u64), String> = (|| {
        let out_file = File::create(&tmp_path)
            .map_err(|e| format!("Failed creating {}: {e}", tmp_path.display()))?;
        let mut writer = BufWriter::with_capacity(256 * 1024, out_file);

        // 1. Reserve 72B wia_file_head_t + 192B wia_disc_t = 264 (0x108) bytes
        let header_reserve = vec![0u8; RVZ_FILE_HEAD_LEN + RVZ_DISC_STRUCT_LEN];
        writer
            .write_all(&header_reserve)
            .map_err(|e| format!("Failed reserving RVZ headers: {e}"))?;
        let mut cur_pos = (RVZ_FILE_HEAD_LEN + RVZ_DISC_STRUCT_LEN) as u64;

        let mut src_sha1 = Sha1::new();
        let mut src_crc32 = Crc32Hasher::new();
        src_sha1.update(dhead);
        src_crc32.update(&dhead);

        let data_bytes = iso_file_size - (RVZ_DISC_HEADER_COPY_LEN as u64);
        let num_groups_u64 = data_bytes.div_ceil(RVZ_CHUNK_SIZE as u64);
        let num_groups = usize::try_from(num_groups_u64)
            .map_err(|_| "RVZ chunk count overflow".to_string())?;
        let mut groups = Vec::with_capacity(num_groups);
        let mut chunk_buf = vec![0u8; RVZ_CHUNK_SIZE];
        let mut remaining = data_bytes;

        while remaining > 0 {
            let want = remaining.min(RVZ_CHUNK_SIZE as u64) as usize;
            src_file
                .read_exact(&mut chunk_buf[..want])
                .map_err(|e| format!("Failed reading ISO chunk: {e}"))?;
            remaining -= want as u64;

            src_sha1.update(&chunk_buf[..want]);
            src_crc32.update(&chunk_buf[..want]);

            let align_pad = (4 - (cur_pos % 4)) % 4;
            if align_pad > 0 {
                let zeros = [0u8; 4];
                writer
                    .write_all(&zeros[..align_pad as usize])
                    .map_err(|e| format!("Failed aligning RVZ chunk: {e}"))?;
                cur_pos += align_pad;
            }

            let offset_u32 = u32::try_from(cur_pos >> 2)
                .map_err(|_| "RVZ chunk offset exceeds 16 GiB 32-bit >>2 addressable limit".to_string())?;
            let compressed = zstd_compress_with_checksum(&chunk_buf[..want], ZSTD_COMPRESSION_LEVEL)?;
            if compressed.len() < want {
                let clen = u32::try_from(compressed.len())
                    .map_err(|_| "RVZ compressed chunk size overflow".to_string())?;
                writer
                    .write_all(&compressed)
                    .map_err(|e| format!("Failed writing RVZ compressed chunk: {e}"))?;
                cur_pos += u64::from(clen);
                groups.push(RvzGroupEntry {
                    data_offset: offset_u32,
                    data_size: 0x8000_0000 | clen,
                    n_exceptions: 0,
                    rvz_packed_size: 0,
                });
            } else {
                let rlen = want as u32;
                writer
                    .write_all(&chunk_buf[..want])
                    .map_err(|e| format!("Failed writing RVZ raw chunk: {e}"))?;
                cur_pos += want as u64;
                groups.push(RvzGroupEntry {
                    data_offset: offset_u32,
                    data_size: rlen,
                    n_exceptions: 0,
                    rvz_packed_size: 0,
                });
            }
        }

        // 2. Write compressed wia_raw_data_t table
        let raw_entries = [RvzRawDataEntry {
            data_offset: RVZ_DISC_HEADER_COPY_LEN as u64,
            data_size: data_bytes,
            group_index: 0,
            n_groups: groups.len() as u32,
        }];
        let raw_table_bytes = serialize_rvz_raw_data(&raw_entries);
        let raw_table_comp = zstd_compress_with_checksum(&raw_table_bytes, ZSTD_COMPRESSION_LEVEL)?;
        let raw_data_offset = cur_pos;
        writer
            .write_all(&raw_table_comp)
            .map_err(|e| format!("Failed writing RVZ raw_data table: {e}"))?;
        cur_pos += raw_table_comp.len() as u64;

        // 3. Write compressed rvz_group_t table
        let group_table_bytes = serialize_rvz_groups(&groups);
        let group_table_comp = zstd_compress_with_checksum(&group_table_bytes, ZSTD_COMPRESSION_LEVEL)?;
        let group_offset = cur_pos;
        writer
            .write_all(&group_table_comp)
            .map_err(|e| format!("Failed writing RVZ group table: {e}"))?;
        cur_pos += group_table_comp.len() as u64;

        let wia_file_size = cur_pos;
        writer
            .flush()
            .map_err(|e| format!("Failed flushing RVZ file: {e}"))?;

        // 4. Build 192B wia_disc_t and 72B wia_file_head_t
        let mut disc = vec![0u8; RVZ_DISC_STRUCT_LEN];
        disc[0x00..0x04].copy_from_slice(&disc_type.to_be_bytes());
        disc[0x04..0x08].copy_from_slice(&RVZ_COMPRESSION_ZSTD.to_be_bytes());
        disc[0x08..0x0C].copy_from_slice(&ZSTD_COMPRESSION_LEVEL.to_be_bytes());
        disc[0x0C..0x10].copy_from_slice(&(RVZ_CHUNK_SIZE as u32).to_be_bytes());
        disc[0x10..0x90].copy_from_slice(&dhead);
        // n_part = 0, part_t_size = 0, part_offset = 0
        disc[0xA0..0xA4].copy_from_slice(&1u32.to_be_bytes());
        disc[0xA4..0xAC].copy_from_slice(&raw_data_offset.to_be_bytes());
        disc[0xAC..0xB0].copy_from_slice(&(groups.len() as u32).to_be_bytes());
        disc[0xB0..0xB8].copy_from_slice(&group_offset.to_be_bytes());
        let disc_hash: [u8; 20] = Sha1::digest(&disc).into();

        let mut fhead = [0u8; RVZ_FILE_HEAD_LEN];
        fhead[0x00..0x04].copy_from_slice(RVZ_MAGIC);
        fhead[0x04..0x08].copy_from_slice(&RVZ_VERSION.to_be_bytes());
        fhead[0x08..0x0C].copy_from_slice(&RVZ_VERSION_COMPATIBLE.to_be_bytes());
        fhead[0x0C..0x10].copy_from_slice(&(RVZ_DISC_STRUCT_LEN as u32).to_be_bytes());
        fhead[0x10..0x24].copy_from_slice(&disc_hash);
        fhead[0x24..0x2C].copy_from_slice(&iso_file_size.to_be_bytes());
        fhead[0x2C..0x34].copy_from_slice(&wia_file_size.to_be_bytes());
        let file_head_hash: [u8; 20] = Sha1::digest(&fhead[0..0x34]).into();
        fhead[0x34..0x48].copy_from_slice(&file_head_hash);

        let mut file = writer
            .into_inner()
            .map_err(|e| format!("Failed retrieving RVZ handle: {e}"))?;
        file.seek(SeekFrom::Start(0))
            .map_err(|e| format!("Failed seeking RVZ header: {e}"))?;
        file.write_all(&fhead)
            .map_err(|e| format!("Failed writing RVZ file header: {e}"))?;
        file.write_all(&disc)
            .map_err(|e| format!("Failed writing RVZ disc header: {e}"))?;
        file.sync_all()
            .map_err(|e| format!("Failed syncing RVZ temp file: {e}"))?;
        drop(file);

        // 5. Pure-Rust roundtrip verification pass before atomic promotion
        let expected_sha1 = hex::encode(src_sha1.finalize());
        let expected_crc32 = format!("{:08x}", src_crc32.finalize());
        let (ver_sha1, ver_crc32, ver_bytes) = verify_rvz(&tmp_path)?;
        if ver_sha1 != expected_sha1 || ver_crc32 != expected_crc32 || ver_bytes != iso_file_size {
            return Err(format!(
                "RVZ roundtrip verification mismatch (sha1 {ver_sha1} vs {expected_sha1}, crc32 {ver_crc32} vs {expected_crc32})"
            ));
        }

        Ok((expected_sha1, expected_crc32, iso_file_size, wia_file_size))
    })();

    match write_res {
        Ok(res) => {
            if let Err(e) = fs::rename(&tmp_path, out_rvz) {
                let _ = fs::remove_file(&tmp_path);
                return Err(format!("Failed promoting {} to {}: {e}", tmp_path.display(), out_rvz.display()));
            }
            Ok(res)
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp_path);
            Err(e)
        }
    }
}

fn verify_rvz(path: &Path) -> Result<(String, String, u64), String> {
    let mut file = File::open(path)
        .map_err(|e| format!("Failed opening RVZ {}: {e}", path.display()))?;
    let actual_file_len = file
        .metadata()
        .map_err(|e| format!("Failed reading RVZ metadata: {e}"))?
        .len();
    if actual_file_len < (RVZ_FILE_HEAD_LEN + RVZ_DISC_STRUCT_LEN) as u64 {
        return Err("File too small to be a valid RVZ container".to_string());
    }

    let mut fhead = [0u8; RVZ_FILE_HEAD_LEN];
    file.read_exact(&mut fhead)
        .map_err(|e| format!("Failed reading RVZ file header: {e}"))?;
    if &fhead[0..4] != RVZ_MAGIC {
        return Err("Invalid RVZ magic header".to_string());
    }
    let expected_head_hash: [u8; 20] = Sha1::digest(&fhead[0..0x34]).into();
    if fhead[0x34..0x48] != expected_head_hash {
        return Err("RVZ wia_file_head_t SHA-1 integrity mismatch".to_string());
    }

    let disc_size = u32::from_be_bytes([fhead[0x0C], fhead[0x0D], fhead[0x0E], fhead[0x0F]]) as usize;
    let mut expected_disc_hash = [0u8; 20];
    expected_disc_hash.copy_from_slice(&fhead[0x10..0x24]);
    let iso_file_size = u64::from_be_bytes([
        fhead[0x24], fhead[0x25], fhead[0x26], fhead[0x27], fhead[0x28], fhead[0x29], fhead[0x2A], fhead[0x2B],
    ]);
    let wia_file_size = u64::from_be_bytes([
        fhead[0x2C], fhead[0x2D], fhead[0x2E], fhead[0x2F], fhead[0x30], fhead[0x31], fhead[0x32], fhead[0x33],
    ]);
    if wia_file_size != actual_file_len {
        return Err(format!(
            "RVZ file size mismatch (header={wia_file_size}, actual={actual_file_len})"
        ));
    }
    if !(RVZ_DISC_STRUCT_LEN..=4096).contains(&disc_size)
        || (RVZ_FILE_HEAD_LEN + disc_size) as u64 > actual_file_len
    {
        return Err("Invalid RVZ wia_disc_t struct size".to_string());
    }
    if iso_file_size < RVZ_DISC_HEADER_COPY_LEN as u64 {
        return Err("Invalid RVZ iso_file_size".to_string());
    }

    let mut disc = vec![0u8; disc_size];
    file.read_exact(&mut disc)
        .map_err(|e| format!("Failed reading RVZ disc struct: {e}"))?;
    let actual_disc_hash: [u8; 20] = Sha1::digest(&disc).into();
    if actual_disc_hash != expected_disc_hash {
        return Err("RVZ wia_disc_t SHA-1 integrity mismatch".to_string());
    }

    let compression = u32::from_be_bytes([disc[0x04], disc[0x05], disc[0x06], disc[0x07]]);
    if compression != RVZ_COMPRESSION_ZSTD {
        return Err(format!("Unsupported RVZ compression algorithm {compression}"));
    }
    let chunk_size = u32::from_be_bytes([disc[0x0C], disc[0x0D], disc[0x0E], disc[0x0F]]) as usize;
    if chunk_size == 0 || chunk_size > RVZ_MAX_CHUNK_SIZE {
        return Err(format!("Invalid RVZ chunk size {chunk_size}"));
    }
    let dhead = &disc[0x10..0x90];
    let n_part = u32::from_be_bytes([disc[0x90], disc[0x91], disc[0x92], disc[0x93]]);
    if n_part != 0 {
        return Err("Encrypted Wii partition RVZ tables require partition key decoding".to_string());
    }
    let n_raw_data = u32::from_be_bytes([disc[0xA0], disc[0xA1], disc[0xA2], disc[0xA3]]) as usize;
    let raw_data_offset = u64::from_be_bytes([
        disc[0xA4], disc[0xA5], disc[0xA6], disc[0xA7], disc[0xA8], disc[0xA9], disc[0xAA], disc[0xAB],
    ]);
    let n_groups = u32::from_be_bytes([disc[0xAC], disc[0xAD], disc[0xAE], disc[0xAF]]) as usize;
    let group_offset = u64::from_be_bytes([
        disc[0xB0], disc[0xB1], disc[0xB2], disc[0xB3], disc[0xB4], disc[0xB5], disc[0xB6], disc[0xB7],
    ]);

    if raw_data_offset >= group_offset || group_offset >= wia_file_size {
        return Err("Invalid RVZ metadata table offsets".to_string());
    }
    let raw_comp_len = (group_offset - raw_data_offset) as usize;
    let group_comp_len = (wia_file_size - group_offset) as usize;
    if raw_comp_len == 0 || raw_comp_len > 8 * 1024 * 1024 || group_comp_len == 0 || group_comp_len > 32 * 1024 * 1024 {
        return Err("RVZ table size out of bounds".to_string());
    }

    let mut raw_comp = vec![0u8; raw_comp_len];
    read_exact_at(&mut file, raw_data_offset, &mut raw_comp)?;
    let raw_expected = n_raw_data
        .checked_mul(RVZ_RAW_DATA_ENTRY_LEN)
        .ok_or_else(|| "RVZ raw_data table length overflow".to_string())?;
    let raw_bytes = zstd::bulk::decompress(&raw_comp, raw_expected)
        .map_err(|e| format!("RVZ raw_data table Zstd corruption: {e}"))?;
    let raw_entries = parse_rvz_raw_data(&raw_bytes)?;

    let mut group_comp = vec![0u8; group_comp_len];
    read_exact_at(&mut file, group_offset, &mut group_comp)?;
    let group_expected = n_groups
        .checked_mul(RVZ_GROUP_ENTRY_LEN)
        .ok_or_else(|| "RVZ group table length overflow".to_string())?;
    let group_bytes = zstd::bulk::decompress(&group_comp, group_expected)
        .map_err(|e| format!("RVZ group table Zstd corruption: {e}"))?;
    let group_entries = parse_rvz_groups(&group_bytes)?;

    let mut sha1 = Sha1::new();
    let mut crc32 = Crc32Hasher::new();
    sha1.update(dhead);
    crc32.update(dhead);

    let mut reconstructed = RVZ_DISC_HEADER_COPY_LEN as u64;
    let mut payload_buf = Vec::with_capacity(chunk_size);

    for raw in &raw_entries {
        if raw.data_offset != reconstructed {
            return Err("RVZ raw_data offset discontinuity".to_string());
        }
        let mut rem = raw.data_size;
        let g_start = raw.group_index as usize;
        let g_end = g_start
            .checked_add(raw.n_groups as usize)
            .ok_or_else(|| "RVZ group range overflow".to_string())?;
        if g_end > group_entries.len() {
            return Err("RVZ group index out of bounds".to_string());
        }

        for (g_idx, grp) in group_entries[g_start..g_end].iter().enumerate() {
            if grp.n_exceptions != 0 || grp.rvz_packed_size != 0 {
                return Err(format!("Unsupported RVZ exception/packed chunk at index {}", g_start + g_idx));
            }
            let expected_chunk = rem.min(chunk_size as u64) as usize;
            let file_off = u64::from(grp.data_offset) << 2;
            let is_compressed = (grp.data_size & 0x8000_0000) != 0;
            let payload_len = (grp.data_size & 0x7FFF_FFFF) as usize;
            if payload_len == 0 || payload_len > chunk_size * 2 || file_off + (payload_len as u64) > raw_data_offset {
                return Err(format!("RVZ chunk {} out of bounds", g_start + g_idx));
            }
            payload_buf.resize(payload_len, 0);
            read_exact_at(&mut file, file_off, &mut payload_buf)?;

            if is_compressed {
                let dec = zstd::bulk::decompress(&payload_buf, expected_chunk)
                    .map_err(|e| format!("RVZ chunk {} Zstd corruption: {e}", g_start + g_idx))?;
                if dec.len() != expected_chunk {
                    return Err(format!(
                        "RVZ chunk {} decoded length {} != expected {expected_chunk}",
                        g_start + g_idx,
                        dec.len()
                    ));
                }
                sha1.update(&dec);
                crc32.update(&dec);
            } else {
                if payload_len != expected_chunk {
                    return Err(format!(
                        "RVZ raw chunk {} length {} != expected {expected_chunk}",
                        g_start + g_idx,
                        payload_len
                    ));
                }
                sha1.update(&payload_buf);
                crc32.update(&payload_buf);
            }

            rem -= expected_chunk as u64;
            reconstructed += expected_chunk as u64;
        }
        if rem != 0 {
            return Err("RVZ raw_data region bytes remaining after group loop".to_string());
        }
    }

    if reconstructed != iso_file_size {
        return Err(format!(
            "RVZ reconstructed byte count mismatch (expected {iso_file_size}, got {reconstructed})"
        ));
    }

    Ok((
        hex::encode(sha1.finalize()),
        format!("{:08x}", crc32.finalize()),
        iso_file_size,
    ))
}

// ============================================================================
// Platform & Target Classification
// ============================================================================

fn detect_iso_target_and_unit_bytes(iso_path: &Path, hint_platform_id: Option<u16>) -> Result<(TargetKind, u32), String> {
    let mut f = File::open(iso_path)
        .map_err(|e| format!("Failed opening {}: {e}", iso_path.display()))?;
    let size = f
        .metadata()
        .map_err(|e| format!("Failed reading metadata for {}: {e}", iso_path.display()))?
        .len();
    let read_len = size.min(64 * 1024) as usize;
    let mut head = vec![0u8; read_len];
    if read_len > 0 {
        f.read_exact(&mut head)
            .map_err(|e| format!("Failed reading header from {}: {e}", iso_path.display()))?;
    }

    if head.len() >= 0x20 && (head[0x18..0x1C] == WII_MAGIC || head[0x1C..0x20] == GC_MAGIC) {
        return Ok((TargetKind::Rvz, 2048));
    }

    let ext = iso_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "gcm" {
        return Ok((TargetKind::Rvz, 2048));
    }

    if let Some(pid) = hint_platform_id {
        if pid == platforms::PLATFORM_GAMECUBE || pid == platforms::PLATFORM_WII {
            return Ok((TargetKind::Rvz, 2048));
        }
    }

    if let Some(inspected) = rom_inspect::inspect_rom_bytes(&head, None, size, iso_path) {
        if inspected.platform_id == platforms::PLATFORM_GAMECUBE || inspected.platform_id == platforms::PLATFORM_WII {
            return Ok((TargetKind::Rvz, 2048));
        }
    }

    // Raw 2352-byte CD sync pattern at sector 0 and sector 1
    let cd_sync: [u8; 12] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];
    let unit_bytes = if head.len() >= 2352 + 12 && head[0..12] == cd_sync && head[2352..2364] == cd_sync {
        2352
    } else {
        2048
    };
    Ok((TargetKind::Chd, unit_bytes))
}

fn is_raw_cd_bin_image(bin_path: &Path) -> bool {
    let Ok(mut f) = File::open(bin_path) else {
        return false;
    };
    let mut head = vec![0u8; 0x9B20];
    let Ok(n) = f.read(&mut head) else {
        return false;
    };
    head.truncate(n);
    let cd_sync: [u8; 12] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];
    if head.len() >= 2352 + 12 && head[0..12] == cd_sync && head[2352..2364] == cd_sync {
        return true;
    }
    if head.len() >= 0x8006 && &head[0x8000..0x8006] == b"\x01CD001" {
        return true;
    }
    if head.len() >= 0x931E && &head[0x9318..0x931E] == b"\x01CD001" {
        return true;
    }
    false
}

fn resolve_game_platform_id(entry: &crate::GameEntry, source_path: &Path) -> u16 {
    if entry.platform_id != 0 {
        return entry.platform_id;
    }
    if let Some(spec) = platforms::platform_from_slug_or_name(&entry.platform) {
        return spec.id;
    }
    if let Some(inspected) = rom_inspect::inspect_rom_file(source_path) {
        return inspected.platform_id;
    }
    let ext = source_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "gcm" => platforms::PLATFORM_GAMECUBE,
        "cue" | "m3u" => platforms::PLATFORM_PS1,
        _ => 0,
    }
}

fn is_chd_platform(pid: u16) -> bool {
    matches!(
        pid,
        platforms::PLATFORM_PS1
            | platforms::PLATFORM_PS2
            | platforms::PLATFORM_PSP
            | platforms::PLATFORM_SATURN
            | platforms::PLATFORM_DREAMCAST
            | platforms::PLATFORM_PC_ENGINE
    )
}

fn is_rvz_platform(pid: u16) -> bool {
    matches!(pid, platforms::PLATFORM_GAMECUBE | platforms::PLATFORM_WII)
}

fn collect_candidate_for_entry(entry: &crate::GameEntry) -> Option<RomShrinkCandidate> {
    let raw_exe = entry.exe_path.trim();
    if raw_exe.is_empty() {
        return None;
    }
    let path = PathBuf::from(raw_exe);
    if !path.is_file() {
        return None;
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(ext.as_str(), "chd" | "rvz" | "exe" | "lnk" | "url" | "bat" | "cmd" | "wbfs" | "cso" | "pbp") {
        return None;
    }

    let pid = resolve_game_platform_id(entry, &path);
    let mut tracked_files = Vec::new();

    let (target_format, source_bytes) = match ext.as_str() {
        "cue" => {
            let cue = parse_cue_sheet(&path).ok()?;
            if pid != 0 && !is_chd_platform(pid) && !is_rvz_platform(pid) {
                return None;
            }
            tracked_files.push(path.to_string_lossy().into_owned());
            for f in &cue.files {
                let s = f.path.to_string_lossy().into_owned();
                if !tracked_files.contains(&s) {
                    tracked_files.push(s);
                }
            }
            (TargetKind::Chd, cue.total_bytes)
        }
        "m3u" => {
            let entries = resolve_m3u_entries(&path).ok()?;
            if entries.len() != 1 {
                return None;
            }
            let inner = &entries[0];
            let inner_ext = inner
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            tracked_files.push(path.to_string_lossy().into_owned());
            if inner_ext == "cue" {
                let cue = parse_cue_sheet(inner).ok()?;
                tracked_files.push(inner.to_string_lossy().into_owned());
                for f in &cue.files {
                    let s = f.path.to_string_lossy().into_owned();
                    if !tracked_files.contains(&s) {
                        tracked_files.push(s);
                    }
                }
                (TargetKind::Chd, cue.total_bytes)
            } else if inner_ext == "iso" || inner_ext == "gcm" {
                let (kind, _) = detect_iso_target_and_unit_bytes(inner, Some(pid)).ok()?;
                let bytes = fs::metadata(inner).ok()?.len();
                tracked_files.push(inner.to_string_lossy().into_owned());
                (kind, bytes)
            } else {
                return None;
            }
        }
        "iso" | "gcm" => {
            let (kind, _) = detect_iso_target_and_unit_bytes(&path, Some(pid)).ok()?;
            if pid != 0 && !is_chd_platform(pid) && !is_rvz_platform(pid) {
                return None;
            }
            let bytes = fs::metadata(&path).ok()?.len();
            tracked_files.push(path.to_string_lossy().into_owned());
            (kind, bytes)
        }
        "bin" => {
            if !is_chd_platform(pid) {
                return None;
            }
            let sibling_cue = path.with_extension("cue");
            if sibling_cue.is_file() {
                if let Ok(cue) = parse_cue_sheet(&sibling_cue) {
                    tracked_files.push(sibling_cue.to_string_lossy().into_owned());
                    for f in &cue.files {
                        let s = f.path.to_string_lossy().into_owned();
                        if !tracked_files.contains(&s) {
                            tracked_files.push(s);
                        }
                    }
                    return Some(RomShrinkCandidate {
                        game_id: entry.id.clone(),
                        title: entry.title.clone(),
                        platform_slug: platforms::platform_by_id(pid).slug.to_string(),
                        source_path: sibling_cue.to_string_lossy().into_owned(),
                        target_format: "chd".to_string(),
                        source_bytes: cue.total_bytes,
                        tracked_files,
                    });
                }
            }
            if !is_raw_cd_bin_image(&path) {
                return None;
            }
            let bytes = fs::metadata(&path).ok()?.len();
            tracked_files.push(path.to_string_lossy().into_owned());
            (TargetKind::Chd, bytes)
        }
        _ => return None,
    };

    let effective_pid = if pid != 0 {
        pid
    } else if matches!(target_format, TargetKind::Rvz) {
        platforms::PLATFORM_GAMECUBE
    } else {
        platforms::PLATFORM_PS1
    };

    Some(RomShrinkCandidate {
        game_id: entry.id.clone(),
        title: entry.title.clone(),
        platform_slug: platforms::platform_by_id(effective_pid).slug.to_string(),
        source_path: raw_exe.to_string(),
        target_format: target_format.as_str().to_string(),
        source_bytes,
        tracked_files,
    })
}

// ============================================================================
// Public Crate Contract
// ============================================================================

pub(crate) fn scan_shrinkable_retro_library(library: &[crate::GameEntry]) -> Vec<RomShrinkCandidate> {
    library
        .iter()
        .filter_map(collect_candidate_for_entry)
        .collect()
}

pub(crate) fn verify_chd_or_rvz_integrity(archive_path: &Path) -> Result<(String, String, u64), String> {
    let mut f = File::open(archive_path)
        .map_err(|e| format!("Failed opening archive {}: {e}", archive_path.display()))?;
    let mut magic = [0u8; 8];
    f.read_exact(&mut magic)
        .map_err(|e| format!("Failed reading archive magic {}: {e}", archive_path.display()))?;
    drop(f);

    if &magic == CHD_MAGIC {
        verify_chd_v5(archive_path)
    } else if &magic[0..4] == RVZ_MAGIC {
        verify_rvz(archive_path)
    } else {
        Err(format!(
            "Unrecognized archive format in {} (expected MComprHD or RVZ\\x01)",
            archive_path.display()
        ))
    }
}

pub(crate) fn compress_rom_path(source_path: &Path, delete_originals: bool) -> Result<RomShrinkReport, String> {
    if !source_path.is_file() {
        return Err(format!("Source ROM path does not exist: {}", source_path.display()));
    }

    let ext = source_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let (actual_source, cleanup_extra) = if ext == "m3u" {
        let targets = resolve_m3u_entries(source_path)?;
        if targets.len() != 1 {
            return Err(format!(
                "Multi-disc M3U {} has {} entries; compress individual .cue/.iso discs",
                source_path.display(),
                targets.len()
            ));
        }
        (targets[0].clone(), Some(source_path.to_path_buf()))
    } else {
        (source_path.to_path_buf(), None)
    };

    let actual_ext = actual_source
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match actual_ext.as_str() {
        "chd" | "rvz" => Err(format!("Source is already compressed: {}", actual_source.display())),
        "cue" => {
            let cue = parse_cue_sheet(&actual_source)?;
            let out_chd = actual_source.with_extension("chd");
            let input_files: Vec<PathBuf> = cue.files.iter().map(|f| f.path.clone()).collect();
            let metadata = build_cht2_metadata(&cue);
            let (sha1_hex, crc32_hex, compressed_bytes) = write_chd_v5_from_stream(
                &input_files,
                cue.total_bytes,
                cue.unit_bytes,
                &metadata,
                &out_chd,
            )?;

            if delete_originals {
                let mut seen = HashSet::new();
                for f in &input_files {
                    if *f != out_chd && seen.insert(f.clone()) {
                        let _ = fs::remove_file(f);
                    }
                }
                if actual_source != out_chd {
                    let _ = fs::remove_file(&actual_source);
                }
                if let Some(m3u) = &cleanup_extra {
                    if *m3u != out_chd {
                        let _ = fs::remove_file(m3u);
                    }
                }
            }

            let saved_bytes = cue.total_bytes.saturating_sub(compressed_bytes);
            Ok(RomShrinkReport {
                game_id: String::new(),
                source_path: source_path.to_string_lossy().into_owned(),
                output_path: out_chd.to_string_lossy().into_owned(),
                format: "chd".to_string(),
                original_bytes: cue.total_bytes,
                compressed_bytes,
                saved_bytes,
                ratio_pct: round_ratio_pct(compressed_bytes, cue.total_bytes),
                sha1_hex,
                crc32_hex,
                verified: true,
            })
        }
        "iso" | "gcm" => {
            let (kind, unit_bytes) = detect_iso_target_and_unit_bytes(&actual_source, None)?;
            match kind {
                TargetKind::Rvz => {
                    let out_rvz = actual_source.with_extension("rvz");
                    let (sha1_hex, crc32_hex, original_bytes, compressed_bytes) =
                        write_rvz_from_iso(&actual_source, &out_rvz)?;
                    if delete_originals {
                        if actual_source != out_rvz {
                            let _ = fs::remove_file(&actual_source);
                        }
                        if let Some(m3u) = &cleanup_extra {
                            if *m3u != out_rvz {
                                let _ = fs::remove_file(m3u);
                            }
                        }
                    }
                    let saved_bytes = original_bytes.saturating_sub(compressed_bytes);
                    Ok(RomShrinkReport {
                        game_id: String::new(),
                        source_path: source_path.to_string_lossy().into_owned(),
                        output_path: out_rvz.to_string_lossy().into_owned(),
                        format: "rvz".to_string(),
                        original_bytes,
                        compressed_bytes,
                        saved_bytes,
                        ratio_pct: round_ratio_pct(compressed_bytes, original_bytes),
                        sha1_hex,
                        crc32_hex,
                        verified: true,
                    })
                }
                TargetKind::Chd => {
                    let original_bytes = fs::metadata(&actual_source)
                        .map_err(|e| format!("Failed reading ISO metadata: {e}"))?
                        .len();
                    let out_chd = actual_source.with_extension("chd");
                    let (sha1_hex, crc32_hex, compressed_bytes) = write_chd_v5_from_stream(
                        std::slice::from_ref(&actual_source),
                        original_bytes,
                        unit_bytes,
                        &[],
                        &out_chd,
                    )?;
                    if delete_originals {
                        if actual_source != out_chd {
                            let _ = fs::remove_file(&actual_source);
                        }
                        if let Some(m3u) = &cleanup_extra {
                            if *m3u != out_chd {
                                let _ = fs::remove_file(m3u);
                            }
                        }
                    }
                    let saved_bytes = original_bytes.saturating_sub(compressed_bytes);
                    Ok(RomShrinkReport {
                        game_id: String::new(),
                        source_path: source_path.to_string_lossy().into_owned(),
                        output_path: out_chd.to_string_lossy().into_owned(),
                        format: "chd".to_string(),
                        original_bytes,
                        compressed_bytes,
                        saved_bytes,
                        ratio_pct: round_ratio_pct(compressed_bytes, original_bytes),
                        sha1_hex,
                        crc32_hex,
                        verified: true,
                    })
                }
            }
        }
        "bin" => {
            let sibling_cue = actual_source.with_extension("cue");
            if sibling_cue.is_file() {
                return compress_rom_path(&sibling_cue, delete_originals);
            }
            if !is_raw_cd_bin_image(&actual_source) {
                return Err(format!(
                    "Refusing to compress non-disc .bin file without CUE/CD sync header: {}",
                    actual_source.display()
                ));
            }
            let original_bytes = fs::metadata(&actual_source)
                .map_err(|e| format!("Failed reading BIN metadata: {e}"))?
                .len();
            let (_, unit_bytes) = detect_iso_target_and_unit_bytes(&actual_source, None)?;
            let out_chd = actual_source.with_extension("chd");
            let (sha1_hex, crc32_hex, compressed_bytes) = write_chd_v5_from_stream(
                std::slice::from_ref(&actual_source),
                original_bytes,
                unit_bytes,
                &[],
                &out_chd,
            )?;
            if delete_originals && actual_source != out_chd {
                let _ = fs::remove_file(&actual_source);
            }
            let saved_bytes = original_bytes.saturating_sub(compressed_bytes);
            Ok(RomShrinkReport {
                game_id: String::new(),
                source_path: source_path.to_string_lossy().into_owned(),
                output_path: out_chd.to_string_lossy().into_owned(),
                format: "chd".to_string(),
                original_bytes,
                compressed_bytes,
                saved_bytes,
                ratio_pct: round_ratio_pct(compressed_bytes, original_bytes),
                sha1_hex,
                crc32_hex,
                verified: true,
            })
        }
        other => Err(format!("Unsupported source ROM extension '.{other}'")),
    }
}

pub(crate) fn compress_retro_library_sync(
    library: &mut [crate::GameEntry],
    game_id: Option<&str>,
    delete_originals: bool,
) -> Result<BatchRomShrinkReport, String> {
    if let Some(target_id) = game_id {
        if !library.iter().any(|g| g.id == target_id) {
            return Err(format!("Game '{target_id}' not found in library"));
        }
    }

    let mut report = BatchRomShrinkReport {
        converted_count: 0,
        skipped_count: 0,
        total_original_bytes: 0,
        total_compressed_bytes: 0,
        total_saved_bytes: 0,
        items: Vec::new(),
        errors: Vec::new(),
    };

    for entry in library.iter_mut() {
        if let Some(target_id) = game_id {
            if entry.id != target_id {
                continue;
            }
        }
        let Some(candidate) = collect_candidate_for_entry(entry) else {
            report.skipped_count += 1;
            continue;
        };

        let src_path = PathBuf::from(&candidate.source_path);
        match compress_rom_path(&src_path, delete_originals) {
            Ok(mut item) => {
                item.game_id = entry.id.clone();
                entry.exe_path = item.output_path.clone();
                entry.size_bytes = item.compressed_bytes;
                entry.size_formatted = crate::format_bytes(item.compressed_bytes);

                report.converted_count += 1;
                report.total_original_bytes = report
                    .total_original_bytes
                    .saturating_add(item.original_bytes);
                report.total_compressed_bytes = report
                    .total_compressed_bytes
                    .saturating_add(item.compressed_bytes);
                report.total_saved_bytes = report
                    .total_saved_bytes
                    .saturating_add(item.saved_bytes);
                report.items.push(item);
            }
            Err(err) => {
                report.skipped_count += 1;
                report.errors.push(format!("{}: {err}", entry.title));
            }
        }
    }

    Ok(report)
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "duke_rom_shrink_{}_{}_{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn multitrack_cue_bin_to_chd_roundtrip_and_library_update() {
        let dir = make_temp_dir("chd_cue");

        // Build Track 1 (Mode2/2352, 24 sectors) + Track 2 (Audio/2352, 16 sectors)
        let track1_sectors = 24usize;
        let track2_sectors = 16usize;
        let mut t1 = vec![0u8; track1_sectors * 2352];
        for s in 0..track1_sectors {
            let off = s * 2352;
            t1[off..off + 12]
                .copy_from_slice(&[0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00]);
            t1[off + 15] = 0x02; // Mode 2
            t1[off + 24..off + 32].copy_from_slice(b"DUKEPS1!");
        }
        // Inject ISO9660 PVD signature in sector 16 user data (+24 in Mode2/2352)
        let pvd_off = 16 * 2352 + 24;
        t1[pvd_off..pvd_off + 6].copy_from_slice(b"\x01CD001");
        t1[pvd_off + 8..pvd_off + 19].copy_from_slice(b"PLAYSTATION");

        let mut t2 = vec![0u8; track2_sectors * 2352];
        for (i, b) in t2.iter_mut().enumerate() {
            *b = ((i * 7) & 0xFF) as u8;
        }

        let t1_path = dir.join("TestGame (Track 1).bin");
        let t2_path = dir.join("TestGame (Track 2).bin");
        fs::write(&t1_path, &t1).unwrap();
        fs::write(&t2_path, &t2).unwrap();

        let cue_path = dir.join("TestGame (PS1).cue");
        let cue_text = concat!(
            "FILE \"TestGame (Track 1).bin\" BINARY\n",
            "  TRACK 01 MODE2/2352\n",
            "    INDEX 01 00:00:00\n",
            "FILE \"TestGame (Track 2).bin\" BINARY\n",
            "  TRACK 02 AUDIO\n",
            "    INDEX 00 00:00:00\n",
            "    INDEX 01 00:02:00\n"
        );
        fs::write(&cue_path, cue_text).unwrap();

        // Compute expected raw stream SHA-1 and CRC32
        let mut expected_sha1 = Sha1::new();
        expected_sha1.update(&t1);
        expected_sha1.update(&t2);
        let expected_sha1_hex = hex::encode(expected_sha1.finalize());

        let mut expected_crc = Crc32Hasher::new();
        expected_crc.update(&t1);
        expected_crc.update(&t2);
        let expected_crc_hex = format!("{:08x}", expected_crc.finalize());
        let total_raw_len = (t1.len() + t2.len()) as u64;

        let mut lib = vec![crate::GameEntry {
            id: "ps1_test_1".to_string(),
            title: "TestGame".to_string(),
            exe_path: cue_path.to_string_lossy().into_owned(),
            install_dir: dir.to_string_lossy().into_owned(),
            size_bytes: total_raw_len,
            size_formatted: crate::format_bytes(total_raw_len),
            platform_id: platforms::PLATFORM_PS1,
            ..Default::default()
        }];

        let candidates = scan_shrinkable_retro_library(&lib);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].target_format, "chd");
        assert_eq!(candidates[0].tracked_files.len(), 3);

        let batch = compress_retro_library_sync(&mut lib, None, true).unwrap();
        assert_eq!(batch.converted_count, 1);
        assert_eq!(batch.errors.len(), 0);
        let report = &batch.items[0];
        assert!(report.verified);
        assert_eq!(report.format, "chd");
        assert_eq!(report.sha1_hex, expected_sha1_hex);
        assert_eq!(report.crc32_hex, expected_crc_hex);
        assert_eq!(report.original_bytes, total_raw_len);
        assert!(report.compressed_bytes < report.original_bytes);

        // Originals deleted cleanly and library entry updated
        assert!(!cue_path.exists());
        assert!(!t1_path.exists());
        assert!(!t2_path.exists());
        let chd_path = PathBuf::from(&report.output_path);
        assert!(chd_path.is_file());
        assert_eq!(lib[0].exe_path, report.output_path);
        assert_eq!(lib[0].size_bytes, report.compressed_bytes);
        assert_eq!(lib[0].size_formatted, crate::format_bytes(report.compressed_bytes));

        // Verify pure-Rust CHD integrity & CHT2 metadata tags
        let (v_sha1, v_crc32, v_len) = verify_chd_or_rvz_integrity(&chd_path).unwrap();
        assert_eq!(v_sha1, expected_sha1_hex);
        assert_eq!(v_crc32, expected_crc_hex);
        assert_eq!(v_len, total_raw_len);

        let chd_bytes = fs::read(&chd_path).unwrap();
        assert_eq!(&chd_bytes[0..8], b"MComprHD");
        assert_eq!(u32::from_be_bytes(chd_bytes[8..12].try_into().unwrap()), 124);
        assert_eq!(u32::from_be_bytes(chd_bytes[12..16].try_into().unwrap()), 5);
        assert_eq!(&chd_bytes[16..20], b"zstd");
        let meta_off = u64::from_be_bytes(chd_bytes[0x30..0x38].try_into().unwrap()) as usize;
        assert_eq!(meta_off, 124);
        assert_eq!(&chd_bytes[meta_off..meta_off + 4], b"CHT2");
        let meta_txt = String::from_utf8_lossy(&chd_bytes[meta_off + 16..meta_off + 160]);
        assert!(meta_txt.contains("TRACK:1 TYPE:MODE2_RAW"));

        // Verify rom_inspect compatibility
        let inspected = rom_inspect::inspect_rom_file(&chd_path).unwrap();
        assert_eq!(inspected.format_label, "CHD v5");
        assert_eq!(inspected.platform_id, platforms::PLATFORM_PS1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn gamecube_wii_iso_to_rvz_roundtrip_and_rom_inspect() {
        let dir = make_temp_dir("rvz_gc");
        let iso_len = 512 * 1024; // 4 chunks of 128 KiB
        let mut iso = vec![0u8; iso_len];
        iso[0x00..0x06].copy_from_slice(b"GM8E01");
        iso[0x1C..0x20].copy_from_slice(&GC_MAGIC);
        iso[0x20..0x2D].copy_from_slice(b"Metroid Prime");
        for (idx, chunk) in iso[0x80..].chunks_mut(256).enumerate() {
            chunk[0..4].copy_from_slice(&(idx as u32).to_be_bytes());
            chunk[4..16].copy_from_slice(b"GAMECUBE_DAT");
        }

        let iso_path = dir.join("Metroid Prime.iso");
        fs::write(&iso_path, &iso).unwrap();

        let expected_sha1 = hex::encode(Sha1::digest(&iso));
        let mut crc = Crc32Hasher::new();
        crc.update(&iso);
        let expected_crc = format!("{:08x}", crc.finalize());

        let report = compress_rom_path(&iso_path, false).unwrap();
        assert!(report.verified);
        assert_eq!(report.format, "rvz");
        assert_eq!(report.sha1_hex, expected_sha1);
        assert_eq!(report.crc32_hex, expected_crc);
        assert_eq!(report.original_bytes, iso_len as u64);
        assert!(report.compressed_bytes < report.original_bytes);

        let rvz_path = PathBuf::from(&report.output_path);
        let (v_sha1, v_crc, v_len) = verify_chd_or_rvz_integrity(&rvz_path).unwrap();
        assert_eq!(v_sha1, expected_sha1);
        assert_eq!(v_crc, expected_crc);
        assert_eq!(v_len, iso_len as u64);

        let inspected = rom_inspect::inspect_rom_file(&rvz_path).unwrap();
        assert_eq!(inspected.format_label, "Dolphin RVZ");
        assert_eq!(inspected.platform_id, platforms::PLATFORM_GAMECUBE);
        assert_eq!(inspected.rom_id, "GM8E01");
        assert_eq!(inspected.internal_title, "Metroid Prime");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn tampering_chd_and_rvz_hunks_detected_cleanly() {
        let dir = make_temp_dir("tamper");

        // 1. Corrupt CHD hunk payload
        let iso_ps2_path = dir.join("ps2_disc.iso");
        let mut ps2_iso = vec![0u8; 128 * 1024];
        ps2_iso[0x8000..0x8006].copy_from_slice(b"\x01CD001");
        ps2_iso[0x8008..0x8013].copy_from_slice(b"PLAYSTATION");
        for (i, b) in ps2_iso.iter_mut().enumerate() {
            if i >= 0x9000 {
                *b = (i % 29) as u8;
            }
        }
        fs::write(&iso_ps2_path, &ps2_iso).unwrap();
        let chd_rep = compress_rom_path(&iso_ps2_path, false).unwrap();
        let chd_path = PathBuf::from(&chd_rep.output_path);

        let mut chd_data = fs::read(&chd_path).unwrap();
        // Flip a bit inside the compressed hunk area (right after 124B header)
        chd_data[140] ^= 0x55;
        let corrupt_chd = dir.join("corrupt.chd");
        fs::write(&corrupt_chd, &chd_data).unwrap();
        assert!(verify_chd_or_rvz_integrity(&corrupt_chd).is_err());

        // 2. Corrupt RVZ chunk payload (not header!)
        let wii_iso_path = dir.join("galaxy.iso");
        let mut wii_iso = vec![0u8; 320 * 1024];
        wii_iso[0x00..0x06].copy_from_slice(b"RMGE01");
        wii_iso[0x18..0x1C].copy_from_slice(&WII_MAGIC);
        wii_iso[0x20..0x32].copy_from_slice(b"Super Mario Galaxy");
        for (i, b) in wii_iso[0x100..].iter_mut().enumerate() {
            *b = ((i / 16) % 251) as u8;
        }
        fs::write(&wii_iso_path, &wii_iso).unwrap();
        let rvz_rep = compress_rom_path(&wii_iso_path, false).unwrap();
        let rvz_path = PathBuf::from(&rvz_rep.output_path);

        let mut rvz_data = fs::read(&rvz_path).unwrap();
        // Flip bit inside chunk 0 payload (past 72 + 192 = 264 header)
        rvz_data[275] ^= 0x80;
        let corrupt_rvz = dir.join("corrupt.rvz");
        fs::write(&corrupt_rvz, &rvz_data).unwrap();
        assert!(verify_chd_or_rvz_integrity(&corrupt_rvz).is_err());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cue_path_traversal_and_m3u_safety_enforced() {
        let dir = make_temp_dir("safety");
        let evil_cue = dir.join("evil.cue");
        fs::write(&evil_cue, "FILE \"../secret.bin\" BINARY\n  TRACK 01 MODE1/2048\n    INDEX 01 00:00:00\n").unwrap();
        assert!(compress_rom_path(&evil_cue, false).is_err());

        let inner_iso = dir.join("disc1.iso");
        let mut data = vec![0u8; 64 * 1024];
        data[0x8000..0x8006].copy_from_slice(b"\x01CD001");
        data[0x8008..0x8013].copy_from_slice(b"PLAYSTATION");
        let mut f = File::create(&inner_iso).unwrap();
        f.write_all(&data).unwrap();
        let m3u_path = dir.join("game.m3u");
        fs::write(&m3u_path, "# comment\ndisc1.iso\n").unwrap();

        let rep = compress_rom_path(&m3u_path, false).unwrap();
        assert_eq!(rep.format, "chd");
        assert!(Path::new(&rep.output_path).is_file());

        let _ = fs::remove_dir_all(&dir);
    }
}

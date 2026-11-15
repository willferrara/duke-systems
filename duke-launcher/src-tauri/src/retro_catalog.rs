//! Public Libretro RDB binary catalog harvester & Libretro Thumbnails resolver.
//!
//! Fetches official open-source `.rdb` (`"RARCHDB\0"` MessagePack) databases and `.dat`
//! fallbacks via `duke-core` DoH using a 4-permit semaphore, progressively flushing 20,000+
//! Nintendo and retro console games into memory-mapped `catalog.dkcat` (`DKCAT002`) with
//! zero idle heap allocation.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::sync::Semaphore;

use crate::catalog_mmap;
use crate::platforms::{self, PlatformSpec, PLATFORMS};
use crate::CatalogGameEntry;

const RDB_RAW_BASE: &str =
    "https://raw.githubusercontent.com/libretro/libretro-database/master/rdb";
const DAT_RAW_BASE: &str =
    "https://raw.githubusercontent.com/libretro/libretro-database/master";
pub(crate) const LIBRETRO_THUMBS_BASE: &str = "https://thumbnails.libretro.com";
const RETRO_SYNC_TTL_SECS: u64 = 7 * 24 * 60 * 60;
const RETRO_HARVEST_CONCURRENCY: usize = 4;

static RETRO_HARVEST_RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RetroSyncStatus {
    pub running: bool,
    pub completed_platforms: usize,
    pub total_platforms: usize,
    pub current_platform: String,
    pub entries_added: usize,
    pub total_catalog_entries: usize,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct RdbGameRecord {
    pub raw_name: String,
    pub display_name: String,
    pub description: String,
    pub genre: String,
    pub developer: String,
    pub publisher: String,
    pub franchise: String,
    pub serial: String,
    pub crc_hex: String,
    pub release_year: u32,
    pub max_users: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct RetroSyncStateFile {
    pub last_platform_sync: HashMap<String, u64>,
}

fn load_retro_sync_state() -> RetroSyncStateFile {
    let Ok(path) = crate::get_app_data_file_standalone("retro_sync_state.json") else {
        return RetroSyncStateFile::default();
    };
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_retro_sync_state(state: &RetroSyncStateFile) {
    if let Ok(path) = crate::get_app_data_file_standalone("retro_sync_state.json") {
        if let Ok(json) = serde_json::to_string_pretty(state) {
            let _ = fs::write(path, json);
        }
    }
}

/// Sanitizes a database title per Libretro Thumbnail naming specification
/// (`&*/:<>?\|"` replaced with `_`) and builds `Named_Boxarts` + `Named_Titles` + `Named_Snaps` URLs.
pub fn sanitize_libretro_thumb_name(raw_name: &str) -> String {
    raw_name
        .trim()
        .chars()
        .map(|c| match c {
            '&' | '*' | '/' | ':' | '<' | '>' | '?' | '\\' | '|' | '"' => '_',
            _ => c,
        })
        .collect()
}

pub fn build_libretro_thumbnail_urls(
    platform_id: u16,
    raw_title: &str,
) -> (String, String, String) {
    let spec = platforms::platform_by_id(platform_id);
    let Some(sys_folder) = spec.thumb_system else {
        return (String::new(), String::new(), String::new());
    };
    let clean_name = sanitize_libretro_thumb_name(raw_title);
    if clean_name.is_empty() {
        return (String::new(), String::new(), String::new());
    }
    let sys_enc = urlencoding::encode(sys_folder);
    let file_enc = urlencoding::encode(&clean_name);
    let boxart = format!(
        "{}/{}/Named_Boxarts/{}.png",
        LIBRETRO_THUMBS_BASE, sys_enc, file_enc
    );
    let title_screen = format!(
        "{}/{}/Named_Titles/{}.png",
        LIBRETRO_THUMBS_BASE, sys_enc, file_enc
    );
    let snap = format!(
        "{}/{}/Named_Snaps/{}.png",
        LIBRETRO_THUMBS_BASE, sys_enc, file_enc
    );
    (boxart, title_screen, snap)
}

pub fn normalize_retro_base_title(raw_name: &str) -> String {
    let mut out = String::with_capacity(raw_name.len());
    let mut depth_paren = 0usize;
    let mut depth_bracket = 0usize;
    for ch in raw_name.chars() {
        match ch {
            '(' => depth_paren += 1,
            ')' => depth_paren = depth_paren.saturating_sub(1),
            '[' => depth_bracket += 1,
            ']' => depth_bracket = depth_bracket.saturating_sub(1),
            _ if depth_paren == 0 && depth_bracket == 0 => out.push(ch),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn region_priority_score(raw_name: &str) -> i32 {
    let lower = raw_name.to_ascii_lowercase();
    let mut score = 0i32;
    if lower.contains("(usa)") || lower.contains("(usa, europe)") {
        score += 100;
    } else if lower.contains("(world)") {
        score += 95;
    } else if lower.contains("(europe)") {
        score += 80;
    } else if lower.contains("(japan)") {
        score += 55;
    } else if lower.contains("usa") {
        score += 70;
    }

    if lower.contains("(en)") || lower.contains("(en,") || lower.contains(",en)") {
        score += 20;
    }
    if lower.contains("(beta")
        || lower.contains("(proto")
        || lower.contains("(demo")
        || lower.contains("(sample")
        || lower.contains("(kiosk")
        || lower.contains("(unl)")
        || lower.contains("(pirate)")
        || lower.contains("[b")
    {
        score -= 200;
    }
    if lower.contains("(rev ") {
        score -= 5;
    }
    if lower.contains("(virtual console)") || lower.contains("(switch online)") {
        score -= 15;
    }
    score
}

#[derive(Debug, Clone)]
enum MsgPackVal {
    Nil,
    Bool(#[allow(dead_code)] bool),
    Int(i64),
    Uint(u64),
    Float(#[allow(dead_code)] f64),
    Str(String),
    Bin(Vec<u8>),
    ArrayLen(usize),
    MapLen(usize),
}

struct CursorReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> CursorReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn read_u8(&mut self) -> Result<u8, String> {
        let b = *self
            .data
            .get(self.pos)
            .ok_or_else(|| "Unexpected EOF reading RDB byte".to_string())?;
        self.pos += 1;
        Ok(b)
    }

    fn read_u16_be(&mut self) -> Result<u16, String> {
        if self.remaining() < 2 {
            return Err("Unexpected EOF reading u16".to_string());
        }
        let v = u16::from_be_bytes([self.data[self.pos], self.data[self.pos + 1]]);
        self.pos += 2;
        Ok(v)
    }

    fn read_u32_be(&mut self) -> Result<u32, String> {
        if self.remaining() < 4 {
            return Err("Unexpected EOF reading u32".to_string());
        }
        let b = &self.data[self.pos..self.pos + 4];
        let v = u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
        self.pos += 4;
        Ok(v)
    }

    fn read_u64_be(&mut self) -> Result<u64, String> {
        if self.remaining() < 8 {
            return Err("Unexpected EOF reading u64".to_string());
        }
        let b = &self.data[self.pos..self.pos + 8];
        let v = u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]);
        self.pos += 8;
        Ok(v)
    }

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], String> {
        let end = self
            .pos
            .checked_add(len)
            .filter(|&e| e <= self.data.len())
            .ok_or_else(|| "Unexpected EOF reading RDB slice".to_string())?;
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn read_value(&mut self, capture_bin: bool) -> Result<MsgPackVal, String> {
        let tag = self.read_u8()?;
        match tag {
            0x00..=0x7f => Ok(MsgPackVal::Uint(u64::from(tag))),
            0x80..=0x8f => Ok(MsgPackVal::MapLen((tag & 0x0f) as usize)),
            0x90..=0x9f => Ok(MsgPackVal::ArrayLen((tag & 0x0f) as usize)),
            0xa0..=0xbf => {
                let len = (tag & 0x1f) as usize;
                let s = String::from_utf8_lossy(self.read_bytes(len)?).into_owned();
                Ok(MsgPackVal::Str(s))
            }
            0xc0 => Ok(MsgPackVal::Nil),
            0xc2 => Ok(MsgPackVal::Bool(false)),
            0xc3 => Ok(MsgPackVal::Bool(true)),
            0xc4 => {
                let len = self.read_u8()? as usize;
                let bytes = self.read_bytes(len)?;
                Ok(MsgPackVal::Bin(if capture_bin {
                    bytes.to_vec()
                } else {
                    Vec::new()
                }))
            }
            0xc5 => {
                let len = self.read_u16_be()? as usize;
                let bytes = self.read_bytes(len)?;
                Ok(MsgPackVal::Bin(if capture_bin {
                    bytes.to_vec()
                } else {
                    Vec::new()
                }))
            }
            0xc6 => {
                let len = self.read_u32_be()? as usize;
                let bytes = self.read_bytes(len)?;
                Ok(MsgPackVal::Bin(if capture_bin {
                    bytes.to_vec()
                } else {
                    Vec::new()
                }))
            }
            0xca => {
                let b = self.read_bytes(4)?;
                Ok(MsgPackVal::Float(f64::from(f32::from_be_bytes([
                    b[0], b[1], b[2], b[3],
                ]))))
            }
            0xcb => {
                let b = self.read_bytes(8)?;
                Ok(MsgPackVal::Float(f64::from_be_bytes([
                    b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
                ])))
            }
            0xcc => Ok(MsgPackVal::Uint(u64::from(self.read_u8()?))),
            0xcd => Ok(MsgPackVal::Uint(u64::from(self.read_u16_be()?))),
            0xce => Ok(MsgPackVal::Uint(u64::from(self.read_u32_be()?))),
            0xcf => Ok(MsgPackVal::Uint(self.read_u64_be()?)),
            0xd0 => Ok(MsgPackVal::Int(i64::from(self.read_u8()? as i8))),
            0xd1 => Ok(MsgPackVal::Int(i64::from(self.read_u16_be()? as i16))),
            0xd2 => Ok(MsgPackVal::Int(i64::from(self.read_u32_be()? as i32))),
            0xd3 => Ok(MsgPackVal::Int(self.read_u64_be()? as i64)),
            0xd4 => {
                let _ = self.read_bytes(2)?;
                Ok(MsgPackVal::Nil)
            }
            0xd5 => {
                let _ = self.read_bytes(3)?;
                Ok(MsgPackVal::Nil)
            }
            0xd6 => {
                let _ = self.read_bytes(5)?;
                Ok(MsgPackVal::Nil)
            }
            0xd7 => {
                let _ = self.read_bytes(9)?;
                Ok(MsgPackVal::Nil)
            }
            0xd8 => {
                let _ = self.read_bytes(17)?;
                Ok(MsgPackVal::Nil)
            }
            0xd9 => {
                let len = self.read_u8()? as usize;
                let s = String::from_utf8_lossy(self.read_bytes(len)?).into_owned();
                Ok(MsgPackVal::Str(s))
            }
            0xda => {
                let len = self.read_u16_be()? as usize;
                let s = String::from_utf8_lossy(self.read_bytes(len)?).into_owned();
                Ok(MsgPackVal::Str(s))
            }
            0xdb => {
                let len = self.read_u32_be()? as usize;
                let s = String::from_utf8_lossy(self.read_bytes(len)?).into_owned();
                Ok(MsgPackVal::Str(s))
            }
            0xdc => Ok(MsgPackVal::ArrayLen(self.read_u16_be()? as usize)),
            0xdd => Ok(MsgPackVal::ArrayLen(self.read_u32_be()? as usize)),
            0xde => Ok(MsgPackVal::MapLen(self.read_u16_be()? as usize)),
            0xdf => Ok(MsgPackVal::MapLen(self.read_u32_be()? as usize)),
            0xe0..=0xff => Ok(MsgPackVal::Int(i64::from(tag as i8))),
            _ => Err(format!("Unsupported MessagePack tag 0x{:02x}", tag)),
        }
    }

    fn skip_nested(&mut self, val: &MsgPackVal, depth: usize) -> Result<(), String> {
        if depth > 16 {
            return Err("MessagePack nesting depth exceeded".to_string());
        }
        match val {
            MsgPackVal::ArrayLen(len) => {
                for _ in 0..*len {
                    let v = self.read_value(false)?;
                    self.skip_nested(&v, depth + 1)?;
                }
            }
            MsgPackVal::MapLen(len) => {
                for _ in 0..*len {
                    let k = self.read_value(false)?;
                    self.skip_nested(&k, depth + 1)?;
                    let v = self.read_value(false)?;
                    self.skip_nested(&v, depth + 1)?;
                }
            }
            MsgPackVal::Nil
            | MsgPackVal::Bool(_)
            | MsgPackVal::Int(_)
            | MsgPackVal::Uint(_)
            | MsgPackVal::Float(_)
            | MsgPackVal::Str(_)
            | MsgPackVal::Bin(_) => {}
        }
        Ok(())
    }
}

fn msgpack_val_as_string(val: MsgPackVal) -> String {
    match val {
        MsgPackVal::Str(s) => s.trim().to_string(),
        MsgPackVal::Bin(b) => String::from_utf8_lossy(&b).trim().to_string(),
        MsgPackVal::Uint(u) => u.to_string(),
        MsgPackVal::Int(i) => i.to_string(),
        _ => String::new(),
    }
}

fn msgpack_val_as_u32(val: MsgPackVal) -> u32 {
    match val {
        MsgPackVal::Uint(u) => u32::try_from(u).unwrap_or(0),
        MsgPackVal::Int(i) if i >= 0 => u32::try_from(i as u64).unwrap_or(0),
        MsgPackVal::Str(s) => s.trim().parse::<u32>().unwrap_or(0),
        _ => 0,
    }
}

/// Parses an official Libretro `.rdb` (`"RARCHDB\0"`) binary database buffer into structured game records.
pub fn parse_libretro_rdb(bytes: &[u8]) -> Result<Vec<RdbGameRecord>, String> {
    if bytes.len() < 16 || &bytes[0..8] != b"RARCHDB\0" {
        return Err("Not a valid Libretro RDB file (missing RARCHDB\\0 header)".to_string());
    }
    let metadata_offset = u64::from_be_bytes([
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    ]) as usize;
    let end = if metadata_offset >= 16 && metadata_offset <= bytes.len() {
        metadata_offset
    } else {
        bytes.len()
    };

    let mut reader = CursorReader::new(&bytes[16..end]);
    let mut records = Vec::new();

    while reader.remaining() > 0 {
        let head = reader.read_value(false)?;
        let map_len = match head {
            MsgPackVal::Nil => break,
            MsgPackVal::MapLen(n) => n,
            other => {
                reader.skip_nested(&other, 0)?;
                continue;
            }
        };

        let mut rec = RdbGameRecord::default();
        for _ in 0..map_len {
            let key_val = reader.read_value(false)?;
            let key = msgpack_val_as_string(key_val);
            let capture_bin = matches!(key.as_str(), "crc" | "serial" | "name" | "rom_name");
            let val = reader.read_value(capture_bin)?;
            if matches!(val, MsgPackVal::ArrayLen(_) | MsgPackVal::MapLen(_)) {
                reader.skip_nested(&val, 0)?;
                continue;
            }
            match key.as_str() {
                "name" => {
                    let raw = msgpack_val_as_string(val);
                    if !raw.is_empty() {
                        rec.raw_name = raw;
                    }
                }
                "description" => {
                    let s = msgpack_val_as_string(val);
                    if !s.is_empty() {
                        rec.description = s;
                    }
                }
                "genre" => {
                    let s = msgpack_val_as_string(val);
                    if !s.is_empty() {
                        rec.genre = s;
                    }
                }
                "developer" => {
                    let s = msgpack_val_as_string(val);
                    if !s.is_empty() {
                        rec.developer = s;
                    }
                }
                "publisher" => {
                    let s = msgpack_val_as_string(val);
                    if !s.is_empty() {
                        rec.publisher = s;
                    }
                }
                "franchise" => {
                    let s = msgpack_val_as_string(val);
                    if !s.is_empty() {
                        rec.franchise = s;
                    }
                }
                "serial" => {
                    let s = match val {
                        MsgPackVal::Bin(b) => {
                            if b.iter().all(|ch| ch.is_ascii_graphic() || *ch == b' ' || *ch == b'-') {
                                String::from_utf8_lossy(&b).trim().to_string()
                            } else {
                                hex::encode_upper(&b)
                            }
                        }
                        other => msgpack_val_as_string(other),
                    };
                    if !s.is_empty() {
                        rec.serial = s;
                    }
                }
                "crc" => {
                    let s = match val {
                        MsgPackVal::Bin(b) => hex::encode_upper(b),
                        MsgPackVal::Uint(u) => format!("{:08X}", u),
                        MsgPackVal::Str(st) => st.trim().to_ascii_uppercase(),
                        _ => String::new(),
                    };
                    if !s.is_empty() {
                        rec.crc_hex = s;
                    }
                }
                "releaseyear" => {
                    rec.release_year = msgpack_val_as_u32(val);
                }
                "users" => {
                    rec.max_users = msgpack_val_as_u32(val);
                }
                _ => {}
            }
        }

        if rec.raw_name.is_empty() && !rec.description.is_empty() {
            rec.raw_name = rec.description.clone();
        }
        if !rec.raw_name.is_empty() {
            let cleaned = normalize_retro_base_title(&rec.raw_name);
            rec.display_name = if cleaned.is_empty() {
                rec.raw_name.clone()
            } else {
                cleaned
            };
            records.push(rec);
        }
    }

    Ok(records)
}

/// Fallback ClrMamePro `.dat` parser for systems distributed in `libretro-database/metadat/`.
pub fn parse_clrmamepro_dat(text: &str) -> Vec<RdbGameRecord> {
    let mut out = Vec::new();
    let mut in_game = false;
    let mut depth = 0usize;
    let mut cur = RdbGameRecord::default();

    for line in text.lines() {
        let trimmed = line.trim();
        if !in_game {
            if trimmed.starts_with("game (") || trimmed == "game(" {
                in_game = true;
                depth = 1;
                cur = RdbGameRecord::default();
            }
            continue;
        }
        for ch in trimmed.chars() {
            match ch {
                '(' => depth += 1,
                ')' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }

        if let Some(val) = extract_dat_field(trimmed, "name ") {
            if cur.raw_name.is_empty() {
                cur.raw_name = val;
            }
        } else if let Some(val) = extract_dat_field(trimmed, "description ") {
            cur.description = val;
        } else if let Some(val) = extract_dat_field(trimmed, "genre ") {
            cur.genre = val;
        } else if let Some(val) = extract_dat_field(trimmed, "developer ") {
            cur.developer = val;
        } else if let Some(val) = extract_dat_field(trimmed, "publisher ") {
            cur.publisher = val;
        } else if let Some(val) = extract_dat_field(trimmed, "serial ") {
            cur.serial = val;
        } else if let Some(val) = extract_dat_field(trimmed, "releaseyear ") {
            cur.release_year = val.parse::<u32>().unwrap_or(0);
        } else if let Some(val) = extract_dat_field(trimmed, "users ") {
            cur.max_users = val.parse::<u32>().unwrap_or(0);
        } else if trimmed.starts_with("rom (") {
            if let Some(idx) = trimmed.find("crc ") {
                let after = trimmed[idx + 4..].trim_start();
                let tok = after.split_whitespace().next().unwrap_or("");
                if !tok.is_empty() && cur.crc_hex.is_empty() {
                    cur.crc_hex = tok.to_ascii_uppercase();
                }
            }
        }

        if depth == 0 {
            in_game = false;
            if cur.raw_name.is_empty() && !cur.description.is_empty() {
                cur.raw_name = cur.description.clone();
            }
            if !cur.raw_name.is_empty() {
                let cleaned = normalize_retro_base_title(&cur.raw_name);
                cur.display_name = if cleaned.is_empty() {
                    cur.raw_name.clone()
                } else {
                    cleaned
                };
                out.push(cur.clone());
            }
        }
    }
    out
}

fn extract_dat_field(line: &str, prefix: &str) -> Option<String> {
    let rest = line.strip_prefix(prefix)?.trim();
    if let Some(stripped) = rest.strip_prefix('"') {
        let end = stripped.find('"').unwrap_or(stripped.len());
        Some(stripped[..end].trim().to_string())
    } else {
        Some(rest.trim_end_matches(')').trim().to_string())
    }
}

/// Converts parsed `.rdb`/`.dat` records into deduplicated canonical `CatalogGameEntry` rows
/// for `catalog.dkcat` while keeping `raw_name` for exact Libretro Boxart & Title fallback URLs.
pub fn convert_rdb_records_to_catalog(
    spec: &PlatformSpec,
    records: Vec<RdbGameRecord>,
    now: u64,
) -> Vec<CatalogGameEntry> {
    let mut best_by_key: HashMap<String, (i32, RdbGameRecord)> = HashMap::new();

    for rec in records {
        if rec.display_name.trim().is_empty() {
            continue;
        }
        let lower = rec.raw_name.to_ascii_lowercase();
        // Skip noisy non-game demos, betas, protos and diagnostic BIOS images when building catalog
        if lower.contains("(bios)")
            || lower.contains("[bios]")
            || lower.contains("(program)")
            || lower.contains("(test program)")
        {
            continue;
        }
        let key = format!(
            "{}|{}",
            spec.id,
            rec.display_name
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .map(|c| c.to_ascii_lowercase())
                .collect::<String>()
        );
        if key.ends_with('|') {
            continue;
        }
        let prio = region_priority_score(&rec.raw_name);
        match best_by_key.get_mut(&key) {
            Some((existing_prio, existing_rec)) => {
                if prio > *existing_prio {
                    *existing_prio = prio;
                    *existing_rec = rec;
                }
            }
            None => {
                best_by_key.insert(key, (prio, rec));
            }
        }
    }

    let mut out = Vec::with_capacity(best_by_key.len());
    let mut used_ids: HashSet<u32> = HashSet::with_capacity(best_by_key.len());

    for (_k, (_prio, rec)) in best_by_key {
        let id_hint = if !rec.serial.is_empty() {
            rec.serial.as_str()
        } else if !rec.crc_hex.is_empty() {
            rec.crc_hex.as_str()
        } else {
            rec.raw_name.as_str()
        };
        let mut app_id = platforms::synthetic_retro_catalog_id(spec.id, &rec.display_name, id_hint);
        while !used_ids.insert(app_id) {
            app_id = app_id.wrapping_add(1).max(0x8000_0001);
        }

        let (cover_url, header_url, hero_url) =
            build_libretro_thumbnail_urls(spec.id, &rec.raw_name);

        let mut genres = Vec::new();
        for g in rec.genre.split(&[',', '/', ';'][..]) {
            let gt = g.trim();
            if !gt.is_empty() && !genres.iter().any(|e: &String| e.eq_ignore_ascii_case(gt)) {
                genres.push(gt.to_string());
            }
        }
        if genres.is_empty() {
            genres.push("Retro Console".to_string());
        }

        let mut tags = vec![spec.short_name.to_string()];
        if spec.is_nintendo {
            tags.push("Nintendo".to_string());
        }
        if !rec.franchise.trim().is_empty() {
            tags.push(rec.franchise.trim().to_string());
        }
        if !rec.serial.trim().is_empty() {
            tags.push(rec.serial.trim().to_string());
        }

        let mut categories = vec!["Single-player".to_string(), spec.display_name.to_string()];
        let is_multiplayer = rec.max_users > 1;
        let is_coop = rec.max_users > 1;
        if is_multiplayer {
            categories.push("Shared/Split Screen".to_string());
            categories.push("Local Co-op".to_string());
        }

        let release_date = if rec.release_year > 1970 {
            rec.release_year.to_string()
        } else {
            String::new()
        };

        let short_description = if !rec.description.trim().is_empty()
            && !rec.description.trim().eq_ignore_ascii_case(&rec.display_name)
        {
            rec.description.clone()
        } else {
            format!(
                "{} catalog release{}",
                spec.display_name,
                if !rec.developer.is_empty() {
                    format!(" by {}", rec.developer)
                } else {
                    String::new()
                }
            )
        };

        // Rank retro titles realistically (~58..82) so searches and console browsing sort cleanly
        let mut score = 62.0f64;
        if !rec.developer.is_empty() {
            score += 4.0;
        }
        if !rec.genre.is_empty() {
            score += 3.0;
        }
        if !rec.serial.is_empty() {
            score += 3.0;
        }
        if spec.is_nintendo {
            score += 3.0;
        }

        out.push(CatalogGameEntry {
            app_id,
            name: rec.display_name,
            platform_id: spec.id,
            console_group: spec.console_group,
            platform_name: spec.short_name.to_string(),
            platform_slug: spec.slug.to_string(),
            price_cents: 0,
            initial_price_cents: 0,
            discount_percent: 0,
            price_formatted: spec.short_name.to_string(),
            is_free: true,
            ccu: 0,
            owners_min: 0,
            owners_max: 0,
            owners_label: spec.short_name.to_string(),
            positive: 0,
            negative: 0,
            review_pct: 0.0,
            is_singleplayer: true,
            is_coop,
            is_multiplayer,
            is_online_only_pvp: false,
            genres,
            tags,
            categories,
            short_description,
            developers: if rec.developer.trim().is_empty() {
                Vec::new()
            } else {
                vec![rec.developer.trim().to_string()]
            },
            publishers: if rec.publisher.trim().is_empty() {
                Vec::new()
            } else {
                vec![rec.publisher.trim().to_string()]
            },
            release_date,
            cover_url,
            header_url,
            hero_url,
            value_score: score,
            details_enriched: true,
            updated_at: now,
        });
    }

    out
}

async fn fetch_single_platform_rdb(spec: &'static PlatformSpec) -> Result<Vec<RdbGameRecord>, String> {
    let client = duke_core::net_doh::build_doh_client_for_url(RDB_RAW_BASE, true, true).await;
    let mut merged: Vec<RdbGameRecord> = Vec::new();

    for &rdb_file in spec.rdb_files {
        let encoded = urlencoding::encode(rdb_file);
        let url = format!("{}/{}", RDB_RAW_BASE, encoded);
        if let Ok(resp) = client.get(&url).send().await {
            if resp.status().is_success() {
                if let Ok(bytes) = resp.bytes().await {
                    if let Ok(mut parsed) = parse_libretro_rdb(&bytes) {
                        merged.append(&mut parsed);
                    }
                }
            }
        }
    }

    if merged.is_empty() && !spec.dat_files.is_empty() {
        for &dat_rel in spec.dat_files {
            let encoded_path: String = dat_rel
                .split('/')
                .map(|seg| urlencoding::encode(seg).into_owned())
                .collect::<Vec<_>>()
                .join("/");
            let url = format!("{}/{}", DAT_RAW_BASE, encoded_path);
            if let Ok(resp) = client.get(&url).send().await {
                if resp.status().is_success() {
                    if let Ok(txt) = resp.text().await {
                        let mut parsed = parse_clrmamepro_dat(&txt);
                        merged.append(&mut parsed);
                    }
                }
            }
        }
    }

    Ok(merged)
}

/// Runs the public Libretro RDB catalog harvester over `net-doh` with a 4-permit semaphore,
/// progressively flushing console entries into `catalog.dkcat`.
pub async fn harvest_retro_catalog_internal(
    app: Option<AppHandle>,
    force_refresh: bool,
) -> Result<RetroSyncStatus, String> {
    if RETRO_HARVEST_RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Ok(RetroSyncStatus {
            running: true,
            message: "Console catalog sync is already in progress.".to_string(),
            ..Default::default()
        });
    }

    struct ResetGuard;
    impl Drop for ResetGuard {
        fn drop(&mut self) {
            RETRO_HARVEST_RUNNING.store(false, Ordering::Release);
        }
    }
    let _guard = ResetGuard;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut sync_state = load_retro_sync_state();

    let targets: Vec<&'static PlatformSpec> = PLATFORMS
        .iter()
        .filter(|p| p.id > 0)
        .filter(|p| {
            if force_refresh {
                return true;
            }
            let last = sync_state
                .last_platform_sync
                .get(p.slug)
                .copied()
                .unwrap_or(0);
            now.saturating_sub(last) >= RETRO_SYNC_TTL_SECS
        })
        .collect();

    if targets.is_empty() {
        let (total_indexed, _, _) = catalog_mmap::catalog_mmap_header_meta();
        return Ok(RetroSyncStatus {
            running: false,
            completed_platforms: 24,
            total_platforms: 24,
            current_platform: "Up to date".to_string(),
            entries_added: 0,
            total_catalog_entries: total_indexed,
            message: "All 24 Nintendo & retro console catalogs are up to date.".to_string(),
        });
    }

    let dkcat_path = crate::get_app_data_file_standalone("catalog.dkcat")?;
    let total_platforms = targets.len();
    let sem = Arc::new(Semaphore::new(RETRO_HARVEST_CONCURRENCY));
    let mut handles = Vec::with_capacity(targets.len());

    for spec in targets {
        let sem_c = sem.clone();
        handles.push(tauri::async_runtime::spawn(async move {
            let _permit = sem_c.acquire_owned().await.ok();
            let recs = fetch_single_platform_rdb(spec).await.unwrap_or_default();
            (spec, recs)
        }));
    }

    let mut pending_batch: Vec<CatalogGameEntry> = Vec::new();
    let mut total_added = 0usize;
    let mut completed = 0usize;

    for handle in handles {
        let Ok((spec, recs)) = handle.await else {
            completed += 1;
            continue;
        };
        completed += 1;
        let converted = convert_rdb_records_to_catalog(spec, recs, now);
        if !converted.is_empty() {
            sync_state
                .last_platform_sync
                .insert(spec.slug.to_string(), now);
            total_added += converted.len();
            pending_batch.extend(converted);
        }

        // Progressive flush every 3 platforms or final step so users see titles appear immediately
        if pending_batch.len() >= 1800 || (completed == total_platforms && !pending_batch.is_empty()) {
            let batch = std::mem::take(&mut pending_batch);
            let dk_c = dkcat_path.clone();
            let _ = tokio::task::spawn_blocking(move || {
                catalog_mmap::upsert_entries_dkcat(&dk_c, &batch, None, &[])
            })
            .await;
        }

        let (total_indexed, _, _) = catalog_mmap::catalog_mmap_header_meta();
        if let Some(ref ah) = app {
            let _ = ah.emit(
                "retro-catalog-progress",
                RetroSyncStatus {
                    running: completed < total_platforms,
                    completed_platforms: completed,
                    total_platforms,
                    current_platform: spec.display_name.to_string(),
                    entries_added: total_added,
                    total_catalog_entries: total_indexed,
                    message: format!(
                        "Indexed {} ({}/{} systems, {} titles total)",
                        spec.short_name, completed, total_platforms, total_indexed
                    ),
                },
            );
        }
    }

    save_retro_sync_state(&sync_state);
    let (total_indexed, _, _) = catalog_mmap::catalog_mmap_header_meta();
    Ok(RetroSyncStatus {
        running: false,
        completed_platforms: total_platforms,
        total_platforms,
        current_platform: "Complete".to_string(),
        entries_added: total_added,
        total_catalog_entries: total_indexed,
        message: format!(
            "Synced {} Nintendo & retro titles ({} total searchable catalog games).",
            total_added, total_indexed
        ),
    })
}

/// Checks if any retro platforms are already indexed in `catalog.dkcat`; if zero, schedules a
/// background harvest over `net-doh` without blocking startup or allocating idle heap memory.
pub fn spawn_background_retro_catalog_sync_if_needed(app: AppHandle) {
    let sync_state = load_retro_sync_state();
    if sync_state.last_platform_sync.len() >= 20 {
        return;
    }
    // Use Tauri's managed async runtime because `.setup()` runs synchronously before the main thread enters Tokio.
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        let _ = harvest_retro_catalog_internal(Some(app), false).await;
    });
}

/// Downloads crisp boxart (`Named_Boxarts`) or cascading multi-source fallback to `<data_root>/covers/`
/// and builds a cached `32x32` PNG sidebar icon into `<data_root>/sidebar_icons/` for `assetProtocol`.
pub async fn cache_retro_cover_and_icon(
    platform_id: u16,
    game_id: &str,
    title: &str,
    hint_cover_url: Option<&str>,
) -> Option<(String, String)> {
    let covers_dir = crate::get_covers_dir_standalone().ok()?;
    let icons_dir = crate::sidebar_icons::get_sidebar_icons_dir_standalone().ok()?;
    let spec = platforms::platform_by_id(platform_id);
    let settings = crate::load_settings_standalone();
    let req = crate::artwork::CoverResolveRequest {
        cache_id: game_id.to_string(),
        title: title.to_string(),
        exe_path: None,
        platform_id: (platform_id > 0).then_some(platform_id),
        platform_hint: Some(spec.short_name.to_string()),
        steam_app_id: None,
        existing_cover_url: hint_cover_url.map(|s| s.to_string()),
        steamgriddb_api_key: settings.steamgriddb_api_key.unwrap_or_default(),
        sidebar_icon_id: Some(game_id.to_string()),
    };

    let outcome = crate::artwork::resolve_and_cache_cover_in_dirs(&req, &covers_dir, &icons_dir).await?;
    let icon_path = outcome
        .sidebar_icon_path
        .unwrap_or_else(|| crate::sidebar_icons::cached_sidebar_icon_path(&icons_dir, game_id));
    Some((
        outcome.cover_path.to_string_lossy().into_owned(),
        icon_path.to_string_lossy().into_owned(),
    ))
}

/// Public helper returning cached `32x32` sidebar icon path if present.
#[allow(dead_code)]
pub fn cached_retro_icon_path(_data_root: &Path, game_id: &str) -> Option<PathBuf> {
    let icons_dir = crate::sidebar_icons::get_sidebar_icons_dir_standalone().ok()?;
    let icon = crate::sidebar_icons::cached_sidebar_icon_path(&icons_dir, game_id);
    if icon.is_file() {
        Some(icon)
    } else {
        None
    }
}

#[tauri::command]
pub async fn sync_retro_catalog(
    app: AppHandle,
    force_refresh: Option<bool>,
) -> Result<RetroSyncStatus, String> {
    harvest_retro_catalog_internal(Some(app), force_refresh.unwrap_or(false)).await
}

#[tauri::command]
pub async fn get_retro_platforms() -> Result<Vec<platforms::PlatformInfoDto>, String> {
    Ok(platforms::list_platform_dtos())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_mp_str(buf: &mut Vec<u8>, s: &str) {
        let len = s.len();
        if len < 32 {
            buf.push(0xa0 | (len as u8));
        } else {
            buf.push(0xd9);
            buf.push(len as u8);
        }
        buf.extend_from_slice(s.as_bytes());
    }

    fn build_synthetic_rdb(records: &[(&str, &str, &str, u32, u32)]) -> Vec<u8> {
        let mut body = Vec::new();
        for (name, genre, serial, year, users) in records {
            body.push(0x85); // fixmap with 5 fields
            push_mp_str(&mut body, "name");
            push_mp_str(&mut body, name);
            push_mp_str(&mut body, "genre");
            push_mp_str(&mut body, genre);
            push_mp_str(&mut body, "serial");
            push_mp_str(&mut body, serial);
            push_mp_str(&mut body, "releaseyear");
            body.push(0xcd);
            body.extend_from_slice(&(*year as u16).to_be_bytes());
            push_mp_str(&mut body, "users");
            body.push((*users as u8) & 0x7f);
        }
        body.push(0xc0); // nil terminator
        let meta_offset = 16 + body.len() as u64;
        let mut full = Vec::with_capacity(16 + body.len() + 4);
        full.extend_from_slice(b"RARCHDB\0");
        full.extend_from_slice(&meta_offset.to_be_bytes());
        full.extend_from_slice(&body);
        full.push(0x80); // trailing metadata empty map
        full
    }

    #[test]
    fn parses_binary_libretro_rdb_into_dkcat_entries() {
        let rdb_bytes = build_synthetic_rdb(&[
            ("Super Mario 64 (USA)", "Platform", "NSME", 1996, 1),
            ("Super Mario 64 (Japan)", "Platform", "NSMJ", 1996, 1),
            ("Mario Kart 64 (USA)", "Racing", "NKTE", 1997, 4),
        ]);
        let parsed = parse_libretro_rdb(&rdb_bytes).unwrap();
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].display_name, "Super Mario 64");
        assert_eq!(parsed[2].max_users, 4);

        let spec = platforms::platform_by_id(platforms::PLATFORM_N64);
        let catalog_entries = convert_rdb_records_to_catalog(spec, parsed, 1_700_000_000);
        // Super Mario 64 (USA) + (Japan) collapse to single USA canonical release
        assert_eq!(catalog_entries.len(), 2);
        let mario64 = catalog_entries
            .iter()
            .find(|e| e.name == "Super Mario 64")
            .unwrap();
        assert_eq!(mario64.platform_id, platforms::PLATFORM_N64);
        assert_eq!(mario64.platform_name, "N64");
        assert!(mario64.cover_url.contains("Super%20Mario%2064%20%28USA%29.png"));
        assert!(mario64.header_url.contains("Named_Titles"));

        let mk64 = catalog_entries
            .iter()
            .find(|e| e.name == "Mario Kart 64")
            .unwrap();
        assert!(mk64.is_coop);
        assert!(mk64.is_multiplayer);

        // Round-trip into DKCAT002 archive
        let dir = std::env::temp_dir().join(format!("duke_rdb_dkcat_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let dkcat_path = dir.join("catalog.dkcat");
        let cache = crate::CatalogCacheFile {
            fetched_at: 1_700_000_000,
            fetched_pages: Vec::new(),
            entries: catalog_entries,
        };
        let stats = catalog_mmap::atomic_rebuild_dkcat(&dkcat_path, &cache).unwrap();
        assert_eq!(stats.record_count, 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sanitizes_libretro_thumbnail_special_characters() {
        assert_eq!(
            sanitize_libretro_thumb_name("Zelda II: The Adventure of Link (USA)"),
            "Zelda II_ The Adventure of Link (USA)"
        );
        let (boxart, title, _) =
            build_libretro_thumbnail_urls(platforms::PLATFORM_NES, "Castlevania III: Dracula's Curse (USA)");
        assert!(boxart.starts_with("https://thumbnails.libretro.com/Nintendo%20-%20Nintendo%20Entertainment%20System/Named_Boxarts/"));
        assert!(boxart.contains("Castlevania%20III_%20Dracula%27s%20Curse"));
        assert!(title.contains("/Named_Titles/"));
    }
}

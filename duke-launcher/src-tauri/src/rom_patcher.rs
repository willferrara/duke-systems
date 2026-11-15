//! Multi-Disc `.m3u` Auto-Grouping + Pure-Rust ROM Patcher (`.ips`, `.bps`, `.ups`, `.xdelta`).
//!
//! 1. Groups `(Disc 1)`, `(Disc 2)`, `(Disc 3)` console dumps into unified `.m3u` playlists and
//!    collapses duplicate cards in `library.json` into 1 clean game card.
//! 2. Patches ROMs cleanly in pure Rust with source CRC32/SHA-1 header verification, `.duke-cas/`
//!    restore points, non-destructive base ROM preservation, and separate 1-click patched profiles.

use serde::{Deserialize, Serialize};
use tauri::Emitter;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::bios_vault::sha1_hex_bytes;
use crate::crc32_ieee;
use crate::snapshots::{create_snapshot_in_dir, CAS_DIR_NAME};

const MAX_ROM_PATCH_BYTES: u64 = 128 * 1024 * 1024;

// ---------- Multi-Disc `.m3u` Grouping ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiDiscGroupSummary {
    pub title: String,
    pub m3u_path: String,
    pub disc_count: usize,
    pub collapsed_entries: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiDiscOrganizeReport {
    pub playlists_created: usize,
    pub entries_collapsed: usize,
    pub groups: Vec<MultiDiscGroupSummary>,
    pub summary: String,
}

/// Extracts `(base_stem_normalized, disc_number)` from filenames containing `(Disc 1)`, `(Disk 2)`, etc.
pub fn parse_multi_disc_stem(stem: &str) -> Option<(String, u32)> {
    let lower = stem.to_ascii_lowercase();
    for marker in ["(disc ", "(disk ", "[disc ", "[disk "] {
        if let Some(pos) = lower.find(marker) {
            let after = &lower[pos + marker.len()..];
            let digits: String = after
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if let Ok(disc_num) = digits.parse::<u32>() {
                let close_char = if marker.starts_with('(') { ')' } else { ']' };
                let end_pos = after
                    .find(close_char)
                    .map(|idx| pos + marker.len() + idx + 1)
                    .unwrap_or(stem.len());
                let mut base = format!("{}{}", &stem[..pos], &stem[end_pos..]);
                base = base.split_whitespace().collect::<Vec<_>>().join(" ");
                if !base.is_empty() {
                    return Some((base, disc_num));
                }
            }
        }
    }
    None
}

fn is_disc_rom_extension(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "chd" | "iso" | "cue" | "rvz" | "gcz" | "pbp" | "cso"
    )
}

/// Scans `dir` for multi-disc console ROM files, writes `.m3u` playlists, and returns created groups.
pub fn generate_m3u_playlists_in_dir(dir: &Path) -> Result<Vec<MultiDiscGroupSummary>, String> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    // Map: (parent_dir, lower_base_title) -> (display_base_title, Vec<(disc_num, filename)>)
    type DiscBucketMap = HashMap<(PathBuf, String), (String, Vec<(u32, String)>)>;
    let mut buckets: DiscBucketMap = HashMap::new();
    for ent in WalkDir::new(dir)
        .max_depth(3)
        .follow_links(false)
        .into_iter()
        .flatten()
    {
        if !ent.file_type().is_file() {
            continue;
        }
        let path = ent.path();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        if !is_disc_rom_extension(ext) {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if let Some((base_title, disc_num)) = parse_multi_disc_stem(stem) {
            let Some(parent) = path.parent() else {
                continue;
            };
            let Some(fname) = path.file_name().and_then(|f| f.to_str()) else {
                continue;
            };
            let key = (parent.to_path_buf(), base_title.to_ascii_lowercase());
            let entry = buckets
                .entry(key)
                .or_insert_with(|| (base_title.clone(), Vec::new()));
            entry.1.push((disc_num, fname.to_string()));
        }
    }

    let mut out = Vec::new();
    for ((parent, _), (display_title, mut discs)) in buckets {
        discs.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        discs.dedup_by(|a, b| a.1.eq_ignore_ascii_case(&b.1));
        if discs.len() < 2 {
            continue;
        }
        let m3u_name = format!("{display_title}.m3u");
        let m3u_path = parent.join(m3u_name);
        let content = discs
            .iter()
            .map(|(_, f)| f.as_str())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        crate::atomic_write_string(&m3u_path, &content)?;
        out.push(MultiDiscGroupSummary {
            title: display_title,
            m3u_path: m3u_path.to_string_lossy().into_owned(),
            disc_count: discs.len(),
            collapsed_entries: 0,
        });
    }
    Ok(out)
}

// ---------- Pure-Rust ROM Patchers (.ips, .bps, .ups, .xdelta/VCDIFF) ----------

fn read_beat_vli(data: &[u8], cursor: &mut usize) -> Result<u64, String> {
    let mut val: u64 = 0;
    let mut shift: u64 = 1;
    loop {
        if *cursor >= data.len() {
            return Err("Unexpected EOF while reading BPS/UPS variable integer.".to_string());
        }
        let x = data[*cursor];
        *cursor += 1;
        val = val
            .checked_add(((x & 0x7F) as u64).saturating_mul(shift))
            .ok_or_else(|| "VLI integer overflow in patch.".to_string())?;
        if (x & 0x80) != 0 {
            return Ok(val);
        }
        shift = shift.saturating_mul(128);
        val = val.saturating_add(shift);
    }
}

pub fn apply_ips_patch(source: &[u8], patch: &[u8]) -> Result<Vec<u8>, String> {
    if patch.len() < 8 || &patch[..5] != b"PATCH" {
        return Err("Invalid IPS patch header (missing PATCH magic).".to_string());
    }
    let mut out = source.to_vec();
    let mut i = 5usize;
    while i + 3 <= patch.len() {
        if &patch[i..i + 3] == b"EOF" {
            i += 3;
            if i + 3 <= patch.len() {
                let trunc_len = ((patch[i] as usize) << 16)
                    | ((patch[i + 1] as usize) << 8)
                    | (patch[i + 2] as usize);
                out.resize(trunc_len, 0);
            }
            return Ok(out);
        }
        if i + 5 > patch.len() {
            return Err("Truncated IPS record header.".to_string());
        }
        let offset = ((patch[i] as usize) << 16)
            | ((patch[i + 1] as usize) << 8)
            | (patch[i + 2] as usize);
        let size = ((patch[i + 3] as usize) << 8) | (patch[i + 4] as usize);
        i += 5;
        if size == 0 {
            if i + 3 > patch.len() {
                return Err("Truncated IPS RLE record.".to_string());
            }
            let rle_len = ((patch[i] as usize) << 8) | (patch[i + 1] as usize);
            let fill = patch[i + 2];
            i += 3;
            if offset + rle_len > out.len() {
                out.resize(offset + rle_len, 0);
            }
            for b in &mut out[offset..offset + rle_len] {
                *b = fill;
            }
        } else {
            if i + size > patch.len() {
                return Err("Truncated IPS data record.".to_string());
            }
            if offset + size > out.len() {
                out.resize(offset + size, 0);
            }
            out[offset..offset + size].copy_from_slice(&patch[i..i + size]);
            i += size;
        }
    }
    Err("Invalid IPS patch: missing EOF footer.".to_string())
}

pub fn apply_bps_patch(source: &[u8], patch: &[u8]) -> Result<Vec<u8>, String> {
    if patch.len() < 19 || &patch[..4] != b"BPS1" {
        return Err("Invalid BPS patch header (expected BPS1).".to_string());
    }
    let body_end = patch.len() - 12;
    let footer_patch_crc = u32::from_le_bytes([
        patch[patch.len() - 4],
        patch[patch.len() - 3],
        patch[patch.len() - 2],
        patch[patch.len() - 1],
    ]);
    if crc32_ieee(&patch[..patch.len() - 4]) != footer_patch_crc {
        return Err("BPS patch file checksum mismatch (corrupted .bps file).".to_string());
    }
    let expected_src_crc = u32::from_le_bytes([
        patch[body_end],
        patch[body_end + 1],
        patch[body_end + 2],
        patch[body_end + 3],
    ]);
    if crc32_ieee(source) != expected_src_crc {
        return Err(format!(
            "Source ROM CRC32 mismatch (expected 0x{:08X}, got 0x{:08X}).",
            expected_src_crc,
            crc32_ieee(source)
        ));
    }
    let expected_dst_crc = u32::from_le_bytes([
        patch[body_end + 4],
        patch[body_end + 5],
        patch[body_end + 6],
        patch[body_end + 7],
    ]);

    let mut cursor = 4usize;
    let src_len = read_beat_vli(patch, &mut cursor)? as usize;
    let dst_len = read_beat_vli(patch, &mut cursor)? as usize;
    let meta_len = read_beat_vli(patch, &mut cursor)? as usize;
    cursor = cursor
        .checked_add(meta_len)
        .ok_or_else(|| "BPS metadata overflow.".to_string())?;
    if source.len() != src_len || dst_len > MAX_ROM_PATCH_BYTES as usize {
        return Err("BPS source length mismatch or target size out of range.".to_string());
    }

    let mut out = vec![0u8; dst_len];
    let mut out_offset = 0usize;
    let mut src_rel_offset: isize = 0;
    let mut dst_rel_offset: isize = 0;

    while cursor < body_end {
        let cmd = read_beat_vli(patch, &mut cursor)?;
        let mode = cmd & 3;
        let len = ((cmd >> 2) + 1) as usize;
        if out_offset + len > dst_len {
            return Err("BPS output stream exceeded declared target length.".to_string());
        }
        match mode {
            0 => {
                // SourceRead
                if out_offset + len > source.len() {
                    return Err("BPS SourceRead out of bounds.".to_string());
                }
                out[out_offset..out_offset + len].copy_from_slice(&source[out_offset..out_offset + len]);
                out_offset += len;
            }
            1 => {
                // TargetRead
                if cursor + len > body_end {
                    return Err("BPS TargetRead out of bounds.".to_string());
                }
                out[out_offset..out_offset + len].copy_from_slice(&patch[cursor..cursor + len]);
                cursor += len;
                out_offset += len;
            }
            2 => {
                // SourceCopy
                let raw = read_beat_vli(patch, &mut cursor)?;
                let delta = (raw >> 1) as isize;
                src_rel_offset += if (raw & 1) != 0 { -delta } else { delta };
                let s = src_rel_offset as usize;
                if src_rel_offset < 0 || s + len > source.len() {
                    return Err("BPS SourceCopy out of bounds.".to_string());
                }
                out[out_offset..out_offset + len].copy_from_slice(&source[s..s + len]);
                src_rel_offset += len as isize;
                out_offset += len;
            }
            _ => {
                // TargetCopy
                let raw = read_beat_vli(patch, &mut cursor)?;
                let delta = (raw >> 1) as isize;
                dst_rel_offset += if (raw & 1) != 0 { -delta } else { delta };
                if dst_rel_offset < 0 || (dst_rel_offset as usize) >= out_offset {
                    return Err("BPS TargetCopy out of bounds.".to_string());
                }
                for _ in 0..len {
                    let b = out[dst_rel_offset as usize];
                    out[out_offset] = b;
                    dst_rel_offset += 1;
                    out_offset += 1;
                }
            }
        }
    }

    if crc32_ieee(&out) != expected_dst_crc {
        return Err("BPS patched output CRC32 verification failed.".to_string());
    }
    Ok(out)
}

pub fn apply_ups_patch(source: &[u8], patch: &[u8]) -> Result<Vec<u8>, String> {
    if patch.len() < 18 || &patch[..4] != b"UPS1" {
        return Err("Invalid UPS patch header (expected UPS1).".to_string());
    }
    let body_end = patch.len() - 12;
    let footer_patch_crc = u32::from_le_bytes([
        patch[patch.len() - 4],
        patch[patch.len() - 3],
        patch[patch.len() - 2],
        patch[patch.len() - 1],
    ]);
    if crc32_ieee(&patch[..patch.len() - 4]) != footer_patch_crc {
        return Err("UPS patch file checksum mismatch.".to_string());
    }
    let expected_src_crc = u32::from_le_bytes([
        patch[body_end],
        patch[body_end + 1],
        patch[body_end + 2],
        patch[body_end + 3],
    ]);
    if crc32_ieee(source) != expected_src_crc {
        return Err("UPS source ROM CRC32 mismatch.".to_string());
    }
    let expected_dst_crc = u32::from_le_bytes([
        patch[body_end + 4],
        patch[body_end + 5],
        patch[body_end + 6],
        patch[body_end + 7],
    ]);

    let mut cursor = 4usize;
    let src_len = read_beat_vli(patch, &mut cursor)? as usize;
    let dst_len = read_beat_vli(patch, &mut cursor)? as usize;
    if source.len() != src_len || dst_len > MAX_ROM_PATCH_BYTES as usize {
        return Err("UPS source length mismatch or target size too large.".to_string());
    }
    let mut out = source.to_vec();
    out.resize(dst_len, 0);

    let mut offset = 0usize;
    while cursor < body_end {
        let skip = read_beat_vli(patch, &mut cursor)? as usize;
        offset = offset.saturating_add(skip);
        while cursor < body_end {
            let b = patch[cursor];
            cursor += 1;
            if b == 0 {
                offset += 1;
                break;
            }
            if offset >= out.len() {
                return Err("UPS XOR offset out of bounds.".to_string());
            }
            out[offset] ^= b;
            offset += 1;
        }
    }

    if crc32_ieee(&out) != expected_dst_crc {
        return Err("UPS patched ROM CRC32 verification failed.".to_string());
    }
    Ok(out)
}

fn read_vcdiff_int(data: &[u8], cursor: &mut usize) -> Result<usize, String> {
    let mut acc: usize = 0;
    for _ in 0..8 {
        if *cursor >= data.len() {
            return Err("Unexpected EOF in VCDIFF integer.".to_string());
        }
        let b = data[*cursor];
        *cursor += 1;
        acc = (acc << 7) | ((b & 0x7F) as usize);
        if (b & 0x80) == 0 {
            return Ok(acc);
        }
    }
    Err("VCDIFF integer too large.".to_string())
}

/// Pure-Rust RFC 3284 VCDIFF (`.xdelta`) decoder supporting default code table & COPY/ADD/RUN windows.
pub fn apply_vcdiff_xdelta_patch(source: &[u8], patch: &[u8]) -> Result<Vec<u8>, String> {
    if patch.len() < 5 || patch[0..3] != [0xD6, 0xC3, 0xC4] {
        return Err("Invalid xdelta/VCDIFF header magic.".to_string());
    }
    let hdr_indicator = patch[4];
    let mut cur = 5usize;
    if (hdr_indicator & 0x01) != 0 {
        return Err("VCDIFF external secondary compressor not supported without xdelta3.".to_string());
    }
    if (hdr_indicator & 0x02) != 0 {
        // Skip VCD_CODETABLE indicator -> custom code table length
        let table_len = read_vcdiff_int(patch, &mut cur)?;
        cur = cur.saturating_add(table_len);
    }
    if (hdr_indicator & 0x04) != 0 {
        // VCD_APPHEADER
        let app_len = read_vcdiff_int(patch, &mut cur)?;
        cur = cur.saturating_add(app_len);
    }

    let mut target_out = Vec::<u8>::new();
    while cur < patch.len() {
        let win_ind = patch[cur];
        cur += 1;
        let has_source = (win_ind & 0x01) != 0;
        let has_target_seg = (win_ind & 0x02) != 0;
        let has_adler = (win_ind & 0x04) != 0;

        let mut src_seg_len = 0usize;
        let mut src_seg_pos = 0usize;
        if has_source || has_target_seg {
            src_seg_len = read_vcdiff_int(patch, &mut cur)?;
            src_seg_pos = read_vcdiff_int(patch, &mut cur)?;
        }

        let _delta_enc_len = read_vcdiff_int(patch, &mut cur)?;
        let tgt_win_len = read_vcdiff_int(patch, &mut cur)?;
        let delta_ind = *patch.get(cur).ok_or_else(|| "Truncated VCDIFF window.".to_string())?;
        cur += 1;
        if (delta_ind & 0x07) != 0 {
            return Err("Secondary VCDIFF compression flags unsupported.".to_string());
        }

        let data_len = read_vcdiff_int(patch, &mut cur)?;
        let inst_len = read_vcdiff_int(patch, &mut cur)?;
        let addr_len = read_vcdiff_int(patch, &mut cur)?;
        if has_adler {
            cur = cur.checked_add(4).ok_or_else(|| "VCDIFF Adler32 overflow.".to_string())?;
        }

        if cur + data_len + inst_len + addr_len > patch.len() {
            return Err("Truncated VCDIFF delta section.".to_string());
        }
        let data_sec = &patch[cur..cur + data_len];
        let inst_sec = &patch[cur + data_len..cur + data_len + inst_len];
        let addr_sec = &patch[cur + data_len + inst_len..cur + data_len + inst_len + addr_len];
        cur += data_len + inst_len + addr_len;

        let mut data_pos = 0usize;
        let mut addr_pos = 0usize;
        let mut win_out = Vec::<u8>::with_capacity(tgt_win_len);
        let mut near = [0usize; 4];
        let mut near_idx = 0usize;
        let mut same = [0usize; 3 * 256];

        let resolve_addr = |mode: u8,
                            here: usize,
                            addr_pos: &mut usize,
                            near: &mut [usize; 4],
                            near_idx: &mut usize,
                            same: &mut [usize; 768]|
         -> Result<usize, String> {
            let addr = match mode {
                0 => read_vcdiff_int(addr_sec, addr_pos)?,
                1 => {
                    let d = read_vcdiff_int(addr_sec, addr_pos)?;
                    here.checked_sub(d).ok_or_else(|| "VCDIFF HERE addr underflow.".to_string())?
                }
                2..=5 => {
                    let m = (mode - 2) as usize;
                    let d = read_vcdiff_int(addr_sec, addr_pos)?;
                    near[m].saturating_add(d)
                }
                6..=8 => {
                    let b = *addr_sec
                        .get(*addr_pos)
                        .ok_or_else(|| "VCDIFF SAME addr EOF.".to_string())?;
                    *addr_pos += 1;
                    same[((mode - 6) as usize) * 256 + (b as usize)]
                }
                _ => return Err("Unsupported VCDIFF copy mode.".to_string()),
            };
            near[*near_idx % 4] = addr;
            *near_idx += 1;
            same[addr % (3 * 256)] = addr;
            Ok(addr)
        };

        let mut ipos = 0usize;
        while ipos < inst_sec.len() {
            let opcode = inst_sec[ipos];
            ipos += 1;
            // RFC 3284 Section 5.6 default code table dispatch
            let exec_single = |op_type: u8,
                               size: usize,
                               mode: u8,
                               win_out: &mut Vec<u8>,
                               data_pos: &mut usize,
                               addr_pos: &mut usize,
                               near: &mut [usize; 4],
                               near_idx: &mut usize,
                               same: &mut [usize; 768]|
             -> Result<(), String> {
                match op_type {
                    0 => Ok(()), // NOOP
                    1 => {
                        // ADD
                        if *data_pos + size > data_sec.len() {
                            return Err("VCDIFF ADD data overflow.".to_string());
                        }
                        win_out.extend_from_slice(&data_sec[*data_pos..*data_pos + size]);
                        *data_pos += size;
                        Ok(())
                    }
                    2 => {
                        // RUN
                        if *data_pos >= data_sec.len() {
                            return Err("VCDIFF RUN data EOF.".to_string());
                        }
                        let byte = data_sec[*data_pos];
                        *data_pos += 1;
                        win_out.resize(win_out.len() + size, byte);
                        Ok(())
                    }
                    3 => {
                        // COPY
                        let here = src_seg_len + win_out.len();
                        let addr = resolve_addr(mode, here, addr_pos, near, near_idx, same)?;
                        for k in 0..size {
                            let idx = addr + k;
                            let byte = if has_source && idx < src_seg_len {
                                *source
                                    .get(src_seg_pos + idx)
                                    .ok_or_else(|| "VCDIFF COPY source out of bounds.".to_string())?
                            } else if has_target_seg && idx < src_seg_len {
                                *target_out
                                    .get(src_seg_pos + idx)
                                    .ok_or_else(|| "VCDIFF COPY prior target out of bounds.".to_string())?
                            } else {
                                let w_idx = idx
                                    .checked_sub(src_seg_len)
                                    .ok_or_else(|| "VCDIFF COPY window index invalid.".to_string())?;
                                *win_out
                                    .get(w_idx)
                                    .ok_or_else(|| "VCDIFF COPY window out of bounds.".to_string())?
                            };
                            win_out.push(byte);
                        }
                        Ok(())
                    }
                    _ => Err("Unknown VCDIFF instruction.".to_string()),
                }
            };

            let read_size = |raw: u8, ipos: &mut usize| -> Result<usize, String> {
                if raw == 0 {
                    read_vcdiff_int(inst_sec, ipos)
                } else {
                    Ok(raw as usize)
                }
            };

            match opcode {
                0 => {
                    let sz = read_size(0, &mut ipos)?;
                    exec_single(2, sz, 0, &mut win_out, &mut data_pos, &mut addr_pos, &mut near, &mut near_idx, &mut same)?;
                }
                1..=18 => {
                    let sz = if opcode == 1 { read_size(0, &mut ipos)? } else { (opcode - 1) as usize };
                    exec_single(1, sz, 0, &mut win_out, &mut data_pos, &mut addr_pos, &mut near, &mut near_idx, &mut same)?;
                }
                19..=162 => {
                    let idx = opcode - 19;
                    let mode = idx / 16;
                    let raw_sz = idx % 16;
                    let sz = if raw_sz == 0 { read_size(0, &mut ipos)? } else { (raw_sz + 3) as usize };
                    exec_single(3, sz, mode, &mut win_out, &mut data_pos, &mut addr_pos, &mut near, &mut near_idx, &mut same)?;
                }
                163..=234 => {
                    let idx = opcode - 163;
                    let add_sz = (idx / 18) + 1;
                    let rem = idx % 18;
                    let mode = rem / 3;
                    let copy_sz = (rem % 3) + 4;
                    exec_single(1, add_sz as usize, 0, &mut win_out, &mut data_pos, &mut addr_pos, &mut near, &mut near_idx, &mut same)?;
                    exec_single(3, copy_sz as usize, mode, &mut win_out, &mut data_pos, &mut addr_pos, &mut near, &mut near_idx, &mut same)?;
                }
                235..=246 => {
                    let idx = opcode - 235;
                    let add_sz = (idx % 4) + 1;
                    let mode = 6 + (idx / 4);
                    exec_single(1, add_sz as usize, 0, &mut win_out, &mut data_pos, &mut addr_pos, &mut near, &mut near_idx, &mut same)?;
                    exec_single(3, 4, mode, &mut win_out, &mut data_pos, &mut addr_pos, &mut near, &mut near_idx, &mut same)?;
                }
                _ => {
                    let mode = opcode - 247;
                    exec_single(3, 4, mode, &mut win_out, &mut data_pos, &mut addr_pos, &mut near, &mut near_idx, &mut same)?;
                    exec_single(1, 1, 0, &mut win_out, &mut data_pos, &mut addr_pos, &mut near, &mut near_idx, &mut same)?;
                }
            }
        }
        target_out.extend_from_slice(&win_out);
    }

    Ok(target_out)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RomPatchResult {
    pub patched_rom_path: String,
    pub patched_game_id: String,
    pub patched_title: String,
    pub snapshot_id: Option<String>,
    pub source_crc32: String,
    pub source_sha1: String,
    pub patched_crc32: String,
    pub patch_format: String,
    pub summary: String,
}

pub(crate) fn apply_rom_patch_bytes_auto(
    source_bytes: &[u8],
    patch_bytes: &[u8],
    patch_ext_lower: &str,
) -> Result<(Vec<u8>, &'static str), String> {
    if patch_ext_lower == "ips" || patch_bytes.starts_with(b"PATCH") {
        return Ok((apply_ips_patch(source_bytes, patch_bytes)?, "IPS"));
    }
    if patch_ext_lower == "bps" || patch_bytes.starts_with(b"BPS1") {
        return Ok((apply_bps_patch(source_bytes, patch_bytes)?, "BPS"));
    }
    if patch_ext_lower == "ups" || patch_bytes.starts_with(b"UPS1") {
        return Ok((apply_ups_patch(source_bytes, patch_bytes)?, "UPS"));
    }
    if patch_ext_lower == "xdelta"
        || patch_ext_lower == "vcdiff"
        || patch_bytes.starts_with(&[0xD6, 0xC3, 0xC4])
    {
        return Ok((apply_vcdiff_xdelta_patch(source_bytes, patch_bytes)?, "XDELTA/VCDIFF"));
    }
    Err(format!(
        "Unsupported patch format '.{}'. Supported: .ips, .bps, .ups, .xdelta.",
        patch_ext_lower
    ))
}

#[tauri::command]
pub async fn group_multi_disc_library(app: tauri::AppHandle) -> Result<MultiDiscOrganizeReport, String> {
    let mut library = crate::load_library_standalone()?;
    let dirs: Vec<PathBuf> = {
        let mut set = BTreeMap::<String, PathBuf>::new();
        for g in &library {
            let p = PathBuf::from(&g.install_dir);
            if p.is_dir() {
                set.insert(p.to_string_lossy().to_ascii_lowercase(), p);
            } else if let Some(par) = PathBuf::from(&g.exe_path).parent() {
                if par.is_dir() {
                    set.insert(par.to_string_lossy().to_ascii_lowercase(), par.to_path_buf());
                }
            }
        }
        set.into_values().collect()
    };

    let created = tokio::task::spawn_blocking(move || -> Result<Vec<MultiDiscGroupSummary>, String> {
        let mut all = Vec::new();
        for d in dirs {
            all.extend(generate_m3u_playlists_in_dir(&d)?);
        }
        Ok(all)
    })
    .await
    .map_err(|e| format!("Multi-disc grouping failed: {e}"))??;

    let mut entries_collapsed = 0usize;
    let mut final_groups = Vec::with_capacity(created.len());
    for mut grp in created {
        let m3u_p = PathBuf::from(&grp.m3u_path);
        let Some(m3u_dir) = m3u_p.parent() else {
            continue;
        };
        let norm_base = grp.title.to_ascii_lowercase();
        let mut matching_indices = Vec::new();
        for (idx, g) in library.iter().enumerate() {
            let exe = PathBuf::from(&g.exe_path);
            let same_dir = exe.parent().is_some_and(|p| p == m3u_dir);
            let stem = exe.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let parsed_match = parse_multi_disc_stem(stem)
                .map(|(b, _)| b.eq_ignore_ascii_case(&norm_base))
                .unwrap_or(false);
            if same_dir && (parsed_match || exe == m3u_p) {
                matching_indices.push(idx);
            }
        }
        if !matching_indices.is_empty() {
            let keep_idx = matching_indices[0];
            library[keep_idx].title = grp.title.clone();
            library[keep_idx].exe_path = grp.m3u_path.clone();
            let mut to_remove: Vec<usize> = matching_indices.into_iter().skip(1).collect();
            to_remove.sort_unstable_by(|a, b| b.cmp(a));
            for rm in to_remove {
                library.remove(rm);
                entries_collapsed += 1;
                grp.collapsed_entries += 1;
            }
        }
        final_groups.push(grp);
    }

    if entries_collapsed > 0 || !final_groups.is_empty() {
        crate::save_library_to_disk(&app, &library)?;
        let _ = app.emit("library-updated", &library);
    }

    let summary = if final_groups.is_empty() {
        "No multi-disc ROM sets found to collapse.".to_string()
    } else {
        format!(
            "Created {} .m3u multi-disc playlist(s) and collapsed {} duplicate card(s).",
            final_groups.len(),
            entries_collapsed
        )
    };

    Ok(MultiDiscOrganizeReport {
        playlists_created: final_groups.len(),
        entries_collapsed,
        groups: final_groups,
        summary,
    })
}

#[tauri::command]
pub async fn apply_rom_patch_and_create_profile(
    app: tauri::AppHandle,
    game_id: String,
    patch_path: String,
    expected_source_crc32: Option<String>,
    expected_source_sha1: Option<String>,
) -> Result<RomPatchResult, String> {
    let mut library = crate::load_library_standalone()?;
    let base_entry = library
        .iter()
        .find(|g| g.id == game_id)
        .cloned()
        .ok_or_else(|| format!("Base ROM game '{}' not found.", game_id))?;

    let rom_path = PathBuf::from(&base_entry.exe_path);
    let patch_p = PathBuf::from(patch_path.trim().trim_matches('"'));
    if !rom_path.is_file() {
        return Err("Base ROM file does not exist on disk.".to_string());
    }
    if !patch_p.is_file() {
        return Err("Patch file does not exist on disk.".to_string());
    }

    let result = tokio::task::spawn_blocking(move || -> Result<(RomPatchResult, crate::GameEntry), String> {
        let meta_len = fs::metadata(&rom_path).map(|m| m.len()).unwrap_or(0);
        if meta_len > MAX_ROM_PATCH_BYTES {
            return Err(format!(
                "ROM exceeds pure-Rust in-memory patch limit ({} MB > 128 MB).",
                meta_len / (1024 * 1024)
            ));
        }
        let source_bytes =
            fs::read(&rom_path).map_err(|e| format!("Failed reading base ROM: {e}"))?;
        let patch_bytes =
            fs::read(&patch_p).map_err(|e| format!("Failed reading patch file: {e}"))?;

        let src_crc_hex = format!("{:08X}", crc32_ieee(&source_bytes));
        let src_sha1_hex = sha1_hex_bytes(&source_bytes);
        if let Some(exp_crc) = expected_source_crc32.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            if !src_crc_hex.eq_ignore_ascii_case(exp_crc) {
                return Err(format!(
                    "Source ROM CRC32 verification failed (expected {exp_crc}, actual {src_crc_hex})."
                ));
            }
        }
        if let Some(exp_sha) = expected_source_sha1.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            if !src_sha1_hex.eq_ignore_ascii_case(exp_sha) {
                return Err(format!(
                    "Source ROM SHA-1 verification failed (expected {exp_sha}, actual {src_sha1_hex})."
                ));
            }
        }

        let rom_dir = rom_path
            .parent()
            .unwrap_or_else(|| Path::new(&base_entry.install_dir))
            .to_path_buf();
        let cas_dir = rom_dir.join(CAS_DIR_NAME);
        let snapshot_id = create_snapshot_in_dir(
            &base_entry.id,
            &rom_dir,
            &cas_dir,
            Some(&format!("Pre-Patch: {}", patch_p.file_name().and_then(|n| n.to_str()).unwrap_or("ROM"))),
        )
        .ok()
        .map(|s| s.id);

        let patch_ext = patch_p
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let (patched_bytes, fmt) = apply_rom_patch_bytes_auto(&source_bytes, &patch_bytes, &patch_ext)?;
        let patched_crc_hex = format!("{:08X}", crc32_ieee(&patched_bytes));

        let base_stem = rom_path.file_stem().and_then(|s| s.to_str()).unwrap_or("ROM");
        let patch_stem = patch_p
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Patched");
        let rom_ext = rom_path.extension().and_then(|e| e.to_str()).unwrap_or("bin");
        let out_name = format!("{base_stem} [{patch_stem}].{rom_ext}");
        let out_path = rom_dir.join(out_name);
        fs::write(&out_path, &patched_bytes)
            .map_err(|e| format!("Failed writing patched ROM '{}': {e}", out_path.display()))?;

        let mut patched_entry = base_entry.clone();
        let new_id = format!(
            "patched_{}_{}",
            &blake3::hash(out_path.to_string_lossy().as_bytes()).to_hex()[..12],
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        );
        patched_entry.id = new_id.clone();
        patched_entry.title = format!("{} [{}]", base_entry.title, patch_stem);
        patched_entry.exe_path = out_path.to_string_lossy().into_owned();
        patched_entry.source = format!("{} (Patched)", base_entry.source);
        patched_entry.size_bytes = patched_bytes.len() as u64;
        patched_entry.size_formatted = crate::format_bytes(patched_bytes.len() as u64);

        let res = RomPatchResult {
            patched_rom_path: patched_entry.exe_path.clone(),
            patched_game_id: new_id,
            patched_title: patched_entry.title.clone(),
            snapshot_id,
            source_crc32: src_crc_hex,
            source_sha1: src_sha1_hex,
            patched_crc32: patched_crc_hex,
            patch_format: fmt.to_string(),
            summary: format!(
                "Applied {fmt} patch cleanly -> launched separate profile '{}' (base ROM preserved).",
                patched_entry.title
            ),
        };
        Ok((res, patched_entry))
    })
    .await
    .map_err(|e| format!("ROM patch task failed: {e}"))??;

    library.insert(0, result.1);
    crate::save_library_to_disk(&app, &library)?;
    let _ = app.emit("library-updated", &library);

    Ok(result.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_bps_vli(buf: &mut Vec<u8>, mut val: u64) {
        loop {
            let x = (val & 0x7F) as u8;
            val >>= 7;
            if val == 0 {
                buf.push(x | 0x80);
                break;
            }
            buf.push(x);
            val -= 1;
        }
    }

    #[test]
    fn pure_rust_ips_bps_ups_and_vcdiff_patchers_roundtrip() {
        let src = b"HELLO WORLD ROM!".to_vec();

        // 1. IPS patch changing WORLD -> DUKE!
        let mut ips = Vec::new();
        ips.extend_from_slice(b"PATCH");
        ips.extend_from_slice(&[0x00, 0x00, 0x06]); // offset 6
        ips.extend_from_slice(&[0x00, 0x05]); // size 5
        ips.extend_from_slice(b"DUKE!");
        ips.extend_from_slice(b"EOF");
        let ips_out = apply_ips_patch(&src, &ips).unwrap();
        assert_eq!(&ips_out, b"HELLO DUKE! ROM!");

        // 2. BPS patch SourceRead(6) + TargetRead("DUKE!") + SourceRead(5) with real CRC32 footer
        let expected_dst = b"HELLO DUKE! ROM!".to_vec();
        let mut bps = Vec::new();
        bps.extend_from_slice(b"BPS1");
        write_bps_vli(&mut bps, src.len() as u64);
        write_bps_vli(&mut bps, expected_dst.len() as u64);
        write_bps_vli(&mut bps, 0);
        write_bps_vli(&mut bps, (6 - 1) << 2); // SourceRead 6
        write_bps_vli(&mut bps, ((5 - 1) << 2) | 1); // TargetRead 5
        bps.extend_from_slice(b"DUKE!");
        write_bps_vli(&mut bps, (5 - 1) << 2); // SourceRead 5
        bps.extend_from_slice(&crc32_ieee(&src).to_le_bytes());
        bps.extend_from_slice(&crc32_ieee(&expected_dst).to_le_bytes());
        let pcrc = crc32_ieee(&bps);
        bps.extend_from_slice(&pcrc.to_le_bytes());
        let bps_out = apply_bps_patch(&src, &bps).unwrap();
        assert_eq!(bps_out, expected_dst);

        // 3. UPS patch XOR delta
        let mut ups = Vec::new();
        ups.extend_from_slice(b"UPS1");
        write_bps_vli(&mut ups, src.len() as u64);
        write_bps_vli(&mut ups, expected_dst.len() as u64);
        write_bps_vli(&mut ups, 6);
        for k in 0..5 {
            ups.push(src[6 + k] ^ expected_dst[6 + k]);
        }
        ups.push(0);
        ups.extend_from_slice(&crc32_ieee(&src).to_le_bytes());
        ups.extend_from_slice(&crc32_ieee(&expected_dst).to_le_bytes());
        let ucrc = crc32_ieee(&ups);
        ups.extend_from_slice(&ucrc.to_le_bytes());
        let ups_out = apply_ups_patch(&src, &ups).unwrap();
        assert_eq!(ups_out, expected_dst);

        // 4. VCDIFF/xdelta minimal ADD window producing expected_dst
        let mut xdelta = vec![0xD6, 0xC3, 0xC4, 0x00, 0x00];
        xdelta.push(0x00); // window indicator (no src)
        xdelta.push(21); // delta encoding len
        xdelta.push(expected_dst.len() as u8); // tgt window len
        xdelta.push(0x00); // delta indicator
        xdelta.push(expected_dst.len() as u8); // data len
        xdelta.push(2); // inst len (opcode 1 + size)
        xdelta.push(0); // addr len
        xdelta.extend_from_slice(&expected_dst);
        xdelta.push(1); // ADD size=read_vcdiff_int
        xdelta.push(expected_dst.len() as u8);
        let xd_out = apply_vcdiff_xdelta_patch(&src, &xdelta).unwrap();
        assert_eq!(xd_out, expected_dst);
    }

    #[test]
    fn groups_multi_disc_roms_into_sorted_m3u() {
        let dir = std::env::temp_dir().join(format!("duke_m3u_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Xenogears (USA) (Disc 2).chd"), b"d2").unwrap();
        fs::write(dir.join("Xenogears (USA) (Disc 1).chd"), b"d1").unwrap();

        let groups = generate_m3u_playlists_in_dir(&dir).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].disc_count, 2);
        let content = fs::read_to_string(&groups[0].m3u_path).unwrap();
        assert_eq!(
            content,
            "Xenogears (USA) (Disc 1).chd\nXenogears (USA) (Disc 2).chd\n"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}

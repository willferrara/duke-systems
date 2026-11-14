//! Pure-Rust `<1 ms` ROM, ISO, CHD, RVZ, Switch, and Archive binary magic-byte inspector.
//!
//! Reads at most the first `64 KiB` (plus ISO9660 Primary Volume Descriptor at sector `0x8000`
//! and zero-extraction `.zip` central directory entries when needed) so drag-and-drop and folder
//! scanning identify Nintendo, PlayStation, Sega, Atari, Arcade, and PC Engine games accurately
//! without trusting file extensions.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::platforms::{self};

const HEADER_READ_LEN: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RomInspectionResult {
    pub path: String,
    pub platform_id: u16,
    pub platform_slug: String,
    pub platform_name: String,
    pub console_group: u8,
    pub detected_title: String,
    pub internal_title: String,
    pub rom_id: String,
    pub region: String,
    pub format_label: String,
    pub size_bytes: u64,
}

const GB_NINTENDO_LOGO_48: [u8; 48] = [
    0xCE, 0xED, 0x66, 0x66, 0xCC, 0x0D, 0x00, 0x0B, 0x03, 0x73, 0x00, 0x83, 0x00, 0x0C, 0x00, 0x0D,
    0x00, 0x08, 0x11, 0x1F, 0x88, 0x89, 0x00, 0x0E, 0xDC, 0xCC, 0x6E, 0xE6, 0xDD, 0xDD, 0xD9, 0x99,
    0xBB, 0xBB, 0x67, 0x63, 0x6E, 0x0E, 0xEC, 0xCC, 0xDD, 0xDC, 0x99, 0x9F, 0xBB, 0xB9, 0x33, 0x3E,
];

fn clean_ascii_slice(bytes: &[u8]) -> String {
    let trimmed_end = bytes
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(bytes.len());
    let s: String = bytes[..trimmed_end]
        .iter()
        .filter(|&&b| (0x20..=0x7E).contains(&b))
        .map(|&b| b as char)
        .collect();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn infer_region_from_game_code_4(code: &str, path: &Path) -> String {
    let bytes = code.as_bytes();
    if bytes.len() >= 4 {
        match bytes[3] as char {
            'E' | 'U' => return "USA".to_string(),
            'P' | 'X' | 'Y' | 'Z' | 'D' | 'F' | 'S' | 'I' => return "Europe".to_string(),
            'J' => return "Japan".to_string(),
            'K' => return "Korea".to_string(),
            'W' => return "World".to_string(),
            _ => {}
        }
    }
    infer_region_from_filename(path)
}

fn infer_region_from_filename(path: &Path) -> String {
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if name.contains("(usa)") || name.contains("(u)") {
        "USA".to_string()
    } else if name.contains("(world)") {
        "World".to_string()
    } else if name.contains("(europe)") || name.contains("(e)") {
        "Europe".to_string()
    } else if name.contains("(japan)") || name.contains("(j)") {
        "Japan".to_string()
    } else {
        String::new()
    }
}

fn title_from_filename(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Console Game");
    let cleaned = crate::retro_catalog::normalize_retro_base_title(stem);
    if cleaned.trim().is_empty() {
        stem.trim().to_string()
    } else {
        cleaned
    }
}

fn make_result(
    path: &Path,
    platform_id: u16,
    internal_title: String,
    rom_id: String,
    format_label: &str,
    size_bytes: u64,
) -> RomInspectionResult {
    let spec = platforms::platform_by_id(platform_id);
    let region = infer_region_from_game_code_4(&rom_id, path);
    let fname_title = title_from_filename(path);
    let detected_title = if !fname_title.is_empty() {
        fname_title
    } else if !internal_title.is_empty() {
        internal_title.clone()
    } else {
        spec.short_name.to_string()
    };
    RomInspectionResult {
        path: path.to_string_lossy().into_owned(),
        platform_id: spec.id,
        platform_slug: spec.slug.to_string(),
        platform_name: spec.short_name.to_string(),
        console_group: spec.console_group,
        detected_title,
        internal_title,
        rom_id,
        region,
        format_label: format_label.to_string(),
        size_bytes,
    }
}

fn validate_snes_header_at(head: &[u8], off: usize) -> Option<(String, &'static str)> {
    if off + 0x40 > head.len() {
        return None;
    }
    let slice = &head[off..off + 0x40];
    // Internal title is 21 bytes at +0x00..+0x15
    let title_bytes = &slice[0x00..0x15];
    let printable_cnt = title_bytes
        .iter()
        .filter(|&&b| (0x20..=0x7E).contains(&b))
        .count();
    if printable_cnt < 15 {
        return None;
    }
    let map_mode = slice[0x15];
    if (map_mode & 0xE0) != 0x20 {
        return None;
    }
    let complement = u16::from_le_bytes([slice[0x1C], slice[0x1D]]);
    let checksum = u16::from_le_bytes([slice[0x1E], slice[0x1F]]);
    if (checksum ^ complement) != 0xFFFF || checksum == 0 {
        return None;
    }
    let layout = if off >= 0xFFC0 {
        if (map_mode & 0x01) == 1 {
            "SNES HiROM"
        } else {
            "SNES ROM"
        }
    } else {
        "SNES LoROM"
    };
    Some((clean_ascii_slice(title_bytes), layout))
}

/// Inspects an already-loaded `<= 64 KiB` header buffer (plus optional full-file size and ISO PVD slice).
pub fn inspect_rom_bytes(
    head: &[u8],
    pvd_sector: Option<&[u8]>,
    size_bytes: u64,
    path: &Path,
) -> Option<RomInspectionResult> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    // 1. iNES / NES 2.0: `NES\x1a`
    if head.len() >= 16 && &head[0..4] == b"NES\x1A" {
        let fmt = if (head[7] & 0x0C) == 0x08 {
            "NES 2.0"
        } else {
            "iNES"
        };
        return Some(make_result(
            path,
            platforms::PLATFORM_NES,
            String::new(),
            String::new(),
            fmt,
            size_bytes,
        ));
    }

    // 2. N64 (.z64 / .v64 / .n64)
    if head.len() >= 0x40 {
        let m = &head[0..4];
        if m == [0x80, 0x37, 0x12, 0x40] {
            let internal = clean_ascii_slice(&head[0x20..0x34]);
            let id = clean_ascii_slice(&head[0x3B..0x3F]);
            return Some(make_result(
                path,
                platforms::PLATFORM_N64,
                internal,
                id,
                "N64 (.z64 BE)",
                size_bytes,
            ));
        } else if m == [0x37, 0x80, 0x40, 0x12] {
            let mut swapped = head[0..0x40].to_vec();
            for pair in swapped.chunks_exact_mut(2) {
                pair.swap(0, 1);
            }
            let internal = clean_ascii_slice(&swapped[0x20..0x34]);
            let id = clean_ascii_slice(&swapped[0x3B..0x3F]);
            return Some(make_result(
                path,
                platforms::PLATFORM_N64,
                internal,
                id,
                "N64 (.v64 ByteSwap)",
                size_bytes,
            ));
        } else if m == [0x40, 0x12, 0x37, 0x80] {
            let mut swapped = head[0..0x40].to_vec();
            for quad in swapped.chunks_exact_mut(4) {
                quad.reverse();
            }
            let internal = clean_ascii_slice(&swapped[0x20..0x34]);
            let id = clean_ascii_slice(&swapped[0x3B..0x3F]);
            return Some(make_result(
                path,
                platforms::PLATFORM_N64,
                internal,
                id,
                "N64 (.n64 LE)",
                size_bytes,
            ));
        }
    }

    // 3. Dolphin RVZ / WIA container (`RVZ\x01` / `WIA\x01`, inner disc header at 0x58)
    if head.len() >= 0xC0 && (&head[0..4] == b"RVZ\x01" || &head[0..4] == b"WIA\x01") {
        let disc_hdr = &head[0x58..];
        let game_id = clean_ascii_slice(&disc_hdr[0x00..0x06]);
        let internal = clean_ascii_slice(&disc_hdr[0x20..0x60]);
        let is_wii = disc_hdr[0x18..0x1C] == [0x5D, 0x1C, 0x9E, 0xA3];
        let is_gc = disc_hdr[0x1C..0x20] == [0xC2, 0x33, 0x9F, 0x3D];
        if is_wii || is_gc {
            let pid = if is_wii {
                platforms::PLATFORM_WII
            } else {
                platforms::PLATFORM_GAMECUBE
            };
            let fmt = if &head[0..4] == b"RVZ\x01" { "Dolphin RVZ" } else { "Dolphin WIA" };
            return Some(make_result(path, pid, internal, game_id, fmt, size_bytes));
        }
    }

    // 4. Raw WBFS Wii image
    if head.len() >= 0x260 && &head[0..4] == b"WBFS" && head[0x218..0x21C] == [0x5D, 0x1C, 0x9E, 0xA3] {
        let game_id = clean_ascii_slice(&head[0x200..0x206]);
        let internal = clean_ascii_slice(&head[0x220..0x260]);
        return Some(make_result(
            path,
            platforms::PLATFORM_WII,
            internal,
            game_id,
            "Wii WBFS",
            size_bytes,
        ));
    }

    // 5. Raw GameCube (.iso/.gcm) & Wii (.iso) magics at 0x18 / 0x1C
    if head.len() >= 0x60 {
        if head[0x18..0x1C] == [0x5D, 0x1C, 0x9E, 0xA3] {
            let game_id = clean_ascii_slice(&head[0x00..0x06]);
            let internal = clean_ascii_slice(&head[0x20..0x60]);
            return Some(make_result(
                path,
                platforms::PLATFORM_WII,
                internal,
                game_id,
                "Wii Disc ISO",
                size_bytes,
            ));
        }
        if head[0x1C..0x20] == [0xC2, 0x33, 0x9F, 0x3D] {
            let game_id = clean_ascii_slice(&head[0x00..0x06]);
            let internal = clean_ascii_slice(&head[0x20..0x60]);
            return Some(make_result(
                path,
                platforms::PLATFORM_GAMECUBE,
                internal,
                game_id,
                "GameCube Disc Image",
                size_bytes,
            ));
        }
    }

    // 6. Wii U (`WUX0` / `.wud` / `.wua` / `.rpx`)
    if head.len() >= 8 && &head[0..4] == b"WUX0" {
        return Some(make_result(
            path,
            platforms::PLATFORM_WII_U,
            String::new(),
            String::new(),
            "Wii U WUX",
            size_bytes,
        ));
    }
    if ext == "wua" || (ext == "rpx" && head.len() >= 20 && &head[0..4] == b"\x7FELF") {
        return Some(make_result(
            path,
            platforms::PLATFORM_WII_U,
            String::new(),
            String::new(),
            if ext == "wua" { "Wii U WUA Archive" } else { "Wii U RPX" },
            size_bytes,
        ));
    }

    // 7. Nintendo Switch (`PFS0` .nsp / `HEAD` at 0x100 .xci / `NRO0` at 0x10 homebrew)
    if head.len() >= 16 && &head[0..4] == b"PFS0" {
        return Some(make_result(
            path,
            platforms::PLATFORM_SWITCH,
            String::new(),
            String::new(),
            "Switch NSP (PFS0)",
            size_bytes,
        ));
    }
    if head.len() >= 0x110 && &head[0x100..0x104] == b"HEAD" {
        return Some(make_result(
            path,
            platforms::PLATFORM_SWITCH,
            String::new(),
            String::new(),
            "Switch XCI Gamecard",
            size_bytes,
        ));
    }
    if head.len() >= 0x20 && &head[0x10..0x14] == b"NRO0" {
        return Some(make_result(
            path,
            platforms::PLATFORM_SWITCH,
            String::new(),
            String::new(),
            "Switch NRO Homebrew",
            size_bytes,
        ));
    }

    // 8. Nintendo 3DS (`NCSD` / `NCCH` at 0x100)
    if head.len() >= 0x120 && &head[0x100..0x104] == b"NCSD" {
        let media_id = hex::encode_upper(&head[0x108..0x110]);
        return Some(make_result(
            path,
            platforms::PLATFORM_3DS,
            String::new(),
            media_id,
            "Nintendo 3DS NCSD",
            size_bytes,
        ));
    }
    if head.len() >= 0x160 && &head[0x100..0x104] == b"NCCH" && matches!(ext.as_str(), "3ds" | "cci" | "cxi" | "cia" | "") {
        let product_code = clean_ascii_slice(&head[0x150..0x160]);
        if !product_code.is_empty() || ext == "cci" || ext == "cxi" || ext == "3ds" {
            return Some(make_result(
                path,
                platforms::PLATFORM_3DS,
                String::new(),
                product_code,
                "Nintendo 3DS NCCH",
                size_bytes,
            ));
        }
    }

    // 9. Game Boy / Game Boy Color (Nintendo logo at 0x104..0x134)
    if head.len() >= 0x150 && head[0x104..0x134] == GB_NINTENDO_LOGO_48 {
        let cgb_flag = head[0x143];
        let is_gbc = cgb_flag == 0x80 || cgb_flag == 0xC0;
        let title_end = if is_gbc || (head[0x13F..0x143].iter().all(|&b| b.is_ascii_uppercase() || b.is_ascii_digit())) {
            0x143
        } else {
            0x144
        };
        let internal = clean_ascii_slice(&head[0x134..title_end]);
        let pid = if is_gbc {
            platforms::PLATFORM_GBC
        } else {
            platforms::PLATFORM_GB
        };
        return Some(make_result(
            path,
            pid,
            internal,
            String::new(),
            if is_gbc { "Game Boy Color" } else { "Game Boy" },
            size_bytes,
        ));
    }

    // 10. Game Boy Advance (fixed byte 0x96 at 0xB2 + Nintendo compressed logo bitmap header)
    if head.len() >= 0xC0 && head[0xB2] == 0x96 && head[0x04..0x0C] == [0x24, 0xFF, 0xAE, 0x51, 0x69, 0x9A, 0xA2, 0x21] {
        let internal = clean_ascii_slice(&head[0xA0..0xAC]);
        let code = clean_ascii_slice(&head[0xAC..0xB0]);
        return Some(make_result(
            path,
            platforms::PLATFORM_GBA,
            internal,
            code,
            "Game Boy Advance",
            size_bytes,
        ));
    }

    // 11. Nintendo DS (logo signature at 0xC0 + logo CRC 0xCF56 at 0x15C)
    if head.len() >= 0x160
        && head[0xC0..0xC8] == [0x24, 0xFF, 0xAE, 0x51, 0x69, 0x9A, 0xA2, 0x21]
        && (u16::from_le_bytes([head[0x15C], head[0x15D]]) == 0xCF56 || ext == "nds")
    {
        let internal = clean_ascii_slice(&head[0x00..0x0C]);
        let code = clean_ascii_slice(&head[0x0C..0x10]);
        return Some(make_result(
            path,
            platforms::PLATFORM_NDS,
            internal,
            code,
            "Nintendo DS",
            size_bytes,
        ));
    }

    // 12. Sega Genesis / Mega Drive (`SEGA MEGA DRIVE` / `SEGA GENESIS` at 0x100)
    if head.len() >= 0x190
        && (&head[0x100..0x10F] == b"SEGA MEGA DRIVE"
            || &head[0x100..0x10C] == b"SEGA GENESIS"
            || &head[0x100..0x108] == b"SEGA 32X")
    {
        let internal = clean_ascii_slice(&head[0x150..0x180]);
        let internal_fb = if internal.is_empty() {
            clean_ascii_slice(&head[0x120..0x150])
        } else {
            internal
        };
        let serial = clean_ascii_slice(&head[0x180..0x18E]);
        return Some(make_result(
            path,
            platforms::PLATFORM_GENESIS,
            internal_fb,
            serial,
            "Sega Genesis / Mega Drive",
            size_bytes,
        ));
    }

    // 13. Sega Master System (`TMR SEGA` at 0x7FF0, plus SMS copier 0x81F0)
    if (head.len() >= 0x8000 && &head[0x7FF0..0x7FF8] == b"TMR SEGA")
        || (head.len() >= 0x8200 && &head[0x81F0..0x81F8] == b"TMR SEGA")
    {
        return Some(make_result(
            path,
            platforms::PLATFORM_SMS,
            String::new(),
            String::new(),
            "Sega Master System",
            size_bytes,
        ));
    }

    // 14. PSP CSO compressed ISO (`CISO`)
    if head.len() >= 0x18 && &head[0..4] == b"CISO" {
        return Some(make_result(
            path,
            platforms::PLATFORM_PSP,
            String::new(),
            String::new(),
            "PSP CISO",
            size_bytes,
        ));
    }

    // 15. Sony PS3 PKG / SELF or PSP PBP (`\x00PBP`)
    if head.len() >= 4 && &head[0..4] == b"\x00PBP" {
        return Some(make_result(
            path,
            platforms::PLATFORM_PSP,
            String::new(),
            String::new(),
            "PSP EBOOT PBP",
            size_bytes,
        ));
    }
    if head.len() >= 4 && &head[0..4] == b"\x7FPKG" {
        return Some(make_result(
            path,
            platforms::PLATFORM_PS3,
            String::new(),
            String::new(),
            "PS3 PKG",
            size_bytes,
        ));
    }

    // 16. Sector 0 IP.BIN Sega Saturn / Dreamcast strings
    if head.len() >= 0x30 {
        let first64 = clean_ascii_slice(&head[0..head.len().min(0x100)]);
        if first64.contains("SEGA SEGASATURN") {
            return Some(make_result(
                path,
                platforms::PLATFORM_SATURN,
                String::new(),
                String::new(),
                "Sega Saturn Image",
                size_bytes,
            ));
        }
        if first64.contains("SEGA SEGAKATANA") {
            return Some(make_result(
                path,
                platforms::PLATFORM_DREAMCAST,
                String::new(),
                String::new(),
                "Sega Dreamcast Image",
                size_bytes,
            ));
        }
    }

    // 17. ISO9660 Primary Volume Descriptor (`\x01CD001`) at standard sector 0x8000 or Mode2 raw 0x9318
    for pvd_candidate in [
        pvd_sector,
        head.get(0x8000..0x8800),
        head.get(0x9318..0x9B18),
    ]
    .into_iter()
    .flatten()
    {
        if pvd_candidate.len() >= 0x80 && &pvd_candidate[0..6] == b"\x01CD001" {
            let sys_id = clean_ascii_slice(&pvd_candidate[8..40]).to_ascii_uppercase();
            let vol_id = clean_ascii_slice(&pvd_candidate[40..72]);
            let pid = if sys_id.contains("PSP GAME") {
                platforms::PLATFORM_PSP
            } else if sys_id.contains("SEGAKATANA") {
                platforms::PLATFORM_DREAMCAST
            } else if sys_id.contains("SEGASATURN") {
                platforms::PLATFORM_SATURN
            } else if sys_id.contains("PLAYSTATION") {
                let lower_path = path.to_string_lossy().to_ascii_lowercase();
                if lower_path.contains("ps2")
                    || lower_path.contains("playstation 2")
                    || size_bytes > 750 * 1024 * 1024
                {
                    platforms::PLATFORM_PS2
                } else {
                    platforms::PLATFORM_PS1
                }
            } else {
                0
            };
            if pid > 0 {
                return Some(make_result(
                    path,
                    pid,
                    vol_id,
                    String::new(),
                    "ISO9660 Disc Image",
                    size_bytes,
                ));
            }
        }
    }

    // 18. MAME / Redump CHD (`MComprHD`) v1-v5 container
    if head.len() >= 0x4C && &head[0..8] == b"MComprHD" {
        let chd_version = u32::from_be_bytes([head[12], head[13], head[14], head[15]]);
        if (1..=5).contains(&chd_version) {
            let lower_path = path.to_string_lossy().to_ascii_lowercase();
            let pid = infer_disc_platform_from_path_hint(&lower_path)
                .unwrap_or(platforms::PLATFORM_PS1);
            return Some(make_result(
                path,
                pid,
                String::new(),
                String::new(),
                &format!("CHD v{}", chd_version),
                size_bytes,
            ));
        }
    }

    // 19. SNES LoROM (0x7FC0) / HiROM (0xFFC0) with optional 512-byte SMC copier header
    for off in [0x7FC0usize, 0xFFC0, 0x81C0, 0x101C0] {
        if let Some((internal, layout)) = validate_snes_header_at(head, off) {
            return Some(make_result(
                path,
                platforms::PLATFORM_SNES,
                internal,
                String::new(),
                layout,
                size_bytes,
            ));
        }
    }

    // 20. Extension fallback strictly for unambiguous retro formats (Atari 2600, PC Engine, VB, etc.)
    let ext_pid = match ext.as_str() {
        "nes" | "unf" => Some((platforms::PLATFORM_NES, "NES ROM")),
        "sfc" | "smc" => Some((platforms::PLATFORM_SNES, "SNES ROM")),
        "z64" | "v64" | "n64" => Some((platforms::PLATFORM_N64, "N64 ROM")),
        "gcm" | "rvz" => Some((platforms::PLATFORM_GAMECUBE, "GameCube Image")),
        "wbfs" => Some((platforms::PLATFORM_WII, "Wii WBFS")),
        "wux" | "wua" | "wud" | "rpx" => Some((platforms::PLATFORM_WII_U, "Wii U Image")),
        "nsp" | "xci" => Some((platforms::PLATFORM_SWITCH, "Switch Image")),
        "gb" => Some((platforms::PLATFORM_GB, "Game Boy ROM")),
        "gbc" => Some((platforms::PLATFORM_GBC, "Game Boy Color ROM")),
        "gba" => Some((platforms::PLATFORM_GBA, "GBA ROM")),
        "nds" => Some((platforms::PLATFORM_NDS, "Nintendo DS ROM")),
        "3ds" | "cci" | "cia" => Some((platforms::PLATFORM_3DS, "Nintendo 3DS ROM")),
        "vb" => Some((platforms::PLATFORM_VIRTUAL_BOY, "Virtual Boy ROM")),
        "md" | "gen" | "smd" => Some((platforms::PLATFORM_GENESIS, "Genesis ROM")),
        "sms" => Some((platforms::PLATFORM_SMS, "Master System ROM")),
        "cdi" | "gdi" => Some((platforms::PLATFORM_DREAMCAST, "Dreamcast Image")),
        "cso" | "pbp" => Some((platforms::PLATFORM_PSP, "PSP Image")),
        "a26" => Some((platforms::PLATFORM_ATARI_2600, "Atari 2600 ROM")),
        "pce" | "sgx" => Some((platforms::PLATFORM_PC_ENGINE, "PC Engine ROM")),
        "chd" => Some((
            infer_disc_platform_from_path_hint(&path.to_string_lossy().to_ascii_lowercase())
                .unwrap_or(platforms::PLATFORM_PS1),
            "CHD Image",
        )),
        "cue" | "iso" => infer_disc_platform_from_path_hint(&path.to_string_lossy().to_ascii_lowercase())
            .map(|p| (p, "Disc Cue/ISO")),
        _ => None,
    };

    ext_pid.map(|(pid, fmt)| make_result(path, pid, String::new(), String::new(), fmt, size_bytes))
}

fn infer_disc_platform_from_path_hint(lower_path: &str) -> Option<u16> {
    if lower_path.contains("ps2") || lower_path.contains("playstation 2") {
        Some(platforms::PLATFORM_PS2)
    } else if lower_path.contains("ps3") || lower_path.contains("playstation 3") {
        Some(platforms::PLATFORM_PS3)
    } else if lower_path.contains("psp") || lower_path.contains("portable") {
        Some(platforms::PLATFORM_PSP)
    } else if lower_path.contains("ps1")
        || lower_path.contains("psx")
        || lower_path.contains("playstation")
    {
        Some(platforms::PLATFORM_PS1)
    } else if lower_path.contains("dreamcast") || lower_path.contains("\\dc\\") {
        Some(platforms::PLATFORM_DREAMCAST)
    } else if lower_path.contains("saturn") {
        Some(platforms::PLATFORM_SATURN)
    } else if lower_path.contains("gamecube") || lower_path.contains("\\gc\\") {
        Some(platforms::PLATFORM_GAMECUBE)
    } else if lower_path.contains("wii u") || lower_path.contains("wiiu") {
        Some(platforms::PLATFORM_WII_U)
    } else if lower_path.contains("wii") {
        Some(platforms::PLATFORM_WII)
    } else if lower_path.contains("pc engine") || lower_path.contains("turbografx") {
        Some(platforms::PLATFORM_PC_ENGINE)
    } else if lower_path.contains("mame") || lower_path.contains("fbneo") || lower_path.contains("neogeo") || lower_path.contains("arcade") {
        Some(platforms::PLATFORM_ARCADE)
    } else {
        None
    }
}

/// Zero-extraction `.zip` central directory + first-member 64 KiB header inspector.
fn inspect_zip_rom(file: &mut File, size_bytes: u64, path: &Path) -> Option<RomInspectionResult> {
    let tail_len = size_bytes.min(64 * 1024) as usize;
    if tail_len < 22 {
        return None;
    }
    let _ = file.seek(SeekFrom::Start(size_bytes - tail_len as u64)).ok()?;
    let mut tail = vec![0u8; tail_len];
    let _ = file.read(&mut tail).ok()?;

    // Locate EOCD (`PK\x05\x06`)
    let mut eocd_off = None;
    for i in (0..=tail.len() - 22).rev() {
        if &tail[i..i + 4] == b"PK\x05\x06" {
            eocd_off = Some(i);
            break;
        }
    }
    let eocd_off = eocd_off?;
    let total_entries = u16::from_le_bytes([tail[eocd_off + 10], tail[eocd_off + 11]]) as usize;
    let cd_size = u32::from_le_bytes(tail[eocd_off + 12..eocd_off + 16].try_into().ok()?) as usize;
    let cd_offset = u32::from_le_bytes(tail[eocd_off + 16..eocd_off + 20].try_into().ok()?) as u64;

    if cd_offset + (cd_size as u64) > size_bytes || total_entries == 0 {
        return None;
    }
    let read_cd = cd_size.min(128 * 1024);
    file.seek(SeekFrom::Start(cd_offset)).ok()?;
    let mut cd = vec![0u8; read_cd];
    let n = file.read(&mut cd).ok()?;
    let cd = &cd[..n];

    let mut pos = 0usize;
    let mut member_names = Vec::new();
    let mut first_rom_meta: Option<(u16, u32, u32, String)> = None;

    while pos + 46 <= cd.len() && &cd[pos..pos + 4] == b"PK\x01\x02" {
        let method = u16::from_le_bytes([cd[pos + 10], cd[pos + 11]]);
        let comp_size = u32::from_le_bytes(cd[pos + 20..pos + 24].try_into().ok()?);
        let name_len = u16::from_le_bytes([cd[pos + 28], cd[pos + 29]]) as usize;
        let extra_len = u16::from_le_bytes([cd[pos + 30], cd[pos + 31]]) as usize;
        let comment_len = u16::from_le_bytes([cd[pos + 32], cd[pos + 33]]) as usize;
        let local_off = u32::from_le_bytes(cd[pos + 42..pos + 46].try_into().ok()?);
        if pos + 46 + name_len > cd.len() {
            break;
        }
        let name = String::from_utf8_lossy(&cd[pos + 46..pos + 46 + name_len]).into_owned();
        if first_rom_meta.is_none()
            && !name.ends_with('/')
            && !name.to_ascii_lowercase().ends_with(".txt")
            && !name.to_ascii_lowercase().ends_with(".nfo")
        {
            first_rom_meta = Some((method, comp_size, local_off, name.clone()));
        }
        member_names.push(name);
        pos += 46 + name_len + extra_len + comment_len;
    }

    // Single-file ROM zipped cleanly: inspect up to 64 KiB of the member bytes in memory
    if member_names.len() == 1 {
        if let Some((method, comp_size, local_off, member_name)) = first_rom_meta {
            if let Some(hdr) = read_zip_member_prefix(file, local_off as u64, method, comp_size) {
                let virtual_path = path.with_file_name(member_name);
                if let Some(mut res) = inspect_rom_bytes(&hdr, None, size_bytes, &virtual_path) {
                    res.path = path.to_string_lossy().into_owned();
                    res.detected_title = title_from_filename(path);
                    res.format_label = format!("Zipped {}", res.format_label);
                    return Some(res);
                }
            }
        }
    }

    // Check internal filenames against retro console extensions
    for m in &member_names {
        let p = Path::new(m);
        if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
            let low = ext.to_ascii_lowercase();
            let pid_opt = match low.as_str() {
                "nes" => Some(platforms::PLATFORM_NES),
                "sfc" | "smc" => Some(platforms::PLATFORM_SNES),
                "z64" | "v64" | "n64" => Some(platforms::PLATFORM_N64),
                "gb" => Some(platforms::PLATFORM_GB),
                "gbc" => Some(platforms::PLATFORM_GBC),
                "gba" => Some(platforms::PLATFORM_GBA),
                "nds" => Some(platforms::PLATFORM_NDS),
                "3ds" | "cci" => Some(platforms::PLATFORM_3DS),
                "md" | "gen" | "smd" => Some(platforms::PLATFORM_GENESIS),
                "sms" => Some(platforms::PLATFORM_SMS),
                "a26" => Some(platforms::PLATFORM_ATARI_2600),
                "pce" => Some(platforms::PLATFORM_PC_ENGINE),
                "vb" => Some(platforms::PLATFORM_VIRTUAL_BOY),
                _ => None,
            };
            if let Some(pid) = pid_opt {
                return Some(make_result(
                    path,
                    pid,
                    String::new(),
                    String::new(),
                    "Zipped ROM Archive",
                    size_bytes,
                ));
            }
        }
    }

    // MAME / FBNeo arcade sets contain multi-chip `.rom`, `.bin`, `.ic*`, `.u*`, `.c1` dumps
    let looks_like_arcade = member_names.len() >= 2
        && member_names.iter().any(|m| {
            let lm = m.to_ascii_lowercase();
            lm.ends_with(".rom")
                || lm.ends_with(".ic1")
                || lm.contains("-p1")
                || lm.contains("-c1")
                || lm.contains("_m1")
                || lm.ends_with(".u1")
        });
    if looks_like_arcade {
        return Some(make_result(
            path,
            platforms::PLATFORM_ARCADE,
            String::new(),
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string(),
            "Arcade / Neo Geo ROM Set (ZIP)",
            size_bytes,
        ));
    }

    None
}

fn read_zip_member_prefix(
    file: &mut File,
    local_header_off: u64,
    method: u16,
    comp_size: u32,
) -> Option<Vec<u8>> {
    file.seek(SeekFrom::Start(local_header_off)).ok()?;
    let mut lfh = [0u8; 30];
    file.read_exact(&mut lfh).ok()?;
    if &lfh[0..4] != b"PK\x03\x04" {
        return None;
    }
    let name_len = u16::from_le_bytes([lfh[26], lfh[27]]) as u64;
    let extra_len = u16::from_le_bytes([lfh[28], lfh[29]]) as u64;
    file.seek(SeekFrom::Start(local_header_off + 30 + name_len + extra_len))
        .ok()?;
    let max_comp = (comp_size as usize).min(HEADER_READ_LEN * 2);
    let mut comp_buf = vec![0u8; max_comp];
    let n = file.read(&mut comp_buf).ok()?;
    comp_buf.truncate(n);

    if method == 0 {
        comp_buf.truncate(HEADER_READ_LEN);
        Some(comp_buf)
    } else if method == 8 {
        let mut decoder = flate2::read::DeflateDecoder::new(&comp_buf[..]);
        let mut out = vec![0u8; HEADER_READ_LEN];
        let mut total = 0usize;
        while total < HEADER_READ_LEN {
            match decoder.read(&mut out[total..]) {
                Ok(0) => break,
                Ok(k) => total += k,
                Err(_) => break,
            }
        }
        out.truncate(total);
        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    } else {
        None
    }
}

/// Inspects a file on disk in `<1 ms`, reading at most the first `64 KiB` (+ ISO9660 PVD or `.zip` tail).
pub fn inspect_rom_file(path: &Path) -> Option<RomInspectionResult> {
    if !path.is_file() {
        return None;
    }
    let mut file = File::open(path).ok()?;
    let size_bytes = file.metadata().map(|m| m.len()).unwrap_or(0);
    if size_bytes < 16 {
        return None;
    }

    let read_len = size_bytes.min(HEADER_READ_LEN as u64) as usize;
    let mut head = vec![0u8; read_len];
    let n = file.read(&mut head).ok()?;
    head.truncate(n);

    if head.len() >= 4 && &head[0..4] == b"PK\x03\x04" {
        if let Some(zip_res) = inspect_zip_rom(&mut file, size_bytes, path) {
            return Some(zip_res);
        }
    }

    inspect_rom_bytes(&head, None, size_bytes, path)
}

/// Walks a dropped or chosen folder looking for ROMs/ISOs while avoiding `.cue`+`.bin` duplicates.
pub fn scan_folder_for_roms(dir: &Path, max_depth: usize) -> Vec<RomInspectionResult> {
    let mut cue_referenced_bins: HashSet<String> = HashSet::new();
    let mut candidate_files: Vec<PathBuf> = Vec::new();

    for entry in walkdir::WalkDir::new(dir)
        .max_depth(max_depth)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let p = entry.path();
        if p.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("cue"))
            .unwrap_or(false)
        {
            if let Ok(txt) = fs::read_to_string(p) {
                for line in txt.lines() {
                    let tr = line.trim();
                    if tr.to_ascii_uppercase().starts_with("FILE ") {
                        if let Some(first_q) = tr.find('"') {
                            if let Some(rel) = tr[first_q + 1..].find('"') {
                                let bin_name = &tr[first_q + 1..first_q + 1 + rel];
                                if let Some(parent) = p.parent() {
                                    cue_referenced_bins.insert(
                                        parent.join(bin_name).to_string_lossy().to_ascii_lowercase(),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        candidate_files.push(p.to_path_buf());
    }

    let mut results = Vec::new();
    for path in candidate_files {
        let lower = path.to_string_lossy().to_ascii_lowercase();
        if lower.ends_with(".bin") && cue_referenced_bins.contains(&lower) {
            continue;
        }
        // Skip unrelated executables/libraries during ROM scanning
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if matches!(
            ext.as_str(),
            "exe" | "dll" | "lnk" | "url" | "png" | "jpg" | "jpeg" | "txt" | "sav" | "srm" | "state" | "json"
        ) {
            continue;
        }
        if let Some(res) = inspect_rom_file(&path) {
            results.push(res);
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn detects_all_nintendo_rom_and_disc_headers() {
        // 1. NES iNES 2.0
        let mut nes = vec![0u8; 64];
        nes[0..4].copy_from_slice(b"NES\x1A");
        nes[7] = 0x08;
        let r = inspect_rom_bytes(&nes, None, 4096, Path::new("C:\\roms\\mario.nes")).unwrap();
        assert_eq!(r.platform_id, platforms::PLATFORM_NES);
        assert_eq!(r.format_label, "NES 2.0");

        // 2. SNES LoROM checksum valid + 512B copier variant
        let mut snes = vec![0u8; 0x8000];
        snes[0x7FC0..0x7FD5].copy_from_slice(b"CHRONO TRIGGER       ");
        snes[0x7FD5] = 0x20;
        snes[0x7FDC..0x7FDE].copy_from_slice(&0x55AAu16.to_le_bytes());
        snes[0x7FDE..0x7FE0].copy_from_slice(&0xAA55u16.to_le_bytes());
        let r = inspect_rom_bytes(&snes, None, 0x8000, Path::new("chrono.bin")).unwrap();
        assert_eq!(r.platform_id, platforms::PLATFORM_SNES);
        assert_eq!(r.internal_title, "CHRONO TRIGGER");

        let mut snes_copier = vec![0u8; 0x8200];
        snes_copier[0x81C0..0x81D5].copy_from_slice(b"SUPER METROID        ");
        snes_copier[0x81D5] = 0x20;
        snes_copier[0x81DC..0x81DE].copy_from_slice(&0x1234u16.to_le_bytes());
        snes_copier[0x81DE..0x81E0].copy_from_slice(&0xEDCBu16.to_le_bytes());
        let r_smc = inspect_rom_bytes(&snes_copier, None, 0x8200, Path::new("metroid.smc")).unwrap();
        assert_eq!(r_smc.platform_id, platforms::PLATFORM_SNES);

        // 3. N64 .z64 / .v64 / .n64
        let mut z64 = vec![0u8; 0x100];
        z64[0..4].copy_from_slice(&[0x80, 0x37, 0x12, 0x40]);
        z64[0x20..0x34].copy_from_slice(b"SUPER MARIO 64      ");
        z64[0x3B..0x3F].copy_from_slice(b"NSME");
        let r_z64 = inspect_rom_bytes(&z64, None, 8_388_608, Path::new("sm64.rom")).unwrap();
        assert_eq!(r_z64.platform_id, platforms::PLATFORM_N64);
        assert_eq!(r_z64.rom_id, "NSME");
        assert_eq!(r_z64.region, "USA");

        let mut v64 = z64.clone();
        for p in v64.chunks_exact_mut(2) {
            p.swap(0, 1);
        }
        let r_v64 = inspect_rom_bytes(&v64, None, 8_388_608, Path::new("sm64.rom")).unwrap();
        assert_eq!(r_v64.platform_id, platforms::PLATFORM_N64);
        assert_eq!(r_v64.internal_title, "SUPER MARIO 64");

        // 4. GameCube / Wii / Dolphin RVZ / Wii U / Switch
        let mut gc = vec![0u8; 0x100];
        gc[0..6].copy_from_slice(b"GM8E01");
        gc[0x1C..0x20].copy_from_slice(&[0xC2, 0x33, 0x9F, 0x3D]);
        gc[0x20..0x2D].copy_from_slice(b"Metroid Prime");
        let r_gc = inspect_rom_bytes(&gc, None, 1_400_000_000, Path::new("prime.iso")).unwrap();
        assert_eq!(r_gc.platform_id, platforms::PLATFORM_GAMECUBE);
        assert_eq!(r_gc.rom_id, "GM8E01");

        let mut rvz_wii = vec![0u8; 0x100];
        rvz_wii[0..4].copy_from_slice(b"RVZ\x01");
        rvz_wii[0x58..0x5E].copy_from_slice(b"RMGE01");
        rvz_wii[0x58 + 0x18..0x58 + 0x1C].copy_from_slice(&[0x5D, 0x1C, 0x9E, 0xA3]);
        rvz_wii[0x58 + 0x20..0x58 + 0x32].copy_from_slice(b"Super Mario Galaxy");
        let r_rvz = inspect_rom_bytes(&rvz_wii, None, 1_200_000_000, Path::new("galaxy.rvz")).unwrap();
        assert_eq!(r_rvz.platform_id, platforms::PLATFORM_WII);
        assert_eq!(r_rvz.format_label, "Dolphin RVZ");
        assert_eq!(r_rvz.rom_id, "RMGE01");

        let mut nsp = vec![0u8; 64];
        nsp[0..4].copy_from_slice(b"PFS0");
        assert_eq!(
            inspect_rom_bytes(&nsp, None, 500_000_000, Path::new("game.nsp"))
                .unwrap()
                .platform_id,
            platforms::PLATFORM_SWITCH
        );

        let mut xci = vec![0u8; 0x200];
        xci[0x100..0x104].copy_from_slice(b"HEAD");
        assert_eq!(
            inspect_rom_bytes(&xci, None, 2_000_000_000, Path::new("game.xci"))
                .unwrap()
                .platform_id,
            platforms::PLATFORM_SWITCH
        );

        let mut wux = vec![0u8; 64];
        wux[0..4].copy_from_slice(b"WUX0");
        assert_eq!(
            inspect_rom_bytes(&wux, None, 2_000_000_000, Path::new("game.wux"))
                .unwrap()
                .platform_id,
            platforms::PLATFORM_WII_U
        );

        // 5. GB / GBC / GBA / NDS / 3DS
        let mut gbc = vec![0u8; 0x150];
        gbc[0x104..0x134].copy_from_slice(&GB_NINTENDO_LOGO_48);
        gbc[0x134..0x143].copy_from_slice(b"POKEMON YELL   ");
        gbc[0x143] = 0x80;
        assert_eq!(
            inspect_rom_bytes(&gbc, None, 1_048_576, Path::new("yellow.rom"))
                .unwrap()
                .platform_id,
            platforms::PLATFORM_GBC
        );

        let mut gba = vec![0u8; 0xD0];
        gba[0x04..0x0C].copy_from_slice(&[0x24, 0xFF, 0xAE, 0x51, 0x69, 0x9A, 0xA2, 0x21]);
        gba[0xA0..0xAC].copy_from_slice(b"METROID4    ");
        gba[0xAC..0xB0].copy_from_slice(b"AMTE");
        gba[0xB2] = 0x96;
        let r_gba = inspect_rom_bytes(&gba, None, 8_388_608, Path::new("fusion.bin")).unwrap();
        assert_eq!(r_gba.platform_id, platforms::PLATFORM_GBA);
        assert_eq!(r_gba.rom_id, "AMTE");

        let mut nds = vec![0u8; 0x160];
        nds[0x00..0x0C].copy_from_slice(b"MARIOKARTDS ");
        nds[0x0C..0x10].copy_from_slice(b"AMCE");
        nds[0xC0..0xC8].copy_from_slice(&[0x24, 0xFF, 0xAE, 0x51, 0x69, 0x9A, 0xA2, 0x21]);
        nds[0x15C..0x15E].copy_from_slice(&0xCF56u16.to_le_bytes());
        assert_eq!(
            inspect_rom_bytes(&nds, None, 32_000_000, Path::new("mkds.rom"))
                .unwrap()
                .platform_id,
            platforms::PLATFORM_NDS
        );

        let mut cci = vec![0u8; 0x200];
        cci[0x100..0x104].copy_from_slice(b"NCSD");
        assert_eq!(
            inspect_rom_bytes(&cci, None, 512_000_000, Path::new("zelda.3ds"))
                .unwrap()
                .platform_id,
            platforms::PLATFORM_3DS
        );
    }

    #[test]
    fn detects_playstation_sega_chd_and_zipped_roms() {
        // PS1 ISO9660 PVD
        let mut ps1 = vec![0u8; 0x8800];
        ps1[0x8000..0x8006].copy_from_slice(b"\x01CD001");
        ps1[0x8008..0x8013].copy_from_slice(b"PLAYSTATION");
        ps1[0x8028..0x8033].copy_from_slice(b"CASTLEVANIA");
        assert_eq!(
            inspect_rom_bytes(&ps1, None, 500 * 1024 * 1024, Path::new("sotn.iso"))
                .unwrap()
                .platform_id,
            platforms::PLATFORM_PS1
        );

        // Genesis
        let mut gen = vec![0u8; 0x200];
        gen[0x100..0x10F].copy_from_slice(b"SEGA MEGA DRIVE");
        gen[0x150..0x160].copy_from_slice(b"SONIC THE HEDGEH");
        gen[0x180..0x188].copy_from_slice(b"GM 00100");
        assert_eq!(
            inspect_rom_bytes(&gen, None, 524_288, Path::new("sonic.bin"))
                .unwrap()
                .platform_id,
            platforms::PLATFORM_GENESIS
        );

        // CHD
        let mut chd = vec![0u8; 0x60];
        chd[0..8].copy_from_slice(b"MComprHD");
        chd[12..16].copy_from_slice(&5u32.to_be_bytes());
        assert_eq!(
            inspect_rom_bytes(&chd, None, 700_000_000, Path::new("C:\\Dreamcast\\crazy.chd"))
                .unwrap()
                .platform_id,
            platforms::PLATFORM_DREAMCAST
        );

        // Zipped iNES ROM inspected via EOCD central directory + stored member header
        let dir = std::env::temp_dir().join(format!("duke_zip_rom_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let zip_path = dir.join("Castlevania (USA).zip");
        let mut nes_payload = vec![0u8; 32];
        nes_payload[0..4].copy_from_slice(b"NES\x1A");
        let name = b"Castlevania.nes";
        let mut zip_buf = Vec::new();
        // Local file header
        zip_buf.extend_from_slice(b"PK\x03\x04");
        zip_buf.extend_from_slice(&20u16.to_le_bytes()); // version
        zip_buf.extend_from_slice(&0u16.to_le_bytes()); // flags
        zip_buf.extend_from_slice(&0u16.to_le_bytes()); // method=store
        zip_buf.extend_from_slice(&0u16.to_le_bytes()); // mtime
        zip_buf.extend_from_slice(&0u16.to_le_bytes()); // mdate
        zip_buf.extend_from_slice(&0u32.to_le_bytes()); // crc
        zip_buf.extend_from_slice(&(nes_payload.len() as u32).to_le_bytes());
        zip_buf.extend_from_slice(&(nes_payload.len() as u32).to_le_bytes());
        zip_buf.extend_from_slice(&(name.len() as u16).to_le_bytes());
        zip_buf.extend_from_slice(&0u16.to_le_bytes());
        zip_buf.extend_from_slice(name);
        zip_buf.extend_from_slice(&nes_payload);
        let cd_off = zip_buf.len() as u32;
        // Central directory header
        zip_buf.extend_from_slice(b"PK\x01\x02");
        zip_buf.extend_from_slice(&20u16.to_le_bytes());
        zip_buf.extend_from_slice(&20u16.to_le_bytes());
        zip_buf.extend_from_slice(&0u16.to_le_bytes());
        zip_buf.extend_from_slice(&0u16.to_le_bytes()); // stored
        zip_buf.extend_from_slice(&0u16.to_le_bytes());
        zip_buf.extend_from_slice(&0u16.to_le_bytes());
        zip_buf.extend_from_slice(&0u32.to_le_bytes());
        zip_buf.extend_from_slice(&(nes_payload.len() as u32).to_le_bytes());
        zip_buf.extend_from_slice(&(nes_payload.len() as u32).to_le_bytes());
        zip_buf.extend_from_slice(&(name.len() as u16).to_le_bytes());
        zip_buf.extend_from_slice(&0u16.to_le_bytes());
        zip_buf.extend_from_slice(&0u16.to_le_bytes());
        zip_buf.extend_from_slice(&0u16.to_le_bytes());
        zip_buf.extend_from_slice(&0u16.to_le_bytes());
        zip_buf.extend_from_slice(&0u32.to_le_bytes());
        zip_buf.extend_from_slice(&0u32.to_le_bytes()); // local_off = 0
        zip_buf.extend_from_slice(name);
        let cd_size = (zip_buf.len() as u32) - cd_off;
        // EOCD
        zip_buf.extend_from_slice(b"PK\x05\x06");
        zip_buf.extend_from_slice(&0u16.to_le_bytes());
        zip_buf.extend_from_slice(&0u16.to_le_bytes());
        zip_buf.extend_from_slice(&1u16.to_le_bytes());
        zip_buf.extend_from_slice(&1u16.to_le_bytes());
        zip_buf.extend_from_slice(&cd_size.to_le_bytes());
        zip_buf.extend_from_slice(&cd_off.to_le_bytes());
        zip_buf.extend_from_slice(&0u16.to_le_bytes());

        let mut f = File::create(&zip_path).unwrap();
        f.write_all(&zip_buf).unwrap();
        let zr = inspect_rom_file(&zip_path).unwrap();
        assert_eq!(zr.platform_id, platforms::PLATFORM_NES);
        assert_eq!(zr.detected_title, "Castlevania");
        let _ = fs::remove_dir_all(&dir);
    }
}

//! Canonical platform and console group registry for Duke Launcher.
//!
//! Assigns stable numeric `platform_id: u16` (`0` = PC/Steam, `1..=24` = Retro & Nintendo systems)
//! and coarse `console_group: u8` values stored directly inside `catalog.dkcat` (`DKCAT002`)
//! fixed-stride records and `Local\duke-suite-shm-v1` launch frames.

use serde::{Deserialize, Serialize};

pub const PLATFORM_PC: u16 = 0;
pub const PLATFORM_NES: u16 = 1;
pub const PLATFORM_SNES: u16 = 2;
pub const PLATFORM_N64: u16 = 3;
pub const PLATFORM_GAMECUBE: u16 = 4;
pub const PLATFORM_WII: u16 = 5;
pub const PLATFORM_WII_U: u16 = 6;
pub const PLATFORM_SWITCH: u16 = 7;
pub const PLATFORM_GB: u16 = 8;
pub const PLATFORM_GBC: u16 = 9;
pub const PLATFORM_GBA: u16 = 10;
pub const PLATFORM_NDS: u16 = 11;
pub const PLATFORM_3DS: u16 = 12;
pub const PLATFORM_VIRTUAL_BOY: u16 = 13;
pub const PLATFORM_PS1: u16 = 14;
pub const PLATFORM_PS2: u16 = 15;
pub const PLATFORM_PSP: u16 = 16;
pub const PLATFORM_PS3: u16 = 17;
pub const PLATFORM_GENESIS: u16 = 18;
pub const PLATFORM_SMS: u16 = 19;
pub const PLATFORM_SATURN: u16 = 20;
pub const PLATFORM_DREAMCAST: u16 = 21;
pub const PLATFORM_ATARI_2600: u16 = 22;
pub const PLATFORM_ARCADE: u16 = 23;
pub const PLATFORM_PC_ENGINE: u16 = 24;

// Coarse O(1) filter group IDs stored in CatalogRecord::console_group
#[allow(dead_code)]
pub const GROUP_ALL: u8 = 0;
pub const GROUP_PC: u8 = 1;
pub const GROUP_SWITCH: u8 = 2;
pub const GROUP_WII_U: u8 = 3;
pub const GROUP_3DS: u8 = 4;
pub const GROUP_DS: u8 = 5;
pub const GROUP_GC_WII: u8 = 6;
pub const GROUP_N64: u8 = 7;
pub const GROUP_SNES_NES: u8 = 8;
pub const GROUP_GAMEBOY: u8 = 9;
pub const GROUP_PLAYSTATION: u8 = 10;
pub const GROUP_SEGA: u8 = 11;
pub const GROUP_ARCADE: u8 = 12;
pub const GROUP_NINTENDO_OTHER: u8 = 13;

#[derive(Debug, Clone, Copy)]
pub struct PlatformSpec {
    pub id: u16,
    pub slug: &'static str,
    pub short_name: &'static str,
    pub display_name: &'static str,
    pub console_group: u8,
    pub is_nintendo: bool,
    pub default_emulator_id: &'static str,
    pub rdb_files: &'static [&'static str],
    pub dat_files: &'static [&'static str],
    pub thumb_system: Option<&'static str>,
    pub alt_thumb_systems: &'static [&'static str],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformInfoDto {
    pub id: u16,
    pub slug: String,
    pub short_name: String,
    pub display_name: String,
    pub console_group: u8,
    pub is_nintendo: bool,
    pub default_emulator_id: String,
}

pub const PLATFORMS: &[PlatformSpec] = &[
    PlatformSpec {
        id: PLATFORM_PC,
        slug: "pc",
        short_name: "PC",
        display_name: "PC (Steam)",
        console_group: GROUP_PC,
        is_nintendo: false,
        default_emulator_id: "",
        rdb_files: &[],
        dat_files: &[],
        thumb_system: None,
        alt_thumb_systems: &[],
    },
    PlatformSpec {
        id: PLATFORM_NES,
        slug: "nes",
        short_name: "NES",
        display_name: "Nintendo Entertainment System",
        console_group: GROUP_SNES_NES,
        is_nintendo: true,
        default_emulator_id: "retroarch_nes",
        rdb_files: &["Nintendo - Nintendo Entertainment System.rdb"],
        dat_files: &["metadat/no-intro/Nintendo - Nintendo Entertainment System.dat"],
        thumb_system: Some("Nintendo - Nintendo Entertainment System"),
        alt_thumb_systems: &[],
    },
    PlatformSpec {
        id: PLATFORM_SNES,
        slug: "snes",
        short_name: "SNES",
        display_name: "Super Nintendo",
        console_group: GROUP_SNES_NES,
        is_nintendo: true,
        default_emulator_id: "retroarch_snes",
        rdb_files: &["Nintendo - Super Nintendo Entertainment System.rdb"],
        dat_files: &["metadat/no-intro/Nintendo - Super Nintendo Entertainment System.dat"],
        thumb_system: Some("Nintendo - Super Nintendo Entertainment System"),
        alt_thumb_systems: &[],
    },
    PlatformSpec {
        id: PLATFORM_N64,
        slug: "n64",
        short_name: "N64",
        display_name: "Nintendo 64",
        console_group: GROUP_N64,
        is_nintendo: true,
        default_emulator_id: "retroarch_n64",
        rdb_files: &["Nintendo - Nintendo 64.rdb"],
        dat_files: &["metadat/no-intro/Nintendo - Nintendo 64.dat"],
        thumb_system: Some("Nintendo - Nintendo 64"),
        alt_thumb_systems: &["Nintendo - Nintendo 64DD"],
    },
    PlatformSpec {
        id: PLATFORM_GAMECUBE,
        slug: "gamecube",
        short_name: "GameCube",
        display_name: "Nintendo GameCube",
        console_group: GROUP_GC_WII,
        is_nintendo: true,
        default_emulator_id: "dolphin",
        rdb_files: &["Nintendo - GameCube.rdb"],
        dat_files: &["metadat/redump/Nintendo - GameCube.dat"],
        thumb_system: Some("Nintendo - GameCube"),
        alt_thumb_systems: &[],
    },
    PlatformSpec {
        id: PLATFORM_WII,
        slug: "wii",
        short_name: "Wii",
        display_name: "Nintendo Wii",
        console_group: GROUP_GC_WII,
        is_nintendo: true,
        default_emulator_id: "dolphin",
        rdb_files: &["Nintendo - Wii.rdb"],
        dat_files: &["metadat/redump/Nintendo - Wii.dat"],
        thumb_system: Some("Nintendo - Wii"),
        alt_thumb_systems: &["Nintendo - Wii (Digital)"],
    },
    PlatformSpec {
        id: PLATFORM_WII_U,
        slug: "wiiu",
        short_name: "Wii U",
        display_name: "Nintendo Wii U",
        console_group: GROUP_WII_U,
        is_nintendo: true,
        default_emulator_id: "cemu",
        rdb_files: &["Nintendo - Wii U.rdb"],
        dat_files: &["metadat/redump/Nintendo - Wii U.dat", "metadat/no-intro/Nintendo - Wii U (Digital) (CDN).dat"],
        thumb_system: Some("Nintendo - Wii U"),
        alt_thumb_systems: &[],
    },
    PlatformSpec {
        id: PLATFORM_SWITCH,
        slug: "switch",
        short_name: "Switch",
        display_name: "Nintendo Switch",
        console_group: GROUP_SWITCH,
        is_nintendo: true,
        default_emulator_id: "switch_custom",
        rdb_files: &["Nintendo - Nintendo Switch.rdb", "Nintendo - Nintendo Switch Digital.rdb"],
        dat_files: &["metadat/no-intro/Nintendo - Nintendo Switch.dat"],
        thumb_system: Some("Nintendo - Nintendo Switch"),
        alt_thumb_systems: &[],
    },
    PlatformSpec {
        id: PLATFORM_GB,
        slug: "gb",
        short_name: "Game Boy",
        display_name: "Game Boy",
        console_group: GROUP_GAMEBOY,
        is_nintendo: true,
        default_emulator_id: "mgba",
        rdb_files: &["Nintendo - Game Boy.rdb"],
        dat_files: &["metadat/no-intro/Nintendo - Game Boy.dat"],
        thumb_system: Some("Nintendo - Game Boy"),
        alt_thumb_systems: &[],
    },
    PlatformSpec {
        id: PLATFORM_GBC,
        slug: "gbc",
        short_name: "Game Boy Color",
        display_name: "Game Boy Color",
        console_group: GROUP_GAMEBOY,
        is_nintendo: true,
        default_emulator_id: "mgba",
        rdb_files: &["Nintendo - Game Boy Color.rdb"],
        dat_files: &["metadat/no-intro/Nintendo - Game Boy Color.dat"],
        thumb_system: Some("Nintendo - Game Boy Color"),
        alt_thumb_systems: &[],
    },
    PlatformSpec {
        id: PLATFORM_GBA,
        slug: "gba",
        short_name: "Game Boy Advance",
        display_name: "Game Boy Advance",
        console_group: GROUP_GAMEBOY,
        is_nintendo: true,
        default_emulator_id: "mgba",
        rdb_files: &["Nintendo - Game Boy Advance.rdb"],
        dat_files: &["metadat/no-intro/Nintendo - Game Boy Advance.dat"],
        thumb_system: Some("Nintendo - Game Boy Advance"),
        alt_thumb_systems: &[],
    },
    PlatformSpec {
        id: PLATFORM_NDS,
        slug: "nds",
        short_name: "Nintendo DS",
        display_name: "Nintendo DS",
        console_group: GROUP_DS,
        is_nintendo: true,
        default_emulator_id: "melonds",
        rdb_files: &["Nintendo - Nintendo DS.rdb"],
        dat_files: &["metadat/no-intro/Nintendo - Nintendo DS.dat"],
        thumb_system: Some("Nintendo - Nintendo DS"),
        alt_thumb_systems: &["Nintendo - Nintendo DSi"],
    },
    PlatformSpec {
        id: PLATFORM_3DS,
        slug: "3ds",
        short_name: "Nintendo 3DS",
        display_name: "Nintendo 3DS",
        console_group: GROUP_3DS,
        is_nintendo: true,
        default_emulator_id: "azahar",
        rdb_files: &["Nintendo - Nintendo 3DS.rdb"],
        dat_files: &["metadat/no-intro/Nintendo - Nintendo 3DS.dat"],
        thumb_system: Some("Nintendo - Nintendo 3DS"),
        alt_thumb_systems: &["Nintendo - New Nintendo 3DS"],
    },
    PlatformSpec {
        id: PLATFORM_VIRTUAL_BOY,
        slug: "virtual_boy",
        short_name: "Virtual Boy",
        display_name: "Nintendo Virtual Boy",
        console_group: GROUP_NINTENDO_OTHER,
        is_nintendo: true,
        default_emulator_id: "retroarch_vb",
        rdb_files: &["Nintendo - Virtual Boy.rdb"],
        dat_files: &["metadat/no-intro/Nintendo - Virtual Boy.dat"],
        thumb_system: Some("Nintendo - Virtual Boy"),
        alt_thumb_systems: &[],
    },
    PlatformSpec {
        id: PLATFORM_PS1,
        slug: "ps1",
        short_name: "PS1",
        display_name: "Sony PlayStation",
        console_group: GROUP_PLAYSTATION,
        is_nintendo: false,
        default_emulator_id: "duckstation",
        rdb_files: &["Sony - PlayStation.rdb"],
        dat_files: &["metadat/redump/Sony - PlayStation.dat"],
        thumb_system: Some("Sony - PlayStation"),
        alt_thumb_systems: &[],
    },
    PlatformSpec {
        id: PLATFORM_PS2,
        slug: "ps2",
        short_name: "PS2",
        display_name: "Sony PlayStation 2",
        console_group: GROUP_PLAYSTATION,
        is_nintendo: false,
        default_emulator_id: "pcsx2",
        rdb_files: &["Sony - PlayStation 2.rdb"],
        dat_files: &["metadat/redump/Sony - PlayStation 2.dat"],
        thumb_system: Some("Sony - PlayStation 2"),
        alt_thumb_systems: &[],
    },
    PlatformSpec {
        id: PLATFORM_PSP,
        slug: "psp",
        short_name: "PSP",
        display_name: "Sony PlayStation Portable",
        console_group: GROUP_PLAYSTATION,
        is_nintendo: false,
        default_emulator_id: "ppsspp",
        rdb_files: &["Sony - PlayStation Portable.rdb"],
        dat_files: &["metadat/redump/Sony - PlayStation Portable.dat"],
        thumb_system: Some("Sony - PlayStation Portable"),
        alt_thumb_systems: &[],
    },
    PlatformSpec {
        id: PLATFORM_PS3,
        slug: "ps3",
        short_name: "PS3",
        display_name: "Sony PlayStation 3",
        console_group: GROUP_PLAYSTATION,
        is_nintendo: false,
        default_emulator_id: "ps3_custom",
        rdb_files: &["Sony - PlayStation 3.rdb"],
        dat_files: &["metadat/redump/Sony - PlayStation 3.dat"],
        thumb_system: Some("Sony - PlayStation 3"),
        alt_thumb_systems: &[],
    },
    PlatformSpec {
        id: PLATFORM_GENESIS,
        slug: "genesis",
        short_name: "Sega Genesis",
        display_name: "Sega Genesis / Mega Drive",
        console_group: GROUP_SEGA,
        is_nintendo: false,
        default_emulator_id: "retroarch_genesis",
        rdb_files: &["Sega - Mega Drive - Genesis.rdb"],
        dat_files: &["metadat/no-intro/Sega - Mega Drive - Genesis.dat"],
        thumb_system: Some("Sega - Mega Drive - Genesis"),
        alt_thumb_systems: &["Sega - Mega-CD - Sega CD", "Sega - 32X"],
    },
    PlatformSpec {
        id: PLATFORM_SMS,
        slug: "sms",
        short_name: "Master System",
        display_name: "Sega Master System",
        console_group: GROUP_SEGA,
        is_nintendo: false,
        default_emulator_id: "retroarch_sms",
        rdb_files: &["Sega - Master System - Mark III.rdb"],
        dat_files: &["metadat/no-intro/Sega - Master System - Mark III.dat"],
        thumb_system: Some("Sega - Master System - Mark III"),
        alt_thumb_systems: &["Sega - Game Gear"],
    },
    PlatformSpec {
        id: PLATFORM_SATURN,
        slug: "saturn",
        short_name: "Sega Saturn",
        display_name: "Sega Saturn",
        console_group: GROUP_SEGA,
        is_nintendo: false,
        default_emulator_id: "retroarch_saturn",
        rdb_files: &["Sega - Saturn.rdb"],
        dat_files: &["metadat/redump/Sega - Saturn.dat"],
        thumb_system: Some("Sega - Saturn"),
        alt_thumb_systems: &[],
    },
    PlatformSpec {
        id: PLATFORM_DREAMCAST,
        slug: "dreamcast",
        short_name: "Dreamcast",
        display_name: "Sega Dreamcast",
        console_group: GROUP_SEGA,
        is_nintendo: false,
        default_emulator_id: "retroarch_dreamcast",
        rdb_files: &["Sega - Dreamcast.rdb"],
        dat_files: &["metadat/redump/Sega - Dreamcast.dat"],
        thumb_system: Some("Sega - Dreamcast"),
        alt_thumb_systems: &[],
    },
    PlatformSpec {
        id: PLATFORM_ATARI_2600,
        slug: "atari2600",
        short_name: "Atari 2600",
        display_name: "Atari 2600",
        console_group: GROUP_ARCADE,
        is_nintendo: false,
        default_emulator_id: "retroarch_atari2600",
        rdb_files: &["Atari - 2600.rdb"],
        dat_files: &["metadat/no-intro/Atari - 2600.dat"],
        thumb_system: Some("Atari - 2600"),
        alt_thumb_systems: &[],
    },
    PlatformSpec {
        id: PLATFORM_ARCADE,
        slug: "arcade",
        short_name: "Arcade",
        display_name: "Neo Geo / Arcade (MAME)",
        console_group: GROUP_ARCADE,
        is_nintendo: false,
        default_emulator_id: "retroarch_arcade",
        rdb_files: &["FBNeo - Arcade Games.rdb", "SNK - Neo Geo.rdb", "MAME.rdb"],
        dat_files: &["metadat/fbneo-split/FBNeo - Arcade Games.dat", "dat/SNK - Neo Geo.dat"],
        thumb_system: Some("FBNeo - Arcade Games"),
        alt_thumb_systems: &["MAME", "SNK - Neo Geo"],
    },
    PlatformSpec {
        id: PLATFORM_PC_ENGINE,
        slug: "pce",
        short_name: "PC Engine",
        display_name: "PC Engine / TurboGrafx-16",
        console_group: GROUP_ARCADE,
        is_nintendo: false,
        default_emulator_id: "retroarch_pce",
        rdb_files: &["NEC - PC Engine - TurboGrafx 16.rdb"],
        dat_files: &["metadat/no-intro/NEC - PC Engine - TurboGrafx 16.dat"],
        thumb_system: Some("NEC - PC Engine - TurboGrafx 16"),
        alt_thumb_systems: &["NEC - PC Engine SuperGrafx", "NEC - PC Engine CD - TurboGrafx-CD"],
    },
];

pub fn platform_by_id(platform_id: u16) -> &'static PlatformSpec {
    PLATFORMS
        .get(platform_id as usize)
        .filter(|p| p.id == platform_id)
        .or_else(|| PLATFORMS.iter().find(|p| p.id == platform_id))
        .unwrap_or(&PLATFORMS[0])
}

pub fn console_group_for_platform(platform_id: u16) -> u8 {
    platform_by_id(platform_id).console_group
}

pub fn platform_from_slug_or_name(raw: &str) -> Option<&'static PlatformSpec> {
    let key = raw.trim().to_ascii_lowercase();
    if key.is_empty() {
        return None;
    }
    if let Ok(num) = key.parse::<u16>() {
        return PLATFORMS.iter().find(|p| p.id == num);
    }
    PLATFORMS.iter().find(|p| {
        p.slug == key
            || p.short_name.to_ascii_lowercase() == key
            || p.display_name.to_ascii_lowercase() == key
    })
}

/// Evaluates whether a `CatalogRecord` (`platform_id`, `console_group`) matches a UI filter token.
/// Supports `"all"`, `"pc"`, `"nintendo"` (all Nintendo consoles `1..=13`), specific group keys
/// (`"switch"`, `"wiiu"`, `"3ds"`, `"ds"`, `"gc_wii"`, `"n64"`, `"snes_nes"`, `"gameboy"`,
/// `"playstation"`, `"sega"`, `"arcade"`), or any individual platform slug (`"nes"`, `"ps2"`, etc.).
#[inline]
pub fn record_matches_platform_filter(platform_id: u16, console_group: u8, filter_key: &str) -> bool {
    let k = filter_key.trim().to_ascii_lowercase();
    if k.is_empty() || k == "all" || k == "all_systems" {
        return true;
    }
    match k.as_str() {
        "pc" | "steam" | "windows" => platform_id == PLATFORM_PC,
        "nintendo" => (PLATFORM_NES..=PLATFORM_VIRTUAL_BOY).contains(&platform_id),
        "switch" => console_group == GROUP_SWITCH || platform_id == PLATFORM_SWITCH,
        "wiiu" | "wii_u" | "wii-u" => console_group == GROUP_WII_U || platform_id == PLATFORM_WII_U,
        "3ds" | "n3ds" => console_group == GROUP_3DS || platform_id == PLATFORM_3DS,
        "ds" | "nds" => console_group == GROUP_DS || platform_id == PLATFORM_NDS,
        "gc_wii" | "gc-wii" | "gamecube_wii" | "gamecube / wii" | "gc / wii" => {
            console_group == GROUP_GC_WII
                || platform_id == PLATFORM_GAMECUBE
                || platform_id == PLATFORM_WII
        }
        "n64" => console_group == GROUP_N64 || platform_id == PLATFORM_N64,
        "snes_nes" | "snes-nes" | "snes / nes" => {
            console_group == GROUP_SNES_NES
                || platform_id == PLATFORM_NES
                || platform_id == PLATFORM_SNES
        }
        "gameboy" | "gba" | "gb" | "gba_gbc_gb" | "gba / gbc / gb" => {
            console_group == GROUP_GAMEBOY
                || matches!(platform_id, PLATFORM_GB | PLATFORM_GBC | PLATFORM_GBA)
        }
        "playstation" | "ps" => {
            console_group == GROUP_PLAYSTATION
                || matches!(
                    platform_id,
                    PLATFORM_PS1 | PLATFORM_PS2 | PLATFORM_PSP | PLATFORM_PS3
                )
        }
        "sega" => {
            console_group == GROUP_SEGA
                || matches!(
                    platform_id,
                    PLATFORM_GENESIS | PLATFORM_SMS | PLATFORM_SATURN | PLATFORM_DREAMCAST
                )
        }
        "arcade" | "mame" | "classic" => {
            console_group == GROUP_ARCADE
                || matches!(
                    platform_id,
                    PLATFORM_ATARI_2600 | PLATFORM_ARCADE | PLATFORM_PC_ENGINE
                )
        }
        _ => platform_from_slug_or_name(&k)
            .map(|p| p.id == platform_id)
            .unwrap_or(false),
    }
}

/// Deterministic non-colliding synthetic `u32` catalog ID (`>= 0x8000_0000`) for Retro/Nintendo
/// entries in `catalog.dkcat` so Steam Store numeric IDs (`< 0x8000_0000`) never collide.
pub fn synthetic_retro_catalog_id(platform_id: u16, title: &str, serial_or_crc: &str) -> u32 {
    let mut input = String::with_capacity(title.len() + serial_or_crc.len() + 12);
    input.push_str(&platform_id.to_string());
    input.push('|');
    if !serial_or_crc.trim().is_empty() {
        input.push_str(&serial_or_crc.trim().to_ascii_lowercase());
    } else {
        input.push_str(&title.trim().to_ascii_lowercase());
    }
    let hash = blake3::hash(input.as_bytes());
    let b = hash.as_bytes();
    let hash24 = ((u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2])) & 0x00FF_FFFF;
    let plat7 = (u32::from(platform_id) & 0x7F) << 24;
    0x8000_0000 | plat7 | hash24.max(1)
}

#[inline]
pub const fn is_synthetic_retro_appid(appid: u32) -> bool {
    (appid & 0x8000_0000) != 0
}

pub fn list_platform_dtos() -> Vec<PlatformInfoDto> {
    PLATFORMS
        .iter()
        .map(|p| PlatformInfoDto {
            id: p.id,
            slug: p.slug.to_string(),
            short_name: p.short_name.to_string(),
            display_name: p.display_name.to_string(),
            console_group: p.console_group,
            is_nintendo: p.is_nintendo,
            default_emulator_id: p.default_emulator_id.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_pc_plus_24_retro_and_nintendo_platforms() {
        assert_eq!(PLATFORMS.len(), 25);
        for (idx, p) in PLATFORMS.iter().enumerate() {
            assert_eq!(p.id as usize, idx);
            if idx > 0 {
                assert!(!p.rdb_files.is_empty());
                assert!(!p.default_emulator_id.is_empty());
            }
        }
    }

    #[test]
    fn platform_filter_groups_match_cleanly() {
        assert!(record_matches_platform_filter(PLATFORM_PC, GROUP_PC, "all"));
        assert!(record_matches_platform_filter(PLATFORM_PC, GROUP_PC, "pc"));
        assert!(!record_matches_platform_filter(PLATFORM_SNES, GROUP_SNES_NES, "pc"));
        assert!(record_matches_platform_filter(PLATFORM_SNES, GROUP_SNES_NES, "nintendo"));
        assert!(record_matches_platform_filter(PLATFORM_3DS, GROUP_3DS, "nintendo"));
        assert!(record_matches_platform_filter(PLATFORM_3DS, GROUP_3DS, "3ds"));
        assert!(record_matches_platform_filter(PLATFORM_GAMECUBE, GROUP_GC_WII, "gc_wii"));
        assert!(record_matches_platform_filter(PLATFORM_WII, GROUP_GC_WII, "gc_wii"));
        assert!(record_matches_platform_filter(PLATFORM_PS2, GROUP_PLAYSTATION, "playstation"));
        assert!(record_matches_platform_filter(PLATFORM_DREAMCAST, GROUP_SEGA, "sega"));
        assert!(record_matches_platform_filter(PLATFORM_ARCADE, GROUP_ARCADE, "arcade"));
    }

    #[test]
    fn synthetic_retro_ids_stay_in_high_space() {
        let a = synthetic_retro_catalog_id(PLATFORM_GAMECUBE, "Super Mario Sunshine", "GMSE01");
        let b = synthetic_retro_catalog_id(PLATFORM_GAMECUBE, "Metroid Prime", "GM8E01");
        assert!(is_synthetic_retro_appid(a));
        assert!(is_synthetic_retro_appid(b));
        assert_ne!(a, b);
    }
}

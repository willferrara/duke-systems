use memmap2::Mmap;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};
use std::time::Instant;

use crate::{get_app_data_file_standalone, CatalogCacheFile, CatalogGameEntry};

pub const DKCAT_MAGIC_V1: [u8; 8] = *b"DKCAT001";
pub const DKCAT_MAGIC_V2: [u8; 8] = *b"DKCAT002";
pub const DKCAT_MAGIC: [u8; 8] = *b"DKCAT003";
pub const DKCAT_VERSION: u32 = 4;
const FLAG_IS_FREE: u32 = 1 << 0;
const FLAG_SINGLEPLAYER: u32 = 1 << 1;
const FLAG_COOP: u32 = 1 << 2;
const FLAG_MULTIPLAYER: u32 = 1 << 3;
const FLAG_ONLINE_ONLY_PVP: u32 = 1 << 4;
const FLAG_DETAILS_ENRICHED: u32 = 1 << 5;
const GENRE_MASK_POPULATED: u64 = 1u64 << 63;

pub const SEM_CONTROLLER_SUPPORT: u16 = 1 << 0;
pub const SEM_COZY_RELAXING: u16 = 1 << 1;
pub const SEM_SHORT_SESSION: u16 = 1 << 2;
pub const SEM_PHYSICS_CHAOS: u16 = 1 << 3;
pub const SEM_HANDHELD_FRIENDLY: u16 = 1 << 4;

pub struct GenreBitDef {
    pub name: &'static str,
    pub bit: u64,
    pub synonyms: &'static [&'static str],
}

/// 36-genre/sub-genre taxonomy mapped into `CatalogRecord::genre_bits` (`u64`) for O(1) bitwise filtering.
pub const GENRE_BIT_DEFS: [GenreBitDef; 36] = [
    // Engineering & Building (12)
    GenreBitDef {
        name: "Engineering",
        bit: 1u64 << 0,
        synonyms: &[
            "engineering",
            "programming",
            "automation",
            "robotics",
            "electronics",
            "vehicular combat",
            "space sim",
            "rocket",
            "building",
            "logic",
            "coding",
            "hardware",
        ],
    },
    GenreBitDef {
        name: "Sandbox",
        bit: 1u64 << 1,
        synonyms: &[
            "sandbox",
            "open world survival craft",
            "moddable",
            "physics",
            "voxel",
            "level editor",
        ],
    },
    GenreBitDef {
        name: "Automation",
        bit: 1u64 << 2,
        synonyms: &[
            "automation",
            "factory",
            "logistics",
            "programming",
            "conveyor",
            "resource management",
        ],
    },
    GenreBitDef {
        name: "Building",
        bit: 1u64 << 3,
        synonyms: &[
            "building",
            "base building",
            "base-building",
            "city builder",
            "colony sim",
            "construction",
        ],
    },
    GenreBitDef {
        name: "Physics",
        bit: 1u64 << 4,
        synonyms: &["physics", "destruction", "ragdoll"],
    },
    GenreBitDef {
        name: "Space",
        bit: 1u64 << 5,
        synonyms: &[
            "space",
            "space sim",
            "spaceships",
            "rocket",
            "orbital",
            "astronomy",
            "mars",
        ],
    },
    GenreBitDef {
        name: "Flight",
        bit: 1u64 << 6,
        synonyms: &[
            "flight",
            "aviation",
            "jet",
            "helicopter",
            "dogfight",
            "flying",
            "air combat",
        ],
    },
    GenreBitDef {
        name: "Crafting",
        bit: 1u64 << 7,
        synonyms: &["crafting", "open world survival craft", "smithing"],
    },
    GenreBitDef {
        name: "Survival",
        bit: 1u64 << 8,
        synonyms: &["survival", "open world survival craft", "survival horror"],
    },
    GenreBitDef {
        name: "City Builder",
        bit: 1u64 << 9,
        synonyms: &["city builder", "city-builder", "colony sim"],
    },
    GenreBitDef {
        name: "Management",
        bit: 1u64 << 10,
        synonyms: &[
            "management",
            "tycoon",
            "colony sim",
            "time management",
            "resource management",
        ],
    },
    GenreBitDef {
        name: "Economy",
        bit: 1u64 << 11,
        synonyms: &["economy", "trading", "capitalism", "tycoon"],
    },
    // Core Genres (15)
    GenreBitDef {
        name: "Action",
        bit: 1u64 << 12,
        synonyms: &["action", "hack and slash", "beat 'em up", "character action"],
    },
    GenreBitDef {
        name: "Adventure",
        bit: 1u64 << 13,
        synonyms: &["adventure", "action-adventure", "point & click", "exploration"],
    },
    GenreBitDef {
        name: "RPG",
        bit: 1u64 << 14,
        synonyms: &[
            "rpg",
            "role-playing",
            "jrpg",
            "action rpg",
            "crpg",
            "party-based rpg",
        ],
    },
    GenreBitDef {
        name: "Strategy",
        bit: 1u64 << 15,
        synonyms: &["strategy", "rts", "real time tactics", "grand strategy", "4x"],
    },
    GenreBitDef {
        name: "Simulation",
        bit: 1u64 << 16,
        synonyms: &[
            "simulation",
            "life sim",
            "farming sim",
            "immersive sim",
            "space sim",
            "colony sim",
            "automobile sim",
            "dating sim",
            "job simulator",
            "flight",
        ],
    },
    GenreBitDef {
        name: "Shooter",
        bit: 1u64 << 17,
        synonyms: &[
            "shooter",
            "shoot 'em up",
            "twin stick shooter",
            "hero shooter",
            "looter shooter",
            "fps",
            "third-person shooter",
        ],
    },
    GenreBitDef {
        name: "FPS",
        bit: 1u64 << 18,
        synonyms: &["fps", "first-person shooter", "boomer shooter", "arena shooter"],
    },
    GenreBitDef {
        name: "Third-Person",
        bit: 1u64 << 19,
        synonyms: &[
            "third-person",
            "third person",
            "3rd person",
            "third-person shooter",
        ],
    },
    GenreBitDef {
        name: "Puzzle",
        bit: 1u64 << 20,
        synonyms: &["puzzle", "puzzle-platformer", "logic", "sokoban", "escape room"],
    },
    GenreBitDef {
        name: "Platformer",
        bit: 1u64 << 21,
        synonyms: &[
            "platformer",
            "2d platformer",
            "3d platformer",
            "precision platformer",
            "puzzle-platformer",
        ],
    },
    GenreBitDef {
        name: "Racing",
        bit: 1u64 << 22,
        synonyms: &[
            "racing",
            "driving",
            "motorsport",
            "offroad",
            "kart racing",
            "rally",
        ],
    },
    GenreBitDef {
        name: "Sports",
        bit: 1u64 << 23,
        synonyms: &[
            "sports",
            "football",
            "soccer",
            "basketball",
            "baseball",
            "golf",
            "tennis",
            "skateboarding",
        ],
    },
    GenreBitDef {
        name: "Fighting",
        bit: 1u64 << 24,
        synonyms: &[
            "fighting",
            "2d fighter",
            "3d fighter",
            "martial arts",
            "beat 'em up",
        ],
    },
    GenreBitDef {
        name: "Horror",
        bit: 1u64 << 25,
        synonyms: &[
            "horror",
            "survival horror",
            "psychological horror",
            "lovecraftian",
            "cosmic horror",
        ],
    },
    GenreBitDef {
        name: "Stealth",
        bit: 1u64 << 26,
        synonyms: &["stealth", "assassination", "heist", "espionage"],
    },
    // Sub-Genres & Structure (9)
    GenreBitDef {
        name: "Open World",
        bit: 1u64 << 27,
        synonyms: &["open world", "open-world", "open world survival craft"],
    },
    GenreBitDef {
        name: "Souls-like",
        bit: 1u64 << 28,
        synonyms: &["souls-like", "soulslike", "soulsborne"],
    },
    GenreBitDef {
        name: "Metroidvania",
        bit: 1u64 << 29,
        synonyms: &["metroidvania"],
    },
    GenreBitDef {
        name: "Roguelike",
        bit: 1u64 << 30,
        synonyms: &[
            "roguelike",
            "roguelite",
            "rogue-like",
            "rogue-lite",
            "action roguelike",
            "roguelike deckbuilder",
        ],
    },
    GenreBitDef {
        name: "Tactical",
        bit: 1u64 << 31,
        synonyms: &[
            "tactical",
            "tactics",
            "real time tactics",
            "tactical rpg",
            "turn-based tactics",
        ],
    },
    GenreBitDef {
        name: "Turn-Based",
        bit: 1u64 << 32,
        synonyms: &[
            "turn-based",
            "turn based",
            "turn-based strategy",
            "turn-based combat",
            "turn-based tactics",
        ],
    },
    GenreBitDef {
        name: "Tower Defense",
        bit: 1u64 << 33,
        synonyms: &["tower defense", "tower-defense"],
    },
    GenreBitDef {
        name: "Story Rich",
        bit: 1u64 << 34,
        synonyms: &[
            "story rich",
            "narrative",
            "visual novel",
            "interactive fiction",
            "choices matter",
        ],
    },
    GenreBitDef {
        name: "Indie",
        bit: 1u64 << 35,
        synonyms: &["indie"],
    },
];

const FIELD_SEP: char = '\x1f';
const LIST_SEP: char = '\x1e';

/// 72-byte zero-copy binary header for `catalog.dkcat` (`#[repr(C)]`, 8-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DkCatHeader {
    pub magic: [u8; 8],
    pub version: u32,
    pub record_count: u32,
    pub records_offset: u64,
    pub strings_offset: u64,
    pub trigram_offset: u64,
    pub checksum_blake3: [u8; 32],
}

/// Legacy 104-byte fixed-stride record layout used in `DKCAT001` archives (kept for migration).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct CatalogRecordV1 {
    pub ccu: u64,
    pub owners_min: u64,
    pub owners_max: u64,
    pub positive: u64,
    pub negative: u64,
    pub value_score: f64,
    pub updated_at: u64,
    pub genre_bits: u64,
    pub appid: u32,
    pub price_cents: u32,
    pub initial_price_cents: u32,
    pub flags: u32,
    pub name_off: u32,
    pub name_len: u32,
    pub tags_off: u32,
    pub tags_len: u32,
    pub meta_off: u32,
    pub meta_len: u32,
}

/// Legacy fixed-stride `#[repr(C)]` catalog entry (112 bytes) preserved for transparent `DKCAT002` -> `DKCAT003` migration.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct CatalogRecordV2 {
    pub ccu: u64,
    pub owners_min: u64,
    pub owners_max: u64,
    pub positive: u64,
    pub negative: u64,
    pub value_score: f64,
    pub updated_at: u64,
    pub genre_bits: u64,
    pub appid: u32,
    pub price_cents: u32,
    pub initial_price_cents: u32,
    pub flags: u32,
    pub name_off: u32,
    pub name_len: u32,
    pub tags_off: u32,
    pub tags_len: u32,
    pub meta_off: u32,
    pub meta_len: u32,
    pub platform_id: u16,
    pub console_group: u8,
    pub _pad: [u8; 5],
}

/// Packed fixed-stride `#[repr(C)]` catalog entry (152 bytes) stored sequentially in `catalog.dkcat` (`DKCAT003`)
/// with a quantized 32-dim semantic embedding `[i8; 32]`, estimated disk footprint, HLTB tenths, and capability flags.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CatalogRecord {
    pub ccu: u64,
    pub owners_min: u64,
    pub owners_max: u64,
    pub positive: u64,
    pub negative: u64,
    pub value_score: f64,
    pub updated_at: u64,
    pub genre_bits: u64,
    pub appid: u32,
    pub price_cents: u32,
    pub initial_price_cents: u32,
    pub flags: u32,
    pub name_off: u32,
    pub name_len: u32,
    pub tags_off: u32,
    pub tags_len: u32,
    pub meta_off: u32,
    pub meta_len: u32,
    pub platform_id: u16,
    pub console_group: u8,
    pub _pad: [u8; 5],
    pub est_size_mb: u32,
    pub hltb_tenths: u16,
    pub semantic_flags: u16,
    pub semantic_vec: [i8; 32],
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct TrigramIndexEntry {
    pub trigram: [u8; 3],
    pub _pad: u8,
    pub list_off: u32,
    pub list_len: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CatalogMmapStats {
    pub active: bool,
    pub path: String,
    pub file_size_bytes: u64,
    pub record_count: u32,
    pub trigram_count: u32,
    pub fetched_at: u64,
    pub checksum_hex: String,
    pub map_time_us: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CatalogQueryParams {
    pub query: String,
    pub sort_by: String,
    pub paid_only: bool,
    pub singleplayer: bool,
    pub coop: bool,
    pub exclude_pvp: bool,
    pub hide_installed: bool,
    pub installed_app_ids: Vec<u32>,
    pub installed_titles_norm: Vec<String>,
    pub min_price_cents: u32,
    pub price_tier: String,
    pub min_rating_pct: u32,
    pub min_reviews: u64,
    pub player_mode: String,
    pub min_ccu: u64,
    pub genres: Vec<String>,
    pub platform: String,
    pub platform_id: Option<u16>,
    pub console_group: String,
    pub offset: usize,
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CatalogQueryPage {
    pub items: Vec<CatalogGameEntry>,
    pub total_matches: usize,
    pub total_indexed: usize,
    pub fetched_at: u64,
    pub offset: usize,
    pub limit: usize,
    pub query_time_us: u64,
    #[serde(default)]
    pub semantic_mode: bool,
    #[serde(default)]
    pub semantic_summary: String,
}

const SEMANTIC_CONCEPT_AXES: [&[&str]; 32] = [
    &["action", "combat", "fast-paced", "arcade", "hack", "brawler"],
    &["rpg", "role-playing", "jrpg", "crpg", "leveling", "character customization"],
    &["co-op", "coop", "couch co-op", "local co-op", "online co-op", "split screen", "friends"],
    &["physics", "ragdoll", "destruction", "sandbox physics", "momentum", "gravity"],
    &["chaos", "mayhem", "party", "funny", "comedy", "slapstick", "hilarious", "silly"],
    &["controller", "gamepad", "full controller support", "dualsense", "handheld", "console"],
    &["puzzle", "logic", "brainteaser", "portal", "mystery", "sokoban"],
    &["roguelike", "roguelite", "deckbuilder", "run-based", "permadeath", "procedural"],
    &["strategy", "tactical", "turn-based", "rts", "4x", "tower defense", "grand strategy"],
    &["engineering", "automation", "factory", "building", "crafting", "sandbox", "voxel"],
    &["survival", "zombies", "base building", "open world survival craft", "scavenging"],
    &["horror", "scary", "psychological horror", "survival horror", "dark", "creepy"],
    &["cozy", "relaxing", "wholesome", "farming", "life sim", "chill", "casual"],
    &["story rich", "narrative", "choices matter", "visual novel", "cinematic"],
    &["indie", "minimalist", "2d", "compact", "lightweight", "solo dev"],
    &["open world", "exploration", "immersive", "seamless", "sandbox world"],
    &["shooter", "fps", "third-person shooter", "bullet hell", "twin stick", "gunplay"],
    &["platformer", "metroidvania", "precision platformer", "jump", "side scroller"],
    &["racing", "driving", "motorsport", "kart", "drift", "arcade racer"],
    &["simulation", "flight", "space", "management", "economy", "city builder"],
    &["souls-like", "difficult", "boss rush", "parry", "action rpg"],
    &["anime", "stylized", "cel shaded", "jrpg"],
    &["retro", "pixel", "pixel graphics", "classic", "90s", "8-bit", "16-bit"],
    &["short", "quick", "pick up", "bite-sized", "replayable", "arcade loop", "session"],
    &["epic", "massive", "100 hour", "endless", "mmo"],
    &["small", "compact", "under 5 gb", "low spec", "portable", "handheld friendly"],
    &["sci-fi", "cyberpunk", "futuristic", "space sim", "mechs"],
    &["fantasy", "magic", "medieval", "dungeon", "dragon"],
    &["nintendo", "switch", "wii u", "wii", "gamecube", "3ds", "n64", "snes", "nes", "gba"],
    &["playstation", "ps1", "ps2", "ps3", "psp", "sony"],
    &["local multiplayer", "family friendly", "couch", "shared screen", "4 player"],
    &["free to play", "free", "high rated", "overwhelmingly positive", "gem"],
];

#[inline]
pub fn dot_i8_32(a: &[i8; 32], b: &[i8; 32]) -> i32 {
    let mut acc = 0i32;
    let mut i = 0usize;
    while i < 32 {
        acc += (a[i] as i32) * (b[i] as i32);
        i += 1;
    }
    acc
}

fn quantize_float_vec_i8(weights: &[f32; 32]) -> [i8; 32] {
    let norm_sq: f32 = weights.iter().map(|v| v * v).sum();
    if norm_sq <= 1e-6 {
        return [0i8; 32];
    }
    let inv_norm = 127.0 / norm_sq.sqrt();
    let mut out = [0i8; 32];
    for (idx, &w) in weights.iter().enumerate() {
        let q = (w * inv_norm).round().clamp(-127.0, 127.0);
        out[idx] = q as i8;
    }
    out
}

fn parse_explicit_size_mb_hint(hay_lower: &str) -> Option<u32> {
    let bytes = hay_lower.as_bytes();
    for (idx, w) in bytes.windows(2).enumerate() {
        if w == b"gb" || w == b"mb" {
            let prefix = hay_lower[..idx].trim_end();
            let num_str: String = prefix
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            if let Ok(val) = num_str.parse::<f64>() {
                if val > 0.0 && val < 2000.0 {
                    let mb = if w == b"gb" { val * 1024.0 } else { val };
                    return Some(mb.round().clamp(1.0, 500_000.0) as u32);
                }
            }
        }
    }
    None
}

fn parse_explicit_hours_tenths_hint(hay_lower: &str) -> Option<u16> {
    let tokens: Vec<&str> = hay_lower
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '.' && c != '-')
        .filter(|s| !s.is_empty())
        .collect();
    for (idx, tok) in tokens.iter().enumerate() {
        if let Some(prefix) = tok.strip_suffix("-hour").or_else(|| tok.strip_suffix("-hr")) {
            if let Ok(h) = prefix.parse::<f64>() {
                if (0.5..=400.0).contains(&h) {
                    return Some((h * 10.0).round() as u16);
                }
            }
        }
        if matches!(*tok, "hour" | "hours" | "hr" | "hrs") && idx > 0 {
            if let Ok(h) = tokens[idx - 1].parse::<f64>() {
                if (0.5..=400.0).contains(&h) {
                    return Some((h * 10.0).round() as u16);
                }
            }
        }
    }
    None
}

fn estimate_entry_size_and_hltb(entry: &CatalogGameEntry, hay_lower: &str) -> (u32, u16, u16) {
    let mut flags = 0u16;
    if entry.platform_id > 0
        || entry
            .categories
            .iter()
            .any(|c| c.to_ascii_lowercase().contains("controller"))
        || hay_lower.contains("controller support")
        || hay_lower.contains("gamepad")
        || entry
            .genres
            .iter()
            .chain(entry.tags.iter())
            .any(|g| {
                matches!(
                    g.to_ascii_lowercase().as_str(),
                    "action" | "platformer" | "racing" | "metroidvania" | "rpg" | "fighting" | "sports" | "physics"
                )
            })
    {
        flags |= SEM_CONTROLLER_SUPPORT;
    }

    if hay_lower.contains("cozy")
        || hay_lower.contains("relaxing")
        || hay_lower.contains("wholesome")
        || hay_lower.contains("farming")
    {
        flags |= SEM_COZY_RELAXING;
    }
    if hay_lower.contains("physics")
        || hay_lower.contains("chaos")
        || hay_lower.contains("ragdoll")
        || hay_lower.contains("destruction")
    {
        flags |= SEM_PHYSICS_CHAOS;
    }

    let est_size_mb = if let Some(explicit_mb) = parse_explicit_size_mb_hint(hay_lower) {
        explicit_mb
    } else if entry.platform_id > 0 {
        match entry.platform_id {
            p if p == crate::platforms::PLATFORM_SWITCH => 3400,
            p if p == crate::platforms::PLATFORM_WII_U => 4200,
            p if p == crate::platforms::PLATFORM_PS3 => 7500,
            p if p == crate::platforms::PLATFORM_3DS => 1100,
            p if p == crate::platforms::PLATFORM_PS2 || p == crate::platforms::PLATFORM_WII => 2100,
            p if p == crate::platforms::PLATFORM_GAMECUBE || p == crate::platforms::PLATFORM_PSP => 1100,
            _ => 128,
        }
    } else if hay_lower.contains("open world") || hay_lower.contains("cinematic") {
        42_000
    } else if hay_lower.contains("aaa") || hay_lower.contains("mmorpg") || hay_lower.contains("shooter") {
        18_000
    } else if hay_lower.contains("indie")
        || hay_lower.contains("pixel")
        || hay_lower.contains("2d")
        || hay_lower.contains("puzzle")
        || hay_lower.contains("roguelike")
        || hay_lower.contains("party")
        || hay_lower.contains("physics")
    {
        1_800
    } else {
        8_500
    };

    if est_size_mb <= 5120 || entry.platform_id > 0 || (flags & SEM_CONTROLLER_SUPPORT) != 0 {
        flags |= SEM_HANDHELD_FRIENDLY;
    }

    let hltb_tenths = if let Some(explicit_t) = parse_explicit_hours_tenths_hint(hay_lower) {
        explicit_t
    } else if hay_lower.contains("short") || hay_lower.contains("quick") || hay_lower.contains("party") {
        50
    } else if hay_lower.contains("roguelike") || hay_lower.contains("arcade") || hay_lower.contains("platformer") {
        60
    } else if hay_lower.contains("jrpg") || hay_lower.contains("open world") || hay_lower.contains("4x") {
        360
    } else if hay_lower.contains("rpg") {
        140
    } else {
        90
    };

    if hltb_tenths <= 80 {
        flags |= SEM_SHORT_SESSION;
    }

    (est_size_mb, hltb_tenths, flags)
}

pub fn encode_semantic_embedding_i8(entry: &CatalogGameEntry) -> ([i8; 32], u32, u16, u16) {
    let plat_spec = crate::platforms::platform_by_id(entry.platform_id);
    let hay = format!(
        "{} {} {} {} {} {} {} {}",
        entry.name,
        entry.genres.join(" "),
        entry.tags.join(" "),
        entry.categories.join(" "),
        entry.short_description,
        entry.developers.join(" "),
        plat_spec.short_name,
        plat_spec.display_name
    )
    .to_ascii_lowercase();

    let (est_size_mb, hltb_tenths, sem_flags) = estimate_entry_size_and_hltb(entry, &hay);
    let mut weights = [0.0f32; 32];

    for (dim, keywords) in SEMANTIC_CONCEPT_AXES.iter().enumerate() {
        let mut score = 0.0f32;
        for &kw in *keywords {
            if hay.contains(kw) {
                score += 1.0;
            }
        }
        weights[dim] = score;
    }

    if entry.is_coop {
        weights[2] += 2.2;
    }
    if (sem_flags & SEM_CONTROLLER_SUPPORT) != 0 {
        weights[5] += 1.8;
    }
    if (sem_flags & SEM_COZY_RELAXING) != 0 {
        weights[12] += 1.6;
    }
    if (sem_flags & SEM_SHORT_SESSION) != 0 || hltb_tenths <= 70 {
        weights[23] += 1.8;
    } else if hltb_tenths >= 250 {
        weights[24] += 1.5;
    }
    if est_size_mb <= 5120 {
        weights[25] += 1.7;
    }
    if entry.platform_id > 0 {
        if (crate::platforms::PLATFORM_NES..=crate::platforms::PLATFORM_VIRTUAL_BOY)
            .contains(&entry.platform_id)
        {
            weights[28] += 2.4;
        } else if entry.console_group == crate::platforms::GROUP_PLAYSTATION {
            weights[29] += 2.4;
        }
    }
    if entry.is_free || entry.review_pct >= 90.0 {
        weights[31] += 1.0;
    }

    (
        quantize_float_vec_i8(&weights),
        est_size_mb,
        hltb_tenths,
        sem_flags,
    )
}

#[derive(Debug, Clone, Default)]
pub struct ParsedNlSemanticQuery {
    pub is_nl_semantic: bool,
    pub max_size_mb: Option<u32>,
    pub max_hours_tenths: Option<u16>,
    pub require_controller: bool,
    pub require_coop: bool,
    pub inferred_platform: Option<&'static str>,
    pub query_vec: [i8; 32],
    pub content_terms: Vec<String>,
    pub summary: String,
}

pub fn parse_nl_semantic_query(raw: &str) -> ParsedNlSemanticQuery {
    let lower = raw.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return ParsedNlSemanticQuery::default();
    }

    let max_size_mb = if lower.contains("under ")
        || lower.contains("< ")
        || lower.contains("<=")
        || lower.contains("less than ")
        || lower.contains("max ")
        || lower.contains("below ")
    {
        parse_explicit_size_mb_hint(&lower)
    } else {
        None
    };

    let mut max_hours_tenths = parse_explicit_hours_tenths_hint(&lower);
    if max_hours_tenths.is_none() && (lower.contains("short ") || lower.starts_with("short")) {
        max_hours_tenths = Some(80);
    }

    let require_controller = lower.contains("controller support")
        || lower.contains("with controller")
        || lower.contains("gamepad support");
    let require_coop = lower.contains("co-op") || lower.contains("coop");

    let inferred_platform = if lower.contains("switch") {
        Some("switch")
    } else if lower.contains("wii u") || lower.contains("wiiu") {
        Some("wiiu")
    } else if lower.contains("3ds") {
        Some("3ds")
    } else if lower.contains("ps2") {
        Some("ps2")
    } else if lower.contains("nintendo") {
        Some("nintendo")
    } else {
        None
    };

    let has_intent_syntax = max_size_mb.is_some()
        || max_hours_tenths.is_some()
        || require_controller
        || lower.contains(" games")
        || lower.contains(" game ")
        || lower.contains(" with ")
        || lower.contains(" under ")
        || lower.contains(" like ");

    let word_count = lower.split_whitespace().count();
    let is_nl_semantic = has_intent_syntax || (word_count >= 3 && (require_coop || inferred_platform.is_some()));

    let mut weights = [0.0f32; 32];
    for (dim, keywords) in SEMANTIC_CONCEPT_AXES.iter().enumerate() {
        let mut s = 0.0f32;
        for &kw in *keywords {
            if lower.contains(kw) {
                s += 1.4;
            }
        }
        weights[dim] = s;
    }
    if max_size_mb.is_some_and(|mb| mb <= 5500) {
        weights[25] += 1.5;
    }
    if max_hours_tenths.is_some_and(|t| t <= 100) {
        weights[23] += 1.5;
    }
    if require_coop {
        weights[2] += 2.0;
    }
    if require_controller {
        weights[5] += 1.8;
    }

    const STOPWORDS: &[&str] = &[
        "games", "game", "with", "under", "less", "than", "below", "support", "hour", "hours",
        "hr", "hrs", "short", "gb", "mb", "for", "and", "or", "a", "an", "the", "that", "have",
        "good", "best", "play", "tonight", "my", "me", "co-op", "coop", "controller", "gamepad",
        "switch", "wiiu", "wii", "3ds", "ps2", "nintendo", "pc",
    ];
    let mut content_terms = Vec::new();
    for tok in lower.split(|c: char| !c.is_ascii_alphanumeric() && c != '-') {
        let t = tok.trim();
        if t.len() < 2 || t.chars().all(|c| c.is_ascii_digit() || c == '-') {
            continue;
        }
        if t.ends_with("-hour") || t.ends_with("-hr") {
            continue;
        }
        if !STOPWORDS.contains(&t) {
            content_terms.push(t.to_string());
        }
    }

    let mut badges = Vec::new();
    if let Some(mb) = max_size_mb {
        if mb >= 1024 {
            badges.push(format!("<= {:.1} GB", mb as f64 / 1024.0));
        } else {
            badges.push(format!("<= {mb} MB"));
        }
    }
    if let Some(t) = max_hours_tenths {
        badges.push(format!("<= {:.0}h", t as f64 / 10.0));
    }
    if require_coop {
        badges.push("Co-op".to_string());
    }
    if require_controller {
        badges.push("Controller".to_string());
    }
    if let Some(p) = inferred_platform {
        badges.push(p.to_ascii_uppercase());
    }
    for term in content_terms.iter().take(3) {
        badges.push(term.clone());
    }

    ParsedNlSemanticQuery {
        is_nl_semantic,
        max_size_mb,
        max_hours_tenths,
        require_controller,
        require_coop,
        inferred_platform,
        query_vec: quantize_float_vec_i8(&weights),
        content_terms,
        summary: if badges.is_empty() {
            "DKCAT003 Quantized Semantic Ranking".to_string()
        } else {
            format!("DKCAT003 Semantic: {}", badges.join(" · "))
        },
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayTonightCandidate {
    pub game_id: String,
    pub title: String,
    pub exe_path: String,
    pub score: f64,
    pub fit_label: String,
    pub estimated_session_fit: String,
    pub hltb_main_hours: Option<f64>,
    pub matched_online_friends: Vec<String>,
    pub gamepad_friendly: bool,
    pub reasons: Vec<String>,
}

pub fn rank_play_tonight_library(
    library: &[crate::GameEntry],
    session_minutes: u32,
    has_gamepad: bool,
    online_friends: &[(String, String)],
    limit: usize,
) -> Vec<PlayTonightCandidate> {
    let target_hrs = (session_minutes.max(15) as f64) / 60.0;
    let hltb_cache = crate::metadata_ext::load_hltb_cache();
    let mut ranked = Vec::with_capacity(library.len());

    for game in library {
        let norm_title = normalize_catalog_lookup_key(&game.title);
        let genre_lower = game.genre.to_ascii_lowercase();
        let mut score = 40.0f64;
        let mut reasons = Vec::new();

        // 1. Check live online friends playing the same title
        let mut matched_friends = Vec::new();
        for (friend_name, cur_game) in online_friends {
            let norm_friend_game = normalize_catalog_lookup_key(cur_game);
            if !norm_friend_game.is_empty()
                && !norm_title.is_empty()
                && (norm_friend_game == norm_title
                    || norm_friend_game.contains(&norm_title)
                    || norm_title.contains(&norm_friend_game))
            {
                matched_friends.push(friend_name.clone());
            }
        }
        if !matched_friends.is_empty() {
            score += 48.0 + (matched_friends.len().min(3) as f64 * 8.0);
            reasons.push(format!(
                "{} online friend(s) in-game now ({})",
                matched_friends.len(),
                matched_friends.join(", ")
            ));
        }

        // 2. Compare available session minutes against HLTB main hours
        let hltb_hours = hltb_cache
            .entries
            .get(&norm_title)
            .and_then(|h| h.main_story_hours)
            .filter(|&hrs| hrs > 0.0);

        let session_fit = if let Some(main_hrs) = hltb_hours {
            let played_hrs = (game.playtime_seconds as f64) / 3600.0;
            let remaining_hrs = (main_hrs - played_hrs).max(0.5);
            if remaining_hrs <= target_hrs * 1.35 {
                score += 34.0;
                reasons.push(format!(
                    "Finishable in this ~{}m session ({:.1}h remaining of {:.1}h HLTB)",
                    session_minutes, remaining_hrs, main_hrs
                ));
                "Ideal Finish Window".to_string()
            } else if main_hrs <= 10.0 || target_hrs >= 1.5 {
                score += 20.0;
                reasons.push(format!("Strong session pace ({:.1}h HLTB main)", main_hrs));
                "Great Session Fit".to_string()
            } else {
                score += 4.0;
                reasons.push(format!("Longer campaign ({:.1}h HLTB main)", main_hrs));
                "Deep Session".to_string()
            }
        } else if genre_lower.contains("roguelike")
            || genre_lower.contains("arcade")
            || genre_lower.contains("coop")
            || genre_lower.contains("co-op")
            || genre_lower.contains("racing")
            || genre_lower.contains("platformer")
            || game.platform_id > 0
        {
            score += 24.0;
            reasons.push(format!("Fast pick-up-and-play loop for {}m", session_minutes));
            "Pick-Up & Play".to_string()
        } else {
            score += 10.0;
            "Flexible Session".to_string()
        };

        // 3. Gamepad readiness boost
        let gamepad_friendly = game.platform_id > 0
            || genre_lower.contains("action")
            || genre_lower.contains("rpg")
            || genre_lower.contains("platformer")
            || genre_lower.contains("racing")
            || genre_lower.contains("souls")
            || genre_lower.contains("metroidvania")
            || genre_lower.contains("arcade")
            || genre_lower.contains("adventure");
        if has_gamepad && gamepad_friendly {
            score += 22.0;
            reasons.push("Gamepad connected & ready".to_string());
        }

        if game.is_favorite {
            score += 9.0;
            reasons.push("Pinned favorite".to_string());
        }
        if game.playtime_seconds > 0 && game.playtime_seconds < 4 * 3600 {
            score += 6.0;
        }

        let fit_label = if !matched_friends.is_empty() {
            "Squad Active".to_string()
        } else if score >= 95.0 {
            "Top Match".to_string()
        } else if score >= 75.0 {
            "Strong Pick".to_string()
        } else {
            "Good Fit".to_string()
        };

        ranked.push(PlayTonightCandidate {
            game_id: game.id.clone(),
            title: game.title.clone(),
            exe_path: game.exe_path.clone(),
            score,
            fit_label,
            estimated_session_fit: session_fit,
            hltb_main_hours: hltb_hours,
            matched_online_friends: matched_friends,
            gamepad_friendly,
            reasons,
        });
    }

    ranked.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.title.cmp(&b.title))
    });
    ranked.truncate(limit.max(1));
    ranked
}

pub struct MappedCatalog {
    mmap: Mmap,
    path: PathBuf,
    header: DkCatHeader,
    fetched_at: u64,
    fetched_pages: Vec<u32>,
    trigram_count: u32,
    trigram_table_offset: usize,
    postings_offset: usize,
    map_time_us: u64,
}

static GLOBAL_MMAP_CATALOG: OnceLock<RwLock<Option<MappedCatalog>>> = OnceLock::new();

fn global_catalog_slot() -> &'static RwLock<Option<MappedCatalog>> {
    GLOBAL_MMAP_CATALOG.get_or_init(|| RwLock::new(None))
}

fn read_catalog_slot() -> std::sync::RwLockReadGuard<'static, Option<MappedCatalog>> {
    global_catalog_slot()
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn write_catalog_slot() -> std::sync::RwLockWriteGuard<'static, Option<MappedCatalog>> {
    global_catalog_slot()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[inline]
unsafe fn read_struct_unaligned<T: Copy>(bytes: &[u8], offset: usize) -> Result<T, String> {
    let size = core::mem::size_of::<T>();
    let end = offset
        .checked_add(size)
        .ok_or_else(|| "Offset overflow reading binary catalog".to_string())?;
    if end > bytes.len() {
        return Err(format!(
            "Truncated .dkcat archive (needed {end} bytes, have {})",
            bytes.len()
        ));
    }
    Ok(unsafe { core::ptr::read_unaligned(bytes.as_ptr().add(offset).cast::<T>()) })
}

#[inline]
fn bytes_of_struct<T: Copy>(val: &T) -> &[u8] {
    unsafe {
        core::slice::from_raw_parts((val as *const T).cast::<u8>(), core::mem::size_of::<T>())
    }
}

fn push_trigrams(text: &str, out: &mut HashSet<[u8; 3]>) {
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    if bytes.len() < 3 {
        return;
    }
    for win in bytes.windows(3) {
        out.insert([win[0], win[1], win[2]]);
    }
}

fn pack_flags(entry: &CatalogGameEntry) -> u32 {
    let mut flags = 0u32;
    if entry.is_free {
        flags |= FLAG_IS_FREE;
    }
    if entry.is_singleplayer {
        flags |= FLAG_SINGLEPLAYER;
    }
    if entry.is_coop {
        flags |= FLAG_COOP;
    }
    if entry.is_multiplayer {
        flags |= FLAG_MULTIPLAYER;
    }
    if entry.is_online_only_pvp {
        flags |= FLAG_ONLINE_ONLY_PVP;
    }
    if entry.details_enriched {
        flags |= FLAG_DETAILS_ENRICHED;
    }
    let discount = entry.discount_percent.min(255);
    flags |= discount << 8;
    flags
}

#[inline]
fn contains_ignore_ascii_case(haystack: &str, needle_lower: &str) -> bool {
    let n = needle_lower.as_bytes();
    if n.is_empty() {
        return true;
    }
    let h = haystack.as_bytes();
    if n.len() > h.len() {
        return false;
    }
    if n.len() == 1 {
        let b0 = n[0];
        return h.iter().any(|&hb| hb.to_ascii_lowercase() == b0);
    }
    let first = n[0];
    let limit = h.len() - n.len();
    'outer: for i in 0..=limit {
        if h[i].to_ascii_lowercase() != first {
            continue;
        }
        for j in 1..n.len() {
            if h[i + j].to_ascii_lowercase() != n[j] {
                continue 'outer;
            }
        }
        return true;
    }
    false
}

#[inline]
fn starts_with_ignore_ascii_case(haystack: &str, prefix_lower: &str) -> bool {
    let p = prefix_lower.as_bytes();
    let h = haystack.as_bytes();
    if p.len() > h.len() {
        return false;
    }
    for i in 0..p.len() {
        if h[i].to_ascii_lowercase() != p[i] {
            return false;
        }
    }
    true
}

#[inline]
fn normalize_alnum_ascii(input: &str, scratch: &mut String) {
    scratch.clear();
    for b in input.bytes() {
        if b.is_ascii_alphanumeric() {
            scratch.push(b.to_ascii_lowercase() as char);
        }
    }
}

fn pack_genre_bits(entry: &CatalogGameEntry) -> u64 {
    let mut mask = GENRE_MASK_POPULATED;
    for def in &GENRE_BIT_DEFS {
        let matched = entry
            .genres
            .iter()
            .chain(entry.tags.iter())
            .chain(entry.categories.iter())
            .any(|item| {
                def.synonyms
                    .iter()
                    .any(|needle| contains_ignore_ascii_case(item, needle))
            });
        if matched {
            mask |= def.bit;
        }
    }
    mask
}

#[inline]
fn pack_title_8(bytes: &[u8], offset: usize) -> u64 {
    let mut v = 0u64;
    for i in 0..8 {
        let b = bytes
            .get(offset + i)
            .copied()
            .map(|c| c.to_ascii_lowercase())
            .unwrap_or(0);
        v = (v << 8) | (b as u64);
    }
    !v
}

#[inline]
fn cmp_ignore_ascii_case(a: &str, b: &str) -> std::cmp::Ordering {
    let mut ia = a.bytes().map(|c| c.to_ascii_lowercase());
    let mut ib = b.bytes().map(|c| c.to_ascii_lowercase());
    loop {
        match (ia.next(), ib.next()) {
            (Some(ca), Some(cb)) => {
                if ca != cb {
                    return ca.cmp(&cb);
                }
            }
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (None, None) => return std::cmp::Ordering::Equal,
        }
    }
}

#[inline]
fn f64_to_sortable_u64(v: f64) -> u64 {
    if !v.is_finite() {
        return 0;
    }
    let bits = v.to_bits();
    if (bits >> 63) != 0 {
        !bits
    } else {
        bits | (1u64 << 63)
    }
}

#[derive(Clone, Copy)]
struct CandidateKey {
    rec_idx: u32,
    k1: u64,
    k2: u64,
    k3: u64,
}

fn join_safe(items: &[String]) -> String {
    items
        .iter()
        .map(|s| s.replace([FIELD_SEP, LIST_SEP], " "))
        .collect::<Vec<_>>()
        .join(&LIST_SEP.to_string())
}

fn split_safe(raw: &str) -> Vec<String> {
    if raw.is_empty() {
        Vec::new()
    } else {
        raw.split(LIST_SEP)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .collect()
    }
}

fn sanitize_field(raw: &str) -> String {
    raw.replace([FIELD_SEP, LIST_SEP], " ")
}

fn encode_meta(entry: &CatalogGameEntry) -> String {
    let fields = [
        sanitize_field(&entry.price_formatted),
        sanitize_field(&entry.owners_label),
        join_safe(&entry.genres),
        join_safe(&entry.categories),
        sanitize_field(&entry.short_description),
        join_safe(&entry.developers),
        join_safe(&entry.publishers),
        sanitize_field(&entry.release_date),
        sanitize_field(&entry.cover_url),
        sanitize_field(&entry.header_url),
        sanitize_field(&entry.hero_url),
    ];
    fields.join(&FIELD_SEP.to_string())
}

pub fn serialize_dkcat(cache: &CatalogCacheFile) -> Result<Vec<u8>, String> {
    let header_size = core::mem::size_of::<DkCatHeader>();
    let record_size = core::mem::size_of::<CatalogRecord>();
    let record_count = u32::try_from(cache.entries.len())
        .map_err(|_| "Catalog exceeds u32 record limit".to_string())?;

    let records_offset = header_size as u64;
    let records_bytes_len = (record_count as usize)
        .checked_mul(record_size)
        .ok_or_else(|| "Catalog records section overflow".to_string())?;
    let strings_offset = records_offset
        .checked_add(records_bytes_len as u64)
        .ok_or_else(|| "Catalog string offset overflow".to_string())?;

    let mut string_pool = Vec::<u8>::with_capacity(cache.entries.len() * 192);
    let mut records = Vec::<CatalogRecord>::with_capacity(cache.entries.len());
    let mut trigrams: BTreeMap<[u8; 3], Vec<u32>> = BTreeMap::new();

    for (idx, entry) in cache.entries.iter().enumerate() {
        let rec_idx = idx as u32;
        let name_off = u32::try_from(string_pool.len())
            .map_err(|_| "Catalog string pool exceeds 4 GiB".to_string())?;
        string_pool.extend_from_slice(entry.name.as_bytes());
        let name_len = u32::try_from(entry.name.len()).unwrap_or(u32::MAX);

        let tags_joined = entry
            .tags
            .iter()
            .map(|t| t.replace(',', " "))
            .collect::<Vec<_>>()
            .join(", ");
        let tags_off = u32::try_from(string_pool.len())
            .map_err(|_| "Catalog string pool exceeds 4 GiB".to_string())?;
        string_pool.extend_from_slice(tags_joined.as_bytes());
        let tags_len = u32::try_from(tags_joined.len()).unwrap_or(u32::MAX);

        let meta_str = encode_meta(entry);
        let meta_off = u32::try_from(string_pool.len())
            .map_err(|_| "Catalog string pool exceeds 4 GiB".to_string())?;
        string_pool.extend_from_slice(meta_str.as_bytes());
        let meta_len = u32::try_from(meta_str.len()).unwrap_or(u32::MAX);

        let mut unique_tri = HashSet::new();
        push_trigrams(&entry.name, &mut unique_tri);
        let appid_str = entry.app_id.to_string();
        push_trigrams(&appid_str, &mut unique_tri);
        for tag in &entry.tags {
            push_trigrams(tag, &mut unique_tri);
        }
        for genre in &entry.genres {
            push_trigrams(genre, &mut unique_tri);
        }
        for dev in &entry.developers {
            push_trigrams(dev, &mut unique_tri);
        }
        if entry.platform_id > 0 {
            let plat = crate::platforms::platform_by_id(entry.platform_id);
            push_trigrams(plat.short_name, &mut unique_tri);
            push_trigrams(plat.slug, &mut unique_tri);
        }
        for tri in unique_tri {
            trigrams.entry(tri).or_default().push(rec_idx);
        }

        let console_group = if entry.console_group != 0 {
            entry.console_group
        } else {
            crate::platforms::console_group_for_platform(entry.platform_id)
        };

        let (semantic_vec, est_size_mb, hltb_tenths, semantic_flags) =
            encode_semantic_embedding_i8(entry);

        records.push(CatalogRecord {
            ccu: entry.ccu,
            owners_min: entry.owners_min,
            owners_max: entry.owners_max,
            positive: entry.positive,
            negative: entry.negative,
            value_score: entry.value_score,
            updated_at: entry.updated_at,
            genre_bits: pack_genre_bits(entry),
            appid: entry.app_id,
            price_cents: entry.price_cents,
            initial_price_cents: entry.initial_price_cents,
            flags: pack_flags(entry),
            name_off,
            name_len,
            tags_off,
            tags_len,
            meta_off,
            meta_len,
            platform_id: entry.platform_id,
            console_group,
            _pad: [0u8; 5],
            est_size_mb,
            hltb_tenths,
            semantic_flags,
            semantic_vec,
        });
    }

    let trigram_offset = strings_offset
        .checked_add(string_pool.len() as u64)
        .ok_or_else(|| "Catalog trigram offset overflow".to_string())?;

    let mut body = Vec::<u8>::with_capacity(
        records_bytes_len + string_pool.len() + 64 + trigrams.len() * 24,
    );
    for rec in &records {
        body.extend_from_slice(bytes_of_struct(rec));
    }
    body.extend_from_slice(&string_pool);

    // Trigram section header:
    // fetched_at: u64 | trigram_count: u32 | fetched_pages_count: u32 | fetched_pages: [u32] | TrigramIndexEntry[] | postings: [u32]
    body.extend_from_slice(&cache.fetched_at.to_le_bytes());
    let trigram_count = u32::try_from(trigrams.len())
        .map_err(|_| "Trigram index exceeds u32".to_string())?;
    body.extend_from_slice(&trigram_count.to_le_bytes());
    let pages_count = cache.fetched_pages.len() as u32;
    body.extend_from_slice(&pages_count.to_le_bytes());
    for page in &cache.fetched_pages {
        body.extend_from_slice(&page.to_le_bytes());
    }

    let mut postings = Vec::<u8>::new();
    for (tri, mut ids) in trigrams {
        ids.sort_unstable();
        ids.dedup();
        let list_off = u32::try_from(postings.len())
            .map_err(|_| "Trigram postings overflow".to_string())?;
        let list_len = u32::try_from(ids.len()).unwrap_or(u32::MAX);
        for id in ids {
            postings.extend_from_slice(&id.to_le_bytes());
        }
        let tri_entry = TrigramIndexEntry {
            trigram: tri,
            _pad: 0,
            list_off,
            list_len,
        };
        body.extend_from_slice(bytes_of_struct(&tri_entry));
    }
    body.extend_from_slice(&postings);

    let checksum_blake3 = *blake3::hash(&body).as_bytes();
    let header = DkCatHeader {
        magic: DKCAT_MAGIC,
        version: DKCAT_VERSION,
        record_count,
        records_offset,
        strings_offset,
        trigram_offset,
        checksum_blake3,
    };

    let mut out = Vec::with_capacity(header_size + body.len());
    out.extend_from_slice(bytes_of_struct(&header));
    out.extend_from_slice(&body);
    Ok(out)
}

impl MappedCatalog {
    /// Zero-copy opens `catalog.dkcat` via OS `mmap`, validating the 72-byte header, stride alignment,
    /// string-pool offsets, and trigram table bounds before casting or indexing.
    pub fn open(path: &Path) -> Result<Self, String> {
        let started = Instant::now();
        let file =
            File::open(path).map_err(|e| format!("Failed to open {}: {}", path.display(), e))?;
        let mmap =
            unsafe { Mmap::map(&file) }.map_err(|e| format!("Mmap failed for {}: {}", path.display(), e))?;
        let bytes: &[u8] = &mmap;
        let header_len = core::mem::size_of::<DkCatHeader>();
        if bytes.len() < header_len {
            return Err(format!(
                "Truncated .dkcat archive (needed {header_len} header bytes, have {})",
                bytes.len()
            ));
        }

        let header: DkCatHeader = unsafe { read_struct_unaligned(bytes, 0)? };
        if header.magic != DKCAT_MAGIC {
            return Err("Invalid .dkcat magic header".to_string());
        }
        if header.version != DKCAT_VERSION {
            return Err(format!("Unsupported .dkcat version {}", header.version));
        }

        let rec_stride = core::mem::size_of::<CatalogRecord>();
        let rec_bytes = (header.record_count as usize)
            .checked_mul(rec_stride)
            .ok_or_else(|| "Corrupt record count in .dkcat".to_string())?;
        let rec_start = usize::try_from(header.records_offset)
            .map_err(|_| "Corrupt records_offset in .dkcat".to_string())?;
        let str_start = usize::try_from(header.strings_offset)
            .map_err(|_| "Corrupt strings_offset in .dkcat".to_string())?;
        let tri_start = usize::try_from(header.trigram_offset)
            .map_err(|_| "Corrupt trigram_offset in .dkcat".to_string())?;

        let rec_end = rec_start
            .checked_add(rec_bytes)
            .ok_or_else(|| "Corrupt record section bounds in .dkcat".to_string())?;
        let tri_meta_end = tri_start
            .checked_add(16)
            .ok_or_else(|| "Corrupt trigram header offset in .dkcat".to_string())?;

        if rec_start < header_len
            || rec_end > str_start
            || (str_start - rec_start) % rec_stride != 0
            || str_start > tri_start
            || tri_meta_end > bytes.len()
        {
            return Err("Corrupt section bounds or stride alignment in .dkcat header".to_string());
        }

        let fetched_at = u64::from_le_bytes(
            bytes[tri_start..tri_start + 8]
                .try_into()
                .map_err(|_| "Truncated fetched_at".to_string())?,
        );
        let trigram_count = u32::from_le_bytes(
            bytes[tri_start + 8..tri_start + 12]
                .try_into()
                .map_err(|_| "Truncated trigram_count".to_string())?,
        );
        let pages_count = u32::from_le_bytes(
            bytes[tri_start + 12..tri_start + 16]
                .try_into()
                .map_err(|_| "Truncated pages_count".to_string())?,
        ) as usize;

        let pages_start = tri_meta_end;
        let pages_bytes = pages_count
            .checked_mul(4)
            .ok_or_else(|| "Corrupt page count in .dkcat".to_string())?;
        let pages_end = pages_start
            .checked_add(pages_bytes)
            .ok_or_else(|| "Corrupt page table in .dkcat".to_string())?;
        if pages_end > bytes.len() {
            return Err("Truncated page list in .dkcat".to_string());
        }

        let mut fetched_pages = Vec::with_capacity(pages_count);
        for chunk in bytes[pages_start..pages_end].chunks_exact(4) {
            fetched_pages.push(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }

        let trigram_table_offset = pages_end;
        let trigram_table_bytes = (trigram_count as usize)
            .checked_mul(core::mem::size_of::<TrigramIndexEntry>())
            .ok_or_else(|| "Corrupt trigram table in .dkcat".to_string())?;
        let postings_offset = trigram_table_offset
            .checked_add(trigram_table_bytes)
            .ok_or_else(|| "Corrupt postings offset in .dkcat".to_string())?;
        if postings_offset > bytes.len() {
            return Err("Truncated trigram index table in .dkcat".to_string());
        }

        Ok(Self {
            mmap,
            path: path.to_path_buf(),
            header,
            fetched_at,
            fetched_pages,
            trigram_count,
            trigram_table_offset,
            postings_offset,
            map_time_us: started.elapsed().as_micros() as u64,
        })
    }

    /// On-demand full BLAKE3 payload integrity verification (not run on startup open to keep mmap `0 ms`).
    #[allow(dead_code)]
    pub fn verify_blake3(&self) -> bool {
        let hdr_len = core::mem::size_of::<DkCatHeader>();
        if self.mmap.len() < hdr_len {
            return false;
        }
        let actual = *blake3::hash(&self.mmap[hdr_len..]).as_bytes();
        actual == self.header.checksum_blake3
    }

    pub fn stats(&self) -> CatalogMmapStats {
        CatalogMmapStats {
            active: true,
            path: self.path.to_string_lossy().into_owned(),
            file_size_bytes: self.mmap.len() as u64,
            record_count: self.header.record_count,
            trigram_count: self.trigram_count,
            fetched_at: self.fetched_at,
            checksum_hex: hex::encode(self.header.checksum_blake3),
            map_time_us: self.map_time_us,
        }
    }

    #[inline]
    pub fn record_count(&self) -> usize {
        self.header.record_count as usize
    }

    pub fn fetched_at(&self) -> u64 {
        self.fetched_at
    }

    pub fn fetched_pages(&self) -> &[u32] {
        &self.fetched_pages
    }

    pub fn get_entry_by_appid(&self, app_id: u32) -> Option<CatalogGameEntry> {
        for idx in 0..self.record_count() {
            if let Ok(rec) = self.read_record(idx) {
                if rec.appid == app_id {
                    return Some(self.hydrate_record(&rec));
                }
            }
        }
        None
    }

    fn read_record(&self, index: usize) -> Result<CatalogRecord, String> {
        if index >= self.record_count() {
            return Err(format!("Catalog record index {index} out of bounds"));
        }
        let rec_base = usize::try_from(self.header.records_offset)
            .map_err(|_| "Catalog records_offset overflow".to_string())?;
        let byte_off = index
            .checked_mul(core::mem::size_of::<CatalogRecord>())
            .and_then(|rel| rec_base.checked_add(rel))
            .ok_or_else(|| "Catalog record offset overflow".to_string())?;
        unsafe { read_struct_unaligned(&self.mmap, byte_off) }
    }

    fn pool_str(&self, off: u32, len: u32) -> &str {
        let Some(base) = usize::try_from(self.header.strings_offset).ok() else {
            return "";
        };
        let Some(tri) = usize::try_from(self.header.trigram_offset).ok() else {
            return "";
        };
        let Some(start) = base.checked_add(off as usize) else {
            return "";
        };
        let Some(end) = start.checked_add(len as usize) else {
            return "";
        };
        if start <= end && end <= tri && end <= self.mmap.len() {
            std::str::from_utf8(&self.mmap[start..end]).unwrap_or("")
        } else {
            ""
        }
    }

    fn hydrate_record(&self, rec: &CatalogRecord) -> CatalogGameEntry {
        let name = self.pool_str(rec.name_off, rec.name_len).to_string();
        let tags_raw = self.pool_str(rec.tags_off, rec.tags_len);
        let tags: Vec<String> = if tags_raw.is_empty() {
            Vec::new()
        } else {
            tags_raw
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        };

        let meta_raw = self.pool_str(rec.meta_off, rec.meta_len);
        let mut meta_parts = meta_raw.split(FIELD_SEP);
        let price_formatted = meta_parts.next().unwrap_or("").to_string();
        let owners_label = meta_parts.next().unwrap_or("").to_string();
        let genres = split_safe(meta_parts.next().unwrap_or(""));
        let categories = split_safe(meta_parts.next().unwrap_or(""));
        let short_description = meta_parts.next().unwrap_or("").to_string();
        let developers = split_safe(meta_parts.next().unwrap_or(""));
        let publishers = split_safe(meta_parts.next().unwrap_or(""));
        let release_date = meta_parts.next().unwrap_or("").to_string();
        let cover_url = meta_parts.next().unwrap_or("").to_string();
        let header_url = meta_parts.next().unwrap_or("").to_string();
        let hero_url = meta_parts.next().unwrap_or("").to_string();

        let positive = rec.positive;
        let negative = rec.negative;
        let total_reviews = positive.saturating_add(negative);
        let review_pct = if total_reviews > 0 {
            ((positive as f64 / total_reviews as f64) * 1000.0).round() / 10.0
        } else {
            0.0
        };

        let plat = crate::platforms::platform_by_id(rec.platform_id);
        let console_group = if rec.console_group != 0 {
            rec.console_group
        } else {
            plat.console_group
        };

        CatalogGameEntry {
            app_id: rec.appid,
            name,
            platform_id: rec.platform_id,
            console_group,
            platform_name: plat.short_name.to_string(),
            platform_slug: plat.slug.to_string(),
            price_cents: rec.price_cents,
            initial_price_cents: rec.initial_price_cents,
            discount_percent: (rec.flags >> 8) & 0xFF,
            price_formatted,
            is_free: (rec.flags & FLAG_IS_FREE) != 0,
            ccu: rec.ccu,
            owners_min: rec.owners_min,
            owners_max: rec.owners_max,
            owners_label,
            positive,
            negative,
            review_pct,
            is_singleplayer: (rec.flags & FLAG_SINGLEPLAYER) != 0,
            is_coop: (rec.flags & FLAG_COOP) != 0,
            is_multiplayer: (rec.flags & FLAG_MULTIPLAYER) != 0,
            is_online_only_pvp: (rec.flags & FLAG_ONLINE_ONLY_PVP) != 0,
            genres,
            tags,
            categories,
            short_description,
            developers,
            publishers,
            release_date,
            cover_url,
            header_url,
            hero_url,
            value_score: rec.value_score,
            details_enriched: (rec.flags & FLAG_DETAILS_ENRICHED) != 0,
            updated_at: rec.updated_at,
        }
    }

    pub fn to_catalog_cache_file(&self) -> CatalogCacheFile {
        let mut entries = Vec::with_capacity(self.record_count());
        for idx in 0..self.record_count() {
            if let Ok(rec) = self.read_record(idx) {
                entries.push(self.hydrate_record(&rec));
            }
        }
        CatalogCacheFile {
            fetched_at: self.fetched_at,
            fetched_pages: self.fetched_pages.clone(),
            entries,
        }
    }

    fn lookup_trigram_postings_bytes(&self, target: [u8; 3]) -> Option<&[u8]> {
        let count = self.trigram_count as usize;
        if count == 0 {
            return Some(&[]);
        }
        let stride = core::mem::size_of::<TrigramIndexEntry>();
        let mut lo = 0usize;
        let mut hi = count;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let entry_off = mid
                .checked_mul(stride)
                .and_then(|rel| self.trigram_table_offset.checked_add(rel))?;
            let entry: TrigramIndexEntry =
                unsafe { read_struct_unaligned(&self.mmap, entry_off) }.ok()?;
            match entry.trigram.cmp(&target) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => {
                    let start = self.postings_offset.checked_add(entry.list_off as usize)?;
                    let byte_len = (entry.list_len as usize).checked_mul(4)?;
                    let end = start.checked_add(byte_len)?;
                    if end > self.mmap.len() {
                        return Some(&[]);
                    }
                    return Some(&self.mmap[start..end]);
                }
            }
        }
        Some(&[])
    }

    /// Zero-copy Rust-side query, filter, sort, and pagination over 30,000+ `.dkcat` records.
    pub fn query(&self, params: &CatalogQueryParams) -> CatalogQueryPage {
        let started = Instant::now();
        let total_indexed = self.record_count();
        let limit = if params.limit == 0 {
            60
        } else {
            params.limit.clamp(1, 250)
        };
        let offset = params.offset;
        if total_indexed == 0 {
            return CatalogQueryPage {
                items: Vec::new(),
                total_matches: 0,
                total_indexed: 0,
                fetched_at: self.fetched_at,
                offset,
                limit,
                query_time_us: started.elapsed().as_micros() as u64,
                semantic_mode: false,
                semantic_summary: String::new(),
            };
        }

        let min_ccu = params.min_ccu;
        let price_tier = params.price_tier.trim();
        let player_mode = params.player_mode.trim();
        let min_rating_pct = params.min_rating_pct;
        let min_reviews = params.min_reviews;

        let installed_ids: HashSet<u32> = if params.hide_installed {
            params.installed_app_ids.iter().copied().collect()
        } else {
            HashSet::new()
        };
        let installed_titles: HashSet<&str> = if params.hide_installed {
            params
                .installed_titles_norm
                .iter()
                .map(String::as_str)
                .filter(|s| !s.is_empty())
                .collect()
        } else {
            HashSet::new()
        };

        let mut req_standard_mask = 0u64;
        let mut all_standard_genres = true;
        let mut all_genres_lower: Vec<String> = Vec::with_capacity(params.genres.len());
        for g in &params.genres {
            let low = g.trim().to_ascii_lowercase();
            if low.is_empty() {
                continue;
            }
            if let Some(def) = GENRE_BIT_DEFS
                .iter()
                .find(|d| d.name.eq_ignore_ascii_case(&low))
            {
                req_standard_mask |= def.bit;
            } else {
                all_standard_genres = false;
            }
            all_genres_lower.push(low);
        }

        let nl_query = parse_nl_semantic_query(&params.query);
        let semantic_mode = nl_query.is_nl_semantic;
        let q_lower = params.query.trim().to_ascii_lowercase();
        let q_tokens: Vec<&str> = q_lower
            .split_whitespace()
            .filter(|t| !t.is_empty())
            .collect();

        let trigram_candidates: Option<Vec<u32>> = if !semantic_mode && !q_tokens.is_empty() {
            let mut posting_slices: Vec<&[u8]> = Vec::new();
            let mut seen_tris: HashSet<[u8; 3]> = HashSet::new();
            for tok in &q_tokens {
                let bytes = tok.as_bytes();
                if bytes.len() >= 3 {
                    for win in bytes.windows(3) {
                        let tri = [win[0], win[1], win[2]];
                        if seen_tris.insert(tri) {
                            let slice = self.lookup_trigram_postings_bytes(tri).unwrap_or(&[]);
                            if slice.is_empty() {
                                return CatalogQueryPage {
                                    items: Vec::new(),
                                    total_matches: 0,
                                    total_indexed,
                                    fetched_at: self.fetched_at,
                                    offset,
                                    limit,
                                    query_time_us: started.elapsed().as_micros() as u64,
                                    semantic_mode: false,
                                    semantic_summary: String::new(),
                                };
                            }
                            posting_slices.push(slice);
                        }
                    }
                }
            }
            if posting_slices.is_empty() {
                None
            } else {
                posting_slices.sort_by_key(|s| s.len());
                let mut acc = Vec::with_capacity(posting_slices[0].len() / 4);
                for chunk in posting_slices[0].chunks_exact(4) {
                    acc.push(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
                }
                for other in posting_slices.into_iter().skip(1) {
                    intersect_sorted_u32_bytes_in_place(&mut acc, other);
                    if acc.is_empty() {
                        return CatalogQueryPage {
                            items: Vec::new(),
                            total_matches: 0,
                            total_indexed,
                            fetched_at: self.fetched_at,
                            offset,
                            limit,
                            query_time_us: started.elapsed().as_micros() as u64,
                            semantic_mode: false,
                            semantic_summary: String::new(),
                        };
                    }
                }
                Some(acc)
            }
        } else {
            None
        };

        let sort_mode = params.sort_by.trim();
        let explicit_platform_id = params.platform_id;
        let platform_filter_key = if !params.platform.trim().is_empty() {
            params.platform.trim()
        } else {
            params.console_group.trim()
        };
        let has_platform_filter = explicit_platform_id.is_some()
            || (!platform_filter_key.is_empty()
                && !platform_filter_key.eq_ignore_ascii_case("all")
                && !platform_filter_key.eq_ignore_ascii_case("all_systems"));

        let initial_cap = match &trigram_candidates {
            Some(vec) => vec.len(),
            None => 1024.min(total_indexed),
        };
        let mut matched: Vec<CandidateKey> = Vec::with_capacity(initial_cap);
        let mut norm_scratch = String::with_capacity(64);
        let mut id_buf = [0u8; 10];

        let eval_count = match &trigram_candidates {
            Some(vec) => vec.len(),
            None => total_indexed,
        };

        for step in 0..eval_count {
            let rec_idx = match &trigram_candidates {
                Some(vec) => vec[step],
                None => step as u32,
            };
            let Ok(rec) = self.read_record(rec_idx as usize) else {
                continue;
            };

            if has_platform_filter {
                if let Some(pid) = explicit_platform_id {
                    if rec.platform_id != pid {
                        continue;
                    }
                } else if !crate::platforms::record_matches_platform_filter(
                    rec.platform_id,
                    rec.console_group,
                    platform_filter_key,
                ) {
                    continue;
                }
            } else if semantic_mode {
                if let Some(inferred_plat) = nl_query.inferred_platform {
                    if !crate::platforms::record_matches_platform_filter(
                        rec.platform_id,
                        rec.console_group,
                        inferred_plat,
                    ) {
                        continue;
                    }
                }
            }

            if semantic_mode {
                if let Some(max_mb) = nl_query.max_size_mb {
                    if rec.est_size_mb > max_mb {
                        continue;
                    }
                }
                if let Some(max_t) = nl_query.max_hours_tenths {
                    if rec.hltb_tenths > max_t {
                        continue;
                    }
                }
                if nl_query.require_controller && (rec.semantic_flags & SEM_CONTROLLER_SUPPORT) == 0 {
                    continue;
                }
                if nl_query.require_coop && (rec.flags & FLAG_COOP) == 0 {
                    continue;
                }
            }

            let is_free_rec = rec.price_cents == 0 || (rec.flags & FLAG_IS_FREE) != 0;
            match price_tier {
                "paid" => {
                    if is_free_rec {
                        continue;
                    }
                }
                "free" => {
                    if !is_free_rec {
                        continue;
                    }
                }
                "under_10" => {
                    if is_free_rec || rec.price_cents > 1000 {
                        continue;
                    }
                }
                "under_20" => {
                    if is_free_rec || rec.price_cents > 2000 {
                        continue;
                    }
                }
                "over_30" => {
                    if is_free_rec || rec.price_cents < 3000 {
                        continue;
                    }
                }
                _ => {
                    if params.paid_only && is_free_rec {
                        continue;
                    }
                }
            }
            if params.min_price_cents > 0 && rec.price_cents < params.min_price_cents {
                continue;
            }
            if min_ccu > 0 && rec.ccu < min_ccu {
                continue;
            }

            let total_reviews = rec.positive.saturating_add(rec.negative);
            if min_reviews > 0 && total_reviews < min_reviews {
                continue;
            }
            if min_rating_pct > 0 {
                if total_reviews == 0 {
                    continue;
                }
                if (rec.positive as u128) * 100 < (min_rating_pct as u128) * (total_reviews as u128) {
                    continue;
                }
            }

            if params.exclude_pvp && (rec.flags & FLAG_ONLINE_ONLY_PVP) != 0 {
                continue;
            }

            let is_sp = (rec.flags & FLAG_SINGLEPLAYER) != 0;
            let is_coop = (rec.flags & FLAG_COOP) != 0;
            let is_mp = (rec.flags & FLAG_MULTIPLAYER) != 0;
            let mode_ok = match player_mode {
                "singleplayer" => is_sp,
                "coop" => is_coop,
                "multiplayer" => is_mp,
                "all" => true,
                _ => match (params.singleplayer, params.coop) {
                    (true, true) => is_sp || is_coop,
                    (true, false) => is_sp,
                    (false, true) => is_coop,
                    (false, false) => true,
                },
            };
            if !mode_ok {
                continue;
            }

            if params.hide_installed && installed_ids.contains(&rec.appid) {
                continue;
            }

            let mut cached_tags: Option<&str> = None;
            let mut cached_genres: Option<&str> = None;
            let mut cached_categories: Option<&str> = None;
            let mut cached_devs: Option<&str> = None;

            if !all_genres_lower.is_empty() {
                if all_standard_genres && (rec.genre_bits & GENRE_MASK_POPULATED) != 0 {
                    if (rec.genre_bits & req_standard_mask) != req_standard_mask {
                        continue;
                    }
                } else {
                    let tags_str = self.pool_str(rec.tags_off, rec.tags_len);
                    let meta_str = self.pool_str(rec.meta_off, rec.meta_len);
                    let mut parts = meta_str.split(FIELD_SEP);
                    let _ = parts.next();
                    let _ = parts.next();
                    let genres_str = parts.next().unwrap_or("");
                    let categories_str = parts.next().unwrap_or("");
                    let _ = parts.next();
                    let devs_str = parts.next().unwrap_or("");
                    cached_tags = Some(tags_str);
                    cached_genres = Some(genres_str);
                    cached_categories = Some(categories_str);
                    cached_devs = Some(devs_str);

                    let genre_ok = all_genres_lower.iter().all(|g_low| {
                        contains_ignore_ascii_case(genres_str, g_low)
                            || contains_ignore_ascii_case(tags_str, g_low)
                            || contains_ignore_ascii_case(categories_str, g_low)
                    });
                    if !genre_ok {
                        continue;
                    }
                }
            }

            let name_str = self.pool_str(rec.name_off, rec.name_len);
            if params.hide_installed && !installed_titles.is_empty() {
                normalize_alnum_ascii(name_str, &mut norm_scratch);
                if !norm_scratch.is_empty() && installed_titles.contains(norm_scratch.as_str()) {
                    continue;
                }
            }

            let mut semantic_score_bonus = 0i64;
            if semantic_mode {
                let dot = dot_i8_32(&rec.semantic_vec, &nl_query.query_vec) as i64;
                semantic_score_bonus = dot.max(0);
                if !nl_query.content_terms.is_empty() {
                    let tags_str = cached_tags
                        .unwrap_or_else(|| self.pool_str(rec.tags_off, rec.tags_len));
                    let (genres_str, desc_str) =
                        if let Some(g) = cached_genres {
                            let _ = cached_categories;
                            let _ = cached_devs;
                            (g, "")
                        } else {
                            let meta_str = self.pool_str(rec.meta_off, rec.meta_len);
                            let mut parts = meta_str.split(FIELD_SEP);
                            let _ = parts.next();
                            let _ = parts.next();
                            let g = parts.next().unwrap_or("");
                            let _ = parts.next();
                            let desc = parts.next().unwrap_or("");
                            (g, desc)
                        };
                    let plat_spec = crate::platforms::platform_by_id(rec.platform_id);
                    let mut term_hits = 0i64;
                    for term in &nl_query.content_terms {
                        if contains_ignore_ascii_case(name_str, term)
                            || contains_ignore_ascii_case(tags_str, term)
                            || contains_ignore_ascii_case(genres_str, term)
                            || (!desc_str.is_empty() && contains_ignore_ascii_case(desc_str, term))
                            || (rec.platform_id > 0
                                && (contains_ignore_ascii_case(plat_spec.short_name, term)
                                    || contains_ignore_ascii_case(plat_spec.display_name, term)))
                        {
                            term_hits += 1;
                        }
                    }
                    semantic_score_bonus += term_hits * 45_000;
                }
                if semantic_score_bonus <= 0 {
                    continue;
                }
            } else if !q_tokens.is_empty() {
                let tags_str = cached_tags
                    .unwrap_or_else(|| self.pool_str(rec.tags_off, rec.tags_len));
                let (genres_str, devs_str) =
                    if let (Some(g), Some(d)) = (cached_genres, cached_devs) {
                        let _ = cached_categories;
                        (g, d)
                    } else {
                        let meta_str = self.pool_str(rec.meta_off, rec.meta_len);
                        let mut parts = meta_str.split(FIELD_SEP);
                        let _ = parts.next();
                        let _ = parts.next();
                        let g = parts.next().unwrap_or("");
                        let _ = parts.next();
                        let _ = parts.next();
                        let d = parts.next().unwrap_or("");
                        (g, d)
                    };
                let appid_str = format_u32_stack(rec.appid, &mut id_buf);
                let plat_spec = crate::platforms::platform_by_id(rec.platform_id);
                let all_toks_match = q_tokens.iter().all(|&tok| {
                    contains_ignore_ascii_case(name_str, tok)
                        || contains_ignore_ascii_case(appid_str, tok)
                        || contains_ignore_ascii_case(genres_str, tok)
                        || contains_ignore_ascii_case(tags_str, tok)
                        || contains_ignore_ascii_case(devs_str, tok)
                        || (rec.platform_id > 0
                            && (contains_ignore_ascii_case(plat_spec.short_name, tok)
                                || contains_ignore_ascii_case(plat_spec.display_name, tok)))
                });
                if !all_toks_match {
                    continue;
                }
            }

            let rating_tenths = if total_reviews > 0 {
                ((rec.positive as u128 * 1000 + (total_reviews as u128 / 2))
                    / total_reviews as u128) as u64
            } else {
                0
            };

            let effective_sort = if semantic_mode && matches!(sort_mode, "" | "value_score" | "value" | "relevancy") {
                "semantic"
            } else {
                sort_mode
            };

            let (k1, k2, k3) = match effective_sort {
                "semantic" => (
                    semantic_score_bonus as u64,
                    rating_tenths,
                    f64_to_sortable_u64(rec.value_score),
                ),
                "ccu_desc" | "ccu" => (
                    rec.ccu,
                    f64_to_sortable_u64(rec.value_score),
                    rec.owners_min,
                ),
                "rating_desc" | "rating" => (rating_tenths, total_reviews, rec.ccu),
                "reviews_desc" => (total_reviews, rec.positive, rec.ccu),
                "hidden_gems" => {
                    let tier = if rating_tenths >= 880
                        && total_reviews >= 20
                        && rec.ccu <= 2_500
                        && rec.owners_min <= 1_000_000
                    {
                        3u64
                    } else if rating_tenths >= 880 && total_reviews >= 10 && rec.ccu <= 5_000 {
                        2u64
                    } else if rating_tenths >= 880 {
                        1u64
                    } else {
                        0u64
                    };
                    let confidence_pts = if total_reviews > 0 {
                        (((total_reviews.min(10_000) as f64) + 1.0).log10() * 350.0) as u64
                    } else {
                        0
                    };
                    let gem_score = rating_tenths
                        .saturating_mul(10)
                        .saturating_add(confidence_pts)
                        .saturating_add(if rec.ccu > 0 && rec.ccu <= 2_500 {
                            400
                        } else {
                            0
                        });
                    (tier, gem_score, u64::MAX - rec.ccu.min(u64::MAX - 1))
                }
                "trending" => {
                    let ccu_pts = (((rec.ccu as f64) + 1.0).log10() * 1400.0) as u64;
                    let recency_pts = if rec.platform_id == 0
                        && !crate::platforms::is_synthetic_retro_appid(rec.appid)
                    {
                        (((rec.appid as f64).min(4_000_000.0) / 4_000_000.0) * 3200.0) as u64
                    } else {
                        0
                    };
                    let discount_pts = if ((rec.flags >> 8) & 0xFF) >= 20 { 450 } else { 0 };
                    let rating_pts = if total_reviews >= 20 {
                        rating_tenths * 2
                    } else {
                        0
                    };
                    let trend_score = if rec.ccu >= 10 {
                        ccu_pts + recency_pts + discount_pts + rating_pts
                    } else {
                        ccu_pts + (rating_pts / 2)
                    };
                    (trend_score, rec.ccu, rec.appid as u64)
                }
                "price_desc" | "price" => (
                    rec.price_cents as u64,
                    f64_to_sortable_u64(rec.value_score),
                    rec.ccu,
                ),
                "price_asc" => (
                    u64::MAX - (rec.price_cents as u64),
                    f64_to_sortable_u64(rec.value_score),
                    rec.ccu,
                ),
                "newest_appid" => (
                    if rec.platform_id == 0
                        && !crate::platforms::is_synthetic_retro_appid(rec.appid)
                    {
                        rec.appid as u64
                    } else {
                        0
                    },
                    rec.ccu,
                    f64_to_sortable_u64(rec.value_score),
                ),
                "title_asc" => {
                    let nb = name_str.as_bytes();
                    (
                        pack_title_8(nb, 0),
                        pack_title_8(nb, 8),
                        pack_title_8(nb, 16),
                    )
                }
                "owners" => (
                    rec.owners_min.saturating_add(rec.owners_max),
                    rec.ccu,
                    f64_to_sortable_u64(rec.value_score),
                ),
                "relevancy" => {
                    let rank = compute_relevancy_rank(name_str, &q_lower, &q_tokens);
                    (rank, f64_to_sortable_u64(rec.value_score), rec.ccu)
                }
                _ => (
                    f64_to_sortable_u64(rec.value_score),
                    rec.ccu,
                    rec.owners_min,
                ),
            };

            matched.push(CandidateKey {
                rec_idx,
                k1,
                k2,
                k3,
            });
        }

        let total_matches = matched.len();
        let is_title_asc = sort_mode == "title_asc";
        let cmp_desc = |a: &CandidateKey, b: &CandidateKey| {
            let ord = b
                .k1
                .cmp(&a.k1)
                .then_with(|| b.k2.cmp(&a.k2))
                .then_with(|| b.k3.cmp(&a.k3));
            if ord != std::cmp::Ordering::Equal {
                return ord;
            }
            if is_title_asc {
                if let (Ok(ra), Ok(rb)) = (
                    self.read_record(a.rec_idx as usize),
                    self.read_record(b.rec_idx as usize),
                ) {
                    let na = self.pool_str(ra.name_off, ra.name_len);
                    let nb = self.pool_str(rb.name_off, rb.name_len);
                    let tie = cmp_ignore_ascii_case(na, nb);
                    if tie != std::cmp::Ordering::Equal {
                        return tie;
                    }
                }
            }
            a.rec_idx.cmp(&b.rec_idx)
        };

        let end = offset.saturating_add(limit).min(total_matches);
        let items = if offset < end {
            if end < total_matches {
                matched.select_nth_unstable_by(end - 1, cmp_desc);
            }
            matched[..end].sort_unstable_by(cmp_desc);
            let mut out = Vec::with_capacity(end - offset);
            for key in &matched[offset..end] {
                if let Ok(rec) = self.read_record(key.rec_idx as usize) {
                    out.push(self.hydrate_record(&rec));
                }
            }
            out
        } else {
            Vec::new()
        };

        CatalogQueryPage {
            items,
            total_matches,
            total_indexed,
            fetched_at: self.fetched_at,
            offset,
            limit,
            query_time_us: started.elapsed().as_micros() as u64,
            semantic_mode,
            semantic_summary: if semantic_mode {
                nl_query.summary
            } else {
                String::new()
            },
        }
    }

    /// Searches mapped records via trigram inverted index intersection in microseconds.
    pub fn search(&self, query: &str, limit: usize) -> Vec<CatalogGameEntry> {
        let params = CatalogQueryParams {
            query: query.to_string(),
            sort_by: if query.trim().is_empty() {
                "value".to_string()
            } else {
                "relevancy".to_string()
            },
            limit: limit.clamp(1, 250),
            ..CatalogQueryParams::default()
        };
        self.query(&params).items
    }
}

fn format_u32_stack(mut val: u32, buf: &mut [u8; 10]) -> &str {
    let mut idx = buf.len();
    loop {
        idx -= 1;
        buf[idx] = b'0' + (val % 10) as u8;
        val /= 10;
        if val == 0 {
            break;
        }
    }
    unsafe { std::str::from_utf8_unchecked(&buf[idx..]) }
}

fn compute_relevancy_rank(name: &str, query_lower: &str, tokens: &[&str]) -> u64 {
    if query_lower.is_empty() {
        return 0;
    }
    if name.eq_ignore_ascii_case(query_lower) {
        return 1000;
    }
    if starts_with_ignore_ascii_case(name, query_lower) {
        return 800;
    }
    let mut score = 0u64;
    for &tok in tokens {
        if contains_ignore_ascii_case(name, tok) {
            score += 120;
        }
    }
    score
}

fn intersect_sorted_u32_bytes_in_place(acc: &mut Vec<u32>, b_bytes: &[u8]) {
    let b_len = b_bytes.len() / 4;
    if acc.is_empty() || b_len == 0 {
        acc.clear();
        return;
    }

    #[inline]
    fn read_le_u32(bytes: &[u8], idx: usize) -> u32 {
        let o = idx * 4;
        u32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]])
    }

    let mut write_idx = 0usize;
    if b_len > acc.len().saturating_mul(8) {
        let mut lo = 0usize;
        for read_i in 0..acc.len() {
            let target = acc[read_i];
            let mut hi = b_len;
            let mut found = false;
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                let mid_val = read_le_u32(b_bytes, mid);
                match mid_val.cmp(&target) {
                    std::cmp::Ordering::Less => lo = mid + 1,
                    std::cmp::Ordering::Greater => hi = mid,
                    std::cmp::Ordering::Equal => {
                        found = true;
                        lo = mid + 1;
                        break;
                    }
                }
            }
            if found {
                acc[write_idx] = target;
                write_idx += 1;
            }
        }
    } else {
        let (mut i, mut j) = (0usize, 0usize);
        while i < acc.len() && j < b_len {
            let a_val = acc[i];
            let b_val = read_le_u32(b_bytes, j);
            match a_val.cmp(&b_val) {
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => j += 1,
                std::cmp::Ordering::Equal => {
                    acc[write_idx] = a_val;
                    write_idx += 1;
                    i += 1;
                    j += 1;
                }
            }
        }
    }
    acc.truncate(write_idx);
}

pub fn atomic_rebuild_dkcat(dkcat_path: &Path, cache: &CatalogCacheFile) -> Result<CatalogMmapStats, String> {
    let bytes = serialize_dkcat(cache)?;
    if let Some(parent) = dkcat_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create catalog dir {}: {}", parent.display(), e))?;
    }
    let tmp_path = dkcat_path.with_extension(format!("dkcat.tmp.{}", std::process::id()));
    fs::write(&tmp_path, &bytes)
        .map_err(|e| format!("Failed to write temp .dkcat archive: {}", e))?;

    // Release active Windows memory mapping handle before replacing catalog.dkcat.
    let mut slot = write_catalog_slot();
    *slot = None;

    if dkcat_path.exists() {
        let _ = fs::remove_file(dkcat_path);
    }
    if let Err(err) = fs::rename(&tmp_path, dkcat_path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(format!(
            "Failed to replace {}: {}",
            dkcat_path.display(),
            err
        ));
    }

    // Purge legacy catalog_cache.json so RAM and disk are never duplicated.
    let legacy_json = dkcat_path.with_file_name("catalog_cache.json");
    if legacy_json.is_file() {
        let _ = fs::remove_file(&legacy_json);
    }

    let mapped = MappedCatalog::open(dkcat_path)?;
    let stats = mapped.stats();
    *slot = Some(mapped);
    Ok(stats)
}

fn pool_str_slice(bytes: &[u8], strings_offset: usize, tri_offset: usize, off: u32, len: u32) -> &str {
    let start = strings_offset.saturating_add(off as usize);
    let end = start.saturating_add(len as usize);
    if start <= end && end <= tri_offset && end <= bytes.len() {
        std::str::from_utf8(&bytes[start..end]).unwrap_or("")
    } else {
        ""
    }
}

#[allow(clippy::too_many_arguments)]
fn hydrate_legacy_entry(
    bytes: &[u8],
    str_start: usize,
    tri_start: usize,
    appid: u32,
    platform_id: u16,
    console_group: u8,
    price_cents: u32,
    initial_price_cents: u32,
    flags: u32,
    ccu: u64,
    owners_min: u64,
    owners_max: u64,
    positive: u64,
    negative: u64,
    value_score: f64,
    updated_at: u64,
    name_off: u32,
    name_len: u32,
    tags_off: u32,
    tags_len: u32,
    meta_off: u32,
    meta_len: u32,
) -> CatalogGameEntry {
    let name = pool_str_slice(bytes, str_start, tri_start, name_off, name_len).to_string();
    let tags_raw = pool_str_slice(bytes, str_start, tri_start, tags_off, tags_len);
    let tags: Vec<String> = if tags_raw.is_empty() {
        Vec::new()
    } else {
        tags_raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    };
    let meta_raw = pool_str_slice(bytes, str_start, tri_start, meta_off, meta_len);
    let mut meta_parts = meta_raw.split(FIELD_SEP);
    let price_formatted = meta_parts.next().unwrap_or("").to_string();
    let owners_label = meta_parts.next().unwrap_or("").to_string();
    let genres = split_safe(meta_parts.next().unwrap_or(""));
    let categories = split_safe(meta_parts.next().unwrap_or(""));
    let short_description = meta_parts.next().unwrap_or("").to_string();
    let developers = split_safe(meta_parts.next().unwrap_or(""));
    let publishers = split_safe(meta_parts.next().unwrap_or(""));
    let release_date = meta_parts.next().unwrap_or("").to_string();
    let cover_url = meta_parts.next().unwrap_or("").to_string();
    let header_url = meta_parts.next().unwrap_or("").to_string();
    let hero_url = meta_parts.next().unwrap_or("").to_string();

    let total_reviews = positive.saturating_add(negative);
    let review_pct = if total_reviews > 0 {
        ((positive as f64 / total_reviews as f64) * 1000.0).round() / 10.0
    } else {
        0.0
    };
    let plat_spec = crate::platforms::platform_by_id(platform_id);
    let resolved_cg = if console_group != 0 {
        console_group
    } else {
        plat_spec.console_group
    };

    CatalogGameEntry {
        app_id: appid,
        name,
        platform_id,
        console_group: resolved_cg,
        platform_name: plat_spec.short_name.to_string(),
        platform_slug: plat_spec.slug.to_string(),
        price_cents,
        initial_price_cents,
        discount_percent: (flags >> 8) & 0xFF,
        price_formatted,
        is_free: (flags & FLAG_IS_FREE) != 0,
        ccu,
        owners_min,
        owners_max,
        owners_label,
        positive,
        negative,
        review_pct,
        is_singleplayer: (flags & FLAG_SINGLEPLAYER) != 0,
        is_coop: (flags & FLAG_COOP) != 0,
        is_multiplayer: (flags & FLAG_MULTIPLAYER) != 0,
        is_online_only_pvp: (flags & FLAG_ONLINE_ONLY_PVP) != 0,
        genres,
        tags,
        categories,
        short_description,
        developers,
        publishers,
        release_date,
        cover_url,
        header_url,
        hero_url,
        value_score,
        details_enriched: (flags & FLAG_DETAILS_ENRICHED) != 0,
        updated_at,
    }
}

/// Automatically upgrades legacy `DKCAT001` (`CatalogRecordV1` 104B) or `DKCAT002` (`CatalogRecordV2` 112B)
/// archives into `DKCAT003` (`152B` with quantized `[i8; 32]` semantic embeddings) without data loss.
pub fn try_migrate_dkcat_v1_to_v2(dkcat_path: &Path) -> Result<bool, String> {
    try_migrate_dkcat_legacy_to_v3(dkcat_path)
}

pub fn try_migrate_dkcat_legacy_to_v3(dkcat_path: &Path) -> Result<bool, String> {
    if !dkcat_path.is_file() {
        return Ok(false);
    }
    let cache_opt = {
        let file = File::open(dkcat_path)
            .map_err(|e| format!("Failed to open legacy catalog {}: {}", dkcat_path.display(), e))?;
        let mmap = unsafe { Mmap::map(&file) }
            .map_err(|e| format!("Mmap failed during catalog migration: {}", e))?;
        let bytes: &[u8] = &mmap;
        let header_len = core::mem::size_of::<DkCatHeader>();
        if bytes.len() < header_len {
            return Err("Truncated legacy .dkcat header".to_string());
        }
        let header: DkCatHeader = unsafe { read_struct_unaligned(bytes, 0)? };
        let is_v1 = header.magic == DKCAT_MAGIC_V1 && header.version == 2;
        let is_v2 = header.magic == DKCAT_MAGIC_V2 && header.version == 3;
        if !is_v1 && !is_v2 {
            None
        } else {
            let rec_size = if is_v1 {
                core::mem::size_of::<CatalogRecordV1>()
            } else {
                core::mem::size_of::<CatalogRecordV2>()
            };
            let rec_start = usize::try_from(header.records_offset)
                .map_err(|_| "Corrupt legacy records_offset".to_string())?;
            let str_start = usize::try_from(header.strings_offset)
                .map_err(|_| "Corrupt legacy strings_offset".to_string())?;
            let tri_start = usize::try_from(header.trigram_offset)
                .map_err(|_| "Corrupt legacy trigram_offset".to_string())?;
            let rec_bytes = (header.record_count as usize)
                .checked_mul(rec_size)
                .ok_or_else(|| "Corrupt legacy record count".to_string())?;
            let rec_end = rec_start
                .checked_add(rec_bytes)
                .ok_or_else(|| "Corrupt legacy record bounds".to_string())?;
            let tri_meta_end = tri_start
                .checked_add(16)
                .ok_or_else(|| "Corrupt legacy trigram offset".to_string())?;
            if rec_start < header_len
                || rec_end > str_start
                || (str_start - rec_start) % rec_size != 0
                || str_start > tri_start
                || tri_meta_end > bytes.len()
            {
                return Err("Corrupt section bounds in legacy .dkcat header".to_string());
            }

            let fetched_at = u64::from_le_bytes(
                bytes[tri_start..tri_start + 8]
                    .try_into()
                    .map_err(|_| "Truncated legacy fetched_at".to_string())?,
            );
            let pages_count = u32::from_le_bytes(
                bytes[tri_start + 12..tri_start + 16]
                    .try_into()
                    .map_err(|_| "Truncated legacy pages_count".to_string())?,
            ) as usize;
            let pages_start = tri_meta_end;
            let pages_bytes = pages_count
                .checked_mul(4)
                .ok_or_else(|| "Corrupt legacy page count".to_string())?;
            let pages_end = pages_start
                .checked_add(pages_bytes)
                .ok_or_else(|| "Corrupt legacy page table".to_string())?;
            if pages_end > bytes.len() {
                return Err("Truncated legacy page list".to_string());
            }
            let mut fetched_pages = Vec::with_capacity(pages_count);
            for chunk in bytes[pages_start..pages_end].chunks_exact(4) {
                fetched_pages.push(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
            }

            let mut entries = Vec::with_capacity(header.record_count as usize);
            for idx in 0..header.record_count as usize {
                let off = rec_start + idx * rec_size;
                if is_v1 {
                    let v1: CatalogRecordV1 = unsafe { read_struct_unaligned(bytes, off)? };
                    entries.push(hydrate_legacy_entry(
                        bytes,
                        str_start,
                        tri_start,
                        v1.appid,
                        crate::platforms::PLATFORM_PC,
                        0,
                        v1.price_cents,
                        v1.initial_price_cents,
                        v1.flags,
                        v1.ccu,
                        v1.owners_min,
                        v1.owners_max,
                        v1.positive,
                        v1.negative,
                        v1.value_score,
                        v1.updated_at,
                        v1.name_off,
                        v1.name_len,
                        v1.tags_off,
                        v1.tags_len,
                        v1.meta_off,
                        v1.meta_len,
                    ));
                } else {
                    let v2: CatalogRecordV2 = unsafe { read_struct_unaligned(bytes, off)? };
                    entries.push(hydrate_legacy_entry(
                        bytes,
                        str_start,
                        tri_start,
                        v2.appid,
                        v2.platform_id,
                        v2.console_group,
                        v2.price_cents,
                        v2.initial_price_cents,
                        v2.flags,
                        v2.ccu,
                        v2.owners_min,
                        v2.owners_max,
                        v2.positive,
                        v2.negative,
                        v2.value_score,
                        v2.updated_at,
                        v2.name_off,
                        v2.name_len,
                        v2.tags_off,
                        v2.tags_len,
                        v2.meta_off,
                        v2.meta_len,
                    ));
                }
            }
            Some(CatalogCacheFile {
                fetched_at,
                fetched_pages,
                entries,
            })
        }
    };

    if let Some(cache) = cache_opt {
        atomic_rebuild_dkcat(dkcat_path, &cache)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

pub fn open_or_rebuild_dkcat_at(
    dkcat_path: &Path,
    legacy_json_path: Option<&Path>,
) -> Result<(), String> {
    let cleanup_json = || {
        if let Some(jp) = legacy_json_path {
            if jp.is_file() {
                let _ = fs::remove_file(jp);
            }
        }
    };

    if dkcat_path.is_file() {
        if let Ok(mapped) = MappedCatalog::open(dkcat_path) {
            let mut write_guard = write_catalog_slot();
            *write_guard = Some(mapped);
            cleanup_json();
            return Ok(());
        }
        if matches!(try_migrate_dkcat_v1_to_v2(dkcat_path), Ok(true)) {
            cleanup_json();
            return Ok(());
        }

        // Corrupt, truncated, or unmigratable archive on disk: release any mmap handle,
        // delete the bad file, and rebuild cleanly so startup never crashes.
        {
            let mut write_guard = write_catalog_slot();
            *write_guard = None;
        }
        let _ = fs::remove_file(dkcat_path);

        if let Some(jp) = legacy_json_path {
            if jp.is_file() {
                if let Ok(raw) = fs::read_to_string(jp) {
                    if let Ok(parsed) = serde_json::from_str::<CatalogCacheFile>(&raw) {
                        atomic_rebuild_dkcat(dkcat_path, &parsed)?;
                        let _ = fs::remove_file(jp);
                        return Ok(());
                    }
                }
            }
        }
        let _ = atomic_rebuild_dkcat(dkcat_path, &CatalogCacheFile::default());
        return Ok(());
    }

    // One-time transparent upgrade from legacy catalog_cache.json if present.
    if let Some(jp) = legacy_json_path {
        if jp.is_file() {
            if let Ok(raw) = fs::read_to_string(jp) {
                if let Ok(parsed) = serde_json::from_str::<CatalogCacheFile>(&raw) {
                    atomic_rebuild_dkcat(dkcat_path, &parsed)?;
                    let _ = fs::remove_file(jp);
                    return Ok(());
                }
            }
        }
    }
    Ok(())
}

pub fn ensure_catalog_mapped_standalone() -> Result<(), String> {
    let json_path = get_app_data_file_standalone("catalog_cache.json")?;
    {
        let read_guard = read_catalog_slot();
        if read_guard.is_some() {
            if json_path.is_file() {
                let _ = fs::remove_file(&json_path);
            }
            return Ok(());
        }
    }
    let dkcat_path = get_app_data_file_standalone("catalog.dkcat")?;
    open_or_rebuild_dkcat_at(&dkcat_path, Some(&json_path))
}

pub fn catalog_mmap_header_meta() -> (usize, u64, Vec<u32>) {
    let _ = ensure_catalog_mapped_standalone();
    let guard = read_catalog_slot();
    match guard.as_ref() {
        Some(mapped) => (
            mapped.record_count(),
            mapped.fetched_at(),
            mapped.fetched_pages().to_vec(),
        ),
        None => (0, 0, Vec::new()),
    }
}

pub fn get_entry_via_mmap(app_id: u32) -> Option<CatalogGameEntry> {
    let _ = ensure_catalog_mapped_standalone();
    let guard = read_catalog_slot();
    guard.as_ref().and_then(|m| m.get_entry_by_appid(app_id))
}

fn normalize_catalog_lookup_key(raw: &str) -> String {
    raw.chars()
        .filter_map(|c| {
            if c.is_ascii_alphanumeric() {
                Some(c.to_ascii_lowercase())
            } else {
                None
            }
        })
        .collect()
}

/// Resolves a catalog entry in sub-millisecond time from the memory-mapped `catalog.dkcat` index,
/// checking explicit `app_id` first and falling back to normalized trigram title matching.
pub fn lookup_entry_by_title_or_appid_via_mmap(
    app_id: Option<u32>,
    title: &str,
) -> Option<CatalogGameEntry> {
    let _ = ensure_catalog_mapped_standalone();
    let guard = read_catalog_slot();
    let mapped = guard.as_ref()?;
    if let Some(id) = app_id.filter(|&id| id > 0) {
        if let Some(hit) = mapped.get_entry_by_appid(id) {
            return Some(hit);
        }
    }
    let cleaned = crate::clean_game_title(title);
    let query = if cleaned.trim().is_empty() {
        title.trim().to_string()
    } else {
        cleaned
    };
    if query.is_empty() {
        return None;
    }
    let target_key = normalize_catalog_lookup_key(&query);
    if target_key.len() < 3 {
        return None;
    }
    let candidates = mapped.search(&query, 15);
    if let Some(exact) = candidates
        .iter()
        .find(|c| normalize_catalog_lookup_key(&c.name) == target_key)
    {
        return Some(exact.clone());
    }
    let query_tokens: Vec<String> = query
        .split_whitespace()
        .map(normalize_catalog_lookup_key)
        .filter(|t| !t.is_empty())
        .collect();
    candidates.into_iter().find(|c| {
        let cand_clean = crate::clean_game_title(&c.name);
        let cand_key = normalize_catalog_lookup_key(&cand_clean);
        cand_key == target_key
            || (!query_tokens.is_empty()
                && query_tokens.iter().all(|t| cand_key.contains(t.as_str()))
                && cand_key.len() <= target_key.len() + 8)
    })
}

/// Resolves a retro console title or serial/ROM ID from `catalog.dkcat` filtered to `platform_id`.
pub fn lookup_retro_entry_by_title_or_id(
    platform_id: u16,
    rom_id: &str,
    title: &str,
) -> Option<CatalogGameEntry> {
    if platform_id == 0 {
        return None;
    }
    let _ = ensure_catalog_mapped_standalone();
    let guard = read_catalog_slot();
    let mapped = guard.as_ref()?;

    let clean_serial = rom_id.trim();
    if !clean_serial.is_empty() {
        let page = mapped.query(&CatalogQueryParams {
            query: clean_serial.to_string(),
            platform_id: Some(platform_id),
            limit: 8,
            ..CatalogQueryParams::default()
        });
        if let Some(hit) = page.items.into_iter().next() {
            return Some(hit);
        }
    }

    let cleaned = crate::clean_game_title(title);
    let q = if cleaned.trim().is_empty() {
        title.trim().to_string()
    } else {
        cleaned
    };
    if q.is_empty() {
        return None;
    }
    let page = mapped.query(&CatalogQueryParams {
        query: q.clone(),
        platform_id: Some(platform_id),
        sort_by: "relevancy".to_string(),
        limit: 12,
        ..CatalogQueryParams::default()
    });
    let target_key = normalize_catalog_lookup_key(&q);
    if let Some(exact) = page
        .items
        .iter()
        .find(|c| normalize_catalog_lookup_key(&crate::clean_game_title(&c.name)) == target_key)
    {
        return Some(exact.clone());
    }
    page.items.into_iter().next()
}

pub fn upsert_entries_dkcat(
    dkcat_path: &Path,
    updates: &[CatalogGameEntry],
    fetched_at_override: Option<u64>,
    extra_pages: &[u32],
) -> Result<CatalogMmapStats, String> {
    let mut cache = {
        let _ = ensure_catalog_mapped_standalone();
        let guard = read_catalog_slot();
        match guard.as_ref() {
            Some(mapped) => mapped.to_catalog_cache_file(),
            None if dkcat_path.is_file() => MappedCatalog::open(dkcat_path)
                .map(|m| m.to_catalog_cache_file())
                .unwrap_or_default(),
            None => CatalogCacheFile::default(),
        }
    };

    if let Some(ts) = fetched_at_override {
        cache.fetched_at = ts;
    } else if cache.fetched_at == 0 {
        cache.fetched_at = updates.iter().map(|u| u.updated_at).max().unwrap_or(0);
    }

    for &page in extra_pages {
        if !cache.fetched_pages.contains(&page) {
            cache.fetched_pages.push(page);
        }
    }

    let mut pos_by_id: std::collections::HashMap<u32, usize> =
        std::collections::HashMap::with_capacity(cache.entries.len());
    for (idx, item) in cache.entries.iter().enumerate() {
        pos_by_id.insert(item.app_id, idx);
    }

    for upd in updates {
        if upd.app_id == 0 || upd.name.is_empty() {
            continue;
        }
        if let Some(&idx) = pos_by_id.get(&upd.app_id) {
            cache.entries[idx] = upd.clone();
        } else {
            pos_by_id.insert(upd.app_id, cache.entries.len());
            cache.entries.push(upd.clone());
        }
    }

    atomic_rebuild_dkcat(dkcat_path, &cache)
}

pub fn load_catalog_via_mmap_standalone() -> CatalogCacheFile {
    let _ = ensure_catalog_mapped_standalone();
    let guard = read_catalog_slot();
    match guard.as_ref() {
        Some(mapped) => mapped.to_catalog_cache_file(),
        None => CatalogCacheFile::default(),
    }
}

pub fn search_catalog_via_mmap(query: &str, limit: usize) -> Option<Vec<CatalogGameEntry>> {
    let _ = ensure_catalog_mapped_standalone();
    let guard = read_catalog_slot();
    guard.as_ref().map(|m| m.search(query, limit))
}

#[tauri::command]
pub async fn get_catalog_mmap_stats() -> Result<CatalogMmapStats, String> {
    let _ = ensure_catalog_mapped_standalone();
    let guard = read_catalog_slot();
    if let Some(mapped) = guard.as_ref() {
        Ok(mapped.stats())
    } else {
        let dkcat_path = get_app_data_file_standalone("catalog.dkcat")?;
        Ok(CatalogMmapStats {
            active: false,
            path: dkcat_path.to_string_lossy().into_owned(),
            ..CatalogMmapStats::default()
        })
    }
}

#[tauri::command]
pub async fn search_catalog_mmap(
    query: String,
    limit: Option<usize>,
) -> Result<Vec<CatalogGameEntry>, String> {
    let cap = limit.unwrap_or(50);
    Ok(search_catalog_via_mmap(&query, cap).unwrap_or_default())
}

#[tauri::command]
pub async fn query_catalog_mmap(params: CatalogQueryParams) -> Result<CatalogQueryPage, String> {
    tokio::task::spawn_blocking(move || {
        let _ = ensure_catalog_mapped_standalone();
        let guard = read_catalog_slot();
        match guard.as_ref() {
            Some(mapped) => mapped.query(&params),
            None => CatalogQueryPage {
                items: Vec::new(),
                total_matches: 0,
                total_indexed: 0,
                fetched_at: 0,
                offset: params.offset,
                limit: if params.limit == 0 {
                    60
                } else {
                    params.limit.clamp(1, 250)
                },
                query_time_us: 0,
                semantic_mode: false,
                semantic_summary: String::new(),
            },
        }
    })
    .await
    .map_err(|e| format!("Catalog mmap query task failed: {}", e))
}

#[tauri::command]
pub async fn get_play_tonight_recommendations(
    app: tauri::AppHandle,
    session_minutes: Option<u32>,
    limit: Option<usize>,
) -> Result<Vec<PlayTonightCandidate>, String> {
    let mins = session_minutes.unwrap_or(60).clamp(15, 480);
    let max_items = limit.unwrap_or(18).clamp(1, 60);
    let library = crate::load_library_from_disk(&app)?;
    let friends_snap = crate::friends::get_unified_friends(app.clone())
        .await
        .ok();

    tokio::task::spawn_blocking(move || {
        let pad = duke_core::win32_ffi::poll_xinput_state();
        let online_pairs: Vec<(String, String)> = friends_snap
            .map(|snap| {
                snap.in_game
                    .into_iter()
                    .filter_map(|f| {
                        let gtitle = f.game_title?;
                        if gtitle.trim().is_empty() {
                            None
                        } else {
                            Some((f.display_name, gtitle))
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(rank_play_tonight_library(
            &library,
            mins,
            pad.connected,
            &online_pairs,
            max_items,
        ))
    })
    .await
    .map_err(|e| format!("Play Tonight ranking task failed: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dkcat_structs_have_zero_unexpected_layout_issues() {
        assert_eq!(core::mem::size_of::<DkCatHeader>(), 72);
        assert_eq!(core::mem::size_of::<CatalogRecordV1>(), 104);
        assert_eq!(core::mem::size_of::<CatalogRecordV2>(), 112);
        assert_eq!(core::mem::size_of::<CatalogRecord>(), 152);
        assert_eq!(core::mem::size_of::<TrigramIndexEntry>(), 12);
    }

    #[test]
    fn dkcat_mmap_roundtrip_and_trigram_search_work() {
        let dir = std::env::temp_dir().join(format!("duke_dkcat_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let dkcat_path = dir.join("catalog.dkcat");

        let cache = CatalogCacheFile {
            fetched_at: 1_700_000_123,
            fetched_pages: vec![0, 1],
            entries: vec![
                CatalogGameEntry {
                    app_id: 1145360,
                    name: "Hades".to_string(),
                    price_cents: 2499,
                    initial_price_cents: 2499,
                    discount_percent: 0,
                    price_formatted: "$24.99".to_string(),
                    is_free: false,
                    ccu: 12000,
                    owners_min: 5_000_000,
                    owners_max: 10_000_000,
                    owners_label: "5M - 10M".to_string(),
                    positive: 240_000,
                    negative: 4_000,
                    review_pct: 98.4,
                    is_singleplayer: true,
                    is_coop: false,
                    is_multiplayer: false,
                    is_online_only_pvp: false,
                    genres: vec!["Action".to_string(), "Indie".to_string()],
                    tags: vec!["Roguelike".to_string(), "Hack and Slash".to_string()],
                    categories: vec!["Single-player".to_string()],
                    short_description: "Defy the god of the dead".to_string(),
                    developers: vec!["Supergiant Games".to_string()],
                    publishers: vec!["Supergiant Games".to_string()],
                    release_date: "Sep 17, 2020".to_string(),
                    cover_url: "https://example.invalid/cover.jpg".to_string(),
                    header_url: "https://example.invalid/header.jpg".to_string(),
                    hero_url: "https://example.invalid/hero.jpg".to_string(),
                    value_score: 97.5,
                    details_enriched: true,
                    updated_at: 1_700_000_000,
                    ..CatalogGameEntry::default()
                },
                CatalogGameEntry {
                    app_id: 413150,
                    name: "Stardew Valley".to_string(),
                    price_cents: 1499,
                    initial_price_cents: 1499,
                    discount_percent: 0,
                    price_formatted: "$14.99".to_string(),
                    is_free: false,
                    ccu: 55000,
                    owners_min: 20_000_000,
                    owners_max: 50_000_000,
                    owners_label: "20M - 50M".to_string(),
                    positive: 600_000,
                    negative: 10_000,
                    review_pct: 98.4,
                    is_singleplayer: true,
                    is_coop: true,
                    is_multiplayer: true,
                    is_online_only_pvp: false,
                    genres: vec!["RPG".to_string(), "Simulation".to_string()],
                    tags: vec!["Farming Sim".to_string(), "Co-op".to_string()],
                    categories: vec!["Single-player".to_string(), "Online Co-op".to_string()],
                    short_description: "You've inherited your grandfather's old farm plot".to_string(),
                    developers: vec!["ConcernedApe".to_string()],
                    publishers: vec!["ConcernedApe".to_string()],
                    release_date: "Feb 26, 2016".to_string(),
                    cover_url: String::new(),
                    header_url: String::new(),
                    hero_url: String::new(),
                    value_score: 99.1,
                    details_enriched: true,
                    updated_at: 1_700_000_001,
                    ..CatalogGameEntry::default()
                },
            ],
        };

        let bytes = serialize_dkcat(&cache).unwrap();
        fs::write(&dkcat_path, bytes).unwrap();

        let mapped = MappedCatalog::open(&dkcat_path).unwrap();
        assert!(mapped.verify_blake3());
        assert_eq!(mapped.record_count(), 2);

        let hades_hits = mapped.search("roguelike", 10);
        assert_eq!(hades_hits.len(), 1);
        assert_eq!(hades_hits[0].app_id, 1145360);
        assert_eq!(hades_hits[0].developers, vec!["Supergiant Games".to_string()]);

        let farm_hits = mapped.search("valley", 10);
        assert_eq!(farm_hits.len(), 1);
        assert_eq!(farm_hits[0].app_id, 413150);
        assert!(farm_hits[0].is_coop);

        let all = mapped.to_catalog_cache_file();
        assert_eq!(all.fetched_pages, vec![0, 1]);
        assert_eq!(all.entries.len(), 2);

        drop(mapped);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dkcat_30000_record_mmap_query_pagination_benchmark() {
        let dir = std::env::temp_dir().join(format!("duke_dkcat_30k_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let dkcat_path = dir.join("catalog.dkcat");

        let genre_pool = [
            "Action",
            "Adventure",
            "RPG",
            "Simulation",
            "Horror",
            "Indie",
            "Racing",
            "Strategy",
        ];
        let mut entries = Vec::with_capacity(30_000);
        for i in 0..30_000u32 {
            let is_free = i % 7 == 0;
            let is_pvp = i % 19 == 0;
            let g1 = genre_pool[(i as usize) % genre_pool.len()].to_string();
            let g2 = genre_pool[((i as usize) + 3) % genre_pool.len()].to_string();
            let name = if i == 12_345 {
                "Hollow Knight Silksong".to_string()
            } else if i == 25_001 {
                "Hollow Knight".to_string()
            } else {
                format!("Duke Preservation Title {}", i)
            };
            entries.push(CatalogGameEntry {
                app_id: 100_000 + i,
                name,
                price_cents: if is_free { 0 } else { 999 + (i % 50) * 100 },
                initial_price_cents: if is_free { 0 } else { 2999 },
                discount_percent: 0,
                price_formatted: if is_free {
                    "Free".to_string()
                } else {
                    "$19.99".to_string()
                },
                is_free,
                ccu: u64::from((i * 17) % 85_000),
                owners_min: 100_000,
                owners_max: 500_000,
                owners_label: "100K - 500K".to_string(),
                positive: 1000 + u64::from(i % 500),
                negative: 50,
                review_pct: 95.0,
                is_singleplayer: i % 5 != 0,
                is_coop: i % 3 == 0,
                is_multiplayer: i % 4 == 0,
                is_online_only_pvp: is_pvp,
                genres: vec![g1, g2],
                tags: if i == 12_345 || i == 25_001 {
                    vec!["Metroidvania".to_string(), "Souls-like".to_string()]
                } else {
                    vec!["Singleplayer".to_string()]
                },
                categories: vec!["Single-player".to_string()],
                short_description: String::new(),
                developers: vec!["Studio".to_string()],
                publishers: vec!["Pub".to_string()],
                release_date: String::new(),
                cover_url: String::new(),
                header_url: String::new(),
                hero_url: String::new(),
                value_score: (i % 10_000) as f64 / 100.0,
                details_enriched: false,
                updated_at: 1_700_000_000,
                ..CatalogGameEntry::default()
            });
        }

        let cache = CatalogCacheFile {
            fetched_at: 1_700_000_555,
            fetched_pages: (0..30).collect(),
            entries,
        };
        let bytes = serialize_dkcat(&cache).unwrap();
        fs::write(&dkcat_path, bytes).unwrap();
        drop(cache);

        let mapped = MappedCatalog::open(&dkcat_path).unwrap();
        assert_eq!(mapped.record_count(), 30_000);

        // 1. Filtered browse page 0 & page 1 over all 30,000 records
        let page0_params = CatalogQueryParams {
            sort_by: "value".to_string(),
            paid_only: true,
            singleplayer: true,
            coop: true,
            exclude_pvp: true,
            genres: vec!["RPG".to_string()],
            min_price_cents: 1000,
            offset: 0,
            limit: 60,
            ..CatalogQueryParams::default()
        };
        let p0 = mapped.query(&page0_params);
        assert_eq!(p0.total_indexed, 30_000);
        assert_eq!(p0.items.len(), 60);
        assert!(p0.total_matches > 100);
        assert!(
            p0.query_time_us < 50_000,
            "30k filtered browse took {} us (expected < 50_000 us)",
            p0.query_time_us
        );

        let page1_params = CatalogQueryParams {
            offset: 60,
            ..page0_params.clone()
        };
        let p1 = mapped.query(&page1_params);
        assert_eq!(p1.items.len(), 60);
        assert_ne!(p0.items[0].app_id, p1.items[0].app_id);

        // 2. Trigram text query over 30,000 records
        let tri_params = CatalogQueryParams {
            query: "hollow knight".to_string(),
            sort_by: "relevancy".to_string(),
            paid_only: true,
            singleplayer: true,
            coop: true,
            exclude_pvp: true,
            offset: 0,
            limit: 60,
            ..CatalogQueryParams::default()
        };
        let tri_page = mapped.query(&tri_params);
        assert_eq!(tri_page.total_matches, 2);
        assert_eq!(tri_page.items[0].name, "Hollow Knight");
        assert_eq!(tri_page.items[1].name, "Hollow Knight Silksong");
        assert!(
            tri_page.query_time_us < 50_000,
            "30k trigram query took {} us (expected < 50_000 us)",
            tri_page.query_time_us
        );

        drop(mapped);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dkcat_36_genre_bitmask_quantitative_filters_and_10_sorts() {
        let dir = std::env::temp_dir().join(format!(
            "duke_dkcat_36g_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let dkcat_path = dir.join("catalog.dkcat");

        let make_entry = |app_id: u32,
                          name: &str,
                          price_cents: u32,
                          ccu: u64,
                          positive: u64,
                          negative: u64,
                          owners_min: u64,
                          value_score: f64,
                          is_sp: bool,
                          is_coop: bool,
                          is_mp: bool,
                          genres: Vec<&str>,
                          tags: Vec<&str>| {
            let total = positive + negative;
            let review_pct = if total > 0 {
                ((positive as f64 / total as f64) * 1000.0).round() / 10.0
            } else {
                0.0
            };
            CatalogGameEntry {
                app_id,
                name: name.to_string(),
                price_cents,
                initial_price_cents: price_cents,
                discount_percent: 0,
                price_formatted: if price_cents == 0 {
                    "Free".to_string()
                } else {
                    format!("${:.2}", price_cents as f64 / 100.0)
                },
                is_free: price_cents == 0,
                ccu,
                owners_min,
                owners_max: owners_min * 2,
                owners_label: "100K - 200K".to_string(),
                positive,
                negative,
                review_pct,
                is_singleplayer: is_sp,
                is_coop,
                is_multiplayer: is_mp,
                is_online_only_pvp: false,
                genres: genres.into_iter().map(String::from).collect(),
                tags: tags.into_iter().map(String::from).collect(),
                categories: Vec::new(),
                short_description: String::new(),
                developers: vec!["Duke Studio".to_string()],
                publishers: vec!["Duke Pub".to_string()],
                release_date: String::new(),
                cover_url: String::new(),
                header_url: String::new(),
                hero_url: String::new(),
                value_score,
                details_enriched: true,
                updated_at: 1_700_000_000,
                ..CatalogGameEntry::default()
            }
        };

        let entries = vec![
            // Engineering + Sandbox + Singleplayer (via synonyms Space Sim & Moddable), Hidden Gem (95%+, low CCU)
            make_entry(
                220200,
                "Orbital Rocket Lab",
                1999,
                420,
                1900,
                100,
                200_000,
                91.2,
                true,
                false,
                false,
                vec!["Simulation", "Indie"],
                vec!["Space Sim", "Moddable", "Physics"],
            ),
            // Engineering + Automation + Co-op, Mega Hit (98%+, high CCU & reviews)
            make_entry(
                427520,
                "Factory Blueprint",
                3499,
                45000,
                196_000,
                4_000,
                5_000_000,
                98.5,
                true,
                true,
                true,
                vec!["Strategy", "Simulation"],
                vec!["Automation", "Base Building", "Logistics"],
            ),
            // Sandbox + Survival + Crafting, Free, Mixed Rating (72%)
            make_entry(
                1500000,
                "Alpha Voxel World",
                0,
                1200,
                720,
                280,
                500_000,
                42.0,
                true,
                true,
                true,
                vec!["Adventure"],
                vec!["Open World Survival Craft", "Sandbox"],
            ),
            // Souls-like + Metroidvania + Action, Under $10, Newest AppID, Trending
            make_entry(
                3100000,
                "Zeta Hollow Blade",
                899,
                8500,
                1350,
                150,
                200_000,
                88.0,
                true,
                false,
                false,
                vec!["Action"],
                vec!["Souls-like", "Metroidvania", "Roguelike"],
            ),
        ];

        assert_eq!(GENRE_BIT_DEFS.len(), 36);
        let cache = CatalogCacheFile {
            fetched_at: 1_700_000_100,
            fetched_pages: vec![0],
            entries,
        };
        let bytes = serialize_dkcat(&cache).unwrap();
        fs::write(&dkcat_path, bytes).unwrap();
        let mapped = MappedCatalog::open(&dkcat_path).unwrap();

        // 1. Multi-genre bitwise intersection: Engineering + Sandbox + Singleplayer
        let eng_sb = mapped.query(&CatalogQueryParams {
            genres: vec!["Engineering".to_string(), "Sandbox".to_string()],
            player_mode: "singleplayer".to_string(),
            sort_by: "value_score".to_string(),
            limit: 60,
            ..CatalogQueryParams::default()
        });
        assert_eq!(eng_sb.total_matches, 1);
        assert_eq!(eng_sb.items[0].app_id, 220200);

        // 2. Engineering single tag matches both Orbital Rocket Lab (Space Sim) and Factory Blueprint (Automation + Base Building)
        let eng_only = mapped.query(&CatalogQueryParams {
            genres: vec!["Engineering".to_string()],
            limit: 60,
            ..CatalogQueryParams::default()
        });
        assert_eq!(eng_only.total_matches, 2);

        // 3. Hidden Gems puts Orbital Rocket Lab (95%+, 2000 reviews, CCU 420) ahead of mega-hit Factory Blueprint (CCU 45000) and <88% Alpha Voxel World
        let gems = mapped.query(&CatalogQueryParams {
            sort_by: "hidden_gems".to_string(),
            limit: 60,
            ..CatalogQueryParams::default()
        });
        assert_eq!(gems.items[0].app_id, 220200);

        // 4. Review Score (% Positive) puts Factory Blueprint (98.0%) first
        let by_rating = mapped.query(&CatalogQueryParams {
            sort_by: "rating_desc".to_string(),
            limit: 60,
            ..CatalogQueryParams::default()
        });
        assert_eq!(by_rating.items[0].app_id, 427520);
        assert_eq!(by_rating.items.last().unwrap().app_id, 1500000);

        // 5. Title A->Z puts Alpha Voxel World first and Zeta Hollow Blade last
        let by_title = mapped.query(&CatalogQueryParams {
            sort_by: "title_asc".to_string(),
            limit: 60,
            ..CatalogQueryParams::default()
        });
        assert_eq!(by_title.items[0].name, "Alpha Voxel World");
        assert_eq!(by_title.items.last().unwrap().name, "Zeta Hollow Blade");

        // 6. Price Asc puts Free first, then $8.99; Price Desc puts $34.99 first
        let by_price_asc = mapped.query(&CatalogQueryParams {
            sort_by: "price_asc".to_string(),
            limit: 60,
            ..CatalogQueryParams::default()
        });
        assert_eq!(by_price_asc.items[0].app_id, 1500000);
        assert_eq!(by_price_asc.items[1].app_id, 3100000);

        let by_price_desc = mapped.query(&CatalogQueryParams {
            sort_by: "price_desc".to_string(),
            limit: 60,
            ..CatalogQueryParams::default()
        });
        assert_eq!(by_price_desc.items[0].app_id, 427520);

        // 7. Newest AppID puts 3100000 first
        let newest = mapped.query(&CatalogQueryParams {
            sort_by: "newest_appid".to_string(),
            limit: 60,
            ..CatalogQueryParams::default()
        });
        assert_eq!(newest.items[0].app_id, 3100000);

        // 8. Quantitative filters: under_10, free, over_30, min_rating_pct=95, min_reviews=10000, min_ccu=5000, hide_installed
        assert_eq!(
            mapped
                .query(&CatalogQueryParams {
                    price_tier: "under_10".to_string(),
                    ..CatalogQueryParams::default()
                })
                .items
                .into_iter()
                .map(|e| e.app_id)
                .collect::<Vec<_>>(),
            vec![3100000]
        );
        assert_eq!(
            mapped
                .query(&CatalogQueryParams {
                    price_tier: "free".to_string(),
                    ..CatalogQueryParams::default()
                })
                .items
                .into_iter()
                .map(|e| e.app_id)
                .collect::<Vec<_>>(),
            vec![1500000]
        );
        assert_eq!(
            mapped
                .query(&CatalogQueryParams {
                    price_tier: "over_30".to_string(),
                    ..CatalogQueryParams::default()
                })
                .items
                .into_iter()
                .map(|e| e.app_id)
                .collect::<Vec<_>>(),
            vec![427520]
        );
        assert_eq!(
            mapped
                .query(&CatalogQueryParams {
                    min_rating_pct: 95,
                    min_reviews: 10_000,
                    min_ccu: 5_000,
                    ..CatalogQueryParams::default()
                })
                .total_matches,
            1
        );
        assert_eq!(
            mapped
                .query(&CatalogQueryParams {
                    hide_installed: true,
                    installed_app_ids: vec![427520],
                    ..CatalogQueryParams::default()
                })
                .total_matches,
            3
        );

        drop(mapped);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dkcat002_filters_retro_platforms_and_migrates_v1_automatically() {
        let dir = std::env::temp_dir().join(format!("duke_dkcat_v2_plat_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let dkcat_path = dir.join("catalog.dkcat");

        let mut cache = CatalogCacheFile {
            fetched_at: 1_700_000_555,
            fetched_pages: vec![0, 1],
            entries: vec![
                CatalogGameEntry {
                    app_id: 730,
                    name: "Counter-Strike 2".to_string(),
                    platform_id: crate::platforms::PLATFORM_PC,
                    console_group: crate::platforms::GROUP_PC,
                    platform_name: "PC".to_string(),
                    platform_slug: "pc".to_string(),
                    is_free: true,
                    price_formatted: "Free".to_string(),
                    value_score: 90.0,
                    ..CatalogGameEntry::default()
                },
                CatalogGameEntry {
                    app_id: crate::platforms::synthetic_retro_catalog_id(
                        crate::platforms::PLATFORM_GAMECUBE,
                        "Metroid Prime (USA)",
                        "GM8E01",
                    ),
                    name: "Metroid Prime (USA)".to_string(),
                    platform_id: crate::platforms::PLATFORM_GAMECUBE,
                    console_group: crate::platforms::GROUP_GC_WII,
                    platform_name: "GameCube".to_string(),
                    platform_slug: "gamecube".to_string(),
                    genres: vec!["Action".to_string(), "Adventure".to_string()],
                    tags: vec!["Nintendo".to_string(), "GM8E01".to_string()],
                    value_score: 96.0,
                    ..CatalogGameEntry::default()
                },
                CatalogGameEntry {
                    app_id: crate::platforms::synthetic_retro_catalog_id(
                        crate::platforms::PLATFORM_3DS,
                        "The Legend of Zelda: Ocarina of Time 3D",
                        "AQEE",
                    ),
                    name: "The Legend of Zelda: Ocarina of Time 3D".to_string(),
                    platform_id: crate::platforms::PLATFORM_3DS,
                    console_group: crate::platforms::GROUP_3DS,
                    platform_name: "Nintendo 3DS".to_string(),
                    platform_slug: "3ds".to_string(),
                    genres: vec!["Adventure".to_string()],
                    tags: vec!["Nintendo".to_string(), "3DS".to_string()],
                    value_score: 97.0,
                    ..CatalogGameEntry::default()
                },
                CatalogGameEntry {
                    app_id: crate::platforms::synthetic_retro_catalog_id(
                        crate::platforms::PLATFORM_PS2,
                        "Shadow of the Colossus (USA)",
                        "SCUS-97472",
                    ),
                    name: "Shadow of the Colossus (USA)".to_string(),
                    platform_id: crate::platforms::PLATFORM_PS2,
                    console_group: crate::platforms::GROUP_PLAYSTATION,
                    platform_name: "PS2".to_string(),
                    platform_slug: "ps2".to_string(),
                    genres: vec!["Adventure".to_string()],
                    tags: vec!["PS2".to_string(), "SCUS-97472".to_string()],
                    value_score: 95.0,
                    ..CatalogGameEntry::default()
                },
            ],
        };

        let bytes = serialize_dkcat(&cache).unwrap();
        assert_eq!(&bytes[0..8], b"DKCAT003");
        fs::write(&dkcat_path, &bytes).unwrap();
        let mapped = MappedCatalog::open(&dkcat_path).unwrap();

        // Filter by explicit platform_id
        let gc = mapped.query(&CatalogQueryParams {
            platform_id: Some(crate::platforms::PLATFORM_GAMECUBE),
            ..CatalogQueryParams::default()
        });
        assert_eq!(gc.total_matches, 1);
        assert_eq!(gc.items[0].platform_id, crate::platforms::PLATFORM_GAMECUBE);
        assert_eq!(gc.items[0].platform_name, "GameCube");

        // Filter by Nintendo umbrella console_group (matches GC + 3DS)
        let nintendo = mapped.query(&CatalogQueryParams {
            platform: "nintendo".to_string(),
            ..CatalogQueryParams::default()
        });
        assert_eq!(nintendo.total_matches, 2);

        // Filter by PlayStation group + query
        let ps = mapped.query(&CatalogQueryParams {
            query: "Colossus".to_string(),
            platform: "playstation".to_string(),
            ..CatalogQueryParams::default()
        });
        assert_eq!(ps.total_matches, 1);
        assert_eq!(ps.items[0].platform_slug, "ps2");

        // Verify DKCAT001 -> DKCAT002 transparent migration
        drop(mapped);
        cache.entries.truncate(1);
        let rec_v1 = CatalogRecordV1 {
            ccu: 900,
            owners_min: 1000,
            owners_max: 2000,
            positive: 500,
            negative: 10,
            value_score: 88.0,
            updated_at: 1_700_000_111,
            genre_bits: 0,
            appid: 730,
            price_cents: 0,
            initial_price_cents: 0,
            flags: FLAG_IS_FREE,
            name_off: 0,
            name_len: 16,
            tags_off: 16,
            tags_len: 0,
            meta_off: 16,
            meta_len: 0,
        };
        let hdr_sz = core::mem::size_of::<DkCatHeader>();
        let rec_v1_sz = core::mem::size_of::<CatalogRecordV1>();
        let mut body_v1 = Vec::new();
        body_v1.extend_from_slice(bytes_of_struct(&rec_v1));
        body_v1.extend_from_slice(b"Counter-Strike 2");
        let tri_off_v1 = (hdr_sz + body_v1.len()) as u64;
        body_v1.extend_from_slice(&1_700_000_555u64.to_le_bytes()); // fetched_at
        body_v1.extend_from_slice(&0u32.to_le_bytes()); // trigram_count
        body_v1.extend_from_slice(&1u32.to_le_bytes()); // pages_count
        body_v1.extend_from_slice(&0u32.to_le_bytes()); // page 0
        let hdr_v1 = DkCatHeader {
            magic: DKCAT_MAGIC_V1,
            version: 2,
            record_count: 1,
            records_offset: hdr_sz as u64,
            strings_offset: (hdr_sz + rec_v1_sz) as u64,
            trigram_offset: tri_off_v1,
            checksum_blake3: *blake3::hash(&body_v1).as_bytes(),
        };
        let mut raw_v1 = Vec::with_capacity(hdr_sz + body_v1.len());
        raw_v1.extend_from_slice(bytes_of_struct(&hdr_v1));
        raw_v1.extend_from_slice(&body_v1);
        let v1_path = dir.join("legacy_v1.dkcat");
        fs::write(&v1_path, &raw_v1).unwrap();

        let migrated = try_migrate_dkcat_v1_to_v2(&v1_path).unwrap();
        assert!(migrated);
        let reopened = MappedCatalog::open(&v1_path).unwrap();
        assert_eq!(reopened.record_count(), 1);
        assert_eq!(reopened.get_entry_by_appid(730).unwrap().name, "Counter-Strike 2");
        assert_eq!(reopened.fetched_pages(), &[0]);

        drop(reopened);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn mapped_catalog_handles_legacy_v1_truncated_and_corrupt_files_without_panic() {
        let dir = std::env::temp_dir().join(format!("duke_dkcat_corrupt_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        // 1. Truncated (< HEADER_LEN) file must return Err cleanly and recover to valid empty DKCAT002.
        let trunc_path = dir.join("truncated.dkcat");
        fs::write(&trunc_path, b"DKCAT002short").unwrap();
        assert!(MappedCatalog::open(&trunc_path).is_err());
        open_or_rebuild_dkcat_at(&trunc_path, None).unwrap();
        let rebuilt_trunc = MappedCatalog::open(&trunc_path).unwrap();
        assert_eq!(rebuilt_trunc.record_count(), 0);
        drop(rebuilt_trunc);

        // 2. Corrupt DKCAT002 with integer-overflow trigram_offset (u64::MAX) and misaligned stride must not panic.
        let hdr_sz = core::mem::size_of::<DkCatHeader>();
        let overflow_hdr = DkCatHeader {
            magic: DKCAT_MAGIC,
            version: DKCAT_VERSION,
            record_count: 1,
            records_offset: hdr_sz as u64,
            strings_offset: (hdr_sz + 1) as u64,
            trigram_offset: u64::MAX,
            checksum_blake3: [0u8; 32],
        };
        let mut corrupt_bytes = Vec::with_capacity(hdr_sz + 32);
        corrupt_bytes.extend_from_slice(bytes_of_struct(&overflow_hdr));
        corrupt_bytes.extend_from_slice(&[0u8; 32]);
        let corrupt_path = dir.join("corrupt_overflow.dkcat");
        fs::write(&corrupt_path, &corrupt_bytes).unwrap();
        assert!(MappedCatalog::open(&corrupt_path).is_err());
        open_or_rebuild_dkcat_at(&corrupt_path, None).unwrap();
        let rebuilt_corrupt = MappedCatalog::open(&corrupt_path).unwrap();
        assert_eq!(rebuilt_corrupt.record_count(), 0);
        drop(rebuilt_corrupt);

        // 3. Legacy DKCAT001 must fail MappedCatalog::open cleanly (zero panic), and recover via migration.
        let rec_v1 = CatalogRecordV1 {
            ccu: 100,
            owners_min: 100,
            owners_max: 200,
            positive: 90,
            negative: 10,
            value_score: 75.0,
            updated_at: 1_700_000_000,
            genre_bits: 0,
            appid: 440,
            price_cents: 0,
            initial_price_cents: 0,
            flags: FLAG_IS_FREE,
            name_off: 0,
            name_len: 15,
            tags_off: 15,
            tags_len: 0,
            meta_off: 15,
            meta_len: 0,
        };
        let rec_v1_sz = core::mem::size_of::<CatalogRecordV1>();
        let mut body_v1 = Vec::new();
        body_v1.extend_from_slice(bytes_of_struct(&rec_v1));
        body_v1.extend_from_slice(b"Team Fortress 2");
        let tri_off_v1 = (hdr_sz + body_v1.len()) as u64;
        body_v1.extend_from_slice(&1_700_000_123u64.to_le_bytes());
        body_v1.extend_from_slice(&0u32.to_le_bytes());
        body_v1.extend_from_slice(&0u32.to_le_bytes());
        let hdr_v1 = DkCatHeader {
            magic: DKCAT_MAGIC_V1,
            version: 2,
            record_count: 1,
            records_offset: hdr_sz as u64,
            strings_offset: (hdr_sz + rec_v1_sz) as u64,
            trigram_offset: tri_off_v1,
            checksum_blake3: *blake3::hash(&body_v1).as_bytes(),
        };
        let mut raw_v1 = Vec::with_capacity(hdr_sz + body_v1.len());
        raw_v1.extend_from_slice(bytes_of_struct(&hdr_v1));
        raw_v1.extend_from_slice(&body_v1);
        let v1_path = dir.join("legacy_recover.dkcat");
        fs::write(&v1_path, &raw_v1).unwrap();

        assert!(MappedCatalog::open(&v1_path).is_err());
        open_or_rebuild_dkcat_at(&v1_path, None).unwrap();
        let migrated = MappedCatalog::open(&v1_path).unwrap();
        assert_eq!(migrated.record_count(), 1);
        assert_eq!(migrated.get_entry_by_appid(440).unwrap().name, "Team Fortress 2");
        drop(migrated);

        // 4. Truncated/corrupt DKCAT001 must not panic and must delete + rebuild cleanly.
        let mut bad_v1 = raw_v1[..hdr_sz + 12].to_vec();
        let bad_v1_path = dir.join("corrupt_v1.dkcat");
        fs::write(&bad_v1_path, &bad_v1).unwrap();
        assert!(MappedCatalog::open(&bad_v1_path).is_err());
        open_or_rebuild_dkcat_at(&bad_v1_path, None).unwrap();
        let rebuilt_v1 = MappedCatalog::open(&bad_v1_path).unwrap();
        assert_eq!(rebuilt_v1.record_count(), 0);
        drop(rebuilt_v1);
        bad_v1.clear();

        *write_catalog_slot() = None;
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dkcat003_quantized_semantic_nl_and_v2_migration_work() {
        let dir = std::env::temp_dir().join(format!("duke_dkcat003_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let dkcat_path = dir.join("catalog.dkcat");

        let cache = CatalogCacheFile {
            fetched_at: 1_700_000_555,
            fetched_pages: vec![0],
            entries: vec![
                CatalogGameEntry {
                    app_id: 477160,
                    name: "Human Fall Flat".to_string(),
                    is_coop: true,
                    genres: vec!["Indie".to_string(), "Physics".to_string(), "Casual".to_string()],
                    tags: vec!["Co-op".to_string(), "Physics".to_string(), "Chaos".to_string(), "Funny".to_string()],
                    categories: vec!["Full controller support".to_string(), "Co-op".to_string()],
                    short_description: "Hilarious physics chaos co-op platformer under 2 GB.".to_string(),
                    positive: 150_000,
                    negative: 7_000,
                    review_pct: 95.5,
                    value_score: 94.0,
                    ..CatalogGameEntry::default()
                },
                CatalogGameEntry {
                    app_id: crate::platforms::synthetic_retro_catalog_id(
                        crate::platforms::PLATFORM_SWITCH,
                        "Golf Story Switch RPG",
                        "0100",
                    ),
                    name: "Golf Story".to_string(),
                    platform_id: crate::platforms::PLATFORM_SWITCH,
                    console_group: crate::platforms::GROUP_SWITCH,
                    platform_name: "Switch".to_string(),
                    platform_slug: "switch".to_string(),
                    genres: vec!["RPG".to_string(), "Indie".to_string()],
                    tags: vec!["Switch".to_string(), "RPG".to_string(), "Short".to_string()],
                    categories: vec!["Full controller support".to_string()],
                    short_description: "Charming 6-hour Switch RPG adventure with gamepad support.".to_string(),
                    positive: 500,
                    negative: 15,
                    review_pct: 97.1,
                    value_score: 96.0,
                    ..CatalogGameEntry::default()
                },
            ],
        };

        let bytes = serialize_dkcat(&cache).unwrap();
        assert_eq!(&bytes[0..8], b"DKCAT003");
        fs::write(&dkcat_path, &bytes).unwrap();
        let mapped = MappedCatalog::open(&dkcat_path).unwrap();

        let q1 = mapped.query(&CatalogQueryParams {
            query: "co-op physics chaos games under 5 GB".to_string(),
            ..CatalogQueryParams::default()
        });
        assert!(q1.semantic_mode);
        assert_eq!(q1.total_matches, 1);
        assert_eq!(q1.items[0].name, "Human Fall Flat");

        let q2 = mapped.query(&CatalogQueryParams {
            query: "short 6-hour Switch RPG with controller support".to_string(),
            ..CatalogQueryParams::default()
        });
        assert!(q2.semantic_mode);
        assert_eq!(q2.total_matches, 1);
        assert_eq!(q2.items[0].name, "Golf Story");

        drop(mapped);

        // Verify DKCAT002 (112B) -> DKCAT003 automatic upgrade
        let v2_path = dir.join("legacy_v2.dkcat");
        let rec_v2 = CatalogRecordV2 {
            ccu: 50,
            owners_min: 1000,
            owners_max: 5000,
            positive: 400,
            negative: 20,
            value_score: 88.0,
            updated_at: 1_700_000_000,
            genre_bits: 0,
            appid: 999999,
            price_cents: 999,
            initial_price_cents: 999,
            flags: FLAG_COOP,
            name_off: 0,
            name_len: 10,
            tags_off: 10,
            tags_len: 0,
            meta_off: 10,
            meta_len: 0,
            platform_id: crate::platforms::PLATFORM_SWITCH,
            console_group: crate::platforms::GROUP_SWITCH,
            _pad: [0u8; 5],
        };
        let rec_v2_sz = core::mem::size_of::<CatalogRecordV2>();
        let hdr_sz = core::mem::size_of::<DkCatHeader>();
        let mut body_v2 = Vec::new();
        body_v2.extend_from_slice(bytes_of_struct(&rec_v2));
        body_v2.extend_from_slice(b"Peak Co-op");
        let tri_off_v2 = (hdr_sz + body_v2.len()) as u64;
        body_v2.extend_from_slice(&1_700_000_000u64.to_le_bytes());
        body_v2.extend_from_slice(&0u32.to_le_bytes());
        body_v2.extend_from_slice(&0u32.to_le_bytes());
        let hdr_v2 = DkCatHeader {
            magic: DKCAT_MAGIC_V2,
            version: 3,
            record_count: 1,
            records_offset: hdr_sz as u64,
            strings_offset: (hdr_sz + rec_v2_sz) as u64,
            trigram_offset: tri_off_v2,
            checksum_blake3: *blake3::hash(&body_v2).as_bytes(),
        };
        let mut raw_v2 = Vec::with_capacity(hdr_sz + body_v2.len());
        raw_v2.extend_from_slice(bytes_of_struct(&hdr_v2));
        raw_v2.extend_from_slice(&body_v2);
        fs::write(&v2_path, &raw_v2).unwrap();

        assert!(try_migrate_dkcat_legacy_to_v3(&v2_path).unwrap());
        let upgraded = MappedCatalog::open(&v2_path).unwrap();
        assert_eq!(upgraded.record_count(), 1);
        assert_eq!(upgraded.get_entry_by_appid(999999).unwrap().name, "Peak Co-op");

        // Verify Play Tonight ranking prioritizes active online friend matches
        let lib = vec![
            crate::GameEntry {
                id: "g1".to_string(),
                title: "Balatro".to_string(),
                genre: "Roguelike".to_string(),
                playtime_seconds: 1800,
                ..Default::default()
            },
            crate::GameEntry {
                id: "g2".to_string(),
                title: "Deep Rock Galactic".to_string(),
                genre: "Co-op Shooter".to_string(),
                playtime_seconds: 7200,
                ..Default::default()
            },
        ];
        let ranked = rank_play_tonight_library(
            &lib,
            45,
            true,
            &[("Alex".to_string(), "Deep Rock Galactic".to_string())],
            5,
        );
        assert_eq!(ranked[0].title, "Deep Rock Galactic");
        assert_eq!(ranked[0].matched_online_friends, vec!["Alex".to_string()]);

        let _ = fs::remove_dir_all(&dir);
    }
}

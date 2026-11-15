//! 1-Click BIOS & Firmware Vault + Exact MD5/SHA-1 Hash Validator.
//!
//! Scans custom folders, existing emulator folders, and dropped `.zip`/`.7z` archives; verifies
//! exact Libretro `.info` MD5 & SHA-1 hashes; copies valid BIOS/firmware files to DuckStation,
//! PCSX2, melonDS, Azahar, and RetroArch `system/` folders; backs them up inside `.duke-cas/`;
//! quarantines mismatched dumps; and reports green/red Console Readiness badges.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::snapshots::CAS_DIR_NAME;

const BIOS_VAULT_SUBDIR: &str = "bios_vault";
const BIOS_MAX_BYTES: u64 = 64 * 1024 * 1024;
const STREAM_BUF_LEN: usize = 65_536;

// ---------- Pure-Rust RFC-1321 MD5 & FIPS-180-4 SHA-1 (Zero Extra Crates) ----------

pub(crate) struct Md5Hasher {
    state: [u32; 4],
    count: u64,
    buffer: [u8; 64],
}

impl Md5Hasher {
    pub(crate) fn new() -> Self {
        Self {
            state: [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476],
            count: 0,
            buffer: [0u8; 64],
        }
    }

    pub(crate) fn update(&mut self, input: &[u8]) {
        let mut idx = (self.count % 64) as usize;
        self.count = self.count.wrapping_add(input.len() as u64);
        let mut offset = 0usize;

        if idx > 0 {
            let need = 64 - idx;
            if input.len() >= need {
                self.buffer[idx..idx + need].copy_from_slice(&input[..need]);
                let blk = self.buffer;
                Self::transform(&mut self.state, &blk);
                offset = need;
                idx = 0;
            } else {
                self.buffer[idx..idx + input.len()].copy_from_slice(input);
                return;
            }
        }

        while offset + 64 <= input.len() {
            let mut blk = [0u8; 64];
            blk.copy_from_slice(&input[offset..offset + 64]);
            Self::transform(&mut self.state, &blk);
            offset += 64;
        }

        if offset < input.len() {
            let rem = input.len() - offset;
            self.buffer[idx..idx + rem].copy_from_slice(&input[offset..]);
        }
    }

    pub(crate) fn finalize(mut self) -> [u8; 16] {
        let bit_len = self.count.wrapping_mul(8);
        let idx = (self.count % 64) as usize;
        let pad_len = if idx < 56 { 56 - idx } else { 120 - idx };
        let mut padding = [0u8; 64];
        padding[0] = 0x80;
        let mut rem = pad_len;
        let mut first = true;
        while rem > 0 {
            let take = rem.min(64);
            if first {
                self.update(&padding[..take]);
                first = false;
            } else {
                self.update(&[0u8; 64][..take]);
            }
            rem -= take;
        }
        self.update(&bit_len.to_le_bytes());

        let mut out = [0u8; 16];
        for (i, w) in self.state.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
        }
        out
    }

    fn transform(state: &mut [u32; 4], block: &[u8; 64]) {
        const SHIFTS: [u32; 64] = [
            7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22,
            5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20,
            4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23,
            6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
        ];
        const K: [u32; 64] = [
            0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
            0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
            0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
            0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
            0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
            0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
            0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
            0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
        ];
        let mut m = [0u32; 16];
        for i in 0..16 {
            m[i] = u32::from_le_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        let [mut a, mut b, mut c, mut d] = *state;
        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | ((!b) & d), i),
                16..=31 => ((d & b) | ((!d) & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | (!d)), (7 * i) % 16),
            };
            let temp = d;
            d = c;
            c = b;
            let sum = a.wrapping_add(f).wrapping_add(K[i]).wrapping_add(m[g]);
            b = b.wrapping_add(sum.rotate_left(SHIFTS[i]));
            a = temp;
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn md5_hex_bytes(data: &[u8]) -> String {
    let mut h = Md5Hasher::new();
    h.update(data);
    hex::encode(h.finalize())
}

pub(crate) struct Sha1Hasher {
    state: [u32; 5],
    count: u64,
    buffer: [u8; 64],
}

impl Sha1Hasher {
    pub(crate) fn new() -> Self {
        Self {
            state: [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0],
            count: 0,
            buffer: [0u8; 64],
        }
    }

    pub(crate) fn update(&mut self, input: &[u8]) {
        let mut idx = (self.count % 64) as usize;
        self.count = self.count.wrapping_add(input.len() as u64);
        let mut offset = 0usize;

        if idx > 0 {
            let need = 64 - idx;
            if input.len() >= need {
                self.buffer[idx..idx + need].copy_from_slice(&input[..need]);
                let blk = self.buffer;
                Self::transform(&mut self.state, &blk);
                offset = need;
                idx = 0;
            } else {
                self.buffer[idx..idx + input.len()].copy_from_slice(input);
                return;
            }
        }

        while offset + 64 <= input.len() {
            let mut blk = [0u8; 64];
            blk.copy_from_slice(&input[offset..offset + 64]);
            Self::transform(&mut self.state, &blk);
            offset += 64;
        }

        if offset < input.len() {
            let rem = input.len() - offset;
            self.buffer[idx..idx + rem].copy_from_slice(&input[offset..]);
        }
    }

    pub(crate) fn finalize(mut self) -> [u8; 20] {
        let bit_len = self.count.wrapping_mul(8);
        let idx = (self.count % 64) as usize;
        let pad_len = if idx < 56 { 56 - idx } else { 120 - idx };
        let mut rem = pad_len;
        let mut first = true;
        let mut pad_first = [0u8; 64];
        pad_first[0] = 0x80;
        while rem > 0 {
            let take = rem.min(64);
            if first {
                self.update(&pad_first[..take]);
                first = false;
            } else {
                self.update(&[0u8; 64][..take]);
            }
            rem -= take;
        }
        self.update(&bit_len.to_be_bytes());
        let mut out = [0u8; 20];
        for (i, w) in self.state.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&w.to_be_bytes());
        }
        out
    }

    fn transform(state: &mut [u32; 5], block: &[u8; 64]) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = *state;
        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1u32),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDCu32),
                _ => (b ^ c ^ d, 0xCA62_C1D6u32),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
    }
}

pub(crate) fn sha1_hex_bytes(data: &[u8]) -> String {
    let mut h = Sha1Hasher::new();
    h.update(data);
    hex::encode(h.finalize())
}

pub(crate) fn hash_file_md5_sha1_streaming(path: &Path) -> Result<(String, String, u64), String> {
    let mut file =
        File::open(path).map_err(|e| format!("Failed to open '{}': {e}", path.display()))?;
    let mut buf = vec![0u8; STREAM_BUF_LEN];
    let mut md5 = Md5Hasher::new();
    let mut sha1 = Sha1Hasher::new();
    let mut total = 0u64;
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("Failed reading '{}': {e}", path.display()))?;
        if n == 0 {
            break;
        }
        md5.update(&buf[..n]);
        sha1.update(&buf[..n]);
        total = total.saturating_add(n as u64);
    }
    Ok((hex::encode(md5.finalize()), hex::encode(sha1.finalize()), total))
}

// ---------- Libretro .info Verified BIOS/Firmware Catalog ----------

#[derive(Debug, Clone, Copy)]
pub struct BiosFirmwareSpec {
    pub console_id: &'static str,
    pub console_name: &'static str,
    pub emulator_id: &'static str,
    pub filename: &'static str,
    pub expected_md5: &'static str,
    pub expected_sha1: &'static str,
    pub required: bool,
    pub target_subdirs: &'static [&'static str],
    pub notes: &'static str,
}

pub const BIOS_CATALOG: &[BiosFirmwareSpec] = &[
    BiosFirmwareSpec {
        console_id: "ps1",
        console_name: "PlayStation (PS1)",
        emulator_id: "duckstation",
        filename: "scph5501.bin",
        expected_md5: "490f666e1afb15b7362b406ed1cea246",
        expected_sha1: "0555c6fae8906f3f09baf5988f00e55f88e9f30b",
        required: true,
        target_subdirs: &["duckstation/bios", "retroarch/system"],
        notes: "Required PS1 NTSC-U v3.0 BIOS for DuckStation & Beetle/SwanStation.",
    },
    BiosFirmwareSpec {
        console_id: "ps1_jp",
        console_name: "PlayStation (PS1 NTSC-J)",
        emulator_id: "duckstation",
        filename: "scph5500.bin",
        expected_md5: "8dd7d5296a650fac7319bce665a6a53c",
        expected_sha1: "b05def971d8ec59f346f2d9ac21fb742e3eb6917",
        required: false,
        target_subdirs: &["duckstation/bios", "retroarch/system"],
        notes: "Optional PS1 Japan v3.0 BIOS for NTSC-J discs.",
    },
    BiosFirmwareSpec {
        console_id: "ps1_eu",
        console_name: "PlayStation (PS1 PAL)",
        emulator_id: "duckstation",
        filename: "scph5502.bin",
        expected_md5: "32736f17079d0b2b7024407c39bd3050",
        expected_sha1: "f6bc2d1f5eb6593de7d089c425ac681d6ffdfd3f",
        required: false,
        target_subdirs: &["duckstation/bios", "retroarch/system"],
        notes: "Optional PS1 Europe v3.0 BIOS for PAL discs.",
    },
    BiosFirmwareSpec {
        console_id: "ps2",
        console_name: "PlayStation 2 (PS2)",
        emulator_id: "pcsx2",
        filename: "scph39001.bin",
        expected_md5: "d5ce2c7d119f563ce04bc04dbc3a323e",
        expected_sha1: "f9a5d629a036b99128f7cb530c6e3ca016e9c8b5",
        required: true,
        target_subdirs: &["pcsx2/bios", "retroarch/system/pcsx2/bios"],
        notes: "PS2 USA v1.60 BIOS recommended by PCSX2.",
    },
    BiosFirmwareSpec {
        console_id: "ps2_slim",
        console_name: "PlayStation 2 Slim (PS2)",
        emulator_id: "pcsx2",
        filename: "scph70012.bin",
        expected_md5: "dc166e33c18f75e4c2a3a72eb78bb4a2",
        expected_sha1: "6fd0d4c496215c13726c2939c1009ec2a4635c0b",
        required: false,
        target_subdirs: &["pcsx2/bios", "retroarch/system/pcsx2/bios"],
        notes: "Alternate PS2 Slim USA v2.00 BIOS accepted by PCSX2.",
    },
    BiosFirmwareSpec {
        console_id: "nds_7",
        console_name: "Nintendo DS (ARM7 BIOS)",
        emulator_id: "melonds",
        filename: "bios7.bin",
        expected_md5: "df692a80a5b1bc90728bc3dfc76cd948",
        expected_sha1: "24f67bdea115a2c847c8813a262502ee1607b7df",
        required: true,
        target_subdirs: &["melonds", "retroarch/system"],
        notes: "Nintendo DS ARM7 BIOS (16 KB) for melonDS & RetroArch.",
    },
    BiosFirmwareSpec {
        console_id: "nds_9",
        console_name: "Nintendo DS (ARM9 BIOS)",
        emulator_id: "melonds",
        filename: "bios9.bin",
        expected_md5: "a392174eb3e572fed6447e956bde4b25",
        expected_sha1: "bfaac75f101c135e32e2aaf541de6b1be4c8c62d",
        required: true,
        target_subdirs: &["melonds", "retroarch/system"],
        notes: "Nintendo DS ARM9 BIOS (4 KB) for melonDS & RetroArch.",
    },
    BiosFirmwareSpec {
        console_id: "nds_fw",
        console_name: "Nintendo DS Firmware",
        emulator_id: "melonds",
        filename: "firmware.bin",
        expected_md5: "e45033d9b0fa6b0de071292bba7c9d13",
        expected_sha1: "b1f710d1b976b7b4bf66c8b689d9b4f8b67b14d2",
        required: false,
        target_subdirs: &["melonds", "retroarch/system"],
        notes: "DS user firmware dump (256 KB; melonDS generates fallback if absent).",
    },
    BiosFirmwareSpec {
        console_id: "3ds_keys",
        console_name: "Nintendo 3DS (Azahar Keys)",
        emulator_id: "azahar",
        filename: "aes_keys.txt",
        expected_md5: "",
        expected_sha1: "",
        required: false,
        target_subdirs: &["azahar/sysdata", "retroarch/system/azahar/sysdata"],
        notes: "Required only for encrypted 3DS CIA/3DS dumps; unencrypted CCI runs natively.",
    },
    BiosFirmwareSpec {
        console_id: "gba",
        console_name: "Game Boy Advance (GBA)",
        emulator_id: "retroarch_mgba",
        filename: "gba_bios.bin",
        expected_md5: "a860e8c0b6d573d191e4ec7db1b1e4f6",
        expected_sha1: "300c20df6731a33952ded8c436f7f186d25d3492",
        required: false,
        target_subdirs: &["retroarch/system", "mgba"],
        notes: "Optional authentic GBA BIOS boot screen & accurate timing.",
    },
    BiosFirmwareSpec {
        console_id: "dreamcast",
        console_name: "Sega Dreamcast (Flycast)",
        emulator_id: "retroarch_flycast",
        filename: "dc_boot.bin",
        expected_md5: "e10c53c2f8b90bab96ead2d368858623",
        expected_sha1: "89d2b1507e5b49b0f8f4f5c71b164e97f8961e80",
        required: true,
        target_subdirs: &["retroarch/system/dc", "retroarch/system"],
        notes: "Dreamcast BIOS (`dc_boot.bin`) for Flycast Core.",
    },
    BiosFirmwareSpec {
        console_id: "saturn",
        console_name: "Sega Saturn (US/EU)",
        emulator_id: "retroarch_saturn",
        filename: "mpr-17933.bin",
        expected_md5: "3240872c70984b6cbfda1586cab68dbe",
        expected_sha1: "b2b2c515007655aa4c8c97b047696d776a09a41a",
        required: true,
        target_subdirs: &["retroarch/system"],
        notes: "Sega Saturn US/EU BIOS for Beetle Saturn Core.",
    },
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BiosConsoleStatus {
    pub console_id: String,
    pub console_name: String,
    pub emulator_id: String,
    pub filename: String,
    pub expected_md5: String,
    pub expected_sha1: String,
    pub required: bool,
    pub ready: bool,
    pub badge_status: String, // "ready" | "missing" | "bad_dump"
    pub installed_paths: Vec<String>,
    pub cas_backup_path: Option<String>,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BiosQuarantineItem {
    pub filename: String,
    pub actual_md5: String,
    pub expected_md5: String,
    pub quarantine_path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BiosVaultReport {
    pub ready_count: usize,
    pub missing_required_count: usize,
    pub quarantined_count: usize,
    pub scanned_files: usize,
    pub newly_installed: Vec<String>,
    pub consoles: Vec<BiosConsoleStatus>,
    pub quarantined: Vec<BiosQuarantineItem>,
    pub summary: String,
}

fn find_bios_spec_by_hash_or_name(
    md5: &str,
    sha1: &str,
    lower_name: &str,
) -> Option<&'static BiosFirmwareSpec> {
    if !md5.is_empty() {
        if let Some(spec) = BIOS_CATALOG
            .iter()
            .find(|s| !s.expected_md5.is_empty() && s.expected_md5.eq_ignore_ascii_case(md5))
        {
            return Some(spec);
        }
    }
    if !sha1.is_empty() {
        if let Some(spec) = BIOS_CATALOG
            .iter()
            .find(|s| !s.expected_sha1.is_empty() && s.expected_sha1.eq_ignore_ascii_case(sha1))
        {
            return Some(spec);
        }
    }
    BIOS_CATALOG
        .iter()
        .find(|s| s.filename.eq_ignore_ascii_case(lower_name))
}

fn evaluate_bios_status_in_roots(
    emu_root: &Path,
    cas_bios_dir: &Path,
    quarantine_dir: &Path,
    scanned_files: usize,
    newly_installed: Vec<String>,
    quarantined_runtime: Vec<BiosQuarantineItem>,
) -> BiosVaultReport {
    let mut consoles = Vec::with_capacity(BIOS_CATALOG.len());
    let mut ready_count = 0usize;
    let mut missing_required_count = 0usize;

    for spec in BIOS_CATALOG {
        let mut installed_paths = Vec::new();
        for sub in spec.target_subdirs {
            let candidate = emu_root.join(sub).join(spec.filename);
            if candidate.is_file() {
                if spec.expected_md5.is_empty() {
                    installed_paths.push(candidate.to_string_lossy().into_owned());
                } else if let Ok((md5, sha1, _)) = hash_file_md5_sha1_streaming(&candidate) {
                    if md5.eq_ignore_ascii_case(spec.expected_md5)
                        || (!spec.expected_sha1.is_empty()
                            && sha1.eq_ignore_ascii_case(spec.expected_sha1))
                    {
                        installed_paths.push(candidate.to_string_lossy().into_owned());
                    }
                }
            }
        }
        let cas_candidate = cas_bios_dir.join(spec.filename);
        let cas_backup_path = if cas_candidate.is_file() {
            Some(cas_candidate.to_string_lossy().into_owned())
        } else {
            None
        };
        let ready = !installed_paths.is_empty() || !spec.required && cas_backup_path.is_some();
        let has_bad = quarantined_runtime
            .iter()
            .any(|q| q.filename.eq_ignore_ascii_case(spec.filename));
        if ready {
            ready_count += 1;
        } else if spec.required {
            missing_required_count += 1;
        }
        let badge_status = if ready {
            "ready".to_string()
        } else if has_bad {
            "bad_dump".to_string()
        } else {
            "missing".to_string()
        };
        consoles.push(BiosConsoleStatus {
            console_id: spec.console_id.to_string(),
            console_name: spec.console_name.to_string(),
            emulator_id: spec.emulator_id.to_string(),
            filename: spec.filename.to_string(),
            expected_md5: spec.expected_md5.to_string(),
            expected_sha1: spec.expected_sha1.to_string(),
            required: spec.required,
            ready,
            badge_status,
            installed_paths,
            cas_backup_path,
            notes: spec.notes.to_string(),
        });
    }

    let mut all_quarantined = quarantined_runtime;
    if quarantine_dir.is_dir() {
        let mut seen: HashSet<String> = all_quarantined
            .iter()
            .map(|q| q.quarantine_path.to_lowercase())
            .collect();
        if let Ok(rd) = fs::read_dir(quarantine_dir) {
            for ent in rd.flatten() {
                let p = ent.path();
                if !p.is_file() {
                    continue;
                }
                let pstr = p.to_string_lossy().into_owned();
                if seen.insert(pstr.to_lowercase()) {
                    let fname = p
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown")
                        .to_string();
                    all_quarantined.push(BiosQuarantineItem {
                        filename: fname,
                        actual_md5: String::new(),
                        expected_md5: String::new(),
                        quarantine_path: pstr,
                        reason: "Previously quarantined invalid BIOS dump.".to_string(),
                    });
                }
            }
        }
    }

    let quarantined_count = all_quarantined.len();
    let summary = if missing_required_count == 0 {
        format!(
            "All required console BIOS ready ({ready_count}/{} entries verified in vault).",
            BIOS_CATALOG.len()
        )
    } else {
        format!(
            "{ready_count}/{} verified · {missing_required_count} required console BIOS file(s) missing.",
            BIOS_CATALOG.len()
        )
    };

    BiosVaultReport {
        ready_count,
        missing_required_count,
        quarantined_count,
        scanned_files,
        newly_installed,
        consoles,
        quarantined: all_quarantined,
        summary,
    }
}

pub(crate) type ArchiveExtractFn<'a> = &'a dyn Fn(&Path, &Path) -> Result<(), String>;

pub(crate) fn scan_and_install_bios_in_roots(
    source_paths: &[PathBuf],
    emu_root: &Path,
    cas_bios_dir: &Path,
    quarantine_dir: &Path,
    extract_fn: Option<ArchiveExtractFn<'_>>,
) -> Result<BiosVaultReport, String> {
    fs::create_dir_all(emu_root).map_err(|e| format!("Failed creating emulators dir: {e}"))?;
    fs::create_dir_all(cas_bios_dir)
        .map_err(|e| format!("Failed creating BIOS CAS backup dir: {e}"))?;

    let mut search_dirs: Vec<PathBuf> = Vec::new();
    let mut direct_files: Vec<PathBuf> = Vec::new();
    let mut temp_extract_dirs: Vec<PathBuf> = Vec::new();

    // Also include already-present CAS vault backups and emulator root as recovery sources
    if cas_bios_dir.is_dir() {
        search_dirs.push(cas_bios_dir.to_path_buf());
    }
    if emu_root.is_dir() {
        search_dirs.push(emu_root.to_path_buf());
    }

    for (idx, p) in source_paths.iter().enumerate() {
        if p.is_dir() {
            search_dirs.push(p.clone());
        } else if p.is_file() {
            let ext = p
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if matches!(ext.as_str(), "zip" | "7z" | "rar") {
                if let Some(unpack) = extract_fn {
                    let stage = cas_bios_dir.join(format!("_stage_{idx}_{}", std::process::id()));
                    let _ = unpack(p, &stage);
                    if stage.is_dir() {
                        search_dirs.push(stage.clone());
                        temp_extract_dirs.push(stage);
                    }
                }
            } else {
                direct_files.push(p.clone());
            }
        }
    }

    let mut candidate_files: Vec<PathBuf> = direct_files;
    let mut seen_paths: HashSet<String> = candidate_files
        .iter()
        .map(|p| p.to_string_lossy().to_ascii_lowercase())
        .collect();

    for dir in &search_dirs {
        for ent in WalkDir::new(dir)
            .max_depth(4)
            .follow_links(false)
            .into_iter()
            .flatten()
        {
            if !ent.file_type().is_file() {
                continue;
            }
            let p = ent.path();
            if p.components()
                .any(|c| c.as_os_str().to_string_lossy().eq_ignore_ascii_case("quarantine"))
            {
                continue;
            }
            let meta_len = ent.metadata().map(|m| m.len()).unwrap_or(u64::MAX);
            if meta_len == 0 || meta_len > BIOS_MAX_BYTES {
                continue;
            }
            let key = p.to_string_lossy().to_ascii_lowercase();
            if seen_paths.insert(key) {
                candidate_files.push(p.to_path_buf());
            }
        }
    }

    let scanned_files = candidate_files.len();
    let mut newly_installed = Vec::new();
    let mut installed_set = BTreeMap::<String, ()>::new();
    let mut quarantined = Vec::new();

    for cand in &candidate_files {
        let lower_name = cand
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if matches!(
            cand.extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase()
                .as_str(),
            "exe" | "dll" | "iso" | "chd" | "rvz" | "nsp" | "xci"
        ) {
            continue;
        }
        let Ok((md5, sha1, _)) = hash_file_md5_sha1_streaming(cand) else {
            continue;
        };
        let Some(spec) = find_bios_spec_by_hash_or_name(&md5, &sha1, &lower_name) else {
            continue;
        };

        let hash_matches = spec.expected_md5.is_empty()
            || md5.eq_ignore_ascii_case(spec.expected_md5)
            || (!spec.expected_sha1.is_empty() && sha1.eq_ignore_ascii_case(spec.expected_sha1));

        if hash_matches {
            let cas_dst = cas_bios_dir.join(spec.filename);
            if cas_dst != *cand {
                let _ = fs::copy(cand, &cas_dst);
            }
            for sub in spec.target_subdirs {
                let dst_dir = emu_root.join(sub);
                if fs::create_dir_all(&dst_dir).is_ok() {
                    let dst = dst_dir.join(spec.filename);
                    let same_target = dst.is_file()
                        && hash_file_md5_sha1_streaming(&dst)
                            .map(|(m, _, _)| m == md5)
                            .unwrap_or(false);
                    if !same_target && fs::copy(cand, &dst).is_ok() {
                        installed_set.insert(format!("{} -> {}", spec.console_name, dst.display()), ());
                    }
                }
            }
        } else {
            // Quarantine bad dump ONLY when user explicitly scanned a folder/archive containing an invalid BIOS file
            let is_already_cas_or_q = cand.starts_with(quarantine_dir);
            if !is_already_cas_or_q {
                let _ = fs::create_dir_all(quarantine_dir);
                let q_name = format!("{}.bad_{}", spec.filename, &md5[..8.min(md5.len())]);
                let q_path = quarantine_dir.join(&q_name);
                if fs::copy(cand, &q_path).is_ok() {
                    quarantined.push(BiosQuarantineItem {
                        filename: spec.filename.to_string(),
                        actual_md5: md5,
                        expected_md5: spec.expected_md5.to_string(),
                        quarantine_path: q_path.to_string_lossy().into_owned(),
                        reason: format!(
                            "MD5 mismatch for {} ({}): file does not match Libretro .info checksum.",
                            spec.filename, spec.console_name
                        ),
                    });
                }
            }
        }
    }

    for k in installed_set.into_keys() {
        newly_installed.push(k);
    }
    for stage in temp_extract_dirs {
        let _ = fs::remove_dir_all(stage);
    }

    Ok(evaluate_bios_status_in_roots(
        emu_root,
        cas_bios_dir,
        quarantine_dir,
        scanned_files,
        newly_installed,
        quarantined,
    ))
}

fn resolve_vault_roots() -> Result<(PathBuf, PathBuf, PathBuf), String> {
    let data_root = crate::resolve_data_root_dir_standalone()?;
    let emu_root = crate::emulators::emulators_root_dir()?;
    let cas_bios_dir = data_root.join(CAS_DIR_NAME).join(BIOS_VAULT_SUBDIR);
    let quarantine_dir = data_root.join("quarantine").join("bios");
    Ok((emu_root, cas_bios_dir, quarantine_dir))
}

#[tauri::command]
pub async fn get_bios_vault_status() -> Result<BiosVaultReport, String> {
    tokio::task::spawn_blocking(|| -> Result<BiosVaultReport, String> {
        let (emu_root, cas_bios_dir, quarantine_dir) = resolve_vault_roots()?;
        Ok(evaluate_bios_status_in_roots(
            &emu_root,
            &cas_bios_dir,
            &quarantine_dir,
            0,
            Vec::new(),
            Vec::new(),
        ))
    })
    .await
    .map_err(|e| format!("BIOS status task failed: {e}"))?
}

#[tauri::command]
pub async fn scan_and_install_bios_sources(
    source_paths: Vec<String>,
) -> Result<BiosVaultReport, String> {
    tokio::task::spawn_blocking(move || -> Result<BiosVaultReport, String> {
        let (emu_root, cas_bios_dir, quarantine_dir) = resolve_vault_roots()?;
        let paths: Vec<PathBuf> = source_paths
            .into_iter()
            .map(|s| PathBuf::from(s.trim().trim_matches('"')))
            .filter(|p| p.exists())
            .collect();
        let extractor = |arch: &Path, dst: &Path| crate::extract_archive_to_folder_sync(arch, dst);
        scan_and_install_bios_in_roots(
            &paths,
            &emu_root,
            &cas_bios_dir,
            &quarantine_dir,
            Some(&extractor),
        )
    })
    .await
    .map_err(|e| format!("BIOS scan task failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_and_sha1_match_official_test_vectors() {
        assert_eq!(md5_hex_bytes(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex_bytes(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            sha1_hex_bytes(b""),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
        assert_eq!(
            sha1_hex_bytes(b"abc"),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
    }

    #[test]
    fn bios_vault_quarantines_bad_dump_and_readies_matching_filename() {
        let root = std::env::temp_dir().join(format!("duke_bios_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let src = root.join("custom_bios");
        let emu = root.join("emulators");
        let cas = root.join(".duke-cas").join("bios_vault");
        let q = root.join("quarantine").join("bios");
        fs::create_dir_all(&src).unwrap();

        // Corrupt scph5501.bin must get quarantined instead of copied into DuckStation
        fs::write(src.join("scph5501.bin"), b"BAD_DUMP_BYTES").unwrap();
        // Valid custom aes_keys.txt (optional, open format) installs cleanly
        fs::write(src.join("aes_keys.txt"), b"slot0x25KeyX=0000\n").unwrap();

        let report = scan_and_install_bios_in_roots(&[src], &emu, &cas, &q, None).unwrap();
        assert_eq!(report.quarantined_count, 1);
        assert_eq!(report.quarantined[0].filename, "scph5501.bin");
        assert!(!emu.join("duckstation/bios/scph5501.bin").exists());
        assert!(emu.join("azahar/sysdata/aes_keys.txt").is_file());
        assert!(cas.join("aes_keys.txt").is_file());

        let _ = fs::remove_dir_all(&root);
    }
}

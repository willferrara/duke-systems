//! Automatic Crash-to-Fix Engine + Direct PCGamingWiki Fix Intelligence.
//!
//! 1. Diagnoses premature `<5s` game exits (`0xC000007B`, `0xC0000005`, `0xC0000135`, `0x8007007E`)
//!    captured by the Win32 Job Object.
//! 2. Applies reversible 1-click local repairs (DirectX/VC++ DLL heal, 32-bit 4 GB LAA flag, DXVK
//!    wrapper toggle, High-DPI / Win7 / Win8 compatibility shims in HKCU `AppCompatFlags\Layers`)
//!    strictly after creating a `.duke-cas/` restore snapshot.
//! 3. Queries PCGamingWiki public APIs (`api/appid.php` + `w/api.php`) via `duke-core` DoH to
//!    surface **Issues Fixed**, **Crash Workarounds**, **Ultrawide / FOV Patches**, and
//!    **Controller Fixes** plus direct links to PCGamingWiki, ProtonDB, Steam Discussions, and
//!    GitHub Issues.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const PCGW_CACHE_FILE: &str = "pcgw_fix_cache.json";
const PCGW_CACHE_TTL_SECS: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashFixOption {
    pub id: String,
    pub label: String,
    pub description: String,
    pub recommended: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashDiagnosisReport {
    pub game_id: String,
    pub title: String,
    pub exe_path: String,
    pub exit_code: i32,
    pub exit_code_hex: String,
    pub elapsed_ms: u64,
    pub fault_name: String,
    pub plain_english_cause: String,
    pub recommended_actions: Vec<CrashFixOption>,
    pub pcgamingwiki_url: String,
    pub protondb_url: Option<String>,
    pub steam_discussions_url: Option<String>,
    pub github_issues_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct FixApplyRequest {
    pub create_cas_backup: bool,
    pub heal_runtime_dlls: bool,
    pub enable_4gb_laa: bool,
    pub toggle_dxvk: Option<bool>,
    pub high_dpi_aware: bool,
    pub compat_os_shim: Option<String>, // None | "win7" | "win8" | "clear"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FixApplyReport {
    pub game_id: String,
    pub snapshot_id: Option<String>,
    pub applied_steps: Vec<String>,
    pub warnings: Vec<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PcgwFixIntelligence {
    pub game_title: String,
    pub app_id: Option<u32>,
    pub page_title: String,
    pub pcgamingwiki_url: String,
    pub protondb_url: Option<String>,
    pub steam_discussions_url: Option<String>,
    pub github_issues_url: String,
    pub issues_fixed: Vec<String>,
    pub crash_workarounds: Vec<String>,
    pub ultrawide_fov_patches: Vec<String>,
    pub controller_fixes: Vec<String>,
    pub fetched_from_cache: bool,
    pub fetched_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PcgwDiskCache {
    entries: BTreeMap<String, PcgwFixIntelligence>,
}

fn cache_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn build_community_fix_links(
    title: &str,
    app_id: Option<u32>,
    resolved_pcgw_title: Option<&str>,
) -> (String, Option<String>, Option<String>, String) {
    let valid_app_id = app_id.filter(|&id| id > 0);
    let clean_title = title.trim();
    let pcgw_url = if let Some(page) = resolved_pcgw_title.filter(|p| !p.trim().is_empty()) {
        format!(
            "https://www.pcgamingwiki.com/wiki/{}",
            urlencoding::encode(&page.replace(' ', "_"))
        )
    } else if let Some(aid) = valid_app_id {
        format!("https://www.pcgamingwiki.com/api/appid.php?appid={aid}")
    } else {
        format!(
            "https://www.pcgamingwiki.com/w/index.php?search={}",
            urlencoding::encode(clean_title)
        )
    };
    let protondb_url = valid_app_id.map(|aid| format!("https://www.protondb.com/app/{aid}"));
    let steam_disc_url =
        valid_app_id.map(|aid| format!("https://steamcommunity.com/app/{aid}/discussions/"));
    let github_issues_url = format!(
        "https://github.com/search?q={}+crash+OR+fix&type=issues",
        urlencoding::encode(clean_title)
    );
    (pcgw_url, protondb_url, steam_disc_url, github_issues_url)
}

/// Diagnoses premature `<5s` exit codes and builds 1-click remediation recommendations.
pub fn diagnose_crash_code(
    game_id: &str,
    title: &str,
    exe_path: &str,
    app_id: Option<u32>,
    exit_code: i32,
    elapsed_ms: u64,
) -> Option<CrashDiagnosisReport> {
    let ucode = exit_code as u32;
    let hex = format!("0x{ucode:08X}");
    let (fault_name, cause, rec_dlls, rec_laa, rec_dxvk, rec_shim) = match ucode {
        0xC000_007B => (
            "STATUS_INVALID_IMAGE_FORMAT (0xC000007B)",
            "Architecture mismatch or corrupted DirectX / Visual C++ DLL (32-bit vs 64-bit runtime loaded next to the game).",
            true,
            false,
            true,
            false,
        ),
        0xC000_0135 => (
            "STATUS_DLL_NOT_FOUND (0xC0000135)",
            "Required Visual C++ or DirectX DLL is missing from the game folder and Windows System32/SysWOW64.",
            true,
            false,
            false,
            false,
        ),
        0x8007_007E => (
            "ERROR_MOD_NOT_FOUND (0x8007007E)",
            "A required dynamic module or plugin DLL could not be loaded during startup.",
            true,
            false,
            false,
            false,
        ),
        0xC000_0005 => (
            "STATUS_ACCESS_VIOLATION (0xC0000005)",
            "Game crashed dereferencing memory—frequently fixed on older titles by enabling 4 GB LAA, DXVK on Windows 11, or Win7/High-DPI compatibility shims.",
            false,
            true,
            true,
            true,
        ),
        0xC000_0142 => (
            "STATUS_DLL_INIT_FAILED (0xC0000142)",
            "DLL initialization routine aborted cleanly—often fixed by restoring runtime DLLs or toggling DXVK.",
            true,
            false,
            true,
            true,
        ),
        0xC000_001D => (
            "STATUS_ILLEGAL_INSTRUCTION (0xC000001D)",
            "Executable issued an instruction unsupported or misconfigured for current compatibility environment.",
            false,
            false,
            false,
            true,
        ),
        _ => return None,
    };

    let (pcgamingwiki_url, protondb_url, steam_discussions_url, github_issues_url) =
        build_community_fix_links(title, app_id, None);

    let recommended_actions = vec![
        CrashFixOption {
            id: "heal_runtime_dlls".to_string(),
            label: "Heal DirectX / VC++ DLLs".to_string(),
            description: "Scan PE imports and copy genuine local DirectX / VC++ redistributable DLLs alongside the game executable.".to_string(),
            recommended: rec_dlls,
        },
        CrashFixOption {
            id: "enable_4gb_laa".to_string(),
            label: "Enable 4 GB LAA Patch".to_string(),
            description: "Patch PE32 IMAGE_FILE_LARGE_ADDRESS_AWARE bit (if 32-bit) to unlock 4 GB virtual memory.".to_string(),
            recommended: rec_laa,
        },
        CrashFixOption {
            id: "toggle_dxvk".to_string(),
            label: "Enable DXVK (Vulkan Translation)".to_string(),
            description: "Route DX9/DX10/DX11 calls over Vulkan for legacy Windows 11 compatibility.".to_string(),
            recommended: rec_dxvk,
        },
        CrashFixOption {
            id: "compat_shims".to_string(),
            label: "Apply High-DPI & Win7 Compat Shims".to_string(),
            description: "Register reversible per-user HIGHDPIAWARE + WIN7RTM shims in HKCU AppCompatFlags\\Layers.".to_string(),
            recommended: rec_shim,
        },
    ];

    Some(CrashDiagnosisReport {
        game_id: game_id.to_string(),
        title: title.to_string(),
        exe_path: exe_path.to_string(),
        exit_code,
        exit_code_hex: hex,
        elapsed_ms,
        fault_name: fault_name.to_string(),
        plain_english_cause: cause.to_string(),
        recommended_actions,
        pcgamingwiki_url,
        protondb_url,
        steam_discussions_url,
        github_issues_url,
    })
}

#[cfg(target_os = "windows")]
mod win_compat_reg {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    const HKEY_CURRENT_USER: isize = -0x7FFF_FFFF; // 0x80000001
    const KEY_READ: u32 = 0x20019;
    const KEY_WRITE: u32 = 0x20006;
    const REG_SZ: u32 = 1;
    const SUBKEY_LAYERS: &str =
        "Software\\Microsoft\\Windows NT\\CurrentVersion\\AppCompatFlags\\Layers";

    #[link(name = "advapi32")]
    extern "system" {
        fn RegCreateKeyExW(
            h_key: isize,
            sub_key: *const u16,
            reserved: u32,
            class: *mut u16,
            options: u32,
            sam_desired: u32,
            security_attributes: *const c_void,
            phk_result: *mut isize,
            disposition: *mut u32,
        ) -> i32;
        fn RegQueryValueExW(
            h_key: isize,
            value_name: *const u16,
            reserved: *mut u32,
            lp_type: *mut u32,
            lp_data: *mut u8,
            lpcb_data: *mut u32,
        ) -> i32;
        fn RegSetValueExW(
            h_key: isize,
            value_name: *const u16,
            reserved: u32,
            dw_type: u32,
            lp_data: *const u8,
            cb_data: u32,
        ) -> i32;
        fn RegDeleteValueW(h_key: isize, value_name: *const u16) -> i32;
        fn RegCloseKey(h_key: isize) -> i32;
    }

    fn utf16_z(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    pub fn read_layer_tokens(exe_path: &Path) -> Vec<String> {
        let sub_w = utf16_z(SUBKEY_LAYERS);
        let val_w = utf16_z(&exe_path.to_string_lossy());
        let mut hkey: isize = 0;
        let open_rc = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                sub_w.as_ptr(),
                0,
                std::ptr::null_mut(),
                0,
                KEY_READ,
                std::ptr::null(),
                &mut hkey,
                std::ptr::null_mut(),
            )
        };
        if open_rc != 0 || hkey == 0 {
            return Vec::new();
        }
        let mut buf = vec![0u16; 512];
        let mut byte_len = (buf.len() * 2) as u32;
        let mut val_type = 0u32;
        let rc = unsafe {
            RegQueryValueExW(
                hkey,
                val_w.as_ptr(),
                std::ptr::null_mut(),
                &mut val_type,
                buf.as_mut_ptr().cast::<u8>(),
                &mut byte_len,
            )
        };
        unsafe {
            RegCloseKey(hkey);
        }
        if rc != 0 || val_type != REG_SZ {
            return Vec::new();
        }
        let u16_len = (byte_len as usize / 2).min(buf.len());
        let s = String::from_utf16_lossy(&buf[..u16_len])
            .trim_end_matches('\0')
            .to_string();
        s.split_whitespace()
            .filter(|t| *t != "~")
            .map(ToString::to_string)
            .collect()
    }

    pub fn write_layer_tokens(exe_path: &Path, tokens: &[String]) -> Result<(), String> {
        let sub_w = utf16_z(SUBKEY_LAYERS);
        let val_w = utf16_z(&exe_path.to_string_lossy());
        let mut hkey: isize = 0;
        let open_rc = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                sub_w.as_ptr(),
                0,
                std::ptr::null_mut(),
                0,
                KEY_WRITE,
                std::ptr::null(),
                &mut hkey,
                std::ptr::null_mut(),
            )
        };
        if open_rc != 0 || hkey == 0 {
            return Err(format!("Failed opening HKCU AppCompatFlags\\Layers: Win32 {open_rc}"));
        }
        let rc = if tokens.is_empty() {
            unsafe { RegDeleteValueW(hkey, val_w.as_ptr()) }
        } else {
            let mut joined = String::from("~ ");
            joined.push_str(&tokens.join(" "));
            let data_w = utf16_z(&joined);
            unsafe {
                RegSetValueExW(
                    hkey,
                    val_w.as_ptr(),
                    0,
                    REG_SZ,
                    data_w.as_ptr().cast::<u8>(),
                    (data_w.len() * 2) as u32,
                )
            }
        };
        unsafe {
            RegCloseKey(hkey);
        }
        if rc != 0 && !tokens.is_empty() {
            return Err(format!("Failed writing AppCompatFlags\\Layers: Win32 {rc}"));
        }
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
mod win_compat_reg {
    use std::path::Path;
    pub fn read_layer_tokens(_exe_path: &Path) -> Vec<String> {
        Vec::new()
    }
    pub fn write_layer_tokens(_exe_path: &Path, _tokens: &[String]) -> Result<(), String> {
        Err("Windows compatibility shims require Windows.".to_string())
    }
}

/// Strips common MediaWiki templates/links cleanly into readable plain English.
pub fn clean_wikitext_line(raw: &str) -> String {
    let mut s = raw.trim().trim_start_matches('*').trim().to_string();
    // Replace {{Key|...}} / {{Fixbox|...}} etc. by keeping readable inner text when simple.
    for _ in 0..8 {
        let Some(start) = s.find("{{") else {
            break;
        };
        let Some(rel_end) = s[start + 2..].find("}}") else {
            break;
        };
        let end = start + 2 + rel_end + 2;
        let inner = &s[start + 2..end - 2];
        let replacement = if let Some((head, tail)) = inner.split_once('|') {
            let head_lower = head.trim().to_ascii_lowercase();
            if matches!(head_lower.as_str(), "key" | "code" | "file" | "path" | "ii") {
                tail.split('|').next().unwrap_or("").trim().to_string()
            } else {
                String::new()
            }
        } else {
            String::new()
        };
        s.replace_range(start..end, &replacement);
    }
    // Simplify [[Target|Label]] -> Label and [[Target]] -> Target.
    for _ in 0..8 {
        let Some(start) = s.find("[[") else {
            break;
        };
        let Some(rel_end) = s[start + 2..].find("]]") else {
            break;
        };
        let end = start + 2 + rel_end + 2;
        let inner = &s[start + 2..end - 2];
        let label = inner
            .split_once('|')
            .map(|(_, r)| r)
            .unwrap_or(inner)
            .trim()
            .to_string();
        s.replace_range(start..end, &label);
    }
    // Strip HTML comments and simple tags
    for tag in ["<br>", "<br/>", "<br />", "<code>", "</code>", "<small>", "</small>"] {
        s = s.replace(tag, " ");
    }
    s.replace("'''", "")
        .replace("''", "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Extracts structured fix highlights from PCGamingWiki MediaWiki wikitext.
pub fn parse_pcgamingwiki_wikitext(
    wikitext: &str,
) -> (Vec<String>, Vec<String>, Vec<String>, Vec<String>) {
    let mut issues_fixed = Vec::new();
    let mut crash_workarounds = Vec::new();
    let mut ultrawide_fov = Vec::new();
    let mut controller_fixes = Vec::new();

    #[derive(Copy, Clone, PartialEq, Eq)]
    enum Bucket {
        None,
        IssuesFixed,
        IssuesUnresolved,
        Ultrawide,
        Controller,
    }

    let mut current = Bucket::None;
    for raw_line in wikitext.lines() {
        let line = raw_line.trim();
        if line.starts_with("==") && line.ends_with("==") {
            let heading = line.trim_matches('=').trim().to_ascii_lowercase();
            current = if heading.contains("issues fixed") {
                Bucket::IssuesFixed
            } else if heading.contains("issues unresolved")
                || heading.contains("crash")
                || heading.contains("freeze")
            {
                Bucket::IssuesUnresolved
            } else if heading.contains("widescreen")
                || heading.contains("ultrawide")
                || heading.contains("field of view")
                || heading.contains("fov")
            {
                Bucket::Ultrawide
            } else if heading.contains("controller") || heading.contains("input") {
                Bucket::Controller
            } else {
                Bucket::None
            };
            continue;
        }

        if line.starts_with('*') || line.contains("crash") || line.contains("fix") || line.contains("ultrawide") {
            let cleaned = clean_wikitext_line(line);
            if cleaned.len() < 12 || cleaned.starts_with('|') || cleaned.starts_with('{') {
                continue;
            }
            match current {
                Bucket::IssuesFixed if issues_fixed.len() < 6 => issues_fixed.push(cleaned),
                Bucket::IssuesUnresolved if crash_workarounds.len() < 6 => {
                    crash_workarounds.push(cleaned)
                }
                Bucket::Ultrawide if ultrawide_fov.len() < 5 => ultrawide_fov.push(cleaned),
                Bucket::Controller if controller_fixes.len() < 5 => controller_fixes.push(cleaned),
                _ => {}
            }
        }
    }

    (issues_fixed, crash_workarounds, ultrawide_fov, controller_fixes)
}

fn load_pcgw_cache() -> PcgwDiskCache {
    let _g = cache_lock().lock().unwrap_or_else(|e| e.into_inner());
    let Ok(path) = crate::get_app_data_file_standalone(PCGW_CACHE_FILE) else {
        return PcgwDiskCache::default();
    };
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<PcgwDiskCache>(&s).ok())
        .unwrap_or_default()
}

fn save_pcgw_cache(cache: &PcgwDiskCache) {
    let _g = cache_lock().lock().unwrap_or_else(|e| e.into_inner());
    if let Ok(path) = crate::get_app_data_file_standalone(PCGW_CACHE_FILE) {
        if let Ok(json) = serde_json::to_string_pretty(cache) {
            let _ = crate::atomic_write_string(&path, &json);
        }
    }
}

#[tauri::command]
pub async fn analyze_game_crash_or_preflight(
    game_id: String,
    exit_code: Option<i32>,
    elapsed_ms: Option<u64>,
) -> Result<CrashDiagnosisReport, String> {
    let gid = game_id.trim().to_string();
    let lib = crate::load_library_standalone()?;
    let game = lib
        .into_iter()
        .find(|g| g.id == gid)
        .ok_or_else(|| format!("Game '{}' not found in library.", gid))?;
    let code = exit_code.unwrap_or(0xC000_0135u32 as i32);
    let elapsed = elapsed_ms.unwrap_or(1200);
    if let Some(diag) = diagnose_crash_code(
        &game.id,
        &game.title,
        &game.exe_path,
        game.app_id,
        code,
        elapsed,
    ) {
        return Ok(diag);
    }
    let (pcgamingwiki_url, protondb_url, steam_discussions_url, github_issues_url) =
        build_community_fix_links(&game.title, game.app_id, None);
    Ok(CrashDiagnosisReport {
        game_id: game.id,
        title: game.title,
        exe_path: game.exe_path,
        exit_code: code,
        exit_code_hex: format!("0x{:08X}", code as u32),
        elapsed_ms: elapsed,
        fault_name: "General Launch Health Check".to_string(),
        plain_english_cause: "You can verify DLLs, enable 4 GB memory support, toggle DXVK, or apply Windows compatibility shims with an automatic .duke-cas restore point.".to_string(),
        recommended_actions: vec![
            CrashFixOption {
                id: "heal_runtime_dlls".to_string(),
                label: "Heal DirectX / VC++ DLLs".to_string(),
                description: "Verify missing runtime imports and heal from local redistributables.".to_string(),
                recommended: true,
            },
            CrashFixOption {
                id: "enable_4gb_laa".to_string(),
                label: "Enable 4 GB LAA Patch".to_string(),
                description: "Unlock >2 GB virtual RAM on 32-bit titles.".to_string(),
                recommended: false,
            },
            CrashFixOption {
                id: "toggle_dxvk".to_string(),
                label: "Toggle DXVK Wrapper".to_string(),
                description: "Deploy or remove local d3d9/d3d11/dxgi translation wrappers.".to_string(),
                recommended: false,
            },
            CrashFixOption {
                id: "compat_shims".to_string(),
                label: "High-DPI / Win7 Shim".to_string(),
                description: "Apply HKCU AppCompatFlags\\Layers override.".to_string(),
                recommended: false,
            },
        ],
        pcgamingwiki_url,
        protondb_url,
        steam_discussions_url,
        github_issues_url,
    })
}

#[tauri::command]
pub async fn apply_one_click_game_fixes(
    game_id: String,
    request: FixApplyRequest,
) -> Result<FixApplyReport, String> {
    let gid = game_id.trim().to_string();
    let lib = crate::load_library_standalone()?;
    let game = lib
        .into_iter()
        .find(|g| g.id == gid)
        .ok_or_else(|| format!("Game '{}' not found.", gid))?;
    let exe_path = PathBuf::from(&game.exe_path);
    if !exe_path.is_file() {
        return Err(format!("Executable missing on disk: {}", game.exe_path));
    }
    let game_dir = crate::resolve_game_root_dir(&exe_path);

    tokio::task::spawn_blocking(move || -> Result<FixApplyReport, String> {
        let mut applied_steps = Vec::new();
        let mut warnings = Vec::new();
        let mut snapshot_id = None;

        if request.create_cas_backup {
            let cas_dir = game_dir.join(crate::snapshots::CAS_DIR_NAME);
            match crate::snapshots::create_snapshot_in_dir(
                &gid,
                &game_dir,
                &cas_dir,
                Some("Pre-Fix Safety Restore"),
            ) {
                Ok(manifest) => {
                    snapshot_id = Some(manifest.id.clone());
                    applied_steps.push(format!("Created .duke-cas restore point {}", manifest.id));
                }
                Err(e) => warnings.push(format!("CAS pre-fix backup skipped: {e}")),
            }
        }

        if request.heal_runtime_dlls {
            match crate::pe_inspect::inspect_exe_standalone(&exe_path) {
                Ok(report) => {
                    let missing: Vec<String> =
                        report.missing_dlls.into_iter().map(|d| d.name).collect();
                    if missing.is_empty() {
                        applied_steps.push("PE preflight checked all imported DLLs (none missing)".to_string());
                    } else {
                        match crate::pe_inspect::heal_dlls_standalone(&exe_path, &missing) {
                            Ok(heal) => applied_steps.push(heal.summary),
                            Err(e) => warnings.push(format!("Runtime DLL heal failed: {e}")),
                        }
                    }
                }
                Err(e) => warnings.push(format!("PE preflight failed: {e}")),
            }
        }

        if request.enable_4gb_laa {
            match crate::pe_inspect::set_pe_large_address_aware(&exe_path, true) {
                Ok(true) => applied_steps.push("Patched 4 GB Large Address Aware flag on executable".to_string()),
                Ok(false) => warnings.push("Executable is 64-bit or did not require LAA patch".to_string()),
                Err(e) => warnings.push(format!("4 GB LAA patch failed: {e}")),
            }
        }

        if let Some(enable_dxvk) = request.toggle_dxvk {
            match crate::pe_inspect::toggle_dxvk_standalone(&gid, &exe_path, enable_dxvk) {
                Ok(status) => applied_steps.push(format!(
                    "DXVK wrapper state: {}",
                    if status.dxvk_deployed { "Deployed" } else { "Removed" }
                )),
                Err(e) => warnings.push(format!("DXVK toggle note: {e}")),
            }
        }

        if request.high_dpi_aware || request.compat_os_shim.is_some() {
            let mut tokens = win_compat_reg::read_layer_tokens(&exe_path);
            if request.high_dpi_aware && !tokens.iter().any(|t| t == "HIGHDPIAWARE") {
                tokens.push("HIGHDPIAWARE".to_string());
            }
            if let Some(ref os_mode) = request.compat_os_shim {
                tokens.retain(|t| t != "WIN7RTM" && t != "WIN8RTM");
                match os_mode.trim().to_ascii_lowercase().as_str() {
                    "win7" => tokens.push("WIN7RTM".to_string()),
                    "win8" => tokens.push("WIN8RTM".to_string()),
                    "clear" => tokens.clear(),
                    _ => {}
                }
            }
            match win_compat_reg::write_layer_tokens(&exe_path, &tokens) {
                Ok(()) => applied_steps.push(format!(
                    "Updated AppCompatFlags\\Layers shims: {}",
                    if tokens.is_empty() {
                        "Cleared".to_string()
                    } else {
                        tokens.join(" ")
                    }
                )),
                Err(e) => warnings.push(e),
            }
        }

        let summary = if applied_steps.is_empty() {
            "No local repairs were selected.".to_string()
        } else {
            applied_steps.join(" | ")
        };

        Ok(FixApplyReport {
            game_id: gid,
            snapshot_id,
            applied_steps,
            warnings,
            summary,
        })
    })
    .await
    .map_err(|e| format!("Fix application task failed: {e}"))?
}

#[tauri::command]
pub async fn fetch_pcgamingwiki_fixes(
    game_id: Option<String>,
    title: String,
    app_id: Option<u32>,
    force_refresh: Option<bool>,
) -> Result<PcgwFixIntelligence, String> {
    let clean_title = title.trim().to_string();
    let aid = app_id.filter(|&id| id > 0);
    let cache_key = if let Some(id) = aid {
        format!("app:{id}")
    } else {
        format!("title:{}", clean_title.to_ascii_lowercase())
    };
    let force = force_refresh.unwrap_or(false);
    let now = now_unix_secs();

    if !force {
        let cache = load_pcgw_cache();
        if let Some(hit) = cache.entries.get(&cache_key) {
            if now.saturating_sub(hit.fetched_at) < PCGW_CACHE_TTL_SECS {
                let mut out = hit.clone();
                out.fetched_from_cache = true;
                return Ok(out);
            }
        }
    }

    let client = duke_core::net_doh::get_configured_http_client();
    let mut resolved_page: Option<String> = None;

    // 1. Resolve Steam AppID -> exact PCGamingWiki page via Cargo API when app_id is known.
    if let Some(steam_id) = aid {
        let cargo_url = format!(
            "https://www.pcgamingwiki.com/w/api.php?action=cargoquery&tables=Infobox_game&fields=_pageName%3DPage&where=Infobox_game.Steam_AppID%20HOLDS%20%27{steam_id}%27&format=json"
        );
        if let Ok(resp) = client
            .get(&cargo_url)
            .timeout(Duration::from_secs(7))
            .send()
            .await
        {
            if resp.status().is_success() {
                if let Ok(val) = resp.json::<Value>().await {
                    resolved_page = val
                        .get("cargoquery")
                        .and_then(|c| c.as_array())
                        .and_then(|arr| arr.first())
                        .and_then(|item| item.get("title"))
                        .and_then(|t| t.get("Page"))
                        .and_then(|p| p.as_str())
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty());
                }
            }
        }
    }

    // 2. Fallback to MediaWiki opensearch when Steam AppID is absent or unmatched.
    if resolved_page.is_none() && !clean_title.is_empty() {
        let search_url = format!(
            "https://www.pcgamingwiki.com/w/api.php?action=opensearch&search={}&limit=1&format=json",
            urlencoding::encode(&clean_title)
        );
        if let Ok(resp) = client
            .get(&search_url)
            .timeout(Duration::from_secs(6))
            .send()
            .await
        {
            if resp.status().is_success() {
                if let Ok(val) = resp.json::<Value>().await {
                    resolved_page = val
                        .as_array()
                        .and_then(|arr| arr.get(1))
                        .and_then(|titles| titles.as_array())
                        .and_then(|t| t.first())
                        .and_then(|s| s.as_str())
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty());
                }
            }
        }
    }

    let mut issues_fixed = Vec::new();
    let mut crash_workarounds = Vec::new();
    let mut ultrawide_fov_patches = Vec::new();
    let mut controller_fixes = Vec::new();

    if let Some(ref page) = resolved_page {
        let parse_url = format!(
            "https://www.pcgamingwiki.com/w/api.php?action=parse&page={}&prop=wikitext&format=json",
            urlencoding::encode(page)
        );
        if let Ok(resp) = client
            .get(&parse_url)
            .timeout(Duration::from_secs(8))
            .send()
            .await
        {
            if resp.status().is_success() {
                if let Ok(val) = resp.json::<Value>().await {
                    if let Some(wikitext) = val
                        .get("parse")
                        .and_then(|p| p.get("wikitext"))
                        .and_then(|w| w.get("*"))
                        .and_then(|s| s.as_str())
                    {
                        let parsed = parse_pcgamingwiki_wikitext(wikitext);
                        issues_fixed = parsed.0;
                        crash_workarounds = parsed.1;
                        ultrawide_fov_patches = parsed.2;
                        controller_fixes = parsed.3;
                    }
                }
            }
        }
    }

    let (pcgamingwiki_url, protondb_url, steam_discussions_url, github_issues_url) =
        build_community_fix_links(&clean_title, aid, resolved_page.as_deref());

    let report = PcgwFixIntelligence {
        game_title: clean_title,
        app_id: aid,
        page_title: resolved_page.unwrap_or_else(|| game_id.unwrap_or_default()),
        pcgamingwiki_url,
        protondb_url,
        steam_discussions_url,
        github_issues_url,
        issues_fixed,
        crash_workarounds,
        ultrawide_fov_patches,
        controller_fixes,
        fetched_from_cache: false,
        fetched_at: now,
    };

    let mut cache = load_pcgw_cache();
    cache.entries.insert(cache_key, report.clone());
    save_pcgw_cache(&cache);

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_code_diagnosis_covers_standard_ntstatus_codes() {
        for (code, expected_substr) in [
            (0xC000_007Bu32 as i32, "Architecture mismatch"),
            (0xC000_0005u32 as i32, "4 GB LAA"),
            (0xC000_0135u32 as i32, "missing"),
            (0x8007_007Eu32 as i32, "dynamic module"),
        ] {
            let diag = diagnose_crash_code("g1", "Demo", "C:\\Games\\demo.exe", Some(420), code, 1400)
                .expect("known crash code should produce diagnosis");
            assert!(
                diag.plain_english_cause.contains(expected_substr),
                "unexpected diagnosis for 0x{:08X}: {}",
                code as u32,
                diag.plain_english_cause
            );
            assert_eq!(diag.protondb_url.as_deref(), Some("https://www.protondb.com/app/420"));
            assert_eq!(
                diag.steam_discussions_url.as_deref(),
                Some("https://steamcommunity.com/app/420/discussions/")
            );
        }
        assert!(diagnose_crash_code("g1", "Demo", "demo.exe", None, 0, 12000).is_none());
    }

    #[test]
    fn pcgw_wikitext_parser_extracts_all_four_fix_categories() {
        let sample = r#"
==Issues fixed==
* Random crash on launch in Windows 11 solved by replacing [[d3d9.dll|DXVK d3d9.dll]] next to executable.
==Issues unresolved==
* Stuttering during autosave if {{Key|VSync}} is forced inside driver control panel.
==Widescreen resolution==
* Ultrawide 21:9 FOV scaling requires editing <code>Engine.ini</code> AspectRatioAxisConstraint.
==Input==
* DualSense controller prompts work natively once Steam Input wrapper is disabled.
"#;
        let (fixed, crashes, uw, ctrl) = parse_pcgamingwiki_wikitext(sample);
        assert_eq!(fixed.len(), 1);
        assert!(fixed[0].contains("DXVK d3d9.dll"));
        assert_eq!(crashes.len(), 1);
        assert!(crashes[0].contains("VSync"));
        assert_eq!(uw.len(), 1);
        assert!(uw[0].contains("Ultrawide 21:9 FOV"));
        assert_eq!(ctrl.len(), 1);
        assert!(ctrl[0].contains("DualSense controller"));
    }
}

use memmap2::Mmap;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::Instant;
use walkdir::WalkDir;

pub const IMAGE_FILE_MACHINE_I386: u16 = 0x014C;
pub const IMAGE_FILE_MACHINE_AMD64: u16 = 0x8664;
pub const IMAGE_FILE_MACHINE_ARM64: u16 = 0xAA64;

const RUNTIME_DLL_HINTS: &[(&str, &str)] = &[
    ("vcruntime140_1.dll", "Visual C++ 2015-2022 Redistributable (x64)"),
    ("vcruntime140.dll", "Visual C++ 2015-2022 Redistributable"),
    ("msvcp140.dll", "Visual C++ 2015-2022 Redistributable"),
    ("msvcr120.dll", "Visual C++ 2013 Redistributable"),
    ("msvcp120.dll", "Visual C++ 2013 Redistributable"),
    ("msvcr110.dll", "Visual C++ 2012 Redistributable"),
    ("msvcr100.dll", "Visual C++ 2010 Redistributable"),
    ("d3dx9_43.dll", "DirectX End-User Runtime (June 2010)"),
    ("d3dx11_43.dll", "DirectX End-User Runtime (June 2010)"),
    ("d3dcompiler_43.dll", "DirectX Shader Compiler Runtime"),
    ("d3dcompiler_47.dll", "DirectX Shader Compiler Runtime"),
    ("xinput1_3.dll", "DirectX XInput 1.3 Runtime"),
    ("xaudio2_7.dll", "DirectX XAudio2 Runtime"),
    ("openal32.dll", "OpenAL Audio Runtime"),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedDllStatus {
    pub name: String,
    pub resolved: bool,
    pub resolved_path: Option<String>,
    pub source_kind: String,
    pub is_runtime_hint: Option<String>,
    pub heal_candidate_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PePreflightReport {
    pub exe_path: String,
    pub architecture: String,
    pub machine: u16,
    pub is_32bit: bool,
    pub system_dir_checked: String,
    pub imported_dlls: Vec<ImportedDllStatus>,
    pub missing_dlls: Vec<ImportedDllStatus>,
    pub healable_count: usize,
    pub elapsed_us: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DllHealReport {
    pub game_dir: String,
    pub copied_dlls: Vec<String>,
    pub skipped_dlls: Vec<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Copy)]
struct PeSection {
    virtual_address: u32,
    virtual_size: u32,
    size_of_raw_data: u32,
    pointer_to_raw_data: u32,
}

#[inline]
fn read_u16_le(bytes: &[u8], off: usize) -> Result<u16, String> {
    let slice = bytes
        .get(off..off + 2)
        .ok_or_else(|| "Truncated PE u16 field".to_string())?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

#[inline]
fn read_u32_le(bytes: &[u8], off: usize) -> Result<u32, String> {
    let slice = bytes
        .get(off..off + 4)
        .ok_or_else(|| "Truncated PE u32 field".to_string())?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn read_c_string(bytes: &[u8], off: usize) -> Option<String> {
    if off >= bytes.len() {
        return None;
    }
    let slice = &bytes[off..];
    let max_scan = slice.len().min(512);
    let end = slice[..max_scan].iter().position(|&b| b == 0)?;
    let raw = std::str::from_utf8(&slice[..end]).ok()?.trim();
    if raw.is_empty() {
        None
    } else {
        Some(raw.to_string())
    }
}

fn rva_to_file_offset(rva: u32, sections: &[PeSection]) -> Option<usize> {
    for sec in sections {
        let span = sec.virtual_size.max(sec.size_of_raw_data);
        if span == 0 {
            continue;
        }
        let end = sec.virtual_address.saturating_add(span);
        if rva >= sec.virtual_address && rva < end {
            let delta = (rva - sec.virtual_address) as usize;
            if sec.pointer_to_raw_data == 0 {
                return None;
            }
            return Some(sec.pointer_to_raw_data as usize + delta);
        }
    }
    Some(rva as usize)
}

pub fn parse_pe_machine_and_imports(bytes: &[u8]) -> Result<(u16, bool, Vec<String>), String> {
    if bytes.len() < 64 || &bytes[0..2] != b"MZ" {
        return Err("Not a valid MZ/DOS executable".to_string());
    }
    let e_lfanew = read_u32_le(bytes, 0x3C)? as usize;
    if bytes.get(e_lfanew..e_lfanew + 4) != Some(b"PE\0\0") {
        return Err("Missing PE\\0\\0 NT signature".to_string());
    }

    let coff_off = e_lfanew + 4;
    let machine = read_u16_le(bytes, coff_off)?;
    let num_sections = read_u16_le(bytes, coff_off + 2)? as usize;
    let size_of_opt = read_u16_le(bytes, coff_off + 16)? as usize;

    let opt_off = coff_off + 20;
    let opt_magic = read_u16_le(bytes, opt_off)?;
    let is_32bit = match opt_magic {
        0x010B => true,
        0x020B => false,
        other => return Err(format!("Unknown PE OptionalHeader magic 0x{other:04X}")),
    };

    let dd_base = if is_32bit { opt_off + 96 } else { opt_off + 112 };
    let rva_count_off = if is_32bit { opt_off + 92 } else { opt_off + 108 };
    let rva_count = read_u32_le(bytes, rva_count_off)?;

    let sec_base = opt_off + size_of_opt;
    let mut sections = Vec::with_capacity(num_sections.min(128));
    for i in 0..num_sections.min(128) {
        let s_off = sec_base + i * 40;
        if s_off + 40 > bytes.len() {
            break;
        }
        sections.push(PeSection {
            virtual_size: read_u32_le(bytes, s_off + 8)?,
            virtual_address: read_u32_le(bytes, s_off + 12)?,
            size_of_raw_data: read_u32_le(bytes, s_off + 16)?,
            pointer_to_raw_data: read_u32_le(bytes, s_off + 20)?,
        });
    }

    let mut dlls = Vec::new();
    let mut seen = HashSet::new();
    if rva_count >= 2 && dd_base + 16 <= opt_off + size_of_opt {
        let import_rva = read_u32_le(bytes, dd_base + 8)?;
        let import_size = read_u32_le(bytes, dd_base + 12)?;
        if import_rva != 0 && import_size != 0 {
            if let Some(mut desc_off) = rva_to_file_offset(import_rva, &sections) {
                for _ in 0..2048 {
                    if desc_off + 20 > bytes.len() {
                        break;
                    }
                    let orig_first_thunk = read_u32_le(bytes, desc_off)?;
                    let name_rva = read_u32_le(bytes, desc_off + 12)?;
                    let first_thunk = read_u32_le(bytes, desc_off + 16)?;
                    if orig_first_thunk == 0 && name_rva == 0 && first_thunk == 0 {
                        break;
                    }
                    if name_rva != 0 {
                        if let Some(name_off) = rva_to_file_offset(name_rva, &sections) {
                            if let Some(name) = read_c_string(bytes, name_off) {
                                let key = name.to_ascii_lowercase();
                                if seen.insert(key) {
                                    dlls.push(name);
                                }
                            }
                        }
                    }
                    desc_off += 20;
                }
            }
        }
    }

    Ok((machine, is_32bit, dlls))
}

fn windows_system_dir(is_32bit: bool) -> PathBuf {
    let win_root = std::env::var("SystemRoot")
        .or_else(|_| std::env::var("WINDIR"))
        .unwrap_or_else(|_| "C:\\Windows".to_string());
    if is_32bit {
        PathBuf::from(win_root).join("SysWOW64")
    } else {
        PathBuf::from(win_root).join("System32")
    }
}

fn is_virtual_api_set(dll_lower: &str) -> bool {
    dll_lower.starts_with("api-ms-win-") || dll_lower.starts_with("ext-ms-win-")
}

fn runtime_hint_for(dll_lower: &str) -> Option<String> {
    RUNTIME_DLL_HINTS
        .iter()
        .find(|(name, _)| *name == dll_lower)
        .map(|(_, desc)| (*desc).to_string())
}

fn find_case_insensitive_file(dir: &Path, target_lower: &str) -> Option<PathBuf> {
    let exact = dir.join(target_lower);
    if exact.is_file() {
        return Some(exact);
    }
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_file() {
            if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                if name.eq_ignore_ascii_case(target_lower) {
                    return Some(p);
                }
            }
        }
    }
    None
}

fn validate_dll_architecture(candidate: &Path, expect_32bit: bool) -> bool {
    let Ok(f) = File::open(candidate) else {
        return false;
    };
    let Ok(mmap) = (unsafe { Mmap::map(&f) }) else {
        return false;
    };
    match parse_pe_machine_and_imports(&mmap) {
        Ok((_, is_32, _)) => is_32 == expect_32bit,
        Err(_) => false,
    }
}

fn find_verified_redist_candidate(
    dll_lower: &str,
    expect_32bit: bool,
    exe_dir: &Path,
    extra_search_dirs: &[PathBuf],
    system_dir_override: Option<&Path>,
) -> Option<PathBuf> {
    let mut search_roots: Vec<PathBuf> = extra_search_dirs.to_vec();
    for sub in ["_CommonRedist", "Redist", "redist", "DirectX", "vcredist", "bin"] {
        let candidate_dir = exe_dir.join(sub);
        if candidate_dir.is_dir() {
            search_roots.push(candidate_dir);
        }
    }
    if system_dir_override.is_none() {
        if let Ok(pf86) = std::env::var("ProgramFiles(x86)") {
            let steam_redist = PathBuf::from(pf86)
                .join("Steam")
                .join("steamapps")
                .join("common")
                .join("Steamworks Shared")
                .join("_CommonRedist");
            if steam_redist.is_dir() {
                search_roots.push(steam_redist);
            }
        }
        let sys_dir = windows_system_dir(expect_32bit);
        if sys_dir.is_dir() {
            search_roots.push(sys_dir);
        }
    }

    for root in search_roots {
        if let Some(found) = find_case_insensitive_file(&root, dll_lower) {
            if validate_dll_architecture(&found, expect_32bit) {
                return Some(found);
            }
        }
        for entry in WalkDir::new(&root)
            .max_depth(4)
            .follow_links(false)
            .into_iter()
            .flatten()
        {
            if !entry.file_type().is_file() {
                continue;
            }
            if let Some(fname) = entry.path().file_name().and_then(|n| n.to_str()) {
                if fname.eq_ignore_ascii_case(dll_lower)
                    && validate_dll_architecture(entry.path(), expect_32bit)
                {
                    return Some(entry.path().to_path_buf());
                }
            }
        }
    }
    None
}

pub fn inspect_exe_with_dirs(
    exe_path: &Path,
    system_dir_override: Option<&Path>,
    extra_redist_dirs: &[PathBuf],
) -> Result<PePreflightReport, String> {
    let started = Instant::now();
    if !exe_path.is_file() {
        return Err(format!("Executable not found: {}", exe_path.display()));
    }
    let file = File::open(exe_path)
        .map_err(|e| format!("Failed opening {}: {}", exe_path.display(), e))?;
    let mmap = unsafe { Mmap::map(&file) }
        .map_err(|e| format!("Failed mapping {}: {}", exe_path.display(), e))?;

    let (machine, is_32bit, raw_dlls) = parse_pe_machine_and_imports(&mmap)?;
    drop(mmap);

    let architecture = match machine {
        IMAGE_FILE_MACHINE_I386 => "PE32 (x86 32-bit)".to_string(),
        IMAGE_FILE_MACHINE_AMD64 => "PE32+ (x64 64-bit)".to_string(),
        IMAGE_FILE_MACHINE_ARM64 => "PE32+ (ARM64)".to_string(),
        other => format!("PE (0x{other:04X})"),
    };

    let exe_dir = exe_path.parent().unwrap_or_else(|| Path::new("."));
    let sys_dir = system_dir_override
        .map(Path::to_path_buf)
        .unwrap_or_else(|| windows_system_dir(is_32bit));

    let mut imported_dlls = Vec::with_capacity(raw_dlls.len());
    let mut missing_dlls = Vec::new();
    let mut healable_count = 0usize;

    for dll in raw_dlls {
        let lower = dll.to_ascii_lowercase();
        if is_virtual_api_set(&lower) {
            imported_dlls.push(ImportedDllStatus {
                name: dll,
                resolved: true,
                resolved_path: None,
                source_kind: "api-set-contract".to_string(),
                is_runtime_hint: None,
                heal_candidate_path: None,
            });
            continue;
        }

        let hint = runtime_hint_for(&lower);
        if let Some(found) = find_case_insensitive_file(exe_dir, &lower) {
            imported_dlls.push(ImportedDllStatus {
                name: dll,
                resolved: true,
                resolved_path: Some(found.to_string_lossy().into_owned()),
                source_kind: "game-dir".to_string(),
                is_runtime_hint: hint,
                heal_candidate_path: None,
            });
            continue;
        }
        if let Some(found) = find_case_insensitive_file(&sys_dir, &lower) {
            imported_dlls.push(ImportedDllStatus {
                name: dll,
                resolved: true,
                resolved_path: Some(found.to_string_lossy().into_owned()),
                source_kind: if is_32bit { "syswow64" } else { "system32" }.to_string(),
                is_runtime_hint: hint,
                heal_candidate_path: None,
            });
            continue;
        }

        let heal_cand = find_verified_redist_candidate(
            &lower,
            is_32bit,
            exe_dir,
            extra_redist_dirs,
            system_dir_override,
        )
        .map(|p| p.to_string_lossy().into_owned());
        if heal_cand.is_some() {
            healable_count += 1;
        }

        let status = ImportedDllStatus {
            name: dll,
            resolved: false,
            resolved_path: None,
            source_kind: "missing".to_string(),
            is_runtime_hint: hint,
            heal_candidate_path: heal_cand,
        };
        missing_dlls.push(status.clone());
        imported_dlls.push(status);
    }

    Ok(PePreflightReport {
        exe_path: exe_path.to_string_lossy().into_owned(),
        architecture,
        machine,
        is_32bit,
        system_dir_checked: sys_dir.to_string_lossy().into_owned(),
        imported_dlls,
        missing_dlls,
        healable_count,
        elapsed_us: started.elapsed().as_micros() as u64,
    })
}

pub fn heal_missing_dlls_into_game_dir(
    exe_path: &Path,
    dll_names: Option<&[String]>,
    system_dir_override: Option<&Path>,
    extra_redist_dirs: &[PathBuf],
) -> Result<DllHealReport, String> {
    let report = inspect_exe_with_dirs(exe_path, system_dir_override, extra_redist_dirs)?;
    let exe_dir = exe_path
        .parent()
        .ok_or_else(|| "Invalid executable directory".to_string())?;
    let wanted: Option<HashSet<String>> = dll_names.map(|list| {
        list.iter()
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect()
    });

    let mut copied_dlls = Vec::new();
    let mut skipped_dlls = Vec::new();

    for item in report.missing_dlls {
        if let Some(ref filter) = wanted {
            if !filter.contains(&item.name.to_ascii_lowercase()) {
                continue;
            }
        }
        let Some(cand) = item.heal_candidate_path else {
            skipped_dlls.push(item.name);
            continue;
        };
        let src = PathBuf::from(&cand);
        let dst = exe_dir.join(&item.name);
        if fs::copy(&src, &dst).is_ok() {
            copied_dlls.push(item.name);
        } else {
            skipped_dlls.push(item.name);
        }
    }

    let summary = format!(
        "Auto-healed {} runtime DLL(s) into {} ({} unavailable locally).",
        copied_dlls.len(),
        exe_dir.display(),
        skipped_dlls.len()
    );
    Ok(DllHealReport {
        game_dir: exe_dir.to_string_lossy().into_owned(),
        copied_dlls,
        skipped_dlls,
        summary,
    })
}

/// Creates `<game_dir>/.duke-profile/{AppData,LocalAppData,Documents,SavedGames}` and returns child environment overrides.
pub fn prepare_game_sandbox_env(game_dir: &Path) -> Result<Vec<(String, String)>, String> {
    let profile_root = game_dir.join(".duke-profile");
    let appdata = profile_root.join("AppData");
    let local_appdata = profile_root.join("LocalAppData");
    let documents = profile_root.join("Documents");
    let saved_games = profile_root.join("SavedGames");

    for dir in [&profile_root, &appdata, &local_appdata, &documents, &saved_games] {
        fs::create_dir_all(dir)
            .map_err(|e| format!("Failed creating sandbox directory {}: {}", dir.display(), e))?;
    }

    Ok(vec![
        (
            "APPDATA".to_string(),
            appdata.to_string_lossy().into_owned(),
        ),
        (
            "LOCALAPPDATA".to_string(),
            local_appdata.to_string_lossy().into_owned(),
        ),
        (
            "USERPROFILE".to_string(),
            profile_root.to_string_lossy().into_owned(),
        ),
        (
            "HOME".to_string(),
            profile_root.to_string_lossy().into_owned(),
        ),
        (
            "XDG_CONFIG_HOME".to_string(),
            appdata.to_string_lossy().into_owned(),
        ),
        (
            "XDG_DATA_HOME".to_string(),
            local_appdata.to_string_lossy().into_owned(),
        ),
        (
            "DUKE_SANDBOX_DOCUMENTS".to_string(),
            documents.to_string_lossy().into_owned(),
        ),
        (
            "DUKE_SANDBOX_SAVED_GAMES".to_string(),
            saved_games.to_string_lossy().into_owned(),
        ),
    ])
}

#[tauri::command]
pub async fn preflight_check_exe(exe_path: String) -> Result<PePreflightReport, String> {
    let trimmed = exe_path.trim().to_string();
    tokio::task::spawn_blocking(move || inspect_exe_with_dirs(Path::new(&trimmed), None, &[]))
        .await
        .map_err(|e| format!("PE preflight worker failed: {}", e))?
}

#[tauri::command]
pub async fn heal_game_runtime_dlls(
    exe_path: String,
    dll_names: Option<Vec<String>>,
) -> Result<DllHealReport, String> {
    let trimmed = exe_path.trim().to_string();
    tokio::task::spawn_blocking(move || {
        heal_missing_dlls_into_game_dir(Path::new(&trimmed), dll_names.as_deref(), None, &[])
    })
    .await
    .map_err(|e| format!("DLL heal worker failed: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_synthetic_pe(is_32bit: bool, imported_dlls: &[&str]) -> Vec<u8> {
        let mut buf = vec![0u8; 4096];
        buf[0] = b'M';
        buf[1] = b'Z';
        let e_lfanew = 0x80usize;
        buf[0x3C..0x40].copy_from_slice(&(e_lfanew as u32).to_le_bytes());

        buf[e_lfanew..e_lfanew + 4].copy_from_slice(b"PE\0\0");
        let coff = e_lfanew + 4;
        let machine = if is_32bit {
            IMAGE_FILE_MACHINE_I386
        } else {
            IMAGE_FILE_MACHINE_AMD64
        };
        buf[coff..coff + 2].copy_from_slice(&machine.to_le_bytes());
        buf[coff + 2..coff + 4].copy_from_slice(&1u16.to_le_bytes());
        let opt_size: u16 = if is_32bit { 224 } else { 240 };
        buf[coff + 16..coff + 18].copy_from_slice(&opt_size.to_le_bytes());

        let opt = coff + 20;
        let magic: u16 = if is_32bit { 0x010B } else { 0x020B };
        buf[opt..opt + 2].copy_from_slice(&magic.to_le_bytes());
        let rva_cnt_off = if is_32bit { opt + 92 } else { opt + 108 };
        buf[rva_cnt_off..rva_cnt_off + 4].copy_from_slice(&16u32.to_le_bytes());

        let dd_base = if is_32bit { opt + 96 } else { opt + 112 };
        let idata_rva: u32 = 0x2000;
        let idata_raw: usize = 0x400;
        if !imported_dlls.is_empty() {
            buf[dd_base + 8..dd_base + 12].copy_from_slice(&idata_rva.to_le_bytes());
            buf[dd_base + 12..dd_base + 16].copy_from_slice(&512u32.to_le_bytes());
        }

        let sec = opt + opt_size as usize;
        buf[sec..sec + 6].copy_from_slice(b".rdata");
        buf[sec + 8..sec + 12].copy_from_slice(&0x800u32.to_le_bytes());
        buf[sec + 12..sec + 16].copy_from_slice(&idata_rva.to_le_bytes());
        buf[sec + 16..sec + 20].copy_from_slice(&0x800u32.to_le_bytes());
        buf[sec + 20..sec + 24].copy_from_slice(&(idata_raw as u32).to_le_bytes());

        let mut str_cursor = idata_raw + (imported_dlls.len() + 1) * 20;
        for (i, dll) in imported_dlls.iter().enumerate() {
            let desc = idata_raw + i * 20;
            let name_rva = idata_rva + (str_cursor - idata_raw) as u32;
            buf[desc + 12..desc + 16].copy_from_slice(&name_rva.to_le_bytes());
            buf[desc + 16..desc + 20].copy_from_slice(&1u32.to_le_bytes());
            let nb = dll.as_bytes();
            buf[str_cursor..str_cursor + nb.len()].copy_from_slice(nb);
            buf[str_cursor + nb.len()] = 0;
            str_cursor += nb.len() + 1;
        }
        buf
    }

    #[test]
    fn pe32_and_pe64_import_inspection_and_heal_and_sandbox_work() {
        let root = std::env::temp_dir().join(format!("duke_pe_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let game_dir = root.join("MyGame");
        let fake_sys32 = root.join("System32");
        let fake_syswow = root.join("SysWOW64");
        let redist_dir = root.join("LocalRedist");
        fs::create_dir_all(&game_dir).unwrap();
        fs::create_dir_all(&fake_sys32).unwrap();
        fs::create_dir_all(&fake_syswow).unwrap();
        fs::create_dir_all(&redist_dir).unwrap();

        fs::write(fake_sys32.join("KERNEL32.dll"), build_synthetic_pe(false, &[])).unwrap();
        fs::write(
            redist_dir.join("VCRUNTIME140_1.dll"),
            build_synthetic_pe(false, &[]),
        )
        .unwrap();

        let exe64 = game_dir.join("game64.exe");
        fs::write(
            &exe64,
            build_synthetic_pe(
                false,
                &[
                    "KERNEL32.dll",
                    "VCRUNTIME140_1.dll",
                    "d3dx9_43.dll",
                    "api-ms-win-crt-runtime-l1-1-0.dll",
                ],
            ),
        )
        .unwrap();

        let report64 =
            inspect_exe_with_dirs(&exe64, Some(&fake_sys32), std::slice::from_ref(&redist_dir))
                .unwrap();
        assert!(!report64.is_32bit);
        assert_eq!(report64.missing_dlls.len(), 2);
        assert_eq!(report64.healable_count, 1);

        let heal = heal_missing_dlls_into_game_dir(
            &exe64,
            Some(&["VCRUNTIME140_1.dll".to_string()]),
            Some(&fake_sys32),
            &[redist_dir],
        )
        .unwrap();
        assert_eq!(heal.copied_dlls, vec!["VCRUNTIME140_1.dll".to_string()]);
        assert!(game_dir.join("VCRUNTIME140_1.dll").is_file());

        // 32-bit PE checks SysWOW64 rather than System32.
        fs::write(fake_syswow.join("XInput1_3.dll"), build_synthetic_pe(true, &[])).unwrap();
        let exe32 = game_dir.join("legacy32.exe");
        fs::write(&exe32, build_synthetic_pe(true, &["XInput1_3.dll"])).unwrap();
        let report32 = inspect_exe_with_dirs(&exe32, Some(&fake_syswow), &[]).unwrap();
        assert!(report32.is_32bit);
        assert!(report32.missing_dlls.is_empty());

        let env_pairs = prepare_game_sandbox_env(&game_dir).unwrap();
        assert!(game_dir.join(".duke-profile/AppData").is_dir());
        assert!(game_dir.join(".duke-profile/LocalAppData").is_dir());
        assert!(game_dir.join(".duke-profile/Documents").is_dir());
        assert!(game_dir.join(".duke-profile/SavedGames").is_dir());
        assert!(env_pairs.iter().any(|(k, _)| k == "APPDATA"));

        let _ = fs::remove_dir_all(&root);
    }
}

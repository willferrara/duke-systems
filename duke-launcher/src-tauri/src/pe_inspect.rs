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
pub const IMAGE_FILE_LARGE_ADDRESS_AWARE: u16 = 0x0020;

const DXVK_WRAPPER_DLLS: &[&str] = &[
    "d3d8.dll",
    "d3d9.dll",
    "d3d10core.dll",
    "d3d11.dll",
    "dxgi.dll",
];
const WRAPPERS_MANIFEST_FILE: &str = ".duke-dxvk-manifest.json";

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

#[inline]
fn read_u64_le(bytes: &[u8], off: usize) -> Result<u64, String> {
    let slice = bytes
        .get(off..off + 8)
        .ok_or_else(|| "Truncated PE u64 field".to_string())?;
    Ok(u64::from_le_bytes([
        slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
    ]))
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeExportSymbol {
    pub ordinal: u32,
    pub name: Option<String>,
    pub forwarder: Option<String>,
    pub rva: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeExportInfo {
    pub machine: u16,
    pub is_32bit: bool,
    pub dll_name: Option<String>,
    pub ordinal_base: u32,
    pub exported_names: Vec<String>,
    pub symbols: Vec<PeExportSymbol>,
}

struct ParsedPeTables {
    machine: u16,
    characteristics: u16,
    is_32bit: bool,
    dd_base: usize,
    opt_end: usize,
    rva_count: u32,
    sections: Vec<PeSection>,
}

fn parse_pe_tables(bytes: &[u8]) -> Result<ParsedPeTables, String> {
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
    let characteristics = read_u16_le(bytes, coff_off + 18)?;

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
    let opt_end = opt_off + size_of_opt;

    let mut sections = Vec::with_capacity(num_sections.min(128));
    for i in 0..num_sections.min(128) {
        let s_off = opt_end + i * 40;
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

    Ok(ParsedPeTables {
        machine,
        characteristics,
        is_32bit,
        dd_base,
        opt_end,
        rva_count,
        sections,
    })
}

pub fn parse_pe_header_and_imports(bytes: &[u8]) -> Result<(u16, u16, bool, Vec<String>), String> {
    let tables = parse_pe_tables(bytes)?;
    let mut dlls = Vec::new();
    let mut seen = HashSet::new();
    if tables.rva_count >= 2 && tables.dd_base + 16 <= tables.opt_end {
        let import_rva = read_u32_le(bytes, tables.dd_base + 8)?;
        let import_size = read_u32_le(bytes, tables.dd_base + 12)?;
        if import_rva != 0 && import_size != 0 {
            if let Some(mut desc_off) = rva_to_file_offset(import_rva, &tables.sections) {
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
                        if let Some(name_off) = rva_to_file_offset(name_rva, &tables.sections) {
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

    Ok((tables.machine, tables.characteristics, tables.is_32bit, dlls))
}

/// Parses `IMAGE_DIRECTORY_ENTRY_EXPORT` (`DataDirectory[0]`) of a PE32 or PE32+ binary.
pub fn parse_pe_exports(bytes: &[u8]) -> Result<PeExportInfo, String> {
    let tables = parse_pe_tables(bytes)?;
    let mut info = PeExportInfo {
        machine: tables.machine,
        is_32bit: tables.is_32bit,
        dll_name: None,
        ordinal_base: 0,
        exported_names: Vec::new(),
        symbols: Vec::new(),
    };

    if tables.rva_count < 1 || tables.dd_base + 8 > tables.opt_end {
        return Ok(info);
    }

    let export_rva = read_u32_le(bytes, tables.dd_base)?;
    let export_size = read_u32_le(bytes, tables.dd_base + 4)?;
    if export_rva == 0 || export_size == 0 {
        return Ok(info);
    }

    let Some(dir_off) = rva_to_file_offset(export_rva, &tables.sections) else {
        return Ok(info);
    };
    if dir_off + 40 > bytes.len() {
        return Ok(info);
    }

    let name_rva = read_u32_le(bytes, dir_off + 12)?;
    let ordinal_base = read_u32_le(bytes, dir_off + 16)?;
    let num_funcs = (read_u32_le(bytes, dir_off + 20)? as usize).min(16384);
    let num_names = (read_u32_le(bytes, dir_off + 24)? as usize).min(16384);
    let addr_funcs_rva = read_u32_le(bytes, dir_off + 28)?;
    let addr_names_rva = read_u32_le(bytes, dir_off + 32)?;
    let addr_ords_rva = read_u32_le(bytes, dir_off + 36)?;

    info.ordinal_base = ordinal_base;
    if name_rva != 0 {
        if let Some(name_off) = rva_to_file_offset(name_rva, &tables.sections) {
            info.dll_name = read_c_string(bytes, name_off);
        }
    }

    let funcs_off = rva_to_file_offset(addr_funcs_rva, &tables.sections);
    let names_off = rva_to_file_offset(addr_names_rva, &tables.sections);
    let ords_off = rva_to_file_offset(addr_ords_rva, &tables.sections);

    let mut ordinal_to_name: std::collections::HashMap<usize, String> =
        std::collections::HashMap::with_capacity(num_names);
    if let (Some(n_off), Some(o_off)) = (names_off, ords_off) {
        for i in 0..num_names {
            let nr_off = n_off + i * 4;
            let or_off = o_off + i * 2;
            if nr_off + 4 > bytes.len() || or_off + 2 > bytes.len() {
                break;
            }
            let sym_name_rva = read_u32_le(bytes, nr_off)?;
            let sym_ord_idx = read_u16_le(bytes, or_off)? as usize;
            if let Some(s_off) = rva_to_file_offset(sym_name_rva, &tables.sections) {
                if let Some(sym_name) = read_c_string(bytes, s_off) {
                    if sym_ord_idx < num_funcs {
                        ordinal_to_name.entry(sym_ord_idx).or_insert_with(|| sym_name.clone());
                    }
                    info.exported_names.push(sym_name);
                }
            }
        }
    }

    let export_end = export_rva.saturating_add(export_size);
    if let Some(f_off) = funcs_off {
        for idx in 0..num_funcs {
            let entry_off = f_off + idx * 4;
            if entry_off + 4 > bytes.len() {
                break;
            }
            let fn_rva = read_u32_le(bytes, entry_off)?;
            if fn_rva == 0 {
                continue;
            }
            let forwarder = if fn_rva >= export_rva && fn_rva < export_end {
                rva_to_file_offset(fn_rva, &tables.sections)
                    .and_then(|fo| read_c_string(bytes, fo))
            } else {
                None
            };
            info.symbols.push(PeExportSymbol {
                ordinal: ordinal_base.saturating_add(idx as u32),
                name: ordinal_to_name.get(&idx).cloned(),
                forwarder,
                rva: fn_rva,
            });
        }
    }

    Ok(info)
}

pub fn parse_pe_exports_file(path: &Path) -> Result<PeExportInfo, String> {
    let file = File::open(path).map_err(|e| format!("Failed opening {}: {e}", path.display()))?;
    let mmap = unsafe {
        Mmap::map(&file).map_err(|e| format!("Failed mapping {}: {e}", path.display()))?
    };
    parse_pe_exports(&mmap)
}

pub fn parse_pe_machine_and_imports(bytes: &[u8]) -> Result<(u16, bool, Vec<String>), String> {
    let (machine, _, is_32bit, dlls) = parse_pe_header_and_imports(bytes)?;
    Ok((machine, is_32bit, dlls))
}

/// Parses imported DLLs and imported Win32/native API function names (`IMAGE_IMPORT_BY_NAME`) for PE32/PE32+.
pub fn parse_pe_import_functions(
    bytes: &[u8],
) -> Result<(u16, bool, Vec<String>, Vec<String>), String> {
    let tables = parse_pe_tables(bytes)?;
    let mut dlls = Vec::new();
    let mut seen_dlls = HashSet::new();
    let mut funcs = Vec::new();
    let mut seen_funcs = HashSet::new();

    if tables.rva_count >= 2 && tables.dd_base + 16 <= tables.opt_end {
        let import_rva = read_u32_le(bytes, tables.dd_base + 8)?;
        let import_size = read_u32_le(bytes, tables.dd_base + 12)?;
        if import_rva != 0 && import_size != 0 {
            if let Some(mut desc_off) = rva_to_file_offset(import_rva, &tables.sections) {
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
                        if let Some(name_off) = rva_to_file_offset(name_rva, &tables.sections) {
                            if let Some(name) = read_c_string(bytes, name_off) {
                                if seen_dlls.insert(name.to_ascii_lowercase()) {
                                    dlls.push(name);
                                }
                            }
                        }
                    }

                    let thunk_rva = if orig_first_thunk != 0 {
                        orig_first_thunk
                    } else {
                        first_thunk
                    };
                    if thunk_rva != 0 && funcs.len() < 4096 {
                        if let Some(mut t_off) = rva_to_file_offset(thunk_rva, &tables.sections) {
                            let step = if tables.is_32bit { 4usize } else { 8usize };
                            for _ in 0..1024 {
                                if t_off + step > bytes.len() || funcs.len() >= 4096 {
                                    break;
                                }
                                let raw_entry = if tables.is_32bit {
                                    read_u32_le(bytes, t_off)? as u64
                                } else {
                                    read_u64_le(bytes, t_off)?
                                };
                                if raw_entry == 0 {
                                    break;
                                }
                                let is_ordinal = if tables.is_32bit {
                                    (raw_entry & 0x8000_0000) != 0
                                } else {
                                    (raw_entry & 0x8000_0000_0000_0000) != 0
                                };
                                if !is_ordinal {
                                    let hint_name_rva = (raw_entry & 0x7FFF_FFFF) as u32;
                                    if let Some(hn_off) =
                                        rva_to_file_offset(hint_name_rva, &tables.sections)
                                    {
                                        if hn_off + 2 < bytes.len() {
                                            if let Some(fn_name) = read_c_string(bytes, hn_off + 2) {
                                                if seen_funcs.insert(fn_name.to_ascii_lowercase()) {
                                                    funcs.push(fn_name);
                                                }
                                            }
                                        }
                                    }
                                }
                                t_off += step;
                            }
                        }
                    }
                    desc_off += 20;
                }
            }
        }
    }

    Ok((tables.machine, tables.is_32bit, dlls, funcs))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeSecurityFinding {
    pub code: String,
    pub severity: String,
    pub title: String,
    pub explanation: String,
    pub indicators: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PeSecurityShieldResult {
    pub shield_badge: String,
    pub authenticode_status: String,
    pub authenticode_detail: String,
    pub imported_api_hits: Vec<String>,
    pub suspicious_behaviors: Vec<PeSecurityFinding>,
    pub quarantine_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ThreatFeedLookupResult {
    pub sha256: String,
    pub provider: String,
    pub matched: bool,
    pub classification: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryShieldSummary {
    pub scanned_files: usize,
    pub clean_files: Vec<PathBuf>,
    pub warning_files: Vec<PathBuf>,
    pub blocked_files: Vec<(PathBuf, String, Option<PathBuf>)>,
}

#[derive(Default)]
struct BinaryStringIndicators {
    has_virtual_alloc_ex: bool,
    has_write_process_memory: bool,
    has_create_remote_thread: bool,
    has_crypt_unprotect: bool,
    has_lsass_or_credential_theft: bool,
    has_hidden_script_dropper: bool,
    has_reg_persistence: bool,
    has_shell_exec_or_proc: bool,
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
}

fn scan_binary_string_indicators(
    bytes: &[u8],
    imported_funcs_lower: &[String],
) -> BinaryStringIndicators {
    let mut flags = BinaryStringIndicators::default();
    let has_imp = |needle: &str| imported_funcs_lower.iter().any(|f| f == needle);
    let has_imp_prefix = |prefix: &str| imported_funcs_lower.iter().any(|f| f.starts_with(prefix));

    if has_imp("virtualallocex") {
        flags.has_virtual_alloc_ex = true;
    }
    if has_imp("writeprocessmemory") {
        flags.has_write_process_memory = true;
    }
    if has_imp("createremotethread")
        || has_imp("createremotethreadex")
        || has_imp("ntcreatethreadex")
        || has_imp("queueuserapc")
    {
        flags.has_create_remote_thread = true;
    }
    if has_imp("cryptunprotectdata") {
        flags.has_crypt_unprotect = true;
    }
    if has_imp_prefix("shellexecute") || has_imp_prefix("createprocess") || has_imp("winexec") {
        flags.has_shell_exec_or_proc = true;
    }
    let has_reg_write_import = has_imp_prefix("regsetvalue") || has_imp_prefix("regcreatekey");

    const MAX_SCAN_BYTES: usize = 24 * 1024 * 1024;
    const CHUNK_LEN: usize = 64 * 1024;
    const OVERLAP: usize = 128;
    let slice = &bytes[..bytes.len().min(MAX_SCAN_BYTES)];
    let mut ascii_buf = Vec::with_capacity(CHUNK_LEN + OVERLAP * 2);
    let mut utf16_buf = Vec::with_capacity(CHUNK_LEN + OVERLAP * 2);
    let mut start = 0usize;
    let mut has_run_key_str = false;

    while start < slice.len() {
        let win_start = start.saturating_sub(OVERLAP);
        let end = (start + CHUNK_LEN).min(slice.len());
        let chunk = &slice[win_start..end];

        ascii_buf.clear();
        for &b in chunk {
            ascii_buf.push(if b.is_ascii_graphic() || b == b' ' {
                b.to_ascii_lowercase()
            } else {
                b' '
            });
        }

        utf16_buf.clear();
        let mut idx = 0usize;
        while idx + 1 < chunk.len() {
            let b0 = chunk[idx];
            let b1 = chunk[idx + 1];
            if (b0.is_ascii_graphic() || b0 == b' ') && b1 == 0 {
                utf16_buf.push(b0.to_ascii_lowercase());
                idx += 2;
            } else {
                utf16_buf.push(b' ');
                idx += 1;
            }
        }

        for view in [&ascii_buf, &utf16_buf] {
            if !flags.has_virtual_alloc_ex && contains_subslice(view, b"virtualallocex") {
                flags.has_virtual_alloc_ex = true;
            }
            if !flags.has_write_process_memory && contains_subslice(view, b"writeprocessmemory") {
                flags.has_write_process_memory = true;
            }
            if !flags.has_create_remote_thread
                && (contains_subslice(view, b"createremotethread")
                    || contains_subslice(view, b"ntcreatethreadex"))
            {
                flags.has_create_remote_thread = true;
            }
            if !flags.has_crypt_unprotect && contains_subslice(view, b"cryptunprotectdata") {
                flags.has_crypt_unprotect = true;
            }
            if !flags.has_lsass_or_credential_theft
                && (contains_subslice(view, b"lsass.exe")
                    || contains_subslice(view, b"minidumpwritedump")
                    || contains_subslice(view, b"chrome\\user data")
                    || contains_subslice(view, b"login data")
                    || contains_subslice(view, b"cookies.sqlite")
                    || contains_subslice(view, b"local state")
                    || contains_subslice(view, b"vaultcli.dll"))
            {
                flags.has_lsass_or_credential_theft = true;
            }
            if !flags.has_hidden_script_dropper
                && (contains_subslice(view, b"powershell -enc")
                    || contains_subslice(view, b"-encodedcommand")
                    || contains_subslice(view, b"powershell -w hidden")
                    || contains_subslice(view, b"-windowstyle hidden")
                    || contains_subslice(view, b"cmd /c powershell")
                    || contains_subslice(view, b"cmd.exe /c powershell")
                    || contains_subslice(view, b"certutil -urlcache")
                    || contains_subslice(view, b"bitsadmin /transfer")
                    || contains_subslice(view, b"mshta http"))
            {
                flags.has_hidden_script_dropper = true;
            }
            if !has_run_key_str
                && (contains_subslice(view, b"currentversion\\run")
                    || contains_subslice(view, b"currentversion\\runonce"))
            {
                has_run_key_str = true;
            }
            if !flags.has_shell_exec_or_proc
                && (contains_subslice(view, b"shellexecute")
                    || contains_subslice(view, b"createprocess"))
            {
                flags.has_shell_exec_or_proc = true;
            }
        }

        if end == slice.len() {
            break;
        }
        start = end;
    }

    flags.has_reg_persistence =
        has_run_key_str && (has_reg_write_import || flags.has_shell_exec_or_proc);
    flags
}

/// Evaluates static PE security findings cleanly without filesystem/Win32 side effects (unit-testable).
pub fn evaluate_pe_security_shield_bytes(
    bytes: &[u8],
    authenticode_status: &str,
    authenticode_detail: &str,
    shannon_entropy: f64,
    is_whitelisted_game_wrapper: bool,
) -> PeSecurityShieldResult {
    let parsed_funcs = parse_pe_import_functions(bytes)
        .map(|(_, _, _, f)| f)
        .unwrap_or_default();
    let imported_lower: Vec<String> = parsed_funcs
        .iter()
        .map(|f| f.to_ascii_lowercase())
        .collect();
    let flags = scan_binary_string_indicators(bytes, &imported_lower);

    let interesting = [
        "virtualallocex",
        "writeprocessmemory",
        "createremotethread",
        "createremotethreadex",
        "ntcreatethreadex",
        "cryptunprotectdata",
        "regsetvalueex",
        "shellexecute",
        "createprocess",
    ];
    let mut imported_api_hits = Vec::new();
    for fn_name in &parsed_funcs {
        let lower = fn_name.to_ascii_lowercase();
        if interesting.iter().any(|k| lower.starts_with(k)) && imported_api_hits.len() < 16 {
            imported_api_hits.push(fn_name.clone());
        }
    }

    let is_signed_valid = authenticode_status == "SignedVerified";
    let mut findings = Vec::new();
    let mut blocked_score = 0u32;
    let mut warning_score = 0u32;

    if flags.has_virtual_alloc_ex && flags.has_write_process_memory && flags.has_create_remote_thread
    {
        if is_signed_valid {
            warning_score += 1;
            findings.push(PeSecurityFinding {
                code: "PROCESS_INJECTION_SIGNED".to_string(),
                severity: "Warning".to_string(),
                title: "Remote Process Memory/Thread APIs (Signed Binary)".to_string(),
                explanation: "This binary imports VirtualAllocEx, WriteProcessMemory, and CreateRemoteThread, typically used by overlays or anti-cheat/mod loaders, but carries a valid Authenticode signature.".to_string(),
                indicators: vec![
                    "VirtualAllocEx".to_string(),
                    "WriteProcessMemory".to_string(),
                    "CreateRemoteThread".to_string(),
                ],
            });
        } else {
            blocked_score += 2;
            findings.push(PeSecurityFinding {
                code: "PROCESS_INJECTION_TRIAD".to_string(),
                severity: "Blocked".to_string(),
                title: "Process Hollowing / Remote Code Injection Triad".to_string(),
                explanation: "Imports the classic VirtualAllocEx + WriteProcessMemory + CreateRemoteThread injection combination in an unsigned PE binary.".to_string(),
                indicators: vec![
                    "VirtualAllocEx".to_string(),
                    "WriteProcessMemory".to_string(),
                    "CreateRemoteThread".to_string(),
                ],
            });
        }
    } else if flags.has_virtual_alloc_ex && flags.has_write_process_memory && !is_signed_valid {
        warning_score += 1;
        findings.push(PeSecurityFinding {
            code: "MEMORY_WRITE_PAIR".to_string(),
            severity: "Warning".to_string(),
            title: "Cross-Process Memory Allocation & Write APIs".to_string(),
            explanation: "Unsigned binary contains VirtualAllocEx and WriteProcessMemory imports; treat unknown trainers/mods carefully.".to_string(),
            indicators: vec!["VirtualAllocEx".to_string(), "WriteProcessMemory".to_string()],
        });
    }

    if flags.has_crypt_unprotect && flags.has_lsass_or_credential_theft {
        if !is_signed_valid {
            blocked_score += 2;
            findings.push(PeSecurityFinding {
                code: "CREDENTIAL_LSASS_STEALER".to_string(),
                severity: "Blocked".to_string(),
                title: "Credential Store / LSASS Dump Risk".to_string(),
                explanation: "Combines Windows DPAPI CryptUnprotectData with browser credential stores or LSASS dumping indicators.".to_string(),
                indicators: vec![
                    "CryptUnprotectData".to_string(),
                    "Browser/LSASS Access".to_string(),
                ],
            });
        } else {
            warning_score += 1;
            findings.push(PeSecurityFinding {
                code: "DPAPI_SIGNED_WARNING".to_string(),
                severity: "Warning".to_string(),
                title: "DPAPI + Browser Engine References".to_string(),
                explanation: "Signed executable references CryptUnprotectData and local storage databases.".to_string(),
                indicators: vec!["CryptUnprotectData".to_string()],
            });
        }
    } else if flags.has_lsass_or_credential_theft && !is_signed_valid && !is_whitelisted_game_wrapper
    {
        blocked_score += 2;
        findings.push(PeSecurityFinding {
            code: "CREDENTIAL_THEFT_STRINGS".to_string(),
            severity: "Blocked".to_string(),
            title: "Sensitive Credential Store / LSASS Indicator".to_string(),
            explanation: "Unsigned binary references LSASS memory or browser credential store targets.".to_string(),
            indicators: vec!["LSASS / Browser Profile Target".to_string()],
        });
    } else if flags.has_crypt_unprotect && !is_signed_valid && !is_whitelisted_game_wrapper {
        warning_score += 1;
        findings.push(PeSecurityFinding {
            code: "DPAPI_UNPROTECT_UNSIGNED".to_string(),
            severity: "Warning".to_string(),
            title: "Uses DPAPI Secret Decryption (CryptUnprotectData)".to_string(),
            explanation: "Games rarely decrypt OS user secrets directly; review unknown launchers or mods inside an AppContainer Jail.".to_string(),
            indicators: vec!["CryptUnprotectData".to_string()],
        });
    }

    if flags.has_hidden_script_dropper {
        blocked_score += 2;
        findings.push(PeSecurityFinding {
            code: "HIDDEN_SCRIPT_DROPPER".to_string(),
            severity: "Blocked".to_string(),
            title: "Hidden PowerShell / Script Dropper Command".to_string(),
            explanation: "Embedded hidden or Base64-encoded PowerShell/cmd/certutil execution patterns were found inside the binary.".to_string(),
            indicators: vec!["Hidden/Encoded Script Dropper".to_string()],
        });
    }

    if flags.has_reg_persistence {
        if !is_signed_valid {
            blocked_score += 2;
            findings.push(PeSecurityFinding {
                code: "REGISTRY_RUN_PERSISTENCE".to_string(),
                severity: "Blocked".to_string(),
                title: "Windows Startup Registry Run Persistence".to_string(),
                explanation: "Unsigned binary modifies CurrentVersion\\Run startup keys combined with registry or process execution APIs.".to_string(),
                indicators: vec!["CurrentVersion\\Run".to_string()],
            });
        } else {
            warning_score += 1;
            findings.push(PeSecurityFinding {
                code: "REGISTRY_RUN_SIGNED".to_string(),
                severity: "Warning".to_string(),
                title: "Startup Registry Key Reference (Signed)".to_string(),
                explanation: "Binary references CurrentVersion\\Run; typical only for updaters or background launchers.".to_string(),
                indicators: vec!["CurrentVersion\\Run".to_string()],
            });
        }
    }

    if authenticode_status.starts_with("Invalid") {
        blocked_score += 2;
        findings.push(PeSecurityFinding {
            code: "INVALID_AUTHENTICODE".to_string(),
            severity: "Blocked".to_string(),
            title: "Corrupted or Tampered Digital Signature".to_string(),
            explanation: authenticode_detail.to_string(),
            indicators: vec![authenticode_status.to_string()],
        });
    }

    if shannon_entropy >= 7.50 && !is_signed_valid && !is_whitelisted_game_wrapper {
        warning_score += 1;
        findings.push(PeSecurityFinding {
            code: "HIGH_ENTROPY_UNSIGNED".to_string(),
            severity: "Warning".to_string(),
            title: "Packed or Encrypted Binary Payload".to_string(),
            explanation: format!(
                "Unsigned binary exhibits high Shannon entropy ({:.2} b/B). Consider running in AppContainer Jail.",
                shannon_entropy
            ),
            indicators: vec![format!("Entropy {:.2} b/B", shannon_entropy)],
        });
    }

    let shield_badge = if blocked_score >= 2 {
        "Blocked".to_string()
    } else if warning_score > 0 || authenticode_status == "Unsigned" && !findings.is_empty() {
        "Warning".to_string()
    } else {
        "Safe".to_string()
    };

    PeSecurityShieldResult {
        shield_badge,
        authenticode_status: authenticode_status.to_string(),
        authenticode_detail: authenticode_detail.to_string(),
        imported_api_hits,
        suspicious_behaviors: findings,
        quarantine_path: None,
    }
}

#[cfg(target_os = "windows")]
pub fn verify_authenticode_winverifytrust(path: &Path) -> (String, String) {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;

    #[repr(C)]
    #[allow(non_snake_case)]
    struct WinTrustGuid {
        Data1: u32,
        Data2: u16,
        Data3: u16,
        Data4: [u8; 8],
    }

    #[repr(C)]
    #[allow(non_snake_case)]
    struct WINTRUST_FILE_INFO {
        cbStruct: u32,
        pcwszFilePath: *const u16,
        hFile: *mut c_void,
        pgKnownSubject: *const WinTrustGuid,
    }

    #[repr(C)]
    #[allow(non_snake_case)]
    struct WINTRUST_DATA {
        cbStruct: u32,
        pPolicyCallbackData: *mut c_void,
        pSIPClientData: *mut c_void,
        dwUIChoice: u32,
        fdwRevocationChecks: u32,
        dwUnionChoice: u32,
        pFile: *mut WINTRUST_FILE_INFO,
        dwStateAction: u32,
        hWVTStateData: *mut c_void,
        pwszURLReference: *const u16,
        dwProvFlags: u32,
        dwUIContext: u32,
        pSignatureSettings: *mut c_void,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryW(lpLibFileName: *const u16) -> isize;
        fn GetProcAddress(hModule: isize, lpProcName: *const u8) -> *const c_void;
        fn FreeLibrary(hLibModule: isize) -> i32;
    }

    type FnWinVerifyTrust = unsafe extern "system" fn(
        hwnd: *mut c_void,
        pg_action_id: *const WinTrustGuid,
        p_wvt_data: *mut WINTRUST_DATA,
    ) -> i32;

    let dll_w: Vec<u16> = std::ffi::OsStr::new("wintrust.dll\0")
        .encode_wide()
        .collect();
    let hmod = unsafe { LoadLibraryW(dll_w.as_ptr()) };
    if hmod == 0 {
        return (
            "CheckUnavailable".to_string(),
            "wintrust.dll could not be loaded on this system".to_string(),
        );
    }

    let proc_ptr = unsafe { GetProcAddress(hmod, c"WinVerifyTrust".as_ptr().cast::<u8>()) };
    if proc_ptr.is_null() {
        unsafe {
            FreeLibrary(hmod);
        }
        return (
            "CheckUnavailable".to_string(),
            "WinVerifyTrust export unavailable in wintrust.dll".to_string(),
        );
    }

    let win_verify_trust: FnWinVerifyTrust = unsafe { std::mem::transmute(proc_ptr) };
    let file_w: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let action_guid = WinTrustGuid {
        Data1: 0x00AA_C56B,
        Data2: 0xCD44,
        Data3: 0x11D0,
        Data4: [0x8C, 0xC2, 0x00, 0xC0, 0x4F, 0xC2, 0x95, 0xEE],
    };

    let mut file_info = WINTRUST_FILE_INFO {
        cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: file_w.as_ptr(),
        hFile: std::ptr::null_mut(),
        pgKnownSubject: std::ptr::null(),
    };

    const WTD_UI_NONE: u32 = 2;
    const WTD_REVOKE_NONE: u32 = 0;
    const WTD_CHOICE_FILE: u32 = 1;
    const WTD_STATEACTION_VERIFY: u32 = 1;
    const WTD_STATEACTION_CLOSE: u32 = 2;
    const WTD_CACHE_ONLY_URL_RETRIEVAL: u32 = 0x0000_1000;

    let mut wvt_data = WINTRUST_DATA {
        cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
        pPolicyCallbackData: std::ptr::null_mut(),
        pSIPClientData: std::ptr::null_mut(),
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        pFile: &mut file_info,
        dwStateAction: WTD_STATEACTION_VERIFY,
        hWVTStateData: std::ptr::null_mut(),
        pwszURLReference: std::ptr::null(),
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL,
        dwUIContext: 0,
        pSignatureSettings: std::ptr::null_mut(),
    };

    let status_i32 = unsafe {
        win_verify_trust(
            std::ptr::null_mut(),
            &action_guid,
            &mut wvt_data,
        )
    };
    wvt_data.dwStateAction = WTD_STATEACTION_CLOSE;
    let _ = unsafe { win_verify_trust(std::ptr::null_mut(), &action_guid, &mut wvt_data) };
    unsafe {
        FreeLibrary(hmod);
    }

    let status_u32 = status_i32 as u32;
    match status_u32 {
        0x0000_0000 => (
            "SignedVerified".to_string(),
            "Authenticode digital signature verified via WinVerifyTrust.".to_string(),
        ),
        0x800B_0100 => (
            "Unsigned".to_string(),
            "File has no embedded Authenticode digital signature.".to_string(),
        ),
        0x8009_6010 | 0x800B_0109 | 0x800B_0101 | 0x800B_010C => (
            "InvalidSignature".to_string(),
            format!("Authenticode signature check failed (0x{status_u32:08X})."),
        ),
        other => (
            "Unsigned".to_string(),
            format!("Authenticode unverified or untrusted root (0x{other:08X})."),
        ),
    }
}

#[cfg(not(target_os = "windows"))]
pub fn verify_authenticode_winverifytrust(_path: &Path) -> (String, String) {
    (
        "CheckUnavailable".to_string(),
        "WinVerifyTrust Authenticode verification runs on Windows.".to_string(),
    )
}

/// Inspects a PE file on disk with Security Shield v2 (import table APIs, droppers, Authenticode).
pub fn inspect_pe_security_shield_file(
    path: &Path,
    is_whitelisted_game_wrapper: bool,
    shannon_entropy: Option<f64>,
) -> Result<PeSecurityShieldResult, String> {
    if !path.is_file() {
        return Err(format!("PE file does not exist: {}", path.display()));
    }
    let file = File::open(path).map_err(|e| format!("Failed opening PE {}: {e}", path.display()))?;
    let mmap = unsafe { Mmap::map(&file) }
        .map_err(|e| format!("Failed mapping PE {}: {e}", path.display()))?;
    if mmap.len() < 64 || &mmap[0..2] != b"MZ" {
        return Err("File is not a Windows PE executable/DLL.".to_string());
    }
    let (auth_status, auth_detail) = verify_authenticode_winverifytrust(path);
    let entropy = shannon_entropy.unwrap_or_else(|| {
        let sample = &mmap[..mmap.len().min(2 * 1024 * 1024)];
        if sample.is_empty() {
            return 0.0;
        }
        let mut counts = [0u64; 256];
        for &b in sample {
            counts[b as usize] += 1;
        }
        let total = sample.len() as f64;
        counts
            .iter()
            .filter(|&&c| c > 0)
            .map(|&c| {
                let p = (c as f64) / total;
                -p * p.log2()
            })
            .sum()
    });
    Ok(evaluate_pe_security_shield_bytes(
        &mmap,
        &auth_status,
        &auth_detail,
        entropy,
        is_whitelisted_game_wrapper,
    ))
}

/// Moves a high-risk binary into `<quarantine_dir>` with a sidecar metadata file.
pub fn quarantine_high_risk_binary(
    target: &Path,
    quarantine_dir: &Path,
    reasons: &[String],
    sha256: Option<&str>,
) -> Result<PathBuf, String> {
    if !target.is_file() {
        return Err(format!("Target missing for quarantine: {}", target.display()));
    }
    fs::create_dir_all(quarantine_dir)
        .map_err(|e| format!("Failed creating quarantine dir {}: {e}", quarantine_dir.display()))?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let base_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("binary.bin");
    let safe_name: String = base_name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let dest = quarantine_dir.join(format!("{}_{}.quarantined", ts, safe_name));
    if fs::rename(target, &dest).is_err() {
        fs::copy(target, &dest)
            .map_err(|e| format!("Failed copying threat to quarantine {}: {e}", dest.display()))?;
        fs::remove_file(target)
            .map_err(|e| format!("Failed removing original after quarantine copy: {e}"))?;
    }
    let meta = serde_json::json!({
        "originalPath": target.to_string_lossy(),
        "quarantinedPath": dest.to_string_lossy(),
        "sha256": sha256.unwrap_or(""),
        "quarantinedAt": ts,
        "reasons": reasons,
    });
    if let Ok(json) = serde_json::to_string_pretty(&meta) {
        let meta_path = quarantine_dir.join(format!("{}_{}.quarantined.json", ts, safe_name));
        let _ = crate::atomic_write_string(&meta_path, &json);
    }
    Ok(dest)
}

/// Inspects extracted/dropped `.exe`, `.dll`, and `.asi` binaries before post-scan MOTW unblocking.
pub fn gate_game_directory_pe_security(
    root: &Path,
    quarantine_dir: Option<&Path>,
    max_files: usize,
) -> DirectoryShieldSummary {
    let mut summary = DirectoryShieldSummary::default();
    if !root.exists() {
        return summary;
    }

    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
    {
        if summary.scanned_files >= max_files {
            break;
        }
        let path = entry.path().to_path_buf();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        if !matches!(ext.as_str(), "exe" | "dll" | "asi") {
            continue;
        }
        summary.scanned_files += 1;
        let file_lower = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        let is_wrapper = matches!(
            file_lower.as_str(),
            "steam_api.dll" | "steam_api64.dll" | "winmm.dll" | "d3d9.dll" | "dxgi.dll" | "dinput8.dll" | "xinput1_3.dll"
        );
        match inspect_pe_security_shield_file(&path, is_wrapper, None) {
            Ok(res) => match res.shield_badge.as_str() {
                "Blocked" => {
                    let reasons: Vec<String> = res
                        .suspicious_behaviors
                        .iter()
                        .map(|f| format!("{}: {}", f.title, f.explanation))
                        .collect();
                    let quarantined = quarantine_dir
                        .and_then(|qdir| quarantine_high_risk_binary(&path, qdir, &reasons, None).ok());
                    let msg = reasons.join(" | ");
                    summary.blocked_files.push((path, msg, quarantined));
                }
                "Warning" => summary.warning_files.push(path),
                _ => summary.clean_files.push(path),
            },
            Err(_) => {
                // Non-PE or zeroed stub files inside game folders are left untouched
            }
        }
    }

    summary
}

/// Optional privacy-safe read-only SHA-256 lookup against CIRCL hashLookup (never uploads file bytes).
pub async fn query_sha256_threat_feed(sha256_hex: &str) -> Result<ThreatFeedLookupResult, String> {
    let normalized = sha256_hex.trim().to_ascii_lowercase();
    if normalized.len() != 64 || !normalized.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("Invalid 64-character SHA-256 hash".to_string());
    }
    let client = duke_core::net_doh::get_configured_http_client();
    let url = format!("https://hashlookup.circl.lu/lookup/sha256/{}", normalized);
    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .timeout(std::time::Duration::from_secs(6))
        .send()
        .await
        .map_err(|e| format!("Threat feed lookup failed: {e}"))?;

    if resp.status().as_u16() == 404 {
        return Ok(ThreatFeedLookupResult {
            sha256: normalized,
            provider: "CIRCL hashLookup (read-only SHA-256)".to_string(),
            matched: false,
            classification: "Not listed".to_string(),
            detail: "Hash not present in known public threat or reference dataset.".to_string(),
        });
    }
    if !resp.status().is_success() {
        return Err(format!("Threat feed HTTP status {}", resp.status()));
    }

    let val: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Threat feed returned unreadable JSON: {e}"))?;
    let known_malicious = val
        .get("KnownMalicious")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let file_name = val
        .get("FileName")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown");
    let product = val
        .get("ProductName")
        .and_then(|v| v.as_str())
        .unwrap_or("Reference Dataset");
    if let Some(mal) = known_malicious {
        Ok(ThreatFeedLookupResult {
            sha256: normalized,
            provider: "CIRCL hashLookup (read-only SHA-256)".to_string(),
            matched: true,
            classification: format!("Malicious ({mal})"),
            detail: format!("Matched threat source '{}' (file reference: {}).", mal, file_name),
        })
    } else {
        Ok(ThreatFeedLookupResult {
            sha256: normalized,
            provider: "CIRCL hashLookup (read-only SHA-256)".to_string(),
            matched: true,
            classification: "Known Reference / NSRL".to_string(),
            detail: format!("Known hash record: {} ({}).", file_name, product),
        })
    }
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GameCompatStatus {
    pub game_id: String,
    pub exe_path: String,
    pub is_32bit: bool,
    pub large_address_aware: bool,
    pub laa_eligible: bool,
    pub legacy_dx_imports: Vec<String>,
    pub dxvk_recommended_dlls: Vec<String>,
    pub dxvk_deployed: bool,
    pub dxvk_deployed_dlls: Vec<String>,
    pub dxvk_source_dir: String,
    pub dxvk_source_ready: bool,
    pub dxvk_available_dlls: Vec<String>,
    pub last_snapshot_id: Option<String>,
    pub status_message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct DxvkDeployManifest {
    deployed_dlls: Vec<String>,
    backed_up_originals: Vec<String>,
}

pub fn detect_legacy_dx_apis(imports: &[String]) -> (Vec<String>, Vec<String>) {
    let mut apis: Vec<String> = Vec::new();
    let mut rec_dlls: Vec<String> = Vec::new();
    let push_unique = |list: &mut Vec<String>, item: &str| {
        if !list.iter().any(|x| x.eq_ignore_ascii_case(item)) {
            list.push(item.to_string());
        }
    };

    for dll in imports {
        let lower = dll.to_ascii_lowercase();
        if lower == "d3d8.dll" || lower.starts_with("d3dx8") {
            push_unique(&mut apis, "DirectX 8");
            push_unique(&mut rec_dlls, "d3d8.dll");
            push_unique(&mut rec_dlls, "d3d9.dll");
        } else if lower == "d3d9.dll" || lower.starts_with("d3dx9") {
            push_unique(&mut apis, "DirectX 9");
            push_unique(&mut rec_dlls, "d3d9.dll");
        } else if lower == "d3d10.dll" || lower == "d3d10_1.dll" || lower.starts_with("d3dx10") {
            push_unique(&mut apis, "DirectX 10");
            push_unique(&mut rec_dlls, "d3d10core.dll");
            push_unique(&mut rec_dlls, "dxgi.dll");
        } else if lower == "d3d11.dll" || lower.starts_with("d3dx11") {
            push_unique(&mut apis, "DirectX 11");
            push_unique(&mut rec_dlls, "d3d11.dll");
            push_unique(&mut rec_dlls, "dxgi.dll");
        } else if lower == "dxgi.dll" {
            push_unique(&mut apis, "DXGI");
            push_unique(&mut rec_dlls, "dxgi.dll");
        }
    }
    (apis, rec_dlls)
}

fn compute_pe_checksum(bytes: &[u8], checksum_offset: usize) -> u32 {
    let mut sum: u64 = 0;
    let mut idx = 0usize;
    while idx + 1 < bytes.len() {
        if idx == checksum_offset || idx == checksum_offset + 2 {
            idx += 2;
            continue;
        }
        let word = u16::from_le_bytes([bytes[idx], bytes[idx + 1]]) as u64;
        sum += word;
        sum = (sum & 0xFFFF) + (sum >> 16);
        idx += 2;
    }
    if idx < bytes.len() {
        sum += bytes[idx] as u64;
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    sum = (sum & 0xFFFF) + (sum >> 16);
    (sum as u32).wrapping_add(bytes.len() as u32)
}

/// Patches or clears `IMAGE_FILE_LARGE_ADDRESS_AWARE` (`0x0020`) in a 32-bit PE32 header in place.
pub fn set_pe_large_address_aware(exe_path: &Path, enable: bool) -> Result<bool, String> {
    let mut bytes = fs::read(exe_path)
        .map_err(|e| format!("Failed to read executable {}: {}", exe_path.display(), e))?;
    let (_, _, is_32bit, _) = parse_pe_header_and_imports(&bytes)?;
    if !is_32bit {
        return Err("64-bit PE32+ executables already address >4 GB memory".to_string());
    }

    let e_lfanew = read_u32_le(&bytes, 0x3C)? as usize;
    let coff_off = e_lfanew + 4;
    let char_off = coff_off + 18;
    let mut characteristics = read_u16_le(&bytes, char_off)?;
    if enable {
        characteristics |= IMAGE_FILE_LARGE_ADDRESS_AWARE;
    } else {
        characteristics &= !IMAGE_FILE_LARGE_ADDRESS_AWARE;
    }
    bytes[char_off..char_off + 2].copy_from_slice(&characteristics.to_le_bytes());

    let opt_off = coff_off + 20;
    let checksum_off = opt_off + 64;
    if checksum_off + 4 <= bytes.len() {
        let old_csum = read_u32_le(&bytes, checksum_off)?;
        if old_csum != 0 {
            let new_csum = compute_pe_checksum(&bytes, checksum_off);
            bytes[checksum_off..checksum_off + 4].copy_from_slice(&new_csum.to_le_bytes());
        }
    }

    fs::write(exe_path, &bytes)
        .map_err(|e| format!("Failed to write patched PE {}: {}", exe_path.display(), e))?;
    Ok((characteristics & IMAGE_FILE_LARGE_ADDRESS_AWARE) != 0)
}

fn find_dxvk_source_dir_and_dlls(
    wrappers_root: &Path,
    is_32bit: bool,
) -> (PathBuf, Vec<PathBuf>) {
    let arch_sub = if is_32bit { "x32" } else { "x64" };
    let arch_dir = wrappers_root.join("dxvk").join(arch_sub);
    let fallback_dir = wrappers_root.join("dxvk");
    let chosen_dir = if arch_dir.is_dir() {
        arch_dir
    } else {
        fallback_dir
    };

    let mut available = Vec::new();
    if chosen_dir.is_dir() {
        for &dll in DXVK_WRAPPER_DLLS {
            if let Some(found) = find_case_insensitive_file(&chosen_dir, dll) {
                if validate_dll_architecture(&found, is_32bit) {
                    available.push(found);
                }
            }
        }
    }
    (chosen_dir, available)
}

pub fn inspect_compat_with_dirs(
    game_id: &str,
    exe_path: &Path,
    wrappers_root: &Path,
    last_snapshot_id: Option<String>,
) -> Result<GameCompatStatus, String> {
    let file = File::open(exe_path)
        .map_err(|e| format!("Failed to open executable {}: {}", exe_path.display(), e))?;
    let mmap = unsafe { Mmap::map(&file) }
        .map_err(|e| format!("Failed to mmap executable {}: {}", exe_path.display(), e))?;
    let (_, characteristics, is_32bit, imports) = parse_pe_header_and_imports(&mmap)?;
    drop(mmap);
    drop(file);

    let large_address_aware = (characteristics & IMAGE_FILE_LARGE_ADDRESS_AWARE) != 0;
    let (legacy_dx_imports, dxvk_recommended_dlls) = detect_legacy_dx_apis(&imports);
    let exe_dir = exe_path.parent().unwrap_or_else(|| Path::new("."));

    let manifest_path = exe_dir.join(WRAPPERS_MANIFEST_FILE);
    let manifest_dlls: Vec<String> = fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|raw| serde_json::from_str::<DxvkDeployManifest>(&raw).ok())
        .map(|m| m.deployed_dlls)
        .unwrap_or_default();

    let mut deployed_dlls: Vec<String> = Vec::new();
    for &dll in DXVK_WRAPPER_DLLS {
        if find_case_insensitive_file(exe_dir, dll).is_some()
            || manifest_dlls.iter().any(|m| m.eq_ignore_ascii_case(dll))
        {
            deployed_dlls.push(dll.to_string());
        }
    }

    let (source_dir, available_paths) = find_dxvk_source_dir_and_dlls(wrappers_root, is_32bit);
    let dxvk_available_dlls: Vec<String> = available_paths
        .iter()
        .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(ToString::to_string))
        .collect();
    let dxvk_source_ready = !dxvk_available_dlls.is_empty();
    let dxvk_deployed = !deployed_dlls.is_empty();

    let status_message = if !dxvk_source_ready && !dxvk_deployed {
        format!(
            "Place matching DXVK DLLs in {} to enable 1-click Vulkan translation.",
            source_dir.display()
        )
    } else {
        "Compatibility inspection ready.".to_string()
    };

    Ok(GameCompatStatus {
        game_id: game_id.to_string(),
        exe_path: exe_path.to_string_lossy().into_owned(),
        is_32bit,
        large_address_aware,
        laa_eligible: is_32bit,
        legacy_dx_imports,
        dxvk_recommended_dlls,
        dxvk_deployed,
        dxvk_deployed_dlls: deployed_dlls,
        dxvk_source_dir: source_dir.to_string_lossy().into_owned(),
        dxvk_source_ready,
        dxvk_available_dlls,
        last_snapshot_id,
        status_message,
    })
}

pub fn deploy_or_remove_dxvk_in_dirs(
    game_id: &str,
    exe_path: &Path,
    wrappers_root: &Path,
    enable: bool,
    last_snapshot_id: Option<String>,
) -> Result<GameCompatStatus, String> {
    let exe_dir = exe_path
        .parent()
        .ok_or_else(|| "Executable has no parent directory".to_string())?;
    let manifest_path = exe_dir.join(WRAPPERS_MANIFEST_FILE);

    if enable {
        let pre = inspect_compat_with_dirs(game_id, exe_path, wrappers_root, None)?;
        let (src_dir, available) = find_dxvk_source_dir_and_dlls(wrappers_root, pre.is_32bit);
        if available.is_empty() {
            return Err(format!(
                "No {} DXVK DLLs found in {}",
                if pre.is_32bit { "32-bit (x32)" } else { "64-bit (x64)" },
                src_dir.display()
            ));
        }
        let target_names: Vec<String> = if pre.dxvk_recommended_dlls.is_empty() {
            available
                .iter()
                .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(ToString::to_string))
                .collect()
        } else {
            let matching: Vec<String> = available
                .iter()
                .filter_map(|p| {
                    let name = p.file_name()?.to_str()?;
                    if pre
                        .dxvk_recommended_dlls
                        .iter()
                        .any(|r| r.eq_ignore_ascii_case(name))
                    {
                        Some(name.to_string())
                    } else {
                        None
                    }
                })
                .collect();
            if matching.is_empty() {
                available
                    .iter()
                    .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(ToString::to_string))
                    .collect()
            } else {
                matching
            }
        };

        let mut manifest = DxvkDeployManifest::default();
        for src in &available {
            let Some(fname) = src.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !target_names.iter().any(|t| t.eq_ignore_ascii_case(fname)) {
                continue;
            }
            let dest = exe_dir.join(fname);
            let orig_bak = exe_dir.join(format!("{fname}.duke-orig"));
            if dest.is_file() && !orig_bak.is_file() {
                let _ = fs::copy(&dest, &orig_bak);
                manifest.backed_up_originals.push(fname.to_string());
            }
            fs::copy(src, &dest)
                .map_err(|e| format!("Failed copying DXVK DLL {}: {}", fname, e))?;
            manifest.deployed_dlls.push(fname.to_string());
        }
        if let Ok(json) = serde_json::to_string_pretty(&manifest) {
            let _ = crate::atomic_write_string(&manifest_path, &json);
        }
    } else {
        let manifest = fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|raw| serde_json::from_str::<DxvkDeployManifest>(&raw).ok());
        let to_remove: Vec<String> = manifest
            .as_ref()
            .map(|m| m.deployed_dlls.clone())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| DXVK_WRAPPER_DLLS.iter().map(|s| (*s).to_string()).collect());
        let backed_up: Vec<String> = manifest
            .map(|m| m.backed_up_originals)
            .unwrap_or_default();

        for dll in &to_remove {
            if let Some(existing) = find_case_insensitive_file(exe_dir, dll) {
                let _ = fs::remove_file(&existing);
            }
            let orig_bak = exe_dir.join(format!("{dll}.duke-orig"));
            if orig_bak.is_file() && backed_up.iter().any(|b| b.eq_ignore_ascii_case(dll)) {
                let _ = fs::rename(&orig_bak, exe_dir.join(dll));
            }
        }
        let _ = fs::remove_file(&manifest_path);
    }

    inspect_compat_with_dirs(game_id, exe_path, wrappers_root, last_snapshot_id)
}

fn resolve_game_exe_and_dir(game_id: &str) -> Result<(crate::GameEntry, PathBuf, PathBuf), String> {
    let library = crate::load_library_standalone()?;
    let game = library
        .into_iter()
        .find(|g| g.id == game_id)
        .ok_or_else(|| "Game not found in library".to_string())?;
    let exe_path = PathBuf::from(game.exe_path.trim());
    if !exe_path.is_file() {
        return Err(format!("Executable does not exist: {}", exe_path.display()));
    }
    let game_dir = if !game.install_dir.trim().is_empty() {
        PathBuf::from(game.install_dir.trim())
    } else {
        crate::resolve_game_root_dir(&exe_path)
    };
    Ok((game, exe_path, game_dir))
}

pub fn quarantine_root_dir_standalone() -> Result<PathBuf, String> {
    Ok(crate::resolve_data_root_dir_standalone()?.join(".duke-quarantine"))
}

pub fn inspect_exe_standalone(exe_path: &Path) -> Result<PePreflightReport, String> {
    inspect_exe_with_dirs(exe_path, None, &[])
}

pub fn heal_dlls_standalone(exe_path: &Path, dll_names: &[String]) -> Result<DllHealReport, String> {
    heal_missing_dlls_into_game_dir(exe_path, Some(dll_names), None, &[])
}

pub fn toggle_dxvk_standalone(
    game_id: &str,
    exe_path: &Path,
    enable: bool,
) -> Result<GameCompatStatus, String> {
    let wrappers_root = crate::resolve_data_root_dir_standalone()?.join("wrappers");
    deploy_or_remove_dxvk_in_dirs(game_id, exe_path, &wrappers_root, enable, None)
}

#[tauri::command]
pub async fn check_sha256_threat_feed(sha256: String) -> Result<ThreatFeedLookupResult, String> {
    query_sha256_threat_feed(&sha256).await
}

#[tauri::command]
pub async fn get_game_compat_status(game_id: String) -> Result<GameCompatStatus, String> {
    tokio::task::spawn_blocking(move || {
        let (game, exe_path, _) = resolve_game_exe_and_dir(&game_id)?;
        let wrappers_root = crate::resolve_data_root_dir_standalone()?.join("wrappers");
        inspect_compat_with_dirs(&game.id, &exe_path, &wrappers_root, None)
    })
    .await
    .map_err(|e| format!("Compat status worker failed: {e}"))?
}

#[tauri::command]
pub async fn toggle_game_laa_patch(
    game_id: String,
    enable: bool,
) -> Result<GameCompatStatus, String> {
    tokio::task::spawn_blocking(move || {
        let (game, exe_path, game_dir) = resolve_game_exe_and_dir(&game_id)?;
        let snap = crate::snapshots::create_snapshot_in_dir(
            &game.id,
            &game_dir,
            &game_dir.join(crate::snapshots::CAS_DIR_NAME),
            Some(if enable {
                "Auto-Backup Before 4GB LAA Patch"
            } else {
                "Auto-Backup Before LAA Revert"
            }),
        )?;
        set_pe_large_address_aware(&exe_path, enable)?;
        let wrappers_root = crate::resolve_data_root_dir_standalone()?.join("wrappers");
        let mut status = inspect_compat_with_dirs(&game.id, &exe_path, &wrappers_root, Some(snap.id))?;
        status.status_message = if status.large_address_aware {
            "4 GB Large Address Aware flag enabled (CAS snapshot saved).".to_string()
        } else {
            "4 GB Large Address Aware flag cleared (CAS snapshot saved).".to_string()
        };
        Ok(status)
    })
    .await
    .map_err(|e| format!("LAA patch worker failed: {e}"))?
}

#[tauri::command]
pub async fn toggle_game_dxvk_wrapper(
    game_id: String,
    enable: bool,
) -> Result<GameCompatStatus, String> {
    tokio::task::spawn_blocking(move || {
        let (game, exe_path, game_dir) = resolve_game_exe_and_dir(&game_id)?;
        let wrappers_root = crate::resolve_data_root_dir_standalone()?.join("wrappers");
        if enable {
            let pre = inspect_compat_with_dirs(&game.id, &exe_path, &wrappers_root, None)?;
            if !pre.dxvk_source_ready {
                return Err(format!(
                    "No {} DXVK DLLs found in {}",
                    if pre.is_32bit { "32-bit (x32)" } else { "64-bit (x64)" },
                    pre.dxvk_source_dir
                ));
            }
        }
        let snap = crate::snapshots::create_snapshot_in_dir(
            &game.id,
            &game_dir,
            &game_dir.join(crate::snapshots::CAS_DIR_NAME),
            Some(if enable {
                "Auto-Backup Before DXVK Deploy"
            } else {
                "Auto-Backup Before DXVK Removal"
            }),
        )?;
        let mut status =
            deploy_or_remove_dxvk_in_dirs(&game.id, &exe_path, &wrappers_root, enable, Some(snap.id))?;
        status.status_message = if status.dxvk_deployed {
            format!(
                "Deployed DXVK ({}) next to executable (CAS snapshot saved).",
                status.dxvk_deployed_dlls.join(", ")
            )
        } else {
            "Removed DXVK wrappers from executable folder (CAS snapshot saved).".to_string()
        };
        Ok(status)
    })
    .await
    .map_err(|e| format!("DXVK toggle worker failed: {e}"))?
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
pub(crate) mod tests {
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

    #[test]
    fn pe32_laa_patch_and_dxvk_wrapper_round_trip() {
        let root = std::env::temp_dir().join(format!("duke_compat_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let game_dir = root.join("LegacyDX9Game");
        let wrappers_root = root.join("wrappers");
        let dxvk_x32 = wrappers_root.join("dxvk").join("x32");
        fs::create_dir_all(&game_dir).unwrap();
        fs::create_dir_all(&dxvk_x32).unwrap();

        let exe32 = game_dir.join("retro32.exe");
        let mut pe_bytes = build_synthetic_pe(true, &["KERNEL32.dll", "d3d9.dll", "dxgi.dll"]);
        // Set a non-zero initial checksum to verify checksum update on patch.
        let opt_off = 0x80usize + 4 + 20;
        pe_bytes[opt_off + 64..opt_off + 68].copy_from_slice(&0x1234u32.to_le_bytes());
        fs::write(&exe32, pe_bytes).unwrap();
        fs::write(dxvk_x32.join("d3d9.dll"), build_synthetic_pe(true, &[])).unwrap();
        fs::write(dxvk_x32.join("dxgi.dll"), build_synthetic_pe(true, &[])).unwrap();

        let initial = inspect_compat_with_dirs("g1", &exe32, &wrappers_root, None).unwrap();
        assert!(initial.is_32bit);
        assert!(!initial.large_address_aware);
        assert!(initial.laa_eligible);
        assert!(initial.legacy_dx_imports.iter().any(|x| x == "DirectX 9"));
        assert!(initial.dxvk_source_ready);
        assert!(!initial.dxvk_deployed);

        assert!(set_pe_large_address_aware(&exe32, true).unwrap());
        let after_laa = inspect_compat_with_dirs("g1", &exe32, &wrappers_root, None).unwrap();
        assert!(after_laa.large_address_aware);

        assert!(!set_pe_large_address_aware(&exe32, false).unwrap());
        let after_unlaa = inspect_compat_with_dirs("g1", &exe32, &wrappers_root, None).unwrap();
        assert!(!after_unlaa.large_address_aware);

        let after_deploy =
            deploy_or_remove_dxvk_in_dirs("g1", &exe32, &wrappers_root, true, None).unwrap();
        assert!(after_deploy.dxvk_deployed);
        assert!(game_dir.join("d3d9.dll").is_file());

        let after_remove =
            deploy_or_remove_dxvk_in_dirs("g1", &exe32, &wrappers_root, false, None).unwrap();
        assert!(!after_remove.dxvk_deployed);
        assert!(!game_dir.join("d3d9.dll").is_file());

        // 64-bit binary rejects LAA bit toggle.
        let exe64 = game_dir.join("modern64.exe");
        fs::write(&exe64, build_synthetic_pe(false, &[])).unwrap();
        assert!(set_pe_large_address_aware(&exe64, true).is_err());

        let _ = fs::remove_dir_all(&root);
    }

    pub(crate) fn build_synthetic_pe_with_exports(
        is_32bit: bool,
        dll_name: &str,
        exports: &[(&str, Option<&str>)],
    ) -> Vec<u8> {
        let mut buf = build_synthetic_pe(is_32bit, &[]);
        let opt = 0x80usize + 4 + 20;
        let dd_base = if is_32bit { opt + 96 } else { opt + 112 };
        let edata_rva: u32 = 0x2000;
        let edata_raw: usize = 0x400;
        let edata_size: u32 = 0x600;
        buf[dd_base..dd_base + 4].copy_from_slice(&edata_rva.to_le_bytes());
        buf[dd_base + 4..dd_base + 8].copy_from_slice(&edata_size.to_le_bytes());

        let n = exports.len() as u32;
        let funcs_raw = edata_raw + 40;
        let names_raw = funcs_raw + (exports.len() * 4);
        let ords_raw = names_raw + (exports.len() * 4);
        let mut str_cursor = ords_raw + (exports.len() * 2);

        let dll_name_rva = edata_rva + (str_cursor - edata_raw) as u32;
        buf[str_cursor..str_cursor + dll_name.len()].copy_from_slice(dll_name.as_bytes());
        buf[str_cursor + dll_name.len()] = 0;
        str_cursor += dll_name.len() + 1;

        buf[edata_raw + 12..edata_raw + 16].copy_from_slice(&dll_name_rva.to_le_bytes());
        buf[edata_raw + 16..edata_raw + 20].copy_from_slice(&1u32.to_le_bytes());
        buf[edata_raw + 20..edata_raw + 24].copy_from_slice(&n.to_le_bytes());
        buf[edata_raw + 24..edata_raw + 28].copy_from_slice(&n.to_le_bytes());
        buf[edata_raw + 28..edata_raw + 32]
            .copy_from_slice(&(edata_rva + (funcs_raw - edata_raw) as u32).to_le_bytes());
        buf[edata_raw + 32..edata_raw + 36]
            .copy_from_slice(&(edata_rva + (names_raw - edata_raw) as u32).to_le_bytes());
        buf[edata_raw + 36..edata_raw + 40]
            .copy_from_slice(&(edata_rva + (ords_raw - edata_raw) as u32).to_le_bytes());

        for (i, (sym_name, forwarder)) in exports.iter().enumerate() {
            let fn_rva = if let Some(fwd) = forwarder {
                let rva = edata_rva + (str_cursor - edata_raw) as u32;
                buf[str_cursor..str_cursor + fwd.len()].copy_from_slice(fwd.as_bytes());
                buf[str_cursor + fwd.len()] = 0;
                str_cursor += fwd.len() + 1;
                rva
            } else {
                0x1000 + (i as u32) * 0x10
            };
            let f_off = funcs_raw + i * 4;
            buf[f_off..f_off + 4].copy_from_slice(&fn_rva.to_le_bytes());

            let sym_rva = edata_rva + (str_cursor - edata_raw) as u32;
            buf[str_cursor..str_cursor + sym_name.len()].copy_from_slice(sym_name.as_bytes());
            buf[str_cursor + sym_name.len()] = 0;
            str_cursor += sym_name.len() + 1;

            let n_off = names_raw + i * 4;
            buf[n_off..n_off + 4].copy_from_slice(&sym_rva.to_le_bytes());
            let o_off = ords_raw + i * 2;
            buf[o_off..o_off + 2].copy_from_slice(&(i as u16).to_le_bytes());
        }
        buf
    }

    #[test]
    fn pe32_and_pe64_export_directory_parses_symbols_and_forwarders() {
        for is_32bit in [true, false] {
            let bytes = build_synthetic_pe_with_exports(
                is_32bit,
                "version.dll",
                &[
                    ("GetFileVersionInfoW", Some("version_orig.GetFileVersionInfoW")),
                    ("VerQueryValueW", None),
                ],
            );
            let parsed = parse_pe_exports(&bytes).unwrap();
            assert_eq!(parsed.is_32bit, is_32bit);
            assert_eq!(parsed.dll_name.as_deref(), Some("version.dll"));
            assert_eq!(parsed.ordinal_base, 1);
            assert_eq!(
                parsed.exported_names,
                vec!["GetFileVersionInfoW".to_string(), "VerQueryValueW".to_string()]
            );
            assert_eq!(parsed.symbols.len(), 2);
            assert_eq!(
                parsed.symbols[0].forwarder.as_deref(),
                Some("version_orig.GetFileVersionInfoW")
            );
            assert_eq!(parsed.symbols[1].forwarder, None);
        }
    }

    fn build_synthetic_pe_with_thunk_imports(
        is_32bit: bool,
        dll_name: &str,
        fn_names: &[&str],
        extra_payload: &[u8],
    ) -> Vec<u8> {
        let mut buf = vec![0u8; 8192];
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
        buf[coff + 18..coff + 20].copy_from_slice(&0x0022u16.to_le_bytes());

        let opt = coff + 20;
        let opt_magic: u16 = if is_32bit { 0x010B } else { 0x020B };
        buf[opt..opt + 2].copy_from_slice(&opt_magic.to_le_bytes());
        let rva_count_off = if is_32bit { opt + 92 } else { opt + 108 };
        let dd_base = if is_32bit { opt + 96 } else { opt + 112 };
        buf[rva_count_off..rva_count_off + 4].copy_from_slice(&16u32.to_le_bytes());

        let rdata_rva: u32 = 0x2000;
        let rdata_raw: usize = 0x400;
        buf[dd_base + 8..dd_base + 12].copy_from_slice(&rdata_rva.to_le_bytes());
        buf[dd_base + 12..dd_base + 16].copy_from_slice(&0x400u32.to_le_bytes());

        let sec = opt + opt_size as usize;
        buf[sec..sec + 6].copy_from_slice(b".rdata");
        buf[sec + 8..sec + 12].copy_from_slice(&0x1C00u32.to_le_bytes());
        buf[sec + 12..sec + 16].copy_from_slice(&rdata_rva.to_le_bytes());
        buf[sec + 16..sec + 20].copy_from_slice(&0x1C00u32.to_le_bytes());
        buf[sec + 20..sec + 24].copy_from_slice(&(rdata_raw as u32).to_le_bytes());

        let thunk_raw = rdata_raw + 64;
        let step = if is_32bit { 4usize } else { 8usize };
        let mut cursor = thunk_raw + (fn_names.len() + 1) * step;

        let dll_rva = rdata_rva + (cursor - rdata_raw) as u32;
        buf[cursor..cursor + dll_name.len()].copy_from_slice(dll_name.as_bytes());
        buf[cursor + dll_name.len()] = 0;
        cursor += dll_name.len() + 1;

        let ilt_rva = rdata_rva + (thunk_raw - rdata_raw) as u32;
        buf[rdata_raw..rdata_raw + 4].copy_from_slice(&ilt_rva.to_le_bytes());
        buf[rdata_raw + 12..rdata_raw + 16].copy_from_slice(&dll_rva.to_le_bytes());
        buf[rdata_raw + 16..rdata_raw + 20].copy_from_slice(&ilt_rva.to_le_bytes());

        for (i, fn_name) in fn_names.iter().enumerate() {
            let hn_rva = rdata_rva + (cursor - rdata_raw) as u32;
            buf[cursor..cursor + 2].copy_from_slice(&0u16.to_le_bytes());
            cursor += 2;
            buf[cursor..cursor + fn_name.len()].copy_from_slice(fn_name.as_bytes());
            buf[cursor + fn_name.len()] = 0;
            cursor += fn_name.len() + 1;

            let t_off = thunk_raw + i * step;
            if is_32bit {
                buf[t_off..t_off + 4].copy_from_slice(&hn_rva.to_le_bytes());
            } else {
                buf[t_off..t_off + 8].copy_from_slice(&(hn_rva as u64).to_le_bytes());
            }
        }

        if cursor + extra_payload.len() < buf.len() {
            buf[cursor..cursor + extra_payload.len()].copy_from_slice(extra_payload);
        }
        buf
    }

    #[test]
    fn security_shield_v2_flags_injection_triad_and_droppers_and_quarantines() {
        // 1. Classic injection triad on unsigned PE32+ -> Blocked
        let mal_pe = build_synthetic_pe_with_thunk_imports(
            false,
            "kernel32.dll",
            &["VirtualAllocEx", "WriteProcessMemory", "CreateRemoteThread"],
            b"",
        );
        let parsed = parse_pe_import_functions(&mal_pe).unwrap();
        assert_eq!(parsed.3.len(), 3);
        let shield_blocked =
            evaluate_pe_security_shield_bytes(&mal_pe, "Unsigned", "Unsigned PE", 4.2, false);
        assert_eq!(shield_blocked.shield_badge, "Blocked");
        assert!(shield_blocked
            .suspicious_behaviors
            .iter()
            .any(|f| f.code == "PROCESS_INJECTION_TRIAD"));

        // SignedVerified binary with identical APIs drops severity to Warning
        let shield_signed =
            evaluate_pe_security_shield_bytes(&mal_pe, "SignedVerified", "Signed", 4.2, false);
        assert_eq!(shield_signed.shield_badge, "Warning");

        // 2. Hidden UTF-16LE PowerShell dropper + Run persistence -> Blocked
        let utf16_payload: Vec<u8> = "powershell -w hidden CurrentVersion\\Run"
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        let dropper_pe = build_synthetic_pe_with_thunk_imports(
            true,
            "advapi32.dll",
            &["RegSetValueExW", "CryptUnprotectData"],
            &utf16_payload,
        );
        let dropper_shield =
            evaluate_pe_security_shield_bytes(&dropper_pe, "Unsigned", "Unsigned PE", 4.5, false);
        assert_eq!(dropper_shield.shield_badge, "Blocked");
        assert!(dropper_shield
            .suspicious_behaviors
            .iter()
            .any(|f| f.code == "HIDDEN_SCRIPT_DROPPER"));
        assert!(dropper_shield
            .suspicious_behaviors
            .iter()
            .any(|f| f.code == "REGISTRY_RUN_PERSISTENCE"));

        // 3. Quarantine moves Blocked file out of staging directory cleanly
        let tmp = std::env::temp_dir().join("duke_shield_v2_test");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let stage_file = tmp.join("evil_drop.exe");
        let q_dir = tmp.join("quarantine");
        fs::write(&stage_file, &mal_pe).unwrap();
        let summary = gate_game_directory_pe_security(&tmp, Some(&q_dir), 16);
        assert_eq!(summary.blocked_files.len(), 1);
        assert!(!stage_file.exists());
        assert!(summary.blocked_files[0].2.is_some());
        let _ = fs::remove_dir_all(&tmp);
    }
}

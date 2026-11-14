use crate::duke_ai_rt::{acquire_worker_lease, query_foundry_local_chat};
use crate::mod_graph::scan_game_mod_graph_in_dir;
use crate::shader_swarm::detect_gpu_hardware_identity;
use crate::snapshots::{
    list_game_snapshots, restore_game_snapshot, GameSnapshotSummary, SnapshotRestoreReport,
    CAS_DIR_NAME,
};
use crate::{load_library_standalone, resolve_game_root_dir};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedMinidumpSummary {
    pub valid_mdmp: bool,
    pub stream_count: u32,
    pub exception_code: String,
    pub exception_label: String,
    pub exception_address: String,
    pub faulting_module: Option<String>,
    pub loaded_modules_sample: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashArtifactEvidence {
    pub file_path: String,
    pub artifact_kind: String,
    pub modified_ts: u64,
    pub size_bytes: u64,
    pub exception_code: Option<String>,
    pub faulting_module: Option<String>,
    pub offending_mod: Option<String>,
    pub offending_config_line: Option<String>,
    pub excerpt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashDiagnosisReport {
    pub game_id: String,
    pub game_title: String,
    pub game_dir: String,
    pub severity: String, // "healthy" | "warning" | "critical"
    pub summary: String,
    pub root_cause: String,
    pub offending_mod: Option<String>,
    pub faulting_module: Option<String>,
    pub exception_code: Option<String>,
    pub offending_config_line: Option<String>,
    pub mod_conflicts: Vec<String>,
    pub active_mods_count: usize,
    pub gpu_adapter: String,
    pub gpu_driver: String,
    pub execution_provider: String,
    pub vram_allocated_mb: f32,
    pub ai_backend: String,
    pub recommended_actions: Vec<String>,
    pub can_revert_snapshot: bool,
    pub latest_snapshot: Option<GameSnapshotSummary>,
    pub artifacts: Vec<CrashArtifactEvidence>,
}

fn read_u32_le(bytes: &[u8], off: usize) -> Option<u32> {
    bytes
        .get(off..off + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn read_u64_le(bytes: &[u8], off: usize) -> Option<u64> {
    bytes
        .get(off..off + 8)
        .map(|s| u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
}

fn read_mdmp_string(bytes: &[u8], rva: u32) -> Option<String> {
    let off = rva as usize;
    let byte_len = read_u32_le(bytes, off)? as usize;
    if byte_len == 0 || byte_len > 2048 || off + 4 + byte_len > bytes.len() {
        return None;
    }
    let slice = &bytes[off + 4..off + 4 + byte_len];
    let mut utf16 = Vec::with_capacity(byte_len / 2);
    for idx in (0..slice.len().saturating_sub(1)).step_by(2) {
        let code = u16::from_le_bytes([slice[idx], slice[idx + 1]]);
        if code != 0 {
            utf16.push(code);
        }
    }
    let full = String::from_utf16_lossy(&utf16);
    Some(
        Path::new(&full)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or(full),
    )
}

pub fn describe_ntstatus_hex(code: u32) -> &'static str {
    match code {
        0xC0000005 => "Access Violation (Invalid Memory Read/Write)",
        0xC000001D => "Illegal Instruction (AVX/ISA Mismatch or Corrupt Code)",
        0xC00000FD => "Stack Overflow (Infinite Recursion / Hook Loop)",
        0xC0000135 => "DLL Not Found / Missing Runtime Dependency",
        0xC0000409 => "Stack Buffer Overrun / Security Cookie Fault",
        0x80000003 => "Debug Breakpoint / Assertion Trap",
        0xE06D7363 => "Unhandled C++ Exception",
        _ => "Fatal Native Exception",
    }
}

/// Pure-Rust Windows `.dmp` (`MDMP`) binary parser inspecting ExceptionStream (`6`)
/// and ModuleListStream (`4`) to pinpoint exception codes and the exact faulting DLL/EXE.
pub fn parse_windows_minidump_bytes(bytes: &[u8]) -> Option<ParsedMinidumpSummary> {
    if bytes.len() < 32 || &bytes[0..4] != b"MDMP" {
        return None;
    }
    let stream_count = read_u32_le(bytes, 8)?;
    let dir_rva = read_u32_le(bytes, 12)? as usize;

    let mut exception_code_num: Option<u32> = None;
    let mut exception_addr: Option<u64> = None;
    let mut modules: Vec<(u64, u32, String)> = Vec::new();

    for idx in 0..(stream_count as usize).min(64) {
        let entry_off = dir_rva + idx * 12;
        if entry_off + 12 > bytes.len() {
            break;
        }
        let stream_type = read_u32_le(bytes, entry_off)?;
        let _data_size = read_u32_le(bytes, entry_off + 4)?;
        let rva = read_u32_le(bytes, entry_off + 8)? as usize;

        if stream_type == 6 && rva + 32 <= bytes.len() {
            // MINIDUMP_EXCEPTION_STREAM: ThreadId(4) + Align(4) + ExceptionRecord{Code(4),Flags(4),Rec(8),Addr(8)}
            exception_code_num = read_u32_le(bytes, rva + 8);
            exception_addr = read_u64_le(bytes, rva + 24);
        } else if stream_type == 4 && rva + 4 <= bytes.len() {
            // MINIDUMP_MODULE_LIST: NumberOfModules(4) + N * 108-byte MINIDUMP_MODULE
            let num_modules = read_u32_le(bytes, rva).unwrap_or(0).min(512);
            for m_idx in 0..num_modules as usize {
                let m_off = rva + 4 + m_idx * 108;
                if m_off + 108 > bytes.len() {
                    break;
                }
                let base = read_u64_le(bytes, m_off).unwrap_or(0);
                let size = read_u32_le(bytes, m_off + 8).unwrap_or(0);
                let name_rva = read_u32_le(bytes, m_off + 20).unwrap_or(0);
                if let Some(name) = read_mdmp_string(bytes, name_rva) {
                    modules.push((base, size, name));
                }
            }
        }
    }

    let faulting_module = exception_addr.and_then(|addr| {
        modules
            .iter()
            .find(|(base, size, _)| addr >= *base && addr < base.saturating_add(*size as u64))
            .map(|(_, _, name)| name.clone())
    });

    let code_u32 = exception_code_num.unwrap_or(0);
    let sample = modules.into_iter().take(12).map(|(_, _, name)| name).collect();

    Some(ParsedMinidumpSummary {
        valid_mdmp: true,
        stream_count,
        exception_code: format!("0x{:08X}", code_u32),
        exception_label: describe_ntstatus_hex(code_u32).to_string(),
        exception_address: format!("0x{:016X}", exception_addr.unwrap_or(0)),
        faulting_module,
        loaded_modules_sample: sample,
    })
}

fn extract_xml_tag_value(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    let clean = xml[start..end].trim().to_string();
    (!clean.is_empty()).then_some(clean)
}

fn extract_hex_exception(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    for i in 0..bytes.len().saturating_sub(9) {
        if (bytes[i] == b'0') && (bytes[i + 1] == b'x' || bytes[i + 1] == b'X') {
            let slice = &line[i + 2..];
            let hex_len = slice
                .chars()
                .take_while(|c| c.is_ascii_hexdigit())
                .count();
            if hex_len == 8 {
                return Some(format!("0x{}", slice[..8].to_ascii_uppercase()));
            }
        }
    }
    None
}

fn extract_dll_or_exe_token(line: &str) -> Option<String> {
    for raw_tok in line.split(|c: char| !c.is_ascii_alphanumeric() && c != '.' && c != '_' && c != '-') {
        let lower = raw_tok.to_ascii_lowercase();
        if (lower.ends_with(".dll") || lower.ends_with(".exe")) && raw_tok.len() > 4 {
            return Some(raw_tok.to_string());
        }
    }
    None
}

/// Parses `Crash.log`, `Player.log`, `BepInEx/LogOutput.log`, and Unreal `CrashContext.runtime-xml`.
pub fn parse_crash_log_text(path: &Path, content: &str) -> CrashArtifactEvidence {
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown.log".to_string());
    let lower_name = filename.to_ascii_lowercase();
    let is_xml_crash =
        lower_name.ends_with(".xml") || lower_name.ends_with("-xml") || lower_name.contains("crashcontext");
    let kind = if is_xml_crash {
        "Unreal CrashContext XML"
    } else if lower_name.contains("logoutput") {
        "BepInEx LogOutput"
    } else if lower_name.contains("player") {
        "Unity Player.log"
    } else {
        "Crash Log"
    };

    let mut exception_code = None;
    let mut faulting_module = None;
    let mut offending_mod = None;
    let mut offending_config_line = None;
    let mut excerpt_lines: Vec<String> = Vec::new();

    if is_xml_crash {
        if let Some(err_msg) = extract_xml_tag_value(content, "ErrorMessage") {
            excerpt_lines.push(err_msg.clone());
            if let Some(code) = extract_hex_exception(&err_msg) {
                exception_code = Some(code);
            }
            if let Some(dll) = extract_dll_or_exe_token(&err_msg) {
                faulting_module = Some(dll);
            }
        }
        if let Some(stack) = extract_xml_tag_value(content, "CallStack") {
            for line in stack.lines().take(5) {
                let trim = line.trim();
                if !trim.is_empty() {
                    if faulting_module.is_none() {
                        faulting_module = extract_dll_or_exe_token(trim);
                    }
                    excerpt_lines.push(trim.to_string());
                }
            }
        }
    } else {
        for line in content.lines().take(6000) {
            let trim = line.trim();
            if trim.is_empty() {
                continue;
            }
            let lower = trim.to_ascii_lowercase();
            let is_err = lower.contains("[error")
                || lower.contains("fatal")
                || lower.contains("exception")
                || lower.contains("access violation")
                || lower.contains("caused an access")
                || lower.contains("dxgi_error")
                || lower.contains("crash")
                || lower.contains("unhandled")
                || lower.contains("missingmethodexception")
                || lower.contains("could not load")
                || lower.contains("syntax error")
                || lower.contains("invalid config");

            if exception_code.is_none() {
                if let Some(code) = extract_hex_exception(trim) {
                    exception_code = Some(code);
                } else if lower.contains("dxgi_error_device_hung") {
                    exception_code = Some("DXGI_ERROR_DEVICE_HUNG".to_string());
                } else if lower.contains("dxgi_error_device_removed") {
                    exception_code = Some("DXGI_ERROR_DEVICE_REMOVED".to_string());
                } else if lower.contains("outofmemory") {
                    exception_code = Some("E_OUTOFMEMORY".to_string());
                }
            }

            if faulting_module.is_none()
                && (lower.contains("caused") || lower.contains("fault") || lower.contains("error loading"))
            {
                faulting_module = extract_dll_or_exe_token(trim);
            }

            if offending_mod.is_none() && lower.contains("[error  :") {
                if let Some(rest) = trim.split("[Error  :").nth(1) {
                    let mod_name = rest.split(']').next().unwrap_or("").trim();
                    if !mod_name.is_empty() && !mod_name.eq_ignore_ascii_case("Unity Log") {
                        offending_mod = Some(mod_name.to_string());
                    }
                }
            }

            if offending_config_line.is_none()
                && (lower.contains(".ini") || lower.contains(".cfg") || lower.contains(".toml"))
                && (lower.contains("error") || lower.contains("invalid") || lower.contains("parse"))
            {
                offending_config_line = Some(trim.chars().take(180).collect());
            }

            if is_err && excerpt_lines.len() < 8 {
                excerpt_lines.push(trim.chars().take(200).collect());
            }
        }
    }

    let meta = fs::metadata(path).ok();
    CrashArtifactEvidence {
        file_path: path.to_string_lossy().to_string(),
        artifact_kind: kind.to_string(),
        modified_ts: meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0),
        size_bytes: meta.map(|m| m.len()).unwrap_or(content.len() as u64),
        exception_code,
        faulting_module,
        offending_mod,
        offending_config_line,
        excerpt: excerpt_lines.join(" | "),
    }
}

fn scan_candidate_crash_paths(game_dir: &Path, exe_stem: &str, game_title: &str) -> Vec<PathBuf> {
    let mut found: Vec<(u64, PathBuf)> = Vec::new();
    let mut roots = vec![game_dir.to_path_buf()];
    if let Ok(local_app) = std::env::var("LOCALAPPDATA") {
        let base = PathBuf::from(&local_app);
        roots.push(base.join("CrashDumps"));
        let compact_title: String = game_title
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        if !compact_title.is_empty() {
            roots.push(base.join(&compact_title).join("Saved").join("Crashes"));
        }
    }

    let exe_l = exe_stem.to_ascii_lowercase();
    for root in roots {
        if !root.exists() {
            continue;
        }
        for entry in WalkDir::new(&root)
            .max_depth(4)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let p = entry.path();
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            let in_crashdumps = p
                .parent()
                .and_then(|d| d.file_name())
                .map(|d| d.to_string_lossy().eq_ignore_ascii_case("CrashDumps"))
                .unwrap_or(false);
            if in_crashdumps && !exe_l.is_empty() && !name.contains(&exe_l) {
                continue;
            }

            let is_crash_artifact = name.ends_with(".dmp")
                || name == "crash.log"
                || name == "player.log"
                || name == "logoutput.log"
                || name == "error.log"
                || name == "crashcontext.runtime-xml"
                || (name.contains("crash") && name.ends_with(".log"));
            if is_crash_artifact {
                let ts = entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                found.push((ts, p.to_path_buf()));
            }
        }
    }

    found.sort_by_key(|a| std::cmp::Reverse(a.0));
    found.truncate(16);
    found.into_iter().map(|(_, p)| p).collect()
}

fn inspect_artifact_path(path: &Path) -> Option<CrashArtifactEvidence> {
    let meta = fs::metadata(path).ok()?;
    let modified_ts = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    if ext == "dmp" {
        let mut f = File::open(path).ok()?;
        let cap = (meta.len() as usize).min(12 * 1024 * 1024);
        let mut buf = vec![0u8; cap];
        let n = f.read(&mut buf).ok()?;
        buf.truncate(n);
        let parsed = parse_windows_minidump_bytes(&buf)?;
        return Some(CrashArtifactEvidence {
            file_path: path.to_string_lossy().to_string(),
            artifact_kind: "Windows Minidump (.dmp)".to_string(),
            modified_ts,
            size_bytes: meta.len(),
            exception_code: Some(parsed.exception_code.clone()),
            faulting_module: parsed.faulting_module.clone(),
            offending_mod: None,
            offending_config_line: None,
            excerpt: format!(
                "{} @ {} | Faulting: {} | Loaded sample: {}",
                parsed.exception_label,
                parsed.exception_address,
                parsed
                    .faulting_module
                    .unwrap_or_else(|| "Unresolved module".to_string()),
                parsed.loaded_modules_sample.join(", ")
            ),
        });
    }

    let mut f = File::open(path).ok()?;
    let cap = (meta.len() as usize).min(2 * 1024 * 1024);
    let mut raw = vec![0u8; cap];
    let n = f.read(&mut raw).ok()?;
    raw.truncate(n);
    let text = String::from_utf8_lossy(&raw);
    Some(parse_crash_log_text(path, &text))
}

pub fn diagnose_game_crash_sync(
    game_id: &str,
    extra_log_path: Option<&str>,
) -> Result<CrashDiagnosisReport, String> {
    let lib = load_library_standalone()?;
    let entry = lib
        .iter()
        .find(|g| g.id == game_id)
        .ok_or_else(|| format!("Game '{}' not found in library.", game_id))?;

    let exe_path = PathBuf::from(&entry.exe_path);
    let exe_stem = exe_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let game_dir = if !entry.install_dir.trim().is_empty() {
        PathBuf::from(&entry.install_dir)
    } else {
        resolve_game_root_dir(&exe_path)
    };
    let cas_root = game_dir.join(CAS_DIR_NAME);

    let mod_report = scan_game_mod_graph_in_dir(
        &entry.id,
        &game_dir,
        Some(exe_path.as_path()),
        &cas_root,
    )
    .ok();

    let mod_conflicts: Vec<String> = mod_report
        .as_ref()
        .map(|r| {
            r.collisions
                .iter()
                .map(|c| format!("{}: {}", c.proxy_dll, c.contending_packs.join(" vs ")))
                .collect()
        })
        .unwrap_or_default();
    let active_mods_count = mod_report.as_ref().map(|r| r.packs.len()).unwrap_or(0);

    let mut paths = scan_candidate_crash_paths(&game_dir, &exe_stem, &entry.title);
    if let Some(extra) = extra_log_path.filter(|s| !s.trim().is_empty()) {
        let ep = PathBuf::from(extra.trim());
        if ep.is_file() && !paths.contains(&ep) {
            paths.insert(0, ep);
        }
    }

    let artifacts: Vec<CrashArtifactEvidence> = paths
        .iter()
        .filter_map(|p| inspect_artifact_path(p))
        .collect();

    let first_exc = artifacts
        .iter()
        .find_map(|a| a.exception_code.clone());
    let first_fault_mod = artifacts
        .iter()
        .find_map(|a| a.faulting_module.clone());
    let first_offending_mod = artifacts
        .iter()
        .find_map(|a| a.offending_mod.clone())
        .or_else(|| {
            first_fault_mod.as_ref().and_then(|dll| {
                let l = dll.to_ascii_lowercase();
                if l.contains("reshade") || l.contains("dxgi") || l.contains("bepinex") || l.contains("specialk") || l.contains("dlssg") {
                    Some(dll.clone())
                } else {
                    None
                }
            })
        });
    let first_cfg_line = artifacts
        .iter()
        .find_map(|a| a.offending_config_line.clone());

    let gpu = detect_gpu_hardware_identity();
    let gpu_driver = duke_core::win32_ffi::detect_primary_gpu_driver_version()
        .unwrap_or_else(|| gpu.driver_version.clone());

    let lease = acquire_worker_lease("doctor", "phi-3.5-mini-npu.onnx");
    let snaps_dir = crate::snapshots::list_snapshots_in_cas(&cas_root).unwrap_or_default();
    let latest_snapshot = snaps_dir.first().cloned();

    let (severity, summary, root_cause, recommended_actions, ai_backend) = if artifacts.is_empty() && mod_conflicts.is_empty() {
        (
            "healthy".to_string(),
            format!("No recent crash dumps, error logs, or DLL hook collisions found for {}.", entry.title),
            "Game installation and active mod graph appear clean.".to_string(),
            vec![
                "Keep a .duke-cas pre-mod snapshot up to date before installing new ReShade or SKSE/BepInEx packs.".to_string(),
            ],
            lease.model_name.clone(),
        )
    } else {
        let context_prompt = format!(
            "Game: {}\nGPU: {} (Driver {})\nException: {:?}\nFaulting Module: {:?}\nOffending Mod: {:?}\nConfig Line: {:?}\nMod Collisions: {:?}\nExcerpts:\n{}",
            entry.title,
            gpu.adapter_name,
            gpu_driver,
            first_exc,
            first_fault_mod,
            first_offending_mod,
            first_cfg_line,
            mod_conflicts,
            artifacts
                .iter()
                .take(3)
                .map(|a| format!("{}: {}", a.artifact_kind, a.excerpt))
                .collect::<Vec<_>>()
                .join("\n")
        );
        let llm_hit = query_foundry_local_chat(
            "You are Duke Doctor. Identify exact root cause & actionable fix in 2 concise sentences.",
            &context_prompt,
        );

        let sev = if first_exc.is_some() || !artifacts.is_empty() {
            "critical".to_string()
        } else {
            "warning".to_string()
        };

        let heuristic_root = if let Some(ref conflicts) = mod_conflicts.first() {
            format!(
                "Proxy DLL hook collision ({}) combined with {} fault in {}.",
                conflicts,
                first_exc.as_deref().unwrap_or("runtime"),
                first_fault_mod.as_deref().unwrap_or("active binary")
            )
        } else if let Some(ref fm) = first_fault_mod {
            format!(
                "Crash exception {} originated inside '{}' (GPU driver {}).",
                first_exc.as_deref().unwrap_or("fault"),
                fm,
                gpu_driver
            )
        } else if let Some(ref om) = first_offending_mod {
            format!("Mod assembly '{}' threw an unhandled initialization fault.", om)
        } else {
            format!(
                "Detected {} crash artifact(s) ({}) during last execution.",
                artifacts.len(),
                first_exc.as_deref().unwrap_or("log error")
            )
        };

        let (summary_line, root_text, backend) = if let Some((reply, bk)) = llm_hit {
            (
                format!(
                    "{} issue diagnosed ({})",
                    entry.title,
                    first_fault_mod
                        .as_deref()
                        .or(first_exc.as_deref())
                        .unwrap_or("Crash Artifact")
                ),
                reply,
                bk,
            )
        } else {
            (
                format!(
                    "{} {} detected ({})",
                    entry.title,
                    sev.to_ascii_uppercase(),
                    first_fault_mod
                        .as_deref()
                        .or(first_offending_mod.as_deref())
                        .or(first_exc.as_deref())
                        .unwrap_or("Mod/Driver Conflict")
                ),
                heuristic_root,
                format!("Duke Doctor Heuristic + {}", lease.ep_label()),
            )
        };

        let mut actions = Vec::new();
        if latest_snapshot.is_some() {
            actions.push("Click '1-Click Revert Snapshot (.duke-cas)' to roll back game/mod state losslessly.".to_string());
        }
        if !mod_conflicts.is_empty() {
            actions.push(format!("Resolve proxy DLL collision in Mod Graph ({}).", mod_conflicts[0]));
        }
        if let Some(ref cfg_err) = first_cfg_line {
            actions.push(format!("Fix or reset malformed config entry: {}", cfg_err));
        }
        if first_fault_mod
            .as_deref()
            .map(|m| {
                let l = m.to_ascii_lowercase();
                l.contains("d3d") || l.contains("dxgi") || l.contains("nv") || l.contains("amd")
            })
            .unwrap_or(false)
        {
            actions.push("Disable overlay injectors / ReShade temporarily or purge stale shader cache in Performance Tools.".to_string());
        }
        if actions.is_empty() {
            actions.push("Inspect crash excerpt below and verify game runtime DLLs via Preflight Check.".to_string());
        }

        (sev, summary_line, root_text, actions, backend)
    };

    Ok(CrashDiagnosisReport {
        game_id: entry.id.clone(),
        game_title: entry.title.clone(),
        game_dir: game_dir.to_string_lossy().to_string(),
        severity,
        summary,
        root_cause,
        offending_mod: first_offending_mod,
        faulting_module: first_fault_mod,
        exception_code: first_exc,
        offending_config_line: first_cfg_line,
        mod_conflicts,
        active_mods_count,
        gpu_adapter: gpu.adapter_name,
        gpu_driver,
        execution_provider: lease.ep_label().to_string(),
        vram_allocated_mb: lease.ep.vram_allocated_mb(),
        ai_backend,
        recommended_actions,
        can_revert_snapshot: latest_snapshot.is_some(),
        latest_snapshot,
        artifacts,
    })
}

#[tauri::command]
pub async fn diagnose_game_crash(
    game_id: String,
    extra_log_path: Option<String>,
) -> Result<CrashDiagnosisReport, String> {
    tokio::task::spawn_blocking(move || diagnose_game_crash_sync(&game_id, extra_log_path.as_deref()))
        .await
        .map_err(|e| format!("Duke Doctor task panicked: {}", e))?
}

#[tauri::command]
pub async fn doctor_revert_game_snapshot(
    game_id: String,
    snapshot_id: Option<String>,
) -> Result<SnapshotRestoreReport, String> {
    let target_id = if let Some(sid) = snapshot_id.filter(|s| !s.trim().is_empty()) {
        sid
    } else {
        let snaps = list_game_snapshots(game_id.clone()).await?;
        snaps
            .first()
            .map(|s| s.id.clone())
            .ok_or_else(|| "No .duke-cas rollback snapshot found for this game.".to_string())?
    };
    restore_game_snapshot(game_id, target_id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_synthetic_mdmp(exc_code: u32, fault_addr: u64, module_name: &str) -> Vec<u8> {
        let mut buf = vec![0u8; 512];
        buf[0..4].copy_from_slice(b"MDMP");
        // version = 0xA793
        buf[4..8].copy_from_slice(&0x0000_A793u32.to_le_bytes());
        // stream count = 2
        buf[8..12].copy_from_slice(&2u32.to_le_bytes());
        // stream dir rva = 32
        buf[12..16].copy_from_slice(&32u32.to_le_bytes());

        // Stream dir entries @ 32:
        // Entry 0: ExceptionStream(6), size=168, rva=64
        buf[32..36].copy_from_slice(&6u32.to_le_bytes());
        buf[36..40].copy_from_slice(&168u32.to_le_bytes());
        buf[40..44].copy_from_slice(&64u32.to_le_bytes());
        // Entry 1: ModuleListStream(4), size=112, rva=232
        buf[44..48].copy_from_slice(&4u32.to_le_bytes());
        buf[48..52].copy_from_slice(&112u32.to_le_bytes());
        buf[52..56].copy_from_slice(&232u32.to_le_bytes());

        // ExceptionStream @ 64: ThreadId=1, ExceptionCode @ +8, ExceptionAddr @ +24
        buf[64..68].copy_from_slice(&1u32.to_le_bytes());
        buf[72..76].copy_from_slice(&exc_code.to_le_bytes());
        buf[88..96].copy_from_slice(&fault_addr.to_le_bytes());

        // ModuleListStream @ 232: NumModules=1, MINIDUMP_MODULE @ 236 (108 bytes), string @ 360
        buf[232..236].copy_from_slice(&1u32.to_le_bytes());
        buf[236..244].copy_from_slice(&0x7FF8_1000_0000u64.to_le_bytes()); // BaseOfImage
        buf[244..248].copy_from_slice(&0x0020_0000u32.to_le_bytes()); // SizeOfImage
        buf[256..260].copy_from_slice(&360u32.to_le_bytes()); // ModuleNameRva @ +20

        let utf16: Vec<u16> = module_name.encode_utf16().collect();
        let byte_len = (utf16.len() * 2) as u32;
        buf[360..364].copy_from_slice(&byte_len.to_le_bytes());
        for (i, ch) in utf16.iter().enumerate() {
            let off = 364 + i * 2;
            buf[off..off + 2].copy_from_slice(&ch.to_le_bytes());
        }
        buf
    }

    #[test]
    fn minidump_parser_extracts_exception_and_faulting_dll() {
        let raw = build_synthetic_mdmp(0xC0000005, 0x7FF8_1004_1234, r"C:\Games\reshade64.dll");
        let parsed = parse_windows_minidump_bytes(&raw).expect("valid MDMP header");
        assert!(parsed.valid_mdmp);
        assert_eq!(parsed.exception_code, "0xC0000005");
        assert_eq!(parsed.faulting_module.as_deref(), Some("reshade64.dll"));
    }

    #[test]
    fn crash_log_parser_extracts_bepinex_and_unreal_faults() {
        let bepinex = "[Error  : BetterGraphicsMod] MissingMethodException in d3d11.dll at settings.ini parse error";
        let ev = parse_crash_log_text(Path::new("BepInEx/LogOutput.log"), bepinex);
        assert_eq!(ev.offending_mod.as_deref(), Some("BetterGraphicsMod"));
        assert!(ev.offending_config_line.is_some());

        let xml = "<FGenericCrashContext><ErrorMessage>Unhandled Exception: 0xC0000005 in dxgi.dll</ErrorMessage></FGenericCrashContext>";
        let ev_xml = parse_crash_log_text(Path::new("CrashContext.runtime-xml"), xml);
        assert_eq!(ev_xml.exception_code.as_deref(), Some("0xC0000005"));
        assert_eq!(ev_xml.faulting_module.as_deref(), Some("dxgi.dll"));
    }
}

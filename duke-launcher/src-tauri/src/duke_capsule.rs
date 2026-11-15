//! Duke Capsule: 1-click portable USB & handheld capsule compiler.
//!
//! Packages a game directory, resolved non-OS runtime DLLs (`pe_inspect`),
//! active `mod_graph` proxy chains & `.duke-mods/`, `.duke-profile/` save
//! sandboxing (seeded optionally from Ludusavi save paths), and a zero-install
//! `Launch_Portable_Capsule.cmd` into either a self-contained folder or a
//! seekable multi-frame Zstd archive (`.dkcap`, magic `DKCAP001`).

use crate::mod_graph::{self, ModGraphReport, CAS_DIR_NAME, MODS_ROOT_DIR};
use crate::pe_inspect;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tauri::AppHandle;
use walkdir::WalkDir;

pub const DKCAP_MAGIC: &[u8; 8] = b"DKCAP001";
pub const CAPSULE_META_DIR: &str = "_DukeCapsuleData";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapsuleRuntimeDll {
    pub name: String,
    pub source_kind: String,
    pub bundled_rel_path: Option<String>,
    pub runtime_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapsuleModSummary {
    pub framework_packs: usize,
    pub active_loadout_id: Option<String>,
    pub proxy_chains: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapsuleFileRecord {
    pub rel_path: String,
    pub size_bytes: u64,
    pub blake3: String,
    pub compressed_offset: u64,
    pub compressed_len: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DukeCapsuleManifest {
    pub format_version: u32,
    pub game_id: String,
    pub title: String,
    pub architecture: String,
    pub exe_rel_path: String,
    pub launch_args: Option<String>,
    pub created_ts: u64,
    pub total_files: usize,
    pub total_uncompressed_bytes: u64,
    pub bundled_runtime_dlls: Vec<CapsuleRuntimeDll>,
    pub missing_runtime_dlls: Vec<String>,
    pub mod_summary: CapsuleModSummary,
    pub bundled_save_roots: Vec<String>,
    pub launcher_script: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DukeCapsuleExportReport {
    pub game_id: String,
    pub title: String,
    pub format: String,
    pub output_path: String,
    pub file_count: usize,
    pub uncompressed_bytes: u64,
    pub archive_bytes: u64,
    pub bundled_dll_count: usize,
    pub proxy_chain_count: usize,
    pub bundled_save_count: usize,
    pub elapsed_ms: u64,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DkcapFooterToc {
    manifest: DukeCapsuleManifest,
    entries: Vec<CapsuleFileRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DkcapInspectReport {
    pub archive_path: String,
    pub archive_bytes: u64,
    pub manifest: DukeCapsuleManifest,
    pub entry_count: usize,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn sanitize_slug(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_sep = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            prev_sep = false;
        } else if !prev_sep && !out.is_empty() {
            out.push('_');
            prev_sep = true;
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    if out.is_empty() {
        "PortableGame".to_string()
    } else {
        out
    }
}

fn sanitize_rel_path(rel: &str) -> Result<PathBuf, String> {
    let normalized = rel.replace('\\', "/");
    let candidate = Path::new(&normalized);
    let mut clean = PathBuf::new();
    for comp in candidate.components() {
        match comp {
            Component::Normal(part) => {
                let s = part.to_string_lossy();
                if s.contains(':') {
                    return Err(format!("Blocked unsafe path component: {}", rel));
                }
                clean.push(part);
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!("Blocked path traversal in capsule entry: {}", rel));
            }
        }
    }
    if clean.as_os_str().is_empty() {
        return Err("Capsule entry path cannot be empty".to_string());
    }
    Ok(clean)
}

fn hash_file_blake3(path: &Path) -> Result<String, String> {
    let file = File::open(path)
        .map_err(|e| format!("Failed reading '{}' for BLAKE3: {}", path.display(), e))?;
    let mut reader = BufReader::with_capacity(64 * 1024, file);
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("Read error hashing '{}': {}", path.display(), e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn collect_redist_candidates(game_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for name in [
        "Redist",
        "_Redist",
        "_CommonRedist",
        "CommonRedist",
        "DirectX",
        "vcredist",
        "bin",
    ] {
        let cand = game_dir.join(name);
        if cand.is_dir() {
            dirs.push(cand);
        }
    }
    dirs
}

fn copy_dir_recursive_filtered(src_root: &Path, dst_root: &Path) -> Result<(usize, u64), String> {
    fs::create_dir_all(dst_root)
        .map_err(|e| format!("Failed creating '{}': {}", dst_root.display(), e))?;
    let mut count = 0usize;
    let mut bytes = 0u64;
    for entry in WalkDir::new(src_root).follow_links(false).into_iter() {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        let Ok(rel) = path.strip_prefix(src_root) else {
            continue;
        };
        if rel.as_os_str().is_empty() {
            continue;
        }
        // Skip previous capsule outputs if nesting inside same folder.
        if rel.components().next().is_some_and(|c| {
            let s = c.as_os_str().to_string_lossy();
            s.eq_ignore_ascii_case(CAPSULE_META_DIR)
                || s.to_ascii_lowercase().ends_with(".dkcap")
                || s.to_ascii_lowercase().ends_with("_portablecapsule")
        }) {
            continue;
        }
        let target = dst_root.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)
                .map_err(|e| format!("Failed creating directory '{}': {}", target.display(), e))?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed creating parent '{}': {}", parent.display(), e))?;
            }
            let copied = fs::copy(path, &target)
                .map_err(|e| format!("Failed copying '{}': {}", path.display(), e))?;
            count += 1;
            bytes = bytes.saturating_add(copied);
        }
    }
    Ok((count, bytes))
}

fn render_portable_launcher_cmd(exe_rel: &str, launch_args: Option<&str>) -> String {
    let win_rel = exe_rel.replace('/', "\\");
    let args = launch_args.map(str::trim).unwrap_or("");
    let args_suffix = if args.is_empty() {
        String::new()
    } else {
        format!(" {}", args)
    };
    format!(
        "@echo off\r\n\
setlocal enableextensions\r\n\
set \"CAPSULE_ROOT=%~dp0\"\r\n\
if \"%CAPSULE_ROOT:~-1%\"==\"\\\" set \"CAPSULE_ROOT=%CAPSULE_ROOT:~0,-1%\"\r\n\
if not exist \"%CAPSULE_ROOT%\\.duke-profile\\AppData\" mkdir \"%CAPSULE_ROOT%\\.duke-profile\\AppData\"\r\n\
if not exist \"%CAPSULE_ROOT%\\.duke-profile\\LocalAppData\" mkdir \"%CAPSULE_ROOT%\\.duke-profile\\LocalAppData\"\r\n\
if not exist \"%CAPSULE_ROOT%\\.duke-profile\\Documents\" mkdir \"%CAPSULE_ROOT%\\.duke-profile\\Documents\"\r\n\
if not exist \"%CAPSULE_ROOT%\\.duke-profile\\SavedGames\" mkdir \"%CAPSULE_ROOT%\\.duke-profile\\SavedGames\"\r\n\
set \"USERPROFILE=%CAPSULE_ROOT%\\.duke-profile\"\r\n\
set \"HOME=%CAPSULE_ROOT%\\.duke-profile\"\r\n\
set \"APPDATA=%CAPSULE_ROOT%\\.duke-profile\\AppData\"\r\n\
set \"LOCALAPPDATA=%CAPSULE_ROOT%\\.duke-profile\\LocalAppData\"\r\n\
set \"PATH=%CAPSULE_ROOT%\\{meta}\\runtime;%PATH%\"\r\n\
pushd \"%CAPSULE_ROOT%\"\r\n\
start \"\" \"%CAPSULE_ROOT%\\{win_rel}\"{args_suffix} %*\r\n\
popd\r\n\
endlocal\r\n",
        meta = CAPSULE_META_DIR,
        win_rel = win_rel,
        args_suffix = args_suffix
    )
}

#[derive(Debug, Clone)]
pub struct CapsuleSourceSpec {
    pub game_id: String,
    pub title: String,
    pub game_dir: PathBuf,
    pub exe_path: PathBuf,
    pub launch_args: Option<String>,
    pub extra_save_dirs: Vec<(String, PathBuf)>,
}

fn prepare_capsule_staging(
    spec: &CapsuleSourceSpec,
    staging_root: &Path,
) -> Result<(DukeCapsuleManifest, usize, u64), String> {
    if !spec.game_dir.is_dir() {
        return Err(format!(
            "Game directory not found: {}",
            spec.game_dir.display()
        ));
    }
    if !spec.exe_path.is_file() {
        return Err(format!(
            "Game executable not found: {}",
            spec.exe_path.display()
        ));
    }

    let (copied_game_files, copied_game_bytes) =
        copy_dir_recursive_filtered(&spec.game_dir, staging_root)?;
    let _ = pe_inspect::prepare_game_sandbox_env(staging_root)?;

    let exe_rel = spec
        .exe_path
        .strip_prefix(&spec.game_dir)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| {
            spec.exe_path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "game.exe".to_string())
        });

    let redist_dirs = collect_redist_candidates(&spec.game_dir);
    let pe_report = pe_inspect::inspect_exe_with_dirs(&spec.exe_path, None, &redist_dirs)?;
    let runtime_dir = staging_root.join(CAPSULE_META_DIR).join("runtime");
    fs::create_dir_all(&runtime_dir)
        .map_err(|e| format!("Failed creating runtime dir: {}", e))?;

    let mut bundled_runtime_dlls = Vec::new();
    let mut missing_runtime_dlls = Vec::new();
    let mut seen_dlls = HashSet::new();
    let staged_exe_dir = staging_root
        .join(&exe_rel)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| staging_root.to_path_buf());

    for imp in &pe_report.imported_dlls {
        let lower = imp.name.to_ascii_lowercase();
        if !seen_dlls.insert(lower) {
            continue;
        }
        let is_system_os = matches!(
            imp.source_kind.as_str(),
            "system32" | "syswow64" | "api-set-contract"
        );
        if imp.resolved {
            if !is_system_os {
                if let Some(ref src_str) = imp.resolved_path {
                    let src_path = Path::new(src_str);
                    if src_path.is_file() {
                        let dst_runtime = runtime_dir.join(&imp.name);
                        let _ = fs::copy(src_path, &dst_runtime);
                        bundled_runtime_dlls.push(CapsuleRuntimeDll {
                            name: imp.name.clone(),
                            source_kind: imp.source_kind.clone(),
                            bundled_rel_path: Some(format!(
                                "{}/runtime/{}",
                                CAPSULE_META_DIR, imp.name
                            )),
                            runtime_hint: imp.is_runtime_hint.clone(),
                        });
                    }
                }
            }
        } else if let Some(ref cand_str) = imp.heal_candidate_path {
            let cand_path = Path::new(cand_str);
            if cand_path.is_file() {
                let dst_runtime = runtime_dir.join(&imp.name);
                let dst_exe_side = staged_exe_dir.join(&imp.name);
                let _ = fs::copy(cand_path, &dst_runtime);
                if !dst_exe_side.exists() {
                    let _ = fs::copy(cand_path, &dst_exe_side);
                }
                bundled_runtime_dlls.push(CapsuleRuntimeDll {
                    name: imp.name.clone(),
                    source_kind: "healed-redist".to_string(),
                    bundled_rel_path: Some(format!("{}/runtime/{}", CAPSULE_META_DIR, imp.name)),
                    runtime_hint: imp.is_runtime_hint.clone(),
                });
            } else {
                missing_runtime_dlls.push(imp.name.clone());
            }
        } else {
            missing_runtime_dlls.push(imp.name.clone());
        }
    }

    let cas_root = staging_root.join(CAS_DIR_NAME);
    let mod_report: ModGraphReport = mod_graph::scan_game_mod_graph_in_dir(
        &spec.game_id,
        staging_root,
        Some(&staging_root.join(&exe_rel)),
        &cas_root,
    )
    .unwrap_or_else(|_| ModGraphReport {
        game_id: spec.game_id.clone(),
        game_dir: staging_root.to_string_lossy().into_owned(),
        exe_is_32bit: Some(pe_report.is_32bit),
        packs: Vec::new(),
        collisions: Vec::new(),
        loadouts: Vec::new(),
        active_loadout_id: None,
        deployed_file_count: 0,
        summary: String::new(),
    });
    let mut proxy_chains = Vec::new();
    if staging_root.join(MODS_ROOT_DIR).is_dir() && mod_report.active_loadout_id.is_some() {
        if let Ok(applied) = mod_graph::apply_mod_loadout_in_dir(
            &spec.game_id,
            staging_root,
            Some(&staging_root.join(&exe_rel)),
            &cas_root,
            mod_report.active_loadout_id.as_deref(),
        ) {
            proxy_chains = applied.chain_configs_written;
        }
    }

    let mut bundled_save_roots = Vec::new();
    let saves_dir = staging_root.join(CAPSULE_META_DIR).join("saves");
    fs::create_dir_all(&saves_dir)
        .map_err(|e| format!("Failed creating capsule saves folder: {}", e))?;
    for (idx, (label, save_src)) in spec.extra_save_dirs.iter().enumerate() {
        if !save_src.exists() {
            continue;
        }
        let slot_name = format!("{:02}_{}", idx + 1, sanitize_slug(label));
        let dst = saves_dir.join(&slot_name);
        if save_src.is_dir() {
            if copy_dir_recursive_filtered(save_src, &dst).is_ok() {
                bundled_save_roots.push(format!("{}/saves/{}", CAPSULE_META_DIR, slot_name));
            }
        } else if save_src.is_file() {
            fs::create_dir_all(&dst)
                .map_err(|e| format!("Failed creating save slot '{}': {}", dst.display(), e))?;
            if let Some(fname) = save_src.file_name() {
                if fs::copy(save_src, dst.join(fname)).is_ok() {
                    bundled_save_roots.push(format!("{}/saves/{}", CAPSULE_META_DIR, slot_name));
                }
            }
        }
    }

    let cmd_name = "Launch_Portable_Capsule.cmd".to_string();
    let cmd_contents = render_portable_launcher_cmd(&exe_rel, spec.launch_args.as_deref());
    fs::write(staging_root.join(&cmd_name), cmd_contents.as_bytes())
        .map_err(|e| format!("Failed writing {}: {}", cmd_name, e))?;

    let manifest = DukeCapsuleManifest {
        format_version: 1,
        game_id: spec.game_id.clone(),
        title: spec.title.clone(),
        architecture: pe_report.architecture,
        exe_rel_path: exe_rel,
        launch_args: spec.launch_args.clone(),
        created_ts: now_secs(),
        total_files: copied_game_files,
        total_uncompressed_bytes: copied_game_bytes,
        bundled_runtime_dlls,
        missing_runtime_dlls,
        mod_summary: CapsuleModSummary {
            framework_packs: mod_report.packs.len(),
            active_loadout_id: mod_report.active_loadout_id,
            proxy_chains,
        },
        bundled_save_roots,
        launcher_script: cmd_name,
    };

    let manifest_path = staging_root
        .join(CAPSULE_META_DIR)
        .join("capsule-manifest.json");
    let manifest_json = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| format!("Manifest serialize error: {}", e))?;
    fs::write(&manifest_path, manifest_json)
        .map_err(|e| format!("Failed writing capsule-manifest.json: {}", e))?;

    // Recount total files & bytes across complete staging tree.
    let mut final_files = 0usize;
    let mut final_bytes = 0u64;
    for entry in WalkDir::new(staging_root).follow_links(false).into_iter() {
        let Ok(entry) = entry else {
            continue;
        };
        if entry.file_type().is_file() {
            final_files += 1;
            if let Ok(meta) = entry.metadata() {
                final_bytes = final_bytes.saturating_add(meta.len());
            }
        }
    }

    Ok((manifest, final_files, final_bytes))
}

fn write_seekable_dkcap_from_dir(
    staging_root: &Path,
    manifest: &mut DukeCapsuleManifest,
    out_file: &Path,
) -> Result<(Vec<CapsuleFileRecord>, u64), String> {
    if let Some(parent) = out_file.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed creating output directory: {}", e))?;
    }
    let tmp_out = out_file.with_extension("dkcap.tmp");
    let mut writer =
        File::create(&tmp_out).map_err(|e| format!("Failed creating '{}': {}", tmp_out.display(), e))?;
    writer
        .write_all(DKCAP_MAGIC)
        .map_err(|e| format!("Write magic error: {}", e))?;

    let mut rel_files = Vec::new();
    for entry in WalkDir::new(staging_root).follow_links(false).into_iter() {
        let Ok(entry) = entry else {
            continue;
        };
        if entry.file_type().is_file() {
            if let Ok(rel) = entry.path().strip_prefix(staging_root) {
                rel_files.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    rel_files.sort();

    let mut entries = Vec::with_capacity(rel_files.len());
    let mut offset = DKCAP_MAGIC.len() as u64;
    let mut total_uncompressed = 0u64;

    for rel in rel_files {
        let clean = sanitize_rel_path(&rel)?;
        let full_path = staging_root.join(&clean);
        let raw_size = fs::metadata(&full_path)
            .map_err(|e| format!("Stat error '{}': {}", full_path.display(), e))?
            .len();
        total_uncompressed = total_uncompressed.saturating_add(raw_size);
        let blake = hash_file_blake3(&full_path)?;
        let raw_bytes = fs::read(&full_path)
            .map_err(|e| format!("Read error '{}': {}", full_path.display(), e))?;
        let compressed = zstd::bulk::compress(&raw_bytes, 6)
            .map_err(|e| format!("Zstd frame compress error on '{}': {}", rel, e))?;
        writer
            .write_all(&compressed)
            .map_err(|e| format!("Write frame error: {}", e))?;
        let compressed_len = compressed.len() as u64;
        entries.push(CapsuleFileRecord {
            rel_path: rel,
            size_bytes: raw_size,
            blake3: blake,
            compressed_offset: offset,
            compressed_len,
        });
        offset = offset.saturating_add(compressed_len);
    }

    manifest.total_files = entries.len();
    manifest.total_uncompressed_bytes = total_uncompressed;

    let toc = DkcapFooterToc {
        manifest: manifest.clone(),
        entries: entries.clone(),
    };
    let toc_raw = serde_json::to_vec(&toc).map_err(|e| format!("TOC encode error: {}", e))?;
    let toc_zst =
        zstd::bulk::compress(&toc_raw, 6).map_err(|e| format!("TOC Zstd error: {}", e))?;
    let toc_offset = offset;
    let toc_len = toc_zst.len() as u64;
    writer
        .write_all(&toc_zst)
        .map_err(|e| format!("Write TOC error: {}", e))?;
    // 24-byte footer: toc_offset (u64) + toc_len (u64) + magic (8 bytes)
    writer
        .write_all(&toc_offset.to_le_bytes())
        .map_err(|e| format!("Write footer offset error: {}", e))?;
    writer
        .write_all(&toc_len.to_le_bytes())
        .map_err(|e| format!("Write footer len error: {}", e))?;
    writer
        .write_all(DKCAP_MAGIC)
        .map_err(|e| format!("Write footer magic error: {}", e))?;
    writer
        .sync_all()
        .map_err(|e| format!("Archive flush error: {}", e))?;
    drop(writer);

    if out_file.exists() {
        let _ = fs::remove_file(out_file);
    }
    fs::rename(&tmp_out, out_file).map_err(|e| {
        let _ = fs::remove_file(&tmp_out);
        format!("Failed finalizing '{}': {}", out_file.display(), e)
    })?;
    let archive_bytes = fs::metadata(out_file).map(|m| m.len()).unwrap_or(0);
    Ok((entries, archive_bytes))
}

fn read_dkcap_toc(archive_path: &Path) -> Result<(DkcapFooterToc, u64), String> {
    let mut file = File::open(archive_path)
        .map_err(|e| format!("Failed opening '{}': {}", archive_path.display(), e))?;
    let total_len = file
        .metadata()
        .map_err(|e| format!("Metadata error: {}", e))?
        .len();
    if total_len < 32 {
        return Err("Archive is too small to be a valid DKCAP001 file.".to_string());
    }
    let mut header = [0u8; 8];
    file.read_exact(&mut header)
        .map_err(|e| format!("Header read error: {}", e))?;
    if &header != DKCAP_MAGIC {
        return Err("Invalid DKCAP001 header magic.".to_string());
    }
    file.seek(SeekFrom::End(-24))
        .map_err(|e| format!("Footer seek error: {}", e))?;
    let mut footer = [0u8; 24];
    file.read_exact(&mut footer)
        .map_err(|e| format!("Footer read error: {}", e))?;
    if &footer[16..24] != DKCAP_MAGIC {
        return Err("Invalid DKCAP001 footer magic.".to_string());
    }
    let toc_offset = u64::from_le_bytes(footer[0..8].try_into().unwrap_or([0; 8]));
    let toc_len = u64::from_le_bytes(footer[8..16].try_into().unwrap_or([0; 8]));
    if toc_len == 0
        || toc_len > 32 * 1024 * 1024
        || toc_offset.saturating_add(toc_len) > total_len.saturating_sub(24)
    {
        return Err("Corrupt DKCAP001 TOC bounds.".to_string());
    }
    file.seek(SeekFrom::Start(toc_offset))
        .map_err(|e| format!("TOC seek error: {}", e))?;
    let mut toc_zst = vec![0u8; toc_len as usize];
    file.read_exact(&mut toc_zst)
        .map_err(|e| format!("TOC read error: {}", e))?;
    let toc_raw = zstd::stream::decode_all(toc_zst.as_slice())
        .map_err(|e| format!("TOC Zstd decode failed: {}", e))?;
    let toc: DkcapFooterToc =
        serde_json::from_slice(&toc_raw).map_err(|e| format!("TOC JSON parse failed: {}", e))?;
    Ok((toc, total_len))
}

#[allow(dead_code)]
pub fn extract_dkcap_single_entry(archive_path: &Path, rel_path: &str) -> Result<Vec<u8>, String> {
    let (toc, _) = read_dkcap_toc(archive_path)?;
    let target_norm = rel_path.replace('\\', "/");
    let rec = toc
        .entries
        .iter()
        .find(|e| e.rel_path.eq_ignore_ascii_case(&target_norm))
        .ok_or_else(|| format!("Entry '{}' not found in capsule.", rel_path))?;

    let mut file = File::open(archive_path)
        .map_err(|e| format!("Failed opening '{}': {}", archive_path.display(), e))?;
    file.seek(SeekFrom::Start(rec.compressed_offset))
        .map_err(|e| format!("Seek error on entry '{}': {}", rel_path, e))?;
    let mut compressed = vec![0u8; rec.compressed_len as usize];
    file.read_exact(&mut compressed)
        .map_err(|e| format!("Read error on entry '{}': {}", rel_path, e))?;
    let raw = zstd::stream::decode_all(compressed.as_slice())
        .map_err(|e| format!("Zstd decode error on '{}': {}", rel_path, e))?;
    let actual_blake = blake3::hash(&raw).to_hex().to_string();
    if actual_blake != rec.blake3 {
        return Err(format!(
            "BLAKE3 checksum mismatch extracting '{}'.",
            rel_path
        ));
    }
    Ok(raw)
}

#[allow(dead_code)]
pub fn extract_dkcap_archive(archive_path: &Path, dest_dir: &Path) -> Result<usize, String> {
    let (toc, _) = read_dkcap_toc(archive_path)?;
    fs::create_dir_all(dest_dir)
        .map_err(|e| format!("Failed creating extract dir '{}': {}", dest_dir.display(), e))?;
    let mut file = File::open(archive_path)
        .map_err(|e| format!("Failed opening '{}': {}", archive_path.display(), e))?;
    for rec in &toc.entries {
        let clean = sanitize_rel_path(&rec.rel_path)?;
        let out_path = dest_dir.join(clean);
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed creating parent '{}': {}", parent.display(), e))?;
        }
        file.seek(SeekFrom::Start(rec.compressed_offset))
            .map_err(|e| format!("Seek error: {}", e))?;
        let mut compressed = vec![0u8; rec.compressed_len as usize];
        file.read_exact(&mut compressed)
            .map_err(|e| format!("Read error: {}", e))?;
        let raw = zstd::stream::decode_all(compressed.as_slice())
            .map_err(|e| format!("Decode error on '{}': {}", rec.rel_path, e))?;
        if blake3::hash(&raw).to_hex().to_string() != rec.blake3 {
            return Err(format!("Integrity mismatch on '{}'", rec.rel_path));
        }
        fs::write(&out_path, &raw)
            .map_err(|e| format!("Write error '{}': {}", out_path.display(), e))?;
    }
    Ok(toc.entries.len())
}

pub fn export_capsule_from_spec(
    spec: &CapsuleSourceSpec,
    output_parent_dir: &Path,
    format: &str,
) -> Result<DukeCapsuleExportReport, String> {
    let started = Instant::now();
    fs::create_dir_all(output_parent_dir)
        .map_err(|e| format!("Failed creating output dir '{}': {}", output_parent_dir.display(), e))?;

    let slug = sanitize_slug(&spec.title);
    let mode_norm = format.trim().to_ascii_lowercase();
    let is_dkcap = mode_norm == "dkcap" || mode_norm == "archive" || mode_norm == "zstd";

    if is_dkcap {
        let staging_dir = output_parent_dir.join(format!(".{}_capsule_staging_{}", slug, now_secs()));
        if staging_dir.exists() {
            let _ = fs::remove_dir_all(&staging_dir);
        }
        let prep_res = (|| {
            let (mut manifest, _, _) = prepare_capsule_staging(spec, &staging_dir)?;
            let out_archive = output_parent_dir.join(format!("{}.dkcap", slug));
            let (entries, archive_bytes) =
                write_seekable_dkcap_from_dir(&staging_dir, &mut manifest, &out_archive)?;
            Ok::<_, String>((manifest, entries, archive_bytes, out_archive))
        })();
        let _ = fs::remove_dir_all(&staging_dir);
        let (manifest, entries, archive_bytes, out_archive) = prep_res?;
        let elapsed_ms = started.elapsed().as_millis() as u64;
        Ok(DukeCapsuleExportReport {
            game_id: spec.game_id.clone(),
            title: spec.title.clone(),
            format: "dkcap".to_string(),
            output_path: out_archive.to_string_lossy().into_owned(),
            file_count: entries.len(),
            uncompressed_bytes: manifest.total_uncompressed_bytes,
            archive_bytes,
            bundled_dll_count: manifest.bundled_runtime_dlls.len(),
            proxy_chain_count: manifest.mod_summary.proxy_chains.len(),
            bundled_save_count: manifest.bundled_save_roots.len(),
            elapsed_ms,
            summary: format!(
                "Packaged {} files into seekable capsule {} ({} runtime DLLs, {} proxy chains, {} save roots).",
                entries.len(),
                out_archive.display(),
                manifest.bundled_runtime_dlls.len(),
                manifest.mod_summary.proxy_chains.len(),
                manifest.bundled_save_roots.len()
            ),
        })
    } else {
        let out_folder = output_parent_dir.join(format!("{}_PortableCapsule", slug));
        if out_folder.exists() {
            let _ = fs::remove_dir_all(&out_folder);
        }
        let (manifest, file_count, total_bytes) = prepare_capsule_staging(spec, &out_folder)?;
        let elapsed_ms = started.elapsed().as_millis() as u64;
        Ok(DukeCapsuleExportReport {
            game_id: spec.game_id.clone(),
            title: spec.title.clone(),
            format: "folder".to_string(),
            output_path: out_folder.to_string_lossy().into_owned(),
            file_count,
            uncompressed_bytes: total_bytes,
            archive_bytes: total_bytes,
            bundled_dll_count: manifest.bundled_runtime_dlls.len(),
            proxy_chain_count: manifest.mod_summary.proxy_chains.len(),
            bundled_save_count: manifest.bundled_save_roots.len(),
            elapsed_ms,
            summary: format!(
                "Exported portable folder capsule to {} ({} files, {} runtime DLLs, {} save roots). Run {} on any Windows 11 PC.",
                out_folder.display(),
                file_count,
                manifest.bundled_runtime_dlls.len(),
                manifest.bundled_save_roots.len(),
                manifest.launcher_script
            ),
        })
    }
}

pub(crate) async fn export_game_capsule_core(
    app: &AppHandle,
    game_id: &str,
    output_dir: Option<String>,
    format: Option<String>,
    include_saves: bool,
) -> Result<DukeCapsuleExportReport, String> {
    let lib = crate::load_library_from_disk(app)?;
    let entry = lib
        .iter()
        .find(|g| g.id == game_id || g.title.eq_ignore_ascii_case(game_id))
        .ok_or_else(|| format!("Game '{}' not found in library.", game_id))?
        .clone();

    let exe_path = PathBuf::from(entry.exe_path.trim());
    let game_dir = if !entry.install_dir.trim().is_empty() && Path::new(&entry.install_dir).is_dir() {
        PathBuf::from(&entry.install_dir)
    } else {
        crate::resolve_game_root_dir(&exe_path)
    };

    let mut extra_save_dirs = Vec::new();
    if include_saves {
        if let Ok(save_report) =
            crate::ludusavi::discover_game_save_paths_core(&entry.id, false).await
        {
            for cand in save_report.existing_paths {
                let p = PathBuf::from(&cand.path);
                if p.exists() {
                    extra_save_dirs.push((cand.source, p));
                }
            }
        }
    }

    let out_parent = match output_dir {
        Some(ref s) if !s.trim().is_empty() => PathBuf::from(s.trim()),
        _ => crate::resolve_data_root_dir(app)?.join("Capsules"),
    };
    let fmt = format.unwrap_or_else(|| "folder".to_string());

    let spec = CapsuleSourceSpec {
        game_id: entry.id,
        title: entry.title,
        game_dir,
        exe_path,
        launch_args: entry.launch_args,
        extra_save_dirs,
    };

    tokio::task::spawn_blocking(move || export_capsule_from_spec(&spec, &out_parent, &fmt))
        .await
        .map_err(|e| format!("Capsule export task failed: {e}"))?
}

#[tauri::command]
pub async fn export_game_capsule(
    app: AppHandle,
    game_id: String,
    output_dir: Option<String>,
    format: Option<String>,
    include_saves: Option<bool>,
) -> Result<DukeCapsuleExportReport, String> {
    export_game_capsule_core(
        &app,
        &game_id,
        output_dir,
        format,
        include_saves.unwrap_or(true),
    )
    .await
}

#[tauri::command]
pub fn inspect_game_capsule(archive_path: String) -> Result<DkcapInspectReport, String> {
    let path = PathBuf::from(archive_path.trim());
    let (toc, total_len) = read_dkcap_toc(&path)?;
    Ok(DkcapInspectReport {
        archive_path: path.to_string_lossy().into_owned(),
        archive_bytes: total_len,
        entry_count: toc.entries.len(),
        manifest: toc.manifest,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_minimal_pe64_with_imports(dll_names: &[&str]) -> Vec<u8> {
        let mut buf = vec![0u8; 0x1000];
        buf[0] = b'M';
        buf[1] = b'Z';
        buf[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());

        let pe = 0x80usize;
        buf[pe..pe + 4].copy_from_slice(b"PE\0\0");
        buf[pe + 4..pe + 6].copy_from_slice(&0x8664u16.to_le_bytes());
        buf[pe + 6..pe + 8].copy_from_slice(&2u16.to_le_bytes());
        buf[pe + 20..pe + 22].copy_from_slice(&0xF0u16.to_le_bytes());
        let opt = pe + 24;
        buf[opt..opt + 2].copy_from_slice(&0x20Bu16.to_le_bytes());
        buf[opt + 108..opt + 112].copy_from_slice(&16u32.to_le_bytes());
        let dd_import = opt + 112 + 8;
        buf[dd_import..dd_import + 4].copy_from_slice(&0x2000u32.to_le_bytes());
        buf[dd_import + 4..dd_import + 8].copy_from_slice(&0x100u32.to_le_bytes());

        let sec0 = opt + 0xF0;
        buf[sec0..sec0 + 5].copy_from_slice(b".text");
        buf[sec0 + 8..sec0 + 12].copy_from_slice(&0x1000u32.to_le_bytes());
        buf[sec0 + 12..sec0 + 16].copy_from_slice(&0x1000u32.to_le_bytes());
        buf[sec0 + 16..sec0 + 20].copy_from_slice(&0x400u32.to_le_bytes());
        buf[sec0 + 20..sec0 + 24].copy_from_slice(&0x400u32.to_le_bytes());

        let sec1 = sec0 + 40;
        buf[sec1..sec1 + 6].copy_from_slice(b".idata");
        buf[sec1 + 8..sec1 + 12].copy_from_slice(&0x1000u32.to_le_bytes());
        buf[sec1 + 12..sec1 + 16].copy_from_slice(&0x2000u32.to_le_bytes());
        buf[sec1 + 16..sec1 + 20].copy_from_slice(&0x800u32.to_le_bytes());
        buf[sec1 + 20..sec1 + 24].copy_from_slice(&0x800u32.to_le_bytes());

        let idata_file = 0x800usize;
        let idata_rva = 0x2000u32;
        let mut str_cursor = (dll_names.len() + 1) * 20;
        for (i, dll) in dll_names.iter().enumerate() {
            let desc_off = idata_file + i * 20;
            let name_rva = idata_rva + str_cursor as u32;
            buf[desc_off + 12..desc_off + 16].copy_from_slice(&name_rva.to_le_bytes());
            buf[desc_off + 16..desc_off + 20].copy_from_slice(&(idata_rva + 0x400).to_le_bytes());
            let name_file = idata_file + str_cursor;
            let bytes = dll.as_bytes();
            buf[name_file..name_file + bytes.len()].copy_from_slice(bytes);
            buf[name_file + bytes.len()] = 0;
            str_cursor += bytes.len() + 1;
        }
        buf
    }

    #[test]
    fn capsule_folder_and_seekable_dkcap_bundle_runtime_mods_and_saves() {
        let tmp = std::env::temp_dir().join(format!(
            "duke_capsule_test_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let game_dir = tmp.join("Game");
        let redist_dir = game_dir.join("Redist");
        let save_src = tmp.join("HostSaves");
        let out_dir = tmp.join("Out");
        fs::create_dir_all(&redist_dir).unwrap();
        fs::create_dir_all(&save_src).unwrap();
        fs::create_dir_all(&out_dir).unwrap();

        let exe_path = game_dir.join("zomboid_mock.exe");
        let pe_bytes = build_minimal_pe64_with_imports(&["CUSTOM_RUNTIME_HELPER.dll"]);
        fs::write(&exe_path, &pe_bytes).unwrap();
        fs::write(redist_dir.join("CUSTOM_RUNTIME_HELPER.dll"), &pe_bytes).unwrap();
        fs::write(save_src.join("savegame.bin"), b"SURVIVOR_SAVE_DATA_v1").unwrap();

        let spec = CapsuleSourceSpec {
            game_id: "pz-mock".to_string(),
            title: "Project Zomboid Mock".to_string(),
            game_dir: game_dir.clone(),
            exe_path,
            launch_args: Some("-novoip".to_string()),
            extra_save_dirs: vec![("ludusavi".to_string(), save_src)],
        };

        let folder_rep = export_capsule_from_spec(&spec, &out_dir, "folder").unwrap();
        assert_eq!(folder_rep.format, "folder");
        assert!(folder_rep.bundled_dll_count >= 1);
        assert_eq!(folder_rep.bundled_save_count, 1);
        let folder_root = PathBuf::from(&folder_rep.output_path);
        assert!(folder_root.join("Launch_Portable_Capsule.cmd").is_file());
        assert!(folder_root
            .join(CAPSULE_META_DIR)
            .join("capsule-manifest.json")
            .is_file());
        assert!(folder_root
            .join(CAPSULE_META_DIR)
            .join("runtime")
            .join("CUSTOM_RUNTIME_HELPER.dll")
            .is_file());
        assert!(folder_root
            .join(".duke-profile")
            .join("AppData")
            .is_dir());

        let dkcap_rep = export_capsule_from_spec(&spec, &out_dir, "dkcap").unwrap();
        assert_eq!(dkcap_rep.format, "dkcap");
        let dkcap_path = PathBuf::from(&dkcap_rep.output_path);
        assert!(dkcap_path.is_file());

        let single_cmd =
            extract_dkcap_single_entry(&dkcap_path, "Launch_Portable_Capsule.cmd").unwrap();
        let cmd_text = String::from_utf8(single_cmd).unwrap();
        assert!(cmd_text.contains(".duke-profile\\AppData"));
        assert!(cmd_text.contains("-novoip"));

        let extracted_dir = tmp.join("Extracted");
        let count = extract_dkcap_archive(&dkcap_path, &extracted_dir).unwrap();
        assert_eq!(count, dkcap_rep.file_count);
        assert!(extracted_dir
            .join(CAPSULE_META_DIR)
            .join("saves")
            .join("01_ludusavi")
            .join("savegame.bin")
            .is_file());

        let _ = fs::remove_dir_all(&tmp);
    }
}

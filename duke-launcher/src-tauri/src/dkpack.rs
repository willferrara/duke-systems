use crate::mod_graph::{apply_mod_loadout_in_dir, scan_game_mod_graph_in_dir, TRACKED_PROXY_DLLS};
use crate::snapshots::CAS_DIR_NAME;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};
use walkdir::WalkDir;

pub const DKPACK_MAGIC: &[u8; 8] = b"DKPACK01";
pub const MAX_DKPACK_BYTES: usize = 50 * 1024; // Strict <50 KB target for shareable deterministic recipes

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DkpackFileHash {
    pub rel_path: String,
    pub size_bytes: u64,
    pub blake3: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DkpackModEntry {
    pub pack_id: String,
    pub name: String,
    pub framework: String,
    pub proxy_dlls: Vec<String>,
    pub file_hashes: Vec<DkpackFileHash>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DkpackByteSpan {
    pub offset: u64,
    pub data_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DkpackConfigDiff {
    pub rel_path: String,
    pub mode: String, // "text_override" | "patch_spans"
    #[serde(default)]
    pub content_utf8: Option<String>,
    #[serde(default)]
    pub spans: Vec<DkpackByteSpan>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DkpackPortableFile {
    pub rel_path: String,
    pub content_utf8: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DkpackRecipe {
    pub format_version: u32,
    pub pack_id: String,
    pub name: String,
    pub game_title: String,
    pub steam_app_id: Option<u32>,
    pub platform_id: u16,
    pub emulator_id: Option<String>,
    pub created_at_secs: u64,
    pub base_files: Vec<DkpackFileHash>,
    pub mod_packs: Vec<DkpackModEntry>,
    pub proxy_chain_order: Vec<String>,
    pub config_diffs: Vec<DkpackConfigDiff>,
    pub emulator_portable_configs: Vec<DkpackPortableFile>,
    #[serde(default)]
    pub recipe_blake3: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DkpackExportReport {
    pub output_path: String,
    pub pack_id: String,
    pub game_title: String,
    pub size_bytes: usize,
    pub under_50kb: bool,
    pub base_hash_count: usize,
    pub proxy_chain_order: Vec<String>,
    pub config_diff_count: usize,
    pub emulator_config_count: usize,
    pub recipe_blake3: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DkpackApplyReport {
    pub pack_id: String,
    pub pack_name: String,
    pub game_id: String,
    pub game_title: String,
    pub verified_base_files: usize,
    pub reconstructed_cas_files: usize,
    pub config_diffs_applied: usize,
    pub emulator_configs_applied: usize,
    pub isolated_profile_dir: String,
    pub proxy_chain_order: Vec<String>,
    pub summary: String,
}

fn sanitize_relative_path(raw: &str) -> Result<PathBuf, String> {
    let trimmed = raw.trim().replace('\\', "/");
    if trimmed.is_empty() {
        return Err("Empty relative path inside .dkpack recipe.".to_string());
    }
    let p = Path::new(&trimmed);
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::Normal(seg) => out.push(seg),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!("Blocked unsafe path traversal in .dkpack: {}", raw));
            }
        }
    }
    if out.as_os_str().is_empty() {
        return Err(format!("Invalid relative path in .dkpack: {}", raw));
    }
    Ok(out)
}

fn sanitize_slug(raw: &str) -> String {
    let mut s: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    while s.contains("__") {
        s = s.replace("__", "_");
    }
    s.trim_matches('_').to_string()
}

pub fn compute_recipe_digest(recipe: &DkpackRecipe) -> Result<String, String> {
    let mut clean = recipe.clone();
    clean.recipe_blake3.clear();
    let canonical =
        serde_json::to_vec(&clean).map_err(|e| format!("Failed canonicalizing .dkpack: {}", e))?;
    Ok(blake3::hash(&canonical).to_hex().to_string())
}

pub fn serialize_dkpack_bytes(recipe: &DkpackRecipe) -> Result<Vec<u8>, String> {
    let mut signed = recipe.clone();
    signed.recipe_blake3 = compute_recipe_digest(&signed)?;
    let json = serde_json::to_vec(&signed)
        .map_err(|e| format!("Failed serializing .dkpack JSON: {}", e))?;
    let compressed = zstd::stream::encode_all(json.as_slice(), 19)
        .map_err(|e| format!("Failed Zstd compressing .dkpack: {}", e))?;
    let mut out = Vec::with_capacity(DKPACK_MAGIC.len() + compressed.len());
    out.extend_from_slice(DKPACK_MAGIC);
    out.extend_from_slice(&compressed);
    if out.len() > MAX_DKPACK_BYTES {
        return Err(format!(
            ".dkpack recipe size {} bytes exceeds strict 50 KB budget",
            out.len()
        ));
    }
    Ok(out)
}

pub fn deserialize_dkpack_bytes(raw: &[u8]) -> Result<DkpackRecipe, String> {
    let recipe: DkpackRecipe = if raw.starts_with(DKPACK_MAGIC) {
        let payload = &raw[DKPACK_MAGIC.len()..];
        let decompressed = zstd::stream::decode_all(payload)
            .map_err(|e| format!("Failed Zstd decompressing .dkpack: {}", e))?;
        serde_json::from_slice(&decompressed)
            .map_err(|e| format!("Corrupt .dkpack payload JSON: {}", e))?
    } else if raw
        .iter()
        .copied()
        .find(|b| !b.is_ascii_whitespace())
        == Some(b'{')
    {
        serde_json::from_slice(raw).map_err(|e| format!("Invalid plain JSON .dkpack: {}", e))?
    } else {
        return Err("Unsupported file header: expected DKPACK01 or JSON .dkpack recipe.".to_string());
    };

    if !recipe.recipe_blake3.is_empty() {
        let expected = compute_recipe_digest(&recipe)?;
        if expected != recipe.recipe_blake3 {
            return Err("Checksum mismatch inside .dkpack recipe (recipeBlake3 failed verification).".to_string());
        }
    }
    Ok(recipe)
}

fn is_small_config_ext(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("ini" | "cfg" | "toml" | "json" | "yaml" | "yml" | "txt")
    )
}

pub fn export_game_dkpack_in_dir(
    game: &crate::GameEntry,
    game_dir: &Path,
    out_path: &Path,
    emulator_config_dir: Option<&Path>,
) -> Result<DkpackExportReport, String> {
    if !game_dir.is_dir() {
        return Err(format!(
            "Game folder does not exist: {}",
            game_dir.display()
        ));
    }

    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let slug = sanitize_slug(&game.title);
    let pack_id = format!(
        "{}_{}",
        if slug.is_empty() { "game" } else { &slug },
        now_secs
    );

    let exe_rel = Path::new(&game.exe_path)
        .strip_prefix(game_dir)
        .ok()
        .and_then(|p| p.to_str())
        .map(|s| s.replace('\\', "/"));

    // Collect core executable / base hashes (sorted deterministically, capped cleanly to stay well under 50 KB)
    let mut candidates: Vec<(String, PathBuf, u64)> = Vec::new();
    for entry in WalkDir::new(game_dir)
        .max_depth(3)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let p = entry.path();
        let Ok(rel) = p.strip_prefix(game_dir) else {
            continue;
        };
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if rel_str.starts_with(".duke-") {
            continue;
        }
        let lower_name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let is_exe = Some(rel_str.clone()) == exe_rel || lower_name.ends_with(".exe");
        let is_proxy = TRACKED_PROXY_DLLS.iter().any(|&t| t == lower_name);
        if is_exe || is_proxy {
            let meta = entry.metadata().ok();
            candidates.push((rel_str, p.to_path_buf(), meta.map(|m| m.len()).unwrap_or(0)));
        }
    }
    candidates.sort_by(|a, b| a.0.cmp(&b.0));
    candidates.truncate(32);

    let mut base_files = Vec::new();
    for (rel_str, abs_path, size_bytes) in candidates {
        if let Ok(hash) = crate::snapshots::hash_file_blake3_64k(&abs_path) {
            base_files.push(DkpackFileHash {
                rel_path: rel_str,
                size_bytes,
                blake3: hash,
            });
        }
    }

    // Collect mod packs and proxy-DLL chain order from mod_graph.rs
    let mod_report = scan_game_mod_graph_in_dir(
        &game.id,
        game_dir,
        Some(Path::new(&game.exe_path)),
        &game_dir.join(CAS_DIR_NAME),
    )?;
    let mut proxy_chain_order: Vec<String> = Vec::new();
    let mut mod_packs: Vec<DkpackModEntry> = Vec::new();

    for col in &mod_report.collisions {
        for item in &col.recommended_chain {
            if !proxy_chain_order.contains(item) {
                proxy_chain_order.push(item.clone());
            }
        }
    }
    for pack in &mod_report.packs {
        let mut dll_names = Vec::new();
        for art in &pack.proxy_dlls {
            dll_names.push(art.rel_path.clone());
            if art.is_proxy_dll && !proxy_chain_order.contains(&art.file_name_lower) {
                proxy_chain_order.push(art.file_name_lower.clone());
            }
        }
        let mut pack_hashes = Vec::new();
        let pack_root = game_dir.join(".duke-mods").join("packs").join(&pack.pack_id);
        if pack_root.is_dir() {
            for ent in WalkDir::new(&pack_root)
                .max_depth(3)
                .into_iter()
                .filter_map(|e| e.ok())
                .take(16)
            {
                if ent.file_type().is_file() {
                    if let Ok(rel) = ent.path().strip_prefix(&pack_root) {
                        if let Ok(h) = crate::snapshots::hash_file_blake3_64k(ent.path()) {
                            pack_hashes.push(DkpackFileHash {
                                rel_path: rel.to_string_lossy().replace('\\', "/"),
                                size_bytes: ent.metadata().map(|m| m.len()).unwrap_or(0),
                                blake3: h,
                            });
                        }
                    }
                }
            }
        }
        mod_packs.push(DkpackModEntry {
            pack_id: pack.pack_id.clone(),
            name: pack.name.clone(),
            framework: pack.framework.clone(),
            proxy_dlls: dll_names,
            file_hashes: pack_hashes,
        });
    }

    // Capture small deterministic config files (proxy chain INIs, mod configs, portable settings)
    let mut config_diffs: Vec<DkpackConfigDiff> = Vec::new();
    let mut text_budget: usize = 24 * 1024;
    for ent in WalkDir::new(game_dir)
        .max_depth(2)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !ent.file_type().is_file() || !is_small_config_ext(ent.path()) {
            continue;
        }
        let Ok(rel) = ent.path().strip_prefix(game_dir) else {
            continue;
        };
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if rel_str.starts_with(".duke-") && !rel_str.contains("loadout") {
            continue;
        }
        if let Ok(bytes) = fs::read(ent.path()) {
            if bytes.len() <= 6 * 1024 && bytes.len() <= text_budget {
                if let Ok(txt) = String::from_utf8(bytes) {
                    text_budget = text_budget.saturating_sub(txt.len());
                    config_diffs.push(DkpackConfigDiff {
                        rel_path: rel_str,
                        mode: "text_override".to_string(),
                        content_utf8: Some(txt),
                        spans: Vec::new(),
                    });
                }
            }
        }
        if config_diffs.len() >= 12 {
            break;
        }
    }

    // Capture portable emulator configs if this is a retro title
    let mut emulator_portable_configs = Vec::new();
    let emu_id = if game.platform_id > 0 {
        let plat = crate::platforms::platform_by_id(game.platform_id);
        let spec_opt = if !plat.default_emulator_id.is_empty() {
            crate::emulators::EMULATOR_SPECS
                .iter()
                .find(|s| s.id == plat.default_emulator_id)
        } else {
            None
        };
        if let Some(spec) = spec_opt {
            if let Some(emu_dir) = emulator_config_dir {
                if emu_dir.is_dir() {
                    let mut emu_budget: usize = 12 * 1024;
                    for ent in WalkDir::new(emu_dir)
                        .max_depth(2)
                        .into_iter()
                        .filter_map(|e| e.ok())
                    {
                        if ent.file_type().is_file() && is_small_config_ext(ent.path()) {
                            if let Ok(rel) = ent.path().strip_prefix(emu_dir) {
                                if let Ok(txt) = fs::read_to_string(ent.path()) {
                                    if txt.len() <= 4096 && txt.len() <= emu_budget {
                                        emu_budget = emu_budget.saturating_sub(txt.len());
                                        emulator_portable_configs.push(DkpackPortableFile {
                                            rel_path: rel.to_string_lossy().replace('\\', "/"),
                                            content_utf8: txt,
                                        });
                                    }
                                }
                            }
                        }
                        if emulator_portable_configs.len() >= 8 {
                            break;
                        }
                    }
                }
            }
            Some(spec.id.to_string())
        } else {
            None
        }
    } else {
        None
    };

    let mut recipe = DkpackRecipe {
        format_version: 1,
        pack_id: pack_id.clone(),
        name: format!("{} Deterministic Recipe", game.title),
        game_title: game.title.clone(),
        steam_app_id: game.steam_app_id.and_then(|id| u32::try_from(id).ok()),
        platform_id: game.platform_id,
        emulator_id: emu_id,
        created_at_secs: now_secs,
        base_files,
        mod_packs,
        proxy_chain_order,
        config_diffs,
        emulator_portable_configs,
        recipe_blake3: String::new(),
    };

    // Trim inline text configs if needed to guarantee <50 KB compressed bundle
    let encoded = match serialize_dkpack_bytes(&recipe) {
        Ok(b) => b,
        Err(_) => {
            recipe.config_diffs.truncate(4);
            recipe.emulator_portable_configs.truncate(3);
            serialize_dkpack_bytes(&recipe)?
        }
    };
    recipe.recipe_blake3 = compute_recipe_digest(&recipe)?;

    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed creating .dkpack output folder: {}", e))?;
    }
    fs::write(out_path, &encoded)
        .map_err(|e| format!("Failed writing .dkpack file {}: {}", out_path.display(), e))?;

    Ok(DkpackExportReport {
        output_path: out_path.to_string_lossy().into_owned(),
        pack_id: recipe.pack_id,
        game_title: recipe.game_title,
        size_bytes: encoded.len(),
        under_50kb: encoded.len() <= MAX_DKPACK_BYTES,
        base_hash_count: recipe.base_files.len(),
        proxy_chain_order: recipe.proxy_chain_order,
        config_diff_count: recipe.config_diffs.len(),
        emulator_config_count: recipe.emulator_portable_configs.len(),
        recipe_blake3: recipe.recipe_blake3,
    })
}

pub fn apply_dkpack_recipe_in_dir(
    recipe: &DkpackRecipe,
    game_id: &str,
    game_dir: &Path,
    emulator_target_dir: Option<&Path>,
) -> Result<DkpackApplyReport, String> {
    if !game_dir.is_dir() {
        return Err(format!("Target game folder does not exist: {}", game_dir.display()));
    }

    // 1. Validate every base game file hash BEFORE mutating disk
    let mut verified_base_files = 0usize;
    let mut cas_rel_hashes: BTreeMap<String, String> = BTreeMap::new();
    for bf in &recipe.base_files {
        let rel = sanitize_relative_path(&bf.rel_path)?;
        let abs = game_dir.join(&rel);
        if !abs.is_file() {
            return Err(format!(
                "Base game validation failed: required file '{}' is missing in '{}'",
                bf.rel_path,
                game_dir.display()
            ));
        }
        let actual_hash = crate::snapshots::hash_file_blake3_64k(&abs)?;
        if actual_hash != bf.blake3 {
            return Err(format!(
                "Base game hash mismatch on '{}': expected {}, got {}",
                bf.rel_path, bf.blake3, actual_hash
            ));
        }
        verified_base_files += 1;
        cas_rel_hashes.insert(bf.rel_path.clone(), actual_hash);
    }

    // 2. Construct deterministic .duke-cas hardlink store + .duke-profile sandbox
    let cas_blobs_dir = game_dir.join(CAS_DIR_NAME).join("blobs");
    fs::create_dir_all(&cas_blobs_dir)
        .map_err(|e| format!("Failed creating .duke-cas blobs directory: {}", e))?;
    let safe_pack_id = sanitize_slug(&recipe.pack_id);
    let profile_dir = game_dir
        .join(".duke-profile")
        .join("sandboxes")
        .join(if safe_pack_id.is_empty() { "dkpack" } else { &safe_pack_id });
    fs::create_dir_all(&profile_dir)
        .map_err(|e| format!("Failed creating .duke-profile sandbox directory: {}", e))?;

    let mut reconstructed_cas_files = 0usize;
    for (rel_str, hash) in &cas_rel_hashes {
        let rel = sanitize_relative_path(rel_str)?;
        let src = game_dir.join(&rel);
        let blob_path = cas_blobs_dir.join(hash);
        if !blob_path.exists() {
            let _ = duke_core::win32_ffi::create_hard_link(&src, &blob_path)
                .or_else(|_| fs::copy(&src, &blob_path).map(|_| ()));
        }
        let sandbox_link = profile_dir.join(&rel);
        if let Some(parent) = sandbox_link.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if sandbox_link.exists() {
            let _ = fs::remove_file(&sandbox_link);
        }
        if duke_core::win32_ffi::create_hard_link(&blob_path, &sandbox_link).is_ok()
            || fs::copy(&blob_path, &sandbox_link).is_ok()
        {
            reconstructed_cas_files += 1;
        }
    }

    // 3. Reconstruct mod loadout profile + deterministic proxy chain order
    let enabled_ids: Vec<String> = recipe.mod_packs.iter().map(|m| m.pack_id.clone()).collect();
    if !enabled_ids.is_empty() {
        let _ = crate::mod_graph::save_loadout_definition(
            game_dir,
            &recipe.name,
            enabled_ids.clone(),
        );
        let _ = apply_mod_loadout_in_dir(
            game_id,
            game_dir,
            None,
            &game_dir.join(CAS_DIR_NAME),
            Some(recipe.pack_id.as_str()),
        );
    }

    if !recipe.proxy_chain_order.is_empty() {
        let chain_path = profile_dir.join("proxy_chain_order.ini");
        let mut chain_txt = String::from("[DukeProxyChain]\n");
        for (idx, dll) in recipe.proxy_chain_order.iter().enumerate() {
            chain_txt.push_str(&format!("order_{idx}={dll}\n"));
        }
        fs::write(&chain_path, chain_txt.as_bytes())
            .map_err(|e| format!("Failed writing proxy chain INI: {}", e))?;
    }

    // 4. Apply deterministic config overrides / binary span patches inside sandbox & game folder
    let mut config_diffs_applied = 0usize;
    for diff in &recipe.config_diffs {
        let rel = sanitize_relative_path(&diff.rel_path)?;
        let sandbox_target = profile_dir.join(&rel);
        if let Some(parent) = sandbox_target.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed preparing config sandbox dir: {}", e))?;
        }
        match diff.mode.as_str() {
            "text_override" => {
                if let Some(ref content) = diff.content_utf8 {
                    fs::write(&sandbox_target, content.as_bytes())
                        .map_err(|e| format!("Failed writing sandbox config '{}': {}", diff.rel_path, e))?;
                    config_diffs_applied += 1;
                }
            }
            "patch_spans" => {
                let live_file = game_dir.join(&rel);
                if live_file.is_file() {
                    let mut buf = fs::read(&live_file)
                        .map_err(|e| format!("Failed reading patch target '{}': {}", diff.rel_path, e))?;
                    for sp in &diff.spans {
                        let bytes = decode_hex_bytes(&sp.data_hex)?;
                        let off = usize::try_from(sp.offset)
                            .map_err(|_| "Patch offset overflow".to_string())?;
                        let end = off
                            .checked_add(bytes.len())
                            .ok_or_else(|| "Patch span overflow".to_string())?;
                        if end > buf.len() {
                            return Err(format!(
                                "Binary config patch out of bounds on '{}' ({} > {})",
                                diff.rel_path,
                                end,
                                buf.len()
                            ));
                        }
                        buf[off..end].copy_from_slice(&bytes);
                    }
                    let mut f = fs::File::create(&sandbox_target)
                        .map_err(|e| format!("Failed writing patched sandbox config: {}", e))?;
                    f.write_all(&buf)
                        .map_err(|e| format!("Failed flushing patched sandbox config: {}", e))?;
                    config_diffs_applied += 1;
                }
            }
            _ => {}
        }
    }

    // 5. Reconstruct portable emulator configs if present
    let mut emulator_configs_applied = 0usize;
    if !recipe.emulator_portable_configs.is_empty() {
        let target_emu_root = emulator_target_dir
            .map(PathBuf::from)
            .unwrap_or_else(|| profile_dir.join("emulator_portable"));
        fs::create_dir_all(&target_emu_root)
            .map_err(|e| format!("Failed creating portable emulator config dir: {}", e))?;
        for cfg in &recipe.emulator_portable_configs {
            let rel = sanitize_relative_path(&cfg.rel_path)?;
            let dest = target_emu_root.join(&rel);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed creating emulator sub-dir: {}", e))?;
            }
            fs::write(&dest, cfg.content_utf8.as_bytes())
                .map_err(|e| format!("Failed writing portable emulator config '{}': {}", cfg.rel_path, e))?;
            emulator_configs_applied += 1;
        }
    }

    Ok(DkpackApplyReport {
        pack_id: recipe.pack_id.clone(),
        pack_name: recipe.name.clone(),
        game_id: game_id.to_string(),
        game_title: recipe.game_title.clone(),
        verified_base_files,
        reconstructed_cas_files,
        config_diffs_applied,
        emulator_configs_applied,
        isolated_profile_dir: profile_dir.to_string_lossy().into_owned(),
        proxy_chain_order: recipe.proxy_chain_order.clone(),
        summary: format!(
            "Applied '{}': verified {} base hash(es), built {} .duke-cas hardlink(s), applied {} config diff(s) inside .duke-profile/.",
            recipe.name, verified_base_files, reconstructed_cas_files, config_diffs_applied
        ),
    })
}

fn decode_hex_bytes(hex: &str) -> Result<Vec<u8>, String> {
    let clean = hex.trim();
    if clean.len() % 2 != 0 {
        return Err("Invalid odd-length hex patch payload".to_string());
    }
    let mut out = Vec::with_capacity(clean.len() / 2);
    for pair in clean.as_bytes().chunks_exact(2) {
        let s = std::str::from_utf8(pair).map_err(|_| "Non-UTF8 hex patch".to_string())?;
        let val = u8::from_str_radix(s, 16)
            .map_err(|_| format!("Invalid hex byte '{}' in .dkpack patch", s))?;
        out.push(val);
    }
    Ok(out)
}

pub fn load_dkpack_file(path: &Path) -> Result<DkpackRecipe, String> {
    let bytes = fs::read(path)
        .map_err(|e| format!("Failed reading .dkpack file '{}': {}", path.display(), e))?;
    deserialize_dkpack_bytes(&bytes)
}

pub async fn ingest_dropped_dkpack_file(
    app: &AppHandle,
    dkpack_path: &Path,
    explicit_game_id: Option<&str>,
) -> Result<DkpackApplyReport, String> {
    let recipe = load_dkpack_file(dkpack_path)?;
    let library = crate::load_library_from_disk(app)?;

    let matched = if let Some(gid) = explicit_game_id.filter(|s| !s.trim().is_empty()) {
        library
            .iter()
            .find(|g| g.id == gid || g.title.eq_ignore_ascii_case(gid))
            .cloned()
    } else {
        library
            .iter()
            .find(|g| {
                (recipe.steam_app_id.is_some()
                    && g.steam_app_id.and_then(|id| u32::try_from(id).ok()) == recipe.steam_app_id)
                    || g.title.eq_ignore_ascii_case(&recipe.game_title)
                    || g.title
                        .to_ascii_lowercase()
                        .contains(&recipe.game_title.to_ascii_lowercase())
            })
            .cloned()
    };

    let Some(game) = matched else {
        return Err(format!(
            "No matching installed game found for .dkpack recipe '{}' (target: '{}'). Add the base game first.",
            recipe.name, recipe.game_title
        ));
    };

    let game_dir = if !game.install_dir.trim().is_empty() && Path::new(&game.install_dir).is_dir() {
        PathBuf::from(&game.install_dir)
    } else {
        crate::resolve_game_root_dir(Path::new(&game.exe_path))
    };

    let emu_dir = recipe.emulator_id.as_ref().and_then(|eid| {
        crate::emulators::emulators_root_dir()
            .ok()
            .map(|r| r.join(eid))
            .filter(|p| p.exists())
    });

    let report = tokio::task::spawn_blocking(move || {
        apply_dkpack_recipe_in_dir(&recipe, &game.id, &game_dir, emu_dir.as_deref())
    })
    .await
    .map_err(|e| format!("DKPack apply worker failed: {}", e))??;

    let _ = app.emit("dkpack-applied", &report);
    Ok(report)
}

#[tauri::command]
pub async fn export_game_dkpack(
    app: AppHandle,
    game_id: String,
    out_path: Option<String>,
) -> Result<DkpackExportReport, String> {
    let library = crate::load_library_from_disk(&app)?;
    let game = library
        .into_iter()
        .find(|g| g.id == game_id || g.title.eq_ignore_ascii_case(&game_id))
        .ok_or_else(|| format!("Game '{}' not found in library.", game_id))?;

    let game_dir = if !game.install_dir.trim().is_empty() && Path::new(&game.install_dir).is_dir() {
        PathBuf::from(&game.install_dir)
    } else {
        crate::resolve_game_root_dir(Path::new(&game.exe_path))
    };

    let dest = match out_path.filter(|s| !s.trim().is_empty()) {
        Some(p) => PathBuf::from(p),
        None => {
            let root = crate::resolve_data_root_dir(&app)?;
            let slug = sanitize_slug(&game.title);
            root.join("dkpacks")
                .join(format!("{}.dkpack", if slug.is_empty() { "recipe" } else { &slug }))
        }
    };

    let emu_dir = if game.platform_id > 0 {
        crate::emulators::emulators_root_dir().ok()
    } else {
        None
    };

    tokio::task::spawn_blocking(move || {
        export_game_dkpack_in_dir(&game, &game_dir, &dest, emu_dir.as_deref())
    })
    .await
    .map_err(|e| format!("DKPack export worker failed: {}", e))?
}

#[tauri::command]
pub async fn inspect_dkpack_file(path: String) -> Result<DkpackRecipe, String> {
    let p = PathBuf::from(path.trim());
    tokio::task::spawn_blocking(move || load_dkpack_file(&p))
        .await
        .map_err(|e| format!("DKPack inspect worker failed: {}", e))?
}

#[tauri::command]
pub async fn apply_dkpack_recipe(
    app: AppHandle,
    dkpack_path: String,
    game_id: Option<String>,
) -> Result<DkpackApplyReport, String> {
    ingest_dropped_dkpack_file(&app, Path::new(dkpack_path.trim()), game_id.as_deref()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dkpack_roundtrip_under_50kb_and_rejects_hash_mismatch() {
        let dir = std::env::temp_dir().join(format!("duke_dkpack_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let game_dir = dir.join("EldenGame");
        fs::create_dir_all(&game_dir).unwrap();
        let exe = game_dir.join("elden.exe");
        fs::write(&exe, b"MZ-VANILLA-GAME-EXECUTABLE-HEADER").unwrap();
        fs::write(game_dir.join("version.dll"), b"MZ-PROXY-DLL").unwrap();
        fs::write(game_dir.join("modengine.ini"), b"[mods]\ncoop=true\n").unwrap();

        let game = crate::GameEntry {
            id: "elden_1".to_string(),
            title: "Elden Ring".to_string(),
            exe_path: exe.to_string_lossy().into_owned(),
            install_dir: game_dir.to_string_lossy().into_owned(),
            steam_app_id: Some(1245620),
            ..Default::default()
        };

        let out_pack = dir.join("elden_coop.dkpack");
        let export_rep = export_game_dkpack_in_dir(&game, &game_dir, &out_pack, None).unwrap();
        assert!(export_rep.under_50kb);
        assert!(export_rep.size_bytes < MAX_DKPACK_BYTES);
        assert!(export_rep.base_hash_count >= 1);

        let loaded = load_dkpack_file(&out_pack).unwrap();
        let apply_rep = apply_dkpack_recipe_in_dir(&loaded, &game.id, &game_dir, None).unwrap();
        assert!(apply_rep.verified_base_files >= 1);
        assert!(apply_rep.reconstructed_cas_files >= 1);
        assert!(Path::new(&apply_rep.isolated_profile_dir).join("modengine.ini").is_file());

        // Mutate base file -> must fail BLAKE3 verification before modifying anything
        fs::write(&exe, b"CORRUPTED-OR-WRONG-PATCH-VERSION").unwrap();
        let err = apply_dkpack_recipe_in_dir(&loaded, &game.id, &game_dir, None).unwrap_err();
        assert!(err.contains("hash mismatch") || err.contains("Base game"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dkpack_blocks_path_traversal() {
        assert!(sanitize_relative_path("../../Windows/System32/cmd.exe").is_err());
        assert!(sanitize_relative_path("C:\\evil.dll").is_err());
        assert!(sanitize_relative_path("mods/seamless_coop.dll").is_ok());
    }
}

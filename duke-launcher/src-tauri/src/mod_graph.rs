use crate::pe_inspect::{parse_pe_exports_file, PeExportInfo};
use crate::projfs_stream::store_cas_block;
use crate::snapshots::CAS_DIR_NAME;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

const MODS_ROOT_DIR: &str = ".duke-mods";
const PACKS_DIR: &str = "packs";
const STATE_FILE: &str = "mod_graph_state.json";

pub const TRACKED_PROXY_DLLS: &[&str] = &[
    "version.dll",
    "winmm.dll",
    "dinput8.dll",
    "dxgi.dll",
    "dsound.dll",
    "d3d9.dll",
    "d3d11.dll",
    "winhttp.dll",
    "dwmapi.dll",
    "xinput1_3.dll",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModDllArtifact {
    pub rel_path: String,
    pub file_name_lower: String,
    pub is_proxy_dll: bool,
    pub detected_framework: String,
    pub is_32bit: bool,
    pub internal_dll_name: Option<String>,
    pub export_count: usize,
    pub sample_exports: Vec<String>,
    pub has_forwarders: bool,
    pub blake3: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModPackDescriptor {
    pub pack_id: String,
    pub name: String,
    pub framework: String,
    pub file_count: usize,
    pub total_bytes: u64,
    pub proxy_dlls: Vec<ModDllArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyDllCollision {
    pub proxy_dll: String,
    pub severity: String,
    pub contending_packs: Vec<String>,
    pub recommended_chain: Vec<String>,
    pub generated_config_file: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModLoadoutProfile {
    pub loadout_id: String,
    pub name: String,
    pub enabled_pack_ids: Vec<String>,
    pub isolated_profile_dir: String,
    pub updated_at_secs: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModGraphDiskState {
    active_loadout_id: Option<String>,
    deployed_rel_paths: Vec<String>,
    loadouts: Vec<ModLoadoutProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModGraphReport {
    pub game_id: String,
    pub game_dir: String,
    pub exe_is_32bit: Option<bool>,
    pub packs: Vec<ModPackDescriptor>,
    pub collisions: Vec<ProxyDllCollision>,
    pub loadouts: Vec<ModLoadoutProfile>,
    pub active_loadout_id: Option<String>,
    pub deployed_file_count: usize,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModLoadoutApplyResult {
    pub game_id: String,
    pub loadout_id: Option<String>,
    pub loadout_name: String,
    pub hardlinked_files: usize,
    pub chain_configs_written: Vec<String>,
    pub isolated_profile_dir: String,
    pub elapsed_ms: f64,
    pub report: ModGraphReport,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn sanitize_id(raw: &str) -> String {
    let clean: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    clean.trim_matches('-').to_string()
}

pub fn is_tracked_proxy_dll(file_name_lower: &str) -> bool {
    TRACKED_PROXY_DLLS.contains(&file_name_lower)
}

pub fn classify_mod_framework(
    pack_name_or_rel: &str,
    dll_name_lower: &str,
    exports: Option<&PeExportInfo>,
) -> String {
    let lower_path = pack_name_or_rel.to_ascii_lowercase();
    let export_names: Vec<String> = exports
        .map(|e| {
            e.exported_names
                .iter()
                .map(|s| s.to_ascii_lowercase())
                .collect()
        })
        .unwrap_or_default();

    if export_names.iter().any(|n| n.starts_with("reshade"))
        || lower_path.contains("reshade")
    {
        return "ReShade".to_string();
    }
    if export_names.iter().any(|n| n.starts_with("sk_") || n.contains("specialk"))
        || lower_path.contains("specialk")
        || dll_name_lower.starts_with("specialk")
    {
        return "SpecialK".to_string();
    }
    if lower_path.contains("bepinex") || dll_name_lower == "winhttp.dll" {
        return "BepInEx".to_string();
    }
    if lower_path.contains("ue4ss") || dll_name_lower == "dwmapi.dll" {
        return "UE4SS".to_string();
    }
    if export_names.iter().any(|n| n.contains("dxvk")) || lower_path.contains("dxvk") {
        return "DXVK".to_string();
    }
    if lower_path.contains("asi")
        || matches!(
            dll_name_lower,
            "version.dll" | "winmm.dll" | "dinput8.dll" | "dsound.dll"
        )
    {
        return "ASI Loader".to_string();
    }
    "Custom Mod".to_string()
}

fn load_disk_state(mods_root: &Path) -> ModGraphDiskState {
    let path = mods_root.join(STATE_FILE);
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_disk_state(mods_root: &Path, state: &ModGraphDiskState) -> Result<(), String> {
    fs::create_dir_all(mods_root)
        .map_err(|e| format!("Failed creating {}: {e}", mods_root.display()))?;
    let json = serde_json::to_string_pretty(state)
        .map_err(|e| format!("Failed serializing mod graph state: {e}"))?;
    fs::write(mods_root.join(STATE_FILE), json)
        .map_err(|e| format!("Failed writing mod graph state: {e}"))
}

fn scan_single_pack(pack_dir: &Path, cas_root: &Path) -> ModPackDescriptor {
    let pack_id = pack_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("mod-pack")
        .to_string();
    let mut proxy_dlls = Vec::new();
    let mut file_count = 0usize;
    let mut total_bytes = 0u64;
    let mut detected_frameworks: HashMap<String, usize> = HashMap::new();

    for entry in WalkDir::new(pack_dir)
        .follow_links(false)
        .into_iter()
        .flatten()
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        file_count += 1;
        let len = entry.metadata().map(|m| m.len()).unwrap_or(0);
        total_bytes = total_bytes.saturating_add(len);

        let Ok(rel) = path.strip_prefix(pack_dir) else {
            continue;
        };
        let rel_norm = rel.to_string_lossy().replace('\\', "/");
        let name_lower = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        if name_lower.ends_with(".dll") {
            let export_opt = parse_pe_exports_file(path).ok();
            let fw = classify_mod_framework(&format!("{pack_id}/{rel_norm}"), &name_lower, export_opt.as_ref());
            *detected_frameworks.entry(fw.clone()).or_insert(0) += 1;

            let is_proxy = is_tracked_proxy_dll(&name_lower);
            let blake3 = fs::read(path)
                .ok()
                .and_then(|b| store_cas_block(cas_root, &b).ok())
                .unwrap_or_default();

            if let Some(exp) = export_opt {
                let has_fwd = exp.symbols.iter().any(|s| s.forwarder.is_some());
                proxy_dlls.push(ModDllArtifact {
                    rel_path: rel_norm,
                    file_name_lower: name_lower,
                    is_proxy_dll: is_proxy,
                    detected_framework: fw,
                    is_32bit: exp.is_32bit,
                    internal_dll_name: exp.dll_name,
                    export_count: exp.symbols.len(),
                    sample_exports: exp.exported_names.into_iter().take(6).collect(),
                    has_forwarders: has_fwd,
                    blake3,
                });
            }
        }
    }

    let framework = detected_frameworks
        .into_iter()
        .max_by_key(|(_, c)| *c)
        .map(|(k, _)| k)
        .unwrap_or_else(|| classify_mod_framework(&pack_id, "", None));

    ModPackDescriptor {
        pack_id: pack_id.clone(),
        name: pack_id,
        framework,
        file_count,
        total_bytes,
        proxy_dlls,
    }
}

/// Evaluates deterministic proxy-DLL collision graph across selected mod packs and game bitness.
pub fn analyze_proxy_collisions(
    packs: &[ModPackDescriptor],
    exe_is_32bit: Option<bool>,
) -> Vec<ProxyDllCollision> {
    let mut by_proxy: BTreeMap<String, Vec<(String, &ModDllArtifact)>> = BTreeMap::new();
    let mut collisions = Vec::new();

    for pack in packs {
        for dll in &pack.proxy_dlls {
            if let Some(target_32) = exe_is_32bit {
                if dll.is_32bit != target_32 {
                    collisions.push(ProxyDllCollision {
                        proxy_dll: dll.rel_path.clone(),
                        severity: "arch-mismatch".to_string(),
                        contending_packs: vec![pack.pack_id.clone()],
                        recommended_chain: Vec::new(),
                        generated_config_file: String::new(),
                        description: format!(
                            "Bitness mismatch: '{}' in '{}' is {}, but game executable is {}.",
                            dll.rel_path,
                            pack.pack_id,
                            if dll.is_32bit { "32-bit" } else { "64-bit" },
                            if target_32 { "32-bit" } else { "64-bit" }
                        ),
                    });
                }
            }
            if dll.is_proxy_dll {
                by_proxy
                    .entry(dll.file_name_lower.clone())
                    .or_default()
                    .push((pack.pack_id.clone(), dll));
            }
        }
    }

    // Priority order for chain-capable loaders when multiple mods collide on one proxy slot.
    let chain_rank = |fw: &str| -> u8 {
        match fw {
            "SpecialK" => 1,
            "ASI Loader" => 2,
            "DXVK" => 3,
            "ReShade" => 4,
            _ => 5,
        }
    };

    for (proxy_dll, mut contenders) in by_proxy {
        if contenders.len() < 2 {
            continue;
        }
        contenders.sort_by_key(|(_, dll)| chain_rank(&dll.detected_framework));
        let pack_names: Vec<String> = contenders.iter().map(|(p, _)| p.clone()).collect();
        let chain_desc: Vec<String> = contenders
            .iter()
            .map(|(p, d)| format!("{p} ({})", d.detected_framework))
            .collect();

        let primary_fw = contenders[0].1.detected_framework.as_str();
        let cfg = match primary_fw {
            "SpecialK" => "SpecialK.ini".to_string(),
            "ASI Loader" => "duke_proxy_chain.ini".to_string(),
            "DXVK" => "dxvk_chain.conf".to_string(),
            _ => "ReShade.ini".to_string(),
        };

        collisions.push(ProxyDllCollision {
            proxy_dll: proxy_dll.clone(),
            severity: "auto-chainable".to_string(),
            contending_packs: pack_names,
            recommended_chain: chain_desc,
            generated_config_file: cfg.clone(),
            description: format!(
                "Multiple packs target '{}'; auto-chain via {} without modifying base game files.",
                proxy_dll, cfg
            ),
        });
    }

    collisions
}

pub fn scan_game_mod_graph_in_dir(
    game_id: &str,
    game_dir: &Path,
    exe_path: Option<&Path>,
    cas_root: &Path,
) -> Result<ModGraphReport, String> {
    let mods_root = game_dir.join(MODS_ROOT_DIR);
    let packs_root = mods_root.join(PACKS_DIR);
    let state = load_disk_state(&mods_root);
    let exe_is_32bit = exe_path
        .filter(|p| p.is_file())
        .and_then(|p| parse_pe_exports_file(p).ok().map(|i| i.is_32bit));

    let mut packs = Vec::new();
    if packs_root.is_dir() {
        if let Ok(rd) = fs::read_dir(&packs_root) {
            for ent in rd.flatten() {
                if ent.path().is_dir() {
                    packs.push(scan_single_pack(&ent.path(), cas_root));
                }
            }
        }
    }
    packs.sort_by(|a, b| a.pack_id.cmp(&b.pack_id));

    let collisions = analyze_proxy_collisions(&packs, exe_is_32bit);
    let summary = format!(
        "Scanned {} mod pack(s), {} proxy collision(s), {} loadout(s) (active: {}).",
        packs.len(),
        collisions.len(),
        state.loadouts.len(),
        state.active_loadout_id.as_deref().unwrap_or("Vanilla")
    );

    Ok(ModGraphReport {
        game_id: game_id.to_string(),
        game_dir: game_dir.to_string_lossy().into_owned(),
        exe_is_32bit,
        packs,
        collisions,
        loadouts: state.loadouts,
        active_loadout_id: state.active_loadout_id,
        deployed_file_count: state.deployed_rel_paths.len(),
        summary,
    })
}

fn ensure_cas_file_for_pack_item(src: &Path, cas_root: &Path) -> Result<PathBuf, String> {
    let data = fs::read(src)
        .map_err(|e| format!("Failed reading mod file {} for CAS: {e}", src.display()))?;
    let hash = store_cas_block(cas_root, &data)?;
    let prefix = hash.get(..2).unwrap_or("00");
    let cas_file = cas_root
        .join("mod_files")
        .join(prefix)
        .join(format!("{hash}.dkcas"));
    if !cas_file.is_file() {
        if let Some(p) = cas_file.parent() {
            fs::create_dir_all(p)
                .map_err(|e| format!("Failed creating CAS mod folder: {e}"))?;
        }
        fs::write(&cas_file, &data)
            .map_err(|e| format!("Failed caching mod CAS file: {e}"))?;
    }
    Ok(cas_file)
}

fn remove_previously_deployed(game_dir: &Path, state: &mut ModGraphDiskState) {
    for rel in state.deployed_rel_paths.drain(..) {
        let p = game_dir.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        if p.is_file() {
            let _ = fs::remove_file(p);
        }
    }
    state.active_loadout_id = None;
}

/// Applies a deterministic mod loadout using `<50 ms` `.duke-cas` hardlinks + auto wrapper chain INIs,
/// strictly preserving all existing base game files untouched.
pub fn apply_mod_loadout_in_dir(
    game_id: &str,
    game_dir: &Path,
    exe_path: Option<&Path>,
    cas_root: &Path,
    loadout_id: Option<&str>,
) -> Result<ModLoadoutApplyResult, String> {
    let started = Instant::now();
    let mods_root = game_dir.join(MODS_ROOT_DIR);
    let packs_root = mods_root.join(PACKS_DIR);
    let mut state = load_disk_state(&mods_root);

    // 1. Cleanly unlink previous Duke-deployed mod files ONLY (never deletes base game files).
    remove_previously_deployed(game_dir, &mut state);

    let Some(target_lid) = loadout_id.map(str::trim).filter(|s| !s.is_empty()) else {
        save_disk_state(&mods_root, &state)?;
        let report = scan_game_mod_graph_in_dir(game_id, game_dir, exe_path, cas_root)?;
        return Ok(ModLoadoutApplyResult {
            game_id: game_id.to_string(),
            loadout_id: None,
            loadout_name: "Vanilla".to_string(),
            hardlinked_files: 0,
            chain_configs_written: Vec::new(),
            isolated_profile_dir: String::new(),
            elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
            report,
        });
    };

    let loadout = state
        .loadouts
        .iter()
        .find(|l| l.loadout_id == target_lid || l.name.eq_ignore_ascii_case(target_lid))
        .cloned()
        .ok_or_else(|| format!("Mod loadout '{}' not found.", target_lid))?;

    let mut active_packs = Vec::new();
    for pid in &loadout.enabled_pack_ids {
        let pdir = packs_root.join(pid);
        if pdir.is_dir() {
            active_packs.push(scan_single_pack(&pdir, cas_root));
        }
    }

    let exe_is_32bit = exe_path
        .filter(|p| p.is_file())
        .and_then(|p| parse_pe_exports_file(p).ok().map(|i| i.is_32bit));
    let collisions = analyze_proxy_collisions(&active_packs, exe_is_32bit);

    let mut chain_secondary_map: HashMap<(String, String), String> = HashMap::new();
    let mut chain_configs: HashMap<String, String> = HashMap::new();

    for col in &collisions {
        if col.severity != "auto-chainable" || col.contending_packs.len() < 2 {
            continue;
        }
        let primary_pack = &col.contending_packs[0];
        let mut sk_imports = String::new();
        let mut proxy_targets = Vec::new();

        for (idx, pack_id) in col.contending_packs.iter().enumerate() {
            if idx == 0 {
                continue;
            }
            let stem = Path::new(&col.proxy_dll)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("proxy");
            let chain_name = format!("{}_{}_chain.dll", stem, sanitize_id(pack_id));
            chain_secondary_map.insert(
                (pack_id.clone(), col.proxy_dll.to_ascii_lowercase()),
                chain_name.clone(),
            );
            proxy_targets.push(chain_name.clone());
            sk_imports.push_str(&format!(
                "[Import.Duke_{idx}]\nArchitecture=Win64\nFilename={chain_name}\nRole=ThirdParty\nWhen=PlugIn\n\n"
            ));
        }

        if col.generated_config_file == "SpecialK.ini" {
            chain_configs.insert(
                "SpecialK.ini".to_string(),
                format!("; Auto-generated by Duke Launcher Mod Graph ({primary_pack})\n{sk_imports}"),
            );
        } else {
            let entry = chain_configs
                .entry(col.generated_config_file.clone())
                .or_default();
            entry.push_str(&format!(
                "[ProxyChain_{}]\nPrimary={}\nProxyLibrary={}\nEnableProxyLibrary=1\n\n",
                col.proxy_dll,
                primary_pack,
                proxy_targets.join(",")
            ));
        }
    }

    // Create isolated `.duke-profile/loadouts/<loadout_id>` directory.
    let isolated_dir = game_dir
        .join(".duke-profile")
        .join("loadouts")
        .join(&loadout.loadout_id);
    let _ = fs::create_dir_all(&isolated_dir);

    let mut deployed_set: HashSet<String> = HashSet::new();
    let mut hardlinked = 0usize;

    for pack in &active_packs {
        let pack_dir = packs_root.join(&pack.pack_id);
        for ent in WalkDir::new(&pack_dir).follow_links(false).into_iter().flatten() {
            if !ent.file_type().is_file() {
                continue;
            }
            let Ok(rel) = ent.path().strip_prefix(&pack_dir) else {
                continue;
            };
            let mut rel_norm = rel.to_string_lossy().replace('\\', "/");
            let file_lower = rel
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();

            if let Some(remapped) = chain_secondary_map.get(&(pack.pack_id.clone(), file_lower)) {
                rel_norm = remapped.clone();
            }

            let dst = game_dir.join(rel_norm.replace('/', std::path::MAIN_SEPARATOR_STR));
            // Never overwrite an untouched base game file.
            if dst.exists() && !deployed_set.contains(&rel_norm) {
                continue;
            }
            if let Some(p) = dst.parent() {
                let _ = fs::create_dir_all(p);
            }
            let cas_src = ensure_cas_file_for_pack_item(ent.path(), cas_root)?;
            if dst.exists() {
                let _ = fs::remove_file(&dst);
            }
            if duke_core::win32_ffi::create_hard_link(&dst, &cas_src).is_ok()
                || fs::copy(&cas_src, &dst).is_ok()
            {
                hardlinked += 1;
                deployed_set.insert(rel_norm.clone());
                state.deployed_rel_paths.push(rel_norm);
            }
        }
    }

    let mut configs_written = Vec::new();
    for (cfg_name, body) in chain_configs {
        let dst = game_dir.join(&cfg_name);
        if !dst.exists() && fs::write(&dst, body).is_ok() {
            state.deployed_rel_paths.push(cfg_name.clone());
            configs_written.push(cfg_name);
        }
    }

    state.active_loadout_id = Some(loadout.loadout_id.clone());
    save_disk_state(&mods_root, &state)?;

    let elapsed_ms = (started.elapsed().as_secs_f64() * 1000.0 * 100.0).round() / 100.0;
    let report = scan_game_mod_graph_in_dir(game_id, game_dir, exe_path, cas_root)?;

    Ok(ModLoadoutApplyResult {
        game_id: game_id.to_string(),
        loadout_id: Some(loadout.loadout_id),
        loadout_name: loadout.name,
        hardlinked_files: hardlinked,
        chain_configs_written: configs_written,
        isolated_profile_dir: isolated_dir.to_string_lossy().into_owned(),
        elapsed_ms,
        report,
    })
}

pub fn register_mod_pack_from_dir(
    game_dir: &Path,
    source_dir: &Path,
    pack_name: &str,
) -> Result<String, String> {
    if !source_dir.is_dir() {
        return Err(format!("Mod source folder does not exist: {}", source_dir.display()));
    }
    let clean = sanitize_id(pack_name);
    let pack_id = if clean.is_empty() {
        "custom-mod".to_string()
    } else {
        clean
    };
    let dst_pack = game_dir.join(MODS_ROOT_DIR).join(PACKS_DIR).join(&pack_id);
    fs::create_dir_all(&dst_pack)
        .map_err(|e| format!("Failed creating mod pack folder: {e}"))?;

    for ent in WalkDir::new(source_dir).follow_links(false).into_iter().flatten() {
        if !ent.file_type().is_file() {
            continue;
        }
        let Ok(rel) = ent.path().strip_prefix(source_dir) else {
            continue;
        };
        let dst = dst_pack.join(rel);
        if let Some(p) = dst.parent() {
            fs::create_dir_all(p)
                .map_err(|e| format!("Failed creating mod pack subfolder: {e}"))?;
        }
        fs::copy(ent.path(), &dst)
            .map_err(|e| format!("Failed copying mod pack file {}: {e}", rel.display()))?;
    }
    Ok(pack_id)
}

pub fn save_loadout_definition(
    game_dir: &Path,
    loadout_name: &str,
    enabled_pack_ids: Vec<String>,
) -> Result<ModLoadoutProfile, String> {
    let mods_root = game_dir.join(MODS_ROOT_DIR);
    let mut state = load_disk_state(&mods_root);
    let clean = sanitize_id(loadout_name);
    let loadout_id = if clean.is_empty() {
        format!("loadout-{}", now_secs())
    } else {
        clean
    };
    let iso_dir = game_dir
        .join(".duke-profile")
        .join("loadouts")
        .join(&loadout_id)
        .to_string_lossy()
        .into_owned();
    let prof = ModLoadoutProfile {
        loadout_id: loadout_id.clone(),
        name: loadout_name.trim().to_string(),
        enabled_pack_ids,
        isolated_profile_dir: iso_dir,
        updated_at_secs: now_secs(),
    };
    if let Some(existing) = state
        .loadouts
        .iter_mut()
        .find(|l| l.loadout_id == loadout_id)
    {
        *existing = prof.clone();
    } else {
        state.loadouts.push(prof.clone());
    }
    save_disk_state(&mods_root, &state)?;
    Ok(prof)
}

fn game_paths_for_id(app: &tauri::AppHandle, game_id: &str) -> Result<(PathBuf, Option<PathBuf>), String> {
    let lib = crate::load_library_lightweight(app)?;
    let entry = lib
        .into_iter()
        .find(|g| g.id == game_id)
        .ok_or_else(|| format!("Game '{}' not found in library.", game_id))?;
    let exe = PathBuf::from(&entry.exe_path);
    let gdir = if !entry.install_dir.trim().is_empty() {
        PathBuf::from(&entry.install_dir)
    } else {
        exe.parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    };
    Ok((gdir, exe.is_file().then_some(exe)))
}

#[tauri::command]
pub async fn scan_game_mod_graph(
    app: tauri::AppHandle,
    game_id: String,
) -> Result<ModGraphReport, String> {
    let (gdir, exe) = game_paths_for_id(&app, &game_id)?;
    let cas_root = gdir.join(CAS_DIR_NAME);
    tokio::task::spawn_blocking(move || {
        scan_game_mod_graph_in_dir(&game_id, &gdir, exe.as_deref(), &cas_root)
    })
    .await
    .map_err(|e| format!("Mod graph scan task failed: {e}"))?
}

#[tauri::command]
pub async fn register_game_mod_pack(
    app: tauri::AppHandle,
    game_id: String,
    source_dir: String,
    pack_name: String,
) -> Result<ModGraphReport, String> {
    let (gdir, exe) = game_paths_for_id(&app, &game_id)?;
    let cas_root = gdir.join(CAS_DIR_NAME);
    tokio::task::spawn_blocking(move || {
        let _ = register_mod_pack_from_dir(&gdir, Path::new(&source_dir), &pack_name)?;
        scan_game_mod_graph_in_dir(&game_id, &gdir, exe.as_deref(), &cas_root)
    })
    .await
    .map_err(|e| format!("Mod pack registration task failed: {e}"))?
}

#[tauri::command]
pub async fn save_game_mod_loadout(
    app: tauri::AppHandle,
    game_id: String,
    loadout_name: String,
    enabled_pack_ids: Vec<String>,
) -> Result<ModGraphReport, String> {
    let (gdir, exe) = game_paths_for_id(&app, &game_id)?;
    let cas_root = gdir.join(CAS_DIR_NAME);
    tokio::task::spawn_blocking(move || {
        let _ = save_loadout_definition(&gdir, &loadout_name, enabled_pack_ids)?;
        scan_game_mod_graph_in_dir(&game_id, &gdir, exe.as_deref(), &cas_root)
    })
    .await
    .map_err(|e| format!("Save mod loadout task failed: {e}"))?
}

#[tauri::command]
pub async fn activate_game_mod_loadout(
    app: tauri::AppHandle,
    game_id: String,
    loadout_id: Option<String>,
) -> Result<ModLoadoutApplyResult, String> {
    let (gdir, exe) = game_paths_for_id(&app, &game_id)?;
    let cas_root = gdir.join(CAS_DIR_NAME);
    tokio::task::spawn_blocking(move || {
        apply_mod_loadout_in_dir(&game_id, &gdir, exe.as_deref(), &cas_root, loadout_id.as_deref())
    })
    .await
    .map_err(|e| format!("Apply mod loadout task failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pe_inspect::tests::build_synthetic_pe_with_exports;

    #[test]
    fn proxy_dll_conflict_graph_chains_and_hardlinks_cleanly_without_touching_base_files() {
        let root = std::env::temp_dir().join(format!("duke_mod_graph_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let game_dir = root.join("Game");
        let cas_root = game_dir.join(CAS_DIR_NAME);
        let pack_sk = root.join("src_specialk");
        let pack_rs = root.join("src_reshade");
        let pack_asi = root.join("src_asi");
        fs::create_dir_all(&game_dir).unwrap();
        fs::create_dir_all(&pack_sk).unwrap();
        fs::create_dir_all(&pack_rs).unwrap();
        fs::create_dir_all(&pack_asi).unwrap();

        let game_exe = game_dir.join("game64.exe");
        fs::write(&game_exe, build_synthetic_pe_with_exports(false, "game64.exe", &[])).unwrap();
        let vanilla_marker = game_dir.join("vanilla_engine.dll");
        fs::write(&vanilla_marker, b"VANILLA_BASE_FILE").unwrap();

        // SpecialK + ReShade collide on dxgi.dll; ASI Loader uses version.dll + dinput8.dll + winmm.dll + dsound.dll.
        fs::write(
            pack_sk.join("dxgi.dll"),
            build_synthetic_pe_with_exports(false, "dxgi.dll", &[("SK_Initialize", None)]),
        )
        .unwrap();
        fs::write(
            pack_rs.join("dxgi.dll"),
            build_synthetic_pe_with_exports(false, "dxgi.dll", &[("ReShadeRegisterAddon", None)]),
        )
        .unwrap();
        fs::write(
            pack_asi.join("version.dll"),
            build_synthetic_pe_with_exports(
                false,
                "version.dll",
                &[("GetFileVersionInfoW", Some("version_orig.GetFileVersionInfoW"))],
            ),
        )
        .unwrap();

        let p1 = register_mod_pack_from_dir(&game_dir, &pack_sk, "SpecialK").unwrap();
        let p2 = register_mod_pack_from_dir(&game_dir, &pack_rs, "ReShade").unwrap();
        let p3 = register_mod_pack_from_dir(&game_dir, &pack_asi, "UltimateASI").unwrap();
        let prof = save_loadout_definition(&game_dir, "HDR+Mods", vec![p1, p2, p3]).unwrap();

        let graph =
            scan_game_mod_graph_in_dir("g-mod", &game_dir, Some(&game_exe), &cas_root).unwrap();
        assert_eq!(graph.packs.len(), 3);
        assert!(graph
            .collisions
            .iter()
            .any(|c| c.proxy_dll == "dxgi.dll" && c.severity == "auto-chainable"));

        let applied = apply_mod_loadout_in_dir(
            "g-mod",
            &game_dir,
            Some(&game_exe),
            &cas_root,
            Some(&prof.loadout_id),
        )
        .unwrap();
        assert_eq!(applied.hardlinked_files, 3);
        assert!(game_dir.join("dxgi.dll").is_file());
        assert!(game_dir.join("dxgi_reshade_chain.dll").is_file());
        assert!(game_dir.join("version.dll").is_file());
        assert!(game_dir.join("SpecialK.ini").is_file());
        assert_eq!(fs::read(&vanilla_marker).unwrap(), b"VANILLA_BASE_FILE");

        // Switch cleanly back to Vanilla and verify base game file remains untouched.
        let reverted =
            apply_mod_loadout_in_dir("g-mod", &game_dir, Some(&game_exe), &cas_root, None).unwrap();
        assert!(!game_dir.join("dxgi.dll").exists());
        assert!(!game_dir.join("dxgi_reshade_chain.dll").exists());
        assert!(!game_dir.join("SpecialK.ini").exists());
        assert_eq!(fs::read(&vanilla_marker).unwrap(), b"VANILLA_BASE_FILE");
        assert!(reverted.report.active_loadout_id.is_none());

        let _ = fs::remove_dir_all(&root);
    }
}

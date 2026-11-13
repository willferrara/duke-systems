use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VdfNode {
    Str(String),
    Map(Vec<(String, VdfNode)>),
}

impl VdfNode {
    pub fn as_map(&self) -> Option<&[(String, VdfNode)]> {
        match self {
            VdfNode::Map(m) => Some(m),
            VdfNode::Str(_) => None,
        }
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        let map = self.as_map()?;
        map.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .and_then(|(_, v)| match v {
                VdfNode::Str(s) => Some(s.as_str()),
                _ => None,
            })
    }

    pub fn get_node(&self, key: &str) -> Option<&VdfNode> {
        let map = self.as_map()?;
        map.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum VdfToken {
    Str(String),
    LBrace,
    RBrace,
}

fn tokenize_vdf(input: &str) -> Vec<VdfToken> {
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;
    let mut tokens = Vec::new();

    while i < len {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if b == b'{' {
            tokens.push(VdfToken::LBrace);
            i += 1;
            continue;
        }
        if b == b'}' {
            tokens.push(VdfToken::RBrace);
            i += 1;
            continue;
        }
        if b == b'"' {
            i += 1;
            let mut out = String::new();
            while i < len {
                let ch_b = bytes[i];
                if ch_b == b'\\' && i + 1 < len {
                    let next_b = bytes[i + 1];
                    match next_b {
                        b'\\' => out.push('\\'),
                        b'"' => out.push('"'),
                        b'n' => out.push('\n'),
                        b't' => out.push('\t'),
                        other => out.push(other as char),
                    }
                    i += 2;
                    continue;
                }
                if ch_b == b'"' {
                    i += 1;
                    break;
                }
                // Decode UTF-8 safely from original str slice
                let rest = &input[i..];
                if let Some(ch) = rest.chars().next() {
                    out.push(ch);
                    i += ch.len_utf8();
                } else {
                    break;
                }
            }
            tokens.push(VdfToken::Str(out));
            continue;
        }

        // Unquoted word token
        let start = i;
        while i < len {
            let c = bytes[i];
            if c.is_ascii_whitespace() || c == b'{' || c == b'}' || c == b'"' {
                break;
            }
            i += 1;
        }
        tokens.push(VdfToken::Str(input[start..i].to_string()));
    }

    tokens
}

fn parse_vdf_map(tokens: &[VdfToken], idx: &mut usize) -> Vec<(String, VdfNode)> {
    let mut entries = Vec::new();
    while *idx < tokens.len() {
        match &tokens[*idx] {
            VdfToken::RBrace => {
                *idx += 1;
                break;
            }
            VdfToken::Str(key) => {
                let k = key.clone();
                *idx += 1;
                if *idx >= tokens.len() {
                    break;
                }
                match &tokens[*idx] {
                    VdfToken::LBrace => {
                        *idx += 1;
                        let child = parse_vdf_map(tokens, idx);
                        entries.push((k, VdfNode::Map(child)));
                    }
                    VdfToken::Str(val) => {
                        let v = val.clone();
                        *idx += 1;
                        entries.push((k, VdfNode::Str(v)));
                    }
                    VdfToken::RBrace => {
                        *idx += 1;
                        break;
                    }
                }
            }
            VdfToken::LBrace => {
                *idx += 1;
            }
        }
    }
    entries
}

/// Parses Valve KeyValues (`.vdf` / `.acf`) into a navigable tree.
pub fn parse_vdf_kv_tree(input: &str) -> VdfNode {
    let cleaned = input.trim_start_matches('\u{feff}');
    let tokens = tokenize_vdf(cleaned);
    let mut idx = 0usize;
    VdfNode::Map(parse_vdf_map(&tokens, &mut idx))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamAcfManifest {
    pub app_id: u32,
    pub name: String,
    pub install_dir_name: String,
    pub size_on_disk: u64,
}

pub fn extract_steam_library_paths_from_vdf(vdf_text: &str) -> Vec<PathBuf> {
    let tree = parse_vdf_kv_tree(vdf_text);
    let map = tree
        .get_node("libraryfolders")
        .or(Some(&tree))
        .and_then(|n| n.as_map())
        .unwrap_or(&[]);

    let mut paths = Vec::new();
    for (k, node) in map {
        if k.chars().all(|c| c.is_ascii_digit()) {
            match node {
                VdfNode::Map(_) => {
                    if let Some(p) = node.get_str("path") {
                        let pb = PathBuf::from(p.replace('/', "\\"));
                        if !pb.as_os_str().is_empty() && !paths.contains(&pb) {
                            paths.push(pb);
                        }
                    }
                }
                VdfNode::Str(legacy_path) => {
                    let pb = PathBuf::from(legacy_path.replace('/', "\\"));
                    if !pb.as_os_str().is_empty() && !paths.contains(&pb) {
                        paths.push(pb);
                    }
                }
            }
        }
    }
    paths
}

pub fn parse_steam_appmanifest_acf(acf_text: &str) -> Option<SteamAcfManifest> {
    let tree = parse_vdf_kv_tree(acf_text);
    let state = tree.get_node("AppState").unwrap_or(&tree);
    let app_id = state.get_str("appid")?.trim().parse::<u32>().ok()?;
    let name = state.get_str("name")?.trim().to_string();
    let install_dir_name = state.get_str("installdir")?.trim().to_string();
    if name.is_empty() || install_dir_name.is_empty() {
        return None;
    }
    let size_on_disk = state
        .get_str("SizeOnDisk")
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0);

    Some(SteamAcfManifest {
        app_id,
        name,
        install_dir_name,
        size_on_disk,
    })
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
struct EpicItemManifestRaw {
    #[serde(rename = "DisplayName")]
    display_name: String,
    #[serde(rename = "InstallLocation")]
    install_location: String,
    #[serde(rename = "LaunchExecutable")]
    launch_executable: String,
    #[serde(rename = "InstallSize")]
    install_size: Option<u64>,
    #[serde(rename = "bIsIncompleteInstall")]
    is_incomplete: Option<bool>,
}

#[derive(Debug, Clone)]
struct DiscoveredImportCandidate {
    title: String,
    exe_path: PathBuf,
    install_dir: PathBuf,
    size_bytes: u64,
    steam_app_id: Option<u32>,
    source: &'static str,
    platform: &'static str,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ImportLibraryResult {
    pub added: usize,
    pub updated: usize,
    pub total: usize,
    pub by_source: HashMap<String, usize>,
    pub games: Vec<crate::GameEntry>,
}

fn is_ignored_helper_exe_name(file_name_lower: &str) -> bool {
    const BLOCKED_PREFIXES: &[&str] = &[
        "unins",
        "unitycrashhandler",
        "vcredist",
        "dxsetup",
        "crashreport",
        "ue4prereq",
        "ueprereq",
        "dotnetfx",
    ];
    BLOCKED_PREFIXES
        .iter()
        .any(|p| file_name_lower.starts_with(p))
}

fn detect_primary_exe_excluding_redist(folder: &Path, title: &str) -> Option<PathBuf> {
    if let Some(found) = crate::find_best_game_exe_in_folder(folder, title) {
        let lower = found
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !is_ignored_helper_exe_name(&lower) {
            return Some(found);
        }
    }

    let mut fallback: Option<PathBuf> = None;
    for entry in walkdir::WalkDir::new(folder)
        .max_depth(3)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            let p = entry.path();
            if p.extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("exe"))
                .unwrap_or(false)
            {
                let fname = entry.file_name().to_string_lossy().to_lowercase();
                if is_ignored_helper_exe_name(&fname) || fname.contains("setup") {
                    continue;
                }
                fallback = Some(p.to_path_buf());
                break;
            }
        }
    }
    fallback
}

fn scan_steam_libraries() -> Vec<DiscoveredImportCandidate> {
    const IGNORED_STEAM_RUNTIME_IDS: &[u32] = &[228980, 1070560, 1391110, 1628350, 1493710, 1826330];

    let Some(raw_steam_path) =
        duke_core::win32_ffi::reg_read_string(false, "Software\\Valve\\Steam", "SteamPath")
    else {
        return Vec::new();
    };

    let main_steam = PathBuf::from(raw_steam_path.replace('/', "\\"));
    let mut lib_roots = vec![main_steam.clone()];
    let vdf_path = main_steam.join("steamapps").join("libraryfolders.vdf");
    if let Ok(vdf_text) = fs::read_to_string(&vdf_path) {
        for p in extract_steam_library_paths_from_vdf(&vdf_text) {
            if !lib_roots
                .iter()
                .any(|x| x.to_string_lossy().eq_ignore_ascii_case(&p.to_string_lossy()))
            {
                lib_roots.push(p);
            }
        }
    }

    let mut out = Vec::new();
    for root in lib_roots {
        let steamapps = root.join("steamapps");
        let Ok(read_dir) = fs::read_dir(&steamapps) else {
            continue;
        };
        for entry in read_dir.flatten() {
            let fname = entry.file_name().to_string_lossy().to_lowercase();
            if !(fname.starts_with("appmanifest_") && fname.ends_with(".acf")) {
                continue;
            }
            let Ok(acf_text) = fs::read_to_string(entry.path()) else {
                continue;
            };
            let Some(acf) = parse_steam_appmanifest_acf(&acf_text) else {
                continue;
            };
            if IGNORED_STEAM_RUNTIME_IDS.contains(&acf.app_id)
                || acf.name.to_lowercase().contains("proton")
                || acf.name.to_lowercase().contains("steamworks common")
                || acf.name.to_lowercase().contains("steam linux runtime")
            {
                continue;
            }
            let install_dir = steamapps.join("common").join(&acf.install_dir_name);
            if !install_dir.is_dir() {
                continue;
            }
            let Some(exe_path) = detect_primary_exe_excluding_redist(&install_dir, &acf.name)
            else {
                continue;
            };

            let size_bytes = if acf.size_on_disk > 0 {
                acf.size_on_disk
            } else {
                crate::calculate_dir_size_sync(&install_dir)
            };

            out.push(DiscoveredImportCandidate {
                title: acf.name,
                exe_path,
                install_dir,
                size_bytes,
                steam_app_id: Some(acf.app_id),
                source: "Steam",
                platform: "Steam",
            });
        }
    }

    out
}

pub fn parse_epic_item_manifest_str(json_text: &str) -> Option<(String, PathBuf, PathBuf, u64)> {
    let parsed: EpicItemManifestRaw = serde_json::from_str(json_text.trim_start_matches('\u{feff}')).ok()?;
    if parsed.is_incomplete.unwrap_or(false) {
        return None;
    }
    let title = parsed.display_name.trim().to_string();
    let install_loc = parsed.install_location.trim().to_string();
    let launch_exe = parsed.launch_executable.trim().to_string();
    if title.is_empty() || install_loc.is_empty() {
        return None;
    }
    let install_dir = PathBuf::from(install_loc.replace('/', "\\"));
    let exe_candidate = if !launch_exe.is_empty() {
        install_dir.join(launch_exe.replace('/', "\\"))
    } else {
        PathBuf::new()
    };
    Some((
        title,
        install_dir,
        exe_candidate,
        parsed.install_size.unwrap_or(0),
    ))
}

fn scan_epic_libraries() -> Vec<DiscoveredImportCandidate> {
    let program_data = std::env::var("PROGRAMDATA")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "C:\\ProgramData".to_string());
    let manifests_dir = PathBuf::from(program_data)
        .join("Epic")
        .join("EpicGamesLauncher")
        .join("Data")
        .join("Manifests");

    let Ok(read_dir) = fs::read_dir(&manifests_dir) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for entry in read_dir.flatten() {
        let p = entry.path();
        if !p.is_file()
            || !p
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.eq_ignore_ascii_case("item"))
                .unwrap_or(false)
        {
            continue;
        }
        let Ok(raw) = fs::read_to_string(&p) else {
            continue;
        };
        let Some((title, install_dir, exe_cand, size_hint)) = parse_epic_item_manifest_str(&raw)
        else {
            continue;
        };
        if !install_dir.is_dir() {
            continue;
        }
        let exe_path = if exe_cand.is_file() {
            exe_cand
        } else if let Some(best) = detect_primary_exe_excluding_redist(&install_dir, &title) {
            best
        } else {
            continue;
        };
        let size_bytes = if size_hint > 0 {
            size_hint
        } else {
            crate::calculate_dir_size_sync(&install_dir)
        };
        out.push(DiscoveredImportCandidate {
            title,
            exe_path,
            install_dir,
            size_bytes,
            steam_app_id: None,
            source: "Epic",
            platform: "Epic Games",
        });
    }
    out
}

fn scan_gog_libraries() -> Vec<DiscoveredImportCandidate> {
    let mut out = Vec::new();
    let bases = [
        "SOFTWARE\\WOW6432Node\\GOG.com\\Games",
        "SOFTWARE\\GOG.com\\Games",
    ];
    let mut seen_keys = std::collections::HashSet::new();

    for reg_base in bases {
        let subkeys = duke_core::win32_ffi::reg_enum_subkeys(true, reg_base);
        for sub in subkeys {
            if !seen_keys.insert(sub.clone()) {
                continue;
            }
            let full_key = format!("{}\\{}", reg_base, sub);
            let Some(game_name) = duke_core::win32_ffi::reg_read_string(true, &full_key, "gameName")
            else {
                continue;
            };
            let Some(path_str) = duke_core::win32_ffi::reg_read_string(true, &full_key, "path")
            else {
                continue;
            };
            let install_dir = PathBuf::from(path_str.trim().trim_matches('"'));
            if !install_dir.is_dir() {
                continue;
            }

            let exe_opt = duke_core::win32_ffi::reg_read_string(true, &full_key, "exe")
                .map(|s| {
                    let trimmed = s.trim().trim_matches('"');
                    let p = PathBuf::from(trimmed);
                    if p.is_absolute() {
                        p
                    } else {
                        install_dir.join(p)
                    }
                })
                .filter(|p| p.is_file())
                .or_else(|| detect_primary_exe_excluding_redist(&install_dir, &game_name));

            let Some(exe_path) = exe_opt else {
                continue;
            };
            let size_bytes = crate::calculate_dir_size_sync(&install_dir);
            out.push(DiscoveredImportCandidate {
                title: game_name.trim().to_string(),
                exe_path,
                install_dir,
                size_bytes,
                steam_app_id: None,
                source: "GOG",
                platform: "GOG Galaxy",
            });
        }
    }
    out
}

fn parse_xbox_microsoftgame_config_exe(folder: &Path, title_hint: &str) -> Option<PathBuf> {
    for cfg in [folder.join("MicrosoftGame.config"), folder.join("Content").join("MicrosoftGame.config")] {
        if let Ok(xml) = fs::read_to_string(&cfg) {
            if let Some(idx) = xml.find("<Executable") {
                let tail = &xml[idx..];
                if let Some(name_pos) = tail.find("Name=\"") {
                    let after = &tail[name_pos + 6..];
                    if let Some(end) = after.find('"') {
                        let rel = after[..end].trim();
                        if !rel.is_empty() {
                            let base_dir = cfg.parent().unwrap_or(folder);
                            let cand = base_dir.join(rel.replace('/', "\\"));
                            if cand.is_file() {
                                return Some(cand);
                            }
                        }
                    }
                }
            }
        }
    }
    let content_dir = folder.join("Content");
    if content_dir.is_dir() {
        if let Some(found) = detect_primary_exe_excluding_redist(&content_dir, title_hint) {
            return Some(found);
        }
    }
    detect_primary_exe_excluding_redist(folder, title_hint)
}

fn extract_gaming_root_subdir(bytes: &[u8]) -> Option<String> {
    // `.GamingRoot` starts with `RGBX` header + UTF-16LE folder name (e.g., `XboxGames`)
    if bytes.len() <= 8 {
        return None;
    }
    let slice = &bytes[8..];
    let mut u16_units = Vec::with_capacity(slice.len() / 2);
    for pair in slice.chunks_exact(2) {
        let code = u16::from_le_bytes([pair[0], pair[1]]);
        if code == 0 {
            if !u16_units.is_empty() {
                break;
            }
            continue;
        }
        u16_units.push(code);
    }
    let decoded = String::from_utf16_lossy(&u16_units).trim().to_string();
    if decoded.is_empty() {
        None
    } else {
        Some(decoded)
    }
}

fn scan_xbox_libraries() -> Vec<DiscoveredImportCandidate> {
    let mut out = Vec::new();
    let mut xbox_roots: Vec<PathBuf> = Vec::new();

    for drive in b'C'..=b'Z' {
        let drive_root = PathBuf::from(format!("{}:\\", drive as char));
        if !drive_root.exists() {
            continue;
        }
        let default_xbox = drive_root.join("XboxGames");
        if default_xbox.is_dir() {
            xbox_roots.push(default_xbox);
        }
        let gaming_root_file = drive_root.join(".GamingRoot");
        if gaming_root_file.is_file() {
            if let Ok(bytes) = fs::read(&gaming_root_file) {
                if let Some(sub) = extract_gaming_root_subdir(&bytes) {
                    let custom_dir = drive_root.join(sub.trim_start_matches(['\\', '/']));
                    if custom_dir.is_dir()
                        && !xbox_roots.iter().any(|p| {
                            p.to_string_lossy()
                                .eq_ignore_ascii_case(&custom_dir.to_string_lossy())
                        })
                    {
                        xbox_roots.push(custom_dir);
                    }
                }
            }
        }
    }

    for root in xbox_roots {
        let Ok(read_dir) = fs::read_dir(&root) else {
            continue;
        };
        for entry in read_dir.flatten() {
            let game_dir = entry.path();
            if !game_dir.is_dir() {
                continue;
            }
            let raw_name = entry.file_name().to_string_lossy().to_string();
            let title = crate::clean_game_title(&raw_name);
            let title = if title.is_empty() { raw_name } else { title };
            let Some(exe_path) = parse_xbox_microsoftgame_config_exe(&game_dir, &title) else {
                continue;
            };
            let size_bytes = crate::calculate_dir_size_sync(&game_dir);
            out.push(DiscoveredImportCandidate {
                title,
                exe_path,
                install_dir: game_dir,
                size_bytes,
                steam_app_id: None,
                source: "Xbox",
                platform: "Xbox Game Pass",
            });
        }
    }
    out
}

pub fn normalize_win_path_key(raw: &str) -> String {
    raw.trim()
        .trim_matches('"')
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

fn merge_discovered_games_into_library(
    library: &mut Vec<crate::GameEntry>,
    candidates: Vec<DiscoveredImportCandidate>,
    now_ts: u64,
) -> (usize, usize, HashMap<String, usize>) {
    let mut added = 0usize;
    let mut updated = 0usize;
    let mut by_source: HashMap<String, usize> = HashMap::new();

    for cand in candidates {
        let exe_str = cand.exe_path.to_string_lossy().to_string();
        let dir_str = cand.install_dir.to_string_lossy().to_string();
        let exe_key = normalize_win_path_key(&exe_str);
        let dir_key = normalize_win_path_key(&dir_str);

        let existing_idx = library.iter().position(|g| {
            normalize_win_path_key(&g.exe_path) == exe_key
                || (!dir_key.is_empty() && normalize_win_path_key(&g.install_dir) == dir_key)
                || cand
                    .steam_app_id
                    .map(|aid| g.app_id == Some(aid) || g.steam_app_id == Some(aid as u64))
                    .unwrap_or(false)
        });

        *by_source.entry(cand.source.to_string()).or_insert(0) += 1;

        if let Some(idx) = existing_idx {
            let g = &mut library[idx];
            let mut changed = false;
            if g.source.trim().is_empty()
                || (g.source.eq_ignore_ascii_case("Portable") && cand.source != "Portable")
            {
                g.source = cand.source.to_string();
                changed = true;
            }
            if g.steam_app_id.is_none() {
                if let Some(aid) = cand.steam_app_id {
                    g.steam_app_id = Some(aid as u64);
                    g.app_id = Some(aid);
                    changed = true;
                }
            }
            if g.cover_url.trim().is_empty() {
                if let Some(aid) = cand.steam_app_id {
                    g.cover_url = format!(
                        "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/{}/library_600x900.jpg",
                        aid
                    );
                    changed = true;
                }
            }
            if changed {
                updated += 1;
            }
            continue;
        }

        let id = format!(
            "game_{}",
            exe_str
                .to_lowercase()
                .chars()
                .filter(|c| c.is_alphanumeric())
                .collect::<String>()
        );

        let cover_url = cand
            .steam_app_id
            .map(|aid| {
                format!(
                    "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/{}/library_600x900.jpg",
                    aid
                )
            })
            .unwrap_or_default();

        let hero_url = cand.steam_app_id.map(|aid| {
            format!(
                "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/{}/library_hero.jpg",
                aid
            )
        });

        library.push(crate::GameEntry {
            id,
            title: cand.title,
            exe_path: exe_str,
            install_dir: dir_str,
            size_bytes: cand.size_bytes,
            size_formatted: crate::format_bytes(cand.size_bytes),
            cover_url,
            hero_url,
            icon_path: None,
            genre: "Library Import".to_string(),
            platform: cand.platform.to_string(),
            source: cand.source.to_string(),
            is_favorite: false,
            is_installed: true,
            added_at: now_ts,
            last_played: None,
            steam_app_id: cand.steam_app_id.map(|v| v as u64),
            app_id: cand.steam_app_id,
            playtime_seconds: 0,
            last_played_ts: None,
            steam_price_cents: None,
            steam_price_formatted: None,
            launch_args: None,
            sandbox_enabled: false,
            nvme_prefetch_enabled: None,
        });
        added += 1;
    }

    (added, updated, by_source)
}

pub(crate) async fn import_existing_libraries_core() -> Result<ImportLibraryResult, String> {
    tokio::task::spawn_blocking(|| -> Result<ImportLibraryResult, String> {
        let mut discovered = Vec::new();
        discovered.extend(scan_steam_libraries());
        discovered.extend(scan_epic_libraries());
        discovered.extend(scan_gog_libraries());
        discovered.extend(scan_xbox_libraries());

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut library = crate::load_library_standalone()?;
        let (added, updated, by_source) =
            merge_discovered_games_into_library(&mut library, discovered, now);

        for entry in &mut library {
            crate::normalize_game_entry(entry);
        }

        if added > 0 || updated > 0 {
            crate::save_library_standalone(&library)?;
        }

        let total = library.len();
        crate::trim_process_working_set();
        Ok(ImportLibraryResult {
            added,
            updated,
            total,
            by_source,
            games: library,
        })
    })
    .await
    .map_err(|e| format!("Library import worker failed: {}", e))?
}

#[tauri::command]
pub(crate) async fn import_existing_libraries(
    app: tauri::AppHandle,
) -> Result<ImportLibraryResult, String> {
    use tauri::Emitter;
    let res = import_existing_libraries_core().await?;
    let _ = app.emit("library-updated", &res.games);
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_steam_libraryfolders_vdf_correctly() {
        let vdf = r#"
"libraryfolders"
{
    "0"
    {
        "path"      "C:\\Program Files (x86)\\Steam"
        "label"     ""
    }
    "1"
    {
        "path"      "D:\\SteamLibrary"
    }
}
"#;
        let paths = extract_steam_library_paths_from_vdf(vdf);
        assert_eq!(
            paths,
            vec![
                PathBuf::from("C:\\Program Files (x86)\\Steam"),
                PathBuf::from("D:\\SteamLibrary"),
            ]
        );
    }

    #[test]
    fn parses_steam_appmanifest_acf_correctly() {
        let acf = r#"
"AppState"
{
    "appid"         "1245620"
    "Universe"      "1"
    "name"          "ELDEN RING"
    "installdir"    "ELDEN RING"
    "SizeOnDisk"    "51234567890"
}
"#;
        let parsed = parse_steam_appmanifest_acf(acf).unwrap();
        assert_eq!(parsed.app_id, 1245620);
        assert_eq!(parsed.name, "ELDEN RING");
        assert_eq!(parsed.install_dir_name, "ELDEN RING");
        assert_eq!(parsed.size_on_disk, 51234567890);
    }

    #[test]
    fn parses_epic_item_manifest_and_deduplicates_library() {
        let json = r#"{
            "DisplayName": "Alan Wake 2",
            "InstallLocation": "D:/Games/AlanWake2",
            "LaunchExecutable": "AlanWake2.exe",
            "InstallSize": 85000000000,
            "bIsIncompleteInstall": false
        }"#;
        let parsed = parse_epic_item_manifest_str(json).unwrap();
        assert_eq!(parsed.0, "Alan Wake 2");
        assert_eq!(parsed.1, PathBuf::from("D:\\Games\\AlanWake2"));
        assert_eq!(parsed.2, PathBuf::from("D:\\Games\\AlanWake2\\AlanWake2.exe"));

        let mut library = vec![crate::GameEntry {
            id: "existing_1".to_string(),
            title: "Alan Wake 2".to_string(),
            exe_path: "D:\\Games\\AlanWake2\\AlanWake2.exe".to_string(),
            install_dir: "D:\\Games\\AlanWake2".to_string(),
            source: "Portable".to_string(),
            ..Default::default()
        }];
        let cands = vec![DiscoveredImportCandidate {
            title: "Alan Wake 2".to_string(),
            exe_path: PathBuf::from("D:\\Games\\AlanWake2\\AlanWake2.exe"),
            install_dir: PathBuf::from("D:\\Games\\AlanWake2"),
            size_bytes: 85000000000,
            steam_app_id: None,
            source: "Epic",
            platform: "Epic Games",
        }];
        let (added, updated, _) = merge_discovered_games_into_library(&mut library, cands, 100);
        assert_eq!(added, 0);
        assert_eq!(updated, 1);
        assert_eq!(library.len(), 1);
        assert_eq!(library[0].source, "Epic");
    }
}

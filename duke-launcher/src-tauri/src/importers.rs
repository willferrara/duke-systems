use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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
    pub last_played: Option<u64>,
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

    // Reject Steam .acf manifests explicitly marked as DLC, Tool, Config, Music, Driver, or Application
    for type_candidate in [
        state.get_str("type"),
        state.get_node("UserConfig").and_then(|n| n.get_str("type")),
        state
            .get_node("MountedConfig")
            .and_then(|n| n.get_str("type")),
    ]
    .into_iter()
    .flatten()
    {
        let t_lower = type_candidate.trim().to_ascii_lowercase();
        if matches!(
            t_lower.as_str(),
            "dlc" | "tool" | "config" | "music" | "driver" | "application" | "video" | "series"
        ) {
            return None;
        }
    }

    if is_non_game_or_dlc_entry(&name, Some(app_id), &install_dir_name, "placeholder_game.exe") {
        return None;
    }

    let size_on_disk = state
        .get_str("SizeOnDisk")
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0);
    let last_played = state
        .get_str("LastPlayed")
        .and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|&v| v > 0);

    Some(SteamAcfManifest {
        app_id,
        name,
        install_dir_name,
        size_on_disk,
        last_played,
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
    #[serde(rename = "AppName")]
    app_name: String,
    #[serde(rename = "CatalogItemId")]
    catalog_item_id: String,
}

#[derive(Debug, Clone)]
struct DiscoveredImportCandidate {
    title: String,
    exe_path: PathBuf,
    install_dir: PathBuf,
    size_bytes: u64,
    steam_app_id: Option<u32>,
    source: &'static str,
    acquisition_source: &'static str,
    platform: &'static str,
    imported_playtime_secs: u64,
    last_played_ts: Option<u64>,
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
        "vc_redist",
        "dxsetup",
        "dxwebsetup",
        "crashreport",
        "crashhandler",
        "crashpad_handler",
        "crs-handler",
        "ue4prereq",
        "ueprereq",
        "dotnetfx",
        "easyanticheat",
        "battleye",
        "steamerrorreporter",
        "steamservice",
        "wallpaper32",
        "wallpaper64",
        "wallpaperservice",
        "webwallpaper",
        "applicationwallpaperinject",
    ];
    BLOCKED_PREFIXES
        .iter()
        .any(|p| file_name_lower.starts_with(p))
}

/// Strictly classifies whether an entry is a DLC pack, add-on, soundtrack, tool, redistributable,
/// or non-game directory so library scanning and cleanup keep `My Games` 100% playable games.
pub fn is_non_game_or_dlc_entry<P1: AsRef<Path>, P2: AsRef<Path>>(
    title: &str,
    app_id: Option<u32>,
    install_dir: P1,
    exe_path: P2,
) -> bool {
    const BLOCKED_STEAM_APP_IDS: &[u32] = &[
        228980,  // Steamworks Common Redistributables
        1070560, // Steam Linux Runtime
        1391110, // Steam Linux Runtime - Soldier
        1628350, // Steam Linux Runtime - Sniper
        1493710, // Proton Experimental
        1826330, // Steam Linux Runtime
        2180100, // Proton Hotfix
        2348590, // Proton 8.0
        2805730, // Proton 9.0
        431960,  // Wallpaper Engine utility/helper
        250820,  // SteamVR runtime
    ];

    if let Some(aid) = app_id {
        if BLOCKED_STEAM_APP_IDS.contains(&aid) {
            return true;
        }
    }

    let title_trimmed = title.trim();
    if title_trimmed.is_empty() {
        return true;
    }
    let title_lower = title_trimmed.to_ascii_lowercase();

    const BLOCKED_TITLE_SUBSTRINGS: &[&str] = &[
        // Call of Duty / Black Ops 6 & BO7 / Modern Warfare packs
        "content pack",
        "multiplayer pack",
        "campaign pack",
        "zombies pack",
        "co-op pack",
        "coop pack",
        "special ops pack",
        "spec ops pack",
        "survival pack",
        "warzone",
        "hd texture pack",
        "high resolution texture",
        "texture pack",
        "shader pack",
        "beta pack",
        "shared content",
        // General non-games, media & Steamworks helpers
        "soundtrack",
        "artbook",
        "art book",
        "digital art",
        "digital comic",
        "dedicated server",
        "mod tools",
        "authoring tools",
        "creation kit",
        "level editor",
        "redistributable",
        "common redistributables",
        "steamworks common",
        "directx",
        "physx",
        "vcredist",
        "vc_redist",
        "steam linux runtime",
        "wallpaper engine",
        "easyanticheat",
        "battleye",
        "crash reporter",
        "crash handler",
        "benchmark tool",
        // Add-on / DLC / Season Pass bundles
        "season pass",
        "expansion pass",
        "expansion pack",
        "dlc pack",
        "starter pack",
        "supporter pack",
        "founder's pack",
        "founders pack",
        "upgrade pack",
        "vault edition upgrade",
        "blackcell",
        "weapon pack",
        "skin pack",
        "costume pack",
        "character pack",
        "music pack",
        "voice pack",
        "language pack",
        "audio pack",
        "bonus content",
        "digital deluxe content",
        "digital extras",
    ];

    if BLOCKED_TITLE_SUBSTRINGS
        .iter()
        .any(|needle| title_lower.contains(needle))
    {
        return true;
    }

    // Check word-boundary tokens so "OST", "SDK", "DLC", "Proton", "Redist" are blocked
    // without false-positiving on real games like "Ghost of Tsushima", "Lost Ark", or "Frostpunk".
    let words: Vec<&str> = title_lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    if words.iter().any(|&w| {
        matches!(
            w,
            "ost" | "sdk" | "dlc" | "redist" | "proton" | "uninstaller" | "uninstall"
        )
    }) {
        return true;
    }

    // Call of Duty / Black Ops / Modern Warfare sub-packs that use "Pack" or "Add-On"
    let is_cod_family = title_lower.contains("call of duty")
        || title_lower.contains("black ops")
        || title_lower.contains("modern warfare")
        || words.iter().any(|&w| matches!(w, "bo6" | "bo7" | "mw2" | "mw3" | "mwiii" | "cod"));
    if is_cod_family
        && (words.contains(&"pack")
            || words.contains(&"addon")
            || title_lower.contains("add-on")
            || words.contains(&"beta")
            || words.contains(&"shader")
            || words.contains(&"texture"))
    {
        return true;
    }

    let dir_path = install_dir.as_ref();
    let exe_ref = exe_path.as_ref();
    let dir_lower = dir_path
        .to_string_lossy()
        .trim()
        .replace('/', "\\")
        .to_ascii_lowercase();
    let exe_lower = exe_ref
        .to_string_lossy()
        .trim()
        .replace('/', "\\")
        .to_ascii_lowercase();

    if dir_lower.contains("\\_commonredist")
        || dir_lower.contains("\\steamworks shared")
        || dir_lower.contains("\\steam linux runtime")
        || dir_lower.contains("\\wallpaper_engine")
        || dir_lower.contains("\\wallpaper engine")
    {
        return true;
    }

    if !exe_lower.is_empty() {
        let exe_file_lower = Path::new(&exe_lower)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if is_ignored_helper_exe_name(exe_file_lower)
            || crate::cmd_library::PENALIZED_HELPER_EXES
                .iter()
                .any(|bad| exe_file_lower.starts_with(bad) || exe_file_lower.contains(bad))
            || matches!(
                exe_file_lower,
                "steam.exe"
                    | "epicgameslauncher.exe"
                    | "galaxyclient.exe"
                    | "upc.exe"
                    | "uplay.exe"
                    | "eadesktop.exe"
                    | "origin.exe"
                    | "battle.net.exe"
                    | "agent.exe"
                    | "ui32.exe"
            )
        {
            return true;
        }
    }

    // If no executable path is supplied, or the install directory exists on disk,
    // verify the directory actually contains a playable game executable.
    if exe_lower.is_empty() {
        if dir_path.is_dir() {
            if detect_primary_exe_excluding_redist(dir_path, title_trimmed).is_none() {
                return true;
            }
        } else {
            return true;
        }
    } else if dir_path.is_dir()
        && !exe_ref.is_file()
        && detect_primary_exe_excluding_redist(dir_path, title_trimmed).is_none()
    {
        return true;
    }

    false
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
                if is_ignored_helper_exe_name(&fname)
                    || fname.contains("setup")
                    || crate::cmd_library::PENALIZED_HELPER_EXES
                        .iter()
                        .any(|bad| fname.starts_with(bad) || fname.contains(bad))
                {
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
    let steam_roots = find_steam_root_dirs();
    if steam_roots.is_empty() {
        return Vec::new();
    }

    let mut lib_roots: Vec<PathBuf> = Vec::new();
    for main_steam in steam_roots {
        if !lib_roots
            .iter()
            .any(|x| x.to_string_lossy().eq_ignore_ascii_case(&main_steam.to_string_lossy()))
        {
            lib_roots.push(main_steam.clone());
        }
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
            let install_dir = steamapps.join("common").join(&acf.install_dir_name);
            if !install_dir.is_dir() {
                continue;
            }
            let Some(exe_path) = detect_primary_exe_excluding_redist(&install_dir, &acf.name)
            else {
                continue;
            };
            if is_non_game_or_dlc_entry(&acf.name, Some(acf.app_id), &install_dir, &exe_path) {
                continue;
            }

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
                acquisition_source: "steam",
                platform: "Steam",
                imported_playtime_secs: 0,
                last_played_ts: acf.last_played,
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
        if is_non_game_or_dlc_entry(&title, None, &install_dir, &exe_path) {
            continue;
        }
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
            acquisition_source: "epic",
            platform: "Epic Games",
            imported_playtime_secs: 0,
            last_played_ts: None,
        });
    }
    out
}

fn scan_gog_libraries() -> Vec<DiscoveredImportCandidate> {
    let mut out = Vec::new();
    let (gog_by_key, gog_by_title) = collect_gog_local_playtimes();
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
            let title = game_name.trim().to_string();
            if is_non_game_or_dlc_entry(&title, None, &install_dir, &exe_path) {
                continue;
            }
            let size_bytes = crate::calculate_dir_size_sync(&install_dir);
            let gog_secs = gog_by_key
                .get(&format!("gog_{}", sub.trim().to_ascii_lowercase()))
                .or_else(|| gog_by_title.get(&normalize_title_playtime_key(&title)))
                .copied()
                .unwrap_or(0);
            out.push(DiscoveredImportCandidate {
                title,
                exe_path,
                install_dir,
                size_bytes,
                steam_app_id: None,
                source: "GOG",
                acquisition_source: "gog",
                platform: "GOG Galaxy",
                imported_playtime_secs: gog_secs,
                last_played_ts: None,
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
    for pair in slice.as_chunks::<2>().0 {
        let code = u16::from_le_bytes(*pair);
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
            if is_non_game_or_dlc_entry(&title, None, &game_dir, &exe_path) {
                continue;
            }
            let size_bytes = crate::calculate_dir_size_sync(&game_dir);
            out.push(DiscoveredImportCandidate {
                title,
                exe_path,
                install_dir: game_dir,
                size_bytes,
                steam_app_id: None,
                source: "Xbox",
                acquisition_source: "xbox",
                platform: "Xbox Game Pass",
                imported_playtime_secs: 0,
                last_played_ts: None,
            });
        }
    }
    out
}

fn scan_ubisoft_libraries() -> Vec<DiscoveredImportCandidate> {
    let mut out = Vec::new();
    let mut seen_dirs = std::collections::HashSet::new();

    for reg_base in [
        "SOFTWARE\\WOW6432Node\\Ubisoft\\Launcher\\Installs",
        "SOFTWARE\\Ubisoft\\Launcher\\Installs",
    ] {
        for sub in duke_core::win32_ffi::reg_enum_subkeys(true, reg_base) {
            let full_key = format!("{}\\{}", reg_base, sub);
            let Some(raw_dir) = duke_core::win32_ffi::reg_read_string(true, &full_key, "InstallDir")
            else {
                continue;
            };
            let install_dir = PathBuf::from(raw_dir.trim().trim_matches('"').replace('/', "\\"));
            if !install_dir.is_dir() {
                continue;
            }
            let dir_key = normalize_win_path_key(&install_dir.to_string_lossy());
            if dir_key.is_empty() || !seen_dirs.insert(dir_key) {
                continue;
            }
            let uplay_uninst = format!(
                "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Uplay Install {}",
                sub.trim()
            );
            let raw_title = duke_core::win32_ffi::reg_read_string(true, &uplay_uninst, "DisplayName")
                .or_else(|| {
                    install_dir
                        .file_name()
                        .and_then(|s| s.to_str())
                        .map(crate::clean_game_title)
                })
                .unwrap_or_default();
            let title = raw_title.trim().to_string();
            if title.is_empty() {
                continue;
            }
            let Some(exe_path) = detect_primary_exe_excluding_redist(&install_dir, &title) else {
                continue;
            };
            if is_non_game_or_dlc_entry(&title, None, &install_dir, &exe_path) {
                continue;
            }
            let size_bytes = crate::calculate_dir_size_sync(&install_dir);
            out.push(DiscoveredImportCandidate {
                title,
                exe_path,
                install_dir,
                size_bytes,
                steam_app_id: None,
                source: "Ubisoft",
                acquisition_source: "ubisoft",
                platform: "Ubisoft Connect",
                imported_playtime_secs: 0,
                last_played_ts: None,
            });
        }
    }
    out
}

fn scan_ea_libraries() -> Vec<DiscoveredImportCandidate> {
    let mut out = Vec::new();
    let mut seen_dirs = std::collections::HashSet::new();

    for reg_base in ["SOFTWARE\\WOW6432Node\\EA Games", "SOFTWARE\\EA Games"] {
        for sub in duke_core::win32_ffi::reg_enum_subkeys(true, reg_base) {
            let full_key = format!("{}\\{}", reg_base, sub);
            let Some(raw_dir) = duke_core::win32_ffi::reg_read_string(true, &full_key, "Install Dir")
            else {
                continue;
            };
            let install_dir = PathBuf::from(raw_dir.trim().trim_matches('"').replace('/', "\\"));
            if !install_dir.is_dir() {
                continue;
            }
            let dir_key = normalize_win_path_key(&install_dir.to_string_lossy());
            if dir_key.is_empty() || !seen_dirs.insert(dir_key) {
                continue;
            }
            let title = duke_core::win32_ffi::reg_read_string(true, &full_key, "DisplayName")
                .unwrap_or_else(|| sub.clone())
                .trim()
                .to_string();
            if title.is_empty() {
                continue;
            }
            let Some(exe_path) = detect_primary_exe_excluding_redist(&install_dir, &title) else {
                continue;
            };
            if is_non_game_or_dlc_entry(&title, None, &install_dir, &exe_path) {
                continue;
            }
            let size_bytes = crate::calculate_dir_size_sync(&install_dir);
            out.push(DiscoveredImportCandidate {
                title,
                exe_path,
                install_dir,
                size_bytes,
                steam_app_id: None,
                source: "EA",
                acquisition_source: "ea",
                platform: "EA App",
                imported_playtime_secs: 0,
                last_played_ts: None,
            });
        }
    }

    for env_key in ["ProgramFiles", "ProgramFiles(x86)"] {
        let Ok(base) = std::env::var(env_key) else {
            continue;
        };
        for folder_name in ["EA Games", "Origin Games"] {
            let root = PathBuf::from(&base).join(folder_name);
            let Ok(rd) = fs::read_dir(&root) else {
                continue;
            };
            for entry in rd.flatten() {
                let install_dir = entry.path();
                if !install_dir.is_dir() {
                    continue;
                }
                let dir_key = normalize_win_path_key(&install_dir.to_string_lossy());
                if dir_key.is_empty() || !seen_dirs.insert(dir_key) {
                    continue;
                }
                let raw_name = entry.file_name().to_string_lossy().to_string();
                let title = crate::clean_game_title(&raw_name);
                let Some(exe_path) = detect_primary_exe_excluding_redist(&install_dir, &title) else {
                    continue;
                };
                if is_non_game_or_dlc_entry(&title, None, &install_dir, &exe_path) {
                    continue;
                }
                let size_bytes = crate::calculate_dir_size_sync(&install_dir);
                out.push(DiscoveredImportCandidate {
                    title,
                    exe_path,
                    install_dir,
                    size_bytes,
                    steam_app_id: None,
                    source: "EA",
                    acquisition_source: "ea",
                    platform: "EA App",
                    imported_playtime_secs: 0,
                    last_played_ts: None,
                });
            }
        }
    }

    out
}

fn scan_battlenet_libraries() -> Vec<DiscoveredImportCandidate> {
    let mut out = Vec::new();
    let mut seen_dirs = std::collections::HashSet::new();

    for reg_base in [
        "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
    ] {
        for sub in duke_core::win32_ffi::reg_enum_subkeys(true, reg_base) {
            let full_key = format!("{}\\{}", reg_base, sub);
            let Some(publisher) = duke_core::win32_ffi::reg_read_string(true, &full_key, "Publisher")
            else {
                continue;
            };
            let pub_lower = publisher.trim().to_ascii_lowercase();
            if !(pub_lower.contains("blizzard entertainment") || pub_lower.contains("activision")) {
                continue;
            }
            let Some(display_name) =
                duke_core::win32_ffi::reg_read_string(true, &full_key, "DisplayName")
            else {
                continue;
            };
            let title = display_name.trim().to_string();
            if title.is_empty() || title.eq_ignore_ascii_case("battle.net") {
                continue;
            }
            let Some(raw_loc) =
                duke_core::win32_ffi::reg_read_string(true, &full_key, "InstallLocation")
            else {
                continue;
            };
            let install_dir = PathBuf::from(raw_loc.trim().trim_matches('"').replace('/', "\\"));
            if !install_dir.is_dir() {
                continue;
            }
            let dir_key = normalize_win_path_key(&install_dir.to_string_lossy());
            if dir_key.is_empty() || !seen_dirs.insert(dir_key) {
                continue;
            }
            let Some(exe_path) = detect_primary_exe_excluding_redist(&install_dir, &title) else {
                continue;
            };
            if is_non_game_or_dlc_entry(&title, None, &install_dir, &exe_path) {
                continue;
            }
            let size_bytes = crate::calculate_dir_size_sync(&install_dir);
            out.push(DiscoveredImportCandidate {
                title,
                exe_path,
                install_dir,
                size_bytes,
                steam_app_id: None,
                source: "Battle.net",
                acquisition_source: "battlenet",
                platform: "Battle.net",
                imported_playtime_secs: 0,
                last_played_ts: None,
            });
        }
    }
    out
}

fn scan_retroarch_libraries() -> Vec<DiscoveredImportCandidate> {
    let mut out = Vec::new();
    let mut seen_paths = std::collections::HashSet::new();

    for env_key in ["APPDATA", "LOCALAPPDATA"] {
        let Ok(base) = std::env::var(env_key) else {
            continue;
        };
        let ra_root = PathBuf::from(base).join("RetroArch");
        let playlists_dir = ra_root.join("playlists");
        let Ok(rd) = fs::read_dir(&playlists_dir) else {
            continue;
        };
        for entry in rd.flatten().take(32) {
            let p = entry.path();
            if !p
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.eq_ignore_ascii_case("lpl"))
                .unwrap_or(false)
            {
                continue;
            }
            let Ok(raw) = fs::read_to_string(&p) else {
                continue;
            };
            let Ok(val) = serde_json::from_str::<Value>(raw.trim_start_matches('\u{feff}')) else {
                continue;
            };
            let Some(items) = val.get("items").and_then(|v| v.as_array()) else {
                continue;
            };
            for item in items.iter().take(128) {
                let raw_path = item
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .split('#')
                    .next()
                    .unwrap_or("")
                    .trim();
                if raw_path.is_empty() {
                    continue;
                }
                let rom_path = PathBuf::from(raw_path.replace('/', "\\"));
                if !rom_path.is_file() {
                    continue;
                }
                let path_key = normalize_win_path_key(&rom_path.to_string_lossy());
                if path_key.is_empty() || !seen_paths.insert(path_key) {
                    continue;
                }
                let label = item
                    .get("label")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToString::to_string)
                    .or_else(|| {
                        rom_path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .map(crate::clean_game_title)
                    })
                    .unwrap_or_default();
                if label.is_empty()
                    || is_non_game_or_dlc_entry(&label, None, "", "retroarch_rom.exe")
                {
                    continue;
                }
                let parent_dir = rom_path
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| ra_root.clone());
                let size_bytes = fs::metadata(&rom_path).map(|m| m.len()).unwrap_or(0);
                out.push(DiscoveredImportCandidate {
                    title: label,
                    exe_path: rom_path,
                    install_dir: parent_dir,
                    size_bytes,
                    steam_app_id: None,
                    source: "RetroArch",
                    acquisition_source: "retroarch",
                    platform: "RetroArch",
                    imported_playtime_secs: 0,
                    last_played_ts: None,
                });
            }
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
        if cand.source != "RetroArch"
            && is_non_game_or_dlc_entry(&cand.title, cand.steam_app_id, &dir_str, &exe_str)
        {
            continue;
        }
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
            if g.acquisition_source.trim().is_empty() {
                g.acquisition_source = cand.acquisition_source.to_string();
                changed = true;
            }
            if let Some(lp) = cand.last_played_ts.filter(|&v| v > g.last_played_ts.unwrap_or(0)) {
                g.last_played_ts = Some(lp);
                g.last_played = Some(lp);
                changed = true;
            }
            if cand.imported_playtime_secs > 0
                && cand.imported_playtime_secs != g.imported_playtime_secs
            {
                g.imported_playtime_secs = cand.imported_playtime_secs;
                g.playtime_seconds = g.imported_playtime_secs.saturating_add(g.duke_playtime_secs);
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
            acquisition_source: cand.acquisition_source.to_string(),
            is_favorite: false,
            is_installed: true,
            added_at: now_ts,
            last_played: cand.last_played_ts,
            steam_app_id: cand.steam_app_id.map(|v| v as u64),
            app_id: cand.steam_app_id,
            playtime_seconds: cand.imported_playtime_secs,
            imported_playtime_secs: cand.imported_playtime_secs,
            duke_playtime_secs: 0,
            last_played_ts: cand.last_played_ts,
            steam_price_cents: None,
            steam_price_formatted: None,
            launch_args: None,
            sandbox_enabled: false,
            nvme_prefetch_enabled: None,
            ..Default::default()
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
        discovered.extend(scan_ubisoft_libraries());
        discovered.extend(scan_ea_libraries());
        discovered.extend(scan_battlenet_libraries());
        discovered.extend(scan_retroarch_libraries());

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut library = crate::load_library_standalone()?;
        let prev_snapshots: std::collections::HashMap<String, (String, String, Option<u32>, u64)> =
            library
                .iter()
                .map(|g| {
                    (
                        g.id.clone(),
                        (g.exe_path.clone(), g.cover_url.clone(), g.app_id, g.playtime_seconds),
                    )
                })
                .collect();
        let (added, mut updated, by_source) =
            merge_discovered_games_into_library(&mut library, discovered, now);

        for entry in &mut library {
            crate::normalize_game_entry(entry);
        }
        updated += sync_local_storefront_playtimes(&mut library);

        if added > 0 || updated > 0 {
            crate::save_library_standalone(&library)?;
            for entry in &library {
                let changed = match prev_snapshots.get(&entry.id) {
                    None => true,
                    Some((exe, cover, aid, _)) => {
                        *exe != entry.exe_path || *cover != entry.cover_url || *aid != entry.app_id
                    }
                };
                if changed {
                    crate::sidebar_icons::invalidate_sidebar_icon_standalone(&entry.id);
                }
            }
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PlaytimeSyncResult {
    pub updated_games: usize,
    pub total_imported_secs: u64,
    pub total_duke_secs: u64,
    pub total_combined_secs: u64,
    pub sources_checked: Vec<String>,
    pub games: Vec<crate::GameEntry>,
}

pub(crate) fn normalize_title_playtime_key(raw: &str) -> String {
    crate::clean_game_title(raw)
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

pub fn parse_steam_localconfig_playtime_vdf(vdf_text: &str) -> HashMap<u32, (u64, Option<u64>)> {
    let tree = parse_vdf_kv_tree(vdf_text);
    let root = tree.get_node("UserLocalConfigStore").unwrap_or(&tree);
    let apps = root
        .get_node("Software")
        .and_then(|s| s.get_node("Valve"))
        .and_then(|v| v.get_node("Steam"))
        .and_then(|s| s.get_node("apps"))
        .or_else(|| root.get_node("apps"))
        .and_then(|n| n.as_map())
        .unwrap_or(&[]);

    let mut out = HashMap::new();
    for (k, node) in apps {
        let Ok(app_id) = k.trim().parse::<u32>() else {
            continue;
        };
        let mins = node
            .get_str("Playtime")
            .or_else(|| node.get_str("PlaytimeForever"))
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(0);
        let last_played = node
            .get_str("LastPlayed")
            .and_then(|s| s.trim().parse::<u64>().ok())
            .filter(|&v| v > 0);
        if mins > 0 || last_played.is_some() {
            out.insert(app_id, (mins.saturating_mul(60), last_played));
        }
    }
    out
}

pub(crate) fn find_steam_root_dirs() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(raw) = duke_core::win32_ffi::reg_read_string(false, "Software\\Valve\\Steam", "SteamPath")
        .or_else(|| {
            duke_core::win32_ffi::reg_read_string(true, "SOFTWARE\\WOW6432Node\\Valve\\Steam", "InstallPath")
        })
    {
        let p = PathBuf::from(raw.replace('/', "\\"));
        if p.is_dir() {
            roots.push(p);
        }
    }
    for env_key in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Ok(base) = std::env::var(env_key) {
            let cand = PathBuf::from(base).join("Steam");
            if cand.is_dir() && !roots.iter().any(|x| x.eq(&cand)) {
                roots.push(cand);
            }
        }
    }
    roots
}

pub(crate) fn collect_steam_local_playtimes() -> HashMap<u32, (u64, Option<u64>)> {
    let mut merged: HashMap<u32, (u64, Option<u64>)> = HashMap::new();
    for steam_root in find_steam_root_dirs() {
        let userdata = steam_root.join("userdata");
        if let Ok(entries) = fs::read_dir(&userdata) {
            for entry in entries.flatten() {
                let localconfig = entry.path().join("config").join("localconfig.vdf");
                if !localconfig.is_file() {
                    continue;
                }
                if let Ok(raw) = fs::read_to_string(&localconfig) {
                    for (aid, (secs, lp)) in parse_steam_localconfig_playtime_vdf(&raw) {
                        merged
                            .entry(aid)
                            .and_modify(|cur| {
                                cur.0 = cur.0.max(secs);
                                cur.1 = match (cur.1, lp) {
                                    (Some(a), Some(b)) => Some(a.max(b)),
                                    (a, b) => a.or(b),
                                };
                            })
                            .or_insert((secs, lp));
                    }
                }
            }
        }

        // Also incorporate LastPlayed timestamps from appmanifest_*.acf
        let mut lib_roots = vec![steam_root.clone()];
        let vdf_path = steam_root.join("steamapps").join("libraryfolders.vdf");
        if let Ok(vdf_text) = fs::read_to_string(&vdf_path) {
            lib_roots.extend(extract_steam_library_paths_from_vdf(&vdf_text));
        }
        for root in lib_roots {
            let Ok(rd) = fs::read_dir(root.join("steamapps")) else {
                continue;
            };
            for fentry in rd.flatten() {
                let name = fentry.file_name().to_string_lossy().to_ascii_lowercase();
                if name.starts_with("appmanifest_") && name.ends_with(".acf") {
                    if let Ok(raw) = fs::read_to_string(fentry.path()) {
                        if let Some(acf) = parse_steam_appmanifest_acf(&raw) {
                            if let Some(lp) = acf.last_played {
                                merged
                                    .entry(acf.app_id)
                                    .and_modify(|cur| {
                                        cur.1 = Some(cur.1.map(|x| x.max(lp)).unwrap_or(lp));
                                    })
                                    .or_insert((0, Some(lp)));
                            }
                        }
                    }
                }
            }
        }
    }
    merged
}

pub type GogPlaytimeMaps = (HashMap<String, u64>, HashMap<String, u64>);

pub fn read_gog_galaxy_playtime_from_db(db_path: &Path) -> Result<GogPlaytimeMaps, String> {
    if !db_path.is_file() {
        return Ok((HashMap::new(), HashMap::new()));
    }
    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("Failed opening GOG Galaxy DB read-only: {e}"))?;

    let mut by_key: HashMap<String, u64> = HashMap::new();
    let mut id_to_title: HashMap<String, String> = HashMap::new();

    if let Ok(mut stmt) = conn.prepare("SELECT productId, title FROM LimitedDetails") {
        if let Ok(rows) = stmt.query_map([], |row| {
            let pid: rusqlite::types::Value = row.get(0)?;
            let title: Option<String> = row.get(1)?;
            let pid_str = match pid {
                rusqlite::types::Value::Integer(n) => n.to_string(),
                rusqlite::types::Value::Text(t) => t,
                _ => String::new(),
            };
            Ok((pid_str, title.unwrap_or_default()))
        }) {
            for r in rows.flatten() {
                if !r.0.is_empty() && !r.1.trim().is_empty() {
                    id_to_title.insert(format!("gog_{}", r.0), r.1);
                }
            }
        }
    }

    let mut by_title: HashMap<String, u64> = HashMap::new();
    if let Ok(mut stmt) = conn.prepare("SELECT releaseKey, minutesInGame FROM GameTimes") {
        let rows = stmt
            .query_map([], |row| {
                let release_key: String = row.get(0)?;
                let minutes: i64 = row.get(1)?;
                Ok((release_key, minutes.max(0) as u64))
            })
            .map_err(|e| format!("Failed querying GOG GameTimes: {e}"))?;

        for row in rows.flatten() {
            let (release_key, mins) = row;
            let secs = mins.saturating_mul(60);
            if secs == 0 {
                continue;
            }
            let key_lower = release_key.trim().to_ascii_lowercase();
            by_key
                .entry(key_lower.clone())
                .and_modify(|v| *v = (*v).max(secs))
                .or_insert(secs);
            if let Some(title) = id_to_title.get(&key_lower) {
                let tkey = normalize_title_playtime_key(title);
                if !tkey.is_empty() {
                    by_title
                        .entry(tkey)
                        .and_modify(|v| *v = (*v).max(secs))
                        .or_insert(secs);
                }
            }
        }
    }

    Ok((by_key, by_title))
}

pub(crate) fn collect_gog_local_playtimes() -> GogPlaytimeMaps {
    let program_data = std::env::var("PROGRAMDATA")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "C:\\ProgramData".to_string());
    let db_path = PathBuf::from(program_data)
        .join("GOG.com")
        .join("Galaxy")
        .join("storage")
        .join("galaxy-2.0.db");
    read_gog_galaxy_playtime_from_db(&db_path).unwrap_or_default()
}

pub(crate) fn collect_epic_local_playtimes() -> HashMap<String, u64> {
    let mut artifact_to_title: HashMap<String, String> = HashMap::new();
    let program_data = std::env::var("PROGRAMDATA")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "C:\\ProgramData".to_string());
    let manifests_dir = PathBuf::from(&program_data)
        .join("Epic")
        .join("EpicGamesLauncher")
        .join("Data")
        .join("Manifests");
    if let Ok(rd) = fs::read_dir(&manifests_dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_file()
                && p.extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s.eq_ignore_ascii_case("item"))
                    .unwrap_or(false)
            {
                if let Ok(raw) = fs::read_to_string(&p) {
                    if let Ok(parsed) =
                        serde_json::from_str::<EpicItemManifestRaw>(raw.trim_start_matches('\u{feff}'))
                    {
                        if !parsed.display_name.trim().is_empty() {
                            if !parsed.app_name.trim().is_empty() {
                                artifact_to_title.insert(
                                    parsed.app_name.trim().to_ascii_lowercase(),
                                    parsed.display_name.clone(),
                                );
                            }
                            if !parsed.catalog_item_id.trim().is_empty() {
                                artifact_to_title.insert(
                                    parsed.catalog_item_id.trim().to_ascii_lowercase(),
                                    parsed.display_name.clone(),
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    let mut out: HashMap<String, u64> = HashMap::new();
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let saved_data = PathBuf::from(local_app_data)
            .join("EpicGamesLauncher")
            .join("Saved")
            .join("Data");
        if saved_data.is_dir() {
            if let Ok(rd) = fs::read_dir(&saved_data) {
                for entry in rd.flatten() {
                    let p = entry.path();
                    if p.extension()
                        .and_then(|s| s.to_str())
                        .map(|s| s.eq_ignore_ascii_case("json"))
                        .unwrap_or(false)
                    {
                        if let Ok(raw) = fs::read_to_string(&p) {
                            if let Ok(val) = serde_json::from_str::<Value>(&raw) {
                                if let Some(arr) = val.as_array() {
                                    for item in arr {
                                        let artifact = item
                                            .get("artifactId")
                                            .or_else(|| item.get("appName"))
                                            .and_then(|v| v.as_str())
                                            .map(|s| s.to_ascii_lowercase())
                                            .unwrap_or_default();
                                        let secs = item
                                            .get("totalTime")
                                            .or_else(|| item.get("playTimeSeconds"))
                                            .and_then(|v| v.as_u64())
                                            .unwrap_or(0);
                                        if secs > 0 {
                                            if let Some(title) = artifact_to_title.get(&artifact) {
                                                let key = normalize_title_playtime_key(title);
                                                out.entry(key).and_modify(|v| *v = (*v).max(secs)).or_insert(secs);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

async fn fetch_xbox_titlehub_playtime(xbox_key: &str) -> HashMap<String, u64> {
    let mut out: HashMap<String, u64> = HashMap::new();
    let Ok(val) = crate::friends::fetch_doh_json_redacted(
        "https://xbl.io/api/v2/player/titleHistory",
        &[("X-Authorization", xbox_key)],
        "Xbox TitleHub",
    )
    .await
    else {
        return out;
    };

    if let Some(arr) = val.get("titles").and_then(|v| v.as_array()) {
        for title_obj in arr {
            let name = title_obj
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let key = normalize_title_playtime_key(name);
            if key.is_empty() {
                continue;
            }
            let mins = title_obj
                .get("minutesPlayed")
                .or_else(|| title_obj.pointer("/detail/minutesPlayed"))
                .or_else(|| title_obj.pointer("/stats/minutesPlayed"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let secs = mins.saturating_mul(60);
            if secs > 0 {
                out.entry(key).and_modify(|v| *v = (*v).max(secs)).or_insert(secs);
            }
        }
    }
    out
}

pub fn apply_playtimes_to_library(
    library: &mut [crate::GameEntry],
    steam_times: &HashMap<u32, (u64, Option<u64>)>,
    gog_by_title: &HashMap<String, u64>,
    epic_by_title: &HashMap<String, u64>,
    xbox_by_title: &HashMap<String, u64>,
) -> usize {
    let mut updated = 0usize;
    for g in library.iter_mut() {
        let mut changed = false;
        let tkey = normalize_title_playtime_key(&g.title);

        let mut storefront_secs = 0u64;
        let mut storefront_lp: Option<u64> = None;

        if let Some(aid) = g
            .app_id
            .or_else(|| g.steam_app_id.and_then(|v| u32::try_from(v).ok()))
        {
            if let Some((secs, lp)) = steam_times.get(&aid) {
                storefront_secs = storefront_secs.max(*secs);
                storefront_lp = *lp;
            }
        }
        if !tkey.is_empty() {
            if let Some(secs) = gog_by_title.get(&tkey) {
                storefront_secs = storefront_secs.max(*secs);
            }
            if let Some(secs) = epic_by_title.get(&tkey) {
                storefront_secs = storefront_secs.max(*secs);
            }
            if let Some(secs) = xbox_by_title.get(&tkey) {
                storefront_secs = storefront_secs.max(*secs);
            }
        }

        if storefront_secs > g.imported_playtime_secs {
            g.imported_playtime_secs = storefront_secs;
            changed = true;
        }
        if let Some(lp) = storefront_lp.filter(|&v| v > g.last_played_ts.unwrap_or(0)) {
            g.last_played_ts = Some(lp);
            g.last_played = Some(lp);
            changed = true;
        }
        let total = g.imported_playtime_secs.saturating_add(g.duke_playtime_secs);
        if g.playtime_seconds != total {
            g.playtime_seconds = total;
            changed = true;
        }
        if changed {
            updated += 1;
        }
    }
    updated
}

pub fn sync_local_storefront_playtimes(library: &mut [crate::GameEntry]) -> usize {
    let steam_times = collect_steam_local_playtimes();
    let (_gog_keys, gog_by_title) = collect_gog_local_playtimes();
    let epic_by_title = collect_epic_local_playtimes();
    apply_playtimes_to_library(
        library,
        &steam_times,
        &gog_by_title,
        &epic_by_title,
        &HashMap::new(),
    )
}

#[tauri::command]
pub async fn sync_storefront_playtime(app: tauri::AppHandle) -> Result<PlaytimeSyncResult, String> {
    use tauri::Emitter;
    let settings = crate::load_settings_standalone();
    let xbox_key = settings
        .xbox_api_key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);

    let (steam_times, (_gog_keys, gog_by_title), epic_by_title) =
        tokio::task::spawn_blocking(|| {
            (
                collect_steam_local_playtimes(),
                collect_gog_local_playtimes(),
                collect_epic_local_playtimes(),
            )
        })
        .await
        .map_err(|e| format!("Local playtime scan failed: {e}"))?;

    let mut sources_checked = vec![
        "Steam localconfig.vdf".to_string(),
        "GOG galaxy-2.0.db".to_string(),
        "Epic Manifests".to_string(),
    ];
    let xbox_by_title = if let Some(ref key) = xbox_key {
        sources_checked.push("Xbox TitleHub".to_string());
        fetch_xbox_titlehub_playtime(key).await
    } else {
        HashMap::new()
    };

    let mut library = crate::load_library_from_disk(&app)?;
    for entry in &mut library {
        crate::normalize_game_entry(entry);
    }
    let updated_games = apply_playtimes_to_library(
        &mut library,
        &steam_times,
        &gog_by_title,
        &epic_by_title,
        &xbox_by_title,
    );
    if updated_games > 0 {
        crate::save_library_to_disk(&app, &library)?;
    }

    let total_imported_secs: u64 = library.iter().map(|g| g.imported_playtime_secs).sum();
    let total_duke_secs: u64 = library.iter().map(|g| g.duke_playtime_secs).sum();
    let total_combined_secs = total_imported_secs.saturating_add(total_duke_secs);

    let _ = app.emit("library-updated", &library);
    Ok(PlaytimeSyncResult {
        updated_games,
        total_imported_secs,
        total_duke_secs,
        total_combined_secs,
        sources_checked,
        games: library,
    })
}

#[tauri::command]
pub(crate) async fn import_existing_libraries(
    app: tauri::AppHandle,
) -> Result<ImportLibraryResult, String> {
    use tauri::Emitter;
    let res = import_existing_libraries_core().await?;
    let app_for_settings = app.clone();
    let _ = tokio::task::spawn_blocking(move || {
        let mut settings = crate::load_settings_from_disk(&app_for_settings);
        if !settings.auto_initial_scan_done {
            settings.auto_initial_scan_done = true;
            let _ = crate::save_settings_to_disk(&app_for_settings, &settings);
        }
    })
    .await;
    let _ = app.emit("library-updated", &res.games);
    crate::spawn_background_missing_covers_resolver(app.clone());
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_dlc_packs_soundtracks_and_redistributables_while_allowing_real_games() {
        // Real games must pass (returns false)
        assert!(!is_non_game_or_dlc_entry(
            "ELDEN RING",
            Some(1245620),
            "D:\\Games\\ELDEN RING",
            "D:\\Games\\ELDEN RING\\Game\\eldenring.exe"
        ));
        assert!(!is_non_game_or_dlc_entry(
            "Ghost of Tsushima DIRECTOR'S CUT",
            Some(2215430),
            "D:\\Games\\Ghost of Tsushima",
            "D:\\Games\\Ghost of Tsushima\\GhostOfTsushima.exe"
        ));
        assert!(!is_non_game_or_dlc_entry(
            "Call of Duty: Black Ops 6",
            Some(2933620),
            "D:\\Games\\Call of Duty",
            "D:\\Games\\Call of Duty\\cod.exe"
        ));
        assert!(!is_non_game_or_dlc_entry(
            "Frostpunk 2",
            Some(1601580),
            "D:\\Games\\Frostpunk 2",
            "D:\\Games\\Frostpunk 2\\Frostpunk2.exe"
        ));

        // Call of Duty / BO6 / BO7 / Modern Warfare packs must be blocked (returns true)
        for blocked_cod in [
            "Call of Duty: Black Ops 7 - Content Pack 1",
            "Call of Duty: Black Ops 6 - Multiplayer Pack",
            "Call of Duty: Modern Warfare III - Campaign Pack",
            "Black Ops 6 Zombies Pack",
            "Call of Duty: Warzone",
            "Call of Duty HD Texture Pack",
            "Black Ops 7 Shader Pack",
            "Call of Duty: Black Ops 7 Beta Pack",
        ] {
            assert!(
                is_non_game_or_dlc_entry(
                    blocked_cod,
                    None,
                    "D:\\Games\\Call of Duty",
                    "D:\\Games\\Call of Duty\\cod.exe"
                ),
                "Expected '{}' to be blocked as DLC/pack",
                blocked_cod
            );
        }

        // General non-games, soundtracks, SDKs, redistributables & helpers must be blocked
        for (title, aid, exe) in [
            ("Cyberpunk 2077 Original Soundtrack", None, "D:\\Games\\CP\\ost.exe"),
            ("Hollow Knight - Official OST", None, "D:\\Games\\HK\\hk.exe"),
            ("ELDEN RING Digital Artbook", None, "D:\\Games\\ER\\artbook.exe"),
            ("Palworld Dedicated Server", None, "D:\\Games\\Pal\\PalServer.exe"),
            ("Source SDK Base 2013", None, "D:\\Games\\SDK\\hl2.exe"),
            ("Call of Duty: Black Ops III - Mod Tools", None, "D:\\Games\\BO3\\mod.exe"),
            ("Left 4 Dead 2 Authoring Tools", None, "D:\\Games\\L4D2\\tools.exe"),
            ("Steamworks Common Redistributables", Some(228980), "D:\\Steam\\vcredist_x64.exe"),
            ("DirectX Redistributable", None, "D:\\Steam\\dxsetup.exe"),
            ("NVIDIA PhysX System Software", None, "D:\\Steam\\physx.exe"),
            ("Proton Experimental", Some(1493710), "D:\\Steam\\proton.exe"),
            ("Steam Linux Runtime 3.0 (sniper)", Some(1628350), "D:\\Steam\\runtime.exe"),
            ("Wallpaper Engine", Some(431960), "D:\\Steam\\wallpaper64.exe"),
        ] {
            assert!(
                is_non_game_or_dlc_entry(title, aid, "D:\\Games\\Test", exe),
                "Expected '{}' to be blocked as non-game",
                title
            );
        }

        // Steam .acf manifests with type = DLC / Tool / Config / Music must be rejected
        let dlc_acf = r#"
"AppState"
{
    "appid"         "3199999"
    "name"          "Some Expansion"
    "installdir"    "Some Expansion"
    "type"          "DLC"
}
"#;
        assert!(parse_steam_appmanifest_acf(dlc_acf).is_none());

        // Real directory on disk with zero playable .exe files must be rejected
        let empty_dir = std::env::temp_dir().join(format!("duke_empty_dlc_{}", std::process::id()));
        let _ = fs::create_dir_all(&empty_dir);
        assert!(is_non_game_or_dlc_entry(
            "Valid Looking Title",
            None,
            &empty_dir,
            ""
        ));
        let _ = fs::remove_dir_all(&empty_dir);
    }

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
            acquisition_source: "epic",
            platform: "Epic Games",
            imported_playtime_secs: 0,
            last_played_ts: None,
        }];
        let (added, updated, _) = merge_discovered_games_into_library(&mut library, cands, 100);
        assert_eq!(added, 0);
        assert_eq!(updated, 1);
        assert_eq!(library.len(), 1);
        assert_eq!(library[0].source, "Epic");
    }

    #[test]
    fn syncs_steam_localconfig_and_gog_sqlite_without_overwriting_duke_hours() {
        let localconfig = r#"
"UserLocalConfigStore"
{
    "Software"
    {
        "Valve"
        {
            "Steam"
            {
                "apps"
                {
                    "1245620"
                    {
                        "Playtime"      "120"
                        "LastPlayed"    "1710000000"
                    }
                }
            }
        }
    }
}
"#;
        let parsed_steam = parse_steam_localconfig_playtime_vdf(localconfig);
        assert_eq!(parsed_steam.get(&1245620), Some(&(7200, Some(1710000000))));

        let db_path = std::env::temp_dir().join(format!("duke_gog_test_{}.db", std::process::id()));
        let _ = fs::remove_file(&db_path);
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE LimitedDetails (productId TEXT, title TEXT);
                 CREATE TABLE GameTimes (releaseKey TEXT, minutesInGame INTEGER);
                 INSERT INTO LimitedDetails VALUES ('1454587428', 'Cyberpunk 2077');
                 INSERT INTO GameTimes VALUES ('gog_1454587428', 300);",
            )
            .unwrap();
        }
        let (by_key, by_title) = read_gog_galaxy_playtime_from_db(&db_path).unwrap();
        let _ = fs::remove_file(&db_path);
        assert_eq!(by_key.get("gog_1454587428"), Some(&18000));
        assert_eq!(by_title.get("cyberpunk2077"), Some(&18000));

        let mut library = vec![
            crate::GameEntry {
                id: "steam_elden".to_string(),
                title: "Elden Ring".to_string(),
                app_id: Some(1245620),
                imported_playtime_secs: 3600,
                duke_playtime_secs: 1800,
                playtime_seconds: 5400,
                ..Default::default()
            },
            crate::GameEntry {
                id: "gog_cp".to_string(),
                title: "Cyberpunk 2077".to_string(),
                duke_playtime_secs: 600,
                playtime_seconds: 600,
                ..Default::default()
            },
        ];

        let changed = apply_playtimes_to_library(
            &mut library,
            &parsed_steam,
            &by_title,
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(changed, 2);
        // Preserves Duke session time and updates storefront hours cleanly without double-counting.
        assert_eq!(library[0].imported_playtime_secs, 7200);
        assert_eq!(library[0].duke_playtime_secs, 1800);
        assert_eq!(library[0].playtime_seconds, 9000);
        assert_eq!(library[1].imported_playtime_secs, 18000);
        assert_eq!(library[1].duke_playtime_secs, 600);
        assert_eq!(library[1].playtime_seconds, 18600);
    }
}

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

static SITES_FILE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn sites_lock() -> &'static Mutex<()> {
    SITES_FILE_LOCK.get_or_init(|| Mutex::new(()))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchSiteEntry {
    pub id: String,
    pub name: String,
    pub search_url: String,
    pub home_url: String,
    pub tag: String,
    pub note: String,
    pub is_default: bool,
}

impl Default for SearchSiteEntry {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            search_url: String::new(),
            home_url: String::new(),
            tag: "Custom".to_string(),
            note: String::new(),
            is_default: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkImportSitesResult {
    pub added_count: usize,
    pub skipped_count: usize,
    pub sites: Vec<SearchSiteEntry>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawBulkSiteInput {
    #[serde(default)]
    name: String,
    #[serde(default, alias = "search_url", alias = "url")]
    search_url: String,
    #[serde(default, alias = "home_url")]
    home_url: String,
    #[serde(default, alias = "category")]
    tag: String,
    #[serde(default, alias = "description")]
    note: String,
}

pub fn default_search_sites() -> Vec<SearchSiteEntry> {
    vec![
        SearchSiteEntry {
            id: "default_gog".to_string(),
            name: "GOG.com".to_string(),
            search_url: "https://www.gog.com/en/games?query={query}".to_string(),
            home_url: "https://www.gog.com/".to_string(),
            tag: "DRM-Free".to_string(),
            note: "Official DRM-free offline installers".to_string(),
            is_default: true,
        },
        SearchSiteEntry {
            id: "default_itch".to_string(),
            name: "Itch.io".to_string(),
            search_url: "https://itch.io/search?q={query}".to_string(),
            home_url: "https://itch.io/".to_string(),
            tag: "Indie & Free".to_string(),
            note: "Direct indie downloads & portable builds".to_string(),
            is_default: true,
        },
        SearchSiteEntry {
            id: "default_archive_org".to_string(),
            name: "Internet Archive (Classic PC)".to_string(),
            search_url: "https://archive.org/details/softwarelibrary_msdos_games?query={query}"
                .to_string(),
            home_url: "https://archive.org/details/softwarelibrary_msdos_games".to_string(),
            tag: "Preservation".to_string(),
            note: "Preserved classic PC & DOS archives".to_string(),
            is_default: true,
        },
        SearchSiteEntry {
            id: "default_myabandonware".to_string(),
            name: "My Abandonware".to_string(),
            search_url: "https://www.myabandonware.com/search/q/{query}".to_string(),
            home_url: "https://www.myabandonware.com/".to_string(),
            tag: "Preservation".to_string(),
            note: "Retro & out-of-print PC game database".to_string(),
            is_default: true,
        },
        SearchSiteEntry {
            id: "default_pcgamingwiki".to_string(),
            name: "PCGamingWiki".to_string(),
            search_url: "https://www.pcgamingwiki.com/w/index.php?search={query}".to_string(),
            home_url: "https://www.pcgamingwiki.com/".to_string(),
            tag: "Fixes & Guides".to_string(),
            note: "Save paths, widescreen patches & PC fixes".to_string(),
            is_default: true,
        },
        SearchSiteEntry {
            id: "default_steamdb".to_string(),
            name: "SteamDB".to_string(),
            search_url: "https://steamdb.info/search/?a=all&q={query}".to_string(),
            home_url: "https://steamdb.info/".to_string(),
            tag: "Database".to_string(),
            note: "AppIDs, depots, patch history & player stats".to_string(),
            is_default: true,
        },
    ]
}

fn generate_site_id(name: &str, search_url: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(name.trim().to_lowercase().as_bytes());
    hasher.update(b"|");
    hasher.update(search_url.trim().as_bytes());
    let digest = hex::encode(hasher.finalize());
    format!("site_{}", &digest[..12])
}

fn validate_search_template_url(raw_url: &str) -> Result<String, String> {
    let trimmed = raw_url.trim();
    if trimmed.is_empty() {
        return Err("Search URL is required.".to_string());
    }
    let test_url = trimmed.replace("{query}", "duke");
    crate::validate_external_http_url(&test_url)?;
    Ok(trimmed.to_string())
}

fn derive_home_url(search_url: &str, explicit_home_url: Option<&str>) -> Result<String, String> {
    if let Some(home) = explicit_home_url {
        let trimmed = home.trim();
        if !trimmed.is_empty() {
            let parsed = crate::validate_external_http_url(trimmed)?;
            return Ok(parsed.to_string());
        }
    }

    let test_url = search_url.replace("{query}", "duke");
    let parsed = crate::validate_external_http_url(&test_url)?;
    let origin = parsed.origin().ascii_serialization();
    if origin.is_empty() || origin == "null" {
        return Ok(parsed.to_string());
    }
    Ok(format!("{origin}/"))
}

fn build_validated_site_entry(
    name: &str,
    search_url: &str,
    home_url: Option<&str>,
    tag: Option<&str>,
    note: Option<&str>,
    is_default: bool,
) -> Result<SearchSiteEntry, String> {
    let clean_name = name.trim();
    if clean_name.is_empty() {
        return Err("Site Name is required.".to_string());
    }

    let clean_search = validate_search_template_url(search_url)?;
    let clean_home = derive_home_url(&clean_search, home_url)?;
    let clean_tag = tag
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Custom")
        .to_string();
    let clean_note = note
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Custom game directory search")
        .to_string();

    Ok(SearchSiteEntry {
        id: generate_site_id(clean_name, &clean_search),
        name: clean_name.to_string(),
        search_url: clean_search,
        home_url: clean_home,
        tag: clean_tag,
        note: clean_note,
        is_default,
    })
}

fn read_sites_from_path(path: &Path) -> Result<Vec<SearchSiteEntry>, String> {
    if !path.exists() {
        let defaults = default_search_sites();
        write_sites_to_path(path, &defaults)?;
        return Ok(defaults);
    }

    let raw = fs::read_to_string(path)
        .map_err(|e| format!("Failed reading search_sites.json: {e}"))?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        let defaults = default_search_sites();
        write_sites_to_path(path, &defaults)?;
        return Ok(defaults);
    }

    serde_json::from_str::<Vec<SearchSiteEntry>>(trimmed)
        .map_err(|e| format!("Invalid search_sites.json format: {e}"))
}

fn write_sites_to_path(path: &Path, sites: &[SearchSiteEntry]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(sites)
        .map_err(|e| format!("Failed serializing search_sites.json: {e}"))?;
    crate::atomic_write_string(path, &json)
}

pub fn load_search_sites_from_path(path: &Path) -> Result<Vec<SearchSiteEntry>, String> {
    let _guard = sites_lock().lock().map_err(|_| "Sites lock poisoned".to_string())?;
    read_sites_from_path(path)
}

pub fn add_search_site_to_path(
    path: &Path,
    name: String,
    search_url: String,
    tag: String,
    note: String,
    home_url: Option<String>,
) -> Result<Vec<SearchSiteEntry>, String> {
    let entry = build_validated_site_entry(
        &name,
        &search_url,
        home_url.as_deref(),
        Some(&tag),
        Some(&note),
        false,
    )?;

    let _guard = sites_lock().lock().map_err(|_| "Sites lock poisoned".to_string())?;
    let mut sites = read_sites_from_path(path)?;

    if let Some(existing) = sites
        .iter_mut()
        .find(|s| s.search_url.eq_ignore_ascii_case(&entry.search_url))
    {
        existing.name = entry.name;
        existing.tag = entry.tag;
        existing.note = entry.note;
        existing.home_url = entry.home_url;
    } else {
        sites.push(entry);
    }

    write_sites_to_path(path, &sites)?;
    Ok(sites)
}

pub fn remove_search_site_from_path(
    path: &Path,
    site_id: &str,
) -> Result<Vec<SearchSiteEntry>, String> {
    let target = site_id.trim();
    if target.is_empty() {
        return Err("Site ID is required.".to_string());
    }

    let _guard = sites_lock().lock().map_err(|_| "Sites lock poisoned".to_string())?;
    let mut sites = read_sites_from_path(path)?;
    let before = sites.len();
    sites.retain(|s| s.id != target);
    if sites.len() == before {
        return Err("Directory site not found.".to_string());
    }
    write_sites_to_path(path, &sites)?;
    Ok(sites)
}

pub fn parse_bulk_sites_input(raw_input: &str) -> Result<(Vec<SearchSiteEntry>, Vec<String>), String> {
    let trimmed = raw_input.trim();
    if trimmed.is_empty() {
        return Err(
            "Paste a JSON array or lines formatted as Name | SearchURL | Tag | Note.".to_string(),
        );
    }

    let mut parsed_entries = Vec::new();
    let mut warnings = Vec::new();

    if trimmed.starts_with('[') || trimmed.starts_with('{') {
        let items: Vec<RawBulkSiteInput> = if trimmed.starts_with('[') {
            serde_json::from_str(trimmed)
                .map_err(|e| format!("Invalid JSON array format: {e}"))?
        } else {
            vec![serde_json::from_str(trimmed)
                .map_err(|e| format!("Invalid JSON object format: {e}"))?]
        };

        for (idx, item) in items.into_iter().enumerate() {
            match build_validated_site_entry(
                &item.name,
                &item.search_url,
                if item.home_url.trim().is_empty() {
                    None
                } else {
                    Some(&item.home_url)
                },
                Some(&item.tag),
                Some(&item.note),
                false,
            ) {
                Ok(entry) => parsed_entries.push(entry),
                Err(err) => warnings.push(format!("Item #{}: {}", idx + 1, err)),
            }
        }
    } else {
        for (idx, line) in trimmed.lines().enumerate() {
            let line_trimmed = line.trim();
            if line_trimmed.is_empty()
                || line_trimmed.starts_with('#')
                || line_trimmed.starts_with("//")
            {
                continue;
            }

            let parts: Vec<&str> = line_trimmed.split('|').map(str::trim).collect();
            if parts.len() < 2 {
                warnings.push(format!(
                    "Line {}: expected `Name | SearchURL | Tag | Note`",
                    idx + 1
                ));
                continue;
            }

            let name = parts[0];
            let search_url = parts[1];
            let tag = parts.get(2).copied().unwrap_or("Custom");
            let note = parts.get(3).copied().unwrap_or("");

            match build_validated_site_entry(name, search_url, None, Some(tag), Some(note), false) {
                Ok(entry) => parsed_entries.push(entry),
                Err(err) => warnings.push(format!("Line {}: {}", idx + 1, err)),
            }
        }
    }

    if parsed_entries.is_empty() && warnings.is_empty() {
        return Err("No valid site entries found in input.".to_string());
    }

    Ok((parsed_entries, warnings))
}

pub fn bulk_import_search_sites_to_path(
    path: &Path,
    raw_input: &str,
) -> Result<BulkImportSitesResult, String> {
    let (incoming, mut warnings) = parse_bulk_sites_input(raw_input)?;
    let _guard = sites_lock().lock().map_err(|_| "Sites lock poisoned".to_string())?;
    let mut sites = read_sites_from_path(path)?;

    let mut seen_urls: HashSet<String> = sites
        .iter()
        .map(|s| s.search_url.trim().to_lowercase())
        .collect();
    let mut added_count = 0usize;
    let mut skipped_count = 0usize;

    for entry in incoming {
        let key = entry.search_url.trim().to_lowercase();
        if seen_urls.contains(&key) {
            skipped_count += 1;
            continue;
        }
        seen_urls.insert(key);
        sites.push(entry);
        added_count += 1;
    }

    if added_count > 0 {
        write_sites_to_path(path, &sites)?;
    }

    if skipped_count > 0 {
        warnings.push(format!("{skipped_count} duplicate search URL(s) skipped."));
    }

    Ok(BulkImportSitesResult {
        added_count,
        skipped_count,
        sites,
        warnings,
    })
}

pub fn reset_default_search_sites_at_path(path: &Path) -> Result<Vec<SearchSiteEntry>, String> {
    let _guard = sites_lock().lock().map_err(|_| "Sites lock poisoned".to_string())?;
    let defaults = default_search_sites();
    write_sites_to_path(path, &defaults)?;
    Ok(defaults)
}

#[tauri::command]
pub fn get_search_sites() -> Result<Vec<SearchSiteEntry>, String> {
    let path = crate::get_app_data_file_standalone("search_sites.json")?;
    load_search_sites_from_path(&path)
}

#[tauri::command]
pub fn add_search_site(
    name: String,
    search_url: String,
    tag: Option<String>,
    note: Option<String>,
    home_url: Option<String>,
) -> Result<Vec<SearchSiteEntry>, String> {
    let path = crate::get_app_data_file_standalone("search_sites.json")?;
    add_search_site_to_path(
        &path,
        name,
        search_url,
        tag.unwrap_or_else(|| "Custom".to_string()),
        note.unwrap_or_default(),
        home_url,
    )
}

#[tauri::command]
pub fn remove_search_site(site_id: String) -> Result<Vec<SearchSiteEntry>, String> {
    let path = crate::get_app_data_file_standalone("search_sites.json")?;
    remove_search_site_from_path(&path, &site_id)
}

#[tauri::command]
pub fn bulk_import_search_sites(raw_input: String) -> Result<BulkImportSitesResult, String> {
    let path = crate::get_app_data_file_standalone("search_sites.json")?;
    bulk_import_search_sites_to_path(&path, &raw_input)
}

#[tauri::command]
pub fn reset_default_search_sites() -> Result<Vec<SearchSiteEntry>, String> {
    let path = crate::get_app_data_file_standalone("search_sites.json")?;
    reset_default_search_sites_at_path(&path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_sites_path(label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("duke_launcher_sites_{label}_{nanos}.json"))
    }

    #[test]
    fn default_sites_match_exact_six_directories() {
        let defaults = default_search_sites();
        assert_eq!(defaults.len(), 6);
        assert_eq!(defaults[0].name, "GOG.com");
        assert_eq!(defaults[0].tag, "DRM-Free");
        assert_eq!(
            defaults[0].search_url,
            "https://www.gog.com/en/games?query={query}"
        );
        assert_eq!(defaults[1].name, "Itch.io");
        assert_eq!(defaults[2].name, "Internet Archive (Classic PC)");
        assert_eq!(defaults[3].name, "My Abandonware");
        assert_eq!(defaults[4].name, "PCGamingWiki");
        assert_eq!(defaults[5].name, "SteamDB");
    }

    #[test]
    fn parse_bulk_pipe_lines_and_json_array() {
        let pipe_input = r#"
# comment line
MobyGames | https://www.mobygames.com/search/?q={query} | Database | Credits & release years
ProtonDB | https://www.protondb.com/search?q={query} | Fixes & Guides | Linux & Deck compatibility
invalid line without separator
"#;
        let (pipe_sites, pipe_warnings) = parse_bulk_sites_input(pipe_input).unwrap();
        assert_eq!(pipe_sites.len(), 2);
        assert_eq!(pipe_sites[0].name, "MobyGames");
        assert_eq!(pipe_sites[0].home_url, "https://www.mobygames.com/");
        assert_eq!(pipe_warnings.len(), 1);

        let json_input = r#"[
          {
            "name": "WineHQ AppDB",
            "searchUrl": "https://appdb.winehq.org/objectManager.php?sClass=application&sTitle=Browse+Applications&iappVersion-ratingOp=0&sName={query}",
            "tag": "Fixes & Guides",
            "note": "Compatibility workarounds"
          }
        ]"#;
        let (json_sites, json_warnings) = parse_bulk_sites_input(json_input).unwrap();
        assert_eq!(json_sites.len(), 1);
        assert!(json_warnings.is_empty());
        assert_eq!(json_sites[0].home_url, "https://appdb.winehq.org/");
    }

    #[test]
    fn persistence_add_bulk_remove_and_reset_roundtrip() {
        let path = temp_sites_path("roundtrip");
        let _ = fs::remove_file(&path);

        let initial = load_search_sites_from_path(&path).unwrap();
        assert_eq!(initial.len(), 6);
        assert!(path.exists());

        let after_add = add_search_site_to_path(
            &path,
            "PCGW Alt".to_string(),
            "https://www.pcgamingwiki.com/api/opensearch.php?search={query}".to_string(),
            "Custom".to_string(),
            "Quick index search".to_string(),
            None,
        )
        .unwrap();
        assert_eq!(after_add.len(), 7);
        let added_id = after_add.last().unwrap().id.clone();

        let bulk_res = bulk_import_search_sites_to_path(
            &path,
            "GOG.com | https://www.gog.com/en/games?query={query} | DRM-Free | Dup test\nIGDB | https://www.igdb.com/search?q={query} | Database | Metadata search",
        )
        .unwrap();
        assert_eq!(bulk_res.added_count, 1);
        assert_eq!(bulk_res.skipped_count, 1);
        assert_eq!(bulk_res.sites.len(), 8);

        let after_remove = remove_search_site_from_path(&path, &added_id).unwrap();
        assert_eq!(after_remove.len(), 7);

        let after_reset = reset_default_search_sites_at_path(&path).unwrap();
        assert_eq!(after_reset.len(), 6);

        let _ = fs::remove_file(&path);
    }
}

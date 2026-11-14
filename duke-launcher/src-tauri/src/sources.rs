use reqwest::header::ACCEPT;
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static SOURCES_IO_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn get_sources_io_lock() -> &'static Mutex<()> {
    SOURCES_IO_LOCK.get_or_init(|| Mutex::new(()))
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn deserialize_string_or_size<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let val = serde_json::Value::deserialize(deserializer)?;
    match val {
        serde_json::Value::Null => Ok(String::new()),
        serde_json::Value::String(s) => Ok(s.trim().to_string()),
        serde_json::Value::Number(n) => {
            if let Some(u) = n.as_u64() {
                Ok(crate::format_bytes(u))
            } else {
                Ok(n.to_string())
            }
        }
        other => Ok(other.to_string()),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct HydraDownloadEntry {
    pub title: String,
    #[serde(default)]
    pub uris: Vec<String>,
    #[serde(alias = "upload_date", default)]
    pub upload_date: String,
    #[serde(alias = "file_size", deserialize_with = "deserialize_string_or_size", default)]
    pub file_size: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct HydraSourceManifest {
    pub name: String,
    #[serde(default)]
    pub downloads: Vec<HydraDownloadEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ImportedSourceRecord {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub location: String,
    pub enabled: bool,
    pub last_synced_ts: u64,
    pub item_count: usize,
    pub downloads: Vec<HydraDownloadEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ImportedSourceSummary {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub location: String,
    pub enabled: bool,
    pub last_synced_ts: u64,
    pub item_count: usize,
}

impl From<&ImportedSourceRecord> for ImportedSourceSummary {
    fn from(r: &ImportedSourceRecord) -> Self {
        Self {
            id: r.id.clone(),
            name: r.name.clone(),
            kind: r.kind.clone(),
            location: r.location.clone(),
            enabled: r.enabled,
            last_synced_ts: r.last_synced_ts,
            item_count: r.item_count,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct HydraSearchItem {
    pub source_id: String,
    pub source_name: String,
    pub title: String,
    pub uris: Vec<String>,
    pub upload_date: String,
    pub file_size: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct HydraSearchResponse {
    pub items: Vec<HydraSearchItem>,
    pub total_matches: usize,
    pub enabled_source_count: usize,
    pub total_source_count: usize,
}

fn stable_source_id(kind: &str, location: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(kind.as_bytes());
    hasher.update(b":");
    hasher.update(location.to_lowercase().as_bytes());
    let digest = hex::encode(hasher.finalize());
    format!("src_{}", &digest[..16])
}

pub fn parse_hydra_manifest_str(
    raw: &str,
    fallback_name: &str,
) -> Result<HydraSourceManifest, String> {
    let cleaned = raw.trim().trim_start_matches('\u{feff}');
    if cleaned.is_empty() {
        return Err("Source JSON manifest is empty.".to_string());
    }

    let mut manifest: HydraSourceManifest = serde_json::from_str(cleaned)
        .map_err(|e| format!("Invalid Hydra JSON source schema: {}", e))?;

    if manifest.name.trim().is_empty() {
        manifest.name = if fallback_name.trim().is_empty() {
            "Community Source".to_string()
        } else {
            fallback_name.trim().to_string()
        };
    } else {
        manifest.name = manifest.name.trim().to_string();
    }

    manifest.downloads = manifest
        .downloads
        .into_iter()
        .filter_map(|mut item| {
            item.title = item.title.trim().to_string();
            item.upload_date = item.upload_date.trim().to_string();
            item.file_size = item.file_size.trim().to_string();
            item.uris = item
                .uris
                .into_iter()
                .map(|u| u.trim().to_string())
                .filter(|u| u.starts_with("https://") || u.starts_with("http://"))
                .collect();
            if item.title.is_empty() || item.uris.is_empty() {
                None
            } else {
                Some(item)
            }
        })
        .collect();

    if manifest.downloads.is_empty() {
        return Err(
            "Source manifest contained 0 valid HTTP/HTTPS download entries.".to_string(),
        );
    }

    Ok(manifest)
}

pub(crate) fn load_sources_from_disk_standalone() -> Vec<ImportedSourceRecord> {
    let _guard = get_sources_io_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let Ok(path) = crate::get_app_data_file_standalone("sources.json") else {
        return Vec::new();
    };
    if !path.exists() {
        return Vec::new();
    }
    let Ok(raw) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    if raw.trim().is_empty() {
        return Vec::new();
    }
    serde_json::from_str::<Vec<ImportedSourceRecord>>(&raw).unwrap_or_default()
}

pub(crate) fn save_sources_to_disk_standalone(
    records: &[ImportedSourceRecord],
) -> Result<(), String> {
    let _guard = get_sources_io_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let path = crate::get_app_data_file_standalone("sources.json")?;
    let serialized = serde_json::to_string(records)
        .map_err(|e| format!("Failed to serialize sources.json: {}", e))?;
    crate::atomic_write_string(&path, &serialized)
}

async fn fetch_hydra_manifest_from_url(url: &str) -> Result<HydraSourceManifest, String> {
    let parsed = crate::validate_external_http_url(url)?;
    let fallback_name = parsed
        .host_str()
        .unwrap_or("Remote Source")
        .to_string();

    let client = crate::build_doh_client_for_url(parsed.as_str(), false, true).await;
    let resp = client
        .get(parsed.as_str())
        .header(ACCEPT, "application/json, text/plain, */*")
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| format!("Failed to fetch source URL '{}': {}", parsed, e))?;

    if !resp.status().is_success() {
        return Err(format!(
            "Source URL returned HTTP {} for '{}'",
            resp.status().as_u16(),
            parsed
        ));
    }

    let body = resp
        .text()
        .await
        .map_err(|e| format!("Failed reading response body from '{}': {}", parsed, e))?;
    parse_hydra_manifest_str(&body, &fallback_name)
}

fn read_hydra_manifest_from_file(file_path: &str) -> Result<(String, HydraSourceManifest), String> {
    let trimmed = file_path.trim().trim_matches('"');
    let path = Path::new(trimmed);
    if !path.exists() || !path.is_file() {
        return Err(format!("Source JSON file not found: '{}'", trimmed));
    }
    let canonical = fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string();
    let fallback_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Local Source");
    let raw = fs::read_to_string(path)
        .map_err(|e| format!("Failed reading '{}': {}", path.display(), e))?;
    let manifest = parse_hydra_manifest_str(&raw, fallback_name)?;
    Ok((canonical, manifest))
}

fn upsert_source_record(
    kind: &str,
    location: &str,
    manifest: HydraSourceManifest,
) -> Result<ImportedSourceSummary, String> {
    let id = stable_source_id(kind, location);
    let now = now_unix_secs();
    let mut records = load_sources_from_disk_standalone();

    let mut summary = None;
    for rec in &mut records {
        if rec.id == id || rec.location.eq_ignore_ascii_case(location) {
            rec.name = manifest.name.clone();
            rec.last_synced_ts = now;
            rec.item_count = manifest.downloads.len();
            rec.downloads = manifest.downloads.clone();
            summary = Some(ImportedSourceSummary::from(&*rec));
            break;
        }
    }

    if summary.is_none() {
        let new_rec = ImportedSourceRecord {
            id,
            name: manifest.name,
            kind: kind.to_string(),
            location: location.to_string(),
            enabled: true,
            last_synced_ts: now,
            item_count: manifest.downloads.len(),
            downloads: manifest.downloads,
        };
        summary = Some(ImportedSourceSummary::from(&new_rec));
        records.push(new_rec);
    }

    save_sources_to_disk_standalone(&records)?;
    summary.ok_or_else(|| "Failed to construct imported source summary.".to_string())
}

pub(crate) fn search_imported_sources_from_records(
    records: &[ImportedSourceRecord],
    query: &str,
    source_id: Option<&str>,
    offset: usize,
    limit: usize,
) -> HydraSearchResponse {
    let q_lower = query.trim().to_lowercase();
    let tokens: Vec<&str> = q_lower.split_whitespace().collect();
    let total_source_count = records.len();
    let enabled_source_count = records.iter().filter(|r| r.enabled).count();

    let mut matched = Vec::new();
    for src in records {
        if !src.enabled {
            continue;
        }
        if let Some(filter_id) = source_id {
            if !filter_id.trim().is_empty() && src.id != filter_id.trim() {
                continue;
            }
        }

        for dl in &src.downloads {
            if !tokens.is_empty() {
                let hay = dl.title.to_lowercase();
                if !tokens.iter().all(|t| hay.contains(t)) {
                    continue;
                }
            }
            matched.push(HydraSearchItem {
                source_id: src.id.clone(),
                source_name: src.name.clone(),
                title: dl.title.clone(),
                uris: dl.uris.clone(),
                upload_date: dl.upload_date.clone(),
                file_size: dl.file_size.clone(),
            });
        }
    }

    let total_matches = matched.len();
    let items = matched
        .into_iter()
        .skip(offset)
        .take(limit.clamp(1, 200))
        .collect();

    HydraSearchResponse {
        items,
        total_matches,
        enabled_source_count,
        total_source_count,
    }
}

pub(crate) fn search_imported_sources_core(
    query: &str,
    source_id: Option<&str>,
    offset: usize,
    limit: usize,
) -> HydraSearchResponse {
    let records = load_sources_from_disk_standalone();
    search_imported_sources_from_records(&records, query, source_id, offset, limit)
}

#[tauri::command]
pub(crate) async fn get_imported_sources() -> Result<Vec<ImportedSourceSummary>, String> {
    tokio::task::spawn_blocking(|| {
        load_sources_from_disk_standalone()
            .iter()
            .map(ImportedSourceSummary::from)
            .collect()
    })
    .await
    .map_err(|e| format!("Failed to read sources: {}", e))
}

#[tauri::command]
pub(crate) async fn search_imported_sources(
    query: Option<String>,
    source_id: Option<String>,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<HydraSearchResponse, String> {
    let q = query.unwrap_or_default();
    let sid = source_id;
    let off = offset.unwrap_or(0);
    let lim = limit.unwrap_or(80);
    tokio::task::spawn_blocking(move || {
        search_imported_sources_core(&q, sid.as_deref(), off, lim)
    })
    .await
    .map_err(|e| format!("Failed searching imported sources: {}", e))
}

#[tauri::command]
pub(crate) async fn import_source_url(url: String) -> Result<ImportedSourceSummary, String> {
    let normalized = crate::validate_external_http_url(&url)?.to_string();
    let manifest = fetch_hydra_manifest_from_url(&normalized).await?;
    let summary = tokio::task::spawn_blocking(move || {
        upsert_source_record("url", &normalized, manifest)
    })
    .await
    .map_err(|e| format!("Failed saving source manifest: {}", e))??;
    crate::trim_process_working_set();
    Ok(summary)
}

#[tauri::command]
pub(crate) async fn import_source_file(file_path: String) -> Result<ImportedSourceSummary, String> {
    let summary = tokio::task::spawn_blocking(move || -> Result<ImportedSourceSummary, String> {
        let (canonical, manifest) = read_hydra_manifest_from_file(&file_path)?;
        upsert_source_record("file", &canonical, manifest)
    })
    .await
    .map_err(|e| format!("Failed importing local source JSON: {}", e))??;
    crate::trim_process_working_set();
    Ok(summary)
}

#[tauri::command]
pub(crate) async fn toggle_imported_source(
    source_id: String,
    enabled: bool,
) -> Result<Vec<ImportedSourceSummary>, String> {
    tokio::task::spawn_blocking(move || -> Result<Vec<ImportedSourceSummary>, String> {
        let mut records = load_sources_from_disk_standalone();
        let Some(target) = records.iter_mut().find(|r| r.id == source_id) else {
            return Err(format!("Source '{}' not found.", source_id));
        };
        target.enabled = enabled;
        save_sources_to_disk_standalone(&records)?;
        Ok(records.iter().map(ImportedSourceSummary::from).collect())
    })
    .await
    .map_err(|e| format!("Failed toggling source: {}", e))?
}

#[tauri::command]
pub(crate) async fn refresh_imported_source(
    source_id: String,
) -> Result<ImportedSourceSummary, String> {
    let records = tokio::task::spawn_blocking(load_sources_from_disk_standalone)
        .await
        .map_err(|e| format!("Failed loading sources: {}", e))?;

    let target = records
        .into_iter()
        .find(|r| r.id == source_id)
        .ok_or_else(|| format!("Source '{}' not found.", source_id))?;

    let (kind, location, manifest) = if target.kind == "url" {
        let m = fetch_hydra_manifest_from_url(&target.location).await?;
        ("url".to_string(), target.location, m)
    } else {
        let loc = target.location.clone();
        let (canon, m) = tokio::task::spawn_blocking(move || read_hydra_manifest_from_file(&loc))
            .await
            .map_err(|e| format!("Failed reading local source: {}", e))??;
        ("file".to_string(), canon, m)
    };

    let summary = tokio::task::spawn_blocking(move || upsert_source_record(&kind, &location, manifest))
        .await
        .map_err(|e| format!("Failed saving refreshed source: {}", e))??;
    crate::trim_process_working_set();
    Ok(summary)
}

#[tauri::command]
pub(crate) async fn refresh_all_imported_sources() -> Result<Vec<ImportedSourceSummary>, String> {
    let records = tokio::task::spawn_blocking(load_sources_from_disk_standalone)
        .await
        .map_err(|e| format!("Failed loading sources: {}", e))?;

    for rec in records {
        let _ = refresh_imported_source(rec.id).await;
    }

    get_imported_sources().await
}

#[tauri::command]
pub(crate) async fn remove_imported_source(
    source_id: String,
) -> Result<Vec<ImportedSourceSummary>, String> {
    tokio::task::spawn_blocking(move || -> Result<Vec<ImportedSourceSummary>, String> {
        let mut records = load_sources_from_disk_standalone();
        records.retain(|r| r.id != source_id);
        save_sources_to_disk_standalone(&records)?;
        Ok(records.iter().map(ImportedSourceSummary::from).collect())
    })
    .await
    .map_err(|e| format!("Failed removing source: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_hydra_manifest_and_filters_empty_uris() {
        let json = r#"{
            "name": "Community Test Feed",
            "downloads": [
                {
                    "title": "Hades II (v1.0)",
                    "uris": ["https://pixeldrain.com/u/test123", "   ", "ftp://ignored.invalid/file"],
                    "uploadDate": "2026-01-15T12:00:00Z",
                    "fileSize": "8.4 GB"
                },
                {
                    "title": "Numeric Size Release",
                    "uris": ["https://gofile.io/d/abc123"],
                    "uploadDate": "2026-02-01",
                    "fileSize": 1073741824
                },
                {
                    "title": "Empty Links Should Drop",
                    "uris": []
                }
            ]
        }"#;

        let parsed = parse_hydra_manifest_str(json, "Fallback").unwrap();
        assert_eq!(parsed.name, "Community Test Feed");
        assert_eq!(parsed.downloads.len(), 2);
        assert_eq!(parsed.downloads[0].uris, vec!["https://pixeldrain.com/u/test123"]);
        assert_eq!(parsed.downloads[1].file_size, "1.00 GB");
    }

    #[test]
    fn rejects_invalid_or_empty_hydra_manifest() {
        assert!(parse_hydra_manifest_str("", "Fallback").is_err());
        assert!(parse_hydra_manifest_str(r#"{"name":"Empty","downloads":[]}"#, "Fallback").is_err());
        assert!(parse_hydra_manifest_str("{not_json", "Fallback").is_err());
    }

    #[test]
    fn source_search_filters_by_enabled_and_query() {
        let records = vec![
            ImportedSourceRecord {
                id: "s1".to_string(),
                name: "Enabled Feed".to_string(),
                kind: "url".to_string(),
                location: "https://example.com/a.json".to_string(),
                enabled: true,
                last_synced_ts: 100,
                item_count: 2,
                downloads: vec![
                    HydraDownloadEntry {
                        title: "Hollow Knight: Silksong".to_string(),
                        uris: vec!["https://pixeldrain.com/u/hk".to_string()],
                        upload_date: "2026-01-01".to_string(),
                        file_size: "5.2 GB".to_string(),
                    },
                    HydraDownloadEntry {
                        title: "Celeste".to_string(),
                        uris: vec!["https://pixeldrain.com/u/cel".to_string()],
                        upload_date: "2026-01-02".to_string(),
                        file_size: "1.1 GB".to_string(),
                    },
                ],
            },
            ImportedSourceRecord {
                id: "s2".to_string(),
                name: "Disabled Feed".to_string(),
                kind: "file".to_string(),
                location: "C:\\feeds\\b.json".to_string(),
                enabled: false,
                last_synced_ts: 100,
                item_count: 1,
                downloads: vec![HydraDownloadEntry {
                    title: "Hollow Knight Classic".to_string(),
                    uris: vec!["https://gofile.io/d/hk0".to_string()],
                    upload_date: "2025-01-01".to_string(),
                    file_size: "4.0 GB".to_string(),
                }],
            },
        ];

        let res = search_imported_sources_from_records(&records, "hollow", None, 0, 50);
        assert_eq!(res.enabled_source_count, 1);
        assert_eq!(res.total_source_count, 2);
        assert_eq!(res.total_matches, 1);
        assert_eq!(res.items[0].title, "Hollow Knight: Silksong");
    }
}

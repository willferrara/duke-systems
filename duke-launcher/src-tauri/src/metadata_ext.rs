use reqwest::header::{ACCEPT, CONTENT_TYPE, ORIGIN, REFERER, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::AppHandle;

const HLTB_CACHE_FILE: &str = "hltb_cache.json";
const HLTB_TTL_SECS: u64 = 7 * 24 * 60 * 60;
const CHROME_UA: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct HltbEntry {
    pub hltb_id: Option<u64>,
    pub matched_title: String,
    pub main_story_hours: Option<f64>,
    pub main_plus_extras_hours: Option<f64>,
    pub completionist_hours: Option<f64>,
    pub hltb_url: Option<String>,
    pub fetched_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct HltbCacheStore {
    pub(crate) entries: HashMap<String, HltbEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PcgwSummary {
    pub page_title: Option<String>,
    pub wiki_url: Option<String>,
    pub engine: Option<String>,
    pub controller_support: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct GameExtendedTelemetry {
    pub game_id: String,
    pub game_title: String,
    pub unified_playtime_secs: u64,
    pub unified_playtime_hours: f64,
    pub hltb: Option<HltbEntry>,
    pub hltb_status: String,
    pub main_progress_pct: Option<f64>,
    pub extras_progress_pct: Option<f64>,
    pub completionist_progress_pct: Option<f64>,
    pub pcgw: Option<PcgwSummary>,
    pub pcgw_status: String,
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn normalize_title_key(title: &str) -> String {
    crate::clean_game_title(title)
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || c.is_ascii_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn round_1_decimal(val: f64) -> f64 {
    (val * 10.0).round() / 10.0
}

pub(crate) fn load_hltb_cache() -> HltbCacheStore {
    let Ok(path) = crate::get_app_data_file_standalone(HLTB_CACHE_FILE) else {
        return HltbCacheStore::default();
    };
    let Ok(raw) = fs::read_to_string(path) else {
        return HltbCacheStore::default();
    };
    serde_json::from_str::<HltbCacheStore>(&raw).unwrap_or_default()
}

fn save_hltb_cache(store: &HltbCacheStore) {
    let Ok(path) = crate::get_app_data_file_standalone(HLTB_CACHE_FILE) else {
        return;
    };
    if let Ok(json) = serde_json::to_string_pretty(store) {
        let _ = crate::atomic_write_string(&path, &json);
    }
}

fn pick_best_hltb_item(items: &[Value], cleaned_query: &str) -> Option<HltbEntry> {
    let target_key = normalize_title_key(cleaned_query);
    let best = items.iter().max_by_key(|item| {
        let name = item
            .get("game_name")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let cand_key = normalize_title_key(name);
        let title_score: i64 = if cand_key == target_key {
            10_000
        } else if cand_key.starts_with(&target_key) || target_key.starts_with(&cand_key) {
            5_000
        } else {
            let matches = target_key
                .split_whitespace()
                .filter(|w| cand_key.contains(w))
                .count() as i64;
            matches * 400
        };
        let has_data = item.get("comp_main").and_then(|v| v.as_u64()).unwrap_or(0) > 0;
        title_score + if has_data { 100 } else { 0 }
    })?;

    let matched_title = best
        .get("game_name")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim()
        .to_string();
    if matched_title.is_empty() {
        return None;
    }

    let secs_to_hours = |key: &str| -> Option<f64> {
        let secs = best.get(key).and_then(|v| v.as_u64()).unwrap_or(0);
        if secs >= 60 {
            Some(round_1_decimal(secs as f64 / 3600.0))
        } else {
            None
        }
    };

    let hltb_id = best.get("game_id").and_then(|v| v.as_u64());
    let hltb_url = hltb_id.map(|id| format!("https://howlongtobeat.com/game/{id}"));
    Some(HltbEntry {
        hltb_id,
        matched_title,
        main_story_hours: secs_to_hours("comp_main"),
        main_plus_extras_hours: secs_to_hours("comp_plus"),
        completionist_hours: secs_to_hours("comp_100"),
        hltb_url,
        fetched_at: now_unix_secs(),
    })
}

pub fn parse_hltb_search_json(val: &Value, cleaned_query: &str) -> Option<HltbEntry> {
    let data = val.get("data").and_then(|v| v.as_array())?;
    pick_best_hltb_item(data, cleaned_query)
}

async fn query_hltb_remote(title: &str) -> Result<Option<HltbEntry>, String> {
    let cleaned = crate::clean_game_title(title);
    let terms: Vec<&str> = cleaned.split_whitespace().take(10).collect();
    if terms.is_empty() {
        return Ok(None);
    }

    let init_url = format!("https://howlongtobeat.com/api/search/init?t={}", now_unix_secs());
    let client = duke_core::net_doh::build_doh_client_with_timeouts(
        &init_url,
        Duration::from_secs(6),
        Duration::from_secs(8),
        true,
    )
    .await;

    let init_token = match client
        .get(&init_url)
        .header(USER_AGENT, CHROME_UA)
        .header(ACCEPT, "application/json")
        .header(REFERER, "https://howlongtobeat.com/")
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => resp
            .json::<Value>()
            .await
            .ok()
            .and_then(|v| v.get("token").and_then(|t| t.as_str()).map(ToString::to_string)),
        _ => None,
    };

    let payload = json!({
        "searchType": "games",
        "searchTerms": terms,
        "searchPage": 1,
        "size": 10,
        "searchOptions": {
            "games": {
                "userId": 0,
                "platform": "",
                "sortCategory": "popular",
                "rangeCategory": "main",
                "rangeTime": { "min": null, "max": null },
                "gameplay": { "perspective": "", "flow": "", "genre": "", "difficulty": "" },
                "rangeYear": { "min": "", "max": "" },
                "modifier": ""
            },
            "users": { "sortCategory": "postcount" },
            "lists": { "sortCategory": "follows" },
            "filter": "",
            "sort": 0,
            "randomizer": 0
        },
        "useCache": true
    });

    for endpoint in [
        "https://howlongtobeat.com/api/search",
        "https://howlongtobeat.com/api/locate",
    ] {
        let mut req = client
            .post(endpoint)
            .header(USER_AGENT, CHROME_UA)
            .header(ACCEPT, "application/json")
            .header(CONTENT_TYPE, "application/json")
            .header(ORIGIN, "https://howlongtobeat.com")
            .header(REFERER, "https://howlongtobeat.com/");
        if let Some(ref tok) = init_token {
            req = req.header("x-auth-token", tok);
        }
        let Ok(resp) = req.json(&payload).send().await else {
            continue;
        };
        if !resp.status().is_success() {
            continue;
        }
        if let Ok(json_val) = resp.json::<Value>().await {
            if let Some(entry) = parse_hltb_search_json(&json_val, &cleaned) {
                return Ok(Some(entry));
            }
        }
    }

    Ok(None)
}

fn clean_pcgw_field(raw: Option<&str>) -> Option<String> {
    let text = raw?.trim();
    if text.is_empty() {
        return None;
    }
    let decoded = text
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#039;", "'")
        .replace("[[", "")
        .replace("]]", "");
    let compact: Vec<&str> = decoded
        .split([',', ';'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if compact.is_empty() {
        None
    } else {
        Some(compact.join(", "))
    }
}

pub fn parse_pcgw_cargo_response(val: &Value) -> Option<PcgwSummary> {
    let first_row = val
        .get("cargoquery")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|item| item.get("title").or(Some(item)))?;

    let page_title = first_row
        .get("Page")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    let engine = clean_pcgw_field(first_row.get("Engines").and_then(|v| v.as_str()));
    let full_ctrl = clean_pcgw_field(
        first_row
            .get("Full controller support")
            .or_else(|| first_row.get("Full_controller_support"))
            .and_then(|v| v.as_str()),
    );
    let partial_ctrl = clean_pcgw_field(
        first_row
            .get("Controller support")
            .or_else(|| first_row.get("Controller_support"))
            .and_then(|v| v.as_str()),
    );
    let controller_support = match (full_ctrl.as_deref(), partial_ctrl.as_deref()) {
        (Some(f), _) if f.eq_ignore_ascii_case("true") || f.eq_ignore_ascii_case("yes") => {
            Some("Full Controller Support".to_string())
        }
        (_, Some(p)) if p.eq_ignore_ascii_case("true") || p.eq_ignore_ascii_case("yes") => {
            Some("Partial / Controller Support".to_string())
        }
        (Some(f), _) if !f.eq_ignore_ascii_case("false") && !f.eq_ignore_ascii_case("no") => {
            Some(f.to_string())
        }
        (_, Some(p)) => Some(p.to_string()),
        _ => None,
    };

    let wiki_url = page_title.as_ref().map(|p| {
        format!(
            "https://www.pcgamingwiki.com/wiki/{}",
            urlencoding::encode(&p.replace(' ', "_"))
        )
    });

    if page_title.is_none() && engine.is_none() && controller_support.is_none() {
        return None;
    }

    Some(PcgwSummary {
        page_title,
        wiki_url,
        engine,
        controller_support,
    })
}

async fn query_pcgw_cargo(title: &str, steam_app_id: Option<u32>) -> Result<Option<PcgwSummary>, String> {
    let fields = "Infobox_game._pageName=Page,Infobox_game.Engines,Infobox_game.Controller_support,Infobox_game.Full_controller_support";
    let mut candidate_wheres: Vec<String> = Vec::new();
    if let Some(aid) = steam_app_id.filter(|&v| v > 0) {
        candidate_wheres.push(format!("Infobox_game.Steam_AppID HOLDS '{aid}'"));
    }
    let cleaned = crate::clean_game_title(title);
    if !cleaned.is_empty() {
        let escaped = cleaned.replace('\'', "\\'");
        candidate_wheres.push(format!("Infobox_game._pageName='{escaped}'"));
    }

    let base_url = "https://www.pcgamingwiki.com/w/api.php";
    let client = duke_core::net_doh::build_doh_client_with_timeouts(
        base_url,
        Duration::from_secs(6),
        Duration::from_secs(8),
        true,
    )
    .await;

    for where_clause in candidate_wheres {
        let url = format!(
            "{base_url}?action=cargoquery&format=json&tables=Infobox_game&fields={}&where={}&limit=2",
            urlencoding::encode(fields),
            urlencoding::encode(&where_clause)
        );
        if let Ok(resp) = client
            .get(&url)
            .header(USER_AGENT, CHROME_UA)
            .header(ACCEPT, "application/json")
            .send()
            .await
        {
            if resp.status().is_success() {
                if let Ok(json_val) = resp.json::<Value>().await {
                    if let Some(parsed) = parse_pcgw_cargo_response(&json_val) {
                        return Ok(Some(parsed));
                    }
                }
            }
        }
    }

    // Fallback: OpenSearch -> exact page CargoQuery
    if !cleaned.is_empty() {
        let os_url = format!(
            "{base_url}?action=opensearch&format=json&search={}&limit=3",
            urlencoding::encode(&cleaned)
        );
        if let Ok(resp) = client.get(&os_url).header(USER_AGENT, CHROME_UA).send().await {
            if let Ok(Value::Array(arr)) = resp.json::<Value>().await {
                if let Some(page) = arr
                    .get(1)
                    .and_then(|v| v.as_array())
                    .and_then(|list| list.first())
                    .and_then(|v| v.as_str())
                {
                    let escaped = page.replace('\'', "\\'");
                    let url = format!(
                        "{base_url}?action=cargoquery&format=json&tables=Infobox_game&fields={}&where={}&limit=1",
                        urlencoding::encode(fields),
                        urlencoding::encode(&format!("Infobox_game._pageName='{escaped}'"))
                    );
                    if let Ok(c_resp) = client.get(&url).header(USER_AGENT, CHROME_UA).send().await {
                        if let Ok(json_val) = c_resp.json::<Value>().await {
                            if let Some(parsed) = parse_pcgw_cargo_response(&json_val) {
                                return Ok(Some(parsed));
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(None)
}

fn calc_progress_pct(played_hours: f64, target_hours: Option<f64>) -> Option<f64> {
    let target = target_hours.filter(|&h| h > 0.05)?;
    let pct = ((played_hours / target) * 100.0).clamp(0.0, 999.0);
    Some(round_1_decimal(pct))
}

#[tauri::command]
pub async fn get_game_extended_telemetry(
    app: AppHandle,
    game_id: String,
    force_refresh: Option<bool>,
) -> Result<GameExtendedTelemetry, String> {
    let library = crate::load_library_from_disk(&app)?;
    let game = library
        .iter()
        .find(|g| g.id == game_id)
        .cloned()
        .ok_or_else(|| "Game not found in library".to_string())?;

    let unified_playtime_secs = game.total_playtime_secs();
    let unified_playtime_hours = round_1_decimal(unified_playtime_secs as f64 / 3600.0);
    let cache_key = normalize_title_key(&game.title);
    let force = force_refresh.unwrap_or(false);
    let now = now_unix_secs();

    let cached_hltb = if !force && !cache_key.is_empty() {
        load_hltb_cache()
            .entries
            .get(&cache_key)
            .filter(|e| now.saturating_sub(e.fetched_at) < HLTB_TTL_SECS)
            .cloned()
    } else {
        None
    };

    let hltb_fut = async {
        if let Some(hit) = cached_hltb {
            (Some(hit), "Cached".to_string(), false)
        } else {
            match query_hltb_remote(&game.title).await {
                Ok(Some(entry)) => (Some(entry), "Live".to_string(), true),
                Ok(None) => (None, "No HLTB match".to_string(), false),
                Err(e) => (None, e, false),
            }
        }
    };

    let pcgw_fut = query_pcgw_cargo(
        &game.title,
        game.steam_app_id.and_then(|v| u32::try_from(v).ok()),
    );

    let ((hltb, hltb_status, should_cache_hltb), pcgw_res) = tokio::join!(hltb_fut, pcgw_fut);

    if should_cache_hltb && !cache_key.is_empty() {
        if let Some(ref entry) = hltb {
            let mut store = load_hltb_cache();
            store.entries.insert(cache_key, entry.clone());
            save_hltb_cache(&store);
        }
    }

    let (pcgw, pcgw_status) = match pcgw_res {
        Ok(Some(info)) => (Some(info), "Live".to_string()),
        Ok(None) => (None, "No PCGamingWiki page found".to_string()),
        Err(e) => (None, e),
    };

    let main_progress_pct =
        calc_progress_pct(unified_playtime_hours, hltb.as_ref().and_then(|h| h.main_story_hours));
    let extras_progress_pct = calc_progress_pct(
        unified_playtime_hours,
        hltb.as_ref().and_then(|h| h.main_plus_extras_hours),
    );
    let completionist_progress_pct = calc_progress_pct(
        unified_playtime_hours,
        hltb.as_ref().and_then(|h| h.completionist_hours),
    );

    Ok(GameExtendedTelemetry {
        game_id: game.id,
        game_title: game.title,
        unified_playtime_secs,
        unified_playtime_hours,
        hltb,
        hltb_status,
        main_progress_pct,
        extras_progress_pct,
        completionist_progress_pct,
        pcgw,
        pcgw_status,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hltb_json_and_computes_completion_percentage() {
        let mock = json!({
            "data": [
                {
                    "game_id": 2670,
                    "game_name": "Elden Ring",
                    "comp_main": 216000,
                    "comp_plus": 360000,
                    "comp_100": 486000
                }
            ]
        });
        let parsed = parse_hltb_search_json(&mock, "Elden Ring").unwrap();
        assert_eq!(parsed.hltb_id, Some(2670));
        assert_eq!(parsed.main_story_hours, Some(60.0));
        assert_eq!(parsed.main_plus_extras_hours, Some(100.0));
        assert_eq!(parsed.completionist_hours, Some(135.0));

        let pct = calc_progress_pct(30.0, parsed.main_story_hours);
        assert_eq!(pct, Some(50.0));
    }

    #[test]
    fn parses_pcgamingwiki_cargo_response() {
        let mock = json!({
            "cargoquery": [
                {
                    "title": {
                        "Page": "Cyberpunk 2077",
                        "Engines": "REDengine 4",
                        "Full controller support": "true",
                        "Controller support": "true"
                    }
                }
            ]
        });
        let pcgw = parse_pcgw_cargo_response(&mock).unwrap();
        assert_eq!(pcgw.page_title.as_deref(), Some("Cyberpunk 2077"));
        assert_eq!(pcgw.engine.as_deref(), Some("REDengine 4"));
        assert_eq!(pcgw.controller_support.as_deref(), Some("Full Controller Support"));
        assert_eq!(
            pcgw.wiki_url.as_deref(),
            Some("https://www.pcgamingwiki.com/wiki/Cyberpunk_2077")
        );
    }
}

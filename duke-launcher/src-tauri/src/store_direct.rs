use crate::steam_cm::{load_persisted_store_sessions, save_persisted_store_sessions};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;
use tauri::AppHandle;

// Public desktop OAuth client constants documented by open storefront integrations (Heroic/Legendary/GOGDL).
const GOG_CLIENT_ID: &str = "46899977096215655";
const GOG_CLIENT_SECRET: &str = "9d85c43b1482497dbbce61f6e4aa173a433796eeae2ca8c5f6129f2dc4de46d9";
const GOG_REDIRECT_URI: &str = "https://embed.gog.com/on_login_success?origin=client";
const GOG_TOKEN_URL: &str = "https://auth.gog.com/token";
const GOG_OWNED_PRODUCTS_URL: &str = "https://embed.gog.com/account/getFilteredProducts";
const GOG_PRODUCT_DOWNLOADS_URL: &str = "https://api.gog.com/products";
const GOG_BUILDS_URL: &str = "https://content-system.gog.com/products";

const EPIC_LAUNCHER_CLIENT_ID: &str = "34a02cf8f4414e29b15921876da36f9a";
const EPIC_LAUNCHER_SECRET: &str = "daafbccc737745039dffe53d94fc76cf";
const EPIC_OAUTH_URL: &str =
    "https://account-public-service-prod.ol.epicgames.com/account/api/oauth/token";
const EPIC_ASSETS_URL: &str =
    "https://launcher-public-service-prod06.ol.epicgames.com/launcher/api/public/assets/v2/platform/Windows";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreDirectStatus {
    pub gog_connected: bool,
    pub gog_user_id: String,
    pub gog_auth_url: String,
    pub epic_connected: bool,
    pub epic_account_id: String,
    pub epic_display_name: String,
    pub epic_auth_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectStoreTitle {
    pub store: String,
    pub product_id: String,
    pub app_name: String,
    pub namespace: String,
    pub title: String,
    pub version: String,
    pub total_size_bytes: u64,
    pub download_url: String,
    pub installer_filename: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectInstallDispatchReport {
    pub store: String,
    pub title: String,
    pub download_url: String,
    pub save_dir: String,
    pub queue_len: usize,
    pub summary: String,
}

pub fn gog_oauth_authorize_url() -> String {
    format!(
        "https://auth.gog.com/auth?client_id={}&redirect_uri={}&response_type=code&layout=client2",
        GOG_CLIENT_ID,
        urlencoding::encode(GOG_REDIRECT_URI)
    )
}

pub fn epic_oauth_authorize_url() -> String {
    format!(
        "https://www.epicgames.com/id/login?redirectUrl={}",
        urlencoding::encode(&format!(
            "https://www.epicgames.com/id/api/redirect?clientId={EPIC_LAUNCHER_CLIENT_ID}&responseType=code"
        ))
    )
}

pub fn parse_gog_owned_products_json(json_str: &str) -> Result<Vec<DirectStoreTitle>, String> {
    let v: Value =
        serde_json::from_str(json_str).map_err(|e| format!("Invalid GOG products JSON: {e}"))?;
    let products = v
        .get("products")
        .and_then(|p| p.as_array())
        .ok_or_else(|| "Missing products array in GOG response".to_string())?;
    let mut out = Vec::with_capacity(products.len());
    for p in products {
        let product_id = p
            .get("id")
            .and_then(|n| {
                n.as_u64()
                    .map(|u| u.to_string())
                    .or_else(|| n.as_str().map(str::to_string))
            })
            .unwrap_or_default();
        let title = p
            .get("title")
            .and_then(|s| s.as_str())
            .unwrap_or("GOG Title")
            .trim()
            .to_string();
        if product_id.is_empty() || title.is_empty() {
            continue;
        }
        out.push(DirectStoreTitle {
            store: "GOG".to_string(),
            product_id: product_id.clone(),
            app_name: product_id,
            namespace: "gog".to_string(),
            title,
            version: "latest".to_string(),
            total_size_bytes: 0,
            download_url: String::new(),
            installer_filename: String::new(),
        });
    }
    Ok(out)
}

pub fn parse_gog_builds_json(json_str: &str) -> Option<(String, String)> {
    let v: Value = serde_json::from_str(json_str).ok()?;
    let items = v.get("items")?.as_array()?;
    let item = items
        .iter()
        .find(|i| {
            i.get("os")
                .and_then(|o| o.as_str())
                .map(|o| o.eq_ignore_ascii_case("windows"))
                .unwrap_or(true)
        })
        .or_else(|| items.first())?;
    let link = item.get("link")?.as_str()?.trim().to_string();
    let version = item
        .get("version_name")
        .and_then(|s| s.as_str())
        .unwrap_or("v2")
        .trim()
        .to_string();
    Some((version, link))
}

pub fn parse_epic_assets_json(json_str: &str) -> Result<Vec<DirectStoreTitle>, String> {
    let v: Value =
        serde_json::from_str(json_str).map_err(|e| format!("Invalid Epic assets JSON: {e}"))?;
    let arr = v
        .as_array()
        .ok_or_else(|| "Expected JSON array from Epic assets API".to_string())?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let app_name = item
            .get("appName")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let namespace = item
            .get("namespace")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let catalog_item_id = item
            .get("catalogItemId")
            .and_then(|s| s.as_str())
            .unwrap_or(&app_name)
            .to_string();
        let version = item
            .get("buildVersion")
            .and_then(|s| s.as_str())
            .unwrap_or("1.0")
            .to_string();
        if app_name.is_empty() || namespace.is_empty() {
            continue;
        }
        out.push(DirectStoreTitle {
            store: "Epic".to_string(),
            product_id: catalog_item_id,
            app_name: app_name.clone(),
            namespace,
            title: app_name,
            version,
            total_size_bytes: 0,
            download_url: String::new(),
            installer_filename: String::new(),
        });
    }
    Ok(out)
}

pub fn parse_epic_manifest_distribution_json(json_str: &str) -> Option<(String, String)> {
    let v: Value = serde_json::from_str(json_str).ok()?;
    let elem = v.get("elements")?.as_array()?.first()?;
    let manifests = elem.get("manifests")?.as_array()?;
    let m = manifests.first()?;
    let uri = m.get("uri")?.as_str()?.trim().to_string();
    let app = elem
        .get("appName")
        .and_then(|s| s.as_str())
        .unwrap_or("epic-game")
        .to_string();
    Some((app, uri))
}

#[tauri::command]
pub async fn get_store_direct_status() -> StoreDirectStatus {
    let sess = load_persisted_store_sessions();
    StoreDirectStatus {
        gog_connected: !sess.gog_access_token.is_empty() || !sess.gog_refresh_token.is_empty(),
        gog_user_id: sess.gog_user_id,
        gog_auth_url: gog_oauth_authorize_url(),
        epic_connected: !sess.epic_access_token.is_empty() || !sess.epic_refresh_token.is_empty(),
        epic_account_id: sess.epic_account_id,
        epic_display_name: sess.epic_display_name,
        epic_auth_url: epic_oauth_authorize_url(),
    }
}

#[tauri::command]
pub async fn login_gog_direct(
    auth_code_or_token: String,
) -> Result<StoreDirectStatus, String> {
    let raw = auth_code_or_token.trim();
    if raw.is_empty() {
        return Err("Provide a GOG authorization code or refresh/access token.".to_string());
    }
    let client = duke_core::net_doh::get_configured_http_client();
    let mut sess = load_persisted_store_sessions();

    // Support direct Bearer/JWT pasting or OAuth code exchange.
    if raw.contains('.') && raw.len() > 80 {
        sess.gog_access_token = raw.to_string();
    } else {
        let resp = client
            .get(GOG_TOKEN_URL)
            .query(&[
                ("client_id", GOG_CLIENT_ID),
                ("client_secret", GOG_CLIENT_SECRET),
                ("grant_type", "authorization_code"),
                ("code", raw),
                ("redirect_uri", GOG_REDIRECT_URI),
            ])
            .timeout(Duration::from_secs(12))
            .send()
            .await
            .map_err(|e| format!("GOG OAuth exchange failed: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("GOG OAuth HTTP {}", resp.status()));
        }
        let body: Value = resp
            .json()
            .await
            .map_err(|e| format!("Invalid GOG OAuth JSON: {e}"))?;
        sess.gog_access_token = body
            .get("access_token")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        if let Some(rt) = body.get("refresh_token").and_then(|s| s.as_str()) {
            sess.gog_refresh_token = rt.to_string();
        }
        if let Some(uid) = body.get("user_id").and_then(|s| s.as_str()) {
            sess.gog_user_id = uid.to_string();
        }
    }
    save_persisted_store_sessions(&sess)?;
    Ok(get_store_direct_status().await)
}

#[tauri::command]
pub async fn login_epic_direct(
    authorization_code: String,
) -> Result<StoreDirectStatus, String> {
    let code = authorization_code.trim();
    if code.is_empty() {
        return Err("Provide an Epic authorization code from the OAuth redirect.".to_string());
    }
    let basic = base64::engine::general_purpose::STANDARD
        .encode(format!("{EPIC_LAUNCHER_CLIENT_ID}:{EPIC_LAUNCHER_SECRET}"));
    let client = duke_core::net_doh::get_configured_http_client();
    let mut form = HashMap::new();
    form.insert("grant_type", "authorization_code");
    form.insert("code", code);

    let resp = client
        .post(EPIC_OAUTH_URL)
        .header("Authorization", format!("Basic {basic}"))
        .form(&form)
        .timeout(Duration::from_secs(12))
        .send()
        .await
        .map_err(|e| format!("Epic OAuth exchange failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Epic OAuth HTTP {}", resp.status()));
    }
    let body: Value = resp
        .json()
        .await
        .map_err(|e| format!("Invalid Epic OAuth JSON: {e}"))?;

    let mut sess = load_persisted_store_sessions();
    sess.epic_access_token = body
        .get("access_token")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    sess.epic_refresh_token = body
        .get("refresh_token")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    sess.epic_account_id = body
        .get("account_id")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    sess.epic_display_name = body
        .get("displayName")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    save_persisted_store_sessions(&sess)?;
    Ok(get_store_direct_status().await)
}

#[tauri::command]
pub async fn list_gog_owned_titles() -> Result<Vec<DirectStoreTitle>, String> {
    let sess = load_persisted_store_sessions();
    if sess.gog_access_token.is_empty() {
        return Err("Authenticate with GOG OAuth first.".to_string());
    }
    let client = duke_core::net_doh::get_configured_http_client();
    let resp = client
        .get(GOG_OWNED_PRODUCTS_URL)
        .bearer_auth(&sess.gog_access_token)
        .query(&[("mediaType", "1"), ("page", "1")])
        .timeout(Duration::from_secs(12))
        .send()
        .await
        .map_err(|e| format!("Failed fetching GOG owned products: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("GOG owned products HTTP {}", resp.status()));
    }
    let text = resp
        .text()
        .await
        .map_err(|e| format!("Failed reading GOG products response: {e}"))?;
    parse_gog_owned_products_json(&text)
}

#[tauri::command]
pub async fn list_epic_owned_titles() -> Result<Vec<DirectStoreTitle>, String> {
    let sess = load_persisted_store_sessions();
    if sess.epic_access_token.is_empty() {
        return Err("Authenticate with Epic OAuth first.".to_string());
    }
    let client = duke_core::net_doh::get_configured_http_client();
    let resp = client
        .get(EPIC_ASSETS_URL)
        .bearer_auth(&sess.epic_access_token)
        .query(&[("label", "Live")])
        .timeout(Duration::from_secs(12))
        .send()
        .await
        .map_err(|e| format!("Failed fetching Epic owned assets: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Epic assets HTTP {}", resp.status()));
    }
    let text = resp
        .text()
        .await
        .map_err(|e| format!("Failed reading Epic assets response: {e}"))?;
    parse_epic_assets_json(&text)
}

#[tauri::command]
pub async fn start_store_direct_install(
    app: AppHandle,
    store: String,
    product_id: String,
    app_name: Option<String>,
    namespace: Option<String>,
    title: String,
    save_dir: Option<String>,
) -> Result<DirectInstallDispatchReport, String> {
    let sess = load_persisted_store_sessions();
    let client = duke_core::net_doh::get_configured_http_client();
    let store_norm = store.trim().to_ascii_lowercase();

    let (cdn_url, suggested_name) = if store_norm == "gog" {
        if sess.gog_access_token.is_empty() {
            return Err("GOG OAuth token required for direct CDN download.".to_string());
        }
        let builds_url = format!("{}/{}/os/windows/builds?generation=2", GOG_BUILDS_URL, product_id.trim());
        let builds_resp = client
            .get(&builds_url)
            .bearer_auth(&sess.gog_access_token)
            .timeout(Duration::from_secs(12))
            .send()
            .await
            .map_err(|e| format!("Failed querying GOG CDN builds: {e}"))?;
        let builds_text = builds_resp.text().await.unwrap_or_default();
        if let Some((ver, link)) = parse_gog_builds_json(&builds_text) {
            (
                link,
                Some(format!("{}_{ver}.gogmanifest.json", crate::sanitize_filename(&title))),
            )
        } else {
            let api_url = format!("{}/{}?expand=downloads", GOG_PRODUCT_DOWNLOADS_URL, product_id.trim());
            let prod_resp = client
                .get(&api_url)
                .bearer_auth(&sess.gog_access_token)
                .timeout(Duration::from_secs(12))
                .send()
                .await
                .map_err(|e| format!("Failed querying GOG product installers: {e}"))?;
            let val: Value = prod_resp
                .json()
                .await
                .map_err(|e| format!("Invalid GOG product JSON: {e}"))?;
            let manual_url = val
                .pointer("/downloads/installers/0/files/0/downlink")
                .and_then(|s| s.as_str())
                .ok_or_else(|| "No Windows CDN installer link available for this GOG title.".to_string())?;
            (
                manual_url.to_string(),
                Some(format!("{}_setup.exe", crate::sanitize_filename(&title))),
            )
        }
    } else if store_norm == "epic" {
        if sess.epic_access_token.is_empty() {
            return Err("Epic OAuth token required for direct CDN download.".to_string());
        }
        let ns = namespace.unwrap_or_default();
        let app_id = app_name.unwrap_or_else(|| product_id.clone());
        let manifest_api = format!(
            "{}/namespace/{}/catalogItem/{}/appName/{}/label/Live",
            EPIC_ASSETS_URL,
            urlencoding::encode(&ns),
            urlencoding::encode(product_id.trim()),
            urlencoding::encode(&app_id),
        );
        let resp = client
            .get(&manifest_api)
            .bearer_auth(&sess.epic_access_token)
            .timeout(Duration::from_secs(12))
            .send()
            .await
            .map_err(|e| format!("Failed fetching Epic CDN manifest URL: {e}"))?;
        let text = resp.text().await.unwrap_or_default();
        let (_, uri) = parse_epic_manifest_distribution_json(&text)
            .ok_or_else(|| "Epic did not return a downloadable CDN manifest URI.".to_string())?;
        (
            uri,
            Some(format!("{}.epicmanifest", crate::sanitize_filename(&title))),
        )
    } else {
        return Err(format!("Unsupported direct storefront '{}'. Use 'gog' or 'epic'.", store));
    };

    let target_dir = save_dir
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            crate::resolve_default_downloads_dir(&app)
                .to_string_lossy()
                .into_owned()
        });

    let queue = crate::enqueue_download_internal(
        app,
        cdn_url.clone(),
        None,
        None,
        None,
        suggested_name,
        Some(target_dir.clone()),
        Some(8),
        None,
        true,
        false,
    )
    .await?;

    Ok(DirectInstallDispatchReport {
        store: store.clone(),
        title: title.clone(),
        download_url: cdn_url,
        save_dir: target_dir,
        queue_len: queue.len(),
        summary: format!(
            "Dispatched direct {} CDN manifest/installer for '{}' into the 8-worker AIMD queue.",
            store, title
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gog_owned_products_and_builds_v2() {
        let owned = r#"{
            "products": [
                { "id": 1207658924, "title": "System Shock 2" },
                { "id": 1441974651, "title": "Baldurs Gate 3" }
            ]
        }"#;
        let list = parse_gog_owned_products_json(owned).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].product_id, "1207658924");
        assert_eq!(list[1].title, "Baldurs Gate 3");

        let builds = r#"{
            "items": [
                { "os": "windows", "version_name": "4.1.1", "link": "https://cdn.gog.com/content-system/v2/meta/aa/bb/manifest" }
            ]
        }"#;
        let (ver, url) = parse_gog_builds_json(builds).unwrap();
        assert_eq!(ver, "4.1.1");
        assert!(url.starts_with("https://cdn.gog.com/"));
    }

    #[test]
    fn parses_epic_assets_and_manifest_distribution_uri() {
        let assets = r#"[
            { "appName": "Sugar", "namespace": "ns1", "catalogItemId": "cat1", "buildVersion": "1.2.3" }
        ]"#;
        let list = parse_epic_assets_json(assets).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].app_name, "Sugar");
        assert_eq!(list[0].namespace, "ns1");

        let dist = r#"{
            "elements": [
                {
                    "appName": "Sugar",
                    "manifests": [
                        { "uri": "https://fastly-download.epicgames.com/Builds/Org/CloudDir/test.manifest" }
                    ]
                }
            ]
        }"#;
        let (app, uri) = parse_epic_manifest_distribution_json(dist).unwrap();
        assert_eq!(app, "Sugar");
        assert!(uri.contains(".manifest"));
    }
}

use reqwest::header::{ACCEPT, USER_AGENT};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::AppHandle;
use tokio::io::{AsyncWriteExt, BufWriter};

pub const GITHUB_LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/willferrara/duke-systems/releases/latest";

const UPDATE_API_TIMEOUT_SECS: u64 = 20;
const UPDATE_DOWNLOAD_TIMEOUT_SECS: u64 = 300;
const MAX_RELEASE_JSON_BYTES: usize = 4 * 1024 * 1024;
const PROGRESS_EMIT_MIN_BYTES: u64 = 128 * 1024;
const PROGRESS_EMIT_MIN_INTERVAL_MS: u64 = 100;
const SHA256_STREAM_BUF_LEN: usize = 65_536;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseAssetInfo {
    pub name: String,
    pub download_url: String,
    pub size_bytes: u64,
    pub sha256_digest: Option<String>,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckReport {
    pub current_version: String,
    pub latest_version: String,
    pub tag_name: String,
    pub update_available: bool,
    pub release_name: String,
    pub release_notes: String,
    pub published_at: String,
    pub html_url: String,
    pub install_mode: String,
    pub selected_asset: Option<ReleaseAssetInfo>,
    pub extra_assets: Vec<ReleaseAssetInfo>,
    pub all_assets: Vec<ReleaseAssetInfo>,
    pub checked_at_unix: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgressPayload {
    pub stage: String,
    pub message: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub progress_pct: f64,
    pub asset_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateApplyResult {
    pub launched: bool,
    pub version: String,
    pub asset_name: String,
    pub sha256_verified: String,
    pub install_mode: String,
    pub staged_path: String,
    pub extra_assets_staged: Vec<String>,
    pub message: String,
}

static LATEST_UPDATE_CACHE: OnceLock<Mutex<Option<UpdateCheckReport>>> = OnceLock::new();

pub fn get_cached_update_report() -> Option<UpdateCheckReport> {
    let cell = LATEST_UPDATE_CACHE.get_or_init(|| Mutex::new(None));
    cell.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

pub fn set_cached_update_report(report: UpdateCheckReport) {
    let cell = LATEST_UPDATE_CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = cell.lock().unwrap_or_else(|p| p.into_inner());
    *guard = Some(report);
}

pub fn spawn_startup_update_check(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(4)).await;
        if crate::is_shutting_down() {
            return;
        }
        if let Ok(report) = check_for_app_updates_internal().await {
            if !crate::is_shutting_down() && report.update_available {
                crate::emit_if_alive(&app, "app-update-available", &report);
            }
        }
    });
}

fn strip_leading_v(raw: &str) -> &str {
    let trimmed = raw.trim();
    trimmed
        .strip_prefix('v')
        .or_else(|| trimmed.strip_prefix('V'))
        .unwrap_or(trimmed)
}

pub fn parse_semver(raw: &str) -> Option<(u64, u64, u64)> {
    let no_prefix = strip_leading_v(raw);
    let core = no_prefix.split(['-', '+']).next()?.trim();
    if core.is_empty() {
        return None;
    }
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() != 2 && parts.len() != 3 {
        return None;
    }
    let parse_component = |segment: &str| -> Option<u64> {
        if segment.is_empty() || !segment.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        segment.parse::<u64>().ok()
    };
    let major = parse_component(parts[0])?;
    let minor = parse_component(parts[1])?;
    let patch = if parts.len() == 3 {
        parse_component(parts[2])?
    } else {
        0
    };
    Some((major, minor, patch))
}

pub fn is_newer_version(current: &str, latest: &str) -> bool {
    match (parse_semver(current), parse_semver(latest)) {
        (Some(cur), Some(lat)) => lat > cur,
        _ => false,
    }
}

pub fn parse_sha256_digest_field(digest: Option<&str>) -> Result<String, String> {
    let raw = digest
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Release asset is missing mandatory sha256 digest.".to_string())?;
    let hex_part = raw.strip_prefix("sha256:").ok_or_else(|| {
        format!(
            "Invalid asset digest format '{}': expected 'sha256:<64 hex chars>'.",
            raw
        )
    })?;
    if hex_part.len() != 64 || !hex_part.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!(
            "Invalid sha256 digest length or characters in '{}'.",
            raw
        ));
    }
    Ok(hex_part.to_ascii_lowercase())
}

pub fn detect_install_mode_with_portable(exe_path: &Path, is_portable_flag: bool) -> &'static str {
    if is_portable_flag {
        return "portable";
    }
    let lower = exe_path.to_string_lossy().to_ascii_lowercase();
    if lower.contains(r"\program files")
        || lower.contains(r"\appdata\local\programs")
        || lower.contains("/program files")
        || lower.contains("/appdata/local/programs")
    {
        "installed"
    } else {
        "portable"
    }
}

pub fn detect_current_install_mode() -> &'static str {
    let is_portable = crate::resolve_portable_data_dir().is_some();
    match std::env::current_exe() {
        Ok(exe) => detect_install_mode_with_portable(&exe, is_portable),
        Err(_) => "portable",
    }
}

pub fn classify_release_asset(name: &str) -> &'static str {
    let low = name.trim().to_ascii_lowercase();
    if low.is_empty() {
        return "other";
    }
    if low.ends_with(".msi")
        || (low.ends_with(".exe") && (low.contains("setup") || low.contains("installer")))
    {
        return "installer";
    }
    if low.ends_with(".exe") {
        return "portable_exe";
    }
    if low.contains("symbols")
        || low.contains("debug")
        || low.ends_with(".pdb")
        || low.ends_with(".sha256")
        || low.ends_with(".sig")
        || low.ends_with(".asc")
        || low.ends_with(".txt")
        || low.ends_with(".md")
    {
        return "other";
    }
    if low.ends_with(".dkcat")
        || low.ends_with(".dkcat.zst")
        || low.ends_with(".dkpack")
        || low.ends_with(".zip")
        || low.contains("bundle")
    {
        return "bundle_or_catalog";
    }
    "other"
}

fn installer_priority_rank(name: &str) -> u8 {
    let low = name.trim().to_ascii_lowercase();
    if low == "duke launcher setup.exe" {
        0
    } else if low.ends_with(".exe") && low.contains("setup") {
        1
    } else if low.ends_with(".exe") {
        2
    } else {
        3
    }
}

fn portable_exe_priority_rank(name: &str) -> u8 {
    let low = name.trim().to_ascii_lowercase();
    if low == "duke launcher.exe" || low == "duke-launcher.exe" {
        0
    } else if low.contains("duke") && low.contains("launcher") && low.ends_with(".exe") {
        1
    } else {
        2
    }
}

pub fn select_primary_release_asset(
    assets: &[ReleaseAssetInfo],
    install_mode: &str,
) -> Option<ReleaseAssetInfo> {
    let best_installer = || {
        assets
            .iter()
            .filter(|a| a.kind == "installer")
            .min_by_key(|a| installer_priority_rank(&a.name))
            .cloned()
    };
    let best_portable = || {
        assets
            .iter()
            .filter(|a| a.kind == "portable_exe")
            .min_by_key(|a| portable_exe_priority_rank(&a.name))
            .cloned()
    };

    if install_mode.eq_ignore_ascii_case("installed") {
        best_installer().or_else(best_portable)
    } else {
        best_portable().or_else(best_installer)
    }
}

pub fn parse_github_release_json(
    json_bytes: &[u8],
    current_version: &str,
    exe_path: &Path,
    is_portable_flag: bool,
) -> Result<UpdateCheckReport, String> {
    let val: serde_json::Value = serde_json::from_slice(json_bytes)
        .map_err(|e| format!("Invalid GitHub release JSON: {}", e))?;

    let tag_name = val
        .get("tag_name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or("")
        .to_string();
    if tag_name.is_empty() {
        return Err("GitHub release JSON missing required tag_name.".to_string());
    }

    let latest_version = strip_leading_v(&tag_name).to_string();
    let release_name = val
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&tag_name)
        .to_string();
    let release_notes = val
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let published_at = val
        .get("published_at")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let html_url = val
        .get("html_url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let is_draft = val.get("draft").and_then(|v| v.as_bool()).unwrap_or(false);

    let install_mode = detect_install_mode_with_portable(exe_path, is_portable_flag).to_string();

    let mut all_assets = Vec::new();
    if let Some(raw_assets) = val.get("assets").and_then(|v| v.as_array()) {
        for raw_asset in raw_assets {
            let name = raw_asset
                .get("name")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .unwrap_or("")
                .to_string();
            let download_url = raw_asset
                .get("browser_download_url")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .unwrap_or("")
                .to_string();
            if name.is_empty() || download_url.is_empty() {
                continue;
            }
            let size_bytes = raw_asset.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
            let sha256_digest = raw_asset
                .get("digest")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned);
            let kind = classify_release_asset(&name).to_string();
            all_assets.push(ReleaseAssetInfo {
                name,
                download_url,
                size_bytes,
                sha256_digest,
                kind,
            });
        }
    }

    let selected_asset = select_primary_release_asset(&all_assets, &install_mode);
    let extra_assets = all_assets
        .iter()
        .filter(|asset| {
            asset.kind == "bundle_or_catalog"
                && selected_asset
                    .as_ref()
                    .is_none_or(|sel| sel.name != asset.name)
        })
        .cloned()
        .collect();

    let update_available = !is_draft && is_newer_version(current_version, &tag_name);
    let checked_at_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    Ok(UpdateCheckReport {
        current_version: strip_leading_v(current_version).to_string(),
        latest_version,
        tag_name,
        update_available,
        release_name,
        release_notes,
        published_at,
        html_url,
        install_mode,
        selected_asset,
        extra_assets,
        all_assets,
        checked_at_unix,
        error: None,
    })
}

pub fn verify_file_sha256_streaming(path: &Path, expected_hex: &str) -> Result<String, String> {
    let trimmed = expected_hex.trim();
    let normalized_expected = trimmed
        .strip_prefix("sha256:")
        .unwrap_or(trimmed)
        .to_ascii_lowercase();
    if normalized_expected.len() != 64
        || !normalized_expected.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(format!(
            "Invalid expected SHA-256 hex digest '{}'.",
            expected_hex
        ));
    }

    let mut file = File::open(path).map_err(|e| {
        format!(
            "Failed opening {} for SHA-256 verification: {}",
            path.display(),
            e
        )
    })?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; SHA256_STREAM_BUF_LEN];
    loop {
        let read_bytes = file.read(&mut buf).map_err(|e| {
            format!(
                "Failed reading {} during SHA-256 verification: {}",
                path.display(),
                e
            )
        })?;
        if read_bytes == 0 {
            break;
        }
        hasher.update(&buf[..read_bytes]);
    }

    let actual_hex = hex::encode(hasher.finalize());
    if actual_hex != normalized_expected {
        return Err(format!(
            "SHA-256 mismatch for {}: expected {}, got {}",
            path.display(),
            normalized_expected,
            actual_hex
        ));
    }
    Ok(actual_hex)
}

fn escape_ps_single_quoted(path: &Path) -> String {
    path.to_string_lossy().replace('\'', "''")
}

pub fn build_windows_update_trampoline_script(
    pid: u32,
    current_exe: &Path,
    staged_exe: &Path,
    install_mode: &str,
    staged_extras: &[(PathBuf, PathBuf)],
) -> String {
    let current_ps = escape_ps_single_quoted(current_exe);
    let staged_ps = escape_ps_single_quoted(staged_exe);

    let is_msi = staged_exe
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("msi"));

    let apply_block = if install_mode.eq_ignore_ascii_case("installed") {
        if is_msi {
            "    Start-Process -FilePath 'msiexec.exe' -ArgumentList @('/i', $stagedExe, '/passive', '/norestart') -Wait -WindowStyle Hidden\n    if (Test-Path -LiteralPath $stagedExe) {\n        Remove-Item -LiteralPath $stagedExe -Force -ErrorAction SilentlyContinue\n    }".to_string()
        } else {
            "    Start-Process -FilePath $stagedExe -ArgumentList @('/S') -Wait -WindowStyle Hidden\n    if (Test-Path -LiteralPath $stagedExe) {\n        Remove-Item -LiteralPath $stagedExe -Force -ErrorAction SilentlyContinue\n    }".to_string()
        }
    } else {
        [
            "    # Portable swap: preserve data/ and portable.txt alongside executable",
            "    $backupExe = \"$currentExe.old\"",
            "    if (Test-Path -LiteralPath $backupExe) {",
            "        Remove-Item -LiteralPath $backupExe -Force -ErrorAction SilentlyContinue",
            "    }",
            "    $movedOld = $false",
            "    try {",
            "        if (Test-Path -LiteralPath $currentExe) {",
            "            Move-FileWithRetry -SourcePath $currentExe -DestinationPath $backupExe",
            "            $movedOld = $true",
            "        }",
            "        Move-FileWithRetry -SourcePath $stagedExe -DestinationPath $currentExe",
            "    } catch {",
            "        if ($movedOld -and (Test-Path -LiteralPath $backupExe) -and -not (Test-Path -LiteralPath $currentExe)) {",
            "            Move-Item -LiteralPath $backupExe -DestinationPath $currentExe -Force -ErrorAction SilentlyContinue",
            "        }",
            "        throw",
            "    }",
        ]
        .join("\n")
    };

    let mut extras_block = String::new();
    for (staged_src, target_dst) in staged_extras {
        let src_ps = escape_ps_single_quoted(staged_src);
        let dst_ps = escape_ps_single_quoted(target_dst);
        extras_block.push_str(&format!(
            "    Move-FileWithRetry -SourcePath '{}' -DestinationPath '{}'\n",
            src_ps, dst_ps
        ));
    }

    format!(
        r#"$ErrorActionPreference = 'Stop'
$targetPid = {pid}
$currentExe = '{current_ps}'
$stagedExe = '{staged_ps}'
$stageDir = Split-Path -Parent $stagedExe

$deadline = (Get-Date).AddSeconds(45)
while ((Get-Date) -lt $deadline) {{
    $proc = Get-Process -Id $targetPid -ErrorAction SilentlyContinue
    if (-not $proc) {{ break }}
    Start-Sleep -Milliseconds 250
}}
Start-Sleep -Milliseconds 350

function Move-FileWithRetry {{
    param(
        [string]$SourcePath,
        [string]$DestinationPath
    )
    $parent = Split-Path -Parent $DestinationPath
    if ($parent -and -not (Test-Path -LiteralPath $parent)) {{
        New-Item -ItemType Directory -Path $parent -Force | Out-Null
    }}
    for ($attempt = 0; $attempt -lt 15; $attempt++) {{
        try {{
            if (Test-Path -LiteralPath $DestinationPath) {{
                Remove-Item -LiteralPath $DestinationPath -Force -ErrorAction SilentlyContinue
            }}
            Move-Item -LiteralPath $SourcePath -Destination $DestinationPath -Force
            return
        }} catch {{
            if ($attempt -eq 14) {{ throw }}
            Start-Sleep -Milliseconds 300
        }}
    }}
}}

try {{
{apply_block}
{extras_block}    $relaunchExe = $currentExe
    if (-not (Test-Path -LiteralPath $relaunchExe)) {{
        $exeDir = Split-Path -Parent $currentExe
        $fallbackExe = Join-Path $exeDir 'Duke Launcher.exe'
        if (Test-Path -LiteralPath $fallbackExe) {{
            $relaunchExe = $fallbackExe
        }}
    }}
    if (Test-Path -LiteralPath $relaunchExe) {{
        $launchDir = Split-Path -Parent $relaunchExe
        Start-Process -FilePath $relaunchExe -WorkingDirectory $launchDir
    }}
}} finally {{
    if ($PSCommandPath -and (Test-Path -LiteralPath $PSCommandPath)) {{
        Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue
    }}
    if ($stageDir -and (Test-Path -LiteralPath $stageDir)) {{
        $remaining = Get-ChildItem -LiteralPath $stageDir -Force -ErrorAction SilentlyContinue
        if (-not $remaining) {{
            Remove-Item -LiteralPath $stageDir -Force -ErrorAction SilentlyContinue
        }}
    }}
}}
"#
    )
}

pub async fn check_for_app_updates_internal() -> Result<UpdateCheckReport, String> {
    let user_agent = format!("Duke-Launcher/{}", env!("CARGO_PKG_VERSION"));
    let client = crate::net_engine::get_pooled_client_for_url(
        GITHUB_LATEST_RELEASE_API,
        Some(Duration::from_secs(UPDATE_API_TIMEOUT_SECS)),
        true,
    )
    .await;

    let resp = client
        .get(GITHUB_LATEST_RELEASE_API)
        .header(USER_AGENT, &user_agent)
        .header(ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("Failed querying GitHub Releases API: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!(
            "GitHub Releases API returned HTTP {}",
            resp.status().as_u16()
        ));
    }

    if resp
        .content_length()
        .is_some_and(|len| len > MAX_RELEASE_JSON_BYTES as u64)
    {
        return Err("GitHub release metadata payload exceeds maximum safe size.".to_string());
    }

    let body_bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("Failed reading GitHub release JSON body: {}", e))?;
    if body_bytes.len() > MAX_RELEASE_JSON_BYTES {
        return Err("GitHub release metadata body exceeds maximum safe size.".to_string());
    }

    let exe_path = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("Duke Launcher.exe"));
    let is_portable = crate::resolve_portable_data_dir().is_some();
    let report = parse_github_release_json(
        &body_bytes,
        env!("CARGO_PKG_VERSION"),
        &exe_path,
        is_portable,
    )?;

    set_cached_update_report(report.clone());
    Ok(report)
}

fn validate_release_download_url(raw_url: &str) -> Result<Url, String> {
    let trimmed = raw_url.trim();
    let parsed =
        Url::parse(trimmed).map_err(|e| format!("Invalid asset URL '{}': {}", trimmed, e))?;
    let host = parsed.host_str().unwrap_or("");
    if host.is_empty() {
        return Err("Release asset download URL is missing a hostname.".to_string());
    }
    match parsed.scheme() {
        "https" => Ok(parsed),
        "http" if host == "127.0.0.1" || host.eq_ignore_ascii_case("localhost") => Ok(parsed),
        other => Err(format!(
            "Unsupported release download scheme '{}': HTTPS is required.",
            other
        )),
    }
}

fn sanitize_asset_filename(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains("..")
        || trimmed.contains(':')
    {
        return Err(format!("Unsafe release asset filename '{}'.", raw));
    }
    let cleaned = crate::sanitize_filename(trimmed);
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        return Err(format!("Invalid release asset filename '{}'.", raw));
    }
    Ok(cleaned)
}

fn sanitize_version_dir_component(version: &str) -> String {
    let cleaned: String = strip_leading_v(version)
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('.');
    if trimmed.is_empty() {
        "latest".to_string()
    } else {
        trimmed.to_string()
    }
}

fn emit_update_progress(
    app: &AppHandle,
    stage: &str,
    message: &str,
    downloaded_bytes: u64,
    total_bytes: u64,
    asset_name: &str,
) {
    let progress_pct = if total_bytes > 0 {
        ((downloaded_bytes as f64 / total_bytes as f64) * 100.0).clamp(0.0, 100.0)
    } else if matches!(stage, "staging" | "restarting") {
        100.0
    } else {
        0.0
    };
    crate::emit_if_alive(
        app,
        "app-update-progress",
        UpdateProgressPayload {
            stage: stage.to_string(),
            message: message.to_string(),
            downloaded_bytes,
            total_bytes,
            progress_pct,
            asset_name: asset_name.to_string(),
        },
    );
}

async fn download_and_verify_asset_streaming(
    app: &AppHandle,
    asset: &ReleaseAssetInfo,
    expected_sha256_hex: &str,
    dest_path: &Path,
) -> Result<String, String> {
    let parsed_url = validate_release_download_url(&asset.download_url)?;
    let url_str = parsed_url.as_str();
    let user_agent = format!("Duke-Launcher/{}", env!("CARGO_PKG_VERSION"));

    emit_update_progress(
        app,
        "downloading",
        &format!("Downloading {}...", asset.name),
        0,
        asset.size_bytes,
        &asset.name,
    );

    let client = crate::net_engine::get_pooled_client_for_url(
        url_str,
        Some(Duration::from_secs(UPDATE_DOWNLOAD_TIMEOUT_SECS)),
        true,
    )
    .await;

    let mut resp = client
        .get(url_str)
        .header(USER_AGENT, &user_agent)
        .send()
        .await
        .map_err(|e| format!("Failed downloading {}: {}", asset.name, e))?;

    if !resp.status().is_success() {
        return Err(format!(
            "HTTP {} while downloading {}",
            resp.status().as_u16(),
            asset.name
        ));
    }

    let total_bytes = resp
        .content_length()
        .filter(|&n| n > 0)
        .unwrap_or(asset.size_bytes);

    let file = tokio::fs::File::create(dest_path).await.map_err(|e| {
        format!(
            "Failed creating temporary update file {}: {}",
            dest_path.display(),
            e
        )
    })?;
    let mut writer = BufWriter::with_capacity(SHA256_STREAM_BUF_LEN, file);
    let mut hasher = Sha256::new();
    let mut downloaded_bytes = 0u64;
    let mut last_emitted_bytes = 0u64;
    let mut last_emit_at = Instant::now();

    loop {
        if crate::APP_SHUTDOWN.load(Ordering::Acquire) {
            drop(writer.into_inner().into_std().await);
            let _ = fs::remove_file(dest_path);
            return Err("Update canceled because Duke Launcher is shutting down.".to_string());
        }
        match resp.chunk().await {
            Ok(Some(chunk)) => {
                if let Err(err) = writer.write_all(&chunk).await {
                    drop(writer.into_inner().into_std().await);
                    let _ = fs::remove_file(dest_path);
                    return Err(format!(
                        "Failed writing update chunk to {}: {}",
                        dest_path.display(),
                        err
                    ));
                }
                hasher.update(&chunk);
                downloaded_bytes = downloaded_bytes.saturating_add(chunk.len() as u64);

                let bytes_delta = downloaded_bytes.saturating_sub(last_emitted_bytes);
                let is_complete = total_bytes > 0 && downloaded_bytes >= total_bytes;
                if is_complete
                    || bytes_delta >= PROGRESS_EMIT_MIN_BYTES
                    || last_emit_at.elapsed()
                        >= Duration::from_millis(PROGRESS_EMIT_MIN_INTERVAL_MS)
                {
                    emit_update_progress(
                        app,
                        "downloading",
                        &format!("Downloading {}...", asset.name),
                        downloaded_bytes,
                        total_bytes,
                        &asset.name,
                    );
                    last_emitted_bytes = downloaded_bytes;
                    last_emit_at = Instant::now();
                }
            }
            Ok(None) => break,
            Err(err) => {
                drop(writer.into_inner().into_std().await);
                let _ = fs::remove_file(dest_path);
                return Err(format!(
                    "Network error while streaming {}: {}",
                    asset.name, err
                ));
            }
        }
    }

    if let Err(err) = writer.flush().await {
        drop(writer.into_inner().into_std().await);
        let _ = fs::remove_file(dest_path);
        return Err(format!(
            "Failed flushing staged file {}: {}",
            dest_path.display(),
            err
        ));
    }
    let std_file = writer.into_inner().into_std().await;
    let _ = std_file.sync_all();
    drop(std_file);

    emit_update_progress(
        app,
        "verifying",
        &format!("Verifying SHA-256 digest for {}...", asset.name),
        downloaded_bytes,
        total_bytes.max(downloaded_bytes),
        &asset.name,
    );

    let stream_hex = hex::encode(hasher.finalize());
    if stream_hex != expected_sha256_hex {
        let _ = fs::remove_file(dest_path);
        return Err(format!(
            "SHA-256 verification failed for {}: expected {}, got {}",
            asset.name, expected_sha256_hex, stream_hex
        ));
    }

    let verify_path = dest_path.to_path_buf();
    let expected_owned = expected_sha256_hex.to_string();
    let verify_res = tokio::task::spawn_blocking(move || {
        verify_file_sha256_streaming(&verify_path, &expected_owned)
    })
    .await
    .map_err(|e| format!("SHA-256 verification worker failed: {}", e))?;

    match verify_res {
        Ok(verified_hex) => {
            crate::self_heal::strip_zone_identifier_ads(dest_path);
            Ok(verified_hex)
        }
        Err(err) => {
            let _ = fs::remove_file(dest_path);
            Err(err)
        }
    }
}

fn spawn_detached_powershell_trampoline(script_path: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};

        const DETACHED_PROCESS: u32 = 0x0000_0008;
        let mut cmd = Command::new("powershell.exe");
        cmd.args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-WindowStyle",
            "Hidden",
            "-File",
        ])
        .arg(script_path)
        .creation_flags(crate::CREATE_NO_WINDOW | DETACHED_PROCESS)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

        cmd.spawn().map_err(|e| {
            format!(
                "Failed launching detached PowerShell update trampoline: {}",
                e
            )
        })?;
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = script_path;
        Err("Self-update trampoline is only supported on Windows.".to_string())
    }
}

pub async fn download_and_apply_app_update_internal(
    app: &AppHandle,
    target_version: Option<String>,
) -> Result<UpdateApplyResult, String> {
    let cached = get_cached_update_report();
    let report = match cached {
        Some(r) if r.update_available && r.selected_asset.is_some() => {
            if let Some(ref tv) = target_version {
                let want = strip_leading_v(tv);
                if want.eq_ignore_ascii_case(strip_leading_v(&r.latest_version))
                    || want.eq_ignore_ascii_case(strip_leading_v(&r.tag_name))
                {
                    r
                } else {
                    check_for_app_updates_internal().await?
                }
            } else {
                r
            }
        }
        _ => check_for_app_updates_internal().await?,
    };

    if let Some(ref tv) = target_version {
        let want = strip_leading_v(tv);
        if !want.eq_ignore_ascii_case(strip_leading_v(&report.latest_version))
            && !want.eq_ignore_ascii_case(strip_leading_v(&report.tag_name))
        {
            return Err(format!(
                "Requested update version '{}' does not match latest GitHub release '{}'.",
                tv, report.latest_version
            ));
        }
    }

    if !report.update_available {
        return Err(format!(
            "Duke Launcher v{} is already up to date (latest: v{}).",
            report.current_version, report.latest_version
        ));
    }

    let primary = report.selected_asset.clone().ok_or_else(|| {
        "No compatible Windows release asset found in the latest GitHub release.".to_string()
    })?;

    let expected_primary_sha = parse_sha256_digest_field(primary.sha256_digest.as_deref())?;
    let safe_primary_name = sanitize_asset_filename(&primary.name)?;

    let mut dkcat_extras: Vec<(ReleaseAssetInfo, String, String)> = Vec::new();
    for extra in &report.extra_assets {
        if extra.name.to_ascii_lowercase().ends_with(".dkcat") {
            let expected_extra_sha = parse_sha256_digest_field(extra.sha256_digest.as_deref())?;
            let safe_extra_name = sanitize_asset_filename(&extra.name)?;
            dkcat_extras.push((extra.clone(), expected_extra_sha, safe_extra_name));
        }
    }

    let safe_ver = sanitize_version_dir_component(&report.latest_version);
    let stage_dir = std::env::temp_dir().join(format!("duke-update-{}", safe_ver));
    let _ = fs::remove_dir_all(&stage_dir);
    fs::create_dir_all(&stage_dir).map_err(|e| {
        format!(
            "Failed creating update staging directory {}: {}",
            stage_dir.display(),
            e
        )
    })?;

    let staged_primary_path = stage_dir.join(&safe_primary_name);
    let verified_primary_sha = match download_and_verify_asset_streaming(
        app,
        &primary,
        &expected_primary_sha,
        &staged_primary_path,
    )
    .await
    {
        Ok(sha) => sha,
        Err(err) => {
            let _ = fs::remove_dir_all(&stage_dir);
            return Err(err);
        }
    };

    let data_root_dir = match crate::resolve_data_root_dir(app) {
        Ok(dir) => dir,
        Err(err) => {
            let _ = fs::remove_dir_all(&stage_dir);
            return Err(err);
        }
    };

    let mut staged_extras_pairs: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut extra_assets_staged: Vec<String> = Vec::new();
    for (extra_asset, expected_extra_sha, safe_extra_name) in &dkcat_extras {
        let staged_extra_path = stage_dir.join(safe_extra_name);
        if let Err(err) = download_and_verify_asset_streaming(
            app,
            extra_asset,
            expected_extra_sha,
            &staged_extra_path,
        )
        .await
        {
            let _ = fs::remove_dir_all(&stage_dir);
            return Err(err);
        }
        let target_extra_path = if safe_extra_name.eq_ignore_ascii_case("catalog.dkcat") {
            data_root_dir.join("catalog.dkcat")
        } else {
            data_root_dir.join(safe_extra_name)
        };
        staged_extras_pairs.push((staged_extra_path, target_extra_path));
        extra_assets_staged.push(safe_extra_name.clone());
    }

    emit_update_progress(
        app,
        "staging",
        &format!("Staging update v{}...", report.latest_version),
        primary.size_bytes,
        primary.size_bytes,
        &primary.name,
    );

    let current_exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(err) => {
            let _ = fs::remove_dir_all(&stage_dir);
            return Err(format!("Failed resolving current executable path: {}", err));
        }
    };

    let install_mode = detect_current_install_mode().to_string();
    let effective_apply_mode = if primary.kind == "installer" {
        "installed"
    } else if primary.kind == "portable_exe" {
        "portable"
    } else {
        install_mode.as_str()
    };

    let script_content = build_windows_update_trampoline_script(
        std::process::id(),
        &current_exe,
        &staged_primary_path,
        effective_apply_mode,
        &staged_extras_pairs,
    );
    let script_path = stage_dir.join("apply-duke-update.ps1");
    let mut script_bytes = Vec::with_capacity(3 + script_content.len());
    script_bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    script_bytes.extend_from_slice(script_content.as_bytes());
    if let Err(err) = fs::write(&script_path, &script_bytes) {
        let _ = fs::remove_dir_all(&stage_dir);
        return Err(format!(
            "Failed writing update trampoline script {}: {}",
            script_path.display(),
            err
        ));
    }

    if let Err(err) = spawn_detached_powershell_trampoline(&script_path) {
        let _ = fs::remove_dir_all(&stage_dir);
        return Err(err);
    }

    emit_update_progress(
        app,
        "restarting",
        &format!(
            "Applying Duke Launcher v{} and restarting...",
            report.latest_version
        ),
        primary.size_bytes,
        primary.size_bytes,
        &primary.name,
    );

    let app_for_exit = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(250)).await;
        if let Some(daemon) = crate::TRAY_DAEMON.get() {
            daemon.app_exiting.store(true, Ordering::Release);
        }
        crate::shutdown_all_runtime_services(&app_for_exit);
        app_for_exit.exit(0);
    });

    Ok(UpdateApplyResult {
        launched: true,
        version: report.latest_version.clone(),
        asset_name: primary.name.clone(),
        sha256_verified: verified_primary_sha,
        install_mode: install_mode.clone(),
        staged_path: staged_primary_path.display().to_string(),
        extra_assets_staged,
        message: format!(
            "Verified {} (SHA-256) and launched {} update trampoline for v{}.",
            primary.name, install_mode, report.latest_version
        ),
    })
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    static TEST_TMP_SEQ: AtomicU64 = AtomicU64::new(1);

    fn make_unique_temp_file(prefix: &str) -> PathBuf {
        let seq = TEST_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "duke-updater-test-{}-{}-{}.bin",
            prefix,
            std::process::id(),
            seq
        ))
    }

    #[test]
    fn test_parse_semver_and_is_newer_version() {
        assert_eq!(parse_semver("v2.3.0"), Some((2, 3, 0)));
        assert_eq!(parse_semver("V2.3"), Some((2, 3, 0)));
        assert_eq!(parse_semver("2.4.1-beta.2+build99"), Some((2, 4, 1)));
        assert_eq!(parse_semver(" 3.0+sha256 "), Some((3, 0, 0)));
        assert_eq!(parse_semver("1"), None);
        assert_eq!(parse_semver("1.2.3.4"), None);
        assert_eq!(parse_semver("vv1.2.3"), None);
        assert_eq!(parse_semver("1.+2.3"), None);

        assert!(is_newer_version("2.2.0", "v2.3.0"));
        assert!(is_newer_version("2.2.0", "2.2.1"));
        assert!(!is_newer_version("2.2.0", "v2.2.0"));
        assert!(!is_newer_version("2.3.0", "v2.2.9"));
        assert!(!is_newer_version("2.2.0", "invalid"));
    }

    #[test]
    fn test_parse_sha256_digest_field_and_github_release_json() {
        let upper_hex = "A".repeat(64);
        let lower_hex = "a".repeat(64);
        let digest_input = format!("sha256:{}", upper_hex);
        assert_eq!(
            parse_sha256_digest_field(Some(&digest_input)),
            Ok(lower_hex.clone())
        );
        assert!(parse_sha256_digest_field(None).is_err());
        assert!(parse_sha256_digest_field(Some("")).is_err());
        assert!(parse_sha256_digest_field(Some(&lower_hex)).is_err());
        assert!(parse_sha256_digest_field(Some("sha256:abcd")).is_err());
        assert!(parse_sha256_digest_field(Some(&format!("sha256:{}z", "a".repeat(63)))).is_err());
        assert!(parse_sha256_digest_field(Some(&format!("md5:{}", "a".repeat(64)))).is_err());

        let json = serde_json::json!({
            "tag_name": "v2.5.0",
            "name": "Duke Launcher v2.5.0",
            "body": "## Highlights\n- Self-update support\n- Faster mmap catalog",
            "published_at": "2026-11-15T12:00:00Z",
            "html_url": "https://github.com/willferrara/duke-systems/releases/tag/v2.5.0",
            "draft": false,
            "assets": [
                {
                    "name": "Duke Launcher Setup.exe",
                    "browser_download_url": "https://github.com/willferrara/duke-systems/releases/download/v2.5.0/Duke.Launcher.Setup.exe",
                    "size": 10240000,
                    "digest": format!("sha256:{}", "b".repeat(64))
                },
                {
                    "name": "Duke Launcher.exe",
                    "browser_download_url": "https://github.com/willferrara/duke-systems/releases/download/v2.5.0/Duke.Launcher.exe",
                    "size": 4200000,
                    "digest": format!("sha256:{}", "c".repeat(64))
                },
                {
                    "name": "catalog.dkcat",
                    "browser_download_url": "https://github.com/willferrara/duke-systems/releases/download/v2.5.0/catalog.dkcat",
                    "size": 1500000,
                    "digest": format!("sha256:{}", "d".repeat(64))
                },
                {
                    "name": "retro-bundle.zip",
                    "browser_download_url": "https://github.com/willferrara/duke-systems/releases/download/v2.5.0/retro-bundle.zip",
                    "size": 500000
                },
                {
                    "name": "Duke-Launcher.pdb",
                    "browser_download_url": "https://github.com/willferrara/duke-systems/releases/download/v2.5.0/Duke-Launcher.pdb",
                    "size": 900000
                }
            ]
        });
        let raw_bytes = match serde_json::to_vec(&json) {
            Ok(b) => b,
            Err(e) => panic!("failed serializing fixture json: {e}"),
        };

        let report = match parse_github_release_json(
            &raw_bytes,
            "2.2.0",
            Path::new(r"C:\Program Files\Duke Launcher\Duke Launcher.exe"),
            false,
        ) {
            Ok(r) => r,
            Err(e) => panic!("expected valid release report, got err: {e}"),
        };

        assert!(report.update_available);
        assert_eq!(report.current_version, "2.2.0");
        assert_eq!(report.latest_version, "2.5.0");
        assert_eq!(report.tag_name, "v2.5.0");
        assert_eq!(report.install_mode, "installed");
        assert!(report.release_notes.contains("Self-update support"));
        assert_eq!(report.all_assets.len(), 5);
        assert_eq!(report.extra_assets.len(), 2);
        assert_eq!(
            report.selected_asset.as_ref().map(|a| a.name.as_str()),
            Some("Duke Launcher Setup.exe")
        );
    }

    #[test]
    fn test_detect_install_mode_and_select_primary_release_asset() {
        assert_eq!(
            detect_install_mode_with_portable(
                Path::new(r"C:\Program Files\Duke Launcher\Duke Launcher.exe"),
                false
            ),
            "installed"
        );
        assert_eq!(
            detect_install_mode_with_portable(
                Path::new(
                    r"C:\Users\Player\AppData\Local\Programs\Duke Launcher\Duke Launcher.exe"
                ),
                false
            ),
            "installed"
        );
        assert_eq!(
            detect_install_mode_with_portable(
                Path::new(r"C:\Program Files\Duke Launcher\Duke Launcher.exe"),
                true
            ),
            "portable"
        );
        assert_eq!(
            detect_install_mode_with_portable(
                Path::new(r"D:\PortableApps\DukeLauncher\Duke Launcher.exe"),
                false
            ),
            "portable"
        );

        assert_eq!(
            classify_release_asset("Duke Launcher Setup.exe"),
            "installer"
        );
        assert_eq!(
            classify_release_asset("Duke-Launcher-x64-setup.exe"),
            "installer"
        );
        assert_eq!(classify_release_asset("Duke-Launcher-x64.msi"), "installer");
        assert_eq!(classify_release_asset("Duke Launcher.exe"), "portable_exe");
        assert_eq!(
            classify_release_asset("Duke-Launcher-x64.exe"),
            "portable_exe"
        );
        assert_eq!(classify_release_asset("catalog.dkcat"), "bundle_or_catalog");
        assert_eq!(
            classify_release_asset("assets-bundle.zip"),
            "bundle_or_catalog"
        );
        assert_eq!(classify_release_asset("Duke-Launcher.pdb"), "other");

        let assets = vec![
            ReleaseAssetInfo {
                name: "Duke-Launcher-x64.msi".to_string(),
                download_url: "https://example.invalid/msi".to_string(),
                size_bytes: 100,
                sha256_digest: None,
                kind: "installer".to_string(),
            },
            ReleaseAssetInfo {
                name: "Duke Launcher Setup.exe".to_string(),
                download_url: "https://example.invalid/setup".to_string(),
                size_bytes: 100,
                sha256_digest: None,
                kind: "installer".to_string(),
            },
            ReleaseAssetInfo {
                name: "Duke-Launcher-x64.exe".to_string(),
                download_url: "https://example.invalid/x64exe".to_string(),
                size_bytes: 100,
                sha256_digest: None,
                kind: "portable_exe".to_string(),
            },
            ReleaseAssetInfo {
                name: "duke-launcher.exe".to_string(),
                download_url: "https://example.invalid/portable".to_string(),
                size_bytes: 100,
                sha256_digest: None,
                kind: "portable_exe".to_string(),
            },
        ];

        assert_eq!(
            select_primary_release_asset(&assets, "installed").map(|a| a.name),
            Some("Duke Launcher Setup.exe".to_string())
        );
        assert_eq!(
            select_primary_release_asset(&assets, "portable").map(|a| a.name),
            Some("duke-launcher.exe".to_string())
        );

        let portable_only = vec![assets[2].clone()];
        assert_eq!(
            select_primary_release_asset(&portable_only, "installed").map(|a| a.name),
            Some("Duke-Launcher-x64.exe".to_string())
        );
        let installer_only = vec![assets[1].clone()];
        assert_eq!(
            select_primary_release_asset(&installer_only, "portable").map(|a| a.name),
            Some("Duke Launcher Setup.exe".to_string())
        );
    }

    #[test]
    fn test_verify_file_sha256_streaming_pass_and_mismatch() {
        let tmp_path = make_unique_temp_file("sha");
        let payload = vec![0x5Au8; 100_000];
        if let Err(e) = fs::write(&tmp_path, &payload) {
            panic!("failed writing temp test payload: {e}");
        }

        let expected = hex::encode(Sha256::digest(&payload));
        let pass_res = verify_file_sha256_streaming(&tmp_path, &expected);
        let wrong_hex = "0".repeat(64);
        let fail_res = verify_file_sha256_streaming(&tmp_path, &wrong_hex);
        let _ = fs::remove_file(&tmp_path);

        assert_eq!(pass_res, Ok(expected));
        assert!(fail_res.is_err());
    }

    #[test]
    fn test_build_windows_update_trampoline_script() {
        let installed_script = build_windows_update_trampoline_script(
            4242,
            Path::new(r"C:\Program Files\Duke Launcher\Duke Launcher.exe"),
            Path::new(
                r"C:\Users\User\AppData\Local\Temp\duke-update-2.5.0\Duke Launcher Setup.exe",
            ),
            "installed",
            &[(
                PathBuf::from(r"C:\Users\User\AppData\Local\Temp\duke-update-2.5.0\catalog.dkcat"),
                PathBuf::from(
                    r"C:\Users\User\AppData\Roaming\com.dukelauncher.desktop\catalog.dkcat",
                ),
            )],
        );
        assert!(installed_script.contains("4242"));
        assert!(installed_script.contains("'/S'"));
        assert!(installed_script.contains("Duke Launcher Setup.exe"));
        assert!(installed_script.contains("catalog.dkcat"));
        assert!(installed_script.contains("Start-Process -FilePath $relaunchExe"));

        let portable_script = build_windows_update_trampoline_script(
            9876,
            Path::new(r"D:\Portable\Duke's App\Duke Launcher.exe"),
            Path::new(r"C:\Users\User\AppData\Local\Temp\duke-update-2.5.0\Duke Launcher.exe"),
            "portable",
            &[],
        );
        assert!(portable_script.contains("9876"));
        assert!(portable_script.contains(".old"));
        assert!(portable_script.contains("Duke''s App"));
        assert!(portable_script.contains("Move-FileWithRetry"));
        assert!(portable_script.contains("data/ and portable.txt"));
        assert!(portable_script.contains("Start-Process -FilePath $relaunchExe"));
    }
}

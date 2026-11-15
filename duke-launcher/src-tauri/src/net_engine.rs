//! Unified HTTP/2 + DoH + Tor/SOCKS5 connection pool with per-domain rate limiting
//! and on-disk `ETag` response caching.

use reqwest::header::{
    HeaderName, HeaderValue, CONTENT_TYPE, ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED,
};
use reqwest::{Client, Url};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_POOLED_CLIENTS: usize = 24;
const DEFAULT_DOMAIN_MIN_INTERVAL_MS: u64 = 60;
const MAX_TRACKED_RATE_DOMAINS: usize = 512;

static NET_ENGINE_STATE: OnceLock<NetEngineState> = OnceLock::new();
static TMP_WRITE_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NetEngineStats {
    pub total_requests: u64,
    pub cache_hits_304: u64,
    pub rate_limited_waits: u64,
    pub cached_clients: usize,
    pub doh_enabled: bool,
    pub proxy_active: bool,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone)]
pub(crate) struct CachedHttpResponse {
    pub status: u16,
    pub bytes: Vec<u8>,
    pub etag: Option<String>,
    pub from_cache_304: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ClientPoolKey {
    use_doh: bool,
    doh_epoch: u64,
    proxy_url: Option<String>,
    target_port: u16,
    follow_redirects: bool,
    timeout_ms: Option<u64>,
}

#[derive(Clone)]
struct PooledClientEntry {
    client: Client,
    last_used_tick: u64,
}

struct ClientPool {
    active_doh_enabled: bool,
    active_doh_epoch: u64,
    active_proxy_url: Option<String>,
    entries: HashMap<ClientPoolKey, PooledClientEntry>,
}

impl ClientPool {
    fn new() -> Self {
        Self {
            active_doh_enabled: duke_core::net_doh::is_doh_enabled(),
            active_doh_epoch: 0,
            active_proxy_url: None,
            entries: HashMap::new(),
        }
    }

    fn sync_epoch(&mut self, doh_enabled: bool, doh_epoch: u64, proxy_url: &Option<String>) {
        if self.active_doh_enabled != doh_enabled
            || self.active_doh_epoch != doh_epoch
            || &self.active_proxy_url != proxy_url
        {
            self.entries.clear();
            self.active_doh_enabled = doh_enabled;
            self.active_doh_epoch = doh_epoch;
            self.active_proxy_url = proxy_url.clone();
        }
    }

    fn insert_bounded(&mut self, doh_enabled: bool, key: ClientPoolKey, client: Client, tick: u64) {
        self.sync_epoch(doh_enabled, key.doh_epoch, &key.proxy_url);
        while self.entries.len() >= MAX_POOLED_CLIENTS && !self.entries.contains_key(&key) {
            let Some(evict_key) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used_tick)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            self.entries.remove(&evict_key);
        }
        self.entries.insert(
            key,
            PooledClientEntry {
                client,
                last_used_tick: tick,
            },
        );
    }
}

struct DomainRateLimiter {
    overrides: HashMap<String, Duration>,
    next_allowed: HashMap<String, Instant>,
}

impl DomainRateLimiter {
    fn new() -> Self {
        Self {
            overrides: HashMap::new(),
            next_allowed: HashMap::new(),
        }
    }

    fn resolve_interval(&self, host: &str, explicit_port: Option<u16>) -> Duration {
        if let Some(port) = explicit_port {
            let host_port = format!("{}:{}", host, port);
            if let Some(d) = self.overrides.get(&host_port) {
                return *d;
            }
        }
        if let Some(d) = self.overrides.get(host) {
            return *d;
        }
        Duration::from_millis(DEFAULT_DOMAIN_MIN_INTERVAL_MS)
    }

    fn reserve_wait_until(
        &mut self,
        slot_key: String,
        host: &str,
        explicit_port: Option<u16>,
        now: Instant,
    ) -> Option<Instant> {
        let min_interval = self.resolve_interval(host, explicit_port);
        if min_interval.is_zero() {
            return None;
        }

        if self.next_allowed.len() >= MAX_TRACKED_RATE_DOMAINS
            && !self.next_allowed.contains_key(&slot_key)
        {
            self.next_allowed.retain(|_, inst| *inst > now);
        }

        let entry = self.next_allowed.entry(slot_key).or_insert(now);
        let scheduled = if *entry > now { *entry } else { now };
        *entry = scheduled.checked_add(min_interval).unwrap_or(scheduled);
        if scheduled > now {
            Some(scheduled)
        } else {
            None
        }
    }
}

struct NetEngineState {
    pool: Mutex<ClientPool>,
    rate_limiter: Mutex<DomainRateLimiter>,
    total_requests: AtomicU64,
    cache_hits_304: AtomicU64,
    rate_limited_waits: AtomicU64,
    pool_access_tick: AtomicU64,
}

impl NetEngineState {
    fn new() -> Self {
        Self {
            pool: Mutex::new(ClientPool::new()),
            rate_limiter: Mutex::new(DomainRateLimiter::new()),
            total_requests: AtomicU64::new(0),
            cache_hits_304: AtomicU64::new(0),
            rate_limited_waits: AtomicU64::new(0),
            pool_access_tick: AtomicU64::new(1),
        }
    }
}

fn state() -> &'static NetEngineState {
    NET_ENGINE_STATE.get_or_init(NetEngineState::new)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EtagCacheMeta {
    url: String,
    etag: Option<String>,
    last_modified: Option<String>,
    content_type: Option<String>,
    fetched_at_unix: u64,
    body_len: u64,
    body_blake3: String,
}

struct LoadedCacheEntry {
    meta: EtagCacheMeta,
    body: Vec<u8>,
}

fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn clamp_nonzero_duration(d: Duration) -> Duration {
    if d.is_zero() {
        Duration::from_secs(1)
    } else {
        d
    }
}

fn duration_to_ms_key(d: Duration) -> u64 {
    u64::try_from(clamp_nonzero_duration(d).as_millis())
        .unwrap_or(u64::MAX)
        .max(1)
}

#[cfg_attr(not(test), allow(dead_code))]
fn normalize_domain_key(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.contains("://") {
        if let Ok(parsed) = Url::parse(trimmed) {
            if let Some(host) = parsed.host_str() {
                let clean = host.trim().trim_end_matches('.').to_ascii_lowercase();
                if let Some(port) = parsed.port() {
                    return format!("{}:{}", clean, port);
                }
                return clean;
            }
        }
    }
    trimmed.trim_end_matches('.').to_ascii_lowercase()
}

fn is_loopback_or_local_host(host: &str) -> bool {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = bare.parse::<IpAddr>() {
        return ip.is_loopback();
    }
    host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local")
}

fn extract_rate_limit_parts(parsed: &Url) -> (String, String, Option<u16>) {
    let host = parsed
        .host_str()
        .unwrap_or_default()
        .trim()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    let explicit_port = parsed.port();
    let slot_key = match explicit_port {
        Some(port) if is_loopback_or_local_host(&host) => format!("{}:{}", host, port),
        _ => host.clone(),
    };
    (slot_key, host, explicit_port)
}

async fn acquire_domain_rate_slot(parsed: &Url) {
    let (slot_key, host, explicit_port) = extract_rate_limit_parts(parsed);
    if host.is_empty() {
        return;
    }

    let wait_until: Option<Instant> = {
        let now = Instant::now();
        let mut limiter = state()
            .rate_limiter
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        limiter.reserve_wait_until(slot_key, &host, explicit_port, now)
    };

    if let Some(target_instant) = wait_until {
        state().rate_limited_waits.fetch_add(1, Ordering::Relaxed);
        tokio::time::sleep_until(tokio::time::Instant::from_std(target_instant)).await;
    }
}

fn cache_key_for_url(url: &str) -> String {
    blake3::hash(url.as_bytes()).to_hex().to_string()
}

fn cache_paths(cache_dir: &Path, key: &str) -> (PathBuf, PathBuf) {
    (
        cache_dir.join(format!("{}.json", key)),
        cache_dir.join(format!("{}.bin", key)),
    )
}

fn atomic_write_file(target: &Path, data: &[u8]) -> Result<(), String> {
    let parent = target
        .parent()
        .ok_or_else(|| format!("Cache path '{}' has no parent directory", target.display()))?;
    fs::create_dir_all(parent)
        .map_err(|e| format!("Failed creating cache directory '{}': {}", parent.display(), e))?;

    let seq = TMP_WRITE_SEQ.fetch_add(1, Ordering::Relaxed);
    let file_name = target
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("cache");
    let tmp_path = parent.join(format!("{}.{}.{}.tmp", file_name, std::process::id(), seq));

    let write_res = (|| -> Result<(), String> {
        let mut file = File::create(&tmp_path)
            .map_err(|e| format!("Failed creating temp cache '{}': {}", tmp_path.display(), e))?;
        file.write_all(data)
            .map_err(|e| format!("Failed writing temp cache '{}': {}", tmp_path.display(), e))?;
        file.flush()
            .map_err(|e| format!("Failed flushing temp cache '{}': {}", tmp_path.display(), e))?;
        drop(file);
        fs::rename(&tmp_path, target).map_err(|e| {
            format!(
                "Failed committing cache file '{}' -> '{}': {}",
                tmp_path.display(),
                target.display(),
                e
            )
        })
    })();

    if write_res.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }
    write_res
}

fn load_cached_entry_sync(cache_dir: &Path, url: &str) -> Option<LoadedCacheEntry> {
    let key = cache_key_for_url(url);
    let (meta_path, bin_path) = cache_paths(cache_dir, &key);
    let meta_raw = fs::read_to_string(&meta_path).ok()?;
    let meta: EtagCacheMeta = serde_json::from_str(&meta_raw).ok()?;
    if meta.url != url || (meta.etag.is_none() && meta.last_modified.is_none()) {
        return None;
    }

    let body = fs::read(&bin_path).ok()?;
    if body.len() as u64 != meta.body_len {
        return None;
    }
    let digest = blake3::hash(&body).to_hex().to_string();
    if digest != meta.body_blake3 {
        return None;
    }
    Some(LoadedCacheEntry { meta, body })
}

fn store_cached_entry_sync(cache_dir: &Path, meta: &EtagCacheMeta, body: &[u8]) -> Result<(), String> {
    let key = cache_key_for_url(&meta.url);
    let (meta_path, bin_path) = cache_paths(cache_dir, &key);
    atomic_write_file(&bin_path, body)?;
    let meta_bytes = serde_json::to_vec(meta)
        .map_err(|e| format!("Failed serializing ETag cache metadata: {}", e))?;
    atomic_write_file(&meta_path, &meta_bytes)
}

fn refresh_cached_meta_sync(cache_dir: &Path, meta: &EtagCacheMeta) -> Result<(), String> {
    let key = cache_key_for_url(&meta.url);
    let (meta_path, _) = cache_paths(cache_dir, &key);
    let meta_bytes = serde_json::to_vec(meta)
        .map_err(|e| format!("Failed serializing refreshed ETag metadata: {}", e))?;
    atomic_write_file(&meta_path, &meta_bytes)
}

fn build_pooled_client(
    use_doh: bool,
    target_port: u16,
    follow_redirects: bool,
    timeout: Option<Duration>,
    doh_snapshot: &[(String, Vec<SocketAddr>)],
) -> Client {
    let mut builder = duke_core::net_doh::build_privacy_client_builder()
        .pool_max_idle_per_host(16)
        .pool_idle_timeout(Duration::from_secs(45))
        .tcp_keepalive(Duration::from_secs(15))
        .http2_adaptive_window(true);

    if let Some(t) = timeout {
        let clamped = clamp_nonzero_duration(t);
        builder = builder
            .connect_timeout(clamped.min(Duration::from_secs(10)))
            .read_timeout(clamped)
            .timeout(clamped);
    } else {
        builder = builder
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(30));
    }

    if !follow_redirects {
        builder = builder.redirect(reqwest::redirect::Policy::none());
    }

    if use_doh {
        for (host, addrs) in doh_snapshot {
            let port_addrs: Vec<SocketAddr> = addrs
                .iter()
                .filter(|sa| sa.port() == target_port)
                .copied()
                .collect();
            if !port_addrs.is_empty() {
                builder = builder.resolve_to_addrs(host, &port_addrs);
            } else if !addrs.is_empty() {
                builder = builder.resolve_to_addrs(host, addrs);
            }
        }
    }

    builder
        .build()
        .unwrap_or_else(|_| duke_core::net_doh::get_configured_http_client())
}

fn header_value_string(headers: &reqwest::header::HeaderMap, name: HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

/// Returns a pooled HTTP/2 + DoH/proxy-aware `reqwest::Client` keyed by privacy epoch and timeouts.
pub(crate) async fn get_pooled_client_for_url(
    raw_url: &str,
    timeout: Option<std::time::Duration>,
    follow_redirects: bool,
) -> reqwest::Client {
    let proxy_url = duke_core::net_doh::get_custom_proxy_url();
    let doh_enabled = duke_core::net_doh::is_doh_enabled();
    let use_doh = doh_enabled && proxy_url.is_none();

    let mut target_port = 443u16;
    if use_doh {
        if let Ok(parsed) = Url::parse(raw_url.trim()) {
            target_port = parsed.port_or_known_default().unwrap_or(443);
            if let Some(host) = parsed.host_str() {
                let clean_host = host.trim().trim_end_matches('.').to_ascii_lowercase();
                if !clean_host.is_empty()
                    && !is_loopback_or_local_host(&clean_host)
                    && clean_host.parse::<IpAddr>().is_err()
                {
                    let _ = duke_core::net_doh::resolve_domain_doh(&clean_host, target_port).await;
                }
            }
        }
    }

    let timeout_ms = timeout.map(duration_to_ms_key);
    let effective_port = if use_doh { target_port } else { 0 };
    let initial_epoch = if use_doh {
        duke_core::net_doh::doh_cache_epoch()
    } else {
        0
    };
    let lookup_key = ClientPoolKey {
        use_doh,
        doh_epoch: initial_epoch,
        proxy_url: proxy_url.clone(),
        target_port: effective_port,
        follow_redirects,
        timeout_ms,
    };

    if let Ok(mut pool) = state().pool.lock() {
        pool.sync_epoch(doh_enabled, initial_epoch, &proxy_url);
        if let Some(entry) = pool.entries.get_mut(&lookup_key) {
            entry.last_used_tick = state().pool_access_tick.fetch_add(1, Ordering::Relaxed);
            return entry.client.clone();
        }
    }

    let (doh_snapshot, build_epoch): (Vec<(String, Vec<SocketAddr>)>, u64) = if use_doh {
        match duke_core::net_doh::get_doh_cache().lock() {
            Ok(cache) => (
                cache.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                duke_core::net_doh::doh_cache_epoch(),
            ),
            Err(_) => (Vec::new(), initial_epoch),
        }
    } else {
        (Vec::new(), 0)
    };

    let build_key = ClientPoolKey {
        doh_epoch: build_epoch,
        ..lookup_key
    };
    let built = build_pooled_client(
        use_doh,
        target_port,
        follow_redirects,
        timeout,
        &doh_snapshot,
    );

    if let Ok(mut pool) = state().pool.lock() {
        pool.sync_epoch(doh_enabled, build_epoch, &proxy_url);
        if let Some(existing) = pool.entries.get_mut(&build_key) {
            existing.last_used_tick = state().pool_access_tick.fetch_add(1, Ordering::Relaxed);
            return existing.client.clone();
        }
        let tick = state().pool_access_tick.fetch_add(1, Ordering::Relaxed);
        pool.insert_bounded(doh_enabled, build_key, built.clone(), tick);
    }

    built
}

/// Fetches raw bytes over the shared HTTP/2 + DoH/proxy pool with per-domain throttling and `ETag` caching.
pub(crate) async fn fetch_bytes_cached(
    cache_dir: Option<&Path>,
    raw_url: &str,
    extra_headers: &[(&str, &str)],
    timeout: std::time::Duration,
    follow_redirects: bool,
) -> Result<CachedHttpResponse, String> {
    let trimmed_url = raw_url.trim();
    let parsed_url =
        Url::parse(trimmed_url).map_err(|e| format!("Invalid URL '{}': {}", trimmed_url, e))?;
    if !matches!(parsed_url.scheme(), "http" | "https") {
        return Err(format!("Unsupported URL scheme '{}'", parsed_url.scheme()));
    }

    acquire_domain_rate_slot(&parsed_url).await;

    let effective_timeout = clamp_nonzero_duration(timeout);
    let client =
        get_pooled_client_for_url(trimmed_url, Some(effective_timeout), follow_redirects).await;

    let cached_entry: Option<LoadedCacheEntry> = if let Some(dir) = cache_dir {
        let dir_owned = dir.to_path_buf();
        let url_owned = trimmed_url.to_string();
        tokio::task::spawn_blocking(move || load_cached_entry_sync(&dir_owned, &url_owned))
            .await
            .ok()
            .flatten()
    } else {
        None
    };

    let mut req = client.get(trimmed_url).timeout(effective_timeout);
    let mut has_if_none_match = false;
    let mut has_if_modified_since = false;
    for &(name_str, val_str) in extra_headers {
        let name = HeaderName::from_bytes(name_str.as_bytes())
            .map_err(|e| format!("Invalid header name '{}': {}", name_str, e))?;
        let value = HeaderValue::from_str(val_str)
            .map_err(|e| format!("Invalid header value for '{}': {}", name_str, e))?;
        if name == IF_NONE_MATCH {
            has_if_none_match = true;
        } else if name == IF_MODIFIED_SINCE {
            has_if_modified_since = true;
        }
        req = req.header(name, value);
    }

    if let Some(ref cached) = cached_entry {
        if !has_if_none_match {
            if let Some(ref etag) = cached.meta.etag {
                if let Ok(val) = HeaderValue::from_str(etag) {
                    req = req.header(IF_NONE_MATCH, val);
                }
            }
        }
        if !has_if_modified_since {
            if let Some(ref lm) = cached.meta.last_modified {
                if let Ok(val) = HeaderValue::from_str(lm) {
                    req = req.header(IF_MODIFIED_SINCE, val);
                }
            }
        }
    }

    state().total_requests.fetch_add(1, Ordering::Relaxed);
    let resp = match req.send().await {
        Ok(r) => r,
        Err(err) => {
            if duke_core::net_doh::is_doh_enabled()
                && duke_core::net_doh::get_custom_proxy_url().is_none()
            {
                duke_core::net_doh::rotate_doh_cached_ips(trimmed_url);
            }
            return Err(format!("HTTP request failed for {}: {}", trimmed_url, err));
        }
    };

    let status = resp.status();
    let resp_etag = header_value_string(resp.headers(), ETAG);
    let resp_last_modified = header_value_string(resp.headers(), LAST_MODIFIED);
    let resp_content_type = header_value_string(resp.headers(), CONTENT_TYPE);

    if status == reqwest::StatusCode::NOT_MODIFIED {
        let Some(cached) = cached_entry else {
            return Err(format!(
                "HTTP 304 Not Modified without cached body for {}",
                trimmed_url
            ));
        };
        state().cache_hits_304.fetch_add(1, Ordering::Relaxed);
        let merged_etag = resp_etag.or(cached.meta.etag.clone());
        let merged_lm = resp_last_modified.or(cached.meta.last_modified.clone());
        let merged_ct = resp_content_type.or(cached.meta.content_type.clone());

        if let Some(dir) = cache_dir {
            let dir_owned = dir.to_path_buf();
            let refreshed_meta = EtagCacheMeta {
                url: cached.meta.url,
                etag: merged_etag.clone(),
                last_modified: merged_lm,
                content_type: merged_ct,
                fetched_at_unix: unix_now_secs(),
                body_len: cached.meta.body_len,
                body_blake3: cached.meta.body_blake3,
            };
            let _ = tokio::task::spawn_blocking(move || {
                let _ = refresh_cached_meta_sync(&dir_owned, &refreshed_meta);
            })
            .await;
        }

        return Ok(CachedHttpResponse {
            status: status.as_u16(),
            bytes: cached.body,
            etag: merged_etag,
            from_cache_304: true,
        });
    }

    if !status.is_success() {
        return Err(format!("HTTP {} for {}", status.as_u16(), trimmed_url));
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("Failed reading HTTP body for {}: {}", trimmed_url, e))?
        .to_vec();

    if let Some(dir) = cache_dir {
        if resp_etag.is_some() || resp_last_modified.is_some() {
            let dir_owned = dir.to_path_buf();
            let meta = EtagCacheMeta {
                url: trimmed_url.to_string(),
                etag: resp_etag.clone(),
                last_modified: resp_last_modified,
                content_type: resp_content_type,
                fetched_at_unix: unix_now_secs(),
                body_len: bytes.len() as u64,
                body_blake3: blake3::hash(&bytes).to_hex().to_string(),
            };
            let body_clone = bytes.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let _ = store_cached_entry_sync(&dir_owned, &meta, &body_clone);
            })
            .await;
        }
    }

    Ok(CachedHttpResponse {
        status: status.as_u16(),
        bytes,
        etag: resp_etag,
        from_cache_304: false,
    })
}

/// Fetches UTF-8 text via `fetch_bytes_cached`, following redirects.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) async fn fetch_text_cached(
    cache_dir: Option<&Path>,
    raw_url: &str,
    extra_headers: &[(&str, &str)],
    timeout: std::time::Duration,
) -> Result<String, String> {
    let resp = fetch_bytes_cached(cache_dir, raw_url, extra_headers, timeout, true).await?;
    Ok(String::from_utf8(resp.bytes)
        .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()))
}

/// Fetches and deserializes a JSON payload via `fetch_bytes_cached`, following redirects.
pub(crate) async fn fetch_json_cached<T: DeserializeOwned>(
    cache_dir: Option<&Path>,
    raw_url: &str,
    extra_headers: &[(&str, &str)],
    timeout: std::time::Duration,
) -> Result<T, String> {
    let resp = fetch_bytes_cached(cache_dir, raw_url, extra_headers, timeout, true).await?;
    serde_json::from_slice::<T>(&resp.bytes)
        .map_err(|e| format!("Failed parsing JSON from {}: {}", raw_url.trim(), e))
}

/// Resolves the standalone on-disk `net_etag_cache` directory under the launcher data root.
pub(crate) fn etag_cache_dir_standalone() -> Option<PathBuf> {
    crate::resolve_data_root_dir_standalone()
        .ok()
        .map(|d| d.join("net_etag_cache"))
}

/// Returns current connection pool, ETag 304 hit, rate-limiter, and privacy routing telemetry.
pub(crate) fn get_net_engine_stats() -> NetEngineStats {
    let s = state();
    let doh_enabled = duke_core::net_doh::is_doh_enabled();
    let proxy_url = duke_core::net_doh::get_custom_proxy_url();
    let use_doh = doh_enabled && proxy_url.is_none();
    let effective_epoch = if use_doh {
        duke_core::net_doh::doh_cache_epoch()
    } else {
        0
    };
    let cached_clients = s
        .pool
        .lock()
        .map(|p| {
            if p.active_doh_enabled == doh_enabled
                && p.active_doh_epoch == effective_epoch
                && p.active_proxy_url == proxy_url
            {
                p.entries.len()
            } else {
                0
            }
        })
        .unwrap_or(0);

    NetEngineStats {
        total_requests: s.total_requests.load(Ordering::Relaxed),
        cache_hits_304: s.cache_hits_304.load(Ordering::Relaxed),
        rate_limited_waits: s.rate_limited_waits.load(Ordering::Relaxed),
        cached_clients,
        doh_enabled,
        proxy_active: proxy_url.is_some(),
    }
}

/// Configures a custom minimum inter-request cooldown for a normalized domain or `host:port`.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn configure_domain_min_interval(domain: &str, min_interval: std::time::Duration) {
    let normalized = normalize_domain_key(domain);
    if normalized.is_empty() {
        return;
    }
    let mut limiter = state()
        .rate_limiter
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    limiter.overrides.insert(normalized.clone(), min_interval);
    if let Some(existing) = limiter.next_allowed.get_mut(&normalized) {
        let cap = Instant::now()
            .checked_add(min_interval)
            .unwrap_or_else(Instant::now);
        if *existing > cap {
            *existing = cap;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex as StdMutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    struct TempTestDir(PathBuf);

    impl TempTestDir {
        fn new(tag: &str) -> Result<Self, String> {
            let seq = TMP_WRITE_SEQ.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "duke_net_engine_test_{}_{}_{}",
                tag,
                std::process::id(),
                seq
            ));
            fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            Ok(Self(dir))
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempTestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn parse_http_headers_and_validators(raw_req: &str) -> (Option<String>, Option<String>) {
        let mut inm = None;
        let mut ims = None;
        for line in raw_req.split("\r\n").skip(1) {
            if let Some((name, val)) = line.split_once(':') {
                let lower = name.trim();
                if lower.eq_ignore_ascii_case("if-none-match") {
                    inm = Some(val.trim().to_string());
                } else if lower.eq_ignore_ascii_case("if-modified-since") {
                    ims = Some(val.trim().to_string());
                }
            }
        }
        (inm, ims)
    }

    #[tokio::test]
    async fn etag_cache_roundtrip_returns_304_from_disk() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind local listener");
        let addr = listener.local_addr().expect("listener addr");
        type SeenValidators = Arc<StdMutex<Vec<(Option<String>, Option<String>)>>>;
        let seen_validators: SeenValidators = Arc::new(StdMutex::new(Vec::new()));
        let seen_clone = Arc::clone(&seen_validators);

        let server_task = tokio::spawn(async move {
            let body = br#"{"title":"Duke Test","ok":true}"#;
            for _ in 0..4 {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let seen_conn = Arc::clone(&seen_clone);
                tokio::spawn(async move {
                    let mut buf = Vec::with_capacity(2048);
                    let mut chunk = [0u8; 1024];
                    loop {
                        buf.clear();
                        let header_end = loop {
                            let Ok(n) = stream.read(&mut chunk).await else {
                                return;
                            };
                            if n == 0 {
                                return;
                            }
                            buf.extend_from_slice(&chunk[..n]);
                            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                                break pos + 4;
                            }
                        };

                        let req_text = String::from_utf8_lossy(&buf[..header_end]);
                        let (inm, ims) = parse_http_headers_and_validators(&req_text);
                        let is_304 = inm.as_deref() == Some("\"v1\"");
                        if let Ok(mut guard) = seen_conn.lock() {
                            guard.push((inm, ims));
                        }

                        let resp = if is_304 {
                            "HTTP/1.1 304 Not Modified\r\nETag: \"v1\"\r\nContent-Length: 0\r\n\r\n"
                                .to_string()
                        } else {
                            format!(
                                "HTTP/1.1 200 OK\r\nETag: \"v1\"\r\nLast-Modified: Wed, 21 Oct 2015 07:28:00 GMT\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
                                body.len()
                            )
                        };
                        if stream.write_all(resp.as_bytes()).await.is_err() {
                            return;
                        }
                        if !is_304 && stream.write_all(body).await.is_err() {
                            return;
                        }
                        if stream.flush().await.is_err() {
                            return;
                        }
                    }
                });
            }
        });

        let cache_dir = TempTestDir::new("etag").expect("temp cache dir");
        let url = format!("http://127.0.0.1:{}/metadata.json", addr.port());
        let stats_before = get_net_engine_stats();

        let first = fetch_bytes_cached(
            Some(cache_dir.path()),
            &url,
            &[("X-Duke-Probe", "1")],
            Duration::from_secs(5),
            true,
        )
        .await
        .expect("first 200 fetch");
        assert_eq!(first.status, 200);
        assert!(!first.from_cache_304);
        assert_eq!(first.etag.as_deref(), Some("\"v1\""));
        assert_eq!(first.bytes, br#"{"title":"Duke Test","ok":true}"#);

        let key = cache_key_for_url(&url);
        let (meta_path, bin_path) = cache_paths(cache_dir.path(), &key);
        assert!(meta_path.exists());
        assert!(bin_path.exists());

        let second = fetch_bytes_cached(
            Some(cache_dir.path()),
            &url,
            &[],
            Duration::from_secs(5),
            true,
        )
        .await
        .expect("second 304 cached fetch");
        assert_eq!(second.status, 304);
        assert!(second.from_cache_304);
        assert_eq!(second.etag.as_deref(), Some("\"v1\""));
        assert_eq!(second.bytes, first.bytes);

        let json_val: serde_json::Value =
            fetch_json_cached(Some(cache_dir.path()), &url, &[], Duration::from_secs(5))
                .await
                .expect("json from 304 cache");
        assert_eq!(json_val["title"], "Duke Test");
        assert_eq!(json_val["ok"], true);

        let text_val =
            fetch_text_cached(Some(cache_dir.path()), &url, &[], Duration::from_secs(5))
                .await
                .expect("text from 304 cache");
        assert_eq!(text_val, r#"{"title":"Duke Test","ok":true}"#);

        let stats_after = get_net_engine_stats();
        assert!(stats_after.cache_hits_304 >= stats_before.cache_hits_304 + 3);
        assert!(stats_after.total_requests >= stats_before.total_requests + 4);

        let recorded = seen_validators.lock().expect("seen lock").clone();
        assert_eq!(recorded.len(), 4);
        assert_eq!(recorded[0], (None, None));
        assert_eq!(recorded[1].0.as_deref(), Some("\"v1\""));
        assert_eq!(
            recorded[1].1.as_deref(),
            Some("Wed, 21 Oct 2015 07:28:00 GMT")
        );
        assert_eq!(recorded[2].0.as_deref(), Some("\"v1\""));
        assert_eq!(recorded[3].0.as_deref(), Some("\"v1\""));

        server_task.abort();
    }

    #[tokio::test]
    async fn domain_rate_limiter_spaces_concurrent_requests() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind rate limit listener");
        let addr = listener.local_addr().expect("listener addr");
        let arrivals: Arc<StdMutex<Vec<Instant>>> = Arc::new(StdMutex::new(Vec::new()));
        let arrivals_srv = Arc::clone(&arrivals);

        let server_task = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let arrivals_conn = Arc::clone(&arrivals_srv);
                tokio::spawn(async move {
                    let mut buf = Vec::with_capacity(1024);
                    let mut chunk = [0u8; 512];
                    loop {
                        buf.clear();
                        loop {
                            let Ok(n) = stream.read(&mut chunk).await else {
                                return;
                            };
                            if n == 0 {
                                return;
                            }
                            buf.extend_from_slice(&chunk[..n]);
                            if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                                break;
                            }
                        }
                        if let Ok(mut guard) = arrivals_conn.lock() {
                            guard.push(Instant::now());
                        }
                        let resp = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok";
                        if stream.write_all(resp.as_bytes()).await.is_err() {
                            return;
                        }
                        let _ = stream.flush().await;
                    }
                });
            }
        });

        let host_port = format!("127.0.0.1:{}", addr.port());
        configure_domain_min_interval(&host_port, Duration::from_millis(75));
        let stats_before = get_net_engine_stats();
        let url = format!("http://{}/ping", host_port);

        let (r1, r2, r3, r4) = tokio::join!(
            fetch_bytes_cached(None, &url, &[], Duration::from_secs(5), true),
            fetch_bytes_cached(None, &url, &[], Duration::from_secs(5), true),
            fetch_bytes_cached(None, &url, &[], Duration::from_secs(5), true),
            fetch_bytes_cached(None, &url, &[], Duration::from_secs(5), true),
        );
        assert_eq!(r1.expect("r1").bytes, b"ok");
        assert_eq!(r2.expect("r2").bytes, b"ok");
        assert_eq!(r3.expect("r3").bytes, b"ok");
        assert_eq!(r4.expect("r4").bytes, b"ok");

        let stats_after = get_net_engine_stats();
        assert!(stats_after.rate_limited_waits >= stats_before.rate_limited_waits + 3);

        let mut times = arrivals.lock().expect("arrivals lock").clone();
        times.sort();
        assert_eq!(times.len(), 4);
        let total_span = times[3].saturating_duration_since(times[0]);
        assert!(
            total_span >= Duration::from_millis(185),
            "expected >= 185ms span across 4 spaced requests, got {:?}",
            total_span
        );

        server_task.abort();
    }

    #[tokio::test]
    async fn corrupt_etag_cache_recovers_with_fresh_200() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("addr");
        let server_task = tokio::spawn(async move {
            let body = b"fresh-payload";
            for _ in 0..2 {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    let _ = stream.read(&mut buf).await;
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nETag: \"v2\"\r\nContent-Length: {}\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(resp.as_bytes()).await;
                    let _ = stream.write_all(body).await;
                    let _ = stream.flush().await;
                });
            }
        });

        let cache_dir = TempTestDir::new("corrupt").expect("temp dir");
        let url = format!("http://127.0.0.1:{}/blob", addr.port());
        let first = fetch_bytes_cached(
            Some(cache_dir.path()),
            &url,
            &[],
            Duration::from_secs(5),
            true,
        )
        .await
        .expect("initial fetch");
        assert!(!first.from_cache_304);

        let key = cache_key_for_url(&url);
        let (_, bin_path) = cache_paths(cache_dir.path(), &key);
        fs::write(&bin_path, b"corrupted-data").expect("corrupt bin");

        let healed = fetch_bytes_cached(
            Some(cache_dir.path()),
            &url,
            &[],
            Duration::from_secs(5),
            true,
        )
        .await
        .expect("healed fetch");
        assert_eq!(healed.status, 200);
        assert!(!healed.from_cache_304);
        assert_eq!(healed.bytes, b"fresh-payload");

        server_task.abort();
    }

    #[test]
    fn client_pool_bounds_and_epoch_invalidation_work() {
        let mut pool = ClientPool::new();
        let dummy = Client::new();
        for idx in 0..32u64 {
            let key = ClientPoolKey {
                use_doh: true,
                doh_epoch: 5,
                proxy_url: None,
                target_port: 443,
                follow_redirects: true,
                timeout_ms: Some(1000 + idx),
            };
            pool.insert_bounded(true, key, dummy.clone(), idx + 1);
            assert!(pool.entries.len() <= MAX_POOLED_CLIENTS);
        }
        assert_eq!(pool.entries.len(), MAX_POOLED_CLIENTS);

        pool.sync_epoch(true, 6, &None);
        assert!(pool.entries.is_empty());

        let proxy_key = ClientPoolKey {
            use_doh: false,
            doh_epoch: 0,
            proxy_url: Some("socks5h://127.0.0.1:9050".to_string()),
            target_port: 0,
            follow_redirects: true,
            timeout_ms: Some(5000),
        };
        pool.insert_bounded(true, proxy_key, dummy, 99);
        assert_eq!(pool.entries.len(), 1);
        pool.sync_epoch(true, 0, &None);
        assert!(pool.entries.is_empty());

        let mut limiter = DomainRateLimiter::new();
        limiter
            .overrides
            .insert("api.steampowered.com".to_string(), Duration::from_millis(120));
        assert_eq!(
            limiter.resolve_interval("api.steampowered.com", Some(443)),
            Duration::from_millis(120)
        );
        assert_eq!(
            limiter.resolve_interval("unknown.example.invalid", Some(443)),
            Duration::from_millis(DEFAULT_DOMAIN_MIN_INTERVAL_MS)
        );
    }

    #[tokio::test]
    async fn pooled_client_reuses_same_key_without_thrashing() {
        let url = "http://127.0.0.1:49999/health";
        let _c1 = get_pooled_client_for_url(url, Some(Duration::from_secs(7)), true).await;
        let count_1 = get_net_engine_stats().cached_clients;
        let _c2 = get_pooled_client_for_url(url, Some(Duration::from_secs(7)), true).await;
        let count_2 = get_net_engine_stats().cached_clients;
        assert!(count_1 >= 1);
        assert_eq!(count_1, count_2);
        assert!(count_2 <= MAX_POOLED_CLIENTS);
    }

    #[test]
    fn etag_cache_dir_standalone_resolves_net_etag_cache_subdir() {
        if let Some(dir) = etag_cache_dir_standalone() {
            assert!(dir.ends_with("net_etag_cache"));
        }
    }
}

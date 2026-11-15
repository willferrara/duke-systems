//! Shared Encrypted DNS-over-HTTPS (DoH) resolver and privacy-hardened `reqwest` client builder.
//!
//! Races queries across Cloudflare Primary (`1.1.1.1`), Cloudflare Secondary (`1.0.0.1`), and
//! Quad9 (`9.9.9.9:5053`) using `Accept: application/dns-json`, caching resolved IPv4 `SocketAddr`
//! pools in a shared `OnceLock` cache.

use reqwest::header::{HeaderMap, HeaderValue, ACCEPT};
use reqwest::{Client, ClientBuilder, Proxy};
use serde_json::Value;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use url::Url;

static HTTP_CLIENT: OnceLock<Client> = OnceLock::new();
static BURST_HTTP_CLIENT: OnceLock<Client> = OnceLock::new();
static DOH_BOOTSTRAP_CLIENT: OnceLock<Client> = OnceLock::new();
static DOH_CACHE: OnceLock<Mutex<HashMap<String, Vec<SocketAddr>>>> = OnceLock::new();
static SESSION_UA_CURSOR: AtomicU64 = AtomicU64::new(0);
static DOH_ENABLED: AtomicBool = AtomicBool::new(true);
static DOH_PROVIDER_MODE: AtomicU8 = AtomicU8::new(0); // 0 = auto race, 1 = cloudflare, 2 = quad9
static CUSTOM_PROXY_URL: OnceLock<RwLock<Option<String>>> = OnceLock::new();
static EFFECTIVE_HTTP_CLIENT: OnceLock<Mutex<(u64, Client)>> = OnceLock::new();
static PROXY_CONFIG_GENERATION: AtomicU64 = AtomicU64::new(1);
static DOH_CACHE_EPOCH: AtomicU64 = AtomicU64::new(1);
static DOH_POOL_ACCESS_TICK: AtomicU64 = AtomicU64::new(0);
const MAX_DOH_CLIENT_POOL_ENTRIES: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct DohClientPoolKey {
    use_doh: bool,
    proxy_gen: u64,
    doh_epoch: u64,
    target_port: u16,
    follow_redirects: bool,
    is_burst: bool,
    connect_ms: u64,
    read_ms: u64,
}

#[derive(Clone)]
struct PooledClientEntry {
    client: Client,
    last_used_tick: u64,
}

static DOH_CLIENT_POOL: OnceLock<Mutex<HashMap<DohClientPoolKey, PooledClientEntry>>> =
    OnceLock::new();

struct Socks5BridgeHandle {
    upstream_url: String,
    local_http_proxy_url: String,
    stop_flag: Arc<AtomicBool>,
}

static SOCKS5_BRIDGE: OnceLock<Mutex<Option<Socks5BridgeHandle>>> = OnceLock::new();

pub const PRIVACY_USER_AGENTS: &[&str] = &[
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:135.0) Gecko/20100101 Firefox/135.0",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:134.0) Gecko/20100101 Firefox/134.0",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/132.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:128.0) Gecko/20100101 Firefox/128.0",
];

pub fn get_doh_cache() -> &'static Mutex<HashMap<String, Vec<SocketAddr>>> {
    DOH_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn doh_cache_epoch() -> u64 {
    DOH_CACHE_EPOCH.load(Ordering::Acquire)
}

fn get_doh_client_pool() -> &'static Mutex<HashMap<DohClientPoolKey, PooledClientEntry>> {
    DOH_CLIENT_POOL.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn cached_doh_host_count() -> usize {
    get_doh_cache().lock().map(|c| c.len()).unwrap_or(0)
}

pub fn rotate_privacy_user_agent() -> &'static str {
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let tick = SESSION_UA_CURSOR.fetch_add(1, Ordering::Relaxed);
    let idx = ((seed.wrapping_add(tick)) as usize) % PRIVACY_USER_AGENTS.len();
    PRIVACY_USER_AGENTS[idx]
}

pub fn get_session_user_agent() -> &'static str {
    let idx = (SESSION_UA_CURSOR.load(Ordering::Relaxed) as usize) % PRIVACY_USER_AGENTS.len();
    PRIVACY_USER_AGENTS[idx]
}

pub fn build_privacy_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("DNT", HeaderValue::from_static("1"));
    headers.insert("Sec-GPC", HeaderValue::from_static("1"));
    headers.remove("Sec-CH-UA");
    headers.remove("Sec-CH-UA-Mobile");
    headers.remove("Sec-CH-UA-Platform");
    headers.remove("Sec-CH-UA-Full-Version-List");
    headers.remove("X-Forwarded-For");
    headers.remove("X-Real-IP");
    headers
}

fn proxy_store() -> &'static RwLock<Option<String>> {
    CUSTOM_PROXY_URL.get_or_init(|| RwLock::new(None))
}

fn socks5_bridge_store() -> &'static Mutex<Option<Socks5BridgeHandle>> {
    SOCKS5_BRIDGE.get_or_init(|| Mutex::new(None))
}

/// Validates and normalizes an optional HTTP/HTTPS/SOCKS5 proxy URL (`""` -> `None`).
pub fn validate_proxy_url(raw: Option<&str>) -> Result<Option<String>, String> {
    let trimmed = raw.map(str::trim).unwrap_or("");
    if trimmed.is_empty() {
        return Ok(None);
    }
    let parsed = Url::parse(trimmed)
        .map_err(|_| "Proxy URL must be a valid URL like http://127.0.0.1:8080 or socks5://127.0.0.1:1080.".to_string())?;
    let scheme = parsed.scheme().to_ascii_lowercase();
    if !matches!(scheme.as_str(), "http" | "https" | "socks5" | "socks5h") {
        return Err("Proxy URL must start with http://, https://, socks5://, or socks5h://.".to_string());
    }
    if parsed.host_str().unwrap_or("").is_empty() {
        return Err("Proxy URL is missing a valid host or IP address.".to_string());
    }
    Ok(Some(trimmed.to_string()))
}

/// Updates the shared encrypted DoH and HTTP/SOCKS5 proxy routing configuration for all `duke-core` clients.
pub fn set_network_privacy_config(doh_enabled: bool, proxy_url: Option<&str>) -> Result<(), String> {
    let normalized = validate_proxy_url(proxy_url)?;
    DOH_ENABLED.store(doh_enabled, Ordering::Relaxed);
    if let Ok(mut guard) = proxy_store().write() {
        *guard = normalized.clone();
    }
    if normalized.is_none()
        && let Ok(mut bridge) = socks5_bridge_store().lock()
            && let Some(prev) = bridge.take() {
                prev.stop_flag.store(true, Ordering::Relaxed);
            }
    PROXY_CONFIG_GENERATION.fetch_add(1, Ordering::AcqRel);
    Ok(())
}

pub fn is_doh_enabled() -> bool {
    DOH_ENABLED.load(Ordering::Relaxed)
}

/// Sets preferred DoH upstream provider (`"auto"`, `"cloudflare"`, `"quad9"`).
pub fn set_doh_provider_mode(provider: &str) {
    let mode = match provider.trim().to_ascii_lowercase().as_str() {
        "cloudflare" | "1.1.1.1" => 1,
        "quad9" | "9.9.9.9" => 2,
        _ => 0,
    };
    let prev = DOH_PROVIDER_MODE.swap(mode, Ordering::Relaxed);
    if prev != mode
        && let Ok(mut cache) = get_doh_cache().lock()
        && !cache.is_empty()
    {
        cache.clear();
        DOH_CACHE_EPOCH.fetch_add(1, Ordering::AcqRel);
    }
}

pub fn get_custom_proxy_url() -> Option<String> {
    proxy_store().read().ok().and_then(|g| g.clone())
}

pub fn build_privacy_client_builder() -> ClientBuilder {
    let mut builder = Client::builder()
        .user_agent(get_session_user_agent())
        .default_headers(build_privacy_headers())
        .tcp_nodelay(true);
    if let Some(effective_proxy) = resolve_effective_http_proxy_sync()
        && let Ok(proxy) = Proxy::all(&effective_proxy) {
            builder = builder.proxy(proxy);
        }
    builder
}

fn resolve_effective_http_proxy_sync() -> Option<String> {
    let configured = get_custom_proxy_url()?;
    let lower = configured.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Some(configured);
    }
    if lower.starts_with("socks5://") || lower.starts_with("socks5h://") {
        if let Ok(guard) = socks5_bridge_store().lock()
            && let Some(existing) = guard.as_ref()
                && existing.upstream_url == configured {
                    return Some(existing.local_http_proxy_url.clone());
                }
        // Attempt lazy bridge startup if running inside a Tokio runtime
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            return start_socks5_http_bridge_blocking(&configured, &handle);
        }
    }
    None
}

async fn resolve_effective_http_proxy_async() -> Option<String> {
    let configured = get_custom_proxy_url()?;
    let lower = configured.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Some(configured);
    }
    if lower.starts_with("socks5://") || lower.starts_with("socks5h://") {
        if let Ok(guard) = socks5_bridge_store().lock()
            && let Some(existing) = guard.as_ref()
                && existing.upstream_url == configured {
                    return Some(existing.local_http_proxy_url.clone());
                }
        return start_socks5_http_bridge_async(&configured).await;
    }
    None
}

fn start_socks5_http_bridge_blocking(
    upstream_url: &str,
    handle: &tokio::runtime::Handle,
) -> Option<String> {
    let std_listener = std::net::TcpListener::bind("127.0.0.1:0").ok()?;
    std_listener.set_nonblocking(true).ok()?;
    let local_port = std_listener.local_addr().ok()?.port();
    let local_proxy = format!("http://127.0.0.1:{}", local_port);
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_worker = Arc::clone(&stop_flag);
    let upstream_owned = upstream_url.to_string();

    {
        let mut guard = socks5_bridge_store().lock().ok()?;
        if let Some(existing) = guard.as_ref()
            && existing.upstream_url == upstream_url
        {
            return Some(existing.local_http_proxy_url.clone());
        }
        if let Some(old) = guard.take() {
            old.stop_flag.store(true, Ordering::Relaxed);
        }
        *guard = Some(Socks5BridgeHandle {
            upstream_url: upstream_owned.clone(),
            local_http_proxy_url: local_proxy.clone(),
            stop_flag,
        });
    }

    handle.spawn(async move {
        if let Ok(listener) = TcpListener::from_std(std_listener) {
            run_socks5_http_bridge_loop(listener, upstream_owned, stop_worker).await;
        }
    });
    Some(local_proxy)
}

async fn start_socks5_http_bridge_async(upstream_url: &str) -> Option<String> {
    let listener = TcpListener::bind("127.0.0.1:0").await.ok()?;
    let local_port = listener.local_addr().ok()?.port();
    let local_proxy = format!("http://127.0.0.1:{}", local_port);
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_worker = Arc::clone(&stop_flag);
    let upstream_owned = upstream_url.to_string();

    {
        let mut guard = socks5_bridge_store().lock().ok()?;
        if let Some(existing) = guard.as_ref()
            && existing.upstream_url == upstream_url
        {
            return Some(existing.local_http_proxy_url.clone());
        }
        if let Some(old) = guard.take() {
            old.stop_flag.store(true, Ordering::Relaxed);
        }
        *guard = Some(Socks5BridgeHandle {
            upstream_url: upstream_owned.clone(),
            local_http_proxy_url: local_proxy.clone(),
            stop_flag,
        });
    }

    tokio::spawn(async move {
        run_socks5_http_bridge_loop(listener, upstream_owned, stop_worker).await;
    });
    Some(local_proxy)
}

async fn run_socks5_http_bridge_loop(
    listener: TcpListener,
    upstream_url: String,
    stop_flag: Arc<AtomicBool>,
) {
    loop {
        if stop_flag.load(Ordering::Relaxed) {
            break;
        }
        let accepted = tokio::select! {
            res = listener.accept() => res,
            _ = tokio::time::sleep(Duration::from_millis(250)) => continue,
        };
        let Ok((client_stream, _)) = accepted else {
            break;
        };
        let proxy_url = upstream_url.clone();
        tokio::spawn(async move {
            let _ = handle_http_to_socks5_conn(client_stream, &proxy_url).await;
        });
    }
}

async fn handle_http_to_socks5_conn(mut client: TcpStream, upstream_url: &str) -> Result<(), String> {
    let parsed_proxy = Url::parse(upstream_url).map_err(|e| e.to_string())?;
    let proxy_host = parsed_proxy
        .host_str()
        .ok_or_else(|| "SOCKS5 proxy host missing".to_string())?;
    let proxy_port = parsed_proxy.port_or_known_default().unwrap_or(1080);

    let mut head_buf = Vec::with_capacity(2048);
    let mut chunk = [0u8; 2048];
    let header_end = loop {
        if head_buf.len() >= 64 * 1024 {
            return Err("Proxy request header too large".to_string());
        }
        let n = client.read(&mut chunk).await.map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("EOF before HTTP proxy header".to_string());
        }
        head_buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = head_buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
    };

    let header_text = String::from_utf8_lossy(&head_buf[..header_end]);
    let first_line = header_text.lines().next().unwrap_or("");
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("");
    let version = parts.next().unwrap_or("HTTP/1.1");

    let (target_host, target_port, is_connect, rewritten_head) = if method.eq_ignore_ascii_case("CONNECT") {
        let (h, p) = split_host_port(target, 443)?;
        (h, p, true, None)
    } else if let Ok(abs_url) = Url::parse(target) {
        let h = abs_url
            .host_str()
            .ok_or_else(|| "Plain HTTP proxy target missing host".to_string())?
            .to_string();
        let p = abs_url.port_or_known_default().unwrap_or(80);
        let mut path_and_query = abs_url.path().to_string();
        if path_and_query.is_empty() {
            path_and_query.push('/');
        }
        if let Some(q) = abs_url.query() {
            path_and_query.push('?');
            path_and_query.push_str(q);
        }
        let rest_headers = header_text
            .split_once("\r\n")
            .map(|(_, rest)| rest)
            .unwrap_or("\r\n");
        let rewritten = format!("{} {} {}\r\n{}", method, path_and_query, version, rest_headers);
        (h, p, false, Some(rewritten))
    } else {
        return Err("Unsupported proxy request target".to_string());
    };

    let mut upstream = TcpStream::connect((proxy_host, proxy_port))
        .await
        .map_err(|e| e.to_string())?;
    let username = parsed_proxy.username();
    let password = parsed_proxy.password().unwrap_or("");
    socks5_connect_handshake(
        &mut upstream,
        (!username.is_empty()).then_some((username, password)),
        &target_host,
        target_port,
    )
    .await?;

    if is_connect {
        client
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await
            .map_err(|e| e.to_string())?;
        if head_buf.len() > header_end {
            upstream
                .write_all(&head_buf[header_end..])
                .await
                .map_err(|e| e.to_string())?;
        }
    } else if let Some(rewritten) = rewritten_head {
        upstream
            .write_all(rewritten.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        if head_buf.len() > header_end {
            upstream
                .write_all(&head_buf[header_end..])
                .await
                .map_err(|e| e.to_string())?;
        }
    }

    let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
    Ok(())
}

fn split_host_port(authority: &str, default_port: u16) -> Result<(String, u16), String> {
    let trimmed = authority.trim();
    if trimmed.starts_with('[')
        && let Some(end_bracket) = trimmed.find(']')
    {
        let host = trimmed[1..end_bracket].to_string();
        let port = trimmed[end_bracket + 1..]
            .strip_prefix(':')
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(default_port);
        return Ok((host, port));
    }
    if let Some((h, p_str)) = trimmed.rsplit_once(':')
        && let Ok(port) = p_str.parse::<u16>()
    {
        return Ok((h.to_string(), port));
    }
    if trimmed.is_empty() {
        return Err("Empty proxy host".to_string());
    }
    Ok((trimmed.to_string(), default_port))
}

async fn socks5_connect_handshake(
    upstream: &mut TcpStream,
    creds: Option<(&str, &str)>,
    target_host: &str,
    target_port: u16,
) -> Result<(), String> {
    if let Some((user, pass)) = creds {
        upstream
            .write_all(&[0x05, 0x02, 0x00, 0x02])
            .await
            .map_err(|e| e.to_string())?;
        let mut choice = [0u8; 2];
        upstream
            .read_exact(&mut choice)
            .await
            .map_err(|e| e.to_string())?;
        if choice[0] != 0x05 || !matches!(choice[1], 0x00 | 0x02) {
            return Err("SOCKS5 proxy rejected authentication methods".to_string());
        }
        if choice[1] == 0x02 {
            let ub = user.as_bytes();
            let pb = pass.as_bytes();
            if ub.len() > 255 || pb.len() > 255 {
                return Err("SOCKS5 credentials exceed 255 bytes".to_string());
            }
            let mut auth_msg = Vec::with_capacity(3 + ub.len() + pb.len());
            auth_msg.push(0x01);
            auth_msg.push(ub.len() as u8);
            auth_msg.extend_from_slice(ub);
            auth_msg.push(pb.len() as u8);
            auth_msg.extend_from_slice(pb);
            upstream.write_all(&auth_msg).await.map_err(|e| e.to_string())?;
            let mut auth_rep = [0u8; 2];
            upstream
                .read_exact(&mut auth_rep)
                .await
                .map_err(|e| e.to_string())?;
            if auth_rep[1] != 0x00 {
                return Err("SOCKS5 username/password authentication failed".to_string());
            }
        }
    } else {
        upstream
            .write_all(&[0x05, 0x01, 0x00])
            .await
            .map_err(|e| e.to_string())?;
        let mut choice = [0u8; 2];
        upstream
            .read_exact(&mut choice)
            .await
            .map_err(|e| e.to_string())?;
        if choice != [0x05, 0x00] {
            return Err("SOCKS5 proxy requires authentication or rejected greeting".to_string());
        }
    }

    let host_bytes = target_host.as_bytes();
    if host_bytes.len() > 255 {
        return Err("Target host name exceeds SOCKS5 limit".to_string());
    }
    let mut req = Vec::with_capacity(7 + host_bytes.len());
    req.extend_from_slice(&[0x05, 0x01, 0x00, 0x03, host_bytes.len() as u8]);
    req.extend_from_slice(host_bytes);
    req.extend_from_slice(&target_port.to_be_bytes());
    upstream.write_all(&req).await.map_err(|e| e.to_string())?;

    let mut hdr = [0u8; 4];
    upstream.read_exact(&mut hdr).await.map_err(|e| e.to_string())?;
    if hdr[0] != 0x05 || hdr[1] != 0x00 {
        return Err(format!("SOCKS5 connect failed with code {}", hdr[1]));
    }
    match hdr[3] {
        0x01 => {
            let mut skip = [0u8; 6];
            upstream.read_exact(&mut skip).await.map_err(|e| e.to_string())?;
        }
        0x03 => {
            let mut len = [0u8; 1];
            upstream.read_exact(&mut len).await.map_err(|e| e.to_string())?;
            let mut skip = vec![0u8; (len[0] as usize) + 2];
            upstream.read_exact(&mut skip).await.map_err(|e| e.to_string())?;
        }
        0x04 => {
            let mut skip = [0u8; 18];
            upstream.read_exact(&mut skip).await.map_err(|e| e.to_string())?;
        }
        _ => return Err("Unsupported SOCKS5 address type".to_string()),
    }
    Ok(())
}

/// Returns the legacy static shared HTTP client (kept `'static` for backwards API compatibility).
pub fn get_http_client() -> &'static Client {
    HTTP_CLIENT.get_or_init(|| {
        let ua = rotate_privacy_user_agent();
        build_privacy_client_builder()
            .user_agent(ua)
            .tcp_keepalive(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(8))
            .pool_max_idle_per_host(12)
            .pool_idle_timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(8))
            .build()
            .unwrap_or_else(|_| Client::new())
    })
}

/// Returns a cloned HTTP client configured with the active proxy/privacy settings, rebuilding
/// automatically whenever runtime proxy configuration changes.
pub fn get_configured_http_client() -> Client {
    let generation = PROXY_CONFIG_GENERATION.load(Ordering::Relaxed);
    let cache = EFFECTIVE_HTTP_CLIENT.get_or_init(|| Mutex::new((0, get_http_client().clone())));

    if let Ok(mut guard) = cache.lock() {
        if guard.0 != generation {
            let ua = get_session_user_agent();
            if let Ok(rebuilt) = build_privacy_client_builder()
                .user_agent(ua)
                .tcp_keepalive(Duration::from_secs(15))
                .read_timeout(Duration::from_secs(8))
                .pool_max_idle_per_host(12)
                .pool_idle_timeout(Duration::from_secs(30))
                .connect_timeout(Duration::from_secs(8))
                .build()
            {
                *guard = (generation, rebuilt);
            } else {
                guard.0 = generation;
            }
        }
        return guard.1.clone();
    }

    get_http_client().clone()
}

pub fn get_burst_http_client() -> &'static Client {
    BURST_HTTP_CLIENT.get_or_init(|| {
        build_privacy_client_builder()
            .http1_only()
            .tcp_keepalive(Duration::from_secs(10))
            .pool_max_idle_per_host(0)
            .connect_timeout(Duration::from_secs(6))
            .read_timeout(Duration::from_secs(5))
            .build()
            .unwrap_or_else(|_| Client::new())
    })
}

pub fn get_doh_bootstrap_client() -> &'static Client {
    DOH_BOOTSTRAP_CLIENT.get_or_init(|| {
        build_privacy_client_builder()
            .connect_timeout(Duration::from_secs(3))
            .read_timeout(Duration::from_secs(4))
            .build()
            .unwrap_or_else(|_| Client::new())
    })
}

/// Races encrypted DoH queries across `1.1.1.1`, `1.0.0.1`, and `9.9.9.9:5053`.
pub async fn resolve_domain_doh(domain: &str, port: u16) -> Vec<SocketAddr> {
    let clean_domain = domain.trim().trim_end_matches('.').to_lowercase();
    if clean_domain.is_empty() {
        return Vec::new();
    }

    if let Ok(ip) = clean_domain.parse::<IpAddr>() {
        return vec![SocketAddr::new(ip, port)];
    }

    if let Ok(cache) = get_doh_cache().lock()
        && let Some(cached) = cache.get(&clean_domain)
            && !cached.is_empty() {
                let mut port_matched: Vec<SocketAddr> = cached
                    .iter()
                    .filter(|sa| sa.port() == port)
                    .copied()
                    .collect();
                if port_matched.is_empty() {
                    port_matched = cached
                        .iter()
                        .map(|sa| SocketAddr::new(sa.ip(), port))
                        .collect();
                }
                return port_matched;
            }

    let encoded_name = urlencoding::encode(&clean_domain).into_owned();
    let cf_primary = format!("https://1.1.1.1/dns-query?name={}&type=A", encoded_name);
    let cf_secondary = format!("https://1.0.0.1/dns-query?name={}&type=A", encoded_name);
    let quad9_primary = format!("https://9.9.9.9:5053/dns-query?name={}&type=A", encoded_name);

    let doh_client = get_doh_bootstrap_client();

    let fetch_provider_ips = |endpoint: String| async move {
        let mut addrs: Vec<Ipv4Addr> = Vec::new();
        if let Ok(resp) = doh_client
            .get(&endpoint)
            .header(ACCEPT, "application/dns-json")
            .timeout(Duration::from_millis(2500))
            .send()
            .await
            && resp.status().is_success()
                && let Ok(json_val) = resp.json::<Value>().await
                    && let Some(answers) = json_val.get("Answer").and_then(|a| a.as_array()) {
                        for record in answers {
                            let rec_type = record.get("type").and_then(|t| t.as_u64()).unwrap_or(0);
                            if rec_type == 1
                                && let Some(ip_str) = record.get("data").and_then(|d| d.as_str())
                                    && let Ok(ipv4) = ip_str.trim().parse::<Ipv4Addr>()
                                        && !addrs.contains(&ipv4) {
                                            addrs.push(ipv4);
                                        }
                        }
                    }
        addrs
    };

    let (ips_cf1, ips_cf2, ips_q9) = match DOH_PROVIDER_MODE.load(Ordering::Relaxed) {
        1 => {
            let (a, b) = tokio::join!(
                fetch_provider_ips(cf_primary),
                fetch_provider_ips(cf_secondary)
            );
            (a, b, Vec::new())
        }
        2 => {
            let q = fetch_provider_ips(quad9_primary).await;
            (Vec::new(), Vec::new(), q)
        }
        _ => {
            tokio::join!(
                fetch_provider_ips(cf_primary),
                fetch_provider_ips(cf_secondary),
                fetch_provider_ips(quad9_primary)
            )
        }
    };

    let mut resolved_addrs: Vec<SocketAddr> = Vec::new();
    for ipv4 in ips_cf1.into_iter().chain(ips_cf2).chain(ips_q9) {
        let addr_443 = SocketAddr::new(IpAddr::V4(ipv4), 443);
        let addr_80 = SocketAddr::new(IpAddr::V4(ipv4), 80);
        if !resolved_addrs.contains(&addr_443) {
            resolved_addrs.push(addr_443);
        }
        if !resolved_addrs.contains(&addr_80) {
            resolved_addrs.push(addr_80);
        }
    }

    if !resolved_addrs.is_empty() {
        if let Ok(mut cache) = get_doh_cache().lock() {
            cache.insert(clean_domain, resolved_addrs.clone());
            DOH_CACHE_EPOCH.fetch_add(1, Ordering::AcqRel);
        }
        return resolved_addrs
            .into_iter()
            .filter(|sa| sa.port() == port)
            .collect();
    }

    Vec::new()
}

/// Rotates cached DoH edge IPs for a URL's host so workers can fail over across CDN edge nodes.
pub fn rotate_doh_cached_ips(raw_url: &str) {
    if let Ok(parsed) = Url::parse(raw_url)
        && let Some(host) = parsed.host_str() {
            let clean_host = host.trim().trim_end_matches('.').to_lowercase();
            if let Ok(mut cache) = get_doh_cache().lock()
                && let Some(addrs) = cache.get_mut(&clean_host) {
                    if addrs.len() > 2 {
                        addrs.rotate_left(2);
                        DOH_CACHE_EPOCH.fetch_add(1, Ordering::AcqRel);
                    } else if addrs.len() > 1 {
                        addrs.rotate_left(1);
                        DOH_CACHE_EPOCH.fetch_add(1, Ordering::AcqRel);
                    }
                }
        }
}

fn clamp_timeout_nonzero(d: Duration) -> Duration {
    if d.is_zero() {
        Duration::from_secs(1)
    } else {
        d
    }
}

fn duration_to_pool_ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX).max(1)
}

fn prune_and_insert_pool_entry(
    pool: &mut HashMap<DohClientPoolKey, PooledClientEntry>,
    key: DohClientPoolKey,
    client: Client,
) {
    pool.retain(|k, _| k.proxy_gen == key.proxy_gen && k.doh_epoch == key.doh_epoch);
    while pool.len() >= MAX_DOH_CLIENT_POOL_ENTRIES && !pool.contains_key(&key) {
        let Some(evict_key) = pool
            .iter()
            .min_by_key(|(_, entry)| entry.last_used_tick)
            .map(|(k, _)| *k)
        else {
            break;
        };
        pool.remove(&evict_key);
    }
    let tick = DOH_POOL_ACCESS_TICK.fetch_add(1, Ordering::Relaxed);
    pool.insert(
        key,
        PooledClientEntry {
            client,
            last_used_tick: tick,
        },
    );
}

async fn get_or_build_pooled_doh_client(
    raw_url: &str,
    is_burst: bool,
    connect_timeout: Duration,
    read_timeout: Duration,
    follow_redirects: bool,
) -> Client {
    let connect_timeout = clamp_timeout_nonzero(connect_timeout);
    let read_timeout = clamp_timeout_nonzero(read_timeout);
    let connect_ms = duration_to_pool_ms(connect_timeout);
    let read_ms = duration_to_pool_ms(read_timeout);

    let proxy_gen = PROXY_CONFIG_GENERATION.load(Ordering::Acquire);
    let proxy_url = resolve_effective_http_proxy_async().await;
    let use_doh = is_doh_enabled() && proxy_url.is_none();

    let mut target_port = 443u16;
    if use_doh
        && let Ok(parsed) = Url::parse(raw_url)
    {
        target_port = parsed.port_or_known_default().unwrap_or(443);
        if let Some(host) = parsed.host_str() {
            let _ = resolve_domain_doh(host, target_port).await;
        }
    }

    let doh_epoch = doh_cache_epoch();
    let key = DohClientPoolKey {
        use_doh,
        proxy_gen,
        doh_epoch,
        target_port,
        follow_redirects,
        is_burst,
        connect_ms,
        read_ms,
    };

    if let Ok(mut pool) = get_doh_client_pool().lock()
        && let Some(entry) = pool.get_mut(&key)
    {
        entry.last_used_tick = DOH_POOL_ACCESS_TICK.fetch_add(1, Ordering::Relaxed);
        return entry.client.clone();
    }

    let (cached_snapshot, build_doh_epoch): (Vec<(String, Vec<SocketAddr>)>, u64) = if use_doh {
        match get_doh_cache().lock() {
            Ok(c) => (
                c.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                doh_cache_epoch(),
            ),
            Err(_) => (Vec::new(), doh_epoch),
        }
    } else {
        (Vec::new(), doh_epoch)
    };

    let build_key = DohClientPoolKey {
        doh_epoch: build_doh_epoch,
        ..key
    };

    let mut builder = Client::builder()
        .user_agent(get_session_user_agent())
        .default_headers(build_privacy_headers())
        .tcp_nodelay(true)
        .connect_timeout(connect_timeout)
        .read_timeout(read_timeout);

    if let Some(ref effective_proxy) = proxy_url
        && let Ok(proxy) = Proxy::all(effective_proxy)
    {
        builder = builder.proxy(proxy);
    }

    if !follow_redirects {
        builder = builder.redirect(reqwest::redirect::Policy::none());
    }

    if is_burst {
        builder = builder
            .http1_only()
            .tcp_keepalive(Duration::from_secs(10))
            .pool_max_idle_per_host(0);
    } else {
        builder = builder
            .tcp_keepalive(Duration::from_secs(15))
            .pool_max_idle_per_host(12)
            .pool_idle_timeout(Duration::from_secs(30));
    }

    for (host, addrs) in cached_snapshot {
        let port_addrs: Vec<SocketAddr> = addrs
            .iter()
            .filter(|sa| sa.port() == target_port)
            .copied()
            .collect();
        if !port_addrs.is_empty() {
            builder = builder.resolve_to_addrs(&host, &port_addrs);
        } else if !addrs.is_empty() {
            builder = builder.resolve_to_addrs(&host, &addrs);
        }
    }

    if let Ok(client) = builder.build() {
        if let Ok(mut pool) = get_doh_client_pool().lock() {
            if let Some(existing) = pool.get_mut(&build_key) {
                existing.last_used_tick = DOH_POOL_ACCESS_TICK.fetch_add(1, Ordering::Relaxed);
                return existing.client.clone();
            }
            prune_and_insert_pool_entry(&mut pool, build_key, client.clone());
        }
        return client;
    }

    if is_burst && proxy_url.is_none() {
        get_burst_http_client().clone()
    } else {
        get_configured_http_client()
    }
}

/// Builds a `reqwest::Client` pre-populated with encrypted DoH `SocketAddr` overrides
/// (when DoH is enabled and no custom proxy handles DNS) and `DNT: 1` / `Sec-GPC: 1` privacy headers.
pub async fn build_doh_client_for_url(
    raw_url: &str,
    is_burst: bool,
    follow_redirects: bool,
) -> Client {
    let (connect_timeout, read_timeout) = if is_burst {
        (Duration::from_secs(6), Duration::from_secs(5))
    } else {
        (Duration::from_secs(7), Duration::from_secs(8))
    };
    get_or_build_pooled_doh_client(
        raw_url,
        is_burst,
        connect_timeout,
        read_timeout,
        follow_redirects,
    )
    .await
}

/// Builds a DoH/proxy-backed `reqwest::Client` with explicit connect/read timeouts.
pub async fn build_doh_client_with_timeouts(
    raw_url: &str,
    connect_timeout: Duration,
    read_timeout: Duration,
    follow_redirects: bool,
) -> Client {
    get_or_build_pooled_doh_client(raw_url, false, connect_timeout, read_timeout, follow_redirects)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_http_and_socks5_proxy_urls() {
        assert_eq!(validate_proxy_url(None).unwrap(), None);
        assert_eq!(validate_proxy_url(Some("   ")).unwrap(), None);
        assert_eq!(
            validate_proxy_url(Some("http://127.0.0.1:8080")).unwrap(),
            Some("http://127.0.0.1:8080".to_string())
        );
        assert_eq!(
            validate_proxy_url(Some("socks5://127.0.0.1:1080")).unwrap(),
            Some("socks5://127.0.0.1:1080".to_string())
        );
        assert!(validate_proxy_url(Some("ftp://127.0.0.1:21")).is_err());
        assert!(validate_proxy_url(Some("socks5://")).is_err());
    }

    #[test]
    fn doh_client_pool_epoch_increments_and_keys_are_bounded() {
        assert_eq!(clamp_timeout_nonzero(Duration::ZERO), Duration::from_secs(1));
        assert_eq!(
            clamp_timeout_nonzero(Duration::from_millis(250)),
            Duration::from_millis(250)
        );

        let host = "pool-test.example.invalid";
        let e0 = doh_cache_epoch();
        if let Ok(mut cache) = get_doh_cache().lock() {
            cache.insert(
                host.to_string(),
                vec![
                    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 1)), 443),
                    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 2)), 443),
                ],
            );
        }
        rotate_doh_cached_ips("https://pool-test.example.invalid/archive.bin");
        let e1 = doh_cache_epoch();
        assert!(e1 > e0, "rotating cached DoH IPs must increment DOH_CACHE_EPOCH");
        if let Ok(mut cache) = get_doh_cache().lock() {
            cache.remove(host);
        }

        // Verify stale epoch/proxy_gen pruning and strict 24-entry bound.
        let mut local_pool: HashMap<DohClientPoolKey, PooledClientEntry> = HashMap::new();
        let dummy_client = Client::new();
        let stale_key = DohClientPoolKey {
            use_doh: true,
            proxy_gen: 1,
            doh_epoch: 1,
            target_port: 443,
            follow_redirects: true,
            is_burst: false,
            connect_ms: 5000,
            read_ms: 5000,
        };
        prune_and_insert_pool_entry(&mut local_pool, stale_key, dummy_client.clone());
        assert_eq!(local_pool.len(), 1);

        for idx in 0..32u64 {
            let key = DohClientPoolKey {
                use_doh: true,
                proxy_gen: 2,
                doh_epoch: 2,
                target_port: 443,
                follow_redirects: true,
                is_burst: false,
                connect_ms: 1000 + idx,
                read_ms: 5000,
            };
            prune_and_insert_pool_entry(&mut local_pool, key, dummy_client.clone());
            assert!(local_pool.len() <= MAX_DOH_CLIENT_POOL_ENTRIES);
        }
        assert!(!local_pool.contains_key(&stale_key));
        assert_eq!(local_pool.len(), MAX_DOH_CLIENT_POOL_ENTRIES);

        // Verify pooled client reuse and bounded global pool via IP literal (no network calls).
        if let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() {
            rt.block_on(async {
                let _c1 = build_doh_client_with_timeouts(
                    "http://127.0.0.1:8080/health",
                    Duration::ZERO,
                    Duration::ZERO,
                    false,
                )
                .await;
                let count_after_first = get_doh_client_pool().lock().map(|p| p.len()).unwrap_or(0);
                let _c2 = build_doh_client_with_timeouts(
                    "http://127.0.0.1:8080/health",
                    Duration::ZERO,
                    Duration::ZERO,
                    false,
                )
                .await;
                let count_after_second = get_doh_client_pool().lock().map(|p| p.len()).unwrap_or(0);
                assert_eq!(count_after_first, count_after_second);
                assert!(count_after_second <= MAX_DOH_CLIENT_POOL_ENTRIES);
            });
        }
    }
}

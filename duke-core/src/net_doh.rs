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
    PROXY_CONFIG_GENERATION.fetch_add(1, Ordering::Relaxed);
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
    {
        cache.clear();
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
                    } else if addrs.len() > 1 {
                        addrs.rotate_left(1);
                    }
                }
        }
}

/// Builds a `reqwest::Client` pre-populated with encrypted DoH `SocketAddr` overrides
/// (when DoH is enabled and no custom proxy handles DNS) and `DNT: 1` / `Sec-GPC: 1` privacy headers.
pub async fn build_doh_client_for_url(
    raw_url: &str,
    is_burst: bool,
    follow_redirects: bool,
) -> Client {
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

    let cached_snapshot: Vec<(String, Vec<SocketAddr>)> = if use_doh {
        get_doh_cache()
            .lock()
            .map(|c| c.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let mut builder = Client::builder()
        .user_agent(get_session_user_agent())
        .default_headers(build_privacy_headers())
        .tcp_nodelay(true);

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
            .pool_max_idle_per_host(0)
            .connect_timeout(Duration::from_secs(6))
            .read_timeout(Duration::from_secs(5));
    } else {
        builder = builder
            .tcp_keepalive(Duration::from_secs(15))
            .pool_max_idle_per_host(12)
            .pool_idle_timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(7))
            .read_timeout(Duration::from_secs(8));
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

    builder.build().unwrap_or_else(|_| {
        if is_burst && proxy_url.is_none() {
            get_burst_http_client().clone()
        } else {
            get_configured_http_client()
        }
    })
}

/// Builds a DoH/proxy-backed `reqwest::Client` with explicit connect/read timeouts.
pub async fn build_doh_client_with_timeouts(
    raw_url: &str,
    connect_timeout: Duration,
    read_timeout: Duration,
    follow_redirects: bool,
) -> Client {
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

    let cached_snapshot: Vec<(String, Vec<SocketAddr>)> = if use_doh {
        get_doh_cache()
            .lock()
            .map(|c| c.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default()
    } else {
        Vec::new()
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

    builder.build().unwrap_or_else(|_| get_configured_http_client())
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
}

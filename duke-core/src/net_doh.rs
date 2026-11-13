//! Shared Encrypted DNS-over-HTTPS (DoH) resolver and privacy-hardened `reqwest` client builder.
//!
//! Races queries across Cloudflare Primary (`1.1.1.1`), Cloudflare Secondary (`1.0.0.1`), and
//! Quad9 (`9.9.9.9:5053`) using `Accept: application/dns-json`, caching resolved IPv4 `SocketAddr`
//! pools in a shared `OnceLock` cache.

use reqwest::header::{HeaderMap, HeaderValue, ACCEPT};
use reqwest::{Client, ClientBuilder};
use serde_json::Value;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use url::Url;

static HTTP_CLIENT: OnceLock<Client> = OnceLock::new();
static BURST_HTTP_CLIENT: OnceLock<Client> = OnceLock::new();
static DOH_BOOTSTRAP_CLIENT: OnceLock<Client> = OnceLock::new();
static DOH_CACHE: OnceLock<Mutex<HashMap<String, Vec<SocketAddr>>>> = OnceLock::new();
static SESSION_UA_CURSOR: AtomicU64 = AtomicU64::new(0);

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

pub fn build_privacy_client_builder() -> ClientBuilder {
    Client::builder()
        .user_agent(get_session_user_agent())
        .default_headers(build_privacy_headers())
        .tcp_nodelay(true)
}

pub fn get_http_client() -> &'static Client {
    HTTP_CLIENT.get_or_init(|| {
        let ua = rotate_privacy_user_agent();
        Client::builder()
            .user_agent(ua)
            .default_headers(build_privacy_headers())
            .tcp_nodelay(true)
            .tcp_keepalive(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(8))
            .pool_max_idle_per_host(12)
            .pool_idle_timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(8))
            .build()
            .expect("Failed to initialize shared HTTP client")
    })
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
            .expect("Failed to initialize burst HTTP client")
    })
}

pub fn get_doh_bootstrap_client() -> &'static Client {
    DOH_BOOTSTRAP_CLIENT.get_or_init(|| {
        build_privacy_client_builder()
            .connect_timeout(Duration::from_secs(3))
            .read_timeout(Duration::from_secs(4))
            .build()
            .expect("Failed to initialize DoH bootstrap client")
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

    let (ips_cf1, ips_cf2, ips_q9) = tokio::join!(
        fetch_provider_ips(cf_primary),
        fetch_provider_ips(cf_secondary),
        fetch_provider_ips(quad9_primary)
    );

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
/// and `DNT: 1` / `Sec-GPC: 1` privacy headers.
pub async fn build_doh_client_for_url(
    raw_url: &str,
    is_burst: bool,
    follow_redirects: bool,
) -> Client {
    let mut target_port = 443u16;
    if let Ok(parsed) = Url::parse(raw_url) {
        target_port = parsed.port_or_known_default().unwrap_or(443);
        if let Some(host) = parsed.host_str() {
            let _ = resolve_domain_doh(host, target_port).await;
        }
    }

    let cached_snapshot: Vec<(String, Vec<SocketAddr>)> = get_doh_cache()
        .lock()
        .map(|c| c.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();

    let mut builder = build_privacy_client_builder();

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
        if is_burst {
            get_burst_http_client().clone()
        } else {
            get_http_client().clone()
        }
    })
}

/// Builds a DoH-backed `reqwest::Client` with explicit connect/read timeouts.
pub async fn build_doh_client_with_timeouts(
    raw_url: &str,
    connect_timeout: Duration,
    read_timeout: Duration,
    follow_redirects: bool,
) -> Client {
    let mut target_port = 443u16;
    if let Ok(parsed) = Url::parse(raw_url) {
        target_port = parsed.port_or_known_default().unwrap_or(443);
        if let Some(host) = parsed.host_str() {
            let _ = resolve_domain_doh(host, target_port).await;
        }
    }

    let cached_snapshot: Vec<(String, Vec<SocketAddr>)> = get_doh_cache()
        .lock()
        .map(|c| c.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();

    let mut builder = build_privacy_client_builder()
        .connect_timeout(connect_timeout)
        .read_timeout(read_timeout);

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

    builder.build().unwrap_or_else(|_| get_http_client().clone())
}

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use wasmtime::{Caller, Config, Engine, Linker, Memory, Module, Store, StoreLimits, StoreLimitsBuilder};

use crate::{atomic_write_string, get_app_data_file_standalone, load_library_standalone, resolve_data_root_dir_standalone};

const WASM_PLUGINS_MANIFEST_FILE: &str = "wasm_plugins.json";
const WASM_PLUGINS_SUBDIR: &str = "plugins";
const DEFAULT_PLUGIN_FUEL: u64 = 2_000_000;
const MAX_PLUGIN_MEMORY_BYTES: usize = 8 * 1024 * 1024; // 8 MiB hard WASM heap ceiling
const MAX_PLUGIN_LOGS: usize = 32;
const MAX_GUEST_IO_BYTES: usize = 256 * 1024;
const ALLOWED_PLUGIN_HOOKS: &[&str] = &["metadata_lookup", "rom_database", "storefront_feed"];

static WASM_IO_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn wasm_lock() -> &'static Mutex<()> {
    WASM_IO_LOCK.get_or_init(|| Mutex::new(()))
}

fn epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginCapabilityGrant {
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    #[serde(default)]
    pub allowed_hooks: Vec<String>,
    #[serde(default)]
    pub allow_read_only_library_titles: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledWasmPlugin {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub module_path: String,
    pub sha256: String,
    pub bytes_len: u64,
    #[serde(default)]
    pub requested_domains: Vec<String>,
    #[serde(default)]
    pub requested_hooks: Vec<String>,
    #[serde(default)]
    pub granted_capabilities: PluginCapabilityGrant,
    pub enabled: bool,
    pub created_at_epoch_sec: u64,
    #[serde(default)]
    pub last_run_epoch_sec: Option<u64>,
    #[serde(default)]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WasmPluginRunResult {
    pub plugin_id: String,
    pub hook: String,
    pub exit_code: i32,
    pub fuel_consumed: u64,
    pub logs: Vec<String>,
    pub emitted_records: Vec<Value>,
    pub capability_denials: Vec<String>,
    pub elapsed_ms: u64,
}

pub type ScopedHttpFetcher = Arc<dyn Fn(&str) -> Result<String, String> + Send + Sync>;

struct PluginHostState {
    grant: PluginCapabilityGrant,
    requested_hook: String,
    hook_input_json: String,
    logs: Vec<String>,
    emitted_records: Vec<Value>,
    capability_denials: Vec<String>,
    limits: StoreLimits,
    http_fetcher: ScopedHttpFetcher,
}

/// Validates and normalizes a single user-approved domain pattern (explicit host only, no wildcards).
pub fn normalize_and_validate_domain(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return Err("Empty domain entry is not allowed.".to_string());
    }
    if trimmed.contains('*')
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains('@')
        || trimmed.contains("://")
        || trimmed.chars().any(|c| c.is_whitespace())
    {
        return Err(format!(
            "Invalid capability domain '{}': supply a clean hostname only (no wildcards, paths, or credentials).",
            raw.trim()
        ));
    }
    if trimmed == "169.254.169.254" || trimmed.starts_with("169.254.") {
        return Err("Link-local cloud metadata IP ranges are blocked from plugin capability grants.".to_string());
    }
    Ok(trimmed)
}

pub fn normalize_hook_name(raw: &str) -> Result<String, String> {
    let clean = raw.trim().to_ascii_lowercase();
    if ALLOWED_PLUGIN_HOOKS.contains(&clean.as_str()) {
        Ok(clean)
    } else {
        Err(format!(
            "Unknown plugin hook '{}'. Permitted hooks: {}",
            raw.trim(),
            ALLOWED_PLUGIN_HOOKS.join(", ")
        ))
    }
}

pub fn is_hook_permitted(hook: &str, grant: &PluginCapabilityGrant) -> bool {
    grant
        .allowed_hooks
        .iter()
        .any(|h| h.eq_ignore_ascii_case(hook.trim()))
}

/// Enforces zero-trust URL and domain capability boundaries before any socket opens.
pub fn validate_plugin_http_url(url_str: &str, allowed_domains: &[String]) -> Result<String, String> {
    let trimmed = url_str.trim();
    if trimmed.is_empty() || trimmed.len() > 1024 {
        return Err("CAPABILITY_DENIED: plugin HTTP URL is empty or exceeds 1024 bytes.".to_string());
    }
    let parsed = tauri::Url::parse(trimmed)
        .map_err(|e| format!("CAPABILITY_DENIED: invalid plugin URL '{}': {e}", trimmed))?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("CAPABILITY_DENIED: URLs containing embedded credentials are forbidden.".to_string());
    }
    let host = parsed
        .host_str()
        .map(|h| h.to_ascii_lowercase())
        .ok_or_else(|| "CAPABILITY_DENIED: URL has no host component.".to_string())?;

    let is_loopback = host == "127.0.0.1" || host == "localhost";
    match parsed.scheme() {
        "https" => {}
        "http" if is_loopback => {}
        other => {
            return Err(format!(
                "CAPABILITY_DENIED: scheme '{}' rejected (only HTTPS, or loopback HTTP when explicitly approved, is allowed).",
                other
            ));
        }
    }

    let host_approved = allowed_domains
        .iter()
        .any(|d| d.trim().eq_ignore_ascii_case(&host));
    if !host_approved {
        return Err(format!(
            "CAPABILITY_DENIED: domain '{}' is not in this plugin's user-approved allowlist.",
            host
        ));
    }

    Ok(parsed.to_string())
}

fn default_doh_http_fetcher(url_str: &str) -> Result<String, String> {
    let url_owned = url_str.to_string();
    std::thread::spawn(move || -> Result<String, String> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Failed creating plugin HTTP runtime: {e}"))?;
        rt.block_on(async move {
            let client = duke_core::net_doh::build_doh_client_with_timeouts(
                &url_owned,
                Duration::from_secs(4),
                Duration::from_secs(6),
                false,
            )
            .await;
            let resp = client
                .get(&url_owned)
                .headers(duke_core::net_doh::build_privacy_headers())
                .send()
                .await
                .map_err(|e| format!("Scoped plugin HTTP request failed: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("Scoped plugin HTTP status {}", resp.status()));
            }
            let bytes = resp
                .bytes()
                .await
                .map_err(|e| format!("Failed reading plugin HTTP body: {e}"))?;
            let capped_len = bytes.len().min(MAX_GUEST_IO_BYTES);
            Ok(String::from_utf8_lossy(&bytes[..capped_len]).to_string())
        })
    })
    .join()
    .map_err(|_| "Plugin HTTP transport thread terminated unexpectedly.".to_string())?
}

fn read_guest_str(
    memory: &Memory,
    caller: &Caller<'_, PluginHostState>,
    ptr: i32,
    len: i32,
    max_len: usize,
) -> Result<String, String> {
    if ptr < 0 || len < 0 || (len as usize) > max_len {
        return Err("Guest pointer or length out of bounds.".to_string());
    }
    let start = ptr as usize;
    let slice_len = len as usize;
    let end = start
        .checked_add(slice_len)
        .ok_or_else(|| "Guest slice overflow.".to_string())?;
    let data = memory.data(caller);
    if end > data.len() {
        return Err("Guest slice exceeds linear memory size.".to_string());
    }
    std::str::from_utf8(&data[start..end])
        .map(|s| s.to_string())
        .map_err(|_| "Guest payload is not valid UTF-8.".to_string())
}

fn write_guest_bytes(
    memory: &Memory,
    caller: &mut Caller<'_, PluginHostState>,
    out_ptr: i32,
    out_cap: i32,
    payload: &[u8],
) -> i32 {
    if out_ptr < 0 || out_cap <= 0 {
        return -1;
    }
    let start = out_ptr as usize;
    let cap = out_cap as usize;
    let n = payload.len().min(cap);
    let Some(end) = start.checked_add(n) else {
        return -1;
    };
    let mem = memory.data_mut(caller);
    if end > mem.len() {
        return -1;
    }
    mem[start..end].copy_from_slice(&payload[..n]);
    n as i32
}

fn guest_memory(caller: &mut Caller<'_, PluginHostState>) -> Option<Memory> {
    caller.get_export("memory").and_then(|e| e.into_memory())
}

/// Executes raw `.wasm` / `.wat` module bytes inside an isolated `wasmtime` sandbox with zero filesystem or credential access.
pub fn execute_wasm_module_bytes(
    plugin_id: &str,
    module_bytes: &[u8],
    grant: PluginCapabilityGrant,
    hook: &str,
    query: &str,
    read_only_titles: &[String],
    custom_http_fetcher: Option<ScopedHttpFetcher>,
) -> Result<WasmPluginRunResult, String> {
    let started = Instant::now();
    let clean_hook = normalize_hook_name(hook)?;

    let mut cfg = Config::new();
    cfg.consume_fuel(true);
    let engine = Engine::new(&cfg).map_err(|e| format!("Failed to init Wasmtime engine: {e}"))?;
    let module = Module::new(&engine, module_bytes)
        .map_err(|e| format!("Invalid or unsafe WebAssembly plugin module: {e}"))?;

    let mut input_obj = json!({
        "hook": clean_hook,
        "query": query.trim(),
    });
    if grant.allow_read_only_library_titles {
        input_obj["libraryTitles"] = Value::Array(
            read_only_titles
                .iter()
                .take(256)
                .map(|t| Value::String(t.clone()))
                .collect(),
        );
    }

    let host_state = PluginHostState {
        grant: grant.clone(),
        requested_hook: clean_hook.clone(),
        hook_input_json: input_obj.to_string(),
        logs: Vec::new(),
        emitted_records: Vec::new(),
        capability_denials: Vec::new(),
        limits: StoreLimitsBuilder::new()
            .memory_size(MAX_PLUGIN_MEMORY_BYTES)
            .tables(4)
            .instances(2)
            .memories(1)
            .build(),
        http_fetcher: custom_http_fetcher.unwrap_or_else(|| Arc::new(default_doh_http_fetcher)),
    };

    let mut store = Store::new(&engine, host_state);
    store.limiter(|s| &mut s.limits);
    store
        .set_fuel(DEFAULT_PLUGIN_FUEL)
        .map_err(|e| format!("Failed configuring Wasmtime fuel budget: {e}"))?;

    let mut linker: Linker<PluginHostState> = Linker::new(&engine);

    linker
        .func_wrap(
            "duke",
            "log_info",
            |mut caller: Caller<'_, PluginHostState>, ptr: i32, len: i32| -> i32 {
                let Some(mem) = guest_memory(&mut caller) else {
                    return -1;
                };
                let Ok(msg) = read_guest_str(&mem, &caller, ptr, len, 512) else {
                    return -1;
                };
                let state = caller.data_mut();
                if state.logs.len() < MAX_PLUGIN_LOGS {
                    state.logs.push(msg);
                }
                0
            },
        )
        .map_err(|e| format!("Linker error (log_info): {e}"))?;

    linker
        .func_wrap(
            "duke",
            "get_hook_input",
            |mut caller: Caller<'_, PluginHostState>, out_ptr: i32, out_cap: i32| -> i32 {
                let (permitted, hook_name, payload_bytes) = {
                    let s = caller.data();
                    (
                        is_hook_permitted(&s.requested_hook, &s.grant),
                        s.requested_hook.clone(),
                        s.hook_input_json.as_bytes().to_vec(),
                    )
                };
                if !permitted {
                    caller.data_mut().capability_denials.push(format!(
                        "CAPABILITY_DENIED: hook '{}' has not been approved for this plugin.",
                        hook_name
                    ));
                    return -1;
                }
                let Some(mem) = guest_memory(&mut caller) else {
                    return -1;
                };
                write_guest_bytes(&mem, &mut caller, out_ptr, out_cap, &payload_bytes)
            },
        )
        .map_err(|e| format!("Linker error (get_hook_input): {e}"))?;

    linker
        .func_wrap(
            "duke",
            "emit_metadata",
            |mut caller: Caller<'_, PluginHostState>, ptr: i32, len: i32| -> i32 {
                let (permitted, hook_name) = {
                    let s = caller.data();
                    (
                        is_hook_permitted(&s.requested_hook, &s.grant),
                        s.requested_hook.clone(),
                    )
                };
                if !permitted {
                    caller.data_mut().capability_denials.push(format!(
                        "CAPABILITY_DENIED: hook '{}' write denied (grant hook permission in Settings).",
                        hook_name
                    ));
                    return -1;
                }
                let Some(mem) = guest_memory(&mut caller) else {
                    return -1;
                };
                let Ok(raw) = read_guest_str(&mem, &caller, ptr, len, 64 * 1024) else {
                    return -1;
                };
                let Ok(val) = serde_json::from_str::<Value>(&raw) else {
                    return -1;
                };
                let state = caller.data_mut();
                if state.emitted_records.len() < 128 {
                    state.emitted_records.push(val);
                    0
                } else {
                    -1
                }
            },
        )
        .map_err(|e| format!("Linker error (emit_metadata): {e}"))?;

    linker
        .func_wrap(
            "duke",
            "http_get_scoped",
            |mut caller: Caller<'_, PluginHostState>,
             url_ptr: i32,
             url_len: i32,
             out_ptr: i32,
             out_cap: i32|
             -> i32 {
                let Some(mem) = guest_memory(&mut caller) else {
                    return -1;
                };
                let Ok(url_str) = read_guest_str(&mem, &caller, url_ptr, url_len, 1024) else {
                    return -1;
                };
                let (allowed_domains, fetcher) = {
                    let s = caller.data();
                    (s.grant.allowed_domains.clone(), s.http_fetcher.clone())
                };
                let validated_url = match validate_plugin_http_url(&url_str, &allowed_domains) {
                    Ok(v) => v,
                    Err(denial) => {
                        caller.data_mut().capability_denials.push(denial);
                        return -1;
                    }
                };
                match fetcher(&validated_url) {
                    Ok(body) => {
                        write_guest_bytes(&mem, &mut caller, out_ptr, out_cap, body.as_bytes())
                    }
                    Err(err) => {
                        caller.data_mut().logs.push(format!("HTTP error: {err}"));
                        -1
                    }
                }
            },
        )
        .map_err(|e| format!("Linker error (http_get_scoped): {e}"))?;

    let instance = linker
        .instantiate(&mut store, &module)
        .map_err(|e| format!("Failed to instantiate WASM plugin: {e}"))?;

    let entry = instance
        .get_typed_func::<(), i32>(&mut store, "duke_run_hook")
        .or_else(|_| instance.get_typed_func::<(), i32>(&mut store, "run"))
        .map_err(|_| {
            "Plugin module must export `duke_run_hook() -> i32` or `run() -> i32`.".to_string()
        })?;

    let exit_code = entry
        .call(&mut store, ())
        .map_err(|e| format!("WASM plugin trapped safely inside sandbox: {e}"))?;

    let remaining_fuel = store.get_fuel().unwrap_or(0);
    let fuel_consumed = DEFAULT_PLUGIN_FUEL.saturating_sub(remaining_fuel);
    let state = store.into_data();

    Ok(WasmPluginRunResult {
        plugin_id: plugin_id.to_string(),
        hook: clean_hook,
        exit_code,
        fuel_consumed,
        logs: state.logs,
        emitted_records: state.emitted_records,
        capability_denials: state.capability_denials,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

fn manifest_path() -> Result<PathBuf, String> {
    get_app_data_file_standalone(WASM_PLUGINS_MANIFEST_FILE)
}

fn plugins_dir() -> Result<PathBuf, String> {
    let dir = resolve_data_root_dir_standalone()?.join(WASM_PLUGINS_SUBDIR);
    fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed creating plugin directory {}: {e}", dir.display()))?;
    Ok(dir)
}

fn load_plugins_manifest() -> Vec<InstalledWasmPlugin> {
    let Ok(path) = manifest_path() else {
        return Vec::new();
    };
    if !path.is_file() {
        return Vec::new();
    }
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_plugins_manifest(plugins: &[InstalledWasmPlugin]) -> Result<(), String> {
    let path = manifest_path()?;
    let json = serde_json::to_string_pretty(plugins)
        .map_err(|e| format!("Failed serializing WASM plugin manifest: {e}"))?;
    atomic_write_string(&path, &json)
}

#[tauri::command]
pub async fn list_wasm_plugins() -> Result<Vec<InstalledWasmPlugin>, String> {
    let _guard = wasm_lock().lock().unwrap_or_else(|p| p.into_inner());
    Ok(load_plugins_manifest())
}

#[tauri::command]
pub async fn install_wasm_plugin(
    file_path: String,
    custom_name: Option<String>,
    requested_domains: Option<Vec<String>>,
    requested_hooks: Option<Vec<String>>,
) -> Result<InstalledWasmPlugin, String> {
    tokio::task::spawn_blocking(move || {
        let src = PathBuf::from(file_path.trim().trim_matches('"'));
        if !src.is_file() {
            return Err(format!("WASM/WAT file not found: {}", src.display()));
        }
        let bytes = fs::read(&src).map_err(|e| format!("Failed reading plugin file: {e}"))?;
        if bytes.is_empty() || bytes.len() > 16 * 1024 * 1024 {
            return Err("Plugin size must be between 1 byte and 16 MiB.".to_string());
        }

        // Compile-validate module upfront before storing it in the plugin vault.
        let mut cfg = Config::new();
        cfg.consume_fuel(true);
        let engine = Engine::new(&cfg).map_err(|e| format!("Wasmtime engine error: {e}"))?;
        let _validated = Module::new(&engine, &bytes)
            .map_err(|e| format!("WASM validation failed: {e}"))?;

        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let sha_hex = hex::encode(hasher.finalize());
        let id = format!("wasm_{}", &sha_hex[..12]);

        let ext = src
            .extension()
            .and_then(|s| s.to_str())
            .filter(|e| e.eq_ignore_ascii_case("wat"))
            .unwrap_or("wasm");
        let dst = plugins_dir()?.join(format!("{id}.{ext}"));
        fs::write(&dst, &bytes).map_err(|e| format!("Failed saving plugin to vault: {e}"))?;

        let mut req_domains = Vec::new();
        if let Some(doms) = requested_domains {
            for d in doms {
                if let Ok(clean) = normalize_and_validate_domain(&d) {
                    if !req_domains.contains(&clean) {
                        req_domains.push(clean);
                    }
                }
            }
        }
        let mut req_hooks = Vec::new();
        if let Some(hooks) = requested_hooks {
            for h in hooks {
                if let Ok(clean) = normalize_hook_name(&h) {
                    if !req_hooks.contains(&clean) {
                        req_hooks.push(clean);
                    }
                }
            }
        }
        if req_hooks.is_empty() {
            req_hooks.push("metadata_lookup".to_string());
        }

        let fallback_name = src
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Community Plugin")
            .to_string();
        let name = custom_name
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or(fallback_name);

        let record = InstalledWasmPlugin {
            id: id.clone(),
            name,
            version: "1.0.0".to_string(),
            description: "Zero-trust WebAssembly extension (domains & hooks require explicit user approval).".to_string(),
            module_path: dst.to_string_lossy().to_string(),
            sha256: sha_hex,
            bytes_len: bytes.len() as u64,
            requested_domains: req_domains,
            requested_hooks: req_hooks,
            // Zero-trust default: NO domain or hook capabilities auto-granted on install.
            granted_capabilities: PluginCapabilityGrant::default(),
            enabled: true,
            created_at_epoch_sec: epoch_secs(),
            last_run_epoch_sec: None,
            last_error: None,
        };

        let _guard = wasm_lock().lock().unwrap_or_else(|p| p.into_inner());
        let mut list = load_plugins_manifest();
        list.retain(|p| p.id != id);
        list.push(record.clone());
        save_plugins_manifest(&list)?;
        Ok(record)
    })
    .await
    .map_err(|e| format!("Install WASM plugin task failed: {e}"))?
}

#[tauri::command]
pub async fn install_demo_wasm_plugin() -> Result<InstalledWasmPlugin, String> {
    let demo_wat = r#"(module
  (import "duke" "log_info" (func $log (param i32 i32) (result i32)))
  (import "duke" "get_hook_input" (func $get_input (param i32 i32) (result i32)))
  (import "duke" "emit_metadata" (func $emit (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "Demo WAT metadata provider initialized")
  (data (i32.const 64) "{\"provider\":\"Community WAT Metadata Sandbox\",\"hook\":\"metadata_lookup\",\"trusted\":false,\"note\":\"Verified zero-trust WASM execution\"}")
  (func (export "duke_run_hook") (result i32)
    (drop (call $log (i32.const 0) (i32.const 38)))
    (if (i32.lt_s (call $get_input (i32.const 1024) (i32.const 2048)) (i32.const 0))
      (then (return (i32.const -1))))
    (call $emit (i32.const 64) (i32.const 129))
  )
)"#;

    let tmp_path = plugins_dir()?.join("community-demo-metadata.wat");
    fs::write(&tmp_path, demo_wat)
        .map_err(|e| format!("Failed writing demo plugin WAT: {e}"))?;

    let plugin = install_wasm_plugin(
        tmp_path.to_string_lossy().to_string(),
        Some("Community Metadata Provider (Demo WAT)".to_string()),
        Some(vec!["api.steampowered.com".to_string()]),
        Some(vec!["metadata_lookup".to_string()]),
    )
    .await?;

    // Auto-grant ONLY read-only metadata_lookup hook for the built-in demo (0 network domains unless approved).
    set_wasm_plugin_capabilities(
        plugin.id,
        true,
        Vec::new(),
        vec!["metadata_lookup".to_string()],
        false,
    )
    .await
}

#[tauri::command]
pub async fn set_wasm_plugin_capabilities(
    plugin_id: String,
    enabled: bool,
    allowed_domains: Vec<String>,
    allowed_hooks: Vec<String>,
    allow_read_only_library_titles: bool,
) -> Result<InstalledWasmPlugin, String> {
    let mut clean_domains = Vec::new();
    let mut seen_d = HashSet::new();
    for raw in allowed_domains {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let norm = normalize_and_validate_domain(trimmed)?;
        if seen_d.insert(norm.clone()) {
            clean_domains.push(norm);
        }
    }

    let mut clean_hooks = Vec::new();
    let mut seen_h = HashSet::new();
    for raw in allowed_hooks {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let norm = normalize_hook_name(trimmed)?;
        if seen_h.insert(norm.clone()) {
            clean_hooks.push(norm);
        }
    }

    let _guard = wasm_lock().lock().unwrap_or_else(|p| p.into_inner());
    let mut list = load_plugins_manifest();
    let idx = list
        .iter()
        .position(|p| p.id == plugin_id)
        .ok_or_else(|| format!("WASM plugin not found: {plugin_id}"))?;

    list[idx].enabled = enabled;
    list[idx].granted_capabilities = PluginCapabilityGrant {
        allowed_domains: clean_domains,
        allowed_hooks: clean_hooks,
        allow_read_only_library_titles,
    };
    list[idx].last_error = None;
    let updated = list[idx].clone();
    save_plugins_manifest(&list)?;
    Ok(updated)
}

#[tauri::command]
pub async fn execute_wasm_plugin(
    plugin_id: String,
    hook: Option<String>,
    query: Option<String>,
) -> Result<WasmPluginRunResult, String> {
    tokio::task::spawn_blocking(move || {
        let plugin = {
            let _guard = wasm_lock().lock().unwrap_or_else(|p| p.into_inner());
            let list = load_plugins_manifest();
            list.into_iter()
                .find(|p| p.id == plugin_id)
                .ok_or_else(|| format!("WASM plugin not found: {plugin_id}"))?
        };
        if !plugin.enabled {
            return Err(format!("Plugin '{}' is disabled in Settings.", plugin.name));
        }

        let module_bytes = fs::read(&plugin.module_path)
            .map_err(|e| format!("Failed reading plugin binary {}: {e}", plugin.module_path))?;

        let titles: Vec<String> = if plugin.granted_capabilities.allow_read_only_library_titles {
            load_library_standalone()
                .unwrap_or_default()
                .into_iter()
                .map(|g| g.title)
                .collect()
        } else {
            Vec::new()
        };

        let hook_name = hook
            .filter(|h| !h.trim().is_empty())
            .unwrap_or_else(|| "metadata_lookup".to_string());
        let res = execute_wasm_module_bytes(
            &plugin.id,
            &module_bytes,
            plugin.granted_capabilities.clone(),
            &hook_name,
            query.as_deref().unwrap_or(""),
            &titles,
            None,
        );

        let _guard = wasm_lock().lock().unwrap_or_else(|p| p.into_inner());
        let mut list = load_plugins_manifest();
        if let Some(rec) = list.iter_mut().find(|p| p.id == plugin.id) {
            rec.last_run_epoch_sec = Some(epoch_secs());
            rec.last_error = match &res {
                Ok(r) if r.capability_denials.is_empty() => None,
                Ok(r) => Some(r.capability_denials.join("; ")),
                Err(e) => Some(e.clone()),
            };
            let _ = save_plugins_manifest(&list);
        }

        res
    })
    .await
    .map_err(|e| format!("Execute WASM plugin task failed: {e}"))?
}

#[tauri::command]
pub async fn remove_wasm_plugin(plugin_id: String) -> Result<Vec<InstalledWasmPlugin>, String> {
    let _guard = wasm_lock().lock().unwrap_or_else(|p| p.into_inner());
    let mut list = load_plugins_manifest();
    if let Some(pos) = list.iter().position(|p| p.id == plugin_id) {
        let removed = list.remove(pos);
        let _ = fs::remove_file(&removed.module_path);
        save_plugins_manifest(&list)?;
    }
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_url_guard_enforces_exact_domains_and_blocks_wildcards_and_ssrf() {
        assert!(normalize_and_validate_domain("*.example.com").is_err());
        assert!(normalize_and_validate_domain("169.254.169.254").is_err());
        assert_eq!(
            normalize_and_validate_domain("  API.Rawg.IO ").unwrap(),
            "api.rawg.io"
        );

        let allow = vec!["metadata.example.org".to_string()];
        assert!(validate_plugin_http_url("https://metadata.example.org/v1/game", &allow).is_ok());
        assert!(validate_plugin_http_url("https://evil.example.net/steal", &allow).is_err());
        assert!(validate_plugin_http_url("file:///C:/Windows/win.ini", &allow).is_err());
        assert!(
            validate_plugin_http_url("https://user:pass@metadata.example.org/", &allow).is_err()
        );
    }

    #[test]
    fn wasm_sandbox_blocks_unapproved_http_domain_and_allows_approved_domain() {
        let wat = br#"(module
  (import "duke" "http_get_scoped" (func $http_get (param i32 i32 i32 i32) (result i32)))
  (import "duke" "emit_metadata" (func $emit (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "https://api.allowed.test/meta")
  (data (i32.const 64) "https://blocked.evil.test/exfil")
  (func (export "duke_run_hook") (result i32)
    (local $n i32)
    ;; Try unauthorized domain first: must return -1 and log denial.
    (drop (call $http_get (i32.const 64) (i32.const 31) (i32.const 1024) (i32.const 4096)))
    ;; Now try approved domain: must succeed cleanly and emit JSON body.
    (local.set $n (call $http_get (i32.const 0) (i32.const 29) (i32.const 1024) (i32.const 4096)))
    (if (i32.le_s (local.get $n) (i32.const 0))
      (then (return (i32.const -2))))
    (call $emit (i32.const 1024) (local.get $n))
  )
)"#;

        let mock_http: ScopedHttpFetcher = Arc::new(|url| {
            if url == "https://api.allowed.test/meta" {
                Ok("{\"title\":\"Portal 2\",\"score\":98}".to_string())
            } else {
                Err("unexpected fetch".to_string())
            }
        });

        let grant = PluginCapabilityGrant {
            allowed_domains: vec!["api.allowed.test".to_string()],
            allowed_hooks: vec!["metadata_lookup".to_string()],
            allow_read_only_library_titles: false,
        };

        let result = execute_wasm_module_bytes(
            "test_plugin",
            wat,
            grant,
            "metadata_lookup",
            "Portal 2",
            &[],
            Some(mock_http),
        )
        .unwrap();

        assert_eq!(result.exit_code, 0);
        assert_eq!(result.capability_denials.len(), 1);
        assert!(result.capability_denials[0].contains("blocked.evil.test"));
        assert_eq!(result.emitted_records.len(), 1);
        assert_eq!(result.emitted_records[0]["title"], "Portal 2");
        assert!(result.fuel_consumed > 0);
    }

    #[test]
    fn wasm_sandbox_fuel_limit_traps_infinite_loops_safely() {
        let infinite_wat = br#"(module
  (func (export "duke_run_hook") (result i32)
    (loop $spin
      (br $spin))
    (i32.const 0)
  )
)"#;

        let res = execute_wasm_module_bytes(
            "spin_plugin",
            infinite_wat,
            PluginCapabilityGrant::default(),
            "metadata_lookup",
            "",
            &[],
            None,
        );
        assert!(res.is_err());
        let err = res.err().unwrap_or_default();
        assert!(err.contains("trapped safely"));
    }
}

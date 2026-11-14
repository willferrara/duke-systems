use crate::{atomic_write_string, get_app_data_file_standalone};
use duke_core::win32_ffi::{get_current_working_set_mb, trim_current_working_set};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::{c_void, OsStr};
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const AI_CONFIG_FILE: &str = "ai_runtime_config.json";
const DEFAULT_FOUNDRY_ENDPOINT: &str = "http://127.0.0.1:5272/v1";

static AI_POOL: OnceLock<Mutex<AiRuntimeState>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionProviderKind {
    NpuQnn,
    NpuVitisAi,
    NpuOpenVino,
    NpuWinMl,
    DirectMl,
    Cpu,
}

impl ExecutionProviderKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::NpuQnn => "NPU (Qualcomm QNN)",
            Self::NpuVitisAi => "NPU (AMD Ryzen AI VitisAI)",
            Self::NpuOpenVino => "NPU (Intel OpenVINO)",
            Self::NpuWinMl => "NPU (Windows ML / MCDM)",
            Self::DirectMl => "DirectML (DirectX 12 GPU)",
            Self::Cpu => "CPU (Zero-VRAM Fallback)",
        }
    }

    pub fn is_npu(self) -> bool {
        matches!(
            self,
            Self::NpuQnn | Self::NpuVitisAi | Self::NpuOpenVino | Self::NpuWinMl
        )
    }

    pub fn vram_allocated_mb(self) -> f32 {
        match self {
            Self::DirectMl => 192.0,
            _ => 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiRuntimeConfig {
    pub preferred_provider: String, // "auto", "npu", "directml", "cpu"
    pub auto_unload_enabled: bool,
    pub idle_ttl_secs: u64,
    pub foundry_endpoint: String,
    pub foundry_model: String,
    pub target_language: String,
}

impl Default for AiRuntimeConfig {
    fn default() -> Self {
        let env_endpoint = std::env::var("DUKE_FOUNDRY_URL")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_FOUNDRY_ENDPOINT.to_string());
        Self {
            preferred_provider: "auto".to_string(),
            auto_unload_enabled: true,
            idle_ttl_secs: 45,
            foundry_endpoint: env_endpoint,
            foundry_model: String::new(),
            target_language: "en".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerAllocation {
    pub name: String,
    pub execution_provider: String,
    pub active_model: String,
    pub last_used_ms: u64,
    pub estimated_ram_mb: f32,
    pub vram_allocated_mb: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareAiProbe {
    pub onnxruntime_available: bool,
    pub qnn_available: bool,
    pub vitis_ai_available: bool,
    pub openvino_available: bool,
    pub winml_available: bool,
    pub directml_available: bool,
    pub oneocr_available: bool,
    pub winrt_ocr_available: bool,
    pub npu_detected: bool,
    pub npu_name: String,
    pub gpu_name: String,
    pub foundry_local_online: bool,
    pub foundry_models: Vec<String>,
    pub model_dir: String,
    pub installed_models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiRuntimeStatus {
    pub config: AiRuntimeConfig,
    pub probe: HardwareAiProbe,
    pub active_execution_provider: String,
    pub zero_vram_mode: bool,
    pub vram_allocated_mb: f32,
    pub worker_ram_mb: f32,
    pub process_working_set_mb: f64,
    pub loaded_workers: Vec<WorkerAllocation>,
}

struct AiRuntimeState {
    config: AiRuntimeConfig,
    workers: HashMap<String, WorkerAllocation>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn load_config_from_disk() -> AiRuntimeConfig {
    let Ok(path) = get_app_data_file_standalone(AI_CONFIG_FILE) else {
        return AiRuntimeConfig::default();
    };
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<AiRuntimeConfig>(&raw).ok())
        .unwrap_or_default()
}

fn save_config_to_disk(cfg: &AiRuntimeConfig) -> Result<(), String> {
    let path = get_app_data_file_standalone(AI_CONFIG_FILE)?;
    let raw = serde_json::to_string_pretty(cfg)
        .map_err(|e| format!("Failed serializing AI runtime config: {}", e))?;
    atomic_write_string(&path, &raw)
}

fn pool_lock() -> &'static Mutex<AiRuntimeState> {
    AI_POOL.get_or_init(|| {
        Mutex::new(AiRuntimeState {
            config: load_config_from_disk(),
            workers: HashMap::new(),
        })
    })
}

pub fn ai_models_directory() -> PathBuf {
    if let Ok(p) = get_app_data_file_standalone("ai_models") {
        let _ = fs::create_dir_all(&p);
        return p;
    }
    let fallback = std::env::temp_dir().join("duke-launcher-ai-models");
    let _ = fs::create_dir_all(&fallback);
    fallback
}

/// Probes dynamically loadable Win32/WinML/ONNX DLLs using `LoadLibraryExW` / `FreeLibrary`
/// so nothing remains pinned in memory while idle.
#[cfg(target_os = "windows")]
fn can_dynamically_load_dll(candidates: &[PathBuf]) -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::FreeLibrary;
    use windows::Win32::System::LibraryLoader::{LoadLibraryExW, LOAD_LIBRARY_FLAGS};

    for path in candidates {
        let is_bare_name = path.components().count() == 1;
        if !is_bare_name && !path.exists() {
            continue;
        }
        let wide: Vec<u16> = OsStr::new(path.as_os_str())
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: Valid null-terminated UTF-16 DLL path, immediately freed after probing.
        if let Ok(hmod) = unsafe { LoadLibraryExW(PCWSTR(wide.as_ptr()), None, LOAD_LIBRARY_FLAGS(0)) } {
            unsafe {
                let _ = FreeLibrary(hmod);
            }
            return true;
        }
    }
    false
}

#[cfg(not(target_os = "windows"))]
fn can_dynamically_load_dll(candidates: &[PathBuf]) -> bool {
    candidates.iter().any(|p| p.exists())
}

#[cfg(target_os = "windows")]
fn probe_dxgi_gpu_and_npu() -> (String, bool, String) {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
    let mut gpu_name = "Unknown GPU".to_string();
    let mut npu_name = String::new();

    unsafe {
        if let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() {
            for idx in 0..12u32 {
                let Ok(adapter) = factory.EnumAdapters1(idx) else {
                    break;
                };
                if let Ok(desc) = adapter.GetDesc1() {
                    let end = desc
                        .Description
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(desc.Description.len());
                    let title = String::from_utf16_lossy(&desc.Description[..end])
                        .trim()
                        .to_string();
                    let lower = title.to_ascii_lowercase();
                    if lower.contains("npu")
                        || lower.contains("ipu")
                        || lower.contains("hexagon")
                        || lower.contains("neural")
                        || lower.contains("ai boost")
                        || lower.contains("xdna")
                    {
                        npu_name = title;
                    } else if !lower.contains("microsoft basic render")
                        && gpu_name == "Unknown GPU"
                        && !title.is_empty()
                    {
                        gpu_name = title;
                    }
                }
            }
        }
    }

    let sys_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
    let drivers_dir = PathBuf::from(&sys_root).join("System32").join("drivers");
    let sys32_dir = PathBuf::from(&sys_root).join("System32");

    if npu_name.is_empty() {
        if drivers_dir.join("amdxdna.sys").exists() || sys32_dir.join("amdxdna.dll").exists() {
            npu_name = "AMD Ryzen AI XDNA NPU".to_string();
        } else if sys32_dir.join("ze_intel_npu.dll").exists() || drivers_dir.join("intel_vpu.sys").exists() {
            npu_name = "Intel Core Ultra AI Boost NPU".to_string();
        } else if sys32_dir.join("QnnHtp.dll").exists() || drivers_dir.join("qcnnpu.sys").exists() {
            npu_name = "Qualcomm Hexagon NPU".to_string();
        }
    }

    let detected = !npu_name.is_empty();
    (gpu_name, detected, npu_name)
}

#[cfg(not(target_os = "windows"))]
fn probe_dxgi_gpu_and_npu() -> (String, bool, String) {
    ("Software Adapter".to_string(), false, String::new())
}

fn find_oneocr_candidates(model_dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![model_dir.join("oneocr.dll"), PathBuf::from("oneocr.dll")];
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let foundry_bin = PathBuf::from(&local_app_data)
            .join("Microsoft")
            .join("WindowsApps");
        out.push(foundry_bin.join("oneocr.dll"));
    }
    if let Ok(pf) = std::env::var("ProgramFiles") {
        let apps_dir = PathBuf::from(pf).join("WindowsApps");
        if let Ok(entries) = fs::read_dir(apps_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("Microsoft.ScreenSketch_") {
                    out.push(entry.path().join("SnippingTool").join("oneocr.dll"));
                    out.push(entry.path().join("oneocr.dll"));
                }
            }
        }
    }
    out
}

fn find_ort_candidates(model_dir: &Path, dll_name: &str) -> Vec<PathBuf> {
    let mut out = vec![model_dir.join(dll_name)];
    if let Ok(ort_env) = std::env::var("ORT_DYLIB_PATH") {
        if let Some(parent) = Path::new(&ort_env).parent() {
            out.push(parent.join(dll_name));
        }
    }
    let sys_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
    out.push(PathBuf::from(&sys_root).join("System32").join(dll_name));
    out.push(PathBuf::from(dll_name));
    out
}

#[cfg(target_os = "windows")]
fn probe_winrt_ocr_available() -> bool {
    use windows::Media::Ocr::OcrEngine;
    OcrEngine::TryCreateFromUserProfileLanguages().is_ok()
        || OcrEngine::AvailableRecognizerLanguages()
            .map(|langs| langs.Size().unwrap_or(0) > 0)
            .unwrap_or(false)
}

#[cfg(not(target_os = "windows"))]
fn probe_winrt_ocr_available() -> bool {
    false
}

fn http_json_loopback(
    method: &str,
    endpoint: &str,
    path_suffix: &str,
    body_json: Option<&str>,
    timeout_ms: u64,
) -> Option<serde_json::Value> {
    let stripped = endpoint
        .trim()
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let (host_port, base_path) = match stripped.split_once('/') {
        Some((hp, rest)) => (hp, format!("/{}", rest.trim_matches('/'))),
        None => (stripped, String::new()),
    };
    let req_path = format!("{}/{}", base_path.trim_end_matches('/'), path_suffix.trim_start_matches('/'));
    let addr = host_port.parse::<SocketAddr>().ok()?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(140)).ok()?;
    let _ = stream.set_read_timeout(Some(Duration::from_millis(timeout_ms)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(400)));

    let payload = body_json.unwrap_or("");
    let req = if method.eq_ignore_ascii_case("POST") {
        format!(
            "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            req_path,
            host_port,
            payload.len(),
            payload
        )
    } else {
        format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nAccept: application/json\r\nConnection: close\r\n\r\n",
            req_path, host_port
        )
    };
    stream.write_all(req.as_bytes()).ok()?;

    let mut raw = Vec::with_capacity(4096);
    let _ = stream.read_to_end(&mut raw);
    let text = String::from_utf8_lossy(&raw);
    let (head, body) = text.split_once("\r\n\r\n")?;
    let status_ok = head
        .lines()
        .next()
        .map(|l| l.contains(" 200 ") || l.contains(" 201 "))
        .unwrap_or(false);
    if !status_ok {
        return None;
    }
    let clean_body = if head.to_ascii_lowercase().contains("transfer-encoding: chunked") {
        body.lines()
            .find(|l| l.trim_start().starts_with('{'))
            .unwrap_or(body)
    } else {
        body
    };
    serde_json::from_str::<serde_json::Value>(clean_body.trim()).ok()
}

fn check_foundry_local_models(endpoint: &str) -> (bool, Vec<String>) {
    if let Some(val) = http_json_loopback("GET", endpoint, "/models", None, 450) {
        let ids: Vec<String> = val
            .get("data")
            .and_then(|d| d.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.get("id").and_then(|s| s.as_str()))
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();
        return (true, ids);
    }
    (false, Vec::new())
}

pub fn probe_hardware_ai(endpoint: &str) -> HardwareAiProbe {
    let model_dir = ai_models_directory();
    let installed_models = fs::read_dir(&model_dir)
        .ok()
        .map(|rd| {
            rd.flatten()
                .filter_map(|entry| {
                    let path = entry.path();
                    if path.is_file() {
                        path.file_name().map(|n| n.to_string_lossy().to_string())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let (gpu_name, mut npu_detected, mut npu_name) = probe_dxgi_gpu_and_npu();
    let onnxruntime_available = can_dynamically_load_dll(&find_ort_candidates(&model_dir, "onnxruntime.dll"));
    let qnn_available = can_dynamically_load_dll(&find_ort_candidates(&model_dir, "onnxruntime_providers_qnn.dll"))
        || can_dynamically_load_dll(&find_ort_candidates(&model_dir, "QnnHtp.dll"));
    let vitis_ai_available = can_dynamically_load_dll(&find_ort_candidates(&model_dir, "onnxruntime_providers_vitisai.dll"));
    let openvino_available = can_dynamically_load_dll(&find_ort_candidates(&model_dir, "onnxruntime_providers_openvino.dll"))
        || can_dynamically_load_dll(&find_ort_candidates(&model_dir, "ze_intel_npu.dll"));
    let winml_available = can_dynamically_load_dll(&find_ort_candidates(
        &model_dir,
        "Microsoft.Windows.AI.MachineLearning.dll",
    )) || can_dynamically_load_dll(&find_ort_candidates(&model_dir, "WinML.dll"));
    let directml_available = can_dynamically_load_dll(&find_ort_candidates(&model_dir, "DirectML.dll"));
    let oneocr_available = can_dynamically_load_dll(&find_oneocr_candidates(&model_dir));
    let winrt_ocr_available = probe_winrt_ocr_available();

    if !npu_detected && (qnn_available || vitis_ai_available || openvino_available) {
        npu_detected = true;
        npu_name = if qnn_available {
            "Qualcomm Hexagon NPU (QNN EP)".to_string()
        } else if vitis_ai_available {
            "AMD Ryzen AI NPU (VitisAI EP)".to_string()
        } else {
            "Intel AI Boost NPU (OpenVINO EP)".to_string()
        };
    }

    let (foundry_local_online, foundry_models) = check_foundry_local_models(endpoint);

    HardwareAiProbe {
        onnxruntime_available,
        qnn_available,
        vitis_ai_available,
        openvino_available,
        winml_available,
        directml_available,
        oneocr_available,
        winrt_ocr_available,
        npu_detected,
        npu_name,
        gpu_name,
        foundry_local_online,
        foundry_models,
        model_dir: model_dir.to_string_lossy().to_string(),
        installed_models,
    }
}

/// Strictly prioritizes `NPU (QNN / VitisAI / OpenVINO / WinML) -> DirectML -> CPU`
/// so gaming GPU VRAM stays 0% touched whenever NPU acceleration exists.
pub fn select_execution_provider(probe: &HardwareAiProbe, preferred: &str) -> ExecutionProviderKind {
    match preferred.trim().to_ascii_lowercase().as_str() {
        "cpu" => return ExecutionProviderKind::Cpu,
        "directml" => {
            if probe.directml_available {
                return ExecutionProviderKind::DirectMl;
            }
            return ExecutionProviderKind::Cpu;
        }
        _ => {}
    }

    if probe.qnn_available {
        return ExecutionProviderKind::NpuQnn;
    }
    if probe.vitis_ai_available {
        return ExecutionProviderKind::NpuVitisAi;
    }
    if probe.openvino_available {
        return ExecutionProviderKind::NpuOpenVino;
    }
    if probe.npu_detected && (probe.winml_available || probe.directml_available || probe.onnxruntime_available) {
        return ExecutionProviderKind::NpuWinMl;
    }
    if probe.directml_available {
        return ExecutionProviderKind::DirectMl;
    }
    ExecutionProviderKind::Cpu
}

/// Lightweight RAII handle for temporarily binding a named worker slot without keeping
/// ONNX Runtime or heavy model allocations pinned at idle.
pub struct DynamicOrtSessionLease {
    pub ep: ExecutionProviderKind,
    pub model_name: String,
    pub onnxruntime_bound: bool,
    #[cfg(target_os = "windows")]
    ort_module: Option<isize>,
}

impl DynamicOrtSessionLease {
    pub fn ep_label(&self) -> &'static str {
        self.ep.label()
    }
}

#[cfg(target_os = "windows")]
impl Drop for DynamicOrtSessionLease {
    fn drop(&mut self) {
        if let Some(raw_handle) = self.ort_module.take() {
            if raw_handle != 0 {
                use windows::Win32::Foundation::{FreeLibrary, HMODULE};
                // SAFETY: `raw_handle` was obtained from `LoadLibraryExW` and not yet freed.
                unsafe {
                    let _ = FreeLibrary(HMODULE(raw_handle as *mut c_void));
                }
            }
        }
    }
}

/// Acquires an execution-provider lease for a specific Wave 3 AI worker (`vision`, `comms`,
/// `doctor`, `director`), recording lightweight telemetry and resolving any local `.onnx` file.
pub fn acquire_worker_lease(worker_name: &str, model_filename: &str) -> DynamicOrtSessionLease {
    let cfg = {
        let mut guard = pool_lock().lock().unwrap_or_else(|e| e.into_inner());
        purge_expired_workers_locked(&mut guard, false);
        guard.config.clone()
    };

    let probe = probe_hardware_ai(&cfg.foundry_endpoint);
    let ep = select_execution_provider(&probe, &cfg.preferred_provider);
    let model_path = ai_models_directory().join(model_filename);
    let model_present = model_path.is_file();

    #[cfg(target_os = "windows")]
    let (ort_module, ort_ok) = if model_present && probe.onnxruntime_available {
        use windows::core::PCWSTR;
        use windows::Win32::System::LibraryLoader::{LoadLibraryExW, LOAD_LIBRARY_FLAGS};
        let mut loaded = None;
        for cand in find_ort_candidates(&ai_models_directory(), "onnxruntime.dll") {
            let is_bare = cand.components().count() == 1;
            if !is_bare && !cand.exists() {
                continue;
            }
            let wide: Vec<u16> = OsStr::new(cand.as_os_str())
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            if let Ok(hmod) = unsafe { LoadLibraryExW(PCWSTR(wide.as_ptr()), None, LOAD_LIBRARY_FLAGS(0)) } {
                loaded = Some(hmod.0 as isize);
                break;
            }
        }
        let ok = loaded.is_some();
        (loaded, ok)
    } else {
        (None, false)
    };

    #[cfg(not(target_os = "windows"))]
    let ort_ok = model_present && probe.onnxruntime_available;

    let active_model = if ort_ok {
        format!("{} (ONNX)", model_filename)
    } else if probe.foundry_local_online && !cfg.foundry_model.is_empty() {
        format!("Foundry: {}", cfg.foundry_model)
    } else if !probe.foundry_models.is_empty() {
        format!("Foundry: {}", probe.foundry_models[0])
    } else {
        format!("{} (Native)", model_filename)
    };

    let estimated_ram_mb = if ort_ok { 14.0 } else { 1.5 };
    let vram_allocated_mb = ep.vram_allocated_mb();

    if let Ok(mut guard) = pool_lock().lock() {
        guard.workers.insert(
            worker_name.to_string(),
            WorkerAllocation {
                name: worker_name.to_string(),
                execution_provider: ep.label().to_string(),
                active_model: active_model.clone(),
                last_used_ms: now_ms(),
                estimated_ram_mb,
                vram_allocated_mb,
            },
        );
    }

    DynamicOrtSessionLease {
        ep,
        model_name: active_model,
        onnxruntime_bound: ort_ok,
        #[cfg(target_os = "windows")]
        ort_module,
    }
}

fn purge_expired_workers_locked(state: &mut AiRuntimeState, force: bool) -> usize {
    if !force && !state.config.auto_unload_enabled {
        return 0;
    }
    let ttl_ms = state.config.idle_ttl_secs.clamp(10, 600) * 1000;
    let now = now_ms();
    let before = state.workers.len();
    state
        .workers
        .retain(|_, alloc| !force && now.saturating_sub(alloc.last_used_ms) < ttl_ms);
    before.saturating_sub(state.workers.len())
}

pub fn unload_idle_workers_internal(force: bool) -> usize {
    let removed = if let Ok(mut guard) = pool_lock().lock() {
        purge_expired_workers_locked(&mut guard, force)
    } else {
        0
    };
    if removed > 0 || force {
        trim_current_working_set();
    }
    removed
}

/// Calls a local Foundry Local OpenAI-compatible HTTP endpoint when available,
/// returning `(response_text, backend_name)`. Returns `None` promptly if offline.
pub fn query_foundry_local_chat(system_prompt: &str, user_prompt: &str) -> Option<(String, String)> {
    let cfg = pool_lock()
        .lock()
        .map(|g| g.config.clone())
        .unwrap_or_default();
    let (online, models) = check_foundry_local_models(&cfg.foundry_endpoint);
    if !online {
        return None;
    }
    let model_id = if !cfg.foundry_model.trim().is_empty() {
        cfg.foundry_model.clone()
    } else {
        models.first().cloned().unwrap_or_else(|| "phi-3.5-mini-instruct".to_string())
    };

    let payload = serde_json::json!({
        "model": model_id,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_prompt }
        ],
        "temperature": 0.2,
        "max_tokens": 220
    });

    let body_str = serde_json::to_string(&payload).ok()?;
    let json = http_json_loopback(
        "POST",
        &cfg.foundry_endpoint,
        "/chat/completions",
        Some(&body_str),
        2800,
    )?;
    let text = json
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|msg| msg.get("content"))
        .and_then(|s| s.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())?;

    Some((text, format!("Foundry Local ({})", model_id)))
}

#[tauri::command]
pub async fn get_ai_runtime_status() -> Result<AiRuntimeStatus, String> {
    tokio::task::spawn_blocking(|| {
        let (cfg, loaded_workers) = {
            let mut guard = pool_lock().lock().unwrap_or_else(|e| e.into_inner());
            let _ = purge_expired_workers_locked(&mut guard, false);
            let workers: Vec<WorkerAllocation> = guard.workers.values().cloned().collect();
            (guard.config.clone(), workers)
        };
        let probe = probe_hardware_ai(&cfg.foundry_endpoint);
        let ep = select_execution_provider(&probe, &cfg.preferred_provider);
        let worker_ram_mb: f32 = loaded_workers.iter().map(|w| w.estimated_ram_mb).sum();
        let vram_allocated_mb: f32 = if loaded_workers.is_empty() {
            0.0
        } else {
            ep.vram_allocated_mb()
        };

        Ok(AiRuntimeStatus {
            config: cfg,
            probe,
            active_execution_provider: ep.label().to_string(),
            zero_vram_mode: ep.is_npu() || ep == ExecutionProviderKind::Cpu,
            vram_allocated_mb,
            worker_ram_mb,
            process_working_set_mb: get_current_working_set_mb(),
            loaded_workers,
        })
    })
    .await
    .map_err(|e| format!("AI runtime status task panicked: {}", e))?
}

#[tauri::command]
pub async fn update_ai_runtime_config(
    preferred_provider: Option<String>,
    auto_unload_enabled: Option<bool>,
    idle_ttl_secs: Option<u64>,
    foundry_endpoint: Option<String>,
    foundry_model: Option<String>,
    target_language: Option<String>,
) -> Result<AiRuntimeConfig, String> {
    let updated = {
        let mut guard = pool_lock().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(p) = preferred_provider {
            let norm = p.trim().to_ascii_lowercase();
            if matches!(norm.as_str(), "auto" | "npu" | "directml" | "cpu") {
                guard.config.preferred_provider = norm;
            }
        }
        if let Some(flag) = auto_unload_enabled {
            guard.config.auto_unload_enabled = flag;
        }
        if let Some(ttl) = idle_ttl_secs {
            guard.config.idle_ttl_secs = ttl.clamp(10, 600);
        }
        if let Some(ep_url) = foundry_endpoint {
            let trimmed = ep_url.trim().to_string();
            if !trimmed.is_empty() {
                guard.config.foundry_endpoint = trimmed;
            }
        }
        if let Some(m) = foundry_model {
            guard.config.foundry_model = m.trim().to_string();
        }
        if let Some(lang) = target_language {
            let clean = lang.trim().to_ascii_lowercase();
            if !clean.is_empty() {
                guard.config.target_language = clean;
            }
        }
        guard.config.clone()
    };
    save_config_to_disk(&updated)?;
    Ok(updated)
}

#[tauri::command]
pub async fn unload_ai_workers() -> Result<AiRuntimeStatus, String> {
    let _ = unload_idle_workers_internal(true);
    get_ai_runtime_status().await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_probe(qnn: bool, vitis: bool, openvino: bool, winml: bool, dml: bool) -> HardwareAiProbe {
        HardwareAiProbe {
            onnxruntime_available: true,
            qnn_available: qnn,
            vitis_ai_available: vitis,
            openvino_available: openvino,
            winml_available: winml,
            directml_available: dml,
            oneocr_available: false,
            winrt_ocr_available: true,
            npu_detected: qnn || vitis || openvino || winml,
            npu_name: "Test NPU".to_string(),
            gpu_name: "Test GPU".to_string(),
            foundry_local_online: false,
            foundry_models: Vec::new(),
            model_dir: ".".to_string(),
            installed_models: Vec::new(),
        }
    }

    #[test]
    fn execution_provider_priority_keeps_zero_vram_on_npu() {
        let p_qnn = mock_probe(true, true, true, true, true);
        let ep = select_execution_provider(&p_qnn, "auto");
        assert_eq!(ep, ExecutionProviderKind::NpuQnn);
        assert_eq!(ep.vram_allocated_mb(), 0.0);

        let p_amd = mock_probe(false, true, false, false, true);
        let ep_amd = select_execution_provider(&p_amd, "auto");
        assert_eq!(ep_amd, ExecutionProviderKind::NpuVitisAi);
        assert_eq!(ep_amd.vram_allocated_mb(), 0.0);

        let p_intel = mock_probe(false, false, true, false, true);
        let ep_intel = select_execution_provider(&p_intel, "auto");
        assert_eq!(ep_intel, ExecutionProviderKind::NpuOpenVino);
        assert_eq!(ep_intel.vram_allocated_mb(), 0.0);

        let p_dml_only = mock_probe(false, false, false, false, true);
        assert_eq!(
            select_execution_provider(&p_dml_only, "auto"),
            ExecutionProviderKind::DirectMl
        );

        let p_none = mock_probe(false, false, false, false, false);
        let ep_cpu = select_execution_provider(&p_none, "auto");
        assert_eq!(ep_cpu, ExecutionProviderKind::Cpu);
        assert_eq!(ep_cpu.vram_allocated_mb(), 0.0);
    }

    #[test]
    fn worker_pool_unloads_cleanly_at_idle() {
        let _lease = acquire_worker_lease("test_worker", "silero_vad.onnx");
        let unloaded = unload_idle_workers_internal(true);
        assert!(unloaded >= 1);
    }
}

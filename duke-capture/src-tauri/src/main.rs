// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use duke_core::pipe_ipc::{DukeBus, DukeEvent};
use duke_core::shm_bus::{DukeShmBus, ShmEventType, ShmSourceApp};
use duke_core::tray_daemon::{handle_run_event, install_standard_tray, TrayDaemonConfig, TrayDaemonState};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::ffi::c_void;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, Url};
use tokio::io::AsyncWriteExt;
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

#[cfg(target_os = "windows")]
use windows::core::{Interface, GUID, PCWSTR, PWSTR};
#[cfg(target_os = "windows")]
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{
    CloseHandle, HANDLE, HINSTANCE, HMODULE, LPARAM, LRESULT, WPARAM,
};
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0,
};
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE,
    D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIDevice, IDXGIFactory1, IDXGIOutput1, IDXGIOutputDuplication,
    DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO,
};
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
};
#[cfg(target_os = "windows")]
use windows::Win32::Media::Audio::{
    eCapture, eConsole, eMultimedia, eRender, IAudioCaptureClient, IAudioClient, IMMDevice,
    IMMDeviceEnumerator, MMDeviceEnumerator, AUDCLNT_SHAREMODE_SHARED,
    AUDCLNT_STREAMFLAGS_LOOPBACK, DEVICE_STATE_ACTIVE, WAVEFORMATEX, WAVEFORMATEXTENSIBLE,
};
#[cfg(target_os = "windows")]
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFSample, IMFSinkWriter, IMFSourceReader, MFAudioFormat_AAC,
    MFAudioFormat_PCM, MFCreateAttributes, MFCreateMFByteStreamOnStream, MFCreateMediaType,
    MFCreateMemoryBuffer, MFCreateSample, MFCreateSinkWriterFromURL,
    MFCreateSourceReaderFromByteStream, MFCreateSourceReaderFromURL,
    MFFrameRateToAverageTimePerFrame, MFMediaType_Audio, MFMediaType_Video, MFShutdown,
    MFStartup, MFTranscodeContainerType_MPEG4, MFVideoFormat_H264, MFVideoFormat_RGB32,
    MFVideoInterlace_Progressive, MF_API_VERSION, MF_MT_AUDIO_AVG_BYTES_PER_SECOND,
    MF_MT_AUDIO_BITS_PER_SAMPLE, MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND,
    MF_MT_AVG_BITRATE, MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE,
    MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SUBTYPE,
    MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, MF_SINK_WRITER_DISABLE_THROTTLING,
    MF_SOURCE_READERF_ENDOFSTREAM, MF_TRANSCODE_CONTAINERTYPE, MFSTARTUP_FULL,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::Com::StructuredStorage::PropVariantClear;
#[cfg(target_os = "windows")]
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IStream, CLSCTX_ALL,
    COINIT_MULTITHREADED, STATFLAG_NONAME, STATSTG, STREAM_SEEK_SET, STGM_READ,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE, GMEM_ZEROINIT};
#[cfg(target_os = "windows")]
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
#[cfg(target_os = "windows")]
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL,
    MOD_NOREPEAT, MOD_SHIFT,
};
#[cfg(target_os = "windows")]
use windows::Win32::UI::Shell::{DROPFILES, SHCreateMemStream};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetForegroundWindow, GetMessageW, GetSystemMetrics,
    GetWindowTextW, GetWindowThreadProcessId, PeekMessageW, PostThreadMessageW,
    SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG,
    MSLLHOOKSTRUCT, PM_NOREMOVE, SM_CXSCREEN, SM_CYSCREEN, WH_KEYBOARD_LL, WH_MOUSE_LL,
    WM_APP, WM_HOTKEY, WM_KEYDOWN, WM_QUIT, WM_SYSKEYDOWN, WM_XBUTTONDOWN,
};

const CREATE_NO_WINDOW: u32 = 0x08000000;
const WM_REBIND_HOTKEY: u32 = WM_APP + 42;
const HOTKEY_ID: i32 = 0x444B;
const MF_SOURCE_READER_FIRST_VIDEO_STREAM: u32 = 0xFFFF_FFFC;
const MF_SOURCE_READER_FIRST_AUDIO_STREAM: u32 = 0xFFFF_FFFD;
const MF_SOURCE_READER_ANY_STREAM: u32 = 0xFFFF_FFFE;
const CF_HDROP_FORMAT: u32 = 15;
const SND_ASYNC: u32 = 0x0001;
const SND_NODEFAULT: u32 = 0x0002;
const SND_MEMORY: u32 = 0x0004;
const AUDIO_SAMPLE_RATE: u32 = 48_000;
const AUDIO_CHANNELS: u32 = 2;
const SEGMENT_DURATION_MS: u64 = 2000;

const PRIVACY_USER_AGENTS: &[&str] = &[
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:135.0) Gecko/20100101 Firefox/135.0",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36",
];

#[cfg(target_os = "windows")]
#[link(name = "winmm")]
extern "system" {
    fn PlaySoundW(pszSound: *const u8, hmod: *mut c_void, fdwSound: u32) -> i32;
}

// ======================== GLOBAL SINGLETONS & ATOMICS ========================

static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();
static DUKE_BUS: OnceLock<DukeBus> = OnceLock::new();
static DUKE_SHM_BUS: OnceLock<Option<DukeShmBus>> = OnceLock::new();
static SHM_STOP_SIGNAL: AtomicBool = AtomicBool::new(false);
static ACTIVE_LAUNCHER_GAME: OnceLock<Mutex<Option<(u32, String)>>> = OnceLock::new();
static CONFIG_STATE: OnceLock<Mutex<CaptureConfig>> = OnceLock::new();
static RING_BUFFER: OnceLock<Mutex<VecDeque<EncodedSegment>>> = OnceLock::new();
static AUDIO_MIX_HUB: OnceLock<Arc<Mutex<AudioMixerBuffers>>> = OnceLock::new();
static HARDWARE_PROBE: OnceLock<Mutex<HardwareCapabilities>> = OnceLock::new();
static HOTKEY_STATE: OnceLock<Mutex<HotkeyBinding>> = OnceLock::new();
static HOTKEY_THREAD_ID: AtomicU32 = AtomicU32::new(0);
static ACTIVE_CHIME_WAV: OnceLock<Mutex<Vec<u8>>> = OnceLock::new();

static REPLAY_RUNNING: AtomicBool = AtomicBool::new(false);
static AUDIO_STOP_SIGNAL: AtomicBool = AtomicBool::new(false);
static CAPTURE_STOP_SIGNAL: AtomicBool = AtomicBool::new(false);
static AUTO_TRIM_ENABLED: AtomicBool = AtomicBool::new(true);
static SYSTEM_LEVEL_PCT: AtomicU32 = AtomicU32::new(0);
static MIC_LEVEL_PCT: AtomicU32 = AtomicU32::new(0);
static LAST_HOTKEY_MS: AtomicU64 = AtomicU64::new(0);
static ACTIVE_SAVE_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

static MIC_MUTED: AtomicBool = AtomicBool::new(false);
static NOISE_GATE_ENABLED: AtomicBool = AtomicBool::new(true);
static SYSTEM_VOLUME_PCT: AtomicU32 = AtomicU32::new(100);
static MIC_GAIN_PCT: AtomicU32 = AtomicU32::new(100);
static CHIME_VOLUME_PCT: AtomicU32 = AtomicU32::new(85);
static AUDIO_GENERATION: AtomicU64 = AtomicU64::new(1);

static DOH_BOOTSTRAP_CLIENT: OnceLock<Client> = OnceLock::new();
static DOH_CACHE: OnceLock<Mutex<HashMap<String, Vec<SocketAddr>>>> = OnceLock::new();
static SESSION_UA_CURSOR: AtomicU64 = AtomicU64::new(0);

// ======================== SERIALIZABLE STRUCTS ========================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyBinding {
    pub label: String,
    pub vk: u32,
    pub modifiers: u32,
    pub is_mouse_xbutton: Option<u16>,
}

impl Default for HotkeyBinding {
    fn default() -> Self {
        Self {
            label: "F8".to_string(),
            vk: 0x77, // VK_F8
            modifiers: 0,
            is_mouse_xbutton: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureConfig {
    pub buffer_seconds: u32,
    pub fps: u32,
    pub resolution: String,
    pub bitrate_kbps: u32,
    pub encoder_preference: String,
    pub clips_dir: String,
    pub mic_device_id: Option<String>,
    pub system_volume_pct: u32,
    pub mic_gain_pct: u32,
    pub mic_muted: bool,
    pub noise_gate_enabled: bool,
    pub chime_volume_pct: u32,
    pub hotkey: HotkeyBinding,
    pub auto_trim_enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareCapabilities {
    pub active_backend: String,
    pub available_encoders: Vec<String>,
    pub gpu_adapter_name: String,
    pub ffmpeg_installed: bool,
    pub ffmpeg_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureStateDto {
    pub replay_running: bool,
    pub status_text: String,
    pub buffer_seconds: u32,
    pub buffered_segments: usize,
    pub buffered_seconds_actual: f64,
    pub buffered_ram_mb: f64,
    pub fps: u32,
    pub resolution: String,
    pub bitrate_kbps: u32,
    pub encoder_preference: String,
    pub active_backend: String,
    pub available_encoders: Vec<String>,
    pub ffmpeg_installed: bool,
    pub ffmpeg_path: Option<String>,
    pub clips_dir: String,
    pub active_game: String,
    pub hotkey: HotkeyBinding,
    pub system_level_pct: u32,
    pub mic_level_pct: u32,
    pub mic_device_id: Option<String>,
    pub system_volume_pct: u32,
    pub mic_gain_pct: u32,
    pub mic_muted: bool,
    pub noise_gate_enabled: bool,
    pub chime_volume_pct: u32,
    pub own_working_set_mb: f64,
    pub doh_shield: String,
    pub eco_scheduler: String,
    pub auto_trim_enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioInputDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipEntry {
    pub id: String,
    pub file_name: String,
    pub full_path: String,
    pub game_name: String,
    pub size_bytes: u64,
    pub duration_secs: f64,
    pub created_at: u64,
}

#[derive(Clone)]
struct EncodedSegment {
    duration_ms: u64,
    bytes: Vec<u8>,
}

struct AudioMixerBuffers {
    sys_samples: VecDeque<f32>,
    mic_samples: VecDeque<f32>,
}

struct RawSegmentInput {
    width: u32,
    height: u32,
    fps: u32,
    bitrate_bps: u32,
    frames_bgra: Vec<Vec<u8>>,
    audio_stereo_i16: Vec<i16>,
    duration_ms: u64,
}

// ======================== CORE UTILS & PORTABLE PATHS ========================

fn get_hotkey_lock() -> &'static Mutex<HotkeyBinding> {
    HOTKEY_STATE.get_or_init(|| Mutex::new(HotkeyBinding::default()))
}

fn get_ring_buffer() -> &'static Mutex<VecDeque<EncodedSegment>> {
    RING_BUFFER.get_or_init(|| Mutex::new(VecDeque::with_capacity(64)))
}

fn get_audio_mix_hub() -> Arc<Mutex<AudioMixerBuffers>> {
    AUDIO_MIX_HUB
        .get_or_init(|| {
            Arc::new(Mutex::new(AudioMixerBuffers {
                sys_samples: VecDeque::with_capacity(AUDIO_SAMPLE_RATE as usize * 8),
                mic_samples: VecDeque::with_capacity(AUDIO_SAMPLE_RATE as usize * 8),
            }))
        })
        .clone()
}

fn resolve_portable_data_dir() -> Option<PathBuf> {
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let flag = exe_dir.join("portable.flag");
            let data_dir = exe_dir.join("data");
            if flag.exists() || data_dir.exists() {
                let _ = fs::create_dir_all(&data_dir);
                return Some(data_dir);
            }
        }
    }
    None
}

fn resolve_app_data_dir(app: &AppHandle) -> PathBuf {
    if let Some(p) = resolve_portable_data_dir() {
        return p;
    }
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("DukeCapture"))
}

fn resolve_default_clips_dir(app: Option<&AppHandle>) -> PathBuf {
    if let Some(handle) = app {
        if let Ok(vid) = handle.path().video_dir() {
            return vid.join("Duke Clips");
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(hkcu) = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Shell Folders")
        {
            if let Ok(my_video) = hkcu.get_value::<String, _>("My Video") {
                if !my_video.trim().is_empty() {
                    return PathBuf::from(my_video).join("Duke Clips");
                }
            }
        }
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        return PathBuf::from(profile).join("Videos").join("Duke Clips");
    }
    std::env::temp_dir().join("Duke Clips")
}

fn default_config(app: Option<&AppHandle>) -> CaptureConfig {
    CaptureConfig {
        buffer_seconds: 30,
        fps: 60,
        resolution: "1080p".to_string(),
        bitrate_kbps: 12000,
        encoder_preference: "auto".to_string(),
        clips_dir: resolve_default_clips_dir(app).to_string_lossy().to_string(),
        mic_device_id: None,
        system_volume_pct: 100,
        mic_gain_pct: 100,
        mic_muted: false,
        noise_gate_enabled: true,
        chime_volume_pct: 85,
        hotkey: HotkeyBinding::default(),
        auto_trim_enabled: true,
    }
}

fn get_config_lock(app: Option<&AppHandle>) -> &'static Mutex<CaptureConfig> {
    CONFIG_STATE.get_or_init(|| {
        let mut cfg = default_config(app);
        if let Some(handle) = app {
            let path = resolve_app_data_dir(handle).join("config.json");
            if let Ok(raw) = fs::read_to_string(&path) {
                if let Ok(loaded) = serde_json::from_str::<CaptureConfig>(&raw) {
                    cfg = loaded;
                }
            }
        }
        sync_atomics_from_config(&cfg);
        Mutex::new(cfg)
    })
}

fn persist_config(app: &AppHandle, cfg: &CaptureConfig) {
    let dir = resolve_app_data_dir(app);
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("config.json");
    if let Ok(json) = serde_json::to_string_pretty(cfg) {
        let _ = fs::write(path, json);
    }
}

fn sync_atomics_from_config(cfg: &CaptureConfig) {
    SYSTEM_VOLUME_PCT.store(cfg.system_volume_pct.min(150), Ordering::Relaxed);
    MIC_GAIN_PCT.store(cfg.mic_gain_pct.min(200), Ordering::Relaxed);
    MIC_MUTED.store(cfg.mic_muted, Ordering::Relaxed);
    NOISE_GATE_ENABLED.store(cfg.noise_gate_enabled, Ordering::Relaxed);
    CHIME_VOLUME_PCT.store(cfg.chime_volume_pct.min(100), Ordering::Relaxed);
    AUTO_TRIM_ENABLED.store(cfg.auto_trim_enabled, Ordering::Relaxed);
    if let Ok(mut hk) = get_hotkey_lock().lock() {
        *hk = cfg.hotkey.clone();
    }
}

fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn trim_process_working_set() {
    duke_core::win32_ffi::trim_current_working_set();
}

fn get_process_working_set_mb() -> f64 {
    duke_core::win32_ffi::get_current_working_set_mb()
}

fn active_launcher_game_lock() -> &'static Mutex<Option<(u32, String)>> {
    ACTIVE_LAUNCHER_GAME.get_or_init(|| Mutex::new(None))
}

fn resolve_active_game_tag() -> String {
    if let Ok(guard) = active_launcher_game_lock().lock() {
        if let Some((_, ref title)) = *guard {
            if !title.trim().is_empty() {
                return title.trim().to_string();
            }
        }
    }
    detect_foreground_game_name()
}

fn unblock_ntfs_zone_identifier(path: &Path) {
    let mut os_str = path.as_os_str().to_os_string();
    os_str.push(":Zone.Identifier");
    let _ = fs::remove_file(PathBuf::from(os_str));
}

fn sanitize_filename_part(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            _ => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.');
    if trimmed.is_empty() {
        "Desktop".to_string()
    } else {
        trimmed.chars().take(48).collect()
    }
}

fn format_timestamp_filename(epoch_secs: u64) -> String {
    // Civil date conversion from Unix epoch (Howard Hinnant algorithm)
    let days = (epoch_secs / 86400) as i64;
    let rem_secs = epoch_secs % 86400;
    let hour = rem_secs / 3600;
    let min = (rem_secs % 3600) / 60;
    let sec = rem_secs % 60;

    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    format!(
        "{:04}{:02}{:02}_{:02}{:02}{:02}",
        year, m, d, hour, min, sec
    )
}

// ======================== ENCRYPTED DoH CLIENT ========================

fn get_doh_cache() -> &'static Mutex<HashMap<String, Vec<SocketAddr>>> {
    DOH_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn get_doh_bootstrap_client() -> Client {
    DOH_BOOTSTRAP_CLIENT
        .get_or_init(|| {
            Client::builder()
                .timeout(Duration::from_secs(6))
                .build()
                .unwrap_or_else(|_| Client::new())
        })
        .clone()
}

fn next_user_agent() -> HeaderValue {
    let idx = (SESSION_UA_CURSOR.fetch_add(1, Ordering::Relaxed) as usize) % PRIVACY_USER_AGENTS.len();
    HeaderValue::from_static(PRIVACY_USER_AGENTS[idx])
}

async fn resolve_host_doh(host: &str) -> Option<Vec<SocketAddr>> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Some(vec![SocketAddr::new(ip, 443)]);
    }
    if let Ok(cache) = get_doh_cache().lock() {
        if let Some(addrs) = cache.get(host) {
            return Some(addrs.clone());
        }
    }
    let client = get_doh_bootstrap_client();
    let encoded = urlencoding::encode(host);
    let endpoints = [
        format!("https://1.1.1.1/dns-query?name={}&type=A", encoded),
        format!("https://1.0.0.1/dns-query?name={}&type=A", encoded),
        format!("https://9.9.9.9:5053/dns-query?name={}&type=A", encoded),
    ];
    for ep in endpoints {
        if let Ok(resp) = client
            .get(&ep)
            .header(ACCEPT, "application/dns-json")
            .send()
            .await
        {
            if !resp.status().is_success() {
                continue;
            }
            if let Ok(json) = resp.json::<Value>().await {
                if let Some(answers) = json.get("Answer").and_then(|v| v.as_array()) {
                    let mut out = Vec::new();
                    for ans in answers {
                        if ans.get("type").and_then(|t| t.as_u64()) == Some(1) {
                            if let Some(data) = ans.get("data").and_then(|d| d.as_str()) {
                                if let Ok(v4) = data.parse::<Ipv4Addr>() {
                                    out.push(SocketAddr::new(IpAddr::V4(v4), 443));
                                }
                            }
                        }
                    }
                    if !out.is_empty() {
                        if let Ok(mut cache) = get_doh_cache().lock() {
                            cache.insert(host.to_string(), out.clone());
                        }
                        return Some(out);
                    }
                }
            }
        }
    }
    None
}

async fn build_doh_client_for_url(raw_url: &str) -> Client {
    let mut headers = HeaderMap::new();
    headers.insert("User-Agent", next_user_agent());
    let mut builder = Client::builder()
        .default_headers(headers)
        .timeout(Duration::from_secs(120));
    if let Ok(parsed) = Url::parse(raw_url) {
        if let Some(host) = parsed.host_str() {
            if let Some(addrs) = resolve_host_doh(host).await {
                let port = parsed.port_or_known_default().unwrap_or(443);
                let socket_addrs: Vec<SocketAddr> = addrs
                    .into_iter()
                    .map(|mut sa| {
                        sa.set_port(port);
                        sa
                    })
                    .collect();
                builder = builder.resolve_to_addrs(host, &socket_addrs);
            }
        }
    }
    builder.build().unwrap_or_else(|_| Client::new())
}

// ======================== CONFIRMATION CHIME (SYNTHESIZED WAV) ========================

fn generate_harmonic_chime_wav(volume_pct: u32) -> Vec<u8> {
    let sample_rate: u32 = 44100;
    let channels: u16 = 2;
    let bits_per_sample: u16 = 16;
    let duration_secs = 0.30_f32;
    let total_frames = (sample_rate as f32 * duration_secs) as usize;
    let data_bytes = (total_frames * channels as usize * 2) as u32;
    let riff_size = 36 + data_bytes;
    let gain = (volume_pct.min(100) as f32 / 100.0) * 0.62;

    let mut wav = Vec::with_capacity(44 + data_bytes as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&riff_size.to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    let byte_rate = sample_rate * channels as u32 * 2;
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&(channels * 2).to_le_bytes());
    wav.extend_from_slice(&bits_per_sample.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes.to_le_bytes());

    let two_pi = std::f32::consts::PI * 2.0;
    for i in 0..total_frames {
        let t = i as f32 / sample_rate as f32;
        // Two crisp harmonic notes: 1046.50 Hz (C6) -> 1567.98 Hz (G6) with shimmer octave
        let env1 = if t < 0.005 {
            t / 0.005
        } else {
            (-(t - 0.005) * 18.0).exp()
        };
        let note1 = ((two_pi * 1046.50 * t).sin() + 0.35 * (two_pi * 2093.0 * t).sin()) * env1;

        let t2 = t - 0.075;
        let note2 = if t2 >= 0.0 {
            let env2 = if t2 < 0.005 {
                t2 / 0.005
            } else {
                (-(t2 - 0.005) * 14.0).exp()
            };
            ((two_pi * 1567.98 * t2).sin() + 0.30 * (two_pi * 3135.96 * t2).sin()) * env2
        } else {
            0.0
        };

        let sample_f = ((note1 * 0.52 + note2 * 0.65) * gain).clamp(-0.98, 0.98);
        let sample_i = (sample_f * i16::MAX as f32) as i16;
        let bytes = sample_i.to_le_bytes();
        wav.extend_from_slice(&bytes);
        wav.extend_from_slice(&bytes);
    }
    wav
}

fn play_confirmation_chime(volume_pct: u32) {
    if volume_pct == 0 {
        return;
    }
    #[cfg(target_os = "windows")]
    {
        let lock = ACTIVE_CHIME_WAV.get_or_init(|| Mutex::new(Vec::new()));
        if let Ok(mut guard) = lock.lock() {
            unsafe {
                // Stop any overlapping chime cleanly before swapping buffer
                let _ = PlaySoundW(std::ptr::null(), std::ptr::null_mut(), 0);
                *guard = generate_harmonic_chime_wav(volume_pct);
                let _ = PlaySoundW(
                    guard.as_ptr(),
                    std::ptr::null_mut(),
                    SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
                );
            }
        }
    }
}

// ======================== FOREGROUND GAME AUTO-TAGGING ========================

fn map_friendly_game_name(raw_stem: &str, window_title: &str) -> String {
    let lower = raw_stem.to_lowercase();
    let known = match lower.as_str() {
        "cs2" => Some("Counter-Strike 2"),
        "valorant-win64-shipping" | "valorant" => Some("Valorant"),
        "r5apex" | "r5apex_dx12" => Some("Apex Legends"),
        "fortniteclient-win64-shipping" => Some("Fortnite"),
        "cod" | "modernwarfare" | "blackops6" => Some("Call of Duty"),
        "overwatch" => Some("Overwatch 2"),
        "rocketleague" => Some("Rocket League"),
        "league of legends" => Some("League of Legends"),
        "dota2" => Some("Dota 2"),
        "eldenring" => Some("Elden Ring"),
        "cyberpunk2077" => Some("Cyberpunk 2077"),
        "tslgame" => Some("PUBG"),
        "rainbowsix" | "rainbowsix_vulkan" | "rainbowsix_dx11" => Some("Rainbow Six Siege"),
        "destiny2" => Some("Destiny 2"),
        "helldivers2" => Some("Helldivers 2"),
        "gtav" | "gta5" | "gta5_enhanced" => Some("Grand Theft Auto V"),
        "minecraft.windows" => Some("Minecraft"),
        "javaw" | "java" => {
            if window_title.to_lowercase().contains("minecraft") {
                Some("Minecraft")
            } else {
                None
            }
        }
        "duke-capture" => Some("Desktop"),
        _ => None,
    };

    if let Some(mapped) = known {
        return mapped.to_string();
    }

    if lower.contains("win64-shipping") {
        let stripped = raw_stem
            .trim_end_matches("-Win64-Shipping")
            .trim_end_matches("-win64-shipping");
        if !stripped.is_empty() {
            return sanitize_filename_part(stripped);
        }
    }

    let skip_shells = ["explorer", "searchhost", "applicationframehost", "shellexperiencehost"];
    if skip_shells.contains(&lower.as_str()) {
        return "Desktop".to_string();
    }

    sanitize_filename_part(raw_stem)
}

fn detect_foreground_game_name() -> String {
    #[cfg(target_os = "windows")]
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return "Desktop".to_string();
        }
        let mut pid: u32 = 0;
        let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return "Desktop".to_string();
        }
        let mut title_buf = [0u16; 256];
        let title_len = GetWindowTextW(hwnd, &mut title_buf);
        let title = if title_len > 0 {
            String::from_utf16_lossy(&title_buf[..title_len as usize])
        } else {
            String::new()
        };

        if let Ok(proc_h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            let mut path_buf = [0u16; 512];
            let mut size = path_buf.len() as u32;
            let ok = QueryFullProcessImageNameW(
                proc_h,
                PROCESS_NAME_WIN32,
                PWSTR(path_buf.as_mut_ptr()),
                &mut size,
            );
            let _ = CloseHandle(proc_h);
            if ok.is_ok() && size > 0 {
                let exe_path = String::from_utf16_lossy(&path_buf[..size as usize]);
                if let Some(stem) = Path::new(&exe_path).file_stem().and_then(|s| s.to_str()) {
                    return map_friendly_game_name(stem, &title);
                }
            }
        }
        if !title.trim().is_empty() {
            return sanitize_filename_part(&title);
        }
    }
    "Desktop".to_string()
}

// ======================== GLOBAL HOTKEY & FULLSCREEN HOOKS ========================

fn modifiers_currently_down(required_mods: u32) -> bool {
    #[cfg(target_os = "windows")]
    unsafe {
        if required_mods & MOD_CONTROL.0 != 0 && (GetAsyncKeyState(0x11) & (0x8000u16 as i16)) == 0 {
            return false;
        }
        if required_mods & MOD_ALT.0 != 0 && (GetAsyncKeyState(0x12) & (0x8000u16 as i16)) == 0 {
            return false;
        }
        if required_mods & MOD_SHIFT.0 != 0 && (GetAsyncKeyState(0x10) & (0x8000u16 as i16)) == 0 {
            return false;
        }
        true
    }
    #[cfg(not(target_os = "windows"))]
    {
        required_mods == 0
    }
}

fn dispatch_hotkey_trigger_if_debounced() {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let prev = LAST_HOTKEY_MS.load(Ordering::Relaxed);
    if now_ms.saturating_sub(prev) < 750 {
        return;
    }
    LAST_HOTKEY_MS.store(now_ms, Ordering::Relaxed);

    let chime_vol = CHIME_VOLUME_PCT.load(Ordering::Relaxed);
    play_confirmation_chime(chime_vol);

    if let Some(app) = APP_HANDLE.get() {
        let app_clone = app.clone();
        tauri::async_runtime::spawn(async move {
            match save_replay_internal(&app_clone, None).await {
                Ok(clip) => {
                    let _ = app_clone.emit("clip-saved", clip);
                }
                Err(err) => {
                    let _ = app_clone.emit("capture-error", err);
                }
            }
        });
    }
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn low_level_keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && (wparam.0 as u32 == WM_KEYDOWN || wparam.0 as u32 == WM_SYSKEYDOWN) {
        let kb = *(lparam.0 as *const KBDLLHOOKSTRUCT);
        if let Ok(hk) = get_hotkey_lock().try_lock() {
            if hk.is_mouse_xbutton.is_none()
                && kb.vkCode == hk.vk
                && modifiers_currently_down(hk.modifiers)
            {
                dispatch_hotkey_trigger_if_debounced();
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn low_level_mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && wparam.0 as u32 == WM_XBUTTONDOWN {
        let ms = *(lparam.0 as *const MSLLHOOKSTRUCT);
        let xbtn = ((ms.mouseData >> 16) & 0xFFFF) as u16;
        if let Ok(hk) = get_hotkey_lock().try_lock() {
            if hk.is_mouse_xbutton == Some(xbtn) && modifiers_currently_down(hk.modifiers) {
                dispatch_hotkey_trigger_if_debounced();
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

fn spawn_hotkey_listener_thread() {
    #[cfg(target_os = "windows")]
    {
        thread::Builder::new()
            .name("duke-hotkey-pump".to_string())
            .spawn(|| unsafe {
                let tid = windows::Win32::System::Threading::GetCurrentThreadId();
                HOTKEY_THREAD_ID.store(tid, Ordering::Relaxed);

                // Ensure message queue exists
                let mut msg = MSG::default();
                let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);

                let register_current = || {
                    let _ = UnregisterHotKey(None, HOTKEY_ID);
                    if let Ok(hk) = get_hotkey_lock().lock() {
                        if hk.is_mouse_xbutton.is_none() && hk.vk != 0 {
                            let mods = HOT_KEY_MODIFIERS(hk.modifiers | MOD_NOREPEAT.0);
                            let _ = RegisterHotKey(None, HOTKEY_ID, mods, hk.vk);
                        }
                    }
                };
                register_current();

                let h_mod: HINSTANCE = GetModuleHandleW(None).unwrap_or_default().into();
                let kb_hook: Option<HHOOK> =
                    SetWindowsHookExW(WH_KEYBOARD_LL, Some(low_level_keyboard_proc), Some(h_mod), 0)
                        .ok();
                let ms_hook: Option<HHOOK> =
                    SetWindowsHookExW(WH_MOUSE_LL, Some(low_level_mouse_proc), Some(h_mod), 0).ok();

                while GetMessageW(&mut msg, None, 0, 0).into() {
                    if msg.message == WM_HOTKEY && msg.wParam.0 as i32 == HOTKEY_ID {
                        dispatch_hotkey_trigger_if_debounced();
                    } else if msg.message == WM_REBIND_HOTKEY {
                        register_current();
                    } else if msg.message == WM_QUIT {
                        break;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }

                let _ = UnregisterHotKey(None, HOTKEY_ID);
                if let Some(h) = kb_hook {
                    let _ = UnhookWindowsHookEx(h);
                }
                if let Some(h) = ms_hook {
                    let _ = UnhookWindowsHookEx(h);
                }
            })
            .ok();
    }
}

fn notify_hotkey_rebind() {
    #[cfg(target_os = "windows")]
    {
        let tid = HOTKEY_THREAD_ID.load(Ordering::Relaxed);
        if tid != 0 {
            unsafe {
                let _ = PostThreadMessageW(tid, WM_REBIND_HOTKEY, WPARAM(0), LPARAM(0));
            }
        }
    }
}

// ======================== HARDWARE ENCODER & FFMPEG DISCOVERY ========================

fn resolve_ffmpeg_bin_dir() -> PathBuf {
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        return PathBuf::from(local).join("DukeCapture").join("bin");
    }
    std::env::temp_dir().join("DukeCapture").join("bin")
}

fn find_ffmpeg_binary() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let local = dir.join("ffmpeg.exe");
            if local.exists() {
                return Some(local);
            }
            let bin = dir.join("bin").join("ffmpeg.exe");
            if bin.exists() {
                return Some(bin);
            }
        }
    }
    let managed = resolve_ffmpeg_bin_dir().join("ffmpeg.exe");
    if managed.exists() {
        return Some(managed);
    }
    if let Ok(path_env) = std::env::var("PATH") {
        for segment in path_env.split(';') {
            let cand = PathBuf::from(segment.trim()).join("ffmpeg.exe");
            if cand.exists() {
                return Some(cand);
            }
        }
    }
    None
}

fn probe_hardware_capabilities() -> HardwareCapabilities {
    let mut gpu_name = "Windows Display Adapter".to_string();
    let mut vendor_id: u32 = 0;

    #[cfg(target_os = "windows")]
    unsafe {
        if let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() {
            let mut idx = 0;
            while let Ok(adapter) = factory.EnumAdapters1(idx) {
                idx += 1;
                if let Ok(desc) = adapter.GetDesc1() {
                    if (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
                        continue;
                    }
                    let len = desc
                        .Description
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(desc.Description.len());
                    let name = String::from_utf16_lossy(&desc.Description[..len]).trim().to_string();
                    if !name.is_empty() {
                        gpu_name = name;
                    }
                    vendor_id = desc.VendorId;
                    break;
                }
            }
        }
    }

    let ffmpeg_path = find_ffmpeg_binary();
    let mut encoders = Vec::new();
    let mut ffmpeg_installed = false;

    if let Some(ref ff) = ffmpeg_path {
        ffmpeg_installed = true;
        if let Ok(out) = Command::new(ff)
            .args(["-hide_banner", "-encoders"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
        {
            let text = String::from_utf8_lossy(&out.stdout);
            for enc in ["h264_nvenc", "h264_qsv", "h264_amf", "libx264"] {
                if text.contains(enc) {
                    encoders.push(enc.to_string());
                }
            }
        }
    }

    // Native Windows Media Foundation H.264 Hardware MFT is always present on Windows 10/11
    encoders.push("mf_hw".to_string());

    let preferred_tag = if encoders.iter().any(|e| e == "h264_nvenc") && vendor_id == 0x10DE {
        "DXGI Duplication + NVIDIA NVENC (h264_nvenc)"
    } else if encoders.iter().any(|e| e == "h264_qsv") && vendor_id == 0x8086 {
        "DXGI Duplication + Intel QuickSync (h264_qsv)"
    } else if encoders.iter().any(|e| e == "h264_amf") && vendor_id == 0x1002 {
        "DXGI Duplication + AMD AMF (h264_amf)"
    } else if vendor_id == 0x10DE {
        "DXGI Duplication + Windows Media Foundation H.264 HW (NVIDIA)"
    } else if vendor_id == 0x8086 {
        "DXGI Duplication + Windows Media Foundation H.264 HW (Intel)"
    } else if vendor_id == 0x1002 {
        "DXGI Duplication + Windows Media Foundation H.264 HW (AMD)"
    } else {
        "DXGI Duplication + Windows Media Foundation H.264 HW"
    };

    HardwareCapabilities {
        active_backend: preferred_tag.to_string(),
        available_encoders: encoders,
        gpu_adapter_name: gpu_name,
        ffmpeg_installed,
        ffmpeg_path: ffmpeg_path.map(|p| p.to_string_lossy().to_string()),
    }
}

fn get_hardware_caps() -> HardwareCapabilities {
    let lock = HARDWARE_PROBE.get_or_init(|| Mutex::new(probe_hardware_capabilities()));
    lock.lock()
        .map(|g| g.clone())
        .unwrap_or_else(|_| probe_hardware_capabilities())
}

// ======================== DUAL WASAPI AUDIO CAPTURE ========================

#[cfg(target_os = "windows")]
unsafe fn read_device_friendly_name(dev: &IMMDevice) -> String {
    if let Ok(store) = dev.OpenPropertyStore(STGM_READ) {
        if let Ok(mut prop) = store.GetValue(&PKEY_Device_FriendlyName) {
            let pwsz = prop.Anonymous.Anonymous.Anonymous.pwszVal;
            let out = if !pwsz.0.is_null() {
                pwsz.to_string().unwrap_or_default()
            } else {
                String::new()
            };
            let _ = PropVariantClear(&mut prop);
            if !out.trim().is_empty() {
                return out;
            }
        }
    }
    "Audio Input Device".to_string()
}

fn enumerate_mic_devices() -> Vec<AudioInputDevice> {
    let mut devices = Vec::new();
    #[cfg(target_os = "windows")]
    unsafe {
        let com_ok = CoInitializeEx(None, COINIT_MULTITHREADED);
        if let Ok(enumerator) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) {
            let default_id = enumerator
                .GetDefaultAudioEndpoint(eCapture, eConsole)
                .ok()
                .and_then(|d| {
                    d.GetId().ok().map(|pw| {
                        let s = pw.to_string().unwrap_or_default();
                        CoTaskMemFree(Some(pw.0 as *const c_void));
                        s
                    })
                })
                .unwrap_or_default();

            if let Ok(collection) = enumerator.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE) {
                let count = collection.GetCount().unwrap_or(0);
                for idx in 0..count {
                    if let Ok(dev) = collection.Item(idx) {
                        let id = dev
                            .GetId()
                            .ok()
                            .map(|pw| {
                                let s = pw.to_string().unwrap_or_default();
                                CoTaskMemFree(Some(pw.0 as *const c_void));
                                s
                            })
                            .unwrap_or_default();
                        if id.is_empty() {
                            continue;
                        }
                        let name = read_device_friendly_name(&dev);
                        devices.push(AudioInputDevice {
                            is_default: id == default_id,
                            id,
                            name,
                        });
                    }
                }
            }
        }
        if com_ok.is_ok() {
            CoUninitialize();
        }
    }
    devices
}

#[cfg(target_os = "windows")]
unsafe fn is_ieee_float_format(fmt: *const WAVEFORMATEX) -> bool {
    let tag = (*fmt).wFormatTag;
    if tag == 3 {
        return true;
    }
    if tag == 0xFFFE && (*fmt).cbSize >= 22 {
        let ext = fmt as *const WAVEFORMATEXTENSIBLE;
        // KSDATAFORMAT_SUBTYPE_IEEE_FLOAT = {00000003-0000-0010-8000-00aa00389b71}
        let sub_float = GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);
        let sub_actual = std::ptr::addr_of!((*ext).SubFormat).read_unaligned();
        return sub_actual == sub_float;
    }
    false
}

fn convert_packet_to_stereo_48k_f32(
    data: &[u8],
    frames: usize,
    src_channels: usize,
    src_rate: u32,
    bits: u16,
    is_float: bool,
) -> Vec<f32> {
    if frames == 0 || src_channels == 0 || data.is_empty() {
        return Vec::new();
    }
    let mut src_stereo = Vec::with_capacity(frames * 2);
    for f in 0..frames {
        let (l, r) = if is_float && bits == 32 {
            let base = f * src_channels * 4;
            if base + 4 > data.len() {
                break;
            }
            let ch0 = f32::from_le_bytes(data[base..base + 4].try_into().unwrap_or([0; 4]));
            let ch1 = if src_channels >= 2 && base + 8 <= data.len() {
                f32::from_le_bytes(data[base + 4..base + 8].try_into().unwrap_or([0; 4]))
            } else {
                ch0
            };
            (ch0, ch1)
        } else if !is_float && bits == 16 {
            let base = f * src_channels * 2;
            if base + 2 > data.len() {
                break;
            }
            let ch0 = i16::from_le_bytes(data[base..base + 2].try_into().unwrap_or([0; 2])) as f32 / 32768.0;
            let ch1 = if src_channels >= 2 && base + 4 <= data.len() {
                i16::from_le_bytes(data[base + 2..base + 4].try_into().unwrap_or([0; 2])) as f32 / 32768.0
            } else {
                ch0
            };
            (ch0, ch1)
        } else {
            (0.0, 0.0)
        };
        src_stereo.push(l);
        src_stereo.push(r);
    }

    if src_rate == AUDIO_SAMPLE_RATE || src_stereo.len() < 4 {
        return src_stereo;
    }

    let in_frames = src_stereo.len() / 2;
    let out_frames = ((in_frames as u64 * AUDIO_SAMPLE_RATE as u64) / src_rate.max(8000) as u64) as usize;
    let mut resampled = Vec::with_capacity(out_frames * 2);
    for i in 0..out_frames {
        let pos = (i as f64) * ((in_frames - 1) as f64) / (out_frames.max(1) as f64);
        let idx = (pos.floor() as usize).min(in_frames.saturating_sub(2));
        let frac = (pos - idx as f64) as f32;
        let l = src_stereo[idx * 2] * (1.0 - frac) + src_stereo[(idx + 1) * 2] * frac;
        let r = src_stereo[idx * 2 + 1] * (1.0 - frac) + src_stereo[(idx + 1) * 2 + 1] * frac;
        resampled.push(l);
        resampled.push(r);
    }
    resampled
}

fn spawn_wasapi_workers(mic_device_id: Option<String>, generation: u64) {
    AUDIO_STOP_SIGNAL.store(false, Ordering::Relaxed);
    let hub_sys = get_audio_mix_hub();
    let hub_mic = get_audio_mix_hub();

    // Stream 1: Full PC System + Discord Loopback (AUDCLNT_STREAMFLAGS_LOOPBACK)
    thread::Builder::new()
        .name("duke-wasapi-loopback".to_string())
        .spawn(move || {
            #[cfg(target_os = "windows")]
            unsafe {
                let com_ok = CoInitializeEx(None, COINIT_MULTITHREADED);
                if let Ok(enumerator) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) {
                    if let Ok(device) = enumerator.GetDefaultAudioEndpoint(eRender, eConsole) {
                        if let Ok(audio_client) = device.Activate::<IAudioClient>(CLSCTX_ALL, None) {
                            if let Ok(mix_ptr) = audio_client.GetMixFormat() {
                                let ch = (*mix_ptr).nChannels as usize;
                                let rate = (*mix_ptr).nSamplesPerSec;
                                let bits = (*mix_ptr).wBitsPerSample;
                                let is_flt = is_ieee_float_format(mix_ptr);
                                let block_align = (*mix_ptr).nBlockAlign as usize;

                                let init_res = audio_client.Initialize(
                                    AUDCLNT_SHAREMODE_SHARED,
                                    AUDCLNT_STREAMFLAGS_LOOPBACK,
                                    1_000_000, // 100ms
                                    0,
                                    mix_ptr,
                                    None,
                                );
                                CoTaskMemFree(Some(mix_ptr as *const c_void));

                                if init_res.is_ok() {
                                    if let Ok(cap_client) = audio_client.GetService::<IAudioCaptureClient>() {
                                        let _ = audio_client.Start();
                                        let mut silence_cycles = 0u32;
                                        while !AUDIO_STOP_SIGNAL.load(Ordering::Relaxed)
                                            && AUDIO_GENERATION.load(Ordering::Relaxed) == generation
                                        {
                                            thread::sleep(Duration::from_millis(12));
                                            let mut received_any = false;
                                            while let Ok(pkt_len) = cap_client.GetNextPacketSize() {
                                                if pkt_len == 0 {
                                                    break;
                                                }
                                                let mut data_ptr = std::ptr::null_mut();
                                                let mut num_frames = 0u32;
                                                let mut flags = 0u32;
                                                if cap_client
                                                    .GetBuffer(&mut data_ptr, &mut num_frames, &mut flags, None, None)
                                                    .is_err()
                                                {
                                                    break;
                                                }
                                                received_any = true;
                                                let silent = (flags & 0x2) != 0;
                                                let samples = if silent || data_ptr.is_null() {
                                                    let out_frames = ((num_frames as u64 * AUDIO_SAMPLE_RATE as u64)
                                                        / rate.max(8000) as u64) as usize;
                                                    vec![0.0f32; out_frames * 2]
                                                } else {
                                                    let byte_len = num_frames as usize * block_align;
                                                    let slice = std::slice::from_raw_parts(data_ptr, byte_len);
                                                    convert_packet_to_stereo_48k_f32(
                                                        slice,
                                                        num_frames as usize,
                                                        ch,
                                                        rate,
                                                        bits,
                                                        is_flt,
                                                    )
                                                };
                                                let _ = cap_client.ReleaseBuffer(num_frames);

                                                let vol = SYSTEM_VOLUME_PCT.load(Ordering::Relaxed) as f32 / 100.0;
                                                let mut peak = 0.0f32;
                                                let scaled: Vec<f32> = samples
                                                    .into_iter()
                                                    .map(|s| {
                                                        let v = s * vol;
                                                        let a = v.abs();
                                                        if a > peak {
                                                            peak = a;
                                                        }
                                                        v
                                                    })
                                                    .collect();
                                                let pct = ((peak.sqrt() * 100.0).round() as u32).min(100);
                                                SYSTEM_LEVEL_PCT.store(pct, Ordering::Relaxed);

                                                if REPLAY_RUNNING.load(Ordering::Relaxed) {
                                                    if let Ok(mut guard) = hub_sys.lock() {
                                                        for s in scaled {
                                                            if guard.sys_samples.len() >= AUDIO_SAMPLE_RATE as usize * 8 {
                                                                guard.sys_samples.pop_front();
                                                            }
                                                            guard.sys_samples.push_back(s);
                                                        }
                                                    }
                                                }
                                            }
                                            // Keep exact sample clock even when Windows render endpoint is silent
                                            if !received_any {
                                                silence_cycles += 1;
                                                SYSTEM_LEVEL_PCT.store(0, Ordering::Relaxed);
                                                if silence_cycles % 2 == 0 && REPLAY_RUNNING.load(Ordering::Relaxed) {
                                                    let fill_samples = (AUDIO_SAMPLE_RATE as usize * 24 / 1000) * 2;
                                                    if let Ok(mut guard) = hub_sys.lock() {
                                                        for _ in 0..fill_samples {
                                                            if guard.sys_samples.len() >= AUDIO_SAMPLE_RATE as usize * 8 {
                                                                guard.sys_samples.pop_front();
                                                            }
                                                            guard.sys_samples.push_back(0.0);
                                                        }
                                                    }
                                                }
                                            } else {
                                                silence_cycles = 0;
                                            }
                                        }
                                        let _ = audio_client.Stop();
                                    }
                                }
                            }
                        }
                    }
                }
                if com_ok.is_ok() {
                    CoUninitialize();
                }
            }
        })
        .ok();

    // Stream 2: Microphone Input Capture (IAudioCaptureClient)
    thread::Builder::new()
        .name("duke-wasapi-mic".to_string())
        .spawn(move || {
            #[cfg(target_os = "windows")]
            unsafe {
                let com_ok = CoInitializeEx(None, COINIT_MULTITHREADED);
                if let Ok(enumerator) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) {
                    let device = if let Some(ref id) = mic_device_id {
                        let wide = to_wide_null(id);
                        enumerator
                            .GetDevice(PCWSTR(wide.as_ptr()))
                            .or_else(|_| enumerator.GetDefaultAudioEndpoint(eCapture, eMultimedia))
                    } else {
                        enumerator.GetDefaultAudioEndpoint(eCapture, eMultimedia)
                    };

                    if let Ok(dev) = device {
                        if let Ok(audio_client) = dev.Activate::<IAudioClient>(CLSCTX_ALL, None) {
                            if let Ok(mix_ptr) = audio_client.GetMixFormat() {
                                let ch = (*mix_ptr).nChannels as usize;
                                let rate = (*mix_ptr).nSamplesPerSec;
                                let bits = (*mix_ptr).wBitsPerSample;
                                let is_flt = is_ieee_float_format(mix_ptr);
                                let block_align = (*mix_ptr).nBlockAlign as usize;

                                let init_res = audio_client.Initialize(
                                    AUDCLNT_SHAREMODE_SHARED,
                                    0,
                                    1_000_000,
                                    0,
                                    mix_ptr,
                                    None,
                                );
                                CoTaskMemFree(Some(mix_ptr as *const c_void));

                                if init_res.is_ok() {
                                    if let Ok(cap_client) = audio_client.GetService::<IAudioCaptureClient>() {
                                        let _ = audio_client.Start();
                                        let mut gate_open_hold: u32 = 0;
                                        while !AUDIO_STOP_SIGNAL.load(Ordering::Relaxed)
                                            && AUDIO_GENERATION.load(Ordering::Relaxed) == generation
                                        {
                                            thread::sleep(Duration::from_millis(12));
                                            while let Ok(pkt_len) = cap_client.GetNextPacketSize() {
                                                if pkt_len == 0 {
                                                    break;
                                                }
                                                let mut data_ptr = std::ptr::null_mut();
                                                let mut num_frames = 0u32;
                                                let mut flags = 0u32;
                                                if cap_client
                                                    .GetBuffer(&mut data_ptr, &mut num_frames, &mut flags, None, None)
                                                    .is_err()
                                                {
                                                    break;
                                                }
                                                let muted = MIC_MUTED.load(Ordering::Relaxed);
                                                let silent = muted || (flags & 0x2) != 0 || data_ptr.is_null();
                                                let samples = if silent {
                                                    let out_frames = ((num_frames as u64 * AUDIO_SAMPLE_RATE as u64)
                                                        / rate.max(8000) as u64) as usize;
                                                    vec![0.0f32; out_frames * 2]
                                                } else {
                                                    let byte_len = num_frames as usize * block_align;
                                                    let slice = std::slice::from_raw_parts(data_ptr, byte_len);
                                                    convert_packet_to_stereo_48k_f32(
                                                        slice,
                                                        num_frames as usize,
                                                        ch,
                                                        rate,
                                                        bits,
                                                        is_flt,
                                                    )
                                                };
                                                let _ = cap_client.ReleaseBuffer(num_frames);

                                                // Compute RMS for noise gate + peak for VU meter
                                                let sum_sq: f32 = samples.iter().map(|&x| x * x).sum();
                                                let rms = (sum_sq / samples.len().max(1) as f32).sqrt();
                                                let gate_enabled = NOISE_GATE_ENABLED.load(Ordering::Relaxed);
                                                let gate_gain = if muted {
                                                    0.0
                                                } else if !gate_enabled || rms >= 0.008 {
                                                    gate_open_hold = 12;
                                                    1.0
                                                } else if gate_open_hold > 0 {
                                                    gate_open_hold -= 1;
                                                    0.85
                                                } else {
                                                    0.0
                                                };

                                                let mic_gain = (MIC_GAIN_PCT.load(Ordering::Relaxed) as f32 / 100.0) * gate_gain;
                                                let mut peak = 0.0f32;
                                                let scaled: Vec<f32> = samples
                                                    .into_iter()
                                                    .map(|s| {
                                                        let v = s * mic_gain;
                                                        let a = v.abs();
                                                        if a > peak {
                                                            peak = a;
                                                        }
                                                        v
                                                    })
                                                    .collect();
                                                let pct = ((peak.sqrt() * 100.0).round() as u32).min(100);
                                                MIC_LEVEL_PCT.store(pct, Ordering::Relaxed);

                                                if REPLAY_RUNNING.load(Ordering::Relaxed) {
                                                    if let Ok(mut guard) = hub_mic.lock() {
                                                        for s in scaled {
                                                            if guard.mic_samples.len() >= AUDIO_SAMPLE_RATE as usize * 8 {
                                                                guard.mic_samples.pop_front();
                                                            }
                                                            guard.mic_samples.push_back(s);
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        let _ = audio_client.Stop();
                                    }
                                }
                            }
                        }
                    }
                }
                if com_ok.is_ok() {
                    CoUninitialize();
                }
            }
        })
        .ok();
}

fn drain_mixed_stereo_i16(target_samples: usize) -> Vec<i16> {
    let hub = get_audio_mix_hub();
    let mut out = Vec::with_capacity(target_samples);
    if let Ok(mut guard) = hub.lock() {
        for _ in 0..target_samples {
            let sys = guard.sys_samples.pop_front().unwrap_or(0.0);
            let mic = guard.mic_samples.pop_front().unwrap_or(0.0);
            let mixed = (sys + mic).tanh();
            out.push((mixed * i16::MAX as f32) as i16);
        }
    } else {
        out.resize(target_samples, 0);
    }
    out
}

// ======================== DXGI DESKTOP DUPLICATION + GDI FALLBACK ========================

struct ScreenCapturer {
    src_w: u32,
    src_h: u32,
    #[cfg(target_os = "windows")]
    d3d_device: Option<ID3D11Device>,
    #[cfg(target_os = "windows")]
    d3d_ctx: Option<ID3D11DeviceContext>,
    #[cfg(target_os = "windows")]
    dupl: Option<IDXGIOutputDuplication>,
    #[cfg(target_os = "windows")]
    staging: Option<ID3D11Texture2D>,
    last_frame_bgra: Vec<u8>,
}

impl ScreenCapturer {
    fn new() -> Self {
        let (src_w, src_h) = unsafe {
            let w = GetSystemMetrics(SM_CXSCREEN).max(640) as u32;
            let h = GetSystemMetrics(SM_CYSCREEN).max(480) as u32;
            (w, h)
        };
        let mut cap = Self {
            src_w,
            src_h,
            #[cfg(target_os = "windows")]
            d3d_device: None,
            #[cfg(target_os = "windows")]
            d3d_ctx: None,
            #[cfg(target_os = "windows")]
            dupl: None,
            #[cfg(target_os = "windows")]
            staging: None,
            last_frame_bgra: vec![0u8; (src_w * src_h * 4) as usize],
        };
        cap.try_init_dxgi();
        cap
    }

    fn try_init_dxgi(&mut self) {
        #[cfg(target_os = "windows")]
        unsafe {
            let mut dev: Option<ID3D11Device> = None;
            let mut ctx: Option<ID3D11DeviceContext> = None;
            if D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut dev),
                None,
                Some(&mut ctx),
            )
            .is_err()
            {
                let _ = D3D11CreateDevice(
                    None,
                    D3D_DRIVER_TYPE_UNKNOWN,
                    HMODULE::default(),
                    D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                    None,
                    D3D11_SDK_VERSION,
                    Some(&mut dev),
                    None,
                    Some(&mut ctx),
                );
            }

            let (Some(d3d_dev), Some(d3d_ctx)) = (dev, ctx) else {
                return;
            };

            if let Ok(dxgi_dev) = d3d_dev.cast::<IDXGIDevice>() {
                if let Ok(adapter) = dxgi_dev.GetAdapter() {
                    if let Ok(output) = adapter.EnumOutputs(0) {
                        if let Ok(output1) = output.cast::<IDXGIOutput1>() {
                            if let Ok(desc) = output1.GetDesc() {
                                let w = (desc.DesktopCoordinates.right - desc.DesktopCoordinates.left).max(640) as u32;
                                let h = (desc.DesktopCoordinates.bottom - desc.DesktopCoordinates.top).max(480) as u32;
                                self.src_w = w;
                                self.src_h = h;
                                self.last_frame_bgra.resize((w * h * 4) as usize, 0);
                            }
                            if let Ok(dupl) = output1.DuplicateOutput(&d3d_dev) {
                                let tex_desc = D3D11_TEXTURE2D_DESC {
                                    Width: self.src_w,
                                    Height: self.src_h,
                                    MipLevels: 1,
                                    ArraySize: 1,
                                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                                    SampleDesc: DXGI_SAMPLE_DESC {
                                        Count: 1,
                                        Quality: 0,
                                    },
                                    Usage: D3D11_USAGE_STAGING,
                                    BindFlags: 0,
                                    CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                                    MiscFlags: 0,
                                };
                                let mut staging_tex: Option<ID3D11Texture2D> = None;
                                if d3d_dev.CreateTexture2D(&tex_desc, None, Some(&mut staging_tex)).is_ok() {
                                    self.d3d_device = Some(d3d_dev);
                                    self.d3d_ctx = Some(d3d_ctx);
                                    self.dupl = Some(dupl);
                                    self.staging = staging_tex;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn capture_bgra_frame(&mut self) -> (u32, u32, Vec<u8>) {
        #[cfg(target_os = "windows")]
        {
            if let (Some(dupl), Some(ctx), Some(staging)) = (&self.dupl, &self.d3d_ctx, &self.staging) {
                unsafe {
                    let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
                    let mut resource = None;
                    let res = dupl.AcquireNextFrame(12, &mut info, &mut resource);
                    match res {
                        Ok(()) => {
                            if let Some(res_unk) = resource {
                                if let Ok(tex) = res_unk.cast::<ID3D11Texture2D>() {
                                    ctx.CopyResource(staging, &tex);
                                    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                                    if ctx.Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).is_ok() {
                                        let row_bytes = (self.src_w * 4) as usize;
                                        let pitch = mapped.RowPitch as usize;
                                        let total_src = pitch * self.src_h as usize;
                                        let src_slice = std::slice::from_raw_parts(mapped.pData as *const u8, total_src);
                                        for y in 0..self.src_h as usize {
                                            let dst_off = y * row_bytes;
                                            let src_off = y * pitch;
                                            self.last_frame_bgra[dst_off..dst_off + row_bytes]
                                                .copy_from_slice(&src_slice[src_off..src_off + row_bytes]);
                                        }
                                        ctx.Unmap(staging, 0);
                                    }
                                }
                            }
                            let _ = dupl.ReleaseFrame();
                            return (self.src_w, self.src_h, self.last_frame_bgra.clone());
                        }
                        Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => {
                            return (self.src_w, self.src_h, self.last_frame_bgra.clone());
                        }
                        Err(_) => {
                            // Session switched (UAC / exclusive game / monitor change): fallback GDI and clear DXGI for retry
                            self.dupl = None;
                            self.staging = None;
                        }
                    }
                }
            } else {
                self.try_init_dxgi();
            }

            // GDI BitBlt Top-Down Fallback
            unsafe {
                let hdc_screen = GetDC(None);
                let hdc_mem = CreateCompatibleDC(Some(hdc_screen));
                let hbm = CreateCompatibleBitmap(hdc_screen, self.src_w as i32, self.src_h as i32);
                let old = SelectObject(hdc_mem, hbm.into());
                let _ = BitBlt(
                    hdc_mem,
                    0,
                    0,
                    self.src_w as i32,
                    self.src_h as i32,
                    Some(hdc_screen),
                    0,
                    0,
                    SRCCOPY,
                );
                let mut bi = BITMAPINFO::default();
                bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
                bi.bmiHeader.biWidth = self.src_w as i32;
                bi.bmiHeader.biHeight = -(self.src_h as i32); // negative => top-down BGRA
                bi.bmiHeader.biPlanes = 1;
                bi.bmiHeader.biBitCount = 32;
                bi.bmiHeader.biCompression = BI_RGB.0;
                let _ = GetDIBits(
                    hdc_mem,
                    hbm,
                    0,
                    self.src_h,
                    Some(self.last_frame_bgra.as_mut_ptr() as *mut c_void),
                    &mut bi,
                    DIB_RGB_COLORS,
                );
                let _ = SelectObject(hdc_mem, old);
                let _ = DeleteObject(hbm.into());
                let _ = DeleteDC(hdc_mem);
                let _ = ReleaseDC(None, hdc_screen);
            }
        }
        (self.src_w, self.src_h, self.last_frame_bgra.clone())
    }
}

fn target_dimensions(src_w: u32, src_h: u32, setting: &str) -> (u32, u32) {
    let target_h = match setting {
        "1080p" => 1080u32.min(src_h),
        "1440p" => 1440u32.min(src_h),
        _ => src_h,
    };
    let mut target_w = ((src_w as u64 * target_h as u64) / src_h.max(1) as u64) as u32;
    // H.264 requires even dimensions
    target_w &= !1;
    let even_h = target_h & !1;
    (target_w.max(320), even_h.max(240))
}

fn downscale_bgra_nearest(src: &[u8], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Vec<u8> {
    if src_w == dst_w && src_h == dst_h {
        return src.to_vec();
    }
    let mut dst = vec![0u8; (dst_w * dst_h * 4) as usize];
    for dy in 0..dst_h {
        let sy = (dy as u64 * src_h as u64 / dst_h as u64) as u32;
        let src_row = (sy * src_w * 4) as usize;
        let dst_row = (dy * dst_w * 4) as usize;
        for dx in 0..dst_w {
            let sx = (dx as u64 * src_w as u64 / dst_w as u64) as u32;
            let si = src_row + (sx * 4) as usize;
            let di = dst_row + (dx * 4) as usize;
            if si + 4 <= src.len() {
                dst[di..di + 4].copy_from_slice(&src[si..si + 4]);
            }
        }
    }
    dst
}

// ======================== IN-RAM SEGMENT ENCODER (MF & FFMPEG) ========================

#[cfg(target_os = "windows")]
unsafe fn pack_u32_pair(hi: u32, lo: u32) -> u64 {
    ((hi as u64) << 32) | (lo as u64)
}

#[cfg(target_os = "windows")]
unsafe fn mf_create_attributes(cap: u32) -> windows::core::Result<IMFAttributes> {
    let mut attrs: Option<IMFAttributes> = None;
    MFCreateAttributes(&mut attrs, cap)?;
    attrs.ok_or_else(windows::core::Error::empty)
}

#[cfg(target_os = "windows")]
unsafe fn mf_writer_attributes() -> Result<IMFAttributes, String> {
    let attrs = mf_create_attributes(3).map_err(|e| format!("MFCreateAttributes failed: {}", e))?;
    let _ = attrs.SetGUID(&MF_TRANSCODE_CONTAINERTYPE, &MFTranscodeContainerType_MPEG4);
    let _ = attrs.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1);
    let _ = attrs.SetUINT32(&MF_SINK_WRITER_DISABLE_THROTTLING, 1);
    Ok(attrs)
}

#[cfg(target_os = "windows")]
unsafe fn mf_reader_attributes() -> Option<IMFAttributes> {
    let attrs = mf_create_attributes(1).ok()?;
    let _ = attrs.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1);
    Some(attrs)
}

#[cfg(target_os = "windows")]
unsafe fn configure_sink_writer_streams(
    writer: &IMFSinkWriter,
    width: u32,
    height: u32,
    fps: u32,
    video_bitrate_bps: u32,
    with_audio: bool,
) -> windows::core::Result<(u32, Option<u32>)> {
    let out_v = MFCreateMediaType()?;
    out_v.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
    out_v.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
    out_v.SetUINT32(&MF_MT_AVG_BITRATE, video_bitrate_bps)?;
    out_v.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
    out_v.SetUINT64(&MF_MT_FRAME_SIZE, pack_u32_pair(width, height))?;
    out_v.SetUINT64(&MF_MT_FRAME_RATE, pack_u32_pair(fps, 1))?;
    out_v.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack_u32_pair(1, 1))?;
    let video_stream = writer.AddStream(&out_v)?;

    let in_v = MFCreateMediaType()?;
    in_v.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
    in_v.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
    in_v.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
    in_v.SetUINT32(&MF_MT_DEFAULT_STRIDE, width * 4)?;
    in_v.SetUINT64(&MF_MT_FRAME_SIZE, pack_u32_pair(width, height))?;
    in_v.SetUINT64(&MF_MT_FRAME_RATE, pack_u32_pair(fps, 1))?;
    in_v.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack_u32_pair(1, 1))?;
    writer.SetInputMediaType(video_stream, &in_v, None)?;

    let audio_stream = if with_audio {
        let out_a = MFCreateMediaType()?;
        out_a.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
        out_a.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC)?;
        out_a.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, AUDIO_SAMPLE_RATE)?;
        out_a.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, AUDIO_CHANNELS)?;
        out_a.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
        out_a.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 24000)?; // 192 kbps
        let a_idx = writer.AddStream(&out_a)?;

        let in_a = MFCreateMediaType()?;
        in_a.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
        in_a.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)?;
        in_a.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, AUDIO_SAMPLE_RATE)?;
        in_a.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, AUDIO_CHANNELS)?;
        in_a.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
        writer.SetInputMediaType(a_idx, &in_a, None)?;
        Some(a_idx)
    } else {
        None
    };

    Ok((video_stream, audio_stream))
}

#[cfg(target_os = "windows")]
unsafe fn read_all_istream_bytes(stream: &IStream) -> Vec<u8> {
    let mut stat = STATSTG::default();
    if stream.Stat(&mut stat, STATFLAG_NONAME).is_err() {
        return Vec::new();
    }
    let total = stat.cbSize as usize;
    if total == 0 {
        return Vec::new();
    }
    let _ = stream.Seek(0, STREAM_SEEK_SET, None);
    let mut out = vec![0u8; total];
    let mut read_len = 0u32;
    let hr = stream.Read(out.as_mut_ptr() as *mut c_void, total as u32, Some(&mut read_len));
    if hr.is_ok() {
        out.truncate(read_len as usize);
        out
    } else {
        Vec::new()
    }
}

fn encode_segment_mf_ram(seg: &RawSegmentInput) -> Result<EncodedSegment, String> {
    #[cfg(target_os = "windows")]
    unsafe {
        let mem_stream = SHCreateMemStream(None).ok_or("Failed allocating in-RAM stream")?;
        let byte_stream = MFCreateMFByteStreamOnStream(&mem_stream)
            .map_err(|e| format!("MFCreateMFByteStreamOnStream failed: {}", e))?;

        let attrs = mf_writer_attributes()?;

        let writer = MFCreateSinkWriterFromURL(None, &byte_stream, Some(&attrs))
            .map_err(|e| format!("MFCreateSinkWriterFromURL(mem) failed: {}", e))?;

        let (v_stream, a_stream) = configure_sink_writer_streams(
            &writer,
            seg.width,
            seg.height,
            seg.fps,
            seg.bitrate_bps,
            true,
        )
        .map_err(|e| format!("configure_sink_writer_streams failed: {}", e))?;

        writer
            .BeginWriting()
            .map_err(|e| format!("BeginWriting failed: {}", e))?;

        let frame_dur = MFFrameRateToAverageTimePerFrame(seg.fps, 1).unwrap_or(166_666) as i64;
        let frame_byte_len = seg.width * seg.height * 4;

        for (idx, frame_bytes) in seg.frames_bgra.iter().enumerate() {
            if frame_bytes.len() < frame_byte_len as usize {
                continue;
            }
            if let (Ok(buf), Ok(sample)) = (MFCreateMemoryBuffer(frame_byte_len), MFCreateSample()) {
                let mut ptr = std::ptr::null_mut();
                if buf.Lock(&mut ptr, None, None).is_ok() {
                    std::ptr::copy_nonoverlapping(frame_bytes.as_ptr(), ptr, frame_byte_len as usize);
                    let _ = buf.Unlock();
                    let _ = buf.SetCurrentLength(frame_byte_len);
                    let _ = sample.AddBuffer(&buf);
                    let _ = sample.SetSampleTime(idx as i64 * frame_dur);
                    let _ = sample.SetSampleDuration(frame_dur);
                    let _ = writer.WriteSample(v_stream, &sample);
                }
            }
        }

        if let Some(a_idx) = a_stream {
            if !seg.audio_stereo_i16.is_empty() {
                let total_bytes = (seg.audio_stereo_i16.len() * 2) as u32;
                let a_dur_100ns = ((seg.duration_ms as i64) * 10_000).max(1);
                if let (Ok(abuf), Ok(asample)) = (MFCreateMemoryBuffer(total_bytes), MFCreateSample()) {
                    let mut ptr = std::ptr::null_mut();
                    if abuf.Lock(&mut ptr, None, None).is_ok() {
                        std::ptr::copy_nonoverlapping(
                            seg.audio_stereo_i16.as_ptr() as *const u8,
                            ptr,
                            total_bytes as usize,
                        );
                        let _ = abuf.Unlock();
                        let _ = abuf.SetCurrentLength(total_bytes);
                        let _ = asample.AddBuffer(&abuf);
                        let _ = asample.SetSampleTime(0);
                        let _ = asample.SetSampleDuration(a_dur_100ns);
                        let _ = writer.WriteSample(a_idx, &asample);
                    }
                }
            }
        }

        writer
            .Finalize()
            .map_err(|e| format!("Finalize segment failed: {}", e))?;
        drop(writer);
        drop(byte_stream);

        let mp4_bytes = read_all_istream_bytes(&mem_stream);
        if mp4_bytes.is_empty() {
            return Err("Encoded segment produced 0 bytes.".to_string());
        }
        Ok(EncodedSegment {
            duration_ms: seg.duration_ms,
            bytes: mp4_bytes,
        })
    }
    #[cfg(not(target_os = "windows"))]
    Err("Unsupported OS".to_string())
}

// ======================== CAPTURE LOOP & RING BUFFER ========================

fn spawn_capture_pipeline(app: AppHandle) {
    REPLAY_RUNNING.store(true, Ordering::Relaxed);
    CAPTURE_STOP_SIGNAL.store(false, Ordering::Relaxed);

    let cfg = get_config_lock(Some(&app))
        .lock()
        .map(|g| g.clone())
        .unwrap_or_else(|_| default_config(Some(&app)));

    // Clear stale buffer on clean arm
    if let Ok(mut rb) = get_ring_buffer().lock() {
        rb.clear();
    }

    let (tx, rx) = mpsc::sync_channel::<RawSegmentInput>(2);

    // Background segment encoder thread (Media Foundation COM MTA)
    thread::Builder::new()
        .name("duke-segment-encoder".to_string())
        .spawn(move || {
            #[cfg(target_os = "windows")]
            unsafe {
                let com_ok = CoInitializeEx(None, COINIT_MULTITHREADED);
                let mf_ok = MFStartup(MF_API_VERSION, MFSTARTUP_FULL);
                while let Ok(raw_seg) = rx.recv() {
                    if let Ok(encoded) = encode_segment_mf_ram(&raw_seg) {
                        let max_ms = get_config_lock(None)
                            .lock()
                            .map(|c| (c.buffer_seconds as u64) * 1000)
                            .unwrap_or(30_000);
                        if let Ok(mut rb) = get_ring_buffer().lock() {
                            rb.push_back(encoded);
                            let mut total_ms: u64 = rb.iter().map(|s| s.duration_ms).sum();
                            while total_ms > max_ms && rb.len() > 1 {
                                if let Some(old) = rb.pop_front() {
                                    total_ms = total_ms.saturating_sub(old.duration_ms);
                                }
                            }
                        }
                    }
                }
                if mf_ok.is_ok() {
                    let _ = MFShutdown();
                }
                if com_ok.is_ok() {
                    CoUninitialize();
                }
            }
        })
        .ok();

    // DXGI / GDI Video Pacing Capture Thread
    thread::Builder::new()
        .name("duke-video-capture".to_string())
        .spawn(move || {
            #[cfg(target_os = "windows")]
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }
            let mut capturer = ScreenCapturer::new();
            let fps = cfg.fps.clamp(30, 60);
            let frame_interval = Duration::from_micros(1_000_000 / fps as u64);
            let frames_per_seg = (fps * (SEGMENT_DURATION_MS / 1000) as u32) as usize;
            let samples_per_seg =
                (AUDIO_SAMPLE_RATE as usize * AUDIO_CHANNELS as usize * SEGMENT_DURATION_MS as usize) / 1000;

            let mut seg_frames: Vec<Vec<u8>> = Vec::with_capacity(frames_per_seg);

            while !CAPTURE_STOP_SIGNAL.load(Ordering::Relaxed) && REPLAY_RUNNING.load(Ordering::Relaxed) {
                let tick_start = Instant::now();

                let live_cfg = get_config_lock(Some(&app))
                    .lock()
                    .map(|g| g.clone())
                    .unwrap_or_else(|_| cfg.clone());

                let (sw, sh, bgra) = capturer.capture_bgra_frame();
                let (cur_w, cur_h) = target_dimensions(sw, sh, &live_cfg.resolution);
                let frame_scaled = downscale_bgra_nearest(&bgra, sw, sh, cur_w, cur_h);
                seg_frames.push(frame_scaled);

                if seg_frames.len() >= frames_per_seg {
                    let audio_chunk = drain_mixed_stereo_i16(samples_per_seg);
                    let raw = RawSegmentInput {
                        width: cur_w,
                        height: cur_h,
                        fps,
                        bitrate_bps: live_cfg.bitrate_kbps.clamp(4000, 30000) * 1000,
                        frames_bgra: std::mem::take(&mut seg_frames),
                        audio_stereo_i16: audio_chunk,
                        duration_ms: SEGMENT_DURATION_MS,
                    };
                    let _ = tx.try_send(raw);
                    seg_frames = Vec::with_capacity(frames_per_seg);
                }

                let elapsed = tick_start.elapsed();
                if elapsed < frame_interval {
                    thread::sleep(frame_interval - elapsed);
                }
            }
            REPLAY_RUNNING.store(false, Ordering::Relaxed);
            #[cfg(target_os = "windows")]
            unsafe {
                CoUninitialize();
            }
        })
        .ok();
}

// ======================== NATIVE MF STITCHER, TRIMMER & DISCORD COMPRESSOR ========================

#[cfg(target_os = "windows")]
fn stitch_ram_segments_mf(
    segments: &[EncodedSegment],
    output_path: &Path,
    bitrate_bps: u32,
) -> Result<(), String> {
    if segments.is_empty() {
        return Err("Replay ring buffer has no completed segments yet (wait ~2 seconds after starting).".to_string());
    }
    unsafe {
        let com_ok = CoInitializeEx(None, COINIT_MULTITHREADED);
        let mf_ok = MFStartup(MF_API_VERSION, MFSTARTUP_FULL);

        // Probe resolution/fps from first segment
        let first_stream = SHCreateMemStream(Some(&segments[0].bytes))
            .ok_or("Failed opening first RAM segment stream")?;
        let first_bs = MFCreateMFByteStreamOnStream(&first_stream)
            .map_err(|e| format!("ByteStream error: {}", e))?;
        let probe_reader = MFCreateSourceReaderFromByteStream(&first_bs, None)
            .map_err(|e| format!("Probe reader error: {}", e))?;

        let probe_v = probe_reader
            .GetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM)
            .map_err(|e| format!("Failed reading video media type: {}", e))?;
        let frame_size = probe_v.GetUINT64(&MF_MT_FRAME_SIZE).unwrap_or(((1920u64) << 32) | 1080);
        let width = (frame_size >> 32) as u32;
        let height = (frame_size & 0xFFFF_FFFF) as u32;
        let frame_rate = probe_v.GetUINT64(&MF_MT_FRAME_RATE).unwrap_or(((60u64) << 32) | 1);
        let fps_num = ((frame_rate >> 32) as u32).max(30);
        drop(probe_reader);

        let attrs = mf_writer_attributes()?;

        let out_wide = to_wide_null(&output_path.to_string_lossy());
        let writer = MFCreateSinkWriterFromURL(PCWSTR(out_wide.as_ptr()), None, Some(&attrs))
            .map_err(|e| format!("MFCreateSinkWriterFromURL failed: {}", e))?;

        let (v_out, a_out) = configure_sink_writer_streams(
            &writer,
            width,
            height,
            fps_num,
            bitrate_bps,
            true,
        )
        .map_err(|e| format!("Output stream config failed: {}", e))?;

        writer.BeginWriting().map_err(|e| format!("BeginWriting failed: {}", e))?;

        let mut base_time_100ns: i64 = 0;

        for seg in segments {
            let s_stream = match SHCreateMemStream(Some(&seg.bytes)) {
                Some(s) => s,
                None => continue,
            };
            let s_bs = match MFCreateMFByteStreamOnStream(&s_stream) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let r_attrs = mf_reader_attributes();
            let reader: IMFSourceReader = match MFCreateSourceReaderFromByteStream(&s_bs, r_attrs.as_ref()) {
                Ok(r) => r,
                Err(_) => continue,
            };

            // Request decoded RGB32 + PCM so timestamps and GOP SPS/PPS stitch cleanly across chunks
            if let Ok(req_v) = MFCreateMediaType() {
                let _ = req_v.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video);
                let _ = req_v.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32);
                let _ = reader.SetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM, None, &req_v);
            }
            if let Ok(req_a) = MFCreateMediaType() {
                let _ = req_a.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio);
                let _ = req_a.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM);
                let _ = req_a.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, AUDIO_SAMPLE_RATE);
                let _ = req_a.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, AUDIO_CHANNELS);
                let _ = req_a.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16);
                let _ = reader.SetCurrentMediaType(MF_SOURCE_READER_FIRST_AUDIO_STREAM, None, &req_a);
            }

            let mut max_seg_ts: i64 = (seg.duration_ms as i64) * 10_000;
            loop {
                let mut stream_idx: u32 = 0;
                let mut flags: u32 = 0;
                let mut ts: i64 = 0;
                let mut sample: Option<IMFSample> = None;
                if reader
                    .ReadSample(
                        MF_SOURCE_READER_ANY_STREAM,
                        0,
                        Some(&mut stream_idx),
                        Some(&mut flags),
                        Some(&mut ts),
                        Some(&mut sample),
                    )
                    .is_err()
                {
                    break;
                }
                if (flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32) != 0 {
                    break;
                }
                if let Some(smp) = sample {
                    let dur = smp.GetSampleDuration().unwrap_or(0);
                    let end_ts = ts + dur;
                    if end_ts > max_seg_ts {
                        max_seg_ts = end_ts;
                    }
                    let _ = smp.SetSampleTime(base_time_100ns + ts);
                    if stream_idx == 0 {
                        let _ = writer.WriteSample(v_out, &smp);
                    } else if let Some(a_idx) = a_out {
                        let _ = writer.WriteSample(a_idx, &smp);
                    }
                }
            }
            base_time_100ns += max_seg_ts.max(10_000);
        }

        let res = writer.Finalize().map_err(|e| format!("Finalize stitched MP4 failed: {}", e));
        if mf_ok.is_ok() {
            let _ = MFShutdown();
        }
        if com_ok.is_ok() {
            CoUninitialize();
        }
        res?;
        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn trim_or_transcode_mf_file(
    input_path: &Path,
    output_path: &Path,
    start_sec: Option<f64>,
    end_sec: Option<f64>,
    override_bitrate_bps: Option<u32>,
) -> Result<(), String> {
    unsafe {
        let com_ok = CoInitializeEx(None, COINIT_MULTITHREADED);
        let mf_ok = MFStartup(MF_API_VERSION, MFSTARTUP_FULL);

        let r_attrs = mf_reader_attributes();
        let in_wide = to_wide_null(&input_path.to_string_lossy());
        let reader = MFCreateSourceReaderFromURL(PCWSTR(in_wide.as_ptr()), r_attrs.as_ref())
            .map_err(|e| format!("Failed opening input MP4: {}", e))?;

        let v_info = reader
            .GetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM)
            .map_err(|e| format!("No video stream found: {}", e))?;
        let frame_size = v_info.GetUINT64(&MF_MT_FRAME_SIZE).unwrap_or(((1920u64) << 32) | 1080);
        let width = (frame_size >> 32) as u32;
        let height = (frame_size & 0xFFFF_FFFF) as u32;
        let frame_rate = v_info.GetUINT64(&MF_MT_FRAME_RATE).unwrap_or(((60u64) << 32) | 1);
        let fps = ((frame_rate >> 32) as u32).max(30);
        let bitrate = override_bitrate_bps.unwrap_or_else(|| v_info.GetUINT32(&MF_MT_AVG_BITRATE).unwrap_or(10_000_000));

        if let Ok(req_v) = MFCreateMediaType() {
            let _ = req_v.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video);
            let _ = req_v.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32);
            reader
                .SetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM, None, &req_v)
                .map_err(|e| format!("Video decoder setup failed: {}", e))?;
        }

        let has_audio = if let Ok(req_a) = MFCreateMediaType() {
            let _ = req_a.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio);
            let _ = req_a.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM);
            let _ = req_a.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, AUDIO_SAMPLE_RATE);
            let _ = req_a.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, AUDIO_CHANNELS);
            let _ = req_a.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16);
            reader
                .SetCurrentMediaType(MF_SOURCE_READER_FIRST_AUDIO_STREAM, None, &req_a)
                .is_ok()
        } else {
            false
        };

        let attrs = mf_writer_attributes()?;

        let out_wide = to_wide_null(&output_path.to_string_lossy());
        let writer = MFCreateSinkWriterFromURL(PCWSTR(out_wide.as_ptr()), None, Some(&attrs))
            .map_err(|e| format!("Writer failed: {}", e))?;

        let (v_out, a_out) = configure_sink_writer_streams(&writer, width, height, fps, bitrate, has_audio)
            .map_err(|e| format!("Stream config failed: {}", e))?;

        writer.BeginWriting().map_err(|e| format!("BeginWriting failed: {}", e))?;

        let start_100ns = (start_sec.unwrap_or(0.0).max(0.0) * 10_000_000.0) as i64;
        let end_100ns = end_sec.map(|e| (e.max(0.0) * 10_000_000.0) as i64);

        let mut base_offset: Option<i64> = None;

        loop {
            let mut stream_idx: u32 = 0;
            let mut flags: u32 = 0;
            let mut ts: i64 = 0;
            let mut sample: Option<IMFSample> = None;
            if reader
                .ReadSample(
                    MF_SOURCE_READER_ANY_STREAM,
                    0,
                    Some(&mut stream_idx),
                    Some(&mut flags),
                    Some(&mut ts),
                    Some(&mut sample),
                )
                .is_err()
            {
                break;
            }
            if (flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32) != 0 {
                break;
            }
            if let Some(end_limit) = end_100ns {
                if ts > end_limit {
                    break;
                }
            }
            if ts < start_100ns {
                continue;
            }
            if let Some(smp) = sample {
                let offset = *base_offset.get_or_insert(ts);
                let _ = smp.SetSampleTime((ts - offset).max(0));
                if stream_idx == 0 {
                    let _ = writer.WriteSample(v_out, &smp);
                } else if let Some(a_idx) = a_out {
                    let _ = writer.WriteSample(a_idx, &smp);
                }
            }
        }

        let res = writer.Finalize().map_err(|e| format!("Finalize trim/compress failed: {}", e));
        if mf_ok.is_ok() {
            let _ = MFShutdown();
        }
        if com_ok.is_ok() {
            CoUninitialize();
        }
        res
    }
}

// ======================== PURE-RUST MP4 DURATION PARSER (MVHD) ========================

fn parse_mp4_duration_secs(path: &Path) -> f64 {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return 0.0,
    };
    let file_len = file.metadata().map(|m| m.len()).unwrap_or(0);
    if file_len < 32 {
        return 0.0;
    }

    let mut pos = 0u64;
    let mut header = [0u8; 8];
    while pos + 8 <= file_len {
        if file.seek(SeekFrom::Start(pos)).is_err() || file.read_exact(&mut header).is_err() {
            break;
        }
        let size32 = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as u64;
        let box_type = &header[4..8];
        let mut hdr_len = 8u64;
        let box_size = if size32 == 1 {
            let mut ext = [0u8; 8];
            if file.read_exact(&mut ext).is_err() {
                break;
            }
            hdr_len = 16;
            u64::from_be_bytes(ext)
        } else if size32 == 0 {
            file_len - pos
        } else {
            size32
        };

        if box_size < hdr_len {
            break;
        }

        if box_type == b"moov" {
            return scan_moov_for_mvhd(&mut file, pos + hdr_len, pos + box_size);
        }
        pos += box_size;
    }
    0.0
}

fn scan_moov_for_mvhd(file: &mut File, mut pos: u64, limit: u64) -> f64 {
    let mut header = [0u8; 8];
    while pos + 8 <= limit {
        if file.seek(SeekFrom::Start(pos)).is_err() || file.read_exact(&mut header).is_err() {
            break;
        }
        let box_size = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as u64;
        if box_size < 8 {
            break;
        }
        if &header[4..8] == b"mvhd" && pos + 32 <= limit {
            let mut buf = [0u8; 32];
            if file.read_exact(&mut buf).is_ok() {
                let version = buf[0];
                if version == 0 {
                    let timescale = u32::from_be_bytes([buf[12], buf[13], buf[14], buf[15]]);
                    let duration = u32::from_be_bytes([buf[16], buf[17], buf[18], buf[19]]);
                    if timescale > 0 {
                        return (duration as f64 / timescale as f64 * 10.0).round() / 10.0;
                    }
                } else if version == 1 {
                    let timescale = u32::from_be_bytes([buf[20], buf[21], buf[22], buf[23]]);
                    let duration = u64::from_be_bytes([
                        buf[24], buf[25], buf[26], buf[27], buf[28], buf[29], buf[30], buf[31],
                    ]);
                    if timescale > 0 {
                        return (duration as f64 / timescale as f64 * 10.0).round() / 10.0;
                    }
                }
            }
            return 0.0;
        }
        pos += box_size;
    }
    0.0
}

// ======================== SAVE REPLAY & CLIP OPERATIONS ========================

async fn save_replay_internal(
    app: &AppHandle,
    game_override: Option<String>,
) -> Result<ClipEntry, String> {
    if ACTIVE_SAVE_IN_FLIGHT.swap(true, Ordering::SeqCst) {
        return Err("Clip save already in progress...".to_string());
    }

    let result = tokio::task::spawn_blocking({
        let app_clone = app.clone();
        move || {
            let cfg = get_config_lock(Some(&app_clone))
                .lock()
                .map(|g| g.clone())
                .unwrap_or_else(|_| default_config(Some(&app_clone)));

            let segments: Vec<EncodedSegment> = get_ring_buffer()
                .lock()
                .map(|rb| rb.iter().cloned().collect())
                .unwrap_or_default();

            let game_raw = game_override
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(resolve_active_game_tag);
            let game_name = sanitize_filename_part(&game_raw);

            let clips_root = PathBuf::from(&cfg.clips_dir);
            let game_dir = clips_root.join(&game_name);
            fs::create_dir_all(&game_dir)
                .map_err(|e| format!("Failed creating clip folder '{}': {}", game_dir.display(), e))?;

            let now_secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let stamp = format_timestamp_filename(now_secs);
            let file_name = format!("{}_{}.mp4", game_name, stamp);
            let out_path = game_dir.join(&file_name);

            #[cfg(target_os = "windows")]
            stitch_ram_segments_mf(&segments, &out_path, cfg.bitrate_kbps * 1000)?;

            let meta = fs::metadata(&out_path)
                .map_err(|e| format!("Saved clip metadata unreadable: {}", e))?;
            let duration_secs = parse_mp4_duration_secs(&out_path);

            let clip = ClipEntry {
                id: out_path.to_string_lossy().to_string(),
                file_name,
                full_path: out_path.to_string_lossy().to_string(),
                game_name,
                size_bytes: meta.len(),
                duration_secs,
                created_at: now_secs,
            };

            if let Some(bus) = DUKE_BUS.get() {
                bus.publish(DukeEvent::ClipSaved {
                    game_title: clip.game_name.clone(),
                    file_path: clip.full_path.clone(),
                    duration_secs: clip.duration_secs as f32,
                });
            }

            Ok(clip)
        }
    })
    .await
    .map_err(|e| format!("Save worker panicked: {}", e))?;

    ACTIVE_SAVE_IN_FLIGHT.store(false, Ordering::SeqCst);
    result
}

fn collect_clips_in_dir(root: &Path) -> Vec<ClipEntry> {
    let mut clips = Vec::new();
    if !root.exists() {
        return clips;
    }

    let mut dirs_to_visit = vec![root.to_path_buf()];
    while let Some(dir) = dirs_to_visit.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs_to_visit.push(path);
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("mp4"))
                .unwrap_or(false)
            {
                let file_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("clip.mp4")
                    .to_string();
                let game_name = path
                    .parent()
                    .and_then(|p| {
                        if p == root {
                            Some("Uncategorized")
                        } else {
                            p.file_name().and_then(|n| n.to_str())
                        }
                    })
                    .unwrap_or("Uncategorized")
                    .to_string();

                let meta = entry.metadata().ok();
                let size_bytes = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                let created_at = meta
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let duration_secs = parse_mp4_duration_secs(&path);

                clips.push(ClipEntry {
                    id: path.to_string_lossy().to_string(),
                    file_name,
                    full_path: path.to_string_lossy().to_string(),
                    game_name,
                    size_bytes,
                    duration_secs,
                    created_at,
                });
            }
        }
    }

    clips.sort_by_key(|a| std::cmp::Reverse(a.created_at));
    clips
}

fn build_state_dto(app: &AppHandle) -> CaptureStateDto {
    let cfg = get_config_lock(Some(app))
        .lock()
        .map(|g| g.clone())
        .unwrap_or_else(|_| default_config(Some(app)));
    let caps = get_hardware_caps();
    let (seg_count, total_ms, total_bytes) = get_ring_buffer()
        .lock()
        .map(|rb| {
            let count = rb.len();
            let ms: u64 = rb.iter().map(|s| s.duration_ms).sum();
            let bytes: usize = rb.iter().map(|s| s.bytes.len()).sum();
            (count, ms, bytes)
        })
        .unwrap_or((0, 0, 0));
    let running = REPLAY_RUNNING.load(Ordering::Relaxed);

    CaptureStateDto {
        replay_running: running,
        status_text: if running {
            format!("BUFFERING ({:.0}s / {}s IN RAM)", total_ms as f64 / 1000.0, cfg.buffer_seconds)
        } else {
            "IDLE - STANDBY".to_string()
        },
        buffer_seconds: cfg.buffer_seconds,
        buffered_segments: seg_count,
        buffered_seconds_actual: (total_ms as f64 / 1000.0 * 10.0).round() / 10.0,
        buffered_ram_mb: (total_bytes as f64 / (1024.0 * 1024.0) * 10.0).round() / 10.0,
        fps: cfg.fps,
        resolution: cfg.resolution,
        bitrate_kbps: cfg.bitrate_kbps,
        encoder_preference: cfg.encoder_preference,
        active_backend: caps.active_backend,
        available_encoders: caps.available_encoders,
        ffmpeg_installed: caps.ffmpeg_installed,
        ffmpeg_path: caps.ffmpeg_path,
        clips_dir: cfg.clips_dir,
        active_game: resolve_active_game_tag(),
        hotkey: cfg.hotkey,
        system_level_pct: SYSTEM_LEVEL_PCT.load(Ordering::Relaxed),
        mic_level_pct: MIC_LEVEL_PCT.load(Ordering::Relaxed),
        mic_device_id: cfg.mic_device_id,
        system_volume_pct: cfg.system_volume_pct,
        mic_gain_pct: cfg.mic_gain_pct,
        mic_muted: cfg.mic_muted,
        noise_gate_enabled: cfg.noise_gate_enabled,
        chime_volume_pct: cfg.chime_volume_pct,
        own_working_set_mb: get_process_working_set_mb(),
        doh_shield: "ACTIVE".to_string(),
        eco_scheduler: "READY".to_string(),
        auto_trim_enabled: AUTO_TRIM_ENABLED.load(Ordering::Relaxed),
    }
}

// ======================== TAURI COMMANDS ========================

#[tauri::command]
async fn get_capture_state(app: AppHandle) -> Result<CaptureStateDto, String> {
    Ok(build_state_dto(&app))
}

#[tauri::command]
async fn list_audio_input_devices() -> Result<Vec<AudioInputDevice>, String> {
    tokio::task::spawn_blocking(enumerate_mic_devices)
        .await
        .map_err(|e| format!("Audio enumeration failed: {}", e))
}

#[tauri::command]
async fn update_capture_config(app: AppHandle, patch: Value) -> Result<CaptureStateDto, String> {
    let mut need_audio_restart = false;
    let new_mic_id;

    {
        let mut cfg = get_config_lock(Some(&app))
            .lock()
            .map_err(|_| "Config lock poisoned".to_string())?;

        if let Some(v) = patch.get("bufferSeconds").and_then(|v| v.as_u64()) {
            let valid = match v {
                15 => 15,
                30 => 30,
                60 => 60,
                120 => 120,
                _ => v.clamp(10, 120) as u32,
            };
            cfg.buffer_seconds = valid;
        }
        if let Some(v) = patch.get("fps").and_then(|v| v.as_u64()) {
            cfg.fps = if v <= 30 { 30 } else { 60 };
        }
        if let Some(v) = patch.get("resolution").and_then(|v| v.as_str()) {
            if ["1080p", "1440p", "native"].contains(&v.to_lowercase().as_str()) {
                cfg.resolution = v.to_lowercase();
            }
        }
        if let Some(v) = patch.get("bitrateKbps").and_then(|v| v.as_u64()) {
            cfg.bitrate_kbps = (v as u32).clamp(4000, 30000);
        }
        if let Some(v) = patch.get("encoderPreference").and_then(|v| v.as_str()) {
            cfg.encoder_preference = v.to_string();
        }
        if let Some(v) = patch.get("clipsDir").and_then(|v| v.as_str()) {
            if !v.trim().is_empty() {
                cfg.clips_dir = v.trim().to_string();
            }
        }
        if let Some(v) = patch.get("micDeviceId") {
            let next_id = v.as_str().map(|s| s.to_string()).filter(|s| !s.trim().is_empty());
            if next_id != cfg.mic_device_id {
                cfg.mic_device_id = next_id.clone();
                need_audio_restart = true;
            }
            new_mic_id = cfg.mic_device_id.clone();
        } else {
            new_mic_id = cfg.mic_device_id.clone();
        }
        if let Some(v) = patch.get("systemVolumePct").and_then(|v| v.as_u64()) {
            cfg.system_volume_pct = (v as u32).min(150);
        }
        if let Some(v) = patch.get("micGainPct").and_then(|v| v.as_u64()) {
            cfg.mic_gain_pct = (v as u32).min(200);
        }
        if let Some(v) = patch.get("micMuted").and_then(|v| v.as_bool()) {
            cfg.mic_muted = v;
        }
        if let Some(v) = patch.get("noiseGateEnabled").and_then(|v| v.as_bool()) {
            cfg.noise_gate_enabled = v;
        }
        if let Some(v) = patch.get("chimeVolumePct").and_then(|v| v.as_u64()) {
            cfg.chime_volume_pct = (v as u32).min(100);
        }
        if let Some(v) = patch.get("autoTrimEnabled").and_then(|v| v.as_bool()) {
            cfg.auto_trim_enabled = v;
        }

        sync_atomics_from_config(&cfg);
        persist_config(&app, &cfg);
    }

    if need_audio_restart {
        let gen = AUDIO_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
        spawn_wasapi_workers(new_mic_id, gen);
    }

    Ok(build_state_dto(&app))
}

#[tauri::command]
async fn toggle_replay_buffer(app: AppHandle, enabled: bool) -> Result<CaptureStateDto, String> {
    if enabled && !REPLAY_RUNNING.load(Ordering::Relaxed) {
        spawn_capture_pipeline(app.clone());
    } else if !enabled {
        CAPTURE_STOP_SIGNAL.store(true, Ordering::Relaxed);
        REPLAY_RUNNING.store(false, Ordering::Relaxed);
        if let Ok(mut rb) = get_ring_buffer().lock() {
            rb.clear();
        }
    }
    Ok(build_state_dto(&app))
}

#[tauri::command]
async fn save_replay_now(app: AppHandle, game_override: Option<String>) -> Result<ClipEntry, String> {
    let chime_vol = CHIME_VOLUME_PCT.load(Ordering::Relaxed);
    play_confirmation_chime(chime_vol);
    let clip = save_replay_internal(&app, game_override).await?;
    let _ = app.emit("clip-saved", clip.clone());
    Ok(clip)
}

#[tauri::command]
async fn set_hotkey(
    app: AppHandle,
    label: String,
    vk: u32,
    modifiers: u32,
    is_mouse_xbutton: Option<u16>,
) -> Result<CaptureStateDto, String> {
    let binding = HotkeyBinding {
        label,
        vk,
        modifiers,
        is_mouse_xbutton,
    };
    {
        let mut cfg = get_config_lock(Some(&app))
            .lock()
            .map_err(|_| "Config lock failed".to_string())?;
        cfg.hotkey = binding.clone();
        sync_atomics_from_config(&cfg);
        persist_config(&app, &cfg);
    }
    notify_hotkey_rebind();
    Ok(build_state_dto(&app))
}

#[tauri::command]
async fn test_confirmation_chime(volume_pct: Option<u32>) -> Result<(), String> {
    let vol = volume_pct.unwrap_or_else(|| CHIME_VOLUME_PCT.load(Ordering::Relaxed));
    play_confirmation_chime(vol);
    Ok(())
}

#[tauri::command]
async fn list_clips(app: AppHandle) -> Result<Vec<ClipEntry>, String> {
    let dir_str = get_config_lock(Some(&app))
        .lock()
        .map(|c| c.clips_dir.clone())
        .unwrap_or_else(|_| resolve_default_clips_dir(Some(&app)).to_string_lossy().to_string());
    tokio::task::spawn_blocking(move || collect_clips_in_dir(Path::new(&dir_str)))
        .await
        .map_err(|e| format!("Clip scan failed: {}", e))
}

#[tauri::command]
async fn trim_clip(
    path: String,
    start_sec: f64,
    end_sec: f64,
) -> Result<ClipEntry, String> {
    if end_sec <= start_sec + 0.15 {
        return Err("Trim end must be at least 0.2s after start.".to_string());
    }
    let src = PathBuf::from(&path);
    if !src.exists() {
        return Err("Source clip not found.".to_string());
    }
    let parent = src.parent().ok_or("Invalid clip directory")?;
    let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("clip");
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let out_name = format!("{}_trim_{}.mp4", stem, now_secs % 10000);
    let out_path = parent.join(&out_name);

    let ff = find_ffmpeg_binary();
    let src_c = src.clone();
    let out_c = out_path.clone();

    tokio::task::spawn_blocking(move || {
        if let Some(ffmpeg_exe) = ff {
            // Fast <0.5s lossless keyframe copy trim via FFmpeg
            let status = Command::new(&ffmpeg_exe)
                .args([
                    "-y",
                    "-ss",
                    &format!("{:.3}", start_sec.max(0.0)),
                    "-to",
                    &format!("{:.3}", end_sec),
                    "-i",
                    &src_c.to_string_lossy(),
                    "-c",
                    "copy",
                    "-movflags",
                    "+faststart",
                    &out_c.to_string_lossy(),
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            if status.map(|s| s.success()).unwrap_or(false) && out_c.exists() {
                return Ok(());
            }
        }
        #[cfg(target_os = "windows")]
        trim_or_transcode_mf_file(&src_c, &out_c, Some(start_sec), Some(end_sec), None)
    })
    .await
    .map_err(|e| format!("Trim worker failed: {}", e))??;

    let meta = fs::metadata(&out_path).map_err(|e| format!("Trimmed clip missing: {}", e))?;
    let duration_secs = parse_mp4_duration_secs(&out_path);
    let game_name = parent
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Clip")
        .to_string();

    Ok(ClipEntry {
        id: out_path.to_string_lossy().to_string(),
        file_name: out_name,
        full_path: out_path.to_string_lossy().to_string(),
        game_name,
        size_bytes: meta.len(),
        duration_secs,
        created_at: now_secs,
    })
}

#[tauri::command]
async fn compress_clip_for_discord(
    path: String,
    target_mb: u32,
) -> Result<ClipEntry, String> {
    let target = target_mb.clamp(8, 50);
    let src = PathBuf::from(&path);
    if !src.exists() {
        return Err("Source clip not found.".to_string());
    }
    let duration = parse_mp4_duration_secs(&src).max(1.0);
    // Reserve 10% container/audio overhead so final MP4 fits comfortably under 10 MB / 25 MB Discord limit
    let target_bits = (target as f64) * 0.86 * 8.0 * 1024.0 * 1024.0;
    let total_bps = (target_bits / duration) as u32;
    let audio_kbps = if target <= 10 { 96u32 } else { 128u32 };
    let video_bps = total_bps.saturating_sub(audio_kbps * 1000).clamp(350_000, 12_000_000);

    let parent = src.parent().ok_or("Invalid clip directory")?;
    let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("clip");
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let out_name = format!("{}_discord_{}mb.mp4", stem, target);
    let out_path = parent.join(&out_name);

    let ff = find_ffmpeg_binary();
    let src_c = src.clone();
    let out_c = out_path.clone();
    tokio::task::spawn_blocking(move || {
        if let Some(ffmpeg_exe) = ff {
            let status = Command::new(&ffmpeg_exe)
                .args([
                    "-y",
                    "-i",
                    &src_c.to_string_lossy(),
                    "-b:v",
                    &format!("{}k", video_bps / 1000),
                    "-maxrate",
                    &format!("{}k", (video_bps / 1000) * 12 / 10),
                    "-bufsize",
                    &format!("{}k", (video_bps / 1000) * 2),
                    "-b:a",
                    &format!("{}k", audio_kbps),
                    "-movflags",
                    "+faststart",
                    &out_c.to_string_lossy(),
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            if status.map(|s| s.success()).unwrap_or(false) && out_c.exists() {
                return Ok(());
            }
        }
        #[cfg(target_os = "windows")]
        trim_or_transcode_mf_file(&src_c, &out_c, None, None, Some(video_bps))
    })
    .await
    .map_err(|e| format!("Discord compression worker failed: {}", e))??;

    let meta = fs::metadata(&out_path).map_err(|e| format!("Compressed file missing: {}", e))?;
    let game_name = parent
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Clip")
        .to_string();

    Ok(ClipEntry {
        id: out_path.to_string_lossy().to_string(),
        file_name: out_name,
        full_path: out_path.to_string_lossy().to_string(),
        game_name,
        size_bytes: meta.len(),
        duration_secs: parse_mp4_duration_secs(&out_path),
        created_at: now_secs,
    })
}

#[tauri::command]
async fn copy_clip_to_clipboard(path: String) -> Result<String, String> {
    let file_path = PathBuf::from(&path);
    if !file_path.exists() {
        return Err("Clip file no longer exists on disk.".to_string());
    }
    #[cfg(target_os = "windows")]
    {
        let canonical = fs::canonicalize(&file_path).unwrap_or(file_path.clone());
        let mut display = canonical.to_string_lossy().to_string();
        if let Some(stripped) = display.strip_prefix("\\\\?\\") {
            display = stripped.to_string();
        }
        let mut wide: Vec<u16> = display.encode_utf16().collect();
        wide.push(0); // single null terminator
        wide.push(0); // double null terminator required by CF_HDROP

        let header_size = std::mem::size_of::<DROPFILES>();
        let total_bytes = header_size + wide.len() * 2;

        unsafe {
            let hmem = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, total_bytes)
                .map_err(|e| format!("GlobalAlloc failed: {}", e))?;
            let ptr = GlobalLock(hmem) as *mut u8;
            if ptr.is_null() {
                return Err("GlobalLock failed for CF_HDROP".to_string());
            }

            let df = &mut *(ptr as *mut DROPFILES);
            df.pFiles = header_size as u32;
            df.fWide = true.into();

            std::ptr::copy_nonoverlapping(
                wide.as_ptr() as *const u8,
                ptr.add(header_size),
                wide.len() * 2,
            );
            let _ = GlobalUnlock(hmem);

            OpenClipboard(None).map_err(|e| format!("OpenClipboard failed: {}", e))?;
            let _ = EmptyClipboard();
            let set_res = SetClipboardData(CF_HDROP_FORMAT, Some(HANDLE(hmem.0)));
            let _ = CloseClipboard();
            set_res.map_err(|e| format!("SetClipboardData(CF_HDROP) failed: {}", e))?;
        }
        Ok(format!("Copied '{}' to clipboard (Press Ctrl+V in Discord).", display))
    }
    #[cfg(not(target_os = "windows"))]
    Err("CF_HDROP clipboard is only supported on Windows.".to_string())
}

#[tauri::command]
async fn open_clip_folder(app: AppHandle, path: Option<String>) -> Result<(), String> {
    if let Some(p) = path {
        let pb = PathBuf::from(&p);
        if pb.exists() {
            Command::new("explorer.exe")
                .args(["/select,", &pb.to_string_lossy()])
                .spawn()
                .map_err(|e| format!("Explorer error: {}", e))?;
            return Ok(());
        }
    }
    let dir = get_config_lock(Some(&app))
        .lock()
        .map(|c| c.clips_dir.clone())
        .unwrap_or_else(|_| resolve_default_clips_dir(Some(&app)).to_string_lossy().to_string());
    let _ = fs::create_dir_all(&dir);
    Command::new("explorer.exe")
        .arg(dir)
        .spawn()
        .map_err(|e| format!("Explorer error: {}", e))?;
    Ok(())
}

#[tauri::command]
async fn delete_clip(app: AppHandle, path: String) -> Result<(), String> {
    let target = PathBuf::from(&path);
    let clips_root = PathBuf::from(
        get_config_lock(Some(&app))
            .lock()
            .map(|c| c.clips_dir.clone())
            .unwrap_or_else(|_| resolve_default_clips_dir(Some(&app)).to_string_lossy().to_string()),
    );
    let canon_target = fs::canonicalize(&target).map_err(|_| "Clip file not found.".to_string())?;
    let canon_root = fs::canonicalize(&clips_root).map_err(|_| "Clips directory not found.".to_string())?;
    if !canon_target.starts_with(&canon_root) {
        return Err("Security Error: Refused deleting file outside Duke Clips root.".to_string());
    }
    fs::remove_file(&canon_target).map_err(|e| format!("Delete failed: {}", e))?;
    if let Some(parent) = canon_target.parent() {
        if parent != canon_root {
            let _ = fs::remove_dir(parent);
        }
    }
    Ok(())
}

#[tauri::command]
async fn install_ffmpeg_essentials(app: AppHandle) -> Result<HardwareCapabilities, String> {
    // Downloads official FFmpeg release zip from BtbN/FFmpeg-Builds via Encrypted DoH & extracts bin/ffmpeg.exe
    let api_url = "https://api.github.com/repos/BtbN/FFmpeg-Builds/releases/latest";
    let client = build_doh_client_for_url(api_url).await;
    let resp = client
        .get(api_url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("GitHub DoH query failed: {}", e))?;
    let json: Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed parsing FFmpeg release JSON: {}", e))?;

    let asset_url = json
        .get("assets")
        .and_then(|a| a.as_array())
        .and_then(|arr| {
            arr.iter().find_map(|item| {
                let name = item.get("name")?.as_str()?;
                if name.ends_with("win64-gpl.zip") && name.contains("ffmpeg-master-latest") {
                    item.get("browser_download_url")?.as_str().map(|s| s.to_string())
                } else {
                    None
                }
            })
        })
        .ok_or("Could not find win64-gpl FFmpeg asset URL.")?;

    let dl_client = build_doh_client_for_url(&asset_url).await;
    let mut dl_resp = dl_client
        .get(&asset_url)
        .send()
        .await
        .map_err(|e| format!("FFmpeg download failed: {}", e))?;

    let bin_dir = resolve_ffmpeg_bin_dir();
    fs::create_dir_all(&bin_dir).map_err(|e| format!("Failed creating bin folder: {}", e))?;
    let zip_path = bin_dir.join("ffmpeg_pkg.zip");
    let total = dl_resp.content_length().unwrap_or(0);
    let mut out_file = tokio::fs::File::create(&zip_path)
        .await
        .map_err(|e| format!("Create zip failed: {}", e))?;
    let mut downloaded = 0u64;
    let mut last_emit = Instant::now();

    while let Some(chunk) = dl_resp
        .chunk()
        .await
        .map_err(|e| format!("Stream read error: {}", e))?
    {
        out_file
            .write_all(&chunk)
            .await
            .map_err(|e| format!("Disk write error: {}", e))?;
        downloaded += chunk.len() as u64;
        if last_emit.elapsed() >= Duration::from_millis(200) {
            last_emit = Instant::now();
            let pct = if total > 0 {
                (downloaded as f64 / total as f64) * 100.0
            } else {
                0.0
            };
            let _ = app.emit(
                "ffmpeg-download-progress",
                serde_json::json!({
                    "downloadedBytes": downloaded,
                    "totalBytes": total,
                    "progressPct": (pct * 10.0).round() / 10.0
                }),
            );
        }
    }
    out_file.flush().await.ok();
    drop(out_file);

    // Extract only ffmpeg.exe cleanly via built-in PowerShell Expand-Archive into temp subdir
    let extract_dir = bin_dir.join("_extract");
    let _ = fs::remove_dir_all(&extract_dir);
    let ps_cmd = format!(
        "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
        zip_path.display(),
        extract_dir.display()
    );
    let _ = Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps_cmd])
        .creation_flags(CREATE_NO_WINDOW)
        .status();

    let mut found_ffmpeg: Option<PathBuf> = None;
    let mut stack = vec![extract_dir.clone()];
    while let Some(d) = stack.pop() {
        if let Ok(entries) = fs::read_dir(&d) {
            for ent in entries.flatten() {
                let p = ent.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.file_name().and_then(|n| n.to_str()) == Some("ffmpeg.exe") {
                    found_ffmpeg = Some(p);
                    break;
                }
            }
        }
    }

    let final_exe = bin_dir.join("ffmpeg.exe");
    if let Some(src_exe) = found_ffmpeg {
        fs::copy(&src_exe, &final_exe).map_err(|e| format!("Failed installing ffmpeg.exe: {}", e))?;
        unblock_ntfs_zone_identifier(&final_exe);
    } else {
        let _ = fs::remove_file(&zip_path);
        let _ = fs::remove_dir_all(&extract_dir);
        return Err("Downloaded archive did not contain ffmpeg.exe.".to_string());
    }
    let _ = fs::remove_file(&zip_path);
    let _ = fs::remove_dir_all(&extract_dir);

    let updated = probe_hardware_capabilities();
    if let Ok(mut guard) = HARDWARE_PROBE.get_or_init(|| Mutex::new(updated.clone())).lock() {
        *guard = updated.clone();
    }
    Ok(updated)
}

#[tauri::command]
async fn set_auto_trim_enabled(app: AppHandle, enabled: bool) -> Result<bool, String> {
    AUTO_TRIM_ENABLED.store(enabled, Ordering::Relaxed);
    if let Ok(mut cfg) = get_config_lock(Some(&app)).lock() {
        cfg.auto_trim_enabled = enabled;
        persist_config(&app, &cfg);
    }
    Ok(enabled)
}

#[tauri::command]
async fn trim_working_set(app: AppHandle) -> Result<Value, String> {
    trim_process_working_set();
    let ram_mb = get_process_working_set_mb();
    let payload = serde_json::json!({
        "workingSetMb": ram_mb,
        "dohShield": "ACTIVE",
        "ecoScheduler": "READY",
        "status": "IDLE TRIMMED"
    });
    let _ = app.emit("memory-telemetry", payload.clone());
    Ok(payload)
}

#[tauri::command]
async fn get_system_telemetry() -> Result<Value, String> {
    Ok(serde_json::json!({
        "workingSetMb": get_process_working_set_mb(),
        "dohShield": "ACTIVE",
        "ecoScheduler": "READY",
        "autoTrimEnabled": AUTO_TRIM_ENABLED.load(Ordering::Relaxed),
        "portableMode": resolve_portable_data_dir().is_some(),
    }))
}

fn query_exe_stem_by_pid(pid: u32) -> Option<String> {
    if pid <= 4 {
        return None;
    }
    #[cfg(target_os = "windows")]
    unsafe {
        if let Ok(proc_h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            let mut path_buf = [0u16; 512];
            let mut size = path_buf.len() as u32;
            let ok = QueryFullProcessImageNameW(
                proc_h,
                PROCESS_NAME_WIN32,
                PWSTR(path_buf.as_mut_ptr()),
                &mut size,
            );
            let _ = CloseHandle(proc_h);
            if ok.is_ok() && size > 0 {
                let exe_path = String::from_utf16_lossy(&path_buf[..size as usize]);
                return Path::new(&exe_path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_string());
            }
        }
    }
    None
}

fn handle_game_launched_tag(app: &AppHandle, pid: u32, title: &str, exe_name: &str) {
    let trimmed_title = title.trim();
    let friendly = if !trimmed_title.is_empty() && !trimmed_title.to_ascii_lowercase().ends_with(".exe") {
        trimmed_title.to_string()
    } else {
        let stem_from_arg = Path::new(exe_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let stem_from_title = Path::new(trimmed_title)
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        if let Some(stem) = stem_from_arg.or(stem_from_title).or_else(|| query_exe_stem_by_pid(pid)) {
            map_friendly_game_name(&stem, trimmed_title)
        } else {
            detect_foreground_game_name()
        }
    };

    let mut changed = false;
    if let Ok(mut guard) = active_launcher_game_lock().lock() {
        let next = Some((pid, friendly.clone()));
        if *guard != next {
            *guard = next;
            changed = true;
        }
    }
    if changed {
        let _ = app.emit(
            "active-game-updated",
            serde_json::json!({
                "activeGame": friendly,
                "pid": pid
            }),
        );
    }
}

fn handle_game_exited_tag(app: &AppHandle, pid: u32) {
    let mut cleared = false;
    if let Ok(mut guard) = active_launcher_game_lock().lock() {
        let should_clear = match *guard {
            Some((active_pid, _)) => pid == 0 || active_pid == 0 || active_pid == pid,
            None => false,
        };
        if should_clear {
            *guard = None;
            cleared = true;
        }
    }
    if !REPLAY_RUNNING.load(Ordering::Relaxed) {
        duke_core::win32_ffi::trim_current_working_set();
    }
    if cleared {
        let _ = app.emit(
            "active-game-updated",
            serde_json::json!({
                "activeGame": resolve_active_game_tag()
            }),
        );
    }
}

fn main() {
    let daemon = TrayDaemonState::new(TrayDaemonConfig {
        tray_id: "duke-capture-tray".to_string(),
        window_label: "main".to_string(),
        tooltip: "Duke Capture - Instant Replay & Clip Engine".to_string(),
        open_label: "Open Duke Capture".to_string(),
        trim_label: "Trim Capture Memory".to_string(),
        exit_label: "Quit Duke Capture".to_string(),
        close_to_tray: Arc::new(AtomicBool::new(true)),
        minimize_to_tray: Arc::new(AtomicBool::new(true)),
    });
    let daemon_setup = daemon.clone();
    let daemon_run = daemon;

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .setup(move |app| {
            let handle = app.handle().clone();
            let _ = APP_HANDLE.set(handle.clone());
            let initial_cfg = get_config_lock(Some(&handle))
                .lock()
                .map(|c| c.clone())
                .unwrap_or_else(|_| default_config(Some(&handle)));
            let _ = fs::create_dir_all(&initial_cfg.clips_dir);
            let _ = HARDWARE_PROBE.set(Mutex::new(probe_hardware_capabilities()));

            install_standard_tray(
                app.handle(),
                &daemon_setup,
                Some(Arc::new(|| {
                    duke_core::win32_ffi::restore_normal_priority_mode();
                })),
                Some(Arc::new(|| {
                    SHM_STOP_SIGNAL.store(true, Ordering::Release);
                    if let Some(Some(bus)) = DUKE_SHM_BUS.get() {
                        bus.unregister_app();
                    }
                })),
            )?;

            // Connect to lock-free Shared Memory Ring Buffer (`Local\duke-suite-shm-v1`)
            let _ = DUKE_SHM_BUS.get_or_init(|| {
                let bus = DukeShmBus::open_or_create().ok();
                if let Some(ref b) = bus {
                    b.register_app(ShmSourceApp::DukeCapture);
                }
                bus
            });

            // Subscribe to GameLaunched / GameExited SHM frames for nanosecond replay auto-tagging
            let shm_app = handle.clone();
            let _ = thread::Builder::new()
                .name("duke-capture-shm-rx".to_string())
                .spawn(move || {
                    let Ok(mut shm_rx) = DukeShmBus::open_or_create() else {
                        return;
                    };
                    while !SHM_STOP_SIGNAL.load(Ordering::Acquire) {
                        let mut processed = false;
                        while let Some(frame) = shm_rx.try_recv() {
                            processed = true;
                            match frame.kind() {
                                ShmEventType::GameLaunched => {
                                    let tag = frame.tag_str();
                                    let console_label = duke_core::shm_bus::unpack_game_launch_platform_id(frame.payload_b)
                                        .and_then(duke_core::shm_bus::console_label_for_platform_id);
                                    let trimmed = tag.trim();
                                    let enriched_title = match console_label {
                                        Some(sys) if !trimmed.is_empty() && !trimmed.to_ascii_lowercase().starts_with(&sys.to_ascii_lowercase()) => {
                                            format!("{} - {}", sys, trimmed)
                                        }
                                        Some(sys) if trimmed.is_empty() => sys.to_string(),
                                        _ => trimmed.to_string(),
                                    };
                                    handle_game_launched_tag(&shm_app, frame.pid, &enriched_title, tag);
                                }
                                ShmEventType::GameExited => {
                                    handle_game_exited_tag(&shm_app, frame.pid);
                                }
                                ShmEventType::RamPurge
                                    if !REPLAY_RUNNING.load(Ordering::Relaxed) =>
                                {
                                    duke_core::win32_ffi::trim_current_working_set();
                                }
                                ShmEventType::ReplaySaveRequested
                                    if REPLAY_RUNNING.load(Ordering::Relaxed) =>
                                {
                                    let clip_app = shm_app.clone();
                                    let tag = frame.tag_str().trim().to_string();
                                    let override_title = if tag.is_empty() { None } else { Some(tag) };
                                    tauri::async_runtime::spawn(async move {
                                        if let Ok(clip) = save_replay_internal(&clip_app, override_title).await {
                                            let _ = clip_app.emit("clip-saved", clip);
                                        }
                                    });
                                }
                                _ => {}
                            }
                        }
                        thread::sleep(Duration::from_millis(if processed { 1 } else { 5 }));
                    }
                });

            // Subscribe to Duke Suite pipe IPC bus to auto-tag active game clips
            let bus_app = handle.clone();
            let _ = DUKE_BUS.get_or_init(|| {
                DukeBus::start("duke-capture", move |event| match event {
                    DukeEvent::GameStarted {
                        title,
                        exe_name,
                        pid,
                    } => {
                        handle_game_launched_tag(&bus_app, pid, &title, &exe_name);
                    }
                    DukeEvent::GameStopped { pid, .. } => {
                        handle_game_exited_tag(&bus_app, pid);
                    }
                    DukeEvent::RamPurgeRequested { .. }
                        if !REPLAY_RUNNING.load(Ordering::Relaxed) =>
                    {
                        duke_core::win32_ffi::trim_current_working_set();
                    }
                    DukeEvent::ReplaySaveRequested { game_title, .. }
                        if REPLAY_RUNNING.load(Ordering::Relaxed) =>
                    {
                        let clip_app = bus_app.clone();
                        tauri::async_runtime::spawn(async move {
                            if let Ok(clip) = save_replay_internal(&clip_app, game_title).await {
                                let _ = clip_app.emit("clip-saved", clip);
                            }
                        });
                    }
                    _ => {}
                })
            });

            spawn_hotkey_listener_thread();
            let gen = AUDIO_GENERATION.load(Ordering::Relaxed);
            spawn_wasapi_workers(initial_cfg.mic_device_id.clone(), gen);

            trim_process_working_set();

            // Live VU Meter & Idle RAM Trimmer Task
            let app_meter = handle.clone();
            tauri::async_runtime::spawn(async move {
                let mut ticker = tokio::time::interval(Duration::from_millis(85));
                let mut trim_ticks: u32 = 0;
                loop {
                    ticker.tick().await;
                    let _ = app_meter.emit(
                        "audio-levels",
                        serde_json::json!({
                            "systemLevelPct": SYSTEM_LEVEL_PCT.load(Ordering::Relaxed),
                            "micLevelPct": MIC_LEVEL_PCT.load(Ordering::Relaxed),
                        }),
                    );
                    trim_ticks += 1;
                    // Every ~60 seconds (705 * 85ms)
                    if trim_ticks >= 705 {
                        trim_ticks = 0;
                        if AUTO_TRIM_ENABLED.load(Ordering::Relaxed) && !REPLAY_RUNNING.load(Ordering::Relaxed) {
                            trim_process_working_set();
                        }
                        let _ = app_meter.emit(
                            "memory-telemetry",
                            serde_json::json!({
                                "workingSetMb": get_process_working_set_mb(),
                                "dohShield": "ACTIVE",
                                "ecoScheduler": "READY",
                            }),
                        );
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_capture_state,
            list_audio_input_devices,
            update_capture_config,
            toggle_replay_buffer,
            save_replay_now,
            set_hotkey,
            test_confirmation_chime,
            list_clips,
            trim_clip,
            compress_clip_for_discord,
            copy_clip_to_clipboard,
            open_clip_folder,
            delete_clip,
            install_ffmpeg_essentials,
            set_auto_trim_enabled,
            trim_working_set,
            get_system_telemetry
        ])
        .build(tauri::generate_context!())
        .expect("error while building Duke Capture Tauri application");

    app.run(move |app_handle, event| {
        handle_run_event(
            app_handle,
            &event,
            &daemon_run,
            Some(&|| {
                if !REPLAY_RUNNING.load(Ordering::Relaxed) {
                    duke_core::win32_ffi::enable_eco_qos_mode();
                }
            }),
            Some(&|| {
                SHM_STOP_SIGNAL.store(true, Ordering::Release);
                if let Some(Some(bus)) = DUKE_SHM_BUS.get() {
                    bus.unregister_app();
                }
            }),
        );
    });
}

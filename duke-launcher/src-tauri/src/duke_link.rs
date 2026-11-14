use base64::Engine as _;
use duke_core::shm_bus::{ShmCoOpInputP2Payload, ShmEventFrame, ShmSourceApp};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

pub const DUKE_LINK_DEFAULT_PORT: u16 = 47882;
const LINK_MAGIC: &[u8; 6] = b"DKLNK1";
const MAX_CHUNK_BYTES: usize = 1280;
const FRAME_CAP_WIDTH: u32 = 960;
const FRAME_CAP_HEIGHT: u32 = 540;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum LinkPacketKind {
    Hello = 1,
    HelloAck = 2,
    VideoFrameChunk = 3,
    InputStateP2 = 4,
    Ping = 5,
    Pong = 6,
    Bye = 7,
}

impl LinkPacketKind {
    fn from_u8(val: u8) -> Option<Self> {
        match val {
            1 => Some(Self::Hello),
            2 => Some(Self::HelloAck),
            3 => Some(Self::VideoFrameChunk),
            4 => Some(Self::InputStateP2),
            5 => Some(Self::Ping),
            6 => Some(Self::Pong),
            7 => Some(Self::Bye),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CoOpInputP2 {
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub thumb_lx: i16,
    pub thumb_ly: i16,
    pub thumb_rx: i16,
    pub thumb_ry: i16,
    pub sequence: u32,
}

impl CoOpInputP2 {
    pub fn to_shm_payload(&self) -> ShmCoOpInputP2Payload {
        ShmCoOpInputP2Payload {
            buttons: self.buttons,
            left_trigger: self.left_trigger,
            right_trigger: self.right_trigger,
            thumb_lx: self.thumb_lx,
            thumb_ly: self.thumb_ly,
            thumb_rx: self.thumb_rx,
            thumb_ry: self.thumb_ry,
            sequence: self.sequence,
        }
    }

    pub fn encode_bytes(&self) -> [u8; 16] {
        let mut out = [0u8; 16];
        out[0..2].copy_from_slice(&self.buttons.to_le_bytes());
        out[2] = self.left_trigger;
        out[3] = self.right_trigger;
        out[4..6].copy_from_slice(&self.thumb_lx.to_le_bytes());
        out[6..8].copy_from_slice(&self.thumb_ly.to_le_bytes());
        out[8..10].copy_from_slice(&self.thumb_rx.to_le_bytes());
        out[10..12].copy_from_slice(&self.thumb_ry.to_le_bytes());
        out[12..16].copy_from_slice(&self.sequence.to_le_bytes());
        out
    }

    pub fn decode_bytes(buf: &[u8]) -> Result<Self, String> {
        if buf.len() < 16 {
            return Err("Co-op P2 input packet truncated (<16 bytes).".to_string());
        }
        Ok(Self {
            buttons: u16::from_le_bytes([buf[0], buf[1]]),
            left_trigger: buf[2],
            right_trigger: buf[3],
            thumb_lx: i16::from_le_bytes([buf[4], buf[5]]),
            thumb_ly: i16::from_le_bytes([buf[6], buf[7]]),
            thumb_rx: i16::from_le_bytes([buf[8], buf[9]]),
            thumb_ry: i16::from_le_bytes([buf[10], buf[11]]),
            sequence: u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DukeLinkPacket {
    Hello {
        room_code: String,
    },
    HelloAck {
        codec: String,
    },
    VideoFrameChunk {
        frame_id: u32,
        chunk_idx: u16,
        total_chunks: u16,
        width: u16,
        height: u16,
        payload: Vec<u8>,
    },
    InputStateP2(CoOpInputP2),
    Ping {
        sent_us: u64,
    },
    Pong {
        echo_us: u64,
    },
    Bye,
}

impl DukeLinkPacket {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(64);
        out.extend_from_slice(LINK_MAGIC);
        match self {
            Self::Hello { room_code } => {
                out.push(LinkPacketKind::Hello as u8);
                let bytes = room_code.as_bytes();
                let len = bytes.len().min(64) as u8;
                out.push(len);
                out.extend_from_slice(&bytes[..len as usize]);
            }
            Self::HelloAck { codec } => {
                out.push(LinkPacketKind::HelloAck as u8);
                let bytes = codec.as_bytes();
                let len = bytes.len().min(64) as u8;
                out.push(len);
                out.extend_from_slice(&bytes[..len as usize]);
            }
            Self::VideoFrameChunk {
                frame_id,
                chunk_idx,
                total_chunks,
                width,
                height,
                payload,
            } => {
                out.push(LinkPacketKind::VideoFrameChunk as u8);
                out.extend_from_slice(&frame_id.to_le_bytes());
                out.extend_from_slice(&chunk_idx.to_le_bytes());
                out.extend_from_slice(&total_chunks.to_le_bytes());
                out.extend_from_slice(&width.to_le_bytes());
                out.extend_from_slice(&height.to_le_bytes());
                out.extend_from_slice(payload);
            }
            Self::InputStateP2(input) => {
                out.push(LinkPacketKind::InputStateP2 as u8);
                out.extend_from_slice(&input.encode_bytes());
            }
            Self::Ping { sent_us } => {
                out.push(LinkPacketKind::Ping as u8);
                out.extend_from_slice(&sent_us.to_le_bytes());
            }
            Self::Pong { echo_us } => {
                out.push(LinkPacketKind::Pong as u8);
                out.extend_from_slice(&echo_us.to_le_bytes());
            }
            Self::Bye => {
                out.push(LinkPacketKind::Bye as u8);
            }
        }
        out
    }

    pub fn decode(buf: &[u8]) -> Result<Self, String> {
        if buf.len() < 7 || &buf[..6] != LINK_MAGIC {
            return Err("Invalid Duke Link magic header.".to_string());
        }
        let kind = LinkPacketKind::from_u8(buf[6])
            .ok_or_else(|| format!("Unknown Duke Link packet opcode {}", buf[6]))?;
        let body = &buf[7..];
        match kind {
            LinkPacketKind::Hello => {
                let len = *body.first().ok_or("Empty Hello packet.")? as usize;
                if body.len() < 1 + len {
                    return Err("Truncated Hello room code.".to_string());
                }
                let room_code = String::from_utf8_lossy(&body[1..1 + len]).into_owned();
                Ok(Self::Hello { room_code })
            }
            LinkPacketKind::HelloAck => {
                let len = *body.first().ok_or("Empty HelloAck packet.")? as usize;
                if body.len() < 1 + len {
                    return Err("Truncated HelloAck codec.".to_string());
                }
                let codec = String::from_utf8_lossy(&body[1..1 + len]).into_owned();
                Ok(Self::HelloAck { codec })
            }
            LinkPacketKind::VideoFrameChunk => {
                if body.len() < 12 {
                    return Err("Truncated VideoFrameChunk header.".to_string());
                }
                let frame_id = u32::from_le_bytes([body[0], body[1], body[2], body[3]]);
                let chunk_idx = u16::from_le_bytes([body[4], body[5]]);
                let total_chunks = u16::from_le_bytes([body[6], body[7]]);
                let width = u16::from_le_bytes([body[8], body[9]]);
                let height = u16::from_le_bytes([body[10], body[11]]);
                Ok(Self::VideoFrameChunk {
                    frame_id,
                    chunk_idx,
                    total_chunks,
                    width,
                    height,
                    payload: body[12..].to_vec(),
                })
            }
            LinkPacketKind::InputStateP2 => Ok(Self::InputStateP2(CoOpInputP2::decode_bytes(body)?)),
            LinkPacketKind::Ping => {
                if body.len() < 8 {
                    return Err("Truncated Ping packet.".to_string());
                }
                let sent_us = u64::from_le_bytes([
                    body[0], body[1], body[2], body[3], body[4], body[5], body[6], body[7],
                ]);
                Ok(Self::Ping { sent_us })
            }
            LinkPacketKind::Pong => {
                if body.len() < 8 {
                    return Err("Truncated Pong packet.".to_string());
                }
                let echo_us = u64::from_le_bytes([
                    body[0], body[1], body[2], body[3], body[4], body[5], body[6], body[7],
                ]);
                Ok(Self::Pong { echo_us })
            }
            LinkPacketKind::Bye => Ok(Self::Bye),
        }
    }
}

#[derive(Default)]
pub struct FrameReassembler {
    active_frame_id: u32,
    expected_chunks: u16,
    width: u16,
    height: u16,
    received_mask: Vec<bool>,
    chunks: Vec<Vec<u8>>,
    received_count: u16,
}

impl FrameReassembler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_chunk(
        &mut self,
        frame_id: u32,
        chunk_idx: u16,
        total_chunks: u16,
        width: u16,
        height: u16,
        payload: Vec<u8>,
    ) -> Option<(u32, u16, u16, Vec<u8>)> {
        if total_chunks == 0 || chunk_idx >= total_chunks {
            return None;
        }
        if frame_id > self.active_frame_id {
            self.active_frame_id = frame_id;
            self.expected_chunks = total_chunks;
            self.width = width;
            self.height = height;
            self.received_mask = vec![false; total_chunks as usize];
            self.chunks = vec![Vec::new(); total_chunks as usize];
            self.received_count = 0;
        } else if frame_id < self.active_frame_id || total_chunks != self.expected_chunks {
            return None;
        }

        let slot = chunk_idx as usize;
        if slot < self.received_mask.len() && !self.received_mask[slot] {
            self.received_mask[slot] = true;
            self.chunks[slot] = payload;
            self.received_count = self.received_count.saturating_add(1);
        }

        if self.received_count == self.expected_chunks && self.expected_chunks > 0 {
            let total_len: usize = self.chunks.iter().map(|c| c.len()).sum();
            let mut joined = Vec::with_capacity(total_len);
            for c in self.chunks.drain(..) {
                joined.extend_from_slice(&c);
            }
            self.expected_chunks = 0;
            self.received_count = 0;
            return Some((self.active_frame_id, self.width, self.height, joined));
        }
        None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DukeLinkStatusSnapshot {
    pub active: bool,
    pub role: String,
    pub game_title: String,
    pub room_code: String,
    pub bind_port: u16,
    pub advertise_addr: String,
    pub peer_addr: Option<String>,
    pub friend_name: Option<String>,
    pub codec: String,
    pub capture_pipeline: String,
    pub input_bridge: String,
    pub rtt_ms: f64,
    pub fps: f64,
    pub bitrate_kbps: u64,
    pub p2_input_packets: u64,
    pub frames_transmitted: u64,
    pub status_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoOpInviteResult {
    pub session: DukeLinkStatusSnapshot,
    pub invite_uri: String,
    pub delivery_note: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CoOpFrameEvent {
    frame_id: u32,
    width: u16,
    height: u16,
    codec: String,
    rtt_ms: f64,
    fps: f64,
    bitrate_kbps: u64,
    frame_b64: String,
}

struct SessionRuntime {
    stop_flag: Arc<AtomicBool>,
    role: String,
    game_title: String,
    room_code: String,
    bind_port: u16,
    advertise_addr: String,
    peer_addr: Option<String>,
    friend_name: Option<String>,
    codec: String,
    capture_pipeline: String,
    input_bridge: String,
    rtt_us: Arc<AtomicU64>,
    fps_x10: Arc<AtomicU32>,
    bitrate_kbps: Arc<AtomicU64>,
    p2_packets: Arc<AtomicU64>,
    frames_count: Arc<AtomicU64>,
    guest_socket: Option<Arc<UdpSocket>>,
    guest_peer: Option<SocketAddr>,
}

impl Default for SessionRuntime {
    fn default() -> Self {
        Self {
            stop_flag: Arc::new(AtomicBool::new(false)),
            role: "idle".to_string(),
            game_title: String::new(),
            room_code: String::new(),
            bind_port: DUKE_LINK_DEFAULT_PORT,
            advertise_addr: String::new(),
            peer_addr: None,
            friend_name: None,
            codec: "none".to_string(),
            capture_pipeline: detect_capture_pipeline(),
            input_bridge: detect_input_bridge(),
            rtt_us: Arc::new(AtomicU64::new(0)),
            fps_x10: Arc::new(AtomicU32::new(0)),
            bitrate_kbps: Arc::new(AtomicU64::new(0)),
            p2_packets: Arc::new(AtomicU64::new(0)),
            frames_count: Arc::new(AtomicU64::new(0)),
            guest_socket: None,
            guest_peer: None,
        }
    }
}

static DUKE_LINK_STATE: OnceLock<Mutex<SessionRuntime>> = OnceLock::new();

fn link_state() -> &'static Mutex<SessionRuntime> {
    DUKE_LINK_STATE.get_or_init(|| Mutex::new(SessionRuntime::default()))
}

fn now_unix_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}

fn generate_room_code() -> String {
    let seed = format!("{}-{}", std::process::id(), now_unix_us());
    let digest = blake3::hash(seed.as_bytes()).to_hex().to_ascii_uppercase();
    format!("DUKE-{}", &digest[..6])
}

fn detect_hardware_encoder_codec() -> String {
    let sys_dir = std::env::var_os("SystemRoot")
        .map(|root| std::path::PathBuf::from(root).join("System32"))
        .unwrap_or_else(|| std::path::PathBuf::from("System32"));
    if sys_dir.join("nvEncodeAPI64.dll").is_file() {
        return "DXGI+NVENC-H264 / UDP Low-Latency".to_string();
    }
    if sys_dir.join("amfrt64.dll").is_file() {
        return "DXGI+AMF-HEVC / UDP Low-Latency".to_string();
    }
    if sys_dir.join("libmfx64.dll").is_file() {
        return "DXGI+QSV-H264 / UDP Low-Latency".to_string();
    }
    "DXGI/GDI-JPEG60 / UDP Low-Latency".to_string()
}

fn detect_capture_pipeline() -> String {
    let active = crate::active_shm_apps();
    if active.contains(&ShmSourceApp::DukeCapture) {
        "duke-capture SHM + DXGI Desktop Duplication".to_string()
    } else {
        "Embedded Win32 GDI/DXGI Capture".to_string()
    }
}

fn detect_input_bridge() -> String {
    let active = crate::active_shm_apps();
    if active.contains(&ShmSourceApp::DukeInput) {
        "duke-input SHM Virtual P2 XInput".to_string()
    } else {
        "SHM CoOpInputP2 Queue (duke-input Standby)".to_string()
    }
}

fn discover_best_local_ip() -> String {
    if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
        // Probe Tailscale / internet outbound interface resolution locally without sending traffic if unconnected.
        if sock.connect("100.100.100.100:80").is_ok() {
            if let Ok(addr) = sock.local_addr() {
                if !addr.ip().is_unspecified() && !addr.ip().is_loopback() {
                    return addr.ip().to_string();
                }
            }
        }
        if sock.connect("8.8.8.8:80").is_ok() {
            if let Ok(addr) = sock.local_addr() {
                if !addr.ip().is_unspecified() {
                    return addr.ip().to_string();
                }
            }
        }
    }
    "127.0.0.1".to_string()
}

fn snapshot_from_runtime(rt: &SessionRuntime) -> DukeLinkStatusSnapshot {
    let active = rt.role != "idle";
    let rtt_ms = (rt.rtt_us.load(Ordering::Acquire) as f64) / 1000.0;
    let fps = (rt.fps_x10.load(Ordering::Acquire) as f64) / 10.0;
    let bitrate_kbps = rt.bitrate_kbps.load(Ordering::Acquire);
    let p2_input_packets = rt.p2_packets.load(Ordering::Acquire);
    let frames_transmitted = rt.frames_count.load(Ordering::Acquire);
    let status_text = match rt.role.as_str() {
        "host" => {
            if let Some(ref peer) = rt.peer_addr {
                format!(
                    "Streaming '{}' to {} ({:.1} ms RTT, {:.0} FPS)",
                    rt.game_title, peer, rtt_ms, fps
                )
            } else {
                format!(
                    "Hosting '{}' on {} ({}) — Waiting for friend",
                    rt.game_title, rt.advertise_addr, rt.room_code
                )
            }
        }
        "guest" => format!(
            "Connected to {} ({:.1} ms RTT, {:.0} FPS)",
            rt.peer_addr.as_deref().unwrap_or("Host"),
            rtt_ms,
            fps
        ),
        _ => "Idle (0 MB overhead — start host or join friend)".to_string(),
    };
    DukeLinkStatusSnapshot {
        active,
        role: rt.role.clone(),
        game_title: rt.game_title.clone(),
        room_code: rt.room_code.clone(),
        bind_port: rt.bind_port,
        advertise_addr: rt.advertise_addr.clone(),
        peer_addr: rt.peer_addr.clone(),
        friend_name: rt.friend_name.clone(),
        codec: rt.codec.clone(),
        capture_pipeline: rt.capture_pipeline.clone(),
        input_bridge: rt.input_bridge.clone(),
        rtt_ms,
        fps,
        bitrate_kbps,
        p2_input_packets,
        frames_transmitted,
        status_text,
    }
}

pub(crate) fn forward_p2_input_to_shm(input: CoOpInputP2) {
    let frame = ShmEventFrame::coop_input_p2(
        ShmSourceApp::DukeLauncher,
        std::process::id(),
        input.to_shm_payload(),
    );
    crate::publish_shm_frame_direct(frame);
}

#[cfg(target_os = "windows")]
fn capture_scaled_desktop_jpeg(max_w: u32, max_h: u32, quality: u8) -> Option<(u16, u16, Vec<u8>)> {
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
        SRCCOPY,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

    let (src_w, src_h, bgra) = unsafe {
        let sw = GetSystemMetrics(SM_CXSCREEN).max(1) as u32;
        let sh = GetSystemMetrics(SM_CYSCREEN).max(1) as u32;
        let hdc_screen = GetDC(None);
        if hdc_screen.is_invalid() {
            return None;
        }
        let hdc_mem = CreateCompatibleDC(Some(hdc_screen));
        let hbm = CreateCompatibleBitmap(hdc_screen, sw as i32, sh as i32);
        let old = SelectObject(hdc_mem, hbm.into());
        let _ = BitBlt(
            hdc_mem,
            0,
            0,
            sw as i32,
            sh as i32,
            Some(hdc_screen),
            0,
            0,
            SRCCOPY,
        );
        let mut bi = BITMAPINFO::default();
        bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bi.bmiHeader.biWidth = sw as i32;
        bi.bmiHeader.biHeight = -(sh as i32);
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 32;
        bi.bmiHeader.biCompression = BI_RGB.0;
        let mut pixels = vec![0u8; (sw * sh * 4) as usize];
        let scan = GetDIBits(
            hdc_mem,
            hbm,
            0,
            sh,
            Some(pixels.as_mut_ptr() as *mut std::ffi::c_void),
            &mut bi,
            DIB_RGB_COLORS,
        );
        let _ = SelectObject(hdc_mem, old);
        let _ = DeleteObject(hbm.into());
        let _ = DeleteDC(hdc_mem);
        let _ = ReleaseDC(None, hdc_screen);
        if scan == 0 {
            return None;
        }
        (sw, sh, pixels)
    };

    // Convert BGRA -> RGB in place before resizing for ultra-fast JPEG encoding.
    let mut rgb = Vec::with_capacity((src_w * src_h * 3) as usize);
    for px in bgra.chunks_exact(4) {
        rgb.push(px[2]);
        rgb.push(px[1]);
        rgb.push(px[0]);
    }

    let img = image::RgbImage::from_raw(src_w, src_h, rgb)?;
    let dyn_img = image::DynamicImage::ImageRgb8(img);
    let target_w = max_w.min(src_w).max(160);
    let target_h = max_h.min(src_h).max(90);
    let resized = if target_w != src_w || target_h != src_h {
        dyn_img.resize_exact(target_w, target_h, image::imageops::FilterType::Nearest)
    } else {
        dyn_img
    };

    let mut jpeg_bytes = Vec::with_capacity(28 * 1024);
    let mut encoder =
        image::codecs::jpeg::JpegEncoder::new_with_quality(Cursor::new(&mut jpeg_bytes), quality);
    encoder.encode_image(&resized).ok()?;
    Some((target_w as u16, target_h as u16, jpeg_bytes))
}

#[cfg(not(target_os = "windows"))]
fn capture_scaled_desktop_jpeg(_max_w: u32, _max_h: u32, _quality: u8) -> Option<(u16, u16, Vec<u8>)> {
    None
}

struct HostLoopParams {
    app: AppHandle,
    socket: UdpSocket,
    codec: String,
    room_code: String,
    stop_flag: Arc<AtomicBool>,
    rtt_us: Arc<AtomicU64>,
    fps_x10: Arc<AtomicU32>,
    bitrate_kbps: Arc<AtomicU64>,
    p2_packets: Arc<AtomicU64>,
    frames_count: Arc<AtomicU64>,
}

fn spawn_host_loop(params: HostLoopParams) {
    let HostLoopParams {
        app,
        socket,
        codec,
        room_code,
        stop_flag,
        rtt_us,
        fps_x10,
        bitrate_kbps,
        p2_packets,
        frames_count,
    } = params;
    let _ = socket.set_read_timeout(Some(Duration::from_millis(8)));
    std::thread::Builder::new()
        .name("duke-link-host".to_string())
        .spawn(move || {
            let mut active_peer: Option<SocketAddr> = None;
            let mut recv_buf = [0u8; 2048];
            let mut frame_id: u32 = 1;
            let mut window_start = Instant::now();
            let mut window_frames: u32 = 0;
            let mut window_bytes: u64 = 0;
            let mut last_ping = Instant::now();
            let mut last_frame_time = Instant::now() - Duration::from_millis(40);
            let target_interval = Duration::from_millis(22);

            while !stop_flag.load(Ordering::Acquire) {
                while let Ok((len, src_addr)) = socket.recv_from(&mut recv_buf) {
                    if let Ok(pkt) = DukeLinkPacket::decode(&recv_buf[..len]) {
                        match pkt {
                            DukeLinkPacket::Hello { room_code: peer_code } => {
                                if room_code.is_empty() || peer_code == room_code || peer_code.is_empty() {
                                    active_peer = Some(src_addr);
                                    if let Ok(mut rt) = link_state().lock() {
                                        rt.peer_addr = Some(src_addr.to_string());
                                        let snap = snapshot_from_runtime(&rt);
                                        let _ = app.emit("duke-link-state", &snap);
                                    }
                                    let ack = DukeLinkPacket::HelloAck { codec: codec.clone() }.encode();
                                    let _ = socket.send_to(&ack, src_addr);
                                }
                            }
                            DukeLinkPacket::InputStateP2(p2) => {
                                active_peer = Some(src_addr);
                                p2_packets.fetch_add(1, Ordering::Relaxed);
                                forward_p2_input_to_shm(p2);
                            }
                            DukeLinkPacket::Ping { sent_us } => {
                                let pong = DukeLinkPacket::Pong { echo_us: sent_us }.encode();
                                let _ = socket.send_to(&pong, src_addr);
                            }
                            DukeLinkPacket::Pong { echo_us } => {
                                let delta = now_unix_us().saturating_sub(echo_us);
                                rtt_us.store(delta.min(500_000), Ordering::Release);
                            }
                            DukeLinkPacket::Bye => {
                                active_peer = None;
                                if let Ok(mut rt) = link_state().lock() {
                                    rt.peer_addr = None;
                                    let snap = snapshot_from_runtime(&rt);
                                    let _ = app.emit("duke-link-state", &snap);
                                }
                            }
                            _ => {}
                        }
                    }
                }

                if let Some(peer) = active_peer {
                    if last_ping.elapsed() >= Duration::from_millis(500) {
                        let ping = DukeLinkPacket::Ping { sent_us: now_unix_us() }.encode();
                        let _ = socket.send_to(&ping, peer);
                        last_ping = Instant::now();
                    }

                    if last_frame_time.elapsed() >= target_interval {
                        last_frame_time = Instant::now();
                        if let Some((w, h, jpeg)) =
                            capture_scaled_desktop_jpeg(FRAME_CAP_WIDTH, FRAME_CAP_HEIGHT, 64)
                        {
                            let total_chunks = jpeg.len().div_ceil(MAX_CHUNK_BYTES).max(1) as u16;
                            for (idx, chunk) in jpeg.chunks(MAX_CHUNK_BYTES).enumerate() {
                                let pkt = DukeLinkPacket::VideoFrameChunk {
                                    frame_id,
                                    chunk_idx: idx as u16,
                                    total_chunks,
                                    width: w,
                                    height: h,
                                    payload: chunk.to_vec(),
                                }
                                .encode();
                                let _ = socket.send_to(&pkt, peer);
                                window_bytes = window_bytes.saturating_add(pkt.len() as u64);
                            }
                            frame_id = frame_id.wrapping_add(1);
                            window_frames = window_frames.saturating_add(1);
                            frames_count.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                } else {
                    std::thread::sleep(Duration::from_millis(12));
                }

                let elapsed = window_start.elapsed();
                if elapsed >= Duration::from_secs(1) {
                    let secs = elapsed.as_secs_f64().max(0.001);
                    let fps10 = ((window_frames as f64 / secs) * 10.0) as u32;
                    let kbps = ((window_bytes as f64 * 8.0) / secs / 1000.0) as u64;
                    fps_x10.store(fps10, Ordering::Release);
                    bitrate_kbps.store(kbps, Ordering::Release);
                    window_start = Instant::now();
                    window_frames = 0;
                    window_bytes = 0;
                    if let Ok(rt) = link_state().lock() {
                        let snap = snapshot_from_runtime(&rt);
                        let _ = app.emit("duke-link-state", &snap);
                    }
                }
            }

            if let Some(peer) = active_peer {
                let _ = socket.send_to(&DukeLinkPacket::Bye.encode(), peer);
            }
        })
        .ok();
}

struct GuestLoopParams {
    app: AppHandle,
    socket: Arc<UdpSocket>,
    peer_addr: SocketAddr,
    room_code: String,
    stop_flag: Arc<AtomicBool>,
    rtt_us: Arc<AtomicU64>,
    fps_x10: Arc<AtomicU32>,
    bitrate_kbps: Arc<AtomicU64>,
    frames_count: Arc<AtomicU64>,
}

fn spawn_guest_loop(params: GuestLoopParams) {
    let GuestLoopParams {
        app,
        socket,
        peer_addr,
        room_code,
        stop_flag,
        rtt_us,
        fps_x10,
        bitrate_kbps,
        frames_count,
    } = params;
    let _ = socket.set_read_timeout(Some(Duration::from_millis(10)));
    std::thread::Builder::new()
        .name("duke-link-guest".to_string())
        .spawn(move || {
            let mut reassembler = FrameReassembler::new();
            let mut active_codec = "DXGI/UDP".to_string();
            let mut recv_buf = [0u8; 2048];
            let mut last_ping = Instant::now() - Duration::from_secs(1);
            let mut last_hello = Instant::now() - Duration::from_secs(1);
            let mut window_start = Instant::now();
            let mut window_frames: u32 = 0;
            let mut window_bytes: u64 = 0;

            while !stop_flag.load(Ordering::Acquire) {
                if frames_count.load(Ordering::Acquire) == 0
                    && last_hello.elapsed() >= Duration::from_millis(400)
                {
                    let hello = DukeLinkPacket::Hello {
                        room_code: room_code.clone(),
                    }
                    .encode();
                    let _ = socket.send_to(&hello, peer_addr);
                    last_hello = Instant::now();
                }

                if last_ping.elapsed() >= Duration::from_millis(500) {
                    let ping = DukeLinkPacket::Ping { sent_us: now_unix_us() }.encode();
                    let _ = socket.send_to(&ping, peer_addr);
                    last_ping = Instant::now();
                }

                while let Ok((len, _)) = socket.recv_from(&mut recv_buf) {
                    window_bytes = window_bytes.saturating_add(len as u64);
                    if let Ok(pkt) = DukeLinkPacket::decode(&recv_buf[..len]) {
                        match pkt {
                            DukeLinkPacket::HelloAck { codec } => {
                                active_codec = codec.clone();
                                if let Ok(mut rt) = link_state().lock() {
                                    rt.codec = codec;
                                    let snap = snapshot_from_runtime(&rt);
                                    let _ = app.emit("duke-link-state", &snap);
                                }
                            }
                            DukeLinkPacket::VideoFrameChunk {
                                frame_id,
                                chunk_idx,
                                total_chunks,
                                width,
                                height,
                                payload,
                            } => {
                                if let Some((fid, w, h, full_bytes)) = reassembler.push_chunk(
                                    frame_id,
                                    chunk_idx,
                                    total_chunks,
                                    width,
                                    height,
                                    payload,
                                ) {
                                    frames_count.fetch_add(1, Ordering::Relaxed);
                                    window_frames = window_frames.saturating_add(1);
                                    let b64 = base64::engine::general_purpose::STANDARD.encode(&full_bytes);
                                    let ev = CoOpFrameEvent {
                                        frame_id: fid,
                                        width: w,
                                        height: h,
                                        codec: active_codec.clone(),
                                        rtt_ms: (rtt_us.load(Ordering::Acquire) as f64) / 1000.0,
                                        fps: (fps_x10.load(Ordering::Acquire) as f64) / 10.0,
                                        bitrate_kbps: bitrate_kbps.load(Ordering::Acquire),
                                        frame_b64: b64,
                                    };
                                    let _ = app.emit("duke-link-frame", &ev);
                                }
                            }
                            DukeLinkPacket::Ping { sent_us } => {
                                let pong = DukeLinkPacket::Pong { echo_us: sent_us }.encode();
                                let _ = socket.send_to(&pong, peer_addr);
                            }
                            DukeLinkPacket::Pong { echo_us } => {
                                let delta = now_unix_us().saturating_sub(echo_us);
                                rtt_us.store(delta.min(500_000), Ordering::Release);
                            }
                            DukeLinkPacket::Bye => {
                                stop_flag.store(true, Ordering::Release);
                                break;
                            }
                            _ => {}
                        }
                    }
                }

                let elapsed = window_start.elapsed();
                if elapsed >= Duration::from_secs(1) {
                    let secs = elapsed.as_secs_f64().max(0.001);
                    fps_x10.store(((window_frames as f64 / secs) * 10.0) as u32, Ordering::Release);
                    bitrate_kbps.store(
                        ((window_bytes as f64 * 8.0) / secs / 1000.0) as u64,
                        Ordering::Release,
                    );
                    window_start = Instant::now();
                    window_frames = 0;
                    window_bytes = 0;
                    if let Ok(rt) = link_state().lock() {
                        let snap = snapshot_from_runtime(&rt);
                        let _ = app.emit("duke-link-state", &snap);
                    }
                }
            }

            let _ = socket.send_to(&DukeLinkPacket::Bye.encode(), peer_addr);
        })
        .ok();
}

fn start_host_internal(
    app: &AppHandle,
    game_title: String,
    friend_name: Option<String>,
    bind_port: u16,
) -> Result<DukeLinkStatusSnapshot, String> {
    stop_session_internal();

    let sock = UdpSocket::bind(("0.0.0.0", bind_port))
        .or_else(|_| UdpSocket::bind("0.0.0.0:0"))
        .map_err(|e| format!("Failed binding Duke Link UDP host socket: {e}"))?;
    let actual_port = sock.local_addr().map(|a| a.port()).unwrap_or(bind_port);
    let local_ip = discover_best_local_ip();
    let room_code = generate_room_code();
    let codec = detect_hardware_encoder_codec();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let rtt_us = Arc::new(AtomicU64::new(0));
    let fps_x10 = Arc::new(AtomicU32::new(0));
    let bitrate_kbps = Arc::new(AtomicU64::new(0));
    let p2_packets = Arc::new(AtomicU64::new(0));
    let frames_count = Arc::new(AtomicU64::new(0));

    let snap = {
        let mut rt = link_state()
            .lock()
            .map_err(|_| "Duke Link state lock poisoned".to_string())?;
        rt.stop_flag = Arc::clone(&stop_flag);
        rt.role = "host".to_string();
        rt.game_title = game_title;
        rt.room_code = room_code.clone();
        rt.bind_port = actual_port;
        rt.advertise_addr = format!("{}:{}", local_ip, actual_port);
        rt.peer_addr = None;
        rt.friend_name = friend_name;
        rt.codec = codec.clone();
        rt.capture_pipeline = detect_capture_pipeline();
        rt.input_bridge = detect_input_bridge();
        rt.rtt_us = Arc::clone(&rtt_us);
        rt.fps_x10 = Arc::clone(&fps_x10);
        rt.bitrate_kbps = Arc::clone(&bitrate_kbps);
        rt.p2_packets = Arc::clone(&p2_packets);
        rt.frames_count = Arc::clone(&frames_count);
        rt.guest_socket = None;
        rt.guest_peer = None;
        snapshot_from_runtime(&rt)
    };

    spawn_host_loop(HostLoopParams {
        app: app.clone(),
        socket: sock,
        codec,
        room_code,
        stop_flag,
        rtt_us,
        fps_x10,
        bitrate_kbps,
        p2_packets,
        frames_count,
    });
    let _ = app.emit("duke-link-state", &snap);
    Ok(snap)
}

fn stop_session_internal() -> DukeLinkStatusSnapshot {
    let mut rt = link_state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    rt.stop_flag.store(true, Ordering::Release);
    *rt = SessionRuntime::default();
    snapshot_from_runtime(&rt)
}

#[tauri::command]
pub(crate) fn get_duke_link_status() -> DukeLinkStatusSnapshot {
    let mut rt = link_state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    rt.capture_pipeline = detect_capture_pipeline();
    rt.input_bridge = detect_input_bridge();
    snapshot_from_runtime(&rt)
}

#[tauri::command]
pub(crate) fn start_coop_host(
    app: AppHandle,
    game_id: Option<String>,
    game_title: Option<String>,
    bind_port: Option<u16>,
) -> Result<DukeLinkStatusSnapshot, String> {
    let title = game_title
        .filter(|s| !s.trim().is_empty())
        .or(game_id)
        .unwrap_or_else(|| "Active Game Session".to_string());
    start_host_internal(&app, title, None, bind_port.unwrap_or(DUKE_LINK_DEFAULT_PORT))
}

#[tauri::command]
pub(crate) fn stop_coop_session(app: AppHandle) -> DukeLinkStatusSnapshot {
    let snap = stop_session_internal();
    let _ = app.emit("duke-link-state", &snap);
    snap
}

#[tauri::command]
pub(crate) async fn invite_friend_to_coop(
    app: AppHandle,
    friend_name: String,
    platform: String,
    steam_id: Option<String>,
    peer_hint: Option<String>,
    game_title: Option<String>,
) -> Result<CoOpInviteResult, String> {
    let title = game_title
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Couch Co-Op Session".to_string());

    let session = {
        let current = get_duke_link_status();
        if current.active && current.role == "host" {
            if let Ok(mut rt) = link_state().lock() {
                rt.friend_name = Some(friend_name.clone());
                rt.game_title = title.clone();
                snapshot_from_runtime(&rt)
            } else {
                current
            }
        } else {
            start_host_internal(
                &app,
                title.clone(),
                Some(friend_name.clone()),
                DUKE_LINK_DEFAULT_PORT,
            )?
        }
    };

    let host_endpoint = peer_hint
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != "127.0.0.1")
        .map(|ip| {
            if ip.contains(':') {
                ip
            } else {
                format!("{}:{}", ip, session.bind_port)
            }
        })
        .unwrap_or_else(|| session.advertise_addr.clone());

    let invite_uri = format!(
        "dukelink://{}?code={}&game={}",
        host_endpoint,
        urlencoding::encode(&session.room_code),
        urlencoding::encode(&title)
    );

    let mut delivery_note = format!(
        "Room {} ready on UDP {} — share connect string with {}.",
        session.room_code, host_endpoint, friend_name
    );

    if platform.eq_ignore_ascii_case("steam") {
        if let Some(sid) = steam_id.filter(|s| !s.trim().is_empty()) {
            let msg = format!(
                "[Duke Link] Join my Couch Co-Op stream for '{}'! Host: {} | Code: {} ({})",
                title, host_endpoint, session.room_code, invite_uri
            );
            match crate::steam_cm::send_steam_chat_message(
                app.clone(),
                sid,
                Some(friend_name.clone()),
                msg,
            )
            .await
            {
                Ok(sent) => {
                    delivery_note = format!(
                        "Invite sent to {} via Steam Chat ({}) — Room {} ({}).",
                        friend_name, sent.delivery_status, session.room_code, host_endpoint
                    );
                }
                Err(_) => {
                    delivery_note = format!(
                        "Host ready at {} (Code {}) — Steam chat not signed in; copied connect string for {}.",
                        host_endpoint, session.room_code, friend_name
                    );
                }
            }
        }
    }

    Ok(CoOpInviteResult {
        session,
        invite_uri,
        delivery_note,
    })
}

fn parse_join_endpoint(raw: &str, code_opt: Option<String>) -> Result<(SocketAddr, String, String), String> {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix("dukelink://") {
        let (host_part, query_part) = rest.split_once('?').unwrap_or((rest, ""));
        let addr_str = if host_part.contains(':') {
            host_part.to_string()
        } else {
            format!("{}:{}", host_part, DUKE_LINK_DEFAULT_PORT)
        };
        let mut parsed_code = code_opt.unwrap_or_default();
        let mut parsed_title = "Co-Op Host Stream".to_string();
        for pair in query_part.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                if k == "code" && parsed_code.is_empty() {
                    parsed_code = urlencoding::decode(v).map(|s| s.into_owned()).unwrap_or_default();
                } else if k == "game" {
                    if let Ok(decoded) = urlencoding::decode(v) {
                        if !decoded.trim().is_empty() {
                            parsed_title = decoded.into_owned();
                        }
                    }
                }
            }
        }
        let peer: SocketAddr = addr_str
            .parse()
            .map_err(|_| format!("Invalid Duke Link address '{}'.", host_part))?;
        return Ok((peer, parsed_code, parsed_title));
    }

    let addr_str = if trimmed.contains(':') {
        trimmed.to_string()
    } else {
        format!("{}:{}", trimmed, DUKE_LINK_DEFAULT_PORT)
    };
    let peer: SocketAddr = addr_str
        .parse()
        .map_err(|_| format!("Invalid host IP:port '{}'.", trimmed))?;
    Ok((
        peer,
        code_opt.unwrap_or_default(),
        "Co-Op Host Stream".to_string(),
    ))
}

#[tauri::command]
pub(crate) fn join_coop_stream(
    app: AppHandle,
    host_addr: String,
    room_code: Option<String>,
    game_title: Option<String>,
) -> Result<DukeLinkStatusSnapshot, String> {
    let (peer_addr, parsed_code, parsed_title) = parse_join_endpoint(&host_addr, room_code)?;
    stop_session_internal();

    let sock = Arc::new(
        UdpSocket::bind("0.0.0.0:0")
            .map_err(|e| format!("Failed binding Duke Link client UDP socket: {e}"))?,
    );
    let local_port = sock.local_addr().map(|a| a.port()).unwrap_or(0);
    let stop_flag = Arc::new(AtomicBool::new(false));
    let rtt_us = Arc::new(AtomicU64::new(0));
    let fps_x10 = Arc::new(AtomicU32::new(0));
    let bitrate_kbps = Arc::new(AtomicU64::new(0));
    let p2_packets = Arc::new(AtomicU64::new(0));
    let frames_count = Arc::new(AtomicU64::new(0));
    let resolved_title = game_title
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or(parsed_title);

    let snap = {
        let mut rt = link_state()
            .lock()
            .map_err(|_| "Duke Link state lock poisoned".to_string())?;
        rt.stop_flag = Arc::clone(&stop_flag);
        rt.role = "guest".to_string();
        rt.game_title = resolved_title;
        rt.room_code = parsed_code.clone();
        rt.bind_port = local_port;
        rt.advertise_addr = peer_addr.to_string();
        rt.peer_addr = Some(peer_addr.to_string());
        rt.friend_name = None;
        rt.codec = "Connecting...".to_string();
        rt.capture_pipeline = detect_capture_pipeline();
        rt.input_bridge = detect_input_bridge();
        rt.rtt_us = Arc::clone(&rtt_us);
        rt.fps_x10 = Arc::clone(&fps_x10);
        rt.bitrate_kbps = Arc::clone(&bitrate_kbps);
        rt.p2_packets = Arc::clone(&p2_packets);
        rt.frames_count = Arc::clone(&frames_count);
        rt.guest_socket = Some(Arc::clone(&sock));
        rt.guest_peer = Some(peer_addr);
        snapshot_from_runtime(&rt)
    };

    spawn_guest_loop(GuestLoopParams {
        app: app.clone(),
        socket: sock,
        peer_addr,
        room_code: parsed_code,
        stop_flag,
        rtt_us,
        fps_x10,
        bitrate_kbps,
        frames_count,
    });
    let _ = app.emit("duke-link-state", &snap);
    Ok(snap)
}

#[tauri::command]
pub(crate) fn send_coop_input_p2(input: CoOpInputP2) -> Result<u64, String> {
    let (guest_sock, guest_peer, p2_counter, role) = {
        let rt = link_state()
            .lock()
            .map_err(|_| "Duke Link state lock poisoned".to_string())?;
        (
            rt.guest_socket.clone(),
            rt.guest_peer,
            Arc::clone(&rt.p2_packets),
            rt.role.clone(),
        )
    };

    let count = p2_counter.fetch_add(1, Ordering::Relaxed) + 1;
    if role == "guest" {
        if let (Some(sock), Some(peer)) = (guest_sock, guest_peer) {
            let pkt = DukeLinkPacket::InputStateP2(input).encode();
            sock.send_to(&pkt, peer)
                .map_err(|e| format!("Failed sending P2 controller packet: {e}"))?;
            return Ok(count);
        }
    }
    forward_p2_input_to_shm(input);
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duke_link_packet_and_input_serialization_roundtrip() {
        let p2 = CoOpInputP2 {
            buttons: 0x300F,
            left_trigger: 180,
            right_trigger: 255,
            thumb_lx: -16_400,
            thumb_ly: 22_000,
            thumb_rx: 1_234,
            thumb_ry: -9_876,
            sequence: 999_123,
        };
        let pkt = DukeLinkPacket::InputStateP2(p2.clone());
        let encoded = pkt.encode();
        let decoded = DukeLinkPacket::decode(&encoded).unwrap();
        assert_eq!(decoded, pkt);

        let shm_payload = p2.to_shm_payload();
        let frame = ShmEventFrame::coop_input_p2(ShmSourceApp::DukeLauncher, 4242, shm_payload);
        assert_eq!(frame.decode_game_launched(), None);
        assert_eq!(frame.decode_coop_input_p2(), Some(shm_payload));

        let hello = DukeLinkPacket::Hello {
            room_code: "DUKE-AB12CD".to_string(),
        };
        assert_eq!(DukeLinkPacket::decode(&hello.encode()).unwrap(), hello);

        let chunk = DukeLinkPacket::VideoFrameChunk {
            frame_id: 77,
            chunk_idx: 1,
            total_chunks: 3,
            width: 960,
            height: 540,
            payload: vec![1, 2, 3, 4, 5],
        };
        assert_eq!(DukeLinkPacket::decode(&chunk.encode()).unwrap(), chunk);
    }

    #[test]
    fn duke_link_chunk_reassembly_and_uri_parse() {
        let mut reasm = FrameReassembler::new();
        assert!(reasm.push_chunk(5, 0, 2, 960, 540, vec![10, 20]).is_none());
        let joined = reasm.push_chunk(5, 1, 2, 960, 540, vec![30, 40]).unwrap();
        assert_eq!(joined.0, 5);
        assert_eq!(joined.3, vec![10, 20, 30, 40]);

        let (peer, code, title) =
            parse_join_endpoint("dukelink://100.64.0.12:47882?code=DUKE-99AABB&game=Halo", None)
                .unwrap();
        assert_eq!(peer.to_string(), "100.64.0.12:47882");
        assert_eq!(code, "DUKE-99AABB");
        assert_eq!(title, "Halo");
    }
}

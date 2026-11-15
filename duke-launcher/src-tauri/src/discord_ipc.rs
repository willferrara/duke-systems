//! Zero-SDK Discord Rich Presence client speaking Discord's local IPC binary
//! protocol over `\\.\pipe\discord-ipc-0`..`9` in pure Rust.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

const OP_HANDSHAKE: u32 = 0;
const OP_FRAME: u32 = 1;
const OP_CLOSE: u32 = 2;
const OP_PING: u32 = 3;
const OP_PONG: u32 = 4;

const MAX_IPC_FRAME_BYTES: usize = 1024 * 1024; // 1 MiB protocol guard
const DISCORD_TEXT_MAX_CHARS: usize = 128;
const DEFAULT_DISCORD_CLIENT_ID: &str = "1350000000000000001";

#[cfg(target_os = "windows")]
const PIPE_PROBE_MAX_INDEX: u8 = 9;
#[cfg(target_os = "windows")]
const PIPE_IO_TIMEOUT_MS: u32 = 1500;
const WORKER_RECONNECT_INTERVAL: Duration = Duration::from_secs(10);

static FRAMES_SENT: AtomicU64 = AtomicU64::new(0);
static NONCE_SEQ: AtomicU64 = AtomicU64::new(1);
static PRESENCE_STATE: OnceLock<Mutex<DiscordPresenceStatus>> = OnceLock::new();
static WORKER_TX: OnceLock<Option<mpsc::Sender<PresenceCommand>>> = OnceLock::new();

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiscordPresenceStatus {
    pub enabled: bool,
    pub connected: bool,
    pub pipe_name: Option<String>,
    pub active_game: Option<String>,
    pub platform_badge: Option<String>,
    pub coop_status: Option<String>,
    pub session_started_ts: Option<u64>,
    pub frames_sent: u64,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
struct GamePresenceSpec {
    title: String,
    platform: String,
    coop_status: Option<String>,
    start_ts: u64,
}

#[derive(Debug)]
enum PresenceCommand {
    SetGame(GamePresenceSpec),
    Clear,
    SetEnabled(bool),
}

fn state_mutex() -> &'static Mutex<DiscordPresenceStatus> {
    PRESENCE_STATE.get_or_init(|| {
        Mutex::new(DiscordPresenceStatus {
            enabled: true,
            ..Default::default()
        })
    })
}

fn lock_status() -> MutexGuard<'static, DiscordPresenceStatus> {
    match state_mutex().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn current_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn next_nonce() -> String {
    let seq = NONCE_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("duke-{}-{seq}", std::process::id())
}

fn resolve_client_id() -> String {
    if let Ok(val) = std::env::var("DUKE_DISCORD_CLIENT_ID") {
        let trimmed = val.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    DEFAULT_DISCORD_CLIENT_ID.to_string()
}

fn clip_presence_text(raw: &str, fallback: &str) -> String {
    let trimmed = raw.trim();
    let source = if trimmed.is_empty() { fallback } else { trimmed };
    source.chars().take(DISCORD_TEXT_MAX_CHARS).collect()
}

fn build_handshake_payload(client_id: &str) -> serde_json::Value {
    serde_json::json!({
        "v": 1,
        "client_id": client_id,
    })
}

fn build_clear_activity_payload(pid: u32, nonce: &str) -> serde_json::Value {
    serde_json::json!({
        "cmd": "SET_ACTIVITY",
        "args": {
            "pid": pid,
            "activity": serde_json::Value::Null,
        },
        "nonce": nonce,
    })
}

/// Encodes a Discord IPC frame: `opcode (u32 LE) + length (u32 LE) + UTF-8 JSON bytes`.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn encode_discord_ipc_frame(
    opcode: u32,
    payload: &serde_json::Value,
) -> Result<Vec<u8>, String> {
    let json_bytes = serde_json::to_vec(payload)
        .map_err(|e| format!("Failed to serialize Discord IPC payload: {e}"))?;
    if json_bytes.len() > MAX_IPC_FRAME_BYTES {
        return Err(format!(
            "Discord IPC payload exceeds 1 MiB limit ({} bytes)",
            json_bytes.len()
        ));
    }
    let len_u32 = u32::try_from(json_bytes.len())
        .map_err(|_| "Discord IPC payload length overflow".to_string())?;
    let mut frame = Vec::with_capacity(8 + json_bytes.len());
    frame.extend_from_slice(&opcode.to_le_bytes());
    frame.extend_from_slice(&len_u32.to_le_bytes());
    frame.extend_from_slice(&json_bytes);
    Ok(frame)
}

/// Decodes a Discord IPC frame into `(opcode, JSON payload)`, guarding against truncated or oversized frames.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn decode_discord_ipc_frame(raw: &[u8]) -> Result<(u32, serde_json::Value), String> {
    if raw.len() < 8 {
        return Err(format!(
            "Truncated Discord IPC header: expected >= 8 bytes, got {}",
            raw.len()
        ));
    }
    let opcode = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
    let length = u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]) as usize;
    if length > MAX_IPC_FRAME_BYTES {
        return Err(format!(
            "Oversized Discord IPC frame: {} bytes exceeds 1 MiB limit",
            length
        ));
    }
    let total_len = 8usize
        .checked_add(length)
        .ok_or_else(|| "Discord IPC frame length overflow".to_string())?;
    if raw.len() < total_len {
        return Err(format!(
            "Truncated Discord IPC payload: expected {} total bytes, got {}",
            total_len,
            raw.len()
        ));
    }
    let payload: serde_json::Value = serde_json::from_slice(&raw[8..total_len])
        .map_err(|e| format!("Invalid Discord IPC JSON payload: {e}"))?;
    Ok((opcode, payload))
}

/// Builds a `SET_ACTIVITY` command payload for a launched PC game, retro ROM, or Duke Link co-op session.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn build_set_activity_payload(
    pid: u32,
    title: &str,
    platform: &str,
    coop_status: Option<&str>,
    start_ts: u64,
    nonce: &str,
) -> serde_json::Value {
    let details = clip_presence_text(title, "Duke Launcher");
    let clean_platform = platform.trim();
    let clean_coop = coop_status.map(str::trim).filter(|s| !s.is_empty());

    let state_raw = match (clean_platform.is_empty(), clean_coop) {
        (false, Some(coop)) => format!("{clean_platform} • {coop}"),
        (true, Some(coop)) => coop.to_string(),
        (false, None) => clean_platform.to_string(),
        (true, None) => "Playing via Duke Launcher".to_string(),
    };
    let state = clip_presence_text(&state_raw, "Duke Launcher");
    let large_text = clip_presence_text(&format!("{details} — Duke Launcher"), "Duke Launcher");
    let small_text = clip_presence_text(clean_platform, "Duke Launcher");

    serde_json::json!({
        "cmd": "SET_ACTIVITY",
        "args": {
            "pid": pid,
            "activity": {
                "details": details,
                "state": state,
                "timestamps": {
                    "start": start_ts,
                },
                "assets": {
                    "large_text": large_text,
                    "small_text": small_text,
                },
            },
        },
        "nonce": nonce,
    })
}

fn worker_sender() -> Option<&'static mpsc::Sender<PresenceCommand>> {
    WORKER_TX
        .get_or_init(|| {
            let (tx, rx) = mpsc::channel::<PresenceCommand>();
            match std::thread::Builder::new()
                .name("discord-ipc-worker".to_string())
                .spawn(move || run_worker_loop(rx))
            {
                Ok(_) => Some(tx),
                Err(err) => {
                    let mut st = lock_status();
                    st.connected = false;
                    st.pipe_name = None;
                    st.last_error = Some(format!("Failed to spawn discord-ipc-worker: {err}"));
                    None
                }
            }
        })
        .as_ref()
}

/// Non-blocking update for the active game's Discord Rich Presence.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn set_discord_presence_game(
    title: &str,
    platform: &str,
    coop_status: Option<&str>,
    start_ts: Option<u64>,
) {
    let clean_title = clip_presence_text(title, "Duke Launcher");
    let clean_platform = platform.trim().to_string();
    let clean_coop = coop_status
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    let effective_ts = start_ts.unwrap_or_else(current_epoch_secs);

    {
        let mut st = lock_status();
        if st.enabled {
            st.active_game = Some(clean_title.clone());
            st.platform_badge = (!clean_platform.is_empty()).then(|| clean_platform.clone());
            st.coop_status = clean_coop.clone();
            st.session_started_ts = Some(effective_ts);
        }
    }

    if let Some(tx) = worker_sender() {
        let _ = tx.send(PresenceCommand::SetGame(GamePresenceSpec {
            title: clean_title,
            platform: clean_platform,
            coop_status: clean_coop,
            start_ts: effective_ts,
        }));
    }
}

/// Non-blocking call to clear the active Discord Rich Presence activity on game exit.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn clear_discord_presence() {
    {
        let mut st = lock_status();
        st.active_game = None;
        st.platform_badge = None;
        st.coop_status = None;
        st.session_started_ts = None;
    }

    if let Some(tx) = worker_sender() {
        let _ = tx.send(PresenceCommand::Clear);
    }
}

/// Non-blocking toggle for Discord Rich Presence integration.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn set_discord_presence_enabled(enabled: bool) {
    {
        let mut st = lock_status();
        st.enabled = enabled;
        st.last_error = None;
        if !enabled {
            st.connected = false;
            st.pipe_name = None;
            st.active_game = None;
            st.platform_badge = None;
            st.coop_status = None;
            st.session_started_ts = None;
        }
    }

    if let Some(tx) = worker_sender() {
        let _ = tx.send(PresenceCommand::SetEnabled(enabled));
    }
}

/// Returns a thread-safe snapshot of current Discord Rich Presence status.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn get_discord_presence_status() -> DiscordPresenceStatus {
    let mut st = lock_status().clone();
    st.frames_sent = FRAMES_SENT.load(Ordering::Acquire);
    st
}

#[cfg(target_os = "windows")]
mod win_pipe {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    use windows::core::{HRESULT, PCWSTR};
    use windows::Win32::Foundation::{
        CloseHandle, ERROR_IO_PENDING, ERROR_MORE_DATA, ERROR_PIPE_BUSY, HANDLE, WAIT_OBJECT_0,
    };
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_OVERLAPPED, FILE_GENERIC_READ,
        FILE_GENERIC_WRITE, FILE_SHARE_MODE, OPEN_EXISTING, ReadFile, WriteFile,
    };
    use windows::Win32::System::Threading::{
        CreateEventW, ResetEvent, WaitForSingleObject,
    };
    use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

    use super::{
        FRAMES_SENT, MAX_IPC_FRAME_BYTES, OP_CLOSE, OP_FRAME, OP_HANDSHAKE, OP_PING, OP_PONG,
        PIPE_IO_TIMEOUT_MS, PIPE_PROBE_MAX_INDEX, build_handshake_payload,
        decode_discord_ipc_frame, encode_discord_ipc_frame,
    };

    struct HandleGuard(HANDLE);

    impl Drop for HandleGuard {
        fn drop(&mut self) {
            if !self.0.is_invalid() {
                // SAFETY: `self.0` is an owned Win32 HANDLE created in this module.
                let _ = unsafe { CloseHandle(self.0) };
            }
        }
    }

    pub(super) struct PipeTransport {
        pipe: HandleGuard,
        event: HandleGuard,
    }

    impl PipeTransport {
        pub(super) fn connect(pipe_path: &str) -> Result<Self, String> {
            if pipe_path.trim().is_empty() {
                return Err("Discord IPC pipe path cannot be empty".to_string());
            }

            let wide: Vec<u16> = OsStr::new(pipe_path)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();

            // SAFETY: Creating an unnamed manual-reset event for overlapped pipe I/O.
            let event_handle = unsafe { CreateEventW(None, true, false, None) }
                .map_err(|e| format!("CreateEventW failed for '{pipe_path}': {e}"))?;
            let event = HandleGuard(event_handle);

            let busy_hr = HRESULT::from_win32(ERROR_PIPE_BUSY.0);
            let mut last_err = String::new();

            for attempt in 0..4u8 {
                // SAFETY: `wide` is null-terminated UTF-16 pipe path opened with overlapped mode.
                match unsafe {
                    CreateFileW(
                        PCWSTR(wide.as_ptr()),
                        FILE_GENERIC_READ.0 | FILE_GENERIC_WRITE.0,
                        FILE_SHARE_MODE(0),
                        None,
                        OPEN_EXISTING,
                        FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OVERLAPPED,
                        None,
                    )
                } {
                    Ok(handle) if !handle.is_invalid() => {
                        return Ok(Self {
                            pipe: HandleGuard(handle),
                            event,
                        });
                    }
                    Ok(_) => {
                        last_err = format!("CreateFileW returned invalid handle for '{pipe_path}'");
                        break;
                    }
                    Err(err) => {
                        if err.code() == busy_hr && attempt < 3 {
                            std::thread::sleep(Duration::from_millis(20));
                            continue;
                        }
                        last_err = format!("CreateFileW('{pipe_path}') failed: {err}");
                        break;
                    }
                }
            }

            Err(last_err)
        }

        fn wait_overlapped(&self, ov: &OVERLAPPED, timeout_ms: u32, op_label: &str) -> Result<u32, String> {
            // SAFETY: Waiting on valid manual-reset event bound to `ov`.
            let wait = unsafe { WaitForSingleObject(self.event.0, timeout_ms) };
            if wait != WAIT_OBJECT_0 {
                // SAFETY: Cancel pending I/O and drain completion before `ov` and caller slice leave scope.
                let _ = unsafe { CancelIoEx(self.pipe.0, Some(ov)) };
                let mut drained = 0u32;
                let _ = unsafe { GetOverlappedResult(self.pipe.0, ov, &mut drained, true) };
                return Err(format!(
                    "Discord IPC pipe {op_label} timed out after {timeout_ms}ms"
                ));
            }
            let mut transferred = 0u32;
            let more_data_hr = HRESULT::from_win32(ERROR_MORE_DATA.0);
            // SAFETY: Event signaled; retrieving completed overlapped byte count.
            match unsafe { GetOverlappedResult(self.pipe.0, ov, &mut transferred, true) } {
                Ok(()) => Ok(transferred),
                Err(err) if err.code() == more_data_hr && transferred > 0 => Ok(transferred),
                Err(err) => Err(format!("Discord IPC pipe {op_label} failed: {err}")),
            }
        }

        fn write_exact(&self, data: &[u8], timeout_ms: u32) -> Result<(), String> {
            let io_pending_hr = HRESULT::from_win32(ERROR_IO_PENDING.0);
            let mut offset = 0usize;

            while offset < data.len() {
                // SAFETY: Resetting manual-reset event before issuing overlapped WriteFile.
                let _ = unsafe { ResetEvent(self.event.0) };
                let mut ov = OVERLAPPED {
                    hEvent: self.event.0,
                    ..Default::default()
                };
                let mut written = 0u32;
                // SAFETY: `data[offset..]` and `ov` stay valid until `GetOverlappedResult` completes.
                let res = unsafe {
                    WriteFile(
                        self.pipe.0,
                        Some(&data[offset..]),
                        Some(&mut written),
                        Some(&mut ov),
                    )
                };
                let chunk_written = match res {
                    Ok(()) => {
                        if written > 0 {
                            written
                        } else {
                            let mut completed = 0u32;
                            // SAFETY: Synchronous completion; fetch exact written count.
                            unsafe { GetOverlappedResult(self.pipe.0, &ov, &mut completed, false) }
                                .map_err(|e| format!("Discord IPC pipe write failed: {e}"))?;
                            completed
                        }
                    }
                    Err(err) if err.code() == io_pending_hr => {
                        self.wait_overlapped(&ov, timeout_ms, "write")?
                    }
                    Err(err) => {
                        return Err(format!("Discord IPC pipe write failed: {err}"));
                    }
                };

                if chunk_written == 0 {
                    return Err("Discord IPC pipe closed during write".to_string());
                }
                offset = offset.saturating_add(chunk_written as usize);
            }

            Ok(())
        }

        fn read_exact(&self, buf: &mut [u8], timeout_ms: u32) -> Result<(), String> {
            let io_pending_hr = HRESULT::from_win32(ERROR_IO_PENDING.0);
            let more_data_hr = HRESULT::from_win32(ERROR_MORE_DATA.0);
            let mut offset = 0usize;

            while offset < buf.len() {
                // SAFETY: Resetting manual-reset event before issuing overlapped ReadFile.
                let _ = unsafe { ResetEvent(self.event.0) };
                let mut ov = OVERLAPPED {
                    hEvent: self.event.0,
                    ..Default::default()
                };
                let mut read_bytes = 0u32;
                // SAFETY: `buf[offset..]` and `ov` stay valid until `GetOverlappedResult` completes.
                let res = unsafe {
                    ReadFile(
                        self.pipe.0,
                        Some(&mut buf[offset..]),
                        Some(&mut read_bytes),
                        Some(&mut ov),
                    )
                };
                let chunk_read = match res {
                    Ok(()) => {
                        if read_bytes > 0 {
                            read_bytes
                        } else {
                            let mut completed = 0u32;
                            // SAFETY: Synchronous completion; fetch exact read count.
                            unsafe { GetOverlappedResult(self.pipe.0, &ov, &mut completed, false) }
                                .map_err(|e| format!("Discord IPC pipe read failed: {e}"))?;
                            completed
                        }
                    }
                    Err(err) if err.code() == more_data_hr && read_bytes > 0 => read_bytes,
                    Err(err) if err.code() == io_pending_hr => {
                        self.wait_overlapped(&ov, timeout_ms, "read")?
                    }
                    Err(err) => {
                        return Err(format!("Discord IPC pipe read failed: {err}"));
                    }
                };

                if chunk_read == 0 {
                    return Err("Discord IPC pipe closed (EOF)".to_string());
                }
                offset = offset.saturating_add(chunk_read as usize);
            }

            Ok(())
        }

        fn write_frame(&self, opcode: u32, payload: &serde_json::Value) -> Result<(), String> {
            let encoded = encode_discord_ipc_frame(opcode, payload)?;
            self.write_exact(&encoded, PIPE_IO_TIMEOUT_MS)?;
            FRAMES_SENT.fetch_add(1, Ordering::Release);
            Ok(())
        }

        fn read_frame(&self) -> Result<(u32, serde_json::Value), String> {
            let mut header = [0u8; 8];
            self.read_exact(&mut header, PIPE_IO_TIMEOUT_MS)?;
            let length = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
            if length > MAX_IPC_FRAME_BYTES {
                return Err(format!(
                    "Oversized Discord IPC response frame: {} bytes exceeds 1 MiB limit",
                    length
                ));
            }
            let total_len = 8usize
                .checked_add(length)
                .ok_or_else(|| "Discord IPC response frame length overflow".to_string())?;
            let mut frame = vec![0u8; total_len];
            frame[..8].copy_from_slice(&header);
            if length > 0 {
                self.read_exact(&mut frame[8..], PIPE_IO_TIMEOUT_MS)?;
            }
            decode_discord_ipc_frame(&frame)
        }

        fn read_reply_expecting_frame(&self) -> Result<serde_json::Value, String> {
            for _ in 0..4 {
                let (opcode, payload) = self.read_frame()?;
                match opcode {
                    OP_PING => {
                        self.write_frame(OP_PONG, &payload)?;
                    }
                    OP_CLOSE => {
                        let msg = payload
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Discord IPC pipe closed by peer");
                        return Err(format!("Discord IPC CLOSE frame: {msg}"));
                    }
                    OP_FRAME => {
                        if payload.get("evt").and_then(|v| v.as_str()) == Some("ERROR") {
                            let msg = payload
                                .get("data")
                                .and_then(|d| d.get("message"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("Discord IPC returned ERROR event");
                            return Err(format!("Discord IPC error: {msg}"));
                        }
                        return Ok(payload);
                    }
                    other => {
                        return Err(format!("Unexpected Discord IPC reply opcode: {other}"));
                    }
                }
            }
            Err("Exceeded max PING/control frames waiting for Discord IPC reply".to_string())
        }

        pub(super) fn handshake(&self, client_id: &str) -> Result<serde_json::Value, String> {
            let payload = build_handshake_payload(client_id);
            self.write_frame(OP_HANDSHAKE, &payload)?;
            self.read_reply_expecting_frame()
        }

        pub(super) fn send_command(
            &self,
            activity_cmd: &serde_json::Value,
        ) -> Result<serde_json::Value, String> {
            self.write_frame(OP_FRAME, activity_cmd)?;
            self.read_reply_expecting_frame()
        }
    }

    pub(super) fn connect_first_available_pipe(
        client_id: &str,
    ) -> Result<(String, PipeTransport), String> {
        let mut last_err = String::from("No Discord IPC pipes probed");
        for idx in 0..=PIPE_PROBE_MAX_INDEX {
            let path = format!(r"\\.\pipe\discord-ipc-{idx}");
            match PipeTransport::connect(&path) {
                Ok(pipe) => match pipe.handshake(client_id) {
                    Ok(_) => return Ok((path, pipe)),
                    Err(err) => {
                        last_err = format!("{path}: {err}");
                    }
                },
                Err(err) => {
                    last_err = err;
                }
            }
        }
        Err(format!(
            "Discord not reachable on \\\\.\\pipe\\discord-ipc-0..{PIPE_PROBE_MAX_INDEX} ({last_err})"
        ))
    }
}

/// Connects to a specific Windows named pipe, completes Discord IPC handshake, and sends a `SET_ACTIVITY` command.
#[cfg(target_os = "windows")]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn send_activity_over_pipe(
    pipe_path: &str,
    client_id: &str,
    activity_cmd: &serde_json::Value,
) -> Result<(), String> {
    let trimmed_id = client_id.trim();
    if trimmed_id.is_empty() {
        return Err("Discord client_id cannot be empty".to_string());
    }
    let pipe = win_pipe::PipeTransport::connect(pipe_path)?;
    let _ready = pipe.handshake(trimmed_id)?;
    let _ack = pipe.send_command(activity_cmd)?;
    Ok(())
}

fn apply_command(
    cmd: PresenceCommand,
    enabled: &mut bool,
    desired_game: &mut Option<GamePresenceSpec>,
    dirty: &mut bool,
) {
    match cmd {
        PresenceCommand::SetGame(spec) => {
            *desired_game = Some(spec);
            *dirty = true;
        }
        PresenceCommand::Clear => {
            *desired_game = None;
            *dirty = true;
        }
        PresenceCommand::SetEnabled(next_enabled) => {
            if *enabled != next_enabled {
                *enabled = next_enabled;
                *dirty = true;
            }
        }
    }
}

fn run_worker_loop(rx: mpsc::Receiver<PresenceCommand>) {
    let mut enabled = lock_status().enabled;
    let mut desired_game: Option<GamePresenceSpec> = None;
    let mut dirty = false;
    let mut has_active_remote = false;

    #[cfg(target_os = "windows")]
    let mut active_pipe: Option<(String, win_pipe::PipeTransport)> = None;

    loop {
        #[cfg(target_os = "windows")]
        let should_retry_connect = enabled && desired_game.is_some() && active_pipe.is_none();
        #[cfg(not(target_os = "windows"))]
        let should_retry_connect = false;

        let first_cmd = if should_retry_connect {
            match rx.recv_timeout(WORKER_RECONNECT_INTERVAL) {
                Ok(cmd) => Some(cmd),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match rx.recv() {
                Ok(cmd) => Some(cmd),
                Err(_) => break,
            }
        };

        if let Some(cmd) = first_cmd {
            apply_command(cmd, &mut enabled, &mut desired_game, &mut dirty);
            while let Ok(queued) = rx.try_recv() {
                apply_command(queued, &mut enabled, &mut desired_game, &mut dirty);
            }
        }

        if !enabled {
            #[cfg(target_os = "windows")]
            {
                if has_active_remote {
                    if let Some((_, ref pipe)) = active_pipe {
                        let clear_cmd = build_clear_activity_payload(std::process::id(), &next_nonce());
                        let _ = pipe.send_command(&clear_cmd);
                    }
                }
                active_pipe = None;
            }
            has_active_remote = false;
            dirty = false;
            let mut st = lock_status();
            if !st.enabled {
                st.connected = false;
                st.pipe_name = None;
            }
            st.frames_sent = FRAMES_SENT.load(Ordering::Acquire);
            continue;
        }

        if let Some(ref spec) = desired_game {
            {
                let mut st = lock_status();
                if st.enabled && st.active_game.is_none() {
                    st.active_game = Some(spec.title.clone());
                    st.platform_badge = (!spec.platform.is_empty()).then(|| spec.platform.clone());
                    st.coop_status = spec.coop_status.clone();
                    st.session_started_ts = Some(spec.start_ts);
                }
            }

            #[cfg(target_os = "windows")]
            if dirty || active_pipe.is_none() {
                let client_id = resolve_client_id();
                let payload = build_set_activity_payload(
                    std::process::id(),
                    &spec.title,
                    &spec.platform,
                    spec.coop_status.as_deref(),
                    spec.start_ts,
                    &next_nonce(),
                );

                let mut send_res: Result<String, String> = Err("No pipe".to_string());
                if let Some((ref path, ref pipe)) = active_pipe {
                    match pipe.send_command(&payload) {
                        Ok(_) => send_res = Ok(path.clone()),
                        Err(_) => {
                            active_pipe = None;
                        }
                    }
                }

                if active_pipe.is_none() {
                    match win_pipe::connect_first_available_pipe(&client_id) {
                        Ok((path, pipe)) => match pipe.send_command(&payload) {
                            Ok(_) => {
                                active_pipe = Some((path.clone(), pipe));
                                send_res = Ok(path);
                            }
                            Err(err) => {
                                send_res = Err(err);
                            }
                        },
                        Err(err) => {
                            send_res = Err(err);
                        }
                    }
                }

                let mut st = lock_status();
                st.frames_sent = FRAMES_SENT.load(Ordering::Acquire);
                match send_res {
                    Ok(path) => {
                        has_active_remote = true;
                        dirty = false;
                        if st.enabled {
                            st.connected = true;
                            st.pipe_name = Some(path);
                            st.last_error = None;
                        }
                    }
                    Err(err) => {
                        has_active_remote = false;
                        active_pipe = None;
                        st.connected = false;
                        st.pipe_name = None;
                        st.last_error = Some(err);
                    }
                }
            }

            #[cfg(not(target_os = "windows"))]
            {
                let _ = dirty;
                dirty = false;
                let mut st = lock_status();
                st.connected = false;
                st.pipe_name = None;
                st.last_error = Some(
                    "Discord named pipe IPC is only available on Windows".to_string(),
                );
            }
        } else {
            #[cfg(target_os = "windows")]
            if has_active_remote {
                if let Some((_, ref pipe)) = active_pipe {
                    let clear_cmd = build_clear_activity_payload(std::process::id(), &next_nonce());
                    if let Err(err) = pipe.send_command(&clear_cmd) {
                        active_pipe = None;
                        let mut st = lock_status();
                        st.connected = false;
                        st.pipe_name = None;
                        st.last_error = Some(err);
                    }
                }
            }
            has_active_remote = false;
            dirty = false;
            let mut st = lock_status();
            st.frames_sent = FRAMES_SENT.load(Ordering::Acquire);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    static GLOBAL_STATUS_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    #[cfg(target_os = "windows")]
    static TEST_PIPE_SEQ: AtomicU64 = AtomicU64::new(1);

    fn lock_global_status_test() -> MutexGuard<'static, ()> {
        let m = GLOBAL_STATUS_TEST_LOCK.get_or_init(|| Mutex::new(()));
        match m.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        }
    }

    #[test]
    fn wire_frame_encode_decode_roundtrip_and_errors() {
        for opcode in [OP_HANDSHAKE, OP_FRAME, OP_CLOSE, OP_PING, OP_PONG] {
            let payload = serde_json::json!({
                "v": 1,
                "opcode": opcode,
                "title": "Duke Nukem 3D",
            });
            let encoded = encode_discord_ipc_frame(opcode, &payload).expect("encode valid frame");
            assert!(encoded.len() >= 8);
            assert_eq!(u32::from_le_bytes([encoded[0], encoded[1], encoded[2], encoded[3]]), opcode);
            let declared_len = u32::from_le_bytes([encoded[4], encoded[5], encoded[6], encoded[7]]) as usize;
            assert_eq!(encoded.len(), 8 + declared_len);

            let (decoded_op, decoded_val) =
                decode_discord_ipc_frame(&encoded).expect("decode valid frame");
            assert_eq!(decoded_op, opcode);
            assert_eq!(decoded_val, payload);
        }

        // Truncated header
        assert!(decode_discord_ipc_frame(&[]).is_err());
        assert!(decode_discord_ipc_frame(&[1, 0, 0, 0, 5, 0, 0]).is_err());

        // Truncated body
        let mut truncated_body = Vec::new();
        truncated_body.extend_from_slice(&OP_FRAME.to_le_bytes());
        truncated_body.extend_from_slice(&32u32.to_le_bytes());
        truncated_body.extend_from_slice(br#"{"v":1}"#);
        assert!(decode_discord_ipc_frame(&truncated_body).is_err());

        // Oversized header (> 1 MiB)
        let mut oversized_hdr = Vec::new();
        oversized_hdr.extend_from_slice(&OP_FRAME.to_le_bytes());
        oversized_hdr.extend_from_slice(&((MAX_IPC_FRAME_BYTES as u32) + 1).to_le_bytes());
        let err = decode_discord_ipc_frame(&oversized_hdr).expect_err("reject oversized frame");
        assert!(err.contains("Oversized"));

        // Oversized encode payload (> 1 MiB)
        let big_str = "A".repeat(MAX_IPC_FRAME_BYTES + 64);
        let big_payload = serde_json::json!({ "blob": big_str });
        assert!(encode_discord_ipc_frame(OP_FRAME, &big_payload).is_err());

        // Invalid UTF-8 / JSON payload
        let mut bad_json = Vec::new();
        bad_json.extend_from_slice(&OP_FRAME.to_le_bytes());
        bad_json.extend_from_slice(&4u32.to_le_bytes());
        bad_json.extend_from_slice(&[0xFF, 0xFE, 0x00, 0x01]);
        assert!(decode_discord_ipc_frame(&bad_json).is_err());
    }

    #[test]
    fn set_activity_payload_pc_retro_and_coop_variants() {
        // 1. PC game
        let pc = build_set_activity_payload(
            4242,
            "Cyberpunk 2077",
            "PC",
            None,
            1_700_000_000,
            "nonce-pc",
        );
        assert_eq!(pc["cmd"], "SET_ACTIVITY");
        assert_eq!(pc["nonce"], "nonce-pc");
        assert_eq!(pc["args"]["pid"], 4242);
        assert_eq!(pc["args"]["activity"]["details"], "Cyberpunk 2077");
        assert_eq!(pc["args"]["activity"]["state"], "PC");
        assert_eq!(pc["args"]["activity"]["timestamps"]["start"], 1_700_000_000);
        assert_eq!(pc["args"]["activity"]["assets"]["small_text"], "PC");
        assert!(
            pc["args"]["activity"]["assets"]["large_text"]
                .as_str()
                .expect("large_text str")
                .contains("Cyberpunk 2077")
        );

        // 2. Retro console badge
        let retro = build_set_activity_payload(
            8800,
            "Chrono Trigger",
            "SNES",
            None,
            1_700_000_123,
            "nonce-snes",
        );
        assert_eq!(retro["args"]["activity"]["details"], "Chrono Trigger");
        assert_eq!(retro["args"]["activity"]["state"], "SNES");
        assert_eq!(retro["args"]["activity"]["assets"]["small_text"], "SNES");

        // 3. Duke Link co-op mode
        let coop = build_set_activity_payload(
            9911,
            "Mario Kart 64",
            "Nintendo 64",
            Some("Duke Link Co-op (2/4 Players)"),
            1_700_000_456,
            "nonce-coop",
        );
        assert_eq!(coop["args"]["activity"]["details"], "Mario Kart 64");
        let coop_state = coop["args"]["activity"]["state"]
            .as_str()
            .expect("coop state");
        assert!(coop_state.contains("Nintendo 64"));
        assert!(coop_state.contains("Duke Link Co-op (2/4 Players)"));
        assert_eq!(coop["args"]["activity"]["assets"]["small_text"], "Nintendo 64");

        // 4. Clear activity payload
        let cleared = build_clear_activity_payload(9911, "nonce-clear");
        assert_eq!(cleared["cmd"], "SET_ACTIVITY");
        assert_eq!(cleared["args"]["pid"], 9911);
        assert!(cleared["args"]["activity"].is_null());
    }

    #[test]
    fn status_camel_case_and_nonblocking_worker_lifecycle() {
        let _guard = lock_global_status_test();

        let sample = DiscordPresenceStatus {
            enabled: true,
            connected: false,
            pipe_name: Some(r"\\.\pipe\discord-ipc-0".to_string()),
            active_game: Some("Hades II".to_string()),
            platform_badge: Some("PC".to_string()),
            coop_status: Some("Duke Link Co-op".to_string()),
            session_started_ts: Some(1_700_000_999),
            frames_sent: 4,
            last_error: None,
        };
        let json = serde_json::to_value(&sample).expect("serialize status");
        assert_eq!(json["pipeName"], r"\\.\pipe\discord-ipc-0");
        assert_eq!(json["activeGame"], "Hades II");
        assert_eq!(json["platformBadge"], "PC");
        assert_eq!(json["coopStatus"], "Duke Link Co-op");
        assert_eq!(json["sessionStartedTs"], 1_700_000_999);
        assert_eq!(json["framesSent"], 4);

        set_discord_presence_enabled(true);
        set_discord_presence_game("Super Metroid", "SNES", Some("Duke Link Co-op"), Some(1_700_001_000));
        let st = get_discord_presence_status();
        assert!(st.enabled);
        assert_eq!(st.active_game.as_deref(), Some("Super Metroid"));
        assert_eq!(st.platform_badge.as_deref(), Some("SNES"));
        assert_eq!(st.coop_status.as_deref(), Some("Duke Link Co-op"));
        assert_eq!(st.session_started_ts, Some(1_700_001_000));

        clear_discord_presence();
        let cleared_st = get_discord_presence_status();
        assert!(cleared_st.active_game.is_none());
        assert!(cleared_st.platform_badge.is_none());
        assert!(cleared_st.coop_status.is_none());
        assert!(cleared_st.session_started_ts.is_none());

        set_discord_presence_enabled(false);
        set_discord_presence_game("Disabled Test", "PC", None, Some(123));
        let disabled_st = get_discord_presence_status();
        assert!(!disabled_st.enabled);
        assert!(!disabled_st.connected);
        assert!(disabled_st.active_game.is_none());

        clear_discord_presence();
        set_discord_presence_enabled(true);
    }

    #[cfg(target_os = "windows")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn windows_named_pipe_handshake_and_set_activity_roundtrip() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::windows::named_pipe::ServerOptions;

        let pipe_path = format!(
            r"\\.\pipe\duke-discord-ipc-test-{}-{}",
            std::process::id(),
            TEST_PIPE_SEQ.fetch_add(1, Ordering::Relaxed)
        );
        let mut server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&pipe_path)
            .expect("create mock named pipe server");

        let server_task = tokio::spawn(async move {
            server.connect().await.expect("accept client pipe");

            async fn read_wire_frame(
                srv: &mut tokio::net::windows::named_pipe::NamedPipeServer,
            ) -> (u32, serde_json::Value) {
                let mut hdr = [0u8; 8];
                srv.read_exact(&mut hdr).await.expect("read frame header");
                let len = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]) as usize;
                let mut full = vec![0u8; 8 + len];
                full[..8].copy_from_slice(&hdr);
                srv.read_exact(&mut full[8..]).await.expect("read frame body");
                decode_discord_ipc_frame(&full).expect("decode server frame")
            }

            let (hs_op, hs_json) = read_wire_frame(&mut server).await;
            assert_eq!(hs_op, OP_HANDSHAKE);
            assert_eq!(hs_json["v"], 1);
            assert_eq!(hs_json["client_id"], "1350000000000000001");

            let ready = serde_json::json!({
                "cmd": "DISPATCH",
                "evt": "READY",
                "data": { "v": 1, "user": { "username": "duke-tester" } },
                "nonce": serde_json::Value::Null,
            });
            let ready_bytes = encode_discord_ipc_frame(OP_FRAME, &ready).expect("encode ready");
            server.write_all(&ready_bytes).await.expect("write ready");
            server.flush().await.expect("flush ready");

            let (act_op, act_json) = read_wire_frame(&mut server).await;
            assert_eq!(act_op, OP_FRAME);
            assert_eq!(act_json["cmd"], "SET_ACTIVITY");
            assert_eq!(act_json["args"]["activity"]["details"], "Elden Ring");
            assert_eq!(
                act_json["args"]["activity"]["state"],
                "PC • Duke Link Co-op (2/3)"
            );
            assert_eq!(act_json["args"]["activity"]["assets"]["small_text"], "PC");

            let ack = serde_json::json!({
                "cmd": "SET_ACTIVITY",
                "evt": serde_json::Value::Null,
                "data": { "name": "Duke Launcher" },
                "nonce": act_json["nonce"].clone(),
            });
            let ack_bytes = encode_discord_ipc_frame(OP_FRAME, &ack).expect("encode ack");
            server.write_all(&ack_bytes).await.expect("write ack");
            server.flush().await.expect("flush ack");

            (hs_json, act_json)
        });

        let activity_cmd = build_set_activity_payload(
            std::process::id(),
            "Elden Ring",
            "PC",
            Some("Duke Link Co-op (2/3)"),
            1_700_002_000,
            "nonce-e2e",
        );
        let client_pipe = pipe_path.clone();
        let client_task = tokio::task::spawn_blocking(move || {
            send_activity_over_pipe(&client_pipe, "1350000000000000001", &activity_cmd)
        });

        let client_res = tokio::time::timeout(Duration::from_secs(5), client_task)
            .await
            .expect("client task timeout")
            .expect("client task join");
        assert!(client_res.is_ok(), "send_activity_over_pipe failed: {client_res:?}");

        let (hs_json, act_json) = tokio::time::timeout(Duration::from_secs(5), server_task)
            .await
            .expect("server task timeout")
            .expect("server task join");
        assert_eq!(hs_json["client_id"], "1350000000000000001");
        assert_eq!(act_json["nonce"], "nonce-e2e");
    }

    #[cfg(target_os = "windows")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn windows_named_pipe_handles_close_and_missing_pipe_errors() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::windows::named_pipe::ServerOptions;

        let missing_pipe = format!(
            r"\\.\pipe\duke-discord-missing-{}-{}",
            std::process::id(),
            TEST_PIPE_SEQ.fetch_add(1, Ordering::Relaxed)
        );
        let dummy_cmd = build_set_activity_payload(1, "Test", "PC", None, 10, "n");
        let err = send_activity_over_pipe(&missing_pipe, "1350000000000000001", &dummy_cmd)
            .expect_err("missing pipe should fail");
        assert!(!err.is_empty());

        let close_pipe = format!(
            r"\\.\pipe\duke-discord-close-{}-{}",
            std::process::id(),
            TEST_PIPE_SEQ.fetch_add(1, Ordering::Relaxed)
        );
        let mut server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&close_pipe)
            .expect("create close test pipe");

        let server_task = tokio::spawn(async move {
            server.connect().await.expect("accept close test client");
            let mut hdr = [0u8; 8];
            server.read_exact(&mut hdr).await.expect("read hdr");
            let len = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]) as usize;
            let mut body = vec![0u8; len];
            server.read_exact(&mut body).await.expect("read body");

            let close_payload = serde_json::json!({ "code": 4000, "message": "Invalid client ID" });
            let close_frame = encode_discord_ipc_frame(OP_CLOSE, &close_payload).expect("encode close");
            server.write_all(&close_frame).await.expect("write close");
            server.flush().await.expect("flush close");
        });

        let client_pipe = close_pipe.clone();
        let client_res = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::task::spawn_blocking(move || {
                send_activity_over_pipe(&client_pipe, "invalid", &dummy_cmd)
            }),
        )
        .await
        .expect("timeout")
        .expect("join");

        server_task.await.expect("server join");
        let close_err = client_res.expect_err("OP_CLOSE must surface error");
        assert!(close_err.contains("Invalid client ID"));
    }
}

//! Self-healing, leader-elected Windows Named Pipe event bus (`\\.\pipe\duke-suite`).
//!
//! The first Duke Suite process to acquire `first_pipe_instance(true)` acts as the
//! broadcast hub; subsequent processes connect as clients. If the current hub exits,
//! remaining peers automatically detect EOF/broken pipe and re-elect a new hub within <150 ms.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{broadcast, mpsc, watch};

pub const DUKE_PIPE_NAME: &str = r"\\.\pipe\duke-suite";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum DukeEvent {
    GameStarted {
        title: String,
        exe_name: String,
        pid: u32,
    },
    GameStopped {
        title: String,
        exe_name: String,
        pid: u32,
    },
    ProfileSwitched {
        app: String,
        profile_name: String,
    },
    ClipSaved {
        game_title: String,
        file_path: String,
        duration_secs: f32,
    },
    RamPurgeRequested {
        source: String,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct PipeEnvelope {
    origin: String,
    event: DukeEvent,
}

static INSTANCE_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub struct DukeBus {
    app_name: String,
    outbox: mpsc::UnboundedSender<DukeEvent>,
    is_hub: Arc<AtomicBool>,
    is_connected: Arc<AtomicBool>,
    shutdown: watch::Sender<bool>,
}

impl DukeBus {
    /// Starts the leader-elected Named Pipe bus on `\\.\pipe\duke-suite`.
    pub fn start<F>(app_name: impl Into<String>, callback: F) -> Self
    where
        F: Fn(DukeEvent) + Send + Sync + 'static,
    {
        Self::start_on_pipe(app_name, DUKE_PIPE_NAME, callback)
    }

    /// Starts the leader-elected Named Pipe bus on an explicit pipe path.
    pub fn start_on_pipe<F>(
        app_name: impl Into<String>,
        pipe_name: impl Into<String>,
        callback: F,
    ) -> Self
    where
        F: Fn(DukeEvent) + Send + Sync + 'static,
    {
        let app_name = app_name.into();
        let pipe_name = pipe_name.into();
        let seq = INSTANCE_SEQ.fetch_add(1, Ordering::Relaxed);
        let origin = format!("{}:{}:{}", app_name, std::process::id(), seq);

        let (outbox, outbox_rx) = mpsc::unbounded_channel::<DukeEvent>();
        let (shutdown, shutdown_rx) = watch::channel(false);
        let is_hub = Arc::new(AtomicBool::new(false));
        let is_connected = Arc::new(AtomicBool::new(false));
        let cb: Arc<dyn Fn(DukeEvent) + Send + Sync> = Arc::new(callback);

        let runner = run_bus_loop(
            pipe_name,
            origin,
            outbox_rx,
            shutdown_rx,
            is_hub.clone(),
            is_connected.clone(),
            cb,
        );

        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(runner);
        } else {
            let thread_name = format!("duke-bus-{}", app_name);
            let _ = std::thread::Builder::new().name(thread_name).spawn(move || {
                if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    rt.block_on(runner);
                }
            });
        }

        Self {
            app_name,
            outbox,
            is_hub,
            is_connected,
            shutdown,
        }
    }

    pub fn publish(&self, event: DukeEvent) {
        let _ = self.outbox.send(event);
    }

    pub fn broadcast(&self, event: DukeEvent) {
        self.publish(event);
    }

    pub fn is_hub(&self) -> bool {
        self.is_hub.load(Ordering::Acquire)
    }

    pub fn is_online(&self) -> bool {
        self.is_hub() || self.is_connected.load(Ordering::Acquire)
    }

    pub fn app_name(&self) -> &str {
        &self.app_name
    }

    pub fn stop(&self) {
        let _ = self.shutdown.send(true);
    }
}

#[cfg(target_os = "windows")]
async fn run_bus_loop(
    pipe_name: String,
    origin: String,
    mut outbox_rx: mpsc::UnboundedReceiver<DukeEvent>,
    mut shutdown_rx: watch::Receiver<bool>,
    is_hub: Arc<AtomicBool>,
    is_connected: Arc<AtomicBool>,
    callback: Arc<dyn Fn(DukeEvent) + Send + Sync>,
) {
    use tokio::net::windows::named_pipe::{ClientOptions, ServerOptions};

    let mut pending_outbound: Option<DukeEvent> = None;

    while !*shutdown_rx.borrow() {
        // Attempt leader election via first_pipe_instance(true)
        match ServerOptions::new()
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .create(&pipe_name)
        {
            Ok(server) => {
                is_hub.store(true, Ordering::Release);
                is_connected.store(true, Ordering::Release);
                run_hub_role(
                    &pipe_name,
                    &origin,
                    server,
                    &mut pending_outbound,
                    &mut outbox_rx,
                    &mut shutdown_rx,
                    &callback,
                )
                .await;
                is_connected.store(false, Ordering::Release);
                is_hub.store(false, Ordering::Release);
            }
            Err(_) => {
                is_hub.store(false, Ordering::Release);
                is_connected.store(false, Ordering::Release);

                // Another Duke app currently owns \\.\pipe\duke-suite; connect as client
                let mut retry_count = 0u32;
                while !*shutdown_rx.borrow() {
                    match ClientOptions::new().open(&pipe_name) {
                        Ok(client) => {
                            is_connected.store(true, Ordering::Release);
                            run_client_role(
                                &origin,
                                client,
                                &mut pending_outbound,
                                &mut outbox_rx,
                                &mut shutdown_rx,
                                &callback,
                            )
                            .await;
                            is_connected.store(false, Ordering::Release);
                            // Hub disconnected/exited; break immediately to race for leadership (<150ms)
                            break;
                        }
                        Err(err) => {
                            let raw_err = err.raw_os_error().unwrap_or(0);
                            if raw_err == 2 {
                                // ERROR_FILE_NOT_FOUND: hub just exited, try leadership right away
                                tokio::time::sleep(Duration::from_millis(12)).await;
                                break;
                            }
                            retry_count = retry_count.saturating_add(1);
                            if retry_count >= 5 {
                                break;
                            }
                            tokio::time::sleep(Duration::from_millis(20)).await;
                        }
                    }
                }
            }
        }

        if *shutdown_rx.borrow() {
            break;
        }
        // Small backoff before next election round so winner establishes listener cleanly
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[cfg(target_os = "windows")]
async fn recv_next_outbound(
    pending: &mut Option<DukeEvent>,
    outbox_rx: &mut mpsc::UnboundedReceiver<DukeEvent>,
) -> Option<DukeEvent> {
    if let Some(ev) = pending.take() {
        return Some(ev);
    }
    outbox_rx.recv().await
}

#[cfg(target_os = "windows")]
async fn run_hub_role(
    pipe_name: &str,
    origin: &str,
    mut server: tokio::net::windows::named_pipe::NamedPipeServer,
    pending_outbound: &mut Option<DukeEvent>,
    outbox_rx: &mut mpsc::UnboundedReceiver<DukeEvent>,
    shutdown_rx: &mut watch::Receiver<bool>,
    callback: &Arc<dyn Fn(DukeEvent) + Send + Sync>,
) {
    use tokio::net::windows::named_pipe::ServerOptions;

    let (bcast_tx, _) = broadcast::channel::<(u64, String)>(256);
    let (incoming_tx, mut incoming_rx) = mpsc::unbounded_channel::<(String, DukeEvent)>();
    let mut next_conn_id: u64 = 1;

    while !*shutdown_rx.borrow() {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                break;
            }
            conn_res = server.connect() => {
                if conn_res.is_err() {
                    break;
                }
                let connected = server;
                match ServerOptions::new()
                    .first_pipe_instance(false)
                    .reject_remote_clients(true)
                    .create(pipe_name)
                {
                    Ok(next_server) => {
                        server = next_server;
                    }
                    Err(_) => {
                        spawn_hub_client_task(
                            next_conn_id,
                            connected,
                            bcast_tx.clone(),
                            incoming_tx.clone(),
                            shutdown_rx.clone(),
                        );
                        break;
                    }
                }

                let conn_id = next_conn_id;
                next_conn_id = next_conn_id.wrapping_add(1).max(1);
                spawn_hub_client_task(
                    conn_id,
                    connected,
                    bcast_tx.clone(),
                    incoming_tx.clone(),
                    shutdown_rx.clone(),
                );
            }
            maybe_ev = recv_next_outbound(pending_outbound, outbox_rx) => {
                let Some(ev) = maybe_ev else {
                    break;
                };
                if let Ok(mut line) = serde_json::to_string(&PipeEnvelope {
                    origin: origin.to_string(),
                    event: ev,
                }) {
                    line.push('\n');
                    let _ = bcast_tx.send((0, line));
                }
            }
            Some((msg_origin, ev)) = incoming_rx.recv() => {
                if msg_origin != origin {
                    callback(ev);
                }
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn spawn_hub_client_task(
    conn_id: u64,
    pipe: tokio::net::windows::named_pipe::NamedPipeServer,
    bcast_tx: broadcast::Sender<(u64, String)>,
    incoming_tx: mpsc::UnboundedSender<(String, DukeEvent)>,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let mut bcast_rx = bcast_tx.subscribe();
    tokio::spawn(async move {
        let (read_half, mut write_half) = tokio::io::split(pipe);
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        loop {
            line.clear();
            tokio::select! {
                _ = shutdown_rx.changed() => {
                    break;
                }
                read_res = reader.read_line(&mut line) => {
                    match read_res {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            let trimmed = line.trim();
                            if trimmed.is_empty() {
                                continue;
                            }
                            if let Ok(env) = serde_json::from_str::<PipeEnvelope>(trimmed) {
                                let _ = incoming_tx.send((env.origin.clone(), env.event.clone()));
                                let mut fwd = trimmed.to_string();
                                fwd.push('\n');
                                let _ = bcast_tx.send((conn_id, fwd));
                            }
                        }
                    }
                }
                recv_res = bcast_rx.recv() => {
                    match recv_res {
                        Ok((sender_conn_id, payload)) => {
                            if sender_conn_id == conn_id {
                                continue;
                            }
                            if write_half.write_all(payload.as_bytes()).await.is_err() {
                                break;
                            }
                            if write_half.flush().await.is_err() {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        }
    });
}

#[cfg(target_os = "windows")]
async fn run_client_role(
    origin: &str,
    client: tokio::net::windows::named_pipe::NamedPipeClient,
    pending_outbound: &mut Option<DukeEvent>,
    outbox_rx: &mut mpsc::UnboundedReceiver<DukeEvent>,
    shutdown_rx: &mut watch::Receiver<bool>,
    callback: &Arc<dyn Fn(DukeEvent) + Send + Sync>,
) {
    let (read_half, mut write_half) = tokio::io::split(client);
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();

    if let Some(ev) = pending_outbound.take()
        && let Ok(mut serialized) = serde_json::to_string(&PipeEnvelope {
            origin: origin.to_string(),
            event: ev.clone(),
        }) {
            serialized.push('\n');
            if write_half.write_all(serialized.as_bytes()).await.is_err()
                || write_half.flush().await.is_err()
            {
                *pending_outbound = Some(ev);
                return;
            }
        }

    while !*shutdown_rx.borrow() {
        line.clear();
        tokio::select! {
            _ = shutdown_rx.changed() => {
                break;
            }
            read_res = reader.read_line(&mut line) => {
                match read_res {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        if let Ok(env) = serde_json::from_str::<PipeEnvelope>(trimmed)
                            && env.origin != origin {
                                callback(env.event);
                            }
                    }
                }
            }
            maybe_ev = outbox_rx.recv() => {
                let Some(ev) = maybe_ev else {
                    break;
                };
                if let Ok(mut serialized) = serde_json::to_string(&PipeEnvelope {
                    origin: origin.to_string(),
                    event: ev.clone(),
                }) {
                    serialized.push('\n');
                    if write_half.write_all(serialized.as_bytes()).await.is_err()
                        || write_half.flush().await.is_err()
                    {
                        *pending_outbound = Some(ev);
                        break;
                    }
                }
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
async fn run_bus_loop(
    _pipe_name: String,
    _origin: String,
    mut outbox_rx: mpsc::UnboundedReceiver<DukeEvent>,
    mut shutdown_rx: watch::Receiver<bool>,
    _is_hub: Arc<AtomicBool>,
    is_connected: Arc<AtomicBool>,
    _callback: Arc<dyn Fn(DukeEvent) + Send + Sync>,
) {
    is_connected.store(true, Ordering::Release);
    while !*shutdown_rx.borrow() {
        tokio::select! {
            _ = shutdown_rx.changed() => break,
            ev = outbox_rx.recv() => {
                if ev.is_none() {
                    break;
                }
            }
        }
    }
    is_connected.store(false, Ordering::Release);
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::Instant;

    #[tokio::test]
    async fn pipe_bus_broadcasts_and_reelects_under_150ms() {
        let pipe = format!(r"\\.\pipe\duke-suite-test-{}", std::process::id());
        let rx_a = Arc::new(Mutex::new(Vec::<DukeEvent>::new()));
        let rx_b = Arc::new(Mutex::new(Vec::<DukeEvent>::new()));
        let rx_c = Arc::new(Mutex::new(Vec::<DukeEvent>::new()));

        let rx_a_cb = rx_a.clone();
        let bus_a = DukeBus::start_on_pipe("app-a", &pipe, move |ev| {
            rx_a_cb.lock().unwrap().push(ev);
        });

        let start = Instant::now();
        while !bus_a.is_hub() && start.elapsed() < Duration::from_millis(500) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(bus_a.is_hub());

        let rx_b_cb = rx_b.clone();
        let bus_b = DukeBus::start_on_pipe("app-b", &pipe, move |ev| {
            rx_b_cb.lock().unwrap().push(ev);
        });

        let rx_c_cb = rx_c.clone();
        let bus_c = DukeBus::start_on_pipe("app-c", &pipe, move |ev| {
            rx_c_cb.lock().unwrap().push(ev);
        });

        tokio::time::sleep(Duration::from_millis(80)).await;
        assert!(!bus_b.is_hub());
        assert!(!bus_c.is_hub());

        bus_a.publish(DukeEvent::GameStarted {
            title: "Elden Ring".to_string(),
            exe_name: "eldenring.exe".to_string(),
            pid: 4242,
        });

        let wait_start = Instant::now();
        while wait_start.elapsed() < Duration::from_millis(400) {
            if !rx_b.lock().unwrap().is_empty() && !rx_c.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(rx_b.lock().unwrap().len(), 1);
        assert_eq!(rx_c.lock().unwrap().len(), 1);
        assert!(rx_a.lock().unwrap().is_empty());

        // Stop current hub A and verify B or C re-elects within 150ms
        let fail_start = Instant::now();
        bus_a.stop();
        while !bus_b.is_hub() && !bus_c.is_hub() && fail_start.elapsed() < Duration::from_millis(500)
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let elapsed = fail_start.elapsed();
        assert!(
            bus_b.is_hub() || bus_c.is_hub(),
            "New hub was not elected"
        );
        assert!(
            elapsed < Duration::from_millis(150),
            "Re-election took {:?} (>= 150ms)",
            elapsed
        );

        tokio::time::sleep(Duration::from_millis(60)).await;
        if bus_b.is_hub() {
            bus_b.publish(DukeEvent::RamPurgeRequested {
                source: "app-b".to_string(),
            });
            tokio::time::sleep(Duration::from_millis(80)).await;
            assert!(rx_c
                .lock()
                .unwrap()
                .iter()
                .any(|e| matches!(e, DukeEvent::RamPurgeRequested { .. })));
        } else {
            bus_c.publish(DukeEvent::RamPurgeRequested {
                source: "app-c".to_string(),
            });
            tokio::time::sleep(Duration::from_millis(80)).await;
            assert!(rx_b
                .lock()
                .unwrap()
                .iter()
                .any(|e| matches!(e, DukeEvent::RamPurgeRequested { .. })));
        }

        bus_b.stop();
        bus_c.stop();
    }
}

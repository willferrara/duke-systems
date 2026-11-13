//! `duke-core` — Shared ultra-optimized Windows systems, IPC, networking, tray,
//! HID, and macro primitives for the Duke Suite applications.
//!
//! All capabilities are behind granular Cargo feature gates (`default = []`) so
//! lightweight processes like `duke-input` compile zero TLS/HTTP overhead.

#[cfg(any(feature = "win32-trim", feature = "eco-qos"))]
pub mod win32_ffi;

#[cfg(feature = "net-doh")]
pub mod net_doh;

#[cfg(feature = "pipe-ipc")]
pub mod pipe_ipc;

#[cfg(feature = "tray-daemon")]
pub mod tray_daemon;

#[cfg(feature = "hid-raw")]
pub mod hid_raw;

#[cfg(feature = "macro-engine")]
pub mod macro_engine;

#[cfg(feature = "shm-bus")]
pub mod shm_bus;

#[cfg(feature = "nvme-prefetch")]
pub mod nvme_prefetch;

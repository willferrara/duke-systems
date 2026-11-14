use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::net::Ipv4Addr;

pub const SWARM_MULTICAST_V4: Ipv4Addr = Ipv4Addr::new(239, 255, 77, 88);
pub const SWARM_DISCOVERY_PORT: u16 = 47880;
pub const SWARM_DEFAULT_TCP_PORT: u16 = 47881;
pub const SWARM_MAGIC: &str = "DKSWRM1";
pub const MAX_FRAME_BYTES: usize = 32 * 1024 * 1024;
pub const DKDELTA_MAGIC: &[u8; 7] = b"DKDLT01";

pub type VectorClock = BTreeMap<String, u64>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClockOrdering {
    Equal,
    Before,
    After,
    Concurrent,
}

pub fn compare_vector_clocks(local: &VectorClock, remote: &VectorClock) -> ClockOrdering {
    let mut keys = HashSet::new();
    for k in local.keys() {
        keys.insert(k.as_str());
    }
    for k in remote.keys() {
        keys.insert(k.as_str());
    }

    let mut local_gt = false;
    let mut remote_gt = false;
    for k in keys {
        let l = local.get(k).copied().unwrap_or(0);
        let r = remote.get(k).copied().unwrap_or(0);
        if l > r {
            local_gt = true;
        } else if r > l {
            remote_gt = true;
        }
    }

    match (local_gt, remote_gt) {
        (false, false) => ClockOrdering::Equal,
        (false, true) => ClockOrdering::Before,
        (true, false) => ClockOrdering::After,
        (true, true) => ClockOrdering::Concurrent,
    }
}

pub fn merge_vector_clocks(local: &mut VectorClock, remote: &VectorClock) {
    for (k, &v) in remote {
        let slot = local.entry(k.clone()).or_insert(0);
        if v > *slot {
            *slot = v;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkEntry {
    pub gear_hash: u64,
    pub offset: u64,
    pub length: u32,
    pub blake3: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestFileEntry {
    pub rel_path: String,
    pub size_bytes: u64,
    pub blake3: String,
    pub chunks: Vec<ChunkEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DkManifest {
    pub magic: String,
    pub root_label: String,
    pub created_at: u64,
    pub chunk_min: u32,
    pub chunk_avg: u32,
    pub chunk_max: u32,
    pub total_bytes: u64,
    pub files: Vec<ManifestFileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SwarmAppAd {
    pub game_key: String,
    pub local_id: String,
    pub title: String,
    pub app_id: Option<u32>,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwarmBeacon {
    pub magic: String,
    pub node_id: String,
    pub node_name: String,
    pub tcp_port: u16,
    pub apps: Vec<SwarmAppAd>,
    pub save_clocks: BTreeMap<String, VectorClock>,
    pub timestamp_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SwarmWireRequest {
    GetBeacon,
    BeamManifest {
        game_key: String,
    },
    BeamDeltaBundle {
        game_key: String,
        missing_hashes: Vec<String>,
    },
    SaveSyncOffer {
        save_key: String,
        sender_clock: VectorClock,
        manifest: Option<DkManifest>,
        delta_bytes: Option<Vec<u8>>,
    },
    SaveSyncPull {
        save_key: String,
        have_hashes: Vec<String>,
    },
    ShaderPull {
        game_blake3: String,
        gpu_key: String,
        driver_version: String,
    },
    ShaderOffer {
        game_blake3: String,
        gpu_key: String,
        driver_version: String,
        file_count: usize,
        bundle_zst_bytes: Vec<u8>,
    },
    ManifestOffer {
        manifest_key: String,
        manifest: DkManifest,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SwarmWireResponse {
    Beacon(SwarmBeacon),
    Manifest(DkManifest),
    DeltaBundle {
        bundle_bytes: Vec<u8>,
    },
    SaveState {
        clock: VectorClock,
        manifest: Option<DkManifest>,
        bundle_bytes: Option<Vec<u8>>,
        applied: bool,
        conflict_backup: bool,
    },
    ShaderBundle {
        found: bool,
        bundle_zst_bytes: Vec<u8>,
        key: String,
        file_count: usize,
    },
    ShaderStored {
        stored: bool,
        key: String,
        bytes: usize,
    },
    ManifestStored {
        stored: bool,
        manifest_key: String,
        chunk_count: usize,
    },
    Error {
        message: String,
    },
}

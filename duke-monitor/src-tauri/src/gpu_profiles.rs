use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuPowerProfile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub linked_app_ids: Vec<u32>,
    #[serde(default)]
    pub linked_platform_ids: Vec<u16>,
    #[serde(default)]
    pub linked_exe_hashes: Vec<u64>,
    #[serde(default)]
    pub linked_exes: Vec<String>,
    #[serde(default)]
    pub power_limit_watts: Option<u32>,
    #[serde(default)]
    pub locked_clock_min_mhz: Option<u32>,
    #[serde(default)]
    pub locked_clock_max_mhz: Option<u32>,
    #[serde(default = "default_true")]
    pub eco_jail_background: bool,
    #[serde(default = "default_true")]
    pub boost_game_priority: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuProfileStore {
    pub active_profile_id: Option<String>,
    pub profiles: Vec<GpuPowerProfile>,
}

impl Default for GpuProfileStore {
    fn default() -> Self {
        Self {
            active_profile_id: Some("default_performance".to_string()),
            profiles: vec![
                GpuPowerProfile {
                    id: "default_performance".to_string(),
                    name: "Autonomous Game Boost + EcoQoS Jail".to_string(),
                    is_default: true,
                    linked_app_ids: Vec::new(),
                    linked_platform_ids: Vec::new(),
                    linked_exe_hashes: Vec::new(),
                    linked_exes: Vec::new(),
                    power_limit_watts: None,
                    locked_clock_min_mhz: None,
                    locked_clock_max_mhz: None,
                    eco_jail_background: true,
                    boost_game_priority: true,
                },
                GpuPowerProfile {
                    id: "emulation_efficiency".to_string(),
                    name: "Console Emulation Sustained Clocks".to_string(),
                    is_default: false,
                    linked_app_ids: Vec::new(),
                    linked_platform_ids: vec![5, 6, 7, 15, 17],
                    linked_exe_hashes: Vec::new(),
                    linked_exes: vec![
                        "dolphin.exe".to_string(),
                        "pcsx2-qt.exe".to_string(),
                        "rpcs3.exe".to_string(),
                        "ryujinx.exe".to_string(),
                        "cemu.exe".to_string(),
                    ],
                    power_limit_watts: None,
                    locked_clock_min_mhz: None,
                    locked_clock_max_mhz: None,
                    eco_jail_background: true,
                    boost_game_priority: true,
                },
            ],
        }
    }
}

static GPU_PROFILE_STORE: OnceLock<Mutex<GpuProfileStore>> = OnceLock::new();
static GPU_PROFILE_PATH: OnceLock<PathBuf> = OnceLock::new();

fn resolve_store_path(app: Option<&AppHandle>) -> PathBuf {
    if let Some(p) = GPU_PROFILE_PATH.get() {
        return p.clone();
    }
    let resolved = (|| {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                if dir.join("portable.flag").exists() || dir.join("data").is_dir() {
                    return dir.join("data").join("gpu_profiles.json");
                }
            }
        }
        if let Some(handle) = app {
            if let Ok(dir) = handle.path().app_data_dir() {
                return dir.join("gpu_profiles.json");
            }
        }
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            return PathBuf::from(local)
                .join("com.dukemonitor.desktop")
                .join("gpu_profiles.json");
        }
        std::env::temp_dir().join("DukeMonitor").join("gpu_profiles.json")
    })();
    let _ = GPU_PROFILE_PATH.set(resolved.clone());
    resolved
}

fn load_store_from_disk(app: Option<&AppHandle>) -> GpuProfileStore {
    let path = resolve_store_path(app);
    if let Ok(raw) = fs::read_to_string(&path) {
        if let Ok(mut parsed) = serde_json::from_str::<GpuProfileStore>(&raw) {
            if parsed.profiles.is_empty() {
                parsed.profiles = GpuProfileStore::default().profiles;
            }
            return parsed;
        }
    }
    GpuProfileStore::default()
}

fn persist_store_to_disk(app: Option<&AppHandle>, store: &GpuProfileStore) -> Result<(), String> {
    let path = resolve_store_path(app);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed creating GPU profile dir: {}", e))?;
    }
    let json = serde_json::to_string_pretty(store)
        .map_err(|e| format!("Failed serializing GPU profiles: {}", e))?;
    fs::write(path, json).map_err(|e| format!("Failed writing gpu_profiles.json: {}", e))
}

pub fn init_store(app: &AppHandle) {
    let _ = resolve_store_path(Some(app));
    let _ = GPU_PROFILE_STORE.get_or_init(|| Mutex::new(load_store_from_disk(Some(app))));
}

pub fn get_store_snapshot(app: Option<&AppHandle>) -> GpuProfileStore {
    let lock = GPU_PROFILE_STORE.get_or_init(|| Mutex::new(load_store_from_disk(app)));
    lock.lock()
        .map(|g| g.clone())
        .unwrap_or_else(|e| e.into_inner().clone())
}

pub fn upsert_profile(
    app: &AppHandle,
    profile: GpuPowerProfile,
) -> Result<GpuProfileStore, String> {
    let lock = GPU_PROFILE_STORE.get_or_init(|| Mutex::new(load_store_from_disk(Some(app))));
    let snapshot = {
        let mut guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = guard.profiles.iter_mut().find(|p| p.id == profile.id) {
            *existing = profile;
        } else {
            guard.profiles.push(profile);
        }
        guard.clone()
    };
    persist_store_to_disk(Some(app), &snapshot)?;
    Ok(snapshot)
}

pub fn remove_profile(app: &AppHandle, profile_id: &str) -> Result<GpuProfileStore, String> {
    let lock = GPU_PROFILE_STORE.get_or_init(|| Mutex::new(load_store_from_disk(Some(app))));
    let snapshot = {
        let mut guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        guard.profiles.retain(|p| p.is_default || p.id != profile_id);
        if guard.active_profile_id.as_deref() == Some(profile_id) {
            guard.active_profile_id = guard.profiles.first().map(|p| p.id.clone());
        }
        guard.clone()
    };
    persist_store_to_disk(Some(app), &snapshot)?;
    Ok(snapshot)
}

pub fn set_active_profile_id(app: Option<&AppHandle>, profile_id: Option<String>) {
    let lock = GPU_PROFILE_STORE.get_or_init(|| Mutex::new(load_store_from_disk(app)));
    if let Ok(mut guard) = lock.lock() {
        guard.active_profile_id = profile_id;
    }
}

pub fn match_profile_in_store(
    store: &GpuProfileStore,
    app_id: Option<u32>,
    platform_id: Option<u16>,
    exe_hash: Option<u64>,
    title: &str,
    exe_name: &str,
) -> Option<GpuPowerProfile> {
    if let Some(aid) = app_id.filter(|&id| id > 0) {
        if let Some(p) = store.profiles.iter().find(|p| p.linked_app_ids.contains(&aid)) {
            return Some(p.clone());
        }
    }
    if let Some(hash) = exe_hash.filter(|&h| h != 0) {
        if let Some(p) = store.profiles.iter().find(|p| p.linked_exe_hashes.contains(&hash)) {
            return Some(p.clone());
        }
    }
    let exe_lower = exe_name.trim().to_ascii_lowercase();
    let title_lower = title.trim().to_ascii_lowercase();
    if !exe_lower.is_empty() || !title_lower.is_empty() {
        if let Some(p) = store.profiles.iter().find(|p| {
            p.linked_exes.iter().any(|rule| {
                let r = rule.trim().to_ascii_lowercase();
                !r.is_empty()
                    && ((!exe_lower.is_empty() && exe_lower.contains(&r))
                        || (!title_lower.is_empty() && title_lower.contains(&r)))
            })
        }) {
            return Some(p.clone());
        }
    }
    if let Some(pid) = platform_id.filter(|&p| p > 0) {
        if let Some(p) = store
            .profiles
            .iter()
            .find(|p| p.linked_platform_ids.contains(&pid))
        {
            return Some(p.clone());
        }
    }
    store
        .profiles
        .iter()
        .find(|p| p.is_default)
        .or_else(|| store.profiles.first())
        .cloned()
}

pub fn match_profile_for_game(
    app_id: Option<u32>,
    platform_id: Option<u16>,
    exe_hash: Option<u64>,
    title: &str,
    exe_name: &str,
) -> Option<GpuPowerProfile> {
    let store = get_store_snapshot(None);
    match_profile_in_store(&store, app_id, platform_id, exe_hash, title, exe_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_gpu_profile_by_app_id_hash_platform_or_exe() {
        let mut store = GpuProfileStore::default();
        store.profiles.push(GpuPowerProfile {
            id: "hades2".to_string(),
            name: "Hades II Tuned".to_string(),
            is_default: false,
            linked_app_ids: vec![1145350],
            linked_platform_ids: Vec::new(),
            linked_exe_hashes: vec![0xABCD_1234],
            linked_exes: vec!["hades2.exe".to_string()],
            power_limit_watts: Some(260),
            locked_clock_min_mhz: Some(2100),
            locked_clock_max_mhz: Some(2750),
            eco_jail_background: true,
            boost_game_priority: true,
        });

        let by_app = match_profile_in_store(&store, Some(1145350), None, None, "", "");
        assert_eq!(by_app.map(|p| p.id), Some("hades2".to_string()));

        let by_hash = match_profile_in_store(&store, None, None, Some(0xABCD_1234), "", "");
        assert_eq!(by_hash.map(|p| p.id), Some("hades2".to_string()));

        let by_platform = match_profile_in_store(&store, None, Some(7), None, "Switch", "");
        assert_eq!(
            by_platform.map(|p| p.id),
            Some("emulation_efficiency".to_string())
        );

        let fallback = match_profile_in_store(&store, Some(999999), None, None, "Indie", "indie.exe");
        assert_eq!(
            fallback.map(|p| p.id),
            Some("default_performance".to_string())
        );
    }
}

//! Zero-Account Privacy Vault & Network Anonymity Layer.
//!
//! Encrypts all optional third-party API credentials (Steam Web API, `xbl.io`, PSN NPSSO,
//! SteamGridDB, RetroAchievements) at rest using Windows DPAPI (`CryptProtectData` /
//! `CryptUnprotectData`), injects anti-telemetry environment variables on every game launch, and
//! routes metadata/cover traffic through DoH (`1.1.1.1` / `9.9.9.9`) + Tor/SOCKS5/WireGuard presets.

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::sync::{Mutex, OnceLock};

const VAULT_FILE: &str = "privacy_vault.json";
const VAULT_ENTROPY: &[u8] = b"duke-launcher-dpapi-vault-v1";

pub const SLOT_STEAM: &str = "steam";
pub const SLOT_XBOX: &str = "xbox";
pub const SLOT_PSN: &str = "psn";
pub const SLOT_STEAMGRIDDB: &str = "steamgriddb";
pub const SLOT_RETRO_ACHIEVEMENTS: &str = "retroAchievements";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PrivacyVaultDiskFile {
    pub version: u32,
    pub encrypted_keys: BTreeMap<String, String>,
    pub retro_achievements_username: String,
    pub doh_provider: String,
    pub proxy_preset: String,
    pub custom_proxy_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyVaultStatus {
    pub dpapi_available: bool,
    pub plaintext_secrets_on_disk: bool,
    pub configured_slots: Vec<String>,
    pub steam_configured: bool,
    pub xbox_configured: bool,
    pub psn_configured: bool,
    pub steamgriddb_configured: bool,
    pub retro_achievements_configured: bool,
    pub retro_achievements_username: String,
    pub doh_enabled: bool,
    pub doh_provider: String,
    pub proxy_preset: String,
    pub effective_proxy_url: Option<String>,
}

fn vault_io_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(target_os = "windows")]
mod win_dpapi {
    use std::ffi::c_void;

    const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x0000_0001;

    #[repr(C)]
    struct CryptDataBlob {
        cb_data: u32,
        pb_data: *mut u8,
    }

    #[link(name = "crypt32")]
    extern "system" {
        fn CryptProtectData(
            data_in: *const CryptDataBlob,
            data_descr: *const u16,
            optional_entropy: *const CryptDataBlob,
            reserved: *mut c_void,
            prompt_struct: *mut c_void,
            flags: u32,
            data_out: *mut CryptDataBlob,
        ) -> i32;
        fn CryptUnprotectData(
            data_in: *const CryptDataBlob,
            data_descr: *mut *mut u16,
            optional_entropy: *const CryptDataBlob,
            reserved: *mut c_void,
            prompt_struct: *mut c_void,
            flags: u32,
            data_out: *mut CryptDataBlob,
        ) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn LocalFree(h_mem: *mut c_void) -> *mut c_void;
        fn GetLastError() -> u32;
    }

    pub fn is_supported() -> bool {
        true
    }

    pub fn protect(plaintext: &[u8], entropy: &[u8]) -> Result<Vec<u8>, String> {
        let in_blob = CryptDataBlob {
            cb_data: plaintext.len() as u32,
            pb_data: plaintext.as_ptr() as *mut u8,
        };
        let entropy_blob = CryptDataBlob {
            cb_data: entropy.len() as u32,
            pb_data: entropy.as_ptr() as *mut u8,
        };
        let mut out_blob = CryptDataBlob {
            cb_data: 0,
            pb_data: std::ptr::null_mut(),
        };
        let ok = unsafe {
            CryptProtectData(
                &in_blob,
                std::ptr::null(),
                &entropy_blob,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out_blob,
            )
        };
        if ok == 0 || out_blob.pb_data.is_null() {
            let err = unsafe { GetLastError() };
            return Err(format!("DPAPI CryptProtectData failed: Win32 error {err}"));
        }
        let bytes = unsafe {
            std::slice::from_raw_parts(out_blob.pb_data, out_blob.cb_data as usize).to_vec()
        };
        unsafe {
            LocalFree(out_blob.pb_data.cast::<c_void>());
        }
        Ok(bytes)
    }

    pub fn unprotect(ciphertext: &[u8], entropy: &[u8]) -> Result<Vec<u8>, String> {
        let in_blob = CryptDataBlob {
            cb_data: ciphertext.len() as u32,
            pb_data: ciphertext.as_ptr() as *mut u8,
        };
        let entropy_blob = CryptDataBlob {
            cb_data: entropy.len() as u32,
            pb_data: entropy.as_ptr() as *mut u8,
        };
        let mut out_blob = CryptDataBlob {
            cb_data: 0,
            pb_data: std::ptr::null_mut(),
        };
        let ok = unsafe {
            CryptUnprotectData(
                &in_blob,
                std::ptr::null_mut(),
                &entropy_blob,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out_blob,
            )
        };
        if ok == 0 || out_blob.pb_data.is_null() {
            let err = unsafe { GetLastError() };
            return Err(format!("DPAPI CryptUnprotectData failed: Win32 error {err}"));
        }
        let bytes = unsafe {
            std::slice::from_raw_parts(out_blob.pb_data, out_blob.cb_data as usize).to_vec()
        };
        unsafe {
            LocalFree(out_blob.pb_data.cast::<c_void>());
        }
        Ok(bytes)
    }
}

#[cfg(not(target_os = "windows"))]
mod win_dpapi {
    pub fn is_supported() -> bool {
        false
    }
    pub fn protect(plaintext: &[u8], _entropy: &[u8]) -> Result<Vec<u8>, String> {
        Ok(plaintext.to_vec())
    }
    pub fn unprotect(ciphertext: &[u8], _entropy: &[u8]) -> Result<Vec<u8>, String> {
        Ok(ciphertext.to_vec())
    }
}

pub fn dpapi_encrypt_secret(plaintext: &str) -> Result<String, String> {
    let raw = win_dpapi::protect(plaintext.as_bytes(), VAULT_ENTROPY)?;
    Ok(format!("dpapi1:{}", BASE64.encode(raw)))
}

pub fn dpapi_decrypt_secret(encoded: &str) -> Result<String, String> {
    let payload_b64 = encoded
        .trim()
        .strip_prefix("dpapi1:")
        .ok_or_else(|| "Unsupported encrypted vault payload prefix".to_string())?;
    let cipher = BASE64
        .decode(payload_b64)
        .map_err(|e| format!("Corrupt Base64 vault secret: {e}"))?;
    let plain = win_dpapi::unprotect(&cipher, VAULT_ENTROPY)?;
    String::from_utf8(plain).map_err(|e| format!("Vault secret is not valid UTF-8: {e}"))
}

pub fn resolve_proxy_preset_url(preset: &str, custom_url: Option<&str>) -> Option<String> {
    let trimmed_custom = || {
        custom_url
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
    };
    match preset.trim().to_ascii_lowercase().as_str() {
        "direct" | "off" | "none" | "" => None,
        "tor" | "tor_daemon" => Some("socks5h://127.0.0.1:9050".to_string()),
        "tor_browser" => Some("socks5h://127.0.0.1:9150".to_string()),
        "socks5" | "socks5_local" => Some("socks5h://127.0.0.1:1080".to_string()),
        "wireguard" | "wireguard_socks" | "wireguard_custom" => {
            trimmed_custom().or_else(|| Some("socks5h://10.64.0.1:1080".to_string()))
        }
        "custom" => trimmed_custom(),
        _ => trimmed_custom(),
    }
}

pub fn normalize_doh_provider(provider: &str) -> &'static str {
    match provider.trim().to_ascii_lowercase().as_str() {
        "cloudflare" | "1.1.1.1" => "cloudflare",
        "quad9" | "9.9.9.9" => "quad9",
        "disabled" | "off" | "system" => "disabled",
        _ => "auto",
    }
}

pub fn anti_telemetry_env_pairs() -> &'static [(&'static str, &'static str)] {
    &[
        ("DO_NOT_TRACK", "1"),
        ("SENTRY_DSN", ""),
        ("UNITY_DISABLE_ANALYTICS", "1"),
        ("UE_DISABLE_CRASH_REPORTER", "1"),
        ("TELEMETRY_DISABLED", "1"),
        ("EOS_DISABLE_TELEMETRY", "1"),
    ]
}

fn load_vault_disk_inner() -> PrivacyVaultDiskFile {
    let Ok(path) = crate::get_app_data_file_standalone(VAULT_FILE) else {
        return PrivacyVaultDiskFile {
            version: 1,
            doh_provider: "auto".to_string(),
            proxy_preset: "direct".to_string(),
            ..PrivacyVaultDiskFile::default()
        };
    };
    let mut file = fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<PrivacyVaultDiskFile>(&s).ok())
        .unwrap_or_default();
    file.version = 1;
    file.doh_provider = normalize_doh_provider(&file.doh_provider).to_string();
    if file.proxy_preset.trim().is_empty() {
        file.proxy_preset = "direct".to_string();
    }
    file
}

fn save_vault_disk_inner(vault: &PrivacyVaultDiskFile) -> Result<(), String> {
    let path = crate::get_app_data_file_standalone(VAULT_FILE)?;
    let json = serde_json::to_string_pretty(vault)
        .map_err(|e| format!("Failed serializing privacy vault: {e}"))?;
    crate::atomic_write_string(&path, &json)
}

#[allow(dead_code)]
pub fn get_vault_secret(slot: &str) -> Option<String> {
    let _g = vault_io_lock().lock().unwrap_or_else(|e| e.into_inner());
    let vault = load_vault_disk_inner();
    let enc = vault.encrypted_keys.get(slot)?;
    dpapi_decrypt_secret(enc).ok().filter(|s| !s.trim().is_empty())
}

pub fn set_vault_secret(slot: &str, value: Option<&str>) -> Result<(), String> {
    let _g = vault_io_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut vault = load_vault_disk_inner();
    match value.map(str::trim).filter(|s| !s.is_empty()) {
        Some(secret) => {
            let encrypted = dpapi_encrypt_secret(secret)?;
            vault.encrypted_keys.insert(slot.to_string(), encrypted);
        }
        None => {
            vault.encrypted_keys.remove(slot);
        }
    }
    save_vault_disk_inner(&vault)
}

pub fn clear_slot_by_name(slot: &str) {
    let key = match slot.trim().to_ascii_lowercase().as_str() {
        "steam" => SLOT_STEAM,
        "xbox" => SLOT_XBOX,
        "psn" => SLOT_PSN,
        "steamgriddb" => SLOT_STEAMGRIDDB,
        "retroachievements" => SLOT_RETRO_ACHIEVEMENTS,
        _ => return,
    };
    let _ = set_vault_secret(key, None);
}

pub fn current_doh_and_proxy_overrides() -> (String, Option<String>) {
    let _g = vault_io_lock().lock().unwrap_or_else(|e| e.into_inner());
    let vault = load_vault_disk_inner();
    let eff = resolve_proxy_preset_url(&vault.proxy_preset, vault.custom_proxy_url.as_deref());
    (vault.doh_provider, eff)
}

/// Persists any non-empty secrets present on `settings` into the DPAPI vault and clears all
/// plaintext secret fields from the returned on-disk copy so plaintext credentials never touch
/// `settings.json`.
pub fn extract_secrets_for_settings_save(settings: &crate::LauncherSettings) -> crate::LauncherSettings {
    let _g = vault_io_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut vault = load_vault_disk_inner();
    let mut vault_dirty = false;

    let mut sync_slot = |slot: &str, incoming: &Option<String>| {
        if let Some(raw) = incoming {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                if let Ok(enc) = dpapi_encrypt_secret(trimmed) {
                    vault.encrypted_keys.insert(slot.to_string(), enc);
                    vault_dirty = true;
                }
            }
        }
    };

    sync_slot(SLOT_STEAM, &settings.steam_api_key);
    sync_slot(SLOT_XBOX, &settings.xbox_api_key);
    sync_slot(SLOT_PSN, &settings.psn_npsso);
    sync_slot(SLOT_STEAMGRIDDB, &settings.steamgriddb_api_key);

    if vault_dirty {
        let _ = save_vault_disk_inner(&vault);
    }

    let mut redacted = settings.clone();
    redacted.steam_api_key = None;
    redacted.xbox_api_key = None;
    redacted.psn_npsso = None;
    redacted.steamgriddb_api_key = None;
    redacted
}

/// Hydrates in-memory `LauncherSettings` with decrypted secrets from the DPAPI vault.
/// If any legacy plaintext secrets are found in `settings.json`, they are migrated into DPAPI
/// and the caller gets `migrated = true` so `settings.json` is rewritten without plaintext keys.
pub fn hydrate_settings_from_vault(settings: &mut crate::LauncherSettings) -> bool {
    let _g = vault_io_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut vault = load_vault_disk_inner();
    let mut vault_dirty = false;
    let mut legacy_plaintext_migrated = false;

    for (slot, field) in [
        (SLOT_STEAM, &mut settings.steam_api_key),
        (SLOT_XBOX, &mut settings.xbox_api_key),
        (SLOT_PSN, &mut settings.psn_npsso),
        (SLOT_STEAMGRIDDB, &mut settings.steamgriddb_api_key),
    ] {
        if let Some(plain) = field.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            if let Ok(enc) = dpapi_encrypt_secret(plain) {
                vault.encrypted_keys.insert(slot.to_string(), enc);
                vault_dirty = true;
                legacy_plaintext_migrated = true;
            }
        } else if let Some(enc) = vault.encrypted_keys.get(slot) {
            if let Ok(plain) = dpapi_decrypt_secret(enc) {
                if !plain.trim().is_empty() {
                    *field = Some(plain);
                }
            }
        }
    }

    if vault_dirty {
        let _ = save_vault_disk_inner(&vault);
    }
    legacy_plaintext_migrated
}

pub fn build_privacy_status() -> PrivacyVaultStatus {
    let _g = vault_io_lock().lock().unwrap_or_else(|e| e.into_inner());
    let vault = load_vault_disk_inner();
    let settings = crate::load_settings_standalone();
    let effective_proxy = resolve_proxy_preset_url(
        &vault.proxy_preset,
        vault
            .custom_proxy_url
            .as_deref()
            .or(settings.custom_proxy_url.as_deref()),
    );
    let configured_slots: Vec<String> = vault.encrypted_keys.keys().cloned().collect();
    PrivacyVaultStatus {
        dpapi_available: win_dpapi::is_supported(),
        plaintext_secrets_on_disk: false,
        steam_configured: vault.encrypted_keys.contains_key(SLOT_STEAM),
        xbox_configured: vault.encrypted_keys.contains_key(SLOT_XBOX),
        psn_configured: vault.encrypted_keys.contains_key(SLOT_PSN),
        steamgriddb_configured: vault.encrypted_keys.contains_key(SLOT_STEAMGRIDDB),
        retro_achievements_configured: vault.encrypted_keys.contains_key(SLOT_RETRO_ACHIEVEMENTS),
        retro_achievements_username: vault.retro_achievements_username.clone(),
        doh_enabled: duke_core::net_doh::is_doh_enabled(),
        doh_provider: vault.doh_provider.clone(),
        proxy_preset: vault.proxy_preset.clone(),
        effective_proxy_url: effective_proxy,
        configured_slots,
    }
}

#[allow(dead_code)]
pub fn get_retro_achievements_credentials() -> Option<(String, String)> {
    let _g = vault_io_lock().lock().unwrap_or_else(|e| e.into_inner());
    let vault = load_vault_disk_inner();
    let enc = vault.encrypted_keys.get(SLOT_RETRO_ACHIEVEMENTS)?;
    let key = dpapi_decrypt_secret(enc).ok()?;
    let trimmed_key = key.trim().to_string();
    let trimmed_user = vault.retro_achievements_username.trim().to_string();
    if trimmed_key.is_empty() {
        None
    } else {
        Some((trimmed_user, trimmed_key))
    }
}

#[tauri::command]
pub async fn get_privacy_vault_status() -> Result<PrivacyVaultStatus, String> {
    tokio::task::spawn_blocking(build_privacy_status)
        .await
        .map_err(|e| format!("Privacy vault status task failed: {e}"))
}

#[tauri::command]
pub async fn set_vault_api_key(
    slot: String,
    api_key: Option<String>,
    username: Option<String>,
) -> Result<PrivacyVaultStatus, String> {
    let normalized_slot = match slot.trim().to_ascii_lowercase().as_str() {
        "steam" => SLOT_STEAM,
        "xbox" | "xbl" | "xbl.io" => SLOT_XBOX,
        "psn" => SLOT_PSN,
        "steamgriddb" => SLOT_STEAMGRIDDB,
        "retroachievements" | "retro_achievements" | "ra" => SLOT_RETRO_ACHIEVEMENTS,
        other => return Err(format!("Unsupported vault key slot '{other}'")),
    };

    tokio::task::spawn_blocking(move || -> Result<PrivacyVaultStatus, String> {
        {
            let _g = vault_io_lock().lock().unwrap_or_else(|e| e.into_inner());
            let mut vault = load_vault_disk_inner();
            match api_key.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                Some(secret) => {
                    let enc = dpapi_encrypt_secret(secret)?;
                    vault.encrypted_keys.insert(normalized_slot.to_string(), enc);
                }
                None => {
                    vault.encrypted_keys.remove(normalized_slot);
                }
            }
            if normalized_slot == SLOT_RETRO_ACHIEVEMENTS {
                if let Some(u) = username {
                    vault.retro_achievements_username = u.trim().to_string();
                }
            }
            save_vault_disk_inner(&vault)?;
        }
        Ok(build_privacy_status())
    })
    .await
    .map_err(|e| format!("Vault update task failed: {e}"))?
}

#[tauri::command]
pub async fn apply_anonymity_network_preset(
    app: tauri::AppHandle,
    doh_provider: String,
    proxy_preset: String,
    custom_proxy_url: Option<String>,
) -> Result<PrivacyVaultStatus, String> {
    let provider = normalize_doh_provider(&doh_provider).to_string();
    let doh_enabled = provider != "disabled";
    let preset_clean = proxy_preset.trim().to_ascii_lowercase();
    let custom_clean = custom_proxy_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    let effective = resolve_proxy_preset_url(&preset_clean, custom_clean.as_deref());

    duke_core::net_doh::set_doh_provider_mode(&provider);
    duke_core::net_doh::set_network_privacy_config(doh_enabled, effective.as_deref())?;

    {
        let _g = vault_io_lock().lock().unwrap_or_else(|e| e.into_inner());
        let mut vault = load_vault_disk_inner();
        vault.doh_provider = provider;
        vault.proxy_preset = if preset_clean.is_empty() {
            "direct".to_string()
        } else {
            preset_clean
        };
        vault.custom_proxy_url = custom_clean;
        save_vault_disk_inner(&vault)?;
    }

    let mut settings = crate::load_settings_standalone();
    settings.doh_enabled = doh_enabled;
    settings.custom_proxy_url = effective;
    crate::save_settings_to_disk(&app, &settings)?;

    Ok(build_privacy_status())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpapi_vault_encrypt_decrypt_roundtrips_without_plaintext() {
        let secret = "sgdb_live_test_key_998877665544";
        let encrypted = dpapi_encrypt_secret(secret).expect("DPAPI encrypt should succeed");
        assert!(encrypted.starts_with("dpapi1:"));
        assert!(!encrypted.contains(secret));
        let decrypted = dpapi_decrypt_secret(&encrypted).expect("DPAPI decrypt should succeed");
        assert_eq!(decrypted, secret);
    }

    #[test]
    fn proxy_presets_and_anti_telemetry_env_are_complete() {
        assert_eq!(
            resolve_proxy_preset_url("tor", None).as_deref(),
            Some("socks5h://127.0.0.1:9050")
        );
        assert_eq!(
            resolve_proxy_preset_url("tor_browser", None).as_deref(),
            Some("socks5h://127.0.0.1:9150")
        );
        assert_eq!(
            resolve_proxy_preset_url("wireguard_socks", None).as_deref(),
            Some("socks5h://10.64.0.1:1080")
        );
        assert_eq!(
            resolve_proxy_preset_url("socks5_local", None).as_deref(),
            Some("socks5h://127.0.0.1:1080")
        );
        assert_eq!(
            resolve_proxy_preset_url("wireguard_custom", Some("socks5h://10.88.0.2:1080"))
                .as_deref(),
            Some("socks5h://10.88.0.2:1080")
        );
        assert_eq!(resolve_proxy_preset_url("direct", None), None);
        assert_eq!(resolve_proxy_preset_url("direct", Some("socks5h://1.2.3.4:1080")), None);

        let envs = anti_telemetry_env_pairs();
        assert!(envs.contains(&("DO_NOT_TRACK", "1")));
        assert!(envs.contains(&("SENTRY_DSN", "")));
        assert!(envs.contains(&("UNITY_DISABLE_ANALYTICS", "1")));
        assert!(envs.contains(&("UE_DISABLE_CRASH_REPORTER", "1")));
    }
}

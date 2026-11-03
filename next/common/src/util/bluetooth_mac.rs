//  Copyright 2023 The Android Open Source Project
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Persistent Bluetooth MAC address assignment for virtual devices.
//!
//! MACs are stored per AVD in `netsim.ini` when `avd_path` is known, and
//! otherwise tracked in memory by device name for the lifetime of netsimd.

use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fmt::Write as _,
    fs,
    io::Read as _,
    path::{Path, PathBuf},
};

use tokio::sync::Mutex;
use tracing::{info, warn};

use super::ini_file::{IniParserOptions, parse_ini};

#[derive(Debug, Default, PartialEq)]
struct NetsimConfig {
    bluetooth_address: Option<String>,
}

fn parse_avd_ini(content: &str) -> Result<HashMap<String, String>, String> {
    parse_ini(content, &IniParserOptions { strict: true }).map_err(|e| e.to_string())
}

fn serialize_ini(config: &NetsimConfig) -> String {
    let mut content = String::new();
    if let Some(address) = &config.bluetooth_address {
        let _ = writeln!(content, "bluetooth.address = {address}");
    }
    content
}

impl NetsimConfig {
    fn from_ini(content: &str) -> Result<Self, String> {
        let map = parse_avd_ini(content)?;
        Ok(NetsimConfig { bluetooth_address: map.get("bluetooth.address").cloned() })
    }
}

fn list_avd_ini_files(avd_root: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut ini_files = Vec::new();
    if !avd_root.exists() {
        return Ok(ini_files);
    }
    for entry in fs::read_dir(avd_root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file()
            && let Some(extension) = path.extension()
            && extension == "ini"
        {
            ini_files.push(path);
        } else if path.is_dir()
            && (path.extension().and_then(|ext| ext.to_str()) == Some("avd")
                || path.join("netsim.ini").exists())
        {
            ini_files.push(path);
        }
    }
    Ok(ini_files)
}

/// Validates that a Bluetooth MAC address consists of 6 two-digit hex octets
/// separated by colons or hyphens (e.g. "00:11:22:33:44:55" or "00-11-22-33-44-55").
pub fn is_valid_mac_address(addr: &str) -> bool {
    let separator = if addr.contains(':') {
        ':'
    } else if addr.contains('-') {
        '-'
    } else {
        return false;
    };
    let parts: Vec<&str> = addr.split(separator).collect();
    parts.len() == 6
        && parts.iter().all(|part| part.len() == 2 && part.chars().all(|c| c.is_ascii_hexdigit()))
}

fn normalize_mac_address(addr: &str) -> String {
    addr.to_ascii_uppercase().replace('-', ":")
}

fn get_path_from_avd_ini(ini_content: &str) -> Result<PathBuf, String> {
    for line in ini_content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("path=")
            && let Some((_, path_value)) = trimmed.split_once('=')
        {
            return Ok(PathBuf::from(path_value.trim()));
        }
    }
    Err("Could not find the 'path=' key in the AVD .ini file.".to_string())
}

const MAX_INI_FILE_BYTES: u64 = 64 * 1024;

fn read_ini_file_bounded(path: &Path) -> Result<String, Box<dyn Error>> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(format!("INI path is not a regular file: {}", path.display()).into());
    }
    if metadata.len() > MAX_INI_FILE_BYTES {
        return Err(format!("INI file exceeds maximum size: {}", path.display()).into());
    }
    let file = fs::File::open(path)?;
    let mut content = String::new();
    file.take(MAX_INI_FILE_BYTES + 1).read_to_string(&mut content)?;
    if content.len() as u64 > MAX_INI_FILE_BYTES {
        return Err(format!("INI file exceeds maximum size: {}", path.display()).into());
    }
    Ok(content)
}

fn get_avd_data_path(avd_path: &Path) -> Result<PathBuf, Box<dyn Error>> {
    if !avd_path.exists() {
        return Err(format!("AVD path not found: {}", avd_path.display()).into());
    }
    if avd_path.is_dir() {
        return Ok(avd_path.to_path_buf());
    }
    let content = read_ini_file_bounded(avd_path)?;
    let avd_dir_path = get_path_from_avd_ini(&content)?;
    Ok(avd_dir_path)
}

fn read_netsim_config_for_avd(avd_ini_path: &Path) -> Result<NetsimConfig, Box<dyn Error>> {
    let avd_dir_path = get_avd_data_path(avd_ini_path)?;
    let config_path = avd_dir_path.join("netsim.ini");
    if !config_path.exists() {
        return Ok(NetsimConfig::default());
    }
    let contents = read_ini_file_bounded(&config_path)?;
    NetsimConfig::from_ini(&contents).map_err(|e| e.into())
}

fn write_netsim_config_for_avd(
    avd_ini_path: &Path,
    config: &NetsimConfig,
) -> Result<(), Box<dyn Error>> {
    let avd_dir_path = get_avd_data_path(avd_ini_path)?;
    if !avd_dir_path.exists() {
        fs::create_dir_all(&avd_dir_path)?;
    }
    let config_path = avd_dir_path.join("netsim.ini");
    let contents = serialize_ini(config);
    fs::write(config_path, contents)?;
    Ok(())
}

fn read_all_bluetooth_addresses(avd_root: &Path) -> Result<HashSet<String>, Box<dyn Error>> {
    let mut used_addresses = HashSet::new();
    let ini_files = list_avd_ini_files(avd_root)?;
    for ini_path in ini_files {
        match read_netsim_config_for_avd(&ini_path) {
            Ok(config) => {
                if let Some(address) = config.bluetooth_address
                    && is_valid_mac_address(&address)
                {
                    used_addresses.insert(normalize_mac_address(&address));
                }
            }
            Err(error) => {
                warn!(
                    %error,
                    path = %ini_path.display(),
                    "Failed to read netsim.ini, skipping address scan"
                )
            }
        }
    }
    Ok(used_addresses)
}

fn generate_next_mac(used_addresses: &HashSet<String>) -> Result<String, Box<dyn Error>> {
    for i in 1..=0xFFFFFF {
        let b1 = (i >> 16) & 0xFF;
        let b2 = (i >> 8) & 0xFF;
        let b3 = i & 0xFF;
        let mac = format!("BB:BB:BB:{:02X}:{:02X}:{:02X}", b1, b2, b3);
        debug_assert!(is_valid_mac_address(&mac), "Generated MAC must be valid: {mac}");
        if !used_addresses.contains(&mac) {
            return Ok(mac);
        }
    }
    Err("No available MAC addresses in the range BB:BB:BB:00:00:01 - BB:BB:BB:FF:FF:FF".into())
}

/// Returns true if `mac` is already assigned to an active device other than the
/// AVD at `avd_ini_path`: another AVD path, or an in-memory (no `avd_path`)
/// device, named or anonymous. MACs that were only discovered by scanning sibling `netsim.ini`
/// files are not claims, so a secondary AVD keeps its persisted MAC.
fn is_claimed_by_other_device(cache: &BluetoothMacCache, avd_ini_path: &str, mac: &str) -> bool {
    let claimed_by_other_avd =
        cache.by_avd_path.iter().any(|(path, assigned)| path != avd_ini_path && assigned == mac);
    let claimed_by_any_avd = cache.by_avd_path.values().any(|assigned| assigned == mac);
    let claimed_in_memory = !claimed_by_any_avd
        && (cache.anonymous_claims.contains(mac)
            || cache.by_device.values().any(|assigned| assigned == mac));
    claimed_by_other_avd || claimed_in_memory
}

fn get_or_create_bluetooth_mac(
    avd_ini_path: &str,
    cache: &mut BluetoothMacCache,
) -> Result<String, Box<dyn Error>> {
    let avd_path = PathBuf::from(avd_ini_path);
    match read_netsim_config_for_avd(&avd_path) {
        Ok(config) => {
            if let Some(address) = config.bluetooth_address
                && !address.is_empty()
                && is_valid_mac_address(&address)
            {
                let normalized = normalize_mac_address(&address);
                if !is_claimed_by_other_device(cache, avd_ini_path, &normalized) {
                    if let Some(avd_root) = avd_path.parent()
                        && let Ok(disk_addresses) = read_all_bluetooth_addresses(avd_root)
                    {
                        cache.used_macs.extend(disk_addresses);
                    }
                    cache.by_avd_path.insert(avd_ini_path.to_string(), normalized.clone());
                    cache.used_macs.insert(normalized.clone());
                    return Ok(normalized);
                }
                warn!(
                    "Configured MAC {normalized} for AVD {avd_ini_path} collides with an active device; generating a new MAC"
                );
            }
        }
        Err(error) => {
            warn!(
                %error,
                "Error reading config for {}. Will attempt to create/overwrite.",
                avd_ini_path
            );
        }
    }

    let avd_root = avd_path.parent().ok_or("Invalid AVD path")?;
    let used_addresses = read_all_bluetooth_addresses(avd_root)?;
    cache.used_macs.extend(used_addresses);
    let new_mac = generate_next_mac(&cache.used_macs)?;

    let new_config = NetsimConfig { bluetooth_address: Some(new_mac.clone()) };
    write_netsim_config_for_avd(&avd_path, &new_config)?;
    cache.by_avd_path.insert(avd_ini_path.to_string(), new_mac.clone());
    cache.used_macs.insert(new_mac.clone());
    info!("Generated and assigned new MAC {} to AVD {}", new_mac, avd_ini_path);
    Ok(new_mac)
}

fn set_bluetooth_mac(avd_path: &str, address: &str) -> Result<(), Box<dyn Error>> {
    let avd_ini_path = PathBuf::from(avd_path);
    let mut config = read_netsim_config_for_avd(&avd_ini_path).unwrap_or_default();
    if !address.is_empty() {
        if !is_valid_mac_address(address) {
            return Err(format!("Invalid Bluetooth MAC address: '{address}'").into());
        }
        if config.bluetooth_address.as_deref() == Some(address) {
            return Ok(());
        }
        config.bluetooth_address = Some(address.to_string());
    } else {
        config.bluetooth_address = None;
    }
    write_netsim_config_for_avd(&avd_ini_path, &config)?;
    info!("Successfully updated MAC address for AVD: '{}'", avd_path);
    Ok(())
}

#[derive(Default)]
struct BluetoothMacCache {
    by_device: HashMap<String, String>,
    by_avd_path: HashMap<String, String>,
    /// Explicit MACs claimed by devices without a stable name (empty or
    /// "Unknown"). Like `by_device`, claims last for the netsimd lifetime.
    anonymous_claims: HashSet<String>,
    used_macs: HashSet<String>,
}

static AVD_CONFIG_LOCK: Mutex<Option<BluetoothMacCache>> = Mutex::const_new(None);

fn resolve_in_memory_bluetooth_mac(
    cache: &mut BluetoothMacCache,
    device_name: &str,
    provided_address: &str,
) -> String {
    if device_name.is_empty() || device_name == "Unknown" {
        if !provided_address.is_empty() && is_valid_mac_address(provided_address) {
            let normalized = normalize_mac_address(provided_address);
            cache.anonymous_claims.insert(normalized.clone());
            cache.used_macs.insert(normalized.clone());
            return normalized;
        }
        return String::new();
    }
    if !provided_address.is_empty() {
        if !is_valid_mac_address(provided_address) {
            warn!(
                "Refusing to persist invalid MAC address '{provided_address}' for device {device_name}"
            );
            return String::new();
        }
        let normalized = normalize_mac_address(provided_address);
        cache.by_device.insert(device_name.to_string(), normalized.clone());
        cache.used_macs.insert(normalized.clone());
        return normalized;
    }
    if let Some(existing) = cache.by_device.get(device_name)
        && is_valid_mac_address(existing)
    {
        info!("Reusing persistent in-memory MAC {existing} for device {device_name}");
        return existing.clone();
    }
    match generate_next_mac(&cache.used_macs) {
        Ok(new_mac) => {
            cache.by_device.insert(device_name.to_string(), new_mac.clone());
            cache.used_macs.insert(new_mac.clone());
            info!("Assigned persistent in-memory MAC {new_mac} to device {device_name}");
            new_mac
        }
        Err(error) => {
            warn!(%error, "Failed to generate in-memory MAC for device {device_name}");
            String::new()
        }
    }
}

/// Resolves or persists the Bluetooth MAC address asynchronously for an AVD path.
///
/// Serializes concurrent calls across async connection streams using an async
/// mutex, and offloads blocking filesystem directory scans and writes
/// to Tokio's blocking thread pool (`spawn_blocking`).
pub async fn resolve_bluetooth_mac(avd_path: &str, provided_address: &str) -> String {
    resolve_bluetooth_mac_for_device(avd_path, "", provided_address).await
}

/// Resolves or persists the Bluetooth MAC address for a device, using `avd_path`
/// when available and falling back to an in-memory map keyed by `device_name`
/// (e.g. when `qemu_next` connects without setting `avd_path`) so the device
/// retains a stable Bluetooth MAC address across `adb reboot`.
pub async fn resolve_bluetooth_mac_for_device(
    avd_path: &str,
    device_name: &str,
    provided_address: &str,
) -> String {
    let mut guard = AVD_CONFIG_LOCK.lock().await;
    if avd_path.is_empty() {
        let cache = guard.get_or_insert_with(BluetoothMacCache::default);
        return resolve_in_memory_bluetooth_mac(cache, device_name, provided_address);
    }
    let avd_path = avd_path.to_string();
    let device_name = device_name.to_string();
    let address = provided_address.to_string();
    tokio::task::spawn_blocking(move || {
        let cache = guard.get_or_insert_with(BluetoothMacCache::default);
        if address.is_empty() {
            match get_or_create_bluetooth_mac(&avd_path, cache) {
                Ok(addr) => {
                    if !device_name.is_empty() && device_name != "Unknown" {
                        cache.by_device.insert(device_name, addr.clone());
                    }
                    info!("Assigned persistent MAC {addr} to AVD {avd_path}");
                    addr
                }
                Err(error) => {
                    warn!(%error, "Failed to get/create MAC for {avd_path}");
                    String::new()
                }
            }
        } else {
            if !is_valid_mac_address(&address) {
                warn!("Refusing to persist invalid MAC address '{address}' for AVD {avd_path}");
                return String::new();
            }
            let normalized = normalize_mac_address(&address);
            if let Err(error) = set_bluetooth_mac(&avd_path, &normalized) {
                warn!(%error, "Failed to persist MAC {normalized} for AVD {avd_path}");
                return String::new();
            }
            cache.by_avd_path.insert(avd_path, normalized.clone());
            cache.used_macs.insert(normalized.clone());
            if !device_name.is_empty() && device_name != "Unknown" {
                cache.by_device.insert(device_name, normalized.clone());
            }
            normalized
        }
    })
    .await
    .unwrap_or_else(|error| {
        warn!(%error, "Failed to join spawn_blocking for resolve_bluetooth_mac");
        String::new()
    })
}

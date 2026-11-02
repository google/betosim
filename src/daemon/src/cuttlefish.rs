// Copyright 2026 The Android Open Source Project
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

//! Cuttlefish host configuration resolver.
//!
//! Parses `cuttlefish_config.json` to resolve guest network parameters
//! (RIL IP, prefix length, gateway, DNS) for each connected CVD instance.

use std::{
    collections::HashMap,
    env,
    net::IpAddr,
    path::{Path, PathBuf},
};

use netsim_model::CellNetworkConfig;
use serde::Deserialize;
use serde_json::Value;
use tracing::{info, warn};

/// RIL network configuration for a Cuttlefish instance.
///
/// Deserialized lazily per matched instance to isolate invalid configurations
/// (e.g. empty IP and prefix 255 when the mobile bridge is absent).
#[derive(Deserialize, Debug)]
struct InstanceRilConfig {
    ril_ipaddr: IpAddr,
    ril_prefixlen: u8,
    ril_gateway: IpAddr,
    ril_dns: IpAddr,
}

#[derive(Deserialize, Debug)]
struct CuttlefishConfig {
    instances: HashMap<String, Value>,
}

/// Resolves the cellular network configuration for a Cuttlefish instance
/// using the file path in the `CUTTLEFISH_CONFIG_FILE` environment variable,
/// or falling back to `$HOME/.cuttlefish_config.json`.
pub(crate) async fn resolve_cuttlefish_network_config(
    device_name: &str,
) -> Option<CellNetworkConfig> {
    let config_path = env::var_os("CUTTLEFISH_CONFIG_FILE").map(PathBuf::from).or_else(|| {
        env::var_os("HOME")
            .map(|h| PathBuf::from(h).join(".cuttlefish_config.json"))
            .filter(|p| p.exists())
    })?;
    resolve_cuttlefish_network_config_from_file(&config_path, device_name).await
}

/// Resolves the cellular network configuration from a specific
/// `cuttlefish_config.json` path.
async fn resolve_cuttlefish_network_config_from_file(
    config_path: &Path,
    device_name: &str,
) -> Option<CellNetworkConfig> {
    info!("Reading Cuttlefish config from {config_path:?} for device {device_name:?}");

    let bytes = match tokio::fs::read(config_path).await {
        Ok(b) => b,
        Err(error) => {
            warn!(%error, "Failed to read Cuttlefish config {config_path:?}");
            return None;
        }
    };

    let config: CuttlefishConfig = match serde_json::from_slice(&bytes) {
        Ok(c) => c,
        Err(error) => {
            warn!(%error, "Failed to parse Cuttlefish config {config_path:?}");
            return None;
        }
    };

    find_instance_network_config(&config, device_name)
}

fn find_instance_network_config(
    config: &CuttlefishConfig,
    device_name: &str,
) -> Option<CellNetworkConfig> {
    let inst = match find_instance_by_name(config, device_name) {
        Some(i) => i,
        None => {
            // Expected in forwarder mode where the device belongs to another deployment.
            warn!("Could not match Cuttlefish device {device_name:?} in cuttlefish_config.json");
            return None;
        }
    };

    let ril = match InstanceRilConfig::deserialize(inst) {
        Ok(ril) => ril,
        Err(error) => {
            warn!(
                %error,
                "Cuttlefish device {device_name:?} has no usable RIL network config; cellular data will be unconfigured"
            );
            return None;
        }
    };

    Some(CellNetworkConfig {
        ip_address: ril.ril_ipaddr,
        prefixlen: ril.ril_prefixlen,
        gateway: ril.ril_gateway,
        dns: ril.ril_dns,
    })
}

// Matches canonical instance names ("cvd-1" / "1") or `adb_ip_and_port`
// (e.g. "0.0.0.0:6520", used by run_cvd).
fn find_instance_by_name<'a>(config: &'a CuttlefishConfig, name: &str) -> Option<&'a Value> {
    let id = name.strip_prefix("cvd-").unwrap_or(name);
    if let Some(inst) = config.instances.get(id) {
        return Some(inst);
    }
    config
        .instances
        .values()
        .find(|inst| inst.get("adb_ip_and_port").and_then(Value::as_str) == Some(name))
}

#[cfg(test)]
mod tests {
    use std::{io::Write, net::Ipv4Addr};

    use tempfile::NamedTempFile;

    use super::*;

    const SINGLE_INSTANCE_JSON: &str = r#"{
        "instances": {
            "1": {
                "ril_ipaddr": "192.168.97.2",
                "ril_prefixlen": 30,
                "ril_gateway": "192.168.97.1",
                "ril_dns": "8.8.8.8"
            }
        }
    }"#;

    const MULTI_INSTANCE_JSON: &str = r#"{
        "instances": {
            "1": {
                "adb_ip_and_port": "0.0.0.0:6520",
                "ril_ipaddr": "192.168.97.2",
                "ril_prefixlen": 30,
                "ril_gateway": "192.168.97.1",
                "ril_dns": "8.8.8.8"
            },
            "2": {
                "adb_ip_and_port": "0.0.0.0:6521",
                "ril_ipaddr": "192.168.97.6",
                "ril_prefixlen": 30,
                "ril_gateway": "192.168.97.5",
                "ril_dns": "8.8.8.8"
            }
        }
    }"#;

    const CLOBBERED_JSON: &str = r#"{
        "instances": {
            "1": {
                "adb_ip_and_port": "0.0.0.0:6520",
                "ril_ipaddr": "",
                "ril_prefixlen": 255,
                "ril_gateway": "",
                "ril_dns": ""
            },
            "2": {
                "adb_ip_and_port": "0.0.0.0:6521",
                "ril_ipaddr": "192.168.97.6",
                "ril_prefixlen": 30,
                "ril_gateway": "192.168.97.5",
                "ril_dns": "8.8.8.8"
            }
        }
    }"#;

    #[tokio::test]
    async fn test_resolve_single_instance() {
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(SINGLE_INSTANCE_JSON.as_bytes()).unwrap();

        let cfg1 = resolve_cuttlefish_network_config_from_file(temp_file.path(), "1")
            .await
            .expect("Failed to resolve single instance by key");
        assert_eq!(cfg1.ip_address, IpAddr::V4(Ipv4Addr::new(192, 168, 97, 2)));

        let cfg2 = resolve_cuttlefish_network_config_from_file(temp_file.path(), "cvd-1")
            .await
            .expect("Failed to resolve single instance by cvd name");
        assert_eq!(cfg2.ip_address, IpAddr::V4(Ipv4Addr::new(192, 168, 97, 2)));
    }

    #[tokio::test]
    async fn test_resolve_multi_instance() {
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(MULTI_INSTANCE_JSON.as_bytes()).unwrap();

        let cfg1 = resolve_cuttlefish_network_config_from_file(temp_file.path(), "cvd-1")
            .await
            .expect("Failed to resolve instance 1 by cvd name");
        assert_eq!(cfg1.ip_address, IpAddr::V4(Ipv4Addr::new(192, 168, 97, 2)));
        assert_eq!(cfg1.prefixlen, 30);
        assert_eq!(cfg1.gateway, IpAddr::V4(Ipv4Addr::new(192, 168, 97, 1)));
        assert_eq!(cfg1.dns, IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)));

        let cfg1_adb =
            resolve_cuttlefish_network_config_from_file(temp_file.path(), "0.0.0.0:6520")
                .await
                .expect("Failed to resolve instance 1 by adb_ip_and_port");
        assert_eq!(cfg1_adb.ip_address, IpAddr::V4(Ipv4Addr::new(192, 168, 97, 2)));

        let cfg2 = resolve_cuttlefish_network_config_from_file(temp_file.path(), "cvd-2")
            .await
            .expect("Failed to resolve instance 2 by cvd name");
        assert_eq!(cfg2.ip_address, IpAddr::V4(Ipv4Addr::new(192, 168, 97, 6)));
        assert_eq!(cfg2.prefixlen, 30);
        assert_eq!(cfg2.gateway, IpAddr::V4(Ipv4Addr::new(192, 168, 97, 5)));
        assert_eq!(cfg2.dns, IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)));

        let cfg2_adb =
            resolve_cuttlefish_network_config_from_file(temp_file.path(), "0.0.0.0:6521")
                .await
                .expect("Failed to resolve instance 2 by adb_ip_and_port");
        assert_eq!(cfg2_adb.ip_address, IpAddr::V4(Ipv4Addr::new(192, 168, 97, 6)));
    }

    #[tokio::test]
    async fn test_bad_instance_does_not_poison_good_instance() {
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(CLOBBERED_JSON.as_bytes()).unwrap();

        // Instance 1 is the CF "ObtainConfig failed" shape: rejected on its own.
        assert!(
            resolve_cuttlefish_network_config_from_file(temp_file.path(), "0.0.0.0:6520")
                .await
                .is_none()
        );
        // Instance 2 still resolves.
        let cfg = resolve_cuttlefish_network_config_from_file(temp_file.path(), "0.0.0.0:6521")
            .await
            .unwrap();
        assert_eq!(cfg.ip_address, IpAddr::V4(Ipv4Addr::new(192, 168, 97, 6)));
        assert_eq!(cfg.prefixlen, 30);
    }

    #[tokio::test]
    async fn test_resolve_unmatched_returns_none() {
        let mut temp_single = NamedTempFile::new().unwrap();
        temp_single.write_all(SINGLE_INSTANCE_JSON.as_bytes()).unwrap();
        assert!(
            resolve_cuttlefish_network_config_from_file(temp_single.path(), "cvd-99")
                .await
                .is_none()
        );

        let mut temp_multi = NamedTempFile::new().unwrap();
        temp_multi.write_all(MULTI_INSTANCE_JSON.as_bytes()).unwrap();
        assert!(
            resolve_cuttlefish_network_config_from_file(temp_multi.path(), "cvd-99")
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_file_not_found() {
        assert!(
            resolve_cuttlefish_network_config_from_file(
                Path::new("/nonexistent/file.json"),
                "cvd-1",
            )
            .await
            .is_none()
        );
    }

    #[tokio::test]
    async fn test_partial_config_missing_ril_ipaddr() {
        const PARTIAL_JSON: &str = r#"{
            "instances": {
                "1": {
                    "ril_prefixlen": 30
                }
            }
        }"#;
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(PARTIAL_JSON.as_bytes()).unwrap();

        assert!(
            resolve_cuttlefish_network_config_from_file(temp_file.path(), "cvd-1").await.is_none()
        );
    }
}

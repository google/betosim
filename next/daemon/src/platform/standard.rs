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

//! Standard (Goldfish / Bazel) platform initialization policy.

use std::{
    collections::HashMap,
    future::{Ready, ready},
};

use device_actor::DeviceClient;
use packet_stream::{StreamAddress, Streams};

use crate::{args::Args, netsimd::RunResult};

#[derive(Debug, Default)]
pub(crate) struct PlatformTasks;

impl PlatformTasks {
    pub(crate) fn abort(&self) {}
}

pub(crate) fn preflight_check(args: &Args) -> Result<(), RunResult> {
    #[cfg(target_os = "linux")]
    if let Some(tap_config) = args.wifi.resolved_tap()
        && let Err(error) = wifi_actor::TapGateway::preflight_check(&tap_config)
    {
        return Err(RunResult::InitializationError(format!("TAP configuration failed: {error}")));
    }
    #[cfg(not(target_os = "linux"))]
    let _ = args;
    Ok(())
}

pub(crate) fn init_transport_listeners(_args: &Args, _streams: &mut Streams) -> Ready<()> {
    ready(())
}

/// No-op connector mode stub for standard (Goldfish / Bazel) platforms.
///
/// Connector/forwarder mode is Cuttlefish-only and is selected when
/// `--connector_instance` is passed.
///
/// Returns:
/// - `None`: Not running in connector mode; the caller should continue normal
///   `NetsimDaemon` startup. Standard builds always return `None` without
///   redirecting logs.
/// - `Some(result)`: Connector mode ran to completion and the caller must
///   return `result` immediately without starting the primary daemon (never
///   returned on standard builds).
pub(crate) fn run_connector_mode_if_requested(_args: &Args) -> Ready<Option<RunResult>> {
    ready(None)
}

pub(crate) fn start_servers(
    _args: &Args,
    _instance_num: u16,
    _device_client: &DeviceClient,
    _listener_addresses: &mut HashMap<String, StreamAddress>,
    _ini_data: &mut HashMap<String, String>,
) -> Result<PlatformTasks, RunResult> {
    Ok(PlatformTasks)
}

#[must_use]
pub(crate) const fn should_start_tcp_listener(resolved_tcp_port: Option<u16>) -> bool {
    resolved_tcp_port.is_some()
}

#[must_use]
pub(crate) const fn default_test_beacons() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_start_tcp_listener() {
        assert!(!should_start_tcp_listener(None));
        assert!(should_start_tcp_listener(Some(8080)));
    }

    #[test]
    fn test_default_test_beacons() {
        assert!(!default_test_beacons());
    }

    #[test]
    fn test_preflight_check_default_args_succeeds() {
        let args = Args::default();
        assert!(preflight_check(&args).is_ok());
    }

    #[tokio::test]
    async fn test_init_transport_listeners_noop() {
        let args = Args::default();
        let mut streams = Streams::new();
        init_transport_listeners(&args, &mut streams).await;
        assert!(streams.listener_address("netsim_dualfd").is_none());
    }

    #[tokio::test]
    async fn test_start_servers_leaves_listeners_and_ini_unchanged() {
        let args = Args::default();
        let (_runner, device_client) = device_actor::new();
        let mut listener_addresses = HashMap::new();
        let mut ini_data = HashMap::new();

        let tasks = start_servers(&args, 1, &device_client, &mut listener_addresses, &mut ini_data)
            .expect("start_servers should succeed");
        tasks.abort();

        assert!(listener_addresses.is_empty());
        assert!(ini_data.is_empty());
    }

    #[tokio::test]
    async fn test_standard_run_connector_mode_if_requested_returns_none() {
        let args = Args { connector_instance: Some(1), logtostderr: true, ..Default::default() };
        assert_eq!(run_connector_mode_if_requested(&args).await, None);
    }
}

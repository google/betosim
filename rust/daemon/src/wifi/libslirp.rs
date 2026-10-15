// Copyright 2024 The Android Open Source Project
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

/// LibSlirp Interface for Network Simulation
use crate::get_runtime;
use crate::wifi::error::WifiResult;

use bytes::Bytes;
use http_proxy::Manager;
pub use libslirp_rs::libslirp::LibSlirp;
use libslirp_rs::libslirp::ProxyManager;
use libslirp_rs::libslirp_config::{SlirpConfig, lookup_host_dns};
use netsim_proto::config::SlirpOptions as ProtoSlirpOptions;
use std::sync::mpsc;

pub fn slirp_run(opt: ProtoSlirpOptions, tx_bytes: mpsc::Sender<Bytes>) -> WifiResult<LibSlirp> {
    // TODO: Convert ProtoSlirpOptions to SlirpConfig.
    let http_proxy = Some(opt.http_proxy).filter(|s| !s.is_empty());
    let (proxy_manager, tx_proxy_bytes) = if let Some(proxy) = http_proxy {
        let (tx_proxy_bytes, rx_proxy_response) = mpsc::channel::<Bytes>();
        (
            Some(Box::new(Manager::new(&proxy, rx_proxy_response)?)
                as Box<dyn ProxyManager + 'static>),
            Some(tx_proxy_bytes),
        )
    } else {
        (None, None)
    };

    let mut config = SlirpConfig { http_proxy_on: proxy_manager.is_some(), ..Default::default() };

    if !opt.host_dns.is_empty() {
        config.host_dns = get_runtime().block_on(lookup_host_dns(&opt.host_dns))?;
    }

    Ok(LibSlirp::new(config, tx_bytes, proxy_manager, tx_proxy_bytes))
}

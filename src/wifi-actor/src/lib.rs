// Copyright 2025 The Android Open Source Project
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

pub(crate) mod client;
pub mod err;
pub(crate) mod error;
pub(crate) mod ftm;
pub(crate) mod gateway;
pub(crate) mod lifecycle;
pub(crate) mod mdns_forwarder;
pub(crate) mod medium;
pub(crate) mod pmsr;
pub(crate) mod service;
pub(crate) mod slirp_gateway;
pub(crate) mod stats;
#[cfg(target_os = "linux")]
pub(crate) mod tap_gateway;
pub(crate) mod wifi_actor;

use actor_framework::ResourceActor;
pub use client::WifiClient;
pub use error::WifiError;
pub use ftm::handle_ftm_request;
pub use gateway::GatewayTrait;
pub use medium::{Medium, types::Station, utils::create_hwsim_msg_from_frame};
pub use slirp_gateway::SlirpGateway;
pub use stats::{Clock, MockClock, SystemClock, WifiStats};
#[cfg(target_os = "linux")]
pub use tap_gateway::TapGateway;
pub use wifi_actor::{WifiActor, WifiReq, WifiResponse};

#[derive(Default, Debug, Clone)]
pub struct DebugArgs {
    pub debug_no_traffic: bool,
    pub debug_no_network: bool,
    pub debug_no_wmedium: bool,
    pub debug_no_mdns_wmedium: bool,
    pub debug_no_guest_to_host_mdns: bool,
}

/// Creates a new WifiActor runner and client.
pub fn new() -> (ResourceActor<WifiActor>, WifiClient) {
    let (runner, client) = ResourceActor::new(32);
    (runner, WifiClient::new(client))
}
// Touch to force clippy rerun

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use tracing_subscriber::layer::SubscriberExt;

    use super::*;

    const UNKNOWN_STATION_MAC: [u8; 6] = [0x02, 0x00, 0x00, 0x00, 0x01, 0x01];
    const GATEWAY_SRC_MAC: [u8; 6] = [0x02, 0x00, 0x00, 0x00, 0x01, 0x02];

    const UNKNOWN_STATION_ETHERNET_FRAME: [u8; crate::gateway::ETHERNET_HEADER_LEN] = [
        UNKNOWN_STATION_MAC[0],
        UNKNOWN_STATION_MAC[1],
        UNKNOWN_STATION_MAC[2],
        UNKNOWN_STATION_MAC[3],
        UNKNOWN_STATION_MAC[4],
        UNKNOWN_STATION_MAC[5],
        GATEWAY_SRC_MAC[0],
        GATEWAY_SRC_MAC[1],
        GATEWAY_SRC_MAC[2],
        GATEWAY_SRC_MAC[3],
        GATEWAY_SRC_MAC[4],
        GATEWAY_SRC_MAC[5],
        0x08,
        0x00, // EtherType: IPv4
    ];

    pub(crate) fn count_unknown_station_drop_logs<F: FnOnce() -> R, R>(f: F) -> (R, usize, usize) {
        struct LogCounter {
            warn_count: Arc<AtomicUsize>,
            debug_count: Arc<AtomicUsize>,
        }

        impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for LogCounter {
            fn on_event(
                &self,
                event: &tracing::Event<'_>,
                _ctx: tracing_subscriber::layer::Context<'_, S>,
            ) {
                struct Visitor(String);
                impl tracing::field::Visit for Visitor {
                    fn record_debug(
                        &mut self,
                        field: &tracing::field::Field,
                        value: &dyn std::fmt::Debug,
                    ) {
                        if field.name() == "message" {
                            self.0 = format!("{:?}", value);
                        }
                    }
                }

                let mut visitor = Visitor(String::new());
                event.record(&mut visitor);

                if visitor.0.contains("Dropping unicast packet to unknown station") {
                    match *event.metadata().level() {
                        tracing::Level::WARN => {
                            self.warn_count.fetch_add(1, Ordering::Relaxed);
                        }
                        tracing::Level::DEBUG => {
                            self.debug_count.fetch_add(1, Ordering::Relaxed);
                        }
                        _ => {}
                    }
                }
            }
        }

        let warn_count = Arc::new(AtomicUsize::new(0));
        let debug_count = Arc::new(AtomicUsize::new(0));
        let subscriber = tracing_subscriber::registry()
            .with(LogCounter { warn_count: warn_count.clone(), debug_count: debug_count.clone() });
        let _multi_dispatch_guard = tracing::Dispatch::new(tracing_subscriber::registry());

        let res = tracing::subscriber::with_default(subscriber, f);
        let warns = warn_count.load(Ordering::Relaxed);
        let debugs = debug_count.load(Ordering::Relaxed);
        (res, warns, debugs)
    }

    pub(crate) fn unknown_station_test_fixture()
    -> (Arc<ap_actor::SharedKeyStore>, crate::medium::Medium, bytes::Bytes) {
        let key_store = Arc::new(ap_actor::SharedKeyStore::new());
        let clock = Arc::new(stats::SystemClock);
        let wifi_stats = stats::WifiStats::new(clock);
        let debug = Arc::new(DebugArgs::default());
        let medium = crate::medium::Medium::new(key_store.clone(), wifi_stats, debug);

        (key_store, medium, bytes::Bytes::from_static(&UNKNOWN_STATION_ETHERNET_FRAME))
    }
}

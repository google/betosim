// Copyright 2023 The Android Open Source Project
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

use bytes::Bytes;
use futures::{StreamExt, channel::mpsc::UnboundedSender, sink::SinkExt};
use pica::{Handle, Pica};

use netsim_proto::model::Chip as ProtoChip;
use netsim_proto::model::chip::Radio as ProtoRadio;
use netsim_proto::stats::{NetsimRadioStats as ProtoRadioStats, netsim_radio_stats};

use crate::devices::chip::ChipIdentifier;
use crate::get_runtime;
use crate::uwb::ranging_estimator::{SharedState, UwbRangingEstimator};
use crate::wireless::packet::handle_response;

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use super::{WirelessChip, WirelessChipImpl};

// TODO(b/331267949): Construct Manager struct for each wireless_chip module
static PICA_HANDLE_TO_STATE: OnceLock<SharedState> = OnceLock::new();

fn get_pica_handle_to_state() -> &'static SharedState {
    PICA_HANDLE_TO_STATE.get_or_init(SharedState::new)
}

static PICA: OnceLock<Arc<Mutex<Pica>>> = OnceLock::new();

fn get_pica() -> Arc<Mutex<Pica>> {
    PICA.get_or_init(|| {
        Arc::new(Mutex::new(Pica::new(
            Box::new(UwbRangingEstimator::new(get_pica_handle_to_state().clone())),
            None,
        )))
    })
    .clone()
}

/// Parameters for creating UWB chips
pub struct CreateParams {
    #[allow(dead_code)]
    pub address: String,
}

/// UWB struct will keep track of pica_id
pub struct Uwb {
    pica_id: Handle,
    uci_stream_writer: UnboundedSender<Vec<u8>>,
    state: AtomicBool,
    tx_count: AtomicI32,
    rx_count: Arc<AtomicI32>,
}

impl Drop for Uwb {
    fn drop(&mut self) {
        get_pica_handle_to_state().remove(&self.pica_id);
    }
}

impl WirelessChip for Uwb {
    fn handle_request(&self, packet: &Bytes) {
        self.uci_stream_writer
            .unbounded_send(packet.clone().into())
            .expect("UciStream Receiver Disconnected");
        let _ = self.tx_count.fetch_add(1, Ordering::SeqCst);
    }

    fn reset(&self) {
        self.state.store(true, Ordering::SeqCst);
        self.tx_count.store(0, Ordering::SeqCst);
        self.rx_count.store(0, Ordering::SeqCst);
    }

    fn get(&self) -> ProtoChip {
        let mut chip_proto = ProtoChip::new();
        let uwb_proto = ProtoRadio {
            state: self.state.load(Ordering::SeqCst).into(),
            tx_count: self.tx_count.load(Ordering::SeqCst),
            rx_count: self.rx_count.load(Ordering::SeqCst),
            ..Default::default()
        };
        chip_proto.mut_uwb().clone_from(&uwb_proto);
        chip_proto
    }

    fn patch(&self, chip: &ProtoChip) {
        if !chip.has_uwb() {
            return;
        }
        if let Some(patch_state) = chip.uwb().state {
            self.state.store(patch_state, Ordering::SeqCst);
        }
    }

    fn get_stats(&self, duration_secs: u64) -> Vec<ProtoRadioStats> {
        let mut stats_proto = ProtoRadioStats::new();
        stats_proto.set_duration_secs(duration_secs);
        stats_proto.set_kind(netsim_radio_stats::Kind::UWB);
        let chip_proto = self.get();
        if chip_proto.has_uwb() {
            stats_proto.set_tx_count(chip_proto.uwb().tx_count);
            stats_proto.set_rx_count(chip_proto.uwb().rx_count);
        }
        vec![stats_proto]
    }
}

pub fn uwb_start() {
    // TODO: Provide TcpStream as UWB connector
    let _ = thread::Builder::new().name("pica_service".to_string()).spawn(move || {
        log::info!("PICA STARTED");
        let _guard = get_runtime().enter();
        futures::executor::block_on(pica::run(&get_pica()))
    });
}

pub fn add_chip(_create_params: &CreateParams, chip_id: ChipIdentifier) -> WirelessChipImpl {
    let (uci_stream_sender, uci_stream_receiver) = futures::channel::mpsc::unbounded();
    let (uci_sink_sender, uci_sink_receiver) = futures::channel::mpsc::unbounded();
    let _guard = get_runtime().enter();
    let pica_id = get_pica()
        .lock()
        .unwrap()
        .add_device(Box::pin(uci_stream_receiver), Box::pin(uci_sink_sender.sink_err_into()))
        .unwrap();
    get_pica_handle_to_state().insert(pica_id, chip_id);

    let rx_count = Arc::new(AtomicI32::new(0));
    let uwb = Uwb {
        pica_id,
        uci_stream_writer: uci_stream_sender,
        state: AtomicBool::new(true),
        tx_count: AtomicI32::new(0),
        rx_count: rx_count.clone(),
    };

    // Spawn a future for obtaining packet from pica and invoking handle_response_rust
    get_runtime().spawn(async move {
        let mut uci_sink_receiver = uci_sink_receiver;
        while let Some(packet) = uci_sink_receiver.next().await {
            handle_response(chip_id, &Bytes::from(packet));
            rx_count.fetch_add(1, Ordering::SeqCst);
        }
    });
    Box::new(uwb)
}

#[cfg(test)]
mod tests {

    use super::*;

    fn add_uwb_wireless_chip() -> WirelessChipImpl {
        add_chip(&CreateParams { address: "test".to_string() }, ChipIdentifier(0))
    }

    fn patch_chip_proto() -> ProtoChip {
        let mut chip_proto = ProtoChip::new();
        let uwb_proto = ProtoRadio { state: false.into(), ..Default::default() };
        chip_proto.mut_uwb().clone_from(&uwb_proto);
        chip_proto
    }

    #[test]
    fn test_uwb_get() {
        let wireless_chip = add_uwb_wireless_chip();
        assert!(wireless_chip.get().has_uwb());
    }

    #[test]
    fn test_uwb_patch_and_reset() {
        let wireless_chip = add_uwb_wireless_chip();
        wireless_chip.patch(&patch_chip_proto());
        let binding = wireless_chip.get();
        let radio = binding.uwb();
        assert_eq!(radio.state, Some(false));
        wireless_chip.reset();
        let binding = wireless_chip.get();
        let radio = binding.uwb();
        assert_eq!(radio.rx_count, 0);
        assert_eq!(radio.tx_count, 0);
        assert_eq!(radio.state, Some(true));
    }

    #[test]
    fn test_get_stats() {
        let wireless_chip = add_uwb_wireless_chip();
        let radio_stat_vec = wireless_chip.get_stats(0);
        let radio_stat = radio_stat_vec.first().unwrap();
        assert_eq!(radio_stat.kind(), netsim_radio_stats::Kind::UWB);
        assert_eq!(radio_stat.duration_secs(), 0);
    }
}

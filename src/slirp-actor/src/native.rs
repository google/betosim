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

use bytes::Bytes;
use slirp_host::TokioHost;
use tokio::sync::mpsc;
use tracing::{error, info};

pub(crate) async fn run_native_slirp_loop(
    config: slirp::Config,
    downlink_tx: mpsc::UnboundedSender<Bytes>,
    mut uplink_rx: mpsc::UnboundedReceiver<Bytes>,
) {
    info!("Starting Rust native slirp engine loop with TokioHost");

    const CHANNEL_CAPACITY: usize = 10000; // Large buffer to absorb traffic bursts

    let (slirp_request_sender, mut slirp_request_receiver) =
        mpsc::channel::<slirp::SlirpRequest>(CHANNEL_CAPACITY);
    let (slirp_response_sender, slirp_response_receiver) =
        mpsc::channel::<slirp::SlirpResponse>(CHANNEL_CAPACITY);
    let (guest_packet_sender, mut guest_packet_receiver) = mpsc::channel::<Bytes>(CHANNEL_CAPACITY);

    let mut slirp_engine = slirp::Slirp::new(config.clone());

    let slirp_task = tokio::spawn(async move {
        info!("slirp task started");
        while let Some(request) = slirp_request_receiver.recv().await {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                slirp_engine.handle_request(request)
            }));

            match result {
                Ok(responses) => {
                    for response in responses {
                        if slirp_response_sender.send(response).await.is_err() {
                            error!("slirp_response_sender closed, exiting slirp task");
                            return;
                        }
                    }
                }
                Err(_) => {
                    error!("PANIC in slirp task, exiting task");
                    return;
                }
            }
        }
        info!("slirp task finished");
    });

    let mut host = TokioHost::from_channels(
        slirp_request_sender.clone(),
        slirp_response_receiver,
        guest_packet_sender,
        None,
        config,
    );

    let host_task = tokio::spawn(async move {
        host.run().await;
    });

    let downlink_task = tokio::spawn(async move {
        while let Some(packet) = guest_packet_receiver.recv().await {
            if downlink_tx.send(packet).is_err() {
                break;
            }
        }
    });

    while let Some(msg) = uplink_rx.recv().await {
        if slirp_request_sender.send(slirp::SlirpRequest::Packet(msg)).await.is_err() {
            break;
        }
    }

    downlink_task.abort();
    host_task.abort();
    slirp_task.abort();
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bytes::Bytes;
    use tokio::sync::mpsc;

    use super::*;

    #[tokio::test]
    async fn test_native_slirp_loop_lifecycle() {
        let (downlink_tx, _downlink_rx) = mpsc::unbounded_channel::<Bytes>();
        let (uplink_tx, uplink_rx) = mpsc::unbounded_channel::<Bytes>();

        let loop_handle =
            tokio::spawn(run_native_slirp_loop(slirp::Config::default(), downlink_tx, uplink_rx));

        uplink_tx.send(Bytes::from_static(&[0u8; 64])).unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        drop(uplink_tx);
        let result = tokio::time::timeout(Duration::from_secs(2), loop_handle).await;
        assert!(result.is_ok());
    }
}

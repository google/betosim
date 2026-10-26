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

//! Decoupled Integration Tests: Backend Lifecycle & Initialization
//! (b/560172253).

use std::time::Duration;

use actor_framework::{ActorService, ResourceActor};
use slirp_actor::{SlirpActor, SlirpBackend, SlirpReq};
use tokio::sync::mpsc;

use super::test_driver::{MockContext, SlirpTestDriver};

async fn run_actor_initialization_test(backend: SlirpBackend) {
    // Given a Slirp driver initialized with the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When we query driver status
    let initialized = driver.is_initialized().await;

    // Then driver is ready
    assert!(initialized);
}

#[tokio::test]
async fn test_actor_initialization_parity() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_actor_initialization_test(backend).await;
    }
}

#[tokio::test]
async fn test_dynamic_backend_switch() {
    // Given a SlirpActor running default backend
    let mut actor = SlirpActor::new(Default::default(), None, None).await;
    let mut ctx = MockContext::default();

    // When we switch backend to Native
    let result =
        actor.handle_action(None, SlirpReq::SwitchBackend(SlirpBackend::Native), &mut ctx).await;

    // Then backend transition succeeds
    assert!(result.is_ok());
    assert_eq!(actor.backend(), SlirpBackend::Native);
}

#[tokio::test]
async fn test_native_backend_survives_client_id_1_unregister_on_actor_runtime() {
    let (runner, client) = ResourceActor::new(32);
    let actor =
        SlirpActor::new_with_backend(Default::default(), None, None, SlirpBackend::Native).await;
    tokio::spawn(runner.run(actor));

    let make_register = |client_id: u32| {
        let (tx_out, rx_out) = mpsc::unbounded_channel::<bytes::Bytes>();
        let (stream_tx, stream_rx) = mpsc::unbounded_channel::<bytes::Bytes>();
        let stream = Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(stream_rx));
        let sink: netsim_model::PacketSink =
            Box::pin(futures::sink::unfold(tx_out, |tx, bytes| async move {
                let _ = tx.send(bytes);
                Ok(tx)
            }));
        (
            SlirpReq::Register { client_id, stream, sink, notifier: None, isolated: false },
            stream_tx,
            rx_out,
        )
    };

    let (req1, _stream_tx_1, mut rx_out_1) = make_register(1);
    client.perform_action(None, req1).await.unwrap();

    let (req_peer, _stream_tx_peer, _rx_out_peer) = make_register(u32::MAX - 1);
    client.perform_action(None, req_peer).await.unwrap();

    let status = client.get(0).await.unwrap().unwrap();
    assert!(status.initialized);

    client.perform_action(None, SlirpReq::Unregister { client_id: 1 }).await.unwrap();

    while tokio::time::timeout(Duration::from_secs(1), rx_out_1.recv()).await.unwrap().is_some() {}
    tokio::time::sleep(Duration::from_millis(20)).await;

    let status = client.get(0).await.unwrap().unwrap();
    assert!(status.initialized);
    assert!(
        client
            .perform_action(
                None,
                SlirpReq::SendPacket(bytes::Bytes::from_static(b"packet_after_unreg"))
            )
            .await
            .is_ok()
    );

    client.perform_action(None, SlirpReq::SwitchBackend(SlirpBackend::CFfi)).await.unwrap();
    client.perform_action(None, SlirpReq::SwitchBackend(SlirpBackend::Native)).await.unwrap();

    let (req2, _stream_tx_2, mut rx_out_2) = make_register(2);
    client.perform_action(None, req2).await.unwrap();
    client.perform_action(None, SlirpReq::Unregister { client_id: 2 }).await.unwrap();

    while tokio::time::timeout(Duration::from_secs(1), rx_out_2.recv()).await.unwrap().is_some() {}
    tokio::time::sleep(Duration::from_millis(20)).await;

    let status = client.get(0).await.unwrap().unwrap();
    assert!(status.initialized);
    assert!(
        client
            .perform_action(
                None,
                SlirpReq::SendPacket(bytes::Bytes::from_static(b"packet_after_switch_unreg"))
            )
            .await
            .is_ok()
    );

    client.shutdown().await.unwrap();
}

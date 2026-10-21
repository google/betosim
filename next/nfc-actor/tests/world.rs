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

//! BDD World test harness for `nfc-actor` integration tests.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use actor_framework::{ActorClient, ActorLifecycle, ActorService, Context};
use bytes::Bytes;
use device_actor::DeviceActor;
use futures::{StreamExt, future::BoxFuture};
use netsim_model::{ChipCreate, ChipId, DeviceId, Position};
use nfc_actor::NfcActor;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

// --- Mock Helpers ---

#[derive(Clone, Default)]
struct MockDeviceActorClient {
    positions: Arc<Mutex<HashMap<DeviceId, Position>>>,
}

impl MockDeviceActorClient {
    fn set_position(&self, id: DeviceId, pos: Position) {
        self.positions.lock().unwrap().insert(id, pos);
    }
}

#[async_trait::async_trait]
impl ActorClient<DeviceActor> for MockDeviceActorClient {
    async fn create(
        &self,
        _params: <DeviceActor as ActorService>::Create,
    ) -> Result<
        <DeviceActor as ActorService>::Id,
        actor_framework::FrameworkError<<DeviceActor as ActorService>::Error>,
    > {
        panic!("not implemented");
    }
    async fn create_with_id(
        &self,
        _id: <DeviceActor as ActorService>::Id,
        _params: <DeviceActor as ActorService>::Create,
    ) -> Result<
        <DeviceActor as ActorService>::Id,
        actor_framework::FrameworkError<<DeviceActor as ActorService>::Error>,
    > {
        panic!("not implemented");
    }
    async fn get(
        &self,
        id: <DeviceActor as ActorService>::Id,
    ) -> Result<
        Option<<DeviceActor as ActorService>::Entity>,
        actor_framework::FrameworkError<<DeviceActor as ActorService>::Error>,
    > {
        let pos = self.positions.lock().unwrap().get(&id).copied().unwrap_or_default();
        Ok(Some(netsim_model::Device {
            id: id.0,
            name: format!("device-{}", id.0),
            pose: netsim_model::Pose { position: pos, ..Default::default() },
            ..Default::default()
        }))
    }
    async fn update(
        &self,
        _id: <DeviceActor as ActorService>::Id,
        _update: <DeviceActor as ActorService>::Update,
    ) -> Result<
        <DeviceActor as ActorService>::Entity,
        actor_framework::FrameworkError<<DeviceActor as ActorService>::Error>,
    > {
        panic!("not implemented");
    }
    async fn delete(
        &self,
        _id: <DeviceActor as ActorService>::Id,
    ) -> Result<(), actor_framework::FrameworkError<<DeviceActor as ActorService>::Error>> {
        panic!("not implemented");
    }
    async fn perform_action(
        &self,
        _id: Option<<DeviceActor as ActorService>::Id>,
        _action: <DeviceActor as ActorService>::Action,
    ) -> Result<
        <DeviceActor as ActorService>::ActionResult,
        actor_framework::FrameworkError<<DeviceActor as ActorService>::Error>,
    > {
        Ok(device_api::DeviceActionResult::Success)
    }
    async fn list(
        &self,
    ) -> Result<
        Vec<<DeviceActor as ActorService>::Entity>,
        actor_framework::FrameworkError<<DeviceActor as ActorService>::Error>,
    > {
        panic!("not implemented");
    }
    async fn shutdown(
        &self,
    ) -> Result<(), actor_framework::FrameworkError<<DeviceActor as ActorService>::Error>> {
        panic!("not implemented");
    }
    fn clone_box(&self) -> Box<dyn ActorClient<DeviceActor>> {
        Box::new(self.clone())
    }
}

#[derive(Default)]
pub(crate) struct TestContext {
    tasks: HashMap<ChipId, tokio::task::JoinHandle<ChipId>>,
    stream_tasks: HashMap<ChipId, tokio::task::JoinHandle<()>>,
    aborted_tasks: Vec<ChipId>,
    removed_streams: Vec<ChipId>,
}

impl Context<NfcActor> for TestContext {
    fn set_interval(&mut self, _duration: std::time::Duration) {}
    fn add_stream(&mut self, id: ChipId, mut stream: actor_framework::BoxStream) {
        let handle = tokio::spawn(async move { while stream.next().await.is_some() {} });
        self.stream_tasks.insert(id, handle);
    }
    fn remove_stream(&mut self, id: ChipId) {
        self.removed_streams.push(id);
        if let Some(handle) = self.stream_tasks.remove(&id) {
            handle.abort();
        }
    }
    fn add_typed_stream(&mut self, _id: usize, _stream: actor_framework::BoxTypedStream<()>) {}
    fn remove_typed_stream(&mut self, _id: usize) {}
    fn spawn(&mut self, id: ChipId, task: BoxFuture<'static, ChipId>) {
        let handle = tokio::spawn(task);
        self.tasks.insert(id, handle);
    }
    fn abort(&mut self, id: ChipId) {
        self.aborted_tasks.push(id);
        if let Some(handle) = self.tasks.remove(&id) {
            handle.abort();
        }
    }
    fn shutdown(&mut self) {}
    fn run_later(
        &mut self,
        _duration: std::time::Duration,
        _f: actor_framework::TimerCallback<NfcActor>,
    ) -> actor_framework::TimerKey {
        panic!("not implemented");
    }
    fn cancel_timer(&mut self, _key: actor_framework::TimerKey) {}
}

struct MockSink(mpsc::Sender<Bytes>);
impl futures::Sink<Bytes> for MockSink {
    type Error = std::io::Error;
    fn poll_ready(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn start_send(self: std::pin::Pin<&mut Self>, item: Bytes) -> Result<(), Self::Error> {
        let _ = self.0.try_send(item);
        Ok(())
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
}

// --- BDD World ---

pub(crate) struct NfcWorld {
    pub(crate) actor: NfcActor,
    pub(crate) ctx: TestContext,
    pub(crate) packet_receivers: HashMap<ChipId, mpsc::Receiver<Bytes>>,
    pub(crate) packet_senders: HashMap<ChipId, mpsc::Sender<Bytes>>,
    mock_device_client: MockDeviceActorClient,
}

impl Drop for NfcWorld {
    fn drop(&mut self) {
        if let Some(task) = self.actor.scene_task.take() {
            task.abort();
        }
        for (_, handle) in self.ctx.tasks.drain() {
            handle.abort();
        }
        for (_, handle) in self.ctx.stream_tasks.drain() {
            handle.abort();
        }
    }
}

impl NfcWorld {
    pub(crate) async fn new() -> Self {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let mock_device_client = MockDeviceActorClient::default();
        let device_client = device_actor::DeviceClient::new(Box::new(mock_device_client.clone()));
        let mut actor = NfcActor::new(device_client, Arc::new(nfc_actor::NfcStats::new()));
        actor.start_casimir();

        Self {
            actor,
            ctx: TestContext::default(),
            packet_receivers: HashMap::new(),
            packet_senders: HashMap::new(),
            mock_device_client,
        }
    }

    pub(crate) fn set_device_position(&self, id: DeviceId, x: f32, y: f32, z: f32) {
        self.mock_device_client.set_position(id, Position { x, y, z });
    }

    pub(crate) async fn given_a_chip(&mut self, id_val: u32) {
        let chip_id = ChipId(id_val);
        let device_id = DeviceId(id_val);

        let (tx_stream, rx_stream) = mpsc::channel(10);
        let (tx_sink, rx_sink) = mpsc::channel(10);
        let packet_stream = ReceiverStream::new(rx_stream);
        let packet_sink = MockSink(tx_sink);

        let params = ChipCreate {
            packet_stream: Some(Box::new(packet_stream)),
            packet_sink: Some(Box::pin(packet_sink)),
            chip: netsim_model::Chip { device_id, ..Default::default() },
        };

        let create_res = self.actor.handle_create(Some(chip_id), params, &mut self.ctx).await;
        assert!(create_res.is_ok());
        assert_eq!(create_res.unwrap(), chip_id);
        assert!(
            self.actor
                .active_chips
                .get(&chip_id)
                .unwrap()
                .enabled
                .load(std::sync::atomic::Ordering::Relaxed),
            "Chip must be enabled by default upon boot across all platforms during transition to Netsim"
        );
        self.packet_receivers.insert(chip_id, rx_sink);
        self.packet_senders.insert(chip_id, tx_stream);
    }

    pub(crate) async fn when_casimir_disconnects(&mut self, id_val: u32) {
        let chip_id = ChipId(id_val);
        let casimir_device_id = self.actor.active_chips.get(&chip_id).unwrap().casimir_device_id;

        let scene_client = self.actor.scene_client.as_ref().unwrap().clone();
        let remove_res = scene_client.remove_device(casimir_device_id).await;
        assert!(remove_res.is_ok());

        if let Some(handle) = self.ctx.tasks.remove(&chip_id) {
            let res = handle.await;
            assert!(res.is_ok());
            assert_eq!(res.unwrap(), chip_id);

            self.actor.on_task_closed(chip_id, &mut self.ctx).await;
        } else {
            panic!("Task handle NOT found for chip {}", id_val);
        }
    }

    pub(crate) async fn then_chip_is_cleaned_up(&mut self, id_val: u32) {
        let chip_id = ChipId(id_val);
        assert!(!self.actor.active_chips.contains_key(&chip_id), "Chip still active in NfcActor");
        assert!(self.ctx.aborted_tasks.contains(&chip_id), "Task was not aborted");
        assert!(self.ctx.removed_streams.contains(&chip_id), "Stream was not removed");
        assert!(
            !self.actor.casimir_to_device.lock().values().any(|&dev_id| dev_id.0 == id_val),
            "Casimir mapping was not cleaned up!"
        );
    }
}

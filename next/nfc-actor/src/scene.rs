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

use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

/// Commands that can be sent to the Netsim-wrapped Casimir Scene.
pub(crate) enum SceneCommand {
    AddDevice {
        builder: Box<
            dyn FnOnce(
                    casimir::Id,
                    tokio::sync::mpsc::UnboundedSender<casimir::packets::rf::RfPacket>,
                ) -> casimir::Device
                + Send
                + 'static,
        >,
        resp: tokio::sync::oneshot::Sender<anyhow::Result<casimir::Id>>,
    },
    RemoveDevice {
        id: casimir::Id,
        resp: tokio::sync::oneshot::Sender<anyhow::Result<()>>,
    },
}

/// Client to interact with the Netsim-wrapped Scene.
#[derive(Clone)]
pub struct SceneClient {
    cmd_tx: tokio::sync::mpsc::UnboundedSender<SceneCommand>,
}

impl SceneClient {
    pub(crate) fn new(cmd_tx: tokio::sync::mpsc::UnboundedSender<SceneCommand>) -> Self {
        Self { cmd_tx }
    }

    pub async fn add_device(
        &self,
        builder: impl FnOnce(
            casimir::Id,
            tokio::sync::mpsc::UnboundedSender<casimir::packets::rf::RfPacket>,
        ) -> casimir::Device
        + Send
        + 'static,
    ) -> anyhow::Result<casimir::Id> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.cmd_tx
            .send(SceneCommand::AddDevice { builder: Box::new(builder), resp: tx })
            .map_err(|err| anyhow::anyhow!("failed to send AddDevice command: {err}"))?;
        rx.await?
    }

    pub async fn remove_device(&self, id: casimir::Id) -> anyhow::Result<()> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.cmd_tx
            .send(SceneCommand::RemoveDevice { id, resp: tx })
            .map_err(|err| anyhow::anyhow!("failed to send RemoveDevice command: {err}"))?;
        rx.await?
    }
}

/// Netsim wrapper around Casimir's Scene to handle concurrency and dynamic
/// device addition.
pub(crate) struct NetsimScene {
    scene: casimir::Scene,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<SceneCommand>,
}

impl NetsimScene {
    pub(crate) fn new(cmd_rx: tokio::sync::mpsc::UnboundedReceiver<SceneCommand>) -> Self {
        Self { scene: casimir::Scene::new(), cmd_rx }
    }
}

impl Future for NetsimScene {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();

        // 1. Process Netsim-specific commands (updating the Casimir Scene)
        while let Poll::Ready(Some(cmd)) = this.cmd_rx.poll_recv(cx) {
            match cmd {
                SceneCommand::AddDevice { builder, resp } => {
                    let res = this.scene.add_device(builder);
                    let _ = resp.send(res);
                }
                SceneCommand::RemoveDevice { id, resp } => {
                    if this.scene.devices.get(id as usize).is_some_and(Option::is_some) {
                        this.scene.disconnect(id as usize);
                    }
                    let _ = resp.send(Ok(()));
                }
            }
        }

        // 2. Poll the underlying Casimir Scene to run the simulation
        Pin::new(&mut this.scene).poll(cx)
    }
}

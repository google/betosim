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

use std::{collections::HashMap, time::Duration};

use actor_framework::{
    ActorLifecycle, ActorService, DynContext, FrameworkError, ResourceActor, ResourceClient,
    TimerKey,
    utils::{handle_get_default, handle_list_default},
};
use tokio::{sync::mpsc, task::JoinHandle, time::timeout};
use tokio_stream::wrappers::ReceiverStream;
use tracing::{field::Visit, instrument::WithSubscriber};

const EVENT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Default)]
struct CapturedWarning {
    actor: Option<String>,
    event: Option<String>,
    elapsed_ms: Option<u64>,
    threshold_ms: Option<u64>,
}

impl Visit for CapturedWarning {
    fn record_debug(&mut self, _field: &tracing::field::Field, _value: &dyn std::fmt::Debug) {}

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        match field.name() {
            "actor" => self.actor = Some(value.to_string()),
            "event" => self.event = Some(value.to_string()),
            _ => {}
        }
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        match field.name() {
            "elapsed_ms" => self.elapsed_ms = Some(value),
            "threshold_ms" => self.threshold_ms = Some(value),
            _ => {}
        }
    }
}

struct WarningCaptureSubscriber {
    tx: mpsc::UnboundedSender<CapturedWarning>,
}

impl tracing::Subscriber for WarningCaptureSubscriber {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        *metadata.level() <= tracing::Level::WARN
    }

    fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        if *event.metadata().level() == tracing::Level::WARN {
            let mut warning = CapturedWarning::default();
            event.record(&mut warning);
            let _ = self.tx.send(warning);
        }
    }

    fn enter(&self, _span: &tracing::span::Id) {}
    fn exit(&self, _span: &tracing::span::Id) {}
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct User {
    id: u32,
    name: String,
    promoted: bool,
}

#[derive(Debug)]
struct UserCreate {
    name: String,
}

#[derive(Debug)]
struct UserUpdate {
    name: Option<String>,
}

#[derive(Debug)]
enum UserAction {
    Rename(String),
    Promote,
    ResetAll,
    AttachTypedStream { stream_id: usize, rx: mpsc::Receiver<String> },
    AttachByteStream { stream_id: u32, rx: mpsc::Receiver<bytes::Bytes>, handler_delay: Duration },
    SimulateSlowAction(Duration),
    SpawnCompletingTask(u32),
    SpawnPendingTask(u32),
    AbortTask(u32),
    ScheduleTimer { timer_id: u32, delay: Duration },
    CancelTimer(u32),
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
enum UserError {
    #[error("user {0} not found")]
    NotFound(u32),
    #[error("entity id required for action")]
    MissingId,
}

#[derive(Debug, PartialEq, Eq)]
enum ActorEvent {
    TypedStreamItem { stream_id: usize, item: String },
    TypedStreamClosed(usize),
    ByteStreamItem { stream_id: u32, len: usize },
    TaskClosed(u32),
    TimerFired(u32),
    Shutdown,
}

struct UserActor {
    users: HashMap<u32, User>,
    next_id: u32,
    timers: HashMap<u32, TimerKey>,
    byte_stream_delays: HashMap<u32, Duration>,
    event_tx: mpsc::UnboundedSender<ActorEvent>,
}

impl UserActor {
    fn new(event_tx: mpsc::UnboundedSender<ActorEvent>) -> Self {
        Self {
            users: HashMap::new(),
            next_id: 0,
            timers: HashMap::new(),
            byte_stream_delays: HashMap::new(),
            event_tx,
        }
    }
}

impl ActorLifecycle for UserActor {
    async fn on_start(&mut self, _ctx: &mut DynContext<Self>) {
        self.next_id = 1;
    }

    async fn on_stream(&mut self, id: u32, msg: bytes::Bytes, _ctx: &mut DynContext<Self>) {
        if let Some(&delay) = self.byte_stream_delays.get(&id).filter(|d| !d.is_zero()) {
            tokio::time::sleep(delay).await;
        }
        let _ = self.event_tx.send(ActorEvent::ByteStreamItem { stream_id: id, len: msg.len() });
    }

    async fn on_typed_stream(
        &mut self,
        id: usize,
        item: Self::TypedStream,
        _ctx: &mut DynContext<Self>,
    ) {
        let _ = self.event_tx.send(ActorEvent::TypedStreamItem { stream_id: id, item });
    }

    async fn on_typed_stream_closed(&mut self, id: usize, _ctx: &mut DynContext<Self>) {
        let _ = self.event_tx.send(ActorEvent::TypedStreamClosed(id));
    }

    async fn on_task_closed(&mut self, id: Self::Id, _ctx: &mut DynContext<Self>) {
        let _ = self.event_tx.send(ActorEvent::TaskClosed(id));
    }

    async fn on_shutdown(&mut self) {
        let _ = self.event_tx.send(ActorEvent::Shutdown);
    }
}

impl ActorService for UserActor {
    type Id = u32;
    type Create = UserCreate;
    type Update = UserUpdate;
    type Action = UserAction;
    type ActionResult = usize;
    type Error = UserError;
    type Entity = User;
    type TypedStream = String;

    async fn handle_create(
        &mut self,
        id: Option<Self::Id>,
        params: Self::Create,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Self::Id, Self::Error> {
        let assigned_id = id.unwrap_or_else(|| {
            let current = self.next_id;
            self.next_id += 1;
            current
        });
        if assigned_id >= self.next_id {
            self.next_id = assigned_id + 1;
        }
        self.users
            .insert(assigned_id, User { id: assigned_id, name: params.name, promoted: false });
        Ok(assigned_id)
    }

    async fn handle_get(
        &self,
        id: Self::Id,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Option<Self::Entity>, Self::Error> {
        Ok(handle_get_default(&self.users, &id))
    }

    async fn handle_update(
        &mut self,
        id: Self::Id,
        update: Self::Update,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Self::Entity, Self::Error> {
        let user = self.users.get_mut(&id).ok_or(UserError::NotFound(id))?;
        if let Some(name) = update.name {
            user.name = name;
        }
        Ok(user.clone())
    }

    async fn handle_delete(
        &mut self,
        id: Self::Id,
        _ctx: &mut DynContext<Self>,
    ) -> Result<(), Self::Error> {
        self.users.remove(&id);
        Ok(())
    }

    async fn handle_action(
        &mut self,
        id: Option<Self::Id>,
        action: Self::Action,
        ctx: &mut DynContext<Self>,
    ) -> Result<Self::ActionResult, Self::Error> {
        match action {
            UserAction::Rename(name) => {
                let target_id = id.ok_or(UserError::MissingId)?;
                let user = self.users.get_mut(&target_id).ok_or(UserError::NotFound(target_id))?;
                user.name = name;
                Ok(1)
            }
            UserAction::Promote => {
                let target_id = id.ok_or(UserError::MissingId)?;
                let user = self.users.get_mut(&target_id).ok_or(UserError::NotFound(target_id))?;
                user.promoted = true;
                Ok(1)
            }
            UserAction::ResetAll => {
                let removed = self.users.len();
                self.users.clear();
                Ok(removed)
            }
            UserAction::AttachTypedStream { stream_id, rx } => {
                ctx.add_typed_stream(stream_id, Box::pin(ReceiverStream::new(rx)));
                Ok(stream_id)
            }
            UserAction::AttachByteStream { stream_id, rx, handler_delay } => {
                self.byte_stream_delays.insert(stream_id, handler_delay);
                ctx.add_stream(stream_id, Box::pin(ReceiverStream::new(rx)));
                Ok(stream_id as usize)
            }
            UserAction::SimulateSlowAction(duration) => {
                tokio::time::sleep(duration).await;
                Ok(1)
            }
            UserAction::SpawnCompletingTask(task_id) => {
                ctx.spawn(task_id, Box::pin(async move { task_id }));
                Ok(task_id as usize)
            }
            UserAction::SpawnPendingTask(task_id) => {
                ctx.spawn(
                    task_id,
                    Box::pin(async move {
                        std::future::pending::<()>().await;
                        task_id
                    }),
                );
                Ok(task_id as usize)
            }
            UserAction::AbortTask(task_id) => {
                ctx.abort(task_id);
                Ok(task_id as usize)
            }
            UserAction::ScheduleTimer { timer_id, delay } => {
                let key = ctx.run_later(
                    delay,
                    Box::new(move |actor: &mut Self, _ctx| {
                        let _ = actor.event_tx.send(ActorEvent::TimerFired(timer_id));
                    }),
                );
                self.timers.insert(timer_id, key);
                Ok(timer_id as usize)
            }
            UserAction::CancelTimer(timer_id) => {
                if let Some(key) = self.timers.remove(&timer_id) {
                    ctx.cancel_timer(key);
                }
                Ok(timer_id as usize)
            }
        }
    }

    async fn handle_list(
        &mut self,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Vec<Self::Entity>, Self::Error> {
        Ok(handle_list_default(&self.users))
    }
}

pub(crate) struct TestWorld {
    client: ResourceClient<UserActor>,
    actor_handle: JoinHandle<()>,
    event_rx: mpsc::UnboundedReceiver<ActorEvent>,
    stream_senders: HashMap<usize, mpsc::Sender<String>>,
    byte_stream_senders: HashMap<u32, mpsc::Sender<bytes::Bytes>>,
    warn_rx: mpsc::UnboundedReceiver<CapturedWarning>,
    last_created_id: Option<u32>,
    last_action_result: Option<usize>,
    last_error: Option<FrameworkError<UserError>>,
}

impl Drop for TestWorld {
    fn drop(&mut self) {
        self.actor_handle.abort();
    }
}

impl TestWorld {
    pub(crate) fn new() -> Self {
        let (actor, client) = ResourceActor::new(16);
        Self::from_actor(actor, client)
    }

    pub(crate) fn with_stall_threshold(stall_threshold: Duration) -> Self {
        let (actor, client) = ResourceActor::new(16);
        Self::from_actor(actor.with_stall_threshold(stall_threshold), client)
    }

    fn from_actor(actor: ResourceActor<UserActor>, client: ResourceClient<UserActor>) -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (warn_tx, warn_rx) = mpsc::unbounded_channel();
        let subscriber = WarningCaptureSubscriber { tx: warn_tx };
        let dispatch = tracing::Dispatch::new(subscriber);
        let actor_task = actor.run(UserActor::new(event_tx)).with_subscriber(dispatch);
        let actor_handle = tokio::spawn(actor_task);
        Self {
            client,
            actor_handle,
            event_rx,
            stream_senders: HashMap::new(),
            byte_stream_senders: HashMap::new(),
            warn_rx,
            last_created_id: None,
            last_action_result: None,
            last_error: None,
        }
    }

    pub(crate) async fn given_user(&mut self, name: &str) -> u32 {
        let id = self
            .client
            .create(UserCreate { name: name.to_string() })
            .await
            .expect("given_user create should succeed");
        self.last_created_id = Some(id);
        id
    }

    pub(crate) async fn when_create_user(&mut self, name: &str) -> u32 {
        match self.client.create(UserCreate { name: name.to_string() }).await {
            Ok(id) => {
                self.last_created_id = Some(id);
                self.last_error = None;
                id
            }
            Err(err) => {
                self.last_error = Some(err);
                0
            }
        }
    }

    pub(crate) async fn when_create_user_with_id(&mut self, id: u32, name: &str) -> &mut Self {
        match self.client.create_with_id(id, UserCreate { name: name.to_string() }).await {
            Ok(created_id) => {
                self.last_created_id = Some(created_id);
                self.last_error = None;
            }
            Err(err) => {
                self.last_error = Some(err);
            }
        }
        self
    }

    pub(crate) async fn when_update_user_name(&mut self, id: u32, new_name: &str) -> &mut Self {
        self.last_error =
            self.client.update(id, UserUpdate { name: Some(new_name.to_string()) }).await.err();
        self
    }

    pub(crate) async fn when_delete_user(&mut self, id: u32) -> &mut Self {
        self.last_error = self.client.delete(id).await.err();
        self
    }

    pub(crate) async fn when_rename_user(&mut self, id: u32, new_name: &str) -> &mut Self {
        self.run_action(Some(id), UserAction::Rename(new_name.to_string())).await
    }

    pub(crate) async fn when_promote_user(&mut self, id: u32) -> &mut Self {
        self.run_action(Some(id), UserAction::Promote).await
    }

    pub(crate) async fn when_reset_all_users(&mut self) -> &mut Self {
        self.run_action(None, UserAction::ResetAll).await
    }

    pub(crate) async fn when_run_slow_action(&mut self, duration: Duration) -> &mut Self {
        self.run_action(None, UserAction::SimulateSlowAction(duration)).await
    }

    pub(crate) async fn given_attached_byte_stream(
        &mut self,
        stream_id: u32,
        handler_delay: Duration,
    ) -> &mut Self {
        let (tx, rx) = mpsc::channel(16);
        self.byte_stream_senders.insert(stream_id, tx);
        self.run_action(None, UserAction::AttachByteStream { stream_id, rx, handler_delay }).await;
        assert!(self.last_error.is_none(), "attaching byte stream should succeed");
        self
    }

    pub(crate) async fn when_send_byte_stream_item(
        &mut self,
        stream_id: u32,
        payload: &[u8],
    ) -> &mut Self {
        let sender = self
            .byte_stream_senders
            .get(&stream_id)
            .expect("byte stream sender must be registered");
        sender
            .send(bytes::Bytes::copy_from_slice(payload))
            .await
            .expect("sending item to byte stream should succeed");
        self
    }

    pub(crate) async fn given_attached_typed_stream(&mut self, stream_id: usize) -> &mut Self {
        let (tx, rx) = mpsc::channel(16);
        self.stream_senders.insert(stream_id, tx);
        self.run_action(None, UserAction::AttachTypedStream { stream_id, rx }).await;
        assert!(self.last_error.is_none(), "attaching typed stream should succeed");
        self
    }

    pub(crate) async fn when_send_typed_stream_item(
        &mut self,
        stream_id: usize,
        item: &str,
    ) -> &mut Self {
        let sender =
            self.stream_senders.get(&stream_id).expect("typed stream sender must be registered");
        sender.send(item.to_string()).await.expect("sending item to typed stream should succeed");
        self
    }

    pub(crate) fn when_close_typed_stream(&mut self, stream_id: usize) -> &mut Self {
        self.stream_senders
            .remove(&stream_id)
            .expect("typed stream sender must exist before closing");
        self
    }

    pub(crate) async fn when_spawn_completing_task(&mut self, task_id: u32) -> &mut Self {
        self.run_action(None, UserAction::SpawnCompletingTask(task_id)).await
    }

    pub(crate) async fn when_spawn_pending_task(&mut self, task_id: u32) -> &mut Self {
        self.run_action(None, UserAction::SpawnPendingTask(task_id)).await
    }

    pub(crate) async fn when_abort_task(&mut self, task_id: u32) -> &mut Self {
        self.run_action(None, UserAction::AbortTask(task_id)).await
    }

    pub(crate) async fn when_schedule_timer(
        &mut self,
        timer_id: u32,
        delay: Duration,
    ) -> &mut Self {
        self.run_action(None, UserAction::ScheduleTimer { timer_id, delay }).await
    }

    pub(crate) async fn when_cancel_timer(&mut self, timer_id: u32) -> &mut Self {
        self.run_action(None, UserAction::CancelTimer(timer_id)).await
    }

    pub(crate) async fn when_shutdown(&mut self) -> &mut Self {
        self.last_error = self.client.shutdown().await.err();
        self
    }

    pub(crate) async fn then_user_matches(
        &self,
        id: u32,
        expected_name: &str,
        expected_promoted: bool,
    ) -> &Self {
        let user =
            self.client.get(id).await.expect("get user should succeed").expect("user should exist");
        assert_eq!(user, User { id, name: expected_name.to_string(), promoted: expected_promoted });
        self
    }

    pub(crate) async fn then_user_does_not_exist(&self, id: u32) -> &Self {
        let user = self.client.get(id).await.expect("get missing user should succeed");
        assert!(user.is_none(), "expected user {id} to be absent, got {user:?}");
        self
    }

    pub(crate) async fn then_listed_user_names_are(&self, expected: &[&str]) -> &Self {
        let mut names: Vec<String> = self
            .client
            .list()
            .await
            .expect("list users should succeed")
            .into_iter()
            .map(|u| u.name)
            .collect();
        names.sort();
        let mut expected_sorted: Vec<String> = expected.iter().map(|s| (*s).to_string()).collect();
        expected_sorted.sort();
        assert_eq!(names, expected_sorted);
        self
    }

    pub(crate) fn then_last_created_id_is(&self, expected: u32) -> &Self {
        assert_eq!(self.last_created_id, Some(expected));
        self
    }

    pub(crate) fn then_last_action_result_is(&self, expected: usize) -> &Self {
        assert_eq!(self.last_action_result, Some(expected));
        self
    }

    pub(crate) fn then_last_error_is_not_found(&self, expected_id: u32) -> &Self {
        let err = self.last_error.as_ref().expect("expected an error to be recorded");
        assert!(
            matches!(err, FrameworkError::ServiceError(UserError::NotFound(id)) if *id == expected_id),
            "expected ServiceError(NotFound({expected_id})), got {err:?}"
        );
        self
    }

    pub(crate) fn then_last_error_is_actor_closed(&self) -> &Self {
        let err = self.last_error.as_ref().expect("expected ActorClosed error");
        assert!(
            matches!(err, FrameworkError::ActorClosed(_)),
            "expected ActorClosed error, got {err:?}"
        );
        self
    }

    pub(crate) async fn then_byte_stream_item_received(
        &mut self,
        expected_stream_id: u32,
        expected_len: usize,
    ) -> &mut Self {
        let event = self.next_event().await;
        assert_eq!(
            event,
            ActorEvent::ByteStreamItem { stream_id: expected_stream_id, len: expected_len }
        );
        self
    }

    pub(crate) async fn then_typed_stream_item_received(
        &mut self,
        expected_stream_id: usize,
        expected_item: &str,
    ) -> &mut Self {
        let event = self.next_event().await;
        assert_eq!(
            event,
            ActorEvent::TypedStreamItem {
                stream_id: expected_stream_id,
                item: expected_item.to_string(),
            }
        );
        self
    }

    pub(crate) async fn then_typed_stream_closed_received(
        &mut self,
        expected_stream_id: usize,
    ) -> &mut Self {
        let event = self.next_event().await;
        assert_eq!(event, ActorEvent::TypedStreamClosed(expected_stream_id));
        self
    }

    pub(crate) async fn then_task_closed_received(&mut self, expected_task_id: u32) -> &mut Self {
        let event = self.next_event().await;
        assert_eq!(event, ActorEvent::TaskClosed(expected_task_id));
        self
    }

    pub(crate) async fn then_timer_fired_received(&mut self, expected_timer_id: u32) -> &mut Self {
        let event = self.next_event().await;
        assert_eq!(event, ActorEvent::TimerFired(expected_timer_id));
        self
    }

    pub(crate) async fn then_shutdown_received(&mut self) -> &mut Self {
        let event = self.next_event().await;
        assert_eq!(event, ActorEvent::Shutdown);
        timeout(EVENT_TIMEOUT, &mut self.actor_handle)
            .await
            .expect("actor task should exit after shutdown")
            .expect("actor task should not panic");
        self
    }

    pub(crate) async fn then_stall_warning_recorded(
        &mut self,
        expected_actor_substr: &str,
        expected_event: &str,
        expected_threshold: Duration,
    ) -> &mut Self {
        let warning = timeout(EVENT_TIMEOUT, self.warn_rx.recv())
            .await
            .expect("timed out waiting for stall warning")
            .expect("warn_rx channel closed unexpectedly");

        let actor = warning.actor.expect("expected structured 'actor' field on warning");
        assert!(
            actor.contains(expected_actor_substr),
            "expected actor to contain {expected_actor_substr:?}, got {actor:?}"
        );

        let event = warning.event.expect("expected structured 'event' field on warning");
        assert_eq!(event, expected_event, "expected event {expected_event:?}, got {event:?}");

        let expected_threshold_ms =
            u64::try_from(expected_threshold.as_millis()).unwrap_or(u64::MAX);
        let threshold_ms =
            warning.threshold_ms.expect("expected structured 'threshold_ms' field on warning");
        assert_eq!(
            threshold_ms, expected_threshold_ms,
            "expected threshold_ms {expected_threshold_ms}, got {threshold_ms}"
        );

        let elapsed_ms =
            warning.elapsed_ms.expect("expected structured 'elapsed_ms' field on warning");
        assert!(
            elapsed_ms >= threshold_ms,
            "expected elapsed_ms {elapsed_ms} >= threshold_ms {threshold_ms}"
        );

        self
    }

    pub(crate) async fn then_no_stall_warnings_recorded(&mut self) -> &mut Self {
        self.client.list().await.expect("synchronizing list should succeed");
        let recv_result = self.warn_rx.try_recv();
        assert!(
            matches!(recv_result, Err(mpsc::error::TryRecvError::Empty)),
            "expected no stall warnings, got: {recv_result:?}"
        );
        self
    }

    async fn run_action(&mut self, id: Option<u32>, action: UserAction) -> &mut Self {
        match self.client.perform_action(id, action).await {
            Ok(res) => {
                self.last_action_result = Some(res);
                self.last_error = None;
            }
            Err(err) => {
                self.last_action_result = None;
                self.last_error = Some(err);
            }
        }
        self
    }

    async fn next_event(&mut self) -> ActorEvent {
        timeout(EVENT_TIMEOUT, self.event_rx.recv())
            .await
            .expect("timed out waiting for actor lifecycle event")
            .expect("actor event channel closed unexpectedly")
    }
}

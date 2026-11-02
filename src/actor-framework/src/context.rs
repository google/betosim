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

//! # Actor Framework Context
//!
//! This module defines the `Context` trait and its standard implementation,
//! `FrameworkContext`. The context allows the implementation of actors to
//! interact with the framework environment:
//! - Time management (setting tick intervals).
//! - Asynchronous stream management.
//! - Lifecycle control (shutdown signals).
//!
//! This abstraction allows for easy testing by mocking the context, as well as
//! providing a consistent interface for actors to interact with the underlying
//! execution environment.

use std::time::Duration;

use futures::future::BoxFuture;
use tokio::sync::oneshot;
use tokio_stream::{StreamMap, StreamNotifyClose};
use tokio_util::time::{DelayQueue, delay_queue};

use crate::{ActorService, BoxStream, BoxTypedStream};

pub type TimerKey = delay_queue::Key;

pub type TimerCallback<T> = Box<dyn FnOnce(&mut T, &mut dyn Context<T>) + Send>;

/// The runtime environment for an actor, providing access to time, streams, and
/// lifecycle.
pub type DynContext<T> = dyn Context<T> + Send;

/// The runtime environment for an actor.
pub trait Context<T: ActorService>: Send + 'static {
    /// Schedule a message to be sent to the actor after a delay.oop.
    fn set_interval(&mut self, duration: Duration);

    /// Adds a new stream to be managed by the actor.
    fn add_stream(&mut self, id: T::Id, stream: BoxStream);

    /// Removes a stream by its ID.
    fn remove_stream(&mut self, id: T::Id);

    /// Adds a typed stream to the context.
    fn add_typed_stream(&mut self, id: usize, stream: BoxTypedStream<T::TypedStream>);

    /// Removes a typed stream by its ID.
    fn remove_typed_stream(&mut self, id: usize);

    /// Spawns a background task to be managed by the runtime.
    ///
    /// The task is identified by `id`. When it completes, the actor's
    /// `on_task_closed` hook will be called with the value returned by the
    /// task (which must be its `id`).
    fn spawn(&mut self, id: T::Id, task: BoxFuture<'static, T::Id>);

    /// Aborts a background task by its ID.
    fn abort(&mut self, id: T::Id);

    /// Signals the actor to stop processing messages and exit its run loop.
    fn shutdown(&mut self);

    /// Schedule a closure to be run after a duration.
    fn run_later(&mut self, duration: Duration, f: TimerCallback<T>) -> TimerKey;

    /// Cancel a scheduled timer.
    fn cancel_timer(&mut self, key: TimerKey);
}

pub(crate) struct FrameworkContext<T: ActorService> {
    pub(crate) interval: tokio::time::Interval,
    pub(crate) streams: StreamMap<T::Id, StreamNotifyClose<BoxStream>>,
    pub(crate) typed_streams: StreamMap<usize, StreamNotifyClose<BoxTypedStream<T::TypedStream>>>,
    pub(crate) shutdown_tx: Option<oneshot::Sender<()>>,
    pub(crate) tasks: tokio::task::JoinSet<T::Id>,
    pub(crate) task_handles: std::collections::HashMap<T::Id, tokio::task::AbortHandle>,
    pub(crate) timers: DelayQueue<TimerCallback<T>>,
}

impl<T: ActorService> FrameworkContext<T> {
    /// Creates a new FrameworkContext with default settings.
    /// Returns the FrameworkContext and a shutdown receiver.
    pub(crate) fn new() -> (Self, oneshot::Receiver<()>) {
        // Default interval is effectively "never" (10 years)
        let mut interval =
            tokio::time::interval(std::time::Duration::from_secs(365 * 10 * 24 * 60 * 60));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        (
            Self {
                interval,
                streams: StreamMap::new(),
                typed_streams: StreamMap::new(),
                shutdown_tx: Some(shutdown_tx),
                tasks: tokio::task::JoinSet::new(),
                task_handles: std::collections::HashMap::new(),
                timers: DelayQueue::new(),
            },
            shutdown_rx,
        )
    }
}

impl<T: ActorService> Context<T> for FrameworkContext<T> {
    fn set_interval(&mut self, duration: Duration) {
        let mut interval = tokio::time::interval(duration);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        self.interval = interval;
    }

    fn add_stream(&mut self, id: T::Id, stream: BoxStream) {
        self.streams.insert(id, StreamNotifyClose::new(stream));
    }

    fn remove_stream(&mut self, id: T::Id) {
        self.streams.remove(&id);
    }

    fn add_typed_stream(&mut self, id: usize, stream: BoxTypedStream<T::TypedStream>) {
        self.typed_streams.insert(id, StreamNotifyClose::new(stream));
    }

    fn remove_typed_stream(&mut self, id: usize) {
        self.typed_streams.remove(&id);
    }

    fn spawn(&mut self, id: T::Id, task: BoxFuture<'static, T::Id>) {
        let handle = self.tasks.spawn(task);
        self.task_handles.insert(id, handle);
    }

    fn abort(&mut self, id: T::Id) {
        if let Some(handle) = self.task_handles.remove(&id) {
            handle.abort();
        }
    }

    fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        self.tasks.abort_all();
        self.task_handles.clear();
    }

    fn run_later(&mut self, duration: Duration, f: TimerCallback<T>) -> TimerKey {
        self.timers.insert(f, duration)
    }

    fn cancel_timer(&mut self, key: TimerKey) {
        self.timers.remove(&key);
    }
}

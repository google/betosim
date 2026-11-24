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

use std::time::Duration;

use crate::world::TestWorld;

// Feature: Actor Service Lifecycle, CRUD, Streams, Tasks, Timers, and Shutdown
//
//   As a framework user
//   I want to manage resource state, streams, background tasks, timers, and shutdown via ResourceClient
//   So that actors execute business logic and lifecycle hooks deterministically

// Scenario: Create (auto and explicit ID), Read, List, Update, and Delete flow
//
//   Given a running user actor
//   When I create users with auto-assigned and explicit IDs, update one, list them, and delete one
//   Then get and list reflect each state transition and deleted users return None
#[tokio::test]
async fn test_crud_and_list_flow() {
    // Given
    let mut world = TestWorld::new();

    // When (Create with auto-assigned ID and explicit ID)
    let alice_id = world.when_create_user("Alice").await;
    world.then_last_created_id_is(1);
    world.when_create_user_with_id(42, "Bob").await.then_last_created_id_is(42);

    // Then (Read and List)
    world
        .then_user_matches(alice_id, "Alice", false)
        .await
        .then_user_matches(42, "Bob", false)
        .await
        .then_listed_user_names_are(&["Alice", "Bob"])
        .await;

    // When (Update Alice)
    world.when_update_user_name(alice_id, "Alicia").await;

    // Then (Read updated Alice)
    world.then_user_matches(alice_id, "Alicia", false).await;

    // When (Delete Alice)
    world.when_delete_user(alice_id).await;

    // Then (Alice is gone, Bob remains)
    world.then_user_does_not_exist(alice_id).await.then_listed_user_names_are(&["Bob"]).await;
}

// Scenario: Custom actions and ServiceError propagation on missing entities
//
//   Given a user actor with existing users
//   When I perform Rename, Promote, and global ResetAll actions, or target a missing user ID
//   Then entity and global actions mutate state as expected and missing IDs return ServiceError(NotFound)
#[tokio::test]
async fn test_action_and_service_error_flow() {
    // Given
    let mut world = TestWorld::new();
    let id = world.given_user("Alice").await;
    let _other_id = world.given_user("Dave").await;

    // When (Rename and Promote existing user)
    world.when_rename_user(id, "Charlie").await.then_last_action_result_is(1);
    world.when_promote_user(id).await.then_last_action_result_is(1);

    // Then (User reflects both Rename and Promote)
    world.then_user_matches(id, "Charlie", true).await;

    // When (Update and Action on non-existent user ID 999)
    world.when_update_user_name(999, "Ghost").await.then_last_error_is_not_found(999);
    world.when_promote_user(999).await.then_last_error_is_not_found(999);

    // When (Global ResetAll action with id = None)
    world.when_reset_all_users().await.then_last_action_result_is(2);

    // Then (All users are cleared)
    world.then_listed_user_names_are(&[]).await;
}

// Scenario: Typed stream registration, message delivery, and stream closure
//
//   Given a user actor with an attached typed stream
//   When items are sent across the typed stream and the stream sender is closed
//   Then on_typed_stream receives each item in order and on_typed_stream_closed fires on EOF
#[tokio::test]
async fn test_typed_stream_lifecycle() {
    // Given
    let mut world = TestWorld::new();
    let stream_id = 7;
    world.given_attached_typed_stream(stream_id).await;

    // When (Send typed items and then close the stream)
    world
        .when_send_typed_stream_item(stream_id, " frame-1 ")
        .await
        .when_send_typed_stream_item(stream_id, "frame-2")
        .await
        .when_close_typed_stream(stream_id);

    // Then (Actor receives both items followed by the stream-closed hook)
    world
        .then_typed_stream_item_received(stream_id, " frame-1 ")
        .await
        .then_typed_stream_item_received(stream_id, "frame-2")
        .await
        .then_typed_stream_closed_received(stream_id)
        .await;
}

// Scenario: Background task completion vs abort and timer execution vs cancellation
//
//   Given a running user actor
//   When a pending background task is aborted while another completes, and a scheduled timer is cancelled while another expires
//   Then only the non-aborted task triggers on_task_closed and only the uncancelled timer callback fires
#[tokio::test]
async fn test_task_abort_and_timer_cancellation() {
    // Given
    let mut world = TestWorld::new();

    // When (Spawn a pending task 10, abort it, and spawn a completing task 11)
    world
        .when_spawn_pending_task(10)
        .await
        .when_abort_task(10)
        .await
        .when_spawn_completing_task(11)
        .await;

    // Then (on_task_closed fires for task 11 without firing for aborted task 10)
    world.then_task_closed_received(11).await;

    // When (Schedule timer 1 and cancel it immediately, then schedule timer 2 to fire)
    world
        .when_schedule_timer(1, Duration::from_millis(500))
        .await
        .when_cancel_timer(1)
        .await
        .when_schedule_timer(2, Duration::from_millis(1000))
        .await;

    // Then (Only timer 2 fires)
    world.then_timer_fired_received(2).await;

    world.when_cancel_timer(2).await.then_last_action_result_is(2);
}

// Scenario: Actor shutdown stops the event loop and rejects subsequent client requests
//
//   Given a running user actor
//   When shutdown is requested through the client
//   Then on_shutdown is invoked, the actor task exits, and subsequent client requests return ActorClosed
#[tokio::test]
async fn test_actor_shutdown_closes_client() {
    // Given
    let mut world = TestWorld::new();
    let _id = world.given_user("Alice").await;

    // When
    world.when_shutdown().await;

    // Then
    world.then_shutdown_received().await;
    world.when_create_user("AfterShutdown").await;
    world.then_last_error_is_actor_closed();
}

// Scenario: Stalled action execution emits structured stall warning
//
//   Given an actor running with default 100ms stall threshold
//   When a custom action stalls for 150ms (> 100ms threshold)
//   Then a structured warning is emitted with actor, event, elapsed_ms, and threshold_ms
#[tokio::test]
async fn test_stalled_action_emits_stall_warning() {
    const DEFAULT_STALL_ACTION_DELAY: Duration = Duration::from_millis(150);

    // Given
    let mut world = TestWorld::new();

    // When
    world.when_run_slow_action(DEFAULT_STALL_ACTION_DELAY).await;

    // Then
    world
        .then_stall_warning_recorded("UserActor", "handle_action", Duration::from_millis(100))
        .await;
}

// Scenario: Stalled stream handler execution emits structured stall warning
//
//   Given an actor running with a 10ms stall threshold and an attached byte stream
//   When a byte stream item triggers an on_stream handler stalling for 35ms
//   Then the item is processed and a structured on_stream warning is recorded
#[tokio::test]
async fn test_stalled_stream_handler_emits_stall_warning() {
    const FAST_STALL_THRESHOLD: Duration = Duration::from_millis(10);
    const STALL_HANDLER_DELAY: Duration = Duration::from_millis(35);

    // Given
    let mut world = TestWorld::with_stall_threshold(FAST_STALL_THRESHOLD);
    let stream_id = 99;
    world.given_attached_byte_stream(stream_id, STALL_HANDLER_DELAY).await;

    // When
    world.when_send_byte_stream_item(stream_id, b"stream-payload").await;

    // Then
    world
        .then_byte_stream_item_received(stream_id, b"stream-payload".len())
        .await
        .then_stall_warning_recorded("UserActor", "on_stream", FAST_STALL_THRESHOLD)
        .await;
}

// Scenario: Fast handlers below stall threshold do not emit warnings
//
//   Given an actor running with a generous 5s stall threshold
//   When standard CRUD operations execute normally
//   Then no stall warnings are recorded
#[tokio::test]
async fn test_fast_handlers_below_stall_threshold_do_not_warn() {
    const GENEROUS_STALL_THRESHOLD: Duration = Duration::from_secs(5);

    // Given
    let mut world = TestWorld::with_stall_threshold(GENEROUS_STALL_THRESHOLD);

    // When
    let alice_id = world.when_create_user("Alice").await;
    world.when_update_user_name(alice_id, "Alicia").await;
    world.when_rename_user(alice_id, "Alice").await;
    world.when_promote_user(alice_id).await;
    world.when_delete_user(alice_id).await;

    // Then
    world.then_no_stall_warnings_recorded().await;
}

// Scenario: Aborting a task that already completed into JoinSet suppresses stale on_task_closed
//
//   Given a running user actor
//   When a task completes before the event loop polls tasks and is then aborted, followed by a 50ms sentinel timer
//   Then the sentinel timer fires without emitting TaskClosed
#[tokio::test(flavor = "current_thread")]
async fn test_abort_after_task_completion_ignores_stale_close() {
    const TASK_ID: u32 = 21;
    const SENTINEL_TIMER_ID: u32 = 101;
    const SENTINEL_DELAY: Duration = Duration::from_millis(50);

    // Given
    let mut world = TestWorld::new();

    // When
    world
        .when_abort_completed_task(TASK_ID)
        .await
        .when_schedule_timer(SENTINEL_TIMER_ID, SENTINEL_DELAY)
        .await;

    // Then
    world.then_timer_fired_received(SENTINEL_TIMER_ID).await;
}

// Scenario: Respawning with the same ID after aborting a completed task ignores stale close and runs the replacement
//
//   Given a running user actor
//   When a completed task is aborted and replaced with a gated task for the same ID, followed by a 50ms sentinel timer
//   Then the sentinel timer fires before any TaskClosed event, and releasing the replacement emits TaskClosed once
#[tokio::test(flavor = "current_thread")]
async fn test_respawn_after_aborting_completed_task_ignores_stale_close() {
    const TASK_ID: u32 = 22;
    const SENTINEL_TIMER_ID: u32 = 102;
    const SENTINEL_DELAY: Duration = Duration::from_millis(50);

    // Given
    let mut world = TestWorld::new();

    // When
    world
        .when_respawn_after_aborting_completed_task(TASK_ID)
        .await
        .when_schedule_timer(SENTINEL_TIMER_ID, SENTINEL_DELAY)
        .await;

    // Then
    world.then_timer_fired_received(SENTINEL_TIMER_ID).await;

    // When
    world.when_release_replacement_task();

    // Then
    world.then_task_closed_received(TASK_ID).await;
}

// Scenario: Superseding a completed task with the same ID suppresses stale on_task_closed
//
//   Given a running user actor
//   When a completed task is superseded by a gated task with the same ID without calling abort, followed by a 50ms sentinel timer
//   Then the sentinel timer fires before any TaskClosed event, and releasing the replacement emits TaskClosed once
#[tokio::test(flavor = "current_thread")]
async fn test_superseded_task_completion_ignores_stale_close() {
    const TASK_ID: u32 = 23;
    const SENTINEL_TIMER_ID: u32 = 103;
    const SENTINEL_DELAY: Duration = Duration::from_millis(50);

    // Given
    let mut world = TestWorld::new();

    // When
    world
        .when_supersede_completed_task(TASK_ID)
        .await
        .when_schedule_timer(SENTINEL_TIMER_ID, SENTINEL_DELAY)
        .await;

    // Then
    world.then_timer_fired_received(SENTINEL_TIMER_ID).await;

    // When
    world.when_release_replacement_task();

    // Then
    world.then_task_closed_received(TASK_ID).await;
}

// Scenario: Superseding a pending task with the same ID aborts the previous task
//
//   Given a running user actor with an active pending task
//   When a new completing task is spawned for the same ID without calling abort
//   Then the previous pending task's drop guard is cancelled and the replacement task emits TaskClosed
#[tokio::test(flavor = "current_thread")]
async fn test_superseding_pending_task_aborts_previous_task() {
    const TASK_ID: u32 = 24;

    // Given
    let mut world = TestWorld::new();
    world.given_pending_task_with_drop_guard(TASK_ID).await;

    // When
    world.when_spawn_completing_task(TASK_ID).await;

    // Then
    world.then_pending_task_was_cancelled(TASK_ID).await.then_task_closed_received(TASK_ID).await;
}

// Scenario: Slow terminal on_shutdown completes without emitting a loop-stall warning
#[tokio::test]
async fn test_slow_on_shutdown_does_not_emit_stall_warning() {
    const STALL_THRESHOLD: Duration = Duration::from_millis(50);
    const SLOW_SHUTDOWN_DELAY: Duration = Duration::from_millis(120);

    // Given an actor with a 50ms stall threshold and a 120ms on_shutdown delay
    let mut world =
        TestWorld::with_stall_threshold_and_shutdown_delay(STALL_THRESHOLD, SLOW_SHUTDOWN_DELAY);

    // When shutdown is requested through the client
    world.when_shutdown().await;

    // Then on_shutdown runs to completion, the actor task exits, and no on_shutdown stall warning is emitted
    world.then_shutdown_received().await.then_no_on_shutdown_stall_warning_emitted().await;
}

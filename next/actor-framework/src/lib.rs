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

//! # Actor Framework
//!
//! This crate provides the foundational building blocks for creating type-safe,
//! concurrent actor systems in Rust. It implements a **Resource-Oriented
//! Architecture (ROA)** pattern on top of the **Actor Model**, providing a
//! clean abstraction for managing stateful resources.
//!
//! ## Why ROA + Actor Model?
//!
//! This framework combines **Resource-Oriented Architecture (ROA)** with the
//! **Actor Model** to create a powerful pattern for building scalable systems.
//!
//! ### Resource-Oriented Architecture (ROA)
//!
//! - Standard CRUD operations (Create, Read, Update, Delete) on well-defined
//!   resources
//! - Predictable lifecycle management
//! - Clean, uniform API surface across all resource types
//!
//! ### Actor Model
//!
//! - Isolated state (no shared memory, no locks)
//! - Message-passing concurrency
//! - Sequential processing within each actor eliminates race conditions
//!
//! ### The Synergy
//!
//! - **Separation**: Each resource type (User, Product, Order) gets its own
//!   actor with completely isolated state
//! - **Coordination**: When resources need to interact (e.g., Order reserving
//!   Product stock), they communicate via **Action messages** instead of direct
//!   coupling
//! - **Scalability**: Independent resources can scale independently without
//!   coordination overhead
//! - **Maintainability**: Changes to one resource type don't ripple through the
//!   system
//!
//! This pattern excels in systems with many loosely-coupled resources that
//! occasionally need to coordinate. The ROA provides structure, while the Actor
//! Model provides safe concurrency.
//!
//! **Further Reading**:
//! - [Actor Model (Wikipedia)](https://en.wikipedia.org/wiki/Actor_model) -
//!   Foundational concurrency pattern by Carl Hewitt
//! - [Resource-Oriented Architecture](https://en.wikipedia.org/wiki/Resource-oriented_architecture#cite_note-Fielding-Ch5-1)
//!   - Roy Fielding's dissertation on REST/ROA principles
//! - [Actors in Rust](https://ryhl.io/blog/actors-with-tokio/) - Practical
//!   guide to implementing actors with Tokio
//!
//! ## Architecture Overview
//!
//! The framework separates concerns into three layers:
//!
//! 1. **Service Layer** ([`ActorService`]) - Business logic and domain models
//! 2. **Lifecycle Layer** ([`ActorLifecycle`]) - State management and
//!    dependencies
//! 3. **Interface Layer** ([`ResourceClient`]) - Type-safe communication
//!
//! This separation means you write your business logic **once** in the service
//! trait, and the framework handles all the async message passing, error
//! handling, and state management.
//!
//! ## Core Abstractions
//!
//! ### [`ActorService`] - The Business Logic
//!
//! Define what your actor manages and how it behaves:
//!
//! ```rust
//! use actor_framework::{
//!     ActorLifecycle, ActorService, Context, DynContext, ResourceActor, ResourceClient,
//! };
//!
//! // 1. Define the Service and Lifecycle
//! #[derive(Clone, Debug)]
//! struct User {
//!     id: u32,
//!     name: String,
//! }
//!
//! #[derive(Debug)]
//! struct UserCreate {
//!     name: String,
//! }
//! #[derive(Debug)]
//! struct UserUpdate {
//!     name: Option<String>,
//! }
//! #[derive(Debug)]
//! enum UserAction {}
//! #[derive(Debug)]
//! struct UserError(String);
//!
//! impl std::fmt::Display for UserError {
//!     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//!         write!(f, "{}", self.0)
//!     }
//! }
//! impl std::error::Error for UserError {}
//! impl From<String> for UserError {
//!     fn from(s: String) -> Self {
//!         UserError(s)
//!     }
//! }
//!
//! impl ActorService for User {
//!     type Id = u32;
//!     type Create = UserCreate;
//!     type Update = UserUpdate;
//!     type Action = UserAction;
//!     type ActionResult = ();
//!     type Error = UserError;
//!     type Entity = User;
//!     type TypedStream = ();
//!
//!     async fn handle_create(
//!         &mut self,
//!         id: Option<u32>,
//!         params: UserCreate,
//!         _: &mut DynContext<Self>,
//!     ) -> Result<u32, Self::Error> {
//!         let id = id.unwrap_or(0);
//!         self.id = id;
//!         self.name = params.name;
//!         Ok(id)
//!     }
//!     async fn handle_get(
//!         &self,
//!         _: u32,
//!         _: &mut DynContext<Self>,
//!     ) -> Result<Option<Self::Entity>, Self::Error> {
//!         Ok(Some(self.clone()))
//!     }
//!     async fn handle_update(
//!         &mut self,
//!         _: u32,
//!         update: UserUpdate,
//!         _: &mut DynContext<Self>,
//!     ) -> Result<Self::Entity, Self::Error> {
//!         if let Some(name) = update.name {
//!             self.name = name;
//!         }
//!         Ok(self.clone())
//!     }
//!     async fn handle_delete(
//!         &mut self,
//!         _: u32,
//!         _: &mut DynContext<Self>,
//!     ) -> Result<(), Self::Error> {
//!         Ok(())
//!     }
//!     async fn handle_action(
//!         &mut self,
//!         _: Option<u32>,
//!         _: UserAction,
//!         _: &mut DynContext<Self>,
//!     ) -> Result<(), Self::Error> {
//!         Ok(())
//!     }
//!     async fn handle_list(
//!         &mut self,
//!         _: &mut DynContext<Self>,
//!     ) -> Result<Vec<Self::Entity>, Self::Error> {
//!         Ok(vec![self.clone()])
//!     }
//! }
//!
//! impl ActorLifecycle for User {
//!     async fn on_start(&mut self, _ctx: &mut DynContext<Self>) {}
//!     async fn on_tick(&mut self, _ctx: &mut DynContext<Self>) {}
//!     async fn on_stream(&mut self, _id: u32, _msg: bytes::Bytes, _ctx: &mut DynContext<Self>) {}
//!     async fn on_stream_closed(&mut self, _id: u32, _ctx: &mut DynContext<Self>) {}
//!     async fn on_task_closed(&mut self, _id: u32, _ctx: &mut DynContext<Self>) {}
//! }
//!
//! // 2. Use the Actor
//! #[tokio::main]
//! async fn main() {
//!     // Create actor and client
//!     let user = User { id: 0, name: "".into() };
//!     let (actor, client) = ResourceActor::new(10);
//!
//!     // Spawn the actor
//!     tokio::spawn(actor.run(user));
//!
//!     // Use the client
//!     let id = client.create(UserCreate { name: "Alice".into() }).await.unwrap();
//!     let user = client.get(id).await.unwrap().unwrap();
//!     assert_eq!(user.name, "Alice");
//! }
//! ```
//!
//! ## Context Injection Pattern
//!
//! ```rust
//! use actor_framework::{
//!     ActorLifecycle, ActorService, Context, DynContext, ResourceActor, ResourceClient,
//! };
//!
//! // --- Define Minimal Services ---
//! #[derive(Clone, Debug)]
//! struct User {
//!     id: u32,
//! }
//! #[derive(Debug)]
//! struct UserCreate;
//! #[derive(Debug)]
//! struct UserUpdate;
//! #[derive(Debug)]
//! enum UserAction {}
//! #[derive(Debug)]
//! struct UserError;
//! impl std::fmt::Display for UserError {
//!     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//!         write!(f, "Err")
//!     }
//! }
//! impl std::error::Error for UserError {}
//! impl From<String> for UserError {
//!     fn from(_: String) -> Self {
//!         UserError
//!     }
//! }
//!
//! impl ActorService for User {
//!     type Id = u32;
//!     type Create = UserCreate;
//!     type Update = UserUpdate;
//!     type Action = UserAction;
//!     type ActionResult = ();
//!     type Error = UserError;
//!     type Entity = User;
//!     type TypedStream = ();
//!
//!     async fn handle_create(
//!         &mut self,
//!         id: Option<u32>,
//!         _: UserCreate,
//!         _: &mut DynContext<Self>,
//!     ) -> Result<u32, Self::Error> {
//!         self.id = id.unwrap_or(0);
//!         Ok(self.id)
//!     }
//!     async fn handle_get(
//!         &self,
//!         _: u32,
//!         _: &mut DynContext<Self>,
//!     ) -> Result<Option<Self::Entity>, Self::Error> {
//!         Ok(Some(self.clone()))
//!     }
//!     async fn handle_update(
//!         &mut self,
//!         _: u32,
//!         _: UserUpdate,
//!         _: &mut DynContext<Self>,
//!     ) -> Result<Self::Entity, Self::Error> {
//!         Ok(self.clone())
//!     }
//!     async fn handle_delete(
//!         &mut self,
//!         _: u32,
//!         _: &mut DynContext<Self>,
//!     ) -> Result<(), Self::Error> {
//!         Ok(())
//!     }
//!     async fn handle_action(
//!         &mut self,
//!         _: Option<u32>,
//!         _: UserAction,
//!         _: &mut DynContext<Self>,
//!     ) -> Result<(), Self::Error> {
//!         Ok(())
//!     }
//!     async fn handle_list(
//!         &mut self,
//!         _: &mut DynContext<Self>,
//!     ) -> Result<Vec<Self::Entity>, Self::Error> {
//!         Ok(vec![self.clone()])
//!     }
//! }
//!
//! impl ActorLifecycle for User {
//!     async fn on_start(&mut self, _ctx: &mut DynContext<Self>) {}
//!     async fn on_tick(&mut self, _ctx: &mut DynContext<Self>) {}
//!     async fn on_stream(&mut self, _id: u32, _msg: bytes::Bytes, _ctx: &mut DynContext<Self>) {}
//!     async fn on_stream_closed(&mut self, _id: u32, _ctx: &mut DynContext<Self>) {}
//!     async fn on_task_closed(&mut self, _id: u32, _ctx: &mut DynContext<Self>) {}
//! }
//!
//! #[derive(Clone, Debug)]
//! struct Product {
//!     id: u32,
//! }
//! #[derive(Debug)]
//! struct ProductCreate;
//! #[derive(Debug)]
//! struct ProductUpdate;
//! #[derive(Debug)]
//! enum ProductAction {}
//! #[derive(Debug)]
//! struct ProductError;
//! impl std::fmt::Display for ProductError {
//!     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//!         write!(f, "Err")
//!     }
//! }
//! impl std::error::Error for ProductError {}
//! impl From<String> for ProductError {
//!     fn from(_: String) -> Self {
//!         ProductError
//!     }
//! }
//! impl ActorService for Product {
//!     type Id = u32;
//!     type Create = ProductCreate;
//!     type Update = ProductUpdate;
//!     type Action = ProductAction;
//!     type ActionResult = ();
//!     type Error = ProductError;
//!     type Entity = Product;
//!     type TypedStream = ();
//!     async fn handle_create(
//!         &mut self,
//!         id: Option<u32>,
//!         _: ProductCreate,
//!         _: &mut DynContext<Self>,
//!     ) -> Result<u32, Self::Error> {
//!         self.id = id.unwrap_or(0);
//!         Ok(self.id)
//!     }
//!     async fn handle_get(
//!         &self,
//!         _: u32,
//!         _: &mut DynContext<Self>,
//!     ) -> Result<Option<Self::Entity>, Self::Error> {
//!         Ok(Some(self.clone()))
//!     }
//!     async fn handle_update(
//!         &mut self,
//!         _: u32,
//!         _: ProductUpdate,
//!         _: &mut DynContext<Self>,
//!     ) -> Result<Self::Entity, Self::Error> {
//!         Ok(self.clone())
//!     }
//!     async fn handle_delete(
//!         &mut self,
//!         _: u32,
//!         _: &mut DynContext<Self>,
//!     ) -> Result<(), Self::Error> {
//!         Ok(())
//!     }
//!     async fn handle_action(
//!         &mut self,
//!         _: Option<u32>,
//!         _: ProductAction,
//!         _: &mut DynContext<Self>,
//!     ) -> Result<(), Self::Error> {
//!         Ok(())
//!     }
//!     async fn handle_list(
//!         &mut self,
//!         _: &mut DynContext<Self>,
//!     ) -> Result<Vec<Product>, Self::Error> {
//!         Ok(vec![self.clone()])
//!     }
//! }
//! impl ActorLifecycle for Product {
//!     async fn on_start(&mut self, _ctx: &mut DynContext<Self>) {}
//!     async fn on_tick(&mut self, _ctx: &mut DynContext<Self>) {}
//!     async fn on_stream(&mut self, _id: u32, _msg: bytes::Bytes, _ctx: &mut DynContext<Self>) {}
//!     async fn on_stream_closed(&mut self, _id: u32, _ctx: &mut DynContext<Self>) {}
//!     async fn on_task_closed(&mut self, _id: u32, _ctx: &mut DynContext<Self>) {}
//! }
//!
//! #[derive(Clone, Debug)]
//! struct Order {
//!     id: u32,
//! }
//! // Order depends on UserClient and ProductClient
//! #[derive(Clone)]
//! struct OrderContext {
//!     user_client: ResourceClient<User>,
//!     product_client: ResourceClient<Product>,
//! }
//!
//! #[derive(Debug)]
//! struct OrderCreate;
//! #[derive(Debug)]
//! struct OrderUpdate;
//! #[derive(Debug)]
//! enum OrderAction {}
//! #[derive(Debug)]
//! struct OrderError;
//! impl std::fmt::Display for OrderError {
//!     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//!         write!(f, "Err")
//!     }
//! }
//! impl std::error::Error for OrderError {}
//! impl From<String> for OrderError {
//!     fn from(_: String) -> Self {
//!         OrderError
//!     }
//! }
//!
//! impl ActorService for Order {
//!     type Id = u32;
//!     type Create = OrderCreate;
//!     type Update = OrderUpdate;
//!     type Action = OrderAction;
//!     type ActionResult = ();
//!     type Error = OrderError;
//!     type Entity = Order;
//!     type TypedStream = ();
//!
//!     async fn handle_create(
//!         &mut self,
//!         id: Option<u32>,
//!         _: OrderCreate,
//!         _: &mut DynContext<Self>,
//!     ) -> Result<u32, Self::Error> {
//!         self.id = id.unwrap_or(0);
//!         Ok(self.id)
//!     }
//!     async fn handle_get(
//!         &self,
//!         _: u32,
//!         _: &mut DynContext<Self>,
//!     ) -> Result<Option<Self::Entity>, Self::Error> {
//!         Ok(Some(self.clone()))
//!     }
//!     async fn handle_update(
//!         &mut self,
//!         _: u32,
//!         _: OrderUpdate,
//!         _: &mut DynContext<Self>,
//!     ) -> Result<Self::Entity, Self::Error> {
//!         Ok(self.clone())
//!     }
//!     async fn handle_delete(
//!         &mut self,
//!         _: u32,
//!         _: &mut DynContext<Self>,
//!     ) -> Result<(), Self::Error> {
//!         Ok(())
//!     }
//!     async fn handle_action(
//!         &mut self,
//!         _: Option<u32>,
//!         _: OrderAction,
//!         _: &mut DynContext<Self>,
//!     ) -> Result<(), Self::Error> {
//!         Ok(())
//!     }
//!     async fn handle_list(
//!         &mut self,
//!         _: &mut DynContext<Self>,
//!     ) -> Result<Vec<Order>, Self::Error> {
//!         Ok(vec![self.clone()])
//!     }
//! }
//!
//! impl ActorLifecycle for Order {
//!     async fn on_start(&mut self, _ctx: &mut DynContext<Self>) {}
//!     async fn on_tick(&mut self, _ctx: &mut DynContext<Self>) {}
//!     async fn on_stream(&mut self, _id: u32, _msg: bytes::Bytes, _ctx: &mut DynContext<Self>) {}
//!     async fn on_stream_closed(&mut self, _id: u32, _ctx: &mut DynContext<Self>) {}
//!     async fn on_task_closed(&mut self, _id: u32, _ctx: &mut DynContext<Self>) {}
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     // 1. Create all actors
//!     let user_actor = User { id: 0 };
//!     let (user_runner, user_client) = ResourceActor::new(10);
//!
//!     let product_actor = Product { id: 0 };
//!     let (product_runner, product_client) = ResourceActor::new(10);
//!
//!     let order_actor = Order { id: 0 };
//!     let (order_runner, order_client) = ResourceActor::new(10);
//!
//!     // 2. Wire dependencies when starting actors
//!     tokio::spawn(user_runner.run(user_actor));
//!     tokio::spawn(product_runner.run(product_actor));
//!     // In a real app we'd pass clients to run if supported, or configure them via action/update
//!     // For this simple example we just run them.
//!     tokio::spawn(order_runner.run(order_actor));
//!
//!     // 3. Use the actor (keeps main alive)
//!     let _ = order_client.create(OrderCreate).await;
//! }
//! ```
//!
//! The `Order` actor receives `(UserClient, ProductClient)` as its context,
//! allowing it to validate users and reserve product stock during order
//! creation.
//!
//! ## Type Safety
//!
//! The framework leverages Rust's type system to eliminate entire classes of
//! runtime errors:
//!
//! - **Compile-time guarantees**: Can't send wrong message types to actors
//! - **Type-safe errors**: Each service defines its own error type
//! - **No stringly-typed APIs**: IDs, actions, and results are all strongly
//!   typed
//!
//! ## Concurrency Model
//!
//! - Each actor runs in its own Tokio task
//! - Messages are processed **sequentially** within an actor (no locks needed!)
//! - Multiple actors run in **parallel** (true concurrency)
//! - No shared mutable state (message passing only)
//!
//! ## Testing
//!
//! The framework provides a **MockActorClient** type (available with the
//! `testing` feature) that implements the [`ActorClient`] trait. It lets you
//! write fast, deterministic unit tests for client logic without spawning any
//! actors.

mod actor;
mod client;
mod context;
mod error;
mod lifecycle;
mod message;
mod service;

pub mod utils;

// Re-export core types for convenience
pub use actor::ResourceActor;
#[cfg(feature = "testing")]
pub use client::MockActorClient;
pub use client::{ActorClient, ResourceClient};
pub use context::{Context, DynContext, TimerCallback, TimerKey};
pub use error::FrameworkError;
pub use lifecycle::ActorLifecycle;
pub use message::{ResourceRequest, Response};
pub use service::{ActorId, ActorService, BoxStream, BoxTypedStream};

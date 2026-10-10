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

use std::collections::HashMap;

use actor_framework::{ActorService, DynContext, ResourceActor};

// Feature: Actor Service CRUD
//
//   As a framework user
//   I want to perform CRUD operations on resources
//   So that I can manage application state

#[derive(Clone, Debug, PartialEq)]
struct User {
    id: u32,
    name: String,
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
}

#[derive(Debug, thiserror::Error)]
#[error("User Error")]
struct UserError;

#[derive(Default)]
struct UserActor {
    users: HashMap<u32, User>,
    next_id: u32,
}

impl actor_framework::ActorLifecycle for UserActor {
    async fn on_start(&mut self, _ctx: &mut DynContext<Self>) {
        self.next_id = 1;
    }
}

impl ActorService for UserActor {
    type Id = u32;
    type Create = UserCreate;
    type Update = UserUpdate;
    type Action = UserAction;
    type ActionResult = bool;
    type Error = UserError;
    type Entity = User;
    type TypedStream = ();

    async fn handle_create(
        &mut self,
        id: Option<Self::Id>,
        params: Self::Create,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Self::Id, Self::Error> {
        let id = id.unwrap_or_else(|| {
            let id = self.next_id;
            self.next_id += 1;
            id
        });
        self.users.insert(id, User { id, name: params.name });
        Ok(id)
    }

    async fn handle_get(
        &self,
        id: Self::Id,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Option<Self::Entity>, Self::Error> {
        Ok(self.users.get(&id).cloned())
    }

    async fn handle_update(
        &mut self,
        id: Self::Id,
        update: Self::Update,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Self::Entity, Self::Error> {
        if let Some(user) = self.users.get_mut(&id) {
            if let Some(name) = update.name {
                user.name = name;
            }
            Ok(user.clone())
        } else {
            Err(UserError)
        }
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
        _ctx: &mut DynContext<Self>,
    ) -> Result<Self::ActionResult, Self::Error> {
        match action {
            UserAction::Rename(name) => {
                if let Some(id) = id {
                    if let Some(user) = self.users.get_mut(&id) {
                        user.name = name;
                        Ok(true)
                    } else {
                        Ok(false)
                    }
                } else {
                    Ok(false)
                }
            }
        }
    }

    async fn handle_list(
        &mut self,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Vec<Self::Entity>, Self::Error> {
        Ok(self.users.values().cloned().collect())
    }
}

// Scenario: Create, Read, Update, Delete flow
//
//   Given a user actor
//   When I create a user
//   Then I can read, update, and delete it
#[tokio::test]
async fn test_crud_flow() {
    // Given
    let (actor, client) = ResourceActor::new(10);
    tokio::spawn(actor.run(UserActor::default()));

    // When (Create)
    let id = client.create(UserCreate { name: "Alice".into() }).await.unwrap();
    assert_eq!(id, 1);

    // Then (Read)
    let user = client.get(id).await.unwrap().unwrap();
    assert_eq!(user.name, "Alice");

    // When (Update)
    let updated = client.update(id, UserUpdate { name: Some("Bob".into()) }).await.unwrap();
    assert_eq!(updated.name, "Bob");

    // Then (Read again)
    let user = client.get(id).await.unwrap().unwrap();
    assert_eq!(user.name, "Bob");

    // When (Delete)
    client.delete(id).await.unwrap();

    // Then (Read deleted)
    let user = client.get(id).await.unwrap();
    assert!(user.is_none());
}

// Scenario: Perform Action
//
//   Given a user actor with a user
//   When I perform an action on the user
//   Then the user state changes
#[tokio::test]
async fn test_action_flow() {
    // Given
    let (actor, client) = ResourceActor::new(10);
    tokio::spawn(actor.run(UserActor::default()));
    let id = client.create(UserCreate { name: "Alice".into() }).await.unwrap();

    // When
    let result =
        client.perform_action(Some(id), UserAction::Rename("Charlie".into())).await.unwrap();

    // Then
    assert!(result);
    let user = client.get(id).await.unwrap().unwrap();
    assert_eq!(user.name, "Charlie");
}

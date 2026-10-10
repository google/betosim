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

//! # Actor Framework Utilities
//!
//! This module provides helper functions to reduce boilerplate in
//! `ActorService` implementations.

use std::{collections::HashMap, hash::Hash};

/// Default implementation for `handle_get`.
///
/// Returns a clone of the entity if it exists in the map.
pub fn handle_get_default<K, V>(map: &HashMap<K, V>, id: &K) -> Option<V>
where
    K: Hash + Eq,
    V: Clone,
{
    map.get(id).cloned()
}

/// Default implementation for `handle_list`.
///
/// Returns a vector of all entities in the map.
pub fn handle_list_default<K, V>(map: &HashMap<K, V>) -> Vec<V>
where
    V: Clone,
{
    map.values().cloned().collect()
}

/// Default implementation for `handle_list` with mapping.
///
/// Returns a vector of mapped entities.
pub fn handle_list_map<K, V, R, F>(map: &HashMap<K, V>, f: F) -> Vec<R>
where
    F: Fn(&V) -> R,
{
    map.values().map(f).collect()
}

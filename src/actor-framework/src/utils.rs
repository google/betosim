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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{handle_get_default, handle_list_default, handle_list_map};

    #[test]
    fn handle_get_default_returns_cloned_entity_or_none() {
        let empty: HashMap<u32, String> = HashMap::new();
        assert_eq!(handle_get_default(&empty, &1), None);

        let map = HashMap::from([(10_u32, "alpha".to_string()), (20_u32, "beta".to_string())]);
        assert_eq!(handle_get_default(&map, &10), Some("alpha".to_string()));
        assert_eq!(handle_get_default(&map, &20), Some("beta".to_string()));
        assert_eq!(handle_get_default(&map, &99), None);
    }

    #[test]
    fn handle_list_default_returns_all_entities() {
        let empty: HashMap<u32, String> = HashMap::new();
        assert!(handle_list_default(&empty).is_empty());

        let map = HashMap::from([
            (1_u32, "first".to_string()),
            (2_u32, "second".to_string()),
            (3_u32, "third".to_string()),
        ]);
        let mut values = handle_list_default(&map);
        values.sort();
        assert_eq!(values, vec!["first".to_string(), "second".to_string(), "third".to_string()]);
    }

    #[test]
    fn handle_list_map_transforms_entities() {
        let empty: HashMap<u32, String> = HashMap::new();
        let empty_mapped = handle_list_map(&empty, |v| v.len());
        assert!(empty_mapped.is_empty());

        let map = HashMap::from([(1_u32, "a".to_string()), (2_u32, "abcd".to_string())]);
        let mut lengths = handle_list_map(&map, |v| v.len());
        lengths.sort_unstable();
        assert_eq!(lengths, vec![1, 4]);
    }
}

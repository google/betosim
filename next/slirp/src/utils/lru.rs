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

use std::collections::HashMap;

pub fn evict_lru<K, V, F>(cache: &mut HashMap<K, V>, max_entries: usize, key: &K, get_tick: F)
where
    K: std::cmp::Eq + std::hash::Hash + Copy,
    F: Fn(&V) -> u64,
{
    if !cache.contains_key(key) && cache.len() >= max_entries {
        let evict_key = cache.iter().min_by_key(|(_, v)| get_tick(v)).map(|(&k, _)| k);
        if let Some(k) = evict_key {
            cache.remove(&k);
        }
    }
}

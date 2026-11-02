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

pub mod features;

pub mod step;
pub mod utils;

pub use features::Features;
pub use step::{AsyncStep, DataTable, StepContext, World};
// Re-export specific utils if needed, or just let users access them via utils::
pub use utils::{
    apply_replacements, assert_json_matches_table, horizontal_table_to_structs, table_to_struct,
    vertical_table_to_map,
};

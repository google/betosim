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

//! # Feature Utilities
//!
//! Helper functions for working with DataTables and JSON.

use serde::{Serialize, de::DeserializeOwned};

use crate::step::DataTable;

/// Helper trait for parsing a single row of a DataTable into a struct.
pub trait FromDataRow: Sized {
    type Error;
    fn from_row(row: &[String]) -> Result<Self, Self::Error>;
}

/// Helper to convert a vertically oriented DataTable (Key | Value) into a
/// HashMap. Assumes rows are [Key, Value].
pub fn vertical_table_to_map(table: &DataTable) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for row in table {
        if row.len() >= 2 {
            map.insert(row[0].clone(), row[1].clone());
        }
    }
    map
}

/// Helper to convert a vertically oriented DataTable into a Struct via JSON.
///
/// This function:
/// 1. Converts the DataTable to a Map<String, String>.
/// 2. Serializes the Map to a serde_json::Value.
/// 3. Deserializes the Value into the target type T.
pub fn table_to_struct<T: DeserializeOwned>(table: &DataTable) -> Result<T, serde_json::Error> {
    let map = vertical_table_to_map(table);
    let json_value = serde_json::to_value(map)?;
    serde_json::from_value(json_value)
}

/// Helper to assert that a struct's JSON representation matches the expected
/// fields in a DataTable.
///
/// This function:
/// 1. Converts the actual struct to a serde_json::Value.
/// 2. Iterates over the key-value pairs in the DataTable.
/// 3. Asserts that each key exists in the JSON and matches the value.
///
/// Panics if a field is missing or mismatching.
pub fn assert_json_matches_table<S: Serialize>(actual: &S, table: &DataTable) {
    let expected_fields = vertical_table_to_map(table);
    let actual_json = serde_json::to_value(actual).expect("Failed to serialize actual value");

    for (key, expected_val) in expected_fields {
        match actual_json.get(&key) {
            Some(val) => {
                // Allow loose comparison for numbers/bools by converting to string
                let actual_str = match val {
                    serde_json::Value::String(s) => s.clone(),
                    _ => val.to_string(),
                };
                assert_eq!(
                    actual_str, *expected_val,
                    "Mismatch for field '{}': expected '{}', got '{}' (from JSON: {:?})",
                    key, expected_val, actual_str, val
                );
            }
            None => {
                panic!(
                    "Field '{}' not found in actual JSON. Available fields: {:?}",
                    key,
                    actual_json.as_object().map(|o| o.keys().collect::<Vec<_>>())
                );
            }
        }
    }
}

/// Helper to convert a horizontally oriented DataTable (Header Row | Data Rows)
/// into a list of Structs.
///
/// This function:
/// 1. Assumes the first row contains headers.
/// 2. Iterates over subsequent rows, creating a Map<Header, Value> for each.
/// 3. Deserializes each Map into the target type T.
pub fn horizontal_table_to_structs<T: DeserializeOwned>(
    table: &DataTable,
) -> Result<Vec<T>, serde_json::Error> {
    if table.is_empty() {
        return Ok(Vec::new());
    }

    let headers = &table[0];
    let mut results = Vec::new();

    for row in table.iter().skip(1) {
        let mut map = serde_json::Map::new();
        for (i, header) in headers.iter().enumerate() {
            if i < row.len() {
                map.insert(header.clone(), serde_json::Value::String(row[i].clone()));
            }
        }
        let struct_obj: T = serde_json::from_value(serde_json::Value::Object(map))?;
        results.push(struct_obj);
    }

    Ok(results)
}

/// Helper to substitute placeholders in a string.
///
/// Replaces all occurrences of keys with their corresponding values.
pub fn apply_replacements(text: &str, replacements: &[(&str, &str)]) -> String {
    let mut result = text.to_string();
    for (key, val) in replacements {
        result = result.replace(*key, val);
    }
    result
}

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

//! Common utility types and functions for the `packets` crate.

use zerocopy::Ref;

/// The result of a successful parse operation on a byte slice.
///
/// It contains a zero-copy reference to the parsed header (`T`) and a slice
/// representing the remaining bytes (`&'a [u8]`).
pub type ParseResult<'a, T> = (Ref<&'a [u8], T>, &'a [u8]);

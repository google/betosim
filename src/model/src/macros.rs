// Copyright 2023-2025 The Android Open Source Project
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

//! This module defines helper macros, particularly for reducing boilerplate
//! in client implementations for sending requests to actor services.

// =============================================================================
// CLIENT METHOD MACRO
// =============================================================================

/// Generate client methods with oneshot channel boilerplate.
/// Client methods convert domain errors to String for API simplicity.
#[macro_export]
macro_rules! client_method {
    // Pattern for methods WITH arguments
    ($client:ty => fn $method:ident($($param:ident: $param_type:ty),*) -> $return_type:ty as $request:ident::$variant:ident) => {
        impl $client {
            #[doc = "Sends the `"]
            #[doc = stringify!($variant)]
            #[doc = "` command to the service and waits for a response."]
            #[doc = ""]
            #[doc = "# Arguments"]
            #[doc = ""]
            $(
                #[doc = "* `"]
                #[doc = stringify!($param)]
                #[doc = "`: The `"]
                #[doc = stringify!($param_type)]
                #[doc = "` for the command."]
            )*
            pub async fn $method(&self, $($param: $param_type),*) -> std::result::Result<$return_type, ClientError> {
                let (respond_to, response) = oneshot::channel();
                self.sender.send($request::$variant {
                    $($param,)*
                    respond_to,
                }).await.map_err(|e| ClientError::Send(e.to_string()))?;
                response.await?.map_err(|device_err| ClientError::Framework(Box::new(device_err)))
            }
        }
    };

    // Pattern for methods WITHOUT arguments
    ($client:ty => fn $method:ident() -> $return_type:ty as $request:ident::$variant:ident) => {
        impl $client {
            #[doc = "Sends the `"]
            #[doc = stringify!($variant)]
            #[doc = "` command to the service and waits for a response."]
            pub async fn $method(&self) -> std::result::Result<$return_type, ClientError> {
                let (respond_to, response) = oneshot::channel();
                self.sender.send($request::$variant { respond_to })
                    .await
                    .map_err(|e| ClientError::Send(e.to_string()))?;
                response.await?
            }
        }
    };
}

/// Helper macro to update a field if an Option value is Some.
///
/// Usage:
/// - `set_if_some!(dest, option)`: explicit clone
/// - `set_if_some!(dest, option, map_fn)`: apply map_fn to inner value
#[macro_export]
macro_rules! set_if_some {
    ($dest:expr, $opt:expr) => {
        if let Some(val) = $opt {
            $dest = val.clone();
        }
    };
    ($dest:expr, $opt:expr, $map:expr) => {
        if let Some(val) = $opt {
            $dest = $map(val);
        }
    };
}

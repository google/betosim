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

use std::pin::Pin;

use bytes::Bytes;
use futures::Sink;
use tokio_stream::Stream;

// The only error from PacketStream occurs when source closes connection.
/// A stream of packets from the chip.
pub type PacketStream = Box<dyn Stream<Item = Bytes> + Send + Sync + Unpin>;
/// A sink for packets to the chip.
pub type PacketSink = Pin<Box<dyn Sink<Bytes, Error = std::io::Error> + Send + Sync>>;

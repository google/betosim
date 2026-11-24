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

use std::{collections::VecDeque, sync::mpsc, time::Duration};

use bytes::Bytes;

use crate::types::ModemSink;

/// Mock handler for modem callbacks that captures responses.
pub struct MockModemHandler {
    rx: mpsc::Receiver<Vec<u8>>,
    buffer: VecDeque<Vec<u8>>,
}

impl MockModemHandler {
    pub fn new(goldfish_37: bool) -> (Self, ModemSink) {
        let (tx, rx) = mpsc::channel();
        let sink = ModemSink::new(move |item: Bytes| {
            let delimiter = if goldfish_37 { b'\r' } else { b'\n' };
            for line in item.split_inclusive(|&b| b == delimiter) {
                tx.send(line.to_vec()).map_err(|e| e.to_string())?;
            }
            Ok(())
        });
        (Self { rx, buffer: VecDeque::new() }, sink)
    }

    /// Peeks at the next response without consuming it (non-blocking).
    pub fn peek_response(&mut self) -> Option<&Vec<u8>> {
        if self.buffer.is_empty()
            && let Ok(msg) = self.rx.try_recv()
        {
            self.buffer.push_back(msg);
        }
        self.buffer.front()
    }

    /// Drains all pending responses from the receiver into the buffer.
    pub fn buffer_pending_responses(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            self.buffer.push_back(msg);
        }
    }

    /// Returns a reference to the internal buffer.
    pub fn get_buffer(&self) -> &VecDeque<Vec<u8>> {
        &self.buffer
    }

    /// Waits for a response.
    /// Since the simulator is synchronous, responses should be available
    /// immediately.
    pub fn wait_for_response(&mut self) -> Vec<u8> {
        if let Some(msg) = self.buffer.pop_front() {
            return msg;
        }
        // Use recv_timeout to avoid hanging forever if logic is wrong, but typically
        // it's instant.
        self.rx
            .recv_timeout(Duration::from_secs(1))
            .expect("Test timed out waiting for response (or channel closed)")
    }

    /// Checks if a response is available (non-blocking).
    pub fn try_get_response(&mut self) -> Option<Vec<u8>> {
        if let Some(msg) = self.buffer.pop_front() {
            return Some(msg);
        }
        self.rx.try_recv().ok()
    }
}

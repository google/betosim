// Copyright 2023 The Android Open Source Project
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

#![allow(clippy::empty_line_after_doc_comments)]

use std::collections::HashMap;
use std::sync::mpsc::{Sender, channel};
use std::sync::{OnceLock, RwLock};
use std::thread;

use crate::captures;
use crate::devices::{chip, chip::ChipIdentifier};

use bytes::Bytes;
use log::{error, info, warn};
use netsim_proto::hci_packet::hcipacket::PacketType;
use protobuf::Enum;

/// The Packet module routes packets from a chip controller instance to
/// different transport managers. Currently transport managers include
///
/// - GRPC is a PacketStreamer
/// - FD is a file descriptor to a pair of Unix Fifos used by "-s" startup
/// - SOCKET is a TCP stream

// When a connection arrives, the transport registers a responder
// implementing Response trait for the packet stream.
pub trait Response {
    fn response(&mut self, packet: Bytes, packet_type: u8);
}

// When a responder is registered a responder thread is created to
// decouple the chip controller from the network. The thread reads
// ResponsePacket from a queue and sends to responder.
struct ResponsePacket {
    packet: Bytes,
    packet_type: u8,
}

// A hash map from chip_id to response channel.

struct PacketManager {
    transports: RwLock<HashMap<ChipIdentifier, Sender<ResponsePacket>>>,
}

static MANAGER: OnceLock<PacketManager> = OnceLock::new();

fn get_manager() -> &'static PacketManager {
    MANAGER.get_or_init(PacketManager::new)
}

/// Register a chip controller instance to a transport manager.
pub fn register_transport(chip_id: ChipIdentifier, responder: Box<dyn Response + Send>) {
    get_manager().register_transport(chip_id, responder);
}

/// Unregister a chip controller instance.
pub fn unregister_transport(chip_id: ChipIdentifier) {
    get_manager().unregister_transport(chip_id);
}

impl PacketManager {
    fn new() -> Self {
        PacketManager { transports: RwLock::new(HashMap::new()) }
    }
    /// Register a transport stream for handle_response calls.
    pub fn register_transport(
        &self,
        chip_id: ChipIdentifier,
        mut transport: Box<dyn Response + Send>,
    ) {
        let (tx, rx) = channel::<ResponsePacket>();
        if self.transports.write().unwrap().insert(chip_id, tx).is_some() {
            error!("register_transport: key already present for chip_id: {chip_id}");
        }
        let _ = thread::Builder::new().name(format!("transport_responder_{chip_id}")).spawn(
            move || {
                info!("register_transport: started thread chip_id: {chip_id}");
                while let Ok(ResponsePacket { packet, packet_type }) = rx.recv() {
                    transport.response(packet, packet_type);
                }
                info!("register_transport: finished thread chip_id: {chip_id}");
            },
        );
    }

    /// Unregister a chip controller instance.
    pub fn unregister_transport(&self, chip_id: ChipIdentifier) {
        // Shuts down the responder thread, because sender is dropped.
        self.transports.write().unwrap().remove(&chip_id);
    }
}

/// Handle requests from gRPC transport in C++.
pub fn handle_response_cxx(chip_id: u32, packet: &cxx::CxxVector<u8>, packet_type: u8) {
    // TODO(b/314840701):
    // 1. Per EChip Struct should contain private field of channel & facade_id
    // 2. Lookup from ECHIPS with given chip_id
    // 3. Call adaptor.handle_response
    let packet = Bytes::from(packet.as_slice().to_vec());
    let chip_id = ChipIdentifier(chip_id);
    captures::controller_to_host(chip_id, &packet, packet_type.into());

    let result = if let Some(transport) = get_manager().transports.read().unwrap().get(&chip_id) {
        transport.send(ResponsePacket { packet, packet_type })
    } else {
        warn!("handle_response: chip {chip_id} not found");
        Ok(())
    };
    // transports lock is now released
    if let Err(e) = result {
        warn!("handle_response: error {e:?}");
        unregister_transport(chip_id);
    }
}

// Handle response from rust libraries
pub fn handle_response(chip_id: ChipIdentifier, packet: &Bytes) {
    let packet_type = PacketType::HCI_PACKET_UNSPECIFIED.value() as u8;
    captures::controller_to_host(chip_id, packet, packet_type.into());

    let result = if let Some(transport) = get_manager().transports.read().unwrap().get(&chip_id) {
        transport.send(ResponsePacket { packet: packet.clone(), packet_type })
    } else {
        warn!("handle_response: chip {chip_id} not found");
        Ok(())
    };
    // transports lock is now released
    if let Err(e) = result {
        warn!("handle_response: error {e:?}");
        unregister_transport(chip_id);
    }
}

/// Handle requests from transports.
pub fn handle_request(chip_id: ChipIdentifier, packet: &Bytes, packet_type: u8) {
    captures::host_to_controller(chip_id, packet, packet_type.into());

    let mut packet_vec = packet.to_vec();
    // Prepend packet_type to packet if specified
    if PacketType::HCI_PACKET_UNSPECIFIED.value()
        != <u8 as std::convert::Into<i32>>::into(packet_type)
    {
        packet_vec.insert(0, packet_type);
    }

    // Perform handle_request
    match chip::get_chip(&chip_id) {
        Some(c) => c.wireless_chip.handle_request(&Bytes::from(packet_vec)),
        None => warn!("SharedWirelessChip doesn't exist for chip_id: {chip_id}"),
    }
}

/// Handle requests from gRPC transport in C++.
pub fn handle_request_cxx(chip_id: u32, packet: &cxx::CxxVector<u8>, packet_type: u8) {
    let packet_bytes = Bytes::from(packet.as_slice().to_vec());
    handle_request(ChipIdentifier(chip_id), &packet_bytes, packet_type);
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestTransport {}
    impl Response for TestTransport {
        fn response(&mut self, _packet: Bytes, _packet_type: u8) {}
    }

    #[test]
    fn test_register_transport() {
        let val: Box<dyn Response + Send> = Box::new(TestTransport {});
        let manager = PacketManager::new();
        let chip_id = ChipIdentifier(0);
        manager.register_transport(chip_id, val);
        {
            assert!(manager.transports.read().unwrap().contains_key(&chip_id));
        }
    }

    #[test]
    fn test_unregister_transport() {
        let manager = PacketManager::new();
        let chip_id = ChipIdentifier(1);
        manager.register_transport(chip_id, Box::new(TestTransport {}));
        manager.unregister_transport(chip_id);
        assert!(manager.transports.read().unwrap().get(&chip_id).is_none());
    }
}

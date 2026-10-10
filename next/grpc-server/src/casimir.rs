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

use std::{collections::HashMap, sync::Arc, time::Duration};

use grpcio::{RpcContext, RpcStatus, RpcStatusCode, UnarySink};
use netsim_proto::{
    casimir_control::{
        PowerLevel, RadioState, SendApduReply, SendApduRequest, SendBroadcastRequest,
        SendBroadcastResponse, SenderId, Void,
    },
    casimir_control_grpc::CasimirControlService,
};
use nfc_actor::{
    NfcClient,
    casimir::{RfReader, RfWriter, packets::rf},
};
use pdl_runtime::Packet;
use tokio::sync::{Mutex, mpsc, oneshot};
use tracing::{error, info, warn};

// Hex helpers using the hex crate
fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    let clean_s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    hex::decode(clean_s).map_err(|e| e.to_string())
}

fn hex_encode(bytes: &[u8]) -> String {
    hex::encode(bytes)
}

// CRC helpers (copied from Casimir crc.rs to avoid dependency modification)
fn crc16(data: &[u8], initial: u16, invert: bool) -> (u8, u8) {
    let mut w_crc: u16 = initial;
    for &byte in data {
        let mut temp: u16 = (byte as u16) ^ (w_crc & 0x00FF);
        temp = (temp ^ (temp << 4)) & 0xFF;
        w_crc = (w_crc >> 8) ^ (temp << 8) ^ (temp << 3) ^ (temp >> 4);
    }
    if invert {
        w_crc = !w_crc;
    }
    ((w_crc & 0xFF) as u8, ((w_crc >> 8) & 0xFF) as u8)
}

fn crc16a(data: &[u8]) -> (u8, u8) {
    crc16(data, 0x6363, false)
}

fn crc16b(data: &[u8]) -> (u8, u8) {
    crc16(data, 0xFFFF, true)
}

fn with_crc16a(mut data: Vec<u8>) -> Vec<u8> {
    let (lo, hi) = crc16a(&data);
    data.push(lo);
    data.push(hi);
    data
}

fn with_crc16b(mut data: Vec<u8>) -> Vec<u8> {
    let (lo, hi) = crc16b(&data);
    data.push(lo);
    data.push(hi);
    data
}

// Central reader task to dispatch incoming RF packets.
async fn run_reader(mut reader: RfReader, state: Arc<Mutex<ServiceState>>) {
    loop {
        match reader.read().await {
            Ok(bytes) => {
                if let Ok(packet) = rf::RfPacket::decode_full(&bytes) {
                    let mut state = state.lock().await;
                    // If we are waiting for ANY packet (for broadcast timeout), resolve it
                    if let Some(responder) = state.pending_any.take() {
                        let _ = responder.send(());
                    }
                    match packet.specialize() {
                        Ok(rf::RfPacketChild::Data(data)) => {
                            let sender = data.sender();
                            if let Some(responder) = state.pending_apdus.remove(&sender) {
                                let _ = responder.send(data);
                            } else {
                                info!("Received unexpected RF Data from {}", sender);
                            }
                        }
                        Ok(rf::RfPacketChild::NfcAPollResponse(poll_resp)) => {
                            let sender = poll_resp.sender();
                            if let Some(responder) = state.pending_poll_a.take() {
                                let _ = responder.send(sender);
                            } else {
                                info!("Received unexpected NfcAPollResponse from {}", sender);
                            }
                        }
                        Ok(rf::RfPacketChild::IsoDepT4ATSelectResponse(select_resp)) => {
                            let sender = select_resp.sender();
                            if let Some(responder) = state.pending_select.take() {
                                let _ = responder.send(());
                            } else {
                                info!(
                                    "Received unexpected IsoDepT4ATSelectResponse from {}",
                                    sender
                                );
                            }
                        }
                        _ => {}
                    }
                } else {
                    warn!("Failed to decode RF packet");
                }
            }
            Err(e) => {
                info!("RF Reader connection closed: {}", e);
                break;
            }
        }
    }
    // Cleanup state on exit
    let mut state = state.lock().await;
    state.tx = None;
    state.pending_apdus.clear();
    state.pending_poll_a = None;
    state.pending_select = None;
    state.pending_any = None;
}

// Central writer task to send RF packets.
async fn run_writer(mut writer: RfWriter, mut rx: mpsc::UnboundedReceiver<rf::RfPacket>) {
    while let Some(packet) = rx.recv().await {
        if let Ok(bytes) = packet.encode_to_vec() {
            if let Err(e) = writer.write(&bytes).await {
                error!("RF Writer error: {}", e);
                break;
            }
        } else {
            error!("Failed to encode RF packet");
        }
    }
}

struct ServiceState {
    tx: Option<mpsc::UnboundedSender<rf::RfPacket>>,
    device_id: u16, // Store the assigned device ID from the scene
    // Note: We store a single responder per receiver. This assumes the client calls
    // SendApdu sequentially (which Cuttlefish host test suites do).
    pending_apdus: HashMap<u16, oneshot::Sender<rf::Data>>,
    pending_poll_a: Option<oneshot::Sender<u16>>,
    pending_select: Option<oneshot::Sender<()>>,
    pending_any: Option<oneshot::Sender<()>>,
    power_level: u8,
    reader_task: Option<tokio::task::JoinHandle<()>>,
    writer_task: Option<tokio::task::JoinHandle<()>>,
}

#[derive(Clone)]
pub struct CasimirControlServiceImpl {
    nfc_client: NfcClient,
    state: Arc<Mutex<ServiceState>>,
    tokio_handle: tokio::runtime::Handle,
}

impl CasimirControlServiceImpl {
    pub fn new(nfc_client: NfcClient) -> Self {
        Self {
            nfc_client,
            state: Arc::new(Mutex::new(ServiceState {
                tx: None,
                device_id: 0, // Default to 0, will be overwritten on init()
                pending_apdus: HashMap::new(),
                pending_poll_a: None,
                pending_select: None,
                pending_any: None,
                power_level: 10, // Default power level
                reader_task: None,
                writer_task: None,
            })),
            tokio_handle: tokio::runtime::Handle::current(),
        }
    }

    async fn get_or_create_connection(
        &self,
    ) -> Result<mpsc::UnboundedSender<rf::RfPacket>, String> {
        let mut state = self.state.lock().await;
        if let Some(ref tx) = state.tx {
            return Ok(tx.clone());
        }

        info!("Establishing connection to in-process Casimir...");
        let (stream, device_id) = self
            .nfc_client
            .create_control_channel()
            .await
            .map_err(|e| format!("Failed to create control channel: {e}"))?;

        state.device_id = device_id;
        info!("Assigned Casimir Device ID: {}", device_id);

        let (reader_half, writer_half) = tokio::io::split(stream);
        let reader = RfReader::new(reader_half);
        let writer = RfWriter::new(writer_half);

        let (tx, rx) = mpsc::unbounded_channel();
        state.tx = Some(tx.clone());

        let state_clone = self.state.clone();
        let reader_handle = tokio::spawn(run_reader(reader, state_clone));
        let writer_handle = tokio::spawn(run_writer(writer, rx));
        state.reader_task = Some(reader_handle);
        state.writer_task = Some(writer_handle);

        Ok(tx)
    }

    // Internal poll helper (returns 0-based sender_id)
    async fn poll_internal(&self, tx: &mpsc::UnboundedSender<rf::RfPacket>) -> Result<u16, String> {
        let (poll_tx, poll_rx) = oneshot::channel();
        let (power_level, sender_id) = {
            let mut state = self.state.lock().await;
            state.pending_poll_a = Some(poll_tx);
            (state.power_level, state.device_id)
        };

        // 1. Send PollCommand (WUPA)
        let poll_cmd = rf::PollCommand {
            sender: sender_id, // Use assigned device ID!
            receiver: u16::MAX,
            technology: rf::Technology::NfcA,
            protocol: rf::Protocol::Undetermined,
            bitrate: rf::BitRate::BitRate106KbitS,
            power_level,
            format: rf::PollingFrameFormat::Short,
            payload: vec![0x52], // WUPA
        };

        let rf_packet =
            poll_cmd.try_into().map_err(|e| format!("Failed to serialize poll command: {e}"))?;

        tx.send(rf_packet).map_err(|e| format!("Failed to send poll command: {e}"))?;

        let remote_id = match tokio::time::timeout(Duration::from_secs(10), poll_rx).await {
            Ok(Ok(id)) => id,
            Ok(Err(_)) => return Err("Poll-A channel closed".to_string()),
            Err(_) => {
                self.state.lock().await.pending_poll_a = None;
                return Err("Poll-A timeout".to_string());
            }
        };

        // 2. Send IsoDepT4ATSelectCommand
        let (select_tx, select_rx) = oneshot::channel();
        let sender_id_val = {
            let mut state = self.state.lock().await;
            state.pending_select = Some(select_tx);
            state.device_id
        };

        let select_cmd = rf::IsoDepT4ATSelectCommand {
            sender: sender_id_val, // Use assigned device ID!
            receiver: remote_id,
            bitrate: rf::BitRate::BitRate106KbitS,
            power_level,
            param: 0x80, // FSD=256
        };

        let rf_packet = select_cmd
            .try_into()
            .map_err(|e| format!("Failed to serialize select command: {e}"))?;

        tx.send(rf_packet).map_err(|e| format!("Failed to send select command: {e}"))?;

        match tokio::time::timeout(Duration::from_secs(1), select_rx).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => return Err("T4AT Select channel closed".to_string()),
            Err(_) => {
                self.state.lock().await.pending_select = None;
                return Err("T4AT Select timeout".to_string());
            }
        };

        Ok(remote_id)
    }
}

impl CasimirControlService for CasimirControlServiceImpl {
    fn send_apdu(
        &mut self,
        _ctx: RpcContext,
        req: SendApduRequest,
        sink: UnarySink<SendApduReply>,
    ) {
        let self_clone = self.clone();
        let handle = self_clone.tokio_handle.clone();
        handle.spawn(async move {
            let tx = match self_clone.get_or_create_connection().await {
                Ok(tx) => tx,
                Err(e) => {
                    let _ = sink.fail(RpcStatus::with_message(RpcStatusCode::INTERNAL, e)).await;
                    return;
                }
            };

            // Parse input APDUs from hex strings
            let mut apdu_bytes = Vec::new();
            for apdu_hex in &req.apdu_hex_strings {
                match hex_decode(apdu_hex) {
                    Ok(bytes) => apdu_bytes.push(bytes),
                    Err(e) => {
                        let _ = sink
                            .fail(RpcStatus::with_message(
                                RpcStatusCode::INVALID_ARGUMENT,
                                format!("Failed to parse input APDU hex: {e}"),
                            ))
                            .await;
                        return;
                    }
                }
            }

            // Determine target ID (1-based in proto, 0-based internally)
            let target_id: u16 = if let Some(sender_id) = req.sender_id {
                if sender_id == 0 || sender_id > (u16::MAX as u32) + 1 {
                    let _ = sink
                        .fail(RpcStatus::with_message(
                            RpcStatusCode::INVALID_ARGUMENT,
                            format!(
                                "Invalid sender_id: {sender_id}. Must be between 1 and {}",
                                (u16::MAX as u32) + 1
                            ),
                        ))
                        .await;
                    return;
                }
                (sender_id - 1) as u16
            } else {
                // Auto-poll if sender_id is not provided
                match self_clone.poll_internal(&tx).await {
                    Ok(id) => id,
                    Err(e) => {
                        let _ = sink
                            .fail(RpcStatus::with_message(
                                RpcStatusCode::INTERNAL,
                                format!("Auto-poll failed: {e}"),
                            ))
                            .await;
                        return;
                    }
                }
            };

            let receiver_id = target_id;
            let mut response_hex_strings = Vec::new();

            for apdu in apdu_bytes {
                let (apdu_tx, apdu_rx) = oneshot::channel();
                let (power_level, sender_id) = {
                    let mut state = self_clone.state.lock().await;
                    state.pending_apdus.insert(receiver_id, apdu_tx);
                    (state.power_level, state.device_id)
                };

                let data_packet = rf::Data {
                    sender: sender_id, // Use assigned device ID!
                    receiver: receiver_id,
                    technology: rf::Technology::NfcA,
                    protocol: rf::Protocol::IsoDep,
                    bitrate: rf::BitRate::BitRate106KbitS,
                    power_level,
                    data: apdu,
                };

                let rf_packet = match data_packet.try_into() {
                    Ok(p) => p,
                    Err(e) => {
                        let _ = sink
                            .fail(RpcStatus::with_message(
                                RpcStatusCode::INTERNAL,
                                format!("Failed to serialize data packet: {e}"),
                            ))
                            .await;
                        return;
                    }
                };

                if let Err(e) = tx.send(rf_packet) {
                    let _ = sink
                        .fail(RpcStatus::with_message(RpcStatusCode::INTERNAL, e.to_string()))
                        .await;
                    return;
                }

                match tokio::time::timeout(Duration::from_secs(3), apdu_rx).await {
                    Ok(Ok(response_packet)) => {
                        response_hex_strings.push(hex_encode(&response_packet.data));
                    }
                    _ => {
                        self_clone.state.lock().await.pending_apdus.remove(&receiver_id);
                        let _ = sink
                            .fail(RpcStatus::with_message(
                                RpcStatusCode::DEADLINE_EXCEEDED,
                                "APDU response timeout".into(),
                            ))
                            .await;
                        return;
                    }
                }
            }

            let mut reply = SendApduReply::new();
            reply.response_hex_strings = response_hex_strings;
            let _ = sink.success(reply).await;
        });
    }

    fn poll_a(&mut self, _ctx: RpcContext, _req: Void, sink: UnarySink<SenderId>) {
        let self_clone = self.clone();
        let handle = self_clone.tokio_handle.clone();
        handle.spawn(async move {
            let tx = match self_clone.get_or_create_connection().await {
                Ok(tx) => tx,
                Err(e) => {
                    let _ = sink.fail(RpcStatus::with_message(RpcStatusCode::INTERNAL, e)).await;
                    return;
                }
            };

            match self_clone.poll_internal(&tx).await {
                Ok(sender_id) => {
                    let mut reply = SenderId::new();
                    // Workaround: 1-based ID
                    reply.sender_id = (sender_id as u32) + 1;
                    let _ = sink.success(reply).await;
                }
                Err(e) => {
                    let _ = sink.fail(RpcStatus::with_message(RpcStatusCode::INTERNAL, e)).await;
                }
            }
        });
    }

    fn set_radio_state(&mut self, _ctx: RpcContext, req: RadioState, sink: UnarySink<Void>) {
        let self_clone = self.clone();
        let handle = self_clone.tokio_handle.clone();
        handle.spawn(async move {
            let tx = match self_clone.get_or_create_connection().await {
                Ok(tx) => tx,
                Err(e) => {
                    let _ = sink.fail(RpcStatus::with_message(RpcStatusCode::INTERNAL, e)).await;
                    return;
                }
            };

            let (power_level, sender_id) = {
                let state = self_clone.state.lock().await;
                (state.power_level, state.device_id)
            };

            let field_status =
                if req.radio_on { rf::FieldStatus::FieldOn } else { rf::FieldStatus::FieldOff };

            let field_info = rf::FieldInfo {
                sender: sender_id, // Use assigned device ID!
                receiver: u16::MAX,
                technology: rf::Technology::Raw,
                protocol: rf::Protocol::Undetermined,
                bitrate: rf::BitRate::BitRate106KbitS,
                power_level,
                field_status,
            };

            let rf_packet = match field_info.try_into() {
                Ok(p) => p,
                Err(e) => {
                    let _ = sink
                        .fail(RpcStatus::with_message(
                            RpcStatusCode::INTERNAL,
                            format!("Failed to serialize field info: {e}"),
                        ))
                        .await;
                    return;
                }
            };

            if let Err(e) = tx.send(rf_packet) {
                let _ = sink
                    .fail(RpcStatus::with_message(RpcStatusCode::INTERNAL, e.to_string()))
                    .await;
                return;
            }

            let _ = sink.success(Void::new()).await;
        });
    }

    fn set_power_level(&mut self, _ctx: RpcContext, req: PowerLevel, sink: UnarySink<Void>) {
        let self_clone = self.clone();
        let handle = self_clone.tokio_handle.clone();
        handle.spawn(async move {
            {
                let mut state = self_clone.state.lock().await;
                state.power_level = ((req.power_level as f32) * 12.0 / 100.0).round() as u8;
            }
            let _ = sink.success(Void::new()).await;
        });
    }

    fn send_broadcast(
        &mut self,
        _ctx: RpcContext,
        req: SendBroadcastRequest,
        sink: UnarySink<SendBroadcastResponse>,
    ) {
        let self_clone = self.clone();
        let handle = self_clone.tokio_handle.clone();
        handle.spawn(async move {
            let tx = match self_clone.get_or_create_connection().await {
                Ok(tx) => tx,
                Err(e) => {
                    let _ = sink.fail(RpcStatus::with_message(RpcStatusCode::INTERNAL, e)).await;
                    return;
                }
            };

            // Default configuration
            let mut req_type = "A".to_string();
            let mut req_crc = true;
            let mut req_bits = 8;
            let mut req_bitrate = 106;
            let mut req_timeout = 0;
            let mut req_power = 100.0;

            // Overwrite defaults if configuration is present
            if let Some(config) = req.configuration.as_ref() {
                if let Some(t) = config.type_.as_ref() {
                    req_type = t.clone();
                }
                if let Some(c) = config.crc {
                    req_crc = c;
                }
                if let Some(b) = config.bits {
                    req_bits = b;
                }
                if let Some(br) = config.bitrate {
                    req_bitrate = br;
                }
                if let Some(to) = config.timeout {
                    req_timeout = to;
                }
                if let Some(p) = config.power {
                    req_power = p;
                }
            }

            let technology = match req_type.as_str() {
                "A" => rf::Technology::NfcA,
                "B" => rf::Technology::NfcB,
                "F" => rf::Technology::NfcF,
                "V" => rf::Technology::NfcV,
                _ => rf::Technology::Raw,
            };

            let bitrate = match req_bitrate {
                106 => rf::BitRate::BitRate106KbitS,
                212 => rf::BitRate::BitRate212KbitS,
                424 => rf::BitRate::BitRate424KbitS,
                848 => rf::BitRate::BitRate848KbitS,
                1695 => rf::BitRate::BitRate1695KbitS,
                3390 => rf::BitRate::BitRate3390KbitS,
                6780 => rf::BitRate::BitRate6780KbitS,
                26 => rf::BitRate::BitRate26KbitS,
                _ => {
                    let _ = sink
                        .fail(RpcStatus::with_message(
                            RpcStatusCode::INVALID_ARGUMENT,
                            "Invalid bitrate".into(),
                        ))
                        .await;
                    return;
                }
            };

            let format = if req_bits != 8 {
                rf::PollingFrameFormat::Short
            } else {
                rf::PollingFrameFormat::Long
            };

            // Adjust range of values from 0-100 to 0-12
            let power_level = (req_power * 12.0 / 100.0).round() as u8;

            // Decode data from hex
            let mut payload = match hex_decode(&req.data) {
                Ok(bytes) => bytes,
                Err(e) => {
                    let _ = sink
                        .fail(RpcStatus::with_message(
                            RpcStatusCode::INVALID_ARGUMENT,
                            format!("Failed to parse input data hex: {e}"),
                        ))
                        .await;
                    return;
                }
            };

            // CRC calculation
            if req_crc {
                match technology {
                    rf::Technology::NfcA => payload = with_crc16a(payload),
                    rf::Technology::NfcB => payload = with_crc16b(payload),
                    _ => {}
                }
            }

            let sender_id = {
                let state = self_clone.state.lock().await;
                state.device_id
            };

            let poll_cmd = rf::PollCommand {
                sender: sender_id, // Use assigned device ID!
                receiver: u16::MAX,
                technology,
                protocol: rf::Protocol::Undetermined,
                bitrate,
                power_level,
                format,
                payload,
            };

            let rf_packet = match poll_cmd.try_into() {
                Ok(p) => p,
                Err(e) => {
                    let _ = sink
                        .fail(RpcStatus::with_message(
                            RpcStatusCode::INTERNAL,
                            format!("Failed to serialize poll command: {e}"),
                        ))
                        .await;
                    return;
                }
            };

            let (any_tx, any_rx) = oneshot::channel();
            if req_timeout != 0 {
                let mut state = self_clone.state.lock().await;
                state.pending_any = Some(any_tx);
            }

            if let Err(e) = tx.send(rf_packet) {
                let _ = sink
                    .fail(RpcStatus::with_message(RpcStatusCode::INTERNAL, e.to_string()))
                    .await;
                return;
            }

            if req_timeout != 0 {
                let _ =
                    tokio::time::timeout(Duration::from_micros(req_timeout as u64), any_rx).await;
            }

            let _ = sink.success(SendBroadcastResponse::new()).await;
        });
    }

    fn init(&mut self, _ctx: RpcContext, _req: Void, sink: UnarySink<Void>) {
        let self_clone = self.clone();
        let handle = self_clone.tokio_handle.clone();
        handle.spawn(async move {
            match self_clone.get_or_create_connection().await {
                Ok(_) => {
                    let _ = sink.success(Void::new()).await;
                }
                Err(e) => {
                    let _ = sink.fail(RpcStatus::with_message(RpcStatusCode::INTERNAL, e)).await;
                }
            }
        });
    }

    fn close(&mut self, _ctx: RpcContext, _req: Void, sink: UnarySink<Void>) {
        let self_clone = self.clone();
        let handle = self_clone.tokio_handle.clone();
        handle.spawn(async move {
            let mut state = self_clone.state.lock().await;
            if let Some(task) = state.reader_task.take() {
                task.abort();
            }
            if let Some(task) = state.writer_task.take() {
                task.abort();
            }
            state.tx = None; // Drop tx, which will close the writer channel and exit tasks
            state.pending_apdus.clear();
            state.pending_poll_a = None;
            state.pending_select = None;
            state.pending_any = None;
            let _ = sink.success(Void::new()).await;
        });
    }
}

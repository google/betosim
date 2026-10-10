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

// devices_handler.rs
//
// Provides the API for the frontend and backend to interact with devices.
//
// The DeviceManager struct is a singleton for the devices collection.
//
// Additional functions are
// -- inactivity instant
// -- vending device identifiers

use super::chip;
use super::chip::ChipIdentifier;
use super::device::DeviceIdentifier;
use crate::devices::device::{AddChipResult, Device};
use crate::events::{
    ChipAdded, ChipRemoved, DeviceAdded, DevicePatched, DeviceRemoved, Event, Events, ShutDown,
};
use crate::http_server::server_response::ResponseWritable;
use crate::links::link::{LinkManager, PhyKind};
use crate::ranging;
use crate::wireless;
use http::Request;
use log::{info, warn};
use netsim_common::util::proto_print_options::JSON_PRINT_OPTION;
use netsim_proto::common::ChipKind as ProtoChipKind;
use netsim_proto::frontend::CreateDeviceRequest;
use netsim_proto::frontend::CreateDeviceResponse;
use netsim_proto::frontend::DeleteChipRequest;
use netsim_proto::frontend::ListDeviceResponse;
use netsim_proto::frontend::PatchDeviceRequest;
use netsim_proto::frontend::SubscribeDeviceRequest;
use netsim_proto::frontend::patch_device_request::PatchDeviceFields as ProtoPatchDeviceFields;
use netsim_proto::model::Chip as ProtoChip;
use netsim_proto::model::Device as ProtoDevice;
use netsim_proto::model::Orientation as ProtoOrientation;
use netsim_proto::model::PhyKind as ProtoPhyKind; // For ProtoPhyKind
use netsim_proto::model::Position as ProtoPosition;
use netsim_proto::model::chip_create::Chip as ProtoBuiltin;
use netsim_proto::startup::DeviceInfo as ProtoDeviceInfo;
use netsim_proto::stats::{NetsimDeviceStats as ProtoDeviceStats, NetsimRadioStats};
use protobuf::well_known_types::timestamp::Timestamp;
use protobuf::{Enum, MessageField};
use protobuf_json_mapping::merge_from_str;
use protobuf_json_mapping::print_to_string;
use protobuf_json_mapping::print_to_string_with_options;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// The amount of seconds netsimd will wait until the first device has attached.
static IDLE_SECS_FOR_SHUTDOWN: u64 = 15;

const INITIAL_DEVICE_ID: u32 = 1;

static POSE_MANAGER: OnceLock<Arc<PoseManager>> = OnceLock::new();

fn get_pose_manager() -> Arc<PoseManager> {
    POSE_MANAGER.get_or_init(|| Arc::new(PoseManager::new())).clone()
}

pub struct PoseManager {
    positions: RwLock<HashMap<DeviceIdentifier, ProtoPosition>>,
    orientations: RwLock<HashMap<DeviceIdentifier, ProtoOrientation>>,
}

impl PoseManager {
    pub fn new() -> Self {
        PoseManager {
            positions: RwLock::new(HashMap::new()),
            orientations: RwLock::new(HashMap::new()),
        }
    }

    pub fn add(&self, device_id: DeviceIdentifier) {
        self.positions.write().unwrap().insert(device_id, ProtoPosition::new());
        self.orientations.write().unwrap().insert(device_id, ProtoOrientation::new());
    }

    pub fn remove(&self, device_id: &DeviceIdentifier) {
        self.positions.write().unwrap().remove(device_id);
        self.orientations.write().unwrap().remove(device_id);
    }

    pub fn reset(&self, device_id: DeviceIdentifier) {
        self.positions.write().unwrap().insert(device_id, ProtoPosition::new());
        self.orientations.write().unwrap().insert(device_id, ProtoOrientation::new());
    }

    pub fn set_position(&self, device_id: DeviceIdentifier, position: &ProtoPosition) {
        self.positions.write().unwrap().insert(device_id, position.clone());
    }
    pub fn get_position(&self, device_id: &DeviceIdentifier) -> Option<ProtoPosition> {
        self.positions.read().unwrap().get(device_id).cloned()
    }
    pub fn set_orientation(&self, device_id: DeviceIdentifier, orientation: &ProtoOrientation) {
        self.orientations.write().unwrap().insert(device_id, orientation.clone());
    }
    pub fn get_orientation(&self, device_id: &DeviceIdentifier) -> Option<ProtoOrientation> {
        self.orientations.read().unwrap().get(device_id).cloned()
    }
}

static DEVICE_MANAGER: OnceLock<Arc<DeviceManager>> = OnceLock::new();

fn get_manager() -> Arc<DeviceManager> {
    DEVICE_MANAGER.get().unwrap().clone()
}

// TODO: last_modified atomic
/// The Device resource is a singleton that manages all devices.
pub struct DeviceManager {
    // BTreeMap allows ListDevice to output devices in order of identifiers.
    devices: RwLock<BTreeMap<DeviceIdentifier, Device>>,
    events: Arc<Events>,
    ids: AtomicU32,
    last_modified: RwLock<Duration>,
    pub link_manager: Arc<LinkManager>,
}

impl DeviceManager {
    pub fn init(events: Arc<Events>, link_manager: Arc<LinkManager>) -> Arc<DeviceManager> {
        let manager = Arc::new(Self::new(events, link_manager));
        if let Err(_e) = DEVICE_MANAGER.set(manager.clone()) {
            panic!("Error setting device manager");
        }
        manager
    }

    fn new(events: Arc<Events>, link_manager: Arc<LinkManager>) -> Self {
        DeviceManager {
            devices: RwLock::new(BTreeMap::new()),
            events,
            ids: AtomicU32::new(INITIAL_DEVICE_ID),
            last_modified: RwLock::new(
                SystemTime::now().duration_since(UNIX_EPOCH).expect("Time went backwards"),
            ),
            link_manager,
        }
    }

    fn next_id(&self) -> DeviceIdentifier {
        DeviceIdentifier(self.ids.fetch_add(1, Ordering::SeqCst))
    }

    fn update_timestamp(&self) {
        *self.last_modified.write().unwrap() =
            SystemTime::now().duration_since(UNIX_EPOCH).expect("Time went backwards");
    }

    /// Get or create a device.
    /// Returns a (device_id, device_name) pair.
    fn get_or_create_device(
        &self,
        guid: Option<&str>,
        name: Option<&str>,
        builtin: bool,
        device_info: ProtoDeviceInfo,
    ) -> (DeviceIdentifier, String) {
        // Hold a lock while checking and updating devices.
        let mut guard = self.devices.write().unwrap();
        // Check if a device with the same guid already exists and if so, return it
        if let Some(guid) = guid {
            if let Some(existing_device) = guard.values().find(|d| d.guid == *guid) {
                if existing_device.builtin != builtin {
                    warn!("builtin mismatch for device {} during add_chip", existing_device.name);
                }
                return (existing_device.id, existing_device.name.clone());
            }
        }
        // A new device needs to be created and inserted
        let id = self.next_id();
        let default = format!("device-{id}");
        let name = name.unwrap_or(&default);
        guard.insert(id, Device::new(id, guid.unwrap_or(&default), name, builtin));
        drop(guard);
        // Update last modified timestamp for devices
        self.update_timestamp();
        let device_stats = ProtoDeviceStats {
            device_id: Some(id.0),
            kind: Some(device_info.kind).filter(|s| !s.is_empty()),
            version: Some(device_info.version).filter(|s| !s.is_empty()),
            sdk_version: Some(device_info.sdk_version).filter(|s| !s.is_empty()),
            variant: Some(device_info.variant).filter(|s| !s.is_empty()),
            build_id: Some(device_info.build_id).filter(|s| !s.is_empty()),
            arch: Some(device_info.arch).filter(|s| !s.is_empty()),
            ..Default::default()
        };
        let event =
            Event::DeviceAdded(DeviceAdded { id, name: String::from(name), builtin, device_stats });
        self.events.publish(event);
        (id, String::from(name))
    }
}

/// Returns a Result<AddChipResult, String> after adding chip to resource.
/// add_chip is called by the transport layer when a new chip is attached.
///
/// The guid is a transport layer identifier for the device (host:port)
/// that is adding the chip.
///
/// TODO: Replace the parameter of add_chip with a single protobuf
pub fn add_chip(
    device_guid: &str,
    device_name: &str,
    chip_create_params: &chip::CreateParams,
    wireless_create_params: &wireless::CreateParam,
    device_info: ProtoDeviceInfo,
) -> Result<AddChipResult, String> {
    let chip_kind = chip_create_params.kind;
    let manager = get_manager();
    let (device_id, _) = manager.get_or_create_device(
        Some(device_guid),
        Some(device_name),
        chip_kind == ProtoChipKind::BLUETOOTH_BEACON,
        device_info,
    );
    get_pose_manager().add(device_id);

    // Create
    let chip_id = chip::next_id();
    let wireless_chip = wireless::add_chip(wireless_create_params, chip_id);

    // This is infrequent, so we can afford to do another lookup for the device.
    let _ = manager
        .devices
        .write()
        .unwrap()
        .get_mut(&device_id)
        .ok_or(format!("Device not found for device_id: {device_id}"))?
        .add_chip(chip_create_params, chip_id, wireless_chip);

    // Update last modified timestamp for devices
    manager.update_timestamp();

    // Update Capture resource
    let event = Event::ChipAdded(ChipAdded {
        chip_id,
        chip_kind,
        device_name: device_name.to_string(),
        builtin: chip_kind == ProtoChipKind::BLUETOOTH_BEACON,
    });
    manager.events.publish(event);
    Ok(AddChipResult { device_id, chip_id })
}

/// Remove a chip from a device.
///
/// Called when the packet transport for the chip shuts down.
pub fn remove_chip(device_id: DeviceIdentifier, chip_id: ChipIdentifier) -> Result<(), String> {
    let manager = get_manager();
    let mut guard = manager.devices.write().unwrap();
    let device =
        guard.get(&device_id).ok_or(format!("RemoveChip device id {device_id} not found"))?;
    // Delete any links associated with this chip before removing it
    manager.link_manager.delete_link_for_chip(chip_id);
    let radio_stats = device.remove_chip(&chip_id)?;

    let mut device_id_to_remove = None;
    if device.chips.read().unwrap().is_empty() {
        device_id_to_remove = Some(device_id);
        let device = guard
            .remove(&device_id)
            .ok_or(format!("RemoveChip device id {device_id} not found"))?;
        manager.events.publish(Event::DeviceRemoved(DeviceRemoved {
            id: device.id,
            name: device.name,
            builtin: device.builtin,
        }));
    }

    let remaining_nonbuiltin_devices = guard.values().filter(|device| !device.builtin).count();
    drop(guard);

    if let Some(device_id) = device_id_to_remove {
        get_pose_manager().remove(&device_id);
    }

    manager.events.publish(Event::ChipRemoved(ChipRemoved {
        chip_id,
        device_id,
        remaining_nonbuiltin_devices,
        radio_stats,
    }));

    manager.update_timestamp();
    Ok(())
}

pub fn delete_chip(request: &DeleteChipRequest) -> Result<(), String> {
    let chip_id = ChipIdentifier(request.id);

    let device_id = get_manager()
        .devices
        .read()
        .unwrap()
        .iter()
        .find(|(_, device)| device.chips.read().unwrap().contains_key(&chip_id))
        .map(|(id, _)| *id)
        .ok_or(format!("failed to delete chip: could not find chip with id {}", request.id))?;

    remove_chip(device_id, chip_id)
}

/// Create a device from a CreateDeviceRequest.
/// Uses a default name if none is provided.
/// Returns an error if the device already exists.
pub fn create_device(create_device_request: &CreateDeviceRequest) -> Result<ProtoDevice, String> {
    let new_device = &create_device_request.device;
    let manager = get_manager();
    // Check if specified device name is already mapped.
    if new_device.name != String::default()
        && manager.devices.read().unwrap().values().any(|d| d.guid == new_device.name)
    {
        return Err(String::from("failed to create device: device already exists"));
    }

    if new_device.chips.is_empty() {
        return Err(String::from("failed to create device: device must contain at least 1 chip"));
    }
    new_device.chips.iter().try_for_each(|chip| match chip.chip {
        Some(ProtoBuiltin::BleBeacon(_)) => Ok(()),
        Some(_) => Err(format!("failed to create device: chip {} was not a built-in", chip.name)),
        None => Err(format!("failed to create device: chip {} was missing a radio", chip.name)),
    })?;

    let device_name = (new_device.name != String::default()).then_some(new_device.name.as_str());
    let device_info =
        ProtoDeviceInfo { kind: "BLUETOOTH_BEACON".to_string(), ..Default::default() };
    let (device_id, device_name) =
        manager.get_or_create_device(device_name, device_name, true, device_info.clone());

    new_device.chips.iter().try_for_each(|chip| {
        {
            let chip_create_params = chip::CreateParams {
                kind: chip.kind.enum_value_or_default(),
                address: chip.address.clone(),
                name: if chip.name.is_empty() { None } else { Some(chip.name.to_string()) },
                manufacturer: chip.manufacturer.clone(),
                product_name: chip.product_name.clone(),
            };
            let wireless_create_params = wireless::wireless_manager::CreateParam::BleBeacon(
                wireless::ble_beacon::CreateParams {
                    device_name: device_name.clone(),
                    chip_proto: chip.clone(),
                },
            );

            add_chip(
                &device_name,
                &device_name,
                &chip_create_params,
                &wireless_create_params,
                device_info.clone(),
            )
        }
        .map(|_| ())
    })?;

    let manager = get_manager();
    let guard = manager.devices.read().unwrap();
    let device = guard.get(&device_id).expect("could not find test bluetooth beacon device");
    let device_proto = device.get(get_pose_manager())?;
    Ok(device_proto)
}

struct ProtoChipDisplay(ProtoChip);

// Due to the low readability of debug formatter for ProtoChip, we implemented our own fmt.
impl std::fmt::Display for ProtoChipDisplay {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let chip = &self.0;
        if let Ok(kind) = chip.kind.enum_value() {
            match kind {
                ProtoChipKind::BLUETOOTH => {
                    chip.bt().low_energy.clone().map(|v| {
                        write!(
                            f,
                            "{{ id: {}, kind: BLUETOOTH_LOW_ENERGY, state: {:?} }}",
                            self.0.id, v.state
                        )
                    });
                    chip.bt().classic.clone().map(|v| {
                        write!(
                            f,
                            "{{ id: {}, kind: BLUETOOTH_CLASSIC, state: {:?} }}",
                            chip.id, v.state
                        )
                    });
                }
                ProtoChipKind::BLUETOOTH_BEACON => {
                    chip.ble_beacon().bt.low_energy.clone().map(|v| {
                        write!(f, "{{ id: {}, kind: BLE_BEACON, state: {:?} }}", chip.id, v.state)
                    });
                    chip.ble_beacon().bt.classic.clone().map(|v| {
                        write!(
                            f,
                            "{{ id: {}, kind: BLUETOOTH_CLASSIC_BEACON, state: {:?} }}",
                            chip.id, v.state
                        )
                    });
                }
                ProtoChipKind::WIFI => {
                    write!(f, "{{ id: {}, kind: WIFI, state: {:?} }}", chip.id, chip.wifi().state)?
                }
                ProtoChipKind::UWB => {
                    write!(f, "{{ id: {}, kind: UWB, state: {:?} }}", chip.id, chip.uwb().state)?
                }
                _ => (),
            }
        }
        Ok(())
    }
}

struct PatchDeviceFieldsDisplay(DeviceIdentifier, ProtoPatchDeviceFields);

// Due to the low readability of debug formatter for ProtoPatchDeviceFields, we implemented our own fmt.
impl std::fmt::Display for PatchDeviceFieldsDisplay {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "PatchDevice: ")?;
        let mut fields = Vec::<String>::new();
        fields.push(format!("id: {}", self.0));
        if let Some(name) = &self.1.name {
            fields.push(format!("name: {name}"));
        }
        if let Some(visible) = &self.1.visible {
            fields.push(format!("visible: {visible}"));
        }
        if let Some(position) = &self.1.position.0 {
            fields.push(format!("position: {{ {position} }}"));
        }
        if let Some(orientation) = &self.1.orientation.0 {
            fields.push(format!("orientation: {{ {orientation} }}"));
        }
        if !self.1.chips.is_empty() {
            let mut chip_field = Vec::<String>::new();
            for chip in &self.1.chips {
                chip_field.push(format!("{}", ProtoChipDisplay(chip.clone())));
            }
            fields.push(format!("chips: {{ {} }}", chip_field.join(", ")));
        }
        write!(f, "{}", fields.join(", "))
    }
}

// lock the devices, find the id and call the patch function
pub fn patch_device(patch_device_request: PatchDeviceRequest) -> Result<(), String> {
    let manager = get_manager();
    let proto_device = patch_device_request
        .device
        .into_option()
        .ok_or("Missing PatchDevice in PatchDeviceRequest".to_string())?;
    match (patch_device_request.id, proto_device.name.clone()) {
        (Some(id), _) => {
            let id = DeviceIdentifier(id);
            match manager.devices.read().unwrap().get(&id) {
                Some(device) => {
                    let result = device.patch(&proto_device, get_pose_manager());
                    let name = device.name.clone();
                    if result.is_ok() {
                        // Update last modified timestamp for manager
                        manager.update_timestamp();

                        // Log patched fields
                        log::info!("{}", PatchDeviceFieldsDisplay(id, proto_device));

                        // Publish Device Patched event
                        manager.events.publish(Event::DevicePatched(DevicePatched { id, name }));
                    }
                    result
                }
                None => Err(format!("No such device with id {id}")),
            }
        }
        (_, Some(name)) => {
            let mut multiple_matches = false;
            let mut target: Option<&Device> = None;
            let devices = manager.devices.read().unwrap();
            for device in devices.values() {
                if device.name.contains(&name) {
                    if device.name == name {
                        let result = device.patch(&proto_device, get_pose_manager());
                        let id = device.id;
                        let name = device.name.clone();
                        if result.is_ok() {
                            // Update last modified timestamp for manager
                            manager.update_timestamp();

                            // Log patched fields
                            log::info!("{}", PatchDeviceFieldsDisplay(id, proto_device));

                            // Publish Device Patched event
                            manager
                                .events
                                .publish(Event::DevicePatched(DevicePatched { id, name }));
                        }
                        return result;
                    }
                    multiple_matches = target.is_some();
                    target = Some(device);
                }
            }
            if multiple_matches {
                return Err(format!("Multiple ambiguous matches were found with substring {name}"));
            }
            match target {
                Some(device) => {
                    let result = device.patch(&proto_device, get_pose_manager());
                    let id = device.id;
                    let name = device.name.clone();
                    if result.is_ok() {
                        // Update last modified timestamp for devices
                        manager.update_timestamp();

                        // Log patched fields
                        log::info!("{}", PatchDeviceFieldsDisplay(id, proto_device));

                        // Publish Device Patched event
                        manager.events.publish(Event::DevicePatched(DevicePatched { id, name }));
                    }
                    result
                }
                None => Err(format!("No such device with name {name}")),
            }
        }
        (_, _) => Err("Both id and name are not provided".to_string()),
    }
}

// Parse from input json string to proto
fn patch_device_json(id_option: Option<DeviceIdentifier>, patch_json: &str) -> Result<(), String> {
    let mut patch_device_request = PatchDeviceRequest::new();
    if merge_from_str(&mut patch_device_request, patch_json).is_ok() {
        if patch_device_request.id.is_none() {
            patch_device_request.id = id_option.map(|id| id.0);
        }
        patch_device(patch_device_request)
    } else {
        Err(format!("Incorrect format of patch json {patch_json}"))
    }
}

fn distance(a: &ProtoPosition, b: &ProtoPosition) -> f32 {
    ((b.x - a.x).powf(2.0) + (b.y - a.y).powf(2.0) + (b.z - a.z).powf(2.0)).sqrt()
}

/// Gets the position of a chip identified by its ID.
/// Returns None if the chip or its device/position cannot be found.
fn get_position(chip_id: ChipIdentifier) -> Option<ProtoPosition> {
    let chip = chip::get_chip(&chip_id).or_else(|| {
        warn!("get_position error for chip {}: No such device", chip_id.0);
        None
    })?;

    let pos = get_pose_manager().get_position(&chip.device_id).or_else(|| {
        warn!(
            "get_position error for chip {}: no position for device {}",
            chip_id.0, chip.device_id
        );
        None
    })?;

    Some(pos)
}

/// Calculates the distance between two chips identified by their IDs.
/// Returns 0.0 if either chip or its device/position cannot be found.
fn get_distance(a: u32, b: u32) -> f32 {
    // Calculate distance only if both positions were successfully found.
    match (get_position(ChipIdentifier(a)), get_position(ChipIdentifier(b))) {
        (Some(pos_a), Some(pos_b)) => distance(&pos_a, &pos_b),
        _ => 0.0, // Error already logged by get_position
    }
}

/// Function to obtain ProtoDevice given a ChipIdentifier
pub fn get_device(chip_id: &ChipIdentifier) -> anyhow::Result<netsim_proto::model::Device> {
    let device_id = match chip::get_chip(chip_id) {
        Some(chip) => chip.device_id,
        None => return Err(anyhow::anyhow!("Can't find chip for chip_id: {chip_id}")),
    };
    get_manager()
        .devices
        .read()
        .unwrap()
        .get(&device_id)
        .ok_or(anyhow::anyhow!("Can't find device for device_id: {device_id}"))?
        .get(get_pose_manager())
        .map_err(|e| anyhow::anyhow!("{e:?}"))
}

pub fn reset_all() -> Result<(), String> {
    let manager = get_manager();
    // Perform reset for all manager
    let mut device_ids = Vec::new();
    for device in manager.devices.read().unwrap().values() {
        device.reset()?;
        device_ids.push(device.id);
    }
    for device_id in device_ids {
        get_pose_manager().reset(device_id);
    }
    // Reset all links
    manager.link_manager.reset();
    // Update last modified timestamp for manager
    manager.update_timestamp();
    manager.events.publish(Event::DeviceReset);
    Ok(())
}

fn handle_device_create(writer: ResponseWritable, create_json: &str) {
    let mut response = CreateDeviceResponse::new();

    let mut get_result = || {
        let mut create_device_request = CreateDeviceRequest::new();
        merge_from_str(&mut create_device_request, create_json)
            .map_err(|_| format!("create device: invalid json: {create_json}"))?;
        let device_proto = create_device(&create_device_request)?;
        response.device = MessageField::some(device_proto);
        print_to_string(&response).map_err(|_| String::from("failed to convert device to json"))
    };

    match get_result() {
        Ok(response) => writer.put_ok("text/json", &response, vec![]),
        Err(err) => writer.put_error(404, err.as_str()),
    }
}

/// Performs PatchDevice to patch a single device
fn handle_device_patch(writer: ResponseWritable, id: Option<DeviceIdentifier>, patch_json: &str) {
    match patch_device_json(id, patch_json) {
        Ok(()) => writer.put_ok("text/plain", "Device Patch Success", vec![]),
        Err(err) => writer.put_error(404, err.as_str()),
    }
}

fn handle_chip_delete(writer: ResponseWritable, delete_json: &str) {
    let get_result = || {
        let mut delete_chip_request = DeleteChipRequest::new();
        merge_from_str(&mut delete_chip_request, delete_json)
            .map_err(|_| format!("delete chip: invalid json: {delete_json}"))?;
        delete_chip(&delete_chip_request)
    };

    match get_result() {
        Ok(()) => writer.put_ok("text/plain", "Chip Delete Success", vec![]),
        Err(err) => writer.put_error(404, err.as_str()),
    }
}

pub fn list_device() -> anyhow::Result<ListDeviceResponse, String> {
    // Instantiate ListDeviceResponse and add DeviceManager
    let mut response = ListDeviceResponse::new();
    let manager = get_manager();

    for device in manager.devices.read().unwrap().values() {
        if let Ok(device_proto) = device.get(get_pose_manager()) {
            response.devices.push(device_proto);
        }
    }

    // Add Last Modified Timestamp into ListDeviceResponse
    response.last_modified = Some(Timestamp {
        seconds: manager.last_modified.read().unwrap().as_secs() as i64,
        nanos: manager.last_modified.read().unwrap().subsec_nanos() as i32,
        ..Default::default()
    })
    .into();
    Ok(response)
}

/// Performs ListDevices to get the list of DeviceManager and write to writer.
fn handle_device_list(writer: ResponseWritable) {
    let response = list_device().unwrap();
    // Perform protobuf-json-mapping with the given protobuf
    if let Ok(json_response) = print_to_string_with_options(&response, &JSON_PRINT_OPTION) {
        writer.put_ok("text/json", &json_response, vec![])
    } else {
        writer.put_error(404, "proto to JSON mapping failure")
    }
}

/// Performs ResetDevice for all devices
fn handle_device_reset(writer: ResponseWritable) {
    match reset_all() {
        Ok(()) => writer.put_ok("text/plain", "Device Reset Success", vec![]),
        Err(err) => writer.put_error(404, err.as_str()),
    }
}

/// Performs SubscribeDevice
fn handle_device_subscribe(writer: ResponseWritable, subscribe_json: &str) {
    // Check if the provided last_modified timestamp is prior to the current last_modified
    let mut subscribe_device_request = SubscribeDeviceRequest::new();
    if merge_from_str(&mut subscribe_device_request, subscribe_json).is_ok() {
        let timestamp_proto = subscribe_device_request.last_modified;
        let provided_last_modified =
            Duration::new(timestamp_proto.seconds as u64, timestamp_proto.nanos as u32);
        if provided_last_modified < *get_manager().last_modified.read().unwrap() {
            info!("Immediate return for SubscribeDevice");
            handle_device_list(writer);
            return;
        }
    }

    let manager = get_manager();
    let event_rx = manager.events.subscribe();
    // Timeout after 15 seconds with no event received
    match event_rx.recv_timeout(Duration::from_secs(15)) {
        Ok(Event::DeviceAdded(_))
        | Ok(Event::DeviceRemoved(_))
        | Ok(Event::ChipAdded(_))
        | Ok(Event::ChipRemoved(_))
        | Ok(Event::DevicePatched(_))
        | Ok(Event::DeviceReset) => handle_device_list(writer),
        Err(err) => writer.put_error(404, format!("{err:?}").as_str()),
        _ => writer.put_error(404, "disconnecting due to unrelated event"),
    }
}

/// The Rust device handler used directly by Http frontend for LIST, GET, and PATCH
pub fn handle_device(request: &Request<Vec<u8>>, param: &str, writer: ResponseWritable) {
    // Route handling
    if request.uri() == "/v1/devices" {
        // Routes with ID not specified
        match request.method().as_str() {
            "GET" => {
                handle_device_list(writer);
            }
            "PUT" => {
                handle_device_reset(writer);
            }
            "SUBSCRIBE" => {
                let body = request.body();
                let subscribe_json = String::from_utf8(body.to_vec()).unwrap();
                handle_device_subscribe(writer, subscribe_json.as_str());
            }
            "PATCH" => {
                let body = request.body();
                let patch_json = String::from_utf8(body.to_vec()).unwrap();
                handle_device_patch(writer, None, patch_json.as_str());
            }
            "POST" => {
                let body = &request.body();
                let create_json = String::from_utf8(body.to_vec()).unwrap();
                handle_device_create(writer, create_json.as_str());
            }
            "DELETE" => {
                let body = &request.body();
                let delete_json = String::from_utf8(body.to_vec()).unwrap();
                handle_chip_delete(writer, delete_json.as_str());
            }
            _ => writer.put_error(404, "Not found."),
        }
    } else {
        // Routes with ID specified
        match request.method().as_str() {
            "PATCH" => {
                let id = match param.parse::<u32>() {
                    Ok(num) => DeviceIdentifier(num),
                    Err(_) => {
                        writer.put_error(404, "Incorrect Id type for devices, ID should be u32.");
                        return;
                    }
                };
                let body = request.body();
                let patch_json = String::from_utf8(body.to_vec()).unwrap();
                handle_device_patch(writer, Some(id), patch_json.as_str());
            }
            _ => writer.put_error(404, "Not found."),
        }
    }
}

/// return enum type for check_device_event
#[derive(Debug, PartialEq)]
enum DeviceWaitStatus {
    LastDeviceRemoved,
    DeviceAdded,
    Timeout,
    IgnoreEvent,
}

/// listening to events
fn check_device_event(
    events_rx: &Receiver<Event>,
    timeout_time: Option<Instant>,
) -> DeviceWaitStatus {
    let wait_time = timeout_time.map_or(Duration::from_secs(u64::MAX), |t| t - Instant::now());
    match events_rx.recv_timeout(wait_time) {
        Ok(Event::ChipRemoved(ChipRemoved { remaining_nonbuiltin_devices: 0, .. })) => {
            DeviceWaitStatus::LastDeviceRemoved
        }
        // DeviceAdded (event from CreateDevice)
        // ChipAdded (event from add_chip)
        Ok(Event::DeviceAdded(DeviceAdded { builtin: false, .. }))
        | Ok(Event::ChipAdded(ChipAdded { builtin: false, .. })) => DeviceWaitStatus::DeviceAdded,
        Err(_) => DeviceWaitStatus::Timeout,
        _ => DeviceWaitStatus::IgnoreEvent,
    }
}

/// wait loop logic for devices
/// the function will publish a ShutDown event when
/// 1. Initial timeout before first device is added
/// 2. Last Chip Removed from netsimd
///    this function should NOT be invoked if running in no-shutdown mode
pub fn spawn_shutdown_publisher(events_rx: Receiver<Event>, events: Arc<Events>) {
    spawn_shutdown_publisher_with_timeout(events_rx, IDLE_SECS_FOR_SHUTDOWN, events);
}

// separate function for testability
fn spawn_shutdown_publisher_with_timeout(
    events_rx: Receiver<Event>,
    timeout_duration_s: u64,
    events: Arc<Events>,
) {
    let _ =
        std::thread::Builder::new().name("device_event_subscriber".to_string()).spawn(move || {
            let mut timeout_time = Some(Instant::now() + Duration::from_secs(timeout_duration_s));
            loop {
                match check_device_event(&events_rx, timeout_time) {
                    DeviceWaitStatus::LastDeviceRemoved => {
                        events.publish(Event::ShutDown(ShutDown {
                            reason: "last device disconnected".to_string(),
                        }));
                        return;
                    }
                    DeviceWaitStatus::DeviceAdded => {
                        timeout_time = None;
                    }
                    DeviceWaitStatus::Timeout => {
                        events.publish(Event::ShutDown(ShutDown {
                            reason: format!(
                                "no devices connected within {IDLE_SECS_FOR_SHUTDOWN}s"
                            ),
                        }));
                        return;
                    }
                    DeviceWaitStatus::IgnoreEvent => continue,
                }
            }
        });
}

/// Return vector containing current radio chip stats from all devices
pub fn get_radio_stats() -> Vec<NetsimRadioStats> {
    let mut result: Vec<NetsimRadioStats> = Vec::new();
    // TODO: b/309805437 - optimize logic using get_stats for WirelessChip
    for (device_id, device) in get_manager().devices.read().unwrap().iter() {
        for chip in device.chips.read().unwrap().values() {
            for mut radio_stats in chip.get_stats() {
                radio_stats.set_device_id(device_id.0);
                result.push(radio_stats);
            }
        }
    }
    result
}

/// A GetRssi function for Rust Device API.
/// Checks for RSSI override settings before calculating based on distance.
/// The backend gRPC code will be invoking this method via FFI.
pub fn get_rssi(sender_id: u32, receiver_id: u32, link_kind_i32: i32, tx_power: i8) -> i8 {
    let proto_link_kind = ProtoPhyKind::from_i32(link_kind_i32).unwrap_or_default();
    let link_kind = match PhyKind::try_from(proto_link_kind) {
        Ok(kind) => kind,
        Err(e) => {
            warn!(
                "FFI: Error converting ProtoPhyKind {proto_link_kind:?} to internal: {e}. Defaulting to None."
            );
            PhyKind::None
        }
    };
    // Check for link RSSI setting first.
    if let Some(override_rssi) = get_manager().link_manager.get_rssi(
        ChipIdentifier(sender_id),
        ChipIdentifier(receiver_id),
        link_kind,
    ) {
        info!(
            "Using RSSI override for sender {sender_id} and receiver {receiver_id}: {override_rssi}",
        );
        return override_rssi;
    }

    // Fallback to distance calculation
    ranging::distance_to_rssi(tx_power, get_distance(sender_id, receiver_id))
}

#[cfg(test)]
mod tests {
    use http::Version;
    use netsim_common::util::netsim_logger::init_for_test;
    use netsim_proto::frontend::patch_device_request::PatchDeviceFields as ProtoPatchDeviceFields;
    use netsim_proto::model::{DeviceCreate as ProtoDeviceCreate, Orientation as ProtoOrientation};
    use protobuf_json_mapping::print_to_string;
    use std::{sync::Once, thread};

    use super::*;

    static TEST_DEVICE_KIND: &str = "TEST_DEVICE";

    // This allows Log init method to be invoked once when running all tests.
    static INIT: Once = Once::new();

    /// Module setup function that is only run once, even if called multiple times.
    fn module_setup() {
        INIT.call_once(|| {
            init_for_test();
            DeviceManager::init(Events::new(), LinkManager::new().into());
        });
    }

    /// TestChipParameters struct to invoke add_chip
    /// This struct contains parameters required to invoke add_chip.
    /// This will eventually be invoked by the facades.
    struct TestChipParameters {
        device_guid: String,
        device_name: String,
        chip_kind: ProtoChipKind,
        chip_name: String,
        chip_manufacturer: String,
        chip_product_name: String,
        device_info: ProtoDeviceInfo,
    }

    impl TestChipParameters {
        fn add_chip(&self) -> Result<AddChipResult, String> {
            let chip_create_params = chip::CreateParams {
                kind: self.chip_kind,
                address: "".to_string(),
                name: Some(self.chip_name.clone()),
                manufacturer: self.chip_manufacturer.clone(),
                product_name: self.chip_product_name.clone(),
            };
            let wireless_create_params =
                wireless::CreateParam::Mock(wireless::mocked::CreateParams {
                    chip_kind: self.chip_kind,
                });
            super::add_chip(
                &self.device_guid,
                &self.device_name,
                &chip_create_params,
                &wireless_create_params,
                self.device_info.clone(),
            )
        }

        fn get_or_create_device(&self) -> DeviceIdentifier {
            let manager = get_manager();
            manager
                .get_or_create_device(
                    Some(&self.device_guid),
                    Some(&self.device_name),
                    false,
                    self.device_info.clone(),
                )
                .0
        }
    }

    /// helper function for test cases to instantiate ProtoPosition
    fn new_position(x: f32, y: f32, z: f32) -> ProtoPosition {
        ProtoPosition { x, y, z, ..Default::default() }
    }

    fn new_orientation(yaw: f32, pitch: f32, roll: f32) -> ProtoOrientation {
        ProtoOrientation { yaw, pitch, roll, ..Default::default() }
    }

    fn test_chip_1_bt() -> TestChipParameters {
        TestChipParameters {
            device_guid: format!("guid-fs-1-{:?}", thread::current().id()),
            device_name: format!("test-device-name-1-{:?}", thread::current().id()),
            chip_kind: ProtoChipKind::BLUETOOTH,
            chip_name: "bt_chip_name".to_string(),
            chip_manufacturer: "netsim".to_string(),
            chip_product_name: "netsim_bt".to_string(),
            device_info: ProtoDeviceInfo {
                kind: TEST_DEVICE_KIND.to_string(),
                ..Default::default()
            },
        }
    }

    fn test_chip_1_wifi() -> TestChipParameters {
        TestChipParameters {
            device_guid: format!("guid-fs-1-{:?}", thread::current().id()),
            device_name: format!("test-device-name-1-{:?}", thread::current().id()),
            chip_kind: ProtoChipKind::WIFI,
            chip_name: "wifi_chip_name".to_string(),
            chip_manufacturer: "netsim".to_string(),
            chip_product_name: "netsim_wifi".to_string(),
            device_info: ProtoDeviceInfo {
                kind: TEST_DEVICE_KIND.to_string(),
                ..Default::default()
            },
        }
    }

    fn test_chip_2_bt() -> TestChipParameters {
        TestChipParameters {
            device_guid: format!("guid-fs-2-{:?}", thread::current().id()),
            device_name: format!("test-device-name-2-{:?}", thread::current().id()),
            chip_kind: ProtoChipKind::BLUETOOTH,
            chip_name: "bt_chip_name".to_string(),
            chip_manufacturer: "netsim".to_string(),
            chip_product_name: "netsim_bt".to_string(),
            device_info: ProtoDeviceInfo {
                kind: TEST_DEVICE_KIND.to_string(),
                ..Default::default()
            },
        }
    }

    fn reset(id: DeviceIdentifier) -> Result<(), String> {
        get_pose_manager().reset(id);

        let manager = get_manager();
        let mut devices = manager.devices.write().unwrap();
        match devices.get_mut(&id) {
            Some(device) => device.reset(),
            None => Err(format!("No such device with id {id}")),
        }
    }

    fn spawn_shutdown_publisher_test_setup(timeout: u64) -> (Arc<Events>, Receiver<Event>) {
        let events = Events::new();
        let events_rx = events.subscribe();
        spawn_shutdown_publisher_with_timeout(events_rx, timeout, events.clone());

        let events_rx2 = events.subscribe();

        (events, events_rx2)
    }

    #[test]
    fn test_spawn_shutdown_publisher_last_chip_removed() {
        let (events, events_rx) = spawn_shutdown_publisher_test_setup(IDLE_SECS_FOR_SHUTDOWN);

        events.publish(Event::ChipRemoved(ChipRemoved {
            remaining_nonbuiltin_devices: 0,
            ..Default::default()
        }));

        // receive our own ChipRemoved
        assert!(matches!(events_rx.recv(), Ok(Event::ChipRemoved(ChipRemoved { .. }))));
        // receive the ShutDown emitted by the function under test
        assert!(matches!(events_rx.recv(), Ok(Event::ShutDown(ShutDown { .. }))));
    }

    #[test]
    fn test_spawn_shutdown_publisher_chip_removed_which_is_not_last_chip() {
        let (events, events_rx) = spawn_shutdown_publisher_test_setup(IDLE_SECS_FOR_SHUTDOWN);
        events.publish(Event::ChipRemoved(ChipRemoved {
            chip_id: ChipIdentifier(1),
            remaining_nonbuiltin_devices: 1,
            ..Default::default()
        }));

        // give other thread time to generate a ShutDown if it was going to
        std::thread::sleep(std::time::Duration::from_secs(1));

        // only the 2nd ChipRemoved should generate a ShutDown as it is marked the last one
        events.publish(Event::ChipRemoved(ChipRemoved {
            chip_id: ChipIdentifier(0),
            remaining_nonbuiltin_devices: 0,
            ..Default::default()
        }));

        // receive our own ChipRemoved
        assert!(matches!(events_rx.recv(), Ok(Event::ChipRemoved(ChipRemoved { .. }))));
        // receive our own ChipRemoved (with no shutdown)
        assert!(matches!(events_rx.recv(), Ok(Event::ChipRemoved(ChipRemoved { .. }))));
        // only then receive the ShutDown emitted by the function under test
        assert!(matches!(events_rx.recv(), Ok(Event::ShutDown(ShutDown { .. }))));
    }

    #[test]
    fn test_spawn_shutdown_publisher_last_chip_removed_with_duplicate_event() {
        let (events, events_rx) = spawn_shutdown_publisher_test_setup(IDLE_SECS_FOR_SHUTDOWN);
        events.publish(Event::ChipRemoved(ChipRemoved {
            chip_id: ChipIdentifier(0),
            remaining_nonbuiltin_devices: 0,
            ..Default::default()
        }));

        // give other thread time to generate a ShutDown if it was going to
        std::thread::sleep(std::time::Duration::from_secs(1));

        // this is a duplicate event and we already sent that all chips were removed
        // this is for strict comparison with test_spawn_shutdown_publisher_chip_removed_which_is_not_last_chip
        // to validate that if the first event has remaining_nonbuiltin_devices 0
        // we would receive ChipRemoved, ShutDown, ChipRemoved
        // but if first ChipRemoved has remaining_nonbuiltin_devices,
        // we instead receive ChipRemoved, ChipRemoved, ShutDown
        events.publish(Event::ChipRemoved(ChipRemoved {
            chip_id: ChipIdentifier(0),
            remaining_nonbuiltin_devices: 0,
            ..Default::default()
        }));

        // receive our own ChipRemoved
        assert!(matches!(events_rx.recv(), Ok(Event::ChipRemoved(_))));
        // receive the ShutDown emitted by the function under test
        assert!(matches!(events_rx.recv(), Ok(Event::ShutDown(_))));
        // receive our own erroneous ChipRemoved which occurs after we said all chips were removed
        // this is just for strict comparison with test_spawn_shutdown_publisher_chip_removed_which_is_not_last_chip
        assert!(matches!(events_rx.recv(), Ok(Event::ChipRemoved(_))));
        // should timeout now (no further events as we expect shutdown publisher thread to have stopped)
        assert!(events_rx.recv_timeout(Duration::from_secs(2)).is_err());
    }

    #[test]
    fn test_spawn_shutdown_publisher_timeout() {
        let (_, events_rx) = spawn_shutdown_publisher_test_setup(1u64);

        // receive the ShutDown emitted by the function under test
        assert!(matches!(events_rx.recv_timeout(Duration::from_secs(2)), Ok(Event::ShutDown(_))));
    }

    #[test]
    fn test_spawn_shutdown_publisher_timeout_is_canceled_if_a_chip_is_added() {
        let (events, events_rx) = spawn_shutdown_publisher_test_setup(1u64);

        events.publish(Event::ChipAdded(ChipAdded {
            chip_id: ChipIdentifier(0),
            chip_kind: ProtoChipKind::BLUETOOTH,
            ..Default::default()
        }));
        assert!(matches!(events_rx.recv(), Ok(Event::ChipAdded(_))));

        // should NO longer receive the ShutDown emitted by the function under test
        // based on timeout removed when chip added
        assert!(events_rx.recv_timeout(Duration::from_secs(2)).is_err());

        events.publish(Event::ChipRemoved(ChipRemoved {
            chip_id: ChipIdentifier(0),
            remaining_nonbuiltin_devices: 0,
            ..Default::default()
        }));
        // receive our own ChipRemoved
        assert!(matches!(events_rx.recv(), Ok(Event::ChipRemoved(_))));
        // receive the ShutDown emitted by the function under test
        assert!(matches!(events_rx.recv(), Ok(Event::ShutDown(_))));
    }

    #[test]
    fn test_distance() {
        // Pythagorean quadruples
        let a = new_position(0.0, 0.0, 0.0);
        let mut b = new_position(1.0, 2.0, 2.0);
        assert_eq!(distance(&a, &b), 3.0);
        b = new_position(2.0, 3.0, 6.0);
        assert_eq!(distance(&a, &b), 7.0);
    }

    #[test]
    fn test_add_chip() {
        module_setup();

        let manager = get_manager();

        // Adding a chip
        let chip_params = test_chip_1_bt();
        let chip_result = chip_params.add_chip().unwrap();
        match manager.devices.read().unwrap().get(&chip_result.device_id) {
            Some(device) => {
                let chips = device.chips.read().unwrap();
                let chip = chips.get(&chip_result.chip_id).unwrap();
                assert_eq!(chip_params.chip_kind, chip.kind);
                assert_eq!(
                    chip_params.chip_manufacturer,
                    chip.manufacturer.read().unwrap().to_string()
                );
                assert_eq!(chip_params.chip_name, chip.name);
                assert_eq!(
                    chip_params.chip_product_name,
                    chip.product_name.read().unwrap().to_string()
                );
                assert_eq!(chip_params.device_name, device.name);
            }
            None => unreachable!(),
        };
    }

    #[test]
    fn test_get_or_create_device() {
        module_setup();

        let manager = get_manager();

        // Creating a device and getting device
        let bt_chip_params = test_chip_1_bt();
        let device_id_1 = bt_chip_params.get_or_create_device();
        let wifi_chip_params = test_chip_1_wifi();
        let device_id_2 = wifi_chip_params.get_or_create_device();
        assert_eq!(device_id_1, device_id_2);
        assert!(manager.devices.read().unwrap().get(&device_id_1).is_some())
    }

    #[test]
    fn test_patch_device_json() {
        module_setup();

        // Patching device position and orientation by id
        let chip_params = test_chip_1_bt();
        let chip_result = chip_params.add_chip().unwrap();
        let mut patch_device_request = PatchDeviceRequest::new();
        let mut proto_device = ProtoPatchDeviceFields::new();
        let request_position = new_position(1.1, 2.2, 3.3);
        let request_orientation = new_orientation(4.4, 5.5, 6.6);
        proto_device.name = Some(chip_params.device_name);
        proto_device.visible = Some(false);
        proto_device.position = Some(request_position.clone()).into();
        proto_device.orientation = Some(request_orientation.clone()).into();
        patch_device_request.device = Some(proto_device.clone()).into();
        let patch_json = print_to_string(&patch_device_request).unwrap();
        patch_device_json(Some(chip_result.device_id), patch_json.as_str()).unwrap();
        match get_manager().devices.read().unwrap().get(&chip_result.device_id) {
            Some(device) => {
                assert!(!device.visible.load(Ordering::SeqCst));
            }
            None => unreachable!(),
        }

        match get_pose_manager().get_position(&chip_result.device_id) {
            Some(position) => {
                assert_eq!(position.x, request_position.x);
                assert_eq!(position.y, request_position.y);
                assert_eq!(position.z, request_position.z);
            }
            None => unreachable!(),
        }

        match get_pose_manager().get_orientation(&chip_result.device_id) {
            Some(orientation) => {
                assert_eq!(orientation.yaw, request_orientation.yaw);
                assert_eq!(orientation.pitch, request_orientation.pitch);
                assert_eq!(orientation.roll, request_orientation.roll);
            }
            None => unreachable!(),
        }

        // Patch device by name with substring match
        proto_device.name = format!("test-device-name-1-{:?}", thread::current().id()).into();
        patch_device_request.device = Some(proto_device).into();
        let patch_json = print_to_string(&patch_device_request).unwrap();
        assert!(patch_device_json(None, patch_json.as_str()).is_ok());
    }

    #[test]
    fn test_patch_error() {
        module_setup();

        // Patch Error Testing
        let bt_chip_params = test_chip_1_bt();
        let bt_chip2_params = test_chip_2_bt();
        let bt_chip_result = bt_chip_params.add_chip().unwrap();
        bt_chip2_params.add_chip().unwrap();

        // Incorrect value type
        let error_json = format!(
            "{{\"device\": {{\"name\": \"test-device-name-1-{:?}\", \"position\": 1.1}}}}",
            thread::current().id()
        );
        let patch_result = patch_device_json(Some(bt_chip_result.device_id), error_json.as_str());
        assert!(patch_result.is_err());
        assert_eq!(
            patch_result.unwrap_err(),
            format!("Incorrect format of patch json {error_json}")
        );

        // Incorrect key
        let error_json = format!(
            "{{\"device\": {{\"name\": \"test-device-name-1-{:?}\", \"hello\": \"world\"}}}}",
            thread::current().id()
        );
        let patch_result = patch_device_json(Some(bt_chip_result.device_id), error_json.as_str());
        assert!(patch_result.is_err());
        assert_eq!(
            patch_result.unwrap_err(),
            format!("Incorrect format of patch json {error_json}")
        );

        // Incorrect Id
        let error_json = r#"{"device": {"name": "test-device-name-1"}}"#;
        let patch_result =
            patch_device_json(Some(DeviceIdentifier(INITIAL_DEVICE_ID - 1)), error_json);
        assert!(patch_result.is_err());
        assert_eq!(
            patch_result.unwrap_err(),
            format!("No such device with id {}", INITIAL_DEVICE_ID - 1)
        );

        // Incorrect name
        let error_json = r#"{"device": {"name": "wrong-name"}}"#;
        let patch_result = patch_device_json(None, error_json);
        assert!(patch_result.is_err());
        assert_eq!(patch_result.unwrap_err(), "No such device with name wrong-name");

        // Multiple ambiguous matching
        let error_json = r#"{"device": {"name": "test-device"}}"#;
        let patch_result = patch_device_json(None, error_json);
        assert!(patch_result.is_err());
        assert_eq!(
            patch_result.unwrap_err(),
            "Multiple ambiguous matches were found with substring test-device"
        );
    }

    #[test]
    fn test_adding_two_chips() {
        module_setup();
        let manager = get_manager();

        // Adding two chips of the same device
        let bt_chip_params = test_chip_1_bt();
        let wifi_chip_params = test_chip_1_wifi();
        let bt_chip_result = bt_chip_params.add_chip().unwrap();
        let wifi_chip_result = wifi_chip_params.add_chip().unwrap();
        assert_eq!(bt_chip_result.device_id, wifi_chip_result.device_id);
        let devices = manager.devices.read().unwrap();
        let device = devices.get(&bt_chip_result.device_id).unwrap();
        assert_eq!(device.id, bt_chip_result.device_id);
        assert_eq!(device.name, bt_chip_params.device_name);
        assert_eq!(device.chips.read().unwrap().len(), 2);
        for chip in device.chips.read().unwrap().values() {
            assert!(chip.id == bt_chip_result.chip_id || chip.id == wifi_chip_result.chip_id);
            if chip.id == bt_chip_result.chip_id {
                assert_eq!(chip.kind, ProtoChipKind::BLUETOOTH);
            } else if chip.id == wifi_chip_result.chip_id {
                assert_eq!(chip.kind, ProtoChipKind::WIFI);
            } else {
                unreachable!();
            }
        }
    }

    #[test]
    fn test_reset() {
        module_setup();
        let manager = get_manager();

        // Patching Device and Resetting scene
        let chip_params = test_chip_1_bt();
        let chip_result = chip_params.add_chip().unwrap();
        let mut patch_device_request = PatchDeviceRequest::new();
        let mut proto_device = ProtoPatchDeviceFields::new();
        let request_position = new_position(10.0, 20.0, 30.0);
        let request_orientation = new_orientation(1.0, 2.0, 3.0);
        proto_device.name = Some(chip_params.device_name);
        proto_device.visible = Some(false);
        proto_device.position = Some(request_position).into();
        proto_device.orientation = Some(request_orientation).into();
        patch_device_request.device = Some(proto_device).into();
        patch_device_json(
            Some(chip_result.device_id),
            print_to_string(&patch_device_request).unwrap().as_str(),
        )
        .unwrap();
        match manager.devices.read().unwrap().get(&chip_result.device_id) {
            Some(device) => {
                assert!(!device.visible.load(Ordering::SeqCst));
            }
            None => unreachable!(),
        }

        match get_pose_manager().get_position(&chip_result.device_id) {
            Some(position) => {
                assert_eq!(position.x, 10.0);
            }
            None => unreachable!(),
        }

        match get_pose_manager().get_orientation(&chip_result.device_id) {
            Some(orientation) => {
                assert_eq!(orientation.yaw, 1.0);
            }
            None => unreachable!(),
        }

        reset(chip_result.device_id).unwrap();
        match manager.devices.read().unwrap().get(&chip_result.device_id) {
            Some(device) => {
                assert!(device.visible.load(Ordering::SeqCst));
            }
            None => unreachable!(),
        }

        match get_pose_manager().get_position(&chip_result.device_id) {
            Some(position) => {
                assert_eq!(position.x, 0.0);
                assert_eq!(position.y, 0.0);
                assert_eq!(position.z, 0.0);
            }
            None => unreachable!(),
        }

        match get_pose_manager().get_orientation(&chip_result.device_id) {
            Some(orientation) => {
                assert_eq!(orientation.yaw, 0.0);
                assert_eq!(orientation.pitch, 0.0);
                assert_eq!(orientation.roll, 0.0);
            }
            None => unreachable!(),
        }
    }

    #[test]
    fn test_remove_chip() {
        module_setup();
        let manager = get_manager();

        // Add 2 chips of same device and 1 chip of different device
        let bt_chip_params = test_chip_1_bt();
        let wifi_chip_params = test_chip_1_wifi();
        let bt_chip_2_params = test_chip_2_bt();
        let bt_chip_result = bt_chip_params.add_chip().unwrap();
        let wifi_chip_result = wifi_chip_params.add_chip().unwrap();
        let bt_chip_2_result = bt_chip_2_params.add_chip().unwrap();

        // Remove a bt chip of first device
        remove_chip(bt_chip_result.device_id, bt_chip_result.chip_id).unwrap();
        match manager.devices.read().unwrap().get(&bt_chip_result.device_id) {
            Some(device) => {
                assert_eq!(device.chips.read().unwrap().len(), 1);
                assert_eq!(
                    device.chips.read().unwrap().get(&wifi_chip_result.chip_id).unwrap().kind,
                    ProtoChipKind::WIFI
                );
            }
            None => unreachable!(),
        }

        // Remove a wifi chip of first device
        remove_chip(wifi_chip_result.device_id, wifi_chip_result.chip_id).unwrap();
        assert!(!manager.devices.read().unwrap().contains_key(&wifi_chip_result.device_id));

        // Remove a bt chip of second device
        remove_chip(bt_chip_2_result.device_id, bt_chip_2_result.chip_id).unwrap();
        assert!(!manager.devices.read().unwrap().contains_key(&bt_chip_2_result.device_id));
    }

    #[test]
    fn test_remove_chip_error() {
        module_setup();
        let manager = get_manager();

        // Add 2 chips of same device and 1 chip of different device
        let bt_chip_params = test_chip_1_bt();
        let bt_chip_result = bt_chip_params.add_chip().unwrap();

        // Invoke remove_chip with incorrect chip_id.
        match remove_chip(bt_chip_result.device_id, ChipIdentifier(9999)) {
            Ok(_) => unreachable!(),
            Err(err) => assert_eq!(err, "RemoveChip chip id 9999 not found"),
        }

        // Invoke remove_chip with incorrect device_id
        match remove_chip(DeviceIdentifier(9999), bt_chip_result.chip_id) {
            Ok(_) => unreachable!(),
            Err(err) => assert_eq!(err, "RemoveChip device id 9999 not found"),
        }
        assert!(manager.devices.read().unwrap().contains_key(&bt_chip_result.device_id));
    }

    #[test]
    fn test_get_distance() {
        module_setup();

        // Add 2 chips of different devices
        let bt_chip_params = test_chip_1_bt();
        let bt_chip_2_params = test_chip_2_bt();
        let bt_chip_result = bt_chip_params.add_chip().unwrap();
        let bt_chip_2_result = bt_chip_2_params.add_chip().unwrap();

        // Patch the first chip
        let mut patch_device_request = PatchDeviceRequest::new();
        let mut proto_device = ProtoPatchDeviceFields::new();
        let request_position = new_position(1.0, 1.0, 1.0);
        proto_device.name = Some(bt_chip_params.device_name);
        proto_device.position = Some(request_position.clone()).into();
        patch_device_request.device = Some(proto_device.clone()).into();
        let patch_json = print_to_string(&patch_device_request).unwrap();
        patch_device_json(Some(bt_chip_result.device_id), patch_json.as_str()).unwrap();

        // Patch the second chip
        let mut patch_device_request = PatchDeviceRequest::new();
        let mut proto_device = ProtoPatchDeviceFields::new();
        let request_position = new_position(1.0, 4.0, 5.0);
        proto_device.name = Some(bt_chip_2_params.device_name);
        proto_device.position = Some(request_position.clone()).into();
        patch_device_request.device = Some(proto_device.clone()).into();
        let patch_json = print_to_string(&patch_device_request).unwrap();
        patch_device_json(Some(bt_chip_2_result.device_id), patch_json.as_str()).unwrap();

        // Verify the get_distance performs the correct computation of
        // sqrt((1-1)**2 + (4-1)**2 + (5-1)**2)
        assert_eq!(5.0, get_distance(bt_chip_result.chip_id.0, bt_chip_2_result.chip_id.0))
    }

    #[allow(dead_code)]
    fn list_request() -> Request<Vec<u8>> {
        Request::builder()
            .method("GET")
            .uri("/v1/devices")
            .version(Version::HTTP_11)
            .body(Vec::<u8>::new())
            .unwrap()
    }

    use netsim_proto::model::Chip as ChipProto;
    use netsim_proto::model::ChipCreate as ProtoChipCreate;
    use netsim_proto::model::chip::{
        BleBeacon, Chip, ble_beacon::AdvertiseData, ble_beacon::AdvertiseSettings,
    };
    use netsim_proto::model::chip_create::{BleBeaconCreate, Chip as BuiltChipProto};
    use protobuf::{EnumOrUnknown, MessageField};

    fn get_test_create_device_request(device_name: Option<String>) -> CreateDeviceRequest {
        let beacon_proto = BleBeaconCreate {
            settings: MessageField::some(AdvertiseSettings { ..Default::default() }),
            adv_data: MessageField::some(AdvertiseData { ..Default::default() }),
            ..Default::default()
        };

        let chip_proto = ProtoChipCreate {
            name: String::from("test-beacon-chip"),
            kind: ProtoChipKind::BLUETOOTH_BEACON.into(),
            chip: Some(BuiltChipProto::BleBeacon(beacon_proto)),
            ..Default::default()
        };

        let device_proto = ProtoDeviceCreate {
            name: device_name.unwrap_or_default(),
            chips: vec![chip_proto],
            ..Default::default()
        };

        CreateDeviceRequest { device: MessageField::some(device_proto), ..Default::default() }
    }

    #[test]
    fn test_create_device_succeeds() {
        module_setup();

        let request = get_test_create_device_request(Some(format!(
            "bob-the-beacon-{:?}",
            thread::current().id()
        )));

        let device_proto = create_device(&request);
        assert!(device_proto.is_ok());
        let device_proto = device_proto.unwrap();
        assert_eq!(request.device.name, device_proto.name);
        assert_eq!(1, device_proto.chips.len());
        assert_eq!(request.device.chips[0].name, device_proto.chips[0].name);
    }

    #[test]
    fn test_create_chipless_device_fails() {
        module_setup();

        let request = CreateDeviceRequest {
            device: MessageField::some(ProtoDeviceCreate { ..Default::default() }),
            ..Default::default()
        };

        let device_proto = create_device(&request);
        assert!(device_proto.is_err(), "{}", device_proto.unwrap());
    }

    #[test]
    fn test_create_radioless_device_fails() {
        module_setup();

        let request = CreateDeviceRequest {
            device: MessageField::some(ProtoDeviceCreate {
                chips: vec![ProtoChipCreate::default()],
                ..Default::default()
            }),
            ..Default::default()
        };

        let device_proto = create_device(&request);
        assert!(device_proto.is_err(), "{}", device_proto.unwrap());
    }

    #[test]
    fn test_get_beacon_device() {
        module_setup();

        let request = get_test_create_device_request(Some(format!(
            "bob-the-beacon-{:?}",
            thread::current().id()
        )));

        let device_proto = create_device(&request);
        assert!(device_proto.is_ok(), "{}", device_proto.unwrap_err());
        let device_proto = device_proto.unwrap();
        assert_eq!(1, device_proto.chips.len());
        assert!(device_proto.chips[0].chip.is_some());
        assert!(matches!(device_proto.chips[0].chip, Some(Chip::BleBeacon(_))));
    }

    #[test]
    fn test_create_device_default_name() {
        module_setup();

        let request = get_test_create_device_request(None);

        let device_proto = create_device(&request);
        assert!(device_proto.is_ok(), "{}", device_proto.unwrap_err());
        let device_proto = device_proto.unwrap();
        assert_eq!(format!("device-{}", device_proto.id), device_proto.name);
    }

    #[test]
    fn test_create_existing_device_fails() {
        module_setup();

        let request = get_test_create_device_request(Some(format!(
            "existing-device-{:?}",
            thread::current().id()
        )));

        let device_proto = create_device(&request);
        assert!(device_proto.is_ok(), "{}", device_proto.unwrap_err());

        // Attempt to create the device again. This should fail because the devices have the same name.
        let device_proto = create_device(&request);
        assert!(device_proto.is_err());
    }

    #[test]
    fn test_patch_beacon_device() {
        module_setup();
        let manager = get_manager();

        let request = get_test_create_device_request(Some(format!(
            "bob-the-beacon-{:?}",
            thread::current().id()
        )));

        let device_proto = create_device(&request);
        assert!(device_proto.is_ok(), "{}", device_proto.unwrap_err());
        let device_proto = device_proto.unwrap();
        let mut devices = manager.devices.write().unwrap();
        let device = devices
            .get_mut(&DeviceIdentifier(device_proto.id))
            .expect("could not find test bluetooth beacon device");
        let patch_result = device.patch(
            &ProtoPatchDeviceFields {
                name: Some(device_proto.name.clone()),
                chips: vec![ChipProto {
                    name: request.device.chips[0].name.clone(),
                    kind: EnumOrUnknown::new(ProtoChipKind::BLUETOOTH_BEACON),
                    chip: Some(Chip::BleBeacon(BleBeacon {
                        bt: MessageField::some(Default::default()),
                        ..Default::default()
                    })),
                    ..Default::default()
                }],
                ..Default::default()
            },
            get_pose_manager(),
        );
        assert!(patch_result.is_ok(), "{}", patch_result.unwrap_err());
        let patched_device = device.get(get_pose_manager());
        assert!(patched_device.is_ok(), "{}", patched_device.unwrap_err());
        let patched_device = patched_device.unwrap();
        assert_eq!(1, patched_device.chips.len());
        assert!(matches!(patched_device.chips[0].chip, Some(Chip::BleBeacon(_))));
    }

    #[test]
    fn test_remove_beacon_device_succeeds() {
        module_setup();
        let manager = get_manager();

        let create_request = get_test_create_device_request(None);
        let device_proto = create_device(&create_request);
        assert!(device_proto.is_ok(), "{}", device_proto.unwrap_err());

        let device_proto = device_proto.unwrap();
        let chip_id = {
            let devices = manager.devices.read().unwrap();
            let device = devices.get(&DeviceIdentifier(device_proto.id)).unwrap();
            let chips = device.chips.read().unwrap();
            chips.first_key_value().map(|(id, _)| *id).unwrap()
        };

        let delete_request = DeleteChipRequest { id: chip_id.0, ..Default::default() };
        let delete_result = delete_chip(&delete_request);
        assert!(delete_result.is_ok(), "{}", delete_result.unwrap_err());

        assert!(!manager.devices.read().unwrap().contains_key(&DeviceIdentifier(device_proto.id)))
    }

    #[test]
    fn test_remove_beacon_device_fails() {
        module_setup();
        let manager = get_manager();

        let create_request = get_test_create_device_request(None);
        let device_proto = create_device(&create_request);
        assert!(device_proto.is_ok(), "{}", device_proto.unwrap_err());

        let device_proto = device_proto.unwrap();
        let chip_id = manager
            .devices
            .read()
            .unwrap()
            .get(&DeviceIdentifier(device_proto.id))
            .unwrap()
            .chips
            .read()
            .unwrap()
            .first_key_value()
            .map(|(id, _)| *id)
            .unwrap();

        let delete_request = DeleteChipRequest { id: chip_id.0, ..Default::default() };
        let delete_result = delete_chip(&delete_request);
        assert!(delete_result.is_ok(), "{}", delete_result.unwrap_err());

        let delete_result = delete_chip(&delete_request);
        assert!(delete_result.is_err());
    }

    #[test]
    fn test_check_device_event_initial_timeout() {
        module_setup();

        let events = get_manager().events.clone();
        let events_rx = events.subscribe();
        assert_eq!(
            check_device_event(&events_rx, Some(std::time::Instant::now())),
            DeviceWaitStatus::Timeout
        );
    }

    #[test]
    fn test_check_device_event_last_device_removed() {
        module_setup();

        let events = Events::new();
        let events_rx = events.subscribe();
        events.publish(Event::ChipRemoved(ChipRemoved {
            remaining_nonbuiltin_devices: 0,
            ..Default::default()
        }));
        assert_eq!(check_device_event(&events_rx, None), DeviceWaitStatus::LastDeviceRemoved);
    }

    #[test]
    fn test_check_device_event_device_chip_added() {
        module_setup();

        let events = Events::new();
        let events_rx = events.subscribe();
        events.publish(Event::DeviceAdded(DeviceAdded {
            id: DeviceIdentifier(0),
            name: "".to_string(),
            builtin: false,
            device_stats: ProtoDeviceStats::new(),
        }));
        assert_eq!(check_device_event(&events_rx, None), DeviceWaitStatus::DeviceAdded);
        events.publish(Event::ChipAdded(ChipAdded { builtin: false, ..Default::default() }));
        assert_eq!(check_device_event(&events_rx, None), DeviceWaitStatus::DeviceAdded);
    }

    #[test]
    fn test_check_device_event_ignore_event() {
        module_setup();

        let events = Events::new();
        let events_rx = events.subscribe();
        events.publish(Event::DevicePatched(DevicePatched {
            id: DeviceIdentifier(0),
            name: "".to_string(),
        }));
        assert_eq!(check_device_event(&events_rx, None), DeviceWaitStatus::IgnoreEvent);
        events.publish(Event::ChipRemoved(ChipRemoved {
            remaining_nonbuiltin_devices: 1,
            ..Default::default()
        }));
        assert_eq!(check_device_event(&events_rx, None), DeviceWaitStatus::IgnoreEvent);
    }

    #[test]
    fn test_check_device_event_ignore_chip_added_for_builtin() {
        module_setup();

        let events = Events::new();
        let events_rx = events.subscribe();
        events.publish(Event::ChipAdded(ChipAdded { builtin: true, ..Default::default() }));
        assert_eq!(check_device_event(&events_rx, None), DeviceWaitStatus::IgnoreEvent);
    }
}

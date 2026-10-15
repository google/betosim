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

//! link_handlers for REST API to call

use std::sync::Arc;

use netsim_common::util::proto_print_options::JSON_PRINT_OPTION;
use netsim_proto::{
    frontend::{DeleteLinkRequest, ListLinkResponse, PatchLinkRequest},
    model,
};
use protobuf_json_mapping::{merge_from_str, print_to_string_with_options};

use crate::devices::chip::ChipIdentifier;

use super::link::{LinkManager, PhyKind};

pub fn handle_link_list(link_manager: &Arc<LinkManager>) -> Result<String, String> {
    let mut response = ListLinkResponse::new();
    for link in link_manager.list() {
        response.links.push(model::Link {
            sender_id: link.sender_id.0,
            receiver_id: link.receiver_id.0,
            link_kind: model::PhyKind::from(link.link_kind).into(),
            rssi: link.rssi as i32,
            ..Default::default()
        });
    }
    print_to_string_with_options(&response, &JSON_PRINT_OPTION).map_err(|e| format!("{e}"))
}

pub fn handle_link_patch(
    link_manager: &Arc<LinkManager>,
    patch_json: &str,
) -> Result<String, String> {
    let mut patch_link_request = PatchLinkRequest::new();
    if let Err(e) = merge_from_str(&mut patch_link_request, patch_json) {
        return Err(format!("Incorrect format of patch link json: {e:?}"));
    }
    let sender = ChipIdentifier(patch_link_request.link.sender_id);
    let receiver = ChipIdentifier(patch_link_request.link.receiver_id);
    let link_kind =
        PhyKind::try_from(patch_link_request.link.link_kind.enum_value_or_default()).unwrap();
    let rssi = patch_link_request.link.rssi as i8;
    link_manager.set_rssi(sender, receiver, link_kind, rssi);
    Ok(format!(
        "Successfully patched RSSI for link (Sender: {sender}, Receiver: {receiver}, Type: {link_kind:?}) to {rssi}."
    ))
}

pub fn handle_link_delete(
    link_manager: &Arc<LinkManager>,
    delete_json: &str,
) -> Result<String, String> {
    let mut delete_link_request = DeleteLinkRequest::new();
    if let Err(e) = merge_from_str(&mut delete_link_request, delete_json) {
        return Err(format!("Incorrect format of patch delete json: {e:?}"));
    }
    let sender = ChipIdentifier(delete_link_request.link.sender_id);
    let receiver = ChipIdentifier(delete_link_request.link.receiver_id);
    let link_kind =
        PhyKind::try_from(delete_link_request.link.link_kind.enum_value_or_default()).unwrap();
    link_manager.delete_rssi(sender, receiver, link_kind);
    Ok(format!(
        "Successfully deleted RSSI for link (Sender: {sender}, Receiver: {receiver}, Type: {link_kind:?})."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::chip::ChipIdentifier;

    fn create_link_manager() -> Arc<LinkManager> {
        let link_manager = LinkManager::new();
        link_manager.set_rssi(
            ChipIdentifier(1),
            ChipIdentifier(2),
            PhyKind::BluetoothLowEnergy,
            -50,
        );
        Arc::new(link_manager)
    }

    #[test]
    fn test_handle_link_list() {
        let link_manager = create_link_manager();
        let result = handle_link_list(&link_manager);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            r#"{"links": [{"senderId": 1, "receiverId": 2, "linkKind": "BLUETOOTH_LOW_ENERGY", "rssi": -50, "kind": "UNSPECIFIED", "id": 0}]}"#
        );
    }

    #[test]
    fn test_handle_link_patch() {
        let link_manager = create_link_manager();
        let patch_json = r#"{"link": {"senderId": 1, "receiverId": 2, "linkKind": "BLUETOOTH_LOW_ENERGY", "rssi": -60}}"#;
        let result = handle_link_patch(&link_manager, patch_json);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            "Successfully patched RSSI for link (Sender: 1, Receiver: 2, Type: BluetoothLowEnergy) to -60."
        );
        let rssi = link_manager.get_rssi(
            ChipIdentifier(1),
            ChipIdentifier(2),
            PhyKind::BluetoothLowEnergy,
        );
        assert_eq!(rssi, Some(-60));
    }

    #[test]
    fn test_handle_link_patch_invalid_json() {
        let link_manager = create_link_manager();
        let patch_json = r#"{invalid_json}"#;
        let result = handle_link_patch(&link_manager, patch_json);
        assert!(result.is_err());
        assert!(result.unwrap_err().starts_with("Incorrect format of patch link json"));
    }

    #[test]
    fn test_handle_link_delete() {
        let link_manager = create_link_manager();
        let delete_json =
            r#"{"link": {"senderId": 1, "receiverId": 2, "linkKind": "BLUETOOTH_LOW_ENERGY"}}"#;
        let result = handle_link_delete(&link_manager, delete_json);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            "Successfully deleted RSSI for link (Sender: 1, Receiver: 2, Type: BluetoothLowEnergy)."
        );
        assert_eq!(link_manager.list().len(), 0);
    }

    #[test]
    fn test_handle_link_delete_invalid_json() {
        let link_manager = create_link_manager();
        let delete_json = r#"{invalid_json}"#;
        let result = handle_link_delete(&link_manager, delete_json);
        assert!(result.is_err());
        assert!(result.unwrap_err().starts_with("Incorrect format of patch delete json"));
    }
}

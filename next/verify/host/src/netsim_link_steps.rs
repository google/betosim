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

// This file is currently disabled because all steps in it rely on gRPC for link
// management. We are removing gRPC to eliminate the grpcio dependency for
// Google3. These steps will be refactored to use the Netsim CLI once link
// management is supported in the CLI. Tracked in b/508244289.

/*
use netsim_proto::frontend;
use verify_macros::{step, step_module};

use crate::orchestrator::TestContext;

#[step_module]
pub mod steps {
    use super::*;

    #[step(r#"@netsim links @(\S+) to @(\S+) by (\S+) RSSI (-?\d+) as "([^"]+)""#)]
    async fn netsim_create_link(
        w: &mut TestContext,
        sender: String,
        receiver: String,
        kind: String,
        rssi: i32,
        var_name: String,
    ) -> Result<(), String> {
        let resolved_sender = w.resolve_placeholders(&format!("@{}", sender));
        let resolved_receiver = w.resolve_placeholders(&format!("@{}", receiver));
        let sender_device = w
            .map_actor_to_netsim(&resolved_sender)
            .map_err(|e| format!("got sender device name: {}", e))?;
        let receiver_device = w
            .map_actor_to_netsim(&resolved_receiver)
            .map_err(|e| format!("got receiver device name: {}", e))?;

        w.log_step(
            "@netsim",
            "->",
            &format!(
                "Links {} to {} by {} RSSI {} as \"{}\"",
                sender_device, receiver_device, kind, rssi, var_name
            ),
        );

        if w.is_dry_run {
            w.variables.insert(var_name, "dummy_link_id".to_string());
            return Ok(());
        }

        let client =
            w.get_or_create_grpc_client().map_err(|e| format!("got grpc client: {}", e))?;

        let (sender_id, receiver_id) =
            get_chip_ids(&client, &sender_device, &receiver_device, &kind)
                .map_err(|e| format!("found chip ids: {}", e))?;

        let mut link = netsim_proto::model::Link::new();
        link.sender_id = sender_id;
        link.receiver_id = receiver_id;
        link.rssi = rssi;
        link.kind = kind_to_proto(&kind).into();

        let mut req = frontend::CreateLinkRequest::new();
        req.link = protobuf::MessageField::some(link);

        client.create_link(&req).map_err(|e| format!("created link: {}", e))?;

        let link_id = find_link_id(&client, sender_id, receiver_id, &kind)
            .ok_or_else(|| "found link id".to_string())?;
        w.variables.insert(var_name, link_id.to_string());
        Ok(())
    }

    #[step(r#"@netsim should list a link with RSSI (-?\d+)"#)]
    async fn netsim_verify_link_rssi(
        w: &mut TestContext,
        expected_rssi: i32,
    ) -> Result<(), String> {
        w.log_step("@netsim", "THEN", &format!("Should list a link with RSSI {}", expected_rssi));

        if w.is_dry_run {
            return Ok(());
        }

        let client =
            w.get_or_create_grpc_client().map_err(|e| format!("got grpc client: {}", e))?;
        let links_resp = client
            .list_link(&protobuf::well_known_types::empty::Empty::new())
            .map_err(|e| format!("listed links: {}", e))?;

        if !links_resp.links.iter().any(|link| link.rssi == expected_rssi) {
            return Err(format!(
                "Link with RSSI '{}' not found in ListLink response",
                expected_rssi
            ));
        }
        Ok(())
    }

    #[step(r#"@netsim patches link (\S+) with RSSI (-?\d+)"#)]
    async fn netsim_patch_link(
        w: &mut TestContext,
        link_var: String,
        rssi: i32,
    ) -> Result<(), String> {
        let resolved_link_id = w.resolve_placeholders(&link_var);

        w.log_step(
            "@netsim",
            "->",
            &format!("Patches link {} with RSSI {}", resolved_link_id, rssi),
        );

        if w.is_dry_run {
            return Ok(());
        }

        let link_id =
            resolved_link_id.parse::<u32>().map_err(|e| format!("valid link id: {}", e))?;

        let client =
            w.get_or_create_grpc_client().map_err(|e| format!("got grpc client: {}", e))?;
        let links_resp = client
            .list_link(&protobuf::well_known_types::empty::Empty::new())
            .map_err(|e| format!("listed links: {}", e))?;
        let existing_link = links_resp
            .links
            .iter()
            .find(|l| l.id == link_id)
            .ok_or_else(|| "found existing link".to_string())?;

        let mut link = netsim_proto::model::Link::new();
        link.sender_id = existing_link.sender_id;
        link.receiver_id = existing_link.receiver_id;
        link.rssi = rssi;
        link.kind = existing_link.kind;

        let mut req = frontend::PatchLinkRequest::new();
        req.link = protobuf::MessageField::some(link);
        req.id = link_id;

        client.patch_link(&req).map_err(|e| format!("patched link: {}", e))?;
        Ok(())
    }

    #[step(r#"@netsim deletes link (\S+)"#)]
    async fn netsim_delete_link(w: &mut TestContext, link_var: String) -> Result<(), String> {
        let resolved_link_id = w.resolve_placeholders(&link_var);

        w.log_step("@netsim", "->", &format!("Deletes link {}", resolved_link_id));

        if w.is_dry_run {
            return Ok(());
        }

        let link_id =
            resolved_link_id.parse::<u32>().map_err(|e| format!("valid link id: {}", e))?;
        let client =
            w.get_or_create_grpc_client().map_err(|e| format!("got grpc client: {}", e))?;

        let mut req = frontend::DeleteLinkRequest::new();
        req.id = link_id;

        client.delete_link(&req).map_err(|e| format!("deleted link: {}", e))?;
        Ok(())
    }

    // Helper functions
    fn find_link_id(
        client: &netsim_proto::frontend_grpc::FrontendServiceClient,
        sender_id: u32,
        receiver_id: u32,
        kind: &str,
    ) -> Option<u32> {
        let links_resp = client.list_link(&protobuf::well_known_types::empty::Empty::new()).ok()?;
        links_resp
            .links
            .iter()
            .find(|link| {
                link.sender_id == sender_id
                    && link.receiver_id == receiver_id
                    && kind_matches(link.kind.enum_value_or_default(), kind)
            })
            .map(|link| link.id)
    }

    fn get_chip_ids(
        client: &netsim_proto::frontend_grpc::FrontendServiceClient,
        sender_device: &str,
        receiver_device: &str,
        kind: &str,
    ) -> Result<(u32, u32), String> {
        let resp = client
            .list_device(&protobuf::well_known_types::empty::Empty::new())
            .map_err(|e| format!("gRPC error: {:?}", e))?;
        let sender_id = find_chip_id(&resp, sender_device, kind)
            .ok_or_else(|| "Sender chip not found".to_string())?;
        let receiver_id = find_chip_id(&resp, receiver_device, kind)
            .ok_or_else(|| "Receiver chip not found".to_string())?;
        Ok((sender_id, receiver_id))
    }

    fn find_chip_id(
        resp: &frontend::ListDeviceResponse,
        device_name: &str,
        kind: &str,
    ) -> Option<u32> {
        resp.devices
            .iter()
            .find(|device| device.name == device_name)?
            .chips
            .iter()
            .find(|chip| kind_matches(chip.kind.enum_value_or_default(), kind))
            .map(|chip| chip.id)
    }

    fn kind_matches(kind: netsim_proto::common::ChipKind, kind_str: &str) -> bool {
        match kind_str {
            "bluetooth" => kind == netsim_proto::common::ChipKind::BLUETOOTH,
            "wifi" => kind == netsim_proto::common::ChipKind::WIFI,
            "uwb" => kind == netsim_proto::common::ChipKind::UWB,
            _ => false,
        }
    }

    fn kind_to_proto(kind_str: &str) -> netsim_proto::common::ChipKind {
        match kind_str {
            "bluetooth" => netsim_proto::common::ChipKind::BLUETOOTH,
            "wifi" => netsim_proto::common::ChipKind::WIFI,
            "uwb" => netsim_proto::common::ChipKind::UWB,
            _ => netsim_proto::common::ChipKind::UNSPECIFIED,
        }
    }
}

pub use steps::register_steps;
*/

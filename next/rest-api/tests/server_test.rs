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

use std::sync::Arc;

use ap_actor::ApClient;
use device_actor::DeviceClient;
use http_body_util::BodyExt;
use hyper::{Method, StatusCode};
use link_api::{Link, LinkAction, LinkCreate, LinkId, LinkUpdate};
use netsim_model::{ChipId, ChipKind, ClientError};
use netsim_rest_api::RestServer;

#[derive(Debug)]
struct MockLinkClient;

#[async_trait::async_trait]
impl link_api::LinkClient for MockLinkClient {
    async fn list(&self) -> Result<Vec<Link>, ClientError> {
        Ok(vec![])
    }
    async fn create(&self, _params: LinkCreate) -> Result<LinkId, ClientError> {
        Ok(LinkId(1))
    }
    async fn update(&self, _id: LinkId, _patch: LinkUpdate) -> Result<(), ClientError> {
        Ok(())
    }
    async fn delete(&self, _id: LinkId) -> Result<(), ClientError> {
        Ok(())
    }
    async fn action(&self, _id: Option<LinkId>, _action: LinkAction) -> Result<(), ClientError> {
        Ok(())
    }
    async fn notify_chip_added(
        &self,
        _chip_id: ChipId,
        _kind: ChipKind,
    ) -> Result<(), ClientError> {
        Ok(())
    }
    async fn notify_chip_removed(&self, _chip_id: ChipId) -> Result<(), ClientError> {
        Ok(())
    }
    async fn reset(&self) -> Result<(), ClientError> {
        Ok(())
    }
    async fn shutdown(&self) -> Result<(), ClientError> {
        Ok(())
    }
}

#[tokio::test]
async fn test_handle_request_version() {
    let (device_tx, _) = tokio::sync::mpsc::channel(1);
    let device_client =
        DeviceClient::new(Box::new(actor_framework::ResourceClient::new(device_tx)));
    let (ap_tx, _) = tokio::sync::mpsc::channel(1);
    let ap_client = ApClient::new(actor_framework::ResourceClient::new(ap_tx));
    let link_client = Arc::new(MockLinkClient);

    let server = RestServer::new(device_client, link_client, ap_client, "0.3.92".to_string());

    let response = server.handle_request(&Method::GET, "/v1/version", &[]).await;
    let status = response.status();
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_str = std::str::from_utf8(&body_bytes).unwrap();
    assert_eq!(status, StatusCode::OK);
    assert!(body_str.contains("0.3.92"));
}

#[tokio::test]
async fn test_handle_request_links() {
    let (device_tx, _) = tokio::sync::mpsc::channel(1);
    let device_client =
        DeviceClient::new(Box::new(actor_framework::ResourceClient::new(device_tx)));
    let (ap_tx, _) = tokio::sync::mpsc::channel(1);
    let ap_client = ApClient::new(actor_framework::ResourceClient::new(ap_tx));
    let link_client = Arc::new(MockLinkClient);

    let server = RestServer::new(device_client, link_client, ap_client, "0.3.92".to_string());

    let response = server.handle_request(&Method::GET, "/v1/links", &[]).await;
    let status = response.status();
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_str = std::str::from_utf8(&body_bytes).unwrap();
    assert_eq!(status, StatusCode::OK);
    assert!(body_str.contains("links"));
}

#[tokio::test]
async fn test_handle_request_not_found() {
    let (device_tx, _) = tokio::sync::mpsc::channel(1);
    let device_client =
        DeviceClient::new(Box::new(actor_framework::ResourceClient::new(device_tx)));
    let (ap_tx, _) = tokio::sync::mpsc::channel(1);
    let ap_client = ApClient::new(actor_framework::ResourceClient::new(ap_tx));
    let link_client = Arc::new(MockLinkClient);

    let server = RestServer::new(device_client, link_client, ap_client, "0.3.92".to_string());

    let response = server.handle_request(&Method::GET, "/v1/unknown", &[]).await;
    let status = response.status();
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_str = std::str::from_utf8(&body_bytes).unwrap();
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body_str.contains("Not Found"));
}

#[tokio::test]
async fn test_handle_request_aps() {
    let (device_tx, _) = tokio::sync::mpsc::channel(1);
    let device_client =
        DeviceClient::new(Box::new(actor_framework::ResourceClient::new(device_tx)));
    let (ap_tx, mut ap_rx) = tokio::sync::mpsc::channel(1);
    let ap_client = ApClient::new(actor_framework::ResourceClient::new(ap_tx));
    let link_client = Arc::new(MockLinkClient);

    // Stand in for the AP actor so the handler receives a successful List.
    tokio::spawn(async move {
        if let Some(actor_framework::ResourceRequest::List { respond_to }) = ap_rx.recv().await {
            let _ = respond_to.send(Ok(Vec::new()));
        }
    });

    let server = RestServer::new(device_client, link_client, ap_client, "0.3.92".to_string());

    let response = server.handle_request(&Method::GET, "/v1/aps", &[]).await;
    let status = response.status();
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_str = std::str::from_utf8(&body_bytes).unwrap();
    assert_eq!(status, StatusCode::OK);
    // The `aps` field name is part of the REST contract; renaming it breaks
    // generated clients. Asserted on the exact body rather than a substring so
    // a rename cannot slip through.
    assert_eq!(body_str, r#"{"aps":[]}"#);
}

/// `NetsimDaemon` derives `Debug` and holds an `Option<RestServer>`, so
/// `RestServer` must remain `Debug`. The daemon cannot be built by Bazel in
/// this checkout, so assert the bound here rather than discovering it in Soong.
#[test]
fn test_rest_server_implements_debug() {
    fn assert_debug<T: std::fmt::Debug>() {}
    assert_debug::<RestServer>();
}

#[tokio::test]
async fn test_handle_request_openapi() {
    let (device_tx, _) = tokio::sync::mpsc::channel(1);
    let device_client =
        DeviceClient::new(Box::new(actor_framework::ResourceClient::new(device_tx)));
    let (ap_tx, _) = tokio::sync::mpsc::channel(1);
    let ap_client = ApClient::new(actor_framework::ResourceClient::new(ap_tx));
    let link_client = Arc::new(MockLinkClient);

    let server = RestServer::new(device_client, link_client, ap_client, "0.3.92".to_string());

    let response = server.handle_request(&Method::GET, netsim_rest_api::path::OPENAPI, &[]).await;
    let status = response.status();
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_str = std::str::from_utf8(&body_bytes).unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body_str, netsim_rest_api::OPENAPI_JSON);
    let doc: serde_json::Value = serde_json::from_str(body_str).unwrap();
    assert_eq!(netsim_rest_api::openapi::validate(&doc), Vec::<String>::new());
}

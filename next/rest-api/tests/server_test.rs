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

use actor_framework::FrameworkError;
use ap_actor::{ApClient, ApError};
use device_actor::DeviceClient;
use http_body_util::BodyExt;
use hyper::{Method, StatusCode};
use link_api::{Link, LinkAction, LinkCreate, LinkError, LinkId, LinkUpdate};
use netsim_model::{ChipError, ChipId, ChipKind, ClientError, DeviceId};
use netsim_rest_api::{
    ApUpdate, CreateLinkRequest, Created, ErrorResponse, JsonResponse, NoContent, Op, RestServer,
    VersionResponse, client_error_status, created_response, no_content_response,
    openapi::{Route, find_route},
    parse_json_body, parse_path_param, path_id, path_param,
};

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

async fn collect_error_response(resp: impl Into<JsonResponse>) -> (StatusCode, ErrorResponse) {
    let resp = resp.into();
    let status = resp.0.status();
    let bytes = resp.0.into_body().collect().await.unwrap().to_bytes();
    let err: ErrorResponse = serde_json::from_slice(&bytes).unwrap();
    (status, err)
}

#[tokio::test]
async fn test_parse_json_body_valid() {
    let body = br#"{"sender":10,"receiver":20,"rssi":-50}"#;
    let req: CreateLinkRequest = parse_json_body(body).unwrap();
    assert_eq!(req.sender, ChipId(10));
    assert_eq!(req.receiver, ChipId(20));
    assert_eq!(req.rssi, -50);

    let ap_body = br#"{"ssid":"AndroidWifi","channel":6}"#;
    let patch: ApUpdate = parse_json_body(ap_body).unwrap();
    assert_eq!(patch.ssid.as_deref(), Some("AndroidWifi"));
    assert_eq!(patch.channel, Some(6));
    assert!(patch.force_disconnect.is_empty());
}

#[tokio::test]
async fn test_parse_json_body_malformed_and_wrong_shape() {
    // Malformed JSON syntax -> 400 Bad Request
    let http_err = parse_json_body::<CreateLinkRequest>(b"{bad json").unwrap_err();
    assert_eq!(http_err.status(), StatusCode::BAD_REQUEST);
    assert!(http_err.message().contains("Invalid JSON body"));
    assert!(http_err.to_string().starts_with("400 Bad Request: Invalid JSON body"));
    let (status, err) = collect_error_response(http_err).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(err.error.contains("Invalid JSON body"));

    // Empty body -> 400 Bad Request
    let (status, err) =
        collect_error_response(parse_json_body::<CreateLinkRequest>(b"").unwrap_err()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(err.error.contains("Invalid JSON body"));

    // Wrong field type -> 400 Bad Request
    let wrong_type = br#"{"sender":"not-a-u32","receiver":2,"rssi":-50}"#;
    let (status, err) =
        collect_error_response(parse_json_body::<CreateLinkRequest>(wrong_type).unwrap_err()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(err.error.contains("Invalid JSON body"));

    // Missing required fields -> 400 Bad Request
    let missing_fields = br#"{"sender":1}"#;
    let (status, err) =
        collect_error_response(parse_json_body::<CreateLinkRequest>(missing_fields).unwrap_err())
            .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(err.error.contains("Invalid JSON body"));
}

#[tokio::test]
async fn test_path_param_and_path_id_extraction() {
    const WRITE_ROUTES: &[Route<Op>] = &[
        Route { method: Method::DELETE, path: "/v1/aps/{id}", op: Op::ListAps },
        Route {
            method: Method::DELETE,
            path: "/v1/devices/{id}/chips/{chip_id}",
            op: Op::ListDevices,
        },
    ];

    let (_, params) = find_route(WRITE_ROUTES, &Method::DELETE, "/v1/aps/42").unwrap();
    assert_eq!(path_param(&params, "id").unwrap(), "42");
    assert_eq!(path_id::<u32>(&params).unwrap(), 42);
    assert_eq!(path_id::<DeviceId>(&params).unwrap(), DeviceId(42));
    assert_eq!(path_id::<LinkId>(&params).unwrap(), LinkId(42));
    assert_eq!(path_id::<ChipId>(&params).unwrap(), ChipId(42));

    let (_, nested_params) =
        find_route(WRITE_ROUTES, &Method::DELETE, "/v1/devices/5/chips/9").unwrap();
    assert_eq!(path_id::<DeviceId>(&nested_params).unwrap(), DeviceId(5));
    assert_eq!(parse_path_param::<u32>(&nested_params, "chip_id").unwrap(), 9);

    // Non-numeric {id} -> 400 Bad Request
    let (_, bad_params) = find_route(WRITE_ROUTES, &Method::DELETE, "/v1/aps/not-a-u32").unwrap();
    let (status, err) = collect_error_response(path_id::<u32>(&bad_params).unwrap_err()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(err.error.contains("Invalid path parameter id"));

    // Integer overflow {id} -> 400 Bad Request
    let (_, overflow_params) =
        find_route(WRITE_ROUTES, &Method::DELETE, "/v1/aps/99999999999999999999").unwrap();
    let (status, _) = collect_error_response(path_id::<u32>(&overflow_params).unwrap_err()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Missing path parameter -> 400 Bad Request
    let (status, err) = collect_error_response(path_param(&params, "missing").unwrap_err()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(err.error, "Missing path parameter: missing");
}

#[tokio::test]
async fn test_client_error_mapping_not_found() {
    let cases: Vec<ClientError> = vec![
        ClientError::Chip(ChipError::ChipNotFound(ChipId(9))),
        FrameworkError::ServiceError(device_actor::DeviceError::NotFound("dev".into())).into(),
        FrameworkError::ServiceError(device_actor::DeviceError::DeviceNotFound("dev-5".into()))
            .into(),
        FrameworkError::ServiceError(LinkError::NotFound(3)).into(),
        FrameworkError::ServiceError(ApError::ApNotFound(7)).into(),
    ];

    for err in cases {
        assert_eq!(client_error_status(&err), StatusCode::NOT_FOUND, "for error: {err}");
        let (status, body) = collect_error_response(err).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(!body.error.is_empty());
    }
}

#[tokio::test]
async fn test_client_error_mapping_bad_request() {
    let cases: Vec<ClientError> = vec![
        ClientError::Chip(ChipError::InvalidArguments("bad arg".into())),
        ClientError::Chip(ChipError::InvalidAddress("bad mac".into())),
        ClientError::Chip(ChipError::UnsupportedChipKind(ChipKind::UNSPECIFIED)),
        ClientError::Chip(ChipError::Unsupported),
        FrameworkError::ServiceError(device_actor::DeviceError::ChipKindNotSupported(
            "UNSPECIFIED".into(),
        ))
        .into(),
        FrameworkError::ServiceError(LinkError::InvalidParam("self-link".into())).into(),
        FrameworkError::ServiceError(ApError::InvalidFrame).into(),
    ];

    for err in cases {
        assert_eq!(client_error_status(&err), StatusCode::BAD_REQUEST, "for error: {err}");
        let (status, body) = collect_error_response(err).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(!body.error.is_empty());
    }
}

#[tokio::test]
async fn test_client_error_mapping_conflict() {
    let cases: Vec<ClientError> = vec![
        ClientError::Chip(ChipError::ChipExists(1)),
        FrameworkError::ServiceError(LinkError::AlreadyExists).into(),
        FrameworkError::ServiceError(ApError::ApAlreadyExists(2)).into(),
    ];

    for err in cases {
        assert_eq!(client_error_status(&err), StatusCode::CONFLICT, "for error: {err}");
        let (status, body) = collect_error_response(err).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(!body.error.is_empty());
    }
}

#[tokio::test]
async fn test_client_error_mapping_internal_server_error() {
    let cases: Vec<ClientError> = vec![
        ClientError::Send("channel closed".into()),
        ClientError::Recv("oneshot dropped".into()),
        FrameworkError::<ApError>::ActorClosed("closed".into()).into(),
        FrameworkError::ServiceError(ApError::Internal("ap failure".into())).into(),
        FrameworkError::ServiceError(LinkError::Internal("link failure".into())).into(),
    ];

    for err in cases {
        assert_eq!(
            client_error_status(&err),
            StatusCode::INTERNAL_SERVER_ERROR,
            "for error: {err}"
        );
        let (status, body) = collect_error_response(err).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!body.error.is_empty());
    }
}

#[tokio::test]
async fn test_write_status_responses_201_and_204() {
    // 201 Created via Created<T> and created_response
    let created: JsonResponse =
        Ok::<_, ClientError>(Created(VersionResponse { version: "0.3.92".to_string() })).into();
    assert_eq!(created.0.status(), StatusCode::CREATED);
    assert_eq!(created.0.headers().get(hyper::header::CONTENT_TYPE).unwrap(), "application/json");
    assert_eq!(created.0.headers().get(hyper::header::X_CONTENT_TYPE_OPTIONS).unwrap(), "nosniff");
    let created_bytes = created.0.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(std::str::from_utf8(&created_bytes).unwrap(), r#"{"version":"0.3.92"}"#);

    let direct_created = created_response(&VersionResponse { version: "1.0.0".to_string() });
    assert_eq!(direct_created.status(), StatusCode::CREATED);

    // 204 No Content via NoContent, (), and no_content_response
    let no_content_marker: JsonResponse = NoContent.into();
    assert_eq!(no_content_marker.0.status(), StatusCode::NO_CONTENT);
    assert!(no_content_marker.0.headers().get(hyper::header::CONTENT_TYPE).is_none());
    assert_eq!(
        no_content_marker.0.headers().get(hyper::header::X_CONTENT_TYPE_OPTIONS).unwrap(),
        "nosniff"
    );
    let empty_bytes = no_content_marker.0.into_body().collect().await.unwrap().to_bytes();
    assert!(empty_bytes.is_empty());

    let unit_ok: JsonResponse = Ok::<(), ClientError>(()).into();
    assert_eq!(unit_ok.0.status(), StatusCode::NO_CONTENT);

    let direct_no_content = no_content_response();
    assert_eq!(direct_no_content.status(), StatusCode::NO_CONTENT);

    // End-to-end actor client write flow: DELETE /v1/aps/{id} -> 204 on Ok, 404 on ApNotFound
    let (ap_tx, mut ap_rx) = tokio::sync::mpsc::channel(2);
    let ap_client = ApClient::new(actor_framework::ResourceClient::new(ap_tx));

    tokio::spawn(async move {
        if let Some(actor_framework::ResourceRequest::Delete { id, respond_to }) =
            ap_rx.recv().await
        {
            assert_eq!(u32::from(id), 1);
            let _ = respond_to.send(Ok(()));
        }
        if let Some(actor_framework::ResourceRequest::Delete { id, respond_to }) =
            ap_rx.recv().await
        {
            let _ = respond_to.send(Err(ApError::ApNotFound(u32::from(id))));
        }
    });

    let delete_ok: JsonResponse = ap_client.destroy_ap(1).await.into();
    assert_eq!(delete_ok.0.status(), StatusCode::NO_CONTENT);

    let delete_missing: JsonResponse = ap_client.destroy_ap(99).await.into();
    let (status, err) = collect_error_response(delete_missing).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(err.error.contains("99"));
}

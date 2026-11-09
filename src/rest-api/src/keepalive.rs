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

//! HTTP connection upgrade keeping `netsimd` alive while open.
//!
//! Handles `GET /v1/keepalive` with `Connection: Upgrade` and
//! `Upgrade: netsim-keepalive`. Acquiring the keepalive lease suppresses
//! startup and idle timeouts; releasing it when the socket drops (EOF) or is
//! closed permits normal idle termination.

use bytes::Bytes;
use device_actor::DeviceClient;
use http_body_util::Full;
use hyper::{
    Request, Response, StatusCode,
    header::{CONNECTION, HeaderMap, HeaderValue, UPGRADE},
};
use hyper_util::rt::TokioIo;
use tokio::io::AsyncReadExt;
use tracing::{debug, info};

use crate::server::error_response;

const UPGRADE_PROTOCOL: &str = "netsim-keepalive";

fn header_contains(
    headers: &HeaderMap,
    name: impl hyper::header::AsHeaderName,
    token: &str,
) -> bool {
    headers.get_all(name).iter().any(|value| {
        value.to_str().is_ok_and(|v| v.split(',').any(|t| t.trim().eq_ignore_ascii_case(token)))
    })
}

pub(super) async fn upgrade<B: Send>(
    device_client: &DeviceClient,
    mut req: Request<B>,
) -> Response<Full<Bytes>> {
    let headers = req.headers();
    if !header_contains(headers, UPGRADE, UPGRADE_PROTOCOL)
        || !header_contains(headers, CONNECTION, "upgrade")
    {
        return error_response(
            StatusCode::BAD_REQUEST,
            "Expected Upgrade: netsim-keepalive request",
        );
    }

    let on_upgrade = hyper::upgrade::on(&mut req);
    drop(req);
    let client = device_client.clone();
    let (acq_tx, acq_rx) = tokio::sync::oneshot::channel();

    tokio::spawn(async move {
        let id = match client.acquire_keepalive().await {
            Ok(id) => id,
            Err(e) => {
                let _ = acq_tx.send(Err(e));
                return;
            }
        };

        #[allow(clippy::collapsible_if)]
        if acq_tx.send(Ok(id)).is_ok() {
            if let Ok(upgraded) = on_upgrade.await {
                info!("Keepalive session {id} opened");
                let mut io = TokioIo::new(upgraded);
                let mut buf = [0u8; 64];
                loop {
                    match io.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                }
                info!("Keepalive session {id} closed");
            }
        }

        if let Err(error) = client.release_keepalive(id).await {
            debug!(%error, "Keepalive session {id} release failed");
        }
    });

    match acq_rx.await {
        Ok(Ok(_)) => Response::builder()
            .status(StatusCode::SWITCHING_PROTOCOLS)
            .header(UPGRADE, HeaderValue::from_static(UPGRADE_PROTOCOL))
            .header(CONNECTION, HeaderValue::from_static("Upgrade"))
            .body(Full::new(Bytes::new()))
            .unwrap_or_else(|_| Response::new(Full::new(Bytes::new()))),
        Ok(Err(e)) => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            &format!("Failed to acquire keepalive: {e}"),
        ),
        Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "Keepalive task cancelled"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn upgrade_rejects_missing_upgrade_header() {
        let (device_tx, _) = tokio::sync::mpsc::channel(1);
        let device_client =
            DeviceClient::new(Box::new(actor_framework::ResourceClient::new(device_tx)));

        let req =
            Request::builder().header(CONNECTION, "Upgrade").body(Full::new(Bytes::new())).unwrap();
        let resp = upgrade(&device_client, req).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn upgrade_rejects_wrong_upgrade_protocol() {
        let (device_tx, _) = tokio::sync::mpsc::channel(1);
        let device_client =
            DeviceClient::new(Box::new(actor_framework::ResourceClient::new(device_tx)));

        let req = Request::builder()
            .header(UPGRADE, "websocket")
            .header(CONNECTION, "Upgrade")
            .body(Full::new(Bytes::new()))
            .unwrap();
        let resp = upgrade(&device_client, req).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn upgrade_rejects_missing_connection_header() {
        let (device_tx, _) = tokio::sync::mpsc::channel(1);
        let device_client =
            DeviceClient::new(Box::new(actor_framework::ResourceClient::new(device_tx)));

        let req = Request::builder()
            .header(UPGRADE, UPGRADE_PROTOCOL)
            .body(Full::new(Bytes::new()))
            .unwrap();
        let resp = upgrade(&device_client, req).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn upgrade_returns_service_unavailable_when_acquire_fails() {
        let (device_tx, mut device_rx) = tokio::sync::mpsc::channel(1);
        let device_client =
            DeviceClient::new(Box::new(actor_framework::ResourceClient::new(device_tx)));

        tokio::spawn(async move {
            let Some(actor_framework::ResourceRequest::Action {
                action: device_actor::DeviceAction::AcquireKeepalive,
                respond_to,
                ..
            }) = device_rx.recv().await
            else {
                panic!("expected AcquireKeepalive");
            };
            let _ = respond_to.send(Err(device_actor::DeviceError::ShuttingDown));
        });

        let req = Request::builder()
            .header(UPGRADE, UPGRADE_PROTOCOL)
            .header(CONNECTION, "Upgrade")
            .body(Full::new(Bytes::new()))
            .unwrap();
        let resp = upgrade(&device_client, req).await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn upgrade_releases_keepalive_if_caller_cancels_before_acquire_completes() {
        let (device_tx, mut device_rx) = tokio::sync::mpsc::channel(2);
        let device_client =
            DeviceClient::new(Box::new(actor_framework::ResourceClient::new(device_tx)));

        let (held_tx, held_rx) = tokio::sync::oneshot::channel();
        let (cancelled_tx, cancelled_rx) = tokio::sync::oneshot::channel();
        let (release_done_tx, release_done_rx) = tokio::sync::oneshot::channel();

        tokio::spawn(async move {
            let Some(actor_framework::ResourceRequest::Action {
                action: device_actor::DeviceAction::AcquireKeepalive,
                respond_to,
                ..
            }) = device_rx.recv().await
            else {
                panic!("expected AcquireKeepalive");
            };
            let _ = held_tx.send(());
            let _ = cancelled_rx.await;
            let _ = respond_to.send(Ok(device_actor::DeviceActionResult::KeepaliveId(7)));

            let Some(actor_framework::ResourceRequest::Action {
                action: device_actor::DeviceAction::ReleaseKeepalive(id),
                respond_to,
                ..
            }) = device_rx.recv().await
            else {
                panic!("expected ReleaseKeepalive");
            };
            assert_eq!(id, 7);
            let _ = respond_to.send(Ok(device_actor::DeviceActionResult::Success));
            let _ = release_done_tx.send(());
        });

        let req = Request::builder()
            .header(UPGRADE, UPGRADE_PROTOCOL)
            .header(CONNECTION, "Upgrade")
            .body(Full::new(Bytes::new()))
            .unwrap();

        {
            let upgrade_fut = upgrade(&device_client, req);
            tokio::pin!(upgrade_fut);

            tokio::select! {
                _ = &mut upgrade_fut => panic!("upgrade should not have completed"),
                _ = held_rx => {}
            }
        }
        let _ = cancelled_tx.send(());

        tokio::time::timeout(std::time::Duration::from_secs(2), release_done_rx)
            .await
            .expect("timed out waiting for ReleaseKeepalive(7)")
            .expect("release channel error");
    }
}

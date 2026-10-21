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

//! Tests for `NetsimRestClient` through its public API, against a one-shot
//! local TCP server that returns canned HTTP responses.

use std::{
    io::{Read, Write},
    net::TcpListener,
    thread::JoinHandle,
};

use hyper::StatusCode;
use netsim_model::{ApCreate, ApUpdate};
use netsim_rest_api::{DisconnectApRequest, NetsimRestClient, RestError, VersionResponse};

/// Accepts one connection, replies with `response`, and closes. The handle
/// yields the raw request the client sent.
fn serve_once(response: &'static str) -> (NetsimRestClient, JoinHandle<String>) {
    let (port, handle) = listen_once(response);
    (NetsimRestClient::new("127.0.0.1", port), handle)
}

/// Like [`serve_once`], but returns the IPv4 loopback port instead of a client.
fn listen_once(response: &'static str) -> (u16, JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("local_addr").port();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let request = read_request(&mut stream);
        stream.write_all(response.as_bytes()).expect("write response");
        request
    });
    (port, handle)
}

/// Reads one HTTP request: headers, then `Content-Length` bytes of body.
fn read_request(stream: &mut impl Read) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        let n = stream.read(&mut chunk).expect("read request");
        buf.extend_from_slice(&chunk[..n]);
        let text = String::from_utf8_lossy(&buf);
        if let Some((head, body)) = text.split_once("\r\n\r\n") {
            let len = head
                .lines()
                .find_map(|l| l.strip_prefix("Content-Length: "))
                .map_or(0, |v| v.parse::<usize>().expect("Content-Length"));
            if body.len() >= len || n == 0 {
                return text.into_owned();
            }
        } else if n == 0 {
            return text.into_owned();
        }
    }
}

#[test]
fn test_request_head_formatting() {
    let client = NetsimRestClient::new("localhost", 8080);
    assert_eq!(
        client.get_version().to_string(),
        "GET /v1/version HTTP/1.1\r\nHost: localhost:8080\r\nConnection: close\r\n\r\n"
    );
    assert_eq!(
        client
            .post::<VersionResponse>("/v1/devices")
            .json(&VersionResponse { version: "1.0".to_string() })
            .to_string(),
        "POST /v1/devices HTTP/1.1\r\nHost: localhost:8080\r\nContent-Type: application/json\r\nContent-Length: 17\r\nConnection: close\r\n\r\n"
    );
}

#[test]
fn test_new_brackets_ipv6_literals() {
    let host = |h: &str| NetsimRestClient::new(h, 80).get_version().to_string();
    assert!(host("::1").contains("\r\nHost: [::1]:80\r\n"));
    assert!(host("[::1]").contains("\r\nHost: [::1]:80\r\n"));
    assert!(host("localhost").contains("\r\nHost: localhost:80\r\n"));
}

#[tokio::test]
async fn test_send_decodes_json() {
    let (client, server) = serve_once("HTTP/1.1 200 OK\r\n\r\n{\"version\":\"0.3.92\"}");
    let resp = client.get_version().send().await.expect("send");
    assert_eq!(resp.version, "0.3.92");
    assert!(server.join().expect("server").starts_with("GET /v1/version HTTP/1.1\r\n"));
}

#[test]
fn test_send_blocking_posts_json_body() {
    let (client, server) = serve_once("HTTP/1.1 200 OK\r\n\r\n{\"version\":\"2\"}");
    let resp = client
        .post::<VersionResponse>("/v1/devices")
        .json(&VersionResponse { version: "1.0".to_string() })
        .send_blocking()
        .expect("send_blocking");
    assert_eq!(resp.version, "2");
    let request = server.join().expect("server");
    assert!(request.starts_with("POST /v1/devices HTTP/1.1\r\n"), "{request}");
    assert!(request.contains("\r\nContent-Length: 17\r\n"), "{request}");
    assert!(request.ends_with("\r\n\r\n{\"version\":\"1.0\"}"), "{request}");
}

#[test]
fn test_delete_empty_body_decodes_to_unit() {
    for response in ["HTTP/1.1 204 No Content\r\n\r\n", "HTTP/1.1 200 OK\r\n\r\n\r\n"] {
        let (client, server) = serve_once(response);
        client.delete("/v1/devices/1").send_blocking().expect(response);
        assert!(server.join().expect("server").starts_with("DELETE /v1/devices/1 HTTP/1.1\r\n"));
    }
}

#[test]
fn test_send_blocking_tries_all_resolved_addresses() {
    // `localhost` may resolve to `[::1]` before `127.0.0.1`; the server only
    // listens on IPv4, so the client must fall through to the next address.
    let (port, server) = listen_once("HTTP/1.1 200 OK\r\n\r\n{\"version\":\"1\"}");
    let client = NetsimRestClient::new("localhost", port);
    assert_eq!(client.get_version().send_blocking().expect("send_blocking").version, "1");
    server.join().expect("server");
}

#[test]
fn test_error_status_messages() {
    let cases = [
        (
            "HTTP/1.1 404 Not Found\r\n\r\n{\"error\":\"Not Found: GET /v1/x\"}",
            StatusCode::NOT_FOUND,
            "HTTP Error 404: Not Found: GET /v1/x",
        ),
        (
            "HTTP/1.1 500 Internal Server Error\r\n\r\nplain",
            StatusCode::INTERNAL_SERVER_ERROR,
            "HTTP Error 500: plain",
        ),
    ];
    for (response, status, expected) in cases {
        let (client, _server) = serve_once(response);
        let err = client.get::<()>("/v1/x").send_blocking().expect_err(response);
        assert_eq!(err.status(), Some(status), "{err:?}");
        assert_eq!(err.to_string(), expected);
    }
}

#[test]
fn test_malformed_responses() {
    for response in ["garbage", "HTTP/1.1 abc\r\n\r\n"] {
        let (client, _server) = serve_once(response);
        let err = client.get::<()>("/v1/x").send_blocking().expect_err(response);
        assert!(matches!(err, RestError::InvalidResponse(_)), "{err:?}");
        assert_eq!(err.status(), None);
    }
}

#[test]
fn test_connect_error() {
    let port = TcpListener::bind("127.0.0.1:0").expect("bind").local_addr().expect("addr").port();
    let err = NetsimRestClient::new("127.0.0.1", port)
        .get_version()
        .send_blocking()
        .expect_err("nothing is listening");
    assert!(matches!(err, RestError::Connect(..)), "{err:?}");
}

const CANNED_AP: &str = "HTTP/1.1 200 OK\r\n\r\n{\"id\":1,\"config\":{\"ssid\":\"AndroidWifi\",\"bssid\":\"02:15:b2:00:00:00\",\"channel\":6,\"hw_mode\":\"g\",\"beacon_interval\":200,\"country_code\":\"US\",\"dtim_period\":2,\"hidden_ssid\":false,\"sae\":false,\"wmm_enabled\":true,\"enterprise_enabled\":false,\"mac_acl_mode\":0,\"mac_acl_list\":[],\"ftm_responder_enabled\":true},\"associations\":[]}";

const CANNED_CREATED_AP: &str = "HTTP/1.1 201 Created\r\n\r\n{\"id\":1,\"config\":{\"ssid\":\"AndroidWifi\",\"bssid\":\"02:15:b2:00:00:00\",\"channel\":6,\"hw_mode\":\"g\",\"beacon_interval\":200,\"country_code\":\"US\",\"dtim_period\":2,\"hidden_ssid\":false,\"sae\":false,\"wmm_enabled\":true,\"enterprise_enabled\":false,\"mac_acl_mode\":0,\"mac_acl_list\":[],\"ftm_responder_enabled\":true},\"associations\":[]}";

#[test]
fn test_create_ap_wire_format_and_response() {
    let (client, server) = serve_once(CANNED_CREATED_AP);
    let create_req = ApCreate::default();
    let ap = client.create_ap(&create_req).send_blocking().expect("create_ap");
    assert_eq!(ap.id, 1);
    assert_eq!(ap.config.ssid, "AndroidWifi");
    let request = server.join().expect("server");
    assert!(request.starts_with("POST /v1/aps HTTP/1.1\r\n"), "{request}");
    assert!(request.contains(r#""ssid":"AndroidWifi""#), "{request}");
}

#[test]
fn test_patch_ap_wire_format_and_response() {
    let (client, server) = serve_once(CANNED_AP);
    let patch_req =
        ApUpdate { ssid: Some("PatchedWifi".to_string()), channel: Some(11), ..Default::default() };
    let ap = client.patch_ap(1, &patch_req).send_blocking().expect("patch_ap");
    assert_eq!(ap.id, 1);
    let request = server.join().expect("server");
    assert!(request.starts_with("PATCH /v1/aps/1 HTTP/1.1\r\n"), "{request}");
    assert!(request.contains(r#""ssid":"PatchedWifi""#), "{request}");
}

#[test]
fn test_delete_ap_wire_format() {
    let (client, server) = serve_once("HTTP/1.1 204 No Content\r\n\r\n");
    client.delete_ap(42).send_blocking().expect("delete_ap");
    let request = server.join().expect("server");
    assert!(request.starts_with("DELETE /v1/aps/42 HTTP/1.1\r\n"), "{request}");
}

#[test]
fn test_disconnect_ap_wire_format() {
    let (client, server) = serve_once("HTTP/1.1 204 No Content\r\n\r\n");
    let disconnect_req = DisconnectApRequest { mac_address: "02:15:b2:00:01:00".to_string() };
    client.disconnect_ap(7, &disconnect_req).send_blocking().expect("disconnect_ap");
    let request = server.join().expect("server");
    assert!(request.starts_with("POST /v1/aps/7:disconnect HTTP/1.1\r\n"), "{request}");
    assert!(request.contains(r#""mac_address":"02:15:b2:00:01:00""#), "{request}");
}

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

#[test]
fn test_reset_wire_format() {
    let (client, server) = serve_once("HTTP/1.1 204 No Content\r\n\r\n");
    client.reset().send_blocking().expect("reset");
    let request = server.join().expect("server");
    assert!(request.starts_with("POST /v1/devices:reset HTTP/1.1\r\n"), "{request}");
}

#[test]
fn test_patch_device_wire_format_and_response() {
    let canned_device = "HTTP/1.1 200 OK\r\n\r\n{\"id\":1,\"name\":\"dev-1\",\"visible\":true,\"pose\":{\"position\":{\"x\":1.5,\"y\":2.5,\"z\":3.5},\"orientation\":{\"yaw\":0.0,\"pitch\":0.0,\"roll\":0.0}},\"builtin\":false,\"chips\":[]}";
    let (client, server) = serve_once(canned_device);
    let patch_req = netsim_model::DeviceUpdate {
        pose: netsim_model::PoseUpdate {
            position: Some(netsim_model::Position { x: 1.5, y: 2.5, z: 3.5 }),
            orientation: None,
        },
        ..Default::default()
    };
    let dev = client.patch_device(1, &patch_req).send_blocking().expect("patch_device");
    assert_eq!(dev.id, 1);
    assert_eq!(dev.pose.position, netsim_model::Position { x: 1.5, y: 2.5, z: 3.5 });
    let request = server.join().expect("server");
    assert!(request.starts_with("PATCH /v1/devices/1 HTTP/1.1\r\n"), "{request}");
    assert!(request.contains(r#""position":{"x":1.5,"y":2.5,"z":3.5}"#), "{request}");
}

#[test]
fn test_create_device_wire_format_and_response() {
    let canned_created_device = "HTTP/1.1 201 Created\r\n\r\n{\"id\":1,\"name\":\"beacon-1\",\"visible\":true,\"pose\":{\"position\":{\"x\":0.0,\"y\":0.0,\"z\":0.0},\"orientation\":{\"yaw\":0.0,\"pitch\":0.0,\"roll\":0.0}},\"builtin\":false,\"chips\":[]}";
    let (client, server) = serve_once(canned_created_device);
    let create_req = netsim_model::DeviceCreate {
        device_config: netsim_model::DeviceConfig {
            name: "beacon-1".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };
    let dev = client.create_device(&create_req).send_blocking().expect("create_device");
    assert_eq!(dev.id, 1);
    assert_eq!(dev.name, "beacon-1");
    let request = server.join().expect("server");
    assert!(request.starts_with("POST /v1/devices HTTP/1.1\r\n"), "{request}");
    assert!(request.contains(r#""name":"beacon-1""#), "{request}");
}

#[test]
fn test_delete_device_wire_format_and_response() {
    let (client, server) = serve_once("HTTP/1.1 204 No Content\r\n\r\n");
    client.delete_device(42).send_blocking().expect("delete_device");
    let request = server.join().expect("server");
    assert!(request.starts_with("DELETE /v1/devices/42 HTTP/1.1\r\n"), "{request}");
}

#[test]
fn test_send_to_file_blocking_error_and_content_length() {
    let tmp_dir = std::env::temp_dir();
    let pid = std::process::id();
    let not_found_path = tmp_dir.join(format!("netsim-rest-test-{pid}-missing.pcap"));
    let _ = std::fs::remove_file(&not_found_path);
    let (client, _server) = serve_once("HTTP/1.1 404 Not Found\r\n\r\nCapture not found");
    let err = client
        .get_capture(9999)
        .send_to_file_blocking(&not_found_path)
        .expect_err("404 must error");
    assert_eq!(err.status(), Some(StatusCode::NOT_FOUND));
    assert!(!not_found_path.exists(), "404 must not create 0-byte file");

    // Verify truncated response (body shorter than Content-Length) returns an
    // error and removes the incomplete file
    let truncated_path = tmp_dir.join(format!("netsim-rest-test-{pid}-truncated.pcap"));
    let _ = std::fs::remove_file(&truncated_path);
    let (client, _server) = serve_once(
        "HTTP/1.1 200 OK\r\nContent-Type: application/vnd.tcpdump.pcap\r\nContent-Length: 24\r\n\r\nshort",
    );
    let err = client
        .get_capture(1000)
        .send_to_file_blocking(&truncated_path)
        .expect_err("truncated response must error");
    assert!(
        matches!(err, RestError::InvalidResponse("response body shorter than Content-Length")),
        "{err:?}"
    );
    assert!(!truncated_path.exists(), "truncated download must clean up partial file");
    assert!(!truncated_path.with_extension("pcap.part").exists());
}

#[test]
fn test_send_to_file_blocking_keeps_existing_file_on_error() {
    let path =
        std::env::temp_dir().join(format!("netsim-rest-test-{}-existing.pcap", std::process::id()));
    std::fs::write(&path, b"previous capture").unwrap();

    let (client, _server) = serve_once("HTTP/1.1 404 Not Found\r\n\r\nCapture not found");
    let err = client.get_capture(9999).send_to_file_blocking(&path).expect_err("404 must error");
    assert_eq!(err.status(), Some(StatusCode::NOT_FOUND));
    assert_eq!(std::fs::read(&path).unwrap(), b"previous capture");

    let (client, _server) = serve_once("HTTP/1.1 200 OK\r\nContent-Length: 24\r\n\r\nshort");
    let err = client.get_capture(1000).send_to_file_blocking(&path).expect_err("short body");
    assert!(
        matches!(err, RestError::InvalidResponse("response body shorter than Content-Length")),
        "{err:?}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"previous capture");
    assert!(!path.with_extension("pcap.part").exists());
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn test_chunked_response_is_rejected() {
    let (client, _server) =
        serve_once("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n");
    let err = client.get_version().send_blocking().expect_err("chunked must error");
    assert!(
        matches!(err, RestError::InvalidResponse("Transfer-Encoding is not supported")),
        "{err:?}"
    );
}

#[test]
fn test_send_to_file_blocking_streams_body_larger_than_buffer() {
    let body: Vec<u8> = (0u8..=250).cycle().take(20_000).collect();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("local_addr").port();
    let sent = body.clone();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        read_request(&mut stream);
        let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", sent.len());
        stream.write_all(head.as_bytes()).expect("write head");
        for chunk in sent.chunks(3_000) {
            stream.write_all(chunk).expect("write chunk");
            stream.flush().expect("flush");
        }
    });
    let path =
        std::env::temp_dir().join(format!("netsim-rest-test-{}-large.pcap", std::process::id()));
    let client = NetsimRestClient::new("127.0.0.1", port);
    let written = client.get_capture(1000).send_to_file_blocking(&path).expect("download");
    server.join().expect("server");
    assert_eq!(written, body.len() as u64);
    assert_eq!(std::fs::read(&path).unwrap(), body);
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn test_captures_wire_format_and_streaming_bytes() {
    let canned_list = "HTTP/1.1 200 OK\r\n\r\n{\"captures\":[{\"chip_id\":1000,\"chip_kind\":\"WIFI\",\"device_name\":\"pixel-wifi\",\"enabled\":false,\"records_written\":0,\"bytes_written\":0,\"seconds\":0,\"nanos\":0}]}";
    let (client, server) = serve_once(canned_list);
    let list_resp = client.get_captures().send_blocking().expect("get_captures");
    assert_eq!(list_resp.captures.len(), 1);
    assert_eq!(list_resp.captures[0].chip_id, netsim_model::ChipId(1000));
    assert!(!list_resp.captures[0].enabled);
    assert!(server.join().expect("server").starts_with("GET /v1/captures HTTP/1.1\r\n"));

    let canned_patch = "HTTP/1.1 200 OK\r\n\r\n{\"chip_id\":1000,\"chip_kind\":\"WIFI\",\"device_name\":\"pixel-wifi\",\"enabled\":true,\"records_written\":0,\"bytes_written\":0,\"seconds\":1700000000,\"nanos\":0}";
    let (client, server) = serve_once(canned_patch);
    let patched = client
        .patch_capture(1000, &netsim_rest_api::CaptureUpdate { state: true })
        .send_blocking()
        .expect("patch_capture");
    assert!(patched.enabled);
    let req = server.join().expect("server");
    assert!(req.starts_with("PATCH /v1/captures/1000 HTTP/1.1\r\n"), "{req}");
    assert!(req.ends_with("\r\n\r\n{\"state\":true}"), "{req}");

    // Test binary PCAP streaming with non-UTF-8 bytes (0xa1b2c3d4 magic header)
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("local_addr").port();
    let expected_pcap: Vec<u8> = vec![0xd4, 0xc3, 0xb2, 0xa1, 0x02, 0x00, 0x04, 0x00, 0xff, 0xfe];
    let expected_clone = expected_pcap.clone();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let request = read_request(&mut stream);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/vnd.tcpdump.pcap\r\n\r\n")
            .expect("write head");
        stream.write_all(&expected_clone).expect("write pcap");
        request
    });
    let client = NetsimRestClient::new("127.0.0.1", port);
    let tmp_dir = std::env::temp_dir();
    let pid = std::process::id();
    let out_file = tmp_dir.join(format!("netsim-rest-test-{pid}-happy.pcap"));
    let _ = std::fs::remove_file(&out_file);
    let written =
        client.get_capture(1000).send_to_file_blocking(&out_file).expect("send_to_file_blocking");
    assert_eq!(written, expected_pcap.len() as u64);
    assert_eq!(std::fs::read(&out_file).unwrap(), expected_pcap);
    assert!(handle.join().expect("server").starts_with("GET /v1/captures/1000 HTTP/1.1\r\n"));
    std::fs::remove_file(&out_file).unwrap();

    // Bytes after Content-Length are not part of the body.
    let (client, _server) = serve_once(
        "HTTP/1.1 200 OK\r\nContent-Type: application/vnd.tcpdump.pcap\r\nContent-Length: 10\r\n\r\n1234567890extra",
    );
    let extra_file = tmp_dir.join(format!("netsim-rest-test-{pid}-extra.pcap"));
    let _ = std::fs::remove_file(&extra_file);
    let written =
        client.get_capture(1001).send_to_file_blocking(&extra_file).expect("send_to_file_blocking");
    assert_eq!(written, 10);
    assert_eq!(std::fs::read(&extra_file).unwrap(), b"1234567890");
    std::fs::remove_file(&extra_file).unwrap();
}

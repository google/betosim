// Copyright 2024 The Android Open Source Project
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

//! Tier 2 BDD scenarios for `http-proxy`.

use std::net::SocketAddr;

use crate::world::{ProxyMode, World};

// Scenario: HTTP CONNECT tunnel establishment and bidirectional payload relay on non-80 port
// Given a mock target TCP server on a non-80 port and a mock HTTP proxy in CONNECT tunnel mode
// And an unauthenticated Connector pointing to the mock proxy
// When the Connector connects to the target address and sends a payload through the tunnel
// Then the proxy receives the HTTP/1.1 CONNECT handshake without Proxy-Authorization
// And the target server receives the tunneled payload and the client receives the response
#[tokio::test]
async fn test_connect_tunnel_establishment_and_bidirectional_relay() {
    // Given
    let mut world = World::new();
    world.given_connect_tunnel(b"TLS-SERVER-HELLO", None).await;
    world.given_connector(None, None);

    // When
    world.when_connector_tunnels_payload(b"TLS-CLIENT-HELLO", b"TLS-SERVER-HELLO".len()).await;

    // Then
    world.then_connector_proxy_addr_matches();
    world
        .then_connect_tunnel_round_trip_succeeded(None, b"TLS-CLIENT-HELLO", b"TLS-SERVER-HELLO")
        .await;
}

// Scenario: CONNECT with standard Proxy-Authorization Basic credentials
// Given a mock target server and a mock proxy requiring Basic auth for "user:password"
// And a Connector configured with username "user" and password "password"
// When the Connector connects to the target and exchanges data over the tunnel
// Then the proxy receives the CONNECT request with Proxy-Authorization: Basic dXNlcjpwYXNzd29yZA==
// And the tunneled payload round-trip succeeds
#[tokio::test]
async fn test_connect_with_basic_auth_credentials() {
    // Given
    let mut world = World::new();
    let expected_auth = "Proxy-Authorization: Basic dXNlcjpwYXNzd29yZA==\r\n";
    world.given_connect_tunnel(b"AUTH-TUNNEL-OK", Some(expected_auth)).await;
    world.given_connector(Some("user"), Some("password"));

    // When
    world.when_connector_tunnels_payload(b"PING", b"AUTH-TUNNEL-OK".len()).await;

    // Then
    world
        .then_connect_tunnel_round_trip_succeeded(Some(expected_auth), b"PING", b"AUTH-TUNNEL-OK")
        .await;
}

// Scenario: CONNECT with bare username defaults to empty password per RFC 7617
// Given a mock target server and a mock proxy requiring Basic auth for "user:" (dXNlcjo=)
// And a Connector configured with username "user" and no password
// When the Connector connects to the target and exchanges data over the tunnel
// Then the proxy receives Proxy-Authorization: Basic dXNlcjo= and relays the payload
#[tokio::test]
async fn test_connect_with_bare_username_empty_password() {
    // Given
    let mut world = World::new();
    world
        .given_connect_tunnel(b"BARE-USER-OK", Some("Proxy-Authorization: Basic dXNlcjo=\r\n"))
        .await;
    world.given_connector(Some("user"), None);

    // When
    world.when_connector_tunnels_payload(b"HELLO", b"BARE-USER-OK".len()).await;

    // Then
    world.then_connector_proxy_addr_matches();
    world
        .then_connect_tunnel_round_trip_succeeded(
            Some("Proxy-Authorization: Basic dXNlcjo=\r\n"),
            b"HELLO",
            b"BARE-USER-OK",
        )
        .await;
}

// Scenario: CONNECT rejects with Error::ConnectionError on 407 Proxy Authentication Required
// Given a mock proxy that replies with HTTP/1.1 407 Proxy Authentication Required
// And a Connector configured with invalid credentials
// When the Connector attempts to connect to a non-80 target address
// Then Connector::connect returns Err(Error::ConnectionError(target_addr, status_line))
#[tokio::test]
async fn test_connect_rejected_with_407_proxy_auth_required() {
    // Given
    let mut world = World::new();
    world.given_mock_proxy(ProxyMode::Reject407).await;
    world.given_connector(Some("user"), Some("wrong-secret"));
    let target_addr: SocketAddr = "127.0.0.1:8443".parse().expect("valid socket addr");

    // When
    world.when_connector_connects_to(target_addr).await;

    // Then
    world.then_connect_failed_with_connection_error(
        target_addr,
        "HTTP/1.1 407 Proxy Authentication Required",
    );
}

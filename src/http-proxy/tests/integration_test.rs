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

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use crate::world::{DnsAnswerRecord, DnsQr, ProxyMode, World};

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

// Scenario: Port-80 HTTP GET request rewriting to absolute-form with Proxy-Authorization
// Given a mock HTTP proxy server and a Connector configured with Basic credentials
// When the Connector connects to port 80 and sends an origin-form GET request
// Then the proxy receives an absolute-form GET http://example.com/... request with Basic auth
// And the client receives the upstream HTTP response
#[tokio::test]
async fn test_port_80_get_rewrite_with_proxy_authorization() {
    // Given
    let mut world = World::new();
    let upstream_reply = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK";
    world
        .given_http_upstream_proxy_and_connector(upstream_reply, Some("user"), Some("password"))
        .await;

    // When
    let get_request =
        b"GET /index.html?lang=en HTTP/1.1\r\nHost: example.com\r\nUser-Agent: netsim\r\n\r\n";
    world.when_port_80_request_sent(get_request, upstream_reply.len()).await;

    // Then
    let expected_rewritten = "GET http://example.com/index.html?lang=en HTTP/1.1\r\n\
Host: example.com\r\n\
User-Agent: netsim\r\n\
Proxy-Authorization: Basic dXNlcjpwYXNzd29yZA==\r\n\r\n";
    world.then_connector_proxy_addr_matches();
    world.then_port_80_exchange_succeeded(expected_rewritten, upstream_reply).await;
}

// Scenario: Port-80 HTTP POST rewriting with case-insensitive Host header and body forwarding
// Given a mock HTTP proxy server and an unauthenticated Connector
// When the Connector connects to port 80 and sends a POST request with mixed-case "hOsT:" and body
// Then the proxy receives the absolute-form POST request and full body
// And the client receives the proxy's response body
#[tokio::test]
async fn test_port_80_post_rewrite_and_body_forwarding() {
    // Given
    let mut world = World::new();
    let upstream_reply = b"HTTP/1.1 201 Created\r\nContent-Length: 7\r\n\r\ncreated";
    world.given_http_upstream_proxy_and_connector(upstream_reply, None, None).await;

    // When
    let post_request = "POST /v1/items HTTP/1.1\r\n\
hOsT:   api.service.internal  \r\n\
Content-Length: 11\r\n\r\n\
item=widget";
    world.when_port_80_request_sent(post_request.as_bytes(), upstream_reply.len()).await;

    // Then
    let expected_rewritten = "POST http://api.service.internal/v1/items HTTP/1.1\r\n\
hOsT:   api.service.internal  \r\n\
Content-Length: 11\r\n\r\n\
item=widget";
    world.then_port_80_exchange_succeeded(expected_rewritten, upstream_reply).await;
    world.then_connector_proxy_addr_matches();
}

// Scenario: Port-80 HTTP requests with missing or empty Host header pass through unchanged
// Given a mock HTTP proxy server and an unauthenticated Connector
// When the Connector connects to port 80 and sends requests with missing or empty Host headers
// Then both requests pass through to the proxy without rewriting the origin-form request line
#[tokio::test]
async fn test_port_80_missing_and_empty_host_header_passthrough() {
    // Given
    let mut world = World::new();
    let upstream_reply = b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\npass";
    world.given_http_upstream_proxy_and_connector(upstream_reply, None, None).await;

    // When (missing Host header)
    let no_host_req = "GET /healthz HTTP/1.1\r\nX-Probe: 1\r\n\r\n";
    world.when_port_80_request_sent(no_host_req.as_bytes(), upstream_reply.len()).await;

    // Then
    world.then_port_80_exchange_succeeded(no_host_req, upstream_reply).await;

    // When (empty Host header value)
    let empty_host_req = "GET /readyz HTTP/1.1\r\nHost:    \r\n\r\n";
    world.when_port_80_request_sent(empty_host_req.as_bytes(), upstream_reply.len()).await;

    // Then
    world.then_port_80_exchange_succeeded(empty_host_req, upstream_reply).await;
}

// Scenario: Manager::new with percent-encoded credentials and ProxyManager::try_connect raw fd
// Given a mock target server and a mock proxy requiring decoded Basic auth ("user name:pass/word")
// And a Manager initialized with "http://user%20name:pass%2fword@<proxy_addr>"
// When ProxyManager::try_connect is called with a ProxyConnect callback and the connection is removed
// Then the callback receives fd >= 0 and the target SocketAddr
// And the proxy received the CONNECT request with decoded Basic auth credentials
#[tokio::test]
async fn test_manager_try_connect_success_with_percent_encoded_auth_and_raw_fd() {
    // Given
    let mut world = World::new();
    let expected_auth = "Proxy-Authorization: Basic dXNlciBuYW1lOnBhc3Mvd29yZA==\r\n";
    world.given_connect_tunnel(b"SLIRP-RAW-FD-PONG", Some(expected_auth)).await;
    let proxy_url = format!("http://user%20name:pass%2fword@{}", world.proxy_addr());
    world.given_manager_with_config(&proxy_url);
    let target_addr = world.target_addr();

    // When
    world.when_manager_try_connect(target_addr, 42).await;
    world.when_manager_removes_connection(42);

    // Then
    world.then_callback_succeeded_for(target_addr);
    world.then_manager_connect_handshake_succeeded(Some(expected_auth)).await;
}

// Scenario: ProxyManager::try_connect delivers fd == -1 on 407 and Manager::new rejects bad config
// Given a mock proxy that rejects CONNECT with 407 Proxy Authentication Required
// And a Manager configured against that proxy
// When ProxyManager::try_connect is invoked
// Then the ProxyConnect callback receives fd == -1 and the requested target SocketAddr
// And Manager::new returns MalformedConfigString, InvalidPortNumber, or InvalidHost for bad configs
#[tokio::test]
async fn test_manager_try_connect_failure_and_invalid_config_rejection() {
    // Given
    let mut world = World::new();
    world.given_mock_proxy(ProxyMode::Reject407).await;
    let proxy_url = format!("http://{}", world.proxy_addr());
    world.given_manager_with_config(&proxy_url);
    let target_addr: SocketAddr = "127.0.0.1:9443".parse().expect("valid target addr");

    // When
    world.when_manager_try_connect(target_addr, 7).await;

    // Then
    world.then_callback_failed_for(target_addr);

    // When / Then (malformed config strings)
    world.given_manager_with_config("http://");
    world.then_manager_failed_with_malformed_config();

    world.given_manager_with_config("127.0.0.1:99999");
    world.then_manager_failed_with_invalid_port();

    world.given_manager_with_config("256.0.0.1:8080");
    world.then_manager_failed_with_invalid_host();
}

// Scenario: DnsManager learns A and AAAA records (including RFC 1035 pointers) and ignores queries
// Given an initially empty DnsManager
// When Ethernet+IPv4+UDP(src=53) DNS responses with A, AAAA, and compressed-label records arrive
// And DNS queries (QR=0), non-port-53 UDP packets, and non-UDP frames are fed
// Then DnsManager caches only valid port-53 DNS response A and AAAA mappings and resolves FQDNs
#[tokio::test]
async fn test_dns_manager_learns_a_and_aaaa_records_and_ignores_non_responses() {
    // Given
    let mut world = World::new();
    world.given_dns_manager();
    world.then_dns_cache_len_is(0);

    let ipv4_example = Ipv4Addr::new(93, 184, 216, 34);
    let ipv4_sub = Ipv4Addr::new(93, 184, 216, 35);
    let ipv6_example: Ipv6Addr =
        "2606:2800:220:1:248:1893:25c8:1946".parse().expect("valid ipv6 addr");
    let ignored_ip = Ipv4Addr::new(198, 51, 100, 99);

    // When (feed A + compressed-prefix A via add_from_ethernet_slice, and AAAA via add_from_packet)
    world.when_dns_ethernet_slice_received(
        53,
        DnsQr::Response,
        "example.com",
        &[
            DnsAnswerRecord::A(ipv4_example),
            DnsAnswerRecord::PrefixedA { prefix_label: "api", ip: ipv4_sub },
        ],
    );
    world.when_dns_parsed_response_packet_received(
        "ipv6.example.org",
        DnsAnswerRecord::Aaaa(ipv6_example),
    );

    // And feed a DNS query (QR=0 on port 53), a non-port-53 UDP response, and non-UDP frames
    world.when_dns_ethernet_slice_received(
        53,
        DnsQr::Query,
        "query-only.example.net",
        &[DnsAnswerRecord::A(ignored_ip)],
    );
    world.when_dns_ethernet_slice_received(
        5353,
        DnsQr::Response,
        "mdns.example.local",
        &[DnsAnswerRecord::A(ignored_ip)],
    );
    world.when_non_udp_and_truncated_frames_received();

    // Then
    world.then_dns_cache_len_is(3);
    world.then_dns_lookup_is(IpAddr::V4(ipv4_example), Some("example.com"));
    world.then_dns_lookup_is(IpAddr::V4(ipv4_sub), Some("api.example.com"));
    world.then_dns_lookup_is(IpAddr::V6(ipv6_example), Some("ipv6.example.org"));
    world.then_dns_lookup_is(IpAddr::V4(ignored_ip), None);
}

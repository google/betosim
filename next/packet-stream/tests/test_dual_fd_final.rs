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

#[cfg(all(target_os = "linux", feature = "cuttlefish"))]
mod tests {
    //=============================================================================
    // tests/test_dual_fd_final.rs - DualFd transport tests (Public API only)
    //=============================================================================

    use packet_stream::{TransportType, transport::dual_fd::DualFdConfig};

    /// Tests for DualFd transport focusing on what's accessible through public
    /// API
    ///
    /// Note: DualFd is designed for pre-existing file descriptors passed from
    /// parent processes (like Cuttlefish). Most functionality requires internal
    /// APIs.

    #[test]
    fn test_dual_fd_transport_creation() {
        // Test that we can create DualFd TransportType instances

        // Test dual FD (separate input and output)
        let dual_fd_transport = TransportType::fd(10, Some(11));
        assert_eq!(dual_fd_transport.description(), "FD 10:11");
        assert!(dual_fd_transport.supports_stream());
        assert!(!dual_fd_transport.supports_listener()); // FDs cannot create listeners

        // Test single FD (bidirectional)
        let single_fd_transport = TransportType::fd(12, None);
        assert_eq!(single_fd_transport.description(), "FD 12");
        assert!(single_fd_transport.supports_stream());
        assert!(!single_fd_transport.supports_listener());
    }

    #[test]
    fn test_dual_fd_config_parsing() {
        // Test JSON configuration parsing for Cuttlefish integration
        let json_config = r#"
    {
        "devices": [
            {
                "name": "emulator-5554",
                "chips": [
                    {
                        "kind": "BLUETOOTH",
                        "fdIn": 10,
                        "fdOut": 11,
                        "model": "Bluetooth Controller"
                    },
                    {
                        "kind": "WIFI",
                        "fdIn": 12,
                        "fdOut": 13,
                        "model": "WiFi Controller"
                    }
                ]
            }
        ]
    }
    "#;

        // Parse the configuration (this would normally be done by Cuttlefish)
        let dual_fd_config: DualFdConfig = serde_json::from_str(json_config).unwrap();

        // Verify configuration parsing
        assert_eq!(dual_fd_config.devices.len(), 1);
        let device = &dual_fd_config.devices[0];
        assert_eq!(device.name, "emulator-5554");
        assert_eq!(device.chips.len(), 2);

        // Verify Bluetooth chip config
        let bt_chip = &device.chips[0];
        assert_eq!(bt_chip.kind, "BLUETOOTH");
        assert_eq!(bt_chip.fd_in, 10);
        assert_eq!(bt_chip.fd_out, Some(11));
        assert_eq!(bt_chip.model, Some("Bluetooth Controller".to_string()));

        // Verify WiFi chip config
        let wifi_chip = &device.chips[1];
        assert_eq!(wifi_chip.kind, "WIFI");
        assert_eq!(wifi_chip.fd_in, 12);
        assert_eq!(wifi_chip.fd_out, Some(13));
        assert_eq!(wifi_chip.model, Some("WiFi Controller".to_string()));
    }

    #[test]
    fn test_dual_fd_config_single_device_multiple_chips() {
        // Test configuration with multiple chips per device (realistic Cuttlefish
        // scenario)
        let json_config = r#"
    {
        "devices": [
            {
                "name": "test-device-001",
                "chips": [
                    {
                        "kind": "BLUETOOTH",
                        "fdIn": 100,
                        "fdOut": 101
                    },
                    {
                        "kind": "WIFI",
                        "fdIn": 102,
                        "fdOut": 103
                    },
                    {
                        "kind": "UWB",
                        "fdIn": 104,
                        "fdOut": 105,
                        "model": "UWB Chip v2.0"
                    }
                ]
            }
        ]
    }
    "#;

        let config: DualFdConfig = serde_json::from_str(json_config).unwrap();

        assert_eq!(config.devices.len(), 1);
        let device = &config.devices[0];
        assert_eq!(device.name, "test-device-001");
        assert_eq!(device.chips.len(), 3);

        // Verify all chips parsed correctly
        let chips = &device.chips;
        assert_eq!(chips[0].kind, "BLUETOOTH");
        assert_eq!(chips[0].fd_in, 100);
        assert_eq!(chips[0].fd_out, Some(101));

        assert_eq!(chips[1].kind, "WIFI");
        assert_eq!(chips[1].fd_in, 102);
        assert_eq!(chips[1].fd_out, Some(103));

        assert_eq!(chips[2].kind, "UWB");
        assert_eq!(chips[2].fd_in, 104);
        assert_eq!(chips[2].fd_out, Some(105));
        assert_eq!(chips[2].model, Some("UWB Chip v2.0".to_string()));
    }

    #[test]
    fn test_dual_fd_config_multiple_devices() {
        // Test configuration with multiple devices (multi-instance Cuttlefish)
        let json_config = r#"
    {
        "devices": [
            {
                "name": "emulator-5554",
                "chips": [
                    {
                        "kind": "BLUETOOTH",
                        "fdIn": 10,
                        "fdOut": 11
                    }
                ]
            },
            {
                "name": "emulator-5556",
                "chips": [
                    {
                        "kind": "WIFI",
                        "fdIn": 20,
                        "fdOut": 21
                    }
                ]
            }
        ]
    }
    "#;

        let config: DualFdConfig = serde_json::from_str(json_config).unwrap();

        assert_eq!(config.devices.len(), 2);

        // First device
        let device1 = &config.devices[0];
        assert_eq!(device1.name, "emulator-5554");
        assert_eq!(device1.chips.len(), 1);
        assert_eq!(device1.chips[0].kind, "BLUETOOTH");
        assert_eq!(device1.chips[0].fd_in, 10);
        assert_eq!(device1.chips[0].fd_out, Some(11));

        // Second device
        let device2 = &config.devices[1];
        assert_eq!(device2.name, "emulator-5556");
        assert_eq!(device2.chips.len(), 1);
        assert_eq!(device2.chips[0].kind, "WIFI");
        assert_eq!(device2.chips[0].fd_in, 20);
        assert_eq!(device2.chips[0].fd_out, Some(21));
    }

    #[test]
    fn test_dual_fd_config_optional_fields() {
        // Test configuration with optional fields missing
        let json_config = r#"
    {
        "devices": [
            {
                "name": "minimal-device",
                "chips": [
                    {
                        "kind": "BLUETOOTH",
                        "fdIn": 50,
                        "fdOut": 51
                    }
                ]
            }
        ]
    }
    "#;

        let config: DualFdConfig = serde_json::from_str(json_config).unwrap();

        let chip = &config.devices[0].chips[0];
        assert_eq!(chip.model, None); // Optional field should be None
        assert_eq!(chip.fd_out, Some(51)); // fdOut is present
    }

    #[test]
    fn test_dual_fd_config_single_fd() {
        // Test configuration with only input FD (bidirectional mode)
        let json_config = r#"
    {
        "devices": [
            {
                "name": "single-fd-device",
                "chips": [
                    {
                        "kind": "BLUETOOTH",
                        "fdIn": 60
                    }
                ]
            }
        ]
    }
    "#;

        let config: DualFdConfig = serde_json::from_str(json_config).unwrap();

        let chip = &config.devices[0].chips[0];
        assert_eq!(chip.fd_in, 60);
        assert_eq!(chip.fd_out, None); // Should be None when not specified
    }

    #[test]
    fn test_dual_fd_transport_type_equality() {
        // Test TransportType equality for DualFd
        let transport1 = TransportType::fd(10, Some(11));
        let transport2 = TransportType::fd(10, Some(11));
        let transport3 = TransportType::fd(10, Some(12)); // Different output FD
        let transport4 = TransportType::fd(11, Some(11)); // Different input FD

        // Note: TransportType may not implement PartialEq, so we test descriptions
        assert_eq!(transport1.description(), transport2.description());
        assert_ne!(transport1.description(), transport3.description());
        assert_ne!(transport1.description(), transport4.description());
    }

    #[test]
    fn test_dual_fd_config_serialization_round_trip() {
        // Test that we can serialize and deserialize DualFd configuration

        let original_config =
            r#"{"devices":[{"name":"test","chips":[{"kind":"BLUETOOTH","fdIn":10,"fdOut":11}]}]}"#;

        // Parse JSON -> Config
        let config: DualFdConfig = serde_json::from_str(original_config).unwrap();

        // Config -> JSON
        let serialized = serde_json::to_string(&config).unwrap();

        // Parse again to verify round-trip
        let config2: DualFdConfig = serde_json::from_str(&serialized).unwrap();

        // Verify round-trip preserved data
        assert_eq!(config.devices.len(), config2.devices.len());
        assert_eq!(config.devices[0].name, config2.devices[0].name);
        assert_eq!(config.devices[0].chips[0].kind, config2.devices[0].chips[0].kind);
        assert_eq!(config.devices[0].chips[0].fd_in, config2.devices[0].chips[0].fd_in);
        assert_eq!(config.devices[0].chips[0].fd_out, config2.devices[0].chips[0].fd_out);
    }

    #[test]
    fn test_dual_fd_config_aliases() {
        let vsock_json = r#"{"devices":[{"name":"cvd-1","chips":[{"kind":"CELLULAR","vsockFd":42,"simType":1}]}]}"#;
        let config: DualFdConfig = serde_json::from_str(vsock_json).unwrap();
        assert_eq!(config.devices[0].chips[0].fd_in, 42);
        assert_eq!(config.devices[0].chips[0].sim_type, Some(1));

        let virtio_json =
            r#"{"devices":[{"name":"cvd-1","chips":[{"kind":"CELLULAR","virtioFd":99}]}]}"#;
        let config2: DualFdConfig = serde_json::from_str(virtio_json).unwrap();
        assert_eq!(config2.devices[0].chips[0].fd_in, 99);
    }
}

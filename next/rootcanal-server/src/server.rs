// Copyright 2026 The Android Open Source Project
// SPDX-License-Identifier: Apache-2.0

use std::io;

use device_actor::DeviceClient;
use netsim_model::{
    BluetoothUpdate, ChipKind, ChipUpdate, ChipVariantUpdate, DeviceConfig, DeviceId, DeviceUpdate,
    Pose, RadioUpdate,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tracing::{error, info, warn};

/// Starts the asynchronous TCP server loop.
///
/// Accepts incoming connections from host-side test runners and spawns
/// a new Tokio task to handle each client session concurrently.
pub async fn run(listener: TcpListener, device_client: DeviceClient) {
    let addr =
        listener.local_addr().map(|a| a.to_string()).unwrap_or_else(|_| "unknown".to_string());
    info!("Rootcanal test channel server started on {addr}");
    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                info!("Accepted Rootcanal control connection from {addr}");
                let client = device_client.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_client(stream, client).await {
                        error!("Error handling Rootcanal client {addr}: {e}");
                    }
                    info!("Rootcanal control connection closed for {addr}");
                });
            }
            Err(e) => {
                error!("Error accepting Rootcanal control connection: {e}");
                // Mitigate tight CPU spin in case of persistent accept errors (e.g. EMFILE)
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }
}

/// Represents the physical radio medium to be toggled.
///
/// Used to map the legacy integer-based PHY index (0 for LE, 1 for Classic)
/// to the strongly-typed radio categories.
enum PhyTarget {
    LowEnergy, // "0"
    Classic,   // "1"
    All,       // Fallback for other values
}

impl From<&str> for PhyTarget {
    fn from(s: &str) -> Self {
        match s {
            "0" => PhyTarget::LowEnergy,
            "1" => PhyTarget::Classic,
            _ => PhyTarget::All,
        }
    }
}

/// Strongly-typed representation of Rootcanal test channel commands.
enum Command {
    List,
    PhyStateUpdate {
        device_id: u32,
        target: PhyTarget,
        raw_phy_str: String, // Necessary for the classic response string
        enabled: bool,
        command_name: String,
    },
    AddDevice {
        device_type: String,
        address: String,
    },
    DelDevice {
        device_id: u32,
    },
    Reset,
    SetDeviceConfiguration {
        device_id: u32,
        preset: String,
    },
    CloseTestChannel,
    Unknown {
        name: String,
    },
}

/// Parses and validates a raw TCP packet's command name and arguments.
///
/// Performs arity checks and parses string arguments into their strongly-typed
/// representations, returning a `Command` or a descriptive error message
/// suitable for sending back to the client.
fn parse_command(name: &str, args: &[String]) -> Result<Command, String> {
    // We match on the command name and the "shape" of the slice simultaneously
    match (name, args) {
        ("CLOSE_TEST_CHANNEL", []) => Ok(Command::CloseTestChannel),
        ("CLOSE_TEST_CHANNEL", _) => {
            Err("TestCommandHandler 'CLOSE_TEST_CHANNEL' takes no arguments".to_string())
        }

        ("list", []) => Ok(Command::List),
        ("list", _) => Err("TestCommandHandler 'list' takes no arguments".to_string()),

        ("reset", []) => Ok(Command::Reset),
        ("reset", _) => Err("TestCommandHandler 'reset' takes no arguments".to_string()),

        // Matches exactly two elements and binds them to `id_str` and `phy_str`
        ("add_device_to_phy" | "del_device_from_phy", [id_str, phy_str]) => {
            let device_id =
                id_str.parse::<u32>().map_err(|_err| "Invalid device_id".to_string())?;
            Ok(Command::PhyStateUpdate {
                device_id,
                target: PhyTarget::from(phy_str.as_str()),
                raw_phy_str: phy_str.clone(),
                enabled: name == "add_device_to_phy",
                command_name: name.to_string(),
            })
        }
        ("add_device_to_phy" | "del_device_from_phy", _) => {
            Err(format!("TestCommandHandler '{name}' takes two arguments"))
        }

        // Matches exactly one element
        ("del", [id_str]) => {
            let device_id =
                id_str.parse::<u32>().map_err(|_err| "Invalid device_id".to_string())?;
            Ok(Command::DelDevice { device_id })
        }
        ("del", _) => Err("TestCommandHandler 'del' takes an argument".to_string()),

        // Handles 1 argument OR 2 arguments cleanly using standard slice matching
        ("add", [device_type]) => {
            Ok(Command::AddDevice { device_type: device_type.clone(), address: String::new() })
        }
        ("add", [device_type, address]) => {
            Ok(Command::AddDevice { device_type: device_type.clone(), address: address.clone() })
        }
        ("add", _) => Err("TestCommandHandler 'add' takes an argument".to_string()),

        ("set_device_configuration", [id_str, preset_str]) => {
            let device_id =
                id_str.parse::<u32>().map_err(|_err| "Invalid device_id".to_string())?;
            Ok(Command::SetDeviceConfiguration { device_id, preset: preset_str.clone() })
        }
        ("set_device_configuration", _) => {
            Err("TestCommandHandler 'set_device_configuration' takes two arguments".to_string())
        }

        (other, _) => Ok(Command::Unknown { name: other.to_string() }),
    }
}

/// Manages the lifecycle of a single test channel session.
///
/// Performs the initial handshake, and then enters a read-parse-dispatch loop
/// until the client disconnects or sends a `CLOSE_TEST_CHANNEL` command.
async fn handle_client(mut stream: TcpStream, device_client: DeviceClient) -> io::Result<()> {
    let welcome = b"RootCanal\n";
    let size = welcome.len() as u32;
    stream.write_all(&size.to_le_bytes()).await?;
    stream.write_all(welcome).await?;
    stream.flush().await?;

    loop {
        let name_len = match stream.read_u8().await {
            Ok(len) => len as usize,
            Err(ref e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                // Client disconnected gracefully
                break;
            }
            Err(e) => return Err(e),
        };

        // Safe to use heap buffers here -- they are all 256 bytes or smaller.
        let mut name_buf = vec![0u8; name_len];
        stream.read_exact(&mut name_buf).await?;
        let name_str = String::from_utf8(name_buf)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        let args_len = stream.read_u8().await? as usize;

        let mut args = Vec::with_capacity(args_len);
        for _ in 0..args_len {
            let arg_len = stream.read_u8().await? as usize;

            let mut arg_buf = vec![0u8; arg_len];
            stream.read_exact(&mut arg_buf).await?;
            let arg_str = String::from_utf8(arg_buf)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            args.push(arg_str);
        }

        info!("Received Rootcanal control command: {name_str} with {args_len} args");

        let command = match parse_command(&name_str, &args) {
            Ok(cmd) => cmd,
            Err(err_msg) => {
                error!("Command '{name_str}' parse failed: {err_msg}");
                // Write the parser error back to the client and continue
                let response_bytes = err_msg.as_bytes();
                let resp_size = response_bytes.len() as u32;
                stream.write_all(&resp_size.to_le_bytes()).await?;
                stream.write_all(response_bytes).await?;
                stream.flush().await?;
                continue;
            }
        };

        let result = match command {
            Command::CloseTestChannel => break,
            Command::List => handle_list(&device_client).await.map(Some),
            Command::PhyStateUpdate { device_id, target, raw_phy_str, enabled, command_name } => {
                handle_phy_state(
                    &device_client,
                    device_id,
                    target,
                    &raw_phy_str,
                    enabled,
                    &command_name,
                )
                .await
                .map(Some)
            }
            Command::AddDevice { device_type, address } => {
                handle_add(&device_client, &device_type, &address).await.map(Some)
            }
            Command::DelDevice { device_id } => {
                handle_del(&device_client, device_id).await.map(Some)
            }
            Command::Reset => handle_reset(&device_client).await.map(|_| None),
            Command::SetDeviceConfiguration { device_id, preset } => {
                handle_set_device_configuration(&device_client, device_id, &preset).await.map(Some)
            }
            Command::Unknown { name } => {
                warn!("Unknown Rootcanal command received: {name}");
                Ok(None)
            }
        };

        let response_str = match result {
            Ok(Some(resp)) => resp,
            Ok(None) => "OK".to_string(),
            Err(err_msg) => {
                error!("Command '{name_str}' execution failed: {err_msg}");
                err_msg
            }
        };

        // Send response packet ([4-byte size][payload])
        let response_bytes = response_str.as_bytes();
        let resp_size = response_bytes.len() as u32;
        stream.write_all(&resp_size.to_le_bytes()).await?;
        stream.write_all(response_bytes).await?;
        stream.flush().await?;
    }

    Ok(())
}

/// Retrieves the list of all active simulated devices and their radio states,
/// formatting it to match the classic Rootcanal console output.
async fn handle_list(device_client: &DeviceClient) -> Result<String, String> {
    let devices = device_client.list().await.map_err(|e| format!("Failed to list devices: {e}"))?;

    let mut le_ids: Vec<u32> = Vec::new();
    let mut classic_ids: Vec<u32> = Vec::new();

    for device in &devices {
        if device
            .chips
            .iter()
            .any(|chip| chip.kind == ChipKind::BLUETOOTH && chip.enabled && chip.is_le_enabled())
        {
            le_ids.push(device.id);
        }

        if device.chips.iter().any(|chip| {
            chip.kind == ChipKind::BLUETOOTH && chip.enabled && chip.is_classic_enabled()
        }) {
            classic_ids.push(device.id);
        }
    }

    let devices_section: String = devices
        .iter()
        .map(|device| format!("  {}:hci_device_{}\n", device.id, device.id))
        .collect();
    let low_energy_section = le_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");
    let br_edr_section = classic_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");

    Ok(format!(
        "Devices:\n{devices_section}Phys:\n  0:LOW_ENERGY:{low_energy_section}\n  1:BR_EDR:{br_edr_section}\n"
    ))
}

/// Toggles the enabled state of a specific radio (LE or Classic) on a device.
///
/// Sends a `DeviceUpdate` to the device actor to patch the chip's enabled
/// status, and returns a classic-compatible confirmation string.
async fn handle_phy_state(
    device_client: &DeviceClient,
    device_id: u32,
    target: PhyTarget,
    raw_phy_str: &str,
    enabled: bool,
    command_name: &str,
) -> Result<String, String> {
    let device_id_typed = DeviceId(device_id);
    let device = device_client
        .get(device_id_typed)
        .await
        .map_err(|e| format!("Failed to get device: {e}"))?
        .ok_or_else(|| format!("Device {device_id} not found"))?;

    let chip_updates: Vec<ChipUpdate> = device
        .chips
        .iter()
        .filter(|chip| chip.kind == ChipKind::BLUETOOTH)
        .map(|chip| {
            let mut update = ChipUpdate { id: Some(chip.id.into()), ..Default::default() };
            match target {
                PhyTarget::LowEnergy => {
                    update.variant = Some(ChipVariantUpdate::Bluetooth(BluetoothUpdate {
                        low_energy: RadioUpdate { state: Some(enabled) },
                        ..Default::default()
                    }));
                }
                PhyTarget::Classic => {
                    update.variant = Some(ChipVariantUpdate::Bluetooth(BluetoothUpdate {
                        classic: RadioUpdate { state: Some(enabled) },
                        ..Default::default()
                    }));
                }
                PhyTarget::All => {
                    update.enabled = Some(enabled);
                }
            }
            update
        })
        .collect();

    if chip_updates.is_empty() {
        return Err(format!("No Bluetooth chips found on device {device_id}"));
    }

    let device_update =
        DeviceUpdate { id: device_id, chips: Some(chip_updates), ..Default::default() };

    device_client
        .update(device_id_typed, device_update)
        .await
        .map_err(|e| format!("Failed to update device: {e}"))?;

    info!("Successfully toggled Bluetooth chips to enabled={enabled} on device {device_id}");

    Ok(format!(
        "TestCommandHandler '{command_name}' called with device {device_id} and phy {raw_phy_str}"
    ))
}

/// Removes a device from the simulation by its ID.
async fn handle_del(device_client: &DeviceClient, device_id: u32) -> Result<String, String> {
    device_client.delete(DeviceId(device_id)).await.map_err(|e| format!("Delete failed: {e}"))?;

    Ok(format!("TestCommandHandler 'del' called with device at index {device_id}"))
}

/// Resets the entire simulation state, clearing all devices.
async fn handle_reset(device_client: &DeviceClient) -> Result<(), String> {
    device_client.reset(None).await.map_err(|e| format!("Reset failed: {e}"))?;
    Ok(())
}

/// Dynamically provisions a new virtual device with a Bluetooth chip.
///
/// The device is created with no local packet stream/sink, making it act
/// as a passive responder or tester in the simulation.
async fn handle_add(
    device_client: &DeviceClient,
    device_type: &str,
    address: &str,
) -> Result<String, String> {
    let device_guid = if address.is_empty() {
        format!("test-channel-{}", rand::random::<u32>())
    } else {
        format!("test-channel-{}", address.replace(':', "-"))
    };
    let mut device_config = DeviceConfig::new(device_guid.clone(), true, Pose::default(), false);
    device_config.device_info = Some(netsim_model::DeviceInfo {
        name: format!("test-{device_type}"),
        ..Default::default()
    });

    let add_chip_params = netsim_model::DeviceAddChip {
        device_guid: device_guid.clone(),
        packet_stream: None,
        packet_sink: None,
        device_config,
        chip: netsim_model::Chip {
            name: format!("chip_{device_guid}"),
            manufacturer: "Netsim".to_string(),
            product_name: "RootcanalTestChannel".to_string(),
            kind: ChipKind::BLUETOOTH,
            variant: Some(netsim_model::ChipVariant::Bluetooth(Box::new(
                netsim_model::Bluetooth {
                    address: address.to_string(),
                    mode: netsim_model::BluetoothMode::Device(Default::default()),
                    bt_properties: Default::default(),
                    ..Default::default()
                },
            ))),
            ..Default::default()
        },
    };

    let device_id =
        device_client.add_chip(add_chip_params).await.map_err(|e| format!("Add failed: {e}"))?;

    Ok(format!("{}:hci_device_{}", device_id.0, device_id.0))
}

/// Handles the 'set_device_configuration' command.
async fn handle_set_device_configuration(
    device_client: &DeviceClient,
    device_id: u32,
    preset: &str,
) -> Result<String, String> {
    let device_id_typed = DeviceId(device_id);
    let device = device_client
        .get(device_id_typed)
        .await
        .map_err(|e| format!("Failed to get device: {e}"))?
        .ok_or_else(|| format!("Device {device_id} not found"))?;

    let bluetooth_chips: Vec<_> =
        device.chips.iter().filter(|chip| chip.kind == ChipKind::BLUETOOTH).collect();

    if bluetooth_chips.is_empty() {
        return Err(format!("Device {device_id} has no Bluetooth chip"));
    }

    let chip_updates: Vec<ChipUpdate> = bluetooth_chips
        .iter()
        .map(|chip| ChipUpdate {
            id: Some(chip.id.into()),
            variant: Some(ChipVariantUpdate::Bluetooth(BluetoothUpdate {
                preset: Some(preset.to_string()),
                ..Default::default()
            })),
            ..Default::default()
        })
        .collect();

    let device_update =
        DeviceUpdate { id: device_id, chips: Some(chip_updates), ..Default::default() };

    device_client
        .update(device_id_typed, device_update)
        .await
        .map_err(|e| format!("Failed to update device configuration: {e}"))?;

    Ok(format!("set_device_configuration {device_id} {preset}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phy_target_from_str() {
        assert!(matches!(PhyTarget::from("0"), PhyTarget::LowEnergy));
        assert!(matches!(PhyTarget::from("1"), PhyTarget::Classic));
        assert!(matches!(PhyTarget::from("2"), PhyTarget::All));
        assert!(matches!(PhyTarget::from("invalid"), PhyTarget::All));
        assert!(matches!(PhyTarget::from(""), PhyTarget::All));
    }

    #[test]
    fn test_parse_command_close_test_channel() {
        assert!(matches!(parse_command("CLOSE_TEST_CHANNEL", &[]), Ok(Command::CloseTestChannel)));
        let err = parse_command("CLOSE_TEST_CHANNEL", &["extra".to_string()]);
        assert_eq!(
            err.err().as_deref(),
            Some("TestCommandHandler 'CLOSE_TEST_CHANNEL' takes no arguments")
        );
    }

    #[test]
    fn test_parse_command_list() {
        assert!(matches!(parse_command("list", &[]), Ok(Command::List)));
        let err = parse_command("list", &["arg".to_string()]);
        assert_eq!(err.err().as_deref(), Some("TestCommandHandler 'list' takes no arguments"));
    }

    #[test]
    fn test_parse_command_reset() {
        assert!(matches!(parse_command("reset", &[]), Ok(Command::Reset)));
        let err = parse_command("reset", &["arg1".to_string(), "arg2".to_string()]);
        assert_eq!(err.err().as_deref(), Some("TestCommandHandler 'reset' takes no arguments"));
    }

    #[test]
    fn test_parse_command_phy_state_update_valid() {
        let cmd = parse_command("add_device_to_phy", &["42".to_string(), "0".to_string()]).unwrap();
        match cmd {
            Command::PhyStateUpdate { device_id, target, raw_phy_str, enabled, command_name } => {
                assert_eq!(device_id, 42);
                assert!(matches!(target, PhyTarget::LowEnergy));
                assert_eq!(raw_phy_str, "0");
                assert!(enabled);
                assert_eq!(command_name, "add_device_to_phy");
            }
            _ => panic!("Expected PhyStateUpdate"),
        }

        let cmd =
            parse_command("del_device_from_phy", &["7".to_string(), "1".to_string()]).unwrap();
        match cmd {
            Command::PhyStateUpdate { device_id, target, raw_phy_str, enabled, command_name } => {
                assert_eq!(device_id, 7);
                assert!(matches!(target, PhyTarget::Classic));
                assert_eq!(raw_phy_str, "1");
                assert!(!enabled);
                assert_eq!(command_name, "del_device_from_phy");
            }
            _ => panic!("Expected PhyStateUpdate"),
        }
    }

    #[test]
    fn test_parse_command_phy_state_update_errors() {
        let err_invalid_id =
            parse_command("add_device_to_phy", &["not_a_number".to_string(), "0".to_string()]);
        assert_eq!(err_invalid_id.err().as_deref(), Some("Invalid device_id"));

        let err_bad_arity = parse_command("del_device_from_phy", &["42".to_string()]);
        assert_eq!(
            err_bad_arity.err().as_deref(),
            Some("TestCommandHandler 'del_device_from_phy' takes two arguments")
        );
    }

    #[test]
    fn test_parse_command_del() {
        let cmd = parse_command("del", &["123".to_string()]).unwrap();
        match cmd {
            Command::DelDevice { device_id } => assert_eq!(device_id, 123),
            _ => panic!("Expected DelDevice"),
        }

        let err_invalid_id = parse_command("del", &["bad".to_string()]);
        assert_eq!(err_invalid_id.err().as_deref(), Some("Invalid device_id"));

        let err_no_args = parse_command("del", &[]);
        assert_eq!(
            err_no_args.err().as_deref(),
            Some("TestCommandHandler 'del' takes an argument")
        );

        let err_too_many = parse_command("del", &["1".to_string(), "2".to_string()]);
        assert_eq!(
            err_too_many.err().as_deref(),
            Some("TestCommandHandler 'del' takes an argument")
        );
    }

    #[test]
    fn test_parse_command_add() {
        let cmd = parse_command("add", &["beacon".to_string()]).unwrap();
        match cmd {
            Command::AddDevice { device_type, address } => {
                assert_eq!(device_type, "beacon");
                assert!(address.is_empty());
            }
            _ => panic!("Expected AddDevice"),
        }

        let cmd =
            parse_command("add", &["beacon".to_string(), "00:11:22:33:44:55".to_string()]).unwrap();
        match cmd {
            Command::AddDevice { device_type, address } => {
                assert_eq!(device_type, "beacon");
                assert_eq!(address, "00:11:22:33:44:55");
            }
            _ => panic!("Expected AddDevice"),
        }

        let err_no_args = parse_command("add", &[]);
        assert_eq!(
            err_no_args.err().as_deref(),
            Some("TestCommandHandler 'add' takes an argument")
        );

        let err_too_many =
            parse_command("add", &["a".to_string(), "b".to_string(), "c".to_string()]);
        assert_eq!(
            err_too_many.err().as_deref(),
            Some("TestCommandHandler 'add' takes an argument")
        );
    }

    #[test]
    fn test_parse_command_set_device_configuration() {
        let cmd = parse_command(
            "set_device_configuration",
            &["10".to_string(), "strict_mode".to_string()],
        )
        .unwrap();
        match cmd {
            Command::SetDeviceConfiguration { device_id, preset } => {
                assert_eq!(device_id, 10);
                assert_eq!(preset, "strict_mode");
            }
            _ => panic!("Expected SetDeviceConfiguration"),
        }

        let err_bad_id = parse_command(
            "set_device_configuration",
            &["xyz".to_string(), "strict_mode".to_string()],
        );
        assert_eq!(err_bad_id.err().as_deref(), Some("Invalid device_id"));

        let err_bad_arity = parse_command("set_device_configuration", &["10".to_string()]);
        assert_eq!(
            err_bad_arity.err().as_deref(),
            Some("TestCommandHandler 'set_device_configuration' takes two arguments")
        );
    }

    #[test]
    fn test_parse_command_unknown() {
        let cmd = parse_command("custom_unknown_op", &["foo".to_string()]).unwrap();
        match cmd {
            Command::Unknown { name } => assert_eq!(name, "custom_unknown_op"),
            _ => panic!("Expected Unknown"),
        }
    }
}

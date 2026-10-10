# Command Line Interface for Netsim (netsim)

Usage:

- `netsim [Options] <COMMAND>`

Options:

- `-v, --verbose`: Set verbose mode
- `-p, --port <PORT>`: Set custom grpc port
- `    --vsock <VSOCK>`: Set vsock cid:port pair
- `-h, --help`: Print help information

## Commands:

- ### `version`: Print Netsim version information
  - Usage: `netsim version`
- ### `radio`: Control the radio state of a device
  - Usage: `netsim radio <RADIO_TYPE> <STATUS> <NAME>`
  - Arguments:
    - \<RADIO_TYPE\>: Radio type [possible values: ble, classic, wifi, uwb]
    - \<STATUS\>: Radio status [possible values: up, down]
    - \<NAME\>: Device name
- ### `move`: Set the device location
  - Usage: `netsim move <NAME> <X> <Y> [Z]`
  - Arguments:
    - \<NAME\>: Device name
    - \<X\>: x position of device
    - \<Y\>: y position of device
    - [Z]: Optional z position of device
- ### `devices`: Display device(s) information
  - Usage: `netsim devices [OPTIONS]`
  - Options:
    - `-c, --continuous`: Continuously print device(s) information every second
- ### `beacon`: A fixed simulated radio node (BLE beacon or UWB anchor) [aliases: anchor]
  - Usage: `netsim beacon <COMMAND>`
  - #### Commands:
    - `create`: Create a beacon
      - Usage: `netsim beacon create <COMMAND>`
        - ##### Commands:
          - `ble`: Create a Bluetooth low-energy beacon
            - Usage:
              `netsim beacon create ble [DEVICE_NAME | DEVICE_NAME CHIP_NAME] [OPTIONS]`
            - Arguments:
              - \[DEVICE_NAME\]: Optional name of the device to create. A
                default name will be generated if not supplied
              - \[CHIP_NAME\]: Optional name of the beacon to create within the
                new device. May only be specified if DEVICE_NAME is specified. A
                default name will be generated if not supplied
            - Advertisement Options:
              - `--advertise-mode <MODE>`: Set the advertise mode which controls
                the duration between advertisements
                - Possible values:
                  - `low-power`
                  - `balanced`
                  - `low-latency`
                  - A number measuring duration in milliseconds
              - `--tx-power-level <LEVEL>`: Set the beacon's transmission power
                level
                - Possible values:
                  - `ultra-low`
                  - `low`
                  - `medium`
                  - `high`
                  - A number measuring transmission power in dBm
              - `--scannable`: Set whether the beacon will respond to scan
                requests
              - `--timeout <MS>`: Limit advertising to an amount of time given
                in milliseconds
            - Advertise Packet Options:
              - `--include-device-name`: Set whether the device name should be
                included in the advertise packet
              - `--include-tx-power-level`: Set whether the transmit power level
                should be included in the advertise packet
              - `--manufacturer-data <DATA>`: Add manufacturer specific data to
                the advertise packet
          - `uwb`: Create an Ultra-wideband anchor
            - Usage:
              `netsim beacon create uwb [DEVICE_NAME] [CHIP_NAME] [OPTIONS]`
            - Arguments:
              - \[DEVICE_NAME\]: Optional name of the device to create. A
                default name will be generated if not supplied
              - \[CHIP_NAME\]: Optional name of the anchor chip to create within
                the new device. A default name will be generated if not supplied
            - Options:
              - `--address <ADDRESS>`: UWB short address of the anchor (e.g.
                00:01). Will be allocated if not provided
              - `--x <X>`: x position of device in meters
              - `--y <Y>`: y position of device in meters
              - `--z <Z>`: z position of device in meters
              - `--yaw <YAW>`: Yaw orientation of device in degrees
              - `--pitch <PITCH>`: Pitch orientation of device in degrees
              - `--roll <ROLL>`: Roll orientation of device in degrees
    - `patch`: Modify a beacon
      - Usage: `netsim beacon patch <COMMAND>`
        - ##### Commands:
          - `ble`: Modify a Bluetooth low-energy beacon
            - Usage:
              `netsim beacon patch ble <DEVICE_NAME> <CHIP_NAME> <OPTIONS>`
            - Arguments:
              - \<DEVICE_NAME\>: Name of the device that contains the beacon
              - \<CHIP_NAME\>: Name of the beacon to modify
            - Advertisement Options:
              - `--advertise-mode <MODE>`: Change the advertise mode which
                controls the duration between advertisements
                - Possible values:
                  - `low-power`
                  - `balanced`
                  - `low-latency`
                  - A number measuring duration in milliseconds
              - `--tx-power-level <LEVEL>`: Change the beacon's transmission
                power level
                - Possible values:
                  - `ultra-low`
                  - `low`
                  - `medium`
                  - `high`
                  - A number measuring transmission power in dBm
              - `--scannable`: Change whether the beacon will respond to scan
                requests
              - `--timeout <MS>`: Limit advertising to an amount of time given
                in milliseconds
            - Advertise Packet Options:
              - `--include-device-name`: Change whether the device name should
                be included in the advertise packet
              - `--include-tx-power-level`: Change whether the transmit power
                level should be included in the advertise packet
              - `--manufacturer-data <DATA>`: Change manufacturer specific data
                within the advertise packet
    - `remove`: Remove a beacon
      - Usage: `netsim beacon remove <DEVICE_NAME> [CHIP_NAME]`
      - Arguments:
        - \<DEVICE_NAME\>: Name of the device to remove
        - \[CHIP_NAME\]: Optional name of the beacon to remove
- ### `reset`: Reset Netsim device scene
  - Usage: `netsim reset`
- ### `capture`: Control the packet capture functionalities with commands: list, patch, get [aliases: pcap]
  - Usage: `netsim capture <COMMAND>`
  - #### Commands
    - `list`: List currently available Captures (packet captures)
      - Usage: `netsim capture list [PATTERNS]...`
      - Arguments:
        - [PATTERNS]...: Optional strings of pattern for captures to list.
          Possible filter fields include ID, Device Name, and Chip Kind
      - Options:
        - `-c, --continuous`: Continuously print Capture information every
          second
    - `patch`: Patch a Capture source to turn packet capture on/off
      - Usage: `netsim capture patch <STATE> [PATTERNS]...`
      - Arguments:
        - \<STATE\>: Packet capture state [possible values: on, off]
        - [PATTERNS]...: Optional strings of pattern for captures to patch.
          Possible filter fields include ID, Device Name, and Chip Kind
    - `get`: Download the packet capture content
      - Usage: `netsim capture get [OPTIONS] [PATTERNS]...`
      - Arguments:
        - [PATTERNS]...: Optional strings of pattern for captures to get.
          Possible filter fields include ID, Device Name, and Chip Kind
      - Options:
        - `-o, --location`: Directory to store downloaded capture file(s)
- ### `link`: Manage Link properties
  - Usage: `netsim link <COMMAND>`
  - #### Commands
    - `create`: Create a new link
      - Usage: `netsim link create <CHIP_KIND> [OPTIONS]`
      - Arguments:
        - \<CHIP_KIND\>: Chip kind for the link [possible values: bluetooth,
          wifi, uwb]
      - Options:
        - `--sender <ID>`: Identifier for the sender chip
        - `--receiver <ID>`: Identifier for the receiver chip
        - `-s, --sender-name <NAME>`: Name of the sender device
        - `-r, --receiver-name <NAME>`: Name of the receiver device
        - `--rssi <VALUE>`: RSSI value in dBm (e.g., -60)
    - `list`: List all current links and their properties
    - `patch`: Add or modify link properties
      - Usage: `netsim link patch <CHIP_KIND> [OPTIONS]`
      - Arguments:
        - \<CHIP_KIND\>: Chip kind for the link [possible values: bluetooth,
          wifi, uwb]
      - Options:
        - `--rssi <VALUE>`: RSSI value in dBm (e.g., -60). Must be between -128
          and 127
        - `--sender <ID>`: Identifier for the sender chip
        - `--receiver <ID>`: Identifier for the receiver chip
        - `-s, --sender-name <NAME>`: Name of the sender device
        - `-r, --receiver-name <NAME>`: Name of the receiver device
    - `delete`: Remove link properties
      - Usage: `netsim link delete <CHIP_KIND> [OPTIONS]`
      - Arguments:
        - \<CHIP_KIND\>: Chip kind for the link [possible values: bluetooth,
          wifi, uwb]
      - Options:
        - `--sender <ID>`: Identifier for the sender chip
        - `--receiver <ID>`: Identifier for the receiver chip
        - `-s, --sender-name <NAME>`: Name of the sender device
        - `-r, --receiver-name <NAME>`: Name of the receiver device
- ### `artifact`: Open netsim artifacts directory (log, pcaps)
- ### `bumble`: Open Bumble Hive Web Page
- ### `help`: Print this message or the help of the given subcommand(s)

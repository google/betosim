## Design Document: TAP Networking for Goldfish Emulator on gLinux via netsimd Gateway

**Status:** Draft **Author:** schilit@ **Last Updated:** 2026-02-07

**1. Objective**

To enable a streamlined and non-root TAP networking experience for the Goldfish
Android Emulator on gLinux development machines (desktops and Cloudtops) by
having the `netsimd` daemon manage and interface with a pool of TAP devices
created by the `cuttlefish-base` package. This will allow Goldfish instances to
have direct layer 2 network presence on the host.

**2. Background**

- **Cuttlefish Networking:** The `cuttlefish-base` package, commonly installed
  on gLinux machines used for Android development, sets up a sophisticated
  network environment for Cuttlefish virtual devices. Via the
  `/etc/init.d/cuttlefish-host-resources` script (run as root), it creates:
  - A pool of TAP network interfaces (e.g., `cvd-etap-01` to `cvd-etap-10`,
    `cvd-mtap-01`, etc.).
  - Sets device ownership to the `cvdnetwork` group, allowing non-root users in
    this group to access them.
  - Network bridges (e.g., `cvd-ebr`) to connect these TAP devices.
  - IP masquerading (NAT) via `iptables` for external network access from the
    bridge.
  - DHCP and DNS services on the bridge using `dnsmasq`.
- **Goldfish Networking:** The Goldfish emulator traditionally uses a user-space
  "slirp" network stack, which requires no special privileges but has
  performance limitations and doesn't provide true layer 2 presence.
- **Netsim Daemon (`netsimd`):** `netsimd` provides network
  simulation for both Cuttlefish and Goldfish. For Goldfish, it currently uses a
  gRPC transport to `libslirp-rs` within `netsimd` for external network access.
- **Internal Use Case:** Many internal Android developers have `cuttlefish-base`
  installed, making its network infrastructure available.

**3. Goal: Centralized TAP Management in `netsimd`**

`netsimd` will provide a "tap-gateway" option. When enabled for an emulator
session, `netsimd` will select, open, and route traffic through a host TAP
device from the pool created by `cuttlefish-base`. The emulator itself will not
need any specific TAP configuration in its launch command.

**4. Proposed Solution: Partitioned TAP Pool & `netsimd` TAP I/O**

- **Device Partitioning:** To avoid conflicts with Cuttlefish instances,
  `netsimd` will be configured to use a specific sub-range of the available
  `cvd-etap-XX` devices (e.g., indices 6-10, while Cuttlefish uses 1-5).
- **`netsimd` TapGatewayManager:** A component within `netsimd` will manage the
  allocation of TAP devices from Goldfish's assigned range.
- **Emulator Configuration:** The Goldfish emulator will be launched _without_
  any `-netdev tap` arguments. Its network stack will be configured to send all
  packets to `netsimd` through the existing PacketStreamer gRPC channel, just as
  it does for the Slirp gateway. The choice of gateway (slirp vs. tap) will be a
  configuration parameter for the netsim session, potentially toggled by an
  emulator feature flag like `-feature NetsimTapGateway`.
- **`netsimd` TAP Handling:**
  1.  When an emulator connection is established with the `NetsimTapGateway`
      feature enabled, the `TapGatewayManager` allocates an available TAP device
      from its range (e.g., `cvd-etap-08`).
  2.  `netsimd` finds the corresponding device node (e.g., `/dev/tap8`) and
      **opens a file descriptor** to it. This is permissible because the user
      running `netsimd` is in the `cvdnetwork` group (see Appendix A).
  3.  A dedicated task/thread within `netsimd` is spawned for this emulator's
      network session.
  4.  This task continuously shuttles Ethernet frames:
      - Reads from the gRPC stream coming from the emulator, writes to the TAP
        file descriptor.
      - Reads from the TAP file descriptor, writes to the gRPC stream going to
        the emulator.

**5. `netsimd` TapGatewayManager Implementation (Conceptual Rust)**

```rust
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::sync::{Arc, Mutex};
// Add necessary crates for async I/O (e.g., tokio) and TUN/TAP device handling

#[derive(Debug, Clone, PartialEq)]
enum DeviceStatus {
    Available,
    InUse,
}

#[derive(Debug, Clone)]
struct TapDeviceState {
    status: DeviceStatus,
    user: Option<String>, // emulator_id
    device_node: Option<String>, // e.g., /dev/tap8
    fd: Option<i32>, // Raw file descriptor
}

pub struct TapGatewayManager {
    tap_device_prefix: String,
    device_indices: std::ops::RangeInclusive<u32>,
    device_pool: Arc<Mutex<HashMap<String, TapDeviceState>>>,
}

impl TapGatewayManager {
    pub fn new(tap_device_prefix: String, min_idx: u32, max_idx: u32) -> Self {
        // ... Initialization as before ...
        let mut pool = HashMap::new();
        for i in min_idx..=max_idx {
            let device_name = format!("{}-{:02}", tap_device_prefix, i);
            // This mapping is an assumption, might need to query system to be sure
            let device_node = format!("/dev/tap{}", i);
            pool.insert(
                device_name,
                TapDeviceState {
                    status: DeviceStatus::Available,
                    user: None,
                    device_node: Some(device_node),
                    fd: None,
                },
            );
        }
        // ...
        Self {
            tap_device_prefix,
            device_indices: min_idx..=max_idx,
            device_pool: Arc::new(Mutex::new(pool)),
        }
    }

    fn open_tap_device(node_path: &str) -> Option<std::fs::File> {
        match OpenOptions::new().read(true).write(true).open(node_path) {
            Ok(file) => Some(file),
            Err(e) => {
                eprintln!("Failed to open TAP device {}: {}", node_path, e);
                None
            }
        }
    }

    pub fn request_tap_device(&self, user_id: &str) -> Option<(String, std::fs::File)> {
        let mut pool = self.device_pool.lock().unwrap();
        for i in self.device_indices.clone().rev() {
            let device_name = format!("{}-{:02}", self.tap_device_prefix, i);
            if let Some(state) = pool.get_mut(&device_name) {
                if state.status == DeviceStatus::Available {
                    if let Some(node_path) = &state.device_node {
                        if let Some(file) = Self::open_tap_device(node_path) {
                            state.status = DeviceStatus::InUse;
                            state.user = Some(user_id.to_string());
                            // state.fd = Some(file.as_raw_fd()); // Don't store raw fd, store File
                            println!("Allocated and opened TAP device {} ({}) for {}", device_name, node_path, user_id);
                            return Some((device_name.clone(), file));
                        } else {
                            eprintln!("Failed to open {}, skipping", node_path);
                            // Potentially mark as problematic
                        }
                    }
                }
            }
        }
        eprintln!("No available TAP devices in the range {:?} for {}", self.device_indices, user_id);
        None
    }

    pub fn release_tap_device(&self, device_name: &str, user_id: &str) {
        let mut pool = self.device_pool.lock().unwrap();
        if let Some(state) = pool.get_mut(device_name) {
            if state.status == DeviceStatus::InUse && state.user.as_deref() == Some(user_id) {
                state.status = DeviceStatus::Available;
                state.user = None;
                state.fd = None; // FD is closed when 'File' is dropped
                println!("Released TAP device {} from {}", device_name, user_id);
            }
        }
    }

    // This function would be called by the main netsimd event loop
    // when a new emulator-netsim gRPC session is established.
    pub fn handle_emulator_connection(&self, emulator_id: &str) {
        if let Some((device_name, tap_file)) = self.request_tap_device(emulator_id) {
            println!("Netsimd will bridge packets between {} and {}", emulator_id, device_name);
            // Spawn an async task to handle packet shuttling for this session
            // using tap_file and the emulator's gRPC stream.
            // e.g., tokio::spawn(async move { shuttle_packets(emulator_id, tap_file, grpc_stream).await });
        } else {
            // Handle allocation failure - perhaps fall back to slirp?
        }
    }
}
```

**6. Advantages of this Model**

- **Zero User Setup:** Users with `cuttlefish-base` installed get TAP networking
  for Goldfish without any manual `ip`, `brctl`, or `sudo` commands beyond the
  initial package install.
- **Leverages Existing Setup:** Reuses the robust bridging, NAT, and DHCP from
  `cuttlefish-base`.
- **`cvdnetwork` Group:** Utilizes the existing group permissions for secure
  access to TAP devices by `netsimd`.
- **Emulator Agnostic:** The emulator launch process is identical regardless of
  whether slirp or TAP gateway is used. The choice is made at the `netsimd`
  level.
- **Centralized Control:** All host network interface logic is within `netsimd`.
- **Clean Separation:** Follows the "netsim manages the packet flows" principle.

**7. Coexistence Strategy & Assumptions**

- **Partitioning:** Still relies on the index partitioning (e.g., CF 1-5,
  Goldfish 6-10) to avoid fundamental conflicts.
- **Error Handling:** If `netsimd` fails to open a TAP device (e.g., because
  Cuttlefish _is_ unexpectedly using it), it can handle this internally,
  potentially trying the next device in its range or logging a clear error. The
  emulator doesn't crash, it just might not get TAP networking.

**8. Open Issues**

- **Device Node Mapping:** The mapping from `cvd-etap-XX` to `/dev/tapY` is
  assumed to be consistent (e.g., index `I` maps to `/dev/tapI`). This should be
  verified on gLinux.
- **Performance:** The overhead of shuttling packets between the gRPC stream and
  the TAP file descriptor in `netsimd` needs to be evaluated.

---

**Appendix A: Host Setup for Cuttlefish TAP Devices**

To ensure the TAP devices and network bridges are created on the host, the
`cuttlefish-common` package must be installed. This package includes
`cuttlefish-base`. The following commands should be run on the gLinux
workstation:

1.  **Add the Cuttlefish repository:**

    ```bash
    sudo glinux-add-repo android-cuttlefish stable
    ```

2.  **Update the package list:**

    ```bash
    sudo apt update
    ```

3.  **Install the package:**

    ```bash
    sudo apt install cuttlefish-common
    ```

4.  **Add your user to the necessary groups:**

    ```bash
    sudo usermod -aG kvm,cvdnetwork,render $USER
    ```

5.  **Apply group changes and ensure services start:** It's best to reboot to
    ensure all changes take effect, including group memberships and the
    automatic start of the `cuttlefish-host-resources` service.
    ```bash
    sudo reboot
    ```
    Alternatively, you can try logging out and logging back in for the group
    changes to apply, and manually start the service with
    `sudo systemctl start cuttlefish-host-resources.service`, but a reboot is
    more thorough.

_See [Cuttlefish Get Started](https://source.android.com/docs/devices/cuttlefish/get-started) for more details._

**Appendix B: Verifying the Setup**

Here's how you can check if `cuttlefish-base` is installed and if the TAP
devices are set up on your gLinux machine:

1.  **Check if `cuttlefish-base` package is installed:** Use `dpkg` to query the
    package status:

    ```bash
    dpkg -s cuttlefish-base
    ```

    Look for `Status: install ok installed`.

2.  **Verify you are in the `cvdnetwork` group:** Run the `groups` command and
    check for `cvdnetwork`:

    ```bash
    groups | grep cvdnetwork
    ```

    You should see `cvdnetwork` in the output.

3.  **List Network Interfaces:** Check for the presence of `cvd-etap-XX`
    interfaces:

    ```bash
    ip link show | grep 'cvd-etap'
    ```

    You should see entries like `cvd-etap-01`, `cvd-etap-02`, etc.

4.  **Check for Bridge Interfaces:** Verify the bridges are created:

    ```bash
    ip link show | grep 'cvd-ebr'
    ```

    You should at least see `cvd-ebr`.

5.  **Check the Service Status:** Ensure the service that creates these devices
    is running:
    ```bash
    systemctl status cuttlefish-host-resources.service
    ```
    Look for `Active: active (running)`.

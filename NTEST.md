This is the complete design document for `ntest` (Network Tester), a Rust-based
network verification tool.

This design incorporates all your requirements:

1. **Reflector Pattern:** A single binary acting as both Client and Server.
2. **Cross-Platform:** Runs on Host (Unix) and Guest (Android).
3. **Dual Modes:**

- `remote` mode: Host controls Android via ADB.
- `local` mode: Runs both client and server locally (loopback) for rapid
  development/debugging.

4. **Native BDD:** Uses direct function calls (step functions) instead of
   parsing text files, keeping the codebase pure Rust and compile-time checked.

---

# Design Document: `ntest` (Network Tester)

## 1. Overview

`ntest` is a lightweight, dependency-free network verification tool designed to
validate `libslirp` and other virtual network interfaces. It compiles into a
single static binary that can function as a **Server (Reflector)**, a **Client
(Tester)**, or a **Controller (Test Runner)**.

It replaces tools like `iperf`, `netcat`, and `scapy` with a single,
programmable binary that handles functional correctness (connectivity, state
transitions) and performance (throughput).

## 2. System Architecture

The system operates in two distinct configurations.

### Configuration A: Local Development (Self-Test)

Used to verify the tool itself and debug test logic without the overhead of
Android/ADB.

- **Controller:** Spawns a local Server process.
- **Client:** Connects to `127.0.0.1`.
- **Result:** Immediate feedback on test logic.

### Configuration B: Integration Testing (Production)

Used to verify the actual `libslirp` boundary.

- **Controller (Host):** runs the BDD test suite.
- **Server (Host):** listens on `vnet0` (or loopback alias).
- **Client (Android):** deployed via ADB, connects to `10.0.2.2` (Host Alias).

---

## 3. The `ntest` Binary CLI

The binary has three primary subcommands.

```bash
# 1. SERVER MODE (The Reflector)
# Binds to ports and echos data back.
ntest server --port 8080

# 2. CLIENT MODE (The Probe)
# Executes a single specific check and returns 0 (Pass) or 1 (Fail).
ntest client \
  --proto tcp \
  --target 10.0.2.2:8080 \
  --payload-size 1024 \
  --expect-eof

# 3. TEST RUNNER (The BDD Controller)
# Orchestrates the other two.
ntest test --mode [local|android]

```

---

## 4. Rust Implementation Details

### 4.1. Project Structure

```text
ntest/
├── Cargo.toml          # Dependencies: clap, tokio, anyhow, socket2
├── src/
│   ├── main.rs         # Entry point (CLI parsing)
│   ├── server.rs       # Reflector logic (TCP/UDP/mDNS listeners)
│   ├── client.rs       # Probe logic (Connection, fragmentation, timeouts)
│   └── tests/          # The BDD Suite
│       ├── mod.rs      # Test Runner Entry
│       ├── steps.rs    # Reusable "Step" functions
│       └── scenarios.rs# The actual test definitions

```

### 4.2. The "Native BDD" Approach

Instead of parsing Gherkin strings, we define a strict Rust API for steps. This
gives us autocomplete, compile-time checking, and zero parsing overhead.

**The Context Struct:** Holds the state of the world (ADB handles, Child
processes).

```rust
struct TestContext {
    mode: TestMode, // Local or Android
    server_process: Option<Child>, // Handle to the server
    adb_device: Option<String>,
}

enum TestMode {
    LocalLoopback,
    AndroidRemote,
}

```

**The Step Trait (DSL):**

```rust
impl TestContext {
    // GIVEN
    fn start_server(&mut self, proto: Protocol, port: u16) -> Result<()>;

    // WHEN
    fn run_client_check(&self, params: ClientParams) -> Result<Output>;

    // THEN
    fn assert_success(&self, output: Output);
    fn assert_throughput_above(&self, output: Output, mbps: f64);
}

```

---

## 5. Detailed Test Scenarios (Native Rust)

Here is how the "scenarios.rs" file looks. It reads exactly like a test plan but
is valid Rust code.

```rust
// src/tests/scenarios.rs

pub fn run_suite(ctx: &mut TestContext) -> Result<()> {
    scenario_tcp_handshake(ctx)?;
    scenario_udp_fragmentation(ctx)?;
    scenario_host_hangup(ctx)?;
    scenario_performance(ctx)?;
    Ok(())
}

fn scenario_tcp_handshake(ctx: &mut TestContext) -> Result<()> {
    println!("SCENARIO: Basic TCP Handshake");

    // Given
    ctx.start_server(Protocol::Tcp, 8080)?;

    // When
    let result = ctx.run_client_check(ClientParams {
        proto: Protocol::Tcp,
        target: "10.0.2.2:8080".into(), // Or 127.0.0.1 in Local mode
        payload: "PING".into(),
        ..Default::default()
    })?;

    // Then
    ctx.assert_success(result);
    Ok(())
}

fn scenario_host_hangup(ctx: &mut TestContext) -> Result<()> {
    println!("SCENARIO: Host Socket Close (Edge Case)");

    // Given
    ctx.start_server(Protocol::Tcp, 8081)?;

    // When (Client connects, Host server is configured to drop immediately)
    let result = ctx.run_client_check(ClientParams {
        proto: Protocol::Tcp,
        target: "10.0.2.2:8081".into(),
        expect_eof: true, // Crucial flag
        ..Default::default()
    })?;

    // Then
    ctx.assert_success(result); // Success here means "We correctly detected the hangup"
    Ok(())
}

fn scenario_performance(ctx: &mut TestContext) -> Result<()> {
    println!("SCENARIO: TCP Throughput");

    ctx.start_server(Protocol::Tcp, 9090)?;

    let result = ctx.run_client_check(ClientParams {
        proto: Protocol::Tcp,
        mode: ClientMode::SpeedTest,
        duration_sec: 5,
        target: "10.0.2.2:9090".into(),
        ..Default::default()
    })?;

    ctx.assert_throughput_above(result, 100.0); // 100 Mbps
    Ok(())
}

```

---

## 6. Implementation Strategy (The "Local First" Switch)

The key to making this work for both Local and Android without code duplication
is the `TargetResolver`.

When `ntest test --mode local` runs:

1. `start_server` spawns `ntest server` as a child process on `localhost`.
2. `run_client_check` spawns `ntest client` as a child process on `localhost`.
3. Target IP is auto-resolved to `127.0.0.1`.

When `ntest test --mode android` runs:

1. `start_server` spawns `ntest server` on the Host (listening on `0.0.0.0`).
2. `run_client_check` constructs an `adb shell /data/local/tmp/ntest client ...`
   command.
3. Target IP is forced to `10.0.2.2` (Host alias).

### 6.1. The Abstraction Layer

```rust
// src/tests/executor.rs

trait Executor {
    fn spawn_server(&self, port: u16) -> Child;
    fn run_client(&self, params: ClientParams) -> Output;
}

struct LocalExecutor;
impl Executor for LocalExecutor {
    fn spawn_server(&self, port: u16) -> Child {
        Command::new("./ntest").arg("server").arg("--port").arg(port.to_string()).spawn().unwrap()
    }
    fn run_client(&self, params: ClientParams) -> Output {
        Command::new("./ntest").arg("client").args(params.to_flags()).output().unwrap()
    }
}

struct AndroidExecutor;
impl Executor for AndroidExecutor {
    fn spawn_server(&self, port: u16) -> Child {
        // Server still runs on Host!
        Command::new("./ntest").arg("server").arg("--port").arg(port.to_string()).spawn().unwrap()
    }
    fn run_client(&self, params: ClientParams) -> Output {
        // Client runs on Android via ADB
        Command::new("adb").arg("shell").arg("/data/local/tmp/ntest").arg("client").args(params.to_flags()).output().unwrap()
    }
}

```

---

## 7. Bazel Build Integration

You define two distinct targets, but only one source tree.

```python
# BUILD.bazel

# 1. The Binary (Builds for whatever CPU is targeted)
rust_binary(
    name = "ntest_bin",
    srcs = glob(["src/**/*.rs"]),
    edition = "2021",
    deps = ["@crate_index//:clap", ...],
)

# 2. The Android Artifact (Forces ARM64 build)
filegroup(
    name = "ntest_android",
    srcs = [":ntest_bin"],
    output_group = "android_arm64", # Requires platform transition config
)

# 3. The Test Runner (Host Only)
rust_test(
    name = "integration_test",
    srcs = ["src/tests/mod.rs"],
    data = [":ntest_android", ":ntest_bin"], # Needs BOTH binaries
    args = ["--mode", "local"], # Default to local for fast `bazel test`
)

```

## 8. Development Workflow

1. **Write a new test scenario:** Add a function to `scenarios.rs`.
2. **Verify locally (Fast):**

```bash
cargo run -- test --mode local
# or
bazel run //:ntest_bin -- test --mode local

```

_This confirms your logic is correct, ignoring network/ADB flakes._ 3. **Verify
on Device (Full):**

```bash
# Push binary once
bazel build --config=android //:ntest_bin
adb push bazel-bin/ntest_bin /data/local/tmp/ntest
adb shell chmod +x /data/local/tmp/ntest

# Run the suite
cargo run -- test --mode android

```

This design document provides a robust, scalable, and developer-friendly path to
building `ntest`. It decouples the "What" (the test logic) from the "Where"
(Local vs Android), satisfying all your constraints.

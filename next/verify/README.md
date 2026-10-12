# Verify: A Software Validation Framework

> "Vibe, but verify." &mdash; Ronald Reagan

`verify` is a HDD-style orchestration tool designed to validate networking scenarios between the Host and Guest devices (Android Emulators).

## Key Features

- **Multi-Device Orchestration**: Discovers available emulators via ADB and manages the test environment lifecycle.
- **Narrative HDD Output**: Provides an aligned console output using `GIVEN / WHEN / THEN / INFO` tags for readability.
- **VBS Integration**: Pairs with a companion `vbs` (Verify Bundled Steps) APK running as a service on Android guests.
- **Variable Resolution**: Supports template placeholders like `{port}`, `{target}`, and `{gateway_target}` that resolve during execution.

---

## Core Concepts

### 1. The Gherkin Language

Verify uses Gherkin syntax to define test features in text.

The implementation adds:
- **Actors**, where the steps are run, can be specified in the feature file using the `@` prefix (e.g., `@Host`, `@Small_Phone`).
- **Parameters** can be expanded in steps using interpolation (e.g., `{port}`).

### 2. Actors

An Actor represents a specific environment or state where the intention step is executed.

#### Actor Ecosystem
The runner registers an initial set of actors:
- `@host`: The Linux host machine running the orchestrator.
- `@adb`: The ADB Bridge (used for environment setup/discovery logs).
- `@network`: The netsim network simulation environment.
- `@AVD#N` or `@DeviceName`: Guest devices (Android Emulators) running the VBS service.

### 3. Codegen

Each line in the feature file is translated into a Rust function call. The codegen reads the feature files and generates the Rust glue code that will be used to run the tests.

### 4. VBS (Verify Bundled Steps)

VBS is the component that lives inside the system being tested (the Android Guest).
- It **Executes** the steps of the scene on the device.
- It **Validates** that project constraints are not violated.

### 5. The Runner

The piece of code that runs on the host.
- It **Executes** the steps of the scene by orchestrating actors.
- It **Validates** overall test success.

---

## Architecture and Extensibility

`verify` is designed to be extensible so that new projects can create their own validation suites by reusing the core framework and combining shared steps.

### Android Side (Guest)

The Android instrumentation code is split into two parts:
- **`core`**: Contains the base `VerifyInstrumentation` and `StepRegistry`. It is agnostic to specific step implementations.
- **`instrumentation/vbs`**: Contains the Verify Bundled Steps (specific implementations for WiFi, UWB, etc.) and extends `VerifyInstrumentation` to register them.

New projects can create their own instrumentation APK by depending on `core`, reusing steps from `vbs` if needed, and adding their own custom steps.

### Host Side

The host-side Rust code follows a similar pattern:
- **`lib`**: The core HDD engine and execution loop.
- **Runner**: A concrete binary that pulls in `lib` and specific step libraries to execute tests.

> [!NOTE]
> To further support extensibility, the `verify_host_lib` library could be split into more granular libraries (e.g., for ADB, Android, and Netsim steps) for future projects to use.

---

## Feature Observables

`verify` supports counting and verifying features during a scenario. This is useful for cross-validating test effectiveness and telemetry correctness.

### Guest-side (Android)

To expose feature counters on the Android guest:

1.  **Implement `FeatureObservable`**: Create a class that implements the interface and returns a map of counters.
    ```kotlin
    import com.android.verify.core.FeatureObservable

    class MyFeatureCounters : FeatureObservable {
        var wifiP2pConnections = 0
        override fun getObservables(): Map<String, String> {
            return mapOf("wifi-p2p-connections" to wifiP2pConnections.toString())
        }
    }
    ```

2.  **Register in your Instrumentation**:
    ```kotlin
    class MyInstrumentation : VerifyInstrumentation() {
        private val myCounters = MyFeatureCounters()
        override fun onCreate(arguments: Bundle) {
            super.onCreate(arguments)
            registerObservable(myCounters)
        }
    }
    ```

### Host-side

On the host, you can verify these counters using Gherkin steps:

```gherkin
  Scenario: Verify P2P connection count
    Then @avd observes "wifi-p2p-connections" should be "1"
```

### Host-side (gRPC) [DISABLED]

> [!WARNING]
> Host-side observables via gRPC have been disabled to remove the `grpcio` dependency for Google3.
> We are transitioning to use direct calls to the `netsim` CLI binary.
> Scenarios using `@netsim observes` will not work with real data until CLI support for state queries is added.
> Tracked in [b/508335216](http://b/508335216).

Previously, you could query `netsimd` directly via gRPC to check for host-side observables, such as the number of connected devices and version:

```gherkin
  Scenario: Verify connected device count
    Then @netsim observes "connected-devices" should be ">=1"
```

You could also use Data Tables to assert on multiple observables at once, and use operators like `>=` or `*` (wildcard for existence):

```gherkin
  Scenario: Verify device count and valid version
    Then @netsim observes:
      | connected-devices | >=1     |
      | netsim-version    | *       |
```

This step used the `ListDevice` gRPC call to count the devices registered in `netsimd`.

### Supported Operators for Observables

When asserting on observables (either via single line or Data Table), you can use the following operators in the expected value string:

- **`*`**: Wildcard. Asserts that the key exists (any value is acceptable).
- **`>=N`**: Asserts that the value parsed as a number is greater than or equal to N.
- **`>N`**: Asserts that the value parsed as a number is greater than N.
- **`<=N`**: Asserts that the value parsed as a number is less than or equal to N.
- **`<N`**: Asserts that the value parsed as a number is less than N.
- **Plain String**: Asserts exact string equality.

### Ergonomic Tags for Automatic Verification

To reduce boilerplate, you can tag a scenario with `@verify_observed:<feature_name>`. The framework will automatically verify that the feature was observed (count > 0) at the end of the scenario.

```gherkin
  @verify_observed:wifi-p2p-connections
  Scenario: Verify P2P connection count with tags
    When @avd fetch feature observables
```

This removes the need to add an explicit `Then` step for verification.

---

## Getting Started

### Prerequisites

To build and run the complete E2E test suite, you need:
- **Android SDK**: Installed and configured.
- **Environment Variables**: `ANDROID_HOME` pointing to your SDK path.
- **Build Tools**: Android `build-tools` installed.

### Building

Build the orchestrator and the E2E runner:

```bash
bazel build //next/verify/runner:runner
```

### Running E2E Tests

To run the full E2E test suite, including daemon and emulator lifecycle management, use the provided script:

```bash
# Recommended way to run E2E tests
tools/netsim/next/verify/scripts/verify_run.sh
```

For more details on running E2E tests, see the workflow documentation at `tools/netsim/next/verify/workflows/verify_run.md`.

### CLI Usage

If you run the `verify` binary directly:

```bash
./verify run --apk-path vbs.apk
```

Options:
- `--android-home <PATH>`: Path to the Android SDK root.
- `--apk-path <PATH>`: Path to the `vbs.apk`.

---

## UI Automator Steps Guidelines

To add support for UI Automator in `verify` tests, follow these guidelines:

### Abstraction Level

- **Generic Steps**: Prefer generic steps for simple interactions to avoid writing new Kotlin functions for every minor UI interaction.
    - Example: `When @android:N clicks on element with text "Label"`
    - Example: `Then @android:N should see text "Label"`
- **Specific Steps**: Use specific steps for complex UI flows or when generic steps lead to overly verbose feature files.
    - Example: `When @android:N toggles Wi-Fi via Settings UI`

### UI Flakiness

- **Waiting**: Always use UI Automator's `wait` with `Until` conditions instead of static sleeps.
- **Timeouts**: Use reasonable timeouts (e.g., 5 seconds) for finding objects.

### Macros

- If sequences of generic steps become repetitive, consider implementing a macro system in the Rust runner (e.g., in `Features::run_steps`) to allow reusability without adding Kotlin boilerplate.

# Verify Examples

This directory contains examples demonstrating how to use and extend the `verify` framework.

## Overview

The `verify` framework is designed to be extensible. You can add custom steps on both the host side (Rust) and the guest side (Kotlin/Android). These examples show how to do both.

## Directory Structure

- `01-basic/`: Demonstrates a minimal feature using standard VBS apk (Verify Bundled Steps).
- `02-host-steps/`: Demonstrates how to add additional host-side step functions to your own host runner.
- `03-guest-steps/`: Demonstrates how to add additional step functions to your own APK and use a custom runner.

## Running Examples

### Basic VBS Example (`01-basic`)
This example uses the standard `verify` runner and does not require a custom binary.

To run it in dry-run mode:
```bash
bazel run @netsim//next/verify/runner:runner -- run --dry-run --spec-dir $(pwd)/tools/netsim/next/verify/examples/01-basic/features
```

### Custom Host Steps Example (`02-host-steps`)
This example demonstrates how to build and use a custom host runner with additional step functions.

To run it in dry-run mode:
```bash
bazel run @netsim//next/verify/examples/02-host-steps:custom_host_runner -- --spec-dir $(pwd)/tools/netsim/next/verify/examples/02-host-steps/features --dry-run
```

### Custom Guest Steps Example (`03-guest-steps`)
This example demonstrates how to use a custom APK with additional step functions and a custom host runner to forward them.

To run it in dry-run mode:
```bash
bazel run @netsim//next/verify/examples/03-guest-steps:custom_runner -- --spec-dir $(pwd)/tools/netsim/next/verify/examples/03-guest-steps/features --dry-run
```

## Adding New Examples

### Guest-side (Kotlin)
1. Create a new Android library or instrumentation test target depending on `//next/verify/instrumentation/core`.
2. Implement your custom steps and register them in your `VerifyInstrumentation` subclass.
3. Build the APK and pass it to the `verify` runner using the `--apk-path` flag.

### Host-side (Rust)
1. Create a new Rust binary depending on `//next/verify/lib` and `//next/verify/macros`.
2. Implement your custom steps using the `#[step]` macro.
3. Register them in your `main.rs` and run the scenario.

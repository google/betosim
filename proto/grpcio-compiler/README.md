# Local grpcio-compiler

This directory contains a local fork of the `grpcio-compiler` crate.

## Why a local fork?

We use a local fork to maintain compatibility with the specific versions of `protobuf-codegen` and `protoc` used in the Android source tree (AOSP) and to support our specific Bazel build requirements.

### Key Modifications
- **Dependency adjustments**: Pinned to specific local crate versions.
- **Protobuf Codegen Integration**: Adapted to work with the `protobuf-codegen` API available in our environment, specifically patching visibility issues.
- **Simplified Build**: Removed unused binaries (`protoc-gen-rust`) to reduce build complexity and failures.

## Usage

This compiler is used by the `netsim_grpc_gen` rule in `tools/netsim/proto/BUILD` to generate Rust gRPC stubs.

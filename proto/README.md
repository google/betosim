# Developer Overview: tools/netsim/proto

## 1. Proto Build Process

The generation of Rust code from `.proto` files in Netsim is orchestrated by a custom tool **`build_proto`** (located at `tools/netsim/proto/build_proto.rs`). This process differs from standard `cargo build` workflows to satisfy Bazel's hermeticity requirements and the project's specific module structure.

### Components
*   **`protoc`**: The standard Protocol Buffers compiler (provided by Bazel).
*   **`grpc_rust_plugin`**: The service generator plugin (compiled locally from `tools/netsim/proto/grpcio-compiler`).
*   **`build_proto`**: The orchestrator binary that wraps `protoc` execution and post-processes the output.

### The Workflow
1.  **Bazel Genrule**: The build starts with a `genrule` in `proto/BUILD` (e.g., `netsim_proto_gen`).
2.  **Orchestrator Invocation**: Bazel calls `build_proto` with paths to inputs (proto files) and the `grpc_rust_plugin`.
3.  **Code Generation**: `build_proto` invokes `protoc`, passing the `grpc_rust_plugin` to generate both message structs (via `protoc-gen-rust`) and service stubs (via `grpc_rust_plugin`).
4.  **Post-Processing & Shimming**:
    *   **File Organization**: Generated files are moved into subdirectories (`netsim/`, `rootcanal/`) to match the desired crate structure.
    *   **Virtual Module Compatibility**: `build_proto` generates a `lib.rs` that creates "shim" modules.
        *   **`google` Shim**: Re-exports `protobuf::well_known_types` so generated code like `super::google::protobuf::Empty` resolves correctly.
        *   **`packet` Shim**: Aggregates `hci_packet` and `packet_streamer` into a logical `packet` module.
    *   **Path Correction**: `#[path = "..."]` attributes are added to generated modules to fix `super` references.

## 2. The `grpcio-compiler` Fork

The directory `tools/netsim/proto/grpcio-compiler` contains a local fork of the operational gRPC compiler plugin for the `grpcio` runtime.

### Why do we fork it?
1.  **Protobuf Version Mismatch**: The upstream `grpcio-compiler` (@ 0.13.0) depends on `protobuf` **v2**. Netsim uses `protobuf` **v3** (via `@protobuf-rust`). Using the upstream compiler would pull in a conflicting, outdated protobuf dependency tree.
2.  **Rust 2024 Compatibility**: Upstream uses the function name `gen`, which is a reserved keyword in Rust 2024. Our fork renames this to `generate`.
3.  **Hermeticity (Bazel)**: We must build the compiler from source within the Bazel tree without relying on pre-compiled binaries or external `cargo install` steps.
4.  **Namespace Injection**: Customized `fq_grpc` logic to ensure generated code properly references the `grpcio` runtime in our specific crate layout.

## 3. Why not use `grpcio-sys` or other ecosystem generators?

There is often confusion between the **Runtime Library**, the **System Bindings**, and the **Code Generator**.

### The `grpcio-sys` Misconception
*   **`grpcio-sys`** is a **System Binding** crate. It provides raw, `unsafe` Rust bindings to the C Core gRPC library. It allows Rust code to link against and call C functions. It does **not** know how to read `.proto` files or generate Rust code. It is merely a dependency of the runtime.
*   **We cannot "use grpcio-sys to generate code"** because it lacks that capability. It is a library, not a compiler plugin.

### Why not use `tonic` / `prost`?
The Rust ecosystem has two main gRPC stacks:
1.  **Tonic** (Pure Rust, uses `prost` for messages).
2.  **gRPC-rs / grpcio** (Wraps C++ gRPC Core, uses `protobuf` for messages).

Netsim uses the **`grpcio`** runtime (likely for historical alignment with C++ gRPC behavior or specific performance characteristics).
*   Because we use the `grpcio` **runtime**, we MUST use the `grpcio-compiler` **generator**.
*   We cannot use `tonic-build` because it generates code compatible with `tonic`, not `grpcio`.

### Usage of Ecosystem Generators
We **do** use the ecosystem's logic, but we compile it ourselves:
*   **For Messages**: We use the standard **`protoc-gen-rust`** (from the `rust-protobuf` ecosystem), invoked by our `build_proto` orchestrator.
*   **For Services**: We use **`grpcio-compiler`** (from the `grpc-rs` ecosystem), but we vendor/fork it because:
    1.  **Hermeticity**: We must build it from source in Bazel.
    2.  **Customization**: We need to inject specific namespace qualifiers (`::grpcio`) and fix language keywords (e.g., renaming `gen` to `generate` for Rust 2024) that upstream may not support yet.

# AI Context for `tools/netsim/proto`


## Rust Proto Generation (`build_proto.rs`)

The `build_proto.rs` script (formerly `proto_build.rs`) manages the generation of Rust code from `.proto` files.

### Virtual Module Compatibility Layer
The `grpcio-compiler` generates code that expects a specific crate structure (e.g., `super::google::protobuf::Empty`, `super::netsim::packet`). The flat output of `protoc` does not match this. `build_proto.rs` implements a compatibility layer in the generated `lib.rs`:

1.  **Root Shims**: Creates a `google` module re-exporting `protobuf::well_known_types` to satisfy `super::google` references.
2.  **Packet Shim**: Creates a `netsim::packet` module aggregating `hci_packet` and `packet_streamer` to satisfy `netsim::packet` references.
3.  **Path Overrides**: Uses `#[path = "..."]` to move `frontend_grpc` and `packet_streamer_grpc` to the root resolution scope, ensuring their `super` references point to the crate root (and thus the shims).

**Do NOT remove these shims** without verifying that `grpcio-compiler` output expectations have changed.

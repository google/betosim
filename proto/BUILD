# Copyright 2026 The Android Open Source Project
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

load("@grpc-java//:java_grpc_library.bzl", "java_grpc_library")
load("@protobuf//bazel:cc_proto_library.bzl", "cc_proto_library")
load("@protobuf//bazel:java_proto_library.bzl", "java_proto_library")
load("@protobuf//bazel:proto_library.bzl", "proto_library")
load("@rules_cc//cc:defs.bzl", "cc_library")
load("@rules_rust//rust:defs.bzl", "rust_binary", "rust_library")

package(default_visibility = ["//visibility:public"])

rust_library(
    name = "grpcio-compiler-lib",
    srcs = glob(
        [
            "grpcio-compiler/src/*.rs",
            "grpcio-compiler/src/**/*.rs",
        ],
        exclude = ["grpcio-compiler/src/bin/**"],
    ),
    crate_features = ["protobuf-codec"],
    crate_name = "grpcio_compiler",
    edition = "2018",
    deps = [
        "@anyhow",
        "@protobuf-codegen//:protobuf_codegen",
        "@protobuf-rust//:protobuf-rust",
    ],
)

rust_binary(
    name = "grpc_rust_plugin",
    srcs = ["grpcio-compiler/src/bin/grpc_rust_plugin.rs"],
    edition = "2018",
    deps = [
        ":grpcio-compiler-lib",
        "@protobuf-rust//:protobuf-rust",
    ],
)

rust_binary(
    name = "build_proto",
    srcs = ["build_proto.rs"],
    edition = "2021",
)

# Common
# Model
# Config
# Frontend
# HCI Packet
# Packet Streamer
# Startup
# Stats
# (Granular targets removed as they are unused)

# Combined target for C++ compatibility (legacy support)
proto_library(
    name = "netsim_proto_library",
    srcs = glob(["netsim/*.proto"]),
    strip_import_prefix = "/proto",
    deps = [
        "@protobuf//:empty_proto",
        "@protobuf//:timestamp_proto",
        "@rootcanal//:rootcanal-configuration-proto",
    ],
)

cc_proto_library(
    name = "netsim_cc_proto",
    deps = [":netsim_proto_library"],
)

java_proto_library(
    name = "netsim_java_proto",
    deps = [":netsim_proto_library"],
)

java_grpc_library(
    name = "netsim_java_grpc",
    srcs = [":netsim_proto_library"],
    deps = [":netsim_java_proto"],
)

filegroup(
    name = "netsim_proto_srcs",
    srcs = glob(["netsim/*.proto"]),
)

# GRPC Generation
genrule(
    name = "netsim_grpc_gen",
    srcs = [
        ":netsim_proto_srcs",
        "netsim/common.proto",
        "@rootcanal//:proto/rootcanal/configuration.proto",
        "@protobuf//:well_known_type_protos",
    ],
    outs = [
        "netsim/frontend_grpc.rs",
        "netsim/packet_streamer_grpc.rs",
        "netsim/access_point_grpc.rs",
        "netsim/ble_service_grpc.rs",
        "netsim/cell_grpc.rs",
        "netsim/casimir_control_grpc.rs",
        "netsim/nfc_service_grpc.rs",
        "netsim/wifi_service_grpc.rs",
        "netsim/reflection_grpc.rs",
    ],
    cmd = """
        NETSIM_PROTO_ROOT=$$(dirname $$(dirname $(execpath netsim/common.proto)))
        ROOTCANAL_PROTO_ROOT=$$(dirname $$(dirname $(execpath @rootcanal//:proto/rootcanal/configuration.proto)))
        # Derive the protobuf root include path dynamically from well-known type protos
        # by stripping the trailing /google/protobuf/... path, and verify it exists.
        PROTOBUF_ROOT=""
        for f in $(execpaths @protobuf//:well_known_type_protos); do
            case "$$f" in
                */google/protobuf/*)
                    PROTOBUF_ROOT="$${f%%/google/protobuf/*}"
                    break
                    ;;
            esac
        done
        if [ -z "$$PROTOBUF_ROOT" ] || [ ! -d "$$PROTOBUF_ROOT/google/protobuf" ]; then
            echo "Error: Unable to resolve valid protobuf root from well-known type protos" >&2
            exit 1
        fi

        $(execpath @protobuf//:protoc) \\
            --plugin=protoc-gen-grpc=$(execpath :grpc_rust_plugin) \\
            --grpc_out=$(@D) \\
            --proto_path=$$NETSIM_PROTO_ROOT \\
            --proto_path=$$ROOTCANAL_PROTO_ROOT \\
            --proto_path=$$PROTOBUF_ROOT \\
            $(execpaths :netsim_proto_srcs)
    """,
    tools = [
        ":grpc_rust_plugin",
        "@protobuf//:protoc",
    ],
)

# Proto Generation (Messages + Lib)
genrule(
    name = "netsim_proto_gen",
    srcs = [
        ":netsim_proto_srcs",
        "netsim/common.proto",
        "@rootcanal//:proto/rootcanal/configuration.proto",
        ":netsim_grpc_gen",
        "@protobuf//:well_known_type_protos",
    ],
    outs = [
        "netsim/common.rs",
        "netsim/config.rs",
        "netsim/frontend.rs",
        "netsim/hci_packet.rs",
        "netsim/model.rs",
        "netsim/packet_streamer.rs",
        "netsim/startup.rs",
        "netsim/stats.rs",
        "netsim/access_point.rs",
        "netsim/ble_service.rs",
        "netsim/cell.rs",
        "netsim/casimir_control.rs",
        "netsim/nfc_service.rs",
        "netsim/wifi_service.rs",
        "netsim/reflection.rs",
        "rootcanal/configuration.rs",
        "lib.rs",
    ],
    cmd = """
        NETSIM_PROTO_ROOT=$$(dirname $$(dirname $(execpath netsim/common.proto)))
        ROOTCANAL_PROTO_ROOT=$$(dirname $$(dirname $(execpath @rootcanal//:proto/rootcanal/configuration.proto)))
        # Derive the protobuf root include path dynamically from well-known type protos
        # by stripping the trailing /google/protobuf/... path, and verify it exists.
        PROTOBUF_ROOT=""
        for f in $(execpaths @protobuf//:well_known_type_protos); do
            case "$$f" in
                */google/protobuf/*)
                    PROTOBUF_ROOT="$${f%%/google/protobuf/*}"
                    break
                    ;;
            esac
        done
        if [ -z "$$PROTOBUF_ROOT" ] || [ ! -d "$$PROTOBUF_ROOT/google/protobuf" ]; then
            echo "Error: Unable to resolve valid protobuf root from well-known type protos" >&2
            exit 1
        fi

        $(execpath :build_proto) \\
            --protoc=$(execpath @protobuf//:protoc) \\
            --plugin=$(execpath @protobuf-codegen//:protoc-gen-rust) \\
            --out-dir=$(@D) \\
            --proto-path=$$NETSIM_PROTO_ROOT \\
            --proto-path=$$ROOTCANAL_PROTO_ROOT \\
            --proto-path=$$PROTOBUF_ROOT \\
            $(execpaths :netsim_proto_srcs) \\
            $(execpath @rootcanal//:proto/rootcanal/configuration.proto) \\
            --grpc-srcs $(execpaths :netsim_grpc_gen)
    """,
    tools = [
        ":build_proto",
        "@protobuf-codegen//:protoc-gen-rust",
        "@protobuf//:protoc",
    ],
)

rust_library(
    name = "proto",
    srcs = [
        "lib.rs",
        "netsim/access_point.rs",
        "netsim/access_point_grpc.rs",
        "netsim/ble_service.rs",
        "netsim/ble_service_grpc.rs",
        "netsim/casimir_control.rs",
        "netsim/casimir_control_grpc.rs",
        "netsim/cell.rs",
        "netsim/cell_grpc.rs",
        "netsim/common.rs",
        "netsim/config.rs",
        "netsim/frontend.rs",
        "netsim/frontend_grpc.rs",
        "netsim/hci_packet.rs",
        "netsim/model.rs",
        "netsim/nfc_service.rs",
        "netsim/nfc_service_grpc.rs",
        "netsim/packet_streamer.rs",
        "netsim/packet_streamer_grpc.rs",
        "netsim/reflection.rs",
        "netsim/reflection_grpc.rs",
        "netsim/startup.rs",
        "netsim/stats.rs",
        "netsim/wifi_service.rs",
        "netsim/wifi_service_grpc.rs",
        "rootcanal/configuration.rs",
    ],
    crate_name = "netsim_proto",
    edition = "2021",
    deps = [
        "@grpcio",
        "@protobuf-rust",
    ],
)

cc_library(
    name = "netsim_cc_grpc",
    srcs = [":netsim_cc_grpc_gen"],
    hdrs = [":netsim_cc_grpc_gen"],
    includes = ["."],
    deps = [
        ":netsim_cc_proto",
        "@grpc//:grpc++",
    ],
)

genrule(
    name = "netsim_cc_grpc_gen",
    srcs = [
        ":netsim_proto_srcs",
        "netsim/common.proto",
        "@protobuf//:well_known_type_protos",
        "@rootcanal//:proto/rootcanal/configuration.proto",
    ],
    outs = [
        "netsim/cell.grpc.pb.cc",
        "netsim/cell.grpc.pb.h",
        "netsim/cell_mock.grpc.pb.h",
        "netsim/frontend.grpc.pb.cc",
        "netsim/frontend.grpc.pb.h",
        "netsim/frontend_mock.grpc.pb.h",
        "netsim/packet_streamer.grpc.pb.cc",
        "netsim/packet_streamer.grpc.pb.h",
        "netsim/packet_streamer_mock.grpc.pb.h",
    ],
    cmd = """
        SRCS="netsim/cell.proto netsim/frontend.proto netsim/packet_streamer.proto"

        # Calculate include paths
        # We need the root of the 'netsim' proto package
        NETSIM_PROTO_ROOT=$$(dirname $$(dirname $(execpath netsim/common.proto)))

        # We need the root of 'rootcanal' package
        ROOTCANAL_PROTO_ROOT=$$(dirname $$(dirname $(execpath @rootcanal//:proto/rootcanal/configuration.proto)))

        # Derive the protobuf root include path dynamically from well-known type protos
        # by stripping the trailing /google/protobuf/... path, and verify it exists.
        PROTOBUF_ROOT=""
        for f in $(execpaths @protobuf//:well_known_type_protos); do
            case "$$f" in
                */google/protobuf/*)
                    PROTOBUF_ROOT="$${f%%/google/protobuf/*}"
                    break
                    ;;
            esac
        done
        if [ -z "$$PROTOBUF_ROOT" ] || [ ! -d "$$PROTOBUF_ROOT/google/protobuf" ]; then
            echo "Error: Unable to resolve valid protobuf root from well-known type protos" >&2
            exit 1
        fi

        for f in $$SRCS; do
            $(execpath @protobuf//:protoc) \\
                --plugin=protoc-gen-grpc=$(execpath @grpc//src/compiler:grpc_cpp_plugin) \\
                --grpc_out=generate_mock_code=true:$(@D) \\
                --proto_path=$$NETSIM_PROTO_ROOT \\
                --proto_path=$$ROOTCANAL_PROTO_ROOT \\
                --proto_path=$$PROTOBUF_ROOT \\
                $$NETSIM_PROTO_ROOT/$$f
        done
    """,
    tools = [
        "@grpc//src/compiler:grpc_cpp_plugin",
        "@protobuf//:protoc",
    ],
)

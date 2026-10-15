// Copyright 2026 The Android Open Source Project
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// This file is generated. Do not edit
// @generated

// https://github.com/Manishearth/rust-clippy/issues/702
#![allow(unknown_lints)]
#![allow(clippy::all)]
#![allow(dead_code)]
#![allow(missing_docs)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(trivial_casts)]
#![allow(unsafe_code)]
#![allow(unused_imports)]
#![allow(unused_results)]

const METHOD_PACKET_STREAMER_STREAM_PACKETS: ::grpcio::Method<
    super::packet_streamer::PacketRequest,
    super::packet_streamer::PacketResponse,
> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Duplex,
    name: "/netsim.packet.PacketStreamer/StreamPackets",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

#[derive(Clone)]
pub struct PacketStreamerClient {
    pub client: ::grpcio::Client,
}

impl PacketStreamerClient {
    pub fn new(channel: ::grpcio::Channel) -> Self {
        PacketStreamerClient { client: ::grpcio::Client::new(channel) }
    }

    pub fn stream_packets_opt(
        &self,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<(
        ::grpcio::ClientDuplexSender<super::packet_streamer::PacketRequest>,
        ::grpcio::ClientDuplexReceiver<super::packet_streamer::PacketResponse>,
    )> {
        self.client.duplex_streaming(&METHOD_PACKET_STREAMER_STREAM_PACKETS, opt)
    }

    pub fn stream_packets(
        &self,
    ) -> ::grpcio::Result<(
        ::grpcio::ClientDuplexSender<super::packet_streamer::PacketRequest>,
        ::grpcio::ClientDuplexReceiver<super::packet_streamer::PacketResponse>,
    )> {
        self.stream_packets_opt(::grpcio::CallOption::default())
    }
    pub fn spawn<F>(&self, f: F)
    where
        F: ::std::future::Future<Output = ()> + Send + 'static,
    {
        self.client.spawn(f)
    }
}

pub trait PacketStreamer {
    fn stream_packets(
        &mut self,
        ctx: ::grpcio::RpcContext,
        _stream: ::grpcio::RequestStream<super::packet_streamer::PacketRequest>,
        sink: ::grpcio::DuplexSink<super::packet_streamer::PacketResponse>,
    ) {
        grpcio::unimplemented_call!(ctx, sink)
    }
}

pub fn create_packet_streamer<S: PacketStreamer + Send + Clone + 'static>(
    s: S,
) -> ::grpcio::Service {
    let mut builder = ::grpcio::ServiceBuilder::new();
    let mut instance = s;
    builder = builder.add_duplex_streaming_handler(
        &METHOD_PACKET_STREAMER_STREAM_PACKETS,
        move |ctx, req, resp| instance.stream_packets(ctx, req, resp),
    );
    builder.build()
}

// Copyright 2023 The Android Open Source Project
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

use crate::http_server::http_handlers::{create_filename_hash_set, handle_connection};

use crate::http_server::thread_pool::ThreadPool;
use crate::links::link::LinkManager;
use log::{info, warn};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener};
use std::sync::Arc;
use std::thread;

const DEFAULT_HTTP_PORT: u16 = 7681;

/// Bind HTTP Server to IPv4 or IPv6 based on availability.
fn bind_listener(http_port: u16) -> Result<TcpListener, std::io::Error> {
    TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, http_port)))
        .or_else(|e| {
            warn!("Failed to bind to 127.0.0.1:{http_port} in netsimd frontend http server. Trying [::1]:{http_port}. {e:?}");
            TcpListener::bind(SocketAddr::from((Ipv6Addr::LOCALHOST, http_port)))
        })
}

/// Start the HTTP Server.
pub fn run_http_server(instance_num: u16, dev: bool, link_manager: Arc<LinkManager>) -> u16 {
    let http_port = DEFAULT_HTTP_PORT + instance_num - 1;
    let _ = thread::Builder::new().name("http_server".to_string()).spawn(move || {
        let listener = match bind_listener(http_port) {
            Ok(listener) => listener,
            Err(e) => {
                warn!("{e:?}");
                return;
            }
        };
        let pool = ThreadPool::new(4);
        info!("Frontend http server is listening on http://localhost:{http_port}");
        let valid_files = Arc::new(create_filename_hash_set());
        for stream in listener.incoming() {
            let stream = stream.unwrap();
            let valid_files = valid_files.clone();
            let link_manager_clone = Arc::clone(&link_manager);
            pool.execute(move || {
                handle_connection(stream, valid_files, dev, link_manager_clone);
            });
        }
        info!("Shutting down frontend http server.");
    });
    http_port
}

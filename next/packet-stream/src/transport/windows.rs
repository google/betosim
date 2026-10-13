// Copyright 2025 The Android Open Source Project
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

use tokio::net::windows::named_pipe::{
    ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
};

use crate::error::Result;

pub struct WindowsListener {
    server: NamedPipeServer,
    pipe_name: String,
}

impl WindowsListener {
    pub fn bind_named_pipe(pipe_name: &str) -> Result<Self> {
        let full_name = format!(r"\\.\pipe\{}", pipe_name);

        let server = ServerOptions::new().first_pipe_instance(true).create(&full_name)?;

        Ok(Self { server, pipe_name: pipe_name.to_string() })
    }

    pub async fn accept(&mut self) -> Result<NamedPipeServer> {
        self.server.connect().await?;

        // Create new server instance for next connection
        let full_name = format!(r"\\.\pipe\{}", self.pipe_name);
        let new_server = ServerOptions::new().create(&full_name)?;

        // Replace current server with new one and return the connected one
        let connected_server = std::mem::replace(&mut self.server, new_server);
        Ok(connected_server)
    }

    pub fn local_addr(&self) -> Result<String> {
        Ok(format!(r"\\.\pipe\{}", self.pipe_name))
    }
}

pub fn connect_named_pipe(pipe_name: &str) -> Result<NamedPipeClient> {
    let full_name = format!(r"\\.\pipe\{}", pipe_name);
    let client = ClientOptions::new().open(&full_name)?;
    Ok(client)
}

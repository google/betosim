// Copyright 2022 The Android Open Source Project
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

use std::{fs::File, io::Write, path::PathBuf};

use crate::{
    error::{Error, Result},
    grpc_client::ClientResponseReadable,
};

pub(crate) struct FileHandler {
    pub file: File,
    pub path: PathBuf,
}

impl ClientResponseReadable for FileHandler {
    fn handle_chunk(&mut self, chunk: &[u8]) -> Result<()> {
        (&self.file).write_all(chunk).map_err(|e| {
            Error::from(format!("Unable to write to file {}: {e}", self.path.display()))
        })
    }
}

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

use std::fmt;

#[derive(Debug)]
pub enum Error {
    Grpc(grpcio::Error),
    Io(std::io::Error),
    Hex(hex::FromHexError),
    Message(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Grpc(e) => write!(f, "gRPC error: {e}"),
            Error::Io(e) => write!(f, "IO error: {e}"),
            Error::Hex(e) => write!(f, "Hex parsing error: {e}"),
            Error::Message(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Grpc(e) => Some(e),
            Error::Io(e) => Some(e),
            Error::Hex(e) => Some(e),
            Error::Message(_) => None,
        }
    }
}

impl From<grpcio::Error> for Error {
    fn from(err: grpcio::Error) -> Self {
        Error::Grpc(err)
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Io(err)
    }
}

impl From<hex::FromHexError> for Error {
    fn from(err: hex::FromHexError) -> Self {
        Error::Hex(err)
    }
}

impl From<&str> for Error {
    fn from(s: &str) -> Self {
        Error::Message(s.to_string())
    }
}

impl From<String> for Error {
    fn from(s: String) -> Self {
        Error::Message(s)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

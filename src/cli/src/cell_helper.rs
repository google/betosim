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

use netsim_proto::{
    cell::{Cell, ExecuteCellRequest, GetCellRequest, ListCellsRequest, ListCellsResponse},
    cell_grpc::CellServiceClient,
};
use protobuf::well_known_types::empty::Empty;

use crate::error::{Error, Result};

/// Trait abstracting the `CellService` gRPC client, enabling mocking in unit
/// tests.
pub(crate) trait CellClient {
    fn list(&self, req: &ListCellsRequest) -> ::grpcio::Result<ListCellsResponse>;
    fn get(&self, req: &GetCellRequest) -> ::grpcio::Result<Cell>;
    fn execute(&self, req: &ExecuteCellRequest) -> ::grpcio::Result<Empty>;
}

impl CellClient for CellServiceClient {
    fn list(&self, req: &ListCellsRequest) -> ::grpcio::Result<ListCellsResponse> {
        self.list(req)
    }
    fn get(&self, req: &GetCellRequest) -> ::grpcio::Result<Cell> {
        self.get(req)
    }
    fn execute(&self, req: &ExecuteCellRequest) -> ::grpcio::Result<Empty> {
        self.execute(req)
    }
}

/// Resolves the cellular device ID from an optional argument.
///
/// If an ID is provided, it is returned directly. If `id` is `None`, this
/// queries the cellular service for all simulated devices:
/// - If exactly one device exists, its ID is automatically returned.
/// - If no devices or multiple devices exist, an error is returned prompting
///   the user to specify an ID.
pub(crate) fn resolve_cell_id(id: Option<u32>, client: &impl CellClient) -> Result<u32> {
    if let Some(id_val) = id {
        return Ok(id_val);
    }
    let req = ListCellsRequest::new();
    let response = client.list(&req)?;
    match response.cells.as_slice() {
        [] => Err(Error::Message("No simulated cellular devices found.".to_string())),
        [cell] => Ok(cell.id),
        cells => {
            let ids: Vec<String> = cells.iter().map(|c| c.id.to_string()).collect();
            Err(Error::Message(format!(
                "Multiple simulated cellular devices found: {}. Please specify device ID.",
                ids.join(", ")
            )))
        }
    }
}

#[cfg(test)]
pub mod test_utils {
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
    };

    use netsim_proto::cell::{Cell, ListCellsResponse};
    use protobuf::well_known_types::empty::Empty;

    use super::*;

    #[derive(Default)]
    pub struct MockCellClient {
        pub list_calls: Arc<Mutex<Vec<ListCellsRequest>>>,
        pub list_responses: Arc<Mutex<VecDeque<::grpcio::Result<ListCellsResponse>>>>,
        pub get_calls: Arc<Mutex<Vec<GetCellRequest>>>,
        pub get_responses: Arc<Mutex<VecDeque<::grpcio::Result<Cell>>>>,
        pub execute_calls: Arc<Mutex<Vec<ExecuteCellRequest>>>,
        pub execute_responses: Arc<Mutex<VecDeque<::grpcio::Result<Empty>>>>,
    }

    impl CellClient for MockCellClient {
        fn list(&self, req: &ListCellsRequest) -> ::grpcio::Result<ListCellsResponse> {
            self.list_calls.lock().unwrap().push(req.clone());
            self.list_responses.lock().unwrap().pop_front().unwrap_or_else(|| {
                Err(grpcio::Error::RpcFailure(grpcio::RpcStatus::new(
                    grpcio::RpcStatusCode::UNIMPLEMENTED,
                )))
            })
        }
        fn get(&self, req: &GetCellRequest) -> ::grpcio::Result<Cell> {
            self.get_calls.lock().unwrap().push(req.clone());
            self.get_responses.lock().unwrap().pop_front().unwrap_or_else(|| {
                Err(grpcio::Error::RpcFailure(grpcio::RpcStatus::new(
                    grpcio::RpcStatusCode::UNIMPLEMENTED,
                )))
            })
        }
        fn execute(&self, req: &ExecuteCellRequest) -> ::grpcio::Result<Empty> {
            self.execute_calls.lock().unwrap().push(req.clone());
            self.execute_responses.lock().unwrap().pop_front().unwrap_or_else(|| {
                Err(grpcio::Error::RpcFailure(grpcio::RpcStatus::new(
                    grpcio::RpcStatusCode::UNIMPLEMENTED,
                )))
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use netsim_proto::cell::{Cell, ListCellsResponse};

    use super::{test_utils::MockCellClient, *};

    #[test]
    fn test_resolve_cell_id_some() {
        let client = MockCellClient::default();
        let result = resolve_cell_id(Some(123), &client);
        assert_eq!(result.unwrap(), 123);
        assert_eq!(client.list_calls.lock().unwrap().len(), 0);
    }

    #[test]
    fn test_resolve_cell_id_none_empty() {
        let client = MockCellClient::default();
        client.list_responses.lock().unwrap().push_back(Ok(ListCellsResponse::new()));
        let result = resolve_cell_id(None, &client);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "No simulated cellular devices found.");
        assert_eq!(client.list_calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn test_resolve_cell_id_none_one() {
        let client = MockCellClient::default();
        let mut response = ListCellsResponse::new();
        let mut cell = Cell::new();
        cell.id = 42;
        response.cells.push(cell);
        client.list_responses.lock().unwrap().push_back(Ok(response));

        let result = resolve_cell_id(None, &client);
        assert_eq!(result.unwrap(), 42);
        assert_eq!(client.list_calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn test_resolve_cell_id_none_multiple() {
        let client = MockCellClient::default();
        let mut response = ListCellsResponse::new();
        let mut cell1 = Cell::new();
        cell1.id = 42;
        let mut cell2 = Cell::new();
        cell2.id = 43;
        response.cells.push(cell1);
        response.cells.push(cell2);
        client.list_responses.lock().unwrap().push_back(Ok(response));

        let result = resolve_cell_id(None, &client);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("Multiple simulated cellular devices found")
        );
        assert_eq!(client.list_calls.lock().unwrap().len(), 1);
    }
}

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

use netsim_proto::cell::{ExecuteCellRequest, ReceivePdu, ReceiveSms};

use super::args::SmsCommand;
use crate::{
    cell_helper::{CellClient, resolve_cell_id},
    error::Result,
};

/// Executes an SMS subcommand against the simulated cellular client.
pub(crate) fn execute(cmd: SmsCommand, client: &impl CellClient, verbose: bool) -> Result<()> {
    match cmd {
        SmsCommand::Send(args) => {
            let id = resolve_cell_id(args.id, client)?;
            if verbose {
                println!("SMS from {} received on cell {}.", args.sender, id);
            }
            let mut req = ExecuteCellRequest::new();
            req.id = id;
            let mut action = ReceiveSms::new();
            action.sender = args.sender;
            action.text = args.text.join(" ");
            req.set_receive_sms(action);
            client.execute(&req)?;
        }
        SmsCommand::Pdu(args) => {
            let id = resolve_cell_id(args.id, client)?;
            let mut req = ExecuteCellRequest::new();
            req.id = id;
            let mut action = ReceivePdu::new();
            action.pdu = args.pdu;
            req.set_receive_pdu(action);
            client.execute(&req)?;
            if verbose {
                println!("PDU received on cell {}.", id);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use protobuf::well_known_types::empty::Empty;

    use super::{
        super::args::{SmsPdu, SmsSend},
        *,
    };
    use crate::cell_helper::test_utils::MockCellClient;

    #[test]
    fn test_sms_send() {
        let client = MockCellClient::default();
        let cmd = SmsCommand::Send(SmsSend {
            sender: "1234".to_string(),
            text: vec!["hello".to_string(), "world".to_string()],
            id: Some(1),
        });
        client.execute_responses.lock().unwrap().push_back(Ok(Empty::new()));

        let result = execute(cmd, &client, false);
        assert!(result.is_ok());
        assert_eq!(client.execute_calls.lock().unwrap().len(), 1);
        let req = &client.execute_calls.lock().unwrap()[0];
        assert_eq!(req.id, 1);
        assert!(req.has_receive_sms());
        assert_eq!(req.receive_sms().sender, "1234");
        assert_eq!(req.receive_sms().text, "hello world");
    }

    #[test]
    fn test_sms_pdu() {
        let client = MockCellClient::default();
        let cmd = SmsCommand::Pdu(SmsPdu { pdu: "00010203".to_string(), id: Some(1) });
        client.execute_responses.lock().unwrap().push_back(Ok(Empty::new()));

        let result = execute(cmd, &client, false);
        assert!(result.is_ok());
        assert_eq!(client.execute_calls.lock().unwrap().len(), 1);
        let req = &client.execute_calls.lock().unwrap()[0];
        assert_eq!(req.id, 1);
        assert!(req.has_receive_pdu());
        assert_eq!(req.receive_pdu().pdu, "00010203");
    }
}

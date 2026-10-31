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

//! Capture REST API endpoints and data transfer models.

pub use capture_actor::CaptureInfo;
use http_body_util::{StreamBody, combinators::BoxBody};
use hyper::{
    Response, StatusCode,
    body::{Bytes, Frame},
    header::{CONTENT_LENGTH, CONTENT_TYPE, HeaderValue, X_CONTENT_TYPE_OPTIONS},
};
use netsim_model::ChipId;
use tokio::io::AsyncReadExt;
use tokio_stream::StreamExt;
use tokio_util::io::ReaderStream;

pub use crate::{CaptureUpdate, ListCaptureResponse};
use crate::{
    client::{NetsimRestClient, RestRequest},
    server::{HttpError, JsonResponse, RestServer, json_response, parse_json_body, path_id},
};

/// Response type of [`NetsimRestClient::get_capture`]: a raw PCAP file that is
/// never decoded as JSON. Download it with `send_to_file_blocking`, whose
/// timeout applies per read rather than to the whole body.
#[derive(Debug)]
pub enum Pcap {}

impl From<Vec<CaptureInfo>> for ListCaptureResponse {
    fn from(captures: Vec<CaptureInfo>) -> Self {
        Self { captures }
    }
}

impl From<CaptureInfo> for JsonResponse {
    fn from(capture: CaptureInfo) -> Self {
        Self(json_response(&capture))
    }
}

impl From<Vec<CaptureInfo>> for JsonResponse {
    fn from(captures: Vec<CaptureInfo>) -> Self {
        Self(json_response(&ListCaptureResponse::from(captures)))
    }
}

impl RestServer {
    pub(crate) async fn list_captures(&self) -> Result<Vec<CaptureInfo>, HttpError> {
        let mut captures = self.capture_client.list_captures().await?;
        captures.sort_by_key(|c| c.chip_id.0);
        Ok(captures)
    }

    pub(crate) async fn patch_capture(
        &self,
        params: &[(&str, &str)],
        body: &[u8],
    ) -> Result<CaptureInfo, HttpError> {
        let id: ChipId = path_id(params)?;
        let req: CaptureUpdate = parse_json_body(body)?;
        Ok(self.capture_client.update_capture(id, req.state).await?)
    }

    pub(crate) async fn get_capture(
        &self,
        params: &[(&str, &str)],
    ) -> Result<Response<BoxBody<Bytes, std::io::Error>>, HttpError> {
        let id: ChipId = path_id(params)?;
        let info = self.capture_client.get_capture(id).await?.ok_or_else(|| {
            HttpError::new(StatusCode::NOT_FOUND, format!("Capture not found for chip_id: {id}"))
        })?;
        let no_pcap =
            || HttpError::new(StatusCode::NOT_FOUND, format!("Capture {id} has no PCAP yet"));
        let read_failed = |e: std::io::Error, path: &std::path::Path| {
            tracing::warn!("Failed to read capture file {}: {e}", path.display());
            let message = format!("Failed to read capture {id}");
            HttpError::new(StatusCode::INTERNAL_SERVER_ERROR, message)
        };
        let filepath = info.filepath.ok_or_else(no_pcap)?;
        let file = tokio::fs::File::open(&filepath).await.map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => no_pcap(),
            _ => read_failed(e, &filepath),
        })?;
        let len = file.metadata().await.map_err(|e| read_failed(e, &filepath))?.len();
        let stream = ReaderStream::new(file.take(len)).map(|res| res.map(Frame::data));
        let mut resp = Response::new(BoxBody::new(StreamBody::new(stream)));
        let headers = resp.headers_mut();
        headers.insert(CONTENT_LENGTH, HeaderValue::from(len));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/vnd.tcpdump.pcap"));
        headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
        Ok(resp)
    }
}

impl NetsimRestClient {
    /// `GET /v1/captures`
    pub fn get_captures(&self) -> RestRequest<'_, ListCaptureResponse> {
        self.get(crate::path::CAPTURES)
    }

    /// `PATCH /v1/captures/{id}`
    pub fn patch_capture(&self, id: u32, patch: &CaptureUpdate) -> RestRequest<'_, CaptureInfo> {
        self.patch(format!("{}/{id}", crate::path::CAPTURES)).json(patch)
    }

    /// `GET /v1/captures/{id}`
    pub fn get_capture(&self, id: u32) -> RestRequest<'_, Pcap> {
        self.get(format!("{}/{id}", crate::path::CAPTURES))
    }
}

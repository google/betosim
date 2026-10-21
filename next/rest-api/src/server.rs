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

//! HTTP REST request dispatcher and endpoint handlers for netsim.

use std::{convert::Infallible, fmt::Display, io::ErrorKind, str::FromStr, sync::Arc};

use ap_actor::{ApClient, ApError};
use capture_actor::CaptureClient;
use device_actor::{DeviceClient, DeviceError};
use http_body_util::{BodyExt, Full, LengthLimitError, Limited};
use hyper::{
    Method, Request, Response, StatusCode,
    body::{Body, Bytes, Incoming},
    header::{CONTENT_TYPE, HeaderValue, X_CONTENT_TYPE_OPTIONS},
    server::conn::http1,
    service::service_fn,
};
use hyper_util::rt::TokioIo;
use link_api::{LinkClient, LinkError};
use netsim_model::{ChipError, ClientError};
use serde::{Serialize, de::DeserializeOwned};
use tokio::{net::TcpListener, task::JoinSet};
use tracing::{debug, info, warn};

/// Largest request body accepted, in bytes. Bodies are buffered in memory
/// before dispatch, so this bounds what a single connection can allocate.
const MAX_BODY_BYTES: usize = 1 << 20;

/// Serves the `/v1` HTTP API on `listener` until the task is cancelled.
///
/// The caller binds the listener so it owns port selection and publishing the
/// bound port; this crate owns the accept loop. The REST endpoints and the
/// `/v1/capture` WebSocket are served here; the WebSocket endpoint at
/// `/v1/websocket/bt` still has its own listener.
pub async fn run(
    listener: TcpListener,
    device_client: DeviceClient,
    link_client: Arc<dyn LinkClient>,
    ap_client: ApClient,
    capture_client: CaptureClient,
    version: String,
) {
    match listener.local_addr() {
        Ok(addr) => info!("REST API server is listening on: {addr}"),
        Err(error) => warn!(%error, "REST API server failed to read local address"),
    }

    let server =
        Arc::new(RestServer::new(device_client, link_client, ap_client, capture_client, version));
    let mut connection_tasks = JoinSet::new();

    loop {
        tokio::select! {
            accept_res = listener.accept() => {
                match accept_res {
                    Ok((socket, _)) => {
                        let server = Arc::clone(&server);
                        connection_tasks.spawn(async move {
                            let service = service_fn(move |req| {
                                let server = Arc::clone(&server);
                                async move { server.serve(req).await }
                            });
                            if let Err(e) =
                                http1::Builder::new()
                                    .serve_connection(TokioIo::new(socket), service)
                                    .with_upgrades()
                                    .await
                            {
                                debug!("REST connection error: {e}");
                            }
                        });
                    }
                    Err(e)
                        if matches!(
                            e.kind(),
                            ErrorKind::ConnectionReset
                                | ErrorKind::ConnectionAborted
                                | ErrorKind::Interrupted
                        ) =>
                    {
                        debug!("Transient error accepting connection on HTTP listener: {e}");
                    }
                    Err(error) => {
                        warn!(%error, "Error accepting connection on HTTP listener");
                        // Back off so a persistent accept error, such as running out
                        // of file descriptors, does not spin the task at full speed.
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                }
            }
            Some(res) = connection_tasks.join_next(), if !connection_tasks.is_empty() => {
                if let Err(e) = res {
                    debug!("REST connection task failed: {e}");
                }
            }
        }
    }
}

/// HTTP REST API Server context holding handles to core actor clients.
pub struct RestServer {
    pub device_client: DeviceClient,
    pub link_client: Arc<dyn LinkClient>,
    pub ap_client: ApClient,
    pub capture_client: CaptureClient,
    pub version: String,
}

/// Hand-written because the actor client handles are not `Debug`; only the
/// version is useful in diagnostics.
impl std::fmt::Debug for RestServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RestServer").field("version", &self.version).finish_non_exhaustive()
    }
}

fn build_response<B: Into<Bytes>>(status: StatusCode, body: B) -> Response<Full<Bytes>> {
    let body = body.into();
    let has_body = !body.is_empty();
    let mut resp = Response::new(Full::new(body));
    *resp.status_mut() = status;
    let headers = resp.headers_mut();
    if has_body {
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    }
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    resp
}

/// Builds an HTTP JSON [`ErrorResponse`](crate::ErrorResponse) with the given
/// `status` and `message`.
pub fn error_response(status: StatusCode, message: &str) -> Response<Full<Bytes>> {
    let body = serde_json::to_string(&crate::ErrorResponse { error: message.to_string() })
        .unwrap_or_else(|_| r#"{"error":"Internal Server Error"}"#.to_string());
    build_response(status, body)
}

fn json_status_response<T: Serialize>(status: StatusCode, val: &T) -> Response<Full<Bytes>> {
    match serde_json::to_string(val) {
        Ok(json) => build_response(status, json),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

/// Serializes `val` to JSON with `200 OK` or returns `500 Internal Server Error`
/// automatically.
pub fn json_response<T: Serialize>(val: &T) -> Response<Full<Bytes>> {
    json_status_response(StatusCode::OK, val)
}

/// Serializes `val` to JSON with `201 Created` or returns
/// `500 Internal Server Error` automatically.
pub fn created_response<T: Serialize>(val: &T) -> Response<Full<Bytes>> {
    json_status_response(StatusCode::CREATED, val)
}

/// Builds an empty `204 No Content` response.
pub fn no_content_response() -> Response<Full<Bytes>> {
    build_response(StatusCode::NO_CONTENT, Bytes::new())
}

/// Lightweight HTTP error carrying a [`StatusCode`] and error message before
/// serialization into a [`JsonResponse`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpError {
    status: StatusCode,
    message: String,
}

impl HttpError {
    /// Creates an [`HttpError`] with the given `status` and `message`.
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self { status, message: message.into() }
    }

    /// Creates a `400 Bad Request` [`HttpError`] with the given `message`.
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }

    /// Returns the HTTP [`StatusCode`] for this error.
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.status, self.message)
    }
}

impl std::error::Error for HttpError {}

/// Parses a JSON request `body` into `T`, returning a `400 Bad Request`
/// [`HttpError`] on malformed JSON or a body that does not match `T`.
pub fn parse_json_body<T: DeserializeOwned>(body: &[u8]) -> Result<T, HttpError> {
    serde_json::from_slice(body)
        .map_err(|e| HttpError::bad_request(format!("Invalid JSON body: {e}")))
}

/// Extracts the raw value of path parameter `name` from `params` (as returned by
/// [`crate::openapi::find_route`]), or returns `400 Bad Request` if missing.
pub fn path_param<'p>(params: &[(&str, &'p str)], name: &str) -> Result<&'p str, HttpError> {
    params
        .iter()
        .find_map(|&(k, v)| (k == name).then_some(v))
        .ok_or_else(|| HttpError::bad_request(format!("Missing path parameter: {name}")))
}

/// Extracts path parameter `name` from `params` and parses it into `T`,
/// returning `400 Bad Request` if missing or invalid.
pub fn parse_path_param<T>(params: &[(&str, &str)], name: &str) -> Result<T, HttpError>
where
    T: FromStr,
    T::Err: Display,
{
    let raw = path_param(params, name)?;
    raw.parse::<T>()
        .map_err(|e| HttpError::bad_request(format!("Invalid path parameter {name}: {e}")))
}

/// Extracts and parses the `{id}` path parameter as a `u32` (converting into
/// `u32`, `DeviceId`, `LinkId`, or `ChipId`), returning `400 Bad Request` if
/// missing or not a valid `u32`.
pub fn path_id<T: From<u32>>(params: &[(&str, &str)]) -> Result<T, HttpError> {
    parse_path_param::<u32>(params, "id").map(T::from)
}

fn chip_error_status(err: &ChipError) -> StatusCode {
    match err {
        ChipError::ChipNotFound(_) => StatusCode::NOT_FOUND,
        ChipError::ChipExists(_) => StatusCode::CONFLICT,
        ChipError::InvalidArguments(_)
        | ChipError::InvalidAddress(_)
        | ChipError::UnexpectedChipKind { .. }
        | ChipError::UnsupportedChipKind(_)
        | ChipError::WrongVariantError
        | ChipError::Unsupported => StatusCode::BAD_REQUEST,
        ChipError::Io(io_err) => io_error_status(io_err),
        ChipError::Backend(inner) | ChipError::Internal(inner) => dyn_error_status(&**inner),
    }
}

fn io_error_status(err: &std::io::Error) -> StatusCode {
    match err.kind() {
        ErrorKind::NotFound => StatusCode::NOT_FOUND,
        ErrorKind::InvalidInput | ErrorKind::InvalidData => StatusCode::BAD_REQUEST,
        ErrorKind::AlreadyExists => StatusCode::CONFLICT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn device_error_status(err: &DeviceError) -> StatusCode {
    match err {
        DeviceError::NotFound(_) | DeviceError::DeviceNotFound(_) => StatusCode::NOT_FOUND,
        DeviceError::ChipKindNotSupported(_) => StatusCode::BAD_REQUEST,
        DeviceError::ActorCommunicationError(inner) => client_error_status(inner),
        DeviceError::ResetErrors { .. } => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn link_error_status(err: &LinkError) -> StatusCode {
    match err {
        LinkError::NotFound(_) => StatusCode::NOT_FOUND,
        LinkError::InvalidParam(_) => StatusCode::BAD_REQUEST,
        LinkError::AlreadyExists => StatusCode::CONFLICT,
        LinkError::Internal(inner) | LinkError::ActorCommunicationError(inner) => {
            dyn_error_status(&**inner)
        }
    }
}

fn ap_error_status(err: &ApError) -> StatusCode {
    match err {
        ApError::ApNotFound(_) => StatusCode::NOT_FOUND,
        ApError::ApAlreadyExists(_) => StatusCode::CONFLICT,
        ApError::InvalidFrame => StatusCode::BAD_REQUEST,
        ApError::Chip(chip_err) => chip_error_status(chip_err),
        ApError::Internal(inner) => dyn_error_status(&**inner),
    }
}

fn dyn_error_status(err: &(dyn std::error::Error + 'static)) -> StatusCode {
    let mut curr = Some(err);
    while let Some(err) = curr {
        if let Some(client_err) = err.downcast_ref::<ClientError>() {
            return client_error_status(client_err);
        }
        if let Some(chip_err) = err.downcast_ref::<ChipError>() {
            return chip_error_status(chip_err);
        }
        if let Some(dev_err) = err.downcast_ref::<DeviceError>() {
            return device_error_status(dev_err);
        }
        if let Some(link_err) = err.downcast_ref::<LinkError>() {
            return link_error_status(link_err);
        }
        if let Some(ap_err) = err.downcast_ref::<ApError>() {
            return ap_error_status(ap_err);
        }
        if let Some(io_err) = err.downcast_ref::<std::io::Error>() {
            return io_error_status(io_err);
        }
        curr = err.source();
    }
    StatusCode::INTERNAL_SERVER_ERROR
}

/// Maps an actor [`ClientError`] to the corresponding HTTP [`StatusCode`]:
/// - Not found -> `404 Not Found`
/// - Invalid input -> `400 Bad Request`
/// - Already exists / conflict -> `409 Conflict`
/// - Channel or internal errors -> `500 Internal Server Error`
pub fn client_error_status(err: &ClientError) -> StatusCode {
    match err {
        ClientError::Chip(chip_err) => chip_error_status(chip_err),
        ClientError::Framework(framework) => dyn_error_status(&**framework),
        ClientError::Send(_) | ClientError::Recv(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// Wrapper for `POST` create handlers that serializes its inner value as a
/// `201 Created` JSON response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Created<T>(pub T);

/// Marker for `DELETE` handlers that produces an empty `204 No Content`
/// response.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoContent;

/// Wrapper around [`Response`] so domain types, [`Created`], [`NoContent`],
/// [`HttpError`], [`ClientError`], and `Result<T, E>` can convert into an HTTP
/// response via standard [`From`]/[`Into`].
#[derive(Debug)]
pub struct JsonResponse(pub Response<Full<Bytes>>);

impl From<JsonResponse> for Response<Full<Bytes>> {
    fn from(resp: JsonResponse) -> Self {
        resp.0
    }
}

impl From<Response<Full<Bytes>>> for JsonResponse {
    fn from(resp: Response<Full<Bytes>>) -> Self {
        Self(resp)
    }
}

impl<T: Serialize> From<Created<T>> for JsonResponse {
    fn from(Created(val): Created<T>) -> Self {
        Self(created_response(&val))
    }
}

impl From<NoContent> for JsonResponse {
    fn from(_: NoContent) -> Self {
        Self(no_content_response())
    }
}

impl From<()> for JsonResponse {
    fn from((): ()) -> Self {
        Self(no_content_response())
    }
}

impl From<crate::VersionResponse> for JsonResponse {
    fn from(resp: crate::VersionResponse) -> Self {
        Self(json_response(&resp))
    }
}

impl From<crate::Device> for JsonResponse {
    fn from(device: crate::Device) -> Self {
        Self(json_response(&device))
    }
}

impl From<Vec<crate::Device>> for JsonResponse {
    fn from(devices: Vec<crate::Device>) -> Self {
        Self(json_response(&crate::ListDeviceResponse::from(devices)))
    }
}

impl From<crate::Link> for JsonResponse {
    fn from(link: crate::Link) -> Self {
        Self(json_response(&link))
    }
}

impl From<Vec<crate::Link>> for JsonResponse {
    fn from(links: Vec<crate::Link>) -> Self {
        Self(json_response(&crate::ListLinkResponse::from(links)))
    }
}

impl From<crate::Ap> for JsonResponse {
    fn from(ap: crate::Ap) -> Self {
        Self(json_response(&ap))
    }
}

impl From<Vec<(u32, ap_actor::ApState)>> for JsonResponse {
    fn from(aps: Vec<(u32, ap_actor::ApState)>) -> Self {
        Self(json_response(&crate::ListApResponse::from(aps)))
    }
}

impl From<HttpError> for JsonResponse {
    fn from(err: HttpError) -> Self {
        Self(error_response(err.status, &err.message))
    }
}

impl From<ClientError> for HttpError {
    fn from(err: ClientError) -> Self {
        Self { status: client_error_status(&err), message: err.to_string() }
    }
}

impl From<ClientError> for JsonResponse {
    fn from(err: ClientError) -> Self {
        HttpError::from(err).into()
    }
}

impl<T: Into<JsonResponse>, E: Into<JsonResponse>> From<Result<T, E>> for JsonResponse {
    fn from(res: Result<T, E>) -> Self {
        match res {
            Ok(val) => val.into(),
            Err(err) => err.into(),
        }
    }
}

impl RestServer {
    pub fn new(
        device_client: DeviceClient,
        link_client: Arc<dyn LinkClient>,
        ap_client: ApClient,
        capture_client: CaptureClient,
        version: String,
    ) -> Self {
        Self { device_client, link_client, ap_client, capture_client, version }
    }

    fn get_version(&self) -> crate::VersionResponse {
        crate::VersionResponse { version: self.version.clone() }
    }

    async fn create_ap(&self, body: &[u8]) -> Result<Created<netsim_model::Ap>, HttpError> {
        let req: netsim_model::ApCreate = parse_json_body(body)?;
        let config = ap_actor::ApConfig::try_from(req).map_err(HttpError::bad_request)?;
        let id = self.ap_client.create_ap(None, config).await?;
        let state = self.ap_client.get_ap(id).await?.ok_or_else(|| {
            HttpError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Access point {id} missing after creation"),
            )
        })?;
        Ok(Created(state.into()))
    }

    async fn patch_ap(
        &self,
        params: &[(&str, &str)],
        body: &[u8],
    ) -> Result<netsim_model::Ap, HttpError> {
        let id: u32 = path_id(params)?;
        let req: netsim_model::ApUpdate = parse_json_body(body)?;
        let state = self
            .ap_client
            .update_ap_config(id, req.ssid, req.channel, Some(req.force_disconnect), None)
            .await?;
        Ok(state.into())
    }

    async fn delete_ap(&self, params: &[(&str, &str)]) -> Result<NoContent, HttpError> {
        let id: u32 = path_id(params)?;
        self.ap_client.destroy_ap(id).await?;
        Ok(NoContent)
    }

    async fn disconnect_ap(
        &self,
        params: &[(&str, &str)],
        body: &[u8],
    ) -> Result<NoContent, HttpError> {
        let id: u32 = path_id(params)?;
        let req: crate::DisconnectApRequest = parse_json_body(body)?;
        self.ap_client.disconnect(id, req.mac_address).await?;
        Ok(NoContent)
    }

    /// Serves one HTTP request. Reads and bounds the body, then dispatches.
    ///
    /// The error type is [`Infallible`] because every failure is reported as an
    /// HTTP status rather than by dropping the connection.
    pub async fn serve(&self, req: Request<Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
        if req.method() == Method::GET
            && req.uri().path().trim_end_matches('/') == crate::path::CAPTURE
        {
            return Ok(crate::capture_stream::upgrade(&self.capture_client, req).await);
        }

        // Split the head from the body so the path can be borrowed rather than
        // copied; consuming the body would otherwise invalidate any borrow of
        // `req`.
        let (parts, body) = req.into_parts();
        let path = parts.uri.path();
        let path = path.strip_suffix('/').filter(|p| !p.is_empty()).unwrap_or(path);

        // Reject an oversized body from the declared length before reading it,
        // so a large upload is not buffered just to be rejected.
        if body.size_hint().lower() > MAX_BODY_BYTES as u64 {
            return Ok(error_response(StatusCode::PAYLOAD_TOO_LARGE, "413 Payload Too Large"));
        }
        // `Limited` stops reading once the cap is hit, which also covers a
        // chunked body whose length is not declared up front and so slips past
        // the check above.
        let body = match Limited::new(body, MAX_BODY_BYTES).collect().await {
            Ok(collected) => collected.to_bytes(),
            Err(e) if e.is::<LengthLimitError>() => {
                return Ok(error_response(StatusCode::PAYLOAD_TOO_LARGE, "413 Payload Too Large"));
            }
            Err(e) => {
                return Ok(error_response(
                    StatusCode::BAD_REQUEST,
                    &format!("400 Bad Request: {e}"),
                ));
            }
        };

        Ok(self.handle_request(&parts.method, path, &body).await)
    }

    /// Dispatches an incoming HTTP REST request path and method to the matching
    /// handler.
    pub async fn handle_request(
        &self,
        method: &Method,
        path: &str,
        body: &[u8],
    ) -> Response<Full<Bytes>> {
        if *method == Method::GET && path == crate::path::OPENAPI {
            return build_response(StatusCode::OK, crate::OPENAPI_JSON);
        }
        let resp: JsonResponse = match crate::openapi::find_route(crate::ROUTES, method, path) {
            Some((route, params)) => match route.op {
                crate::Op::GetVersion => self.get_version().into(),
                crate::Op::ListDevices => self.device_client.list().await.into(),
                crate::Op::ListLinks => self.link_client.list().await.into(),
                crate::Op::ListAps => self.ap_client.list_aps().await.into(),
                crate::Op::CreateAp => self.create_ap(body).await.into(),
                crate::Op::PatchAp => self.patch_ap(&params, body).await.into(),
                crate::Op::DeleteAp => self.delete_ap(&params).await.into(),
                crate::Op::DisconnectAp => self.disconnect_ap(&params, body).await.into(),
            },
            None => {
                error_response(StatusCode::NOT_FOUND, &format!("Not Found: {} {}", method, path))
                    .into()
            }
        };
        resp.0
    }
}

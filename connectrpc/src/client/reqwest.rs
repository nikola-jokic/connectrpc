use super::{AsyncUnaryClient, CommonClient};
use crate::Result;
use crate::client::AsyncStreamingClient;
use crate::codec::Codec;
use crate::connect::{DecodeMessage, EncodeMessage};
use crate::request::{self, ClientStreamingRequest, ServerStreamingRequest, UnaryRequest};
use crate::response::{ClientStreamingResponse, ServerStreamingResponse, UnaryResponse};
use crate::stream::{ConnectFrame, UnpinStream};
use bytes::Bytes;
use futures_util::Stream;
use futures_util::stream::StreamExt;
use http::Uri;

/// A client implementation using the `reqwest` HTTP client library.
#[derive(Clone)]
pub struct ReqwestClient {
    client: reqwest::Client,
    common: CommonClient,
}

impl ReqwestClient {
    pub fn new(client: reqwest::Client, uri: Uri, message_codec: Codec) -> Result<Self> {
        Ok(Self {
            client,
            common: CommonClient::new(uri, message_codec)?,
        })
    }
}

impl<I, O> AsyncUnaryClient<I, O> for ReqwestClient
where
    I: EncodeMessage,
    O: DecodeMessage,
{
    /// Calls a unary RPC method with the given path and request.
    /// The path is the RPC path, e.g. "/package.Service/Method".
    ///
    /// This should mostly be used by the wrapper Client implementation, but can be used directly
    /// if needed.
    async fn call_unary(&self, path: &str, req: UnaryRequest<I>) -> Result<UnaryResponse<O>> {
        let req = self.common.unary_request(path, req)?;
        let timeout = request::get_timeout(&req);
        let mut req: reqwest::Request = req.try_into()?;
        *req.timeout_mut() = timeout;
        let response = self.client.execute(req).await?;
        let response = Self::response_to_http_bytes(response).await?;
        self.common.unary_response(response).await
    }

    /// Calls a unary RPC method with the given path and request using HTTP GET.
    /// The path is the RPC path, e.g. "/package.Service/Method".
    async fn call_unary_get(&self, path: &str, req: UnaryRequest<I>) -> Result<UnaryResponse<O>> {
        let req = self.common.unary_get_request(path, req)?;
        let timeout = request::get_timeout(&req);
        let req = req.map(|()| reqwest::Body::default());
        let mut req: reqwest::Request = req.try_into()?;
        *req.timeout_mut() = timeout;
        let response = self.client.execute(req).await?;
        let response = Self::response_to_http_bytes(response).await?;
        self.common.unary_response(response).await
    }
}

impl<I, O> AsyncStreamingClient<I, O> for ReqwestClient
where
    I: EncodeMessage,
    O: DecodeMessage,
{
    async fn call_server_streaming(
        &self,
        path: &str,
        req: ServerStreamingRequest<I>,
    ) -> Result<ServerStreamingResponse<O>> {
        let req = self.common.streaming_request(path, req)?;
        let timeout = request::get_timeout(&req);
        let mut req: reqwest::Request = req.try_into()?;
        *req.timeout_mut() = timeout;
        let response = self.client.execute(req).await?;
        if response.status().is_success() {
            let stream = response.bytes_stream();
            let frames = ConnectFrame::bytes_stream(stream);
            Ok(ServerStreamingResponse {
                status: http::StatusCode::OK,
                codec: self.common.message_codec,
                metadata: http::HeaderMap::new(),
                message_stream: std::boxed::Box::pin(frames),
                _marker: std::marker::PhantomData,
            })
        } else {
            todo!("Handle error response status: {}", response.status())
        }
    }

    async fn call_client_streaming<S>(
        &self,
        path: &str,
        req: ClientStreamingRequest<I, S>,
    ) -> Result<ClientStreamingResponse<O>>
    where
        S: Stream<Item = I> + Send + Sync + 'static,
        I: 'static,
    {
        use reqwest::Body;

        let crate::request::Parts {
            metadata,
            body: message_stream,
        } = req.into_parts();

        // Build the base request using the builder
        let builder = self
            .common
            .builder
            .clone()
            .rpc_path(path)?
            .message_codec(self.common.message_codec)
            .append_metadata(metadata);

        // Encode each message
        let codec = self.common.message_codec;
        let encoded_stream = message_stream.map(move |msg| codec.encode(&msg));

        // Wrap in UnpinStream for Unpin compatibility
        let unpin_stream = UnpinStream(Box::pin(encoded_stream));

        // Create the HTTP request with frame encoding
        let http_req = builder.client_streaming(unpin_stream)?;
        let timeout = request::get_timeout(&http_req);

        // Split the request to get parts and body separately
        let (parts, frame_encoder) = http_req.into_parts();

        // Construct reqwest request manually
        let mut req_builder = self.client.request(parts.method, parts.uri.to_string());

        // Add headers
        for (name, value) in &parts.headers {
            req_builder = req_builder.header(name.clone(), value.clone());
        }

        // Wrap frame encoder stream directly in reqwest Body (true streaming!)
        let body = Body::wrap_stream(frame_encoder);
        req_builder = req_builder.body(body);

        // Set timeout
        if let Some(timeout) = timeout {
            req_builder = req_builder.timeout(timeout);
        }

        // Execute request
        let response = req_builder.send().await?;

        // Check response status
        if response.status().is_success() {
            let status = response.status();
            let headers = response.headers().clone();

            // Read response body
            let body_bytes = response.bytes().await?;

            // Decode response
            let message: O = self.common.message_codec.decode(&body_bytes)?;

            Ok(ClientStreamingResponse {
                status,
                metadata: headers,
                message,
            })
        } else {
            todo!("Handle error response status: {}", response.status())
        }
    }
}

impl ReqwestClient {
    async fn response_to_http_bytes(mut resp: reqwest::Response) -> Result<http::Response<Bytes>> {
        let status = resp.status();
        let headers = std::mem::take(resp.headers_mut());
        let body = resp.bytes().await?;
        let mut http_resp = http::Response::new(body);
        *http_resp.status_mut() = status;
        *http_resp.headers_mut() = headers;
        Ok(http_resp)
    }
}

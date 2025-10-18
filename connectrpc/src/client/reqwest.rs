use super::{AsyncUnaryClient, CommonClient};
use crate::Result;
use crate::client::AsyncStreamingClient;
use crate::codec::Codec;
use crate::connect::{DecodeMessage, EncodeMessage};
use crate::request::{self, StreamingRequest, UnaryRequest};
use crate::response::{ServerStreamingResponse, UnaryResponse};
use crate::stream::ConnectFrame;
use bytes::Bytes;
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
        req: StreamingRequest<I>,
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
            todo!()
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

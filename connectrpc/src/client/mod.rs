#[cfg(feature = "reqwest")]
pub mod reqwest;
#[cfg(feature = "async")]
use futures_util::Stream;
#[cfg(feature = "reqwest")]
pub use reqwest::ReqwestClient;

use crate::Result;
use crate::codec::Codec;
use crate::connect::{DecodeMessage, EncodeMessage};
use crate::error::Error;
use crate::request::{self, UnaryRequest};
#[cfg(feature = "async")]
use crate::request::{BidiStreamingRequest, ClientStreamingRequest, ServerStreamingRequest};
use crate::response::UnaryResponse;
#[cfg(feature = "async")]
use crate::response::{BidiStreamingResponse, ClientStreamingResponse, ServerStreamingResponse};
use crate::stream::ServerStreamingEncoder;
use bytes::Bytes;
use http::Uri;

/// A client that can make asynchronous unary RPC calls.
/// This is a low-level trait that is typically implemented by specific HTTP client libraries.
/// Higher-level client wrappers can be built on top of this trait.
/// The trait is generic over the request and response message types.
/// The request and response types must implement `EncodeMessage` and `DecodeMessage` respectively.
/// This trait is only available when the `async` feature is enabled.
#[cfg(feature = "async")]
pub trait AsyncUnaryClient<I, O>: Send + Sync
where
    I: Send + Sync,
    O: Send + Sync,
{
    /// Calls a unary RPC method with the given path and request.
    /// The request is *POST* request.
    fn call_unary(
        &self,
        path: &str,
        req: UnaryRequest<I>,
    ) -> impl Future<Output = Result<UnaryResponse<O>>>;

    /// Calls a unary RPC method with the given path and request using HTTP GET.
    /// The request is *GET* request.
    fn call_unary_get(
        &self,
        path: &str,
        req: UnaryRequest<I>,
    ) -> impl Future<Output = Result<UnaryResponse<O>>>;
}

#[cfg(feature = "async")]
pub trait AsyncStreamingClient<I, O>: Send + Sync
where
    I: Send + Sync,
    O: Send + Sync,
{
    fn call_server_streaming(
        &self,
        path: &str,
        req: ServerStreamingRequest<I>,
    ) -> impl Future<Output = Result<ServerStreamingResponse<O>>>;

    fn call_client_streaming<S>(
        &self,
        path: &str,
        req: ClientStreamingRequest<I, S>,
    ) -> impl Future<Output = Result<ClientStreamingResponse<O>>>
    where
        S: Stream<Item = Result<I>> + Send + 'static;

    fn call_bidi_streaming<SReq>(
        &self,
        path: &str,
        req: BidiStreamingRequest<I, SReq>,
    ) -> impl Future<Output = Result<BidiStreamingResponse<O>>>
    where
        SReq: Stream<Item = Result<I>> + Send + 'static;
}

#[cfg(feature = "sync")]
pub trait SyncUnaryClient<I, O>: Send + Sync
where
    I: Send + Sync,
    O: Send + Sync,
{
    fn call_unary(&self, path: &str, req: UnaryRequest<I>) -> Result<UnaryResponse<O>>;

    fn call_unary_get(&self, path: &str, req: UnaryRequest<I>) -> Result<UnaryResponse<O>>;
}

/// A common client implementation that can be used by different HTTP client libraries.
/// This is a low-level client that handles request building and response parsing.
/// It has nothing to do with the actual HTTP transport.
///
/// It uses the codec to encode and decode messages, and the builder as a starting point to build
/// requests.
#[derive(Clone)]
pub struct CommonClient {
    builder: request::Builder,
    message_codec: Codec,
}

impl CommonClient {
    /// Creates a new `CommonClient` with the given base URI and message codec.
    pub fn new(uri: Uri, message_codec: Codec) -> Result<Self> {
        let http::uri::Parts {
            scheme,
            authority,
            path_and_query,
            ..
        } = uri.into_parts();

        let path = path_and_query
            .as_ref()
            .map(|pq| pq.path().trim_end_matches('/').to_string())
            .unwrap_or_default();

        let query = path_and_query
            .as_ref()
            .and_then(|pq| pq.query())
            .map(|q| q.to_string());

        let mut builder = request::Builder::new()
            .scheme(scheme.ok_or_else(|| {
                Error::invalid_request("URI scheme is required, e.g. http or https")
            })?)
            .authority(
                authority
                    .ok_or_else(|| {
                        Error::invalid_request("URI authority is required, e.g. example.com")
                    })?
                    .as_str(),
            )?
            .path_prefix(path);

        if let Some(q) = query {
            builder = builder.query(q);
        }

        Ok(Self {
            builder,
            message_codec,
        })
    }

    /// Builds a unary request with the given path and request.
    pub fn unary_request<Req>(
        &self,
        path: &str,
        req: UnaryRequest<Req>,
    ) -> Result<http::Request<Vec<u8>>>
    where
        Req: EncodeMessage,
    {
        let crate::request::Parts { metadata, body } = req.into_parts();
        let body = self.message_codec.encode(&body);
        self.builder
            .clone()
            .rpc_path(path)?
            .message_codec(self.message_codec)
            .append_metadata(metadata)
            .unary(body)
    }

    /// Builds a unary GET request with the given path and request.
    pub fn unary_get_request<Req>(
        &self,
        path: &str,
        req: UnaryRequest<Req>,
    ) -> Result<http::Request<()>>
    where
        Req: EncodeMessage,
    {
        let crate::request::Parts { metadata, body } = req.into_parts();
        let body = self.message_codec.encode(&body);
        self.builder
            .clone()
            .rpc_path(path)?
            .message_codec(self.message_codec)
            .append_metadata(metadata)
            .unary_get(body)
    }

    pub fn streaming_request<Req>(
        &self,
        path: &str,
        req: ServerStreamingRequest<Req>,
    ) -> Result<http::Request<ServerStreamingEncoder>>
    where
        Req: EncodeMessage,
    {
        let crate::request::Parts { metadata, body } = req.into_parts();
        let body = self.message_codec.encode(&body);
        self.builder
            .clone()
            .rpc_path(path)?
            .message_codec(self.message_codec)
            .append_metadata(metadata)
            .server_streaming(body)
    }

    /// Parses a unary response from the given HTTP response.
    /// This method decodes the response body using the message codec.
    pub async fn unary_response<Res>(
        &self,
        resp: http::Response<Bytes>,
    ) -> Result<UnaryResponse<Res>>
    where
        Res: DecodeMessage,
    {
        let (parts, body) = resp.into_parts();
        let body = self.message_codec.decode(&body)?;
        Ok(UnaryResponse::from_parts(crate::response::Parts {
            status: parts.status,
            metadata: parts.headers,
            message: body,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::b64;
    use prost::Message;
    use serde::{Deserialize, Serialize};
    use std::collections::BTreeMap;

    #[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
    struct TestMessage {
        #[prost(string, tag = "1")]
        message: String,
    }

    #[test]
    fn test_common_client_post() {
        let uri: Uri = "http://example.com/api/v1".parse().unwrap();
        let client = CommonClient::new(uri, Codec::Json).unwrap();

        let req = UnaryRequest::new(TestMessage {
            message: "test message".to_string(),
        });
        let http_req = client
            .unary_request("/TestService/TestMethod", req)
            .unwrap();
        assert_eq!(http_req.method(), http::Method::POST);
        assert_eq!(http_req.uri().path(), "/api/v1/TestService/TestMethod");
        assert_eq!(
            http_req.headers().get(http::header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
    }

    #[test]
    fn test_common_client_get() {
        let uri: Uri = "http://example.com/api/v1".parse().unwrap();
        let client = CommonClient::new(uri, Codec::Json).unwrap();
        let req = UnaryRequest::new(TestMessage {
            message: "test message".to_string(),
        });
        let http_req = client
            .unary_get_request("/TestService/TestMethod", req)
            .unwrap();
        assert_eq!(http_req.method(), http::Method::GET);
        assert_eq!(http_req.uri().path(), "/api/v1/TestService/TestMethod");
        let query = http_req.uri().query().unwrap();
        let query = form_urlencoded::parse(query.as_bytes())
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<BTreeMap<String, String>>();

        let message = b64::url_encode(
            serde_json::to_vec(&TestMessage {
                message: "test message".to_string(),
            })
            .expect("Failed to serialize message"),
        );
        assert_eq!(query.get("message").unwrap(), &message);
        assert_eq!(query.get("encoding").unwrap(), "json");
        assert_eq!(query.get("base64").unwrap(), "1");
    }

    #[tokio::test]
    async fn test_common_client_response() {
        let uri: Uri = "http://example.com/api/v1".parse().unwrap();
        let client = CommonClient::new(uri, Codec::Json).unwrap();
        let response_body = Bytes::from(r#""response message""#);
        let http_resp = http::Response::builder()
            .status(200)
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(response_body)
            .unwrap();

        let unary_resp: UnaryResponse<String> = client.unary_response(http_resp).await.unwrap();
        assert_eq!(unary_resp.status(), http::StatusCode::OK);
        assert_eq!(unary_resp.message(), "response message");
    }
}

#[cfg(feature = "reqwest")]
pub mod reqwest;

#[cfg(feature = "reqwest")]
pub use reqwest::ReqwestClient;

use crate::Result;
use crate::codec::Codec;
use crate::connect::{DecodeMessage, EncodeMessage};
use crate::error::Error;
use crate::request::{self, UnaryRequest};
use crate::response::UnaryResponse;
use bytes::Bytes;
use http::Uri;

#[cfg(feature = "async")]
pub trait AsyncUnaryClient<I, O>: Send + Sync
where
    I: Send + Sync,
    O: Send + Sync,
{
    fn call_unary(
        &self,
        path: &str,
        req: UnaryRequest<I>,
    ) -> impl Future<Output = Result<UnaryResponse<O>>>;

    fn call_unary_get(
        &self,
        path: &str,
        req: UnaryRequest<I>,
    ) -> impl Future<Output = Result<UnaryResponse<O>>>;
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

#[derive(Clone)]
pub struct CommonClient {
    builder: request::Builder,
    message_codec: Codec,
}

impl CommonClient {
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

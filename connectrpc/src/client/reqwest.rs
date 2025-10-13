use super::{AsyncUnaryClient, CommonClient};
use crate::Result;
use crate::codec::Codec;
use crate::connect::{DecodeMessage, EncodeMessage};
use crate::request::{self, UnaryRequest};
use crate::response::UnaryResponse;
use bytes::Bytes;
use http::Uri;

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
    async fn call_unary(&self, path: &str, req: UnaryRequest<I>) -> Result<UnaryResponse<O>> {
        let req = self.common.unary_request(path, req)?;
        let timeout = request::get_timeout(&req);
        let mut req: reqwest::Request = req.try_into()?;
        *req.timeout_mut() = timeout;
        let response = self.client.execute(req).await?;
        let response = Self::response_to_http_bytes(response).await?;
        self.common.unary_response(response).await
    }

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

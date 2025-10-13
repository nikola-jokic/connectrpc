use crate::Result;
use crate::error::Error;
use crate::header::{CONTENT_TYPE, HeaderValue};
use crate::request::RequestResponseOptions;
use crate::request::UnaryRequest;
use crate::response::UnaryResponse;
use crate::server::CommonServer;
use axum::body::{self, Body};
use axum::http::{Method, Request};
use axum::response::Response;
use prost::Message;
use serde::{Serialize, de::DeserializeOwned};
use std::pin::Pin;

pub trait RpcUnaryHandler<TMReq, TMRes, TState>: Clone + Send + Sync + Sized + 'static {
    type Future: Future<Output = Response> + Send + 'static;

    fn call(self, req: Request<Body>, state: TState, srv: CommonServer) -> Self::Future;
}

/// A simple implementation of RpcUnaryHandler for a function
/// that takes a state and a UnaryRequest, and returns a Future
/// that resolves to a UnaryResponse.
///
/// This implementation would allow you to apply any function that matches the
/// following signature:
/// `async fn handler(state: TState, req: UnaryRequest<TMReq>) -> connectrpc::Result<UnaryResponse<TMRes>>`
/// as an RPC handler in an Axum application.
impl<TMReq, TMRes, TFnFut, TFn, TState> RpcUnaryHandler<TMReq, TMRes, TState> for TFn
where
    TMReq: Message + DeserializeOwned + Default + Send + 'static,
    TMRes: Message + Serialize + Send + 'static,
    TFnFut: Future<Output = Result<UnaryResponse<TMRes>>> + Send + 'static,
    TFn: FnOnce(TState, UnaryRequest<TMReq>) -> TFnFut + Clone + Send + Sync + 'static,
    TState: Send + Sync + 'static,
{
    type Future = Pin<Box<dyn Future<Output = Response> + Send>>;

    fn call(self, req: Request<Body>, state: TState, srv: CommonServer) -> Self::Future {
        Box::pin(async move {
            let (req, option) = match parse_request(req, srv).await {
                Ok(r) => r,
                Err(e) => return Response::from(e),
            };
            let res = match self(state, req).await {
                Ok(r) => r,
                Err(e) => return Response::from(e),
            };

            let crate::response::Parts {
                status,
                metadata: headers,
                message: body,
            } = res.into_parts();

            let mut builder = http::Response::builder().status(status);

            for (k, v) in headers.into_iter() {
                builder = builder.header(k.unwrap(), v);
            }

            builder = builder.header(
                CONTENT_TYPE,
                HeaderValue::from_str(&format!("application/{}", option.message_codec.name()))
                    .unwrap(),
            );

            for option in option.accept_encodings {
                builder = builder.header("Accept-Encoding", option);
            }

            let bytes = option.message_codec.encode(&body);

            let result = builder.body(bytes).expect("builder should not fail");

            result.map(Body::from)
        })
    }
}

impl From<Error> for Response {
    fn from(err: Error) -> Self {
        let http_response: http::Response<Vec<u8>> = err.into();
        http_response.map(Body::from)
    }
}

// impl TryFrom<Request<Body>> for UnaryRequest<Vec<u8>> {}
async fn parse_request<TMReq>(
    req: Request<Body>,
    srv: CommonServer,
) -> Result<(UnaryRequest<TMReq>, RequestResponseOptions)>
where
    TMReq: Message + DeserializeOwned + Default + Send + 'static,
{
    let method = req.method();
    match *method {
        Method::POST => parse_unary_post_request(req, srv).await,
        Method::GET => parse_unary_get_request(req, srv).await,
        _ => Err(Error::invalid_request(format!(
            "unsupported HTTP method: {method}"
        ))),
    }
}

async fn parse_unary_post_request<TMReq>(
    req: Request<Body>,
    srv: CommonServer,
) -> Result<(UnaryRequest<TMReq>, RequestResponseOptions)>
where
    TMReq: Message + DeserializeOwned + Default + Send + 'static,
{
    let (parts, body) = req.into_parts();
    let http::request::Parts { headers, .. } = parts;

    let codec = srv.parse_unary_headers(&headers)?;

    let bytes = body::to_bytes(body, usize::MAX)
        .await
        .map_err(|e| Error::invalid_request(format!("failed to read request body: {}", e)))?
        .to_vec();

    let body: TMReq = codec.decode(&bytes).map_err(|e| {
        Error::invalid_request(format!(
            "failed to decode request body with {codec:?}: {}",
            e
        ))
    })?;

    Ok((
        UnaryRequest::from_parts(crate::request::Parts {
            metadata: headers,
            body,
        }),
        RequestResponseOptions {
            message_codec: codec,
            accept_encodings: vec![],
        },
    ))
}

async fn parse_unary_get_request<TMReq>(
    req: Request<Body>,
    srv: CommonServer,
) -> Result<(UnaryRequest<TMReq>, RequestResponseOptions)>
where
    TMReq: Message + DeserializeOwned + Default + Send + 'static,
{
    let (parts, ..) = req.into_parts();
    let http::request::Parts { headers, uri, .. } = parts;

    let (body, codec) = srv.parse_unary_get_request(&uri)?;

    Ok((
        UnaryRequest::from_parts(crate::request::Parts {
            metadata: headers,
            body,
        }),
        RequestResponseOptions {
            message_codec: codec,
            accept_encodings: vec![],
        },
    ))
}

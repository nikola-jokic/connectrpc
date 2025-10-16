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

/// A trait for handling unary RPC requests in an Axum application.
///
/// This implementation is internally used by the `axum` module to handle
/// incoming unary RPC requests. It defines a method `call` that takes an
/// HTTP request, a state, and a common server instance, and returns a future
/// that resolves to an HTTP response.
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
                if let Some(header_name) = k {
                    builder = builder.header(header_name, v);
                }
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

/// Parses a unary POST request, extracting the body and decoding it using the appropriate codec.
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

/// Parses a unary GET request, extracting the body from the URL and decoding it using the
/// appropriate codec.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::response::UnaryResponse;
    use crate::{Codec, request::UnaryRequest};
    use prost::Message;
    use serde::{Deserialize, Serialize};

    #[test]
    fn test_error_to_response() {
        let err = Error::already_exists("example");
        let response: Response = Response::from(err);
        assert_eq!(response.status(), 409);
    }

    #[derive(Message, Serialize, Deserialize, Clone)]
    struct TestRequest {
        #[prost(string, tag = "1")]
        message: String,
    }

    #[derive(Message, Serialize, Deserialize, Clone)]
    struct TestResponse {
        #[prost(string, tag = "1")]
        message: String,
    }

    #[derive(Clone)]
    struct State {}

    #[tokio::test]
    async fn test_handler_error() {
        use crate::header::{CONNECT_PROTOCOL_VERSION, CONNECT_PROTOCOL_VERSION_1};

        let state = State {};
        let srv = CommonServer::default();
        let handler = async |_state: State,
                             _req: UnaryRequest<TestRequest>|
               -> Result<UnaryResponse<TestResponse>> {
            Err(Error::already_exists("the resource already exists"))
        };
        let request = Request::builder()
            .method(Method::POST)
            .uri("http://localhost/svc.Service/Method")
            .header("Content-Type", "application/json")
            .header(CONNECT_PROTOCOL_VERSION, CONNECT_PROTOCOL_VERSION_1)
            .body(Body::from(r#"{"message":"hello"}"#))
            .unwrap();
        let response = handler.call(request, state, srv).await;
        assert_eq!(response.status(), 409);
    }

    #[tokio::test]
    async fn test_handler_success() {
        use crate::header::{CONNECT_PROTOCOL_VERSION, CONNECT_PROTOCOL_VERSION_1};

        let state = State {};
        let srv = CommonServer::default();
        let handler = async |_state: State,
                             req: UnaryRequest<TestRequest>|
               -> Result<UnaryResponse<TestResponse>> {
            let response = TestResponse {
                message: format!("echo: {}", req.message().message),
            };
            Ok(UnaryResponse::new(response))
        };

        let codec = Codec::Proto;
        let req = codec.encode(&TestRequest {
            message: "hello".into(),
        });

        let request = Request::builder()
            .method(Method::POST)
            .uri("http://localhost/svc.Service/Method")
            .header("Content-Type", "application/proto")
            .header(CONNECT_PROTOCOL_VERSION, CONNECT_PROTOCOL_VERSION_1)
            .body(Body::from(req.clone()))
            .unwrap();
        let response = handler.call(request, state, srv).await;
        assert_eq!(response.status(), 200);
        let (parts, body) = response.into_parts();
        assert_eq!(
            parts.headers.get("Content-Type").unwrap(),
            "application/proto"
        );
        let body_bytes = body::to_bytes(body, usize::MAX).await.unwrap();
        let res = codec.decode::<TestResponse>(&body_bytes).unwrap();
        assert_eq!(res.message, "echo: hello");
    }
}

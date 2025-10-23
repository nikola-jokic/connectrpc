use crate::Result;
use crate::b64;
use crate::codec::Codec;
use crate::connect::DecodeMessage;
use crate::error::Error;
use crate::header::{CONNECT_PROTOCOL_VERSION, CONNECT_PROTOCOL_VERSION_1};
use http::header::CONTENT_TYPE;
use http::{HeaderMap, Uri};
use std::collections::BTreeMap;

#[cfg(feature = "axum")]
pub mod axum;

/// A common server implementation that can be used by different HTTP server libraries.
/// For now, it is basically an empty struct with some helper methods.
///
/// However, keeping it around allows us to add codecs if needed in the future.
#[derive(Debug, Clone, Default)]
pub struct CommonServer {}

impl CommonServer {
    pub fn new() -> Self {
        Self {}
    }

    /// Parse headers for unary requests.
    /// This parses only headers so if the request is not valid,
    /// the body doesn't need to be read.
    pub fn parse_unary_headers(&self, headers: &HeaderMap) -> Result<Codec> {
        // Do not require version to be specified: https://connectrpc.com/docs/curl-and-other-clients#curl
        let version = headers.get(CONNECT_PROTOCOL_VERSION).cloned().unwrap_or(CONNECT_PROTOCOL_VERSION_1);
        if version != CONNECT_PROTOCOL_VERSION_1 {
            return Err(Error::unsupported_media_type(format!(
                "unsupported connect-protocol-version version: {:?}",
                version
            )));
        }

        let codec = match headers.get(CONTENT_TYPE).and_then(|v| v.to_str().ok()) {
            Some("application/json") => Codec::Json,
            Some("application/proto") => Codec::Proto,
            Some(ct) => {
                return Err(Error::unsupported_media_type(format!(
                    "unsupported Content-Type: {ct}"
                )));
            }
            None => return Err(Error::invalid_request("missing Content-Type header")),
        };
        Ok(codec)
    }

    /// Parse a unary GET request.
    /// This parses the query string and decodes the message.
    /// If the request is not valid, it returns an error.
    pub fn parse_unary_get_request<Res>(&self, uri: &Uri) -> Result<(Res, Codec)>
    where
        Res: DecodeMessage,
    {
        let query = uri
            .query()
            .ok_or_else(|| Error::invalid_request("missing query string for GET unary request"))?;

        let mut query = form_urlencoded::parse(query.as_bytes())
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<BTreeMap<_, _>>();

        if query.remove("connect").as_deref() != Some("v1") {
            return Err(Error::invalid_request(
                "missing or invalid 'connect' parameter in query string",
            ));
        }

        let content_type = query
            .remove("encoding")
            .and_then(|enc| format!("application/{}", enc).into())
            .ok_or_else(|| {
                Error::invalid_request("missing 'encoding' parameter in query string")
            })?;

        let codec = match content_type.as_str() {
            "application/json" => Codec::Json,
            "application/proto" => Codec::Proto,
            _ => {
                return Err(Error::invalid_request(format!(
                    "unsupported Content-Type: {content_type}"
                )));
            }
        };

        let is_b64 = query.remove("base64").as_deref() == Some("1");

        let message = if is_b64 {
            b64::url_decode(query.remove("message").ok_or_else(|| {
                Error::invalid_request("missing 'message' parameter in query string")
            })?)
            .map_err(|e| {
                Error::invalid_request(format!(
                    "failed to base64-decode 'message' parameter: {}",
                    e
                ))
            })?
        } else {
            query
                .remove("message")
                .ok_or_else(|| {
                    Error::invalid_request("missing 'message' parameter in query string")
                })?
                .into_bytes()
        };

        let body: Res = codec.decode(&message).map_err(|e| {
            Error::invalid_request(format!(
                "failed to decode request body with {codec:?}: {}",
                e
            ))
        })?;
        Ok((body, codec))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::Codec;
    use prost::Message;
    use serde::{Deserialize, Serialize};

    #[test]
    fn test_parse_unary_headers_json_encoder() {
        let srv = CommonServer::new();
        let mut headers = HeaderMap::new();
        headers.insert(CONNECT_PROTOCOL_VERSION, CONNECT_PROTOCOL_VERSION_1);
        headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());
        let codec = srv.parse_unary_headers(&headers).unwrap();
        assert_eq!(codec, Codec::Json);
    }

    #[test]
    fn test_parse_unary_headers_proto_encoder() {
        let srv = CommonServer::new();
        let mut headers = HeaderMap::new();
        headers.insert(CONNECT_PROTOCOL_VERSION, CONNECT_PROTOCOL_VERSION_1);
        headers.insert(CONTENT_TYPE, "application/proto".parse().unwrap());
        let codec = srv.parse_unary_headers(&headers).unwrap();
        assert_eq!(codec, Codec::Proto);
    }

    #[test]
    fn test_parse_unary_headers_missing_version() {
        let srv = CommonServer::new();
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());
        let err = srv.parse_unary_headers(&headers).unwrap_err();
        assert!(
            err.to_string()
                .contains("missing connect-protocol-version header")
        );
    }

    #[test]
    fn test_parse_unary_headers_unsupported_version() {
        let srv = CommonServer::new();
        let mut headers = HeaderMap::new();
        headers.insert(CONNECT_PROTOCOL_VERSION, "v2".parse().unwrap());
        headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());
        let err = srv.parse_unary_headers(&headers).unwrap_err();
        assert!(
            err.to_string()
                .contains("unsupported connect-protocol-version version")
        );
    }

    #[test]
    fn test_parse_unary_headers_missing_content_type() {
        let srv = CommonServer::new();
        let mut headers = HeaderMap::new();
        headers.insert(CONNECT_PROTOCOL_VERSION, CONNECT_PROTOCOL_VERSION_1);
        let err = srv.parse_unary_headers(&headers).unwrap_err();
        assert!(err.to_string().contains("missing Content-Type header"));
    }

    #[test]
    fn test_parse_unary_headers_unsupported_content_type() {
        let srv = CommonServer::new();
        let mut headers = HeaderMap::new();
        headers.insert(CONNECT_PROTOCOL_VERSION, CONNECT_PROTOCOL_VERSION_1);
        headers.insert(CONTENT_TYPE, "application/xml".parse().unwrap());
        let err = srv.parse_unary_headers(&headers).unwrap_err();
        assert!(err.to_string().contains("unsupported Content-Type"));
    }

    #[derive(Serialize, Deserialize, PartialEq, prost::Message)]
    struct TestMessage {
        #[prost(string, tag = "1")]
        foo: String,
    }

    #[test]
    fn test_parse_unary_get_request_json() {
        let srv = CommonServer::new();
        let uri: Uri =
            "/service/method?connect=v1&encoding=json&message=eyJmb28iOiAiYmFyIn0&base64=1"
                .parse()
                .unwrap();
        let (body, codec) = srv.parse_unary_get_request::<TestMessage>(&uri).unwrap();
        assert_eq!(codec, Codec::Json);
        assert_eq!(body, TestMessage { foo: "bar".into() });
    }

    #[test]
    fn test_parse_unary_get_request_proto_b64() {
        let srv = CommonServer::new();
        let msg = TestMessage { foo: "bar".into() };
        let val = b64::url_encode(msg.encode_to_vec());

        let uri: Uri = format!("/service/method?connect=v1&encoding=proto&message={val}&base64=1")
            .parse()
            .unwrap();
        let (body, codec) = srv.parse_unary_get_request::<TestMessage>(&uri).unwrap();
        assert_eq!(codec, Codec::Proto);
        assert_eq!(body, TestMessage { foo: "bar".into() });
    }

    #[test]
    fn test_parse_unary_get_request_proto_no_b64() {
        let srv = CommonServer::new();
        let msg = TestMessage { foo: "bar".into() };
        let val = String::from_utf8(msg.encode_to_vec()).unwrap();
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("connect", "v1")
            .append_pair("encoding", "proto")
            .append_pair("message", &val)
            .finish();
        let uri: Uri = format!("/service/method?{query}").parse().unwrap();
        let (body, codec) = srv.parse_unary_get_request::<TestMessage>(&uri).unwrap();
        assert_eq!(codec, Codec::Proto);
        assert_eq!(body, TestMessage { foo: "bar".into() });
    }

    #[test]
    fn test_parse_unary_get_request_missing_query() {
        let srv = CommonServer::new();
        let uri: Uri = "/service/method".parse().unwrap();
        let err = srv
            .parse_unary_get_request::<TestMessage>(&uri)
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("missing query string for GET unary request")
        );
    }

    #[test]
    fn test_parse_unary_get_request_missing_connect() {
        let srv = CommonServer::new();
        let uri: Uri = "/service/method?encoding=json&message=eyJmb28iOiAiYmFyIn0&base64=1"
            .parse()
            .unwrap();
        let err = srv
            .parse_unary_get_request::<TestMessage>(&uri)
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("missing or invalid 'connect' parameter in query string")
        );
    }

    #[test]
    fn test_parse_unary_get_request_missing_encoding() {
        let srv = CommonServer::new();
        let uri: Uri = "/service/method?connect=v1&message=eyJmb28iOiAiYmFyIn0&base64=1"
            .parse()
            .unwrap();
        let err = srv
            .parse_unary_get_request::<TestMessage>(&uri)
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("missing 'encoding' parameter in query string")
        );
    }
}

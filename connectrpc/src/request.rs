use crate::Result;
use crate::b64;
use crate::codec::Codec;
use crate::error::Error;
use crate::header;
use crate::header::{
    ACCEPT_ENCODING, CONNECT_PROTOCOL_VERSION, CONNECT_PROTOCOL_VERSION_1, CONNECT_TIMEOUT_MS,
    CONTENT_ENCODING, CONTENT_TYPE,
};
use crate::metadata::Metadata;
use http::uri::{Authority, Scheme};
use http::{HeaderMap, HeaderName, HeaderValue, Method, Uri};
use std::time::Duration;

/// A builder for constructing HTTP requests for Connect services.
/// You can use this builder to create requests directly.
/// The same builder is used internally by clients.
#[derive(Debug, Clone)]
pub struct Builder {
    scheme: Option<Scheme>,
    authority: Option<Authority>,
    path_prefix: Option<String>,
    service: Option<String>,
    method: Option<String>,
    metadata: HeaderMap,
    message_codec: Option<Codec>,
    timeout_ms: Option<HeaderValue>,
    query: Option<String>,
    content_encoding: Option<String>,
    accept_encodings: Vec<HeaderValue>,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            scheme: Some(Scheme::HTTPS),
            authority: Some(Authority::from_static("localhost")),
            path_prefix: None,
            service: None,
            method: None,
            metadata: HeaderMap::new(),
            message_codec: None,
            query: None,
            timeout_ms: None,
            content_encoding: None,
            accept_encodings: vec![],
        }
    }
}

impl Builder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the scheme for the request. Typically either `http` or `https`.
    pub fn scheme(mut self, scheme: Scheme) -> Self {
        self.scheme = Some(scheme);
        self
    }

    /// Set the authority (host and optional port) for the request.
    pub fn authority(mut self, authority: impl Into<String>) -> Result<Self> {
        self.authority = Some(
            authority
                .into()
                .parse()
                .map_err(|e| Error::invalid_request(format!("invalid authority: {}", e)))?,
        );
        Ok(self)
    }

    /// Set a path prefix for the request. If you are using something such as /internal as a
    /// prefix for all your services, you can set it here. The leading and trailing '/' will be
    /// trimmed if present.
    pub fn path_prefix(mut self, path: impl Into<String>) -> Self {
        self.path_prefix = Some(path.into().trim_matches('/').to_string());
        self
    }

    /// Set the RPC path in the form of `/Service/Method`. The leading '/' will be trimmed if
    /// present.
    pub fn rpc_path(mut self, segment: &str) -> Result<Self> {
        let (svc, method) = segment
            .trim_start_matches('/')
            .rsplit_once('/')
            .ok_or_else(|| {
                Error::invalid_request(format!(
                    "rpc path must be in the form of Service/Method: {segment}"
                ))
            })?;
        self.service = Some(svc.trim_matches('/').to_string());
        self.method = Some(method.trim_matches('/').to_string());
        Ok(self)
    }

    /// Set the query string for the request. The leading '?' will be trimmed if present.
    pub fn query(mut self, query: impl Into<String>) -> Self {
        self.query = Some(query.into().trim_start_matches('?').to_string());
        self
    }

    /// Extends the metadata with the provided headers.
    pub fn append_metadata(mut self, headerrs: HeaderMap) -> Self {
        self.metadata.extend(headerrs);
        self
    }

    /// Add an ASCII metadata entry.
    pub fn append_ascii_metadata(
        mut self,
        key: impl TryInto<HeaderName, Error: Into<Error>>,
        val: impl Into<String>,
    ) -> Result<Self> {
        self.metadata.append_ascii(key, val)?;
        Ok(self)
    }

    /// Add a binary metadata entry.
    pub fn binary_metadata(
        mut self,
        key: impl TryInto<http::header::HeaderName, Error: Into<Error>>,
        val: impl AsRef<[u8]>,
    ) -> Result<Self> {
        self.metadata.append_binary(key, val)?;
        Ok(self)
    }

    /// Set the timeout for the request.
    pub fn timeout(mut self, timeout: Duration) -> Result<Self> {
        let timeout = timeout.as_millis().to_string();
        if timeout.len() > 10 {
            return Err(Error::invalid_request("timeout too large"));
        }
        self.timeout_ms = Some(timeout.try_into().unwrap());
        Ok(self)
    }

    /// Set the content encoding of the request.
    pub fn content_encoding(mut self, encoding: impl Into<String>) -> Result<Self> {
        let content_encoding: String = encoding.into();
        if header::is_valid_http_token(&content_encoding) {
            return Err(Error::invalid_request(format!(
                "invalid content encoding: {}",
                content_encoding
            )));
        }
        self.content_encoding = Some(content_encoding);
        Ok(self)
    }

    /// Add an accepted encoding to the request.
    pub fn accept_encodings(mut self, encoding: impl Into<String>) -> Result<Self> {
        let encoding: String = encoding.into();
        if !header::is_valid_http_token(&encoding) {
            return Err(Error::invalid_request(format!(
                "invalid accept encoding: {}",
                encoding
            )));
        }
        self.accept_encodings.push(encoding.try_into().unwrap());
        Ok(self)
    }

    /// Set the message codec (e.g. proto or json)
    pub fn message_codec(mut self, codec: Codec) -> Self {
        self.message_codec = Some(codec);
        self
    }

    /// assumes validate has been called
    fn request_uri(&mut self) -> Uri {
        let mut path = String::new();
        if let Some(prefix) = &self.path_prefix {
            path.push_str(&format!("/{}", prefix));
        }
        let service = self.service.take().unwrap();
        let method = self.method.take().unwrap();
        path.push_str(&format!("/{}/{}", service, method));
        if let Some(query) = &self.query {
            path.push_str(&format!("?{}", query));
        }

        Uri::builder()
            .scheme(self.scheme.take().unwrap())
            .authority(self.authority.take().unwrap())
            .path_and_query(path)
            .build()
            .unwrap()
    }

    /// Build logic common to all requests.
    fn request_base<T>(&mut self, method: Method, body: T) -> Result<http::Request<T>> {
        let mut req = http::Request::new(body);
        *req.uri_mut() = self.request_uri();
        *req.method_mut() = method;
        let mut headers: HeaderMap = std::mem::take(&mut self.metadata);
        // Connect-Protocol-Version → "connect-protocol-version" "1"
        headers.insert(CONNECT_PROTOCOL_VERSION, CONNECT_PROTOCOL_VERSION_1);
        // Timeout → "connect-timeout-ms" Timeout-Milliseconds
        if let Some(timeout) = self.timeout_ms.take() {
            headers.insert(CONNECT_TIMEOUT_MS, timeout);
        }
        *req.headers_mut() = headers;
        Ok(req)
    }

    fn request_content_encoding(&mut self) -> HeaderValue {
        if let Some(ce) = std::mem::take(&mut self.content_encoding) {
            ce.try_into().unwrap()
        } else {
            HeaderValue::from_static("identity")
        }
    }

    /// Build a unary request with the given message as the body.
    /// POST request will be used.
    ///
    /// The user should ensure that the message is encoded according to the specified codec.
    pub fn unary(mut self, message: Vec<u8>) -> Result<http::Request<Vec<u8>>> {
        self.validate()?;
        let mut req = self.request_base(Method::POST, message)?;
        let headers = req.headers_mut();
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_str(&format!(
                "application/{}",
                self.message_codec.as_ref().unwrap().name()
            ))?,
        );
        headers.insert(CONTENT_ENCODING, self.request_content_encoding());
        // Accept-Encoding → "accept-encoding" Content-Coding [...]
        for value in std::mem::take(&mut self.accept_encodings) {
            headers.append(ACCEPT_ENCODING, value);
        }
        Ok(req)
    }

    fn rebuild_get_query(&mut self, message: &[u8]) {
        let mut query = form_urlencoded::Serializer::new(String::new());
        query
            .append_pair("message", &b64::url_encode(message))
            .append_pair("base64", "1")
            .append_pair("connect", "v1")
            .append_pair("encoding", self.message_codec.as_ref().unwrap().name());
        if let Some(content_encoding) = std::mem::take(&mut self.content_encoding) {
            query.append_pair("compression", &content_encoding);
        }
        self.query = Some(query.finish())
    }

    /// Build a unary GET request with the given message as a base64-encoded query parameter.
    /// GET request will be used.
    ///
    /// The user should ensure that the message is encoded according to the specified codec.
    pub fn unary_get(mut self, message: Vec<u8>) -> Result<http::Request<()>> {
        self.validate()?;
        self.rebuild_get_query(&message);
        let mut req = self.request_base(Method::GET, ())?;
        let headers = req.headers_mut();
        // Accept-Encoding → "accept-encoding" Content-Coding [...]
        for value in std::mem::take(&mut self.accept_encodings) {
            headers.append(ACCEPT_ENCODING, value);
        }
        Ok(req)
    }

    /// Validate that all required fields are set.
    ///
    /// This method will be called automatically by the build methods.
    pub fn validate(&self) -> Result<()> {
        if self.scheme.is_none() {
            return Err(Error::invalid_request(
                "scheme must be set before building request",
            ));
        }
        if self.authority.is_none() {
            return Err(Error::invalid_request(
                "authority must be set before building request",
            ));
        }
        if self.service.is_none() || self.method.is_none() {
            return Err(Error::invalid_request(
                "service and method must be set before building request",
            ));
        }
        if self.message_codec.is_none() {
            return Err(Error::invalid_request(
                "codec must be set before building request",
            ));
        }
        Ok(())
    }
}

/// Parts of a request, used for decomposing and composing requests.
pub struct Parts<T>
where
    T: Send + Sync,
{
    pub metadata: HeaderMap,
    pub body: T,
}

/// Options for configuring expected response based on the request.
#[derive(Debug, Clone)]
pub struct RequestResponseOptions {
    pub message_codec: Codec,
    pub accept_encodings: Vec<String>,
}

impl Default for RequestResponseOptions {
    fn default() -> Self {
        Self {
            message_codec: Codec::Proto,
            accept_encodings: vec!["identity".to_string()],
        }
    }
}

/// A unary request with metadata and a message.
/// This is used by both client and server.
///
/// The reason we have our own request type instead of using
/// `http::Request<T>` directly is that we want to ensure
/// that the metadata is always a `HeaderMap` and the creation
/// of the request is not containing the URI, method, etc. which
/// won't be respected anyway.
#[derive(Debug, Clone)]
pub struct UnaryRequest<T>
where
    T: Send + Sync,
{
    metadata: HeaderMap,
    message: T,
}

impl<T> UnaryRequest<T>
where
    T: Send + Sync,
{
    /// Create a new unary request with the given message and empty metadata.
    pub fn new(message: T) -> Self {
        Self {
            metadata: HeaderMap::new(),
            message,
        }
    }

    /// Returns a reference to the metadata.
    pub fn metadata(&self) -> &HeaderMap {
        &self.metadata
    }

    /// Returns a mutable reference to the metadata.
    pub fn metadata_mut(&mut self) -> &mut HeaderMap {
        &mut self.metadata
    }

    /// Decomposes the request into its parts.
    pub fn into_parts(self) -> Parts<T> {
        Parts {
            metadata: self.metadata,
            body: self.message,
        }
    }

    /// Creates a request from its parts.
    pub fn from_parts(parts: Parts<T>) -> Self {
        Self {
            metadata: parts.metadata,
            message: parts.body,
        }
    }

    /// Consumes the request, returning the message.
    pub fn into_message(self) -> T {
        self.message
    }

    /// Returns a reference to the message.
    pub fn message(&self) -> &T {
        &self.message
    }
}

#[cfg(feature = "client")]
pub(crate) fn get_timeout<T>(req: &http::Request<T>) -> Option<Duration> {
    req.headers()
        .get(CONNECT_TIMEOUT_MS)
        .and_then(|hv| hv.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_millis)
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost::Message;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, PartialEq, Message)]
    struct HelloRequest {
        #[prost(string, tag = "1")]
        name: String,
    }

    #[test]
    fn test_builder_unary() {
        for codec in [Codec::Proto, Codec::Json] {
            let request = HelloRequest {
                name: "world".to_string(),
            };
            let body = codec.encode(&request);

            let req = Builder::new()
                .scheme(Scheme::HTTPS)
                .authority("example.com")
                .unwrap()
                .rpc_path("/helloworld.Greeter/SayHello")
                .unwrap()
                .message_codec(codec)
                .unary(body.clone())
                .unwrap();
            assert_eq!(req.method(), Method::POST);
            assert_eq!(
                req.uri(),
                &"https://example.com/helloworld.Greeter/SayHello"
            );
            assert_eq!(
                req.headers().get(CONTENT_TYPE).unwrap(),
                &format!("application/{}", codec.name())
            );
            assert_eq!(req.headers().get(CONTENT_ENCODING).unwrap(), "identity");
            assert_eq!(req.into_body(), body);
        }
    }

    #[test]
    fn test_builder_unary_get() {
        for codec in [Codec::Proto, Codec::Json] {
            let request = HelloRequest {
                name: "world".to_string(),
            };
            let body = codec.encode(&request);

            let req = Builder::new()
                .scheme(Scheme::HTTPS)
                .authority("example.com")
                .unwrap()
                .rpc_path("/helloworld.Greeter/SayHello")
                .unwrap()
                .message_codec(codec)
                .unary_get(body.clone())
                .unwrap();
            assert_eq!(req.method(), Method::GET);
            assert_eq!(req.headers().get(CONTENT_TYPE), None);
            assert_eq!(req.headers().get(CONTENT_ENCODING), None);
            assert_eq!(req.uri().scheme_str(), Some("https"));
            assert_eq!(req.uri().authority().unwrap().as_str(), "example.com");
            assert_eq!(req.uri().path(), "/helloworld.Greeter/SayHello");
            let query = form_urlencoded::parse(req.uri().query().unwrap().as_bytes());
            let query_map: std::collections::HashMap<_, _> = query.into_owned().collect();
            assert_eq!(query_map.get("message").unwrap(), &b64::url_encode(&body));
            assert_eq!(query_map.get("base64").unwrap(), "1");
            assert_eq!(query_map.get("connect").unwrap(), "v1");
            assert_eq!(query_map.get("encoding").unwrap(), codec.name());
        }
    }
}

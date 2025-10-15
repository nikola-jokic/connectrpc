pub use http::header::*;

/// Connect specific header specifying the protocol version.
pub const CONNECT_PROTOCOL_VERSION: HeaderName =
    HeaderName::from_static("connect-protocol-version");

/// Currently only version "1" is supported.
pub const CONNECT_PROTOCOL_VERSION_1: HeaderValue = HeaderValue::from_static("1");

/// Connect timeout in milliseconds.
pub const CONNECT_TIMEOUT_MS: HeaderName = HeaderName::from_static("connect-timeout-ms");

/// Connect message content encoding header.
pub const CONNECT_CONTENT_ENCODING: HeaderName =
    HeaderName::from_static("connect-content-encoding");
/// Connect message accept encoding header.
pub const CONNECT_ACCEPT_ENCODING: HeaderName = HeaderName::from_static("connect-accept-encoding");

/// Connect error content type.
pub const CONNECT_ERROR_CONTENT_TYPE: HeaderValue = HeaderValue::from_static("application/json");

/// Check if a string is a valid HTTP token as per RFC 7230.
pub fn is_valid_http_token(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c))
}

pub use http::header::*;

pub const CONNECT_PROTOCOL_VERSION: HeaderName =
    HeaderName::from_static("connect-protocol-version");
pub const CONNECT_PROTOCOL_VERSION_1: HeaderValue = HeaderValue::from_static("1");

pub const CONNECT_TIMEOUT_MS: HeaderName = HeaderName::from_static("connect-timeout-ms");

pub const CONNECT_CONTENT_ENCODING: HeaderName =
    HeaderName::from_static("connect-content-encoding");
pub const CONNECT_ACCEPT_ENCODING: HeaderName = HeaderName::from_static("connect-accept-encoding");
pub const CONTENT_ENCODING_IDENTITY: HeaderValue = HeaderValue::from_static("identity");

pub const CONTENT_TYPE_PREFIX: &str = "application/";
pub const STREAMING_CONTENT_TYPE_PREFIX: &str = "application/connect+";
pub const STREAMING_CONTENT_SUBTYPE_PREFIX: &str = "connect+";

pub const CONTENT_TYPE_APPLICATION_PROTO: &str = "application/proto";
pub const CONTENT_TYPE_APPLICATION_JSON: &str = "application/json";

pub const CONNECT_ERROR_CONTENT_TYPE: HeaderValue = HeaderValue::from_static("application/json");

pub fn is_valid_http_token(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c))
}

use http::HeaderMap;

use crate::b64;
use crate::connect::ConnectCode;
use crate::header::{self, CONNECT_ERROR_CONTENT_TYPE};
use std::fmt;

pub(crate) type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// A general error type for Connect operations.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("base64 decode error: {0}")]
    Base64DecodeError(#[from] base64::DecodeError),
    #[error("body error: {0}")]
    BodyError(#[source] BoxError),
    #[error("{0}")]
    ConnectError(Box<ConnectError>),
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("invalid response: {0}")]
    InvalidResponse(String),
    #[error("invalid metadata: {0}")]
    InvalidMetadata(&'static str),
    #[error("invalid header name: {0}")]
    InvalidHeaderName(#[from] http::header::InvalidHeaderName),
    #[error("invalid header value: {0}")]
    InvalidHeaderValue(#[from] http::header::InvalidHeaderValue),
    #[error("invalid URI: {0}")]
    InvalidUri(#[from] http::uri::InvalidUri),
    #[error("invalid URI: {0}")]
    InvalidUriParts(#[from] http::uri::InvalidUriParts),
    #[error("serde error: {0}")]
    SerdeError(#[from] serde_json::Error),
    #[error("proto serialize error: {0}")]
    ProstEncodeError(#[from] prost::EncodeError),
    #[error("proto deserialize error: {0}")]
    ProstDecodeError(#[from] prost::DecodeError),
    #[error("unsupported message codec {0:?}")]
    UnsupportedMessageCodec(String),
    #[cfg(feature = "reqwest")]
    #[error("reqwest error: {0}")]
    ReqwestError(#[source] ::reqwest::Error),
}

impl Error {
    /// Create an invalid request error with the given message.
    pub fn invalid_request(msg: impl fmt::Display) -> Self {
        Self::InvalidRequest(msg.to_string())
    }

    /// Create a not found error with the given message.
    pub fn internal(message: impl fmt::Display) -> Error {
        Error::ConnectError(Box::new(ConnectError::new(ConnectCode::Internal, message)))
    }

    /// Create an unsupported media type error with the given message.
    /// This is typically used when the message codec is not supported by the server.
    pub fn unsupported_media_type(msg: impl fmt::Display) -> Error {
        Error::UnsupportedMessageCodec(msg.to_string())
    }
}

/// A Connect error.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ConnectError {
    #[serde(default, deserialize_with = "deserialize_error_code")]
    code: Option<ConnectCode>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<ConnectErrorDetail>,
    #[serde(skip)]
    headers: HeaderMap,
}

impl ConnectError {
    pub fn new(code: ConnectCode, message: impl std::fmt::Display) -> Self {
        Self {
            code: Some(code),
            message: message.to_string(),
            details: Default::default(),
            headers: Default::default(),
        }
    }

    pub fn code(&self) -> ConnectCode {
        self.code.unwrap_or(ConnectCode::Unknown)
    }

    pub fn http_code(&self) -> http::StatusCode {
        self.code.unwrap_or(ConnectCode::Unknown).into()
    }

    pub fn metadata(&self) -> &HeaderMap {
        &self.headers
    }
}

pub fn not_found(message: impl fmt::Display) -> Error {
    Error::ConnectError(Box::new(ConnectError::new(ConnectCode::NotFound, message)))
}

pub fn internal(message: impl fmt::Display) -> Error {
    Error::ConnectError(Box::new(ConnectError::new(ConnectCode::Internal, message)))
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(serde_json::to_value(self.code()).unwrap().as_str().unwrap())?;
        if !self.message.is_empty() {
            write!(f, ": {}", self.message)?;
        }
        Ok(())
    }
}

impl<T: AsRef<[u8]>> From<http::Response<T>> for ConnectError {
    fn from(resp: http::Response<T>) -> Self {
        let (parts, body) = resp.into_parts();
        let error = if parts.headers.get(header::CONTENT_TYPE) == Some(&CONNECT_ERROR_CONTENT_TYPE)
        {
            match serde_json::from_slice::<ConnectError>(body.as_ref()) {
                Ok(mut error) => {
                    error.code.get_or_insert_with(|| parts.status.into());
                    Some(error)
                }
                Err(err) => {
                    tracing::debug!(?err, "Failed to decode error JSON");
                    None
                }
            }
        } else {
            None
        };
        let mut error = error.unwrap_or_else(|| Self::new(parts.status.into(), "request invalid"));
        error.headers = parts.headers;
        error
    }
}

impl From<Error> for ConnectError {
    fn from(err: Error) -> Self {
        let code = match err {
            Error::ConnectError(connect_error) => return *connect_error,
            Error::InvalidResponse(_) => ConnectCode::Internal,
            Error::UnsupportedMessageCodec(_) => ConnectCode::UnsupportedMediaType,
            _ => ConnectCode::Unknown,
        };
        let message = match &err {
            Error::UnsupportedMessageCodec(msg) => msg,
            _ => &String::new(),
        };
        Self::new(code, message)
    }
}

impl From<Error> for http::Response<Vec<u8>> {
    fn from(err: Error) -> Self {
        let error: ConnectError = err.into();
        let body = serde_json::to_vec(&error).unwrap_or_default();
        let mut builder = http::Response::builder()
            .status(error.http_code())
            .header(header::CONTENT_TYPE, CONNECT_ERROR_CONTENT_TYPE);

        for (name, value) in error.metadata().iter() {
            builder = builder.header(name, value);
        }
        builder.body(body).unwrap()
    }
}

#[cfg(feature = "reqwest")]
impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            Self::ConnectError(Box::new(ConnectError::new(
                ConnectCode::DeadlineExceeded,
                err.to_string(),
            )))
        } else if err.is_request() {
            Self::InvalidRequest(err.to_string())
        } else {
            Self::ReqwestError(err)
        }
    }
}

fn deserialize_error_code<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<ConnectCode>, D::Error> {
    use serde::Deserialize;
    Option::<ConnectCode>::deserialize(deserializer).or(Ok(None))
}

/// Connect error detail.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ConnectErrorDetail {
    #[serde(rename = "type")]
    pub proto_type: String,
    #[serde(rename = "value")]
    pub value_base64: String,
}

impl ConnectErrorDetail {
    pub fn type_url(&self) -> String {
        format!("type.googleapis.com/{}", self.proto_type)
    }

    pub fn value(&self) -> Result<Vec<u8>, Error> {
        Ok(b64::decode(&self.value_base64)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unsupported_message_codec_error() {
        let err = Error::UnsupportedMessageCodec("unsupported codec".to_string());
        let connect_err: ConnectError = err.into();
        assert_eq!(connect_err.code(), ConnectCode::UnsupportedMediaType);
        assert_eq!(
            connect_err.http_code(),
            http::StatusCode::UNSUPPORTED_MEDIA_TYPE
        );
        assert_eq!(connect_err.message, "unsupported codec");
    }

    #[test]
    fn test_unsupported_message_codec_response() {
        let err = Error::UnsupportedMessageCodec("unsupported codec".to_string());
        let response: http::Response<Vec<u8>> = err.into();
        assert_eq!(response.status(), http::StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .unwrap()
                .to_str()
                .unwrap(),
            CONNECT_ERROR_CONTENT_TYPE.to_str().unwrap(),
        );
        let body = String::from_utf8(response.body().clone()).unwrap();
        let expected_body = r#"{"code":"unsupported_media_type","message":"unsupported codec"}"#;
        assert_eq!(body, expected_body);
    }
}

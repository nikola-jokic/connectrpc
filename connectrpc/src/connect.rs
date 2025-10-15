use serde::{Deserialize, Serialize};

/// Codec represents the encoding format for messages.
/// In theory, it could be only Serialize if json is used,
/// and only prost::Message if proto is used. But having both
/// makes it easier to switch between the two which is especially
/// useful for server implementation.
pub trait EncodeMessage: prost::Message + Serialize {}

impl<T> EncodeMessage for T where T: prost::Message + Serialize {}

/// DecodeMessage represents the decoding format for messages.
/// In theory, it could be only Deserialize if json is used,
/// and only prost::Message if proto is used. But having both
/// makes it easier to switch between the two which is especially
/// useful for server implementation.
pub trait DecodeMessage: prost::Message + for<'de> Deserialize<'de> + Default {}

impl<T> DecodeMessage for T where T: prost::Message + for<'de> Deserialize<'de> + Default {}

/// ConnectCode represents categories of errors as codes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectCode {
    /// The operation completed successfully.
    Ok,
    /// The operation was cancelled.
    Canceled,
    /// Unknown error.
    Unknown,
    /// Client specified an invalid argument.
    InvalidArgument,
    /// Deadline expired before operation could complete.
    DeadlineExceeded,
    /// Some requested entity was not found.
    NotFound,
    /// The entity is not in the expected format.
    UnsupportedMediaType,
    /// Some entity that we attempted to create already exists.
    AlreadyExists,
    /// The caller does not have permission to execute the specified operation.
    PermissionDenied,
    /// Some resource has been exhausted.
    ResourceExhausted,
    /// The system is not in a state required for the operation's execution.
    FailedPrecondition,
    /// The operation was aborted.
    Aborted,
    /// Operation was attempted past the valid range.
    OutOfRange,
    /// Operation is not implemented or not supported.
    Unimplemented,
    /// Internal error.
    Internal,
    /// The service is currently unavailable.
    Unavailable,
    /// Unrecoverable data loss or corruption.
    DataLoss,
    /// The request does not have valid authentication credentials
    Unauthenticated,
}

/// Convert an HTTP status code to a ConnectCode.
/// https://connectrpc.com/docs/protocol/#http-to-error-code
impl From<http::StatusCode> for ConnectCode {
    fn from(code: http::StatusCode) -> Self {
        use http::StatusCode;
        match code {
            StatusCode::BAD_REQUEST => Self::Internal,
            StatusCode::UNAUTHORIZED => Self::Unauthenticated,
            StatusCode::FORBIDDEN => Self::PermissionDenied,
            StatusCode::NOT_FOUND => Self::Unimplemented,
            StatusCode::UNSUPPORTED_MEDIA_TYPE => Self::UnsupportedMediaType,
            StatusCode::NOT_IMPLEMENTED => Self::Unimplemented,
            StatusCode::TOO_MANY_REQUESTS
            | StatusCode::BAD_GATEWAY
            | StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::GATEWAY_TIMEOUT => Self::Unavailable,
            _ => Self::Unknown,
        }
    }
}

impl From<ConnectCode> for http::StatusCode {
    fn from(code: ConnectCode) -> Self {
        use ConnectCode::*;
        match code {
            Ok => http::StatusCode::OK,
            Canceled => http::StatusCode::REQUEST_TIMEOUT,
            Unknown => http::StatusCode::INTERNAL_SERVER_ERROR,
            InvalidArgument => http::StatusCode::BAD_REQUEST,
            DeadlineExceeded => http::StatusCode::GATEWAY_TIMEOUT,
            NotFound => http::StatusCode::NOT_FOUND,
            UnsupportedMediaType => http::StatusCode::UNSUPPORTED_MEDIA_TYPE,
            AlreadyExists => http::StatusCode::CONFLICT,
            PermissionDenied => http::StatusCode::FORBIDDEN,
            ResourceExhausted => http::StatusCode::TOO_MANY_REQUESTS,
            FailedPrecondition => http::StatusCode::PRECONDITION_FAILED,
            Aborted => http::StatusCode::CONFLICT,
            OutOfRange => http::StatusCode::BAD_REQUEST,
            Unimplemented => http::StatusCode::NOT_IMPLEMENTED,
            Internal => http::StatusCode::INTERNAL_SERVER_ERROR,
            Unavailable => http::StatusCode::SERVICE_UNAVAILABLE,
            DataLoss => http::StatusCode::INTERNAL_SERVER_ERROR,
            Unauthenticated => http::StatusCode::UNAUTHORIZED,
        }
    }
}

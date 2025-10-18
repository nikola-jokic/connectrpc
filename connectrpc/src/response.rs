use crate::header::HeaderMap;
use futures_util::StreamExt;
use crate::stream::ConnectFrame;
use crate::{Codec, Result};
use std::pin::Pin;
use crate::connect::DecodeMessage;
use futures_util::Stream;

/// The parts of a unary response.
/// This is useful for constructing a `UnaryResponse` from parts,
/// or for deconstructing a `UnaryResponse` into parts.
#[derive(Debug)]
pub struct Parts<T>
where
    T: Send + Sync,
{
    pub status: http::StatusCode,
    pub metadata: HeaderMap,
    pub message: T,
}

/// A unary response, containing the response status, metadata, and message.
#[derive(Debug, Clone)]
pub struct UnaryResponse<T>
where
    T: Send + Sync,
{
    status: http::StatusCode,
    metadata: HeaderMap,
    message: T,
}

impl<T> From<http::Response<T>> for UnaryResponse<T>
where
    T: Send + Sync,
{
    /// Converts an `http::Response` into a `UnaryResponse`.
    fn from(resp: http::Response<T>) -> Self {
        let (parts, body) = resp.into_parts();
        Self {
            status: parts.status,
            metadata: parts.headers,
            message: body,
        }
    }
}

impl<T> UnaryResponse<T>
where
    T: Send + Sync,
{
    /// Creates a new `UnaryResponse` with the given message and default status and metadata.
    pub fn new(body: T) -> Self {
        Self {
            status: http::StatusCode::OK,
            metadata: HeaderMap::new(),
            message: body,
        }
    }

    /// Returns the http status code of the response.
    pub fn status(&self) -> http::StatusCode {
        self.status
    }

    /// Returns the metadata of the response.
    pub fn metadata(&self) -> &HeaderMap {
        &self.metadata
    }

    /// Returns a reference to the message of the response.
    pub fn message(&self) -> &T {
        &self.message
    }

    /// Consumes the response and returns the message.
    pub fn into_message(self) -> T {
        self.message
    }

    /// Creates a `UnaryResponse` from its parts.
    pub fn from_parts(parts: Parts<T>) -> Self {
        Self {
            status: parts.status,
            metadata: parts.metadata,
            message: parts.message,
        }
    }

    /// Decomposes the `UnaryResponse` into its parts.
    pub fn into_parts(self) -> Parts<T> {
        Parts {
            status: self.status,
            metadata: self.metadata,
            message: self.message,
        }
    }
}

pub struct ServerStreamingResponse<T>
where
    T: Send + Sync,
{
    pub status: http::StatusCode,
    pub metadata: HeaderMap,
    pub codec: Codec,
    pub message_stream: Pin<Box<dyn Stream<Item = Result<ConnectFrame>> + Send + Sync>>,
    pub _marker: std::marker::PhantomData<T>,
}

impl<T> ServerStreamingResponse<T>
where
    T: Send + Sync,
{
    /// Returns the http status code of the response.
    pub fn status(&self) -> http::StatusCode {
        self.status
    }

    /// Returns the metadata of the response.
    pub fn metadata(&self) -> &HeaderMap {
        &self.metadata
    }
}

impl<T> ServerStreamingResponse<T>
where
    T: DecodeMessage + Send + 'static,
{
    /// Consumes the response and returns a stream of decoded messages.
    /// Each item in the stream is a `Result<T>` where `T` is the decoded message type.
    /// The stream automatically deserializes ConnectFrames using the configured codec.
    pub fn into_message_stream(self) -> impl Stream<Item = Result<T>> + Send {
        futures_util::stream::unfold(
            (self.message_stream, self.codec),
            |(mut frame_stream, codec)| async move {
                loop {
                    match frame_stream.next().await {
                        Some(Ok(frame)) => {
                            // Skip empty frames
                            if !frame.data.is_empty() {
                                // Decode the frame data into a message
                                let result = codec.decode::<T>(&frame.data);
                                return Some((result, (frame_stream, codec)));
                            }
                            // Continue to next frame if this one is empty
                        }
                        Some(Err(e)) => {
                            return Some((Err(e), (frame_stream, codec)));
                        }
                        None => {
                            return None;
                        }
                    }
                }
            },
        )
    }
}

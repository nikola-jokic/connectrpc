use crate::connect::DecodeMessage;
use crate::header::HeaderMap;
use crate::stream::ConnectFrame;
use crate::{Codec, Result};
use core::fmt;
use futures_util::Stream;
use futures_util::StreamExt;
use std::pin::Pin;

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

impl<T> fmt::Debug for ServerStreamingResponse<T>
where
    T: Send + Sync + fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerStreamingResponse")
            .field("status", &self.status)
            .field("metadata", &self.metadata)
            .field("codec", &self.codec)
            .finish()
    }
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
    T: DecodeMessage + Send,
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

pub struct BidiStreamingResponse<T>
where
    T: Send + Sync,
{
    pub status: http::StatusCode,
    pub metadata: HeaderMap,
    pub codec: Codec,
    pub message_stream: Pin<Box<dyn Stream<Item = Result<ConnectFrame>> + Send + Sync>>,
    pub _marker: std::marker::PhantomData<T>,
}

impl<T> BidiStreamingResponse<T>
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

impl<T> BidiStreamingResponse<T>
where
    T: DecodeMessage + Send,
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

#[derive(Debug)]
pub struct ClientStreamingResponse<T>
where
    T: Send + Sync,
{
    pub status: http::StatusCode,
    pub metadata: HeaderMap,
    pub message: T,
}

impl<T> ClientStreamingResponse<T>
where
    T: Send + Sync,
{
    /// Creates a new `ClientStreamingResponse` with the given message, status, and metadata.
    pub fn new(message: T) -> Self {
        Self {
            status: http::StatusCode::OK,
            metadata: HeaderMap::new(),
            message,
        }
    }
}

impl<T> ClientStreamingResponse<T>
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

    pub fn into_message(self) -> T {
        self.message
    }

    pub fn into_parts(self) -> Parts<T> {
        Parts {
            status: self.status,
            metadata: self.metadata,
            message: self.message,
        }
    }

    /// Returns a reference to the message of the response.
    pub fn message(&self) -> &T {
        &self.message
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use futures_util::stream::StreamExt;
    use prost::Message;
    use serde::{Deserialize, Serialize};

    // Test message type
    #[derive(Message, Serialize, Deserialize, Clone, PartialEq)]
    struct TestMessage {
        #[prost(string, tag = "1")]
        content: String,
        #[prost(int32, tag = "2")]
        value: i32,
    }

    #[test]
    fn test_unary_response_creation() {
        let msg = TestMessage {
            content: "test".to_string(),
            value: 42,
        };

        let response = UnaryResponse::new(msg.clone());
        assert_eq!(response.status(), http::StatusCode::OK);
        assert_eq!(response.message(), &msg);
    }

    #[test]
    fn test_unary_response_parts() {
        let msg = TestMessage {
            content: "test".to_string(),
            value: 42,
        };

        let response = UnaryResponse::new(msg.clone());
        let parts = response.into_parts();

        assert_eq!(parts.status, http::StatusCode::OK);
        assert_eq!(parts.message, msg);

        let response = UnaryResponse::from_parts(parts);
        assert_eq!(response.message(), &msg);
    }

    #[test]
    fn test_unary_response_into_message() {
        let msg = TestMessage {
            content: "test".to_string(),
            value: 42,
        };

        let response = UnaryResponse::new(msg.clone());
        let extracted = response.into_message();
        assert_eq!(extracted, msg);
    }

    // Tests for ServerStreamingResponse.into_message_stream()

    fn create_frame(data: Bytes, end: bool) -> ConnectFrame {
        ConnectFrame {
            compressed: false,
            end,
            data,
        }
    }

    fn encode_message(msg: &TestMessage, codec: Codec) -> Bytes {
        Bytes::from(codec.encode(msg))
    }

    #[tokio::test]
    async fn test_into_message_stream_single_message_proto() {
        let msg = TestMessage {
            content: "hello".to_string(),
            value: 42,
        };

        let encoded = encode_message(&msg, Codec::Proto);
        let frame = create_frame(encoded, true);

        let frame_stream = Box::new(futures_util::stream::iter(vec![Ok(frame)]));
        let response: ServerStreamingResponse<TestMessage> = ServerStreamingResponse {
            status: http::StatusCode::OK,
            metadata: HeaderMap::new(),
            codec: Codec::Proto,
            message_stream: Box::pin(frame_stream),
            _marker: std::marker::PhantomData,
        };

        let mut msg_stream = std::pin::pin!(response.into_message_stream());

        // First message should decode successfully
        let result = msg_stream.next().await.unwrap();
        assert!(result.is_ok(), "Failed to decode message");
        assert_eq!(result.unwrap(), msg);

        // Stream should end
        assert!(msg_stream.next().await.is_none());
    }

    #[tokio::test]
    async fn test_into_message_stream_multiple_messages_proto() {
        let messages = vec![
            TestMessage {
                content: "first".to_string(),
                value: 1,
            },
            TestMessage {
                content: "second".to_string(),
                value: 2,
            },
            TestMessage {
                content: "third".to_string(),
                value: 3,
            },
        ];

        let frames = messages
            .iter()
            .enumerate()
            .map(|(i, msg)| {
                let is_last = i == messages.len() - 1;
                Ok(create_frame(encode_message(msg, Codec::Proto), is_last))
            })
            .collect::<Vec<_>>();

        let frame_stream = Box::new(futures_util::stream::iter(frames));
        let response: ServerStreamingResponse<TestMessage> = ServerStreamingResponse {
            status: http::StatusCode::OK,
            metadata: HeaderMap::new(),
            codec: Codec::Proto,
            message_stream: Box::pin(frame_stream),
            _marker: std::marker::PhantomData,
        };

        let msg_stream = std::pin::pin!(response.into_message_stream());

        // Collect all messages
        let decoded: Vec<_> = msg_stream
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .map(|r| r.expect("Failed to decode message"))
            .collect();

        assert_eq!(decoded.len(), 3);
        assert_eq!(decoded[0], messages[0]);
        assert_eq!(decoded[1], messages[1]);
        assert_eq!(decoded[2], messages[2]);
    }

    #[tokio::test]
    async fn test_into_message_stream_skips_empty_frames() {
        let msg1 = TestMessage {
            content: "first".to_string(),
            value: 1,
        };
        let msg2 = TestMessage {
            content: "second".to_string(),
            value: 2,
        };

        let frames = vec![
            Ok(create_frame(encode_message(&msg1, Codec::Proto), false)),
            Ok(create_frame(Bytes::new(), false)), // Empty frame
            Ok(create_frame(Bytes::new(), false)), // Another empty frame
            Ok(create_frame(encode_message(&msg2, Codec::Proto), true)),
            Ok(create_frame(Bytes::new(), true)), // Empty frame at end
        ];

        let frame_stream = Box::new(futures_util::stream::iter(frames));
        let response: ServerStreamingResponse<TestMessage> = ServerStreamingResponse {
            status: http::StatusCode::OK,
            metadata: HeaderMap::new(),
            codec: Codec::Proto,
            message_stream: Box::pin(frame_stream),
            _marker: std::marker::PhantomData,
        };

        let mut msg_stream = std::pin::pin!(response.into_message_stream());

        // Should receive only 2 messages, empty frames skipped
        let msg1_decoded = msg_stream.next().await.unwrap().unwrap();
        assert_eq!(msg1_decoded, msg1);

        let msg2_decoded = msg_stream.next().await.unwrap().unwrap();
        assert_eq!(msg2_decoded, msg2);

        assert!(msg_stream.next().await.is_none());
    }

    #[tokio::test]
    async fn test_into_message_stream_with_json_codec() {
        let messages = vec![
            TestMessage {
                content: "json1".to_string(),
                value: 100,
            },
            TestMessage {
                content: "json2".to_string(),
                value: 200,
            },
        ];

        let frames = messages
            .iter()
            .enumerate()
            .map(|(i, msg)| {
                let is_last = i == messages.len() - 1;
                Ok(create_frame(encode_message(msg, Codec::Json), is_last))
            })
            .collect::<Vec<_>>();

        let frame_stream = Box::new(futures_util::stream::iter(frames));
        let response: ServerStreamingResponse<TestMessage> = ServerStreamingResponse {
            status: http::StatusCode::OK,
            metadata: HeaderMap::new(),
            codec: Codec::Json,
            message_stream: Box::pin(frame_stream),
            _marker: std::marker::PhantomData,
        };

        let msg_stream = std::pin::pin!(response.into_message_stream());

        let decoded: Vec<_> = msg_stream
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .map(|r| r.unwrap())
            .collect();

        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0], messages[0]);
        assert_eq!(decoded[1], messages[1]);
    }

    #[tokio::test]
    async fn test_into_message_stream_empty_stream() {
        let frame_stream = Box::new(futures_util::stream::iter(
            Vec::<Result<ConnectFrame>>::new(),
        ));
        let response: ServerStreamingResponse<TestMessage> = ServerStreamingResponse {
            status: http::StatusCode::OK,
            metadata: HeaderMap::new(),
            codec: Codec::Proto,
            message_stream: Box::pin(frame_stream),
            _marker: std::marker::PhantomData,
        };

        let mut msg_stream = std::pin::pin!(response.into_message_stream());
        assert!(msg_stream.next().await.is_none());
    }

    #[tokio::test]
    async fn test_into_message_stream_decode_error_handling() {
        let msg = TestMessage {
            content: "valid".to_string(),
            value: 42,
        };

        let frames = vec![
            Ok(create_frame(encode_message(&msg, Codec::Proto), false)),
            Ok(create_frame(Bytes::from(vec![0xFF, 0xFF, 0xFF]), false)), // Invalid frame
            Ok(create_frame(encode_message(&msg, Codec::Proto), true)),
        ];

        let frame_stream = Box::new(futures_util::stream::iter(frames));
        let response: ServerStreamingResponse<TestMessage> = ServerStreamingResponse {
            status: http::StatusCode::OK,
            metadata: HeaderMap::new(),
            codec: Codec::Proto,
            message_stream: Box::pin(frame_stream),
            _marker: std::marker::PhantomData,
        };

        let mut msg_stream = std::pin::pin!(response.into_message_stream());

        // First message should decode successfully
        let first = msg_stream.next().await.unwrap();
        assert!(first.is_ok());
        assert_eq!(first.unwrap(), msg);

        // Second message should be a decode error
        let second = msg_stream.next().await.unwrap();
        assert!(second.is_err(), "Expected decode error but got Ok");

        // Third message should decode successfully
        let third = msg_stream.next().await.unwrap();
        assert!(third.is_ok());
        assert_eq!(third.unwrap(), msg);
    }

    #[tokio::test]
    async fn test_into_message_stream_with_stream_combinators() {
        let messages = vec![
            TestMessage {
                content: "1".to_string(),
                value: 1,
            },
            TestMessage {
                content: "2".to_string(),
                value: 2,
            },
            TestMessage {
                content: "3".to_string(),
                value: 3,
            },
            TestMessage {
                content: "4".to_string(),
                value: 4,
            },
        ];

        let frames = messages
            .iter()
            .enumerate()
            .map(|(i, msg)| {
                let is_last = i == messages.len() - 1;
                Ok(create_frame(encode_message(msg, Codec::Proto), is_last))
            })
            .collect::<Vec<_>>();

        let frame_stream = Box::new(futures_util::stream::iter(frames));
        let response: ServerStreamingResponse<TestMessage> = ServerStreamingResponse {
            status: http::StatusCode::OK,
            metadata: HeaderMap::new(),
            codec: Codec::Proto,
            message_stream: Box::pin(frame_stream),
            _marker: std::marker::PhantomData,
        };

        // Test .take() combinator
        let msg_stream = response.into_message_stream();
        let msg_stream = std::pin::pin!(msg_stream.take(2));

        let taken: Vec<_> = msg_stream
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .map(|r| r.unwrap())
            .collect();

        assert_eq!(taken.len(), 2);
        assert_eq!(taken[0].value, 1);
        assert_eq!(taken[1].value, 2);
    }

    #[tokio::test]
    async fn test_into_message_stream_large_batch() {
        // Test with many messages to verify streaming works correctly
        let message_count = 100;
        let messages: Vec<_> = (0..message_count)
            .map(|i| TestMessage {
                content: format!("msg_{}", i),
                value: i as i32,
            })
            .collect();

        let frames = messages
            .iter()
            .enumerate()
            .map(|(i, msg)| {
                let is_last = i == messages.len() - 1;
                Ok(create_frame(encode_message(msg, Codec::Proto), is_last))
            })
            .collect::<Vec<_>>();

        let frame_stream = Box::new(futures_util::stream::iter(frames));
        let response: ServerStreamingResponse<TestMessage> = ServerStreamingResponse {
            status: http::StatusCode::OK,
            metadata: HeaderMap::new(),
            codec: Codec::Proto,
            message_stream: Box::pin(frame_stream),
            _marker: std::marker::PhantomData,
        };

        let mut msg_stream = std::pin::pin!(response.into_message_stream());

        // Verify each message is decoded correctly in order
        for (i, expected) in messages.iter().enumerate() {
            let received = msg_stream
                .next()
                .await
                .expect("Stream ended early")
                .expect("Failed to decode message");
            assert_eq!(received, *expected, "Message {} mismatch", i);
        }

        // Stream should now be exhausted
        assert!(msg_stream.next().await.is_none());
    }
}

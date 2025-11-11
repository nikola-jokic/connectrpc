use crate::connect::DecodeMessage;
use crate::error::{BoxError, Error};
use crate::{Codec, Result};
use bytes::{Buf, BufMut, Bytes, BytesMut};
use futures_util::{Stream, StreamExt, TryStream, TryStreamExt, stream};
use http_body::Body;
use http_body_util::BodyExt;
use std::pin::Pin;
use std::task::Poll;

/// Type alias for server streaming frame encoder.
/// This encapsulates the internal implementation and allows for future changes.
pub type ServerStreamingEncoder = StreamingFrameEncoder<
    UnpinStream<futures_util::stream::Iter<std::iter::Once<Result<Vec<u8>>>>>,
>;

/// Type alias for a boxed stream of bytes that yields Results.
/// Used for dynamic dispatch of streaming frame encoders.
pub type BoxedByteStream = Pin<Box<dyn Stream<Item = Result<Bytes>> + Send + Sync>>;

#[derive(Debug, Clone)]
pub struct ConnectFrame {
    pub compressed: bool,
    pub end: bool,
    pub data: Bytes,
}

pub const FLAGS_COMPRESSED: u8 = 0b1; // bit 0
pub const FLAGS_END: u8 = 0b10; // bit 1

impl ConnectFrame {
    /// Create a new message frame (not compressed, not end-of-stream).
    pub fn message(data: Bytes) -> Self {
        Self {
            compressed: false,
            end: false,
            data,
        }
    }

    /// Create an end-of-stream frame.
    pub fn end_of_stream() -> Self {
        Self {
            compressed: false,
            end: true,
            data: Bytes::new(),
        }
    }

    /// Encode this frame to bytes for transmission.
    pub fn encode_to_bytes(&self) -> Bytes {
        let mut encoded = BytesMut::with_capacity(5 + self.data.len());

        // Flags: bit 0 = compressed, bit 1 = end-of-stream
        let flags = if self.compressed { FLAGS_COMPRESSED } else { 0 }
            | if self.end { FLAGS_END } else { 0 };
        encoded.put_u8(flags);

        // Length (big-endian)
        encoded.put_u32(self.data.len() as u32);

        // Data
        encoded.extend_from_slice(&self.data);

        encoded.freeze()
    }

    pub fn body_stream<B>(body: B) -> impl Stream<Item = Result<Self>>
    where
        B: Body<Error: Into<BoxError>>,
    {
        Self::bytes_stream(body.into_data_stream())
    }

    pub fn bytes_stream<S>(s: S) -> impl Stream<Item = Result<Self>>
    where
        S: TryStream<Ok: Buf, Error: Into<BoxError>>,
    {
        let mut parse_state = FrameParseState::default();
        s.map_err(Error::body)
            .map(Some)
            .chain(stream::iter([None]))
            .flat_map(move |item| stream::iter(parse_state.feed(item)))
    }
}

#[derive(Default)]
struct FrameParseState {
    buf: BytesMut,
    failed: bool,
}

impl FrameParseState {
    fn feed(&mut self, item: Option<Result<impl Buf>>) -> Vec<Result<ConnectFrame>> {
        if self.failed {
            return vec![];
        }

        let data = match item {
            Some(Ok(data)) => data,
            Some(Err(err)) => {
                self.failed = true;
                return vec![Err(err)];
            }
            None => {
                if !self.buf.is_empty() {
                    return vec![Err(Error::body("partial frame at end of stream"))];
                }
                return vec![];
            }
        };

        self.buf.put(data);

        let mut frames = Vec::new();

        loop {
            match self.parse_frame() {
                Ok(Some(frame)) => frames.push(Ok(frame)),
                Ok(None) => return frames,
                Err(err) => {
                    self.failed = true;
                    frames.push(Err(err));
                }
            }
        }
    }

    fn parse_frame(&mut self) -> Result<Option<ConnectFrame>> {
        if self.buf.len() < 5 {
            return Ok(None);
        }
        let data_len = (&self.buf[1..]).get_u32();
        let Ok(frame_len) = ((data_len as u64) + 5).try_into() else {
            return Err(Error::body("frame too large"));
        };
        if self.buf.len() < frame_len {
            return Ok(None);
        }
        let mut frame = self.buf.split_to(frame_len);
        let data = frame.split_off(5).freeze();
        let flags = frame[0];
        Ok(Some(ConnectFrame {
            compressed: flags & FLAGS_COMPRESSED != 0,
            end: flags & FLAGS_END != 0,
            data,
        }))
    }
}

/// Encodes a stream of byte messages into Connect protocol frames.
///
/// This is used for client streaming requests where messages are encoded
/// and wrapped in the Connect frame format for transmission to the server.
///
/// Frame format:
/// - Byte 0: Flags (bit 0 = compressed, bit 1 = end-of-stream)
/// - Bytes 1-4: Message length (u32 big-endian)
/// - Bytes 5+: Message data
pub struct FrameEncoder;

impl FrameEncoder {
    /// Encode a single message into a ConnectFrame.
    ///
    /// Returns the frame bytes that can be sent over HTTP.
    pub fn encode_message(message_data: Vec<u8>) -> Result<Bytes> {
        let message_len = message_data.len() as u32;

        // Frame format: [flags(1) | length(4) | data]
        let mut frame = BytesMut::with_capacity(5 + message_data.len());

        // Flags: not compressed, not end-of-stream
        frame.put_u8(0);

        // Message length (big-endian)
        frame.put_u32(message_len);

        // Message data
        frame.extend_from_slice(&message_data);

        Ok(frame.freeze())
    }

    /// Encode the final frame (end-of-stream marker).
    pub fn encode_end_frame() -> Bytes {
        let mut frame = BytesMut::with_capacity(5);

        // Flags: not compressed, end-of-stream
        frame.put_u8(FLAGS_END);

        // Length: 0 (no message data)
        frame.put_u32(0);

        frame.freeze()
    }
}

pub struct StreamingFrameEncoder<S> {
    message_stream: S,
    finished: bool,
}

impl<S> StreamingFrameEncoder<S>
where
    S: Stream<Item = Result<Vec<u8>>> + Send,
{
    /// Create a new frame encoder from a message stream.
    ///
    /// The stream should yield encoded messages (Vec<u8>).
    /// Each message will be wrapped in a Connect frame.
    pub fn new(message_stream: S) -> Self {
        Self {
            message_stream,
            finished: false,
        }
    }
}

impl<S> Stream for StreamingFrameEncoder<S>
where
    S: Stream<Item = Result<Vec<u8>>> + Send + Unpin,
{
    type Item = Result<Bytes>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        // If we've already sent the end frame, we're done
        if self.finished {
            return Poll::Ready(None);
        }

        // Poll the inner message stream
        match Pin::new(&mut self.message_stream).poll_next(cx) {
            Poll::Ready(Some(Ok(message_data))) => {
                // Encode the message into a frame
                match FrameEncoder::encode_message(message_data) {
                    Ok(frame) => Poll::Ready(Some(Ok(frame))),
                    Err(err) => Poll::Ready(Some(Err(err))),
                }
            }
            Poll::Ready(Some(Err(err))) => {
                self.finished = true;
                Poll::Ready(Some(Err(err)))
            }
            Poll::Ready(None) => {
                // Stream ended, send the end-of-stream frame
                self.finished = true;
                Poll::Ready(Some(Ok(FrameEncoder::encode_end_frame())))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

// Wrapper to make non-Unpin streams compatible with FrameEncoder
pub struct UnpinStream<S: Stream>(pub(crate) Pin<Box<S>>);

impl<S: Stream> Unpin for UnpinStream<S> {}

impl<S: Stream> Stream for UnpinStream<S> {
    type Item = S::Item;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        self.0.as_mut().poll_next(cx)
    }
}

/// A utility for decoding Connect frames from a stream of bytes.
///
/// This provides a convenient way to parse frames for testing and validation.
pub struct StreamDecoder;

impl StreamDecoder {
    /// Decode frames from a stream that yields `Result<Bytes>`.
    ///
    /// This is particularly useful for testing frame encoders, as it can
    /// directly consume the output of `StreamingFrameEncoder` and parse
    /// it back into `ConnectFrame` objects.
    pub fn decode_frames<S>(stream: S) -> impl Stream<Item = Result<ConnectFrame>>
    where
        S: Stream<Item = Result<Bytes>>,
    {
        // Convert the Result<Bytes> stream to the format expected by ConnectFrame::bytes_stream
        let byte_stream =
            stream.map(|result: Result<Bytes>| result.map_err(|e| Box::new(e) as BoxError));

        ConnectFrame::bytes_stream(byte_stream)
    }
}

pub struct StreamingFrameDecoder<S, O> {
    message_stream: S,
    finished: bool,
    codec: Codec,
    _marker: std::marker::PhantomData<O>,
}

impl<S, O> Unpin for StreamingFrameDecoder<S, O> where S: Unpin {}

impl<S, O> StreamingFrameDecoder<S, O>
where
    S: Stream<Item = Result<ConnectFrame>> + Send,
    O: DecodeMessage,
{
    /// Create a new frame decoder from a ConnectFrame stream.
    ///
    /// The stream should yield ConnectFrame objects.
    /// Each message will be extracted from the frames.
    pub fn new(message_stream: S, codec: Codec) -> Self {
        Self {
            message_stream,
            finished: false,
            codec,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<S, O> Stream for StreamingFrameDecoder<S, O>
where
    S: Stream<Item = Result<ConnectFrame>> + Send + Unpin,
    O: DecodeMessage,
{
    type Item = Result<O>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let this = self.as_mut().get_mut();

        // If we've already seen the end frame, we're done
        if this.finished {
            return Poll::Ready(None);
        }

        loop {
            match Pin::new(&mut this.message_stream).poll_next(cx) {
                Poll::Ready(Some(Ok(frame))) => {
                    if frame.end {
                        // End-of-stream frame
                        this.finished = true;
                        return Poll::Ready(None);
                    }

                    // Skip empty frames (e.g. keep-alives)
                    if frame.data.is_empty() {
                        continue;
                    }

                    // Decode the message data
                    let decoded = this.codec.decode::<O>(&frame.data);
                    return Poll::Ready(Some(decoded));
                }
                Poll::Ready(Some(Err(err))) => {
                    this.finished = true;
                    return Poll::Ready(Some(Err(err)));
                }
                Poll::Ready(None) => {
                    // Stream ended unexpectedly or cleanly after end frame
                    this.finished = true;
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

/// High-level interface for converting between frame streams and byte streams.
/// This module provides clean abstractions for working with Connect protocol streams
/// without exposing low-level frame details like flags or encoding.
pub mod frame_stream {
    use super::*;

    /// Convert a stream of ConnectFrames to a byte stream suitable for HTTP transmission.
    ///
    /// This handles all frame encoding (flags, length, data format) transparently.
    /// Use this when you have a frame stream and need to send it over HTTP.
    ///
    /// # Example
    /// ```ignore
    /// let frame_stream: Pin<Box<dyn Stream<Item = Result<ConnectFrame>> + Send + Sync>> = ...;
    /// let byte_stream = frame_stream_to_bytes(frame_stream);
    /// ```
    pub fn frame_stream_to_bytes<S>(frame_stream: S) -> impl Stream<Item = Result<Bytes>> + Send
    where
        S: Stream<Item = Result<ConnectFrame>> + Send + 'static,
    {
        frame_stream.map(|frame_result| frame_result.map(|frame| frame.encode_to_bytes()))
    }

    /// Convert a byte stream to a stream of ConnectFrames.
    ///
    /// This handles frame parsing (flags, length, data extraction) transparently.
    /// Use this when you receive bytes from HTTP and need to parse them as frames.
    ///
    /// # Example
    /// ```ignore
    /// let byte_stream: impl Stream<Item = Result<Bytes>> = ...;
    /// let frames = bytes_to_frame_stream(byte_stream);
    /// ```
    pub fn bytes_to_frame_stream<S>(stream: S) -> impl Stream<Item = Result<ConnectFrame>>
    where
        S: TryStream<Ok: Buf, Error: Into<BoxError>> + Send + 'static,
    {
        ConnectFrame::bytes_stream(stream)
    }

    /// Convert a stream of items to a stream of ConnectFrames.
    ///
    /// This automatically encodes each item using the provided codec and wraps it in a frame.
    /// Use this when you have a stream of messages and want to convert them to frames.
    ///
    /// # Example
    /// ```ignore
    /// let messages = stream::iter(vec![msg1, msg2, msg3]);
    /// let frames = items_to_frame_stream(messages, codec);
    /// ```
    pub fn items_to_frame_stream<S, T>(
        stream: S,
        codec: Codec,
    ) -> impl Stream<Item = Result<ConnectFrame>> + Send
    where
        S: Stream<Item = Result<T>> + Send + 'static,
        T: crate::connect::EncodeMessage,
    {
        stream
            .map(move |item_result| {
                item_result.and_then(|item| {
                    let encoded = codec.encode(&item);
                    Ok(ConnectFrame::message(encoded.into()))
                })
            })
            .chain(stream::once(async {
                Ok(ConnectFrame::end_of_stream())
            }))
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;

    #[tokio::test]
    async fn test_frame_encoder_single_message() {
        use futures_util::stream;

        let messages = vec![vec![1, 2, 3, 4, 5]];
        let message_stream = stream::iter(messages.into_iter().map(Ok));
        let mut encoder = StreamingFrameEncoder::new(message_stream);

        // First frame: the message
        let frame1 = encoder.next().await.unwrap().unwrap();
        assert_eq!(frame1[0], 0); // flags: not compressed, not end
        assert_eq!(
            u32::from_be_bytes([frame1[1], frame1[2], frame1[3], frame1[4]]),
            5
        ); // length
        assert_eq!(&frame1[5..], &[1, 2, 3, 4, 5]); // data

        // Second frame: end-of-stream
        let frame2 = encoder.next().await.unwrap().unwrap();
        assert_eq!(frame2[0], FLAGS_END); // flags: end
        assert_eq!(
            u32::from_be_bytes([frame2[1], frame2[2], frame2[3], frame2[4]]),
            0
        ); // length

        // Stream should end
        assert!(encoder.next().await.is_none());
    }

    #[tokio::test]
    async fn test_frame_encoder_multiple_messages() {
        use futures_util::stream;

        let messages = vec![vec![1, 2, 3], vec![4, 5], vec![6, 7, 8, 9]];
        let message_stream = stream::iter(messages.into_iter().map(Ok));
        let mut encoder = StreamingFrameEncoder::new(message_stream);

        // Frame 1: first message (3 bytes)
        let frame1 = encoder.next().await.unwrap().unwrap();
        assert_eq!(frame1[0], 0);
        assert_eq!(
            u32::from_be_bytes([frame1[1], frame1[2], frame1[3], frame1[4]]),
            3
        );
        assert_eq!(&frame1[5..], &[1, 2, 3]);

        // Frame 2: second message (2 bytes)
        let frame2 = encoder.next().await.unwrap().unwrap();
        assert_eq!(frame2[0], 0);
        assert_eq!(
            u32::from_be_bytes([frame2[1], frame2[2], frame2[3], frame2[4]]),
            2
        );
        assert_eq!(&frame2[5..], &[4, 5]);

        // Frame 3: third message (4 bytes)
        let frame3 = encoder.next().await.unwrap().unwrap();
        assert_eq!(frame3[0], 0);
        assert_eq!(
            u32::from_be_bytes([frame3[1], frame3[2], frame3[3], frame3[4]]),
            4
        );
        assert_eq!(&frame3[5..], &[6, 7, 8, 9]);

        // Frame 4: end-of-stream
        let frame4 = encoder.next().await.unwrap().unwrap();
        assert_eq!(frame4[0], FLAGS_END);

        // Stream should end
        assert!(encoder.next().await.is_none());
    }

    #[tokio::test]
    async fn test_frame_encoder_empty_stream() {
        use futures_util::stream;

        let messages: Vec<Vec<u8>> = vec![];
        let message_stream = stream::iter(messages.into_iter().map(Ok));
        let mut encoder = StreamingFrameEncoder::new(message_stream);

        // Only frame: end-of-stream
        let frame = encoder.next().await.unwrap().unwrap();
        assert_eq!(frame[0], FLAGS_END);
        assert_eq!(
            u32::from_be_bytes([frame[1], frame[2], frame[3], frame[4]]),
            0
        );

        // Stream should end
        assert!(encoder.next().await.is_none());
    }
}

use std::pin::Pin;
use crate::Result;
use crate::error::{BoxError, Error};
use bytes::{Buf, BufMut, Bytes, BytesMut};
use futures_util::{Stream, StreamExt, TryStream, TryStreamExt, stream};
use http_body::Body;
use http_body_util::BodyExt;

#[derive(Debug, Clone)]
pub struct ConnectFrame {
    pub compressed: bool,
    pub end: bool,
    pub data: Bytes,
}

pub const FLAGS_COMPRESSED: u8 = 0b1;
pub const FLAGS_END: u8 = 0b1;

impl ConnectFrame {
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
pub struct FrameEncoder<S> {
    message_stream: S,
    finished: bool,
}

impl<S> FrameEncoder<S>
where
    S: Stream<Item = Vec<u8>> + Send + Sync,
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

    /// Encode a single message into a ConnectFrame.
    ///
    /// Returns the frame bytes that can be sent over HTTP.
    fn encode_message(message_data: Vec<u8>) -> Result<Bytes> {
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
    fn encode_end_frame() -> Bytes {
        let mut frame = BytesMut::with_capacity(5);

        // Flags: not compressed, end-of-stream
        frame.put_u8(FLAGS_END);

        // Length: 0 (no message data)
        frame.put_u32(0);

        frame.freeze()
    }
}

impl<S> Stream for FrameEncoder<S>
where
    S: Stream<Item = Vec<u8>> + Send + Sync + Unpin,
{
    type Item = Result<Bytes>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        use std::task::Poll;

        // If we've already sent the end frame, we're done
        if self.finished {
            return Poll::Ready(None);
        }

        // Poll the inner message stream
        match std::pin::Pin::new(&mut self.message_stream).poll_next(cx) {
            Poll::Ready(Some(message_data)) => {
                // Encode the message into a frame
                match Self::encode_message(message_data) {
                    Ok(frame) => Poll::Ready(Some(Ok(frame))),
                    Err(err) => Poll::Ready(Some(Err(err))),
                }
            }
            Poll::Ready(None) => {
                // Stream ended, send the end-of-stream frame
                self.finished = true;
                Poll::Ready(Some(Ok(Self::encode_end_frame())))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

// Wrapper to make non-Unpin streams compatible with FrameEncoder
pub(crate) struct UnpinStream<S: Stream>(pub(crate) Pin<Box<S>>);

impl<S: Stream> Unpin for UnpinStream<S> {}

impl<S: Stream> Stream for UnpinStream<S> {
    type Item = S::Item;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.0.as_mut().poll_next(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_frame_encoder_single_message() {
        use futures_util::stream;

        let messages = vec![vec![1, 2, 3, 4, 5]];
        let message_stream = stream::iter(messages);
        let mut encoder = FrameEncoder::new(message_stream);

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
        let message_stream = stream::iter(messages);
        let mut encoder = FrameEncoder::new(message_stream);

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
        let message_stream = stream::iter(messages);
        let mut encoder = FrameEncoder::new(message_stream);

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

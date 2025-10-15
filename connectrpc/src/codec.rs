use crate::Result;
use crate::connect::{DecodeMessage, EncodeMessage};

/// Supported codecs for encoding and decoding messages.
/// Currently supports Protobuf and JSON.
///
/// The connect allows for other codecs, but we only implement these two for now.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy)]
pub enum Codec {
    Proto,
    Json,
}

impl Codec {
    /// Returns the name of the codec as a static string.
    /// "proto" for Protobuf and "json" for JSON.
    ///
    /// It is especially important since it is used in the
    /// `Content-Type` header of HTTP requests.
    pub fn name(&self) -> &'static str {
        match self {
            Codec::Proto => "proto",
            Codec::Json => "json",
        }
    }
}

impl Codec {
    /// Encode a message into a byte vector using the specified codec.
    pub fn encode<T>(&self, message: &T) -> Vec<u8>
    where
        T: EncodeMessage,
    {
        match self {
            Codec::Proto => {
                let mut buf = Vec::with_capacity(message.encoded_len());
                message.encode(&mut buf).unwrap();
                buf
            }
            Codec::Json => serde_json::to_vec(message).unwrap(),
        }
    }

    /// Decode a byte slice into a message using the specified codec.
    pub fn decode<T>(&self, data: &[u8]) -> Result<T>
    where
        T: DecodeMessage,
    {
        match self {
            Codec::Proto => {
                let message = T::decode(data)?;
                Ok(message)
            }
            Codec::Json => {
                let message = serde_json::from_slice(data)?;
                Ok(message)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost::Message;
    use serde::{Deserialize, Serialize};

    #[derive(Message, Serialize, Deserialize, PartialEq)]
    struct TestMessage {
        #[prost(string, tag = "1")]
        name: String,
        #[prost(int32, tag = "2")]
        value: i32,
    }

    #[test]
    fn test_proto_codec() {
        let codec = Codec::Proto;
        let message = TestMessage {
            name: "test".to_string(),
            value: 42,
        };

        let encoded = codec.encode(&message);
        let decoded: TestMessage = codec.decode(&encoded).unwrap();

        assert_eq!(message, decoded);
    }

    #[test]
    fn test_json_codec() {
        let codec = Codec::Json;
        let message = TestMessage {
            name: "test".to_string(),
            value: 42,
        };

        let encoded = codec.encode(&message);
        let decoded: TestMessage = codec.decode(&encoded).unwrap();

        assert_eq!(message, decoded);
    }
}

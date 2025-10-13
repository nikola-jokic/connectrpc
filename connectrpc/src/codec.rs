use crate::Result;
use crate::connect::{DecodeMessage, EncodeMessage};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy)]
pub enum Codec {
    Proto,
    Json,
}

impl Codec {
    pub fn name(&self) -> &'static str {
        match self {
            Codec::Proto => "proto",
            Codec::Json => "json",
        }
    }
}

impl Codec {
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

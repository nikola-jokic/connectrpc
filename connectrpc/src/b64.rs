use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::prelude::{BASE64_STANDARD_NO_PAD, Engine};

/// Encode input bytes to a base64 string without padding.
#[inline]
pub fn encode<T: AsRef<[u8]>>(input: T) -> String {
    BASE64_STANDARD_NO_PAD.encode(input)
}

/// Decode a base64 string without padding to bytes.
#[inline]
pub fn decode<T: AsRef<[u8]>>(input: T) -> Result<Vec<u8>, base64::DecodeError> {
    BASE64_STANDARD_NO_PAD.decode(input.as_ref())
}

/// Encode input bytes to a URL-safe base64 string without padding.
#[inline]
pub fn url_encode<T: AsRef<[u8]>>(input: T) -> String {
    URL_SAFE_NO_PAD.encode(input)
}

/// Decode a URL-safe base64 string without padding to bytes.
#[inline]
pub fn url_decode<T: AsRef<[u8]>>(input: T) -> Result<Vec<u8>, base64::DecodeError> {
    URL_SAFE_NO_PAD.decode(input.as_ref())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_encode_decode() {
        let data = b"hello world";
        let encoded = super::encode(data);
        assert_eq!(encoded, "aGVsbG8gd29ybGQ");
        let decoded = super::decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_b64_no_pad() {
        // "test" in base64 is "dGVzdA==". In other words, it
        // has padding.
        let data = b"test";
        let encoded = super::encode(data);
        assert_eq!(encoded, "dGVzdA");
        let decoded = super::decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_url_encode_decode() {
        let data = b"hello world";
        let encoded = super::url_encode(data);
        assert_eq!(encoded, "aGVsbG8gd29ybGQ");
        let decoded = super::url_decode(&encoded).unwrap();
        assert_eq!(decoded, data);

        let data = b"hello+world/";
        let encoded = super::url_encode(data);
        assert_eq!(encoded, "aGVsbG8rd29ybGQv");
        let decoded = super::url_decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }
}

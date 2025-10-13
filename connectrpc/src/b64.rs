use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::prelude::{BASE64_STANDARD_NO_PAD, Engine};

#[inline]
pub fn encode<T: AsRef<[u8]>>(input: T) -> String {
    BASE64_STANDARD_NO_PAD.encode(input)
}

#[inline]
pub fn decode<T: AsRef<[u8]>>(input: T) -> Result<Vec<u8>, base64::DecodeError> {
    BASE64_STANDARD_NO_PAD.decode(input.as_ref())
}

#[inline]
pub fn url_encode<T: AsRef<[u8]>>(input: T) -> String {
    URL_SAFE_NO_PAD.encode(input)
}

#[inline]
pub fn url_decode<T: AsRef<[u8]>>(input: T) -> Result<Vec<u8>, base64::DecodeError> {
    URL_SAFE_NO_PAD.decode(input.as_ref())
}

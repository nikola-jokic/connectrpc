use crate::header::HeaderMap;

#[derive(Debug)]
pub struct Parts<T>
where
    T: Send + Sync,
{
    pub status: http::StatusCode,
    pub metadata: HeaderMap,
    pub message: T,
}

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
    pub fn new(body: T) -> Self {
        Self {
            status: http::StatusCode::OK,
            metadata: HeaderMap::new(),
            message: body,
        }
    }

    pub fn status(&self) -> http::StatusCode {
        self.status
    }

    pub fn metadata(&self) -> &HeaderMap {
        &self.metadata
    }

    pub fn message(&self) -> &T {
        &self.message
    }

    pub fn into_message(self) -> T {
        self.message
    }

    pub fn from_parts(parts: Parts<T>) -> Self {
        Self {
            status: parts.status,
            metadata: parts.metadata,
            message: parts.message,
        }
    }

    pub fn into_parts(self) -> Parts<T> {
        Parts {
            status: self.status,
            metadata: self.metadata,
            message: self.message,
        }
    }
}

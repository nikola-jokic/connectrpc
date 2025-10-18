pub mod b64;
#[cfg(feature = "client")]
pub mod client;
pub mod codec;
pub mod connect;
pub mod error;
pub mod header;
pub mod metadata;
pub mod request;
pub mod response;
#[cfg(feature = "server")]
pub mod server;
pub mod stream;

pub use crate::codec::Codec;
pub use crate::error::Error;
pub use crate::request::UnaryRequest;
pub use crate::response::UnaryResponse;

#[cfg(feature = "reqwest")]
pub use client::reqwest::ReqwestClient;

#[cfg(all(feature = "client", feature = "async"))]
pub use client::AsyncUnaryClient;

#[cfg(all(feature = "client", feature = "sync"))]
pub use client::SyncUnaryClient;

pub use http;
pub use prost;

pub type Result<T> = std::result::Result<T, Error>;

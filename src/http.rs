use bytes::{Bytes, BytesMut};
use futures::StreamExt;
use reqwest::redirect::Policy;
use std::{num::NonZeroU64, time::Duration};
use thiserror::Error;

#[inline]
pub fn build_http_client(
    timeout: Duration,
    connect_timeout: Duration,
    https_only: bool,
) -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION_MAJOR"),
            ".",
            env!("CARGO_PKG_VERSION_MINOR"),
            " (",
            env!("CARGO_PKG_REPOSITORY"),
            ")"
        ))
        .https_only(https_only)
        .redirect(Policy::limited(3))
        .gzip(true)
        .brotli(true)
        .zstd(true)
        .deflate(true)
        .connect_timeout(connect_timeout)
        .timeout(timeout)
        .build()
}

#[derive(Debug, Error)]
pub enum BytesStreamCappedError {
    /// The response content length exceeded the size limit.
    #[error("content exceeded the maximum size")]
    TooLarge,
    /// An internal client error occured whilst processing the request,
    /// see [`reqwest::Error`].
    #[error("an internal client error occured: {0}")]
    ClientError(#[from] reqwest::Error),
}

/// A wrapper around `Response::bytes_stream()` that acts like `Response::bytes()`
/// but enforces a maximum size limit while streaming the response.
pub async fn bytes_stream_capped(
    response: reqwest::Response,
    max_size: NonZeroU64,
) -> Result<Bytes, BytesStreamCappedError> {
    if let Some(content_length) = response.content_length()
        && content_length > max_size.get()
    {
        return Err(BytesStreamCappedError::TooLarge);
    }

    let mut buffer = BytesMut::with_capacity(
        response
            .content_length()
            .unwrap_or(64 * 1024)
            .min(max_size.get())
            .try_into()
            .unwrap_or(usize::MAX),
    );
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(BytesStreamCappedError::ClientError)?;
        if buffer.len() as u64 + chunk.len() as u64 > max_size.get() {
            return Err(BytesStreamCappedError::TooLarge);
        }
        buffer.extend_from_slice(&chunk);
    }

    Ok(buffer.freeze())
}

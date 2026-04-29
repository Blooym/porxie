use bytes::Bytes;
use futures_util::StreamExt;
use std::num::NonZeroU64;
use thiserror::Error;

pub const PORXIE_USER_AGENT: &str = concat!(
    env!("CARGO_PKG_NAME"),
    "/",
    env!("CARGO_PKG_VERSION_MAJOR"),
    ".",
    env!("CARGO_PKG_VERSION_MINOR"),
    " (",
    env!("CARGO_PKG_REPOSITORY"),
    ")"
);

#[derive(Debug, Error)]
pub enum BytesStreamCappedError {
    /// The response content length exceeded the size limit.
    #[error("content exceeded the maximum size")]
    TooLarge,
    /// An internal client error occurred whilst processing the request,
    /// see [`reqwest::Error`].
    #[error(transparent)]
    ClientError(#[from] reqwest::Error),
}

/// Stream a response into [`Bytes`], aborting if the buffer exceeds `max_size`.
///
/// Pre-allocates a buffer based on response size heuristics when available, otherwise starts small
/// and grows as data is streamed. If the buffer capacity differs from the buffer length after,
/// the buffer may be shrunk to fit.
pub async fn bytes_stream_capped(
    response: reqwest::Response,
    max_size: NonZeroU64,
) -> Result<Bytes, BytesStreamCappedError> {
    let max_size = max_size.get();

    // Use body size hint, fallback to content-length header.
    let inferred_size = response.content_length().or_else(|| {
        response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
    });

    // Skip stream if the inferred size exceeds max size.
    if inferred_size.is_some_and(|size| size > max_size) {
        return Err(BytesStreamCappedError::TooLarge);
    }

    // Stream bytes in chunks and abort if we exceed max size.
    let mut stream = response.bytes_stream();
    let mut buffer = Vec::with_capacity(
        inferred_size
            .unwrap_or(64 * 1024)
            .min(max_size)
            .try_into()
            .expect("buffer allocation should not exceed usize"),
    );
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(BytesStreamCappedError::ClientError)?;
        if buffer.len() as u64 + chunk.len() as u64 > max_size {
            return Err(BytesStreamCappedError::TooLarge);
        }
        buffer.extend_from_slice(&chunk);
    }

    Ok(Bytes::from(
        buffer.into_boxed_slice(), // shrink capacity to fit
    ))
}

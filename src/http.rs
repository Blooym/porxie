use bytes::{Bytes, BytesMut};
use futures::StreamExt;
use reqwest::{Proxy, Url, redirect::Policy};
use std::{num::NonZeroU64, time::Duration};
use thiserror::Error;

const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
const MAX_REDIRECTS: usize = 5;

#[inline]
pub fn build_internal_http_client(timeout: Duration) -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .https_only(false)
        .redirect(Policy::limited(MAX_REDIRECTS))
        .timeout(timeout)
        .build()
}

#[inline]
pub fn build_external_http_client(
    timeout: Duration,
    proxy_url: Option<Url>,
) -> Result<reqwest::Client, reqwest::Error> {
    let mut builder = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .https_only(!cfg!(debug_assertions))
        .redirect(Policy::limited(MAX_REDIRECTS))
        .timeout(timeout);

    if let Some(proxy) = proxy_url {
        builder = builder.proxy(Proxy::all(proxy)?);
    };

    builder.build()
}

#[derive(Debug, Error)]
pub enum BytesCappedError {
    #[error("content exceeded the maximum size")]
    TooLarge,
    #[error("an internal client error occured: {0}")]
    ClientError(#[from] reqwest::Error),
}

/// A wrapper around `Response::bytes_stream()` that acts like `Response::bytes()`
/// but enforces a maximum size limit while streaming the response.
pub async fn bytes_capped(
    response: reqwest::Response,
    max_size: NonZeroU64,
) -> Result<Bytes, BytesCappedError> {
    if let Some(content_length) = response.content_length()
        && content_length > max_size.get()
    {
        return Err(BytesCappedError::TooLarge);
    }

    let mut buffer = BytesMut::with_capacity(
        response
            .content_length()
            .unwrap_or(64 * 1024)
            .min(max_size.get()) as usize,
    );
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(BytesCappedError::ClientError)?;
        if (buffer.len() + chunk.len()) as u64 > max_size.get() {
            return Err(BytesCappedError::TooLarge);
        }
        buffer.extend_from_slice(&chunk);
    }

    Ok(buffer.freeze())
}

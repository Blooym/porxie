mod blob;
mod index;
pub mod xrpc;

pub use blob::get_blob_handler;
pub use index::get_index_handler;

/// A header value for [`header::CACHE_CONTROL`] indicating the response cannot be cached at all.
const CACHE_CONTROL_NOCACHE_VALUE: &str = "must-understand, no-store";

#[derive(serde::Serialize)]
pub struct ErrorResponse {
    error: &'static str,
    message: Option<&'static str>,
}

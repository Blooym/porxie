mod blob;
mod index;
pub mod xrpc;

pub use blob::get_blob_handler;
pub use index::get_index_handler;

/// Cache-Control header value indicating the response cannot be cached.
const CACHE_CONTROL_NOCACHE_VALUE: &str = "must-understand, no-store";

/// An xrpc-compatiable error response.
#[derive(serde::Serialize)]
pub struct XrpcErrorResponse {
    error: &'static str,
    message: Option<&'static str>,
}

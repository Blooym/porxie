mod blob;
mod cache;
mod health;

pub use blob::get_blob_handler;
pub use cache::delete_cache_handler;
pub use health::get_health_handler;

/// A header value for [`header::CACHE_CONTROL`] indicating the response cannot be cached at all.
pub const CACHE_CONTROL_NOCACHE_VALUE: &str = "must-understand, no-store";

#[derive(serde::Serialize)]
pub struct ErrorResponse {
    error: &'static str,
    message: Option<&'static str>,
}

pub async fn get_index_handler() -> &'static str {
    r#"
 _____                _
|  __ \              (_)
| |__) |__  _ ____  ___  ___
|  ___/ _ \| '__\ \/ / |/ _ \
| |  | (_) | |   >  <| |  __/
|_|   \___/|_|  /_/\_\_|\___|


A correct and efficient ATProto blob proxy for secure content delivery.

Links:
 - Repo:    https://codeberg.org/Blooym/porxie
 - ATProto: https://atproto.com

Routes:
 - HTTP GET /{did}/{cid} - Resolve and fetch a blob from its origin.
 - HTTP DELETE /cache/{cid or did} - Invalidate cache for either a CID (blob, policy, ownership) or for a DID (ownerships and policies). Requires auth.
"#
}

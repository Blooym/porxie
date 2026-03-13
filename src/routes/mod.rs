mod blob;
mod cache;

pub use blob::get_blob_handler;
pub use cache::delete_cache_handler;

use axum::http::{HeaderName, HeaderValue, header};
use serde::Serialize;

#[derive(Serialize)]
pub struct ErrorResponse {
    error: &'static str,
    message: Option<&'static str>,
}

pub async fn get_index_handler() -> ([(HeaderName, HeaderValue); 1], &'static str) {
    (
        [(
            header::CACHE_CONTROL,
            const { HeaderValue::from_static("public, max-age=31536000, immutable") },
        )],
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
 - HTTP DELETE /cache/{cid or did} - Invalidate cache for either a CID (blob, policy, ownership) or for a DID (ownerships and policies). Requires configured bearer auth token.
"#,
    )
}

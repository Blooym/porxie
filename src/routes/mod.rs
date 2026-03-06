mod blob;
mod cache;

pub use blob::get_blob_handler;
pub use cache::*;

pub async fn get_index_handler() -> &'static str {
    r#"
 _____                _
|  __ \              (_)
| |__) |__  _ ____  ___  ___
|  ___/ _ \| '__\ \/ / |/ _ \
| |  | (_) | |   >  <| |  __/
|_|   \___/|_|  /_/\_\_|\___|


A correct and efficient ATProto blob proxy service.

Links:
 - Repo:    https://codeberg.org/Blooym/porxie
 - ATProto: https://atproto.com

Routes:
 - HTTP GET /{did}/{cid} - Resolve and fetch a blob from its origin.
 - HTTP DELETE /cache/{cid or did} - Invalidate cache for either a CID (blob, policy, ownership) or for a DID (ownerships and policies). Requires configured bearer auth token.
"#
}

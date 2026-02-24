mod blob;

pub use blob::{delete_blob_handler, get_blob_handler};

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
 - HTTP GET /did/cid - Resolve and fetch a blob from its origin.
 - HTTP DELETE /did/cid - Invalidate blob and moderation cache for a specific blob. Requires configured bearer auth token.
"#
}

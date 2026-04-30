use crate::{AppState, routes::get_blob_handler};
use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use jacquard_axum::ExtractXrpc;
use lexgen::dev_blooym::porxie::get_blob::GetBlobRequest;
use std::sync::Arc;

/// Compatibility layer that converts the xrpc call into a
/// regular get blob request. May become the primary method
/// in the future.
pub async fn get_blob_handler_xrpc_compat(
    state: State<Arc<AppState>>,
    ExtractXrpc(request): ExtractXrpc<GetBlobRequest>,
) -> impl IntoResponse {
    get_blob_handler(
        Path((request.did.to_string(), request.cid.to_string())),
        state,
    )
    .await
}

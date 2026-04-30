use crate::{
    AppState,
    extractors::AdminXrpcAuth,
    routes::{CACHE_CONTROL_NOCACHE_VALUE, XrpcErrorResponse},
    types::blob_cid::BlobCid,
};
use axum::{
    Json,
    extract::State,
    http::{HeaderName, header},
};
use jacquard_axum::ExtractXrpc;
use lexgen::dev_blooym::porxie::cache::purge_blob::PurgeBlobRequest;
use reqwest::StatusCode;
use std::sync::Arc;

pub async fn xrpc_cache_purge_blob_handler(
    _auth: AdminXrpcAuth,
    State(state): State<Arc<AppState>>,
    ExtractXrpc(request): ExtractXrpc<PurgeBlobRequest>,
) -> Result<
    StatusCode,
    (
        StatusCode,
        [(HeaderName, &'static str); 1],
        Json<XrpcErrorResponse>,
    ),
> {
    let cid = BlobCid::try_from(request.cid.as_str()).map_err(|_| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
            Json(XrpcErrorResponse {
                error: "MalformedCid",
                message: Some("Invalid or unprocessable CID"),
            }),
        )
    })?;

    if let Some(ref policy_client) = state.policy_client {
        policy_client.invalidate_cache_entries(move |k, _v| k.1 == cid)
    }
    state.blob_service.invalidate_blob_cache_entry(&cid).await;
    state
        .blob_service
        .invalidate_blob_ownership_cache_entries(move |k, _v| k.0 == cid);

    Ok(StatusCode::OK)
}

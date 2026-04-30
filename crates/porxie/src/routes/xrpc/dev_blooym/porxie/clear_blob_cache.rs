use crate::{
    AppState,
    extractors::AdminXrpcAuth,
    routes::{CACHE_CONTROL_NOCACHE_VALUE, ErrorResponse},
    types::blob_cid::BlobCid,
};
use axum::{
    Json,
    extract::State,
    http::{HeaderName, header},
};
use jacquard_axum::ExtractXrpc;
use lexgen::dev_blooym::porxie::clear_blob_cache::ClearBlobCacheRequest;
use reqwest::StatusCode;
use std::sync::Arc;

pub async fn clear_blob_cache_handler(
    _auth: AdminXrpcAuth,
    State(state): State<Arc<AppState>>,
    ExtractXrpc(request): ExtractXrpc<ClearBlobCacheRequest>,
) -> Result<
    StatusCode,
    (
        StatusCode,
        [(HeaderName, &'static str); 1],
        Json<ErrorResponse>,
    ),
> {
    let cid = BlobCid::try_from(request.cid.as_str()).map_err(|_| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
            Json(ErrorResponse {
                error: "MalformedCid",
                message: Some("Invalid or unprocessable CID"),
            }),
        )
    })?;

    if let Some(ref policy_client) = state.policy_client {
        policy_client.invalidate_policies(move |k, _v| k.1 == cid)
    }
    state.blob_service.invalidate_blob(&cid).await;
    state
        .blob_service
        .invalidate_blob_ownership(move |k, _v| k.0 == cid);

    Ok(StatusCode::OK)
}

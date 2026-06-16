use crate::{
    server::{ServerState, extractors::AdminXrpcAuth, routes::CACHE_CONTROL_NOCACHE_VALUE},
    types::blob_cid::BlobCid,
};
use axum::{
    Json,
    extract::State,
    http::{HeaderName, header},
};
use jacquard_axum::ExtractXrpc;
use jacquard_common::xrpc::XrpcError;
use porxie_lexgen::dev_blooym::porxie::cache::purge_blob::{PurgeBlobError, PurgeBlobRequest};
use reqwest::StatusCode;
use std::sync::Arc;

pub async fn xrpc_cache_purge_blob_handler(
    _auth: AdminXrpcAuth,
    State(state): State<Arc<ServerState>>,
    ExtractXrpc(request): ExtractXrpc<PurgeBlobRequest>,
) -> Result<
    StatusCode,
    (
        StatusCode,
        [(HeaderName, &'static str); 1],
        Json<XrpcError<PurgeBlobError>>,
    ),
> {
    let cid = BlobCid::try_from(request.cid.as_str()).map_err(|_| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
            Json(XrpcError::Xrpc(PurgeBlobError::MalformedCid(None))),
        )
    })?;

    if let Some(ref policy_client) = state.policy_client {
        policy_client.invalidate_cache_entries_if(move |k, _v| k.1 == cid)
    }
    state.blob_service.invalidate_data_cache_entry(&cid).await;
    state
        .blob_service
        .invalidate_ownership_cache_entries_if(move |k, _v| k.0 == cid);

    Ok(StatusCode::OK)
}

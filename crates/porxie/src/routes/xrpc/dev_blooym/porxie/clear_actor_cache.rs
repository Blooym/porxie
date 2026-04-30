use crate::{AppState, extractors::AdminXrpcAuth, routes::ErrorResponse};
use axum::{Json, extract::State, http::HeaderName};
use jacquard_axum::ExtractXrpc;
use lexgen::dev_blooym::porxie::clear_actor_cache::ClearActorCacheRequest;
use reqwest::StatusCode;
use std::sync::Arc;

pub async fn clear_actor_cache_handler(
    _auth: AdminXrpcAuth,
    State(state): State<Arc<AppState>>,
    ExtractXrpc(request): ExtractXrpc<ClearActorCacheRequest>,
) -> Result<
    StatusCode,
    (
        StatusCode,
        [(HeaderName, &'static str); 1],
        Json<ErrorResponse>,
    ),
> {
    if let Some(ref policy_client) = state.policy_client {
        policy_client.invalidate_policies({
            let did = request.did.clone();
            move |k, _v| k.0 == did
        })
    }
    state
        .identity_service
        .invalidate_did_cache(&request.did)
        .await;
    state
        .blob_service
        .invalidate_blob_ownership(move |k, _v| k.1 == request.did);

    Ok(StatusCode::OK)
}

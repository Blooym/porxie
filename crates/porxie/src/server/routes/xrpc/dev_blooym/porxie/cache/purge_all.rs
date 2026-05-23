use crate::server::{ServerState, extractors::AdminXrpcAuth};
use axum::extract::State;
use jacquard_axum::ExtractXrpc;
use porxie_lexgen::dev_blooym::porxie::cache::purge_all::PurgeAllRequest;
use reqwest::StatusCode;
use std::sync::Arc;

pub async fn xrpc_cache_purge_all_handler(
    _auth: AdminXrpcAuth,
    State(state): State<Arc<ServerState>>,
    ExtractXrpc(_request): ExtractXrpc<PurgeAllRequest>,
) -> StatusCode {
    if let Some(ref policy_client) = state.policy_client {
        policy_client.invalidate_cache_all();
    }
    state.identity_service.invalidate_cache_all();
    state.blob_service.invalidate_data_cache_all();
    state.blob_service.invalidate_ownership_cache_all();
    StatusCode::OK
}

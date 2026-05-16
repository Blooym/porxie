use crate::server::{ServerState, extractors::AdminXrpcAuth};
use axum::extract::State;
use jacquard_axum::ExtractXrpc;
use porxie_lexgen::dev_blooym::porxie::cache::purge_actor::PurgeActorRequest;
use reqwest::StatusCode;
use std::sync::Arc;

pub async fn xrpc_cache_purge_actor_handler(
    _auth: AdminXrpcAuth,
    State(state): State<Arc<ServerState>>,
    ExtractXrpc(request): ExtractXrpc<PurgeActorRequest>,
) -> StatusCode {
    if let Some(ref policy_client) = state.policy_client {
        policy_client.invalidate_cache_entries({
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
        .invalidate_blob_ownership_cache_entries(move |k, _v| k.1 == request.did);

    StatusCode::OK
}

use crate::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, authorization::Bearer},
};
use cid::Cid;
use jacquard_common::types::did::Did;
use std::sync::Arc;
use tracing::info;

pub async fn delete_blob_handler(
    Path((did, cid)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
    TypedHeader(Authorization(bearer)): TypedHeader<Authorization<Bearer>>,
) -> Result<StatusCode, (StatusCode, &'static str)> {
    if state.auth_token.as_deref() != Some(bearer.token()) {
        return Err((StatusCode::UNAUTHORIZED, "Unauthorized"));
    }

    let (did, cid) = (
        Did::new_owned(did).map_err(|_| {
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                "Invalid or unprocessable DID",
            )
        })?,
        Cid::try_from(cid).map_err(|_| {
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                "Invalid or unprocessable CID",
            )
        })?,
    );

    info!("invalidating cached blob '{cid}' and cached moderation action '{cid}:{did}'");

    state.response_cache.invalidate(&cid).await;
    state.moderation_cache.invalidate(&(did, cid)).await;

    Ok(StatusCode::OK)
}

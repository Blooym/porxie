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

pub async fn delete_cache_handler(
    Path(identifier): Path<String>,
    State(state): State<Arc<AppState>>,
    TypedHeader(Authorization(bearer)): TypedHeader<Authorization<Bearer>>,
) -> Result<StatusCode, (StatusCode, &'static str)> {
    if state.auth_token.as_deref() != Some(bearer.token()) {
        return Err((StatusCode::UNAUTHORIZED, "Unauthorized"));
    }

    if identifier.starts_with("did:") {
        tracing::info!("invalidating DID cache entries'");
        let did = Did::new_owned(identifier).map_err(|_| {
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                "Invalid or unprocessable DID",
            )
        })?;

        // Clear all ownership and policy data for this DID.
        state
            .cache
            .blob_policy
            .invalidate_entries_if({
                let did = did.clone();
                move |k, _v| k.0 == did
            })
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to schedule cache invalidation",
                )
            })?;
        state
            .cache
            .blob_ownership
            .invalidate_entries_if(move |k, _v| k.1 == did)
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to schedule cache invalidation",
                )
            })?;
    } else {
        tracing::info!("invalidating CID cache entries");
        let cid = Cid::try_from(identifier.as_str()).map_err(|_| {
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                "Invalid or unprocessable CID",
            )
        })?;

        // Clear blob content from memory as well as ownership and policy data for this CID.
        state.cache.blob_content.invalidate(&cid).await;
        state
            .cache
            .blob_ownership
            .invalidate_entries_if(move |k, _v| k.0 == cid)
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to schedule cache invalidation",
                )
            })?;
        state
            .cache
            .blob_policy
            .invalidate_entries_if(move |k, _v| k.1 == cid)
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to schedule cache invalidation",
                )
            })?;
    }

    Ok(StatusCode::OK)
}

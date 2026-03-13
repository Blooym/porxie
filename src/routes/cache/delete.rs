use crate::{AppState, routes::ErrorResponse, types::blob_cid::BlobCid};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, authorization::Bearer},
};
use jacquard_common::types::did::Did;
use std::sync::Arc;

pub async fn delete_cache_handler(
    Path(identifier): Path<String>,
    State(state): State<Arc<AppState>>,
    TypedHeader(Authorization(bearer)): TypedHeader<Authorization<Bearer>>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    if state.auth_token.as_deref() != Some(bearer.token()) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "Unauthorized",
                message: None,
            }),
        ));
    }

    if identifier.starts_with("did:") {
        tracing::info!("invalidating DID cache entries");
        let did = Did::new_owned(identifier).map_err(|_| {
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ErrorResponse {
                    error: "MalformedDid",
                    message: Some("Invalid or unprocessable DID"),
                }),
            )
        })?;

        // Clear all identity, ownership and policy data for this DID.
        state
            .cache
            .identity
            .invalidate_entries_if({
                let did = did.clone();
                move |k, _v| *k == did
            })
            .map_err(|err| {
                tracing::error!("failed to schedule identity cache invalidation: {err:?}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: "InternalServerError",
                        message: Some("Failed to schedule cache invalidation"),
                    }),
                )
            })?;
        state
            .cache
            .blob_policy
            .invalidate_entries_if({
                let did = did.clone();
                move |k, _v| k.0 == did
            })
            .map_err(|err| {
                tracing::error!("failed to schedule blob policy cache invalidation: {err:?}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: "InternalServerError",
                        message: Some("Failed to schedule cache invalidation"),
                    }),
                )
            })?;
        state
            .cache
            .blob_ownership
            .invalidate_entries_if(move |k, _v| k.1 == did)
            .map_err(|err| {
                tracing::error!("failed to schedule blob ownership cache invalidation: {err:?}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: "InternalServerError",
                        message: Some("Failed to schedule cache invalidation"),
                    }),
                )
            })?;
    } else {
        tracing::info!("invalidating CID cache entries");
        let cid = BlobCid::try_from(identifier.as_str()).map_err(|_| {
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ErrorResponse {
                    error: "MalformedCid",
                    message: Some("Invalid or unprocessable CID"),
                }),
            )
        })?;

        // Clear blob content from memory as well as ownership and policy data for this CID.
        state.cache.blob_content.invalidate(&cid).await;
        state
            .cache
            .blob_ownership
            .invalidate_entries_if(move |k, _v| k.0 == cid)
            .map_err(|err| {
                tracing::error!("failed to schedule blob ownership cache invalidation: {err:?}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: "InternalServerError",
                        message: Some("Failed to schedule cache invalidation"),
                    }),
                )
            })?;
        state
            .cache
            .blob_policy
            .invalidate_entries_if(move |k, _v| k.1 == cid)
            .map_err(|err| {
                tracing::error!("failed to schedule blob policy cache invalidation: {err:?}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: "InternalServerError",
                        message: Some("Failed to schedule cache invalidation"),
                    }),
                )
            })?;
    }

    Ok(StatusCode::OK)
}

use crate::{
    AppState,
    routes::{CACHE_CONTROL_NOCACHE_VALUE, ErrorResponse},
    types::blob_cid::BlobCid,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderName, StatusCode, header},
};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, authorization::Bearer},
};
use jacquard_common::types::did::Did;
use std::sync::Arc;
use subtle::ConstantTimeEq;

pub async fn delete_cache_handler(
    Path(identifier): Path<String>,
    State(state): State<Arc<AppState>>,
    TypedHeader(Authorization(bearer)): TypedHeader<Authorization<Bearer>>,
) -> Result<
    StatusCode,
    (
        StatusCode,
        [(HeaderName, &'static str); 1],
        Json<ErrorResponse>,
    ),
> {
    if state
        .auth_token
        .as_ref()
        .map(|expected| expected.as_bytes().ct_eq(bearer.token().as_bytes()).into())
        .unwrap_or(false)
    {
        return Err((
            StatusCode::UNAUTHORIZED,
            [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
            Json(ErrorResponse {
                error: "Unauthorized",
                message: None,
            }),
        ));
    }

    // TODO: Really need to expose a nicer cache purging API,
    // matching on prefix sucks.
    if identifier.starts_with("did:") {
        tracing::info!("invalidating DID cache entries");
        let did = Did::new_owned(identifier).map_err(|_| {
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                Json(ErrorResponse {
                    error: "MalformedDid",
                    message: Some("Invalid or unprocessable DID"),
                }),
            )
        })?;
        state.identity_service.invalidate_did_cache(&did).await;
        if let Some(ref policy_client) = state.policy_client {
            policy_client.invalidate_policies({
                let did = did.clone();
                move |k, _v| k.0 == did
            })
        }
        state
            .blob_service
            .invalidate_blob_ownership(move |k, _v| k.1 == did);
    } else {
        tracing::info!("invalidating CID cache entries");
        let cid = BlobCid::try_from(identifier.as_str()).map_err(|_| {
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                Json(ErrorResponse {
                    error: "MalformedCid",
                    message: Some("Invalid or unprocessable CID"),
                }),
            )
        })?;
        state.blob_service.invalidate_blob(&cid).await;
        state
            .blob_service
            .invalidate_blob_ownership(move |k, _v| k.0 == cid);
        if let Some(ref policy_client) = state.policy_client {
            policy_client.invalidate_policies(move |k, _v| k.1 == cid)
        }
    }

    Ok(StatusCode::OK)
}

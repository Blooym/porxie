use crate::{
    blob_service::{BlobDownloadError, BlobOwnershipError, BlobUrlResolver},
    policy_client::PolicyDecision,
    server::{
        ServerState,
        routes::{CACHE_CONTROL_NOCACHE_VALUE, XrpcErrorResponse},
    },
    types::blob_cid::BlobCid,
};
use axum::{
    Json,
    body::Body,
    extract::{Path, State},
    http::{HeaderName, HeaderValue, StatusCode, header},
    response::Response,
};
use jacquard_common::types::did::Did;
use std::sync::Arc;

/// Fetch a blob from a given upstream and return it.
pub async fn get_blob_handler(
    Path((raw_did, raw_cid)): Path<(String, String)>,
    State(state): State<Arc<ServerState>>,
) -> Result<
    Response,
    (
        StatusCode,
        [(HeaderName, &'static str); 1],
        Json<XrpcErrorResponse>,
    ),
> {
    let (did, cid) = (
        match Did::new_owned(raw_did.as_str()) {
            Ok(did) => did,
            Err(_) => {
                return Err((
                    StatusCode::UNPROCESSABLE_ENTITY,
                    [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                    Json(XrpcErrorResponse {
                        error: "MalformedDid",
                        message: Some("Invalid or unprocessable DID"),
                    }),
                ));
            }
        },
        match BlobCid::try_from(raw_cid.as_str()) {
            Ok(cid) => cid,
            Err(_) => {
                return Err((
                    StatusCode::UNPROCESSABLE_ENTITY,
                    [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                    Json(XrpcErrorResponse {
                        error: "MalformedCid",
                        message: Some("Invalid or unprocessable CID"),
                    }),
                ));
            }
        },
    );

    // Check the policy status of the blob.
    if let Some(ref policy_client) = state.policy_client {
        match policy_client.get_policy(&did, cid).await {
            Ok(policy) => {
                match policy {
                    PolicyDecision::Allowed => {}
                    PolicyDecision::Forbidden => {
                        return Err((
                            StatusCode::GONE,
                            [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                            Json(XrpcErrorResponse {
                                error: "PolicyForbidden",
                                message: Some("Requested blob has been forbidden by this service"),
                            }),
                        ));
                    }
                };
            }
            Err(_) => {
                if !state.policy_fail_open {
                    // TODO: Maybe give a more precise error?
                    return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                        Json(XrpcErrorResponse {
                            error: "InternalServerError",
                            message: Some("An internal server error occured."),
                        }),
                    ));
                }
            }
        }
    }

    // Fetch the blob from cache/origin.
    let blob = match state
        .blob_service
        .fetch_blob(
            &did,
            &cid,
            BlobUrlResolver::Pds {
                identity_service: &state.identity_service,
            },
            state.max_blob_size,
            &state.allowed_mimetypes,
        )
        .await
    {
        Ok(blob) => blob,
        Err(err) => {
            return Err(match *err {
                BlobDownloadError::NotFound => (
                    StatusCode::NOT_FOUND,
                    [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                    Json(XrpcErrorResponse {
                        error: "BlobNotFound",
                        message: Some("Blob not found"),
                    }),
                ),
                BlobDownloadError::TooLarge => (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                    Json(XrpcErrorResponse {
                        error: "BlobTooLarge",
                        message: Some("Blob exceeds maximum allowed size"),
                    }),
                ),
                BlobDownloadError::ForbiddenMimeType => (
                    StatusCode::FORBIDDEN,
                    [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                    Json(XrpcErrorResponse {
                        error: "BlobForbiddenType",
                        message: Some("Content type is not allowed"),
                    }),
                ),
                BlobDownloadError::CidMismatch => (
                    StatusCode::BAD_GATEWAY,
                    [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                    Json(XrpcErrorResponse {
                        error: "BlobCidMismatch",
                        message: Some("Blob content does not match CID"),
                    }),
                ),
                BlobDownloadError::CidUnsupportedMultihash => (
                    StatusCode::NOT_IMPLEMENTED,
                    [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                    Json(XrpcErrorResponse {
                        error: "CidUnsupported",
                        message: Some("Unsupported CID multihash"),
                    }),
                ),
                BlobDownloadError::BlobResolutionFailure => (
                    StatusCode::BAD_GATEWAY,
                    [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                    Json(XrpcErrorResponse {
                        error: "CannotResolve",
                        message: Some("Failed to resolve source of blob"),
                    }),
                ),
                BlobDownloadError::FetchFailure
                | BlobDownloadError::ErrorStatusCode
                | BlobDownloadError::StreamFailed => (
                    StatusCode::BAD_GATEWAY,
                    [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                    Json(XrpcErrorResponse {
                        error: "BlobFetchFailed",
                        message: Some("Failed to fetch blob from origin"),
                    }),
                ),
            });
        }
    };

    // Check if the user has a copy of this blob via cache/origin.
    //
    // Note: This will just return from cache if the blob was just fetched
    // using the same key. This check does not validate the blob cid matches,
    // just that the blob is reported to exist.
    if let Err(err) = state
        .blob_service
        .fetch_blob_ownership(
            &did,
            cid,
            BlobUrlResolver::Pds {
                identity_service: &state.identity_service,
            },
        )
        .await
    {
        return Err(match *err {
            BlobOwnershipError::NotFound => (
                StatusCode::NOT_FOUND,
                [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                Json(XrpcErrorResponse {
                    error: "BlobNotFound",
                    message: Some("Blob not found"),
                }),
            ),
            BlobOwnershipError::BlobResolutionFailure => (
                StatusCode::BAD_GATEWAY,
                [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                Json(XrpcErrorResponse {
                    error: "CannotResolve",
                    message: Some("Failed to resolve source of blob"),
                }),
            ),
            BlobOwnershipError::ErrorStatusCode | BlobOwnershipError::FetchFailure => (
                StatusCode::BAD_GATEWAY,
                [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                Json(XrpcErrorResponse {
                    error: "BlobFetchFailed",
                    message: Some("Failed to fetch blob from origin"),
                }),
            ),
        });
    }

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, blob.mime_type.essence_str())
        .header(header::CACHE_CONTROL, &state.cache_control_header)
        .header(
            header::CONTENT_SECURITY_POLICY,
            const { HeaderValue::from_static("default-src 'none'; sandbox") },
        )
        .header(
            header::X_CONTENT_TYPE_OPTIONS,
            const { HeaderValue::from_static("nosniff") },
        )
        .header(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_str(&format!(r#"attachment, filename="{cid}""#))
                .unwrap_or(const { HeaderValue::from_static("attachment") }),
        )
        .body(Body::from(blob.bytes))
        .expect("response should always build successfully"))
}

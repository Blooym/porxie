use crate::{
    blob_service::{BlobDownloadError, BlobOwnershipError, BlobUrlResolver},
    policy_client::PolicyDecision,
    server::{ServerState, routes::CACHE_CONTROL_NOCACHE_VALUE},
    types::blob_cid::BlobCid,
};
use axum::{
    Json,
    body::Body,
    extract::{Path, State},
    http::{HeaderName, HeaderValue, StatusCode, header},
    response::Response,
};
use jacquard_common::{
    types::did::Did,
    xrpc::{GenericXrpcError, XrpcError, XrpcRequest},
};
use porxie_lexgen::dev_blooym::porxie::get_blob::{GetBlob, GetBlobError};
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
        Json<XrpcError<GetBlobError<'static>>>,
    ),
> {
    let (did, cid) = (
        match Did::new_owned(raw_did.as_str()) {
            Ok(did) => did,
            Err(_) => {
                return Err((
                    StatusCode::UNPROCESSABLE_ENTITY,
                    [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                    Json(XrpcError::Xrpc(GetBlobError::MalformedCid(None))),
                ));
            }
        },
        match BlobCid::try_from(raw_cid.as_str()) {
            Ok(cid) => cid,
            Err(_) => {
                return Err((
                    StatusCode::UNPROCESSABLE_ENTITY,
                    [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                    Json(XrpcError::Xrpc(GetBlobError::MalformedDid(None))),
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
                            Json(XrpcError::Xrpc(GetBlobError::PolicyForbidden(None))),
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
                        Json(XrpcError::Generic(GenericXrpcError {
                            error: "InternalServerError".into(),
                            http_status: StatusCode::INTERNAL_SERVER_ERROR,
                            message: None,
                            method: GetBlob::METHOD.as_str(),
                            nsid: GetBlob::NSID,
                        })),
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
                // Client.
                BlobDownloadError::CidUnsupportedMultihash => (
                    StatusCode::BAD_REQUEST,
                    [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                    Json(XrpcError::Xrpc(GetBlobError::CidUnsupported(None))),
                ),
                BlobDownloadError::ForbiddenMimeType => (
                    StatusCode::FORBIDDEN,
                    [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                    Json(XrpcError::Xrpc(GetBlobError::BlobForbiddenType(None))),
                ),
                BlobDownloadError::TooLarge => (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                    Json(XrpcError::Xrpc(GetBlobError::BlobTooLarge(None))),
                ),

                // Resolver.
                BlobDownloadError::BlobResolutionFailure => (
                    StatusCode::FAILED_DEPENDENCY,
                    [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                    Json(XrpcError::Xrpc(GetBlobError::CannotResolve(None))),
                ),

                // Origin.
                BlobDownloadError::NotFound => (
                    StatusCode::NOT_FOUND,
                    [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                    Json(XrpcError::Xrpc(GetBlobError::BlobNotFound(None))),
                ),
                BlobDownloadError::CidMismatch => (
                    StatusCode::BAD_GATEWAY,
                    [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                    Json(XrpcError::Xrpc(GetBlobError::BlobCidMismatch(None))),
                ),
                BlobDownloadError::FetchFailure
                | BlobDownloadError::ErrorStatusCode
                | BlobDownloadError::StreamFailed => (
                    StatusCode::BAD_GATEWAY,
                    [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                    Json(XrpcError::Xrpc(GetBlobError::BlobFetchFailed(None))),
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
            // Resolver.
            BlobOwnershipError::BlobResolutionFailure => (
                StatusCode::FAILED_DEPENDENCY,
                [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                Json(XrpcError::Xrpc(GetBlobError::CannotResolve(None))),
            ),

            // Origin.
            BlobOwnershipError::NotFound => (
                StatusCode::NOT_FOUND,
                [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                Json(XrpcError::Xrpc(GetBlobError::BlobNotFound(None))),
            ),
            BlobOwnershipError::ErrorStatusCode | BlobOwnershipError::FetchFailure => (
                StatusCode::BAD_GATEWAY,
                [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                Json(XrpcError::Xrpc(GetBlobError::CannotResolve(None))),
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

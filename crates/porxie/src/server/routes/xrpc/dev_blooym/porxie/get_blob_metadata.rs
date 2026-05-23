use crate::{
    blob_service::{BlobDownloadError, BlobOwnershipError, BlobUrlResolver},
    policy_client::PolicyDecision,
    server::{ServerState, routes::CACHE_CONTROL_NOCACHE_VALUE},
    types::blob_cid::BlobCid,
};
use axum::{
    Json,
    extract::State,
    http::{HeaderName, HeaderValue, StatusCode, header},
};
use jacquard_axum::ExtractXrpc;
use jacquard_common::{
    IntoStatic,
    cowstr::ToCowStr,
    xrpc::{GenericXrpcError, XrpcError, XrpcRequest},
};
use porxie_lexgen::dev_blooym::porxie::get_blob_metadata::{
    AspectRatio, GetBlobMetadata, GetBlobMetadataError, GetBlobMetadataOutput,
    GetBlobMetadataOutputData, GetBlobMetadataRequest, ImageData, VideoData,
};
use porxie_mediautil::{
    deps::mime,
    metadata::{ImageMetadata, VideoMetadata},
};
use std::sync::Arc;

pub async fn xrpc_get_blob_metadata_handler(
    state: State<Arc<ServerState>>,
    ExtractXrpc(request): ExtractXrpc<GetBlobMetadataRequest>,
) -> Result<
    (
        [(HeaderName, HeaderValue); 1],
        Json<GetBlobMetadataOutput<'static>>,
    ),
    (
        StatusCode,
        [(HeaderName, &'static str); 1],
        Json<XrpcError<GetBlobMetadataError<'static>>>,
    ),
> {
    let cid = BlobCid::try_from(request.cid.as_str()).map_err(|_| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
            Json(XrpcError::Xrpc(GetBlobMetadataError::MalformedCid(None))),
        )
    })?;

    // Check policy status of blob.
    if let Some(ref policy_client) = state.policy_client {
        match policy_client.get_policy(&request.did, cid).await {
            Ok(policy) => {
                match policy {
                    PolicyDecision::Allowed => {}
                    PolicyDecision::Forbidden => {
                        return Err((
                            StatusCode::GONE,
                            [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                            Json(XrpcError::Xrpc(GetBlobMetadataError::PolicyForbidden(None))),
                        ));
                    }
                };
            }
            Err(_) => {
                if !state.policy_fail_open {
                    return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                        Json(XrpcError::Generic(GenericXrpcError {
                            error: "InternalServerError".into(),
                            http_status: StatusCode::INTERNAL_SERVER_ERROR,
                            message: None,
                            method: GetBlobMetadata::METHOD.as_str(),
                            nsid: GetBlobMetadata::NSID,
                        })),
                    ));
                }
            }
        }
    }

    // Download or get cached blob.
    let blob = match state
        .blob_service
        .fetch_blob(
            &request.did,
            &cid,
            BlobUrlResolver::Pds {
                identity_service: &state.identity_service,
            },
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
                    Json(XrpcError::Xrpc(GetBlobMetadataError::CidUnsupported(None))),
                ),
                BlobDownloadError::ForbiddenMimeType => (
                    StatusCode::FORBIDDEN,
                    [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                    Json(XrpcError::Xrpc(GetBlobMetadataError::BlobForbiddenType(
                        None,
                    ))),
                ),
                BlobDownloadError::TooLarge => (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                    Json(XrpcError::Xrpc(GetBlobMetadataError::BlobTooLarge(None))),
                ),

                // Resolver.
                BlobDownloadError::BlobResolutionFailure => (
                    StatusCode::FAILED_DEPENDENCY,
                    [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                    Json(XrpcError::Xrpc(GetBlobMetadataError::CannotResolve(None))),
                ),

                // Origin.
                BlobDownloadError::NotFound => (
                    StatusCode::NOT_FOUND,
                    [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                    Json(XrpcError::Xrpc(GetBlobMetadataError::BlobNotFound(None))),
                ),
                BlobDownloadError::CidMismatch => (
                    StatusCode::BAD_GATEWAY,
                    [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                    Json(XrpcError::Xrpc(GetBlobMetadataError::BlobCidMismatch(None))),
                ),
                BlobDownloadError::FetchFailure
                | BlobDownloadError::ErrorStatusCode
                | BlobDownloadError::StreamFailed => (
                    StatusCode::BAD_GATEWAY,
                    [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                    Json(XrpcError::Xrpc(GetBlobMetadataError::BlobFetchFailed(None))),
                ),
            });
        }
    };

    // Ensure this specific actor has the blob in their repository.
    if let Err(err) = state
        .blob_service
        .fetch_blob_ownership(
            &request.did,
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
                Json(XrpcError::Xrpc(GetBlobMetadataError::CannotResolve(None))),
            ),

            // Origin.
            BlobOwnershipError::NotFound => (
                StatusCode::NOT_FOUND,
                [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                Json(XrpcError::Xrpc(GetBlobMetadataError::BlobNotFound(None))),
            ),
            BlobOwnershipError::ErrorStatusCode | BlobOwnershipError::FetchFailure => (
                StatusCode::BAD_GATEWAY,
                [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
                Json(XrpcError::Xrpc(GetBlobMetadataError::CannotResolve(None))),
            ),
        });
    }

    // Calculate other format-specific attributes.
    let format_metadata = match blob.mime_type.type_() {
        mime::IMAGE => tokio::task::spawn_blocking({
            let bytes = blob.bytes.clone();
            move || ImageMetadata::from_bytes(&bytes)
        })
        .await
        .inspect_err(|err| tracing::warn!("image metadata task failed: {err:?}"))
        .ok()
        .and_then(|r| {
            r.inspect_err(|err| tracing::debug!("image metadata extraction failed: {err:?}"))
                .ok()
        })
        .map(|attr| {
            GetBlobMetadataOutputData::ImageData(Box::new(
                ImageData::new()
                    .aspect_ratio(
                        AspectRatio::new()
                            .height(attr.height as i64) // TODO: Convert safely.
                            .width(attr.width as i64) // TODO: Convert safely.
                            .build(),
                    )
                    .build(),
            ))
        }),
        mime::VIDEO => VideoMetadata::from_bytes(&blob.bytes)
            .await
            .inspect_err(|err| tracing::debug!("video metadata extraction failed: {err:?}"))
            .ok()
            .map(|attr| {
                GetBlobMetadataOutputData::VideoData(Box::new(
                    VideoData::new()
                        .aspect_ratio(
                            AspectRatio::new()
                                .width(attr.width)
                                .height(attr.height)
                                .build(),
                        )
                        .duration_ms(attr.duration_ms as i64) // TODO: Convert safely.
                        .build(),
                ))
            }),
        _ => None,
    };

    Ok((
        [(header::CACHE_CONTROL, state.cache_control_header.clone())],
        Json(GetBlobMetadataOutput {
            size: blob.bytes.len() as i64,
            content_type: Some(blob.mime_type.essence_str().to_cowstr().into_static()),
            data: format_metadata,
            extra_data: None,
        }),
    ))
}

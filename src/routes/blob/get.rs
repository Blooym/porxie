use crate::{
    AppState,
    cache::{CachedPolicyAction, CachedResponse},
    mime::is_mime_allowed,
};
use anyhow::bail;
use axum::{
    body::{Body, Bytes},
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, Response, StatusCode, header},
};
use cid::Cid;
use futures::StreamExt;
use jacquard_common::types::did::Did;
use jacquard_identity::resolver::IdentityResolver;
use mime::Mime;
use multihash_codetable::{Code, MultihashDigest};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

#[derive(Debug, Copy, Clone)]
enum BlobFetchError {
    PdsResolutionFailed,
    PdsFetchFailed,
    BlobNotFound,
    PdsErrorResponse,
    BlobTooLarge,
    BlobStreamFailed,
    UnsupportedMultihash,
    CidMismatch,
    DisallowedMimeType,
}

pub async fn get_blob_handler(
    Path((did, cid)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
) -> Result<axum::response::Response, (StatusCode, &'static str)> {
    let (did, cid) = (
        match Did::new_owned(did) {
            Ok(did) => did,
            Err(_) => {
                return Err((
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "Invalid or unprocessable DID",
                ));
            }
        },
        match Cid::try_from(cid) {
            Ok(cid) => cid,
            Err(_) => {
                return Err((
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "Invalid or unprocessable CID",
                ));
            }
        },
    );

    // Query policy service (if set) to see if the blob can be served.
    //
    // Policy queries will be made if needed, even when the blob itself is cached.
    // All policy decisions will be cached for a duration to prevent flooding the upstream.
    if let Some(ref policy_service_url) = state.policy_service_url {
        match state
            .policy_cache
            .try_get_with_by_ref(&(did.clone(), cid), async {
                let mut policy_service_url = policy_service_url.clone();
                policy_service_url
                    .path_segments_mut()
                    .expect("policy service URL cannot be a base")
                    .push(did.as_str())
                    .push(&cid.to_string());
                let mut request = state.internal_http_client.get(policy_service_url);
                if let Some(ref auth) = state.policy_service_auth_header {
                    request = request.header(reqwest::header::AUTHORIZATION, auth);
                }
                match request.send().await {
                    Ok(response) => match response.status() {
                        StatusCode::OK => Ok(CachedPolicyAction { can_serve: true }),
                        StatusCode::GONE => {
                            info!("policy service rejected blob {cid} for {did}");
                            Ok(CachedPolicyAction { can_serve: false })
                        }
                        status => {
                            error!("policy service returned unexpected status: {status}");
                            bail!("unexpected status code: {status}");
                        }
                    },
                    Err(err) => {
                        error!("error occurred contacting the policy service: {err:?}");
                        Err(err.into())
                    }
                }
            })
            .await
        {
            Ok(policy) if !policy.can_serve => {
                return Err((
                    StatusCode::GONE,
                    "Content is not available through this service",
                ));
            }
            Err(_) if !state.policy_service_fail_open => {
                return Err((StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error"));
            }
            _ => {}
        }
    }

    // Fetch the blob from source or cache. Concurrent requests for the same CID are combined.
    // Failures will not be not cached, subsequent requests will always retry.
    match state
        .response_cache
        .try_get_with_by_ref(&cid, {
            async {
                // Lookup PDS for the DID and return the PDS & Blob URL.
                let blob_url = {
                    let mut url = state.resolver.pds_for_did(&did).await.map_err(|err| {
                        warn!("failed to resolve PDS url for '{did}': {err:?}");
                        BlobFetchError::PdsResolutionFailed
                    })?;
                    url.set_path("/xrpc/com.atproto.sync.getBlob");
                    url.set_query(Some(&format!("did={did}&cid={cid}")));
                    url
                };

                // Fetch and validate the requested blob.
                let blob_bytes = {
                    // Request the blob from the PDS.
                    let response = state
                        .external_http_client
                        .get(blob_url)
                        .send()
                        .await
                        .map_err(|err| {
                            error!("failed to fetch blob from PDS: {err:?}");
                            BlobFetchError::PdsFetchFailed
                        })?;

                    if matches!(
                        response.status(),
                        StatusCode::NOT_FOUND | StatusCode::BAD_REQUEST
                    ) {
                        debug!("PDS returned 404 for blob {cid} on {did}");
                        return Err(BlobFetchError::BlobNotFound);
                    }
                    if !response.status().is_success() {
                        warn!("PDS returned error status: {}", response.status());
                        return Err(BlobFetchError::PdsErrorResponse);
                    }

                    // Validate the size of the body making a guess based the inferred size.
                    // This is strictly validated later when downloading.
                    if let Some(content_length) = response.content_length()
                        && content_length > state.max_blob_size
                    {
                        debug!("blob exceeds max size of {} bytes", state.max_blob_size);
                        return Err(BlobFetchError::BlobTooLarge);
                    };

                    // Incrementally download blob and abort if too large.
                    let mut buffer = Vec::with_capacity(
                        response
                            .content_length()
                            .unwrap_or(64 * 1024)
                            .min(state.max_blob_size) as usize,
                    );
                    let mut stream = response.bytes_stream();
                    while let Some(chunk) = stream.next().await {
                        let chunk = chunk.map_err(|err| {
                            warn!("error reading blob stream: {err:?}");
                            BlobFetchError::BlobStreamFailed
                        })?;
                        if (buffer.len() + chunk.len()) as u64 > state.max_blob_size {
                            debug!("blob exceeds max size of {} bytes", state.max_blob_size);
                            return Err(BlobFetchError::BlobTooLarge);
                        }
                        buffer.extend_from_slice(&chunk);
                    }

                    // Compute the blob CID and ensure it matches with the CID hash from the request.
                    let computed_cid = match cid.hash().code() {
                        0x12 => Cid::new_v1(0x55, Code::Sha2_256.digest(&buffer)),
                        0x1e => Cid::new_v1(0x55, Code::Blake3_256.digest(&buffer)),
                        hash => {
                            warn!("unsupported multihash: 0x{hash:x}");
                            return Err(BlobFetchError::UnsupportedMultihash);
                        }
                    };
                    if computed_cid != cid {
                        warn!("CID mismatch: expected {cid}, computed {computed_cid}");
                        return Err(BlobFetchError::CidMismatch);
                    }

                    buffer
                };

                // Loosely determine and validate mimetype. Not a strict check, may need future improvement.
                let mime_type: Mime = match infer::get(&blob_bytes) {
                    Some(m) => m
                        .mime_type()
                        .parse()
                        .expect("infer mimetype should always be valid"),
                    None => mime::APPLICATION_OCTET_STREAM,
                };
                if !is_mime_allowed(&mime_type, &state.allowed_mimetypes) {
                    debug!("blob was inferred to be a disallowed mime type: {mime_type}");
                    return Err(BlobFetchError::DisallowedMimeType);
                }

                // Build response.
                let body = Bytes::from(blob_bytes);
                let mut headers = HeaderMap::new();
                headers.insert(
                    header::CONTENT_TYPE,
                    mime_type
                        .essence_str()
                        .parse()
                        .expect("should parse mime type as header value"),
                );
                headers.insert(
                    header::CONTENT_SECURITY_POLICY,
                    HeaderValue::from_static("default-src 'none'; sandbox"),
                );
                headers.insert(
                    header::X_CONTENT_TYPE_OPTIONS,
                    HeaderValue::from_static("nosniff"),
                );
                headers.insert(header::CACHE_CONTROL, state.cache_control_header.clone());
                Ok(CachedResponse { body, headers })
            }
        })
        .await
    {
        Ok(blob) => {
            let mut response = Response::builder()
                .status(StatusCode::OK)
                .body(Body::from(blob.body))
                .expect("should build valid response");
            response.headers_mut().extend(blob.headers);
            Ok(response)
        }
        Err(err) => Err(match *err {
            BlobFetchError::BlobNotFound => (StatusCode::NOT_FOUND, "Blob not found"),
            BlobFetchError::BlobTooLarge => {
                (StatusCode::FORBIDDEN, "Blob exceeds maximum allowed size")
            }
            BlobFetchError::DisallowedMimeType => {
                (StatusCode::FORBIDDEN, "Content type is not allowed")
            }
            BlobFetchError::UnsupportedMultihash => {
                (StatusCode::NOT_IMPLEMENTED, "Unsupported CID multihash")
            }
            BlobFetchError::PdsResolutionFailed => {
                (StatusCode::BAD_GATEWAY, "Failed to resolve PDS for DID")
            }
            BlobFetchError::PdsFetchFailed
            | BlobFetchError::PdsErrorResponse
            | BlobFetchError::BlobStreamFailed => {
                (StatusCode::BAD_GATEWAY, "Failed to fetch blob from PDS")
            }
            BlobFetchError::CidMismatch => {
                (StatusCode::BAD_GATEWAY, "Blob content does not match CID")
            }
        }),
    }
}

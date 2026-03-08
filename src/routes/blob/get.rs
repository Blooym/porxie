use crate::http::{BytesStreamCappedError, bytes_stream_capped};
use crate::routes::ErrorResponse;
use crate::{
    AppState,
    cache::{CachedBlobData, CachedBlobPolicy},
    mime::is_mime_allowed,
};
use axum::Json;
use axum::{
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, Response, StatusCode, header},
};
use cid::Cid;
use jacquard_common::types::did::Did;
use jacquard_identity::resolver::{IdentityError, IdentityResolver};
use multihash_codetable::{Code, MultihashDigest};
use reqwest::Url;
use std::sync::Arc;

enum BlobPolicyError {
    /// The policy service returned an unexpected status code,.
    UnhandledStatusCode,
    /// The request to the policy service failed, for example due to the server being unavailable.
    FetchFailed,
}

enum BlobDownloadError {
    /// Failed to resolve the PDS for the given DID. The DID may be invalid or the
    /// resolver may be unavailable.
    DidPdsResolutionFailure,
    /// The blob's computed CID does not match the requested CID.
    CidMismatch,
    /// The requested CID uses a multihash algorithm unsupported by this server.
    CidUnsupportedMultihash,
    /// The blob could not be found in the user's repository.
    NotFound,
    /// The blob exceeds the maximum size permitted by this server.
    TooLarge,
    /// The PDS returned a non-successful status code while fetching the blob,
    /// excluding 404 which is handled by [`Self::NotFound`].
    ErrorStatusCode,
    /// The request to the PDS failed, for example due to the server being unavailable.
    FetchFailure,
    /// The blob stream was interrupted before it could be fully downloaded,
    /// for example due to the connection being unexpectedly reset.
    StreamFailed,
    /// The blob's detected MIME type is not permitted by this server.
    ForbiddenMimeType,
}

enum BlobOwnershipError {
    /// Failed to resolve the PDS for the given DID. The DID may be invalid or the
    /// resolver may be unavailable.
    DidPdsResolutionFailure,
    /// The blob could not be found in the user's repository.
    NotFound,
    /// The PDS returned a non-successful status code while fetching the blob,
    /// excluding 404 which is handled by [`Self::NotFound`].
    ErrorStatusCode,
    /// The request to the PDS failed, for example due to the server being unavailable.
    FetchFailure,
}

pub async fn get_blob_handler(
    Path((raw_did, raw_cid)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
) -> Result<axum::response::Response, (StatusCode, Json<ErrorResponse>)> {
    /// Resolve the given DID to a PDS URL and then build the `/xrpc/com.atproto.sync.getBlob` url for the DID+CID.
    #[inline]
    async fn get_blob_url(
        state: &AppState,
        did: &Did<'_>,
        cid: &Cid,
    ) -> Result<Url, IdentityError> {
        let mut url = state.identity_resolver.pds_for_did(did).await?;
        url.set_path("/xrpc/com.atproto.sync.getBlob");
        url.query_pairs_mut()
            .append_pair("did", did.as_str())
            .append_pair("cid", &cid.to_string());
        Ok(url)
    }

    let (did, cid) = (
        match Did::new_owned(raw_did.as_str()) {
            Ok(did) => did,
            Err(_) => {
                return Err((
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(ErrorResponse {
                        error: "MalformedDid",
                        message: Some("Invalid or unprocessable DID"),
                    }),
                ));
            }
        },
        match Cid::try_from(raw_cid.as_str()) {
            Ok(cid) => cid,
            Err(_) => {
                return Err((
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(ErrorResponse {
                        error: "MalformedCid",
                        message: Some("Invalid or unprocessable CID"),
                    }),
                ));
            }
        },
    );

    // Check policy for this DID+CID; concurrent requests for a key are coalesced.
    if let Some(ref policy_service_url) = state.policy_service_url {
        match state
            .cache
            .blob_policy
            .try_get_with_by_ref(&(did.clone(), cid), async {
                tracing::debug!("querying policy service for the status of blob");

                let mut policy_service_url = policy_service_url.clone();
                policy_service_url
                    .path_segments_mut()
                    .expect("policy service URL should not be a base")
                    .push(did.as_str())
                    .push(raw_cid.as_str());

                let mut request = state.internal_http_client.get(policy_service_url);
                for (name, value) in &state.policy_service_headers {
                    request = request.header(name, value);
                }

                match request.send().await {
                    Ok(response) => match response.status() {
                        StatusCode::OK => {
                            tracing::debug!("policy service returned 200 status, can serve blob");
                            Ok(CachedBlobPolicy { can_serve: true })
                        }
                        StatusCode::GONE => {
                            tracing::debug!(
                                "policy service returned 410 status, cannot serve blob"
                            );
                            Ok(CachedBlobPolicy { can_serve: false })
                        }
                        status => {
                            tracing::error!("policy service returned unexpected status: {status}");
                            Err(BlobPolicyError::UnhandledStatusCode)
                        }
                    },
                    Err(err) => {
                        tracing::error!("error occurred contacting the policy service: {err:?}");
                        Err(BlobPolicyError::FetchFailed)
                    }
                }
            })
            .await
        {
            Ok(policy) => {
                if !policy.can_serve {
                    return Err((
                        StatusCode::GONE,
                        Json(ErrorResponse {
                            error: "BlobUnavailable",
                            message: Some("Blob is not available through this service"),
                        }),
                    ));
                }
            }
            Err(_) => {
                if !state.policy_service_fail_open {
                    return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse {
                            error: "InternalServerError",
                            message: Some("Internal Server Error"),
                        }),
                    ));
                }
            }
        }
    }

    // Serve from cache, or fetch from upstream. Concurrent requests for the same key are
    // coalesced — if the initial fetch fails, the next pending request will try instead,
    // continuing until one succeeds or all have failed.
    let blob = match state
        .cache
        .blob_content
        .try_get_with_by_ref(&cid, async {
            tracing::debug!("fetching blob from PDS");
            let blob_url = get_blob_url(&state, &did, &cid).await.map_err(|err| {
                tracing::debug!("failed to resolve PDS: {err:?}");
                BlobDownloadError::DidPdsResolutionFailure
            })?;

            let validated_bytes = {
                let response = state
                    .external_http_client
                    .get(blob_url)
                    .send()
                    .await
                    .map_err(|err| {
                        tracing::warn!("failed to request blob from PDS: {err:?}");
                        BlobDownloadError::FetchFailure
                    })?;

                // Gracefully handle & abort if we do not receive a successful status code.
                if !response.status().is_success() {
                    // Note: Bluesky's PDS implemenation sends 400 instead of 404 when a blob is
                    // not found. This will skip the 404 handler and instead count as an error.
                    // This is not our responsibility to work around as other implementations do it right.
                    return Err(match response.status() {
                        StatusCode::NOT_FOUND => {
                            tracing::debug!("pds returned 404 for blob");
                            BlobDownloadError::NotFound
                        }
                        status => {
                            tracing::debug!("pds returned error status for blob: {status}");
                            BlobDownloadError::ErrorStatusCode
                        }
                    });
                }

                // Download bytes as a stream, enforcing a max size limit
                // and aborting if it's crossed.
                let bytes = bytes_stream_capped(response, state.max_blob_size)
                    .await
                    .map_err(|err| match err {
                        BytesStreamCappedError::TooLarge => {
                            tracing::debug!(
                                "blob exceeds max size of {} bytes",
                                state.max_blob_size
                            );
                            BlobDownloadError::TooLarge
                        }
                        BytesStreamCappedError::ClientError(err) => {
                            tracing::warn!("error reading blob stream: {err:?}");
                            BlobDownloadError::StreamFailed
                        }
                    })?;

                // Verify request CID matches the blob's computed CID.
                //
                // This operation is done via spawn_blocking as creating the digest will block
                // this task's executor from switching to other tasks for as long it runs.
                tokio::task::spawn_blocking({
                    let bytes = bytes.clone();
                    move || {
                        // Enabled Multihashes are set in the multihash-codetable crate features.
                        let computed_cid = match Code::try_from(cid.hash().code()) {
                            Ok(code) => Ok(Cid::new_v1(0x55, code.digest(&bytes))),
                            Err(err) => {
                                tracing::warn!("failed to compute CID: {err:?}");
                                Err(BlobDownloadError::CidUnsupportedMultihash)
                            }
                        }?;

                        if computed_cid != cid {
                            tracing::warn!("cid mismatch: computed {computed_cid} expected {cid}");
                            return Err(BlobDownloadError::CidMismatch);
                        }

                        Ok(())
                    }
                })
                .await
                .expect("CID computing task should not panic")?;

                bytes
            };

            // Infer MIME type from content bytes rather than headers; this is imperfect
            // and falls back to application/octet-stream if the type is unrecognised.
            let mime_type = match infer::get(&validated_bytes) {
                Some(m) => m
                    .mime_type()
                    .parse()
                    .expect("infer mimetype should always be valid"),
                None => mime::APPLICATION_OCTET_STREAM,
            };
            if !is_mime_allowed(&mime_type, &state.allowed_mimetypes) {
                tracing::debug!("blob was inferred to be a disallowed mime type: {mime_type}");
                return Err(BlobDownloadError::ForbiddenMimeType);
            }

            // Build reusable cached headers.
            let mut headers = HeaderMap::new();
            headers.insert(
                header::CONTENT_TYPE,
                mime_type
                    .essence_str()
                    .parse()
                    .expect("should parse mime type as header value"),
            );
            headers.insert(header::CACHE_CONTROL, state.cache_control_header.clone());
            headers.insert(
                header::CONTENT_SECURITY_POLICY,
                const { HeaderValue::from_static("default-src 'none'; sandbox") },
            );
            headers.insert(
                header::X_CONTENT_TYPE_OPTIONS,
                const { HeaderValue::from_static("nosniff") },
            );
            headers.insert(
                header::CONTENT_DISPOSITION,
                const { HeaderValue::from_static("attachment") },
            );

            // Mark this key as verified in the the ownership cache.
            state
                .cache
                .blob_ownership
                .insert((cid, did.clone()), ())
                .await;

            Ok(CachedBlobData {
                bytes: validated_bytes,
                headers,
            })
        })
        .await
    {
        Ok(blob) => blob,
        Err(err) => {
            return Err(match *err {
                BlobDownloadError::NotFound => (
                    StatusCode::NOT_FOUND,
                    Json(ErrorResponse {
                        error: "BlobNotFound",
                        message: Some("Blob not found"),
                    }),
                ),
                BlobDownloadError::TooLarge => (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    Json(ErrorResponse {
                        error: "BlobTooLarge",
                        message: Some("Blob exceeds maximum allowed size"),
                    }),
                ),
                BlobDownloadError::ForbiddenMimeType => (
                    StatusCode::FORBIDDEN,
                    Json(ErrorResponse {
                        error: "BlobForbiddenType",
                        message: Some("Content type is not allowed"),
                    }),
                ),
                BlobDownloadError::CidMismatch => (
                    StatusCode::BAD_GATEWAY,
                    Json(ErrorResponse {
                        error: "BlobCidMismatch",
                        message: Some("Blob content does not match CID"),
                    }),
                ),
                BlobDownloadError::CidUnsupportedMultihash => (
                    StatusCode::NOT_IMPLEMENTED,
                    Json(ErrorResponse {
                        error: "CidUnsupported",
                        message: Some("Unsupported CID multihash"),
                    }),
                ),
                BlobDownloadError::DidPdsResolutionFailure => (
                    StatusCode::BAD_GATEWAY,
                    Json(ErrorResponse {
                        error: "CannotResolvePds",
                        message: Some("Failed to resolve PDS for DID"),
                    }),
                ),
                BlobDownloadError::FetchFailure
                | BlobDownloadError::ErrorStatusCode
                | BlobDownloadError::StreamFailed => (
                    StatusCode::BAD_GATEWAY,
                    Json(ErrorResponse {
                        error: "BlobFetchFailed",
                        message: Some("Failed to fetch blob from PDS"),
                    }),
                ),
            });
        }
    };

    // Verify this DID owns the blob; will skip if we just fetched the blob from the same DID+CID pair.
    // Concurrent requests for the same key are coalesced.
    if let Err(err) = state
        .cache
        .blob_ownership
        .try_get_with((cid, did.clone()), async {
            tracing::debug!("verifying ownership of blob");
            let blob_url = get_blob_url(&state, &did, &cid).await.map_err(|err| {
                tracing::debug!("failed to resolve PDS url: {err:?}");
                BlobOwnershipError::DidPdsResolutionFailure
            })?;

            // Request the blob with as little of the actual body as we can.
            //
            // While some PDS implementations (bsky, tranquil) support HTTP HEAD, it is not
            // actually apart of the XRPC specification and we cannot rely on it (for now).
            // Use a range request to avoid downloading the full body on servers that support it instead.
            match state
                .external_http_client
                .get(blob_url)
                .header(
                    header::RANGE,
                    const { HeaderValue::from_static("bytes=0-1") },
                )
                .send()
                .await
                .map_err(|err| {
                    tracing::warn!("failed to request blob from PDS: {err:?}");
                    BlobOwnershipError::FetchFailure
                })?
                .status()
            {
                status if status.is_success() => {
                    tracing::debug!("verified ownership of blob");
                    Ok(())
                }
                StatusCode::NOT_FOUND | StatusCode::BAD_REQUEST => {
                    tracing::debug!("pds returned 404 for blob");
                    Err(BlobOwnershipError::NotFound)
                }
                status => {
                    tracing::debug!("pds returned error status for blob: {}", status);
                    Err(BlobOwnershipError::ErrorStatusCode)
                }
            }
        })
        .await
    {
        return Err(match *err {
            BlobOwnershipError::NotFound => (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "BlobNotFound",
                    message: Some("Blob not found"),
                }),
            ),
            BlobOwnershipError::DidPdsResolutionFailure => (
                StatusCode::BAD_GATEWAY,
                Json(ErrorResponse {
                    error: "CannotResolvePds",
                    message: Some("Failed to resolve PDS for DID"),
                }),
            ),
            BlobOwnershipError::ErrorStatusCode | BlobOwnershipError::FetchFailure => (
                StatusCode::BAD_GATEWAY,
                Json(ErrorResponse {
                    error: "BlobFetchFailed",
                    message: Some("Failed to fetch blob from PDS"),
                }),
            ),
        });
    }

    let mut response = Response::builder()
        .status(StatusCode::OK)
        .body(Body::from(blob.bytes))
        .expect("response should always build successfully");
    response.headers_mut().extend(blob.headers);
    Ok(response)
}

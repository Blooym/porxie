use crate::{
    AppState,
    cache::{CachedModerationResponse, CachedResponse},
    mime::is_mime_allowed,
};
use axum::{
    body::{Body, Bytes},
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, Response, StatusCode, header},
};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, authorization::Bearer},
};

use cid::Cid;
use futures::StreamExt;
use jacquard_common::types::did::Did;
use jacquard_identity::resolver::IdentityResolver;
use mime::Mime;
use multihash_codetable::{Code, MultihashDigest};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

pub async fn get_index_handler() -> &'static str {
    r#"
 _____                _      
|  __ \              (_)     
| |__) |__  _ ____  ___  ___ 
|  ___/ _ \| '__\ \/ / |/ _ \
| |  | (_) | |   >  <| |  __/
|_|   \___/|_|  /_/\_\_|\___|
                              
                              
A correct and efficient ATProto blob proxy service.

Links:
 - Repo:    https://codeberg.org/Blooym/porxie
 - ATProto: https://atproto.com

Routes:
 - HTTP GET /did/cid - Resolve and fetch a blob from its origin.
 - HTTP DELETE /did/cid - Invalidate blob and moderation cache for a specific blob. Requires configured bearer auth token.
"#
}

pub async fn get_blob_handler(
    Path((did, cid)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
) -> Result<axum::response::Response, (StatusCode, &'static str)> {
    let (did, cid) = (
        match Did::new(&did) {
            Ok(did) => did,
            Err(err) => {
                debug!("invalid DID '{did}': {err:?}");
                return Err((
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "Invalid or unprocessable DID",
                ));
            }
        },
        match Cid::try_from(cid.as_str()) {
            Ok(cid) => cid,
            Err(err) => {
                debug!("invalid CID '{cid}': {err:?}");
                return Err((
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "Invalid or unprocessable CID",
                ));
            }
        },
    );

    // Query moderation service (if set) to see if the blob can be served.
    //
    // Moderation queries will be made if needed, even when blob itself is cached.
    // All moderation queries will be cached for a duration to prevent flooding the upstream.
    if let Some(ref moderation_service_url) = state.moderation_service_url {
        let mod_cache_key = (did.to_string(), cid);

        let is_taken_down = match state.moderation_cache.get(&mod_cache_key).await {
            Some(cached) => cached.takendown,
            None => {
                let mut moderation_service_url = moderation_service_url.clone();
                moderation_service_url
                    .path_segments_mut()
                    .expect("moderation service URL cannot be a base")
                    .push(did.as_str())
                    .push(&cid.to_string());

                let mut request = state.internal_http_client.get(moderation_service_url);
                if let Some(ref auth) = state.moderation_service_auth_header {
                    request = request.header(reqwest::header::AUTHORIZATION, auth);
                }

                match request.send().await {
                    Ok(response) => match response.status() {
                        StatusCode::OK => {
                            state
                                .moderation_cache
                                .insert(
                                    mod_cache_key,
                                    CachedModerationResponse { takendown: false },
                                )
                                .await;
                            false
                        }
                        StatusCode::GONE => {
                            info!("moderation service rejected blob {cid} for {did}");
                            state
                                .moderation_cache
                                .insert(mod_cache_key, CachedModerationResponse { takendown: true })
                                .await;
                            true
                        }
                        status => {
                            error!("moderation service returned unexpected status: {status}");
                            if !state.moderation_service_fail_open {
                                return Err((
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                    "Internal Server Error",
                                ));
                            }
                            false
                        }
                    },
                    Err(err) => {
                        error!("failed to reach moderation service: {err:?}");
                        if !state.moderation_service_fail_open {
                            return Err((
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "Internal Server Error",
                            ));
                        }
                        false
                    }
                }
            }
        };
        if is_taken_down {
            return Err((
                StatusCode::GONE,
                "Content has been removed from the service",
            ));
        }
    }

    // Return cached content from memory if available.
    if let Some(cached) = state.response_cache.get(&cid).await {
        debug!("cache hit for {cid}");
        let mut response = Response::builder()
            .status(StatusCode::OK)
            .body(Body::from(cached.body))
            .expect("should build valid response");
        response
            .headers_mut()
            .extend(cached.headers.as_ref().clone());
        response
            .headers_mut()
            .insert("Porxie-Cache", HeaderValue::from_static("hit"));
        return Ok(response);
    }

    // Lookup PDS for the DID and return the PDS & Blob URL.
    let (pds_url, blob_url) = {
        let pds_url = match state.resolver.pds_for_did(&did).await {
            Ok(url) => url,
            Err(err) => {
                warn!("failed to resolve PDS url for '{did}': {err:?}");
                return Err((StatusCode::BAD_GATEWAY, "Failed to resolve PDS for DID"));
            }
        };
        let mut blob_url = match pds_url.join("/xrpc/com.atproto.sync.getBlob") {
            Ok(url) => url,
            Err(err) => {
                error!("failed to build XRPC URL: {err:?}");
                return Err((StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error"));
            }
        };
        blob_url.set_query(Some(&format!("did={did}&cid={cid}")));
        (pds_url, blob_url)
    };

    // Fetch and validate the requested blob.
    let blob_bytes = {
        // Fetch the blob from the PDS.
        let response = match state.external_http_client.get(blob_url).send().await {
            Ok(response) => response,
            Err(err) => {
                error!("failed to fetch blob from PDS: {err:?}");
                return Err((StatusCode::BAD_GATEWAY, "Failed to fetch blob from PDS"));
            }
        };
        if matches!(
            response.status(),
            StatusCode::NOT_FOUND | StatusCode::BAD_REQUEST
        ) {
            debug!("PDS returned 404 for blob {cid} on {did}");
            return Err((StatusCode::NOT_FOUND, "Not found"));
        }
        if !response.status().is_success() {
            warn!("PDS returned error status: {}", response.status());
            return Err((StatusCode::BAD_GATEWAY, "Failed to fetch blob from PDS"));
        }
        // Validate the size of the body making a guess based the inferred size.
        // This is strictly validated later when downloading the actual content.
        if let Some(content_length) = response.content_length()
            && content_length > state.max_blob_size
        {
            debug!("blob exceeds max size of {} bytes", state.max_blob_size);
            return Err((
                StatusCode::UNPROCESSABLE_ENTITY,
                "Blob exceeds maximum allowed size",
            ));
        };

        // Incrementally download blob content and abort if it grows too large.
        let mut buffer = Vec::with_capacity(
            response
                .content_length()
                .unwrap_or(64 * 1024)
                .min(state.max_blob_size) as usize,
        );
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(err) => {
                    warn!("error reading blob stream: {err:?}");
                    return Err((StatusCode::BAD_GATEWAY, "Failed to fetch blob from PDS"));
                }
            };
            if (buffer.len() + chunk.len()) as u64 > state.max_blob_size {
                debug!("blob exceeds max size of {} bytes", state.max_blob_size);
                return Err((
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "Blob exceeds maximum allowed size",
                ));
            }
            buffer.extend_from_slice(&chunk);
        }

        // Strictly validate the blob, computing and comparing its CID hash and best-guessing its mime-type.
        let computed_cid = match cid.hash().code() {
            0x12 => Cid::new_v1(0x55, Code::Sha2_256.digest(&buffer)),
            0x1e => Cid::new_v1(0x55, Code::Blake3_256.digest(&buffer)),
            hash => {
                warn!("unsupported multihash: 0x{hash:x}");
                return Err((
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "Unsupported CID multihash",
                ));
            }
        };
        if computed_cid != cid {
            warn!("CID mismatch: expected {cid}, computed {computed_cid}");
            return Err((StatusCode::BAD_GATEWAY, "Blob content does not match CID"));
        }

        buffer
    };

    // Loosely determine and validate mimetype. Not the strictest check, but it works.
    let mime_type: Mime = match infer::get(&blob_bytes) {
        Some(m) => m
            .mime_type()
            .parse()
            .expect("infer mimetype should always be valid"),
        None => mime::APPLICATION_OCTET_STREAM,
    };
    if !is_mime_allowed(&mime_type, &state.allowed_mimetypes) {
        debug!("blob was inferred to be a disallowed mime type: {mime_type}");
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            "Content type is not allowed",
        ));
    }

    // Build headers and cache the blob in memory for faster access.
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
    headers.insert(
        "Upstream-PDS",
        pds_url
            .host_str()
            .unwrap_or("unknown")
            .parse()
            .expect("should parse hostname as header value"),
    );
    let headers = Arc::new(headers);
    state
        .response_cache
        .insert(
            cid,
            CachedResponse {
                body: body.clone(),
                headers: Arc::clone(&headers),
            },
        )
        .await;

    // Return response.
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .body(Body::from(body))
        .expect("should build valid response");
    response.headers_mut().extend(headers.as_ref().clone());
    response
        .headers_mut()
        .insert("Porxie-Cache", HeaderValue::from_static("miss"));
    Ok(response)
}

pub async fn delete_blob_handler(
    Path((did, cid)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
    TypedHeader(Authorization(bearer)): TypedHeader<Authorization<Bearer>>,
) -> Result<StatusCode, (StatusCode, &'static str)> {
    if state.auth_token.as_deref() != Some(bearer.token()) {
        return Err((StatusCode::UNAUTHORIZED, "Unauthorized"));
    }

    let (did, cid) = (
        Did::new(&did).map_err(|_| {
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                "Invalid or unprocessable DID",
            )
        })?,
        Cid::try_from(cid.as_str()).map_err(|_| {
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                "Invalid or unprocessable CID",
            )
        })?,
    );

    state.response_cache.remove(&cid).await;
    state.moderation_cache.remove(&(did.to_string(), cid)).await;
    info!("invalidated caches for {cid} on {did}");

    Ok(StatusCode::OK)
}

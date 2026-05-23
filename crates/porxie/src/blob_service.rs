use crate::{
    identity_service::IdentityService,
    networking::{
        dns::SsrfGuardedDnsResolver,
        http::{BytesStreamCappedError, USER_AGENT, bytes_stream_capped},
    },
    types::blob_cid::BlobCid,
};
use bytes::Bytes;
use cid::Cid;
use core::{num::NonZeroU64, time::Duration};
use jacquard_common::types::did::Did;
use moka::{future::Cache as MokaCache, policy::EvictionPolicy};
use multihash_codetable::{Code, MultihashDigest};
use porxie_mediautil::deps::mime::Mime;
use porxie_mediautil::mime::{is_mime_allowed, sniff_mime};
use reqwest::{
    StatusCode,
    header::{self, HeaderValue},
    redirect,
};
use std::sync::Arc;
use thiserror::Error;
use tracing::instrument;

#[derive(Debug, Error)]
pub enum CreateBlobServiceError {
    /// An internal http client error occurred, see [`reqwest::Error`].
    #[error(transparent)]
    HttpClient(#[from] reqwest::Error),
}

#[derive(Debug, Error)]
pub enum BlobDownloadError {
    /// The blob resolver returned an error.
    #[error("blob resolver returned an error")]
    BlobResolutionFailure,
    /// The blob's computed CID does not match the requested CID.
    #[error("blob's computed CID does not match the requested CID")]
    CidMismatch,
    /// The requested CID uses an unsupported multihash algorithm.
    #[error("requested CID uses an unsupported multihash algorithm")]
    CidUnsupportedMultihash,
    /// The blob could not be found at the requested address.
    #[error("blob could not be found at the requested address")]
    NotFound,
    /// The blob exceeds the maximum size permitted by this server.
    #[error("blob exceeded the maximum size")]
    TooLarge,
    /// The origin returned a non-successful status code while fetching the blob,
    /// excluding 404 which is handled by [`Self::NotFound`].
    #[error("origin returned an unsuccessful status code")]
    ErrorStatusCode,
    /// The request to the origin failed.
    #[error("the request to the origin failed")]
    FetchFailure,
    /// The blob stream was interrupted before it could be fully downloaded,
    /// for example due to the connection being unexpectedly reset.
    #[error("the blob stream was interrupted before completion")]
    StreamFailed,
    /// The blob's detected MIME type is not permitted by this server.
    #[error("blob's mimetype was not in the allowlist")]
    ForbiddenMimeType,
}

#[derive(Debug, Error)]
pub enum BlobOwnershipError {
    /// The blob resolver returned an error.
    #[error("blob resolver returned an error")]
    BlobResolutionFailure,
    /// The blob could not be found in the user's repository.
    #[error("blob could not be found at the requested address")]
    NotFound,
    /// The origin returned a non-successful status code while fetching the blob,
    /// excluding 404 which is handled by [`Self::NotFound`].
    #[error("origin returned an unsuccessful status code")]
    ErrorStatusCode,
    /// The request to the origin failed.
    #[error("the request to the origin failed")]
    FetchFailure,
}

#[derive(Clone)]
pub struct BlobData {
    pub bytes: Bytes,
    pub mime_type: Mime,
}

pub enum BlobUrlResolver<'a> {
    Pds {
        identity_service: &'a IdentityService,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct BlobServiceOptions {
    pub data_cache_max_capacity: u64,
    pub data_cache_tti: Duration,
    pub ownership_cache_max_capacity: u64,
    pub ownership_cache_ttl: Duration,
    pub http_timeout: Duration,
}

pub struct BlobService {
    data_cache: MokaCache<BlobCid, BlobData>,
    ownership_cache: MokaCache<(BlobCid, Did<'static>), ()>,
    http_client: reqwest::Client,
}

impl BlobService {
    pub fn new(options: BlobServiceOptions) -> Result<Self, CreateBlobServiceError> {
        tracing::debug!("creating blob service with options: {options:?}");
        Ok(Self {
            data_cache: MokaCache::<BlobCid, BlobData>::builder()
                .name("blob-content")
                .eviction_policy(EvictionPolicy::tiny_lfu())
                .max_capacity(options.data_cache_max_capacity)
                .time_to_idle(options.data_cache_tti)
                .weigher(|_key, value| value.bytes.len().try_into().unwrap_or(u32::MAX))
                .build(),
            ownership_cache: MokaCache::<(BlobCid, Did<'static>), ()>::builder()
                .name("blob-ownership")
                .eviction_policy(EvictionPolicy::tiny_lfu())
                .max_capacity(options.ownership_cache_max_capacity)
                .support_invalidation_closures()
                .time_to_live(options.ownership_cache_ttl)
                .weigher(|key, _value| {
                    (key.0.encoded_len() + key.1.len())
                        .try_into()
                        .unwrap_or(u32::MAX)
                })
                .build(),
            http_client: reqwest::Client::builder()
                .brotli(true)
                .connect_timeout(Duration::from_secs(5))
                .deflate(true)
                .dns_resolver(Arc::new(SsrfGuardedDnsResolver))
                .gzip(true)
                .https_only(true)
                .redirect(redirect::Policy::limited(3))
                .timeout(options.http_timeout)
                .user_agent(USER_AGENT)
                .zstd(true)
                .build()
                .map_err(CreateBlobServiceError::HttpClient)?,
        })
    }

    /// Fetch the given blob either from the cache if available or from the upstream source.
    ///
    /// Concurrent requests for the same blob are coalesced.
    /// If the initial fetch fails, the next pending request will
    /// try instead, continuing until one succeeds or all have failed.
    #[instrument(skip_all, fields(did = %did, cid = %cid))]
    pub async fn fetch_blob(
        &self,
        did: &Did<'static>,
        cid: &BlobCid,
        url_resolver: BlobUrlResolver<'_>,
        max_blob_size: NonZeroU64,
        allowed_mimetypes: &[Mime],
    ) -> Result<BlobData, Arc<BlobDownloadError>> {
        tracing::debug!("fetching blob from origin");

        self.data_cache
            .try_get_with_by_ref(cid, async {
                let blob_url = match url_resolver {
                    BlobUrlResolver::Pds {
                        identity_service: identity_resolver,
                    } => {
                        let mut url = identity_resolver
                            .pds_for_did(did)
                            .await
                            .map_err(|_| BlobDownloadError::BlobResolutionFailure)?;
                        url.set_path("/xrpc/com.atproto.sync.getBlob");
                        url.query_pairs_mut()
                            .append_pair("did", did.as_str())
                            .append_pair("cid", &cid.to_string());
                        url
                    }
                };

                let bytes = {
                    let response = self.http_client.get(blob_url).send().await.map_err(|err| {
                        tracing::warn!("failed to request blob from origin: {err:?}");
                        BlobDownloadError::FetchFailure
                    })?;

                    // Gracefully handle & abort if we do not receive a successful status code.
                    if !response.status().is_success() {
                        return Err(match response.status() {
                            StatusCode::NOT_FOUND => {
                                tracing::debug!("origin returned 404 for blob");
                                BlobDownloadError::NotFound
                            }
                            status => {
                                tracing::debug!("origin returned error status for blob: {status}");
                                BlobDownloadError::ErrorStatusCode
                            }
                        });
                    }

                    // Download bytes as a stream, enforcing a max size limit
                    // and aborting if it's crossed.
                    let bytes = bytes_stream_capped(response, max_blob_size).await.map_err(
                        |err| match err {
                            BytesStreamCappedError::TooLarge => {
                                tracing::debug!("blob exceeds max size of {} bytes", max_blob_size);
                                BlobDownloadError::TooLarge
                            }
                            BytesStreamCappedError::ClientError(err) => {
                                tracing::warn!("error reading blob stream: {err:?}");
                                BlobDownloadError::StreamFailed
                            }
                        },
                    )?;

                    // Verify request CID matches the blob's computed CID.
                    //
                    // This operation is done via spawn_blocking as creating the digest will block
                    // this task's executor from switching to other tasks for as long it runs.
                    //
                    // Passes the bytes as a return value instead of incrementing the reference count.
                    tokio::task::spawn_blocking({
                        let cid = *cid;
                        move || {
                            // Enabled Multihashes are set in the multihash-codetable crate features.
                            let computed_cid = match Code::try_from(cid.hash().code()) {
                                Ok(code) => Ok(Cid::new_v1(
                                    0x55, // RaW codec
                                    code.digest(&bytes),
                                )),
                                Err(err) => {
                                    tracing::warn!("failed to compute CID: {err:?}");
                                    Err(BlobDownloadError::CidUnsupportedMultihash)
                                }
                            }?;

                            if computed_cid != *cid {
                                tracing::warn!(
                                    "cid mismatch: computed {computed_cid} expected {cid}"
                                );
                                return Err(BlobDownloadError::CidMismatch);
                            }

                            Ok(bytes)
                        }
                    })
                    .await
                    .expect("CID computing task should not panic")?
                };

                // Infer MIME type from content bytes rather than headers; this is fallible
                // and falls back to application/octet-stream if the type is unrecognised.
                //
                // TODO: Merge this with the download stream process to reject bad MIMEs
                // early?
                let mime_type = sniff_mime(&bytes);
                if !is_mime_allowed(&mime_type, allowed_mimetypes) {
                    tracing::debug!("blob was inferred to be a disallowed mime type: {mime_type}");
                    return Err(BlobDownloadError::ForbiddenMimeType);
                }

                // Mark this DID+CID pair as ownership-verified since we just fetched it from the origin.
                self.ownership_cache.insert((*cid, did.clone()), ()).await;

                Ok(BlobData { bytes, mime_type })
            })
            .await
    }

    /// Fetch whether the user owns the given blob either from the cache if available or the upstream source.
    ///
    /// The internal cache will be automatically populated if the blob was previously fetched from the same user.
    #[instrument(skip_all, fields(did = %did, cid = %cid))]
    pub async fn fetch_blob_ownership(
        &self,
        did: &Did<'static>,
        cid: BlobCid,
        url_resolver: BlobUrlResolver<'_>,
    ) -> Result<(), Arc<BlobOwnershipError>> {
        tracing::debug!("verifying ownership of blob");

        self.ownership_cache
            // TODO: Remove clone on DID.
            .try_get_with((cid, did.clone()), async {
                let blob_url = match url_resolver {
                    BlobUrlResolver::Pds {
                        identity_service: identity_resolver,
                    } => {
                        let mut url = identity_resolver
                            .pds_for_did(did)
                            .await
                            .map_err(|_| BlobOwnershipError::BlobResolutionFailure)?;
                        url.set_path("/xrpc/com.atproto.sync.getBlob");
                        url.query_pairs_mut()
                            .append_pair("did", did.as_str())
                            .append_pair("cid", &cid.to_string());
                        url
                    }
                };

                // Request the blob with as little of the actual body as we can.
                //
                // While some origins (bsky pds, tranquil pds) may support HTTP HEAD, it is not
                // actually a part of the XRPC specification and we cannot rely on it (for now).
                // Use a range request to avoid downloading the full body on servers that support it instead.
                match self
                    .http_client
                    .get(blob_url)
                    .header(
                        header::RANGE,
                        const { HeaderValue::from_static("bytes=0-1023") },
                    )
                    .send()
                    .await
                    .map_err(|err| {
                        tracing::warn!("failed to request blob from origin: {err:?}");
                        BlobOwnershipError::FetchFailure
                    })?
                    .status()
                {
                    status if status.is_success() => {
                        tracing::debug!("verified ownership of blob");
                        Ok(())
                    }
                    StatusCode::NOT_FOUND => {
                        tracing::debug!("origin returned 404 for blob");
                        Err(BlobOwnershipError::NotFound)
                    }
                    status => {
                        tracing::debug!("origin returned error status for blob: {}", status);
                        Err(BlobOwnershipError::ErrorStatusCode)
                    }
                }
            })
            .await
    }

    /// Invalid a specific blob cache entry.
    pub async fn invalidate_blob_cache_entry(&self, cid: &BlobCid) {
        self.data_cache.invalidate(cid).await
    }

    /// Invalidate blob ownership cache entries if they match the predicate.
    pub fn invalidate_blob_ownership_cache_entries<
        F: Fn(&(BlobCid, Did<'static>), &()) -> bool + Send + Sync + 'static,
    >(
        &self,
        predicate: F,
    ) {
        if let Err(err) = self.ownership_cache.invalidate_entries_if(predicate) {
            tracing::error!(
                "blob service has not enabled support for invalidation closures: {err:?}"
            );
        }
    }
}

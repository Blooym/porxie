use crate::networking::{dns::SsrfGuardedDnsResolver, http::USER_AGENT};
use core::{str::FromStr, time::Duration};
use jacquard_common::types::did::Did;
use jacquard_identity::{
    JacquardResolver,
    resolver::{IdentityError, IdentityResolver as _, PlcSource, ResolverOptions},
};
use moka::{future::Cache as MokaCache, policy::EvictionPolicy};
use porxie_mediautil::deps::mime;
use reqwest::{header, header::HeaderMap, header::HeaderValue, redirect};
use std::sync::Arc;
use thiserror::Error;
use tracing::instrument;
use url::Url;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CreateIdentityServiceError {
    /// An internal http client error occurred, see [`reqwest::Error`].
    #[error(transparent)]
    HttpClient(#[from] reqwest::Error),
}

#[derive(Debug, Clone)]
pub struct IdentityServiceOptions {
    /// Maximum size in memory this cache is permitted to grow to.
    pub cache_memory_allocation: u64,
    /// Time-to-live duration of items in the cache.
    pub cache_ttl: Duration,
    /// URL to the PLC directory to query for `did:plc` requests.
    pub plc_directory_url: Url,
}

pub struct IdentityService {
    cache: MokaCache<Did<'static>, Url>,
    resolver: JacquardResolver,
}

impl IdentityService {
    /// Create a new identity service.
    pub fn new(options: IdentityServiceOptions) -> Result<Self, CreateIdentityServiceError> {
        tracing::debug!("creating identity service with options: {options:?}");

        let default_headers = {
            let mut headers = HeaderMap::new();
            headers.insert(
                header::ACCEPT,
                HeaderValue::from_static(mime::APPLICATION_JSON.essence_str()),
            );
            headers
        };

        Ok(Self {
            resolver: JacquardResolver::new(
                reqwest::Client::builder()
                    .brotli(true)
                    .connect_timeout(Duration::from_secs(5))
                    .default_headers(default_headers)
                    .deflate(true)
                    .dns_resolver(Arc::new(SsrfGuardedDnsResolver))
                    .gzip(true)
                    .https_only(true)
                    .redirect(redirect::Policy::limited(3))
                    .timeout(Duration::from_secs(10))
                    .user_agent(USER_AGENT)
                    .zstd(true)
                    .build()
                    .map_err(CreateIdentityServiceError::HttpClient)?,
                ResolverOptions {
                    plc_source: PlcSource::PlcDirectory {
                        base: jacquard_common::deps::fluent_uri::Uri::from_str(
                            options.plc_directory_url.as_str(),
                        )
                        .expect("conversion between url and fluent_uri should always succeed"),
                    },
                    public_fallback_for_handle: true,
                    validate_doc_id: true,
                    request_timeout: Some(Duration::from_secs(10)),
                    ..Default::default()
                },
            ),
            cache: MokaCache::<Did<'static>, Url>::builder()
                .name("identity")
                .eviction_policy(EvictionPolicy::tiny_lfu())
                .max_capacity(options.cache_memory_allocation)
                .time_to_live(options.cache_ttl)
                .weigher(|key, value| {
                    (key.len() + value.as_str().len())
                        .try_into()
                        .unwrap_or(u32::MAX)
                })
                .build(),
        })
    }

    /// Resolve the PDS assigned by the given Did.
    ///
    /// Concurrent requests for the same key are coalesced.
    #[instrument(skip_all, fields(did = %did))]
    pub async fn pds_for_did(&self, did: &Did<'static>) -> Result<Url, Arc<IdentityError>> {
        self.cache
            .try_get_with_by_ref(did, async {
                let url = Url::parse(self.resolver.pds_for_did(did).await?.as_str())
                    .map_err(|_| IdentityError::invalid_doc("Failed to parse PDS URL"))?;

                match url.host() {
                    // Allow domains.
                    Some(url::Host::Domain(_)) => Ok(url),
                    // Reject everything else as invalid.
                    _ => Err(IdentityError::invalid_doc(
                        "document contained an invalid or missing PDS endpoint",
                    )),
                }
            })
            .await
    }

    /// Invalidate the cache entry for the given DID.
    pub async fn invalidate_cache_entry(&self, did: &Did<'static>) {
        self.cache.invalidate(did).await
    }

    /// Invalidate all cache entries.
    pub fn invalidate_cache_all(&self) {
        self.cache.invalidate_all();
    }
}

#[cfg(test)]
mod tests {
    use crate::identity_service::{IdentityService, IdentityServiceOptions};
    use jacquard_common::types::did::Did;
    use reqwest::Url;
    use std::time::Duration;

    fn make_service() -> IdentityService {
        IdentityService::new(IdentityServiceOptions {
            cache_memory_allocation: 500,
            cache_ttl: Duration::from_hours(24),
            plc_directory_url: Url::parse("https://plc.directory").unwrap(),
        })
        .expect("service constructor should be always be valid")
    }

    #[tokio::test]
    async fn resolve_and_cache() {
        let resolver = make_service();
        let did = Did::new_static("did:plc:ewvi7nxzyoun6zhxrhs64oiz")
            .expect("test did should always be valid"); // atproto.com

        // Test cold resolve and cache.
        assert!(resolver.pds_for_did(&did).await.is_ok());
        assert!(resolver.cache.contains_key(&did));

        // Test invalidation
        resolver.invalidate_cache_entry(&did).await;
        assert!(!resolver.cache.contains_key(&did));
    }

    #[tokio::test]
    async fn resolve_error_uncached() {
        let resolver = make_service();
        let did = Did::new_static("did:plc:aaaaaaaaaaaaaaaaaaaaaaaa")
            .expect("test did should always be valid");

        // Test cold resolve and cache.
        assert!(resolver.pds_for_did(&did).await.is_err());
        assert!(!resolver.cache.contains_key(&did));
    }
}

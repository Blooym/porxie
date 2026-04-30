use crate::{http::PORXIE_USER_AGENT, types::blob_cid::BlobCid};
use jacquard_common::types::did::Did;
use moka::{future::Cache as MokaCache, policy::EvictionPolicy};
use reqwest::{
    StatusCode, Url,
    header::{HeaderName, HeaderValue},
};
use std::{sync::Arc, time::Duration};
use thiserror::Error;
use tracing::instrument;

#[derive(Debug, Clone)]
pub struct PolicyDecision {
    /// Whether the service allows this blob can be served.
    pub can_serve: bool,
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CreatePolicyClientError {
    /// An internal http client error occurred, see [`reqwest::Error`].
    #[error(transparent)]
    HttpClient(#[from] reqwest::Error),
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum GetBlobPolicyError {
    /// Policy service returned an unhandled status code (Not 200 OK or 410 GONE).
    #[error("received an unhandled status code from the policy service: {0}")]
    UnhandledStatusCode(StatusCode),
    /// An internal http client error occurred, see [`reqwest::Error`].
    #[error(transparent)]
    HttpClient(#[from] reqwest::Error),
}

#[derive(Debug, Clone)]
pub struct PolicyClientOptions {
    /// Maximum size in memory this cache is permitted to grow to.
    pub cache_max_memory_allocation: u64,
    /// Time-to-live duration of items in the cache.
    pub cache_ttl: Duration,
    /// HTTP timeout to apply to all identity requests.
    pub http_timeout: Duration,
    /// HTTP connection-phase timeout to apply to all policy requests.
    pub http_connect_timeout: Duration,
    /// URL to the policy service to query.
    pub policy_service_url: Url,
    /// Additional request headers to append to each policy service request.
    pub policy_service_req_headers: Vec<(HeaderName, HeaderValue)>,
}

pub struct PolicyClient {
    cache: MokaCache<(Did<'static>, BlobCid), PolicyDecision>,
    http_client: reqwest::Client,
    policy_service_req_headers: Vec<(HeaderName, HeaderValue)>,
    policy_service_url: Url,
}

impl PolicyClient {
    /// Create a new policy client.
    pub fn new(options: PolicyClientOptions) -> Result<Self, CreatePolicyClientError> {
        tracing::debug!("creating policy service client with options: {options:?}");
        Ok(Self {
            cache: MokaCache::<(Did<'static>, BlobCid), PolicyDecision>::builder()
                .name("blob-policy")
                .weigher(|key, _value| {
                    (key.0.len() + key.1.encoded_len())
                        .try_into()
                        .unwrap_or(u32::MAX)
                })
                .eviction_policy(EvictionPolicy::tiny_lfu())
                .max_capacity(options.cache_max_memory_allocation)
                .time_to_live(options.cache_ttl)
                .support_invalidation_closures()
                .build(),
            http_client: reqwest::Client::builder()
                .user_agent(PORXIE_USER_AGENT)
                .https_only(false)
                .redirect(reqwest::redirect::Policy::limited(2))
                .gzip(true)
                .brotli(true)
                .zstd(true)
                .deflate(true)
                .connect_timeout(options.http_connect_timeout)
                .timeout(options.http_timeout)
                .build()
                .map_err(CreatePolicyClientError::HttpClient)?,
            policy_service_url: options.policy_service_url,
            policy_service_req_headers: options.policy_service_req_headers,
        })
    }

    /// Query the policy service for the policy decision of this blob.
    ///
    /// Concurrent requests for the same policy are coalesced.
    #[instrument(skip_all, fields(did = %did, cid = %cid))]
    pub async fn get_policy_for_blob(
        &self,
        did: &Did<'static>,
        cid: BlobCid,
    ) -> Result<PolicyDecision, Arc<GetBlobPolicyError>> {
        self.cache
            .try_get_with_by_ref(&(did.clone(), cid), async {
                tracing::debug!("querying policy service for the status");

                let mut policy_service_url = self.policy_service_url.clone();
                policy_service_url
                    .path_segments_mut()
                    .expect("policy service URL should not be cannot-be-a-base")
                    .push(did.as_str())
                    .push(&cid.to_string());

                let mut request = self.http_client.get(policy_service_url);
                for (name, value) in &self.policy_service_req_headers {
                    request = request.header(name, value);
                }

                match request.send().await {
                    Ok(response) => match response.status() {
                        StatusCode::OK => {
                            tracing::debug!("policy service allowed blob serving");
                            Ok(PolicyDecision { can_serve: true })
                        }
                        StatusCode::GONE => {
                            tracing::debug!("policy service forbids blob serving");
                            Ok(PolicyDecision { can_serve: false })
                        }
                        status => {
                            tracing::error!("policy service returned unexpected status: {status}");
                            Err(GetBlobPolicyError::UnhandledStatusCode(status))
                        }
                    },
                    Err(err) => {
                        tracing::error!("error occurred contacting the policy service: {err:?}");
                        Err(GetBlobPolicyError::HttpClient(err))
                    }
                }
            })
            .await
    }

    /// Invalidate cached policy decisions with the given predicate.
    pub fn invalidate_policies<
        F: Fn(&(Did<'static>, BlobCid), &PolicyDecision) -> bool + Send + Sync + 'static,
    >(
        &self,
        predicate: F,
    ) {
        if let Err(err) = self.cache.invalidate_entries_if(predicate) {
            tracing::error!(
                "policy client cache has not enabled support for invalidation closures: {err:?}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    // TODO: Create an in-process mock policy service to write tests against.
}

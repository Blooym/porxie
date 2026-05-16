use crate::{networking::http::USER_AGENT, types::blob_cid::BlobCid};
use jacquard_common::types::did::Did;
use moka::{future::Cache as MokaCache, policy::EvictionPolicy};
use porxie_lexgen::dev_blooym::porxie::get_blob_policy::{
    GetBlobPolicyOutput, GetBlobPolicyOutputPolicy,
};
use reqwest::{
    StatusCode, Url,
    header::{HeaderName, HeaderValue},
};
use std::{sync::Arc, time::Duration};
use thiserror::Error;
use tracing::instrument;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CreatePolicyClientError {
    /// An internal http client error occurred, see [`reqwest::Error`].
    #[error(transparent)]
    HttpClient(#[from] reqwest::Error),
}

#[derive(Debug, Clone)]
pub enum PolicyDecision {
    Allowed,
    Forbidden,
}

impl PolicyDecision {
    fn from_service_output(response: &GetBlobPolicyOutput) -> Self {
        match response.policy {
            GetBlobPolicyOutputPolicy::Allowed(_) => Self::Allowed,
            GetBlobPolicyOutputPolicy::Forbidden(_) => Self::Forbidden,
        }
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum GetBlobPolicyError {
    /// Policy service returned an unsuccessful status code.
    #[error("received an unsuccessful status code from the policy service: {0}")]
    StatusCode(StatusCode),

    /// An internal deserialization error occured.
    #[error(transparent)]
    Deserialize(#[from] serde_json::Error),

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
                .name("policy")
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
                .user_agent(USER_AGENT)
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

    /// Query the policy service for any policy decisions applied to this actor/blob.
    ///
    /// Concurrent requests for the same policy are coalesced.
    #[instrument(skip_all, fields(did = %did, cid = %cid))]
    pub async fn get_policy(
        &self,
        did: &Did<'static>,
        cid: BlobCid,
    ) -> Result<PolicyDecision, Arc<GetBlobPolicyError>> {
        self.cache
            .try_get_with_by_ref(&(did.clone(), cid), async {
                tracing::debug!("querying policy service for the status");

                // Build policy service URL.
                let url = {
                    let mut url = self.policy_service_url.clone();
                    url.set_path("/xrpc/dev.blooym.porxie.getBlobPolicy");
                    url.query_pairs_mut()
                        .append_pair("did", did.as_str())
                        .append_pair("cid", &cid.to_string());
                    url
                };

                // Build request.
                let mut request = self.http_client.get(url);
                // TODO: Swap this for xrpc admin authentication.
                for (name, value) in &self.policy_service_req_headers {
                    request = request.header(name, value);
                }

                // Fetch & deserialize policy data.
                match request.send().await {
                    Ok(response) => {
                        let status = response.status();
                        if !status.is_success() {
                            tracing::error!(
                                "policy service returned unsuccessful status: {status}",
                            );
                            return Err(GetBlobPolicyError::StatusCode(status));
                        }
                        match serde_json::from_slice::<GetBlobPolicyOutput>(
                            &response.bytes().await?,
                        ) {
                            Ok(output) => Ok(PolicyDecision::from_service_output(&output)),
                            Err(err) => {
                                tracing::error!(
                                    "failed to deserialize policy service response: {status}",
                                );
                                Err(GetBlobPolicyError::Deserialize(err))
                            }
                        }
                    }
                    Err(err) => {
                        tracing::error!("error occurred contacting the policy service: {err:?}");
                        Err(GetBlobPolicyError::HttpClient(err))
                    }
                }
            })
            .await
    }

    /// Invalidate cached policy entries if they match the predicate.
    pub fn invalidate_cache_entries<
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

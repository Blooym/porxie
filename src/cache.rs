use crate::types::blob_cid::BlobCid;
use anyhow::{Context, Result};
use axum::http::HeaderMap;
use bytes::Bytes;
use jacquard_common::types::did::Did;
use moka::{future::Cache as MokaCache, policy::EvictionPolicy};
use reqwest::Url;
use std::{cmp, num::NonZeroU64, time::Duration};

// Blob Content Cache

type BlobContentCache = MokaCache<BlobCid, CachedBlobData>;

#[derive(Clone)]
pub struct CachedBlobData {
    pub bytes: Bytes,
    pub headers: HeaderMap,
}

#[must_use]
fn build_blob_content_cache(mem_capacity: u64, tti: Duration) -> BlobContentCache {
    tracing::debug!(
        "building blob content cache with a mem_capacity of {mem_capacity} bytes and a tti of {}s",
        tti.as_secs()
    );

    BlobContentCache::builder()
        .name("blob-content")
        .weigher(|_key, value| {
            (value.bytes.len()
                + value
                    .headers
                    .iter()
                    .map(|(k, v)| k.as_str().len() + v.len() + 32)
                    .sum::<usize>())
            .try_into()
            .unwrap_or(u32::MAX)
        })
        .eviction_policy(EvictionPolicy::tiny_lfu())
        .max_capacity(mem_capacity)
        .time_to_idle(tti)
        .build()
}

// Blob Ownership Cache

type BlobOwnershipCache = MokaCache<(BlobCid, Did<'static>), ()>;

#[must_use]
fn build_blob_ownership_cache(mem_capacity: u64, ttl: Duration) -> BlobOwnershipCache {
    tracing::debug!(
        "building blob ownership cache with a mem_capacity of {mem_capacity} bytes and a ttl of {}s",
        ttl.as_secs()
    );

    BlobOwnershipCache::builder()
        .name("blob-ownership")
        .weigher(|key, _value| {
            (key.0.encoded_len() + key.1.len())
                .try_into()
                .unwrap_or(u32::MAX)
        })
        .eviction_policy(EvictionPolicy::tiny_lfu())
        .max_capacity(mem_capacity)
        .time_to_live(ttl)
        .support_invalidation_closures()
        .build()
}

// Policy Cache

type BlobPolicyCache = MokaCache<(Did<'static>, BlobCid), CachedBlobPolicy>;

#[derive(Debug, Copy, Clone)]
pub struct CachedBlobPolicy {
    pub can_serve: bool,
}

#[must_use]
pub fn build_blob_policy_cache(mem_capacity: u64, ttl: Duration) -> BlobPolicyCache {
    tracing::debug!(
        "building blob policy cache with a mem_capacity of {mem_capacity} bytes and a ttl of {}s",
        ttl.as_secs()
    );

    BlobPolicyCache::builder()
        .name("blob-policy")
        .weigher(|key, _value| {
            (key.0.len() + key.1.encoded_len())
                .try_into()
                .unwrap_or(u32::MAX)
        })
        .eviction_policy(EvictionPolicy::tiny_lfu())
        .max_capacity(mem_capacity)
        .time_to_live(ttl)
        .support_invalidation_closures()
        .build()
}

// Identity DID <-> PDS Cache

type IdentityCache = MokaCache<Did<'static>, Url>;

#[must_use]
pub fn build_identity_cache(mem_capacity: u64, ttl: Duration) -> IdentityCache {
    tracing::debug!(
        "building identity cache with a mem_capacity of {mem_capacity} bytes and a ttl of {}s",
        ttl.as_secs()
    );

    IdentityCache::builder()
        .name("identity")
        .weigher(|key, value| {
            (key.len() + value.as_str().len())
                .try_into()
                .unwrap_or(u32::MAX)
        })
        .eviction_policy(EvictionPolicy::tiny_lfu())
        .max_capacity(mem_capacity)
        .time_to_live(ttl)
        .support_invalidation_closures()
        .build()
}

// Builder

pub struct Caches {
    pub blob_content: BlobContentCache,
    pub blob_ownership: BlobOwnershipCache,
    pub blob_policy: BlobPolicyCache,
    pub identity: IdentityCache,
}

pub struct CacheBuildOptions {
    pub memory_capacity: NonZeroU64,
    pub blob_content_ttl: Duration,
    pub blob_ownership_ttl: Duration,
    pub blob_policy_ttl: Duration,
    pub identity_cache_ttl: Duration,
}

pub fn build_caches(options: &CacheBuildOptions) -> Result<Caches> {
    let sizes = {
        struct CacheSizes {
            pub blob: u64,
            pub ownership: u64,
            pub policy: u64,
            pub identity: u64,
        }
        let policy = cmp::min(
            (options.memory_capacity.get() as f64 * 0.06) as u64,
            48_000_000,
        ); // 6% up to 48mb max.
        let ownership = cmp::min(
            (options.memory_capacity.get() as f64 * 0.06) as u64,
            48_000_000,
        ); // 6% up to 48mb max.
        let identity = cmp::min(
            (options.memory_capacity.get() as f64 * 0.06) as u64,
            48_000_000,
        ); // 6% up to 48mb max.
        CacheSizes {
            policy,
            ownership,
            identity,
            blob: options
                .memory_capacity
                .get()
                .checked_sub(policy)
                .and_then(|r| r.checked_sub(ownership))
                .and_then(|r| r.checked_sub(identity))
                .context("cache size allocation underflow")?,
        }
    };

    Ok(Caches {
        blob_content: build_blob_content_cache(sizes.blob, options.blob_content_ttl),
        blob_ownership: build_blob_ownership_cache(sizes.ownership, options.blob_ownership_ttl),
        blob_policy: build_blob_policy_cache(sizes.policy, options.blob_policy_ttl),
        identity: build_identity_cache(sizes.identity, options.identity_cache_ttl),
    })
}

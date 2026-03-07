use anyhow::{Context, Result};
use axum::http::HeaderMap;
use bytes::Bytes;
use cid::Cid;
use jacquard_common::types::did::Did;
use moka::{future::Cache as MokaCache, policy::EvictionPolicy};
use std::{cmp, time::Duration};

// Blob Content Cache

type BlobContentCache = MokaCache<Cid, CachedBlobData>;

#[derive(Clone)]
pub struct CachedBlobData {
    pub bytes: Bytes,
    pub headers: HeaderMap,
}

fn build_blob_content_cache(mem_capacity: u64, ttl: Duration) -> BlobContentCache {
    tracing::debug!(
        "building blob content cache with a mem_capacity of {mem_capacity} bytes and a ttl of {}s",
        ttl.as_secs()
    );

    BlobContentCache::builder()
        .name("blob-content")
        .weigher(|_key, value: &CachedBlobData| -> u32 {
            (value.bytes.len() as u64 + value.headers.len() as u64 * 64)
                .try_into()
                .unwrap_or(u32::MAX)
        })
        .eviction_policy(EvictionPolicy::tiny_lfu())
        .max_capacity(mem_capacity)
        .time_to_idle(ttl)
        .build()
}

// Blob Ownership Cache

type BlobOwnershipCache = MokaCache<(Cid, Did<'static>), ()>;

fn build_blob_ownership_cache(mem_capacity: u64, ttl: Duration) -> BlobOwnershipCache {
    tracing::debug!(
        "building blob ownership cache with a mem_capacity of {mem_capacity} bytes and a ttl of {}s",
        ttl.as_secs()
    );

    BlobOwnershipCache::builder()
        .name("blob-ownership")
        .weigher(|key, _value| -> u32 {
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

type BlobPolicyCache = MokaCache<(Did<'static>, Cid), CachedBlobPolicy>;

#[derive(Debug, Copy, Clone)]
pub struct CachedBlobPolicy {
    pub can_serve: bool,
}

pub fn build_blob_policy_cache(mem_capacity: u64, ttl: Duration) -> BlobPolicyCache {
    tracing::debug!(
        "building blob policy cache with a mem_capacity of {mem_capacity} bytes and a ttl of {}s",
        ttl.as_secs()
    );

    BlobPolicyCache::builder()
        .name("blob-policy")
        .weigher(|key, _value| -> u32 {
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

// Builder

pub struct Caches {
    pub blob_content: BlobContentCache,
    pub blob_ownership: BlobOwnershipCache,
    pub blob_policy: BlobPolicyCache,
}

pub struct CacheBuildOptions {
    pub memory_capacity: u64,
    pub blob_content_ttl: Duration,
    pub blob_ownership_ttl: Duration,
    pub blob_policy_ttl: Duration,
}

pub fn build_caches(options: &CacheBuildOptions) -> Result<Caches> {
    let sizes = {
        struct CacheSizes {
            pub blob: u64,
            pub ownership: u64,
            pub policy: u64,
        }
        let policy = cmp::min((options.memory_capacity as f64 * 0.10) as u64, 68_000_000); // 10% up to 68mb max (roughly. 1mil entries)
        let ownership = cmp::min((options.memory_capacity as f64 * 0.10) as u64, 68_000_000); // 10% up to 68mb max (roughly. 1mil entries)
        CacheSizes {
            policy,
            ownership,
            blob: options
                .memory_capacity
                .checked_sub(policy)
                .and_then(|r| r.checked_sub(ownership))
                .context("cache size allocation overflow")?,
        }
    };

    Ok(Caches {
        blob_content: build_blob_content_cache(sizes.blob, options.blob_content_ttl),
        blob_ownership: build_blob_ownership_cache(sizes.ownership, options.blob_ownership_ttl),
        blob_policy: build_blob_policy_cache(sizes.policy, options.blob_policy_ttl),
    })
}

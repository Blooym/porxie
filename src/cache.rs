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

fn build_blob_content_cache(mem_capacity: u64) -> BlobContentCache {
    BlobContentCache::builder()
        .weigher(|_key, value: &CachedBlobData| -> u32 {
            (value.bytes.len() as u64 + value.headers.len() as u64 * 64)
                .try_into()
                .unwrap_or(u32::MAX)
        })
        .eviction_policy(EvictionPolicy::tiny_lfu())
        .max_capacity(mem_capacity)
        .build()
}

// Blob Ownership Cache

type BlobOwnershipCache = MokaCache<(Cid, Did<'static>), ()>;

fn build_blob_ownership_cache(mem_capacity: u64) -> BlobOwnershipCache {
    BlobOwnershipCache::builder()
        .weigher(|key, _value| -> u32 {
            (key.0.encoded_len() + key.1.len())
                .try_into()
                .unwrap_or(u32::MAX)
        })
        .eviction_policy(EvictionPolicy::tiny_lfu())
        .max_capacity(mem_capacity)
        .time_to_live(Duration::from_hours(1))
        .support_invalidation_closures()
        .build()
}

// Policy Cache

type PolicyCache = MokaCache<(Did<'static>, Cid), CachedPolicy>;

#[derive(Debug, Copy, Clone)]
pub struct CachedPolicy {
    pub can_serve: bool,
}

pub fn build_policy_cache(mem_capacity: u64, ttl: Duration) -> PolicyCache {
    PolicyCache::builder()
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
    pub content: BlobContentCache,
    pub ownership: BlobOwnershipCache,
    pub policy: PolicyCache,
}

pub struct CacheBuildOptions {
    pub memory_capacity: u64,
    pub policy_ttl: Duration,
}

pub fn build_caches(options: &CacheBuildOptions) -> Result<Caches> {
    let sizes = {
        #[derive(Debug)]
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

    tracing::debug!("Building with {sizes:?}",);

    Ok(Caches {
        content: build_blob_content_cache(sizes.blob),
        ownership: build_blob_ownership_cache(sizes.ownership),
        policy: build_policy_cache(sizes.policy, options.policy_ttl),
    })
}

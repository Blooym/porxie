use axum::{body::Bytes, http::HeaderMap};
use cid::Cid;
use moka::{future::Cache as MokaCache, policy::EvictionPolicy};
use std::{sync::Arc, time::Duration};

pub type ResponseCache = MokaCache<Cid, CachedResponse>;
pub type ModerationCache = MokaCache<(String, Cid), CachedModerationResponse>;

#[derive(Debug, Clone)]
pub struct CachedResponse {
    pub body: Bytes,
    pub headers: Arc<HeaderMap>,
}

#[derive(Debug, Copy, Clone)]
pub struct CachedModerationResponse {
    pub takendown: bool,
}

pub fn build_response_cache(max_capacity: u64) -> ResponseCache {
    ResponseCache::builder()
        .weigher(|_key, value: &CachedResponse| -> u32 {
            (value.body.len() as u64 + value.headers.len() as u64 * 64)
                .try_into()
                .unwrap_or(u32::MAX)
        })
        .eviction_policy(EvictionPolicy::tiny_lfu())
        .max_capacity(max_capacity)
        .build()
}

pub fn build_moderation_cache(max_capacity: u64, ttl: Duration) -> ModerationCache {
    ModerationCache::builder()
        .weigher(|key, _value| -> u32 {
            (key.0.len() + key.1.encoded_len())
                .try_into()
                .unwrap_or(u32::MAX)
        })
        .time_to_live(ttl)
        .eviction_policy(EvictionPolicy::tiny_lfu())
        .max_capacity(max_capacity)
        .build()
}

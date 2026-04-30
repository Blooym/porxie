use std::{cmp, num::NonZeroU64};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ComputeCacheSizeError {
    #[error("cache size underflowed capacity")]
    AllocationUnderflow,
}

pub struct CacheSizes {
    pub blob: u64,
    pub ownership: u64,
    pub policy: u64,
    pub identity: u64,
}

pub fn compute_cache_sizes(
    memory_capacity: NonZeroU64,
) -> Result<CacheSizes, ComputeCacheSizeError> {
    let policy = cmp::min((memory_capacity.get() as f64 * 0.10) as u64, 48_000_000); // 10% up to 48mb max.
    let ownership = cmp::min((memory_capacity.get() as f64 * 0.10) as u64, 48_000_000); // 10% up to 48mb max
    let identity = cmp::min((memory_capacity.get() as f64 * 0.10) as u64, 48_000_000); // 10% up to 48mb max.
    Ok(CacheSizes {
        policy,
        ownership,
        identity,
        blob: memory_capacity
            .get()
            .checked_sub(policy)
            .and_then(|r| r.checked_sub(ownership))
            .and_then(|r| r.checked_sub(identity))
            .ok_or(ComputeCacheSizeError::AllocationUnderflow)?,
    })
}

use std::time::Duration;

use crate::CacheEntry;
use crate::CacheStats;

/// Trait for cache backends.
#[async_trait::async_trait]
pub trait CacheBackend: Send + Sync {
    type K: Send + Sync;
    type V: Clone + Send + Sync;

    /// Get a value from the cache.
    async fn get(&self, key: &Self::K) -> Result<Option<CacheEntry<Self::V>>, crate::CacheError>;

    /// Insert a value into the cache.
    async fn insert(&self, key: Self::K, value: Self::V) -> Result<(), crate::CacheError>;

    /// Remove a value from the cache.
    async fn remove(&self, key: &Self::K) -> Result<Option<Self::V>, crate::CacheError>;

    /// Clear all entries from the cache.
    async fn clear(&self) -> Result<(), crate::CacheError>;

    /// Get cache statistics.
    async fn stats(&self) -> Result<CacheStats, crate::CacheError>;
}

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::CacheEntry;
use crate::CacheStats;

/// In-memory cache backend using `moka`.
pub struct InMemoryBackend<K, V> {
    cache: moka::future::Cache<K, CacheEntry<V>>,
    hits: AtomicU64,
    misses: AtomicU64,
}

impl<K, V> InMemoryBackend<K, V>
where
    K: std::hash::Hash + Eq + Send + Sync + Clone + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// Create a new in-memory cache with the given capacity and TTL.
    pub fn new(capacity: u64, ttl: Duration) -> Self {
        let cache = moka::future::Cache::builder()
            .max_capacity(capacity)
            .time_to_live(ttl)
            .build();

        Self {
            cache,
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        }
    }

    /// Create a new in-memory cache with TTI (time-to-idle) in addition to TTL.
    pub fn with_tti(capacity: u64, ttl: Duration, tti: Duration) -> Self {
        let cache = moka::future::Cache::builder()
            .max_capacity(capacity)
            .time_to_live(ttl)
            .time_to_idle(tti)
            .build();

        Self {
            cache,
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        }
    }
}

#[async_trait::async_trait]
impl<K, V> crate::CacheBackend for InMemoryBackend<K, V>
where
    K: std::hash::Hash + Eq + Send + Sync + Clone + 'static,
    V: Clone + Send + Sync + 'static,
{
    type K = K;
    type V = V;

    async fn get(&self, key: &K) -> Result<Option<CacheEntry<V>>, crate::CacheError> {
        match self.cache.get(key).await {
            Some(entry) => {
                self.hits.fetch_add(1, Ordering::Relaxed);
                #[cfg(feature = "metrics")]
                metrics::counter!("cachekit_hits_total").increment(1);
                Ok(Some(entry))
            }
            None => {
                self.misses.fetch_add(1, Ordering::Relaxed);
                #[cfg(feature = "metrics")]
                metrics::counter!("cachekit_misses_total").increment(1);
                Ok(None)
            }
        }
    }

    async fn insert(&self, key: K, value: V) -> Result<(), crate::CacheError> {
        let entry = CacheEntry {
            value,
            created_at: std::time::Instant::now(),
            expires_at: None,
            max_age_at: None,
            stale_until: None,
        };
        self.cache.insert(key, entry).await;
        Ok(())
    }

    async fn insert_with_swr(
        &self,
        key: K,
        value: V,
        max_age: Duration,
        stale_while_revalidate: Duration,
    ) -> Result<(), crate::CacheError> {
        let now = std::time::Instant::now();
        let entry = CacheEntry {
            value,
            created_at: now,
            expires_at: Some(now + max_age + stale_while_revalidate),
            max_age_at: Some(now + max_age),
            stale_until: Some(now + max_age + stale_while_revalidate),
        };
        self.cache.insert(key, entry).await;
        Ok(())
    }

    async fn remove(&self, key: &K) -> Result<Option<V>, crate::CacheError> {
        Ok(self.cache.remove(key).await.map(|e| e.value))
    }

    async fn clear(&self) -> Result<(), crate::CacheError> {
        self.cache.invalidate_all();
        Ok(())
    }

    async fn stats(&self) -> Result<crate::CacheStats, crate::CacheError> {
        let hits = self.hits.load(Ordering::Relaxed);
        let misses = self.misses.load(Ordering::Relaxed);
        let total = hits + misses;
        let hit_rate = if total > 0 {
            hits as f64 / total as f64
        } else {
            0.0
        };
        let size = self.cache.entry_count();

        #[cfg(feature = "metrics")]
        metrics::histogram!("cachekit_size").record(size as f64);

        Ok(CacheStats {
            hits,
            misses,
            hit_rate,
            size,
        })
    }
}

#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! # cachekit
//!
//! Unified caching for Rust — in-memory (moka) and Redis backends with TTL,
//! stale-while-revalidate, and eviction listeners.
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use cachekit::{Cache, InMemoryBackend};
//!
//! # #[tokio::main]
//! # async fn main() {
//! let backend = InMemoryBackend::new(10_000, std::time::Duration::from_secs(300));
//! let cache = Cache::new(backend);
//!
//! // Insert a value
//! cache.insert("key", "value").await.unwrap();
//!
//! // Get a value
//! if let Some(entry) = cache.get(&"key").await.unwrap() {
//!     println!("Got value: {}", entry.value);
//! }
//! # }
//! ```
//!
//! ## Backends
//!
//! | Backend | Feature | Persistent | Distributed | Default |
//! |---------|---------|------------|-------------|---------|
//! | `moka` | `in-memory` | No | No | Yes |
//! | `redis` | `redis` | Yes | Yes | No |
//! | `dashmap` | `dashmap` | No | No | No |

use std::time::Duration;

mod backend;
mod error;
mod stats;

/// In-memory cache backend using moka.
#[cfg(feature = "in-memory")]
pub mod in_memory;

#[cfg(feature = "redis")]
/// Redis cache backend.
#[path = "redis.rs"]
pub mod redis_backend;

#[cfg(feature = "sqlite")]
/// SQLite cache backend.
pub mod sqlite;

pub use backend::CacheBackend;
pub use error::CacheError;
pub use stats::CacheStats;

#[cfg(feature = "in-memory")]
pub use in_memory::InMemoryBackend;

#[cfg(feature = "redis")]
pub use redis_backend::RedisBackend;

#[cfg(feature = "sqlite")]
pub use sqlite::SqliteBackend;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn cache_stats_default() {
        let stats = CacheStats::default();
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);
        assert_eq!(stats.hit_rate, 0.0);
        assert_eq!(stats.size, 0);
        assert!(stats.is_empty());
        assert_eq!(stats.total_lookups(), 0);
    }

    #[test]
    fn cache_stats_hit_rate_calculation() {
        let stats = CacheStats {
            hits: 75,
            misses: 25,
            hit_rate: 0.75,
            size: 100,
        };
        assert!((stats.hit_rate - 0.75).abs() < f64::EPSILON);
        assert_eq!(stats.total_lookups(), 100);
        assert!(!stats.is_empty());
    }

    #[test]
    fn cache_stats_display() {
        let stats = CacheStats {
            hits: 10,
            misses: 5,
            hit_rate: 0.6667,
            size: 15,
        };
        let display = stats.to_string();
        assert!(display.contains("hits: 10"));
        assert!(display.contains("misses: 5"));
        assert!(display.contains("66.7%"));
        assert!(display.contains("size: 15"));
    }

    #[test]
    fn cache_entry_creation_and_age() {
        let entry = CacheEntry {
            value: "hello".to_string(),
            created_at: std::time::Instant::now(),
            expires_at: None,
            max_age_at: None,
            stale_until: None,
        };
        assert!(!entry.is_expired());
        assert!(entry.age() < Duration::from_millis(100));
        assert!(entry.remaining_ttl().is_none());
    }

    #[test]
    fn cache_entry_with_expiry() {
        let entry = CacheEntry {
            value: 42,
            created_at: std::time::Instant::now(),
            expires_at: Some(std::time::Instant::now() + Duration::from_secs(10)),
            max_age_at: None,
            stale_until: None,
        };
        assert!(!entry.is_expired());
        assert!(entry.remaining_ttl().is_some());
        let ttl = entry.remaining_ttl().unwrap();
        assert!(ttl > Duration::from_secs(9) && ttl <= Duration::from_secs(10));
    }

    #[test]
    fn cache_entry_expired() {
        let entry = CacheEntry {
            value: "old",
            created_at: std::time::Instant::now() - Duration::from_secs(100),
            expires_at: Some(std::time::Instant::now() - Duration::from_secs(1)),
            max_age_at: None,
            stale_until: None,
        };
        assert!(entry.is_expired());
        assert_eq!(entry.remaining_ttl(), Some(Duration::ZERO));
    }

    #[test]
    fn cache_error_display() {
        let err = CacheError::Serialization("bad json".to_string());
        assert_eq!(err.to_string(), "serialization error: bad json");

        let err = CacheError::Backend("redis down".to_string());
        assert_eq!(err.to_string(), "backend error: redis down");

        let err = CacheError::Expired;
        assert_eq!(err.to_string(), "cache entry expired");

        let err = CacheError::Full;
        assert_eq!(err.to_string(), "cache full");

        let err = CacheError::Other("something".to_string());
        assert_eq!(err.to_string(), "cache error: something");
    }

    #[tokio::test]
    async fn in_memory_cache_insert_and_get() {
        let backend = InMemoryBackend::new(100, Duration::from_secs(60));
        let cache = Cache::new(backend);

        cache.insert("key1", "value1".to_string()).await.unwrap();
        let entry = cache.get(&"key1").await.unwrap();
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().value, "value1");
    }

    #[tokio::test]
    async fn in_memory_cache_miss() {
        let backend: InMemoryBackend<&str, i32> = InMemoryBackend::new(100, Duration::from_secs(60));
        let cache = Cache::new(backend);

        let entry = cache.get(&"nonexistent").await.unwrap();
        assert!(entry.is_none());
    }

    #[tokio::test]
    async fn in_memory_cache_remove() {
        let backend = InMemoryBackend::new(100, Duration::from_secs(60));
        let cache = Cache::new(backend);

        cache.insert("k", "v".to_string()).await.unwrap();
        let removed = cache.remove(&"k").await.unwrap();
        assert_eq!(removed, Some("v".to_string()));
        assert!(cache.get(&"k").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn in_memory_cache_stats() {
        let backend = InMemoryBackend::new(100, Duration::from_secs(60));
        let cache = Cache::new(backend);

        cache.insert("a", 1).await.unwrap();
        let _ = cache.get(&"a").await.unwrap(); // hit
        let _ = cache.get(&"b").await.unwrap(); // miss

        let stats = cache.stats().await.unwrap();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
        assert!((stats.hit_rate - 0.5).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn in_memory_cache_clear() {
        let backend = InMemoryBackend::new(100, Duration::from_secs(60));
        let cache = Cache::new(backend);

        cache.insert("x", 10).await.unwrap();
        cache.clear().await.unwrap();
        assert!(cache.get(&"x").await.unwrap().is_none());
    }

    #[cfg(feature = "sqlite")]
    mod sqlite_tests {
        use super::*;

        #[tokio::test]
        async fn sqlite_insert_and_get() {
            let backend = crate::SqliteBackend::in_memory().unwrap();
            let cache = Cache::new(backend);

            cache.insert("key1".to_string(), b"value1".to_vec()).await.unwrap();
            let entry = cache.get(&"key1".to_string()).await.unwrap();
            assert!(entry.is_some());
            assert_eq!(entry.unwrap().value, b"value1");
        }

        #[tokio::test]
        async fn sqlite_miss() {
            let backend = crate::SqliteBackend::in_memory().unwrap();
            let cache = Cache::new(backend);

            let entry = cache.get(&"nonexistent".to_string()).await.unwrap();
            assert!(entry.is_none());
        }

        #[tokio::test]
        async fn sqlite_remove() {
            let backend = crate::SqliteBackend::in_memory().unwrap();
            let cache = Cache::new(backend);

            cache.insert("k".to_string(), b"v".to_vec()).await.unwrap();
            let removed = cache.remove(&"k".to_string()).await.unwrap();
            assert_eq!(removed, Some(b"v".to_vec()));
            assert!(cache.get(&"k".to_string()).await.unwrap().is_none());
        }

        #[tokio::test]
        async fn sqlite_remove_nonexistent() {
            let backend = crate::SqliteBackend::in_memory().unwrap();
            let cache = Cache::new(backend);

            let removed = cache.remove(&"nope".to_string()).await.unwrap();
            assert_eq!(removed, None);
        }

        #[tokio::test]
        async fn sqlite_clear() {
            let backend = crate::SqliteBackend::in_memory().unwrap();
            let cache = Cache::new(backend);

            cache.insert("a".to_string(), b"1".to_vec()).await.unwrap();
            cache.insert("b".to_string(), b"2".to_vec()).await.unwrap();
            cache.clear().await.unwrap();
            assert!(cache.get(&"a".to_string()).await.unwrap().is_none());
            assert!(cache.get(&"b".to_string()).await.unwrap().is_none());
        }

        #[tokio::test]
        async fn sqlite_stats() {
            let backend = crate::SqliteBackend::in_memory().unwrap();
            let cache = Cache::new(backend);

            let stats = cache.stats().await.unwrap();
            assert_eq!(stats.size, 0);

            cache.insert("x".to_string(), b"y".to_vec()).await.unwrap();
            let stats = cache.stats().await.unwrap();
            assert_eq!(stats.size, 1);
        }

        #[tokio::test]
        async fn sqlite_overwrite() {
            let backend = crate::SqliteBackend::in_memory().unwrap();
            let cache = Cache::new(backend);

            cache.insert("k".to_string(), b"v1".to_vec()).await.unwrap();
            cache.insert("k".to_string(), b"v2".to_vec()).await.unwrap();
            let entry = cache.get(&"k".to_string()).await.unwrap().unwrap();
            assert_eq!(entry.value, b"v2");
        }

        #[tokio::test]
        async fn sqlite_insert_with_swr() {
            let backend = crate::SqliteBackend::in_memory().unwrap();
            let cache = Cache::new(backend);

            cache
                .insert_with_swr(
                    "k".to_string(),
                    b"v".to_vec(),
                    Duration::from_secs(60),
                    Duration::from_secs(30),
                )
                .await
                .unwrap();

            let entry = cache.get(&"k".to_string()).await.unwrap();
            assert!(entry.is_some());
            let entry = entry.unwrap();
            assert!(entry.expires_at.is_some());
            assert!(entry.max_age_at.is_some());
            assert!(entry.stale_until.is_some());
        }

        #[tokio::test]
        async fn sqlite_persistence() {
            let dir = std::env::temp_dir().join("cachekit_test_persist");
            let db_path = dir.join("test.db");
            let _ = std::fs::remove_file(&db_path);
            let _ = std::fs::create_dir_all(&dir);

            {
                let backend = crate::SqliteBackend::new(&db_path).unwrap();
                let cache = Cache::new(backend);
                cache.insert("persist".to_string(), b"data".to_vec()).await.unwrap();
            }

            {
                let backend = crate::SqliteBackend::new(&db_path).unwrap();
                let cache = Cache::new(backend);
                let entry = cache.get(&"persist".to_string()).await.unwrap();
                assert!(entry.is_some());
                assert_eq!(entry.unwrap().value, b"data");
            }

            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

/// A unified cache interface.
pub struct Cache<K, V> {
    backend: Box<dyn CacheBackend<K = K, V = V>>,
}

impl<K, V> Cache<K, V>
where
    K: Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// Create a new cache with the given backend.
    pub fn new(backend: impl CacheBackend<K = K, V = V> + 'static) -> Self {
        Self {
            backend: Box::new(backend),
        }
    }

    /// Get a value from the cache.
    pub async fn get(&self, key: &K) -> Result<Option<CacheEntry<V>>, CacheError> {
        let result = self.backend.get(key).await;
        #[cfg(feature = "tracing")]
        match &result {
            Ok(Some(_)) => tracing::trace!("cache hit"),
            Ok(None) => tracing::debug!("cache miss"),
            Err(e) => tracing::debug!(error = %e, "cache get error"),
        }
        result
    }

    /// Insert a value into the cache.
    pub async fn insert(&self, key: K, value: V) -> Result<(), CacheError> {
        #[cfg(feature = "tracing")]
        tracing::trace!("cache insert");
        self.backend.insert(key, value).await
    }

    /// Remove a value from the cache.
    pub async fn remove(&self, key: &K) -> Result<Option<V>, CacheError> {
        let result = self.backend.remove(key).await;
        #[cfg(feature = "tracing")]
        match &result {
            Ok(Some(_)) => tracing::info!("cache eviction (remove)"),
            Ok(None) => tracing::debug!("cache remove: not found"),
            Err(e) => tracing::debug!(error = %e, "cache remove error"),
        }
        result
    }

    /// Clear all entries from the cache.
    pub async fn clear(&self) -> Result<(), CacheError> {
        #[cfg(feature = "tracing")]
        tracing::info!("cache cleared");
        self.backend.clear().await
    }

    /// Get cache statistics.
    pub async fn stats(&self) -> Result<CacheStats, CacheError> {
        self.backend.stats().await
    }

    /// Insert a value with stale-while-revalidate semantics.
    ///
    /// - `max_age`: Duration after which the entry becomes stale.
    /// - `stale_while_revalidate`: Duration after `max_age` during which the stale
    ///   value is still returned while the caller refreshes in the background.
    pub async fn insert_with_swr(
        &self,
        key: K,
        value: V,
        max_age: Duration,
        stale_while_revalidate: Duration,
    ) -> Result<(), CacheError> {
        self.backend
            .insert_with_swr(key, value, max_age, stale_while_revalidate)
            .await
    }
}

/// A cached entry with metadata.
#[derive(Debug, Clone)]
pub struct CacheEntry<V> {
    /// The cached value.
    pub value: V,
    /// When the entry was created.
    pub created_at: std::time::Instant,
    /// When the entry expires.
    pub expires_at: Option<std::time::Instant>,
    /// When the entry becomes stale (after max_age has elapsed).
    pub max_age_at: Option<std::time::Instant>,
    /// When the stale entry should stop being served (end of stale_while_revalidate window).
    pub stale_until: Option<std::time::Instant>,
}

impl<V> CacheEntry<V> {
    /// Returns `true` if the entry has expired.
    pub fn is_expired(&self) -> bool {
        self.expires_at
            .map(|exp| std::time::Instant::now() >= exp)
            .unwrap_or(false)
    }

    /// Returns `true` if the entry is stale (past max_age but within stale_while_revalidate window).
    pub fn is_stale(&self) -> bool {
        let now = std::time::Instant::now();
        match self.max_age_at {
            Some(max_age) => {
                if now < max_age {
                    return false;
                }
                // Past max_age — check if within stale window
                match self.stale_until {
                    Some(until) => now < until,
                    None => true, // No stale window limit, always stale after max_age
                }
            }
            None => false, // No max_age set, never stale
        }
    }

    /// Returns the age of the entry.
    pub fn age(&self) -> std::time::Duration {
        self.created_at.elapsed()
    }

    /// Returns the remaining TTL, if any.
    pub fn remaining_ttl(&self) -> Option<std::time::Duration> {
        self.expires_at.map(|exp| {
            exp.checked_duration_since(std::time::Instant::now())
                .unwrap_or_default()
        })
    }
}

#![forbid(unsafe_code)]

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

mod backend;
mod error;
mod stats;

#[cfg(feature = "in-memory")]
pub mod in_memory;

pub use backend::CacheBackend;
pub use error::CacheError;
pub use stats::CacheStats;

#[cfg(feature = "in-memory")]
pub use in_memory::InMemoryBackend;

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
        self.backend.get(key).await
    }

    /// Insert a value into the cache.
    pub async fn insert(&self, key: K, value: V) -> Result<(), CacheError> {
        self.backend.insert(key, value).await
    }

    /// Remove a value from the cache.
    pub async fn remove(&self, key: &K) -> Result<Option<V>, CacheError> {
        self.backend.remove(key).await
    }

    /// Clear all entries from the cache.
    pub async fn clear(&self) -> Result<(), CacheError> {
        self.backend.clear().await
    }

    /// Get cache statistics.
    pub async fn stats(&self) -> Result<CacheStats, CacheError> {
        self.backend.stats().await
    }
}

/// A cached entry with metadata.
#[derive(Debug, Clone)]
pub struct CacheEntry<V> {
    pub value: V,
    pub created_at: std::time::Instant,
    pub expires_at: Option<std::time::Instant>,
}

impl<V> CacheEntry<V> {
    /// Returns `true` if the entry has expired.
    pub fn is_expired(&self) -> bool {
        self.expires_at
            .map(|exp| std::time::Instant::now() >= exp)
            .unwrap_or(false)
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

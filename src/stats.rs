use std::fmt;

/// Cache statistics.
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Number of cache hits.
    pub hits: u64,
    /// Number of cache misses.
    pub misses: u64,
    /// Hit rate (0.0 - 1.0).
    pub hit_rate: f64,
    /// Current number of entries in the cache.
    pub size: u64,
}

impl CacheStats {
    /// Returns `true` if the cache has no entries.
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    /// Returns the total number of lookups (hits + misses).
    pub fn total_lookups(&self) -> u64 {
        self.hits + self.misses
    }
}

impl fmt::Display for CacheStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "CacheStats {{ hits: {}, misses: {}, hit_rate: {:.1}%, size: {} }}",
            self.hits,
            self.misses,
            self.hit_rate * 100.0,
            self.size
        )
    }
}

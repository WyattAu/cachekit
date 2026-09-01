//! Integration tests for the cachekit (cache-pal) crate.
//!
//! Tests CacheStats calculation, CacheEntry creation/expiry,
//! InMemoryBackend insert/get/remove, and CacheError display.

use std::time::Duration;

use cache_pal::{Cache, CacheError, CacheEntry, CacheStats, InMemoryBackend};

// ---------------------------------------------------------------------------
// CacheStats calculation
// ---------------------------------------------------------------------------

#[test]
fn stats_default_all_zero() {
    let s = CacheStats::default();
    assert_eq!(s.hits, 0);
    assert_eq!(s.misses, 0);
    assert_eq!(s.hit_rate, 0.0);
    assert_eq!(s.size, 0);
    assert!(s.is_empty());
    assert_eq!(s.total_lookups(), 0);
}

#[test]
fn stats_total_lookups_sums_hits_and_misses() {
    let s = CacheStats {
        hits: 10,
        misses: 5,
        hit_rate: 10.0 / 15.0,
        size: 15,
    };
    assert_eq!(s.total_lookups(), 15);
    assert!(!s.is_empty());
}

#[test]
fn stats_display_format() {
    let s = CacheStats {
        hits: 42,
        misses: 8,
        hit_rate: 0.84,
        size: 50,
    };
    let display = s.to_string();
    assert!(display.contains("hits: 42"));
    assert!(display.contains("misses: 8"));
    assert!(display.contains("84.0%"));
    assert!(display.contains("size: 50"));
}

#[test]
fn stats_hit_rate_boundary_zero() {
    let s = CacheStats {
        hits: 0,
        misses: 0,
        hit_rate: 0.0,
        size: 0,
    };
    assert_eq!(s.hit_rate, 0.0);
    assert_eq!(s.total_lookups(), 0);
}

#[test]
fn stats_hit_rate_boundary_one() {
    let s = CacheStats {
        hits: 100,
        misses: 0,
        hit_rate: 1.0,
        size: 100,
    };
    assert_eq!(s.hit_rate, 1.0);
    assert_eq!(s.total_lookups(), 100);
}

// ---------------------------------------------------------------------------
// CacheEntry creation and expiry
// ---------------------------------------------------------------------------

#[test]
fn entry_no_expiry_never_expired() {
    let entry: CacheEntry<&str> = CacheEntry {
        value: "data",
        created_at: std::time::Instant::now(),
        expires_at: None,
        max_age_at: None,
        stale_until: None,
    };
    assert!(!entry.is_expired());
    assert!(!entry.is_stale());
    assert!(entry.remaining_ttl().is_none());
    assert!(entry.age() < Duration::from_millis(100));
}

#[test]
fn entry_with_future_expiry_not_expired() {
    let entry = CacheEntry {
        value: 42i32,
        created_at: std::time::Instant::now(),
        expires_at: Some(std::time::Instant::now() + Duration::from_secs(60)),
        max_age_at: None,
        stale_until: None,
    };
    assert!(!entry.is_expired());
    let ttl = entry.remaining_ttl().unwrap();
    assert!(ttl > Duration::from_secs(59) && ttl <= Duration::from_secs(60));
}

#[test]
fn entry_with_past_expiry_is_expired() {
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
fn entry_stale_when_past_max_age() {
    let entry = CacheEntry {
        value: "stale-ok",
        created_at: std::time::Instant::now() - Duration::from_secs(10),
        expires_at: Some(std::time::Instant::now() + Duration::from_secs(60)),
        max_age_at: Some(std::time::Instant::now() - Duration::from_secs(1)),
        stale_until: Some(std::time::Instant::now() + Duration::from_secs(60)),
    };
    assert!(entry.is_stale());
    assert!(!entry.is_expired());
}

#[test]
fn entry_not_stale_before_max_age() {
    let entry = CacheEntry {
        value: "fresh",
        created_at: std::time::Instant::now(),
        expires_at: None,
        max_age_at: Some(std::time::Instant::now() + Duration::from_secs(60)),
        stale_until: None,
    };
    assert!(!entry.is_stale());
}

#[test]
fn entry_stale_without_stale_until() {
    let entry = CacheEntry {
        value: "no-limit",
        created_at: std::time::Instant::now() - Duration::from_secs(10),
        expires_at: None,
        max_age_at: Some(std::time::Instant::now() - Duration::from_secs(1)),
        stale_until: None,
    };
    // Past max_age but no stale_until limit => always stale
    assert!(entry.is_stale());
}

#[test]
fn entry_no_max_age_never_stale() {
    let entry = CacheEntry {
        value: "no-max-age",
        created_at: std::time::Instant::now(),
        expires_at: None,
        max_age_at: None,
        stale_until: Some(std::time::Instant::now() + Duration::from_secs(60)),
    };
    assert!(!entry.is_stale());
}

// ---------------------------------------------------------------------------
// InMemoryBackend insert / get / remove
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cache_insert_and_get() {
    let backend: InMemoryBackend<&str, String> = InMemoryBackend::new(100, Duration::from_secs(60));
    let cache = Cache::new(backend);

    cache.insert("k1", "v1".to_string()).await.unwrap();
    let entry = cache.get(&"k1").await.unwrap();
    assert!(entry.is_some());
    assert_eq!(entry.unwrap().value, "v1");
}

#[tokio::test]
async fn cache_get_miss() {
    let backend: InMemoryBackend<&str, String> = InMemoryBackend::new(100, Duration::from_secs(60));
    let cache = Cache::new(backend);

    let entry = cache.get(&"nonexistent").await.unwrap();
    assert!(entry.is_none());
}

#[tokio::test]
async fn cache_remove_existing() {
    let backend: InMemoryBackend<&str, String> = InMemoryBackend::new(100, Duration::from_secs(60));
    let cache = Cache::new(backend);

    cache.insert("k", "v".to_string()).await.unwrap();
    let removed = cache.remove(&"k").await.unwrap();
    assert_eq!(removed, Some("v".to_string()));
    assert!(cache.get(&"k").await.unwrap().is_none());
}

#[tokio::test]
async fn cache_remove_nonexistent() {
    let backend: InMemoryBackend<&str, String> = InMemoryBackend::new(100, Duration::from_secs(60));
    let cache = Cache::new(backend);

    let removed = cache.remove(&"missing").await.unwrap();
    assert_eq!(removed, None);
}

#[tokio::test]
async fn cache_overwrites_existing_key() {
    let backend: InMemoryBackend<&str, String> = InMemoryBackend::new(100, Duration::from_secs(60));
    let cache = Cache::new(backend);

    cache.insert("k", "first".to_string()).await.unwrap();
    cache.insert("k", "second".to_string()).await.unwrap();

    let entry = cache.get(&"k").await.unwrap().unwrap();
    assert_eq!(entry.value, "second");
}

#[tokio::test]
async fn cache_clear_removes_all() {
    let backend: InMemoryBackend<&str, i32> = InMemoryBackend::new(100, Duration::from_secs(60));
    let cache = Cache::new(backend);

    cache.insert("a", 1).await.unwrap();
    cache.insert("b", 2).await.unwrap();
    cache.clear().await.unwrap();

    assert!(cache.get(&"a").await.unwrap().is_none());
    assert!(cache.get(&"b").await.unwrap().is_none());
}

#[tokio::test]
async fn cache_stats_track_hits_and_misses() {
    let backend: InMemoryBackend<&str, i32> = InMemoryBackend::new(100, Duration::from_secs(60));
    let cache = Cache::new(backend);

    cache.insert("hit", 1).await.unwrap();
    let _ = cache.get(&"hit").await.unwrap(); // hit
    let _ = cache.get(&"hit").await.unwrap(); // hit
    let _ = cache.get(&"miss").await.unwrap(); // miss

    let stats = cache.stats().await.unwrap();
    assert_eq!(stats.hits, 2);
    assert_eq!(stats.misses, 1);
    assert!((stats.hit_rate - 2.0 / 3.0).abs() < 1e-10);
}

#[tokio::test]
async fn cache_stats_empty_initially() {
    let backend: InMemoryBackend<&str, i32> = InMemoryBackend::new(100, Duration::from_secs(60));
    let cache = Cache::new(backend);

    let stats = cache.stats().await.unwrap();
    assert_eq!(stats.hits, 0);
    assert_eq!(stats.misses, 0);
    assert_eq!(stats.hit_rate, 0.0);
}

#[tokio::test]
async fn cache_multiple_types() {
    let backend: InMemoryBackend<u32, String> = InMemoryBackend::new(100, Duration::from_secs(60));
    let cache = Cache::new(backend);

    cache.insert(1u32, "one".to_string()).await.unwrap();
    cache.insert(2u32, "two".to_string()).await.unwrap();

    let e1 = cache.get(&1u32).await.unwrap().unwrap();
    let e2 = cache.get(&2u32).await.unwrap().unwrap();
    assert_eq!(e1.value, "one");
    assert_eq!(e2.value, "two");
}

// ---------------------------------------------------------------------------
// CacheError display
// ---------------------------------------------------------------------------

#[test]
fn error_serialization_display() {
    let e = CacheError::Serialization("bad data".into());
    assert_eq!(e.to_string(), "serialization error: bad data");
}

#[test]
fn error_backend_display() {
    let e = CacheError::Backend("connection refused".into());
    assert_eq!(e.to_string(), "backend error: connection refused");
}

#[test]
fn error_expired_display() {
    let e = CacheError::Expired;
    assert_eq!(e.to_string(), "cache entry expired");
}

#[test]
fn error_full_display() {
    let e = CacheError::Full;
    assert_eq!(e.to_string(), "cache full");
}

#[test]
fn error_other_display() {
    let e = CacheError::Other("unknown failure".into());
    assert_eq!(e.to_string(), "cache error: unknown failure");
}

#[test]
fn error_debug_format() {
    let e = CacheError::Expired;
    let debug = format!("{:?}", e);
    assert!(debug.contains("Expired"));
}

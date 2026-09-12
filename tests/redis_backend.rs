// Tests talk to a real Redis in docker; unwrap/expect is the test signal.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(feature = "redis")]

//! Redis backend integration tests against a real Redis server spun up
//! per run with testcontainers (docker required).
//!
//! ```sh
//! cargo test --features redis --test redis_backend
//! ```
//!
//! Proves the wire behavior the in-memory unit tests can't: value/meta key
//! pairs under the backend's prefix, prefix isolation between backends,
//! SWR expiry windows, remove/clear fan-out to both keys, and concurrent
//! writers through one multiplexed connection.

use std::sync::Arc;
use std::time::Duration;

use cache_pal::{Cache, RedisBackend};
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::redis::Redis;

async fn spawn_redis() -> (testcontainers::ContainerAsync<Redis>, String) {
    let container = Redis::default().start().await.unwrap();
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(6379).await.unwrap();
    let url = format!("redis://{host}:{port}/");
    (container, url)
}

// ---------------------------------------------------------------------------
// Basic lifecycle
// ---------------------------------------------------------------------------

#[tokio::test]
async fn insert_get_roundtrip_over_the_wire() {
    let (_c, url) = spawn_redis().await;
    let cache = Cache::new(
        RedisBackend::<String, String>::connect(&url, "cachekit-rt")
            .await
            .unwrap(),
    );

    cache.insert("k1".into(), "v1".into()).await.unwrap();
    let entry = cache.get(&"k1".to_string()).await.unwrap().expect("hit");
    assert_eq!(entry.value, "v1");
    assert!(!entry.is_expired());

    let stats = cache.stats().await.unwrap();
    assert_eq!(stats.hits, 1);
}

#[tokio::test]
async fn miss_returns_none() {
    let (_c, url) = spawn_redis().await;
    let cache = Cache::new(
        RedisBackend::<String, String>::connect(&url, "cachekit-miss")
            .await
            .unwrap(),
    );
    assert!(cache.get(&"absent".to_string()).await.unwrap().is_none());
    let stats = cache.stats().await.unwrap();
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.hits, 0);
}

#[tokio::test]
async fn remove_deletes_value_and_meta_keys() {
    let (_c, url) = spawn_redis().await;
    let cache = Cache::new(
        RedisBackend::<String, Vec<u8>>::connect(&url, "cachekit-rm")
            .await
            .unwrap(),
    );

    cache.insert("k".into(), b"payload".to_vec()).await.unwrap();
    let removed = cache.remove(&"k".to_string()).await.unwrap();
    assert_eq!(removed, Some(b"payload".to_vec()));

    // Second remove finds nothing — both the value and meta keys are gone.
    assert_eq!(cache.remove(&"k".to_string()).await.unwrap(), None);
    assert!(cache.get(&"k".to_string()).await.unwrap().is_none());
}

#[tokio::test]
async fn clear_removes_every_entry_under_the_prefix() {
    let (_c, url) = spawn_redis().await;
    let cache = Cache::new(
        RedisBackend::<String, u32>::connect(&url, "cachekit-clear")
            .await
            .unwrap(),
    );

    for i in 0..5u32 {
        cache.insert(format!("k{i}"), i).await.unwrap();
    }
    cache.clear().await.unwrap();
    for i in 0..5u32 {
        assert!(cache.get(&format!("k{i}")).await.unwrap().is_none());
    }
    assert_eq!(cache.stats().await.unwrap().size, 0);
}

#[tokio::test]
async fn stats_size_counts_value_and_meta_keys() {
    let (_c, url) = spawn_redis().await;
    let cache = Cache::new(
        RedisBackend::<String, u32>::connect(&url, "cachekit-size")
            .await
            .unwrap(),
    );

    cache.insert("one".into(), 1).await.unwrap();
    // The backend stores a value key plus a meta key, and `stats` counts
    // every key matching the prefix glob — so one live entry reports 2.
    // Pins the documented behavior; a fix that dedupes this should update
    // the assertion to 1.
    assert_eq!(cache.stats().await.unwrap().size, 2);
}

// ---------------------------------------------------------------------------
// Prefix isolation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn distinct_prefixes_do_not_see_each_others_entries() {
    let (_c, url) = spawn_redis().await;
    let a = Cache::new(
        RedisBackend::<String, String>::connect(&url, "svc-a")
            .await
            .unwrap(),
    );
    let b = Cache::new(
        RedisBackend::<String, String>::connect(&url, "svc-b")
            .await
            .unwrap(),
    );

    a.insert("shared-key".into(), "from-a".into())
        .await
        .unwrap();
    assert!(b.get(&"shared-key".to_string()).await.unwrap().is_none());

    b.insert("shared-key".into(), "from-b".into())
        .await
        .unwrap();
    assert_eq!(
        a.get(&"shared-key".to_string())
            .await
            .unwrap()
            .unwrap()
            .value,
        "from-a"
    );
    assert_eq!(
        b.get(&"shared-key".to_string())
            .await
            .unwrap()
            .unwrap()
            .value,
        "from-b"
    );

    // Clearing A leaves B untouched.
    a.clear().await.unwrap();
    assert!(a.get(&"shared-key".to_string()).await.unwrap().is_none());
    assert_eq!(
        b.get(&"shared-key".to_string())
            .await
            .unwrap()
            .unwrap()
            .value,
        "from-b"
    );
}

// ---------------------------------------------------------------------------
// TTL / SWR
// ---------------------------------------------------------------------------

#[tokio::test]
async fn swr_entry_expires_after_max_age_plus_stale_window() {
    let (_c, url) = spawn_redis().await;
    let cache = Cache::new(
        RedisBackend::<String, String>::connect(&url, "cachekit-ttl")
            .await
            .unwrap(),
    );

    cache
        .insert_with_swr(
            "k".into(),
            "v".into(),
            Duration::from_millis(80),
            Duration::from_millis(80),
        )
        .await
        .unwrap();

    let entry = cache
        .get(&"k".to_string())
        .await
        .unwrap()
        .expect("fresh hit");
    assert!(!entry.is_stale());

    tokio::time::sleep(Duration::from_millis(110)).await;
    let entry = cache
        .get(&"k".to_string())
        .await
        .unwrap()
        .expect("stale hit");
    assert!(entry.is_stale());

    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(
        cache.get(&"k".to_string()).await.unwrap().is_none(),
        "expired entries are not served"
    );
}

// ---------------------------------------------------------------------------
// Concurrency through the multiplexed connection
// ---------------------------------------------------------------------------

#[tokio::test]
async fn concurrent_writers_never_yield_torn_reads() {
    let (_c, url) = spawn_redis().await;
    let cache = Arc::new(Cache::new(
        RedisBackend::<String, String>::connect(&url, "cachekit-race")
            .await
            .unwrap(),
    ));

    let mut handles = Vec::new();
    for w in 0..16u32 {
        let cache = cache.clone();
        handles.push(tokio::spawn(async move {
            for r in 0..10u32 {
                let value = format!("w{w}-r{r}");
                cache.insert("contended".into(), value).await.unwrap();
                if let Some(entry) = cache.get(&"contended".to_string()).await.unwrap() {
                    assert!(
                        entry.value.starts_with('w') && entry.value.contains("-r"),
                        "torn read: {:?}",
                        entry.value
                    );
                }
            }
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    let entry = cache
        .get(&"contended".to_string())
        .await
        .unwrap()
        .expect("hit");
    assert!(
        entry.value.starts_with('w') && entry.value.contains("-r"),
        "final value is one complete write"
    );
}

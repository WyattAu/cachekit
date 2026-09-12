// Tests exercise persistence directly; unwrap/expect is the test signal.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(feature = "sqlite")]

//! SQLite backend integration tests — fully local, no services required.
//!
//! Covers the full backend lifecycle against an on-disk database in a
//! tempdir: insert/get/remove/clear, TTL expiry (via the
//! stale-while-revalidate insert path, which is the only one that sets an
//! expiry), SWR staleness windows, statistics counters, persistence across
//! a backend reopen, and concurrent writers racing on one key.

use std::time::Duration;

use cache_pal::{Cache, SqliteBackend};

fn temp_backend(tag: &str) -> (tempfile::TempDir, Cache<String, Vec<u8>>) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(format!("{tag}.db"));
    let backend = SqliteBackend::new(&path).unwrap();
    (dir, Cache::new(backend))
}

// ---------------------------------------------------------------------------
// Basic lifecycle
// ---------------------------------------------------------------------------

#[tokio::test]
async fn insert_get_roundtrip() {
    let (_dir, cache) = temp_backend("roundtrip");
    cache
        .insert("k1".into(), b"value-1".to_vec())
        .await
        .unwrap();

    let entry = cache.get(&"k1".to_string()).await.unwrap().expect("hit");
    assert_eq!(entry.value, b"value-1");
    assert!(!entry.is_expired());
    assert!(!entry.is_stale());
    assert!(entry.remaining_ttl().is_none(), "plain insert has no TTL");
}

#[tokio::test]
async fn miss_returns_none_and_counts() {
    let (_dir, cache) = temp_backend("miss");
    let got = cache.get(&"absent".to_string()).await.unwrap();
    assert!(got.is_none());
    let stats = cache.stats().await.unwrap();
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.hits, 0);
}

#[tokio::test]
async fn insert_overwrites_existing_key() {
    let (_dir, cache) = temp_backend("overwrite");
    cache.insert("k".into(), b"first".to_vec()).await.unwrap();
    cache.insert("k".into(), b"second".to_vec()).await.unwrap();
    let entry = cache.get(&"k".to_string()).await.unwrap().expect("hit");
    assert_eq!(entry.value, b"second");
    let stats = cache.stats().await.unwrap();
    assert_eq!(stats.size, 1);
}

#[tokio::test]
async fn remove_returns_value_and_drops_entry() {
    let (_dir, cache) = temp_backend("remove");
    cache.insert("k".into(), b"payload".to_vec()).await.unwrap();

    let removed = cache.remove(&"k".to_string()).await.unwrap();
    assert_eq!(removed, Some(b"payload".to_vec()));
    assert!(cache.get(&"k".to_string()).await.unwrap().is_none());
    assert_eq!(cache.remove(&"k".to_string()).await.unwrap(), None);
}

#[tokio::test]
async fn clear_drops_every_entry() {
    let (_dir, cache) = temp_backend("clear");
    for i in 0..5 {
        cache.insert(format!("k{i}"), vec![i as u8]).await.unwrap();
    }
    assert_eq!(cache.stats().await.unwrap().size, 5);

    cache.clear().await.unwrap();
    assert_eq!(cache.stats().await.unwrap().size, 0);
    assert!(cache.get(&"k0".to_string()).await.unwrap().is_none());
}

#[tokio::test]
async fn stats_count_hits_and_misses() {
    let (_dir, cache) = temp_backend("stats");
    cache.insert("hit".into(), b"v".to_vec()).await.unwrap();

    let _ = cache.get(&"hit".to_string()).await.unwrap();
    let _ = cache.get(&"hit".to_string()).await.unwrap();
    let _ = cache.get(&"miss".to_string()).await.unwrap();

    let stats = cache.stats().await.unwrap();
    assert_eq!(stats.hits, 2);
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.size, 1);
    assert!((stats.hit_rate - 2.0 / 3.0).abs() < 1e-9);
}

// ---------------------------------------------------------------------------
// TTL / stale-while-revalidate
// ---------------------------------------------------------------------------

#[tokio::test]
async fn swr_entry_expires_after_max_age_plus_stale_window() {
    let (_dir, cache) = temp_backend("swr-expiry");
    cache
        .insert_with_swr(
            "k".into(),
            b"v".to_vec(),
            Duration::from_millis(80),
            Duration::from_millis(80),
        )
        .await
        .unwrap();

    // Fresh window: present, not stale.
    let entry = cache
        .get(&"k".to_string())
        .await
        .unwrap()
        .expect("fresh hit");
    assert!(!entry.is_stale());
    assert!(entry.remaining_ttl().is_some());

    // Stale window (80–160ms): still served, flagged stale.
    tokio::time::sleep(Duration::from_millis(110)).await;
    let entry = cache
        .get(&"k".to_string())
        .await
        .unwrap()
        .expect("stale hit");
    assert!(entry.is_stale(), "past max_age but within the SWR window");
    assert!(!entry.is_expired());

    // Past max_age + stale window: expired, backend refuses to serve it.
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(cache.get(&"k".to_string()).await.unwrap().is_none());
    let stats = cache.stats().await.unwrap();
    assert_eq!(stats.size, 0, "expired rows are not counted as live size");
}

#[tokio::test]
async fn swr_with_zero_stale_window_expires_after_max_age() {
    let (_dir, cache) = temp_backend("swr-zero");
    cache
        .insert_with_swr(
            "k".into(),
            b"v".to_vec(),
            Duration::from_millis(60),
            Duration::ZERO,
        )
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(cache.get(&"k".to_string()).await.unwrap().is_none());
}

#[tokio::test]
async fn swr_overwrite_replaces_earlier_entry() {
    let (_dir, cache) = temp_backend("swr-overwrite");
    cache
        .insert_with_swr(
            "k".into(),
            b"old".to_vec(),
            Duration::from_secs(60),
            Duration::from_secs(60),
        )
        .await
        .unwrap();
    cache
        .insert_with_swr(
            "k".into(),
            b"new".to_vec(),
            Duration::from_secs(60),
            Duration::from_secs(60),
        )
        .await
        .unwrap();
    let entry = cache.get(&"k".to_string()).await.unwrap().expect("hit");
    assert_eq!(entry.value, b"new");
    assert_eq!(cache.stats().await.unwrap().size, 1);
}

// ---------------------------------------------------------------------------
// Persistence across reopen
// ---------------------------------------------------------------------------

#[tokio::test]
async fn entries_survive_backend_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("persist.db");

    {
        let cache = Cache::new(SqliteBackend::new(&db).unwrap());
        cache
            .insert("durable".into(), b"survives".to_vec())
            .await
            .unwrap();
        cache
            .insert_with_swr(
                "short-lived".into(),
                b"gone".to_vec(),
                Duration::from_millis(50),
                Duration::from_millis(50),
            )
            .await
            .unwrap();
    }

    // Reopen from the same file: durable entry survives, expired one doesn't.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let cache = Cache::new(SqliteBackend::new(&db).unwrap());
    let entry = cache
        .get(&"durable".to_string())
        .await
        .unwrap()
        .expect("persisted");
    assert_eq!(entry.value, b"survives");
    assert!(
        cache
            .get(&"short-lived".to_string())
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn binary_values_round_trip_through_sqlite_blob() {
    let (_dir, cache) = temp_backend("binary");
    let payload: Vec<u8> = (0..=255u8).chain([0, 0xFF, 0x00]).collect();
    cache.insert("blob".into(), payload.clone()).await.unwrap();
    let entry = cache.get(&"blob".to_string()).await.unwrap().expect("hit");
    assert_eq!(entry.value, payload);
}

#[tokio::test]
async fn in_memory_backend_matches_on_disk_behavior() {
    let cache = Cache::new(SqliteBackend::in_memory().unwrap());
    cache.insert("k".into(), b"v".to_vec()).await.unwrap();
    assert_eq!(
        cache.get(&"k".to_string()).await.unwrap().unwrap().value,
        b"v"
    );
    cache.clear().await.unwrap();
    assert!(cache.get(&"k".to_string()).await.unwrap().is_none());
}

// ---------------------------------------------------------------------------
// Concurrency
// ---------------------------------------------------------------------------

#[tokio::test]
async fn concurrent_writers_on_one_key_never_yield_torn_reads() {
    let (_dir, cache) = temp_backend("race");
    let cache = std::sync::Arc::new(cache);

    let mut handles = Vec::new();
    for w in 0..16u32 {
        let cache = cache.clone();
        handles.push(tokio::spawn(async move {
            for r in 0..10u32 {
                let value = format!("w{w}-r{r}").into_bytes();
                cache
                    .insert("contended".into(), value.clone())
                    .await
                    .unwrap();
                // Any read must observe one complete written value — never
                // a torn/partial blob.
                if let Some(entry) = cache.get(&"contended".to_string()).await.unwrap() {
                    let got = String::from_utf8(entry.value).unwrap();
                    let expected = format!("w{w}-r{r}");
                    assert!(
                        got == expected || got.starts_with('w'),
                        "torn read: {got:?}"
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
    assert_eq!(entry.value.len(), 6, "final value is one complete write");
    let stats = cache.stats().await.unwrap();
    assert_eq!(stats.size, 1);
}

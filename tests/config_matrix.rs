// Config-knob behavior matrix: every constructor/insert knob on the
// in-memory backend must observably change behavior — default vs configured
// must differ. Time knobs use millisecond-scale real waits (moka owns the
// clock; there is no injection point) and read-driven maintenance, keeping
// the whole suite well under a second.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(feature = "in-memory")]

use std::time::Duration;

use cache_pal::{Cache, InMemoryBackend};

/// Force moka's deferred maintenance (via `stats()`) so eviction decisions
/// are applied before reads — deterministic, no sleeps, no polling.
async fn settle(cache: &Cache<String, i32>) -> u64 {
    let stats = cache.stats().await.unwrap();
    stats.size
}

// ---------------------------------------------------------------------------
// knob: capacity (InMemoryBackend::new / with_tti first arg)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn knob_capacity_evicts_beyond_the_bound() {
    let keys: Vec<String> = (0..8).map(|i| format!("k{i}")).collect();

    let small = Cache::new(InMemoryBackend::new(2, Duration::from_secs(60)));
    for (i, k) in keys.iter().enumerate() {
        small.insert(k.clone(), i as i32).await.unwrap();
    }
    assert!(
        settle(&small).await <= 2,
        "cap-2 cache must not report more than 2 entries"
    );
    let served = hits(&small, &keys).await;
    assert!(
        served <= 2,
        "cap-2 cache served {served} of 8 keys — capacity knob is not enforced"
    );

    // Mirror image: the same inserts with a large capacity keep every entry.
    let large = Cache::new(InMemoryBackend::new(1000, Duration::from_secs(60)));
    for (i, k) in keys.iter().enumerate() {
        large.insert(k.clone(), i as i32).await.unwrap();
    }
    assert_eq!(settle(&large).await, 8);
    assert_eq!(hits(&large, &keys).await, 8);
}

/// Count how many of `keys` are actually served (with correct values).
async fn hits(cache: &Cache<String, i32>, keys: &[String]) -> usize {
    let mut served = 0;
    for (i, k) in keys.iter().enumerate() {
        if let Some(entry) = cache.get(k).await.unwrap() {
            assert_eq!(entry.value, i as i32, "wrong value for {k}");
            served += 1;
        }
    }
    served
}

// ---------------------------------------------------------------------------
// knob: ttl (InMemoryBackend::new / with_tti second arg)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn knob_ttl_expires_entries_on_read() {
    let short = Cache::new(InMemoryBackend::new(100, Duration::from_millis(40)));
    let long = Cache::new(InMemoryBackend::new(100, Duration::from_secs(600)));
    short.insert("k", 1).await.unwrap();
    long.insert("k", 1).await.unwrap();

    // Fresh: both hit.
    assert!(short.get(&"k").await.unwrap().is_some());
    assert!(long.get(&"k").await.unwrap().is_some());

    tokio::time::sleep(Duration::from_millis(80)).await;

    // The 40ms TTL has lapsed: the entry must stop being served…
    assert!(
        short.get(&"k").await.unwrap().is_none(),
        "40ms-TTL entry must expire"
    );
    // …while the same key in the 10-minute-TTL cache is still served.
    assert!(
        long.get(&"k").await.unwrap().is_some(),
        "600s-TTL entry must survive the same wait"
    );
}

// ---------------------------------------------------------------------------
// knob: tti (InMemoryBackend::with_tti third arg — idle eviction)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn knob_tti_expires_idle_entries_and_renews_accessed_ones() {
    let keyed = Cache::new(InMemoryBackend::with_tti(
        100,
        Duration::from_secs(600),
        Duration::from_millis(40),
    ));
    let plain = Cache::new(InMemoryBackend::new(100, Duration::from_secs(600)));
    keyed.insert("idle", 1).await.unwrap();
    keyed.insert("touched", 2).await.unwrap();
    plain.insert("idle", 1).await.unwrap();
    plain.insert("touched", 2).await.unwrap();

    // Access "touched" every 15ms; "idle" is never touched again. After
    // ~90ms the untouched entry is past its idle timeout, the touched one
    // is not (each access renews the idle timer).
    for _ in 0..5 {
        tokio::time::sleep(Duration::from_millis(15)).await;
        let _ = keyed.get(&"touched").await.unwrap();
    }

    assert!(
        keyed.get(&"idle").await.unwrap().is_none(),
        "untouched entry must expire by TTI"
    );
    assert!(
        keyed.get(&"touched").await.unwrap().is_some(),
        "accessed entry must survive past TTI (idle timer renews on access)"
    );

    // Knob-off contrast: without TTI both entries are still served.
    assert!(plain.get(&"idle").await.unwrap().is_some());
    assert!(plain.get(&"touched").await.unwrap().is_some());
}

// ---------------------------------------------------------------------------
// knob: max_age (Cache::insert_with_swr third arg — fresh→stale boundary)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn knob_max_age_marks_entries_stale() {
    let cache = Cache::new(InMemoryBackend::new(100, Duration::from_secs(600)));
    // SWR entry: fresh for 40ms, then stale for the (long) revalidate window.
    cache
        .insert_with_swr("k", 1, Duration::from_millis(40), Duration::from_secs(600))
        .await
        .unwrap();
    // Contrast entry: fresh for 10 minutes.
    cache
        .insert_with_swr(
            "fresh",
            2,
            Duration::from_secs(600),
            Duration::from_secs(600),
        )
        .await
        .unwrap();

    let now = cache.get(&"k").await.unwrap().expect("fresh entry");
    assert!(!now.is_stale(), "entry must start fresh");
    assert!(!now.is_expired());

    tokio::time::sleep(Duration::from_millis(80)).await;

    // Past max_age: still served (within the revalidate window) but the
    // entry now reports stale so the caller knows to refresh.
    let stale = cache.get(&"k").await.unwrap().expect("stale-but-served");
    assert!(stale.is_stale(), "past max_age the entry must report stale");
    assert!(!stale.is_expired(), "stale window must not be an expiry");

    let fresh = cache.get(&"fresh").await.unwrap().expect("contrast entry");
    assert!(!fresh.is_stale(), "long-max_age entry must still be fresh");
}

// ---------------------------------------------------------------------------
// knob: stale_while_revalidate (Cache::insert_with_swr fourth arg —
// the stale serving window's end)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn knob_swr_bounds_the_stale_serving_window() {
    let cache = Cache::new(InMemoryBackend::new(100, Duration::from_secs(600)));
    // swr = 0: the entry is hard-expired the moment max_age lapses.
    cache
        .insert_with_swr("no-swr", 1, Duration::from_millis(40), Duration::ZERO)
        .await
        .unwrap();
    // swr = 10min: the same max_age lapse merely marks the entry stale.
    cache
        .insert_with_swr(
            "with-swr",
            2,
            Duration::from_millis(40),
            Duration::from_secs(600),
        )
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(80)).await;

    let gone = cache.get(&"no-swr").await.unwrap().expect("metadata only");
    assert!(
        gone.is_expired(),
        "swr=0 entry must be expired once max_age lapses"
    );
    assert_eq!(gone.remaining_ttl(), Some(Duration::ZERO));

    let stale_ok = cache.get(&"with-swr").await.unwrap().expect("stale window");
    assert!(!stale_ok.is_expired(), "swr=600s entry must not be expired");
    assert!(stale_ok.is_stale(), "…only stale");

    // The knob is what separates the two expiry timelines.
    assert!(
        gone.expires_at.unwrap() < stale_ok.expires_at.unwrap(),
        "swr=0 must expire strictly earlier than swr=600s"
    );
}

use criterion::{criterion_group, criterion_main, Criterion};
use std::time::Duration;

fn bench_cache_stats_default(c: &mut Criterion) {
    c.bench_function("cache_stats_default", |b| {
        b.iter(|| {
            let stats = cachekit::CacheStats::default();
            std::hint::black_box(stats);
        });
    });
}

fn bench_cache_stats_hit_rate(c: &mut Criterion) {
    c.bench_function("cache_stats_hit_rate_calculation", |b| {
        b.iter(|| {
            let stats = cachekit::CacheStats {
                hits: 75,
                misses: 25,
                hit_rate: 0.75,
                size: 100,
            };
            let _total = stats.total_lookups();
            let _empty = stats.is_empty();
            std::hint::black_box(stats);
        });
    });
}

async fn run_in_memory_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("in_memory_cache");

    group.bench_function("insert", |b| {
        b.iter(|| async {
            let backend = cachekit::InMemoryBackend::new(10_000, Duration::from_secs(300));
            let cache = cachekit::Cache::new(backend);
            for i in 0..100 {
                cache.insert(format!("key_{}", i), i).await.unwrap();
            }
        });
    });

    group.bench_function("get_hit", |b| {
        b.iter(|| async {
            let backend = cachekit::InMemoryBackend::new(10_000, Duration::from_secs(300));
            let cache = cachekit::Cache::new(backend);
            for i in 0..100 {
                cache.insert(format!("key_{}", i), i).await.unwrap();
            }
            for i in 0..100 {
                let _entry = cache.get(&format!("key_{}", i)).await.unwrap();
            }
        });
    });

    group.bench_function("get_miss", |b| {
        b.iter(|| async {
            let backend: cachekit::InMemoryBackend<&str, i32> =
                cachekit::InMemoryBackend::new(10_000, Duration::from_secs(300));
            let cache = cachekit::Cache::new(backend);
            for _i in 0..100 {
                let _entry = cache.get(&"nonexistent").await.unwrap();
            }
        });
    });

    group.bench_function("stats_after_ops", |b| {
        b.iter(|| async {
            let backend = cachekit::InMemoryBackend::new(10_000, Duration::from_secs(300));
            let cache = cachekit::Cache::new(backend);
            cache.insert("a", 1).await.unwrap();
            let _ = cache.get(&"a").await.unwrap();
            let _ = cache.get(&"b").await.unwrap();
            let stats = cache.stats().await.unwrap();
            std::hint::black_box(stats);
        });
    });

    group.finish();
}

fn bench_benchmark_cache(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        run_in_memory_benches(c).await;
    });
}

criterion_group!(
    benches,
    bench_cache_stats_default,
    bench_cache_stats_hit_rate,
    bench_benchmark_cache,
);
criterion_main!(benches);

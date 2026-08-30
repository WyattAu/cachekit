# cachekit

Unified caching for Rust — in-memory (moka) and Redis backends with TTL, stale-while-revalidate, and eviction listeners.

[![Crates.io](https://img.shields.io/crates/v/cachekit.svg)](https://crates.io/crates/cachekit)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](./LICENSE-MIT)

## Purpose

`cachekit` provides a unified caching interface with swappable backends. Choose between fast in-memory caching with `moka`, distributed caching with `Redis`, or simple concurrent maps with `dashmap`.

## Features

- **Unified API** — same `Cache<K,V>` interface regardless of backend
- **TTL support** — configurable time-to-live for entries
- **Stale-while-revalidate** — serve stale data while refreshing
- **Eviction listeners** — react to cache evictions
- **No unsafe code** — `#![forbid(unsafe_code)]`

## Backend Comparison

| Backend | Feature | Persistent | Distributed | Default |
|---------|---------|------------|-------------|---------|
| `moka` | `in-memory` | No | No | Yes |
| `redis` | `redis` | Yes | Yes | No |
| `dashmap` | `dashmap` | No | No | No |

## Usage

```rust
use cachekit::{Cache, InMemoryBackend};
use std::time::Duration;

#[tokio::main]
async fn main() {
    // Create an in-memory cache with 10,000 capacity and 5-minute TTL
    let backend = InMemoryBackend::new(10_000, Duration::from_secs(300));
    let cache = Cache::new(backend);

    // Insert values
    cache.insert("user:123".to_string(), "Alice".to_string()).await.unwrap();

    // Get values
    if let Some(entry) = cache.get(&"user:123".to_string()).await.unwrap() {
        println!("Value: {}", entry.value);
        println!("Age: {:?}", entry.age());
    }

    // Check stats
    let stats = cache.stats().await.unwrap();
    println!("Hit rate: {:.1}%", stats.hit_rate * 100.0);
}
```

## License

Licensed under either of [Apache License, Version 2.0](./LICENSE-APACHE) or [MIT license](./LICENSE-MIT) at your option.

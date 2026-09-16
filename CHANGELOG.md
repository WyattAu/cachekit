# Changelog

All notable changes to this project are documented here. Format: [Keep a
Changelog](https://keepachangelog.com/) — versions follow [semver](https://semver.org).

## [Unreleased]

## [0.4.0] - 2026-09-16

### Breaking (0.x minor re-cut)

- Removed the `dashmap` feature flag (removed in 0.3.2 without a version
  bump — it gated no code and the README claimed a dashmap backend that
  does not exist). cargo-semver-checks correctly flags the feature removal
  against the v0.3.0 baseline, so the minor is re-cut here to start a new
  clean baseline. No other API changes: 0.3.1 → 0.4.0 is API-identical.

### Changed

- The redis backend suite now runs against the CI job's redis service
  container (or any local Redis via `CACHEKIT_TEST_REDIS_URL`) instead of
  spinning up a per-run testcontainers container; testcontainers and
  testcontainers-modules leave the dev-dependency graph entirely.

## [0.3.2] - 2026-09-12

### Added

- `tests/config_matrix.rs` (5 tests): behavior-observable coverage for every
  in-memory knob — `capacity` (eviction enforced vs unbounded), `ttl`
  (expiry on read vs long-TTL contrast), `tti` (idle expiry + renewal on
  access vs no-TTI contrast), `max_age` (fresh→stale boundary) and
  `stale_while_revalidate` (stale serving window / expiry timeline).

### Fixed

- `InMemoryBackend::stats()` now runs moka's pending maintenance tasks
  before reading the entry count, so `CacheStats.size` (and hit/miss
  arithmetic around it) reflects reality instead of lagging arbitrarily
  behind recent inserts/evictions.
- Removed the vestigial `dashmap` feature flag: it gated no code and the
  README claimed a dashmap backend that does not exist.

## [0.3.1] - 2026-09-12

### Added

- SQLite backend integration suite (`tests/sqlite_backend.rs`, 13 tests,
  fully local in a tempdir): insert/get/remove/clear lifecycle, overwrite
  semantics, stats counters, SWR (stale-while-revalidate) staleness and
  expiry windows, persistence across a backend reopen, binary blob
  round trips, and concurrent-writer race coverage.
- Redis backend integration suite (`tests/redis_backend.rs`, 9 tests)
  against a real Redis server spun up per run with testcontainers:
  wire round trips, prefix isolation between backends, value+meta key
  handling (remove/clear/stats), SWR expiry, and concurrent writers
  through one multiplexed connection.

### Fixed

- **Redis SWR entries with sub-second windows were deleted instantly:**
  `EXPIRE` truncated the total TTL to whole seconds, so a 160 ms window
  became `EXPIRE 0` (delete). Now `PEXPIRE` with millisecond precision.
- **Redis SWR deadlines never reported stale/expired correctly:** the
  stored metadata truncated deadlines to whole seconds and reused
  `now.subsec_nanos()` in place of the deadline's sub-second part, and
  the read path saturated future `SystemTime` deadlines at `now`
  (`duration_since` errors on future times). Deadlines are now stored
  with full sub-second precision and converted to `Instant` via
  `checked_add` for future times (mirroring the sqlite backend).
  Caught by the new integration suite.

### CI

- New `integration` job running the sqlite suite (no services) and the
  redis suite (testcontainers).

## [0.3.0] - 2026-09-05

### Added
- Initial public release.

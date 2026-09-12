# Changelog

All notable changes to this project are documented here. Format: [Keep a
Changelog](https://keepachangelog.com/) — versions follow [semver](https://semver.org).

## [Unreleased]

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

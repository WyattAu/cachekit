# Threat Model — cachekit

Reference: STRIDE. Scope: the crate's public API surface. Trust boundary:
(1) bytes/inputs entering public constructors and parsers, (2) concurrent
callers sharing interior state. cachekit is an in-process library — it opens
no sockets and inherits the embedding process's trust domain.

Purpose: Cache abstraction (`cache-pal`) — TTL cache with in-memory/Redis/SQLite backends and stats

## Assets

| ID | Asset | Exposed via |
|----|-------|-------------|
| A1 | no stale reads beyond TTL | hostile input, concurrent callers |
| A2 | memory bounded by capacity | hostile input, concurrent callers |

## STRIDE Analysis

| # | Threat | Category | Surface | Mitigation | Residual risk |
|---|--------|----------|---------|------------|---------------|
| T1 | Unbounded memory via flood of unique keys | DoS | ``InMemoryBackend::new(capacity)`` | hard capacity bound with eviction; documented limit | documented |
| T2 | Cache poisoning via crafted keys | Tampering | `key type` | keys are typed generics, stored as-is; values opaque to the cache | documented |
| T3 | Stale value served after TTL | Spoofing | `expiry check` | expiry checked on every read against monotonic instant | documented |

## Repudiation

The crate keeps no audit trail; attribution of calls to callers is out of
scope for an in-process library.

## Out of Scope

- Network transport security (the crate never opens sockets).
- Storage-host compromise: an attacker who controls the host can bypass all
  in-process mitigations.
- Denial of service via resource exhaustion of the host process beyond the
  bounds enforced above.

Reviewed: 2026-09-11

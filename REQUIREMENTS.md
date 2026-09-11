# Requirements — cachekit

Numbered, testable requirements. Every requirement maps to at least one named
test or doc-comment contract; security-relevant items cite threat-model rows.

Scope: Cache abstraction (`cache-pal`) — TTL cache with in-memory/Redis/SQLite backends and stats

## Functional

| ID | Requirement | Priority |
|----|-------------|----------|
| REQ-CP-001 | Insert/get/remove semantics: get of unexpired key returns value; expired key returns None and is evicted | MUST |
| REQ-CP-002 | `CacheStats` counts hits/misses exactly (hit + miss == total gets) | MUST |
| REQ-CP-003 | Backends are feature-gated; default build ships only in-memory | MUST |

## Security

| ID | Requirement | Priority |
|----|-------------|----------|
| REQ-CP-100 | TTL is enforced on read: a clock that passed expiry never serves stale values | MUST |

## Observability & API hygiene

| ID | Requirement | Priority |
|----|-------------|----------|
| REQ-CP-900 | All fallible public APIs return typed errors; production `unwrap`/`expect` is denied or explicitly justified with an invariant comment | MUST |
| REQ-CP-901 | Public items carry doc comments with runnable examples where practical | SHOULD |

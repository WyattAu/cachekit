# PATTERN_ADOPTION: cachekit

Relationship to the WyattAu pattern canon
(`github.com/WyattAu/pattern-standards`). Machine-checked by
`pattern-standards/scripts/adoption-check.py`; format rules in
`references/adoption-format.md`. One row per pattern with a real
relationship to this repo; unrelated patterns are omitted.

| Pattern | Status | Evidence |
|---|---|---|
| P21 | partial | src/lib.rs:434 (caller-declared max_age + stale-while-revalidate budgets on insert; `is_stale()` at src/lib.rs:471; backends honor the same budget at src/in_memory.rs:87, src/redis.rs:207, src/sqlite.rs:159; no single-flight refresh coalescing, revalidation is the caller's job) |
| P29 | refused | tests/config_matrix.rs:3 (R2: backends are thin wrappers over moka/redis/rusqlite with no injection point; a Faulty wrapper would be a mock seam testing's sake; real-wait integration tests cover the failure knobs instead) |
| P64 | partial | tests/proptest.rs:9 (proptest suite with shrinking over generated inputs; stats invariants only, cache behavior is example-tested in tests/integration.rs and tests/config_matrix.rs) |

## Notes

P21 is partial because the canon mechanism has two halves. The
staleness-budget half is implemented: callers declare `max_age` and
`stale_while_revalidate` per insert, and `CacheEntry::is_stale()` decides
serving from those budgets. The single-flight half is absent: the crate
has no refresh executor, so concurrent misses do not coalesce into one
refresh. Adding coalescing needs a tracking issue before a `planned` row
is honest.

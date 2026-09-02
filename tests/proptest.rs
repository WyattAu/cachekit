//! Property-based tests for cache-pal crate.

use proptest::prelude::*;

use cache_pal::CacheStats;

proptest! {
    #[test]
    fn cache_stats_total_lookups_equals_hits_plus_misses(
        hits in 0u64..1_000_000,
        misses in 0u64..1_000_000,
    ) {
        let stats = CacheStats {
            hits,
            misses,
            hit_rate: if hits + misses == 0 { 0.0 } else { hits as f64 / (hits + misses) as f64 },
            size: hits + misses,
        };
        prop_assert_eq!(stats.total_lookups(), hits + misses);
    }

    #[test]
    fn cache_stats_is_empty_only_when_size_zero(
        hits in 0u64..1_000,
        misses in 0u64..1_000,
    ) {
        let stats = CacheStats {
            hits,
            misses,
            hit_rate: 0.0,
            size: hits + misses,
        };
        prop_assert_eq!(stats.is_empty(), stats.size == 0);
    }

    #[test]
    fn cache_stats_hit_rate_bounded(
        hits in 0u64..1_000_000,
        misses in 0u64..1_000_000,
    ) {
        let total = hits + misses;
        if total == 0 {
            prop_assert_eq!(0.0f64, 0.0);
        } else {
            let rate = hits as f64 / total as f64;
            prop_assert!(rate >= 0.0 && rate <= 1.0);
        }
    }

    #[test]
    fn cache_stats_display_contains_size(
        hits in 0u64..1_000,
        misses in 0u64..1_000,
    ) {
        let stats = CacheStats {
            hits,
            misses,
            hit_rate: if hits + misses == 0 { 0.0 } else { hits as f64 / (hits + misses) as f64 },
            size: hits + misses,
        };
        let display = stats.to_string();
        prop_assert!(display.contains("size:"));
    }

    #[test]
    fn cache_stats_clone_preserves_values(
        hits in 0u64..1_000,
        misses in 0u64..1_000,
    ) {
        let stats = CacheStats {
            hits,
            misses,
            hit_rate: 0.5,
            size: 100,
        };
        let cloned = stats.clone();
        prop_assert_eq!(stats.hits, cloned.hits);
        prop_assert_eq!(stats.misses, cloned.misses);
        prop_assert_eq!(stats.size, cloned.size);
    }
}

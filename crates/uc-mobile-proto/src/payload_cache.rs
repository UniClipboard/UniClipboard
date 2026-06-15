//! Payload-cache eviction DECISION (the only pure slice of uc-ios
//! `Shared/Cache/PayloadCache.swift`).
//!
//! The file I/O — atomic writes, backup-exclusion, the mtime touch on read, the
//! concurrency semaphore, and the in-flight `fetchAndStore` dedup — stays
//! native (it is platform I/O, not a deterministic decision). What moves here
//! is the LRU policy: given a snapshot of the cache directory ({key, size,
//! mtime}) and the cap, decide which keys to delete (snapshot in → command
//! out). The native layer then removes exactly those files.
//!
//! Also here: the path-safety key check shared by every native operation.

/// A snapshot of one cache file, captured by the native layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheEntry {
    /// File name == §2.8 `profileId` (`"<Type>-<HASH>"`).
    pub key: String,
    /// File size in bytes.
    pub size: i64,
    /// Last-modification time, epoch-milliseconds (the LRU sort key).
    pub mtime_millis: i64,
}

/// Decide which cache files to evict so total occupied bytes ≤ `max_bytes`.
/// LRU by mtime (oldest first), matching `PayloadCache.evictIfOverCapacity`.
/// Returns the keys to delete, oldest-first; empty when already under cap.
///
/// Used both after a write and on a settings-driven cap shrink
/// (`setMaxBytes`) — the same decision in both Swift call sites.
pub fn plan_eviction(entries: &[CacheEntry], max_bytes: i64) -> Vec<String> {
    let mut total: i64 = entries.iter().map(|e| e.size).sum();
    if total <= max_bytes {
        return Vec::new();
    }
    let mut sorted: Vec<&CacheEntry> = entries.iter().collect();
    sorted.sort_by_key(|e| e.mtime_millis);
    let mut evict = Vec::new();
    for entry in sorted {
        if total <= max_bytes {
            break;
        }
        evict.push(entry.key.clone());
        total -= entry.size;
    }
    evict
}

/// Path-safety check for a cache key (Swift `PayloadCache.isValidKey`): non-empty,
/// no path separators, not the `.`/`..` directory entries.
pub fn is_valid_cache_key(key: &str) -> bool {
    !key.is_empty() && !key.contains('/') && !key.contains('\\') && key != "." && key != ".."
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(key: &str, size: i64, mtime: i64) -> CacheEntry {
        CacheEntry {
            key: key.to_string(),
            size,
            mtime_millis: mtime,
        }
    }

    /// Swift `test_LRU_doesNotEvictWhenUnderCap`.
    #[test]
    fn nothing_evicted_under_cap() {
        let entries = vec![
            entry("A", 50_000, 1),
            entry("B", 50_000, 2),
            entry("C", 50_000, 3),
            entry("D", 50_000, 4),
        ];
        assert!(plan_eviction(&entries, 250_000).is_empty());
    }

    /// Swift `test_LRU_evictsOldestUntilUnderCap`: cap 200, three 100B entries,
    /// evict oldest (A) → total 200.
    #[test]
    fn evicts_oldest_until_under_cap() {
        let entries = vec![
            entry("A", 100, 10),
            entry("B", 100, 20),
            entry("C", 100, 30),
        ];
        assert_eq!(plan_eviction(&entries, 200), vec!["A".to_string()]);
    }

    /// Swift `test_setMaxBytes_shrinkBelowOccupancy_evictsImmediately`: three
    /// 200B entries, shrink to 250 → evict the oldest two, keep C.
    #[test]
    fn shrink_evicts_oldest_two() {
        let entries = vec![
            entry("A", 200, 10),
            entry("B", 200, 20),
            entry("C", 200, 30),
        ];
        assert_eq!(
            plan_eviction(&entries, 250),
            vec!["A".to_string(), "B".to_string()]
        );
    }

    #[test]
    fn exactly_at_cap_evicts_nothing() {
        let entries = vec![entry("A", 100, 1), entry("B", 100, 2)];
        assert!(plan_eviction(&entries, 200).is_empty());
    }

    #[test]
    fn empty_cache_evicts_nothing() {
        assert!(plan_eviction(&[], 0).is_empty());
    }

    /// Swift `test_invalidKey_writeThrows` rejects exactly these.
    #[test]
    fn invalid_keys_rejected() {
        for bad in ["", "../escape", "with/slash", "with\\backslash", ".", ".."] {
            assert!(!is_valid_cache_key(bad), "{bad:?} must be rejected");
        }
        for ok in ["Image-AAAA", "Text-DEADBEEF", "File-1"] {
            assert!(is_valid_cache_key(ok), "{ok:?} must be accepted");
        }
    }
}

//! Post-load cleaning: sort and deduplicate.
//!
//! Every dataset has a primary key that identifies one event:
//! klines `open_time`, trades `agg_trade_id`/`trade_id`, book ticker
//! `update_id`, book depth `(time, percentage)`, funding `calc_time`,
//! metrics `create_time`. Liquidation snapshots have no key, so exact
//! duplicate rows are removed instead.
//!
//! Sorting is parallel (rayon); dedupe keeps the first occurrence and returns
//! the number removed so the caller can warn.

use std::collections::HashSet;
use std::hash::Hash;

use rayon::prelude::*;

/// Sort by `key` and keep the first row of each key.
///
/// Returns the number of rows removed.
pub(crate) fn sort_dedup_by_key<T, K>(rows: &mut Vec<T>, key: impl Fn(&T) -> K + Sync) -> usize
where
    T: Send,
    K: Ord + Send,
{
    rows.par_sort_by_key(&key);
    let before = rows.len();
    rows.dedup_by(|a, b| key(a) == key(b));
    before - rows.len()
}

/// Sort by `key` without deduplicating.
pub(crate) fn sort_by_key<T, K>(rows: &mut [T], key: impl Fn(&T) -> K + Sync)
where
    T: Send,
    K: Ord + Send,
{
    rows.par_sort_by_key(key);
}

/// Remove rows whose key was already seen, preserving order (keep first).
///
/// Unlike [`sort_dedup_by_key`], this also handles duplicates separated by
/// rows with a different identity (e.g. duplicate liquidation snapshots
/// interleaved with other same-timestamp events).
pub(crate) fn dedupe_by_hash_key<T, K>(rows: &mut Vec<T>, key: impl Fn(&T) -> K) -> usize
where
    K: Hash + Eq,
{
    let mut seen = HashSet::new();
    let mut removed = 0;
    rows.retain(|row| {
        if seen.insert(key(row)) {
            true
        } else {
            removed += 1;
            false
        }
    });
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    struct Row {
        time: i64,
        value: f64,
    }

    fn row(time: i64, value: f64) -> Row {
        Row { time, value }
    }

    #[test]
    fn sort_dedup_keeps_first() {
        let mut rows = vec![row(200, 2.0), row(100, 1.0), row(100, 9.0)];
        let removed = sort_dedup_by_key(&mut rows, |r| r.time);
        assert_eq!(removed, 1);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].time, 100);
        assert!((rows[0].value - 1.0).abs() < f64::EPSILON);
        assert_eq!(rows[1].time, 200);
    }

    #[test]
    fn sort_by_key_is_stable_on_equal_keys() {
        let mut rows = vec![row(100, 1.0), row(100, 2.0), row(50, 0.0)];
        sort_by_key(&mut rows, |r| r.time);
        assert_eq!(rows[0].time, 50);
        assert!((rows[1].value - 1.0).abs() < f64::EPSILON);
        assert!((rows[2].value - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn hash_dedupe_catches_interleaved_duplicates() {
        let mut rows = vec![row(1, 1.0), row(1, 2.0), row(1, 1.0)];
        let removed = dedupe_by_hash_key(&mut rows, |r| (r.time, r.value.to_bits()));
        assert_eq!(removed, 1);
        assert_eq!(rows.len(), 2);
        assert!((rows[1].value - 2.0).abs() < f64::EPSILON);
    }
}

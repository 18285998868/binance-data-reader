//! Structured load report.
//!
//! The report answers "what did I actually get?" without parsing logs: how
//! many files were selected per cadence, how many rows were decoded, dropped
//! as daily/monthly overlap, dropped by the date window, skipped as malformed,
//! and what the resulting time range and kline gap count are.

use chrono::NaiveDate;
use serde::Serialize;

use crate::types::{BadRowPolicy, Dataset, Interval, Market};

/// Everything the reader did for one request.
///
/// `rows` equals `rows_daily + rows_monthly - duplicates_removed` and the
/// returned table length. `rows_decoded` counts successfully parsed rows
/// before overlap, window and duplicate filtering.
#[derive(Debug, Clone, Serialize)]
pub struct LoadReport {
    /// Requested market.
    pub market: Market,
    /// Requested dataset.
    pub dataset: Dataset,
    /// Requested symbol.
    pub symbol: String,
    /// Requested interval, for kline-shaped datasets.
    pub interval: Option<Interval>,
    /// Inclusive UTC start date, when configured.
    pub start_date: Option<NaiveDate>,
    /// Inclusive UTC end date, when configured.
    pub end_date: Option<NaiveDate>,
    /// Row-level damage policy used for this load.
    pub bad_rows: BadRowPolicy,
    /// Daily files selected after the date-window filter.
    pub files_daily: usize,
    /// Monthly files selected after the date-window filter and after dropping
    /// months whose only requested days are covered by daily files.
    pub files_monthly: usize,
    /// Rows parsed before any filtering.
    pub rows_decoded: usize,
    /// Monthly rows dropped because the same UTC day is covered by a daily file.
    pub rows_dropped_overlap: usize,
    /// Rows dropped because they fall outside the requested date window.
    pub rows_dropped_window: usize,
    /// Rows dropped by [`BadRowPolicy::Skip`].
    pub rows_skipped: usize,
    /// Up to five examples of skipped rows (`path:line: detail`).
    pub skipped_samples: Vec<String>,
    /// Exact duplicate rows removed (liquidation snapshots only).
    pub duplicates_removed: usize,
    /// Resulting row count.
    pub rows: usize,
    /// Resulting rows that came from daily files, before duplicate removal.
    pub rows_daily: usize,
    /// Resulting rows that came from monthly files, before duplicate removal.
    pub rows_monthly: usize,
    /// First primary timestamp in the result, in epoch milliseconds.
    pub first_time: Option<i64>,
    /// Last primary timestamp in the result, in epoch milliseconds.
    pub last_time: Option<i64>,
    /// Number of holes between consecutive kline bars; never filled.
    ///
    /// Always zero for non-kline datasets.
    pub gaps: usize,
    /// Wall-clock duration of scan, decode, merge and validation.
    pub elapsed_ms: u64,
}

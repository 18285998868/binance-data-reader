//! Entry point: validate-once config → scan archive → parse rows → sort/dedupe
//! → typed column sets.

#[path = "01_reader/binance.rs"]
mod binance_kline;
#[path = "01_reader/book.rs"]
mod book;
#[path = "02_clean.rs"]
mod clean;
#[path = "02_clean_kline.rs"]
mod clean_kline;
#[path = "03_column.rs"]
mod columns;
#[path = "01_reader/common.rs"]
mod common;
#[path = "01_reader/funding.rs"]
mod funding;
#[path = "03_column_kline.rs"]
mod kline_columns;
#[path = "01_reader/klines.rs"]
mod klines;
#[path = "01_reader/liquidation.rs"]
mod liquidation;
#[path = "01_reader/metrics.rs"]
mod metrics;
#[path = "01_reader/trades.rs"]
mod trades;

use crate::config::{DataConfig, KlineConfig};
use crate::error::{ReaderError, Result};
use crate::types::Dataset;
use chrono::NaiveDate;
use rayon::prelude::*;

pub use columns::{
    AggTradeColumns, BookDepthColumns, BookTickerColumns, DatasetColumns, FundingRateColumns,
    KlineDatasetColumns, LiquidationColumns, MetricsColumns, TradeColumns,
};
pub use kline_columns::KlineColumns;

/// Load one symbol × one dataset from the configured archive root.
///
/// The output rows are sorted by the dataset's primary key and deduplicated
/// (keep-first). Row-level bad data is warned and skipped; file-level damage
/// (corrupt zip, missing CSV, invalid filename date) fails the batch.
///
/// # Errors
///
/// Returns [`crate::ReaderError::Io`] when the dataset directory is missing or
/// a directory/file cannot be read, and [`crate::ReaderError::Parse`] for
/// invalid archive filenames or any file-level parse failure. ZIP errors are
/// folded into the per-file [`crate::ReaderError::Parse`] aggregate.
///
/// # Examples
///
/// ```no_run
/// use binance_data_reader::{DataConfig, Dataset, load_columns};
///
/// let cfg = DataConfig::new(
///     Dataset::Metrics,
///     "/data",
///     "BTCUSDT",
///     None,
///     Some("2024-01-01"),
///     Some("2024-01-31"),
/// )?;
/// let columns = load_columns(&cfg)?;
/// println!("{} rows", columns.len());
/// # Ok::<(), binance_data_reader::ReaderError>(())
/// ```
#[tracing::instrument(skip_all, fields(
    dataset = %config.dataset(),
    market = %config.market(),
    symbol = %config.symbol(),
))]
pub fn load_columns(config: &DataConfig) -> Result<DatasetColumns> {
    let dataset = config.dataset();
    let window = common::DateWindow::new(config.start_date(), config.end_date());
    let files = common::scan_zip_files(
        config.dir(),
        config.market(),
        dataset,
        config.symbol(),
        config.interval(),
        config.start_date(),
        config.end_date(),
    )?;

    let columns = match dataset {
        Dataset::IndexPriceKlines | Dataset::MarkPriceKlines | Dataset::PremiumIndexKlines => {
            let mut rows = klines::load(&files, &window, dataset.allows_non_positive_price())?;
            warn_dedup(
                config,
                clean::sort_dedup_by_key(&mut rows, |row| row.open_time),
                "open_time",
            );
            DatasetColumns::Klines(KlineDatasetColumns::from_records(rows, dataset))
        }
        Dataset::AggTrades => {
            let mut rows = trades::load_agg_trades(&files, &window)?;
            warn_dedup(
                config,
                clean::sort_dedup_by_key(&mut rows, |row| row.agg_trade_id),
                "agg_trade_id",
            );
            DatasetColumns::AggTrades(rows.into())
        }
        Dataset::Trades => {
            let mut rows = trades::load_trades(&files, &window, config.market())?;
            warn_dedup(
                config,
                clean::sort_dedup_by_key(&mut rows, |row| row.trade_id),
                "trade_id",
            );
            DatasetColumns::Trades(rows.into())
        }
        Dataset::BookTicker => {
            let mut rows = book::load_book_ticker(&files, &window)?;
            warn_dedup(
                config,
                clean::sort_dedup_by_key(&mut rows, |row| row.update_id),
                "update_id",
            );
            DatasetColumns::BookTicker(rows.into())
        }
        Dataset::BookDepth => {
            let mut rows = book::load_book_depth(&files, &window)?;
            warn_dedup(
                config,
                clean::sort_dedup_by_key(&mut rows, |row| (row.time, row.percentage)),
                "(time, percentage)",
            );
            DatasetColumns::BookDepth(rows.into())
        }
        Dataset::FundingRate => {
            let mut rows = funding::load(&files, &window)?;
            warn_dedup(
                config,
                clean::sort_dedup_by_key(&mut rows, |row| row.calc_time),
                "calc_time",
            );
            DatasetColumns::FundingRate(rows.into())
        }
        Dataset::Metrics => {
            let mut rows = metrics::load(&files, &window, config.symbol())?;
            warn_dedup(
                config,
                clean::sort_dedup_by_key(&mut rows, |row| row.create_time),
                "create_time",
            );
            DatasetColumns::Metrics(rows.into())
        }
        Dataset::LiquidationSnapshot => {
            let mut rows = liquidation::load(&files, &window)?;
            clean::sort_by_key(&mut rows, |row| row.time);
            warn_dedup(
                config,
                clean::dedupe_by_hash_key(&mut rows, liquidation::LiquidationRecord::identity),
                "full row",
            );
            DatasetColumns::LiquidationSnapshot(rows.into())
        }
    };

    Ok(columns)
}

fn warn_dedup(config: &DataConfig, removed: usize, key: &str) {
    if removed > 0 {
        tracing::warn!(
            dataset = %config.dataset(),
            symbol = %config.symbol(),
            key,
            removed,
            "deduped rows by key (keep first)"
        );
    }
}

/// File-level hard-failure tolerance for kline loads (zero tolerance).
pub(crate) const MAX_FAILED_FILES_TOLERATED: usize = 0;
/// Maximum number of failed-file samples retained for warnings.
const MAX_FAILED_SAMPLES: usize = 5;

/// One OHLCV bar produced by the kline readers.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Record {
    pub open_time: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

/// File-level hard-failure handling: fail above the tolerance, otherwise warn.
pub(crate) fn fail_on_hard_errors(source: &str, failed: &[String]) -> Result<()> {
    if failed.is_empty() {
        return Ok(());
    }
    let samples: Vec<&str> = failed
        .iter()
        .take(MAX_FAILED_SAMPLES)
        .map(String::as_str)
        .collect();
    if failed.len() > MAX_FAILED_FILES_TOLERATED {
        return Err(ReaderError::Parse(format!(
            "{source}: {} file(s) failed to load (tolerated {MAX_FAILED_FILES_TOLERATED}): {}",
            failed.len(),
            samples.join("; ")
        )));
    }
    tracing::warn!(
        source,
        failed_files = failed.len(),
        samples = ?samples,
        "loaded with failed files within tolerance; review data completeness"
    );
    Ok(())
}

/// OHLC row validation shared by both kline readers.
pub(crate) fn is_valid_ohlc(open: f64, high: f64, low: f64, close: f64, volume: f64) -> bool {
    for v in [open, high, low, close, volume] {
        if !v.is_finite() {
            return false;
        }
    }
    if open <= 0.0 || high <= 0.0 || low <= 0.0 || close <= 0.0 || volume < 0.0 {
        return false;
    }
    high >= low && high >= open && high >= close && low <= open && low <= close
}

/// Row-level UTC date trim; `end_date` covers the whole day.
fn trim_to_date_range(
    records: Vec<Record>,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
) -> Vec<Record> {
    if start_date.is_none() && end_date.is_none() {
        return records;
    }
    let day_start_ms = |d: NaiveDate| {
        d.and_hms_opt(0, 0, 0)
            .expect("INVARIANT: NaiveDate always has a midnight instant")
            .and_utc()
            .timestamp_millis()
    };
    let start_ms = start_date.map(day_start_ms);
    let end_exclusive_ms = end_date.and_then(|d| d.succ_opt()).map(day_start_ms);
    records
        .into_iter()
        .filter(|r| {
            if let Some(lo) = start_ms
                && r.open_time < lo
            {
                return false;
            }
            if let Some(hi) = end_exclusive_ms
                && r.open_time >= hi
            {
                return false;
            }
            true
        })
        .collect()
}

/// Load one symbol × one timeframe of Binance klines as OHLCV columns.
///
/// Rows are trimmed to the requested date range, globally sorted, deduplicated
/// and gap-checked (gaps are reported, never filled).
///
/// # Errors
///
/// Returns [`ReaderError::Io`] when the archive directory is missing and
/// [`ReaderError::Parse`] for any file-level failure.
pub fn load_kline_columns(config: &KlineConfig) -> Result<KlineColumns> {
    let mut records = binance_kline::load_pipeline(
        config.dir(),
        config.market(),
        config.symbol(),
        config.timeframe(),
        config.start_date(),
        config.end_date(),
    )?;

    records = trim_to_date_range(records, config.start_date(), config.end_date());
    records.par_sort_by_key(|r| r.open_time);

    let (records, report) = clean_kline::clean_sorted(records, config.timeframe());
    if report.dups_removed > 0 {
        tracing::warn!(
            symbol = %config.symbol(),
            timeframe = %config.timeframe(),
            dups_removed = report.dups_removed,
            "deduped duplicate bars by open_time (keep first)"
        );
    }
    if !report.gaps.is_empty() {
        let missing_total: i64 = report.gaps.iter().map(|g| g.missing_bars.max(0)).sum();
        tracing::warn!(
            symbol = %config.symbol(),
            timeframe = %config.timeframe(),
            gap_segments = report.gaps.len(),
            missing_bars = missing_total,
            samples = ?clean_kline::gap_samples(&report.gaps),
            "detected bar gaps; series kept as-is, review backtest results"
        );
    }

    Ok(KlineColumns::from(records))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(open_time: i64) -> Record {
        Record {
            open_time,
            open: 10.0,
            high: 12.0,
            low: 9.0,
            close: 11.0,
            volume: 100.0,
        }
    }

    fn pd(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn day_ms(s: &str) -> i64 {
        pd(s)
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis()
    }

    #[test]
    fn valid_ohlc_boundaries() {
        assert!(is_valid_ohlc(10.0, 10.0, 10.0, 10.0, 0.0));
        assert!(!is_valid_ohlc(10.0, 9.0, 12.0, 11.0, 1.0));
        assert!(!is_valid_ohlc(f64::NAN, 12.0, 9.0, 11.0, 1.0));
        assert!(!is_valid_ohlc(10.0, 12.0, 9.0, 11.0, f64::INFINITY));
    }

    #[test]
    fn trim_no_bounds_keeps_all() {
        let rows = vec![rec(0), rec(1), rec(i64::MAX), rec(i64::MIN)];
        assert_eq!(trim_to_date_range(rows, None, None).len(), 4);
    }

    #[test]
    fn trim_end_date_covers_whole_utc_day() {
        let rows = vec![
            rec(day_ms("2025-01-02")),
            rec(day_ms("2025-01-02") + 86_399_999),
            rec(day_ms("2025-01-03")),
        ];
        let out = trim_to_date_range(rows, None, Some(pd("2025-01-02")));
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn trim_start_is_inclusive() {
        let rows = vec![rec(day_ms("2025-01-02") - 1), rec(day_ms("2025-01-02"))];
        let out = trim_to_date_range(rows, Some(pd("2025-01-02")), None);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].open_time, day_ms("2025-01-02"));
    }
}

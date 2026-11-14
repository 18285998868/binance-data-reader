//! Dataset loaders and the shared merge pipeline.
//!
//! Every loader follows the same steps:
//!
//! 1. scan both cadences ([`source::files::scan`]);
//! 2. read ZIPs in parallel, validating each row's timestamp against the
//!    calendar period of its file;
//! 3. drop rows covered by a daily file when reading a monthly file;
//! 4. drop rows outside the requested date window;
//! 5. sort by primary timestamp when needed;
//! 6. validate dataset-level invariants and transpose to a [`Table`].

pub(crate) mod book;
pub(crate) mod funding;
pub(crate) mod klines;
pub(crate) mod liquidation;
pub(crate) mod metrics;
pub(crate) mod trades;

use csv::StringRecord;
use rayon::prelude::*;

use crate::config::Config;
use crate::error::{Error, Result};
use crate::source::csv::{FileRows, Header};
use crate::source::files::Scan;
use crate::source::zip;
use crate::table::Table;
use crate::types::{Dataset, Period, day_start_ms, utc_day_index};

/// Row-level counters accumulated across every selected file.
#[derive(Debug, Default)]
pub(crate) struct Stats {
    /// Rows decoded before overlap and window filtering.
    pub decoded: usize,
    /// Monthly rows discarded because a daily file covers their UTC day.
    pub overlap_dropped: usize,
    /// Rows outside the requested date window.
    pub window_dropped: usize,
    /// Malformed rows dropped under [`BadRowPolicy::Skip`](crate::BadRowPolicy::Skip).
    pub skipped: usize,
    /// Bounded samples of skipped rows.
    pub samples: Vec<String>,
    /// Retained rows that came from daily files.
    pub kept_daily: usize,
    /// Retained rows that came from monthly files.
    pub kept_monthly: usize,
}

impl Stats {
    fn absorb(&mut self, skipped: usize, samples: &[String]) {
        self.skipped += skipped;
        for sample in samples {
            if self.samples.len() < crate::source::csv::MAX_SKIP_SAMPLES {
                self.samples.push(sample.clone());
            }
        }
    }
}

/// Rows of one dataset after merge and filtering.
pub(crate) struct LoadedRows<T> {
    pub rows: Vec<T>,
    pub stats: Stats,
}

/// Final dataset payload before the report is attached.
pub(crate) struct Built {
    pub table: Table,
    pub stats: Stats,
    pub gaps: usize,
    pub duplicates_removed: usize,
}

/// Primary timestamp column of a dataset, always stored as `i64` milliseconds.
pub(crate) const fn time_column(dataset: Dataset) -> &'static str {
    match dataset {
        Dataset::AggTrades => "transact_time",
        Dataset::Trades => "time",
        Dataset::Klines
        | Dataset::IndexPriceKlines
        | Dataset::MarkPriceKlines
        | Dataset::PremiumIndexKlines => "open_time",
        Dataset::BookDepth => "timestamp",
        Dataset::BookTicker => "event_time",
        Dataset::FundingRate => "calc_time",
        Dataset::LiquidationSnapshot => "time",
        Dataset::Metrics => "create_time",
    }
}

/// Load the dataset described by `config`, reusing an existing scan.
///
/// # Errors
///
/// Propagates file, row and data-integrity errors from the dataset loader.
pub(crate) fn load(config: &Config, scan: &Scan) -> Result<Built> {
    match config.dataset() {
        Dataset::Klines
        | Dataset::IndexPriceKlines
        | Dataset::MarkPriceKlines
        | Dataset::PremiumIndexKlines => klines::load(config, scan),
        Dataset::AggTrades => trades::load_agg_trades(config, scan),
        Dataset::Trades => trades::load_trades(config, scan),
        Dataset::BookTicker => book::load_book_ticker(config, scan),
        Dataset::BookDepth => book::load_book_depth(config, scan),
        Dataset::FundingRate => funding::load(config, scan),
        Dataset::LiquidationSnapshot => liquidation::load(config, scan),
        Dataset::Metrics => metrics::load(config, scan),
    }
}

/// Read every selected file, validate row timestamps and merge.
///
/// `parse` must not check the timestamp; this function validates it against
/// the calendar period of the file before any filtering, so a row stored in
/// the wrong daily/monthly file is reported as row-level damage.
pub(crate) fn read_dataset<T, P, K>(
    config: &Config,
    scan: &Scan,
    header: Header,
    parse: P,
    time: K,
) -> Result<LoadedRows<T>>
where
    T: Send,
    P: Fn(&StringRecord) -> std::result::Result<T, String> + Sync,
    K: Fn(&T) -> i64 + Sync,
{
    let outcomes: Vec<Result<(crate::layout::ArchiveFile, FileRows<T>)>> = scan
        .files
        .par_iter()
        .map(|file| {
            let parse_row = |row: &StringRecord| {
                let value = parse(row)?;
                let timestamp = time(&value);
                if !file.contains(timestamp) {
                    return Err(format!(
                        "timestamp {timestamp} is outside file period [{}, {})",
                        file.start_ms, file.end_ms
                    ));
                }
                Ok(value)
            };
            let rows = zip::read_csv(&file.path, header, config.bad_rows(), parse_row)?;
            Ok((file.clone(), rows))
        })
        .collect();

    let window = Window::new(config);
    let mut stats = Stats::default();
    let mut rows = Vec::new();

    for outcome in outcomes {
        let (file, file_rows) = outcome?;
        stats.decoded += file_rows.rows.len();
        stats.absorb(file_rows.skipped, &file_rows.samples);

        let monthly = file.period == Period::Monthly;
        for row in file_rows.rows {
            let timestamp = time(&row);
            let day = utc_day_index(timestamp);
            if monthly && scan.daily_days.contains(&day) {
                stats.overlap_dropped += 1;
                continue;
            }
            if !window.contains(timestamp) {
                stats.window_dropped += 1;
                continue;
            }
            if monthly {
                stats.kept_monthly += 1;
            } else {
                stats.kept_daily += 1;
            }
            rows.push(row);
        }
    }

    if rows.windows(2).any(|pair| time(&pair[0]) > time(&pair[1])) {
        rows.sort_by_key(|row| time(row));
    }

    Ok(LoadedRows { rows, stats })
}

/// Inclusive-start / inclusive-end UTC date window.
struct Window {
    start_ms: Option<i64>,
    end_ms: Option<i64>,
}

impl Window {
    fn new(config: &Config) -> Self {
        Self {
            start_ms: config.start_date().map(day_start_ms),
            end_ms: config
                .end_date()
                .map(|date| day_start_ms(date).saturating_add(86_399_999)),
        }
    }

    fn contains(&self, timestamp_ms: i64) -> bool {
        self.start_ms.is_none_or(|start| timestamp_ms >= start)
            && self.end_ms.is_none_or(|end| timestamp_ms <= end)
    }
}

/// Build the final payload and ensure the table row count matches the stats.
///
/// `duplicates_removed` rows were retained during the merge and then dropped
/// from the final payload, so the expected length is `kept - removed`.
pub(crate) fn finish(
    table: Table,
    stats: Stats,
    gaps: usize,
    duplicates_removed: usize,
) -> Result<Built> {
    let kept = stats.kept_daily + stats.kept_monthly;
    let expected = kept.checked_sub(duplicates_removed).ok_or_else(|| {
        Error::data(format!(
            "{duplicates_removed} duplicate rows removed but only {kept} rows were retained"
        ))
    })?;
    if table.len() != expected {
        return Err(Error::data(format!(
            "table has {} rows but {expected} were retained",
            table.len()
        )));
    }
    Ok(Built {
        table,
        stats,
        gaps,
        duplicates_removed,
    })
}

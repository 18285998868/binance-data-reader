//! Public read entry point: validate, scan, decode, merge and report.

use std::time::Instant;

use crate::config::Config;
use crate::dataset::{self, time_column};
use crate::error::Result;
use crate::report::LoadReport;
use crate::source::files;
use crate::table::Table;

/// A loaded dataset and its report.
#[derive(Debug, Clone)]
pub struct DatasetData {
    /// Equal-length typed columns in archive field order.
    pub table: Table,
    /// Scan, filtering and integrity statistics.
    pub report: LoadReport,
}

/// Read one symbol × one dataset from the configured archive root.
///
/// Both `daily` and `monthly` trees are scanned. When both cover the same UTC
/// day, daily rows win and monthly rows for that day are discarded; months not
/// covered by daily files are read in full.
///
/// # Errors
///
/// Returns configuration, filesystem, ZIP, row and data-integrity errors as
/// described by [`crate::Error`]. Under the default [`crate::BadRowPolicy::Fail`],
/// the first malformed row aborts the load.
///
/// # Examples
///
/// ```no_run
/// use binance_data_reader::{Config, Dataset, read};
///
/// let config = Config::builder("/data/data/binance", Dataset::Klines, "BTCUSDT")
///     .interval("1m".parse()?)
///     .build()?;
/// let data = read(&config)?;
/// println!("{} rows", data.table.len());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[tracing::instrument(skip_all, fields(
    dataset = %config.dataset(),
    market = %config.market(),
    symbol = config.symbol().as_str(),
    interval = ?config.interval(),
))]
pub fn read(config: &Config) -> Result<DatasetData> {
    let started = Instant::now();
    let scan = files::scan(config)?;
    let built = dataset::load(config, &scan)?;

    let rows = built.table.len();
    let time = time_column(config.dataset());
    let first_time = built
        .table
        .i64(time)
        .and_then(|values| values.first().copied());
    let last_time = built
        .table
        .i64(time)
        .and_then(|values| values.last().copied());
    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);

    let report = LoadReport {
        market: config.market(),
        dataset: config.dataset(),
        symbol: config.symbol().as_str().to_owned(),
        interval: config.interval(),
        start_date: config.start_date(),
        end_date: config.end_date(),
        bad_rows: config.bad_rows(),
        files_daily: scan.files_daily,
        files_monthly: scan.files_monthly,
        rows_decoded: built.stats.decoded,
        rows_dropped_overlap: built.stats.overlap_dropped,
        rows_dropped_window: built.stats.window_dropped,
        rows_skipped: built.stats.skipped,
        skipped_samples: built.stats.samples,
        duplicates_removed: built.duplicates_removed,
        rows,
        rows_daily: built.stats.kept_daily,
        rows_monthly: built.stats.kept_monthly,
        first_time,
        last_time,
        gaps: built.gaps,
        elapsed_ms,
    };

    tracing::info!(
        rows,
        files_daily = report.files_daily,
        files_monthly = report.files_monthly,
        rows_skipped = report.rows_skipped,
        rows_dropped_overlap = report.rows_dropped_overlap,
        rows_dropped_window = report.rows_dropped_window,
        gaps = report.gaps,
        elapsed_ms,
        "loaded dataset"
    );

    Ok(DatasetData {
        table: built.table,
        report,
    })
}

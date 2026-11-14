//! Archive layout facts shared by the scanner and catalog.
//!
//! The only supported layout is the downloader's:
//!
//! ```text
//! {root}/data/{market}/{period}/{dataset}/{symbol}[/{interval}]/*.zip
//! ```

use std::path::{Path, PathBuf};

use chrono::NaiveDate;

use crate::types::{Dataset, Interval, Market, Period, day_start_ms};

/// Subdirectory between the configured root and the market tree.
pub(crate) const DATA_DIR: &str = "data";

/// Directory holding every archive file of one request.
pub(crate) fn dataset_dir(
    root: &Path,
    market: Market,
    period: Period,
    dataset: Dataset,
    symbol: &str,
    interval: Option<Interval>,
) -> PathBuf {
    let mut dir = root
        .join(DATA_DIR)
        .join(market.as_path())
        .join(period.as_str())
        .join(dataset.as_str())
        .join(dataset.storage_symbol(symbol));
    if let Some(interval) = interval {
        dir.push(interval.as_str());
    }
    dir
}

/// File-name key between the symbol and date: the interval for kline-shaped
/// datasets, otherwise the dataset name.
pub(crate) fn file_key(dataset: Dataset, interval: Option<Interval>) -> &'static str {
    match interval {
        Some(interval) => interval.as_str(),
        None => dataset.as_str(),
    }
}

/// Exact archive file name, e.g. `BTCUSDT-1m-2023-06.zip`.
#[cfg(test)]
pub(crate) fn file_name(
    dataset: Dataset,
    symbol: &str,
    interval: Option<Interval>,
    period: Period,
    date: NaiveDate,
) -> String {
    format!(
        "{}-{}-{}.zip",
        dataset.storage_symbol(symbol),
        file_key(dataset, interval),
        period.format(date)
    )
}

/// One selected archive file and the calendar period it must contain.
#[derive(Debug, Clone)]
pub(crate) struct ArchiveFile {
    /// Absolute or root-relative path as configured.
    pub path: PathBuf,
    /// Cadence of the file.
    pub period: Period,
    /// First day of the file's period.
    pub date: NaiveDate,
    /// Inclusive lower timestamp bound (period start).
    pub start_ms: i64,
    /// Exclusive upper timestamp bound (next period start).
    pub end_ms: i64,
}

impl ArchiveFile {
    /// Build a file descriptor from its calendar period.
    pub(crate) fn new(path: PathBuf, period: Period, date: NaiveDate) -> Self {
        let start_ms = day_start_ms(date);
        let end_ms = period.next_start(date).map_or(i64::MAX, day_start_ms);
        Self {
            path,
            period,
            date,
            start_ms,
            end_ms,
        }
    }

    /// Whether `timestamp_ms` falls inside the file's calendar period.
    pub(crate) fn contains(&self, timestamp_ms: i64) -> bool {
        timestamp_ms >= self.start_ms && timestamp_ms < self.end_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    #[test]
    fn paths_match_downloader_layout() {
        let dir = dataset_dir(
            Path::new("/data"),
            Market::Um,
            Period::Daily,
            Dataset::Klines,
            "BTCUSDT",
            Some(Interval::M1),
        );
        assert_eq!(
            dir,
            Path::new("/data/data/futures/um/daily/klines/BTCUSDT/1m")
        );
    }

    #[test]
    fn index_paths_use_the_underlying_symbol() {
        let dir = dataset_dir(
            Path::new("/data"),
            Market::Cm,
            Period::Daily,
            Dataset::IndexPriceKlines,
            "BTCUSD_PERP",
            Some(Interval::M1),
        );
        assert_eq!(
            dir,
            Path::new("/data/data/futures/cm/daily/indexPriceKlines/BTCUSD/1m")
        );
    }

    #[test]
    fn file_names_follow_the_archive() {
        assert_eq!(
            file_name(
                Dataset::Klines,
                "BTCUSDT",
                Some(Interval::M1),
                Period::Monthly,
                date(2023, 6, 1)
            ),
            "BTCUSDT-1m-2023-06.zip"
        );
        assert_eq!(
            file_name(
                Dataset::FundingRate,
                "BTCUSDT",
                None,
                Period::Monthly,
                date(2023, 6, 1)
            ),
            "BTCUSDT-fundingRate-2023-06.zip"
        );
    }

    #[test]
    fn period_bounds_are_half_open() {
        let daily = ArchiveFile::new(PathBuf::from("x"), Period::Daily, date(2023, 6, 30));
        assert!(daily.contains(day_start_ms(date(2023, 6, 30))));
        assert!(!daily.contains(day_start_ms(date(2023, 7, 1))));

        let monthly = ArchiveFile::new(PathBuf::from("x"), Period::Monthly, date(2023, 6, 1));
        assert!(monthly.contains(day_start_ms(date(2023, 6, 1))));
        assert!(!monthly.contains(day_start_ms(date(2023, 7, 1))));
    }
}

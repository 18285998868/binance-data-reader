//! Inventory of what actually exists below an archive root.
//!
//! [`catalog`] walks the known support matrix and reports, per
//! market/cadence/dataset/symbol/interval, how many archive files exist and
//! their first/last periods. It is read-only and lenient: unrelated files and
//! malformed directory names are ignored so a stray file does not hide the
//! rest of the inventory. Loading, by contrast, is strict.

use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use serde::Serialize;

use crate::error::{Error, Result};
use crate::layout;
use crate::types::{Dataset, Interval, Market, Period, Symbol};

/// Flat inventory of archive files.
#[derive(Debug, Clone, Serialize)]
pub struct Catalog {
    /// Root that was scanned.
    pub root: PathBuf,
    /// One entry per symbol (and interval for kline-shaped datasets).
    pub entries: Vec<CatalogEntry>,
}

/// Files available for one symbol × dataset × interval.
#[derive(Debug, Clone, Serialize)]
pub struct CatalogEntry {
    /// Market.
    pub market: Market,
    /// Cadence.
    pub period: Period,
    /// Dataset family.
    pub dataset: Dataset,
    /// Symbol as spelled by the archive directory.
    pub symbol: String,
    /// Interval for kline-shaped datasets.
    pub interval: Option<Interval>,
    /// Number of archive ZIPs with a valid name.
    pub files: usize,
    /// First file period (first day of the month for monthly files).
    pub first_date: Option<NaiveDate>,
    /// Last file period (first day of the month for monthly files).
    pub last_date: Option<NaiveDate>,
}

/// Optional catalog filters; all fields default to "no filter".
#[derive(Debug, Clone, Default)]
pub struct CatalogFilter {
    /// Keep only this market.
    pub market: Option<Market>,
    /// Keep only this cadence.
    pub period: Option<Period>,
    /// Keep only this dataset.
    pub dataset: Option<Dataset>,
    /// Keep only this symbol.
    pub symbol: Option<Symbol>,
}

/// Scan `{root}/data/{market}/{period}/{dataset}/...`.
///
/// An empty result is valid and returns an empty `entries` vector; only a
/// missing root or an unreadable directory is an error.
///
/// # Errors
///
/// Returns [`Error::RootNotFound`] when `root` is missing or not a directory,
/// and [`Error::Io`] when a directory cannot be read.
pub fn catalog(root: &Path, filter: &CatalogFilter) -> Result<Catalog> {
    match std::fs::metadata(root) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => {
            return Err(Error::RootNotFound {
                root: root.to_path_buf(),
            });
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::RootNotFound {
                root: root.to_path_buf(),
            });
        }
        Err(error) => return Err(Error::io(format!("stat {}", root.display()), error)),
    }

    let mut entries = Vec::new();
    for market in Market::ALL {
        if filter.market.is_some_and(|wanted| wanted != market) {
            continue;
        }
        for period in Period::ALL {
            if filter.period.is_some_and(|wanted| wanted != period) {
                continue;
            }
            for dataset in Dataset::ALL {
                if filter.dataset.is_some_and(|wanted| wanted != dataset) {
                    continue;
                }
                if !dataset.supports(market, period) {
                    continue;
                }
                let dataset_dir = root
                    .join(layout::DATA_DIR)
                    .join(market.as_path())
                    .join(period.as_str())
                    .join(dataset.as_str());
                let symbols = match std::fs::read_dir(&dataset_dir) {
                    Ok(entries) => entries,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(error) => {
                        return Err(Error::io(
                            format!("read_dir {}", dataset_dir.display()),
                            error,
                        ));
                    }
                };

                for symbol_entry in symbols {
                    let symbol_entry = symbol_entry.map_err(|e| {
                        Error::io(format!("read_dir {} entry", dataset_dir.display()), e)
                    })?;
                    let symbol_path = symbol_entry.path();
                    if !symbol_path.is_dir() {
                        continue;
                    }
                    let Some(symbol) = symbol_path.file_name().and_then(std::ffi::OsStr::to_str)
                    else {
                        continue;
                    };
                    if filter
                        .symbol
                        .as_ref()
                        .is_some_and(|wanted| wanted.as_str() != symbol)
                    {
                        continue;
                    }

                    if dataset.needs_interval() {
                        let intervals = match std::fs::read_dir(&symbol_path) {
                            Ok(entries) => entries,
                            Err(error) => {
                                return Err(Error::io(
                                    format!("read_dir {}", symbol_path.display()),
                                    error,
                                ));
                            }
                        };
                        for interval_entry in intervals {
                            let interval_entry = interval_entry.map_err(|e| {
                                Error::io(format!("read_dir {} entry", symbol_path.display()), e)
                            })?;
                            let interval_path = interval_entry.path();
                            if !interval_path.is_dir() {
                                continue;
                            }
                            let Some(raw) =
                                interval_path.file_name().and_then(std::ffi::OsStr::to_str)
                            else {
                                continue;
                            };
                            let Ok(interval) = raw.parse::<Interval>() else {
                                tracing::debug!(
                                    path = %interval_path.display(),
                                    "ignoring non-interval directory in catalog"
                                );
                                continue;
                            };
                            if !interval.available_for(market, period) {
                                continue;
                            }
                            push_entry(
                                &mut entries,
                                &interval_path,
                                market,
                                period,
                                dataset,
                                symbol,
                                Some(interval),
                            );
                        }
                    } else {
                        push_entry(
                            &mut entries,
                            &symbol_path,
                            market,
                            period,
                            dataset,
                            symbol,
                            None,
                        );
                    }
                }
            }
        }
    }

    entries.sort_by(|left, right| {
        left.market
            .cmp(&right.market)
            .then_with(|| left.period.cmp(&right.period))
            .then_with(|| left.dataset.cmp(&right.dataset))
            .then_with(|| left.symbol.cmp(&right.symbol))
            .then_with(|| left.interval.cmp(&right.interval))
    });

    Ok(Catalog {
        root: root.to_path_buf(),
        entries,
    })
}

fn push_entry(
    entries: &mut Vec<CatalogEntry>,
    dir: &Path,
    market: Market,
    period: Period,
    dataset: Dataset,
    symbol: &str,
    interval: Option<Interval>,
) {
    let (files, first_date, last_date) = summarize(dir, dataset, symbol, interval, period);
    if files == 0 {
        return;
    }
    entries.push(CatalogEntry {
        market,
        period,
        dataset,
        symbol: symbol.to_owned(),
        interval,
        files,
        first_date,
        last_date,
    });
}

/// Count valid archive files and their date range, ignoring unrelated names.
fn summarize(
    dir: &Path,
    dataset: Dataset,
    symbol: &str,
    interval: Option<Interval>,
    period: Period,
) -> (usize, Option<NaiveDate>, Option<NaiveDate>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            tracing::debug!(path = %dir.display(), %error, "catalog could not read directory");
            return (0, None, None);
        }
    };
    let prefix = format!(
        "{}-{}-",
        dataset.storage_symbol(symbol),
        layout::file_key(dataset, interval)
    );
    let mut files = 0usize;
    let mut first = None;
    let mut last = None;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let is_zip = path
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"));
        if !is_zip {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(std::ffi::OsStr::to_str) else {
            continue;
        };
        let Some(Ok(date)) = period.parse_file_date(stem) else {
            continue;
        };
        if stem != format!("{prefix}{}", period.format(date)) {
            continue;
        }
        files += 1;
        first = Some(first.map_or(date, |current: NaiveDate| current.min(date)));
        last = Some(last.map_or(date, |current: NaiveDate| current.max(date)));
    }

    (files, first, last)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn touch(dir: &Path, name: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(name), b"x").unwrap();
    }

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    #[test]
    fn catalog_reports_files_and_date_ranges() {
        let tmp = TempDir::new().unwrap();
        let dir = layout::dataset_dir(
            tmp.path(),
            Market::Um,
            Period::Monthly,
            Dataset::Klines,
            "BTCUSDT",
            Some(Interval::M1),
        );
        touch(
            &dir,
            &layout::file_name(
                Dataset::Klines,
                "BTCUSDT",
                Some(Interval::M1),
                Period::Monthly,
                date(2023, 6, 1),
            ),
        );
        touch(
            &dir,
            &layout::file_name(
                Dataset::Klines,
                "BTCUSDT",
                Some(Interval::M1),
                Period::Monthly,
                date(2023, 5, 1),
            ),
        );
        touch(&dir, "README.txt");

        let catalog = catalog(tmp.path(), &CatalogFilter::default()).unwrap();
        assert_eq!(catalog.entries.len(), 1);
        let entry = &catalog.entries[0];
        assert_eq!(entry.files, 2);
        assert_eq!(entry.first_date, Some(date(2023, 5, 1)));
        assert_eq!(entry.last_date, Some(date(2023, 6, 1)));
    }

    #[test]
    fn filters_and_support_matrix_apply() {
        let tmp = TempDir::new().unwrap();
        let klines = layout::dataset_dir(
            tmp.path(),
            Market::Spot,
            Period::Daily,
            Dataset::Klines,
            "BTCUSDT",
            Some(Interval::M1),
        );
        touch(
            &klines,
            &layout::file_name(
                Dataset::Klines,
                "BTCUSDT",
                Some(Interval::M1),
                Period::Daily,
                date(2023, 6, 30),
            ),
        );
        let funding = layout::dataset_dir(
            tmp.path(),
            Market::Um,
            Period::Monthly,
            Dataset::FundingRate,
            "BTCUSDT",
            None,
        );
        touch(
            &funding,
            &layout::file_name(
                Dataset::FundingRate,
                "BTCUSDT",
                None,
                Period::Monthly,
                date(2023, 6, 1),
            ),
        );

        let filter = CatalogFilter {
            dataset: Some(Dataset::FundingRate),
            ..CatalogFilter::default()
        };
        let found = catalog(tmp.path(), &filter).unwrap();
        assert_eq!(found.entries.len(), 1);
        assert_eq!(found.entries[0].dataset, Dataset::FundingRate);

        let filter = CatalogFilter {
            symbol: Some("ETHUSDT".parse().unwrap()),
            ..CatalogFilter::default()
        };
        assert!(catalog(tmp.path(), &filter).unwrap().entries.is_empty());
    }

    #[test]
    fn malformed_interval_dirs_and_files_are_ignored() {
        let tmp = TempDir::new().unwrap();
        let dir = layout::dataset_dir(
            tmp.path(),
            Market::Um,
            Period::Daily,
            Dataset::Klines,
            "BTCUSDT",
            Some(Interval::M1),
        )
        .parent()
        .unwrap()
        .to_path_buf();
        touch(&dir.join("not-an-interval"), "BTCUSDT-1m-2023-06-30.zip");
        touch(&dir.join("1m"), "BTCUSDT-1m-2023-06-30.zip");
        touch(&dir.join("1m"), "BTCUSDT-1m-bad.zip");

        let catalog = catalog(tmp.path(), &CatalogFilter::default()).unwrap();
        assert_eq!(catalog.entries.len(), 1);
        assert_eq!(catalog.entries[0].files, 1);
    }

    #[test]
    fn missing_root_is_typed() {
        let error =
            catalog(Path::new("/definitely/not/here"), &CatalogFilter::default()).unwrap_err();
        assert!(matches!(error, Error::RootNotFound { .. }));
    }
}

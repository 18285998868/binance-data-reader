//! Directory scanning and daily/monthly overlap bookkeeping.
//!
//! Both cadences are scanned. When a daily file and a monthly file cover the
//! same UTC day, the monthly rows for that day are discarded before merging:
//! the daily archive is the finer-grained, authoritative copy. Overlap is
//! resolved by **calendar cover**, not by timestamp equality, so multi-row
//! datasets (trades, book depth, liquidations) are handled correctly.

use std::collections::HashSet;

use crate::config::Config;
use crate::error::{Error, Result};
use crate::layout::{self, ArchiveFile};
use crate::types::{Period, day_start_ms, utc_day_index};

/// Result of scanning the archive tree for one request.
#[derive(Debug)]
pub(crate) struct Scan {
    /// Selected files, ordered daily-first then by date and path.
    pub files: Vec<ArchiveFile>,
    /// UTC day indices covered by selected daily files.
    pub daily_days: HashSet<i64>,
    /// Number of selected daily files.
    pub files_daily: usize,
    /// Number of selected monthly files.
    pub files_monthly: usize,
}

/// Scan `{root}/data/{market}/{daily,monthly}/{dataset}/...` for this request.
///
/// Files outside the requested date window are dropped before reading. Any
/// `.zip` whose name does not match the exact archive pattern is a hard error:
/// a silent skip would hide a mis-downloaded or misplaced file.
///
/// # Errors
///
/// Returns [`Error::RootNotFound`] when the root is missing,
/// [`Error::Io`] for directory read failures, [`Error::File`] for malformed
/// ZIP names, and [`Error::NoFiles`] when nothing matched.
pub(crate) fn scan(config: &Config) -> Result<Scan> {
    let root = config.root();
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
        Err(error) => {
            return Err(Error::io(format!("stat {}", root.display()), error));
        }
    }

    let market = config.market();
    let dataset = config.dataset();
    let symbol = config.symbol().as_str();
    let interval = config.interval();
    let storage_symbol = dataset.storage_symbol(symbol);
    let expected_key = layout::file_key(dataset, interval);

    let mut files = Vec::new();
    let mut daily_days = HashSet::new();
    let mut checked_dirs = Vec::new();
    let mut existing_dirs = 0usize;
    let mut candidates = 0usize;

    for period in Period::ALL {
        if !dataset.supports(market, period) {
            continue;
        }
        let dir = layout::dataset_dir(root, market, period, dataset, symbol, interval);
        checked_dirs.push(dir.clone());
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(Error::io(format!("read_dir {}", dir.display()), error));
            }
        };
        existing_dirs += 1;

        for entry in entries {
            let entry =
                entry.map_err(|e| Error::io(format!("read_dir {} entry", dir.display()), e))?;
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

            let stem = path
                .file_stem()
                .and_then(std::ffi::OsStr::to_str)
                .ok_or_else(|| Error::File {
                    path: path.clone(),
                    detail: "file name is not valid UTF-8".to_owned(),
                })?;
            let parsed = period.parse_file_date(stem).ok_or_else(|| Error::File {
                path: path.clone(),
                detail: format!(
                    "malformed archive file name; expected {storage_symbol}-{expected_key}-<{}>.zip",
                    match period {
                        Period::Daily => "YYYY-MM-DD",
                        Period::Monthly => "YYYY-MM",
                    }
                ),
            })?;
            let date = parsed.map_err(|detail| Error::File {
                path: path.clone(),
                detail,
            })?;
            let date_text = period.format(date);
            let prefix =
                stem.strip_suffix(&format!("-{date_text}"))
                    .ok_or_else(|| Error::File {
                        path: path.clone(),
                        detail: format!("malformed archive file name: {stem}"),
                    })?;
            let expected_prefix = format!("{storage_symbol}-{expected_key}");
            if prefix != expected_prefix {
                let detail = format!(
                    "wrong file for this request: name starts with {prefix:?}, expected {expected_prefix:?}"
                );
                return Err(Error::File { path, detail });
            }
            candidates += 1;

            if !intersects(date, period, config) {
                continue;
            }

            let file = ArchiveFile::new(path, period, date);
            if period == Period::Daily {
                daily_days.insert(utc_day_index(day_start_ms(date)));
            }
            files.push(file);
        }
    }

    files.sort_by(|left, right| {
        left.period
            .cmp(&right.period)
            .then_with(|| left.date.cmp(&right.date))
            .then_with(|| left.path.cmp(&right.path))
    });

    // A monthly archive is only worth decoding for days that are inside the
    // requested window and not already covered by a daily file. This avoids
    // parsing a whole month of tick data to serve a one-day request.
    files.retain(|file| {
        file.period == Period::Daily || monthly_contributes(file, &daily_days, config)
    });

    if files.is_empty() {
        let dirs: Vec<String> = checked_dirs
            .iter()
            .map(|dir| dir.display().to_string())
            .collect();
        let detail = if existing_dirs == 0 {
            format!("no dataset directories exist; checked {}", dirs.join(", "))
        } else if candidates == 0 {
            format!(
                "dataset directories exist but contain no archive ZIP files; checked {}",
                dirs.join(", ")
            )
        } else {
            format!(
                "{candidates} archive file(s) exist but all fall outside the requested date range"
            )
        };
        return Err(Error::NoFiles {
            root: root.to_path_buf(),
            market,
            dataset,
            symbol: symbol.to_owned(),
            interval,
            detail,
        });
    }

    let files_daily = files
        .iter()
        .filter(|file| file.period == Period::Daily)
        .count();
    let files_monthly = files.len() - files_daily;

    Ok(Scan {
        files,
        daily_days,
        files_daily,
        files_monthly,
    })
}

fn intersects(date: chrono::NaiveDate, period: Period, config: &Config) -> bool {
    if let Some(start) = config.start_date()
        && period.next_start(date).is_some_and(|end| end <= start)
    {
        return false;
    }
    if let Some(end) = config.end_date()
        && date > end
    {
        return false;
    }
    true
}

/// Whether a monthly file still has a day that is both requested and not
/// covered by a daily file.
fn monthly_contributes(file: &ArchiveFile, daily_days: &HashSet<i64>, config: &Config) -> bool {
    let mut day = file.start_ms;
    while day < file.end_ms {
        if !daily_days.contains(&utc_day_index(day)) && day_in_window(day, config) {
            return true;
        }
        day += 86_400_000;
    }
    false
}

fn day_in_window(day_start: i64, config: &Config) -> bool {
    config
        .start_date()
        .is_none_or(|start| day_start >= day_start_ms(start))
        && config
            .end_date()
            .is_none_or(|end| day_start <= day_start_ms(end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Dataset, Market};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn config(root: &Path, dataset: Dataset, interval: Option<&str>) -> Config {
        let mut builder = Config::builder(root, dataset, "BTCUSDT");
        if let Some(interval) = interval {
            builder = builder.interval(interval.parse().unwrap());
        }
        builder.build().unwrap()
    }

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, b"not a real zip").unwrap();
    }

    #[test]
    fn scans_both_cadences_and_marks_daily_days() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let daily = layout::dataset_dir(
            root,
            Market::Um,
            Period::Daily,
            Dataset::Klines,
            "BTCUSDT",
            Some(crate::types::Interval::M1),
        );
        let monthly = layout::dataset_dir(
            root,
            Market::Um,
            Period::Monthly,
            Dataset::Klines,
            "BTCUSDT",
            Some(crate::types::Interval::M1),
        );
        touch(&daily.join(layout::file_name(
            Dataset::Klines,
            "BTCUSDT",
            Some(crate::types::Interval::M1),
            Period::Daily,
            chrono::NaiveDate::from_ymd_opt(2023, 6, 25).unwrap(),
        )));
        touch(&monthly.join(layout::file_name(
            Dataset::Klines,
            "BTCUSDT",
            Some(crate::types::Interval::M1),
            Period::Monthly,
            chrono::NaiveDate::from_ymd_opt(2023, 6, 1).unwrap(),
        )));

        let scan = scan(&config(root, Dataset::Klines, Some("1m"))).unwrap();
        assert_eq!(scan.files.len(), 2);
        assert_eq!(scan.files_daily, 1);
        assert_eq!(scan.files_monthly, 1);
        assert_eq!(scan.daily_days.len(), 1);
    }

    #[test]
    fn unrelated_zips_and_non_zips_are_rejected_or_ignored() {
        let tmp = TempDir::new().unwrap();
        let dir = layout::dataset_dir(
            tmp.path(),
            Market::Um,
            Period::Daily,
            Dataset::Metrics,
            "BTCUSDT",
            None,
        );
        touch(&dir.join("README.txt"));
        let config = config(tmp.path(), Dataset::Metrics, None);
        assert!(matches!(scan(&config), Err(Error::NoFiles { .. })));

        touch(&dir.join("BTCUSDT-metrics-latest.zip"));
        let error = scan(&config).unwrap_err();
        assert!(matches!(error, Error::File { .. }), "{error}");
    }

    #[test]
    fn date_window_prunes_files_and_reports_missing_data() {
        let tmp = TempDir::new().unwrap();
        let dir = layout::dataset_dir(
            tmp.path(),
            Market::Um,
            Period::Daily,
            Dataset::Metrics,
            "BTCUSDT",
            None,
        );
        let date = chrono::NaiveDate::from_ymd_opt(2023, 6, 25).unwrap();
        touch(&dir.join(layout::file_name(
            Dataset::Metrics,
            "BTCUSDT",
            None,
            Period::Daily,
            date,
        )));

        let config = Config::builder(tmp.path(), Dataset::Metrics, "BTCUSDT")
            .date_range(
                Some(chrono::NaiveDate::from_ymd_opt(2023, 7, 1).unwrap()),
                None,
            )
            .build()
            .unwrap();
        let error = scan(&config).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("outside the requested date range"),
            "{error}"
        );
    }

    #[test]
    fn fully_covered_monthly_files_are_not_selected() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let day = chrono::NaiveDate::from_ymd_opt(2023, 6, 25).unwrap();
        touch(
            &layout::dataset_dir(
                root,
                Market::Um,
                Period::Daily,
                Dataset::Klines,
                "BTCUSDT",
                Some(crate::types::Interval::M1),
            )
            .join(layout::file_name(
                Dataset::Klines,
                "BTCUSDT",
                Some(crate::types::Interval::M1),
                Period::Daily,
                day,
            )),
        );
        touch(
            &layout::dataset_dir(
                root,
                Market::Um,
                Period::Monthly,
                Dataset::Klines,
                "BTCUSDT",
                Some(crate::types::Interval::M1),
            )
            .join(layout::file_name(
                Dataset::Klines,
                "BTCUSDT",
                Some(crate::types::Interval::M1),
                Period::Monthly,
                chrono::NaiveDate::from_ymd_opt(2023, 6, 1).unwrap(),
            )),
        );

        let all = scan(&config(root, Dataset::Klines, Some("1m"))).unwrap();
        assert_eq!(all.files_monthly, 1);

        let windowed = Config::builder(root, Dataset::Klines, "BTCUSDT")
            .interval(crate::types::Interval::M1)
            .date_range(Some(day), Some(day))
            .build()
            .unwrap();
        let scan = scan(&windowed).unwrap();
        assert_eq!(scan.files_daily, 1);
        assert_eq!(scan.files_monthly, 0);
        assert_eq!(scan.files.len(), 1);
    }

    #[test]
    fn missing_root_is_a_typed_error() {
        let error = scan(
            &Config::builder("/definitely/not/here", Dataset::Trades, "BTCUSDT")
                .build()
                .unwrap(),
        )
        .unwrap_err();
        assert!(matches!(error, Error::RootNotFound { .. }));
    }

    #[test]
    fn wrong_symbol_file_fails() {
        let tmp = TempDir::new().unwrap();
        let dir = layout::dataset_dir(
            tmp.path(),
            Market::Um,
            Period::Daily,
            Dataset::Metrics,
            "BTCUSDT",
            None,
        );
        touch(&dir.join("ETHUSDT-metrics-2023-06-25.zip"));
        let error = scan(&config(tmp.path(), Dataset::Metrics, None)).unwrap_err();
        assert!(error.to_string().contains("wrong file"), "{error}");
    }
}

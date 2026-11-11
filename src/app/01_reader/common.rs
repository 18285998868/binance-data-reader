//! Shared archive plumbing: path scanning, date windows, zip/csv decoding and
//! per-field parsers used by every dataset reader.
//!
//! The layout mirrors the public archive exactly:
//! `{dir}/binance/data/{market}/{daily|monthly}/{dataset}/{symbol}[/{interval}]/*.zip`.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::Instant;

use chrono::{Days, Months, NaiveDate, NaiveDateTime};
use csv::StringRecord;
use rayon::prelude::*;

use crate::error::{ReaderError, Result};
use crate::types::{Dataset, Interval, Market, Symbol};

/// Maximum number of skipped-row samples retained for warnings.
const MAX_SKIP_SAMPLES: usize = 5;
/// Maximum number of failed-file samples retained for the error message.
const MAX_FAILED_SAMPLES: usize = 5;
/// Archive cadence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Period {
    /// One file per day, named `YYYY-MM-DD`.
    Daily,
    /// One file per month, named `YYYY-MM`.
    Monthly,
}

impl Period {
    const ALL: [Self; 2] = [Self::Daily, Self::Monthly];

    const fn as_str(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Monthly => "monthly",
        }
    }

    /// First instant after the file's period; used for overlap filtering.
    fn next(self, start: NaiveDate) -> Result<NaiveDate> {
        let next = match self {
            Self::Daily => start.checked_add_days(Days::new(1)),
            Self::Monthly => start.checked_add_months(Months::new(1)),
        };
        next.ok_or_else(|| {
            ReaderError::Parse(format!(
                "date overflow while advancing {start} by one {self}"
            ))
        })
    }

    /// Parse the date suffix of a file stem (`SYMBOL[-INTERVAL]-KIND-DATE`).
    ///
    /// Returns `Ok(None)` when the suffix does not have a date shape (an
    /// unrelated zip), and `Err` when it looks like a date but is invalid
    /// (`2024-13-01`), which is a hard failure rather than a silent skip.
    fn parse_key_date(self, stem: &str) -> Result<Option<NaiveDate>> {
        let len = match self {
            Self::Daily => 10,
            Self::Monthly => 7,
        };
        if stem.len() < len {
            return Ok(None);
        }
        let Some(candidate) = stem.get(stem.len() - len..) else {
            return Ok(None);
        };
        let shaped = match self {
            Self::Daily => {
                candidate.len() == 10
                    && candidate.as_bytes()[4] == b'-'
                    && candidate.as_bytes()[7] == b'-'
            }
            Self::Monthly => candidate.len() == 7 && candidate.as_bytes()[4] == b'-',
        };
        if !shaped {
            return Ok(None);
        }
        let parsed = match self {
            Self::Daily => NaiveDate::parse_from_str(candidate, "%Y-%m-%d"),
            // `%Y-%m` alone is rejected by chrono (no unique date); pin day 1.
            Self::Monthly => NaiveDate::parse_from_str(&format!("{candidate}-01"), "%Y-%m-%d"),
        };
        parsed
            .map(Some)
            .map_err(|e| ReaderError::Parse(format!("invalid date in filename stem {stem:?}: {e}")))
    }
}

impl std::fmt::Display for Period {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Inclusive-start / exclusive-end overlap against an inclusive date window.
fn should_skip_period(
    file_start: NaiveDate,
    file_end_exclusive: NaiveDate,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
) -> bool {
    if let Some(start) = start_date
        && file_end_exclusive <= start
    {
        return true;
    }
    if let Some(end) = end_date
        && file_start > end
    {
        return true;
    }
    false
}

/// Row-level UTC date window: `[start 00:00, end 23:59:59.999]`.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DateWindow {
    start_ms: Option<i64>,
    end_inclusive_ms: Option<i64>,
}

impl DateWindow {
    /// Build from inclusive calendar bounds; `end` covers its whole UTC day.
    ///
    /// The end is stored as the last millisecond of the day instead of
    /// `end + 1 day`; this keeps `NaiveDate::MAX` representable and avoids
    /// silently dropping the upper bound on overflow.
    pub(crate) fn new(start: Option<NaiveDate>, end: Option<NaiveDate>) -> Self {
        let midnight_ms = |date: NaiveDate| {
            date.and_hms_opt(0, 0, 0)
                .expect("INVARIANT: NaiveDate always has a midnight instant")
                .and_utc()
                .timestamp_millis()
        };
        let end_of_day_ms = |date: NaiveDate| {
            date.and_hms_milli_opt(23, 59, 59, 999)
                .expect("INVARIANT: NaiveDate always has a last millisecond")
                .and_utc()
                .timestamp_millis()
        };
        Self {
            start_ms: start.map(midnight_ms),
            end_inclusive_ms: end.map(end_of_day_ms),
        }
    }

    pub(crate) fn contains(&self, timestamp_ms: i64) -> bool {
        if let Some(start) = self.start_ms
            && timestamp_ms < start
        {
            return false;
        }
        if let Some(end) = self.end_inclusive_ms
            && timestamp_ms > end
        {
            return false;
        }
        true
    }

    /// Drop rows outside the window; a no-op when both bounds are absent.
    pub(crate) fn trim<T>(&self, rows: Vec<T>, time: impl Fn(&T) -> i64) -> Vec<T> {
        if self.start_ms.is_none() && self.end_inclusive_ms.is_none() {
            return rows;
        }
        rows.into_iter()
            .filter(|row| self.contains(time(row)))
            .collect()
    }
}

/// Scan both archive cadences for matching zip files.
///
/// Existence is required for at least one of `daily`/`monthly`; a dataset
/// published only monthly (`fundingRate`) is therefore valid. Files whose
/// stem has no date shape are skipped, while date-shaped-but-invalid stems
/// fail the batch.
pub(crate) fn scan_zip_files(
    dir: &Path,
    market: Market,
    dataset: Dataset,
    symbol: &Symbol,
    interval: Option<&Interval>,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
) -> Result<Vec<PathBuf>> {
    let mut found_dir = false;
    let mut paths = Vec::new();
    let mut invalid = Vec::new();
    let storage_symbol = dataset.storage_symbol(symbol);

    for period in Period::ALL {
        let mut data_dir = dir
            .join("binance")
            .join("data")
            .join(market.as_path())
            .join(period.as_str())
            .join(dataset.as_str())
            .join(storage_symbol);
        if let Some(interval) = interval {
            data_dir = data_dir.join(interval.as_str());
        }
        let entries = match std::fs::read_dir(&data_dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                return Err(ReaderError::Io(std::io::Error::new(
                    e.kind(),
                    format!("read_dir {}: {e}", data_dir.display()),
                )));
            }
        };
        found_dir = true;
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    return Err(ReaderError::Io(std::io::Error::new(
                        e.kind(),
                        format!("read_dir {} entry: {e}", data_dir.display()),
                    )));
                }
            };
            let path = entry.path();
            if !has_zip_extension(&path) {
                continue;
            }
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let file_start = match period.parse_key_date(stem) {
                Ok(Some(date)) => date,
                Ok(None) => {
                    tracing::debug!("skipped {}: no date in filename", path.display());
                    continue;
                }
                Err(e) => {
                    invalid.push(format!("{}: {e}", path.display()));
                    continue;
                }
            };
            let file_end_exclusive = match period.next(file_start) {
                Ok(date) => date,
                Err(e) => {
                    invalid.push(format!("{}: {e}", path.display()));
                    continue;
                }
            };
            if should_skip_period(file_start, file_end_exclusive, start_date, end_date) {
                continue;
            }
            paths.push(path);
        }
    }

    fail_on_hard_errors("binance filenames", &invalid)?;
    if !found_dir {
        return Err(ReaderError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!(
                "dataset directory not found: {}/binance/data/{}/{{daily,monthly}}/{}",
                dir.display(),
                market,
                dataset
            ),
        )));
    }

    paths.sort_unstable();
    Ok(paths)
}

fn has_zip_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("zip"))
}

/// Count and samples (row numbers + snippets) of rows that failed to parse.
#[derive(Default)]
pub(crate) struct SkippedRows {
    count: usize,
    samples: Vec<String>,
}

impl SkippedRows {
    /// Record one skipped row; the detail is only formatted when the sample
    /// budget still has room, so high-error files do not pay snippet cost.
    fn push(&mut self, row_num: usize, detail: impl FnOnce() -> String) {
        self.count += 1;
        if self.samples.len() < MAX_SKIP_SAMPLES {
            self.samples.push(format!("row {row_num}: {}", detail()));
        }
    }

    fn warn(&self, path: &Path) {
        if self.count > 0 {
            tracing::warn!(
                path = %path.display(),
                skipped_rows = self.count,
                samples = ?self.samples,
                "skipped unparseable rows while reading file"
            );
        }
    }
}

/// Exact-match header detection against known header variants.
///
/// A numeric first field never matches, so a headerless file is never
/// mistaken for a headered one. Callers pass a non-empty first data line
/// (leading blank lines are skipped before this check).
fn has_csv_header(first_line: &[u8], headers: &[&[&str]]) -> bool {
    let Ok(line) = std::str::from_utf8(first_line) else {
        return false;
    };
    let line = line.trim_end_matches(['\r', '\n']);
    headers
        .iter()
        .any(|header| line.split(',').eq(header.iter().copied()))
}

fn row_snippet(row: &StringRecord, max_chars: usize) -> String {
    let joined = row.iter().collect::<Vec<_>>().join(",");
    let end = joined
        .char_indices()
        .nth(max_chars)
        .map(|(i, _)| i)
        .unwrap_or(joined.len());
    joined[..end].to_string()
}

/// Parse CSV from a stream, skipping unparseable rows instead of failing the
/// input. `headers` lists the accepted exact header lines; a matching first
/// line is consumed. Leading blank lines are skipped with line numbers
/// preserved in warnings. A single line without a trailing newline is parsed.
///
/// The stream is consumed incrementally and one `StringRecord` buffer is
/// reused, so memory stays bounded by the output rows and CSV read buffers
/// rather than the decompressed file size.
pub(crate) fn parse_csv<T>(
    reader: impl Read,
    headers: &[&[&str]],
    mut parse_row: impl FnMut(&StringRecord) -> Result<T>,
) -> Result<(Vec<T>, SkippedRows)> {
    let mut reader = BufReader::new(reader);
    let mut first_line = Vec::new();
    let mut skipped_blank_lines = 0usize;
    loop {
        first_line.clear();
        if reader.read_until(b'\n', &mut first_line)? == 0 {
            return Ok((Vec::new(), SkippedRows::default()));
        }
        if first_line.iter().all(|byte| matches!(byte, b'\n' | b'\r')) {
            if first_line.contains(&b'\n') {
                skipped_blank_lines += 1;
            }
            continue;
        }
        break;
    }

    let has_header = has_csv_header(&first_line, headers);
    let prefix = if has_header { Vec::new() } else { first_line };
    let data = std::io::Cursor::new(prefix).chain(reader);

    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(data);

    let mut records = Vec::new();
    let mut skipped = SkippedRows::default();
    let mut row_num = skipped_blank_lines + usize::from(has_header);
    let mut row = StringRecord::new();

    loop {
        match reader.read_record(&mut row) {
            Ok(true) => {}
            Ok(false) => break,
            Err(e) => {
                row_num += 1;
                skipped.push(row_num, || e.to_string());
                continue;
            }
        }
        row_num += 1;
        match parse_row(&row) {
            Ok(record) => records.push(record),
            Err(e) => {
                skipped.push(row_num, || {
                    let snippet = row_snippet(&row, 64);
                    format!("[{snippet}] {e}")
                });
            }
        }
    }
    Ok((records, skipped))
}

fn is_csv_entry(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("csv"))
}

fn load_single_zip<T>(
    path: &Path,
    headers: &[&[&str]],
    parse_row: impl FnMut(&StringRecord) -> Result<T>,
) -> Result<(Vec<T>, usize)> {
    let file = std::fs::File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let mut csv_index = None;
    for index in 0..archive.len() {
        let name = archive.by_index(index)?.name()?.into_owned();
        if is_csv_entry(&name) {
            csv_index = Some(index);
            break;
        }
    }
    let csv_index = csv_index.ok_or_else(|| {
        ReaderError::Parse(format!("no CSV file found in zip: {}", path.display()))
    })?;
    let entry = archive.by_index(csv_index)?;
    let (records, skipped) = parse_csv(entry, headers, parse_row)?;
    skipped.warn(path);
    Ok((records, skipped.count))
}

/// Outcome of decoding one zip: `Ok(rows, skipped_rows)` or a file-level error.
type FileOutcome<T> = std::result::Result<(Vec<T>, usize), String>;

/// Load every zip in parallel and merge the rows.
///
/// Row-level failures are warned and skipped; any file-level failure (corrupt
/// zip, no CSV, unreadable file) fails the whole batch, because a silently
/// missing file corrupts backtests.
pub(crate) fn load_all<T: Send>(
    paths: &[PathBuf],
    headers: &[&[&str]],
    parse_row: impl Fn(&StringRecord) -> Result<T> + Sync,
) -> Result<Vec<T>> {
    let t0 = Instant::now();
    let outcomes: Vec<(PathBuf, FileOutcome<T>)> = paths
        .par_iter()
        .map(|path| {
            let outcome = load_single_zip(path, headers, &parse_row).map_err(|e| e.to_string());
            (path.clone(), outcome)
        })
        .collect();

    let total_rows: usize = outcomes
        .iter()
        .filter_map(|(_, outcome)| outcome.as_ref().ok().map(|(rows, _)| rows.len()))
        .sum();
    let mut all = Vec::with_capacity(total_rows);
    let mut failed = Vec::new();
    let mut files_with_skips = 0usize;
    let mut skipped_rows_total = 0usize;
    for (path, outcome) in outcomes {
        match outcome {
            Ok((mut records, skipped)) => {
                if skipped > 0 {
                    files_with_skips += 1;
                    skipped_rows_total += skipped;
                }
                all.append(&mut records);
            }
            Err(detail) => failed.push(format!("{}: {detail}", path.display())),
        }
    }
    fail_on_hard_errors("binance", &failed)?;
    if skipped_rows_total > 0 {
        tracing::warn!(
            files = files_with_skips,
            rows = skipped_rows_total,
            "loaded with data loss: unparseable CSV rows skipped during load"
        );
    }
    tracing::info!(rows = all.len(), elapsed = ?t0.elapsed(), "loaded");
    Ok(all)
}

/// Hard-failure collection: any failed file fails the batch.
pub(crate) fn fail_on_hard_errors(source: &str, failed: &[String]) -> Result<()> {
    if failed.is_empty() {
        return Ok(());
    }
    let samples: Vec<&str> = failed
        .iter()
        .take(MAX_FAILED_SAMPLES)
        .map(String::as_str)
        .collect();
    Err(ReaderError::Parse(format!(
        "{source}: {} file(s) failed to load: {}",
        failed.len(),
        samples.join("; ")
    )))
}

// — field parsers —

fn field<'a>(row: &'a StringRecord, index: usize, name: &str) -> Result<&'a str> {
    row.get(index)
        .ok_or_else(|| ReaderError::Parse(format!("missing {name}")))
}

pub(crate) fn parse_i64(row: &StringRecord, index: usize, name: &str) -> Result<i64> {
    field(row, index, name)?
        .trim()
        .parse()
        .map_err(|e| ReaderError::Parse(format!("{name}: {e}")))
}

pub(crate) fn parse_non_negative_i64(row: &StringRecord, index: usize, name: &str) -> Result<i64> {
    let value = parse_i64(row, index, name)?;
    if value < 0 {
        return Err(ReaderError::Parse(format!(
            "{name}: invalid negative value {value}"
        )));
    }
    Ok(value)
}

pub(crate) fn parse_i32(row: &StringRecord, index: usize, name: &str) -> Result<i32> {
    field(row, index, name)?
        .trim()
        .parse()
        .map_err(|e| ReaderError::Parse(format!("{name}: {e}")))
}

fn parse_f64_raw(row: &StringRecord, index: usize, name: &str) -> Result<f64> {
    let value: f64 = field(row, index, name)?
        .trim()
        .parse()
        .map_err(|e| ReaderError::Parse(format!("{name}: {e}")))?;
    if !value.is_finite() {
        return Err(ReaderError::Parse(format!(
            "{name}: invalid non-finite value {value}"
        )));
    }
    Ok(value)
}

/// Finite `f64`; sign is validated by the caller.
pub(crate) fn parse_f64(row: &StringRecord, index: usize, name: &str) -> Result<f64> {
    parse_f64_raw(row, index, name)
}

pub(crate) fn parse_non_negative_f64(row: &StringRecord, index: usize, name: &str) -> Result<f64> {
    let value = parse_f64_raw(row, index, name)?;
    if value < 0.0 {
        return Err(ReaderError::Parse(format!(
            "{name}: invalid negative value {value}"
        )));
    }
    Ok(value)
}

pub(crate) fn parse_positive_f64(row: &StringRecord, index: usize, name: &str) -> Result<f64> {
    let value = parse_f64_raw(row, index, name)?;
    if value <= 0.0 {
        return Err(ReaderError::Parse(format!(
            "{name}: invalid non-positive value {value}"
        )));
    }
    Ok(value)
}

/// Optional non-negative `f64`; an empty field means "not published".
pub(crate) fn parse_optional_non_negative_f64(
    row: &StringRecord,
    index: usize,
    name: &str,
) -> Result<Option<f64>> {
    let raw = field(row, index, name)?.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    let value: f64 = raw
        .parse()
        .map_err(|e| ReaderError::Parse(format!("{name}: {e}")))?;
    if !value.is_finite() {
        return Err(ReaderError::Parse(format!(
            "{name}: invalid non-finite value {value}"
        )));
    }
    if value < 0.0 {
        return Err(ReaderError::Parse(format!(
            "{name}: invalid negative value {value}"
        )));
    }
    Ok(Some(value))
}

pub(crate) fn parse_bool(row: &StringRecord, index: usize, name: &str) -> Result<bool> {
    let raw = field(row, index, name)?.trim();
    if raw.eq_ignore_ascii_case("true") {
        Ok(true)
    } else if raw.eq_ignore_ascii_case("false") {
        Ok(false)
    } else {
        Err(ReaderError::Parse(format!(
            "{name}: invalid boolean {raw:?}"
        )))
    }
}

/// Non-negative millisecond timestamp.
pub(crate) fn parse_timestamp(row: &StringRecord, index: usize, name: &str) -> Result<i64> {
    parse_non_negative_i64(row, index, name)
}

/// `YYYY-MM-DD HH:MM:SS` (UTC) to epoch milliseconds.
pub(crate) fn parse_utc_datetime(row: &StringRecord, index: usize, name: &str) -> Result<i64> {
    let raw = field(row, index, name)?.trim();
    let datetime = NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S")
        .map_err(|e| ReaderError::Parse(format!("{name}: {e}")))?;
    Ok(datetime.and_utc().timestamp_millis())
}

pub(crate) fn parse_string(row: &StringRecord, index: usize, name: &str) -> Result<String> {
    let raw = field(row, index, name)?.trim();
    if raw.is_empty() {
        return Err(ReaderError::Parse(format!("{name}: empty value")));
    }
    Ok(raw.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pd(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn daily_date_from_stem() {
        let date = Period::Daily
            .parse_key_date("BTCUSDT-1m-2024-01-02")
            .unwrap();
        assert_eq!(date, Some(pd("2024-01-02")));
    }

    #[test]
    fn monthly_date_from_stem() {
        let date = Period::Monthly
            .parse_key_date("BTCUSDT-fundingRate-2024-01")
            .unwrap();
        assert_eq!(date, Some(pd("2024-01-01")));
    }

    #[test]
    fn undated_stem_is_skipped() {
        assert_eq!(Period::Daily.parse_key_date("readme").unwrap(), None);
        assert_eq!(
            Period::Monthly
                .parse_key_date("BTCUSDT-fundingRate")
                .unwrap(),
            None
        );
    }

    #[test]
    fn date_shaped_but_invalid_is_error() {
        assert!(
            Period::Daily
                .parse_key_date("BTCUSDT-1m-2024-13-01")
                .is_err()
        );
        assert!(
            Period::Daily
                .parse_key_date("BTCUSDT-1m-2024-02-30")
                .is_err()
        );
        assert!(
            Period::Monthly
                .parse_key_date("BTCUSDT-fundingRate-2024-13")
                .is_err()
        );
    }

    #[test]
    fn period_overlap_filter() {
        let start = Some(pd("2024-01-10"));
        let end = Some(pd("2024-01-20"));
        assert!(should_skip_period(
            pd("2024-01-01"),
            pd("2024-01-02"),
            start,
            end
        ));
        assert!(should_skip_period(
            pd("2024-01-21"),
            pd("2024-01-22"),
            start,
            end
        ));
        assert!(!should_skip_period(
            pd("2024-01-10"),
            pd("2024-01-11"),
            start,
            end
        ));
        // Monthly file overlapping the window is kept whole.
        assert!(!should_skip_period(
            pd("2024-01-01"),
            pd("2024-02-01"),
            start,
            end
        ));
    }

    #[test]
    fn date_window_is_inclusive_end_day() {
        let window = DateWindow::new(Some(pd("2024-01-10")), Some(pd("2024-01-20")));
        let ms = |d: &str| {
            pd(d)
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc()
                .timestamp_millis()
        };
        assert!(!window.contains(ms("2024-01-10") - 1));
        assert!(window.contains(ms("2024-01-10")));
        assert!(window.contains(ms("2024-01-20") + 86_399_999));
        assert!(!window.contains(ms("2024-01-21")));
    }

    #[test]
    fn date_window_handles_max_date() {
        let max = NaiveDate::MAX;
        let window = DateWindow::new(Some(max), Some(max));
        let last_ms = max
            .and_hms_milli_opt(23, 59, 59, 999)
            .unwrap()
            .and_utc()
            .timestamp_millis();
        assert!(window.contains(last_ms));
        assert!(!window.contains(last_ms + 1));
    }

    #[test]
    fn hard_errors_are_zero_tolerance() {
        assert!(fail_on_hard_errors("test", &[]).is_ok());
        let failed = vec!["a.zip: corrupt".to_string()];
        let error = fail_on_hard_errors("test", &failed)
            .unwrap_err()
            .to_string();
        assert!(error.contains("1 file(s) failed to load"), "{error}");
        assert!(error.contains("a.zip: corrupt"), "{error}");
    }

    #[test]
    fn leading_blank_lines_are_skipped() {
        let csv = "\n\r\n1,10.0\n2,11.0\n";
        let (rows, skipped) = parse_csv::<i64>(csv.as_bytes(), &[], |row| {
            parse_non_negative_i64(row, 0, "id")
        })
        .unwrap();
        assert_eq!(rows, vec![1, 2]);
        assert_eq!(skipped.count, 0);
    }

    #[test]
    fn date_window_trim_keeps_order() {
        let window = DateWindow::new(Some(pd("2024-01-10")), Some(pd("2024-01-10")));
        let ms = |d: &str| {
            pd(d)
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc()
                .timestamp_millis()
        };
        let rows = vec![ms("2024-01-09"), ms("2024-01-10"), ms("2024-01-11")];
        let out = window.trim(rows, |t| *t);
        assert_eq!(out, vec![ms("2024-01-10")]);
    }

    #[test]
    fn header_detection_is_exact() {
        let header: [&str; 2] = ["time", "price"];
        let headers: [&[&str]; 1] = [&header];
        assert!(has_csv_header(b"time,price\r", &headers));
        assert!(!has_csv_header(b"1,price", &headers));
        assert!(!has_csv_header(b"time,price,extra", &headers));
    }

    fn record(fields: &[&str]) -> StringRecord {
        let mut record = StringRecord::new();
        for field in fields {
            record.push_field(field);
        }
        record
    }

    #[test]
    fn parser_helpers_cover_valid_and_invalid_values() {
        assert!(parse_bool(&record(&["TRUE"]), 0, "flag").unwrap());
        assert!(!parse_bool(&record(&["false"]), 0, "flag").unwrap());
        assert!(parse_bool(&record(&["yes"]), 0, "flag").is_err());

        assert_eq!(parse_string(&record(&[" BUY "]), 0, "side").unwrap(), "BUY");
        assert!(parse_string(&record(&["  "]), 0, "side").is_err());

        assert_eq!(
            parse_utc_datetime(&record(&["2024-01-01 00:00:00"]), 0, "time").unwrap(),
            1_704_067_200_000
        );
        assert!(parse_utc_datetime(&record(&["2024-01-01T00:00:00"]), 0, "time").is_err());

        assert_eq!(
            parse_optional_non_negative_f64(&record(&[""]), 0, "ratio").unwrap(),
            None
        );
        assert_eq!(
            parse_optional_non_negative_f64(&record(&["0"]), 0, "ratio").unwrap(),
            Some(0.0)
        );
        for invalid in ["-0.1", "NaN", "inf"] {
            assert!(
                parse_optional_non_negative_f64(&record(&[invalid]), 0, "ratio").is_err(),
                "accepted {invalid:?}"
            );
        }

        assert!(parse_non_negative_i64(&record(&["-1"]), 0, "id").is_err());
        assert!(parse_positive_f64(&record(&["0"]), 0, "price").is_err());
        assert!(parse_positive_f64(&record(&["NaN"]), 0, "price").is_err());
    }

    #[test]
    fn parse_csv_handles_crlf_header_and_empty_input() {
        let header: [&str; 2] = ["time", "price"];
        let headers: [&[&str]; 1] = [&header];
        let (rows, skipped) =
            parse_csv::<i64>(b"time,price\r\n1,10.0\r\n".as_slice(), &headers, |row| {
                parse_non_negative_i64(row, 0, "id")
            })
            .unwrap();
        assert_eq!(rows, vec![1]);
        assert_eq!(skipped.count, 0);

        let (rows, skipped) = parse_csv::<i64>(b"".as_slice(), &headers, |_| Ok(1)).unwrap();
        assert!(rows.is_empty());
        assert_eq!(skipped.count, 0);
    }

    #[test]
    fn csv_entry_extension_is_case_insensitive() {
        assert!(is_csv_entry("data.csv"));
        assert!(is_csv_entry("data.CSV"));
        assert!(!is_csv_entry("data.csv.bak"));
        assert!(!is_csv_entry("data.txt"));
        assert!(!is_csv_entry("data"));
    }

    #[test]
    fn parse_skips_bad_rows_and_keeps_good_ones() {
        let csv = ["1,10.0", "2,abc", "3,-1.0", "4,11.0"].join("\n");
        let (rows, skipped) = parse_csv::<i64>(csv.as_bytes(), &[], |row| {
            let _ = parse_positive_f64(row, 1, "price")?;
            parse_non_negative_i64(row, 0, "id")
        })
        .unwrap();
        assert_eq!(rows, vec![1, 4]);
        assert_eq!(skipped.count, 2);
    }

    #[test]
    fn parse_header_only_is_empty() {
        let header: [&str; 2] = ["time", "price"];
        let headers: [&[&str]; 1] = [&header];
        let (rows, skipped) =
            parse_csv::<i64>(b"time,price".as_slice(), &headers, |_| Ok(1)).unwrap();
        assert!(rows.is_empty());
        assert_eq!(skipped.count, 0);
    }

    #[test]
    fn parse_single_line_without_newline() {
        let (rows, skipped) = parse_csv::<i64>(b"1,10.0".as_slice(), &[], |row| {
            parse_non_negative_i64(row, 0, "id")
        })
        .unwrap();
        assert_eq!(rows, vec![1]);
        assert_eq!(skipped.count, 0);
    }
}

use std::io::Read;
use std::path::Path;
use std::time::Instant;

use chrono::NaiveDate;
use csv::StringRecord;
use rayon::prelude::*;

use crate::app::Record;
use crate::config::Market;
use crate::error::{ReaderError, Result};
use crate::types::{Symbol, Timeframe};

#[cfg(test)]
use crate::test_helpers::create_test_zip;

/// Binance kline CSV column names, in order. Used to reliably detect header lines.
const BINANCE_CSV_HEADER: [&str; 12] = [
    "open_time",
    "open",
    "high",
    "low",
    "close",
    "volume",
    "close_time",
    "quote_volume",
    "count",
    "taker_buy_volume",
    "taker_buy_quote_volume",
    "ignore",
];

/// Maximum number of skipped-row samples retained for warnings.
const MAX_SKIP_SAMPLES: usize = 5;

/// Extracts trailing YYYY-MM-DD date from a filename stem.
///
/// Supports Binance filenames like `BTCUSDT-1m-2024-01-01`.
/// Uses `.get()` for safe slicing even on non-ASCII input.
/// Returns `Err` when a trailing 10-char candidate has the date shape but is not a valid date.
fn extract_date_from_filename(stem: &str) -> Result<Option<&str>> {
    if stem.len() < 10 {
        return Ok(None);
    }
    let Some(candidate) = stem.get(stem.len() - 10..) else {
        return Ok(None);
    };
    let date_shaped =
        candidate.len() == 10 && candidate.as_bytes()[4] == b'-' && candidate.as_bytes()[7] == b'-';
    if !date_shaped {
        return Ok(None);
    }
    if chrono::NaiveDate::parse_from_str(candidate, "%Y-%m-%d").is_err() {
        return Err(ReaderError::Parse(format!(
            "invalid date in filename stem: {stem:?}"
        )));
    }
    Ok(Some(candidate))
}

fn should_skip_by_date(
    file_date: NaiveDate,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
) -> bool {
    if let Some(sd) = start_date
        && file_date < sd
    {
        return true;
    }
    if let Some(ed) = end_date
        && file_date > ed
    {
        return true;
    }
    false
}

/// Returns `true` only when the first line exactly matches the standard 12-column Binance header
/// (optionally CRLF-terminated). A numeric first field is never treated as a header, so a
/// mis-judged header line fails per-row instead of dropping the whole file.
fn has_csv_header(first_line: &[u8]) -> bool {
    if first_line.is_empty() {
        return true;
    }
    let line = match std::str::from_utf8(first_line) {
        Ok(line) => line.trim_end_matches('\r'),
        Err(_) => return false,
    };
    line.split(',').eq(BINANCE_CSV_HEADER.iter().copied())
}

/// Count and samples (line numbers + snippets) of rows that failed to parse.
#[derive(Default)]
struct SkippedRows {
    count: usize,
    samples: Vec<String>,
}

impl SkippedRows {
    fn push(&mut self, row_num: usize, detail: impl std::fmt::Display) {
        self.count += 1;
        if self.samples.len() < MAX_SKIP_SAMPLES {
            self.samples.push(format!("row {row_num}: {detail}"));
        }
    }
}

fn parse_f64_field(row: &StringRecord, idx: usize, name: &str) -> Result<f64> {
    let s = row
        .get(idx)
        .ok_or_else(|| ReaderError::Parse(format!("missing {name}")))?;
    let value: f64 = s
        .parse()
        .map_err(|e| ReaderError::Parse(format!("{name}: {e}")))?;
    if !value.is_finite() || value < 0.0 {
        return Err(ReaderError::Parse(format!("{name}: invalid value {value}")));
    }
    Ok(value)
}

fn parse_row(row: &StringRecord) -> Result<Record> {
    let open_time: i64 = row
        .get(0)
        .ok_or_else(|| ReaderError::Parse("missing open_time".into()))?
        .parse()
        .map_err(|e| ReaderError::Parse(format!("open_time: {e}")))?;
    if open_time < 0 {
        return Err(ReaderError::Parse(format!(
            "open_time: invalid value {open_time}"
        )));
    }
    let open = parse_f64_field(row, 1, "open")?;
    let high = parse_f64_field(row, 2, "high")?;
    let low = parse_f64_field(row, 3, "low")?;
    let close = parse_f64_field(row, 4, "close")?;
    let volume = parse_f64_field(row, 5, "volume")?;
    // 两源统一口径（`super::is_valid_ohlc`）：零价/高低错位即坏行，调用方计数跳过。
    if !super::is_valid_ohlc(open, high, low, close, volume) {
        return Err(ReaderError::Parse(format!(
            "ohlc invalid: open={open} high={high} low={low} close={close} volume={volume}"
        )));
    }
    Ok(Record {
        open_time,
        open,
        high,
        low,
        close,
        volume,
    })
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

/// Parses Binance kline CSV data, skipping unparseable rows instead of failing the whole input.
/// Returns the parsed records plus the rows that were skipped (with their line numbers).
///
/// 边界：无换行单行按单行解析；首部空行跳过（只跳行，不丢文件）。
fn parse_csv_bytes(bytes: &[u8]) -> (Vec<Record>, SkippedRows) {
    // 跳过首部空行：行号基数同步后移，保证 warn 留样行号准确。
    let mut start = 0;
    let mut skipped_blank_lines = 0usize;
    while start < bytes.len() && (bytes[start] == b'\n' || bytes[start] == b'\r') {
        if bytes[start] == b'\n' {
            skipped_blank_lines += 1;
        }
        start += 1;
    }
    if start >= bytes.len() {
        return (Vec::new(), SkippedRows::default());
    }
    let rest = &bytes[start..];

    // 无换行：整块即首行（单行文件，无尾换行也解析）。
    let (first_line, data) = match rest.iter().position(|&b| b == b'\n') {
        Some(p) => (&rest[..p], &rest[p + 1..]),
        None => (rest, &rest[rest.len()..]),
    };

    let has_header = has_csv_header(first_line);
    let data = if has_header { data } else { rest };

    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(data);

    let mut records = Vec::new();
    let mut skipped = SkippedRows::default();
    let mut row_num = skipped_blank_lines + usize::from(has_header);

    for result in rdr.records() {
        row_num += 1;
        let row = match result {
            Ok(row) => row,
            Err(e) => {
                skipped.push(row_num, format!("{e}"));
                continue;
            }
        };
        match parse_row(&row) {
            Ok(record) => records.push(record),
            Err(e) => {
                let snippet = row_snippet(&row, 64);
                skipped.push(row_num, format!("[{snippet}] {e}"));
            }
        }
    }
    (records, skipped)
}

fn load_single_zip(path: &Path) -> Result<(Vec<Record>, usize)> {
    let file = std::fs::File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let mut csv_bytes: Option<Vec<u8>> = None;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_ascii_lowercase();
        if !name.ends_with(".csv") {
            continue;
        }
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf)?;
        csv_bytes = Some(buf);
        break;
    }
    let Some(bytes) = csv_bytes else {
        return Err(ReaderError::Parse(format!(
            "no CSV file found in zip: {}",
            path.display()
        )));
    };
    let (records, skipped) = parse_csv_bytes(&bytes);
    if skipped.count > 0 {
        tracing::warn!(
            path = %path.display(),
            skipped_rows = skipped.count,
            samples = ?skipped.samples,
            "skipped unparseable rows while reading file"
        );
    }
    Ok((records, skipped.count))
}

/// Scan directory for matching zip files, parse them (in parallel), and merge into sorted Records.
#[tracing::instrument(skip_all, fields(market = %market, symbol, timeframe))]
pub(crate) fn load_pipeline(
    dir: impl AsRef<Path>,
    market: Market,
    symbol: &Symbol,
    timeframe: &Timeframe,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
) -> Result<Vec<Record>> {
    let dir = dir.as_ref();
    let data_dir = dir
        .join("binance")
        .join("data")
        .join(market.as_path())
        .join("daily/klines")
        .join(symbol.as_str())
        .join(timeframe.as_str());
    if !data_dir.exists() {
        return Err(ReaderError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!(
                "directory not found: {}/binance/data/{}/daily/klines/{}/{}",
                dir.display(),
                market,
                symbol,
                timeframe,
            ),
        )));
    }

    let rd = match std::fs::read_dir(&data_dir) {
        Ok(rd) => rd,
        Err(e) => return Err(ReaderError::Io(e)),
    };

    let t0 = Instant::now();

    let mut paths: Vec<_> = rd
        .filter_map(|entry| match entry {
            Ok(e) => Some(e.path()),
            Err(e) => {
                tracing::warn!("read_dir entry failed: {e}");
                None
            }
        })
        .filter(|path| {
            path.extension()
                .and_then(|s| s.to_str())
                .map(|s| s.eq_ignore_ascii_case("zip"))
                .unwrap_or(false)
        })
        .collect();

    paths.par_sort_unstable();

    /// 单文件结局：`Loaded` 好行、`Skipped` 意图内跳过（无日期形状/区间外）、
    /// `Failed` 硬失败（非法日期形状、损坏 zip、无 CSV），计入零容忍收口。
    enum FileOutcome {
        Loaded(Vec<Record>, usize),
        Skipped,
        Failed(String),
    }

    let outcomes: Vec<(std::path::PathBuf, FileOutcome)> = paths
        .par_iter()
        .map(|path| {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let date_str = match extract_date_from_filename(stem) {
                Ok(Some(date)) => date,
                Ok(None) => {
                    tracing::debug!("skipped {}: no date in filename", path.display());
                    return (path.clone(), FileOutcome::Skipped);
                }
                Err(e) => return (path.clone(), FileOutcome::Failed(e.to_string())),
            };
            let file_date = match NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
                Ok(d) => d,
                Err(e) => {
                    return (
                        path.clone(),
                        FileOutcome::Failed(format!("invalid date {date_str}: {e}")),
                    );
                }
            };
            if should_skip_by_date(file_date, start_date, end_date) {
                return (path.clone(), FileOutcome::Skipped);
            }
            match load_single_zip(path) {
                Ok((records, skipped)) => (path.clone(), FileOutcome::Loaded(records, skipped)),
                Err(e) => (path.clone(), FileOutcome::Failed(e.to_string())),
            }
        })
        .collect();

    let mut all_records = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    let mut files_with_skips = 0usize;
    let mut skipped_rows_total = 0usize;
    for (path, outcome) in outcomes {
        match outcome {
            FileOutcome::Loaded(records, skipped) => {
                if skipped > 0 {
                    files_with_skips += 1;
                    skipped_rows_total += skipped;
                }
                all_records.extend(records);
            }
            FileOutcome::Skipped => {}
            FileOutcome::Failed(detail) => failed.push(format!("{}: {detail}", path.display())),
        }
    }
    super::fail_on_hard_errors("binance", &failed)?;
    if skipped_rows_total > 0 {
        tracing::warn!(
            files = files_with_skips,
            rows = skipped_rows_total,
            "loaded with data loss: unparseable CSV rows skipped during load"
        );
    }

    tracing::info!(rows = all_records.len(), elapsed = ?t0.elapsed(), "loaded");
    Ok(all_records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    const HEADER12: &str = "open_time,open,high,low,close,volume,close_time,quote_volume,count,taker_buy_volume,taker_buy_quote_volume,ignore";

    use crate::config::{DataSource, Market};

    /// Existing tests exercise the default UM layout; market-specific tests
    /// call the production function directly.
    fn load_pipeline(
        dir: impl AsRef<Path>,
        symbol: &Symbol,
        timeframe: &Timeframe,
        start_date: Option<NaiveDate>,
        end_date: Option<NaiveDate>,
    ) -> Result<Vec<Record>> {
        super::load_pipeline(dir, Market::Um, symbol, timeframe, start_date, end_date)
    }

    fn sym(s: &str) -> Symbol {
        Symbol::parse(s).unwrap()
    }
    fn tf(s: &str) -> Timeframe {
        Timeframe::parse(DataSource::Binance, s).unwrap()
    }
    fn pd(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn write_kline_csv(rows: impl IntoIterator<Item = (i64, f64, f64, f64, f64, f64)>) -> String {
        let mut content = String::new();
        for (ot, o, h, l, c, v) in rows {
            content.push_str(&format!("{ot},{o},{h},{l},{c},{v},0,0,0,0,0,0\n"));
        }
        content
    }

    fn setup_dir(tmp: &Path, symbol: &str, tf: &str) -> std::path::PathBuf {
        let dir = tmp
            .join("binance/data/futures/um/daily/klines")
            .join(symbol)
            .join(tf);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn date_to_millis(date: &str) -> i64 {
        chrono::NaiveDate::parse_from_str(date.trim(), "%Y-%m-%d")
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis()
    }

    #[test]
    fn reader_empty_dir_returns_empty() {
        let tmp = TempDir::new().unwrap();
        setup_dir(tmp.path(), "X", "1m");

        let records = load_pipeline(tmp.path(), &sym("X"), &tf("1m"), None, None).unwrap();
        assert!(records.is_empty());
    }

    #[test]
    fn reader_nonexistent_dir_errors() {
        let result = load_pipeline("/nonexistent/path", &sym("X"), &tf("1m"), None, None);
        assert!(result.is_err());
        match result.unwrap_err() {
            ReaderError::Io(_) => {}
            _ => panic!("expected Io error"),
        }
    }

    #[test]
    fn reader_loads_data() {
        let tmp = TempDir::new().unwrap();
        let dir = setup_dir(tmp.path(), "X", "1m");
        let csv = "1578528000000,240.17,240.19,239.62,239.62,439.917,1578528059999,105529.27598,47,1.035,248.53455,0\n";
        create_test_zip(&dir, "X-1m-2024-01-09.zip", csv);

        let records = load_pipeline(tmp.path(), &sym("X"), &tf("1m"), None, None).unwrap();
        assert_eq!(records.len(), 1);
        assert!((records[0].close - 239.62).abs() < 0.001);
    }

    #[test]
    fn reader_loads_each_binance_market() {
        for market in [Market::Spot, Market::Um, Market::Cm] {
            let tmp = TempDir::new().unwrap();
            let dir = tmp
                .path()
                .join("binance/data")
                .join(market.as_path())
                .join("daily/klines/X/1m");
            std::fs::create_dir_all(&dir).unwrap();
            let csv = write_kline_csv([(100, 10.0, 11.0, 9.0, 10.5, 500.0)]);
            create_test_zip(&dir, "X-1m-2024-01-01.zip", &csv);

            let records =
                super::load_pipeline(tmp.path(), market, &sym("X"), &tf("1m"), None, None).unwrap();
            assert_eq!(records.len(), 1, "market {market}");
        }
    }

    // — extract_date_from_filename —

    #[test]
    fn extract_date_standard() {
        assert_eq!(
            extract_date_from_filename("BTCUSDT-1m-2024-01-01").unwrap(),
            Some("2024-01-01")
        );
    }

    #[test]
    fn extract_date_too_short() {
        assert_eq!(extract_date_from_filename("a").unwrap(), None);
    }

    #[test]
    fn extract_date_rejects_invalid_calendar_date() {
        assert!(extract_date_from_filename("X-1m-2024-13-01").is_err());
        assert!(extract_date_from_filename("X-1m-2024-02-30").is_err());
    }

    #[test]
    fn extract_date_rejects_non_numeric_segments() {
        assert!(extract_date_from_filename("X-1m-2024-01-ab").is_err());
    }

    // — has_csv_header —

    #[test]
    fn detect_numeric_line_no_header() {
        assert!(!has_csv_header(b"1578528000000,240.17,240.19"));
    }

    #[test]
    fn detect_header_line() {
        assert!(has_csv_header(
            b"open_time,open,high,low,close,volume,close_time,quote_volume,count,taker_buy_volume,taker_buy_quote_volume,ignore"
        ));
    }

    #[test]
    fn detect_header_with_crlf() {
        assert!(has_csv_header(
            b"open_time,open,high,low,close,volume,close_time,quote_volume,count,taker_buy_volume,taker_buy_quote_volume,ignore\r"
        ));
    }

    #[test]
    fn detect_numeric_first_field_is_not_header() {
        assert!(!has_csv_header(
            b"1,open,high,low,close,volume,close_time,quote_volume,count,taker_buy_volume,taker_buy_quote_volume,ignore"
        ));
    }

    #[test]
    fn detect_partial_header_is_not_header() {
        assert!(!has_csv_header(b"open_time,open,high,low,close"));
    }

    #[test]
    fn detect_empty_line_no_header() {
        assert!(has_csv_header(b""));
    }

    // — parse_csv_bytes —

    #[test]
    fn parse_skips_bad_rows_keeps_good_rows() {
        let csv = [
            "100,10.0,12.0,9.0,11.0,1000.0,0,0,0,0,0,0",
            "200,abc,12.0,9.0,11.0,1000.0,0,0,0,0,0,0",
            "300,-5.0,12.0,9.0,11.0,1000.0,0,0,0,0,0,0",
            "400,10.0,-12.0,9.0,11.0,1000.0,0,0,0,0,0,0",
            "9999999999999999999999,10.0,12.0,9.0,11.0,1000.0,0,0,0,0,0,0",
            "500,10.0,12.0,9.0,11.0,2000.0,0,0,0,0,0,0",
        ]
        .join("\n");

        let (records, skipped) = parse_csv_bytes(csv.as_bytes());
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].open_time, 100);
        assert_eq!(records[1].open_time, 500);
        assert_eq!(skipped.count, 4);
        assert_eq!(skipped.samples.len(), 4);
    }

    #[test]
    fn parse_rejects_negative_volume_and_non_finite() {
        let csv = [
            "100,10.0,12.0,9.0,11.0,1000.0,0,0,0,0,0,0",
            "200,10.0,12.0,9.0,11.0,-1.0,0,0,0,0,0,0",
            "300,10.0,12.0,9.0,11.0,inf,0,0,0,0,0,0",
        ]
        .join("\n");

        let (records, skipped) = parse_csv_bytes(csv.as_bytes());
        assert_eq!(records.len(), 1);
        assert_eq!(skipped.count, 2);
    }

    #[test]
    fn parse_rejects_misaligned_ohlc_and_zero_price() {
        // 高低错位 / 零价与 dukascopy 同口径跳过并计数。
        let csv = [
            "100,10.0,12.0,9.0,11.0,1000.0,0,0,0,0,0,0",
            "200,10.0,9.0,12.0,11.0,1000.0,0,0,0,0,0,0",
            "300,0.0,0.0,0.0,0.0,1000.0,0,0,0,0,0,0",
            "400,10.0,12.0,9.0,13.0,1000.0,0,0,0,0,0,0",
        ]
        .join("\n");

        let (records, skipped) = parse_csv_bytes(csv.as_bytes());
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].open_time, 100);
        assert_eq!(skipped.count, 3);
    }

    #[test]
    fn parse_with_header_skips_header() {
        let csv = format!("{HEADER12}\n100,10.0,12.0,9.0,11.0,1000.0,0,0,0,0,0,0\n");
        let (records, skipped) = parse_csv_bytes(csv.as_bytes());
        assert_eq!(records.len(), 1);
        assert_eq!(skipped.count, 0);
    }

    #[test]
    fn parse_single_line_without_trailing_newline() {
        // 单行文件无尾换行：整块即一行，必须解析而非丢文件。
        let csv = "100,10.0,12.0,9.0,11.0,1000.0,0,0,0,0,0,0";
        let (records, skipped) = parse_csv_bytes(csv.as_bytes());
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].open_time, 100);
        assert_eq!(skipped.count, 0);
    }

    #[test]
    fn parse_header_only_without_trailing_newline_is_empty() {
        let (records, skipped) = parse_csv_bytes(HEADER12.as_bytes());
        assert!(records.is_empty());
        assert_eq!(skipped.count, 0);
    }

    #[test]
    fn parse_leading_blank_lines_skipped() {
        // 首空行只跳行：好行保留，行号按绝对位置报。
        let csv = "\n\n100,10.0,12.0,9.0,11.0,1000.0,0,0,0,0,0,0\n";
        let (records, skipped) = parse_csv_bytes(csv.as_bytes());
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].open_time, 100);
        assert_eq!(skipped.count, 0);
    }

    #[test]
    fn parse_numeric_first_field_header_drops_only_header_row() {
        let csv = "1,open,high,low,close,volume,close_time,quote_volume,count,taker_buy_volume,taker_buy_quote_volume,ignore\n100,10.0,12.0,9.0,11.0,1000.0,0,0,0,0,0,0\n";
        let (records, skipped) = parse_csv_bytes(csv.as_bytes());
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].open_time, 100);
        assert_eq!(skipped.count, 1);
    }

    // — load_single_zip —

    #[test]
    fn load_single_zip_no_header() {
        let tmp = TempDir::new().unwrap();
        let csv = write_kline_csv([(100, 10.0, 12.0, 9.0, 11.0, 1000.0)]);
        create_test_zip(tmp.path(), "X-1m-2024-01-01.zip", &csv);

        let (records, skipped) = load_single_zip(&tmp.path().join("X-1m-2024-01-01.zip")).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(skipped, 0);
        assert_eq!(records[0].close, 11.0);
    }

    #[test]
    fn load_single_zip_with_header() {
        let tmp = TempDir::new().unwrap();
        let csv = format!("{HEADER12}\n100,10.0,12.0,9.0,11.0,1000.0,0,0,0,0,0,0\n");
        create_test_zip(tmp.path(), "X-1m-2024-01-01.zip", &csv);

        let (records, skipped) = load_single_zip(&tmp.path().join("X-1m-2024-01-01.zip")).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(skipped, 0);
        assert_eq!(records[0].close, 11.0);
    }

    #[test]
    fn load_single_zip_empty() {
        let tmp = TempDir::new().unwrap();
        create_test_zip(tmp.path(), "empty.zip", "");
        let result = load_single_zip(&tmp.path().join("empty.zip"));
        assert!(matches!(result, Ok((ref v, 0)) if v.is_empty()));
    }

    #[test]
    fn load_single_zip_no_csv_errors() {
        let tmp = TempDir::new().unwrap();
        let zip_path = tmp.path().join("bad.zip");
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file::<&str, ()>("readme.txt", zip::write::FileOptions::default())
            .unwrap();
        zip.write_all(b"hello").unwrap();
        zip.finish().unwrap();

        let result = load_single_zip(&zip_path);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("no CSV file found in zip")
        );
    }

    #[test]
    fn load_single_zip_skips_bad_rows() {
        let tmp = TempDir::new().unwrap();
        let csv = [
            "100,10.0,12.0,9.0,11.0,1000.0,0,0,0,0,0,0",
            "200,abc,12.0,9.0,11.0,1000.0,0,0,0,0,0,0",
        ]
        .join("\n");
        create_test_zip(tmp.path(), "X-1m-2024-01-01.zip", &csv);

        let (records, skipped) = load_single_zip(&tmp.path().join("X-1m-2024-01-01.zip")).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(skipped, 1);
    }

    // — load_pipeline —

    #[test]
    fn load_pipeline_single_file() {
        let tmp = TempDir::new().unwrap();
        let dir = setup_dir(tmp.path(), "X", "1m");
        let base_ts = date_to_millis("2024-01-01");
        let csv = write_kline_csv([(base_ts, 10.0, 12.0, 9.0, 11.0, 1000.0)]);
        create_test_zip(&dir, "X-1m-2024-01-01.zip", &csv);

        let records = load_pipeline(tmp.path(), &sym("X"), &tf("1m"), None, None).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].close, 11.0);
    }

    #[test]
    fn load_pipeline_multiple_files() {
        let tmp = TempDir::new().unwrap();
        let dir = setup_dir(tmp.path(), "X", "1m");

        let csv1 = write_kline_csv([(100, 10.0, 11.0, 9.0, 10.5, 500.0)]);
        let csv2 = write_kline_csv([(200, 11.0, 12.0, 10.0, 11.5, 600.0)]);
        create_test_zip(&dir, "X-1m-2024-01-01.zip", &csv1);
        create_test_zip(&dir, "X-1m-2024-01-02.zip", &csv2);

        let records = load_pipeline(tmp.path(), &sym("X"), &tf("1m"), None, None).unwrap();
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn load_pipeline_empty_dir() {
        let tmp = TempDir::new().unwrap();
        setup_dir(tmp.path(), "X", "1m");
        let records = load_pipeline(tmp.path(), &sym("X"), &tf("1m"), None, None).unwrap();
        assert!(records.is_empty());
    }

    #[test]
    fn load_pipeline_nonexistent_dir_errors() {
        let result = load_pipeline("/nonexistent/path", &sym("X"), &tf("1m"), None, None);
        assert!(result.is_err());
        match result.unwrap_err() {
            ReaderError::Io(_) => {}
            _ => panic!("expected Io error"),
        }
    }

    #[test]
    fn load_pipeline_date_filter() {
        let tmp = TempDir::new().unwrap();
        let dir = setup_dir(tmp.path(), "X", "1m");

        let csv1 = write_kline_csv([(100, 10.0, 11.0, 9.0, 10.5, 500.0)]);
        let csv2 = write_kline_csv([(200, 11.0, 12.0, 10.0, 11.5, 600.0)]);
        let csv3 = write_kline_csv([(300, 12.0, 13.0, 11.0, 12.5, 700.0)]);
        create_test_zip(&dir, "X-1m-2024-01-01.zip", &csv1);
        create_test_zip(&dir, "X-1m-2024-01-15.zip", &csv2);
        create_test_zip(&dir, "X-1m-2024-02-01.zip", &csv3);

        let records = load_pipeline(
            tmp.path(),
            &sym("X"),
            &tf("1m"),
            Some(pd("2024-01-10")),
            Some(pd("2024-01-20")),
        )
        .unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].close, 11.5);
    }

    #[test]
    fn load_pipeline_no_date_filter_returns_all() {
        let tmp = TempDir::new().unwrap();
        let dir = setup_dir(tmp.path(), "X", "1d");
        let csv1 = write_kline_csv([(100, 10.0, 11.0, 9.0, 10.5, 500.0)]);
        let csv2 = write_kline_csv([(200, 11.0, 12.0, 10.0, 11.5, 600.0)]);
        create_test_zip(&dir, "X-1d-2024-01-01.zip", &csv1);
        create_test_zip(&dir, "X-1d-2024-01-02.zip", &csv2);

        let records = load_pipeline(tmp.path(), &sym("X"), &tf("1d"), None, None).unwrap();
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn load_pipeline_date_filter_no_match() {
        let tmp = TempDir::new().unwrap();
        let dir = setup_dir(tmp.path(), "X", "1h");
        let csv = write_kline_csv([(100, 10.0, 11.0, 9.0, 10.5, 500.0)]);
        create_test_zip(&dir, "X-1h-2024-06-01.zip", &csv);

        let records = load_pipeline(
            tmp.path(),
            &sym("X"),
            &tf("1h"),
            Some(pd("2025-01-01")),
            None,
        )
        .unwrap();
        assert!(records.is_empty());
    }

    #[test]
    fn load_pipeline_handles_mixed_header_formats() {
        let tmp = TempDir::new().unwrap();
        let dir = setup_dir(tmp.path(), "Y", "1m");

        // no header
        let csv_no_header = "100,10.0,11.0,9.0,10.5,500.0,0,0,0,0,0,0\n";
        create_test_zip(&dir, "Y-1m-2024-01-01.zip", csv_no_header);

        // with header
        let csv_with_header = format!("{HEADER12}\n200,11.0,12.0,10.0,11.5,600.0,0,0,0,0,0,0\n");
        create_test_zip(&dir, "Y-1m-2024-01-02.zip", &csv_with_header);

        let records = load_pipeline(tmp.path(), &sym("Y"), &tf("1m"), None, None).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].open_time, 100);
        assert_eq!(records[1].open_time, 200);
    }

    #[test]
    fn load_pipeline_undated_file_skipped() {
        let tmp = TempDir::new().unwrap();
        let dir = setup_dir(tmp.path(), "Z", "1m");

        // valid
        let csv = write_kline_csv([(100, 10.0, 11.0, 9.0, 10.5, 500.0)]);
        create_test_zip(&dir, "Z-1m-2024-01-01.zip", &csv);

        // corrupt (not a zip)
        std::fs::write(dir.join("corrupt.zip"), b"this is not a zip").unwrap();

        let records = load_pipeline(tmp.path(), &sym("Z"), &tf("1m"), None, None).unwrap();
        assert_eq!(records.len(), 1);
    }

    #[test]
    fn load_pipeline_sorts_by_open_time() {
        let tmp = TempDir::new().unwrap();
        let dir = setup_dir(tmp.path(), "S", "1m");

        let csv1 = write_kline_csv([(200, 11.0, 12.0, 10.0, 11.5, 600.0)]);
        let csv2 = write_kline_csv([(100, 10.0, 11.0, 9.0, 10.5, 500.0)]);
        create_test_zip(&dir, "S-1m-2024-01-02.zip", &csv1);
        create_test_zip(&dir, "S-1m-2024-01-01.zip", &csv2);

        let records = load_pipeline(tmp.path(), &sym("S"), &tf("1m"), None, None).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].open_time, 100);
        assert_eq!(records[1].open_time, 200);
    }

    #[test]
    fn load_pipeline_bad_row_keeps_file_data() {
        let tmp = TempDir::new().unwrap();
        let dir = setup_dir(tmp.path(), "B", "1m");
        let csv = [
            "100,10.0,12.0,9.0,11.0,1000.0,0,0,0,0,0,0",
            "200,abc,12.0,9.0,11.0,1000.0,0,0,0,0,0,0",
        ]
        .join("\n");
        create_test_zip(&dir, "B-1m-2024-01-01.zip", &csv);

        let records = load_pipeline(tmp.path(), &sym("B"), &tf("1m"), None, None).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].open_time, 100);
    }

    #[test]
    fn load_pipeline_numeric_header_first_field_keeps_file_data() {
        let tmp = TempDir::new().unwrap();
        let dir = setup_dir(tmp.path(), "C", "1m");
        let csv = "1,open,high,low,close,volume,close_time,quote_volume,count,taker_buy_volume,taker_buy_quote_volume,ignore\n100,10.0,12.0,9.0,11.0,1000.0,0,0,0,0,0,0\n";
        create_test_zip(&dir, "C-1m-2024-01-01.zip", csv);

        let records = load_pipeline(tmp.path(), &sym("C"), &tf("1m"), None, None).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].open_time, 100);
    }

    #[test]
    fn load_pipeline_invalid_date_filename_errors() {
        // 非法日期形状是硬失败（零容忍）：即使有好文件，整批也 Err 并报告。
        let tmp = TempDir::new().unwrap();
        let dir = setup_dir(tmp.path(), "D", "1m");
        let csv = write_kline_csv([(100, 10.0, 12.0, 9.0, 11.0, 1000.0)]);
        create_test_zip(&dir, "D-1m-2024-01-01.zip", &csv);
        create_test_zip(&dir, "D-1m-2024-13-99.zip", &csv);

        let result = load_pipeline(tmp.path(), &sym("D"), &tf("1m"), None, None);
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("1 file(s) failed"), "unexpected: {msg}");
        assert!(msg.contains("2024-13-99"), "unexpected: {msg}");
    }

    #[test]
    fn load_pipeline_corrupt_dated_file_errors() {
        let tmp = TempDir::new().unwrap();
        let dir = setup_dir(tmp.path(), "E", "1m");
        let csv = write_kline_csv([(100, 10.0, 12.0, 9.0, 11.0, 1000.0)]);
        create_test_zip(&dir, "E-1m-2024-01-01.zip", &csv);
        std::fs::write(dir.join("E-1m-2024-01-02.zip"), b"this is not a zip").unwrap();

        let result = load_pipeline(tmp.path(), &sym("E"), &tf("1m"), None, None);
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("1 file(s) failed"), "unexpected: {msg}");
    }
}

//! CSV decoding shared by every dataset.
//!
//! Archive quirks handled here:
//!
//! - spot files are usually headerless while futures files start with the
//!   exact field-name header; both are accepted;
//! - leading blank lines are skipped without shifting reported line numbers;
//! - a header belonging to another market is a file-level error, not a stream
//!   of confusing row errors;
//! - row-level failures either abort ([`BadRowPolicy::Fail`]) or are counted
//!   and sampled ([`BadRowPolicy::Skip`]).

use std::io::{BufRead, BufReader, Read};

use chrono::NaiveDateTime;
use csv::StringRecord;

use crate::error::{Error, Result};
use crate::types::BadRowPolicy;

/// Maximum number of skipped-row samples kept per file.
pub(crate) const MAX_SKIP_SAMPLES: usize = 5;

/// Header contract for one dataset/market combination.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Header {
    /// Header line this loader expects (and consumes) when present.
    pub expected: &'static [&'static str],
    /// Every header line that is valid for this dataset family.
    ///
    /// A first line matching one of these but not `expected` means the file
    /// belongs to a different market; loading it would silently mislabel
    /// columns, so it is rejected.
    pub accepted: &'static [&'static [&'static str]],
}

impl Header {
    /// Build a header contract.
    pub(crate) const fn new(
        expected: &'static [&'static str],
        accepted: &'static [&'static [&'static str]],
    ) -> Self {
        Self { expected, accepted }
    }
}

/// Successfully decoded rows plus row-level damage statistics.
#[derive(Debug)]
pub(crate) struct FileRows<T> {
    /// Parsed rows in file order.
    pub rows: Vec<T>,
    /// Number of rows dropped by [`BadRowPolicy::Skip`].
    pub skipped: usize,
    /// Bounded samples of skipped rows.
    pub samples: Vec<String>,
}

/// Decode one CSV stream.
///
/// `path` is only used for error messages. `parse` receives every data record;
/// the header is never passed to it.
///
/// # Errors
///
/// Returns [`Error::File`] when the header belongs to another market and
/// [`Error::BadRow`] on the first malformed row under
/// [`BadRowPolicy::Fail`].
pub(crate) fn read_rows<T>(
    path: &std::path::Path,
    reader: impl Read,
    header: Header,
    policy: BadRowPolicy,
    mut parse: impl FnMut(&StringRecord) -> std::result::Result<T, String>,
) -> Result<FileRows<T>> {
    let mut reader = BufReader::new(reader);
    let mut first_line = Vec::new();
    let mut blank_lines = 0usize;
    loop {
        first_line.clear();
        let read = reader
            .read_until(b'\n', &mut first_line)
            .map_err(|e| Error::io(format!("read {}", path.display()), e))?;
        if read == 0 {
            return Ok(FileRows {
                rows: Vec::new(),
                skipped: 0,
                samples: Vec::new(),
            });
        }
        if first_line.iter().all(|b| matches!(b, b'\r' | b'\n')) {
            blank_lines += usize::from(first_line.contains(&b'\n'));
            continue;
        }
        break;
    }

    let line = std::str::from_utf8(&first_line).map_err(|e| Error::File {
        path: path.to_path_buf(),
        detail: format!("first line is not UTF-8: {e}"),
    })?;
    let line = line
        .trim_end_matches(['\r', '\n'])
        .trim_start_matches('\u{feff}');

    let has_header = line.split(',').eq(header.expected.iter().copied());
    if !has_header
        && header
            .accepted
            .iter()
            .any(|candidate| line.split(',').eq(candidate.iter().copied()))
    {
        return Err(Error::File {
            path: path.to_path_buf(),
            detail: format!(
                "header belongs to a different market: [{line}]; expected [{}]",
                header.expected.join(",")
            ),
        });
    }

    let prefix = if has_header {
        Vec::new()
    } else if first_line.starts_with(b"\xef\xbb\xbf") {
        // The BOM was stripped for header detection; strip it for the CSV
        // parser too, otherwise the first field of a headerless file is
        // prefixed with an invisible character.
        first_line[3..].to_vec()
    } else {
        first_line
    };
    let stream = std::io::Cursor::new(prefix).chain(reader);
    let mut csv = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(stream);

    let mut rows = Vec::new();
    let mut skipped = 0usize;
    let mut samples = Vec::with_capacity(MAX_SKIP_SAMPLES);
    let mut record = StringRecord::new();
    let mut line_no = blank_lines + usize::from(has_header);

    loop {
        match csv.read_record(&mut record) {
            Ok(true) => {}
            Ok(false) => break,
            Err(e) => {
                line_no += 1;
                record_bad_row(
                    path,
                    line_no,
                    e.to_string(),
                    policy,
                    &mut skipped,
                    &mut samples,
                )?;
                continue;
            }
        }
        line_no += 1;
        match parse(&record) {
            Ok(row) => rows.push(row),
            Err(detail) => record_bad_row(
                path,
                line_no,
                format!("[{}] {detail}", row_snippet(&record)),
                policy,
                &mut skipped,
                &mut samples,
            )?,
        }
    }

    Ok(FileRows {
        rows,
        skipped,
        samples,
    })
}

fn record_bad_row(
    path: &std::path::Path,
    line: usize,
    detail: String,
    policy: BadRowPolicy,
    skipped: &mut usize,
    samples: &mut Vec<String>,
) -> Result<()> {
    if policy == BadRowPolicy::Fail {
        return Err(Error::BadRow {
            path: path.to_path_buf(),
            line,
            detail,
        });
    }
    *skipped += 1;
    if samples.len() < MAX_SKIP_SAMPLES {
        samples.push(format!("{}:{line}: {detail}", path.display()));
    }
    Ok(())
}

fn row_snippet(record: &StringRecord) -> String {
    let joined = record.iter().collect::<Vec<_>>().join(",");
    let end = joined
        .char_indices()
        .nth(120)
        .map_or(joined.len(), |(index, _)| index);
    if end == joined.len() {
        joined
    } else {
        format!("{}...", &joined[..end])
    }
}

// — field parsers: every failure is a row-level detail string —

/// Reject rows whose field count does not match the schema exactly.
pub(crate) fn expect_fields(
    row: &StringRecord,
    expected: usize,
    dataset: &str,
) -> std::result::Result<(), String> {
    if row.len() == expected {
        Ok(())
    } else {
        Err(format!(
            "{dataset}: expected {expected} fields, got {}",
            row.len()
        ))
    }
}

/// Fetch field `index`, producing a consistent missing-field message.
pub(crate) fn field<'a>(
    row: &'a StringRecord,
    index: usize,
    name: &str,
) -> std::result::Result<&'a str, String> {
    row.get(index)
        .map(str::trim)
        .ok_or_else(|| format!("missing {name}"))
}

/// Parse a signed 64-bit integer.
pub(crate) fn i64(
    row: &StringRecord,
    index: usize,
    name: &str,
) -> std::result::Result<i64, String> {
    field(row, index, name)?
        .parse()
        .map_err(|e| format!("{name}: {e}"))
}

/// Parse a non-negative integer, e.g. identifiers and counts.
pub(crate) fn non_negative_i64(
    row: &StringRecord,
    index: usize,
    name: &str,
) -> std::result::Result<i64, String> {
    let value = i64(row, index, name)?;
    if value < 0 {
        return Err(format!("{name}: negative value {value}"));
    }
    Ok(value)
}

/// Parse a non-negative millisecond timestamp.
pub(crate) fn timestamp(
    row: &StringRecord,
    index: usize,
    name: &str,
) -> std::result::Result<i64, String> {
    non_negative_i64(row, index, name)
}

/// Parse `YYYY-MM-DD HH:MM:SS` as UTC epoch milliseconds.
pub(crate) fn utc_datetime(
    row: &StringRecord,
    index: usize,
    name: &str,
) -> std::result::Result<i64, String> {
    let raw = field(row, index, name)?;
    let datetime = NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S")
        .map_err(|e| format!("{name}: {e}"))?;
    Ok(datetime.and_utc().timestamp_millis())
}

fn f64_raw(row: &StringRecord, index: usize, name: &str) -> std::result::Result<f64, String> {
    let value: f64 = field(row, index, name)?
        .parse()
        .map_err(|e| format!("{name}: {e}"))?;
    if !value.is_finite() {
        return Err(format!("{name}: non-finite value {value}"));
    }
    Ok(value)
}

/// Parse any finite float.
pub(crate) fn f64(
    row: &StringRecord,
    index: usize,
    name: &str,
) -> std::result::Result<f64, String> {
    f64_raw(row, index, name)
}

/// Parse a finite non-negative float.
pub(crate) fn non_negative_f64(
    row: &StringRecord,
    index: usize,
    name: &str,
) -> std::result::Result<f64, String> {
    let value = f64_raw(row, index, name)?;
    if value < 0.0 {
        return Err(format!("{name}: negative value {value}"));
    }
    Ok(value)
}

/// Parse a finite strictly-positive float.
pub(crate) fn positive_f64(
    row: &StringRecord,
    index: usize,
    name: &str,
) -> std::result::Result<f64, String> {
    let value = f64_raw(row, index, name)?;
    if value <= 0.0 {
        return Err(format!("{name}: non-positive value {value}"));
    }
    Ok(value)
}

/// Parse a non-negative float where an empty field means "not published".
pub(crate) fn optional_non_negative_f64(
    row: &StringRecord,
    index: usize,
    name: &str,
) -> std::result::Result<Option<f64>, String> {
    let raw = field(row, index, name)?;
    if raw.is_empty() {
        return Ok(None);
    }
    let value: f64 = raw.parse().map_err(|e| format!("{name}: {e}"))?;
    if !value.is_finite() {
        return Err(format!("{name}: non-finite value {value}"));
    }
    if value < 0.0 {
        return Err(format!("{name}: negative value {value}"));
    }
    Ok(Some(value))
}

/// Parse a case-insensitive boolean.
pub(crate) fn bool(
    row: &StringRecord,
    index: usize,
    name: &str,
) -> std::result::Result<bool, String> {
    let raw = field(row, index, name)?;
    if raw.eq_ignore_ascii_case("true") {
        Ok(true)
    } else if raw.eq_ignore_ascii_case("false") {
        Ok(false)
    } else {
        Err(format!("{name}: invalid boolean {raw:?}"))
    }
}

/// Parse a required non-empty string.
pub(crate) fn string(
    row: &StringRecord,
    index: usize,
    name: &str,
) -> std::result::Result<String, String> {
    let raw = field(row, index, name)?;
    if raw.is_empty() {
        return Err(format!("{name}: empty value"));
    }
    Ok(raw.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const HEADER: [&str; 2] = ["a", "b"];
    const OTHER: [&str; 2] = ["a", "c"];
    const ACCEPTED: [&[&str]; 2] = [&HEADER, &OTHER];
    const CONTRACT: Header = Header::new(&HEADER, &ACCEPTED);

    fn parse_pair(row: &StringRecord) -> std::result::Result<(i64, i64), String> {
        Ok((i64(row, 0, "a")?, i64(row, 1, "b")?))
    }

    #[test]
    fn header_is_consumed_and_lines_are_counted() {
        let rows = read_rows(
            Path::new("x"),
            "a,b\n1,2\n3,4\n".as_bytes(),
            CONTRACT,
            BadRowPolicy::Fail,
            parse_pair,
        )
        .unwrap();
        assert_eq!(rows.rows, vec![(1, 2), (3, 4)]);
        assert_eq!(rows.skipped, 0);
    }

    #[test]
    fn headerless_files_are_accepted() {
        let rows = read_rows(
            Path::new("x"),
            "1,2\n3,4".as_bytes(),
            CONTRACT,
            BadRowPolicy::Fail,
            parse_pair,
        )
        .unwrap();
        assert_eq!(rows.rows, vec![(1, 2), (3, 4)]);
    }

    #[test]
    fn a_byte_order_mark_does_not_corrupt_the_first_field() {
        let rows = read_rows(
            Path::new("x"),
            "\u{feff}1,2\n3,4\n".as_bytes(),
            CONTRACT,
            BadRowPolicy::Fail,
            parse_pair,
        )
        .unwrap();
        assert_eq!(rows.rows, vec![(1, 2), (3, 4)]);
    }

    #[test]
    fn leading_blank_lines_do_not_hide_the_header() {
        let rows = read_rows(
            Path::new("x"),
            "\n\r\na,b\n1,2\nx,3\n".as_bytes(),
            CONTRACT,
            BadRowPolicy::Fail,
            parse_pair,
        )
        .unwrap_err();
        let Error::BadRow { line, .. } = rows else {
            panic!("expected BadRow");
        };
        assert_eq!(line, 5);
    }

    #[test]
    fn skip_policy_samples_at_most_five_rows() {
        let mut csv = String::from("a,b\n");
        for _ in 0..8 {
            csv.push_str("x,1\n");
        }
        csv.push_str("9,10\n");
        let rows = read_rows(
            Path::new("x"),
            csv.as_bytes(),
            CONTRACT,
            BadRowPolicy::Skip,
            parse_pair,
        )
        .unwrap();
        assert_eq!(rows.rows, vec![(9, 10)]);
        assert_eq!(rows.skipped, 8);
        assert_eq!(rows.samples.len(), MAX_SKIP_SAMPLES);
        assert!(rows.samples[0].starts_with("x:2: "));
    }

    #[test]
    fn foreign_header_is_a_file_error() {
        let error = read_rows(
            Path::new("x"),
            "a,c\n1,2\n".as_bytes(),
            CONTRACT,
            BadRowPolicy::Fail,
            parse_pair,
        )
        .unwrap_err();
        assert!(matches!(error, Error::File { .. }));
        assert!(error.to_string().contains("different market"));
    }

    #[test]
    fn optional_fields_distinguish_empty_from_zero() {
        let record = StringRecord::from(vec!["1", "", "0"]);
        assert_eq!(optional_non_negative_f64(&record, 1, "x").unwrap(), None);
        assert_eq!(
            optional_non_negative_f64(&record, 2, "x").unwrap(),
            Some(0.0)
        );
    }

    #[test]
    fn float_parsers_reject_negative_and_non_finite() {
        let record = StringRecord::from(vec!["NaN", "inf", "-1"]);
        assert!(f64(&record, 0, "x").is_err());
        assert!(f64(&record, 1, "x").is_err());
        assert!(non_negative_f64(&record, 2, "x").is_err());
        assert!(positive_f64(&record, 2, "x").is_err());
    }

    #[test]
    fn utc_datetime_is_epoch_millis() {
        let record = StringRecord::from(vec!["2023-06-30 00:00:00"]);
        assert_eq!(
            utc_datetime(&record, 0, "time").unwrap(),
            day_start_ms_for_test()
        );
    }

    fn day_start_ms_for_test() -> i64 {
        chrono::NaiveDate::from_ymd_opt(2023, 6, 30)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis()
    }
}

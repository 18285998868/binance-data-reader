//! Futures metrics (`metrics`), 5-minute snapshots.
//!
//! `create_time` is a UTC string. Long/short ratio fields are frequently
//! empty in real archives (especially COIN-M), so they are optional. Empty is
//! preserved as `None`, never coerced to `0.0`, which would be a valid ratio.
//!
//! The `symbol` column is validated against the configured symbol. Real
//! archives use the contract symbol (e.g. `BTCUSD_PERP` for COIN-M), matching
//! the directory and config.

use std::path::PathBuf;

use csv::StringRecord;

use super::common::{self, DateWindow};
use crate::error::{ReaderError, Result};
use crate::types::Symbol;

/// One metrics snapshot. `symbol` is validated against the config and omitted
/// from the output columns.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MetricsRecord {
    pub(crate) create_time: i64,
    pub(crate) sum_open_interest: f64,
    pub(crate) sum_open_interest_value: f64,
    pub(crate) count_toptrader_long_short_ratio: Option<f64>,
    pub(crate) sum_toptrader_long_short_ratio: Option<f64>,
    pub(crate) count_long_short_ratio: Option<f64>,
    pub(crate) sum_taker_long_short_vol_ratio: Option<f64>,
}

const HEADER: [&str; 8] = [
    "create_time",
    "symbol",
    "sum_open_interest",
    "sum_open_interest_value",
    "count_toptrader_long_short_ratio",
    "sum_toptrader_long_short_ratio",
    "count_long_short_ratio",
    "sum_taker_long_short_vol_ratio",
];

/// Load metrics, keeping rows inside `window` by `create_time`.
///
/// Every row's `symbol` must equal `expected_symbol`; mismatches are reported
/// as row-level parse errors (warned and skipped by the shared loader).
pub(crate) fn load(
    files: &[PathBuf],
    window: &DateWindow,
    expected_symbol: &Symbol,
) -> Result<Vec<MetricsRecord>> {
    let headers: [&[&str]; 1] = [&HEADER];
    let rows = common::load_all(files, &headers, |row| parse_row(row, expected_symbol))?;
    Ok(window.trim(rows, |row| row.create_time))
}

fn parse_row(row: &StringRecord, expected_symbol: &Symbol) -> Result<MetricsRecord> {
    let symbol = common::parse_string(row, 1, "symbol")?;
    if symbol != expected_symbol.as_str() {
        return Err(ReaderError::Parse(format!(
            "symbol mismatch: archive has {symbol:?}, config expects {:?}",
            expected_symbol.as_str()
        )));
    }
    Ok(MetricsRecord {
        create_time: common::parse_utc_datetime(row, 0, "create_time")?,
        sum_open_interest: common::parse_non_negative_f64(row, 2, "sum_open_interest")?,
        sum_open_interest_value: common::parse_non_negative_f64(row, 3, "sum_open_interest_value")?,
        count_toptrader_long_short_ratio: common::parse_optional_non_negative_f64(
            row,
            4,
            "count_toptrader_long_short_ratio",
        )?,
        sum_toptrader_long_short_ratio: common::parse_optional_non_negative_f64(
            row,
            5,
            "sum_toptrader_long_short_ratio",
        )?,
        count_long_short_ratio: common::parse_optional_non_negative_f64(
            row,
            6,
            "count_long_short_ratio",
        )?,
        sum_taker_long_short_vol_ratio: common::parse_optional_non_negative_f64(
            row,
            7,
            "sum_taker_long_short_vol_ratio",
        )?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_for(line: &str, symbol: &str) -> Result<MetricsRecord> {
        let expected = Symbol::parse(symbol).unwrap();
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .from_reader(line.as_bytes());
        let row = reader.records().next().unwrap().unwrap();
        parse_row(&row, &expected)
    }

    #[test]
    fn complete_row() {
        let record = parse_for(
            "2023-11-22 14:05:00,1000BONKUSDT,61980076.0,291851.16,1.70588235,0.65883204,2.34482759,1.12177927",
            "1000BONKUSDT",
        )
        .unwrap();
        assert_eq!(
            record.create_time,
            chrono::NaiveDate::from_ymd_opt(2023, 11, 22)
                .unwrap()
                .and_hms_opt(14, 5, 0)
                .unwrap()
                .and_utc()
                .timestamp_millis()
        );
        assert_eq!(record.count_toptrader_long_short_ratio, Some(1.705_882_35));
    }

    #[test]
    fn empty_ratio_fields_are_none() {
        let record = parse_for(
            "2024-01-01 00:00:00,BTCUSD_PERP,8796805.0,20799.14,,,,1.44124606",
            "BTCUSD_PERP",
        )
        .unwrap();
        assert_eq!(record.count_toptrader_long_short_ratio, None);
        assert_eq!(record.sum_toptrader_long_short_ratio, None);
        assert_eq!(record.count_long_short_ratio, None);
        assert_eq!(record.sum_taker_long_short_vol_ratio, Some(1.441_246_06));
    }

    #[test]
    fn zero_open_interest_is_accepted() {
        let record = parse_for("2023-11-23 03:50:00,X,0E-8,0E-8,,,,1.0", "X").unwrap();
        assert!(record.sum_open_interest.abs() < f64::EPSILON);
    }

    #[test]
    fn bad_timestamp_is_rejected() {
        assert!(parse_for("2023-11-22T14:05:00,X,1,1,1,1,1,1", "X").is_err());
    }

    #[test]
    fn negative_ratio_is_rejected() {
        assert!(parse_for("2023-11-22 14:05:00,X,1,1,-1,1,1,1", "X").is_err());
    }

    #[test]
    fn symbol_mismatch_is_rejected() {
        let error = parse_for("2023-11-22 14:05:00,BTCUSD_PERP,1,1,1,1,1,1", "BTCUSDT")
            .unwrap_err()
            .to_string();
        assert!(error.contains("symbol mismatch"), "{error}");
    }
}

//! Futures metrics (`metrics`), daily archive only, 5-minute snapshots.
//!
//! `create_time` is a UTC string. Long/short ratio fields are frequently empty,
//! especially for COIN-M, so they are nullable columns: empty stays `None` and
//! is never coerced to `0.0` (which is a valid ratio).
//!
//! The archive `symbol` column is validated against the configured symbol and
//! returned, so downstream code can verify provenance without a second lookup.

use csv::StringRecord;

use crate::config::Config;
use crate::error::Result;
use crate::source::csv::{
    Header, expect_fields, non_negative_f64, optional_non_negative_f64, string, utc_datetime,
};
use crate::source::files::Scan;
use crate::table::{Column, Table};

use super::{Built, finish, read_dataset};

const METRICS_HEADER: [&str; 8] = [
    "create_time",
    "symbol",
    "sum_open_interest",
    "sum_open_interest_value",
    "count_toptrader_long_short_ratio",
    "sum_toptrader_long_short_ratio",
    "count_long_short_ratio",
    "sum_taker_long_short_vol_ratio",
];
const METRICS_ACCEPTED: [&[&str]; 1] = [&METRICS_HEADER];
const HEADER: Header = Header::new(&METRICS_HEADER, &METRICS_ACCEPTED);

/// One metrics snapshot.
#[derive(Debug, Clone)]
pub(crate) struct MetricsRow {
    pub create_time: i64,
    pub symbol: String,
    pub sum_open_interest: f64,
    pub sum_open_interest_value: f64,
    pub count_toptrader_long_short_ratio: Option<f64>,
    pub sum_toptrader_long_short_ratio: Option<f64>,
    pub count_long_short_ratio: Option<f64>,
    pub sum_taker_long_short_vol_ratio: Option<f64>,
}

/// Load futures metrics.
pub(crate) fn load(config: &Config, scan: &Scan) -> Result<Built> {
    let expected = config.symbol().as_str();
    let loaded = read_dataset(
        config,
        scan,
        HEADER,
        |row| parse_row(row, expected),
        |row| row.create_time,
    )?;

    let stats = loaded.stats;
    let mut create_time = Vec::with_capacity(loaded.rows.len());
    let mut symbol = Vec::with_capacity(loaded.rows.len());
    let mut sum_open_interest = Vec::with_capacity(loaded.rows.len());
    let mut sum_open_interest_value = Vec::with_capacity(loaded.rows.len());
    let mut count_toptrader_long_short_ratio = Vec::with_capacity(loaded.rows.len());
    let mut sum_toptrader_long_short_ratio = Vec::with_capacity(loaded.rows.len());
    let mut count_long_short_ratio = Vec::with_capacity(loaded.rows.len());
    let mut sum_taker_long_short_vol_ratio = Vec::with_capacity(loaded.rows.len());

    for row in loaded.rows {
        create_time.push(row.create_time);
        symbol.push(row.symbol);
        sum_open_interest.push(row.sum_open_interest);
        sum_open_interest_value.push(row.sum_open_interest_value);
        count_toptrader_long_short_ratio.push(row.count_toptrader_long_short_ratio);
        sum_toptrader_long_short_ratio.push(row.sum_toptrader_long_short_ratio);
        count_long_short_ratio.push(row.count_long_short_ratio);
        sum_taker_long_short_vol_ratio.push(row.sum_taker_long_short_vol_ratio);
    }

    let table = Table::new(vec![
        Column::i64("create_time", create_time),
        Column::strings("symbol", symbol),
        Column::f64("sum_open_interest", sum_open_interest),
        Column::f64("sum_open_interest_value", sum_open_interest_value),
        Column::optional_f64(
            "count_toptrader_long_short_ratio",
            count_toptrader_long_short_ratio,
        ),
        Column::optional_f64(
            "sum_toptrader_long_short_ratio",
            sum_toptrader_long_short_ratio,
        ),
        Column::optional_f64("count_long_short_ratio", count_long_short_ratio),
        Column::optional_f64(
            "sum_taker_long_short_vol_ratio",
            sum_taker_long_short_vol_ratio,
        ),
    ])?;
    finish(table, stats, 0, 0)
}

fn parse_row(row: &StringRecord, expected_symbol: &str) -> std::result::Result<MetricsRow, String> {
    expect_fields(row, METRICS_HEADER.len(), "metrics")?;
    let symbol = string(row, 1, "symbol")?;
    if symbol != expected_symbol {
        return Err(format!(
            "symbol mismatch: archive has {symbol:?}, config expects {expected_symbol:?}"
        ));
    }
    Ok(MetricsRow {
        create_time: utc_datetime(row, 0, "create_time")?,
        symbol,
        sum_open_interest: non_negative_f64(row, 2, "sum_open_interest")?,
        sum_open_interest_value: non_negative_f64(row, 3, "sum_open_interest_value")?,
        count_toptrader_long_short_ratio: optional_non_negative_f64(
            row,
            4,
            "count_toptrader_long_short_ratio",
        )?,
        sum_toptrader_long_short_ratio: optional_non_negative_f64(
            row,
            5,
            "sum_toptrader_long_short_ratio",
        )?,
        count_long_short_ratio: optional_non_negative_f64(row, 6, "count_long_short_ratio")?,
        sum_taker_long_short_vol_ratio: optional_non_negative_f64(
            row,
            7,
            "sum_taker_long_short_vol_ratio",
        )?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str, symbol: &str) -> std::result::Result<MetricsRow, String> {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .from_reader(line.as_bytes());
        parse_row(&reader.records().next().unwrap().unwrap(), symbol)
    }

    #[test]
    fn empty_ratios_stay_none() {
        let row = parse(
            "2024-01-01 00:00:00,BTCUSD_PERP,8796805.0,20799.14,,,,1.44124606",
            "BTCUSD_PERP",
        )
        .unwrap();
        assert_eq!(row.count_toptrader_long_short_ratio, None);
        assert_eq!(row.sum_toptrader_long_short_ratio, None);
        assert_eq!(row.count_long_short_ratio, None);
        assert_eq!(row.sum_taker_long_short_vol_ratio, Some(1.441_246_06));
    }

    #[test]
    fn zero_open_interest_is_valid() {
        let row = parse("2023-11-23 03:50:00,X,0E-8,0E-8,,,,1.0", "X").unwrap();
        assert!(row.sum_open_interest.abs() < f64::EPSILON);
    }

    #[test]
    fn symbol_mismatch_and_bad_time_fail() {
        assert!(parse("2023-11-22 14:05:00,BTCUSD_PERP,1,1,1,1,1,1", "BTCUSDT").is_err());
        assert!(parse("2023-11-22T14:05:00,X,1,1,1,1,1,1", "X").is_err());
    }
}

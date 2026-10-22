//! Dataset readers. Each module returns plain row structs; sorting, dedupe and
//! column transposition happen in `app::load_columns`.

use std::path::PathBuf;

use csv::StringRecord;

use super::common::{self, DateWindow};
use crate::error::{ReaderError, Result};

/// Kline-shaped rows for index/mark/premium price archives.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct KlineRecord {
    pub(crate) open_time: i64,
    pub(crate) open: f64,
    pub(crate) high: f64,
    pub(crate) low: f64,
    pub(crate) close: f64,
    pub(crate) volume: f64,
}

/// Archive CSV header for kline-shaped datasets.
const KLINE_HEADER: [&str; 12] = [
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

/// Load index/mark/premium klines.
///
/// `allows_non_positive_price` is `true` for premium-index values (rates
/// around zero); index/mark prices must be strictly positive.
pub(crate) fn load(
    files: &[PathBuf],
    window: &DateWindow,
    allows_non_positive_price: bool,
) -> Result<Vec<KlineRecord>> {
    let headers: [&[&str]; 1] = [&KLINE_HEADER];
    let rows = common::load_all(files, &headers, |row| {
        parse_row(row, allows_non_positive_price)
    })?;
    Ok(window.trim(rows, |row| row.open_time))
}

fn parse_row(row: &StringRecord, allows_non_positive_price: bool) -> Result<KlineRecord> {
    let open_time = common::parse_timestamp(row, 0, "open_time")?;
    let open = common::parse_f64(row, 1, "open")?;
    let high = common::parse_f64(row, 2, "high")?;
    let low = common::parse_f64(row, 3, "low")?;
    let close = common::parse_f64(row, 4, "close")?;
    let volume = common::parse_non_negative_f64(row, 5, "volume")?;

    if !allows_non_positive_price && (open <= 0.0 || high <= 0.0 || low <= 0.0 || close <= 0.0) {
        return Err(ReaderError::Parse(format!(
            "non-positive price: open={open} high={high} low={low} close={close}"
        )));
    }
    let aligned = high >= low && high >= open && high >= close && low <= open && low <= close;
    if !aligned {
        return Err(ReaderError::Parse(format!(
            "ohlc invalid: open={open} high={high} low={low} close={close}"
        )));
    }
    Ok(KlineRecord {
        open_time,
        open,
        high,
        low,
        close,
        volume,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str, allows: bool) -> Result<KlineRecord> {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .from_reader(line.as_bytes());
        let row = reader.records().next().unwrap().unwrap();
        parse_row(&row, allows)
    }

    #[test]
    fn positive_kline_ok() {
        let record = parse(
            "1704067200000,42314.0,42350.0,42300.0,42331.9,289.641,0,0,0,0,0,0",
            false,
        )
        .unwrap();
        assert_eq!(record.open_time, 1_704_067_200_000);
        assert!((record.close - 42331.9).abs() < 1e-9);
    }

    #[test]
    fn premium_index_accepts_negative_values() {
        let record = parse(
            "1700654400000,0,0.01535645,-0.01398023,0.00178327,0,0,0,0,0,0,0",
            true,
        )
        .unwrap();
        assert!((record.low + 0.01398023).abs() < 1e-9);
    }

    #[test]
    fn index_rejects_zero_price() {
        assert!(
            parse(
                "1700654400000,0,0.01535645,-0.01398023,0.00178327,0,0,0,0,0,0,0",
                false
            )
            .is_err()
        );
    }

    #[test]
    fn misaligned_ohlc_is_rejected() {
        assert!(parse("1704067200000,10.0,9.0,12.0,11.0,100.0,0,0,0,0,0,0", false).is_err());
    }

    #[test]
    fn negative_volume_is_rejected() {
        assert!(parse("1704067200000,10.0,12.0,9.0,11.0,-1.0,0,0,0,0,0,0", false).is_err());
    }
}

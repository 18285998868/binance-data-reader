//! Best bid/ask ticker (`bookTicker`) and order-book depth snapshots
//! (`bookDepth`).
//!
//! `bookTicker` rows are event-stream snapshots keyed by `update_id`; early
//! rows may legitimately carry a zero ask price/quantity (no quote yet), so
//! zero is accepted while negative/non-finite values are rejected.
//!
//! `bookDepth` rows are periodic snapshots with one row per percentage level,
//! keyed by `(timestamp, percentage)`. The timestamp is a UTC string.

use std::path::PathBuf;

use csv::StringRecord;

use super::common::{self, DateWindow};
use crate::error::Result;

/// One best bid/ask snapshot.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BookTickerRecord {
    pub(crate) update_id: i64,
    pub(crate) best_bid_price: f64,
    pub(crate) best_bid_qty: f64,
    pub(crate) best_ask_price: f64,
    pub(crate) best_ask_qty: f64,
    pub(crate) transaction_time: i64,
    pub(crate) event_time: i64,
}

/// One depth level of a book snapshot.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BookDepthRecord {
    pub(crate) time: i64,
    pub(crate) percentage: i32,
    pub(crate) depth: f64,
    pub(crate) notional: f64,
}

const BOOK_TICKER_HEADER: [&str; 7] = [
    "update_id",
    "best_bid_price",
    "best_bid_qty",
    "best_ask_price",
    "best_ask_qty",
    "transaction_time",
    "event_time",
];
const BOOK_DEPTH_HEADER: [&str; 4] = ["timestamp", "percentage", "depth", "notional"];

/// Load book-ticker rows, keeping those inside `window` by `event_time`.
pub(crate) fn load_book_ticker(
    files: &[PathBuf],
    window: &DateWindow,
) -> Result<Vec<BookTickerRecord>> {
    let headers: [&[&str]; 1] = [&BOOK_TICKER_HEADER];
    let rows = common::load_all(files, &headers, parse_book_ticker_row)?;
    Ok(window.trim(rows, |row| row.event_time))
}

/// Load book-depth rows, keeping those inside `window` by `timestamp`.
pub(crate) fn load_book_depth(
    files: &[PathBuf],
    window: &DateWindow,
) -> Result<Vec<BookDepthRecord>> {
    let headers: [&[&str]; 1] = [&BOOK_DEPTH_HEADER];
    let rows = common::load_all(files, &headers, parse_book_depth_row)?;
    Ok(window.trim(rows, |row| row.time))
}

fn parse_book_ticker_row(row: &StringRecord) -> Result<BookTickerRecord> {
    Ok(BookTickerRecord {
        update_id: common::parse_non_negative_i64(row, 0, "update_id")?,
        best_bid_price: common::parse_non_negative_f64(row, 1, "best_bid_price")?,
        best_bid_qty: common::parse_non_negative_f64(row, 2, "best_bid_qty")?,
        best_ask_price: common::parse_non_negative_f64(row, 3, "best_ask_price")?,
        best_ask_qty: common::parse_non_negative_f64(row, 4, "best_ask_qty")?,
        transaction_time: common::parse_timestamp(row, 5, "transaction_time")?,
        event_time: common::parse_timestamp(row, 6, "event_time")?,
    })
}

fn parse_book_depth_row(row: &StringRecord) -> Result<BookDepthRecord> {
    Ok(BookDepthRecord {
        time: common::parse_utc_datetime(row, 0, "timestamp")?,
        percentage: common::parse_i32(row, 1, "percentage")?,
        depth: common::parse_non_negative_f64(row, 2, "depth")?,
        notional: common::parse_non_negative_f64(row, 3, "notional")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ticker(line: &str) -> Result<BookTickerRecord> {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .from_reader(line.as_bytes());
        let row = reader.records().next().unwrap().unwrap();
        parse_book_ticker_row(&row)
    }

    fn parse_depth(line: &str) -> Result<BookDepthRecord> {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .from_reader(line.as_bytes());
        let row = reader.records().next().unwrap().unwrap();
        parse_book_depth_row(&row)
    }

    #[test]
    fn zero_ask_is_accepted() {
        let record =
            parse_ticker("3540021753883,0.004411,100000,0E-8,0E-8,1700661601009,1700661601013")
                .unwrap();
        assert!(record.best_ask_price.abs() < f64::EPSILON);
        assert_eq!(record.event_time, 1_700_661_601_013);
    }

    #[test]
    fn negative_quote_is_rejected() {
        assert!(
            parse_ticker("1,0.004411,100000,0.00488,2000000,1700661601333,1700661601338").is_ok()
        );
        assert!(
            parse_ticker("1,0.004411,-100000,0.00488,2000000,1700661601333,1700661601338").is_err()
        );
    }

    #[test]
    fn book_depth_timestamp_is_utc_ms() {
        let record = parse_depth("2023-11-22 14:00:33,-5,14565358.0,67221.532496").unwrap();
        assert_eq!(record.percentage, -5);
        assert_eq!(
            record.time,
            chrono::NaiveDate::from_ymd_opt(2023, 11, 22)
                .unwrap()
                .and_hms_opt(14, 0, 33)
                .unwrap()
                .and_utc()
                .timestamp_millis()
        );
    }

    #[test]
    fn book_depth_rejects_bad_timestamp() {
        assert!(parse_depth("2023-11-22T14:00:33,-5,1.0,2.0").is_err());
    }

    #[test]
    fn book_depth_rejects_negative_depth() {
        assert!(parse_depth("2023-11-22 14:00:33,-5,-1.0,2.0").is_err());
    }
}

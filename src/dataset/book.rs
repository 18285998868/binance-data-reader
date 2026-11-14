//! Best bid/ask ticker (`bookTicker`) and order-book depth snapshots
//! (`bookDepth`).
//!
//! `bookTicker` rows may legitimately carry a zero ask price/quantity before
//! the first quote, so zero is accepted while negative and non-finite values
//! are rejected. `bookDepth` is a daily-only futures family with one row per
//! percentage level; the archive levels are `-5..=-1` and `1..=5`.

use csv::StringRecord;

use crate::config::Config;
use crate::error::Result;
use crate::source::csv::{
    Header, expect_fields, i64, non_negative_f64, non_negative_i64, timestamp, utc_datetime,
};
use crate::source::files::Scan;
use crate::table::{Column, Table};

use super::{Built, finish, read_dataset};

const BOOK_TICKER_HEADER: [&str; 7] = [
    "update_id",
    "best_bid_price",
    "best_bid_qty",
    "best_ask_price",
    "best_ask_qty",
    "transaction_time",
    "event_time",
];
const BOOK_TICKER_ACCEPTED: [&[&str]; 1] = [&BOOK_TICKER_HEADER];
const BOOK_TICKER: Header = Header::new(&BOOK_TICKER_HEADER, &BOOK_TICKER_ACCEPTED);

const BOOK_DEPTH_HEADER: [&str; 4] = ["timestamp", "percentage", "depth", "notional"];
const BOOK_DEPTH_ACCEPTED: [&[&str]; 1] = [&BOOK_DEPTH_HEADER];
const BOOK_DEPTH: Header = Header::new(&BOOK_DEPTH_HEADER, &BOOK_DEPTH_ACCEPTED);

/// One best bid/ask snapshot.
#[derive(Debug, Clone)]
pub(crate) struct BookTickerRow {
    pub update_id: i64,
    pub best_bid_price: f64,
    pub best_bid_qty: f64,
    pub best_ask_price: f64,
    pub best_ask_qty: f64,
    pub transaction_time: i64,
    pub event_time: i64,
}

/// One depth level of a book snapshot.
#[derive(Debug, Clone)]
pub(crate) struct BookDepthRow {
    pub time: i64,
    pub percentage: i64,
    pub depth: f64,
    pub notional: f64,
}

/// Load best bid/ask ticker snapshots.
pub(crate) fn load_book_ticker(config: &Config, scan: &Scan) -> Result<Built> {
    let loaded = read_dataset(config, scan, BOOK_TICKER, parse_book_ticker, |row| {
        row.event_time
    })?;

    let stats = loaded.stats;
    let mut update_id = Vec::with_capacity(loaded.rows.len());
    let mut best_bid_price = Vec::with_capacity(loaded.rows.len());
    let mut best_bid_qty = Vec::with_capacity(loaded.rows.len());
    let mut best_ask_price = Vec::with_capacity(loaded.rows.len());
    let mut best_ask_qty = Vec::with_capacity(loaded.rows.len());
    let mut transaction_time = Vec::with_capacity(loaded.rows.len());
    let mut event_time = Vec::with_capacity(loaded.rows.len());

    for row in loaded.rows {
        update_id.push(row.update_id);
        best_bid_price.push(row.best_bid_price);
        best_bid_qty.push(row.best_bid_qty);
        best_ask_price.push(row.best_ask_price);
        best_ask_qty.push(row.best_ask_qty);
        transaction_time.push(row.transaction_time);
        event_time.push(row.event_time);
    }

    let table = Table::new(vec![
        Column::i64("update_id", update_id),
        Column::f64("best_bid_price", best_bid_price),
        Column::f64("best_bid_qty", best_bid_qty),
        Column::f64("best_ask_price", best_ask_price),
        Column::f64("best_ask_qty", best_ask_qty),
        Column::i64("transaction_time", transaction_time),
        Column::i64("event_time", event_time),
    ])?;
    finish(table, stats, 0, 0)
}

/// Load order-book depth snapshots.
pub(crate) fn load_book_depth(config: &Config, scan: &Scan) -> Result<Built> {
    let loaded = read_dataset(config, scan, BOOK_DEPTH, parse_book_depth, |row| row.time)?;

    let stats = loaded.stats;
    let mut time = Vec::with_capacity(loaded.rows.len());
    let mut percentage = Vec::with_capacity(loaded.rows.len());
    let mut depth = Vec::with_capacity(loaded.rows.len());
    let mut notional = Vec::with_capacity(loaded.rows.len());

    for row in loaded.rows {
        time.push(row.time);
        percentage.push(row.percentage);
        depth.push(row.depth);
        notional.push(row.notional);
    }

    let table = Table::new(vec![
        Column::i64("timestamp", time),
        Column::i64("percentage", percentage),
        Column::f64("depth", depth),
        Column::f64("notional", notional),
    ])?;
    finish(table, stats, 0, 0)
}

fn parse_book_ticker(row: &StringRecord) -> std::result::Result<BookTickerRow, String> {
    expect_fields(row, BOOK_TICKER_HEADER.len(), "bookTicker")?;
    Ok(BookTickerRow {
        update_id: non_negative_i64(row, 0, "update_id")?,
        best_bid_price: non_negative_f64(row, 1, "best_bid_price")?,
        best_bid_qty: non_negative_f64(row, 2, "best_bid_qty")?,
        best_ask_price: non_negative_f64(row, 3, "best_ask_price")?,
        best_ask_qty: non_negative_f64(row, 4, "best_ask_qty")?,
        transaction_time: timestamp(row, 5, "transaction_time")?,
        event_time: timestamp(row, 6, "event_time")?,
    })
}

fn parse_book_depth(row: &StringRecord) -> std::result::Result<BookDepthRow, String> {
    expect_fields(row, BOOK_DEPTH_HEADER.len(), "bookDepth")?;
    let percentage = i64(row, 1, "percentage")?;
    if !matches!(percentage, -5..=-1 | 1..=5) {
        return Err(format!(
            "percentage {percentage} is not one of the archive levels -5..=-1 or 1..=5"
        ));
    }
    Ok(BookDepthRow {
        time: utc_datetime(row, 0, "timestamp")?,
        percentage,
        depth: non_negative_f64(row, 2, "depth")?,
        notional: non_negative_f64(row, 3, "notional")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(line: &str) -> StringRecord {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .from_reader(line.as_bytes());
        reader.records().next().unwrap().unwrap()
    }

    #[test]
    fn zero_ask_is_accepted_but_negative_quotes_are_not() {
        assert!(
            parse_book_ticker(&record(
                "3540021753883,0.004411,100000,0E-8,0E-8,1700661601009,1700661601013"
            ))
            .is_ok()
        );
        assert!(
            parse_book_ticker(&record(
                "1,0.004411,-100000,0.00488,2000000,1700661601333,1700661601338"
            ))
            .is_err()
        );
    }

    #[test]
    fn book_depth_timestamp_and_levels_are_validated() {
        let row =
            parse_book_depth(&record("2023-11-22 14:00:33,-5,14565358.0,67221.532496")).unwrap();
        assert_eq!(row.percentage, -5);
        assert!(parse_book_depth(&record("2023-11-22T14:00:33,-5,1.0,2.0")).is_err());
        for invalid in [0, 6, -6, 100] {
            assert!(
                parse_book_depth(&record(&format!("2023-11-22 14:00:33,{invalid},1.0,2.0")))
                    .is_err()
            );
        }
    }

    #[test]
    fn ticker_and_depth_tables_match_archive_names() {
        let ticker = Table::new(vec![
            Column::i64("update_id", vec![1]),
            Column::f64("best_bid_price", vec![1.0]),
            Column::f64("best_bid_qty", vec![2.0]),
            Column::f64("best_ask_price", vec![3.0]),
            Column::f64("best_ask_qty", vec![4.0]),
            Column::i64("transaction_time", vec![5]),
            Column::i64("event_time", vec![6]),
        ])
        .unwrap();
        assert_eq!(ticker.column("event_time").unwrap().len(), 1);
    }
}

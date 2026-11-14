//! Liquidation snapshots (`liquidationSnapshot`), COIN-M daily archive only.
//!
//! Real archives contain exactly duplicated lines. Duplicates are compared on
//! the **full row identity** (floats by bit pattern) and removed keep-first;
//! unlike kline duplicates, they are a known archive artefact rather than a
//! sign of an unusable file. String columns are preserved verbatim.

use std::cmp::Ordering;

use csv::StringRecord;

use crate::config::Config;
use crate::error::Result;
use crate::source::csv::{
    Header, expect_fields, non_negative_f64, positive_f64, string, timestamp,
};
use crate::source::files::Scan;
use crate::table::{Column, Table};

use super::{Built, finish, read_dataset};

const LIQUIDATION_HEADER: [&str; 10] = [
    "time",
    "side",
    "order_type",
    "time_in_force",
    "original_quantity",
    "price",
    "average_price",
    "order_status",
    "last_fill_quantity",
    "accumulated_fill_quantity",
];
const LIQUIDATION_ACCEPTED: [&[&str]; 1] = [&LIQUIDATION_HEADER];
const HEADER: Header = Header::new(&LIQUIDATION_HEADER, &LIQUIDATION_ACCEPTED);

/// One liquidation snapshot row.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LiquidationRow {
    pub time: i64,
    pub side: String,
    pub order_type: String,
    pub time_in_force: String,
    pub original_quantity: f64,
    pub price: f64,
    pub average_price: f64,
    pub order_status: String,
    pub last_fill_quantity: f64,
    pub accumulated_fill_quantity: f64,
}

impl LiquidationRow {
    /// Total order over the full row identity, placing exact duplicates next
    /// to each other. Floats are compared by bit pattern for a stable,
    /// total order.
    fn identity_cmp(&self, other: &Self) -> Ordering {
        self.time
            .cmp(&other.time)
            .then_with(|| self.side.cmp(&other.side))
            .then_with(|| self.order_type.cmp(&other.order_type))
            .then_with(|| self.time_in_force.cmp(&other.time_in_force))
            .then_with(|| {
                self.original_quantity
                    .to_bits()
                    .cmp(&other.original_quantity.to_bits())
            })
            .then_with(|| self.price.to_bits().cmp(&other.price.to_bits()))
            .then_with(|| {
                self.average_price
                    .to_bits()
                    .cmp(&other.average_price.to_bits())
            })
            .then_with(|| self.order_status.cmp(&other.order_status))
            .then_with(|| {
                self.last_fill_quantity
                    .to_bits()
                    .cmp(&other.last_fill_quantity.to_bits())
            })
            .then_with(|| {
                self.accumulated_fill_quantity
                    .to_bits()
                    .cmp(&other.accumulated_fill_quantity.to_bits())
            })
    }
}

/// Load liquidation snapshots and remove exact duplicate rows.
pub(crate) fn load(config: &Config, scan: &Scan) -> Result<Built> {
    let loaded = read_dataset(config, scan, HEADER, parse_row, |row| row.time)?;
    let stats = loaded.stats;
    let mut rows = loaded.rows;
    let duplicates_removed = dedupe(&mut rows);

    let mut time = Vec::with_capacity(rows.len());
    let mut side = Vec::with_capacity(rows.len());
    let mut order_type = Vec::with_capacity(rows.len());
    let mut time_in_force = Vec::with_capacity(rows.len());
    let mut original_quantity = Vec::with_capacity(rows.len());
    let mut price = Vec::with_capacity(rows.len());
    let mut average_price = Vec::with_capacity(rows.len());
    let mut order_status = Vec::with_capacity(rows.len());
    let mut last_fill_quantity = Vec::with_capacity(rows.len());
    let mut accumulated_fill_quantity = Vec::with_capacity(rows.len());

    for row in rows {
        time.push(row.time);
        side.push(row.side);
        order_type.push(row.order_type);
        time_in_force.push(row.time_in_force);
        original_quantity.push(row.original_quantity);
        price.push(row.price);
        average_price.push(row.average_price);
        order_status.push(row.order_status);
        last_fill_quantity.push(row.last_fill_quantity);
        accumulated_fill_quantity.push(row.accumulated_fill_quantity);
    }

    let table = Table::new(vec![
        Column::i64("time", time),
        Column::strings("side", side),
        Column::strings("order_type", order_type),
        Column::strings("time_in_force", time_in_force),
        Column::f64("original_quantity", original_quantity),
        Column::f64("price", price),
        Column::f64("average_price", average_price),
        Column::strings("order_status", order_status),
        Column::f64("last_fill_quantity", last_fill_quantity),
        Column::f64("accumulated_fill_quantity", accumulated_fill_quantity),
    ])?;
    finish(table, stats, 0, duplicates_removed)
}

/// Sort by full identity and drop exact duplicates, returning how many were
/// removed.
fn dedupe(rows: &mut Vec<LiquidationRow>) -> usize {
    rows.sort_by(LiquidationRow::identity_cmp);
    let before = rows.len();
    rows.dedup();
    before - rows.len()
}

fn parse_row(row: &StringRecord) -> std::result::Result<LiquidationRow, String> {
    expect_fields(row, LIQUIDATION_HEADER.len(), "liquidationSnapshot")?;
    Ok(LiquidationRow {
        time: timestamp(row, 0, "time")?,
        side: string(row, 1, "side")?,
        order_type: string(row, 2, "order_type")?,
        time_in_force: string(row, 3, "time_in_force")?,
        original_quantity: non_negative_f64(row, 4, "original_quantity")?,
        price: positive_f64(row, 5, "price")?,
        average_price: positive_f64(row, 6, "average_price")?,
        order_status: string(row, 7, "order_status")?,
        last_fill_quantity: non_negative_f64(row, 8, "last_fill_quantity")?,
        accumulated_fill_quantity: non_negative_f64(row, 9, "accumulated_fill_quantity")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str = "1687656471926,BUY,LIMIT,IOC,7,30741.3,30631.6,FILLED,6,7";

    fn parse(line: &str) -> std::result::Result<LiquidationRow, String> {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .from_reader(line.as_bytes());
        parse_row(&reader.records().next().unwrap().unwrap())
    }

    #[test]
    fn row_is_parsed_verbatim() {
        let row = parse(LINE).unwrap();
        assert_eq!(row.side, "BUY");
        assert_eq!(row.order_status, "FILLED");
        assert!((row.average_price - 30_631.6).abs() < 1e-9);
    }

    #[test]
    fn empty_string_and_non_positive_price_fail() {
        assert!(parse("1687656471926,BUY,LIMIT,IOC,7,30741.3,30631.6,,6,7").is_err());
        assert!(parse("1687656471926,BUY,LIMIT,IOC,7,0,30631.6,FILLED,6,7").is_err());
    }

    #[test]
    fn exact_duplicates_are_removed_across_the_whole_batch() {
        let mut rows = vec![
            parse(LINE).unwrap(),
            parse("1687656472000,SELL,MARKET,GTC,1,30000.0,30000.0,FILLED,1,1").unwrap(),
            parse(LINE).unwrap(),
        ];
        assert_eq!(dedupe(&mut rows), 1);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].time, 1_687_656_471_926);
    }

    #[test]
    fn near_duplicates_are_kept() {
        let mut rows = vec![
            parse(LINE).unwrap(),
            parse(&LINE.replace(",7", ",8")).unwrap(),
        ];
        assert_eq!(dedupe(&mut rows), 0);
        assert_eq!(rows.len(), 2);
    }
}

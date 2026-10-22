//! Liquidation snapshots (`liquidationSnapshot`), COIN-M only.
//!
//! Real archives contain exactly duplicated lines, so loading keeps the first
//! occurrence of each full row identity instead of trusting the file to be
//! unique. String columns (`side`, `order_type`, `time_in_force`,
//! `order_status`) are preserved verbatim for downstream filtering.

use std::path::PathBuf;

use csv::StringRecord;

use super::common::{self, DateWindow};
use crate::error::Result;

/// One liquidation snapshot row.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LiquidationRecord {
    pub(crate) time: i64,
    pub(crate) side: String,
    pub(crate) order_type: String,
    pub(crate) time_in_force: String,
    pub(crate) original_quantity: f64,
    pub(crate) price: f64,
    pub(crate) average_price: f64,
    pub(crate) order_status: String,
    pub(crate) last_fill_quantity: f64,
    pub(crate) accumulated_fill_quantity: f64,
}

impl LiquidationRecord {
    /// Full-row identity used for exact duplicate removal.
    ///
    /// Floating-point fields are compared by bit pattern: an exact duplicate
    /// line has identical bits, and distinct values never collide.
    pub(crate) fn identity(
        &self,
    ) -> (i64, String, String, String, u64, u64, u64, String, u64, u64) {
        (
            self.time,
            self.side.clone(),
            self.order_type.clone(),
            self.time_in_force.clone(),
            self.original_quantity.to_bits(),
            self.price.to_bits(),
            self.average_price.to_bits(),
            self.order_status.clone(),
            self.last_fill_quantity.to_bits(),
            self.accumulated_fill_quantity.to_bits(),
        )
    }
}

const HEADER: [&str; 10] = [
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

/// Load liquidation snapshots, keeping rows inside `window` by `time`.
pub(crate) fn load(files: &[PathBuf], window: &DateWindow) -> Result<Vec<LiquidationRecord>> {
    let headers: [&[&str]; 1] = [&HEADER];
    let rows = common::load_all(files, &headers, parse_row)?;
    Ok(window.trim(rows, |row| row.time))
}

fn parse_row(row: &StringRecord) -> Result<LiquidationRecord> {
    let original_quantity = common::parse_non_negative_f64(row, 4, "original_quantity")?;
    let last_fill_quantity = common::parse_non_negative_f64(row, 8, "last_fill_quantity")?;
    let accumulated_fill_quantity =
        common::parse_non_negative_f64(row, 9, "accumulated_fill_quantity")?;
    Ok(LiquidationRecord {
        time: common::parse_timestamp(row, 0, "time")?,
        side: common::parse_string(row, 1, "side")?,
        order_type: common::parse_string(row, 2, "order_type")?,
        time_in_force: common::parse_string(row, 3, "time_in_force")?,
        original_quantity,
        price: common::parse_positive_f64(row, 5, "price")?,
        average_price: common::parse_positive_f64(row, 6, "average_price")?,
        order_status: common::parse_string(row, 7, "order_status")?,
        last_fill_quantity,
        accumulated_fill_quantity,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> Result<LiquidationRecord> {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .from_reader(line.as_bytes());
        let row = reader.records().next().unwrap().unwrap();
        parse_row(&row)
    }

    #[test]
    fn cm_row_is_parsed_verbatim() {
        let record =
            parse("1704068163557,BUY,LIMIT,IOC,285,42708.1,42548.0,FILLED,136,285").unwrap();
        assert_eq!(record.side, "BUY");
        assert_eq!(record.order_status, "FILLED");
        assert!((record.average_price - 42548.0).abs() < 1e-9);
    }

    #[test]
    fn empty_status_is_rejected() {
        assert!(parse("1704068163557,BUY,LIMIT,IOC,285,42708.1,42548.0,,136,285").is_err());
    }

    #[test]
    fn over_filled_quantity_is_preserved() {
        // Real archives are not guaranteed to satisfy fill <= original; the
        // loader must not drop rows on an assumption it cannot verify.
        let record =
            parse("1704068163557,BUY,LIMIT,IOC,100,42708.1,42548.0,FILLED,136,285").unwrap();
        assert!((record.accumulated_fill_quantity - 285.0).abs() < f64::EPSILON);
    }

    #[test]
    fn identity_ignores_object_identity() {
        let line = "1704068163557,BUY,LIMIT,IOC,285,42708.1,42548.0,FILLED,136,285";
        let a = parse(line).unwrap();
        let b = parse(line).unwrap();
        assert_eq!(a.identity(), b.identity());
    }
}

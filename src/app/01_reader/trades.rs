//! Aggregate trades (`aggTrades`) and individual trades (`trades`).
//!
//! Archive quirks verified against real files:
//! - spot `aggTrades`/`trades` are **headerless** and carry an extra
//!   `is_best_match` column (8/7 columns);
//! - USDⓈ-M futures carry a header and no `is_best_match`;
//! - COIN-M `trades` column 3 is `base_qty` (coin quantity), not `quote_qty`.

use std::path::PathBuf;

use csv::StringRecord;

use super::common::{self, DateWindow};
use crate::error::{ReaderError, Result};
use crate::types::Market;

/// One aggregate trade.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AggTradeRecord {
    pub(crate) agg_trade_id: i64,
    pub(crate) price: f64,
    pub(crate) quantity: f64,
    pub(crate) first_trade_id: i64,
    pub(crate) last_trade_id: i64,
    pub(crate) transact_time: i64,
    pub(crate) is_buyer_maker: bool,
}

/// Quantity side carried by a trade row.
///
/// Spot and USDⓈ-M archives publish `quote_qty` in column 3; COIN-M publishes
/// `base_qty` instead. The enum makes the two cases mutually exclusive, so an
/// invalid row cannot carry both or neither.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum TradeQty {
    /// Quote-asset quantity (spot/USDⓈ-M).
    Quote(f64),
    /// Base-asset quantity (COIN-M).
    Base(f64),
}

/// One individual trade.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TradeRecord {
    pub(crate) trade_id: i64,
    pub(crate) price: f64,
    pub(crate) qty: f64,
    pub(crate) qty_side: TradeQty,
    pub(crate) time: i64,
    pub(crate) is_buyer_maker: bool,
}

const AGG_TRADES: [&str; 7] = [
    "agg_trade_id",
    "price",
    "quantity",
    "first_trade_id",
    "last_trade_id",
    "transact_time",
    "is_buyer_maker",
];
const AGG_TRADES_SPOT: [&str; 8] = [
    "agg_trade_id",
    "price",
    "quantity",
    "first_trade_id",
    "last_trade_id",
    "transact_time",
    "is_buyer_maker",
    "is_best_match",
];
const TRADES_UM: [&str; 6] = ["id", "price", "qty", "quote_qty", "time", "is_buyer_maker"];
const TRADES_CM: [&str; 6] = ["id", "price", "qty", "base_qty", "time", "is_buyer_maker"];
const TRADES_SPOT: [&str; 7] = [
    "id",
    "price",
    "qty",
    "quote_qty",
    "time",
    "is_buyer_maker",
    "is_best_match",
];

/// Load aggregate trades, keeping rows inside `window` by `transact_time`.
pub(crate) fn load_agg_trades(
    files: &[PathBuf],
    window: &DateWindow,
) -> Result<Vec<AggTradeRecord>> {
    let headers: [&[&str]; 2] = [&AGG_TRADES, &AGG_TRADES_SPOT];
    let rows = common::load_all(files, &headers, parse_agg_trade_row)?;
    Ok(window.trim(rows, |row| row.transact_time))
}

/// Load individual trades, keeping rows inside `window` by `time`.
///
/// `market` selects the semantics of column 3 (`quote_qty` vs `base_qty`).
pub(crate) fn load_trades(
    files: &[PathBuf],
    window: &DateWindow,
    market: Market,
) -> Result<Vec<TradeRecord>> {
    let headers: [&[&str]; 3] = [&TRADES_UM, &TRADES_CM, &TRADES_SPOT];
    let rows = common::load_all(files, &headers, |row| parse_trade_row(row, market))?;
    Ok(window.trim(rows, |row| row.time))
}

fn parse_agg_trade_row(row: &StringRecord) -> Result<AggTradeRecord> {
    let agg_trade_id = common::parse_non_negative_i64(row, 0, "agg_trade_id")?;
    let price = common::parse_positive_f64(row, 1, "price")?;
    let quantity = common::parse_positive_f64(row, 2, "quantity")?;
    let first_trade_id = common::parse_non_negative_i64(row, 3, "first_trade_id")?;
    let last_trade_id = common::parse_non_negative_i64(row, 4, "last_trade_id")?;
    if first_trade_id > last_trade_id {
        return Err(ReaderError::Parse(format!(
            "first_trade_id {first_trade_id} > last_trade_id {last_trade_id}"
        )));
    }
    let transact_time = common::parse_timestamp(row, 5, "transact_time")?;
    let is_buyer_maker = common::parse_bool(row, 6, "is_buyer_maker")?;
    Ok(AggTradeRecord {
        agg_trade_id,
        price,
        quantity,
        first_trade_id,
        last_trade_id,
        transact_time,
        is_buyer_maker,
    })
}

fn parse_trade_row(row: &StringRecord, market: Market) -> Result<TradeRecord> {
    let trade_id = common::parse_non_negative_i64(row, 0, "id")?;
    let price = common::parse_positive_f64(row, 1, "price")?;
    let qty = common::parse_positive_f64(row, 2, "qty")?;
    let qty_side = match market {
        Market::Cm => TradeQty::Base(common::parse_non_negative_f64(row, 3, "base_qty")?),
        Market::Spot | Market::Um => {
            TradeQty::Quote(common::parse_non_negative_f64(row, 3, "quote_qty")?)
        }
    };
    let time = common::parse_timestamp(row, 4, "time")?;
    let is_buyer_maker = common::parse_bool(row, 5, "is_buyer_maker")?;
    Ok(TradeRecord {
        trade_id,
        price,
        qty,
        qty_side,
        time,
        is_buyer_maker,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_one(line: &str, market: Market) -> Result<TradeRecord> {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .from_reader(line.as_bytes());
        let row = reader.records().next().unwrap().unwrap();
        parse_trade_row(&row, market)
    }

    fn parse_agg(line: &str) -> Result<AggTradeRecord> {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .from_reader(line.as_bytes());
        let row = reader.records().next().unwrap().unwrap();
        parse_agg_trade_row(&row)
    }

    #[test]
    fn um_agg_trade() {
        let record = parse_agg("1,0.00478,10460.0,1,1,1700661606844,false").unwrap();
        assert_eq!(record.agg_trade_id, 1);
        assert!((record.quantity - 10460.0).abs() < f64::EPSILON);
        assert!(!record.is_buyer_maker);
    }

    #[test]
    fn spot_agg_trade_extra_column_is_ignored() {
        let record =
            parse_agg("515330094,311.9,0.022,697594291,697594291,1704067200107,False,True")
                .unwrap();
        assert_eq!(record.last_trade_id, 697_594_291);
        assert!(!record.is_buyer_maker);
    }

    #[test]
    fn spot_trade_extra_column_is_ignored() {
        let record = parse_one(
            "697594291,311.9,0.022,6.8618,1704067200107,False,True",
            Market::Spot,
        )
        .unwrap();
        assert!(matches!(record.qty_side, TradeQty::Quote(value) if (value - 6.8618).abs() < 1e-9));
    }

    #[test]
    fn cm_trade_uses_base_qty() {
        let record = parse_one(
            "704644606,42305.5,3.0,0.00709127,1704067202014,false",
            Market::Cm,
        )
        .unwrap();
        assert!(
            matches!(record.qty_side, TradeQty::Base(value) if (value - 0.00709127).abs() < 1e-12)
        );
    }

    #[test]
    fn invalid_bool_is_rejected() {
        assert!(parse_agg("1,1.0,1.0,1,1,1700661606844,maybe").is_err());
    }

    #[test]
    fn non_positive_price_is_rejected() {
        assert!(parse_agg("1,0.0,1.0,1,1,1700661606844,false").is_err());
        assert!(parse_agg("1,1.0,0.0,1,1,1700661606844,false").is_err());
    }

    #[test]
    fn inverted_trade_ids_are_rejected() {
        assert!(parse_agg("1,1.0,1.0,9,8,1700661606844,false").is_err());
    }
}

//! Aggregate trades (`aggTrades`) and individual trades (`trades`).
//!
//! Archive quirks verified against real files:
//!
//! - spot `aggTrades`/`trades` are usually headerless and carry an extra
//!   `is_best_match` column (8/7 fields);
//! - USDⓈ-M and COIN-M files carry a header with 7/6 fields;
//! - COIN-M `trades` field 3 is `base_qty`, not `quote_qty`.
//!
//! Unlike the old reader, `is_best_match`, `quote_qty` and `base_qty` are
//! returned as columns instead of being dropped.

use csv::StringRecord;

use crate::config::Config;
use crate::error::Result;
use crate::source::csv::{
    Header, bool, expect_fields, non_negative_f64, non_negative_i64, positive_f64, timestamp,
};
use crate::source::files::Scan;
use crate::table::{Column, Table};
use crate::types::Market;

use super::{Built, finish, read_dataset};

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
const AGG_ACCEPTED: [&[&str]; 2] = [&AGG_TRADES, &AGG_TRADES_SPOT];

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
const TRADES_ACCEPTED: [&[&str]; 3] = [&TRADES_UM, &TRADES_CM, &TRADES_SPOT];

/// One aggregate trade.
#[derive(Debug, Clone)]
pub(crate) struct AggTradeRow {
    pub agg_trade_id: i64,
    pub price: f64,
    pub quantity: f64,
    pub first_trade_id: i64,
    pub last_trade_id: i64,
    pub transact_time: i64,
    pub is_buyer_maker: bool,
    pub is_best_match: Option<bool>,
}

/// Quantity side carried by a trade row.
#[derive(Debug, Clone, Copy)]
pub(crate) enum TradeQty {
    /// Quote-asset quantity (spot/USDⓈ-M).
    Quote(f64),
    /// Base-asset quantity (COIN-M).
    Base(f64),
}

/// One individual trade.
#[derive(Debug, Clone)]
pub(crate) struct TradeRow {
    pub id: i64,
    pub price: f64,
    pub qty: f64,
    pub qty_side: TradeQty,
    pub time: i64,
    pub is_buyer_maker: bool,
    pub is_best_match: Option<bool>,
}

/// Load aggregate trades.
pub(crate) fn load_agg_trades(config: &Config, scan: &Scan) -> Result<Built> {
    let market = config.market();
    let expected: &'static [&'static str] = match market {
        Market::Spot => &AGG_TRADES_SPOT,
        Market::Um | Market::Cm => &AGG_TRADES,
    };
    let loaded = read_dataset(
        config,
        scan,
        Header::new(expected, &AGG_ACCEPTED),
        |row| parse_agg_trade(row, market),
        |row| row.transact_time,
    )?;

    let stats = loaded.stats;
    let table = build_agg_table(loaded.rows, market)?;
    finish(table, stats, 0, 0)
}

/// Load individual trades.
pub(crate) fn load_trades(config: &Config, scan: &Scan) -> Result<Built> {
    let market = config.market();
    let expected: &'static [&'static str] = match market {
        Market::Spot => &TRADES_SPOT,
        Market::Um => &TRADES_UM,
        Market::Cm => &TRADES_CM,
    };
    let loaded = read_dataset(
        config,
        scan,
        Header::new(expected, &TRADES_ACCEPTED),
        |row| parse_trade(row, market),
        |row| row.time,
    )?;

    let stats = loaded.stats;
    let table = build_trade_table(loaded.rows, market)?;
    finish(table, stats, 0, 0)
}

fn parse_agg_trade(row: &StringRecord, market: Market) -> std::result::Result<AggTradeRow, String> {
    let spot = market == Market::Spot;
    expect_fields(row, if spot { 8 } else { 7 }, "aggTrades")?;
    let agg_trade_id = non_negative_i64(row, 0, "agg_trade_id")?;
    let first_trade_id = non_negative_i64(row, 3, "first_trade_id")?;
    let last_trade_id = non_negative_i64(row, 4, "last_trade_id")?;
    if first_trade_id > last_trade_id {
        return Err(format!(
            "first_trade_id {first_trade_id} > last_trade_id {last_trade_id}"
        ));
    }
    Ok(AggTradeRow {
        agg_trade_id,
        price: positive_f64(row, 1, "price")?,
        quantity: positive_f64(row, 2, "quantity")?,
        first_trade_id,
        last_trade_id,
        transact_time: timestamp(row, 5, "transact_time")?,
        is_buyer_maker: bool(row, 6, "is_buyer_maker")?,
        is_best_match: if spot {
            Some(bool(row, 7, "is_best_match")?)
        } else {
            None
        },
    })
}

fn parse_trade(row: &StringRecord, market: Market) -> std::result::Result<TradeRow, String> {
    let spot = market == Market::Spot;
    expect_fields(row, if spot { 7 } else { 6 }, "trades")?;
    let qty_side = match market {
        Market::Cm => TradeQty::Base(non_negative_f64(row, 3, "base_qty")?),
        Market::Spot | Market::Um => TradeQty::Quote(non_negative_f64(row, 3, "quote_qty")?),
    };
    Ok(TradeRow {
        id: non_negative_i64(row, 0, "id")?,
        price: positive_f64(row, 1, "price")?,
        qty: positive_f64(row, 2, "qty")?,
        qty_side,
        time: timestamp(row, 4, "time")?,
        is_buyer_maker: bool(row, 5, "is_buyer_maker")?,
        is_best_match: if spot {
            Some(bool(row, 6, "is_best_match")?)
        } else {
            None
        },
    })
}

fn build_agg_table(rows: Vec<AggTradeRow>, market: Market) -> Result<Table> {
    let spot = market == Market::Spot;
    let mut agg_trade_id = Vec::with_capacity(rows.len());
    let mut price = Vec::with_capacity(rows.len());
    let mut quantity = Vec::with_capacity(rows.len());
    let mut first_trade_id = Vec::with_capacity(rows.len());
    let mut last_trade_id = Vec::with_capacity(rows.len());
    let mut transact_time = Vec::with_capacity(rows.len());
    let mut is_buyer_maker = Vec::with_capacity(rows.len());
    let mut is_best_match = Vec::with_capacity(rows.len());

    for row in rows {
        agg_trade_id.push(row.agg_trade_id);
        price.push(row.price);
        quantity.push(row.quantity);
        first_trade_id.push(row.first_trade_id);
        last_trade_id.push(row.last_trade_id);
        transact_time.push(row.transact_time);
        is_buyer_maker.push(row.is_buyer_maker);
        if spot {
            // INVARIANT: the spot parser requires and sets field 7.
            is_best_match.push(row.is_best_match.unwrap_or(false));
        }
    }

    let mut columns = vec![
        Column::i64("agg_trade_id", agg_trade_id),
        Column::f64("price", price),
        Column::f64("quantity", quantity),
        Column::i64("first_trade_id", first_trade_id),
        Column::i64("last_trade_id", last_trade_id),
        Column::i64("transact_time", transact_time),
        Column::bool("is_buyer_maker", is_buyer_maker),
    ];
    if spot {
        columns.push(Column::bool("is_best_match", is_best_match));
    }
    Table::new(columns)
}

fn build_trade_table(rows: Vec<TradeRow>, market: Market) -> Result<Table> {
    let spot = market == Market::Spot;
    let coin_m = market == Market::Cm;
    let mut id = Vec::with_capacity(rows.len());
    let mut price = Vec::with_capacity(rows.len());
    let mut qty = Vec::with_capacity(rows.len());
    let mut qty_side = Vec::with_capacity(rows.len());
    let mut time = Vec::with_capacity(rows.len());
    let mut is_buyer_maker = Vec::with_capacity(rows.len());
    let mut is_best_match = Vec::with_capacity(rows.len());

    for row in rows {
        id.push(row.id);
        price.push(row.price);
        qty.push(row.qty);
        qty_side.push(match row.qty_side {
            TradeQty::Quote(value) | TradeQty::Base(value) => value,
        });
        time.push(row.time);
        is_buyer_maker.push(row.is_buyer_maker);
        if spot {
            // INVARIANT: the spot parser requires and sets field 6.
            is_best_match.push(row.is_best_match.unwrap_or(false));
        }
    }

    let side_name = if coin_m { "base_qty" } else { "quote_qty" };
    let mut columns = vec![
        Column::i64("id", id),
        Column::f64("price", price),
        Column::f64("qty", qty),
        Column::f64(side_name, qty_side),
        Column::i64("time", time),
        Column::bool("is_buyer_maker", is_buyer_maker),
    ];
    if spot {
        columns.push(Column::bool("is_best_match", is_best_match));
    }
    Table::new(columns)
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
    fn um_agg_trade_has_seven_fields() {
        let row = parse_agg_trade(
            &record("1,0.00478,10460.0,1,1,1700661606844,false"),
            Market::Um,
        )
        .unwrap();
        assert_eq!(row.agg_trade_id, 1);
        assert_eq!(row.is_best_match, None);
    }

    #[test]
    fn spot_agg_trade_keeps_is_best_match() {
        let row = parse_agg_trade(
            &record("515330094,311.9,0.022,697594291,697594291,1704067200107,False,True"),
            Market::Spot,
        )
        .unwrap();
        assert_eq!(row.is_best_match, Some(true));
        assert!(!row.is_buyer_maker);
    }

    #[test]
    fn spot_trade_has_quote_qty_and_flag() {
        let row = parse_trade(
            &record("697594291,311.9,0.022,6.8618,1704067200107,False,True"),
            Market::Spot,
        )
        .unwrap();
        assert!(matches!(row.qty_side, TradeQty::Quote(value) if (value - 6.8618).abs() < 1e-9));
        assert_eq!(row.is_best_match, Some(true));
    }

    #[test]
    fn cm_trade_has_base_qty() {
        let row = parse_trade(
            &record("641961668,30441.9,1.0,0.00328494,1688083203266,true"),
            Market::Cm,
        )
        .unwrap();
        assert!(
            matches!(row.qty_side, TradeQty::Base(value) if (value - 0.00328494).abs() < 1e-12)
        );
    }

    #[test]
    fn wrong_field_count_and_boolean_fail() {
        assert!(
            parse_agg_trade(
                &record("1,2.0,3.0,1,1,1700000000000,not-a-bool"),
                Market::Um
            )
            .is_err()
        );
        assert!(parse_trade(&record("1,2.0,3.0,4.0,1700000000000"), Market::Um).is_err());
    }

    #[test]
    fn tables_expose_market_specific_columns() {
        let spot = build_agg_table(
            vec![AggTradeRow {
                agg_trade_id: 1,
                price: 2.0,
                quantity: 3.0,
                first_trade_id: 4,
                last_trade_id: 5,
                transact_time: 6,
                is_buyer_maker: true,
                is_best_match: Some(false),
            }],
            Market::Spot,
        )
        .unwrap();
        assert!(spot.column("is_best_match").is_some());
        assert_eq!(spot.bool("is_best_match"), Some(&[false][..]));

        let um = build_agg_table(
            vec![AggTradeRow {
                agg_trade_id: 1,
                price: 2.0,
                quantity: 3.0,
                first_trade_id: 4,
                last_trade_id: 5,
                transact_time: 6,
                is_buyer_maker: true,
                is_best_match: None,
            }],
            Market::Um,
        )
        .unwrap();
        assert!(um.column("is_best_match").is_none());
    }

    #[test]
    fn trade_table_names_the_third_quantity_field_per_market() {
        let row = TradeRow {
            id: 1,
            price: 2.0,
            qty: 3.0,
            qty_side: TradeQty::Base(4.0),
            time: 5,
            is_buyer_maker: true,
            is_best_match: None,
        };
        let table = build_trade_table(vec![row], Market::Cm).unwrap();
        assert_eq!(table.f64("base_qty"), Some(&[4.0][..]));
        assert!(table.column("quote_qty").is_none());
    }
}

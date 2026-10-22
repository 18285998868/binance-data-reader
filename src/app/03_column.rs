//! Column-oriented output: one typed column set per dataset family.

use crate::app::book::{BookDepthRecord, BookTickerRecord};
use crate::app::funding::FundingRateRecord;
use crate::app::klines::KlineRecord;
use crate::app::liquidation::LiquidationRecord;
use crate::app::metrics::MetricsRecord;
use crate::app::trades::{AggTradeRecord, TradeQty, TradeRecord};
use crate::types::Dataset;

/// OHLCV columns for index/mark/premium klines.
#[derive(Debug, Clone)]
pub struct KlineColumns {
    dataset: Dataset,
    time: Vec<i64>,
    open: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    close: Vec<f64>,
    volume: Vec<f64>,
}

impl KlineColumns {
    pub(crate) fn from_records(records: Vec<KlineRecord>, dataset: Dataset) -> Self {
        let n = records.len();
        let mut columns = Self {
            dataset,
            time: Vec::with_capacity(n),
            open: Vec::with_capacity(n),
            high: Vec::with_capacity(n),
            low: Vec::with_capacity(n),
            close: Vec::with_capacity(n),
            volume: Vec::with_capacity(n),
        };
        for record in records {
            columns.time.push(record.open_time);
            columns.open.push(record.open);
            columns.high.push(record.high);
            columns.low.push(record.low);
            columns.close.push(record.close);
            columns.volume.push(record.volume);
        }
        columns
    }

    /// Which kline-shaped dataset produced this column set.
    #[must_use]
    pub const fn dataset(&self) -> Dataset {
        self.dataset
    }

    /// Bar open times in milliseconds.
    #[must_use]
    pub fn time(&self) -> &[i64] {
        &self.time
    }
    /// Open prices.
    #[must_use]
    pub fn open(&self) -> &[f64] {
        &self.open
    }
    /// High prices.
    #[must_use]
    pub fn high(&self) -> &[f64] {
        &self.high
    }
    /// Low prices.
    #[must_use]
    pub fn low(&self) -> &[f64] {
        &self.low
    }
    /// Close prices.
    #[must_use]
    pub fn close(&self) -> &[f64] {
        &self.close
    }
    /// Volumes (zero for index/premium series).
    #[must_use]
    pub fn volume(&self) -> &[f64] {
        &self.volume
    }
}

/// Aggregate-trade columns.
#[derive(Debug, Clone)]
pub struct AggTradeColumns {
    time: Vec<i64>,
    agg_trade_id: Vec<i64>,
    price: Vec<f64>,
    quantity: Vec<f64>,
    first_trade_id: Vec<i64>,
    last_trade_id: Vec<i64>,
    is_buyer_maker: Vec<bool>,
}

impl AggTradeColumns {
    /// Trade transaction times in milliseconds.
    #[must_use]
    pub fn time(&self) -> &[i64] {
        &self.time
    }
    /// Aggregate trade IDs.
    #[must_use]
    pub fn agg_trade_id(&self) -> &[i64] {
        &self.agg_trade_id
    }
    /// Prices.
    #[must_use]
    pub fn price(&self) -> &[f64] {
        &self.price
    }
    /// Aggregate quantities.
    #[must_use]
    pub fn quantity(&self) -> &[f64] {
        &self.quantity
    }
    /// First raw trade ID in the aggregate.
    #[must_use]
    pub fn first_trade_id(&self) -> &[i64] {
        &self.first_trade_id
    }
    /// Last raw trade ID in the aggregate.
    #[must_use]
    pub fn last_trade_id(&self) -> &[i64] {
        &self.last_trade_id
    }
    /// Whether the buyer was the maker.
    #[must_use]
    pub fn is_buyer_maker(&self) -> &[bool] {
        &self.is_buyer_maker
    }
}

/// Individual-trade columns.
#[derive(Debug, Clone)]
pub struct TradeColumns {
    time: Vec<i64>,
    trade_id: Vec<i64>,
    price: Vec<f64>,
    qty: Vec<f64>,
    quote_qty: Option<Vec<f64>>,
    base_qty: Option<Vec<f64>>,
    is_buyer_maker: Vec<bool>,
}

impl TradeColumns {
    /// Trade times in milliseconds.
    #[must_use]
    pub fn time(&self) -> &[i64] {
        &self.time
    }
    /// Trade IDs.
    #[must_use]
    pub fn trade_id(&self) -> &[i64] {
        &self.trade_id
    }
    /// Prices.
    #[must_use]
    pub fn price(&self) -> &[f64] {
        &self.price
    }
    /// Raw quantity field (base quantity for spot/UM, contracts for CM).
    #[must_use]
    pub fn qty(&self) -> &[f64] {
        &self.qty
    }
    /// Quote-asset quantity (spot/UM only).
    #[must_use]
    pub fn quote_qty(&self) -> Option<&[f64]> {
        self.quote_qty.as_deref()
    }
    /// Base-asset quantity (CM only).
    #[must_use]
    pub fn base_qty(&self) -> Option<&[f64]> {
        self.base_qty.as_deref()
    }
    /// Whether the buyer was the maker.
    #[must_use]
    pub fn is_buyer_maker(&self) -> &[bool] {
        &self.is_buyer_maker
    }
}

/// Best bid/ask ticker columns.
#[derive(Debug, Clone)]
pub struct BookTickerColumns {
    time: Vec<i64>,
    update_id: Vec<i64>,
    best_bid_price: Vec<f64>,
    best_bid_qty: Vec<f64>,
    best_ask_price: Vec<f64>,
    best_ask_qty: Vec<f64>,
    transaction_time: Vec<i64>,
}

impl BookTickerColumns {
    /// Event times in milliseconds (`event_time`).
    #[must_use]
    pub fn time(&self) -> &[i64] {
        &self.time
    }
    /// Book update IDs.
    #[must_use]
    pub fn update_id(&self) -> &[i64] {
        &self.update_id
    }
    /// Best bid prices.
    #[must_use]
    pub fn best_bid_price(&self) -> &[f64] {
        &self.best_bid_price
    }
    /// Best bid quantities.
    #[must_use]
    pub fn best_bid_qty(&self) -> &[f64] {
        &self.best_bid_qty
    }
    /// Best ask prices.
    #[must_use]
    pub fn best_ask_price(&self) -> &[f64] {
        &self.best_ask_price
    }
    /// Best ask quantities.
    #[must_use]
    pub fn best_ask_qty(&self) -> &[f64] {
        &self.best_ask_qty
    }
    /// Exchange transaction times in milliseconds.
    #[must_use]
    pub fn transaction_time(&self) -> &[i64] {
        &self.transaction_time
    }
}

/// Order-book depth snapshot columns.
#[derive(Debug, Clone)]
pub struct BookDepthColumns {
    time: Vec<i64>,
    percentage: Vec<i32>,
    depth: Vec<f64>,
    notional: Vec<f64>,
}

impl BookDepthColumns {
    /// Snapshot times in milliseconds.
    #[must_use]
    pub fn time(&self) -> &[i64] {
        &self.time
    }
    /// Depth percentage levels (negative below the mid, positive above).
    #[must_use]
    pub fn percentage(&self) -> &[i32] {
        &self.percentage
    }
    /// Cumulative depth.
    #[must_use]
    pub fn depth(&self) -> &[f64] {
        &self.depth
    }
    /// Cumulative notional.
    #[must_use]
    pub fn notional(&self) -> &[f64] {
        &self.notional
    }
}

/// Funding-rate columns.
#[derive(Debug, Clone)]
pub struct FundingRateColumns {
    time: Vec<i64>,
    funding_interval_hours: Vec<i64>,
    last_funding_rate: Vec<f64>,
}

impl FundingRateColumns {
    /// Funding calculation times in milliseconds.
    #[must_use]
    pub fn time(&self) -> &[i64] {
        &self.time
    }
    /// Funding interval in hours.
    #[must_use]
    pub fn funding_interval_hours(&self) -> &[i64] {
        &self.funding_interval_hours
    }
    /// Signed funding rates.
    #[must_use]
    pub fn last_funding_rate(&self) -> &[f64] {
        &self.last_funding_rate
    }
}

/// Liquidation-snapshot columns.
#[derive(Debug, Clone)]
pub struct LiquidationColumns {
    time: Vec<i64>,
    side: Vec<String>,
    order_type: Vec<String>,
    time_in_force: Vec<String>,
    original_quantity: Vec<f64>,
    price: Vec<f64>,
    average_price: Vec<f64>,
    order_status: Vec<String>,
    last_fill_quantity: Vec<f64>,
    accumulated_fill_quantity: Vec<f64>,
}

impl LiquidationColumns {
    /// Liquidation times in milliseconds.
    #[must_use]
    pub fn time(&self) -> &[i64] {
        &self.time
    }
    /// Order sides (`BUY`/`SELL`).
    #[must_use]
    pub fn side(&self) -> &[String] {
        &self.side
    }
    /// Order types (`LIMIT`/`MARKET`).
    #[must_use]
    pub fn order_type(&self) -> &[String] {
        &self.order_type
    }
    /// Time in force values.
    #[must_use]
    pub fn time_in_force(&self) -> &[String] {
        &self.time_in_force
    }
    /// Original order quantities.
    #[must_use]
    pub fn original_quantity(&self) -> &[f64] {
        &self.original_quantity
    }
    /// Order prices.
    #[must_use]
    pub fn price(&self) -> &[f64] {
        &self.price
    }
    /// Average fill prices.
    #[must_use]
    pub fn average_price(&self) -> &[f64] {
        &self.average_price
    }
    /// Order statuses (`FILLED`/...).
    #[must_use]
    pub fn order_status(&self) -> &[String] {
        &self.order_status
    }
    /// Last fill quantity.
    #[must_use]
    pub fn last_fill_quantity(&self) -> &[f64] {
        &self.last_fill_quantity
    }
    /// Accumulated fill quantity.
    #[must_use]
    pub fn accumulated_fill_quantity(&self) -> &[f64] {
        &self.accumulated_fill_quantity
    }
}

/// Futures-metrics columns.
#[derive(Debug, Clone)]
pub struct MetricsColumns {
    time: Vec<i64>,
    sum_open_interest: Vec<f64>,
    sum_open_interest_value: Vec<f64>,
    count_toptrader_long_short_ratio: Vec<Option<f64>>,
    sum_toptrader_long_short_ratio: Vec<Option<f64>>,
    count_long_short_ratio: Vec<Option<f64>>,
    sum_taker_long_short_vol_ratio: Vec<Option<f64>>,
}

impl MetricsColumns {
    /// Snapshot times in milliseconds.
    #[must_use]
    pub fn time(&self) -> &[i64] {
        &self.time
    }
    /// Total open interest.
    #[must_use]
    pub fn sum_open_interest(&self) -> &[f64] {
        &self.sum_open_interest
    }
    /// Total open interest value.
    #[must_use]
    pub fn sum_open_interest_value(&self) -> &[f64] {
        &self.sum_open_interest_value
    }
    /// Top-trader long/short account ratio; `None` when not published.
    #[must_use]
    pub fn count_toptrader_long_short_ratio(&self) -> &[Option<f64>] {
        &self.count_toptrader_long_short_ratio
    }
    /// Top-trader long/short position ratio; `None` when not published.
    #[must_use]
    pub fn sum_toptrader_long_short_ratio(&self) -> &[Option<f64>] {
        &self.sum_toptrader_long_short_ratio
    }
    /// Global long/short account ratio; `None` when not published.
    #[must_use]
    pub fn count_long_short_ratio(&self) -> &[Option<f64>] {
        &self.count_long_short_ratio
    }
    /// Taker long/short volume ratio; `None` when not published.
    #[must_use]
    pub fn sum_taker_long_short_vol_ratio(&self) -> &[Option<f64>] {
        &self.sum_taker_long_short_vol_ratio
    }
}

macro_rules! impl_len_is_empty {
    ($($ty:ty),* $(,)?) => {
        $(
            impl $ty {
                /// Number of rows.
                #[must_use]
                pub fn len(&self) -> usize {
                    self.time.len()
                }

                /// Whether the column set has no rows.
                #[must_use]
                pub fn is_empty(&self) -> bool {
                    self.time.is_empty()
                }
            }
        )*
    };
}

impl_len_is_empty!(
    KlineColumns,
    AggTradeColumns,
    TradeColumns,
    BookTickerColumns,
    BookDepthColumns,
    FundingRateColumns,
    LiquidationColumns,
    MetricsColumns,
);

impl From<Vec<AggTradeRecord>> for AggTradeColumns {
    fn from(records: Vec<AggTradeRecord>) -> Self {
        let n = records.len();
        let mut columns = Self {
            time: Vec::with_capacity(n),
            agg_trade_id: Vec::with_capacity(n),
            price: Vec::with_capacity(n),
            quantity: Vec::with_capacity(n),
            first_trade_id: Vec::with_capacity(n),
            last_trade_id: Vec::with_capacity(n),
            is_buyer_maker: Vec::with_capacity(n),
        };
        for record in records {
            columns.time.push(record.transact_time);
            columns.agg_trade_id.push(record.agg_trade_id);
            columns.price.push(record.price);
            columns.quantity.push(record.quantity);
            columns.first_trade_id.push(record.first_trade_id);
            columns.last_trade_id.push(record.last_trade_id);
            columns.is_buyer_maker.push(record.is_buyer_maker);
        }
        columns
    }
}

impl From<Vec<TradeRecord>> for TradeColumns {
    fn from(records: Vec<TradeRecord>) -> Self {
        let n = records.len();
        let quote_side = records
            .first()
            .is_some_and(|row| matches!(row.qty_side, TradeQty::Quote(_)));
        let mut columns = Self {
            time: Vec::with_capacity(n),
            trade_id: Vec::with_capacity(n),
            price: Vec::with_capacity(n),
            qty: Vec::with_capacity(n),
            quote_qty: None,
            base_qty: None,
            is_buyer_maker: Vec::with_capacity(n),
        };
        let mut quote_qty = Vec::with_capacity(if quote_side { n } else { 0 });
        let mut base_qty = Vec::with_capacity(if quote_side { 0 } else { n });
        for record in records {
            columns.time.push(record.time);
            columns.trade_id.push(record.trade_id);
            columns.price.push(record.price);
            columns.qty.push(record.qty);
            match record.qty_side {
                TradeQty::Quote(value) => quote_qty.push(value),
                TradeQty::Base(value) => base_qty.push(value),
            }
            columns.is_buyer_maker.push(record.is_buyer_maker);
        }
        debug_assert!(
            quote_qty.is_empty() || base_qty.is_empty(),
            "trade rows must share a single quantity side"
        );
        columns.quote_qty = (!quote_qty.is_empty()).then_some(quote_qty);
        columns.base_qty = (!base_qty.is_empty()).then_some(base_qty);
        columns
    }
}

impl From<Vec<BookTickerRecord>> for BookTickerColumns {
    fn from(records: Vec<BookTickerRecord>) -> Self {
        let n = records.len();
        let mut columns = Self {
            time: Vec::with_capacity(n),
            update_id: Vec::with_capacity(n),
            best_bid_price: Vec::with_capacity(n),
            best_bid_qty: Vec::with_capacity(n),
            best_ask_price: Vec::with_capacity(n),
            best_ask_qty: Vec::with_capacity(n),
            transaction_time: Vec::with_capacity(n),
        };
        for record in records {
            columns.time.push(record.event_time);
            columns.update_id.push(record.update_id);
            columns.best_bid_price.push(record.best_bid_price);
            columns.best_bid_qty.push(record.best_bid_qty);
            columns.best_ask_price.push(record.best_ask_price);
            columns.best_ask_qty.push(record.best_ask_qty);
            columns.transaction_time.push(record.transaction_time);
        }
        columns
    }
}

impl From<Vec<BookDepthRecord>> for BookDepthColumns {
    fn from(records: Vec<BookDepthRecord>) -> Self {
        let n = records.len();
        let mut columns = Self {
            time: Vec::with_capacity(n),
            percentage: Vec::with_capacity(n),
            depth: Vec::with_capacity(n),
            notional: Vec::with_capacity(n),
        };
        for record in records {
            columns.time.push(record.time);
            columns.percentage.push(record.percentage);
            columns.depth.push(record.depth);
            columns.notional.push(record.notional);
        }
        columns
    }
}

impl From<Vec<FundingRateRecord>> for FundingRateColumns {
    fn from(records: Vec<FundingRateRecord>) -> Self {
        let n = records.len();
        let mut columns = Self {
            time: Vec::with_capacity(n),
            funding_interval_hours: Vec::with_capacity(n),
            last_funding_rate: Vec::with_capacity(n),
        };
        for record in records {
            columns.time.push(record.calc_time);
            columns
                .funding_interval_hours
                .push(record.funding_interval_hours);
            columns.last_funding_rate.push(record.last_funding_rate);
        }
        columns
    }
}

impl From<Vec<LiquidationRecord>> for LiquidationColumns {
    fn from(records: Vec<LiquidationRecord>) -> Self {
        let n = records.len();
        let mut columns = Self {
            time: Vec::with_capacity(n),
            side: Vec::with_capacity(n),
            order_type: Vec::with_capacity(n),
            time_in_force: Vec::with_capacity(n),
            original_quantity: Vec::with_capacity(n),
            price: Vec::with_capacity(n),
            average_price: Vec::with_capacity(n),
            order_status: Vec::with_capacity(n),
            last_fill_quantity: Vec::with_capacity(n),
            accumulated_fill_quantity: Vec::with_capacity(n),
        };
        for record in records {
            columns.time.push(record.time);
            columns.side.push(record.side);
            columns.order_type.push(record.order_type);
            columns.time_in_force.push(record.time_in_force);
            columns.original_quantity.push(record.original_quantity);
            columns.price.push(record.price);
            columns.average_price.push(record.average_price);
            columns.order_status.push(record.order_status);
            columns.last_fill_quantity.push(record.last_fill_quantity);
            columns
                .accumulated_fill_quantity
                .push(record.accumulated_fill_quantity);
        }
        columns
    }
}

impl From<Vec<MetricsRecord>> for MetricsColumns {
    fn from(records: Vec<MetricsRecord>) -> Self {
        let n = records.len();
        let mut columns = Self {
            time: Vec::with_capacity(n),
            sum_open_interest: Vec::with_capacity(n),
            sum_open_interest_value: Vec::with_capacity(n),
            count_toptrader_long_short_ratio: Vec::with_capacity(n),
            sum_toptrader_long_short_ratio: Vec::with_capacity(n),
            count_long_short_ratio: Vec::with_capacity(n),
            sum_taker_long_short_vol_ratio: Vec::with_capacity(n),
        };
        for record in records {
            columns.time.push(record.create_time);
            columns.sum_open_interest.push(record.sum_open_interest);
            columns
                .sum_open_interest_value
                .push(record.sum_open_interest_value);
            columns
                .count_toptrader_long_short_ratio
                .push(record.count_toptrader_long_short_ratio);
            columns
                .sum_toptrader_long_short_ratio
                .push(record.sum_toptrader_long_short_ratio);
            columns
                .count_long_short_ratio
                .push(record.count_long_short_ratio);
            columns
                .sum_taker_long_short_vol_ratio
                .push(record.sum_taker_long_short_vol_ratio);
        }
        columns
    }
}

/// Dataset-tagged column set returned by [`crate::load_columns`].
#[derive(Debug, Clone)]
pub enum DatasetColumns {
    /// Index/mark/premium klines.
    Klines(KlineColumns),
    /// Aggregate trades.
    AggTrades(AggTradeColumns),
    /// Individual trades.
    Trades(TradeColumns),
    /// Best bid/ask ticker.
    BookTicker(BookTickerColumns),
    /// Order-book depth snapshots.
    BookDepth(BookDepthColumns),
    /// Funding rates.
    FundingRate(FundingRateColumns),
    /// Liquidation snapshots.
    LiquidationSnapshot(LiquidationColumns),
    /// Futures metrics.
    Metrics(MetricsColumns),
}

impl DatasetColumns {
    /// The dataset family this column set belongs to.
    #[must_use]
    pub const fn dataset(&self) -> Dataset {
        match self {
            Self::Klines(columns) => columns.dataset(),
            Self::AggTrades(_) => Dataset::AggTrades,
            Self::Trades(_) => Dataset::Trades,
            Self::BookTicker(_) => Dataset::BookTicker,
            Self::BookDepth(_) => Dataset::BookDepth,
            Self::FundingRate(_) => Dataset::FundingRate,
            Self::LiquidationSnapshot(_) => Dataset::LiquidationSnapshot,
            Self::Metrics(_) => Dataset::Metrics,
        }
    }

    /// Number of rows.
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::Klines(columns) => columns.len(),
            Self::AggTrades(columns) => columns.len(),
            Self::Trades(columns) => columns.len(),
            Self::BookTicker(columns) => columns.len(),
            Self::BookDepth(columns) => columns.len(),
            Self::FundingRate(columns) => columns.len(),
            Self::LiquidationSnapshot(columns) => columns.len(),
            Self::Metrics(columns) => columns.len(),
        }
    }

    /// Whether the column set has no rows.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Primary time column of the wrapped dataset.
    #[must_use]
    pub fn time(&self) -> &[i64] {
        match self {
            Self::Klines(columns) => columns.time(),
            Self::AggTrades(columns) => columns.time(),
            Self::Trades(columns) => columns.time(),
            Self::BookTicker(columns) => columns.time(),
            Self::BookDepth(columns) => columns.time(),
            Self::FundingRate(columns) => columns.time(),
            Self::LiquidationSnapshot(columns) => columns.time(),
            Self::Metrics(columns) => columns.time(),
        }
    }

    /// Borrow kline columns, or `None` for any other dataset.
    #[must_use]
    pub fn as_klines(&self) -> Option<&KlineColumns> {
        match self {
            Self::Klines(columns) => Some(columns),
            _ => None,
        }
    }

    /// Borrow aggregate-trade columns, or `None`.
    #[must_use]
    pub fn as_agg_trades(&self) -> Option<&AggTradeColumns> {
        match self {
            Self::AggTrades(columns) => Some(columns),
            _ => None,
        }
    }

    /// Borrow trade columns, or `None`.
    #[must_use]
    pub fn as_trades(&self) -> Option<&TradeColumns> {
        match self {
            Self::Trades(columns) => Some(columns),
            _ => None,
        }
    }

    /// Borrow book-ticker columns, or `None`.
    #[must_use]
    pub fn as_book_ticker(&self) -> Option<&BookTickerColumns> {
        match self {
            Self::BookTicker(columns) => Some(columns),
            _ => None,
        }
    }

    /// Borrow book-depth columns, or `None`.
    #[must_use]
    pub fn as_book_depth(&self) -> Option<&BookDepthColumns> {
        match self {
            Self::BookDepth(columns) => Some(columns),
            _ => None,
        }
    }

    /// Borrow funding-rate columns, or `None`.
    #[must_use]
    pub fn as_funding_rate(&self) -> Option<&FundingRateColumns> {
        match self {
            Self::FundingRate(columns) => Some(columns),
            _ => None,
        }
    }

    /// Borrow liquidation columns, or `None`.
    #[must_use]
    pub fn as_liquidation(&self) -> Option<&LiquidationColumns> {
        match self {
            Self::LiquidationSnapshot(columns) => Some(columns),
            _ => None,
        }
    }

    /// Borrow metrics columns, or `None`.
    #[must_use]
    pub fn as_metrics(&self) -> Option<&MetricsColumns> {
        match self {
            Self::Metrics(columns) => Some(columns),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trade_columns_keep_quote_or_base_only() {
        let quote = TradeRecord {
            trade_id: 1,
            price: 10.0,
            qty: 2.0,
            qty_side: TradeQty::Quote(20.0),
            time: 100,
            is_buyer_maker: false,
        };
        let columns: TradeColumns = vec![quote].into();
        assert_eq!(columns.len(), 1);
        assert_eq!(columns.quote_qty().unwrap(), &[20.0]);
        assert!(columns.base_qty().is_none());

        let base = TradeRecord {
            trade_id: 2,
            price: 10.0,
            qty: 2.0,
            qty_side: TradeQty::Base(0.2),
            time: 101,
            is_buyer_maker: true,
        };
        let columns: TradeColumns = vec![base].into();
        assert_eq!(columns.len(), 1);
        assert!(columns.quote_qty().is_none());
        assert_eq!(columns.base_qty().unwrap(), &[0.2]);
    }

    #[test]
    fn metrics_optional_ratios_roundtrip() {
        let record = MetricsRecord {
            create_time: 100,
            sum_open_interest: 1.0,
            sum_open_interest_value: 2.0,
            count_toptrader_long_short_ratio: None,
            sum_toptrader_long_short_ratio: Some(1.5),
            count_long_short_ratio: None,
            sum_taker_long_short_vol_ratio: Some(0.5),
        };
        let columns: MetricsColumns = vec![record].into();
        assert_eq!(columns.count_toptrader_long_short_ratio(), &[None]);
        assert_eq!(columns.sum_toptrader_long_short_ratio(), &[Some(1.5)]);
        assert_eq!(columns.len(), 1);
        assert!(!columns.is_empty());
    }

    #[test]
    fn dataset_columns_tag_matches_variant() {
        let columns = DatasetColumns::Metrics(MetricsColumns {
            time: Vec::new(),
            sum_open_interest: Vec::new(),
            sum_open_interest_value: Vec::new(),
            count_toptrader_long_short_ratio: Vec::new(),
            sum_toptrader_long_short_ratio: Vec::new(),
            count_long_short_ratio: Vec::new(),
            sum_taker_long_short_vol_ratio: Vec::new(),
        });
        assert_eq!(columns.dataset(), Dataset::Metrics);
        assert!(columns.is_empty());
        assert!(columns.as_metrics().is_some());
        assert!(columns.as_klines().is_none());
    }
}

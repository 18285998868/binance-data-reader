//! Entry point: validate-once config → scan archive → parse rows → sort/dedupe
//! → typed column sets.

#[path = "01_reader/book.rs"]
mod book;
#[path = "02_clean.rs"]
mod clean;
#[path = "03_column.rs"]
pub mod columns;
#[path = "01_reader/common.rs"]
mod common;
#[path = "01_reader/funding.rs"]
mod funding;
#[path = "01_reader/klines.rs"]
mod klines;
#[path = "01_reader/liquidation.rs"]
mod liquidation;
#[path = "01_reader/metrics.rs"]
mod metrics;
#[path = "01_reader/trades.rs"]
mod trades;

use crate::config::DataConfig;
use crate::error::Result;
use crate::types::Dataset;

pub use columns::{
    AggTradeColumns, BookDepthColumns, BookTickerColumns, DatasetColumns, FundingRateColumns,
    KlineColumns, LiquidationColumns, MetricsColumns, TradeColumns,
};

/// Load one symbol × one dataset from the configured archive root.
///
/// The output rows are sorted by the dataset's primary key and deduplicated
/// (keep-first). Row-level bad data is warned and skipped; file-level damage
/// (corrupt zip, missing CSV, invalid filename date) fails the batch.
#[tracing::instrument(skip_all, fields(
    dataset = %config.dataset(),
    market = %config.market(),
    symbol = %config.symbol(),
))]
pub fn load_columns(config: &DataConfig) -> Result<DatasetColumns> {
    let dataset = config.dataset();
    let window = common::DateWindow::new(config.start_date(), config.end_date());
    let files = common::scan_zip_files(
        config.dir(),
        config.market(),
        dataset,
        config.symbol(),
        config.interval(),
        config.start_date(),
        config.end_date(),
    )?;

    let columns = match dataset {
        Dataset::IndexPriceKlines | Dataset::MarkPriceKlines | Dataset::PremiumIndexKlines => {
            let mut rows = klines::load(&files, &window, dataset.allows_non_positive_price())?;
            warn_dedup(
                config,
                clean::sort_dedup_by_key(&mut rows, |row| row.open_time),
                "open_time",
            );
            DatasetColumns::Klines(KlineColumns::from_records(rows, dataset))
        }
        Dataset::AggTrades => {
            let mut rows = trades::load_agg_trades(&files, &window)?;
            warn_dedup(
                config,
                clean::sort_dedup_by_key(&mut rows, |row| row.agg_trade_id),
                "agg_trade_id",
            );
            DatasetColumns::AggTrades(rows.into())
        }
        Dataset::Trades => {
            let mut rows = trades::load_trades(&files, &window, config.market())?;
            warn_dedup(
                config,
                clean::sort_dedup_by_key(&mut rows, |row| row.trade_id),
                "trade_id",
            );
            DatasetColumns::Trades(rows.into())
        }
        Dataset::BookTicker => {
            let mut rows = book::load_book_ticker(&files, &window)?;
            warn_dedup(
                config,
                clean::sort_dedup_by_key(&mut rows, |row| row.update_id),
                "update_id",
            );
            DatasetColumns::BookTicker(rows.into())
        }
        Dataset::BookDepth => {
            let mut rows = book::load_book_depth(&files, &window)?;
            warn_dedup(
                config,
                clean::sort_dedup_by_key(&mut rows, |row| (row.time, row.percentage)),
                "(time, percentage)",
            );
            DatasetColumns::BookDepth(rows.into())
        }
        Dataset::FundingRate => {
            let mut rows = funding::load(&files, &window)?;
            warn_dedup(
                config,
                clean::sort_dedup_by_key(&mut rows, |row| row.calc_time),
                "calc_time",
            );
            DatasetColumns::FundingRate(rows.into())
        }
        Dataset::Metrics => {
            let mut rows = metrics::load(&files, &window)?;
            warn_dedup(
                config,
                clean::sort_dedup_by_key(&mut rows, |row| row.create_time),
                "create_time",
            );
            DatasetColumns::Metrics(rows.into())
        }
        Dataset::LiquidationSnapshot => {
            let mut rows = liquidation::load(&files, &window)?;
            clean::sort_by_key(&mut rows, |row| row.time);
            warn_dedup(
                config,
                clean::dedupe_by_hash_key(&mut rows, liquidation::LiquidationRecord::identity),
                "full row",
            );
            DatasetColumns::LiquidationSnapshot(rows.into())
        }
    };

    Ok(columns)
}

fn warn_dedup(config: &DataConfig, removed: usize, key: &str) {
    if removed > 0 {
        tracing::warn!(
            dataset = %config.dataset(),
            symbol = %config.symbol(),
            key,
            removed,
            "deduped rows by key (keep first)"
        );
    }
}

//! Load Binance public-archive datasets that `kline-reader` does not cover
//! into typed column vectors.
//!
//! Covered families: `indexPriceKlines`/`markPriceKlines`/
//! `premiumIndexKlines`, `aggTrades`, `trades`, `bookTicker`, `bookDepth`,
//! `fundingRate`, `metrics`, and `liquidationSnapshot`.
//!
//! See the crate README for layout and configuration details.

pub mod app;
mod config;
mod error;
mod python;
mod types;

pub use app::{
    AggTradeColumns, BookDepthColumns, BookTickerColumns, DatasetColumns, FundingRateColumns,
    KlineColumns, LiquidationColumns, MetricsColumns, TradeColumns, load_columns,
};
pub use config::DataConfig;
pub use error::{ReaderError, Result};
use pyo3::prelude::*;
pub use types::{Dataset, Interval, Market, Symbol};

/// Python extension module (`maturin build` → `import binance_reader`).
#[pymodule]
fn binance_reader(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(python::py_load_columns, m)?)?;
    Ok(())
}

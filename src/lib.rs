//! Load Binance public-archive data into typed column vectors.
//!
//! Two loaders are exposed:
//!
//! - [`load_columns`] reads one symbol × one dataset (non-kline archive
//!   families plus index/mark/premium klines) into [`DatasetColumns`].
//! - [`load_kline_columns`] reads one symbol × one timeframe of regular klines
//!   into [`KlineColumns`] (OHLCV).
//!
//! See the crate README for layout and configuration details.

mod app;
mod config;
mod error;
mod python;
#[cfg(test)]
#[path = "test_helpers.rs"]
mod test_helpers;
mod types;

pub use app::{
    AggTradeColumns, BookDepthColumns, BookTickerColumns, DatasetColumns, FundingRateColumns,
    KlineColumns, KlineDatasetColumns, LiquidationColumns, MetricsColumns, TradeColumns,
    load_columns, load_kline_columns,
};
pub use config::{DataConfig, KlineConfig};
pub use error::{ReaderError, Result};
use pyo3::prelude::*;
pub use types::{
    Dataset, Interval, Market, Symbol, Timeframe, interval_ms_for, is_supported,
    supported_timeframes,
};

/// Python extension module (`maturin build` → `import binance_data_reader`).
#[pymodule]
fn binance_data_reader(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(python::py_load_columns, m)?)?;
    m.add_function(wrap_pyfunction!(python::py_load_kline_columns, m)?)?;
    Ok(())
}

//! Read Binance public archive data into typed columnar tables.
//!
//! The reader targets the tree produced by `binance-data-downloader`:
//!
//! ```text
//! {root}/data/{market}/{period}/{dataset}/{symbol}[/{interval}]/*.zip
//! ```
//!
//! `root` is the downloader's `output_dir`, **not** its parent. `market` is
//! `spot`, `futures/um` or `futures/cm`; `period` is `daily` or `monthly`.
//!
//! # What makes this reader strict
//!
//! - every `.zip` name must match the exact archive pattern;
//! - every archive must contain exactly one CSV entry;
//! - every row is validated against the schema and the calendar period of its
//!   file;
//! - klines must be strictly increasing with no duplicate open time and a
//!   `close_time` matching the interval;
//! - by default the first malformed row aborts the load
//!   ([`BadRowPolicy::Fail`]); [`BadRowPolicy::Skip`] keeps the rest and
//!   reports counts and samples.
//!
//! # Daily and monthly
//!
//! Both cadences are read. A daily file wins over a monthly file for the same
//! UTC day; monthly rows outside daily coverage are kept. Merging is done by
//! calendar cover, so multi-row datasets remain correct.
//!
//! # Output
//!
//! [`read`] returns a [`DatasetData`] with a [`Table`] of named typed columns
//! and a [`LoadReport`]. Every archive field is preserved, including kline
//! quote volume/count/taker fields, spot `is_best_match`, COIN-M `base_qty`
//! and nullable metrics ratios.
//!
//! # Features
//!
//! - `python` (off by default): build the `binance_data_reader` extension
//!   module with `read(config)` and `catalog(root, ...)`.
//!
//! # Toolchain
//!
//! Latest stable Rust, edition 2024 (`rust-toolchain.toml`).
//!
//! # Example
//!
//! ```no_run
//! use binance_data_reader::{Config, Dataset, read};
//!
//! let config = Config::builder("/data/data/binance", Dataset::Klines, "BTCUSDT")
//!     .interval("1m".parse()?)
//!     .build()?;
//! let data = read(&config)?;
//! assert_eq!(data.table.len(), data.report.rows);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod catalog;
mod config;
mod dataset;
mod error;
mod layout;
mod load;
mod report;
mod source;
mod table;
mod types;

#[cfg(feature = "python")]
mod python;

pub use catalog::{Catalog, CatalogEntry, CatalogFilter, catalog};
pub use config::{Config, ConfigBuilder};
pub use error::{Error, Result};
pub use load::{DatasetData, read};
pub use report::LoadReport;
pub use table::{Column, ColumnValues, Table};
pub use types::{BadRowPolicy, Dataset, Interval, Market, Period, Symbol};

//! Request configuration: TOML files, a typed builder and validation.
//!
//! Configuration is validated exactly once. [`Config`] has private fields and
//! can only be produced through [`ConfigBuilder::build`], [`Config::from_toml_str`]
//! or [`Config::from_toml_file`], so every function that takes `&Config` can
//! rely on the support matrix, interval requirement and date order.

use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use serde::Deserialize;

use crate::error::{Error, Result};
use crate::types::{BadRowPolicy, Dataset, Interval, Market, Symbol};

/// A validated request for one symbol × one dataset.
///
/// # Examples
///
/// ```
/// use binance_data_reader::{Config, Dataset};
///
/// let config = Config::builder("/data/data/binance", Dataset::Klines, "BTCUSDT")
///     .interval("1m".parse()?)
///     .build()?;
/// assert_eq!(config.symbol().as_str(), "BTCUSDT");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone)]
pub struct Config {
    root: PathBuf,
    market: Market,
    dataset: Dataset,
    symbol: Symbol,
    interval: Option<Interval>,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
    bad_rows: BadRowPolicy,
}

impl Config {
    /// Start building a request for `symbol`.
    ///
    /// The market defaults to [`Market::Um`]; call [`ConfigBuilder::market`] to
    /// change it.
    #[must_use]
    pub fn builder(
        root: impl Into<PathBuf>,
        dataset: Dataset,
        symbol: impl AsRef<str>,
    ) -> ConfigBuilder {
        ConfigBuilder {
            root: root.into(),
            market: Market::Um,
            dataset,
            symbol: symbol.as_ref().to_owned(),
            interval: None,
            start_date: None,
            end_date: None,
            bad_rows: BadRowPolicy::Fail,
        }
    }

    /// Parse and validate a TOML document.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Config`] when the TOML is invalid, a field is unknown,
    /// the dataset is not published for the market, an interval is missing or
    /// forbidden, or the date range is reversed.
    pub fn from_toml_str(raw: &str) -> Result<Self> {
        let parsed: RawConfig =
            toml::from_str(raw).map_err(|e| Error::Config(format!("TOML: {e}")))?;
        let mut builder = Self::builder(parsed.root, parsed.dataset, parsed.symbol.as_str())
            .market(parsed.market.unwrap_or_default())
            .bad_rows(parsed.bad_rows.unwrap_or_default());
        if let Some(interval) = parsed.interval {
            builder = builder.interval(interval);
        }
        if parsed.start_date.is_some() || parsed.end_date.is_some() {
            builder = builder.date_range(parsed.start_date, parsed.end_date);
        }
        builder.build()
    }

    /// Read, parse and validate a TOML file.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] when the file cannot be read and the same errors
    /// as [`Config::from_toml_str`] otherwise.
    pub fn from_toml_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let raw = std::fs::read_to_string(path)
            .map_err(|e| Error::io(format!("read config {}", path.display()), e))?;
        Self::from_toml_str(&raw).map_err(|e| match e {
            Error::Config(message) => Error::Config(format!("{}: {message}", path.display())),
            other => other,
        })
    }

    /// Archive root, i.e. the downloader's `output_dir`.
    ///
    /// The reader appends `data/{market}/{period}/...`; do not include the
    /// `data` segment here.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Market to read.
    #[must_use]
    pub const fn market(&self) -> Market {
        self.market
    }

    /// Dataset family to read.
    #[must_use]
    pub const fn dataset(&self) -> Dataset {
        self.dataset
    }

    /// Symbol to read, exactly as the archive spells it.
    #[must_use]
    pub fn symbol(&self) -> &Symbol {
        &self.symbol
    }

    /// Kline interval; present exactly when [`Dataset::needs_interval`].
    #[must_use]
    pub const fn interval(&self) -> Option<Interval> {
        self.interval
    }

    /// Inclusive UTC start date.
    #[must_use]
    pub const fn start_date(&self) -> Option<NaiveDate> {
        self.start_date
    }

    /// Inclusive UTC end date.
    #[must_use]
    pub const fn end_date(&self) -> Option<NaiveDate> {
        self.end_date
    }

    /// Row-level damage policy.
    #[must_use]
    pub const fn bad_rows(&self) -> BadRowPolicy {
        self.bad_rows
    }
}

/// Builder for [`Config`].
///
/// [`ConfigBuilder::build`] performs every cross-field check, so intermediate
/// states are intentionally unvalidated.
#[derive(Debug, Clone)]
pub struct ConfigBuilder {
    root: PathBuf,
    market: Market,
    dataset: Dataset,
    symbol: String,
    interval: Option<Interval>,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
    bad_rows: BadRowPolicy,
}

impl ConfigBuilder {
    /// Set the market (default [`Market::Um`]).
    #[must_use]
    pub fn market(mut self, market: Market) -> Self {
        self.market = market;
        self
    }

    /// Set the kline interval.
    ///
    /// Required for kline-shaped datasets and forbidden for the others.
    #[must_use]
    pub fn interval(mut self, interval: Interval) -> Self {
        self.interval = Some(interval);
        self
    }

    /// Set an inclusive UTC date range; `None` means unbounded on that side.
    #[must_use]
    pub fn date_range(mut self, start: Option<NaiveDate>, end: Option<NaiveDate>) -> Self {
        self.start_date = start;
        self.end_date = end;
        self
    }

    /// Set the row-level damage policy (default [`BadRowPolicy::Fail`]).
    #[must_use]
    pub fn bad_rows(mut self, policy: BadRowPolicy) -> Self {
        self.bad_rows = policy;
        self
    }

    /// Validate the request and produce a [`Config`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::Config`] with a specific reason when a field is
    /// invalid or the dataset/interval is not published for the market.
    pub fn build(self) -> Result<Config> {
        let root = self.root;
        if root.as_os_str().is_empty() {
            return Err(Error::Config("root must not be empty".to_owned()));
        }
        let symbol =
            Symbol::parse(&self.symbol).map_err(|e| Error::Config(format!("symbol: {e}")))?;
        let dataset = self.dataset;
        let market = self.market;

        if !dataset.any_period_supported(market) {
            return Err(Error::Config(format!(
                "dataset {dataset} is not published for {market}; supported markets: {}",
                supported_markets(dataset)
            )));
        }

        let interval = match (dataset.needs_interval(), self.interval) {
            (true, None) => {
                return Err(Error::Config(format!(
                    "dataset {dataset} requires an interval"
                )));
            }
            (false, Some(interval)) => {
                return Err(Error::Config(format!(
                    "dataset {dataset} does not use an interval, got {interval}"
                )));
            }
            (true, Some(interval)) if !interval.available_any(market) => {
                return Err(Error::Config(format!(
                    "interval {interval} is not published for {market}"
                )));
            }
            (_, interval) => interval,
        };

        if let (Some(start), Some(end)) = (self.start_date, self.end_date)
            && start > end
        {
            return Err(Error::Config(format!(
                "start_date {start} is after end_date {end}"
            )));
        }

        Ok(Config {
            root,
            market,
            dataset,
            symbol,
            interval,
            start_date: self.start_date,
            end_date: self.end_date,
            bad_rows: self.bad_rows,
        })
    }
}

fn supported_markets(dataset: Dataset) -> String {
    let markets: Vec<&str> = Market::ALL
        .into_iter()
        .filter(|market| dataset.any_period_supported(*market))
        .map(Market::as_path)
        .collect();
    markets.join(", ")
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    root: PathBuf,
    dataset: Dataset,
    #[serde(default)]
    market: Option<Market>,
    symbol: Symbol,
    #[serde(default)]
    interval: Option<Interval>,
    #[serde(default)]
    start_date: Option<NaiveDate>,
    #[serde(default)]
    end_date: Option<NaiveDate>,
    #[serde(default)]
    bad_rows: Option<BadRowPolicy>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(raw: &str) -> Result<Config> {
        Config::from_toml_str(raw)
    }

    #[test]
    fn toml_defaults_to_um_and_fail() {
        let config = parse(
            r#"
            root = "/data/data/binance"
            dataset = "trades"
            symbol = "BTCUSDT"
            start_date = "2024-01-01"
            end_date = "2024-01-31"
            "#,
        )
        .unwrap();
        assert_eq!(config.market(), Market::Um);
        assert_eq!(config.bad_rows(), BadRowPolicy::Fail);
        assert_eq!(config.dataset(), Dataset::Trades);
        assert_eq!(config.interval(), None);
    }

    #[test]
    fn klines_require_an_interval() {
        let error = parse(
            r#"
            root = "/data"
            dataset = "klines"
            symbol = "BTCUSDT"
            "#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("requires an interval"));
    }

    #[test]
    fn non_interval_dataset_rejects_interval() {
        let error = parse(
            r#"
            root = "/data"
            dataset = "metrics"
            symbol = "BTCUSDT"
            interval = "1m"
            "#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("does not use an interval"));
    }

    #[test]
    fn unsupported_market_dataset_pairs_fail() {
        let error = parse(
            r#"
            root = "/data"
            dataset = "fundingRate"
            market = "spot"
            symbol = "BTCUSDT"
            "#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("not published"));
    }

    #[test]
    fn unsupported_interval_fails() {
        let error = parse(
            r#"
            root = "/data"
            dataset = "klines"
            market = "spot"
            symbol = "BTCUSDT"
            interval = "3d"
            start_date = "2024-01-01"
            "#,
        );
        // 3d exists in spot monthly archives, so this is valid.
        assert!(error.is_ok());

        let error = parse(
            r#"
            root = "/data"
            dataset = "klines"
            market = "futures/um"
            symbol = "BTCUSDT"
            interval = "1s"
            "#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("not published"));
    }

    #[test]
    fn reversed_dates_fail() {
        let error = parse(
            r#"
            root = "/data"
            dataset = "trades"
            symbol = "BTCUSDT"
            start_date = "2024-02-01"
            end_date = "2024-01-01"
            "#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("after end_date"));
    }

    #[test]
    fn unknown_fields_and_bad_dates_fail() {
        assert!(
            parse(
                r#"
                root = "/data"
                dataset = "trades"
                symbol = "BTCUSDT"
                typo = true
                "#,
            )
            .is_err()
        );
        assert!(
            parse(
                r#"
                root = "/data"
                dataset = "trades"
                symbol = "BTCUSDT"
                start_date = "2024-13-01"
                "#,
            )
            .is_err()
        );
    }

    #[test]
    fn builder_validates_symbol_and_interval() {
        assert!(
            Config::builder("/data", Dataset::Trades, "btcusdt")
                .build()
                .is_err()
        );
        assert!(
            Config::builder("/data", Dataset::Klines, "BTCUSDT")
                .build()
                .is_err()
        );
        assert!(
            Config::builder("/data", Dataset::Klines, "BTCUSDT")
                .interval(Interval::M1)
                .build()
                .is_ok()
        );
    }
}

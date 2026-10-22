//! Configuration for one symbol × one dataset.
//!
//! TOML is first deserialized into an unvalidated intermediate form, then
//! validated exactly once at the [`DataConfig`] boundary. The programmatic
//! constructors apply the same rules.

use std::fmt;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use serde::Deserialize;

use crate::error::ReaderError;
use crate::types::{Dataset, Interval, Market, Symbol};

/// TOML intermediate representation: strings only, no validation.
///
/// Required fields (`dataset`/`dir`/`symbol`) fail deserialization when
/// missing. `dataset` also accepts the legacy `type` key, matching the
/// downloader config that spells the dataset family as `type`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDataConfig {
    #[serde(alias = "type")]
    dataset: Dataset,
    /// Optional Binance market; defaults to `futures/um`.
    #[serde(default)]
    market: Option<Market>,
    dir: PathBuf,
    symbol: String,
    /// Required by kline-shaped datasets, rejected by every other dataset.
    interval: Option<String>,
    start_date: Option<String>,
    end_date: Option<String>,
}

fn parse_opt_date(label: &str, v: Option<&str>) -> Result<Option<NaiveDate>, ReaderError> {
    match v {
        None => Ok(None),
        Some(s) => NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map(Some)
            .map_err(|_| ReaderError::Config(format!("invalid {label} (want YYYY-MM-DD): {s}"))),
    }
}

/// Single-symbol × single-dataset load configuration; construction validates.
///
/// - `dataset/market/symbol/interval` are validated against the archive
///   support matrix;
/// - `start_date/end_date` are parsed dates and satisfy `start <= end`;
/// - TOML field names are stable; validation happens once at the
///   deserialization boundary via `TryFrom<RawDataConfig>`.
#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "RawDataConfig")]
pub struct DataConfig {
    dataset: Dataset,
    market: Market,
    dir: PathBuf,
    symbol: Symbol,
    interval: Option<Interval>,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
}

impl DataConfig {
    /// Dataset family, e.g. [`Dataset::AggTrades`].
    #[must_use]
    pub fn dataset(&self) -> Dataset {
        self.dataset
    }

    /// Binance market path.
    #[must_use]
    pub fn market(&self) -> Market {
        self.market
    }

    /// Data root; the reader appends `binance/data/{market}/...`.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Borrow the validated symbol.
    #[must_use]
    pub fn symbol(&self) -> &Symbol {
        &self.symbol
    }

    /// Borrow the validated interval, present only for kline-shaped datasets.
    #[must_use]
    pub fn interval(&self) -> Option<&Interval> {
        self.interval.as_ref()
    }

    /// Inclusive lower date bound (UTC).
    #[must_use]
    pub fn start_date(&self) -> Option<NaiveDate> {
        self.start_date
    }

    /// Inclusive upper date bound (UTC); covers the whole day.
    #[must_use]
    pub fn end_date(&self) -> Option<NaiveDate> {
        self.end_date
    }
}

impl DataConfig {
    /// Programmatic constructor with the default `futures/um` market.
    ///
    /// `interval` is required by index/mark/premium klines and must be `None`
    /// for every other dataset.
    ///
    /// # Errors
    ///
    /// Returns [`ReaderError::Config`] when the dataset/market pair is
    /// unsupported, the symbol or interval is invalid, an interval is missing
    /// or forbidden, or the date range is malformed or reversed.
    ///
    /// # Examples
    ///
    /// ```
    /// use binance_reader::{DataConfig, Dataset};
    ///
    /// let cfg = DataConfig::new(
    ///     Dataset::AggTrades,
    ///     "/data",
    ///     "BTCUSDT",
    ///     None,
    ///     Some("2024-01-01"),
    ///     Some("2024-01-31"),
    /// )?;
    /// assert_eq!(cfg.dataset(), Dataset::AggTrades);
    /// # Ok::<(), binance_reader::ReaderError>(())
    /// ```
    pub fn new(
        dataset: Dataset,
        dir: impl AsRef<Path>,
        symbol: &str,
        interval: Option<&str>,
        start_date: Option<&str>,
        end_date: Option<&str>,
    ) -> Result<Self, ReaderError> {
        Self::build(
            dataset,
            Market::Um,
            dir,
            symbol,
            interval,
            start_date,
            end_date,
        )
    }

    /// Same as [`Self::new`], with an explicit market.
    ///
    /// # Errors
    ///
    /// Same validation rules as [`Self::new`], including the dataset/market
    /// support matrix check.
    pub fn new_with_market(
        dataset: Dataset,
        market: Market,
        dir: impl AsRef<Path>,
        symbol: &str,
        interval: Option<&str>,
        start_date: Option<&str>,
        end_date: Option<&str>,
    ) -> Result<Self, ReaderError> {
        Self::build(dataset, market, dir, symbol, interval, start_date, end_date)
    }

    fn build(
        dataset: Dataset,
        market: Market,
        dir: impl AsRef<Path>,
        symbol: &str,
        interval: Option<&str>,
        start_date: Option<&str>,
        end_date: Option<&str>,
    ) -> Result<Self, ReaderError> {
        if !dataset.supports(market) {
            return Err(ReaderError::Config(format!(
                "dataset {} is not available for market {market}",
                dataset.as_str()
            )));
        }
        let symbol = Symbol::parse(symbol)?;
        let interval = match (dataset.needs_interval(), interval) {
            (true, Some(raw)) => Some(Interval::parse(market, raw)?),
            (true, None) => {
                return Err(ReaderError::Config(format!(
                    "dataset {} requires interval",
                    dataset.as_str()
                )));
            }
            (false, Some(_)) => {
                return Err(ReaderError::Config(format!(
                    "dataset {} does not take interval",
                    dataset.as_str()
                )));
            }
            (false, None) => None,
        };
        let start = parse_opt_date("start_date", start_date)?;
        let end = parse_opt_date("end_date", end_date)?;
        if let (Some(s), Some(e)) = (start, end)
            && s > e
        {
            return Err(ReaderError::Config(format!(
                "start_date {s} is after end_date {e}"
            )));
        }
        Ok(Self {
            dataset,
            market,
            dir: dir.as_ref().to_path_buf(),
            symbol,
            interval,
            start_date: start,
            end_date: end,
        })
    }
}

impl TryFrom<RawDataConfig> for DataConfig {
    type Error = ReaderError;

    fn try_from(raw: RawDataConfig) -> Result<Self, Self::Error> {
        Self::new_with_market(
            raw.dataset,
            raw.market.unwrap_or_default(),
            &raw.dir,
            &raw.symbol,
            raw.interval.as_deref(),
            raw.start_date.as_deref(),
            raw.end_date.as_deref(),
        )
    }
}

impl fmt::Display for DataConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} {} {}",
            self.dataset.as_str(),
            self.market,
            self.symbol,
            self.interval.as_ref().map_or("*", Interval::as_str)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_roundtrip_defaults() {
        let toml_str = r#"
dataset = "aggTrades"
dir = "/data"
symbol = "BTCUSDT"
"#;
        let cfg: DataConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.dataset(), Dataset::AggTrades);
        assert_eq!(cfg.market(), Market::Um);
        assert_eq!(cfg.dir(), Path::new("/data"));
        assert_eq!(cfg.symbol().as_str(), "BTCUSDT");
        assert!(cfg.interval().is_none());
        assert!(cfg.start_date().is_none());
    }

    #[test]
    fn toml_accepts_type_alias() {
        let toml_str = r#"
type = "metrics"
market = "futures/um"
dir = "/data"
symbol = "BTCUSDT"
start_date = "2024-01-01"
end_date = "2024-01-31"
"#;
        let cfg: DataConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.dataset(), Dataset::Metrics);
        assert_eq!(
            cfg.start_date(),
            Some(NaiveDate::from_ymd_opt(2024, 1, 1).unwrap())
        );
        assert_eq!(
            cfg.end_date(),
            Some(NaiveDate::from_ymd_opt(2024, 1, 31).unwrap())
        );
    }

    #[test]
    fn toml_kline_shaped_requires_interval() {
        let missing = r#"
dataset = "indexPriceKlines"
dir = "/data"
symbol = "BTCUSDT"
"#;
        let err = toml::from_str::<DataConfig>(missing)
            .unwrap_err()
            .to_string();
        assert!(err.contains("requires interval"), "unexpected: {err}");

        let ok = r#"
dataset = "indexPriceKlines"
dir = "/data"
symbol = "BTCUSDT"
interval = "4h"
"#;
        let cfg: DataConfig = toml::from_str(ok).unwrap();
        assert_eq!(cfg.interval().unwrap().as_str(), "4h");
    }

    #[test]
    fn toml_non_kline_rejects_interval() {
        let toml_str = r#"
dataset = "metrics"
dir = "/data"
symbol = "BTCUSDT"
interval = "1m"
"#;
        let err = toml::from_str::<DataConfig>(toml_str)
            .unwrap_err()
            .to_string();
        assert!(err.contains("does not take interval"), "unexpected: {err}");
    }

    #[test]
    fn toml_rejects_unsupported_market_pair() {
        let toml_str = r#"
dataset = "metrics"
market = "spot"
dir = "/data"
symbol = "BTCUSDT"
"#;
        let err = toml::from_str::<DataConfig>(toml_str)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("not available for market"),
            "unexpected: {err}"
        );
    }

    #[test]
    fn toml_rejects_missing_required_fields() {
        let toml_str = r#"
dir = "/data"
symbol = "BTCUSDT"
"#;
        let err = toml::from_str::<DataConfig>(toml_str)
            .unwrap_err()
            .to_string();
        assert!(err.contains("dataset"), "unexpected: {err}");
    }

    #[test]
    fn toml_rejects_unknown_fields() {
        let toml_str = r#"
dataset = "metrics"
dir = "/data"
symbol = "BTCUSDT"
timeframe = "1m"
"#;
        assert!(toml::from_str::<DataConfig>(toml_str).is_err());
    }

    #[test]
    fn toml_market_aliases() {
        for raw in ["spot", "futures/um", "um", "futures/cm", "cm"] {
            let market: Market = raw.parse().unwrap();
            assert_eq!(market.to_string(), market.as_path());
        }
        assert!("nope".parse::<Market>().is_err());
    }

    #[test]
    fn date_order_is_validated() {
        let cfg = DataConfig::new(
            Dataset::Trades,
            "/data",
            "BTCUSDT",
            None,
            Some("2024-02-01"),
            Some("2024-01-01"),
        );
        assert!(cfg.is_err());
    }

    #[test]
    fn constructor_rejects_symbol_path_traversal() {
        let err = DataConfig::new(Dataset::Trades, "/data", "../../etc", None, None, None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("invalid symbol"), "unexpected: {err}");
    }

    #[test]
    fn constructor_rejects_bad_interval() {
        assert!(
            DataConfig::new(
                Dataset::MarkPriceKlines,
                "/data",
                "BTCUSDT",
                Some("2d"),
                None,
                None
            )
            .is_err()
        );
    }

    #[test]
    fn cm_liquidation_is_accepted() {
        let cfg = DataConfig::new_with_market(
            Dataset::LiquidationSnapshot,
            Market::Cm,
            "/data",
            "BTCUSD_PERP",
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(cfg.market(), Market::Cm);
    }
}

//! Domain enums and newtypes: market, dataset family, interval, symbol and
//! kline timeframe.

use std::fmt;
use std::str::FromStr;

use serde::Deserialize;

use crate::error::ReaderError;

/// Binance market path segment below `<root>/binance/data`.
///
/// The downloader mirrors the archive layout, so the same value is used on
/// disk: `spot`, `futures/um`, `futures/cm`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Market {
    /// Spot market: `binance/data/spot/...`.
    Spot,
    /// USDⓈ-M futures: `binance/data/futures/um/...`.
    #[default]
    Um,
    /// COIN-M futures: `binance/data/futures/cm/...`.
    Cm,
}

impl Market {
    /// Canonical path segment, e.g. `futures/um`.
    #[must_use]
    pub const fn as_path(self) -> &'static str {
        match self {
            Self::Spot => "spot",
            Self::Um => "futures/um",
            Self::Cm => "futures/cm",
        }
    }
}

impl fmt::Display for Market {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_path())
    }
}

impl FromStr for Market {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            // Canonical archive paths, plus short aliases.
            "spot" => Ok(Self::Spot),
            "futures/um" | "um" => Ok(Self::Um),
            "futures/cm" | "cm" => Ok(Self::Cm),
            other => Err(format!(
                "unsupported market: {other}, valid values: spot, futures/um, futures/cm"
            )),
        }
    }
}

impl<'de> Deserialize<'de> for Market {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

/// One Binance public-archive dataset family.
///
/// `klines` itself is intentionally absent: it is loaded by `kline-reader`.
/// The kline-shaped index/mark/premium datasets live here because their
/// archive layout and column shape match klines, not because they are klines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dataset {
    /// Aggregate trades (`aggTrades`).
    AggTrades,
    /// Individual trades (`trades`).
    Trades,
    /// Best bid/ask ticker (`bookTicker`).
    BookTicker,
    /// Order-book depth snapshots (`bookDepth`).
    BookDepth,
    /// Funding rates (`fundingRate`).
    FundingRate,
    /// Liquidation snapshots (`liquidationSnapshot`).
    LiquidationSnapshot,
    /// Futures metrics (`metrics`).
    Metrics,
    /// Index-price klines (`indexPriceKlines`).
    IndexPriceKlines,
    /// Mark-price klines (`markPriceKlines`).
    MarkPriceKlines,
    /// Premium-index klines (`premiumIndexKlines`).
    PremiumIndexKlines,
}

impl Dataset {
    /// All supported datasets, in archive-name order.
    pub const ALL: [Self; 10] = [
        Self::AggTrades,
        Self::Trades,
        Self::BookTicker,
        Self::BookDepth,
        Self::FundingRate,
        Self::LiquidationSnapshot,
        Self::Metrics,
        Self::IndexPriceKlines,
        Self::MarkPriceKlines,
        Self::PremiumIndexKlines,
    ];

    /// Archive directory name, exactly as the web page spells it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AggTrades => "aggTrades",
            Self::Trades => "trades",
            Self::BookTicker => "bookTicker",
            Self::BookDepth => "bookDepth",
            Self::FundingRate => "fundingRate",
            Self::LiquidationSnapshot => "liquidationSnapshot",
            Self::Metrics => "metrics",
            Self::IndexPriceKlines => "indexPriceKlines",
            Self::MarkPriceKlines => "markPriceKlines",
            Self::PremiumIndexKlines => "premiumIndexKlines",
        }
    }

    /// Whether the archive stores files below a `{interval}` directory.
    #[must_use]
    pub const fn needs_interval(self) -> bool {
        matches!(
            self,
            Self::IndexPriceKlines | Self::MarkPriceKlines | Self::PremiumIndexKlines
        )
    }

    /// Whether prices may be zero or negative.
    ///
    /// Premium-index values are rates around zero and are routinely negative;
    /// every other dataset carries positive prices.
    #[must_use]
    pub const fn allows_non_positive_price(self) -> bool {
        matches!(self, Self::PremiumIndexKlines)
    }

    /// Whether the archive publishes this dataset for `market`.
    #[must_use]
    pub const fn supports(self, market: Market) -> bool {
        match self {
            Self::AggTrades | Self::Trades => true,
            Self::LiquidationSnapshot => matches!(market, Market::Cm),
            Self::BookTicker
            | Self::BookDepth
            | Self::FundingRate
            | Self::Metrics
            | Self::IndexPriceKlines
            | Self::MarkPriceKlines
            | Self::PremiumIndexKlines => matches!(market, Market::Um | Market::Cm),
        }
    }

    /// Directory/file symbol for this dataset.
    ///
    /// `indexPriceKlines` is archived under the **underlying index** symbol,
    /// not the contract: `BTCUSD_PERP` lives below `BTCUSD`, and
    /// `BTCUSDT_250926` below `BTCUSDT`. Every other dataset uses the
    /// configured symbol verbatim.
    #[must_use]
    pub fn storage_symbol(self, symbol: &Symbol) -> &str {
        if matches!(self, Self::IndexPriceKlines) {
            symbol
                .as_str()
                .split_once('_')
                .map_or(symbol.as_str(), |(underlying, _)| underlying)
        } else {
            symbol.as_str()
        }
    }
}

impl fmt::Display for Dataset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Dataset {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|dataset| dataset.as_str() == s)
            .ok_or_else(|| {
                let valid: Vec<&str> = Self::ALL.iter().map(|d| d.as_str()).collect();
                format!("unsupported dataset: {s}, valid values: {valid:?}")
            })
    }
}

impl<'de> Deserialize<'de> for Dataset {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

/// Intervals accepted by the kline-shaped datasets.
const SPOT_INTERVALS: &[&str] = &[
    "1s", "1m", "3m", "5m", "15m", "30m", "1h", "2h", "4h", "6h", "8h", "12h", "1d", "3d", "1w",
    "1mo",
];
const FUTURES_INTERVALS: &[&str] = &[
    "1m", "3m", "5m", "15m", "30m", "1h", "2h", "4h", "6h", "8h", "12h", "1d", "3d", "1w", "1mo",
];

/// Validated kline interval, matching the archive directory name.
///
/// Only used by [`Dataset::needs_interval`] datasets. `3d`, `1w` and `1mo`
/// exist in monthly archives only; the reader scans both cadences, so no
/// further restriction is needed here.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Interval(String);

impl Interval {
    /// Parse and validate an interval for `market`.
    ///
    /// # Errors
    ///
    /// Returns [`ReaderError::Config`] when the trimmed value is not in the
    /// market's interval allowlist.
    pub fn parse(market: Market, s: impl AsRef<str>) -> Result<Self, ReaderError> {
        let s = s.as_ref().trim();
        let valid = match market {
            Market::Spot => SPOT_INTERVALS,
            Market::Um | Market::Cm => FUTURES_INTERVALS,
        };
        if !valid.contains(&s) {
            return Err(ReaderError::Config(format!(
                "unsupported {market} interval: {s}, valid values: {valid:?}"
            )));
        }
        Ok(Self(s.to_owned()))
    }

    /// Borrow the raw interval string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Interval {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for Interval {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Validated Binance symbol.
///
/// Symbols are ASCII letters, digits and `_` (e.g. `BTCUSDT`, `BTCUSD_PERP`,
/// `BTCUSDT_250926`). The first character must be alphanumeric so the value is
/// always safe as a single path component and [`Dataset::storage_symbol`]
/// never yields an empty prefix. Keeping this a newtype means downstream
/// functions never re-check the character set (`parse-don't-validate`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Symbol(String);

impl Symbol {
    /// Parse and validate a symbol.
    ///
    /// # Errors
    ///
    /// Returns [`ReaderError::Config`] when the trimmed value is empty, has a
    /// non-alphanumeric first character, or contains anything other than
    /// ASCII letters, digits and `_`.
    pub fn parse(s: impl AsRef<str>) -> Result<Self, ReaderError> {
        let trimmed = s.as_ref().trim();
        if trimmed.is_empty() {
            return Err(ReaderError::Config("symbol must not be empty".into()));
        }
        let mut chars = trimmed.chars();
        let first_is_alphanumeric = chars.next().is_some_and(|c| c.is_ascii_alphanumeric());
        if !first_is_alphanumeric || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(ReaderError::Config(format!(
                "invalid symbol {trimmed:?}: use ASCII letters, digits and '_', starting with a letter or digit"
            )));
        }
        Ok(Self(trimmed.to_owned()))
    }

    /// Borrow the inner symbol string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for Symbol {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Single source of truth: timeframe → expected bar interval in milliseconds.
/// Both config validation and the gap check read this table.
const TIMEFRAME_INTERVALS_MS: &[(&str, i64)] = &[
    ("1m", 60_000),
    ("3m", 180_000),
    ("5m", 300_000),
    ("15m", 900_000),
    ("30m", 1_800_000),
    ("1h", 3_600_000),
    ("2h", 7_200_000),
    ("4h", 14_400_000),
    ("6h", 21_600_000),
    ("8h", 28_800_000),
    ("12h", 43_200_000),
    ("1d", 86_400_000),
];

/// Binance kline archive timeframes accepted by [`Timeframe`].
const BINANCE_TIMEFRAMES: &[&str] = &[
    "1m", "3m", "5m", "15m", "30m", "1h", "2h", "4h", "6h", "8h", "12h", "1d",
];

/// Interval lookup: unknown timeframes return `None`.
#[must_use]
pub fn interval_ms_for(timeframe: &str) -> Option<i64> {
    TIMEFRAME_INTERVALS_MS
        .iter()
        .find(|(tf, _)| *tf == timeframe)
        .map(|(_, ms)| *ms)
}

/// Whether `timeframe` is supported by Binance kline archives.
#[must_use]
pub fn is_supported(timeframe: &str) -> bool {
    BINANCE_TIMEFRAMES.contains(&timeframe)
}

/// List of supported Binance kline timeframes.
#[must_use]
pub fn supported_timeframes() -> &'static [&'static str] {
    BINANCE_TIMEFRAMES
}

/// Validated kline timeframe; construction binds the expected bar interval.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Timeframe {
    raw: String,
    interval_ms: i64,
}

impl Timeframe {
    /// Parse and validate a Binance kline timeframe.
    pub fn parse(s: impl AsRef<str>) -> Result<Self, ReaderError> {
        let s = s.as_ref().trim();
        if !is_supported(s) {
            return Err(ReaderError::Config(format!(
                "unsupported binance timeframe: {s}, valid values: {:?}",
                supported_timeframes()
            )));
        }
        let interval_ms = interval_ms_for(s).ok_or_else(|| {
            ReaderError::Config(format!("no interval defined for timeframe: {s}"))
        })?;
        Ok(Self {
            raw: s.to_owned(),
            interval_ms,
        })
    }

    /// Borrow the raw timeframe string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// Configured bar interval in milliseconds.
    #[must_use]
    pub const fn interval_ms(&self) -> i64 {
        self.interval_ms
    }
}

impl fmt::Display for Timeframe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

impl AsRef<str> for Timeframe {
    fn as_ref(&self) -> &str {
        &self.raw
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dataset_roundtrip_and_display() {
        for dataset in Dataset::ALL {
            assert_eq!(dataset.as_str().parse::<Dataset>().unwrap(), dataset);
        }
        assert!("Klines".parse::<Dataset>().is_err());
    }

    #[test]
    fn dataset_support_matrix() {
        assert!(Dataset::AggTrades.supports(Market::Spot));
        assert!(Dataset::Metrics.supports(Market::Um));
        assert!(!Dataset::Metrics.supports(Market::Spot));
        assert!(Dataset::LiquidationSnapshot.supports(Market::Cm));
        assert!(!Dataset::LiquidationSnapshot.supports(Market::Um));
    }

    #[test]
    fn index_symbol_is_underlying() {
        let perp = Symbol::parse("BTCUSD_PERP").unwrap();
        let quarterly = Symbol::parse("BTCUSDT_250926").unwrap();
        let plain = Symbol::parse("BTCUSDT").unwrap();
        assert_eq!(Dataset::IndexPriceKlines.storage_symbol(&perp), "BTCUSD");
        assert_eq!(
            Dataset::IndexPriceKlines.storage_symbol(&quarterly),
            "BTCUSDT"
        );
        assert_eq!(Dataset::IndexPriceKlines.storage_symbol(&plain), "BTCUSDT");
        assert_eq!(Dataset::Trades.storage_symbol(&perp), "BTCUSD_PERP");
    }

    #[test]
    fn interval_per_market() {
        assert!(Interval::parse(Market::Spot, "1s").is_ok());
        assert!(Interval::parse(Market::Um, "1s").is_err());
        assert!(Interval::parse(Market::Um, "1mo").is_ok());
        assert!(Interval::parse(Market::Cm, "4h").is_ok());
        assert!(Interval::parse(Market::Um, "2d").is_err());
    }

    #[test]
    fn symbol_rejects_empty() {
        assert!(Symbol::parse("").is_err());
        assert!(Symbol::parse("   ").is_err());
        assert_eq!(Symbol::parse(" BTCUSDT ").unwrap().as_str(), "BTCUSDT");
    }

    #[test]
    fn symbol_rejects_path_and_unsafe_characters() {
        for invalid in [
            "..",
            "../BTCUSDT",
            "BTC/USDT",
            "BTC\\USDT",
            "BTC USDT",
            "BTC.USDT",
            "_BTCUSDT",
            "-BTCUSDT",
            "BTCUSDT-",
            "Ω",
        ] {
            assert!(Symbol::parse(invalid).is_err(), "accepted {invalid:?}");
        }
    }

    #[test]
    fn symbol_accepts_archive_shapes() {
        for valid in [
            "BTCUSDT",
            "1000BONKUSDT",
            "BTCUSD_PERP",
            "BTCUSDT_250926",
            "btcusdt",
        ] {
            assert_eq!(Symbol::parse(valid).unwrap().as_str(), valid);
        }
    }

    #[test]
    fn timeframe_accepts_binance_whitelist() {
        assert!(Timeframe::parse("1m").is_ok());
        assert!(Timeframe::parse("1d").is_ok());
        assert_eq!(Timeframe::parse(" 4h ").unwrap().as_str(), "4h");
        assert!(Timeframe::parse("D1").is_err());
        assert!(Timeframe::parse("3d").is_err());
    }

    #[test]
    fn timeframe_binds_interval() {
        assert_eq!(Timeframe::parse("1m").unwrap().interval_ms(), 60_000);
        assert_eq!(Timeframe::parse("1d").unwrap().interval_ms(), 86_400_000);
        assert_eq!(interval_ms_for("1h"), Some(3_600_000));
        assert_eq!(interval_ms_for("D1"), None);
        assert!(supported_timeframes().contains(&"1d"));
    }
}

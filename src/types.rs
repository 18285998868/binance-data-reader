//! Archive vocabulary: market, cadence, dataset family, interval and symbol.
//!
//! Every value maps one-to-one onto a segment of the Binance public archive
//! path and, because the downloader mirrors that layout, onto the local tree
//! below [`Config::root`](crate::Config::root):
//!
//! ```text
//! {root}/data/{market}/{period}/{dataset}/{symbol}[/{interval}]/{file}.zip
//! ```
//!
//! There is deliberately no second layout.

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Datelike, Days, Months, NaiveDate};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Binance market, matching the archive path segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Market {
    /// Spot market: `data/spot`.
    Spot,
    /// USDⓈ-M futures: `data/futures/um`.
    #[default]
    Um,
    /// COIN-M futures: `data/futures/cm`.
    Cm,
}

impl Market {
    /// All supported markets.
    pub const ALL: [Self; 3] = [Self::Spot, Self::Um, Self::Cm];

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

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw {
            "spot" => Ok(Self::Spot),
            "futures/um" | "um" => Ok(Self::Um),
            "futures/cm" | "cm" => Ok(Self::Cm),
            other => Err(format!(
                "unsupported market: {other}, valid values: spot, futures/um, futures/cm"
            )),
        }
    }
}

impl Serialize for Market {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_path())
    }
}

impl<'de> Deserialize<'de> for Market {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

/// Archive cadence: one file per day or one file per month.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Period {
    /// One file per UTC day, named `YYYY-MM-DD`.
    Daily,
    /// One file per UTC month, named `YYYY-MM`.
    Monthly,
}

impl Period {
    /// Both cadences, in scan order.
    pub const ALL: [Self; 2] = [Self::Daily, Self::Monthly];

    /// Archive path segment, e.g. `daily`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Monthly => "monthly",
        }
    }

    /// Length of the date suffix in an archive file stem.
    #[must_use]
    pub const fn date_len(self) -> usize {
        match self {
            Self::Daily => 10,
            Self::Monthly => 7,
        }
    }

    /// First day of the period containing `date`.
    #[must_use]
    pub fn start_of(self, date: NaiveDate) -> NaiveDate {
        match self {
            Self::Daily => date,
            Self::Monthly => date.with_day(1).unwrap_or(date),
        }
    }

    /// First day after the period containing `date`.
    #[must_use]
    pub fn next_start(self, date: NaiveDate) -> Option<NaiveDate> {
        match self {
            Self::Daily => date.checked_add_days(Days::new(1)),
            Self::Monthly => date.checked_add_months(Months::new(1)),
        }
    }

    /// File-name date component, e.g. `2024-01-02` or `2024-01`.
    #[must_use]
    pub fn format(self, date: NaiveDate) -> String {
        match self {
            Self::Daily => date.format("%Y-%m-%d").to_string(),
            Self::Monthly => date.format("%Y-%m").to_string(),
        }
    }

    /// Parse the date suffix of an archive file stem.
    ///
    /// `BTCUSDT-1m-2024-01-02` (daily) and `BTCUSDT-1m-2024-01` (monthly)
    /// both end in a date component. Symbols and dataset names never contain
    /// `-`, so the suffix length identifies the cadence.
    #[must_use]
    pub fn parse_file_date(self, stem: &str) -> Option<Result<NaiveDate, String>> {
        let len = self.date_len();
        let start = stem.len().checked_sub(len)?;
        let candidate = &stem[start..];
        let shaped = match self {
            Self::Daily => {
                candidate.as_bytes().get(4) == Some(&b'-')
                    && candidate.as_bytes().get(7) == Some(&b'-')
            }
            Self::Monthly => candidate.as_bytes().get(4) == Some(&b'-'),
        };
        if !shaped {
            return None;
        }
        let parsed = match self {
            Self::Daily => NaiveDate::parse_from_str(candidate, "%Y-%m-%d"),
            // `%Y-%m` alone is ambiguous for chrono; pin the first day.
            Self::Monthly => NaiveDate::parse_from_str(&format!("{candidate}-01"), "%Y-%m-%d"),
        };
        Some(parsed.map_err(|e| format!("invalid date suffix {candidate:?}: {e}")))
    }
}

impl fmt::Display for Period {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Period {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|period| period.as_str() == raw)
            .ok_or_else(|| format!("unsupported period: {raw}, valid values: daily, monthly"))
    }
}

impl Serialize for Period {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Period {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

/// One archive dataset family, including regular klines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Dataset {
    /// Aggregate trades (`aggTrades`).
    AggTrades,
    /// Individual trades (`trades`).
    Trades,
    /// Klines (`klines`).
    Klines,
    /// Order-book depth snapshots (`bookDepth`).
    BookDepth,
    /// Best bid/ask ticker (`bookTicker`).
    BookTicker,
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
    pub const ALL: [Self; 11] = [
        Self::AggTrades,
        Self::Trades,
        Self::Klines,
        Self::BookDepth,
        Self::BookTicker,
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
            Self::Klines => "klines",
            Self::BookDepth => "bookDepth",
            Self::BookTicker => "bookTicker",
            Self::FundingRate => "fundingRate",
            Self::LiquidationSnapshot => "liquidationSnapshot",
            Self::Metrics => "metrics",
            Self::IndexPriceKlines => "indexPriceKlines",
            Self::MarkPriceKlines => "markPriceKlines",
            Self::PremiumIndexKlines => "premiumIndexKlines",
        }
    }

    /// Whether archive files live below an `{interval}` directory.
    #[must_use]
    pub const fn needs_interval(self) -> bool {
        matches!(
            self,
            Self::Klines
                | Self::IndexPriceKlines
                | Self::MarkPriceKlines
                | Self::PremiumIndexKlines
        )
    }

    /// Whether premium-index prices may be zero or negative.
    ///
    /// Premium-index values are rates around zero; every other dataset carries
    /// positive prices.
    #[must_use]
    pub const fn allows_non_positive_price(self) -> bool {
        matches!(self, Self::PremiumIndexKlines)
    }

    /// Whether the archive publishes this dataset for a market/cadence pair.
    ///
    /// This mirrors the downloader's support matrix exactly.
    #[must_use]
    pub const fn supports(self, market: Market, period: Period) -> bool {
        let market_ok = match self {
            Self::AggTrades | Self::Trades | Self::Klines => true,
            Self::BookDepth
            | Self::BookTicker
            | Self::FundingRate
            | Self::Metrics
            | Self::IndexPriceKlines
            | Self::MarkPriceKlines
            | Self::PremiumIndexKlines => matches!(market, Market::Um | Market::Cm),
            Self::LiquidationSnapshot => matches!(market, Market::Cm),
        };
        let period_ok = match self {
            // Daily-only families.
            Self::BookDepth | Self::Metrics | Self::LiquidationSnapshot => {
                matches!(period, Period::Daily)
            }
            // Monthly-only family.
            Self::FundingRate => matches!(period, Period::Monthly),
            _ => true,
        };
        market_ok && period_ok
    }

    /// Whether any cadence publishes this dataset for `market`.
    #[must_use]
    pub const fn any_period_supported(self, market: Market) -> bool {
        self.supports(market, Period::Daily) || self.supports(market, Period::Monthly)
    }

    /// Symbol used by the archive for this dataset.
    ///
    /// `indexPriceKlines` is keyed by the **underlying index**, not by the
    /// contract: `BTCUSD_PERP` lives under `BTCUSD`, while a dated contract
    /// such as `BTCUSDT_250926` lives under `BTCUSDT`. Every other dataset
    /// uses the configured symbol verbatim.
    #[must_use]
    pub fn storage_symbol(self, symbol: &str) -> &str {
        if matches!(self, Self::IndexPriceKlines) {
            underlying_index_symbol(symbol)
        } else {
            symbol
        }
    }
}

/// Strip the contract suffix (`_PERP`, `_250926`, ...) to obtain the index
/// symbol used by the archive's `indexPriceKlines` tree.
fn underlying_index_symbol(symbol: &str) -> &str {
    match symbol.rsplit_once('_') {
        Some((base, suffix))
            if suffix == "PERP"
                || (suffix.len() == 6 && suffix.bytes().all(|b| b.is_ascii_digit())) =>
        {
            base
        }
        _ => symbol,
    }
}

impl fmt::Display for Dataset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Dataset {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|dataset| dataset.as_str() == raw)
            .ok_or_else(|| {
                let valid: Vec<&str> = Self::ALL.iter().map(|d| d.as_str()).collect();
                format!("unsupported dataset: {raw}, valid values: {valid:?}")
            })
    }
}

impl Serialize for Dataset {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Dataset {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

/// Kline interval, matching archive directory names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Interval {
    /// 1-second klines (spot only).
    S1,
    /// 1-minute klines.
    M1,
    /// 3-minute klines.
    M3,
    /// 5-minute klines.
    M5,
    /// 15-minute klines.
    M15,
    /// 30-minute klines.
    M30,
    /// 1-hour klines.
    H1,
    /// 2-hour klines.
    H2,
    /// 4-hour klines.
    H4,
    /// 6-hour klines.
    H6,
    /// 8-hour klines.
    H8,
    /// 12-hour klines.
    H12,
    /// 1-day klines.
    D1,
    /// 3-day klines (monthly archives only).
    D3,
    /// 1-week klines (monthly archives only).
    W1,
    /// 1-month klines (monthly archives only).
    MO1,
}

impl Interval {
    /// All accepted intervals, ordered by duration.
    pub const ALL: [Self; 16] = [
        Self::S1,
        Self::M1,
        Self::M3,
        Self::M5,
        Self::M15,
        Self::M30,
        Self::H1,
        Self::H2,
        Self::H4,
        Self::H6,
        Self::H8,
        Self::H12,
        Self::D1,
        Self::D3,
        Self::W1,
        Self::MO1,
    ];

    /// Archive directory name, e.g. `1m`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::S1 => "1s",
            Self::M1 => "1m",
            Self::M3 => "3m",
            Self::M5 => "5m",
            Self::M15 => "15m",
            Self::M30 => "30m",
            Self::H1 => "1h",
            Self::H2 => "2h",
            Self::H4 => "4h",
            Self::H6 => "6h",
            Self::H8 => "8h",
            Self::H12 => "12h",
            Self::D1 => "1d",
            Self::D3 => "3d",
            Self::W1 => "1w",
            Self::MO1 => "1mo",
        }
    }

    /// Whether the archive publishes this interval for a market/cadence pair.
    #[must_use]
    pub const fn available_for(self, market: Market, period: Period) -> bool {
        match (market, period) {
            (Market::Spot, Period::Daily) => self as u8 <= Self::D1 as u8,
            (Market::Spot, Period::Monthly) => true,
            (_, Period::Daily) => self as u8 >= Self::M1 as u8 && self as u8 <= Self::D1 as u8,
            (_, Period::Monthly) => !matches!(self, Self::S1),
        }
    }

    /// Fixed duration in milliseconds, or `None` for calendar months.
    #[must_use]
    pub const fn step_ms(self) -> Option<i64> {
        let day = 86_400_000_i64;
        match self {
            Self::S1 => Some(1_000),
            Self::M1 => Some(60_000),
            Self::M3 => Some(3 * 60_000),
            Self::M5 => Some(5 * 60_000),
            Self::M15 => Some(15 * 60_000),
            Self::M30 => Some(30 * 60_000),
            Self::H1 => Some(60 * 60_000),
            Self::H2 => Some(2 * 60 * 60_000),
            Self::H4 => Some(4 * 60 * 60_000),
            Self::H6 => Some(6 * 60 * 60_000),
            Self::H8 => Some(8 * 60 * 60_000),
            Self::H12 => Some(12 * 60 * 60_000),
            Self::D1 => Some(day),
            Self::D3 => Some(3 * day),
            Self::W1 => Some(7 * day),
            Self::MO1 => None,
        }
    }

    /// Open time of the bar that follows the bar opened at `open_ms`.
    ///
    /// `1mo` advances by one calendar month so February, leap years and
    /// 28/31-day months all line up with the archive.
    #[must_use]
    pub fn next_open(self, open_ms: i64) -> Option<i64> {
        if let Some(step) = self.step_ms() {
            return open_ms.checked_add(step);
        }
        let datetime = DateTime::from_timestamp_millis(open_ms)?;
        let next = datetime.date_naive().checked_add_months(Months::new(1))?;
        Some(next.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis())
    }

    /// Whether the archive publishes this interval for at least one cadence of
    /// `market`.
    #[must_use]
    pub fn available_any(self, market: Market) -> bool {
        self.available_for(market, Period::Daily) || self.available_for(market, Period::Monthly)
    }
}

impl fmt::Display for Interval {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Interval {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|interval| interval.as_str() == raw)
            .ok_or_else(|| {
                let valid: Vec<&str> = Self::ALL.iter().map(|i| i.as_str()).collect();
                format!("unsupported interval: {raw}, valid values: {valid:?}")
            })
    }
}

impl Serialize for Interval {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Interval {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

/// Validated Binance symbol used as a directory and file-name component.
///
/// Symbols are uppercase ASCII letters, digits and `_`; `-` is forbidden
/// because it separates filename components. A symbol may start with a digit
/// (`1000BONKUSDT`) but must contain at least one letter.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Symbol(String);

impl Symbol {
    /// Validate and own a symbol.
    ///
    /// # Errors
    ///
    /// Returns a human-readable reason when the value is empty, longer than
    /// 64 bytes, contains anything other than `A-Z`, `0-9` and `_`, or has no
    /// letter at all.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, String> {
        let raw = raw.as_ref().trim();
        if raw.is_empty() {
            return Err("symbol must not be empty".to_owned());
        }
        if raw.len() > 64 {
            return Err(format!("symbol is too long: {} bytes", raw.len()));
        }
        if !raw
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err(format!(
                "symbol {raw:?} must contain only uppercase ASCII letters, digits and '_'"
            ));
        }
        if !raw.bytes().any(|b| b.is_ascii_uppercase()) {
            return Err(format!("symbol {raw:?} must contain at least one letter"));
        }
        Ok(Self(raw.to_owned()))
    }

    /// Borrow the symbol as a string slice.
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

impl FromStr for Symbol {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        Self::parse(raw)
    }
}

impl AsRef<str> for Symbol {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Serialize for Symbol {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Symbol {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// How row-level data damage is handled.
///
/// File-level damage (corrupt ZIP, missing CSV entry, malformed filename) is
/// always fatal. Row-level damage can be fatal or skipped; skipped rows are
/// counted and sampled in the [`LoadReport`](crate::LoadReport).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BadRowPolicy {
    /// Abort the load on the first malformed row.
    #[default]
    Fail,
    /// Drop malformed rows, keeping the rest of the dataset.
    Skip,
}

impl BadRowPolicy {
    /// Both policies, in documentation order.
    pub const ALL: [Self; 2] = [Self::Fail, Self::Skip];

    /// Lowercase policy name used in config files and reports.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fail => "fail",
            Self::Skip => "skip",
        }
    }
}

impl fmt::Display for BadRowPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for BadRowPolicy {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|policy| policy.as_str() == raw)
            .ok_or_else(|| format!("unsupported bad_rows policy: {raw}, valid values: fail, skip"))
    }
}

impl Serialize for BadRowPolicy {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for BadRowPolicy {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

/// UTC midnight of `date` in epoch milliseconds.
#[must_use]
pub(crate) fn day_start_ms(date: NaiveDate) -> i64 {
    date.and_hms_opt(0, 0, 0)
        .map(|dt| dt.and_utc().timestamp_millis())
        .unwrap_or(0)
}

/// UTC day index (`epoch_ms / 86_400_000`) for a non-negative timestamp.
#[must_use]
pub(crate) fn utc_day_index(timestamp_ms: i64) -> i64 {
    timestamp_ms.div_euclid(86_400_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    #[test]
    fn market_round_trips_and_accepts_aliases() {
        for market in Market::ALL {
            assert_eq!(market.as_path().parse::<Market>().unwrap(), market);
        }
        assert_eq!("um".parse::<Market>().unwrap(), Market::Um);
        assert_eq!("cm".parse::<Market>().unwrap(), Market::Cm);
        assert!("fapi".parse::<Market>().is_err());
    }

    #[test]
    fn period_month_bounds_are_calendar_based() {
        assert_eq!(
            Period::Monthly.start_of(date(2024, 2, 15)),
            date(2024, 2, 1)
        );
        assert_eq!(
            Period::Monthly.next_start(date(2024, 1, 1)),
            Some(date(2024, 2, 1))
        );
        assert_eq!(
            Period::Monthly.next_start(date(2024, 2, 1)),
            Some(date(2024, 3, 1))
        );
    }

    #[test]
    fn file_dates_parse_by_cadence() {
        assert_eq!(
            Period::Daily
                .parse_file_date("BTCUSDT-1m-2024-01-02")
                .unwrap()
                .unwrap(),
            date(2024, 1, 2)
        );
        assert_eq!(
            Period::Monthly
                .parse_file_date("BTCUSDT-fundingRate-2024-01")
                .unwrap()
                .unwrap(),
            date(2024, 1, 1)
        );
        assert!(Period::Daily.parse_file_date("BTCUSDT-1m").is_none());
        assert!(
            Period::Daily
                .parse_file_date("BTCUSDT-1m-2024-13-01")
                .unwrap()
                .is_err()
        );
    }

    #[test]
    fn interval_next_open_handles_calendar_months() {
        let jan = day_start_ms(date(2024, 1, 1));
        let feb = Interval::MO1.next_open(jan).unwrap();
        assert_eq!(feb, day_start_ms(date(2024, 2, 1)));
        assert_eq!(
            Interval::MO1.next_open(feb).unwrap(),
            day_start_ms(date(2024, 3, 1))
        );
        assert_eq!(
            Interval::M1.next_open(jan).unwrap(),
            jan + Interval::M1.step_ms().unwrap()
        );
    }

    #[test]
    fn interval_matrix_matches_downloader() {
        assert!(Interval::S1.available_for(Market::Spot, Period::Daily));
        assert!(Interval::S1.available_for(Market::Spot, Period::Monthly));
        assert!(!Interval::S1.available_for(Market::Um, Period::Daily));
        assert!(!Interval::S1.available_for(Market::Um, Period::Monthly));
        assert!(Interval::D3.available_for(Market::Um, Period::Monthly));
        assert!(!Interval::D3.available_for(Market::Um, Period::Daily));
        assert!(!Interval::D3.available_for(Market::Spot, Period::Daily));
        assert!(Interval::D3.available_for(Market::Spot, Period::Monthly));
    }

    #[test]
    fn dataset_support_matrix() {
        assert!(Dataset::Klines.supports(Market::Spot, Period::Daily));
        assert!(!Dataset::BookTicker.supports(Market::Spot, Period::Daily));
        assert!(!Dataset::BookDepth.supports(Market::Um, Period::Monthly));
        assert!(!Dataset::FundingRate.supports(Market::Um, Period::Daily));
        assert!(Dataset::FundingRate.supports(Market::Um, Period::Monthly));
        assert!(Dataset::LiquidationSnapshot.supports(Market::Cm, Period::Daily));
        assert!(!Dataset::LiquidationSnapshot.supports(Market::Um, Period::Daily));
    }

    #[test]
    fn index_storage_symbol_strips_contract_suffix() {
        assert_eq!(
            Dataset::IndexPriceKlines.storage_symbol("BTCUSD_PERP"),
            "BTCUSD"
        );
        assert_eq!(
            Dataset::IndexPriceKlines.storage_symbol("BTCUSDT_250926"),
            "BTCUSDT"
        );
        assert_eq!(Dataset::Klines.storage_symbol("BTCUSD_PERP"), "BTCUSD_PERP");
    }

    #[test]
    fn symbols_reject_path_and_case_errors() {
        assert!(Symbol::parse("BTCUSDT").is_ok());
        assert!(Symbol::parse("1000BONKUSDT").is_ok());
        assert!(Symbol::parse("BTCUSD_PERP").is_ok());
        assert!(Symbol::parse("btcusdt").is_err());
        assert!(Symbol::parse("BTC-USDT").is_err());
        assert!(Symbol::parse("../BTCUSDT").is_err());
        assert!(Symbol::parse("12345").is_err());
        assert!(Symbol::parse("").is_err());
    }

    #[test]
    fn bad_row_policy_round_trips() {
        for policy in BadRowPolicy::ALL {
            assert_eq!(policy.as_str().parse::<BadRowPolicy>().unwrap(), policy);
        }
    }

    #[test]
    fn day_index_is_floor_division_for_negative_values() {
        assert_eq!(utc_day_index(0), 0);
        assert_eq!(utc_day_index(86_399_999), 0);
        assert_eq!(utc_day_index(86_400_000), 1);
        assert_eq!(utc_day_index(-1), -1);
    }
}

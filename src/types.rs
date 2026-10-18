use std::fmt;

use crate::config::DataSource;
use crate::error::ReaderError;

/// 单一真相源：timeframe 字符串 → 期望 bar 间隔（毫秒）。
/// `config` 白名单与 `clean` 缺口检查都查同一张表，避免双源漂移。
const TIMEFRAME_INTERVALS_MS: &[(&str, i64)] = &[
    ("1m", 60_000),
    ("M1", 60_000),
    ("3m", 180_000),
    ("5m", 300_000),
    ("15m", 900_000),
    ("30m", 1_800_000),
    ("1h", 3_600_000),
    ("H1", 3_600_000),
    ("2h", 7_200_000),
    ("4h", 14_400_000),
    ("6h", 21_600_000),
    ("8h", 28_800_000),
    ("12h", 43_200_000),
    ("1d", 86_400_000),
    ("D1", 86_400_000),
    ("1w", 604_800_000),
];

/// 各源允许的 timeframe（与历史白名单一致），间隔查上表。
const BINANCE_TIMEFRAMES: &[&str] = &[
    "1m", "3m", "5m", "15m", "30m", "1h", "2h", "4h", "6h", "8h", "12h", "1d",
];
const DUKASCOPY_TIMEFRAMES: &[&str] = &["M1", "H1", "D1", "1m", "1h", "1d"];

/// 间隔查询：未知周期返回 `None`，调用方决定跳过还是报错。
#[must_use]
pub fn interval_ms_for(timeframe: &str) -> Option<i64> {
    TIMEFRAME_INTERVALS_MS
        .iter()
        .find(|(tf, _)| *tf == timeframe)
        .map(|(_, ms)| *ms)
}

/// Whether `timeframe` is supported for `source`.
#[must_use]
pub fn is_supported(source: DataSource, timeframe: &str) -> bool {
    match source {
        DataSource::Binance => BINANCE_TIMEFRAMES.contains(&timeframe),
        DataSource::Dukascopy => DUKASCOPY_TIMEFRAMES.contains(&timeframe),
    }
}

/// List of timeframes supported for `source`.
#[must_use]
pub fn supported_timeframes(source: DataSource) -> &'static [&'static str] {
    match source {
        DataSource::Binance => BINANCE_TIMEFRAMES,
        DataSource::Dukascopy => DUKASCOPY_TIMEFRAMES,
    }
}

/// 已验证的交易对符号：构造时保证非空（去首尾空白后）。
/// 后续函数只收 `&Symbol`，不再重复判空（`api-parse-dont-validate`）。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Symbol(String);

impl Symbol {
    /// Parse and validate a symbol (non-empty after trimming).
    pub fn parse(s: impl AsRef<str>) -> Result<Self, ReaderError> {
        let trimmed = s.as_ref().trim();
        if trimmed.is_empty() {
            return Err(ReaderError::Config("symbol must not be empty".into()));
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

/// 已验证的时间周期：构造时按 `DataSource` 白名单校验，并绑定间隔。
/// `interval_ms` 在构造时即确定，`clean` 不再二次查表失败。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Timeframe {
    raw: String,
    interval_ms: i64,
}

impl Timeframe {
    /// Parse and validate a timeframe for the given source.
    pub fn parse(source: DataSource, s: impl AsRef<str>) -> Result<Self, ReaderError> {
        let s = s.as_ref().trim();
        if !is_supported(source, s) {
            let name = match source {
                DataSource::Binance => "binance",
                DataSource::Dukascopy => "dukascopy",
            };
            return Err(ReaderError::Config(format!(
                "unsupported {name} timeframe: {s}, valid values: {:?}",
                supported_timeframes(source)
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
    fn symbol_rejects_empty() {
        assert!(Symbol::parse("").is_err());
        assert!(Symbol::parse("   ").is_err());
        assert_eq!(Symbol::parse(" BTCUSDT ").unwrap().as_str(), "BTCUSDT");
    }

    #[test]
    fn timeframe_per_source() {
        assert!(Timeframe::parse(DataSource::Binance, "1m").is_ok());
        assert!(Timeframe::parse(DataSource::Binance, "D1").is_err());
        assert!(Timeframe::parse(DataSource::Dukascopy, "D1").is_ok());
        assert!(Timeframe::parse(DataSource::Dukascopy, "3m").is_err());
    }

    #[test]
    fn interval_single_source() {
        assert_eq!(interval_ms_for("1m"), Some(60_000));
        assert_eq!(interval_ms_for("M1"), Some(60_000));
        assert_eq!(interval_ms_for("1d"), Some(86_400_000));
        assert_eq!(interval_ms_for("3d"), None);
    }

    #[test]
    fn supported_timeframes_per_source() {
        assert!(is_supported(DataSource::Binance, "1m"));
        assert!(!is_supported(DataSource::Binance, "D1"));
        assert!(is_supported(DataSource::Dukascopy, "D1"));
        assert!(!is_supported(DataSource::Dukascopy, "3m"));
        assert!(supported_timeframes(DataSource::Binance).contains(&"1d"));
        assert!(supported_timeframes(DataSource::Dukascopy).contains(&"M1"));
    }
}

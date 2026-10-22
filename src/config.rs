use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use chrono::NaiveDate;
use serde::{Deserialize, Deserializer};

use crate::error::ReaderError;
use crate::types::{Symbol, Timeframe};

/// Supported kline data sources.
#[derive(Debug, Clone, Copy, PartialEq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DataSource {
    #[default]
    Binance,
    Dukascopy,
}

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
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

/// TOML 反序列化用的原始形态：全 `String`，不做任何校验。
/// 必填字段（`type/dir/symbol/timeframe`）缺失即反序列化失败，
/// 不再静默 default 后报 confusing 的白名单错误；`Option` 日期缺省为 `None`。
/// 公开仅为 `TryFrom` 目标，业务代码请用 `DataConfig::new`。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDataConfig {
    #[serde(rename = "type")]
    source_type: DataSource,
    /// Optional Binance market; defaults to `futures/um`.
    #[serde(default)]
    market: Option<Market>,
    dir: PathBuf,
    symbol: String,
    timeframe: String,
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

/// 单品种 × 单时间帧的数据加载配置：构造即有效（`parse-don't-validate`）。
///
/// - `symbol/timeframe` 为已验证新类型，不存在空串或非法周期；
/// - `start_date/end_date` 为已解析日期，且满足 `start <= end`；
/// - TOML 仍用旧字段名，通过 `TryFrom<RawDataConfig>` 在反序列化边界一次解析。
#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "RawDataConfig")]
pub struct DataConfig {
    source_type: DataSource,
    market: Market,
    dir: PathBuf,
    symbol: Symbol,
    timeframe: Timeframe,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
}

impl DataConfig {
    pub fn source_type(&self) -> DataSource {
        self.source_type
    }

    /// Binance market path; ignored for [`DataSource::Dukascopy`].
    pub fn market(&self) -> Market {
        self.market
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn symbol(&self) -> &Symbol {
        &self.symbol
    }

    pub fn timeframe(&self) -> &Timeframe {
        &self.timeframe
    }

    pub fn start_date(&self) -> Option<NaiveDate> {
        self.start_date
    }

    pub fn end_date(&self) -> Option<NaiveDate> {
        self.end_date
    }
}

impl DataConfig {
    /// 编程构造入口：一次解析全部输入，非法即 `Err`（fail-fast）。
    ///
    /// Binance 数据默认使用 `futures/um`；需要其它市场时用
    /// [`Self::new_with_market`]。
    pub fn new(
        source_type: DataSource,
        dir: impl AsRef<Path>,
        symbol: &str,
        timeframe: &str,
        start_date: Option<&str>,
        end_date: Option<&str>,
    ) -> Result<Self, ReaderError> {
        Self::build(
            source_type,
            None,
            dir,
            symbol,
            timeframe,
            start_date,
            end_date,
        )
    }

    /// 与 [`Self::new`] 相同，但显式指定 Binance 市场路径。
    ///
    /// `market` 只对 [`DataSource::Binance`] 有效；传给 Dukascopy 会返回
    /// [`ReaderError::Config`]。
    pub fn new_with_market(
        source_type: DataSource,
        market: Market,
        dir: impl AsRef<Path>,
        symbol: &str,
        timeframe: &str,
        start_date: Option<&str>,
        end_date: Option<&str>,
    ) -> Result<Self, ReaderError> {
        Self::build(
            source_type,
            Some(market),
            dir,
            symbol,
            timeframe,
            start_date,
            end_date,
        )
    }

    fn build(
        source_type: DataSource,
        market: Option<Market>,
        dir: impl AsRef<Path>,
        symbol: &str,
        timeframe: &str,
        start_date: Option<&str>,
        end_date: Option<&str>,
    ) -> Result<Self, ReaderError> {
        if source_type == DataSource::Dukascopy && market.is_some() {
            return Err(ReaderError::Config(
                "market is only supported for binance".into(),
            ));
        }
        let symbol = Symbol::parse(symbol)?;
        let timeframe = Timeframe::parse(source_type, timeframe)?;
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
            source_type,
            market: market.unwrap_or_default(),
            dir: dir.as_ref().to_path_buf(),
            symbol,
            timeframe,
            start_date: start,
            end_date: end,
        })
    }
}

impl TryFrom<RawDataConfig> for DataConfig {
    type Error = ReaderError;

    fn try_from(raw: RawDataConfig) -> Result<Self, Self::Error> {
        match raw.market {
            Some(market) => Self::new_with_market(
                raw.source_type,
                market,
                &raw.dir,
                &raw.symbol,
                &raw.timeframe,
                raw.start_date.as_deref(),
                raw.end_date.as_deref(),
            ),
            None => Self::new(
                raw.source_type,
                &raw.dir,
                &raw.symbol,
                &raw.timeframe,
                raw.start_date.as_deref(),
                raw.end_date.as_deref(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_source_toml_deserialize_roundtrip() {
        let toml_str = r#"
type = "binance"
dir = "/data/klines"
symbol = "BTCUSDT"
timeframe = "1m"
"#;
        let cfg: DataConfig = toml::from_str(toml_str).unwrap();
        assert!(matches!(cfg.source_type, DataSource::Binance));
        assert_eq!(cfg.market(), Market::Um);
        assert_eq!(cfg.dir, PathBuf::from("/data/klines"));
        assert_eq!(cfg.symbol.as_str(), "BTCUSDT");
        assert_eq!(cfg.timeframe.as_str(), "1m");
    }

    #[test]
    fn data_source_toml_dukascopy_deserialize() {
        let toml_str = r#"
type = "dukascopy"
dir = "/data/dukascopy"
symbol = "EURUSD"
timeframe = "D1"
"#;
        let cfg: DataConfig = toml::from_str(toml_str).unwrap();
        assert!(matches!(cfg.source_type, DataSource::Dukascopy));
        assert_eq!(cfg.timeframe.interval_ms(), 86_400_000);
    }

    #[test]
    fn dir_roundtrip_as_pathbuf() {
        let toml_str = r#"
type = "binance"
dir = "/a/b c"
symbol = "X"
timeframe = "5m"
"#;
        let cfg: DataConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.dir, PathBuf::from("/a/b c"));
    }

    #[test]
    fn market_toml_parses_canonical_and_aliases() {
        let cases = [
            ("spot", Market::Spot),
            ("futures/um", Market::Um),
            ("um", Market::Um),
            ("futures/cm", Market::Cm),
            ("cm", Market::Cm),
        ];
        for (raw, expected) in cases {
            let toml_str = format!(
                r#"
type = "binance"
market = "{raw}"
dir = "/data"
symbol = "BTCUSDT"
timeframe = "1d"
"#
            );
            let cfg: DataConfig = toml::from_str(&toml_str).unwrap();
            assert_eq!(cfg.market(), expected);
        }
        assert_eq!(Market::Spot.as_path(), "spot");
        assert_eq!(Market::Um.as_path(), "futures/um");
        assert_eq!(Market::Cm.as_path(), "futures/cm");
    }

    #[test]
    fn market_toml_rejects_invalid() {
        let toml_str = r#"
type = "binance"
market = "nope"
dir = "/data"
symbol = "BTCUSDT"
timeframe = "1d"
"#;
        let err = toml::from_str::<DataConfig>(toml_str)
            .unwrap_err()
            .to_string();
        assert!(err.contains("unsupported market"), "unexpected: {err}");
    }

    #[test]
    fn market_only_valid_for_binance() {
        let toml_str = r#"
type = "dukascopy"
market = "spot"
dir = "/data"
symbol = "EURUSD"
timeframe = "D1"
"#;
        let err = toml::from_str::<DataConfig>(toml_str)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("market is only supported for binance"),
            "unexpected: {err}"
        );
    }

    #[test]
    fn new_with_market_sets_market_and_rejects_dukascopy() {
        let cfg = DataConfig::new_with_market(
            DataSource::Binance,
            Market::Spot,
            "/data",
            "BTCUSDT",
            "1d",
            None,
            None,
        )
        .unwrap();
        assert_eq!(cfg.market(), Market::Spot);

        assert!(
            DataConfig::new_with_market(
                DataSource::Dukascopy,
                Market::Spot,
                "/data",
                "EURUSD",
                "D1",
                None,
                None,
            )
            .is_err()
        );
    }

    fn valid_cfg() -> DataConfig {
        DataConfig::new(DataSource::Binance, "/data", "BTCUSDT", "1d", None, None).unwrap()
    }

    #[test]
    fn new_ok() {
        let cfg = valid_cfg();
        assert_eq!(cfg.symbol.as_str(), "BTCUSDT");
        assert_eq!(cfg.timeframe.as_str(), "1d");
    }

    #[test]
    fn new_rejects_empty_symbol() {
        assert!(DataConfig::new(DataSource::Binance, "/data", "", "1d", None, None).is_err());
        assert!(DataConfig::new(DataSource::Binance, "/data", "  ", "1d", None, None).is_err());
    }

    #[test]
    fn new_rejects_timeframe_per_source() {
        // "D1" is dukascopy-only, "3m" is binance-only.
        assert!(
            DataConfig::new(DataSource::Binance, "/data", "BTCUSDT", "D1", None, None).is_err()
        );
        assert!(
            DataConfig::new(DataSource::Dukascopy, "/data", "EURUSD", "D1", None, None).is_ok()
        );
        assert!(
            DataConfig::new(DataSource::Dukascopy, "/data", "EURUSD", "3m", None, None).is_err()
        );
    }

    #[test]
    fn new_rejects_bad_date_or_order() {
        assert!(
            DataConfig::new(
                DataSource::Binance,
                "/data",
                "BTCUSDT",
                "1d",
                Some("not-a-date"),
                None
            )
            .is_err()
        );
        assert!(
            DataConfig::new(
                DataSource::Binance,
                "/data",
                "BTCUSDT",
                "1d",
                Some("2025-01-02"),
                Some("2025-01-01")
            )
            .is_err()
        );
    }

    #[test]
    fn toml_rejects_invalid() {
        let bad = r#"
type = "binance"
dir = "/data"
symbol = ""
timeframe = "1d"
"#;
        assert!(toml::from_str::<DataConfig>(bad).is_err());
    }

    #[test]
    fn toml_rejects_missing_required_fields() {
        // 缺字段直接报缺字段，而非 default 后报白名单错误。
        let missing_tf = r#"
type = "binance"
dir = "/data"
symbol = "BTCUSDT"
"#;
        let err = toml::from_str::<DataConfig>(missing_tf)
            .unwrap_err()
            .to_string();
        assert!(err.contains("timeframe"), "unexpected: {err}");

        let missing_type = r#"
dir = "/data"
symbol = "BTCUSDT"
timeframe = "1d"
"#;
        let err = toml::from_str::<DataConfig>(missing_type)
            .unwrap_err()
            .to_string();
        assert!(err.contains("type"), "unexpected: {err}");
    }
}

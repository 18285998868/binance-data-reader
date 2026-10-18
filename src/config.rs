use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use serde::Deserialize;

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

/// TOML 反序列化用的原始形态：全 `String`，不做任何校验。
/// 必填字段（`type/dir/symbol/timeframe`）缺失即反序列化失败，
/// 不再静默 default 后报 confusing 的白名单错误；`Option` 日期缺省为 `None`。
/// 公开仅为 `TryFrom` 目标，业务代码请用 `DataConfig::new`。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDataConfig {
    #[serde(rename = "type")]
    source_type: DataSource,
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
    pub fn new(
        source_type: DataSource,
        dir: impl AsRef<Path>,
        symbol: &str,
        timeframe: &str,
        start_date: Option<&str>,
        end_date: Option<&str>,
    ) -> Result<Self, ReaderError> {
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
        Self::new(
            raw.source_type,
            &raw.dir,
            &raw.symbol,
            &raw.timeframe,
            raw.start_date.as_deref(),
            raw.end_date.as_deref(),
        )
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

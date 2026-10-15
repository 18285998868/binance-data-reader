use std::path::PathBuf;

use serde::Deserialize;

use crate::error::ReaderError;

#[derive(Debug, Clone, Copy, PartialEq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DataSource {
    #[default]
    Binance,
    Dukascopy,
}

/// Binance 支持的时间帧(与 binance-data-downloader 的白名单一致).
const BINANCE_TIMEFRAMES: [&str; 12] = [
    "1m", "3m", "5m", "15m", "30m", "1h", "2h", "4h", "6h", "8h", "12h", "1d",
];

/// Dukascopy 支持的时间帧(与 dukascopy-data-downloader 的白名单一致).
const DUKASCOPY_TIMEFRAMES: [&str; 6] = ["M1", "H1", "D1", "1m", "1h", "1d"];

/// 单品种 × 单时间帧的数据加载配置, 既是 TOML 反序列化目标也是加载入参。
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DataConfig {
    #[serde(rename = "type")]
    pub source_type: DataSource,
    pub dir: PathBuf,
    pub symbol: String,
    pub timeframe: String,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
}

impl Default for DataConfig {
    fn default() -> Self {
        Self {
            source_type: DataSource::Binance,
            dir: PathBuf::new(),
            symbol: String::new(),
            timeframe: String::new(),
            start_date: None,
            end_date: None,
        }
    }
}

impl DataConfig {
    /// 轻量校验: 非空 + 分源 timeframe 白名单 + 日期格式/顺序.
    /// 由 `load_columns` 入口调用, 编程构造的非法配置同样 fail-fast.
    pub fn validate(&self) -> Result<(), ReaderError> {
        if self.symbol.is_empty() {
            return Err(ReaderError::Config("symbol must not be empty".into()));
        }
        if self.timeframe.is_empty() {
            return Err(ReaderError::Config("timeframe must not be empty".into()));
        }
        let (valid, name) = match self.source_type {
            DataSource::Binance => (&BINANCE_TIMEFRAMES[..], "binance"),
            DataSource::Dukascopy => (&DUKASCOPY_TIMEFRAMES[..], "dukascopy"),
        };
        if !valid.contains(&self.timeframe.as_str()) {
            return Err(ReaderError::Config(format!(
                "unsupported {name} timeframe: {}, valid values: {valid:?}",
                self.timeframe
            )));
        }
        let parse = |label: &str, v: Option<&String>| match v {
            None => Ok(None),
            Some(s) => chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .map(Some)
                .map_err(|_| {
                    ReaderError::Config(format!("invalid {label} (want YYYY-MM-DD): {s}"))
                }),
        };
        let start = parse("start_date", self.start_date.as_ref())?;
        let end = parse("end_date", self.end_date.as_ref())?;
        if let (Some(s), Some(e)) = (start, end)
            && s > e
        {
            return Err(ReaderError::Config(format!(
                "start_date {s} is after end_date {e}"
            )));
        }
        Ok(())
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
        assert_eq!(cfg.symbol, "BTCUSDT");
        assert_eq!(cfg.timeframe, "1m");
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
        DataConfig {
            source_type: DataSource::Binance,
            dir: PathBuf::from("/data"),
            symbol: "BTCUSDT".into(),
            timeframe: "1d".into(),
            start_date: None,
            end_date: None,
        }
    }

    #[test]
    fn default_is_invalid_until_filled() {
        let cfg = DataConfig::default();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_ok() {
        assert!(valid_cfg().validate().is_ok());
    }

    #[test]
    fn validate_empty_symbol_or_timeframe_fails() {
        let mut cfg = valid_cfg();
        cfg.symbol.clear();
        assert!(cfg.validate().is_err());
        let mut cfg = valid_cfg();
        cfg.timeframe.clear();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_timeframe_is_per_source() {
        // "D1" is dukascopy-only, "3m" is binance-only.
        let mut cfg = valid_cfg();
        cfg.timeframe = "D1".into();
        assert!(cfg.validate().is_err());
        let mut cfg = valid_cfg();
        cfg.source_type = DataSource::Dukascopy;
        cfg.symbol = "EURUSD".into();
        cfg.timeframe = "D1".into();
        assert!(cfg.validate().is_ok());
        cfg.timeframe = "3m".into();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_bad_date_or_order_fails() {
        let mut cfg = valid_cfg();
        cfg.start_date = Some("not-a-date".into());
        assert!(cfg.validate().is_err());
        let mut cfg = valid_cfg();
        cfg.start_date = Some("2025-01-02".into());
        cfg.end_date = Some("2025-01-01".into());
        assert!(cfg.validate().is_err());
    }
}

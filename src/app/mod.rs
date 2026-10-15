#[path = "01_reader/binance.rs"]
mod binance;
#[path = "02_clean.rs"]
mod clean;
#[path = "03_column.rs"]
pub mod columns;
#[path = "01_reader/dukascopy.rs"]
mod dukascopy;

use crate::config::{DataConfig, DataSource};
use crate::error::{ReaderError, Result};
use rayon::prelude::*;

pub use columns::KlineColumns;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Record {
    pub open_time: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

/// 逻辑编排: 配置读取&校验 -> 管道分发 -> 加载数据 -> 全局排序 -> clean -> 列转置.
///
/// 全局排序在此显式执行: `clean` 要求输入已按 `open_time` 有序.
pub fn load_columns(config: &DataConfig) -> Result<KlineColumns> {
    config.validate()?;
    let dir_str = config
        .dir
        .to_str()
        .ok_or_else(|| ReaderError::Parse("non-UTF8 path in config.dir".into()))?;

    let records = match config.source_type {
        DataSource::Binance => binance::load_pipeline(
            dir_str,
            &config.symbol,
            &config.timeframe,
            config.start_date.as_deref(),
            config.end_date.as_deref(),
        )?,
        DataSource::Dukascopy => dukascopy::load_dukascopy_pipeline(
            dir_str,
            &config.symbol,
            &config.timeframe,
            config.start_date.as_deref(),
            config.end_date.as_deref(),
        )?,
    };

    let mut records: Vec<Record> = records;
    records.par_sort_by_key(|r| r.open_time);

    // 去重 + 缺口检查(只报不改数): 下游序列保持拼接原样, 报告供研究复核回测.
    let (records, report) = clean::clean_sorted(records, &config.timeframe);
    if report.dups_removed > 0 {
        tracing::warn!(
            symbol = %config.symbol,
            timeframe = %config.timeframe,
            dups_removed = report.dups_removed,
            "deduped duplicate bars by open_time (keep first)"
        );
    }
    if report.gap_check_skipped {
        tracing::warn!(
            symbol = %config.symbol,
            timeframe = %config.timeframe,
            "skipped gap check: unknown timeframe"
        );
    } else if !report.gaps.is_empty() {
        let missing_total: i64 = report.gaps.iter().map(|g| g.missing_bars.max(0)).sum();
        tracing::warn!(
            symbol = %config.symbol,
            timeframe = %config.timeframe,
            gap_segments = report.gaps.len(),
            missing_bars = missing_total,
            samples = ?clean::gap_samples(&report.gaps),
            "detected bar gaps; series kept as-is, review backtest results"
        );
    }

    Ok(KlineColumns::from(records))
}

# binance-data-reader

> 从本地 Binance 公共归档加载数据为列向量。只负责加载、读取、清洗数据；
> 格式转换由下游自行处理。

## 两套入口

| 入口 | 配置 | 输出 |
| --- | --- | --- |
| `load_columns(&DataConfig)` | 非 K 线数据集，另含 index/mark/premium klines | `DatasetColumns` |
| `load_kline_columns(&KlineConfig)` | 普通 OHLCV K 线 | `KlineColumns` |

数据布局与 `binance-data-downloader` 一致（`dir` 为 `binance/` 的父目录）：

```text
{dir}/binance/data/{spot|futures/um|futures/cm}/{daily|monthly}/{dataset}/{symbol}[/{interval}]/*.zip
```

普通 K 线：

```text
{dir}/binance/data/{market}/daily/klines/{symbol}/{timeframe}/*.zip
```

## 数据集

| `dataset` | spot | futures/um | futures/cm | 档位 |
| --- | --- | --- | --- | --- |
| `aggTrades` | ✅ | ✅ | ✅ | daily / monthly |
| `trades` | ✅ | ✅ | ✅ | daily / monthly |
| `bookTicker` | ❌ | ✅ | ✅ | daily / monthly |
| `bookDepth` | ❌ | ✅ | ✅ | daily |
| `fundingRate` | ❌ | ✅ | ✅ | monthly |
| `liquidationSnapshot` | ❌ | ❌ | ✅ | daily |
| `metrics` | ❌ | ✅ | ✅ | daily |
| `indexPriceKlines` | ❌ | ✅ | ✅ | daily / monthly |
| `markPriceKlines` | ❌ | ✅ | ✅ | daily / monthly |
| `premiumIndexKlines` | ❌ | ✅ | ✅ | daily / monthly |

- `indexPriceKlines` 按标的指数存放：`BTCUSD_PERP` 读取 `indexPriceKlines/BTCUSD`，
  `BTCUSDT_250926` 读取 `BTCUSDT`。
- reader 同时扫描 `daily` 与 `monthly`，重叠数据按主键去重。
- `DatasetColumns` 由 `dataset` 标签区分具体列集；K 线形态数据集使用
  `KlineDatasetColumns`（保留 `dataset` 字段）。

## 配置

数据集示例见 `config.example.toml`，普通 K 线见 `config.kline.example.toml`。

```rust
let cfg: DataConfig = toml::from_str(&config_str)?;
let columns = load_columns(&cfg)?;

let cfg: KlineConfig = toml::from_str(&kline_config_str)?;
let bars = load_kline_columns(&cfg)?;
```

日期语义为 UTC，`end_date` 含当日整天。构造 `DataConfig`/`KlineConfig` 时完成
全部校验（市场、品种、周期、日期范围），非法输入直接返回 `ReaderError::Config`。

## Python

```python
import binance_data_reader

cols = binance_data_reader.load_columns({
    "dataset": "metrics",
    "dir": "path/to/data",
    "symbol": "BTCUSDT",
})

bars = binance_data_reader.load_kline_columns({
    "dir": "path/to/data",
    "symbol": "BTCUSDT",
    "timeframe": "1d",
})
```

Python 绑定只负责解析参数并调用对应 Rust 接口。

## 处理语义

- 单文件硬失败零容忍：损坏 zip、缺失 CSV、非法文件名日期 → 整批 `Err`。
- 行级坏数据 warn 并跳过（留样上限 5）。
- 普通 K 线：全局按 `open_time` 排序、去重 keep-first、缺口只报告不填充；
  日期裁剪为 `[start 00:00, end 次日 00:00)`。
- 数据集：按各自主键排序去重（K 线按 `open_time`，成交按 ID）。

## 架构

```text
src/
  lib.rs                 导出 + PyO3 模块
  config.rs              DataConfig / KlineConfig
  types.rs               Market / Dataset / Interval / Symbol / Timeframe
  app/mod.rs             两个入口与共享清洗
  app/01_reader/         各数据集 reader + 普通 K 线 reader
  app/02_clean*.rs       数据集去重 / K 线去重与缺口检查
  app/03_column*.rs      数据集列集 / OHLCV 列集
tests/                   zip 归档集成测试
```

依赖：`zip`、`csv`、`chrono`、`rayon`、`serde`、`thiserror`、`tracing`、
`pyo3`（abi3-py39）。无 `unsafe`。

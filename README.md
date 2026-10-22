# binance-reader

## 项目目标

> 加载 `kline-reader` 未覆盖的 Binance 公共归档数据集为类型化列向量，供回测/研究使用。

`kline-reader` 负责 `klines`（OHLCV），本 crate 是其补集：

| `dataset` | spot | futures/um | futures/cm | 归档档位 |
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

数据布局与 `binance-data-downloader` 严格一致（`dir` 为 `binance/` 的父目录，与 `kline-reader` 的 `dir` 相同）：

```text
{dir}/binance/data/{spot|futures/um|futures/cm}/{daily|monthly}/{dataset}/{symbol}[/{interval}]/*.zip
```

- 非 K 线文件名：`SYMBOL-DATASET-YYYY-MM[-DD].zip`
- K 线形态文件名：`SYMBOL-INTERVAL-YYYY-MM[-DD].zip`
- `indexPriceKlines` 按**标的指数**存放：配置 `BTCUSD_PERP` 时读取 `indexPriceKlines/BTCUSD/...`，配置 `BTCUSDT_250926` 时读取 `BTCUSDT`。
- reader 同时扫描 `daily` 与 `monthly`（至少存在一个即可），重叠数据按主键去重。

## 技术选型

- Rust，edition 2024
- `zip` + `csv`：归档为 zip 内单个 CSV
- `rayon`：多文件并行解压/解析
- `pyo3`（abi3-py39）：可选 Python 扩展
- 无 `unsafe`

## 冒烟配置

```toml
dataset = "metrics" # 必填；也可写 type = "metrics"（兼容下载器字段名）
market = "futures/um" # 可选，默认 futures/um；spot | futures/um | futures/cm
dir = "/data/nutilustrader/data" # 必填，数据根目录（程序自动追加 binance/data/...）
symbol = "BTCUSDT" # 必填，单品种
# interval = "4h" # 仅 index/mark/premium klines 必填
# start_date = "2024-01-01" # 可选，含当日（UTC）
# end_date = "2024-01-31" # 可选，含当日整天（UTC）
```

```rust
let cfg: DataConfig = toml::from_str(&config_str)?;
let columns: DatasetColumns = load_columns(&cfg)?;
println!("{} rows", columns.len());
```

## 逻辑编排

```text
调用方          提供 DataConfig(toml / 构造器)
app/mod.rs      load_columns(&cfg)
                ├─ scan_zip_files: 扫 daily+monthly，文件名日期 → 区间重叠过滤
                ├─ 按 dataset 分发 01_reader/*
                │    ├─ 并行解压 zip → 取首个 .csv
                │    ├─ 表头判别（精确匹配已知表头；spot 逐笔无表头）
                │    ├─ 行解析 + 字段校验；坏行记数留样后跳过
                │    └─ 行级 UTC 日期窗口过滤
                ├─ sort + dedupe（按各数据集主键，keep-first）
                └─ 03_column: 转置为对应 Columns / DatasetColumns
```

## 架构设计

```text
lib.rs            只做导出
config.rs         DataConfig：构造即有效（数据集×市场、interval 规则、日期顺序）
error.rs          ReaderError：Io/Csv/Zip/Utf8/Config/Parse
types.rs          Market / Dataset / Interval / Symbol
app/
  mod.rs          编排：扫描 → 分发 → 排序去重 → 列转置
  01_reader/
    common.rs     共享设施：路径/日期窗口/zip/csv/表头/字段解析/硬失败收口
    klines.rs     index/mark/premium klines（premium 允许负价）
    trades.rs     aggTrades + trades（spot 无表头、CM base_qty）
    book.rs       bookTicker + bookDepth（字符串 UTC 时间）
    funding.rs    fundingRate（允许负费率）
    metrics.rs    metrics（字符串 UTC 时间、可选比率字段）
    liquidation.rs liquidationSnapshot（整行精确去重）
  02_clean.rs     排序 + 去重（主键 / 整行身份）
  03_column.rs    各数据集 Columns + DatasetColumns 枚举
python.rs         pyo3：load_columns(dict) -> dict
tests/
  common/mod.rs   合成 zip
  integration.rs  全链路
```

## 实现细节

### 公共设施（01_reader/common.rs）

1. `scan_zip_files`：对 `daily`/`monthly` 两个档位分别拼路径；目录存在才扫描，两者都不存在返回 `Io(NotFound)` 并给出期望路径。
2. 文件名日期取 stem 末尾：日档 `YYYY-MM-DD`（10 字符），月档 `YYYY-MM`（7 字符）。无日期形状的 zip 直接跳过；**形状像日期但非法**（如 `2024-13-01`）按硬失败收口。
3. 文件级过滤按月区间重叠：`file_end <= start` 或 `file_start > end` 才跳过，月文件与窗口部分重叠时整文件保留，后续行级裁剪兜底。
4. zip 取首个 `.csv`（扩展名大小写不敏感）；无 CSV 即硬失败。
5. CSV 表头只接受精确匹配的已知表头（CRLF 兼容）；数字首字段永不判为表头。首部空行只跳行，无尾换行的单行照常解析。
6. 行级坏数据（字段缺失/非法数字/非有限值/负值等）记数并留样 5 条后跳过，不炸整文件。
7. 文件级坏数据（损坏 zip、无 CSV、非法文件名日期、读文件失败）零容忍：整批 `Err` 并附失败数与留样路径。

### klines（indexPriceKlines / markPriceKlines / premiumIndexKlines）

- 12 列标准 K 线表头，取前 6 列。
- `index/mark`：价格必须 `> 0`，`volume >= 0`。
- `premiumIndex`：价格可为 0 或负（溢价率围绕 0），只要求有限且满足 `high >= max(o,l,c)`、`low <= min(o,h,c)`。
- 主键 `open_time`；排序去重后返回 `KlineColumns`，并用 `dataset()` 标记来源。

### trades（aggTrades / trades）

- `aggTrades`：spot 无表头且 8 列（多 `is_best_match`，忽略）；UM/CM 有表头 7 列。
- `trades`：spot 无表头 7 列；UM 6 列 `quote_qty`；**CM 第 4 列是 `base_qty`**。
- `is_buyer_maker` 大小写不敏感解析（spot 为 `False/True`）。
- `aggTrades` 校验 `first_trade_id <= last_trade_id`；价格/数量必须 `> 0`。
- 主键分别为 `agg_trade_id`、`trade_id`。

### book（bookTicker / bookDepth）

- `bookTicker`：7 列表头。早期行可能出现 `0E-8` 的 ask 价/量（尚无卖盘），因此价格/数量允许 0，拒绝负值。主键 `update_id`；时间列取 `event_time`。
- `bookDepth`：4 列表头，`timestamp` 为 `YYYY-MM-DD HH:MM:SS`（UTC）字符串。主键 `(time, percentage)`；`depth/notional >= 0`。

### fundingRate

- 月档 3 列：`calc_time`(ms)、`funding_interval_hours`、`last_funding_rate`。
- 费率**允许负值**；`funding_interval_hours` 必须 `> 0`。主键 `calc_time`。

### metrics

- 8 列表头，`create_time` 为 UTC 字符串。
- `sum_open_interest`/`sum_open_interest_value` 必填且 `>= 0`（可为 0）。
- 四个比率字段在真实归档中可能为空（尤其 COIN-M），解析为 `Option<f64>`，空值保持 `None`，绝不填 0。主键 `create_time`。
- 真实归档中同一 `create_time` 可能出现**完全重复的行**（UM 日档实测每行重复两次），按主键 keep-first 去重。

### liquidationSnapshot

- 仅 COIN-M，日档，10 列表头。
- 字符串列 `side/order_type/time_in_force/order_status` 原样保留；价格必须 `> 0`，数量 `>= 0`。
- 归档中存在**完全重复的行**，因此按整行身份（浮点按位模式）精确去重，keep-first。

## 问题与解决

- 当 `premiumIndexKlines` 出现 0/负价格时，沿用 kline-reader 的 `> 0` 校验会丢掉整段数据，
  - 解决：仅 premium 放宽符号限制，仍校验 OHLC 对齐；index/mark 保持 `> 0`。
  - 文件：`src/app/01_reader/klines.rs`
- 当 spot 逐笔文件没有表头、且比期货多一列时，按固定列数解析会错位，
  - 解决：已知表头精确匹配 + `flexible`；spot 8/7 列只读前 7/6 个有效字段。
  - 文件：`src/app/01_reader/trades.rs`
- 当 COIN-M `trades` 的第 4 列实际是 `base_qty` 时，误命名为 `quote_qty` 会误导下游，
  - 解决：`TradeColumns` 同时提供 `quote_qty()`（spot/UM）与 `base_qty()`（CM），只会有一个 `Some`。
  - 文件：`src/app/03_column.rs`
- 当 `metrics` 比率字段为空时，按必填数字解析会丢掉整行（连带 OI 数据），
  - 解决：比率字段解析为 `Option<f64>`，空值 → `None`。
  - 文件：`src/app/01_reader/metrics.rs`
- 当 liquidation 归档包含重复行时，会重复计算强平规模，
  - 解决：按整行身份精确去重（浮点用 `to_bits`），keep-first。
  - 文件：`src/app/01_reader/liquidation.rs`、`src/app/02_clean.rs`
- 当 daily 与 monthly 同时存在时，会重复加载同一事件，
  - 解决：两个档位都扫描，按各数据集主键去重并 warn 去重数。
  - 文件：`src/app/mod.rs`、`src/app/02_clean.rs`
- 当 `indexPriceKlines` 按标的指数而不是合约存放时，会找不到目录，
  - 解决：`Dataset::storage_symbol` 对 index 数据集去掉 `_PERP`/`_YYMMDD` 后缀。
  - 文件：`src/types.rs`
- 当文件名日期非法（如 `2024-13-01`）时，会误读区间外数据，
  - 解决：形状像日期但非法直接硬失败，超过零容忍阈值整批 `Err`。
  - 文件：`src/app/01_reader/common.rs`

## 完整配置

```toml
dataset = "aggTrades" # 必填；可选 10 种之一，也接受 type = "..."
market = "futures/um" # 可选，默认 futures/um；spot | futures/um | futures/cm
dir = "/data/nutilustrader/data" # 必填，数据根目录
symbol = "BTCUSDT" # 必填，单品种
# interval = "1m" # 仅 index/mark/premium klines 必填；其它数据集禁止出现
# start_date = "2024-01-01" # 可选，UTC，含当日
# end_date = "2024-12-31" # 可选，UTC，含当日整天
```

校验规则：

- `dataset` 与 `market` 必须真实存在（如 spot 不支持 `metrics`，`liquidationSnapshot` 仅 CM）。
- `interval`：K 线形态必填，其它数据集禁止；spot 白名单含 `1s`，期货从 `1m` 起。
- 日期格式 `YYYY-MM-DD` 且 `start <= end`。
- 未知字段（`deny_unknown_fields`）直接报错。

## 完整执行逻辑

- `lib.rs`：只做导出与 `#[pymodule]` 注册。
- `config.rs`：TOML → `RawDataConfig` → `DataConfig`（一次校验）；`dataset` 兼容 `type` 键。
- `app/mod.rs`：`load_columns` 唯一入口；按 dataset 分发；排序去重；记录去重 warn。
- `01_reader/common.rs`：路径扫描、日期窗口、zip/csv、表头、字段解析、坏行/坏文件收口。
- `01_reader/*.rs`：各数据集行解析与行级时间裁剪。
- `02_clean.rs`：`sort_dedup_by_key`、`sort_by_key`、`dedupe_by_hash_key`。
- `03_column.rs`：8 个 Columns 结构 + `DatasetColumns` 枚举 + `From<Vec<Record>>` 转置。

## Python

```bash
uvx maturin build # 产物 target/wheels/*cp39-abi3*.whl
```

```python
import binance_reader

cols = binance_reader.load_columns({
    "dataset": "metrics",          # 或 "type": "metrics"
    "market": "futures/um",
    "dir": "/data/nutilustrader/data",
    "symbol": "BTCUSDT",
    "start_date": "2024-01-01",
    "end_date": "2024-01-03",
})
print(cols["dataset"], len(cols["time"]), sorted(cols))
```

返回值总是包含 `dataset` 与 `time`，其余键随数据集变化：

| dataset | 其余键 |
| --- | --- |
| index/mark/premium klines | `open high low close volume` |
| aggTrades | `agg_trade_id price quantity first_trade_id last_trade_id is_buyer_maker` |
| trades | `trade_id price qty (quote_qty \| base_qty) is_buyer_maker` |
| bookTicker | `update_id best_bid_price best_bid_qty best_ask_price best_ask_qty transaction_time` |
| bookDepth | `percentage depth notional` |
| fundingRate | `funding_interval_hours last_funding_rate` |
| liquidationSnapshot | `side order_type time_in_force original_quantity price average_price order_status last_fill_quantity accumulated_fill_quantity` |
| metrics | `sum_open_interest sum_open_interest_value count_toptrader_long_short_ratio sum_toptrader_long_short_ratio count_long_short_ratio sum_taker_long_short_vol_ratio` |

`metrics` 的可选比率在 Python 中为 `None`。

## 与 kline-reader 的分工

- `klines`（含 monthly）→ `kline-reader`。
- 其余 10 类 → 本 crate。
- 两者使用相同的 `dir` 语义、日期语义（UTC、`end_date` 含当日）与硬失败零容忍策略。

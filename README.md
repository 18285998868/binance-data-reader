# kline-reader

## 项目目标

> 从本地 K 线数据目录加载 OHLCV 为列向量(`KlineColumns`)，供回测/策略使用
> 数据布局(`dir` 为数据根目录，与两下载器 `output_dir` 一致):

- `{dir}/binance/data/um/daily/klines/{symbol}/{timeframe}/*.zip` (Binance): 每个 zip 内含一个 `{symbol}-{tf}-{YYYY-MM-DD}.csv`(Binance 官方 12 列，取前 6 列)，按日分文件
- `{dir}/dukascopy/{symbol}/{YYYY}[/{MM}[/{DD}]]/{PRICE}_{candles_*}.bi5` (Dukascopy, 官方镜像布局): LZMA 压缩二进制，每记录 24 字节大端；D1 按年(`{YYYY}/BID_candles_day_1.bi5`) / H1 按月(`{YYYY}/{MM}/BID_candles_hour_1.bi5`) / M1 按天(`{YYYY}/{MM}/{DD}/BID_candles_min_1.bi5`)
- 调用方: vta / run-backtest-node 等通过 `kline-reader` workspace path 依赖接入，流程为 `DataConfig`(toml)→ `load_columns(&cfg)`

## 技术选型

- rust
- edition = "2024"

## 冒烟配置

```toml
type = "binance" # 必填
dir = "/data/nutilustrader/data" # 必填，数据根目录(程序按数据源自动追加子路径)
symbol = "BTCUSDT" # 必填，单品种
timeframe = "1d" # 必填，单周期；binance "1m"~"1d"，dukascopy "M1"/"H1"/"D1"
# start_date = "2024-01-01" # 可选
# end_date = "2024-12-31" # 可选，含当日整天(UTC)
```

```rust
let cfg: DataConfig = toml::from_str(&config_str)?;
let cols: KlineColumns = load_columns(&cfg)?; // 内部已按 open_time 全局排序+去重
```

## 逻辑编排

```
调用方          提供 DataConfig(toml) 反序列化
app/mod.rs      入口 load_columns(&cfg)：配置读取&校验 -> 按 source 分发单条管线 -> 加载数据 -> 全局排序 -> clean -> 列转置
                01_reader/binance.rs   -> 扫描 {dir}/binance/data/um/daily/klines/{symbol}/{tf}/*.zip → 文件名日期过滤 → par 解压取 .csv → 表头判别 → 解析前 6 列，坏行跳过
                01_reader/dukascopy.rs -> 递归扫描 {dir}/dukascopy/{symbol}/ 下 *.bi5 → 文件名 candles_* 匹配 timeframe + 路径年月日定周期 → 区间重叠过滤 → lzma 解压 → 24B 大端解析
                合并 records -> par_sort_by_key(open_time) 全局排序(clean 要求输入有序)
02_clean        去重 keep-first + 缺口检查(只报不改数)
03_column       KlineColumns::from(records) 列转置 → time/open/high/low/close/volume
```

## 架构设计

```
lib.rs            只做导出
config.rs         支撑件，不编号
error.rs          支撑件，不编号
app/
  mod.rs          逻辑编排: 配置读取&校验 -> 管道分发 -> 加载数据 -> 全局排序 -> clean -> 列转置
  01_reader/      被 mod 分发的加载(分支二选一)
    binance.rs
    dukascopy.rs
  02_clean.rs     去重+缺口检查(要求输入已排序)
  03_column.rs    收尾列转置
test_helpers.rs   支撑件，不编号
tests/
  common/mod.rs
  integration.rs
```

## 实现

### lib.rs

- 只做导出：`pub mod app` + `mod config/error`，`pub use` 对外 API
- `pub use`: KlineColumns / DataConfig / ReaderError / Result / load_columns / DataSource
- 内部 `Record{open_time,o,h,l,c,v}`

### config

解析层：

1. `DataConfig` 为 serde 反序列化目标，同时是 `load_columns` 入参(单品种×单周期)。
2. `type` 重命名到 `source_type: DataSource`；`dir: PathBuf`(数据根目录)；`symbol: Symbol` / `timeframe: Timeframe`(已验证新类型)；`start_date/end_date: Option<NaiveDate>`。
3. 缺必填字段(`type/dir/symbol/timeframe`)即反序列化失败，不静默 default；非法值经 `DataConfig::new` fail-fast 报 `Config` 错。
4. `DataSource` 为 `binance|dukascopy` 小写枚举。

### app/mod

1. `DataConfig::new` / 反序列化即完全校验：symbol 非空，分源 timeframe 白名单(binance 12 值 / dukascopy 6 值)，日期 `YYYY-MM-DD` 合法且 `start≤end`，失败直接 `Config` 报错。
2. `dir` 以 `impl AsRef<Path>` 透传，支持非 UTF-8 路径。
3. 按 `source` 分发到 binance / dukascopy 管线，透传 `start_date/end_date`。
4. 合并后先行级 UTC 日期裁剪(`[start日00:00, end次日00:00)`，`end_date` 含当日整天)，再 `par_sort_by_key(open_time)` 全局排序。
5. 调 `clean::clean_sorted(records, timeframe)`：去重数>0 则 warn；有缺口则 warn(段数/缺数/留样)，序列保持原样。
6. `KlineColumns::from(records)` 返回；单文件硬失败(损坏 zip/bi5、无 CSV、非法文件名日期)超零容忍阈值直接整批 `Err` 并报告失败数+留样路径。

### 01_reader/binance

1. 扫描 `{dir}/binance/data/um/daily/klines/{symbol}/{tf}/*.zip`；目录不存在返 `Io(NotFound)` 带路径。
2. 文件名取 stem 末 10 字符做日期形状校验(`YYYY-MM-DD`)，合法才做字符串区间过滤；形状像日期但非法(如 2024-13-01)直接 `Parse` 报错跳过该文件。
3. `par_sort_unstable` 排路径，`par_iter` 并行解压解析。
4. 单 zip 取首个 `.csv`(大小写不敏感)，无 csv 即 `Parse` 报错跳过该文件。
5. 表头判别：首行严格等于标准 12 列才算表头(CRLF 兼容)；数字首字段永不判为表头(误判行只坏一行，不丢全文件)。无尾换行单行按单行解析，首部空行只跳行。
6. 行解析：取前 6 列，`open_time: i64 ≥ 0`，价格有限且 `> 0`、量有限且 `≥ 0`，且 `high≥max/low≤min`(与 dukascopy 同口径，`is_valid_ohlc`)；坏行记数+留样(上限 5)后跳过，不炸全文件。
7. 单文件硬失败(损坏 zip、无 csv、非法文件名日期)计入零容忍收口，超阈值整批 `Err`；结束打 `rows + elapsed` 日志。

### 01_reader/dukascopy

1. 递归扫描(`walkdir`) `{dir}/dukascopy/{symbol}/` 下全部 `*.bi5`；symbol 目录不存在返 `Io(NotFound)` 带路径。
2. 文件名分类(精确匹配)：`{PRICE}_candles_day_1` 按年、`{PRICE}_candles_hour_1` 按月、`{PRICE}_candles_min_1` 按天；candles 段与请求 timeframe 不一致的文件直接跳过(M1/1m→min_1，H1/1h→hour_1，其余→day_1)。未知映射直接 `Config` 报错，不静默回落。
3. 周期起点从路径取：年文件取 `{YYYY}`，月文件取 `{YYYY}/{MM}`，天文件取 `{YYYY}/{MM}/{DD}`；层数/数字非法直接跳过(旧 `{symbol}-{tf}-{周期}.bi5` 命名不再识别)。周期结束用 checked 计算，溢出即该文件硬失败。
4. 区间重叠过滤：`period_end ≤ start` 跳过，`period_start > end` 跳过，年/月文件有重叠即保留；合并后另有行级 UTC 裁剪兜底越界 bar。
5. `lzma_rs` 解压；每 24 字节大端解析：`time_secs:u32` + open/close/low/high(各 u32 `/100000`，**字段序 O,C,L,H**) + `volume`；`open_time = period_start午夜 + Δ秒`；坏 OHLC 行按统一口径跳过计数。
6. 尾部非 24 整除只 warn；单文件硬失败(解压/解析失败)计入零容忍收口，超阈值整批 `Err`；结束打 `rows + elapsed` 日志。

### 03_column & 02_clean

- `03_column`: `KlineColumns{time,open,high,low,close,volume}` + `From<Vec<Record>>` 列转置；`len/is_empty` 按 close 计。
- `02_clean`: 输入须已排序；`dedup_sorted` 用 `retain` 保序 keep-first；`interval_ms_for_timeframe` 映射 1m/M1~1w，未知周期返 `Err` 调用方降级跳过；`find_gaps` 相邻差≠期望即记段(`missing_bars=diff/iv-1`，`misaligned=diff%iv!=0`)；`gap_samples` 留样上限 5。

## 问题:

- 当 zip 内 csv 混有表头/无表头/坏行时，会丢全文件，
  - 解决: 严格 12 列表头判别 + 坏行记数留样后跳过，只保好行
  - 文件: `src/app/01_reader/binance.rs`(`has_csv_header`/`parse_csv_bytes`/`SkippedRows`)
- 当文件名日期非法(如 2024-13-01)时，会误读区间外数据，
  - 解决: 日期形状但非法直接 `Parse` 报错跳过该文件，不过滤进合并
  - 文件: `src/app/01_reader/binance.rs`(`extract_date_from_filename`)
- 当 dukascopy high/close 进策略错位时，回测用错价格，
  - 解决: 实数据验证字段序为 O,C,L,H（旧代码按 O,H,L,C 读，high/close 互换）；现按正确槽位解析 + 统一 OHLC 校验兜底
  - 文件: `src/app/01_reader/dukascopy.rs`(`parse_bi5_records`)
- 当单个 zip/bi5 损坏、无 CSV、文件名日期非法时，会污染整批回测，
  - 解决: 回测准确性优先，硬失败零容忍(`MAX_FAILED_FILES_TOLERATED=0`)，超阈值整批 `Err` 并报告失败数+留样路径；无日期形状/区间外文件仍意图内跳过
  - 文件: `src/app/mod.rs`(`fail_on_hard_errors`)、`src/app/01_reader/binance.rs`、`src/app/01_reader/dukascopy.rs`
- 当 dukascopy 年/月文件与起止区间部分重叠时，会整文件误杀，
  - 解决: 按 `[period_start, period_end)` 重叠判断，有交集即保留
  - 文件: `src/app/01_reader/dukascopy.rs`(`should_skip_file`)
- 当 dukascopy 目录混入其它 timeframe 文件时，会误解析周期，
  - 解决: 文件名 candles 段必须与请求 timeframe 一致，否则跳过；周期起点只从路径年月日取，不从文件名取
  - 文件: `src/app/01_reader/dukascopy.rs`(`parse_bi5_path`)
- 当序列有重复 bar 或缺口时，回测会压缩跳空、延迟信号，
  - 解决: 去重 keep-first + 缺口只报不改数(段数/缺数/留样 warn)，序列保持原样供复核
  - 文件: `src/app/02_clean.rs`，调用点 `src/app/mod.rs`
- 当 `dir` 含非 UTF-8 路径时，
  - 现状: Rust 入口以 `AsRef<Path>` 透传，已支持；仅 Python dict 入口要求 UTF-8 `String`
  - 文件: `src/app/mod.rs`、`src/python.rs`
- 当调用方沿用老配置(`symbol=[]` 数组 / `timeframes` 复数键 / `to_specs` / 内层 klines 路径 / 旧 dukascopy `{tf}` 目录命名)时，会反序列化失败或找不到目录，
  - 解决: 现行 `DataConfig` 为单品种单周期(`symbol: String` / `timeframe: String`)，`dir` 统一为数据根目录；老 `config.example.toml` 已更新，以本文为准

## 完整架构

```
lib.rs            只做导出: pub mod app + mod config/error + pub use: KlineColumns / DataConfig / ReaderError / Result / load_columns / DataSource
config.rs         # 支撑件: DataConfig(serde default + Default 空值 + validate: 非空/分源白名单/日期合法+顺序) + DataSource 小写枚举 + load_columns 入参(单品种×单周期)
error.rs          # 支撑件: ReaderError(Io/Csv/Zip/Utf8/Parse, non_exhaustive) + Result<T>，被各层引用
test_helpers.rs   # 支撑件: cfg(test) create_test_zip(单测专用的合成 zip)
app/mod.rs        # 入口 load_columns(&DataConfig): 先 validate → 按 source 分发到 01_reader 两管线 → 合并后 par_sort_by_key(open_time) 排序 → clean_sorted 去重+缺口检查 → KlineColumns::from(records); 内部 Record{open_time,o,h,l,c,v}
app/01_reader/    # 被 mod 分发的加载(分支二选一)
  binance.rs      # load_pipeline: 扫描 {dir}/binance/data/um/daily/klines/{symbol}/{tf}/*.zip → 按文件名日期做文件粒度过滤 → par 解压 zip 取 .csv → 表头自动判别 → 解析前 6 列; 坏行/坏文件 warn 跳过
  dukascopy.rs    # load_dukascopy_pipeline: 递归扫描 {dir}/dukascopy/{symbol}/ 下 *.bi5 → 文件名 candles_* 匹配 timeframe + 路径年月日定周期起点 → 区间重叠过滤 → lzma 解压 → 24B 大端解析 → open_time = period_start + Δ; 损坏文件 warn 跳过
app/02_clean.rs   # 清洗: dedup_sorted(keep-first) + find_gaps(缺口分段) + clean_sorted(一站式) + gap_samples(留样 5); 未知周期跳过检查; 要求输入已排序
app/03_column.rs  # 收尾: KlineColumns{time,open,high,low,close,volume} + From<Vec<Record>> 列转置
tests/
  common/mod.rs   # 集成测试共用 create_test_zip
  integration.rs  # DataConfig → load_columns 全链路(多日/空区间/缺目录/无日期过滤/日期过滤/全局排序)
```

## 完整配置

```toml
type = "binance" # 必填，可选 "binance" | "dukascopy"
dir = "/data/nutilustrader/data" # 必填，数据根目录(程序按数据源自动追加子路径)
symbol = "BTCUSDT" # 必填，单品种
timeframe = "1d" # 必填，单周期；binance "1m"~"1d"，dukascopy "M1"/"H1"/"D1"
# start_date = "2024-01-01" # 可选，缺省无(全部数据)，格式 YYYY-MM-DD
# end_date = "2024-12-31" # 可选，缺省无(全部数据)，格式 YYYY-MM-DD，含当日整天(UTC)
```

- 老字段 `symbol=[]` 数组 / `timeframes` 复数键 / `to_specs()` 笛卡尔展开已废弃，以上为准。
- 冒烟示例见本文 `冒烟配置`；`config.test.toml` 为本地 `/tmp` 冒烟用。

## 完整执行逻辑

- lib.rs
  - 只做导出
  - `#[path]` 挂载编号模块，`pub use` 对外 API，内部 `Record`
- config.rs
  - `DataConfig`：serde 反序列化目标兼加载入参(单品种×单周期)，含 `DataSource` 小写枚举(`type` 字段)
  - 缺省：仅 `start/end_date: Option` 缺省 None；必填字段缺失即反序列化失败
  - 校验：`DataConfig::new`(fail-fast：非空 + 分源白名单 + 日期格式/顺序)，`ReaderError::Config`
- error.rs
  - `ReaderError`(thiserror，non_exhaustive)：Io/Csv/Zip/Utf8/Parse
- app/mod.rs
  - `load_columns(&DataConfig)` 唯一入口
  - `dir` 以 `AsRef<Path>` 透传 → 按 source 分发管线 → 行级 UTC 裁剪 → `par_sort_by_key` 排序 → `clean_sorted` 去重+缺口检查打 warn → `KlineColumns::from`
- app/01_reader/binance.rs
  - 扫 `{dir}/binance/data/um/daily/klines/{symbol}/{tf}/*.zip`，缺目录 `Io(NotFound)`
  - `extract_date_from_filename` + 字符串区间过滤(文件粒度)
  - `par_sort_unstable` + `par_iter` 并行解压解析
  - `load_single_zip` 取首个 csv → `parse_csv_bytes`(表头判别/flexible/坏行跳过+留样 5)
  - 坏文件 warn 跳过，结束打 rows+elapsed
- app/01_reader/dukascopy.rs
  - `walkdir` 递归扫 `{dir}/dukascopy/{symbol}/` 下 `*.bi5`，缺目录 `Io(NotFound)`
  - `kind_from_filename` 定 period kind + `parse_bi5_path` 校验 timeframe 一致并从路径年月日取起点 → `should_skip_file` 重叠过滤
  - `lzma_rs` 解压 → 24B 大端解析(`time_secs` + 价格/1e5 + 量) → `open_time=period_start+Δ`
  - 坏文件 warn 跳过，结束打 rows+elapsed
- app/03_column.rs
  - `From<Vec<Record>>` 列转置为等长 6 列
- app/02_clean.rs
  - `dedup_sorted` keep-first 去重计数
  - `interval_ms_for_timeframe` 映射 1m/M1~1w，未知 Err
  - `find_gaps` 记段(missing_bars/misaligned)，`clean_sorted` 一站式，`gap_samples` 留样 5
  - 缺口只上报不补价不切段，下游序列保持原样
- tests/
  - `common` 合成 zip，`integration` 全链路(多日/空区间/缺目录/日期过滤/全局排序)

## Python

```bash
uvx maturin build # 产物 target/wheels/*cp39-abi3*.whl，>=3.9 通用
```

```python
import kline_reader
cols = kline_reader.load_columns({ # dict 同 DataConfig 字段，非法直接报错
    "type": "binance", "dir": "/data/nutilustrader/data",
    "symbol": "BTCUSDT", "timeframe": "1d",
    "start_date": "2024-01-01", "end_date": "2024-01-03",
})
print(len(cols["close"]), sorted(cols)) # -> 3 ['close', 'high', 'low', 'open', 'time', 'volume']
```

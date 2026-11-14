# binance-data-reader

把 `binance-data-downloader` 产出的本地归档读成命名、带类型的列式表。
`root` 是下载器的 `output_dir`（含 `data/` 的那一层），布局固定为：

```text
{root}/data/{market}/{daily|monthly}/{dataset}/{symbol}[/{interval}]/*.zip
```

## 模块一：构建

1. Rust 1.89+。
2. 构建 release 二进制。
3. 运行离线门禁。

```bash
cargo build --release
./scripts/check.sh
```

## 模块二：读取数据

1. 复制并修改示例请求。
2. CLI 只把 JSON 写 stdout，日志写 stderr。
3. 退出码：`0` 成功，`2` 配置/JSON 错误，`1` I/O 或数据错误。

```bash
cp config/00.example.toml /tmp/request.toml
./target/release/binance-data-reader read -c /tmp/request.toml > /tmp/data.json
```

> 省略的日期边界表示不设限。`start_date`/`end_date` 是包含端点的 UTC 日期。

## 模块三：查看归档清单

1. 过滤条件全部可选。
2. 输出 `{root, entries}`，每个条目给出文件数与首末周期。

```bash
./target/release/binance-data-reader list --root /data/data/binance --dataset klines
```

## 模块四：Rust 库

1. 构造并校验 `Config`。
2. `read` 返回 `DatasetData { table, report }`。
3. 按归档列名取列。

```rust
use binance_data_reader::{Config, Dataset, read};

let config = Config::builder("/data/data/binance", Dataset::Klines, "BTCUSDT")
    .interval("1m".parse()?)
    .date_range(
        Some("2023-06-01".parse()?),
        Some("2023-06-30".parse()?),
    )
    .build()?;
let data = read(&config)?;
let closes = data.table.f64("close").unwrap_or_default();
assert_eq!(data.table.len(), data.report.rows);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## 模块五：Python

1. 用 maturin 构建 `python` feature。
2. `read(config)` 接受 TOML 路径或 dict。
3. `catalog(root, ...)` 返回清单列表。

```bash
python -m pip install maturin
maturin develop --features python
```

```python
import binance_data_reader as r

data = r.read({"root": "/data/data/binance", "dataset": "trades",
               "symbol": "BTCUSDT", "market": "futures/um",
               "start_date": "2023-06-01", "end_date": "2023-06-30"})
print(data["report"]["rows"], list(data["columns"]))
```

## 模块六：输出结构

1. `columns` 是 `{归档列名: 数组}`；空字段是 `null`。
2. `report` 记录文件数、解码/重叠/窗口/跳过行数、去重数、首末时间和 K 线缺口。
3. 不丢字段：K 线 12 列、spot `is_best_match`、COIN-M `base_qty`、metrics 的 symbol 与可空比率都会返回。

```json
{"dataset":"klines","columns":{"open_time":[...],"close":[...]},
 "report":{"rows":43200,"rows_dropped_overlap":8640,"gaps":0}}
```

## 模块七：严格性与合并规则

1. 文件名、ZIP 内容、CSV header、每行时间必须匹配请求和文件周期。
2. 默认 `bad_rows = "fail"`；`skip` 只跳过坏行，文件级损坏始终失败。
3. daily 与 monthly 都扫描；daily 覆盖的 UTC 日丢弃 monthly 行；被 daily 全覆盖的 monthly 文件直接跳过读取.
4. K 线要求严格递增且 `close_time` 与 interval 一致；缺口只计数不填充。
5. liquidationSnapshot 的全行精确重复会去重并写入 `duplicates_removed`。

## 模块八：验收

1. 离线门禁 + 真实归档冒烟。
2. 冒烟请求固定为 BTCUSDT USD-M 1m 2023-06，期望 43,200 行。

```bash
cargo build --release
./target/release/binance-data-reader read -c config/00.smoke.toml > /tmp/smoke.json
unzip -p /data/data/binance/data/futures/um/monthly/klines/BTCUSDT/1m/BTCUSDT-1m-2023-06.zip | wc -l
```

## 模块九：文档

1. 执行路径见 [STEP.md](./STEP.md)。
2. 版本与破坏性变更见 [CHANGELOG.md](./CHANGELOG.md)。
3. 设计取舍见 [docs/adr/](./docs/adr/)。
4. 许可证：MIT。

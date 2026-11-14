# 执行路径

## 模块一：入口

1. 库入口：`read(&Config)`。
2. CLI 入口：`read -c <toml>` 或 `list --root <dir>`。
3. `Config` 只能由 builder 或 TOML 构造，构造时完成全部交叉校验。

```bash
cargo run --release -- read -c config/00.smoke.toml
cargo run --release -- list --root /data/data/binance --dataset klines
```

## 模块二：配置校验

1. 校验 symbol、market、dataset 支持矩阵。
2. kline 家族必须有 interval，其他家族禁止 interval。
3. 校验 interval 在该 market 至少有一个档位发布。
4. 校验日期顺序。

```text
src/config.rs -> ConfigBuilder::build
```

## 模块三：扫描文件

1. 同时扫描 `daily` 与 `monthly`。
2. 只接受 `{symbol}-{key}-{YYYY-MM-DD|YYYY-MM}.zip` 的精确文件名。
3. 名字不匹配、symbol 不匹配立即失败。
4. 按日期窗口剪枝文件，并记录 daily 覆盖的 UTC 日集合。
5. monthly 文件若在窗口内没有任何未被 daily 覆盖的日期，直接不读取。

```text
src/source/files.rs -> scan
```

## 模块四：解码 ZIP 与 CSV

1. 每个 ZIP 必须恰好一个 CSV 入口。
2. 首行若是本数据集 header 则消费；若是其他市场的 header 则文件级失败。
3. 每行先按 schema 解析，再校验时间戳落在文件的日历周期内。
4. `bad_rows = "fail"` 遇到首个坏行停止；`skip` 计数并保留最多 5 条样本。

```text
src/source/zip.rs -> read_csv
src/source/csv.rs -> read_rows
```

## 模块五：合并与过滤

1. 并行读取选中文件，结果按扫描顺序收回。
2. monthly 行若落在 daily 覆盖的 UTC 日，计入 `rows_dropped_overlap` 后丢弃。
3. 再按 `start_date..=end_date` 过滤，计入 `rows_dropped_window`。
4. 时间乱序时按主时间列稳定排序；间隙不填充。

```text
src/dataset/mod.rs -> read_dataset
```

## 模块六：数据集校验与建表

1. K 线：`close_time == next_open - 1`，open_time 严格递增，统计 `gaps`。
2. aggTrades/trades：spot 8/7 列，UM 7/6 列，CM trades 第 4 列命名为 `base_qty`。
3. metrics：symbol 必须匹配配置；空比率保留 `None`。
4. liquidationSnapshot：全行精确重复 keep-first 去重。
5. 最终转成等长 `Table`，列名与归档字段一致。

```text
src/dataset/{klines,trades,book,funding,liquidation,metrics}.rs
```

## 模块七：报告与输出

1. `LoadReport` 记录文件数、解码/重叠/窗口/跳过/去重行数、首末时间、缺口和耗时。
2. 不变式：`rows == rows_daily + rows_monthly - duplicates_removed == table.len()`。
3. CLI 将 `{dataset, columns, report}` 写成 JSON 到 stdout。

```text
src/load.rs -> read
src/report.rs -> LoadReport
src/main.rs -> write_json
```

## 模块八：清单

1. 只读遍历支持矩阵内的目录。
2. 忽略无关文件、非法 interval 目录和非法文件名。
3. 每个 symbol × dataset × interval 输出文件数与首末周期。

```text
src/catalog.rs -> catalog
```

## 模块九：失败与退出码

1. `Error::Config`、`Error::Json` -> 退出码 `2`。
2. 根目录、I/O、ZIP、坏行、数据不变式 -> 退出码 `1`。
3. 错误链打印到 stderr，stdout 不输出半截 JSON。

```text
src/main.rs -> exit_code
```

## 模块十：验收

1. 离线门禁。
2. 真实归档冒烟：期望 43,200 行，首时间 `1685577600000`。
3. 同日重跑保持相同数据（耗时字段会变化，只比较数据）。

```bash
./scripts/check.sh
cargo build --release
./target/release/binance-data-reader read -c config/00.smoke.toml > /tmp/smoke.json
python -c 'import json; d=json.load(open("/tmp/smoke.json")); print(d["report"]["rows"], d["columns"]["open_time"][0])'
./target/release/binance-data-reader read -c config/00.smoke.toml > /tmp/smoke2.json
python -c 'import json; a=json.load(open("/tmp/smoke.json")); b=json.load(open("/tmp/smoke2.json")); assert a["columns"] == b["columns"] and a["report"]["rows"] == b["report"]["rows"]'
```

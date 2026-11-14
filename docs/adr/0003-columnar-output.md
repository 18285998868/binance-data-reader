# ADR 0003: 列式表返回全部归档字段

## 状态

已采纳（0.2.0）。

## 决策

1. 所有数据集统一返回 `Table { columns }`，每列有名字和类型。
2. 列名与归档字段一致，顺序与归档一致。
3. 归档里的字段全部保留，包括旧版丢弃的 K 线扩展字段、spot `is_best_match`、COIN-M `base_qty`、metrics symbol 与可空比率。
4. 空字段用 `Option<f64>` 表示，不强制填 0。
5. JSON 形状为 `{列名: 数组}`；不引入 parquet/arrow。

## 理由

1. 下游按列名取数，不受结构体字段改名影响。
2. 全字段返回让下游自行裁剪，避免 reader 变成有损过滤器。
3. 列式形状与 pandas/polars/arrow 的转换路径直接对应。
4. 保持纯 Rust 依赖，构建简单。

## 代价

1. 大 tick 数据集会一次性占用较多内存；仍建议按日期窗口分批读取。
2. 下游必须自己组合列，不能依赖现成的场景结构体。

## 相关

- `src/table.rs`
- `src/dataset/*.rs`
- `src/main.rs`：JSON 输出

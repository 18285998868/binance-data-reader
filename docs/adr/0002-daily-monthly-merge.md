# ADR 0002: daily 与 monthly 按日历覆盖合并

## 状态

已采纳（0.2.0）。

## 决策

1. 每次读取同时扫描 `daily` 与 `monthly` 两棵树。
2. 记录被选中 daily 文件覆盖的 UTC 日集合。
3. monthly 行的时间戳若落在该集合内，整行丢弃并计入 `rows_dropped_overlap`。
4. 不按时间戳逐行去重；同一时间戳只保留 daily 行。
5. 未被 daily 覆盖的 monthly 行全部保留。

## 理由

1. 下载器常同时保留两种档位；只读一种会漏数据或重复。
2. tick 类数据集一天有多行，按时间戳去重既不充分也不必要；按日历覆盖才是正确粒度。
3. daily 是更细、更新更及时的副本，冲突时优先。

## 代价

1. 必须先把 daily 文件全部选中，才能判断 monthly 行的去留。
2. 报告里 `rows` 不等于 `rows_decoded`，需要同时看 overlap/window 计数。

## 相关

- `src/source/files.rs`：`daily_days`
- `src/dataset/mod.rs`：`read_dataset`

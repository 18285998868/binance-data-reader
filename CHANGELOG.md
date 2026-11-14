# Changelog

All notable changes follow [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and this project adheres to [Semantic Versioning](https://semver.org/).

## [0.2.0] - 2026-11-14

### Added

- Unified `Dataset` enum covering klines, index/mark/premium-price klines,
  aggTrades, trades, bookTicker, bookDepth, fundingRate, liquidationSnapshot
  and metrics.
- Columnar `Table`/`Column` output with one typed column per archive field,
  serialized as `{column: array}` by the CLI and Python bindings.
- Structured `LoadReport`: file counts per cadence, decoded/overlap/window/
  skipped/duplicate row counts, skipped-row samples, first/last time, kline
  gaps and elapsed time.
- `catalog`/`list` inventory command with market, period, dataset and symbol
  filters.
- `BadRowPolicy::{Fail, Skip}`; `fail` is the default.
- `ConfigBuilder`, TOML configuration and typed errors with exit codes 0/1/2.
- Python `read(config)` and `catalog(root, ...)` bindings under the `python`
  feature.
- Validation of file names, ZIP contents, CSV headers, row timestamps against
  the file calendar period, kline `close_time` and OHLC consistency.
- Daily/monthly merge by calendar cover: daily files win for the UTC days they
  cover; monthly rows elsewhere are kept.
- All 16 intervals, including `1s`, `3d`, `1w` and calendar-aware `1mo`.
- Exact-duplicate removal for liquidation snapshots.

### Changed

- `root` now means the downloader's `output_dir` (the directory containing
  `data/`); it no longer needs a `binance/` segment appended.
- Klines are a normal dataset variant instead of a separate API.
- The reader returns every archive field. This adds kline `close_time`,
  `quote_volume`, `count`, `taker_buy_volume`, `taker_buy_quote_volume` and
  `ignore`, spot `is_best_match`, COIN-M `base_qty`, metrics `symbol` and
  nullable long/short ratios.

### Removed

- `load_columns` / `load_kline_columns` and the per-dataset column structs.
- Silent skipping of malformed file names and corrupted archives.

### Migration

- Replace `DataConfig`/`KlineConfig` with `ConfigBuilder` or TOML.
- Read results from `DatasetData.table` by archive column name; there are no
  `KlineColumns`/`DatasetColumns` accessors anymore.
- Python callers must use `read(...)["columns"][name]` instead of the old
  load functions.

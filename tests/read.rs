//! End-to-end reading against synthetic archives that mirror the downloader.

// Fixture setup is test code: panicking there means the fixture is broken.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use binance_data_reader::{BadRowPolicy, Config, Dataset, Error, Interval, Market, read};
use common::{KLINE_HEADER, date, day_start, kline, write_archive};
use zip::write::SimpleFileOptions;

const DAY: i64 = 86_400_000;
const AGG_TRADES_HEADER: &str =
    "agg_trade_id,price,quantity,first_trade_id,last_trade_id,transact_time,is_buyer_maker";
const TRADES_UM_HEADER: &str = "id,price,qty,quote_qty,time,is_buyer_maker";
const TRADES_CM_HEADER: &str = "id,price,qty,base_qty,time,is_buyer_maker";
const BOOK_TICKER_HEADER: &str =
    "update_id,best_bid_price,best_bid_qty,best_ask_price,best_ask_qty,transaction_time,event_time";
const BOOK_DEPTH_HEADER: &str = "timestamp,percentage,depth,notional";
const FUNDING_HEADER: &str = "calc_time,funding_interval_hours,last_funding_rate";
const LIQUIDATION_HEADER: &str = "time,side,order_type,time_in_force,original_quantity,price,average_price,order_status,last_fill_quantity,accumulated_fill_quantity";
const METRICS_HEADER: &str = "create_time,symbol,sum_open_interest,sum_open_interest_value,count_toptrader_long_short_ratio,sum_toptrader_long_short_ratio,count_long_short_ratio,sum_taker_long_short_vol_ratio";

fn dataset_dir(
    root: &Path,
    cadence: &str,
    dataset: &str,
    symbol: &str,
    interval: Option<&str>,
) -> PathBuf {
    let mut dir = root
        .join("data")
        .join("futures/um")
        .join(cadence)
        .join(dataset)
        .join(symbol);
    if let Some(interval) = interval {
        dir.push(interval);
    }
    dir
}

fn write_zip(path: &Path, entries: &[(&str, &str)]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create fixture directory");
    }
    let file = File::create(path).expect("create fixture zip");
    let mut writer = zip::ZipWriter::new(file);
    for (name, contents) in entries {
        writer
            .start_file(*name, SimpleFileOptions::default())
            .expect("start fixture entry");
        writer
            .write_all(contents.as_bytes())
            .expect("write fixture entry");
    }
    writer.finish().expect("finish fixture zip");
}

fn klines(root: &Path) -> Config {
    Config::builder(root, Dataset::Klines, "BTCUSDT")
        .interval(Interval::M1)
        .build()
        .expect("valid kline config")
}

fn write_daily_klines(root: &Path, stem: &str, lines: &str) {
    write_archive(
        root,
        "futures/um",
        "daily",
        "klines",
        "BTCUSDT",
        Some("1m"),
        stem,
        lines,
    );
}

#[test]
fn daily_and_monthly_merge_prefers_daily_cover() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let day = day_start(date(2023, 6, 25));
    let previous = day - 60_000;
    let next = day + DAY;

    let daily = format!(
        "{KLINE_HEADER}\n{}\n{}\n",
        kline(day, Interval::M1, 200.0),
        kline(day + 60_000, Interval::M1, 201.0)
    );
    write_daily_klines(root, "BTCUSDT-1m-2023-06-25", &daily);

    let monthly = format!(
        "{KLINE_HEADER}\n{}\n{}\n{}\n{}\n",
        kline(previous, Interval::M1, 199.0),
        kline(day, Interval::M1, 100.0),
        kline(day + 60_000, Interval::M1, 100.0),
        kline(next, Interval::M1, 202.0)
    );
    write_archive(
        root,
        "futures/um",
        "monthly",
        "klines",
        "BTCUSDT",
        Some("1m"),
        "BTCUSDT-1m-2023-06",
        &monthly,
    );

    let data = read(&klines(root)).expect("read merged klines");
    let report = &data.report;
    assert_eq!(report.files_daily, 1);
    assert_eq!(report.files_monthly, 1);
    assert_eq!(report.rows_decoded, 6);
    assert_eq!(report.rows_dropped_overlap, 2);
    assert_eq!(report.rows_dropped_window, 0);
    assert_eq!(report.rows_daily, 2);
    assert_eq!(report.rows_monthly, 2);
    assert_eq!(report.rows, 4);
    assert_eq!(report.duplicates_removed, 0);
    assert_eq!(report.gaps, 1);
    assert_eq!(report.first_time, Some(previous));
    assert_eq!(report.last_time, Some(next));

    let names: Vec<&str> = data
        .table
        .columns()
        .iter()
        .map(binance_data_reader::Column::name)
        .collect();
    assert_eq!(
        names,
        [
            "open_time",
            "open",
            "high",
            "low",
            "close",
            "volume",
            "close_time",
            "quote_volume",
            "count",
            "taker_buy_volume",
            "taker_buy_quote_volume",
            "ignore"
        ]
    );

    let opens = data.table.i64("open_time").expect("open_time column");
    assert_eq!(opens, &[previous, day, day + 60_000, next][..]);
    // The daily row wins for the UTC day it covers.
    assert_eq!(data.table.f64("close").expect("close column")[1], 200.0);
    assert_eq!(
        data.table.i64("count").expect("count column"),
        &[7, 7, 7, 7]
    );
    assert_eq!(
        data.table.f64("quote_volume").expect("quote column")[0],
        10.5
    );
}

#[test]
fn fully_covered_monthly_archives_are_not_read() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let day = date(2023, 6, 25);
    let daily = format!(
        "{KLINE_HEADER}\n{}\n",
        kline(day_start(day), Interval::M1, 100.0)
    );
    write_archive(
        root,
        "futures/um",
        "daily",
        "klines",
        "BTCUSDT",
        Some("1m"),
        "BTCUSDT-1m-2023-06-25",
        &daily,
    );
    let monthly_dir = root.join("data/futures/um/monthly/klines/BTCUSDT/1m");
    fs::create_dir_all(&monthly_dir).expect("create monthly dir");
    fs::write(
        monthly_dir.join("BTCUSDT-1m-2023-06.zip"),
        b"not a zip and never read",
    )
    .expect("write corrupt monthly zip");

    let config = Config::builder(root, Dataset::Klines, "BTCUSDT")
        .interval(Interval::M1)
        .date_range(Some(day), Some(day))
        .build()
        .expect("valid config");
    let data = read(&config).expect("covered monthly file must be skipped");
    assert_eq!(data.report.rows, 1);
    assert_eq!(data.report.files_daily, 1);
    assert_eq!(data.report.files_monthly, 0);
}

#[test]
fn monthly_rows_outside_the_window_are_dropped() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let monthly = format!(
        "{KLINE_HEADER}\n{}\n{}\n{}\n",
        kline(day_start(date(2023, 6, 24)), Interval::M1, 100.0),
        kline(day_start(date(2023, 6, 25)), Interval::M1, 101.0),
        kline(day_start(date(2023, 6, 26)), Interval::M1, 102.0)
    );
    write_archive(
        root,
        "futures/um",
        "monthly",
        "klines",
        "BTCUSDT",
        Some("1m"),
        "BTCUSDT-1m-2023-06",
        &monthly,
    );

    let config = Config::builder(root, Dataset::Klines, "BTCUSDT")
        .interval(Interval::M1)
        .date_range(Some(date(2023, 6, 25)), Some(date(2023, 6, 25)))
        .build()
        .expect("valid config");
    let data = read(&config).expect("read windowed klines");
    assert_eq!(data.report.rows, 1);
    assert_eq!(data.report.rows_dropped_window, 2);
    assert_eq!(data.report.files_daily, 0);
    assert_eq!(data.report.files_monthly, 1);
    assert_eq!(
        data.table.i64("open_time").expect("open_time column"),
        &[day_start(date(2023, 6, 25))][..]
    );
}

#[test]
fn malformed_rows_fail_or_are_skipped_with_samples() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let csv = format!(
        "{AGG_TRADES_HEADER}\n1,100.0,0.5,1,1,{0},false\n2,not-a-price,0.5,2,2,{1},false\n3,101.0,0.5,3,3,{2},false\n",
        day_start(date(2023, 6, 25)),
        day_start(date(2023, 6, 25)) + 1,
        day_start(date(2023, 6, 25)) + 2
    );
    write_archive(
        root,
        "futures/um",
        "daily",
        "aggTrades",
        "BTCUSDT",
        None,
        "BTCUSDT-aggTrades-2023-06-25",
        &csv,
    );

    let strict = Config::builder(root, Dataset::AggTrades, "BTCUSDT")
        .build()
        .expect("valid config");
    let error = read(&strict).expect_err("strict policy must fail");
    let Error::BadRow { line, detail, .. } = &error else {
        panic!("expected a row error, got {error}");
    };
    assert_eq!(*line, 3);
    assert!(detail.contains("price"), "{detail}");

    let lenient = Config::builder(root, Dataset::AggTrades, "BTCUSDT")
        .bad_rows(BadRowPolicy::Skip)
        .build()
        .expect("valid config");
    let data = read(&lenient).expect("skip policy keeps good rows");
    assert_eq!(data.report.rows, 2);
    assert_eq!(data.report.rows_skipped, 1);
    assert_eq!(data.report.rows_decoded, 2);
    assert_eq!(data.report.skipped_samples.len(), 1);
    assert!(
        data.report.skipped_samples[0].contains(":3: "),
        "{:?}",
        data.report.skipped_samples
    );
}

#[test]
fn corrupt_archives_are_fatal() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let dir = dataset_dir(root, "daily", "klines", "BTCUSDT", Some("1m"));
    fs::create_dir_all(&dir).expect("create dir");
    fs::write(dir.join("BTCUSDT-1m-2023-06-25.zip"), b"not a zip").expect("write corrupt zip");
    let error = read(&klines(root)).expect_err("corrupt zip must fail");
    assert!(matches!(error, Error::File { .. }), "{error}");
    assert!(error.to_string().contains("invalid ZIP"), "{error}");
}

#[test]
fn archives_must_contain_exactly_one_csv() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let dir = dataset_dir(root, "daily", "klines", "BTCUSDT", Some("1m"));
    fs::create_dir_all(&dir).expect("create dir");

    write_zip(
        &dir.join("BTCUSDT-1m-2023-06-24.zip"),
        &[("data.txt", "not csv")],
    );
    let error = read(&klines(root)).expect_err("missing CSV must fail");
    assert!(error.to_string().contains("no CSV entry"), "{error}");

    fs::remove_file(dir.join("BTCUSDT-1m-2023-06-24.zip")).expect("remove zip");
    write_zip(
        &dir.join("BTCUSDT-1m-2023-06-25.zip"),
        &[("a.csv", KLINE_HEADER), ("b.csv", KLINE_HEADER)],
    );
    let error = read(&klines(root)).expect_err("two CSVs must fail");
    assert!(error.to_string().contains("multiple CSV"), "{error}");
}

#[test]
fn malformed_and_foreign_file_names_are_fatal() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let dir = dataset_dir(root, "daily", "klines", "BTCUSDT", Some("1m"));
    fs::create_dir_all(&dir).expect("create dir");

    fs::write(dir.join("BTCUSDT-1m-latest.zip"), b"x").expect("write zip");
    let error = read(&klines(root)).expect_err("malformed name must fail");
    assert!(matches!(error, Error::File { .. }), "{error}");

    fs::remove_file(dir.join("BTCUSDT-1m-latest.zip")).expect("remove zip");
    fs::write(dir.join("ETHUSDT-1m-2023-06-25.zip"), b"x").expect("write zip");
    let error = read(&klines(root)).expect_err("wrong symbol must fail");
    assert!(error.to_string().contains("wrong file"), "{error}");
}

#[test]
fn foreign_headers_are_rejected_before_rows_are_read() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    // Spot aggTrades carry the extra is_best_match column.
    let csv = "agg_trade_id,price,quantity,first_trade_id,last_trade_id,transact_time,is_buyer_maker,is_best_match\n";
    write_archive(
        root,
        "futures/um",
        "daily",
        "aggTrades",
        "BTCUSDT",
        None,
        "BTCUSDT-aggTrades-2023-06-25",
        csv,
    );
    let config = Config::builder(root, Dataset::AggTrades, "BTCUSDT")
        .build()
        .expect("valid config");
    let error = read(&config).expect_err("foreign header must fail");
    assert!(matches!(error, Error::File { .. }), "{error}");
    assert!(error.to_string().contains("different market"), "{error}");
}

#[test]
fn rows_outside_the_file_period_are_row_errors() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let july = day_start(date(2023, 7, 1));
    let csv = format!("{KLINE_HEADER}\n{}\n", kline(july, Interval::M1, 100.0));
    write_daily_klines(root, "BTCUSDT-1m-2023-06-25", &csv);

    let error = read(&klines(root)).expect_err("misplaced row must fail");
    assert!(matches!(error, Error::BadRow { .. }), "{error}");
    assert!(error.to_string().contains("outside file period"), "{error}");

    let lenient = Config::builder(root, Dataset::Klines, "BTCUSDT")
        .interval(Interval::M1)
        .bad_rows(BadRowPolicy::Skip)
        .build()
        .expect("valid config");
    let data = read(&lenient).expect("skipped misplaced row");
    assert_eq!(data.report.rows, 0);
    assert_eq!(data.report.rows_skipped, 1);
}

#[test]
fn every_interval_accepts_its_archive_close_time() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();

    for interval in Interval::ALL {
        let (cadence, day) =
            if interval.available_for(Market::Spot, binance_data_reader::Period::Daily) {
                ("daily", date(2023, 6, 25))
            } else {
                ("monthly", date(2023, 6, 1))
            };
        let open = day_start(day);
        let stem = format!("BTCUSDT-{interval}-{}", cadence_stem_suffix(cadence, day));
        let csv = format!("{}\n{}\n", KLINE_HEADER, kline(open, interval, 100.0));
        write_archive(
            root,
            "spot",
            cadence,
            "klines",
            "BTCUSDT",
            Some(interval.as_str()),
            &stem,
            &csv,
        );

        let config = Config::builder(root, Dataset::Klines, "BTCUSDT")
            .market(Market::Spot)
            .interval(interval)
            .build()
            .expect("valid config");
        let data = read(&config).unwrap_or_else(|error| panic!("interval {interval}: {error}"));
        assert_eq!(data.report.rows, 1, "interval {interval}");
        assert_eq!(data.report.gaps, 0, "interval {interval}");
        assert_eq!(
            data.table.i64("open_time").expect("open_time column"),
            &[open][..],
            "interval {interval}"
        );
    }
}

fn cadence_stem_suffix(cadence: &str, day: chrono::NaiveDate) -> String {
    if cadence == "daily" {
        day.format("%Y-%m-%d").to_string()
    } else {
        day.format("%Y-%m").to_string()
    }
}

#[test]
fn market_specific_trade_columns_are_preserved() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();

    write_archive(
        root,
        "futures/um",
        "daily",
        "aggTrades",
        "BTCUSDT",
        None,
        "BTCUSDT-aggTrades-2023-06-25",
        &format!(
            "{AGG_TRADES_HEADER}\n1,100.0,0.5,1,1,{},false\n",
            day_start(date(2023, 6, 25))
        ),
    );
    let um = Config::builder(root, Dataset::AggTrades, "BTCUSDT")
        .build()
        .expect("valid config");
    let data = read(&um).expect("read USD-M aggTrades");
    assert!(data.table.column("is_best_match").is_none());

    write_archive(
        root,
        "spot",
        "daily",
        "aggTrades",
        "BTCUSDT",
        None,
        "BTCUSDT-aggTrades-2023-06-25",
        &format!(
            "1,100.0,0.5,1,1,{},false,true\n",
            day_start(date(2023, 6, 25))
        ),
    );
    let spot = Config::builder(root, Dataset::AggTrades, "BTCUSDT")
        .market(Market::Spot)
        .build()
        .expect("valid config");
    let data = read(&spot).expect("read spot aggTrades");
    assert_eq!(data.table.bool("is_best_match"), Some(&[true][..]));

    write_archive(
        root,
        "futures/cm",
        "daily",
        "trades",
        "BTCUSDT",
        None,
        "BTCUSDT-trades-2023-06-25",
        &format!(
            "{TRADES_CM_HEADER}\n7,27000.0,1.5,0.0000555,{},false\n",
            day_start(date(2023, 6, 25))
        ),
    );
    let cm = Config::builder(root, Dataset::Trades, "BTCUSDT")
        .market(Market::Cm)
        .build()
        .expect("valid config");
    let data = read(&cm).expect("read COIN-M trades");
    assert_eq!(
        data.table.f64("base_qty").expect("base_qty column"),
        &[0.0000555][..]
    );
    assert!(data.table.column("quote_qty").is_none());

    write_archive(
        root,
        "futures/um",
        "daily",
        "trades",
        "BTCUSDT",
        None,
        "BTCUSDT-trades-2023-06-25",
        &format!(
            "{TRADES_UM_HEADER}\n8,27000.0,1.5,40500.0,{},true\n",
            day_start(date(2023, 6, 25))
        ),
    );
    let um = Config::builder(root, Dataset::Trades, "BTCUSDT")
        .build()
        .expect("valid config");
    let data = read(&um).expect("read USD-M trades");
    assert_eq!(
        data.table.f64("quote_qty").expect("quote_qty column"),
        &[40500.0][..]
    );
}

#[test]
fn metrics_keep_symbol_and_nullable_ratios() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write_archive(
        root,
        "futures/um",
        "daily",
        "metrics",
        "BTCUSDT",
        None,
        "BTCUSDT-metrics-2023-06-25",
        &format!("{METRICS_HEADER}\n2023-06-25 00:00:00,BTCUSDT,100.5,200.5,,0.98,,1.1\n"),
    );
    let config = Config::builder(root, Dataset::Metrics, "BTCUSDT")
        .build()
        .expect("valid config");
    let data = read(&config).expect("read metrics");
    assert_eq!(
        data.table.strings("symbol").expect("symbol column"),
        &["BTCUSDT".to_owned()][..]
    );
    assert_eq!(
        data.table
            .optional_f64("count_toptrader_long_short_ratio")
            .expect("ratio column"),
        &[None][..]
    );
    assert_eq!(
        data.table
            .optional_f64("sum_toptrader_long_short_ratio")
            .expect("ratio column"),
        &[Some(0.98)][..]
    );
}

#[test]
fn liquidation_duplicates_are_removed_keep_first() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let row = "1687680000000,SELL,LIMIT,IOC,1.5,27000,27000,FILLED,1.5,1.5";
    write_archive(
        root,
        "futures/cm",
        "daily",
        "liquidationSnapshot",
        "BTCUSDT",
        None,
        "BTCUSDT-liquidationSnapshot-2023-06-25",
        &format!("{LIQUIDATION_HEADER}\n{row}\n{row}\n"),
    );
    let config = Config::builder(root, Dataset::LiquidationSnapshot, "BTCUSDT")
        .market(Market::Cm)
        .build()
        .expect("valid config");
    let data = read(&config).expect("read liquidations");
    assert_eq!(data.report.rows_decoded, 2);
    assert_eq!(data.report.duplicates_removed, 1);
    assert_eq!(data.report.rows, 1);
    assert_eq!(
        data.table.strings("side").expect("side column"),
        &["SELL".to_owned()][..]
    );
}

#[test]
fn funding_rates_are_read_from_monthly_archives() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write_archive(
        root,
        "futures/um",
        "monthly",
        "fundingRate",
        "BTCUSDT",
        None,
        "BTCUSDT-fundingRate-2023-06",
        &format!("{FUNDING_HEADER}\n1688083200000,4,-0.00056702\n1685577600000,8,0.0001\n"),
    );
    let config = Config::builder(root, Dataset::FundingRate, "BTCUSDT")
        .build()
        .expect("valid config");
    let data = read(&config).expect("read funding rates");
    assert_eq!(data.report.rows, 2);
    assert_eq!(
        data.table.f64("last_funding_rate").expect("rate column"),
        &[0.0001, -0.00056702][..]
    );
}

#[test]
fn book_ticker_and_depth_preserve_archive_columns() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let event = day_start(date(2023, 6, 25));
    write_archive(
        root,
        "futures/um",
        "daily",
        "bookTicker",
        "BTCUSDT",
        None,
        "BTCUSDT-bookTicker-2023-06-25",
        &format!("{BOOK_TICKER_HEADER}\n1,100.0,1.0,101.0,1.0,{event},{event}\n"),
    );
    write_archive(
        root,
        "futures/um",
        "daily",
        "bookDepth",
        "BTCUSDT",
        None,
        "BTCUSDT-bookDepth-2023-06-25",
        &format!("{BOOK_DEPTH_HEADER}\n2023-06-25 00:00:00,1,10.5,1050.0\n"),
    );

    let ticker = Config::builder(root, Dataset::BookTicker, "BTCUSDT")
        .build()
        .expect("valid config");
    let data = read(&ticker).expect("read bookTicker");
    assert_eq!(data.report.rows, 1);
    assert_eq!(data.table.i64("update_id").expect("update_id"), &[1][..]);

    let depth = Config::builder(root, Dataset::BookDepth, "BTCUSDT")
        .build()
        .expect("valid config");
    let data = read(&depth).expect("read bookDepth");
    assert_eq!(data.report.rows, 1);
    assert_eq!(data.table.i64("percentage").expect("percentage"), &[1][..]);
}

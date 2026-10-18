use std::path::Path;

use tempfile::TempDir;

use kline_reader::{DataConfig, DataSource, load_columns};

fn cfg(
    source: DataSource,
    dir: std::path::PathBuf,
    symbol: &str,
    tf: &str,
    start: Option<&str>,
    end: Option<&str>,
) -> DataConfig {
    DataConfig::new(source, &dir, symbol, tf, start, end).unwrap()
}

mod common;

use common::create_test_zip;

#[test]
fn integration_global_sort_when_filename_order_conflicts() {
    let tmp = TempDir::new().unwrap();
    let data_dir = tmp.path().join("data");
    let kline_dir = data_dir
        .join("binance/data/um/daily/klines")
        .join("T")
        .join("1m");
    std::fs::create_dir_all(&kline_dir).unwrap();

    let csv_a = "1704067200000,11.0,12.0,10.0,11.5,600.0,0,0,0,0,0,0\n";
    create_test_zip(&kline_dir, "T-1m-2024-01-01.zip", csv_a);
    let csv_b = "1704067140000,10.0,11.0,9.0,10.5,500.0,0,0,0,0,0,0\n";
    create_test_zip(&kline_dir, "U-1m-2024-01-01.zip", csv_b);

    let spec = cfg(DataSource::Binance, data_dir, "T", "1m", None, None);
    let cols = load_columns(&spec).unwrap();
    assert_eq!(cols.len(), 2);
    assert_eq!(cols.time()[0], 1704067140000);
    assert_eq!(cols.time()[1], 1704067200000);
}

#[test]
fn integration_load_multiple_days() {
    let tmp = TempDir::new().unwrap();
    let data_dir = tmp.path().join("data");
    let kline_dir = data_dir
        .join("binance/data/um/daily/klines")
        .join("BTCUSDT")
        .join("1m");
    std::fs::create_dir_all(&kline_dir).unwrap();

    let csv1 = "1704067200000,42314.0,42350.0,42300.0,42331.9,289.641,1704067200000,0,0,0,0,0\n";
    create_test_zip(&kline_dir, "BTCUSDT-1m-2024-01-01.zip", csv1);

    let csv2 = "open_time,open,high,low,close,volume,close_time,quote_volume,count,taker_buy_volume,taker_buy_quote_volume,ignore\n1704153600000,42500.0,42600.0,42450.0,42550.0,300.0,1704153600000,0,0,0,0,0\n";
    create_test_zip(&kline_dir, "BTCUSDT-1m-2024-01-02.zip", csv2);

    let csv3 = "open_time,open,high,low,close,volume,close_time,quote_volume,count,taker_buy_volume,taker_buy_quote_volume,ignore\n1704240000000,43000.0,43100.0,42900.0,43050.0,310.0,1704240000000,0,0,0,0,0\n";
    create_test_zip(&kline_dir, "BTCUSDT-1m-2024-01-03.zip", csv3);

    std::fs::write(kline_dir.join("readme.txt"), b"hello").unwrap();

    let spec = cfg(
        DataSource::Binance,
        data_dir,
        "BTCUSDT",
        "1m",
        Some("2024-01-01"),
        Some("2024-01-02"),
    );
    let cols = load_columns(&spec).unwrap();
    assert_eq!(cols.len(), 2);
    assert_eq!(cols.time()[0], 1704067200000);
    assert_eq!(cols.time()[1], 1704153600000);
    assert!((cols.close()[0] - 42331.9).abs() < 0.001);
    assert!((cols.close()[1] - 42550.0).abs() < 0.001);
}

#[test]
fn integration_load_empty_range() {
    let tmp = TempDir::new().unwrap();
    let data_dir = tmp.path().join("data");
    let kline_dir = data_dir
        .join("binance/data/um/daily/klines")
        .join("ETHUSDT")
        .join("1m");
    std::fs::create_dir_all(&kline_dir).unwrap();

    let csv = "1704067200000,2200.0,2210.0,2190.0,2195.0,500.0,0,0,0,0,0,0\n";
    create_test_zip(&kline_dir, "ETHUSDT-1m-2024-01-01.zip", csv);

    let spec = cfg(
        DataSource::Binance,
        data_dir,
        "ETHUSDT",
        "1m",
        Some("2025-01-01"),
        None,
    );
    let cols = load_columns(&spec).unwrap();
    assert!(cols.is_empty());
}

#[test]
fn integration_load_missing_directory() {
    let spec = cfg(
        DataSource::Binance,
        Path::new("/tmp/nonexistent_reader_test").to_path_buf(),
        "X",
        "1m",
        None,
        None,
    );
    let result = load_columns(&spec);
    assert!(result.is_err());
    match result.unwrap_err() {
        kline_reader::ReaderError::Io(_) => {}
        e => panic!("expected Io error, got: {e}"),
    }
}

#[test]
fn integration_all_data_when_no_date_filter() {
    let tmp = TempDir::new().unwrap();
    let data_dir = tmp.path().join("data");
    let kline_dir = data_dir
        .join("binance/data/um/daily/klines")
        .join("SOLUSDT")
        .join("1m");
    std::fs::create_dir_all(&kline_dir).unwrap();

    let csv1 = "1704067200000,100.0,101.0,99.0,100.5,1000.0,0,0,0,0,0,0\n1704067260000,100.5,102.0,100.0,101.5,800.0,0,0,0,0,0,0\n";
    create_test_zip(&kline_dir, "SOLUSDT-1m-2024-01-01.zip", csv1);
    let csv2 = "1704153600000,102.0,103.0,101.0,102.5,900.0,0,0,0,0,0,0\n";
    create_test_zip(&kline_dir, "SOLUSDT-1m-2024-01-02.zip", csv2);

    let spec = cfg(DataSource::Binance, data_dir, "SOLUSDT", "1m", None, None);
    let cols = load_columns(&spec).unwrap();
    assert_eq!(cols.len(), 3);
    assert_eq!(cols.time()[0], 1704067200000);
    assert_eq!(cols.time()[1], 1704067260000);
    assert_eq!(cols.time()[2], 1704153600000);
}

#[test]
fn integration_full_flow_toml_to_columns() {
    let tmp = TempDir::new().unwrap();
    let kline_dir = tmp
        .path()
        .join("binance/data/um/daily/klines")
        .join("ETHUSDT")
        .join("1h");
    std::fs::create_dir_all(&kline_dir).unwrap();

    let csv = "1704067200000,3000.0,3010.0,2990.0,3005.0,500.0,0,0,0,0,0,0\n1704070800000,3005.0,3020.0,2995.0,3015.0,600.0,0,0,0,0,0,0\n";
    create_test_zip(&kline_dir, "ETHUSDT-1h-2024-01-01.zip", csv);

    let toml_str = format!(
        r#"
type = "binance"
dir = "{}"
symbol = "ETHUSDT"
timeframe = "1h"
"#,
        tmp.path().to_str().unwrap()
    );

    let cfg: DataConfig = toml::from_str(&toml_str).unwrap();
    let cols = load_columns(&cfg).unwrap();
    assert_eq!(cols.len(), 2);
    assert_eq!(cols.time()[0], 1704067200000);
    assert!((cols.close()[0] - 3005.0).abs() < 0.001);
    assert_eq!(cols.time()[1], 1704070800000);
    assert!((cols.close()[1] - 3015.0).abs() < 0.001);
}

#[test]
fn integration_full_flow_toml_with_date_filter() {
    let tmp = TempDir::new().unwrap();
    let kline_dir = tmp
        .path()
        .join("binance/data/um/daily/klines")
        .join("XRPUSDT")
        .join("1d");
    std::fs::create_dir_all(&kline_dir).unwrap();

    let csv1 = "1704067200000,1.0,1.1,0.9,1.05,1000.0,0,0,0,0,0,0\n";
    create_test_zip(&kline_dir, "XRPUSDT-1d-2024-01-01.zip", csv1);
    let csv2 = "1704153600000,1.05,1.15,1.0,1.10,800.0,0,0,0,0,0,0\n";
    create_test_zip(&kline_dir, "XRPUSDT-1d-2024-01-02.zip", csv2);

    let toml_str = format!(
        r#"
type = "binance"
dir = "{}"
symbol = "XRPUSDT"
timeframe = "1d"
"#,
        tmp.path().to_str().unwrap()
    );

    let cfg: DataConfig = toml::from_str(&toml_str).unwrap();
    let cols = load_columns(&cfg).unwrap();
    assert_eq!(cols.len(), 2);
    assert_eq!(cols.time()[0], 1704067200000);
    assert_eq!(cols.time()[1], 1704153600000);
}

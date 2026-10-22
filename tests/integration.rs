//! End-to-end archive loading tests using synthetic ZIP files.

use std::path::{Path, PathBuf};

use tempfile::TempDir;

use binance_reader::{DataConfig, Dataset, Market, ReaderError, load_columns};

mod common;

use common::{create_test_zip, create_test_zip_entry};

const KLINE_HEADER: &str = "open_time,open,high,low,close,volume,close_time,quote_volume,count,taker_buy_volume,taker_buy_quote_volume,ignore";

fn data_root(tmp: &TempDir) -> PathBuf {
    tmp.path().join("data")
}

fn archive_dir(
    root: &Path,
    market: &str,
    period: &str,
    dataset: &str,
    symbol: &str,
    interval: Option<&str>,
) -> PathBuf {
    let mut dir = root
        .join("binance")
        .join("data")
        .join(market)
        .join(period)
        .join(dataset)
        .join(symbol);
    if let Some(interval) = interval {
        dir = dir.join(interval);
    }
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn ms(date: &str, hour: u32, minute: u32) -> i64 {
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .unwrap()
        .and_hms_opt(hour, minute, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis()
}

#[test]
fn premium_index_keeps_negative_values_and_sorts() {
    let tmp = TempDir::new().unwrap();
    let root = data_root(&tmp);
    let dir = archive_dir(
        &root,
        "futures/um",
        "daily",
        "premiumIndexKlines",
        "X",
        Some("4h"),
    );
    create_test_zip(
        &dir,
        "X-4h-2023-11-23.zip",
        &format!(
            "{KLINE_HEADER}\n{},0.0015,0.0116,-0.0129,0.0003,0,0,0,0,0,0,0\n",
            ms("2023-11-23", 0, 0)
        ),
    );
    create_test_zip(
        &dir,
        "X-4h-2023-11-22.zip",
        &format!(
            "{KLINE_HEADER}\n{},0,0.0153,-0.0139,0.0017,0,0,0,0,0,0,0\n",
            ms("2023-11-22", 0, 0)
        ),
    );

    let cfg = DataConfig::new(
        Dataset::PremiumIndexKlines,
        &root,
        "X",
        Some("4h"),
        None,
        None,
    )
    .unwrap();
    let columns = load_columns(&cfg).unwrap();

    assert_eq!(columns.dataset(), Dataset::PremiumIndexKlines);
    assert_eq!(columns.len(), 2);
    assert_eq!(columns.time()[0], ms("2023-11-22", 0, 0));
    assert_eq!(columns.time()[1], ms("2023-11-23", 0, 0));
    let klines = columns.as_klines().unwrap();
    assert!(klines.low()[0] < 0.0);
    assert_eq!(klines.dataset(), Dataset::PremiumIndexKlines);
}

#[test]
fn metrics_string_time_optional_ratios_and_date_filter() {
    let tmp = TempDir::new().unwrap();
    let root = data_root(&tmp);
    let dir = archive_dir(&root, "futures/um", "daily", "metrics", "X", None);
    create_test_zip(
        &dir,
        "X-metrics-2024-01-01.zip",
        "create_time,symbol,sum_open_interest,sum_open_interest_value,count_toptrader_long_short_ratio,sum_toptrader_long_short_ratio,count_long_short_ratio,sum_taker_long_short_vol_ratio\n\
         2024-01-01 00:00:00,X,8796805.0,20799.14,,,,1.44124606\n\
         2024-01-01 00:05:00,X,8822260.0,20798.02,1.5,0.7,2.0,2.75\n",
    );
    create_test_zip(
        &dir,
        "X-metrics-2024-01-02.zip",
        "create_time,symbol,sum_open_interest,sum_open_interest_value,count_toptrader_long_short_ratio,sum_toptrader_long_short_ratio,count_long_short_ratio,sum_taker_long_short_vol_ratio\n\
         2024-01-02 00:00:00,X,8900000.0,21000.0,1.2,0.6,1.8,1.0\n",
    );

    let cfg = DataConfig::new(
        Dataset::Metrics,
        &root,
        "X",
        None,
        Some("2024-01-01"),
        Some("2024-01-01"),
    )
    .unwrap();
    let columns = load_columns(&cfg).unwrap();
    let metrics = columns.as_metrics().unwrap();
    assert_eq!(metrics.len(), 2);
    assert_eq!(metrics.time()[0], ms("2024-01-01", 0, 0));
    assert_eq!(
        metrics.count_toptrader_long_short_ratio(),
        &[None, Some(1.5)]
    );
    assert_eq!(metrics.count_long_short_ratio(), &[None, Some(2.0)]);
}

#[test]
fn metrics_symbol_mismatch_rows_are_skipped() {
    let tmp = TempDir::new().unwrap();
    let root = data_root(&tmp);
    let dir = archive_dir(&root, "futures/um", "daily", "metrics", "X", None);
    create_test_zip(
        &dir,
        "X-metrics-2024-01-01.zip",
        "create_time,symbol,sum_open_interest,sum_open_interest_value,count_toptrader_long_short_ratio,sum_toptrader_long_short_ratio,count_long_short_ratio,sum_taker_long_short_vol_ratio\n\
         2024-01-01 00:00:00,X,1,2,3,4,5,6\n\
         2024-01-01 00:05:00,Y,1,2,3,4,5,6\n",
    );

    let cfg = DataConfig::new(
        Dataset::Metrics,
        &root,
        "X",
        None,
        Some("2024-01-01"),
        Some("2024-01-01"),
    )
    .unwrap();
    let columns = load_columns(&cfg).unwrap();
    let metrics = columns.as_metrics().unwrap();
    assert_eq!(metrics.len(), 1);
    assert_eq!(metrics.time(), &[ms("2024-01-01", 0, 0)]);
}

#[test]
fn funding_rate_monthly_path_and_signed_rate() {
    let tmp = TempDir::new().unwrap();
    let root = data_root(&tmp);
    let dir = archive_dir(&root, "futures/um", "monthly", "fundingRate", "X", None);
    create_test_zip(
        &dir,
        "X-fundingRate-2024-01.zip",
        "calc_time,funding_interval_hours,last_funding_rate\n\
         1704067200000,8,-0.00037711\n\
         1704096000000,8,0.00020162\n",
    );

    let cfg = DataConfig::new(Dataset::FundingRate, &root, "X", None, None, None).unwrap();
    let columns = load_columns(&cfg).unwrap();
    let funding = columns.as_funding_rate().unwrap();
    assert_eq!(funding.len(), 2);
    assert!(funding.last_funding_rate()[0] < 0.0);
    assert_eq!(funding.funding_interval_hours(), &[8, 8]);
}

#[test]
fn index_symbol_strips_contract_suffix() {
    let tmp = TempDir::new().unwrap();
    let root = data_root(&tmp);
    let dir = archive_dir(
        &root,
        "futures/cm",
        "daily",
        "indexPriceKlines",
        "BTCUSD",
        Some("1m"),
    );
    create_test_zip(
        &dir,
        "BTCUSD-1m-2024-01-01.zip",
        &format!(
            "{KLINE_HEADER}\n{},42000.0,42010.0,41990.0,42005.0,0,0,0,0,0,0,0\n",
            ms("2024-01-01", 0, 0)
        ),
    );

    let cfg = DataConfig::new_with_market(
        Dataset::IndexPriceKlines,
        Market::Cm,
        &root,
        "BTCUSD_PERP",
        Some("1m"),
        None,
        None,
    )
    .unwrap();
    let columns = load_columns(&cfg).unwrap();
    assert_eq!(columns.len(), 1);
}

#[test]
fn cm_trades_use_base_qty() {
    let tmp = TempDir::new().unwrap();
    let root = data_root(&tmp);
    let dir = archive_dir(&root, "futures/cm", "daily", "trades", "BTCUSD_PERP", None);
    create_test_zip(
        &dir,
        "BTCUSD_PERP-trades-2024-01-01.zip",
        "id,price,qty,base_qty,time,is_buyer_maker\n\
         704644606,42305.5,3.0,0.00709127,1704067202014,false\n",
    );

    let cfg = DataConfig::new_with_market(
        Dataset::Trades,
        Market::Cm,
        &root,
        "BTCUSD_PERP",
        None,
        None,
        None,
    )
    .unwrap();
    let columns = load_columns(&cfg).unwrap();
    let trades = columns.as_trades().unwrap();
    assert!(trades.quote_qty().is_none());
    assert!((trades.base_qty().unwrap()[0] - 0.00709127).abs() < 1e-12);
}

#[test]
fn spot_agg_trades_are_headerless_with_extra_column() {
    let tmp = TempDir::new().unwrap();
    let root = data_root(&tmp);
    let dir = archive_dir(&root, "spot", "daily", "aggTrades", "BNBUSDT", None);
    create_test_zip(
        &dir,
        "BNBUSDT-aggTrades-2024-01-01.zip",
        "515330094,311.9,0.022,697594291,697594291,1704067200107,False,True\n\
         515330095,311.9,1.349,697594292,697594292,1704067200210,False,True\n",
    );

    let cfg = DataConfig::new_with_market(
        Dataset::AggTrades,
        Market::Spot,
        &root,
        "BNBUSDT",
        None,
        None,
        None,
    )
    .unwrap();
    let columns = load_columns(&cfg).unwrap();
    let agg = columns.as_agg_trades().unwrap();
    assert_eq!(agg.len(), 2);
    assert_eq!(agg.agg_trade_id(), &[515_330_094, 515_330_095]);
    assert_eq!(agg.is_buyer_maker(), &[false, false]);
}

#[test]
fn spot_trades_are_headerless_with_quote_qty() {
    let tmp = TempDir::new().unwrap();
    let root = data_root(&tmp);
    let dir = archive_dir(&root, "spot", "daily", "trades", "BNBUSDT", None);
    create_test_zip(
        &dir,
        "BNBUSDT-trades-2024-01-01.zip",
        "697594291,311.9,0.022,6.8618,1704067200107,False,True\n",
    );

    let cfg = DataConfig::new_with_market(
        Dataset::Trades,
        Market::Spot,
        &root,
        "BNBUSDT",
        None,
        None,
        None,
    )
    .unwrap();
    let columns = load_columns(&cfg).unwrap();
    let trades = columns.as_trades().unwrap();
    assert_eq!(trades.quote_qty().unwrap(), &[6.8618]);
    assert!(trades.base_qty().is_none());
}

#[test]
fn um_agg_trades_use_headered_layout() {
    let tmp = TempDir::new().unwrap();
    let root = data_root(&tmp);
    let dir = archive_dir(&root, "futures/um", "daily", "aggTrades", "X", None);
    create_test_zip(
        &dir,
        "X-aggTrades-2024-01-01.zip",
        "agg_trade_id,price,quantity,first_trade_id,last_trade_id,transact_time,is_buyer_maker\n\
         1,0.00478,10460.0,1,1,1704067200000,false\n",
    );

    let cfg = DataConfig::new(Dataset::AggTrades, &root, "X", None, None, None).unwrap();
    let columns = load_columns(&cfg).unwrap();
    let agg = columns.as_agg_trades().unwrap();
    assert_eq!(agg.agg_trade_id(), &[1]);
    assert_eq!(agg.quantity(), &[10460.0]);
}

#[test]
fn zip_without_csv_fails_batch() {
    let tmp = TempDir::new().unwrap();
    let root = data_root(&tmp);
    let dir = archive_dir(&root, "futures/um", "daily", "metrics", "X", None);
    create_test_zip_entry(&dir, "X-metrics-2024-01-01.zip", "readme.txt", "not a csv");

    let cfg = DataConfig::new(Dataset::Metrics, &root, "X", None, None, None).unwrap();
    let error = load_columns(&cfg).unwrap_err().to_string();
    assert!(error.contains("no CSV file found"), "unexpected: {error}");
}

#[test]
fn book_depth_dedupes_same_time_and_percentage() {
    let tmp = TempDir::new().unwrap();
    let root = data_root(&tmp);
    let dir = archive_dir(&root, "futures/um", "daily", "bookDepth", "X", None);
    create_test_zip(
        &dir,
        "X-bookDepth-2024-01-01.zip",
        "timestamp,percentage,depth,notional\n\
         2024-01-01 00:00:00,-5,1208022.0,2779.0\n\
         2024-01-01 00:00:00,-5,1208022.0,2779.0\n\
         2024-01-01 00:00:00,-4,1110958.0,2568.0\n",
    );

    let cfg = DataConfig::new(Dataset::BookDepth, &root, "X", None, None, None).unwrap();
    let columns = load_columns(&cfg).unwrap();
    let depth = columns.as_book_depth().unwrap();
    assert_eq!(depth.len(), 2);
    assert_eq!(depth.percentage(), &[-5, -4]);
}

#[test]
fn liquidation_exact_duplicates_removed() {
    let tmp = TempDir::new().unwrap();
    let root = data_root(&tmp);
    let dir = archive_dir(
        &root,
        "futures/cm",
        "daily",
        "liquidationSnapshot",
        "BTCUSD_PERP",
        None,
    );
    create_test_zip(
        &dir,
        "BTCUSD_PERP-liquidationSnapshot-2024-01-01.zip",
        "time,side,order_type,time_in_force,original_quantity,price,average_price,order_status,last_fill_quantity,accumulated_fill_quantity\n\
         1704068163557,BUY,LIMIT,IOC,285,42708.1,42548.0,FILLED,136,285\n\
         1704068163557,BUY,LIMIT,IOC,285,42708.1,42548.0,FILLED,136,285\n\
         1704072759690,BUY,LIMIT,IOC,1,42729.9,42579.2,FILLED,1,1\n",
    );

    let cfg = DataConfig::new_with_market(
        Dataset::LiquidationSnapshot,
        Market::Cm,
        &root,
        "BTCUSD_PERP",
        None,
        None,
        None,
    )
    .unwrap();
    let columns = load_columns(&cfg).unwrap();
    let liquidations = columns.as_liquidation().unwrap();
    assert_eq!(liquidations.len(), 2);
    assert_eq!(liquidations.side(), &["BUY", "BUY"]);
}

#[test]
fn corrupt_zip_fails_batch() {
    let tmp = TempDir::new().unwrap();
    let root = data_root(&tmp);
    let dir = archive_dir(&root, "futures/um", "daily", "metrics", "X", None);
    create_test_zip(
        &dir,
        "X-metrics-2024-01-01.zip",
        "create_time,symbol,sum_open_interest,sum_open_interest_value,count_toptrader_long_short_ratio,sum_toptrader_long_short_ratio,count_long_short_ratio,sum_taker_long_short_vol_ratio\n\
         2024-01-01 00:00:00,X,1.0,1.0,1.0,1.0,1.0,1.0\n",
    );
    std::fs::write(dir.join("X-metrics-2024-01-02.zip"), b"not a zip").unwrap();

    let cfg = DataConfig::new(Dataset::Metrics, &root, "X", None, None, None).unwrap();
    let error = load_columns(&cfg).unwrap_err().to_string();
    assert!(error.contains("1 file(s) failed"), "unexpected: {error}");
}

#[test]
fn invalid_filename_date_fails_batch() {
    let tmp = TempDir::new().unwrap();
    let root = data_root(&tmp);
    let dir = archive_dir(&root, "futures/um", "daily", "metrics", "X", None);
    create_test_zip(
        &dir,
        "X-metrics-2024-13-99.zip",
        "create_time,symbol,sum_open_interest,sum_open_interest_value,count_toptrader_long_short_ratio,sum_toptrader_long_short_ratio,count_long_short_ratio,sum_taker_long_short_vol_ratio\n",
    );

    let cfg = DataConfig::new(Dataset::Metrics, &root, "X", None, None, None).unwrap();
    let error = load_columns(&cfg).unwrap_err().to_string();
    assert!(error.contains("2024-13-99"), "unexpected: {error}");
}

#[test]
fn missing_dir_errors_as_io() {
    let tmp = TempDir::new().unwrap();
    let cfg = DataConfig::new(Dataset::Metrics, tmp.path(), "X", None, None, None).unwrap();
    let error = load_columns(&cfg).unwrap_err();
    assert!(matches!(error, ReaderError::Io(_)), "unexpected: {error}");
}

#[test]
fn empty_dir_returns_empty_columns() {
    let tmp = TempDir::new().unwrap();
    let root = data_root(&tmp);
    archive_dir(&root, "futures/um", "daily", "metrics", "X", None);

    let cfg = DataConfig::new(Dataset::Metrics, &root, "X", None, None, None).unwrap();
    let columns = load_columns(&cfg).unwrap();
    assert!(columns.is_empty());
    assert_eq!(columns.dataset(), Dataset::Metrics);
}

#[test]
fn dataset_columns_tag_and_accessors() {
    let tmp = TempDir::new().unwrap();
    let root = data_root(&tmp);
    let dir = archive_dir(&root, "futures/um", "daily", "bookTicker", "X", None);
    create_test_zip(
        &dir,
        "X-bookTicker-2024-01-01.zip",
        "update_id,best_bid_price,best_bid_qty,best_ask_price,best_ask_qty,transaction_time,event_time\n\
         1,0.004411,100000,0E-8,0E-8,1704067200000,1704067200001\n",
    );

    let cfg = DataConfig::new(Dataset::BookTicker, &root, "X", None, None, None).unwrap();
    let columns = load_columns(&cfg).unwrap();
    assert_eq!(columns.dataset(), Dataset::BookTicker);
    assert_eq!(columns.time(), &[1_704_067_200_001]);
    let ticker = columns.as_book_ticker().unwrap();
    assert_eq!(ticker.best_ask_price(), &[0.0]);
    assert_eq!(ticker.update_id(), &[1]);
}

#[test]
fn dedupe_across_daily_and_monthly_files() {
    let tmp = TempDir::new().unwrap();
    let root = data_root(&tmp);
    let daily = archive_dir(&root, "futures/um", "daily", "fundingRate", "X", None);
    let monthly = archive_dir(&root, "futures/um", "monthly", "fundingRate", "X", None);
    let csv = "calc_time,funding_interval_hours,last_funding_rate\n\
               1704067200000,8,0.0001\n";
    create_test_zip(&daily, "X-fundingRate-2024-01-01.zip", csv);
    create_test_zip(&monthly, "X-fundingRate-2024-01.zip", csv);

    let cfg = DataConfig::new(Dataset::FundingRate, &root, "X", None, None, None).unwrap();
    let columns = load_columns(&cfg).unwrap();
    assert_eq!(columns.len(), 1);
}

//! Catalog inventory against synthetic archives.

// Fixture setup is test code: panicking there means the fixture is broken.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use binance_data_reader::{CatalogFilter, Dataset, Interval, Market, Period, catalog};
use common::{KLINE_HEADER, date, kline, write_archive};
use std::path::Path;

const FUNDING_HEADER: &str = "calc_time,funding_interval_hours,last_funding_rate";
const METRICS_HEADER: &str = "create_time,symbol,sum_open_interest,sum_open_interest_value,count_toptrader_long_short_ratio,sum_toptrader_long_short_ratio,count_long_short_ratio,sum_taker_long_short_vol_ratio";

fn write_fixtures(root: &Path) {
    for day in [date(2023, 6, 24), date(2023, 6, 25)] {
        let stem = format!("BTCUSDT-1m-{}", day.format("%Y-%m-%d"));
        write_archive(
            root,
            "futures/um",
            "daily",
            "klines",
            "BTCUSDT",
            Some("1m"),
            &stem,
            &format!(
                "{KLINE_HEADER}\n{}\n",
                kline(common::day_start(day), Interval::M1, 100.0)
            ),
        );
    }
    write_archive(
        root,
        "futures/um",
        "monthly",
        "klines",
        "BTCUSDT",
        Some("1m"),
        "BTCUSDT-1m-2023-06",
        &format!("{KLINE_HEADER}\n"),
    );
    for month in [date(2023, 5, 1), date(2023, 6, 1)] {
        let stem = format!("BTCUSDT-fundingRate-{}", month.format("%Y-%m"));
        write_archive(
            root,
            "futures/um",
            "monthly",
            "fundingRate",
            "BTCUSDT",
            None,
            &stem,
            &format!("{FUNDING_HEADER}\n1685577600000,8,0.0001\n"),
        );
    }
    write_archive(
        root,
        "futures/um",
        "daily",
        "metrics",
        "BTCUSDT",
        None,
        "BTCUSDT-metrics-2023-06-25",
        &format!("{METRICS_HEADER}\n"),
    );
}

#[test]
fn catalog_reports_each_cadence_and_interval() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write_fixtures(tmp.path());

    let found = catalog(tmp.path(), &CatalogFilter::default()).expect("catalog");
    assert_eq!(found.entries.len(), 4);

    let klines: Vec<_> = found
        .entries
        .iter()
        .filter(|entry| entry.dataset == Dataset::Klines)
        .collect();
    assert_eq!(klines.len(), 2);
    let daily = klines
        .iter()
        .find(|entry| entry.period == Period::Daily)
        .expect("daily klines entry");
    assert_eq!(daily.market, Market::Um);
    assert_eq!(daily.symbol, "BTCUSDT");
    assert_eq!(daily.files, 2);
    assert_eq!(daily.first_date, Some(date(2023, 6, 24)));
    assert_eq!(daily.last_date, Some(date(2023, 6, 25)));

    let funding = found
        .entries
        .iter()
        .find(|entry| entry.dataset == Dataset::FundingRate)
        .expect("funding entry");
    assert_eq!(funding.period, Period::Monthly);
    assert_eq!(funding.files, 2);
    assert_eq!(funding.first_date, Some(date(2023, 5, 1)));
    assert_eq!(funding.last_date, Some(date(2023, 6, 1)));
}

#[test]
fn catalog_filters_compose() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write_fixtures(tmp.path());

    let filter = CatalogFilter {
        dataset: Some(Dataset::Klines),
        ..CatalogFilter::default()
    };
    let found = catalog(tmp.path(), &filter).expect("catalog");
    assert_eq!(found.entries.len(), 2);

    let filter = CatalogFilter {
        dataset: Some(Dataset::Klines),
        period: Some(Period::Monthly),
        symbol: Some("BTCUSDT".parse().expect("valid symbol")),
        ..CatalogFilter::default()
    };
    let found = catalog(tmp.path(), &filter).expect("catalog");
    assert_eq!(found.entries.len(), 1);
    assert_eq!(found.entries[0].files, 1);
}

#[test]
fn empty_root_is_a_valid_empty_catalog() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let found = catalog(tmp.path(), &CatalogFilter::default()).expect("catalog");
    assert!(found.entries.is_empty());
}

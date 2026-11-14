//! Synthetic archive fixtures shared by integration tests.
//!
//! Every helper writes exactly the layout produced by the downloader:
//! `{root}/data/{market}/{cadence}/{dataset}/{symbol}[/{interval}]/{name}.zip`.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use binance_data_reader::Interval;
use chrono::NaiveDate;
use zip::write::SimpleFileOptions;

/// Kline CSV header used by futures archives.
pub const KLINE_HEADER: &str = "open_time,open,high,low,close,volume,close_time,quote_volume,count,taker_buy_volume,taker_buy_quote_volume,ignore";

/// Build a naive date.
pub fn date(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).expect("valid fixture date")
}

/// UTC midnight of `date` in epoch milliseconds.
pub fn day_start(date: NaiveDate) -> i64 {
    date.and_hms_opt(0, 0, 0)
        .expect("midnight exists")
        .and_utc()
        .timestamp_millis()
}

/// Write one archive ZIP whose single CSV entry is `{stem}.csv`.
#[expect(
    clippy::too_many_arguments,
    reason = "fixture helper mirrors the archive path segments one-to-one"
)]
pub fn write_archive(
    root: &Path,
    market: &str,
    cadence: &str,
    dataset: &str,
    symbol: &str,
    interval: Option<&str>,
    stem: &str,
    csv: &str,
) {
    let path =
        archive_dir(root, market, cadence, dataset, symbol, interval).join(format!("{stem}.zip"));
    write_zip(&path, &[(format!("{stem}.csv").as_str(), csv)]);
}

/// One kline CSV line with a valid `close_time` for `interval`.
pub fn kline(open_ms: i64, interval: Interval, price: f64) -> String {
    let close = interval
        .next_open(open_ms)
        .expect("fixture open time is representable")
        - 1;
    format!("{open_ms},{price},{price},{price},{price},1.5,{close},10.5,7,0.5,3.5,0")
}

fn archive_dir(
    root: &Path,
    market: &str,
    cadence: &str,
    dataset: &str,
    symbol: &str,
    interval: Option<&str>,
) -> PathBuf {
    let mut dir = root
        .join("data")
        .join(market)
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

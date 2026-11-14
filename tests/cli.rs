//! CLI contract: JSON on stdout, logs on stderr, 0/1/2 exit codes.

// Fixture setup is test code: panicking there means the fixture is broken.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use std::path::Path;
use std::process::{Command, Output};

use binance_data_reader::Interval;
use common::{KLINE_HEADER, date, day_start, kline, write_archive};
use serde_json::Value;

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_binance-data-reader"))
        .args(args)
        .output()
        .expect("spawn reader binary")
}

fn write_klines(root: &Path) {
    let day = date(2023, 6, 25);
    let csv = format!(
        "{KLINE_HEADER}\n{}\n{}\n",
        kline(day_start(day), Interval::M1, 100.0),
        kline(day_start(day) + 60_000, Interval::M1, 101.0)
    );
    write_archive(
        root,
        "futures/um",
        "daily",
        "klines",
        "BTCUSDT",
        Some("1m"),
        "BTCUSDT-1m-2023-06-25",
        &csv,
    );
}

fn write_request(root: &Path) -> std::path::PathBuf {
    let path = root.join("request.toml");
    std::fs::write(
        &path,
        format!(
            "root = \"{}\"\ndataset = \"klines\"\nsymbol = \"BTCUSDT\"\ninterval = \"1m\"\nstart_date = \"2023-06-25\"\nend_date = \"2023-06-25\"\n",
            root.display()
        ),
    )
    .expect("write request");
    path
}

#[test]
fn read_prints_columns_and_report_as_json() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write_klines(root);
    let request = write_request(root);

    let output = run(&["read", "-c", request.to_str().expect("utf-8 path")]);
    assert!(output.status.success(), "{output:?}");
    let json: Value = serde_json::from_slice(&output.stdout).expect("stdout is JSON");
    assert_eq!(json["dataset"], "klines");
    assert_eq!(json["report"]["rows"], 2);
    assert_eq!(json["report"]["dataset"], "klines");
    assert_eq!(json["report"]["interval"], "1m");
    assert_eq!(json["report"]["files_daily"], 1);
    let opens = json["columns"]["open_time"]
        .as_array()
        .expect("open_time column");
    assert_eq!(opens.len(), 2);
    assert_eq!(opens[0], day_start(date(2023, 6, 25)));
    assert!(json["columns"]["quote_volume"].is_array());
}

#[test]
fn invalid_requests_exit_with_code_2() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("bad.toml");
    std::fs::write(
        &path,
        "root = \"/data\"\ndataset = \"klines\"\nsymbol = \"BTCUSDT\"\n",
    )
    .expect("write bad request");

    let output = run(&["read", "-c", path.to_str().expect("utf-8 path")]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("requires an interval"), "{stderr}");
    assert!(output.stdout.is_empty());
}

#[test]
fn missing_archives_exit_with_code_1() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("missing.toml");
    std::fs::write(
        &path,
        format!(
            "root = \"{}\"\ndataset = \"trades\"\nsymbol = \"BTCUSDT\"\n",
            tmp.path().join("does-not-exist").display()
        ),
    )
    .expect("write request");

    let output = run(&["read", "-c", path.to_str().expect("utf-8 path")]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("archive root not found"), "{stderr}");
}

#[test]
fn list_prints_the_catalog() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write_klines(root);

    let output = run(&[
        "list",
        "--root",
        root.to_str().expect("utf-8 path"),
        "--dataset",
        "klines",
    ]);
    assert!(output.status.success(), "{output:?}");
    let json: Value = serde_json::from_slice(&output.stdout).expect("stdout is JSON");
    let entries = json["entries"].as_array().expect("entries array");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["dataset"], "klines");
    assert_eq!(entries[0]["market"], "futures/um");
    assert_eq!(entries[0]["period"], "daily");
    assert_eq!(entries[0]["interval"], "1m");
    assert_eq!(entries[0]["files"], 1);
}

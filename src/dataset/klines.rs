//! Klines: regular klines, index-price, mark-price and premium-index klines.
//!
//! All four families share the same 12-field archive schema. Spot files are
//! usually headerless, futures files carry a header; both are accepted.
//! Premium-index prices are rates around zero and may be negative.
//!
//! Unlike the old reader, every field is returned: `close_time`,
//! `quote_volume`, `count`, `taker_buy_volume`, `taker_buy_quote_volume` and
//! `ignore` are no longer dropped.

use csv::StringRecord;

use crate::config::Config;
use crate::error::{Error, Result};
use crate::source::csv::{
    Header, expect_fields, f64, non_negative_f64, non_negative_i64, timestamp,
};
use crate::source::files::Scan;
use crate::table::{Column, Table};
use crate::types::Interval;

use super::{Built, finish, read_dataset};

const KLINE_HEADER: [&str; 12] = [
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
    "ignore",
];
const KLINE_ACCEPTED: [&[&str]; 1] = [&KLINE_HEADER];
const HEADER: Header = Header::new(&KLINE_HEADER, &KLINE_ACCEPTED);
const FIELDS: usize = KLINE_HEADER.len();

/// One kline row, including fields the old reader discarded.
#[derive(Debug, Clone)]
pub(crate) struct KlineRow {
    pub open_time: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub close_time: i64,
    pub quote_volume: f64,
    pub count: i64,
    pub taker_buy_volume: f64,
    pub taker_buy_quote_volume: f64,
    pub ignore: i64,
}

/// Load one kline-shaped dataset.
pub(crate) fn load(config: &Config, scan: &Scan) -> Result<Built> {
    let Some(interval) = config.interval() else {
        return Err(Error::Config(format!(
            "dataset {} requires an interval",
            config.dataset()
        )));
    };
    let allow_non_positive = config.dataset().allows_non_positive_price();
    let loaded = read_dataset(
        config,
        scan,
        HEADER,
        |row| parse_row(row, interval, allow_non_positive),
        |row| row.open_time,
    )?;

    let stats = loaded.stats;
    let (table, gaps) = build_table(loaded.rows, interval)?;
    finish(table, stats, gaps, 0)
}

fn parse_row(
    row: &StringRecord,
    interval: Interval,
    allow_non_positive: bool,
) -> std::result::Result<KlineRow, String> {
    expect_fields(row, FIELDS, "klines")?;
    let open_time = timestamp(row, 0, "open_time")?;
    let open = f64(row, 1, "open")?;
    let high = f64(row, 2, "high")?;
    let low = f64(row, 3, "low")?;
    let close = f64(row, 4, "close")?;
    let volume = non_negative_f64(row, 5, "volume")?;
    let close_time = timestamp(row, 6, "close_time")?;

    if !allow_non_positive && [open, high, low, close].iter().any(|price| *price <= 0.0) {
        return Err(format!(
            "non-positive price: open={open} high={high} low={low} close={close}"
        ));
    }
    if high < low || high < open || high < close || low > open || low > close {
        return Err(format!(
            "invalid OHLC: open={open} high={high} low={low} close={close}"
        ));
    }

    let expected_close = interval
        .next_open(open_time)
        .and_then(|next| next.checked_sub(1))
        .ok_or_else(|| format!("open_time {open_time} is outside the representable range"))?;
    if close_time != expected_close {
        return Err(format!(
            "close_time {close_time} does not match interval {interval} (expected {expected_close})"
        ));
    }

    Ok(KlineRow {
        open_time,
        open,
        high,
        low,
        close,
        volume,
        close_time,
        quote_volume: non_negative_f64(row, 7, "quote_volume")?,
        count: non_negative_i64(row, 8, "count")?,
        taker_buy_volume: non_negative_f64(row, 9, "taker_buy_volume")?,
        taker_buy_quote_volume: non_negative_f64(row, 10, "taker_buy_quote_volume")?,
        ignore: non_negative_i64(row, 11, "ignore")?,
    })
}

fn build_table(rows: Vec<KlineRow>, interval: Interval) -> Result<(Table, usize)> {
    let mut open_time = Vec::with_capacity(rows.len());
    let mut open = Vec::with_capacity(rows.len());
    let mut high = Vec::with_capacity(rows.len());
    let mut low = Vec::with_capacity(rows.len());
    let mut close = Vec::with_capacity(rows.len());
    let mut volume = Vec::with_capacity(rows.len());
    let mut close_time = Vec::with_capacity(rows.len());
    let mut quote_volume = Vec::with_capacity(rows.len());
    let mut count = Vec::with_capacity(rows.len());
    let mut taker_buy_volume = Vec::with_capacity(rows.len());
    let mut taker_buy_quote_volume = Vec::with_capacity(rows.len());
    let mut ignore = Vec::with_capacity(rows.len());
    let mut gaps = 0usize;
    let mut previous: Option<KlineRow> = None;

    for row in rows {
        if let Some(previous) = &previous {
            if row.open_time <= previous.open_time {
                return Err(Error::data(format!(
                    "kline open_time {} is not after {}; duplicate or misordered rows remain after the daily/monthly merge",
                    row.open_time, previous.open_time
                )));
            }
            let expected = interval.next_open(previous.open_time).ok_or_else(|| {
                Error::data(format!(
                    "open_time {} is outside the representable range",
                    previous.open_time
                ))
            })?;
            if row.open_time != expected {
                gaps += 1;
                tracing::debug!(
                    previous = previous.open_time,
                    next = row.open_time,
                    expected,
                    "kline gap"
                );
            }
        }

        open_time.push(row.open_time);
        open.push(row.open);
        high.push(row.high);
        low.push(row.low);
        close.push(row.close);
        volume.push(row.volume);
        close_time.push(row.close_time);
        quote_volume.push(row.quote_volume);
        count.push(row.count);
        taker_buy_volume.push(row.taker_buy_volume);
        taker_buy_quote_volume.push(row.taker_buy_quote_volume);
        ignore.push(row.ignore);
        previous = Some(row);
    }

    let table = Table::new(vec![
        Column::i64("open_time", open_time),
        Column::f64("open", open),
        Column::f64("high", high),
        Column::f64("low", low),
        Column::f64("close", close),
        Column::f64("volume", volume),
        Column::i64("close_time", close_time),
        Column::f64("quote_volume", quote_volume),
        Column::i64("count", count),
        Column::f64("taker_buy_volume", taker_buy_volume),
        Column::f64("taker_buy_quote_volume", taker_buy_quote_volume),
        Column::i64("ignore", ignore),
    ])?;
    Ok((table, gaps))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str, interval: Interval, allow: bool) -> std::result::Result<KlineRow, String> {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .from_reader(line.as_bytes());
        parse_row(&reader.records().next().unwrap().unwrap(), interval, allow)
    }

    const JUNE_2023: &str = "1685577600000,27210.36,27224.55,27210.35,27215.53,56.34942,1685577659999,1533688.30709290,819,28.38114,772502.81811770,0";

    #[test]
    fn all_twelve_fields_are_kept() {
        let row = parse(JUNE_2023, Interval::M1, false).unwrap();
        assert_eq!(row.open_time, 1_685_577_600_000);
        assert_eq!(row.close_time, 1_685_577_659_999);
        assert_eq!(row.count, 819);
        assert!((row.quote_volume - 1_533_688.307_092_9).abs() < 1e-6);
        assert_eq!(row.ignore, 0);
    }

    #[test]
    fn premium_index_accepts_negative_prices() {
        let line = "1688083200000,-0.00028955,-0.00028955,-0.00054539,-0.00043814,0,1688083259999,0,12,0,0,0";
        assert!(parse(line, Interval::M1, true).is_ok());
        assert!(parse(line, Interval::M1, false).is_err());
    }

    #[test]
    fn misaligned_ohlc_and_negative_volume_fail() {
        assert!(
            parse(
                "1685577600000,10,9,12,11,1,1685577659999,0,0,0,0,0",
                Interval::M1,
                false
            )
            .is_err()
        );
        assert!(
            parse(
                "1685577600000,10,12,9,11,-1,1685577659999,0,0,0,0,0",
                Interval::M1,
                false
            )
            .is_err()
        );
    }

    #[test]
    fn close_time_must_match_the_interval() {
        let wrong = JUNE_2023.replace("1685577659999", "1685577659998");
        assert!(parse(&wrong, Interval::M1, false).is_err());
    }

    #[test]
    fn missing_and_extra_fields_fail() {
        let short = JUNE_2023.rsplit_once(',').unwrap().0;
        assert!(parse(short, Interval::M1, false).is_err());
        let long = format!("{JUNE_2023},0");
        assert!(parse(&long, Interval::M1, false).is_err());
    }

    #[test]
    fn table_has_archive_column_order() {
        let row = parse(JUNE_2023, Interval::M1, false).unwrap();
        let (table, gaps) = build_table(vec![row], Interval::M1).unwrap();
        assert_eq!(gaps, 0);
        let names: Vec<&str> = table.columns().iter().map(Column::name).collect();
        assert_eq!(names, KLINE_HEADER);
    }

    #[test]
    fn duplicate_open_times_are_fatal() {
        let row = parse(JUNE_2023, Interval::M1, false).unwrap();
        let error = build_table(vec![row.clone(), row], Interval::M1).unwrap_err();
        assert!(error.to_string().contains("not after"));
    }

    #[test]
    fn gaps_are_counted_not_filled() {
        let first = parse(JUNE_2023, Interval::M1, false).unwrap();
        let mut second = first.clone();
        second.open_time += 2 * 60_000;
        second.close_time = Interval::M1.next_open(second.open_time).unwrap() - 1;
        let (table, gaps) = build_table(vec![first, second], Interval::M1).unwrap();
        assert_eq!(gaps, 1);
        assert_eq!(table.len(), 2);
    }
}

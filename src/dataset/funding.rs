//! Funding rates (`fundingRate`), monthly archive only.
//!
//! `last_funding_rate` is signed: negative rates are normal and must not be
//! rejected.

use csv::StringRecord;

use crate::config::Config;
use crate::error::Result;
use crate::source::csv::{Header, expect_fields, f64, non_negative_i64, timestamp};
use crate::source::files::Scan;
use crate::table::{Column, Table};

use super::{Built, finish, read_dataset};

const FUNDING_HEADER: [&str; 3] = ["calc_time", "funding_interval_hours", "last_funding_rate"];
const FUNDING_ACCEPTED: [&[&str]; 1] = [&FUNDING_HEADER];
const HEADER: Header = Header::new(&FUNDING_HEADER, &FUNDING_ACCEPTED);

/// One funding-rate settlement.
#[derive(Debug, Clone)]
pub(crate) struct FundingRow {
    pub calc_time: i64,
    pub funding_interval_hours: i64,
    pub last_funding_rate: f64,
}

/// Load funding rates.
pub(crate) fn load(config: &Config, scan: &Scan) -> Result<Built> {
    let loaded = read_dataset(config, scan, HEADER, parse_row, |row| row.calc_time)?;

    let stats = loaded.stats;
    let mut calc_time = Vec::with_capacity(loaded.rows.len());
    let mut funding_interval_hours = Vec::with_capacity(loaded.rows.len());
    let mut last_funding_rate = Vec::with_capacity(loaded.rows.len());

    for row in loaded.rows {
        calc_time.push(row.calc_time);
        funding_interval_hours.push(row.funding_interval_hours);
        last_funding_rate.push(row.last_funding_rate);
    }

    let table = Table::new(vec![
        Column::i64("calc_time", calc_time),
        Column::i64("funding_interval_hours", funding_interval_hours),
        Column::f64("last_funding_rate", last_funding_rate),
    ])?;
    finish(table, stats, 0, 0)
}

fn parse_row(row: &StringRecord) -> std::result::Result<FundingRow, String> {
    expect_fields(row, FUNDING_HEADER.len(), "fundingRate")?;
    let funding_interval_hours = non_negative_i64(row, 1, "funding_interval_hours")?;
    if funding_interval_hours == 0 {
        return Err("funding_interval_hours: zero interval".to_owned());
    }
    Ok(FundingRow {
        calc_time: timestamp(row, 0, "calc_time")?,
        funding_interval_hours,
        last_funding_rate: f64(row, 2, "last_funding_rate")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> std::result::Result<FundingRow, String> {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .from_reader(line.as_bytes());
        parse_row(&reader.records().next().unwrap().unwrap())
    }

    #[test]
    fn negative_rates_are_valid() {
        let row = parse("1700668800000,4,-0.00056702").unwrap();
        assert!(row.last_funding_rate < 0.0);
        assert_eq!(row.funding_interval_hours, 4);
    }

    #[test]
    fn zero_interval_and_non_finite_rates_fail() {
        assert!(parse("1700668800000,0,0.0001").is_err());
        assert!(parse("1700668800000,4,NaN").is_err());
        assert!(parse("1700668800000,4,inf").is_err());
    }
}

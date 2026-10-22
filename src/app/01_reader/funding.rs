//! Funding rates (`fundingRate`), monthly archive only.
//!
//! `last_funding_rate` is signed: negative rates are normal and must not be
//! rejected.

use std::path::PathBuf;

use csv::StringRecord;

use super::common::{self, DateWindow};
use crate::error::{ReaderError, Result};

/// One funding-rate settlement.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FundingRateRecord {
    pub(crate) calc_time: i64,
    pub(crate) funding_interval_hours: i64,
    pub(crate) last_funding_rate: f64,
}

const HEADER: [&str; 3] = ["calc_time", "funding_interval_hours", "last_funding_rate"];

/// Load funding rates, keeping rows inside `window` by `calc_time`.
pub(crate) fn load(files: &[PathBuf], window: &DateWindow) -> Result<Vec<FundingRateRecord>> {
    let headers: [&[&str]; 1] = [&HEADER];
    let rows = common::load_all(files, &headers, parse_row)?;
    Ok(window.trim(rows, |row| row.calc_time))
}

fn parse_row(row: &StringRecord) -> Result<FundingRateRecord> {
    let funding_interval_hours = common::parse_non_negative_i64(row, 1, "funding_interval_hours")?;
    if funding_interval_hours == 0 {
        return Err(ReaderError::Parse(
            "funding_interval_hours: zero interval".into(),
        ));
    }
    Ok(FundingRateRecord {
        calc_time: common::parse_timestamp(row, 0, "calc_time")?,
        funding_interval_hours,
        last_funding_rate: common::parse_f64(row, 2, "last_funding_rate")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> Result<FundingRateRecord> {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .from_reader(line.as_bytes());
        let row = reader.records().next().unwrap().unwrap();
        parse_row(&row)
    }

    #[test]
    fn negative_rate_is_accepted() {
        let record = parse("1700668800000,4,-0.00056702").unwrap();
        assert!(record.last_funding_rate < 0.0);
        assert_eq!(record.funding_interval_hours, 4);
    }

    #[test]
    fn zero_interval_is_rejected() {
        assert!(parse("1700668800000,0,0.0001").is_err());
    }

    #[test]
    fn non_finite_rate_is_rejected() {
        assert!(parse("1700668800000,4,NaN").is_err());
        assert!(parse("1700668800000,4,inf").is_err());
    }
}

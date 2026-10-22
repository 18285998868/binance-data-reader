//! Python bindings (`pyo3`, abi3): `load_columns(dict) -> dict`.
//!
//! The dict mirrors `DataConfig` and returns the dataset's column lists plus a
//! `"dataset"` tag. Parsing happens once at this boundary
//! (`DataConfig::new_with_market`); invalid input raises `ValueError`.

use pyo3::exceptions::{PyKeyError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;

use crate::DatasetColumns;
use crate::config::DataConfig;
use crate::error::ReaderError;
use crate::types::{Dataset, Market};

fn py_err(e: ReaderError) -> PyErr {
    pyo3::exceptions::PyRuntimeError::new_err(e.to_string())
}

fn extract_required<'py, T>(config: &Bound<'py, PyDict>, key: &str) -> PyResult<T>
where
    T: FromPyObject<'py>,
{
    config
        .get_item(key)?
        .ok_or_else(|| PyKeyError::new_err(format!("missing required config key: {key}")))?
        .extract()
        .map_err(|_| PyTypeError::new_err(format!("config[{key}] has wrong type")))
}

fn extract_optional<'py, T>(config: &Bound<'py, PyDict>, key: &str) -> PyResult<Option<T>>
where
    T: FromPyObject<'py>,
{
    let Some(value) = config.get_item(key)? else {
        return Ok(None);
    };
    if value.is_none() {
        return Ok(None);
    }
    value
        .extract()
        .map(Some)
        .map_err(|_| PyTypeError::new_err(format!("config[{key}] has wrong type")))
}

fn extract_dataset(config: &Bound<'_, PyDict>) -> PyResult<Dataset> {
    if config.contains("dataset")? {
        let raw: String = extract_required(config, "dataset")?;
        raw.parse::<Dataset>().map_err(PyValueError::new_err)
    } else if config.contains("type")? {
        let raw: String = extract_required(config, "type")?;
        raw.parse::<Dataset>().map_err(PyValueError::new_err)
    } else {
        Err(PyKeyError::new_err("missing required config key: dataset"))
    }
}

#[pyfunction]
#[pyo3(name = "load_columns", signature = (config))]
pub(crate) fn py_load_columns(config: &Bound<'_, PyDict>) -> PyResult<Py<PyAny>> {
    let dataset = extract_dataset(config)?;
    let dir: String = extract_required(config, "dir")?;
    let symbol: String = extract_required(config, "symbol")?;
    let market: Option<String> = extract_optional(config, "market")?;
    let interval: Option<String> = extract_optional(config, "interval")?;
    let start_date: Option<String> = extract_optional(config, "start_date")?;
    let end_date: Option<String> = extract_optional(config, "end_date")?;
    let market = match market.as_deref() {
        Some(raw) => raw.parse::<Market>().map_err(PyValueError::new_err)?,
        None => Market::default(),
    };
    let data_cfg = DataConfig::new_with_market(
        dataset,
        market,
        &dir,
        &symbol,
        interval.as_deref(),
        start_date.as_deref(),
        end_date.as_deref(),
    )
    .map_err(|e| PyValueError::new_err(e.to_string()))?;
    let columns = crate::app::load_columns(&data_cfg).map_err(py_err)?;
    Python::attach(|py| columns_to_dict(py, &columns))
}

fn columns_to_dict(py: Python<'_>, columns: &DatasetColumns) -> PyResult<Py<PyAny>> {
    let out = PyDict::new(py);
    out.set_item("dataset", columns.dataset().as_str())?;
    match columns {
        DatasetColumns::Klines(columns) => {
            out.set_item("time", columns.time())?;
            out.set_item("open", columns.open())?;
            out.set_item("high", columns.high())?;
            out.set_item("low", columns.low())?;
            out.set_item("close", columns.close())?;
            out.set_item("volume", columns.volume())?;
        }
        DatasetColumns::AggTrades(columns) => {
            out.set_item("time", columns.time())?;
            out.set_item("agg_trade_id", columns.agg_trade_id())?;
            out.set_item("price", columns.price())?;
            out.set_item("quantity", columns.quantity())?;
            out.set_item("first_trade_id", columns.first_trade_id())?;
            out.set_item("last_trade_id", columns.last_trade_id())?;
            out.set_item("is_buyer_maker", columns.is_buyer_maker())?;
        }
        DatasetColumns::Trades(columns) => {
            out.set_item("time", columns.time())?;
            out.set_item("trade_id", columns.trade_id())?;
            out.set_item("price", columns.price())?;
            out.set_item("qty", columns.qty())?;
            if let Some(quote_qty) = columns.quote_qty() {
                out.set_item("quote_qty", quote_qty)?;
            }
            if let Some(base_qty) = columns.base_qty() {
                out.set_item("base_qty", base_qty)?;
            }
            out.set_item("is_buyer_maker", columns.is_buyer_maker())?;
        }
        DatasetColumns::BookTicker(columns) => {
            out.set_item("time", columns.time())?;
            out.set_item("update_id", columns.update_id())?;
            out.set_item("best_bid_price", columns.best_bid_price())?;
            out.set_item("best_bid_qty", columns.best_bid_qty())?;
            out.set_item("best_ask_price", columns.best_ask_price())?;
            out.set_item("best_ask_qty", columns.best_ask_qty())?;
            out.set_item("transaction_time", columns.transaction_time())?;
        }
        DatasetColumns::BookDepth(columns) => {
            out.set_item("time", columns.time())?;
            out.set_item("percentage", columns.percentage())?;
            out.set_item("depth", columns.depth())?;
            out.set_item("notional", columns.notional())?;
        }
        DatasetColumns::FundingRate(columns) => {
            out.set_item("time", columns.time())?;
            out.set_item("funding_interval_hours", columns.funding_interval_hours())?;
            out.set_item("last_funding_rate", columns.last_funding_rate())?;
        }
        DatasetColumns::LiquidationSnapshot(columns) => {
            out.set_item("time", columns.time())?;
            out.set_item("side", columns.side().to_vec())?;
            out.set_item("order_type", columns.order_type().to_vec())?;
            out.set_item("time_in_force", columns.time_in_force().to_vec())?;
            out.set_item("original_quantity", columns.original_quantity())?;
            out.set_item("price", columns.price())?;
            out.set_item("average_price", columns.average_price())?;
            out.set_item("order_status", columns.order_status().to_vec())?;
            out.set_item("last_fill_quantity", columns.last_fill_quantity())?;
            out.set_item(
                "accumulated_fill_quantity",
                columns.accumulated_fill_quantity(),
            )?;
        }
        DatasetColumns::Metrics(columns) => {
            out.set_item("time", columns.time())?;
            out.set_item("sum_open_interest", columns.sum_open_interest())?;
            out.set_item("sum_open_interest_value", columns.sum_open_interest_value())?;
            out.set_item(
                "count_toptrader_long_short_ratio",
                columns.count_toptrader_long_short_ratio().to_vec(),
            )?;
            out.set_item(
                "sum_toptrader_long_short_ratio",
                columns.sum_toptrader_long_short_ratio().to_vec(),
            )?;
            out.set_item(
                "count_long_short_ratio",
                columns.count_long_short_ratio().to_vec(),
            )?;
            out.set_item(
                "sum_taker_long_short_vol_ratio",
                columns.sum_taker_long_short_vol_ratio().to_vec(),
            )?;
        }
    }
    Ok(out.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_dataset_errors() {
        Python::initialize();
        Python::attach(|py| {
            let config = PyDict::new(py);
            let err = py_load_columns(&config).unwrap_err();
            assert!(err.to_string().contains("missing required config key"));
        });
    }

    #[test]
    fn invalid_dataset_errors() {
        Python::initialize();
        Python::attach(|py| {
            let config = PyDict::new(py);
            config.set_item("dataset", "klines").unwrap();
            config.set_item("dir", "/tmp").unwrap();
            config.set_item("symbol", "X").unwrap();
            let err = py_load_columns(&config).unwrap_err();
            assert!(err.to_string().contains("unsupported dataset"));
        });
    }

    #[test]
    fn optional_none_is_accepted() {
        Python::initialize();
        Python::attach(|py| {
            let config = PyDict::new(py);
            config.set_item("interval", py.None()).unwrap();
            let interval = extract_optional::<String>(&config, "interval").unwrap();
            assert!(interval.is_none());
        });
    }

    #[test]
    fn optional_wrong_type_is_rejected() {
        Python::initialize();
        Python::attach(|py| {
            let config = PyDict::new(py);
            config.set_item("interval", 1).unwrap();
            let error = extract_optional::<String>(&config, "interval").unwrap_err();
            assert!(error.to_string().contains("wrong type"));
        });
    }

    #[test]
    fn non_kline_interval_errors() {
        Python::initialize();
        Python::attach(|py| {
            let config = PyDict::new(py);
            config.set_item("dataset", "metrics").unwrap();
            config.set_item("dir", "/tmp").unwrap();
            config.set_item("symbol", "X").unwrap();
            config.set_item("interval", "1m").unwrap();
            let err = py_load_columns(&config).unwrap_err();
            assert!(err.to_string().contains("does not take interval"));
        });
    }
}

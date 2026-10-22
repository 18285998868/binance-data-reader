//! Python bindings (`pyo3`, abi3): thin `load_columns(dict) -> dict` wrapper.
//!
//! The dict mirrors `DataConfig`
//! (`type/dir/market/symbol/timeframe/start_date/end_date`) and returns
//! `{time, open, high, low, close, volume}` column lists.
//! 解析在边界一次完成（`DataConfig::new`），非法输入直接报 `ValueError`。

use pyo3::exceptions::{PyKeyError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;

use crate::config::{DataConfig, DataSource, Market};
use crate::error::ReaderError;

fn py_err(e: ReaderError) -> PyErr {
    pyo3::exceptions::PyRuntimeError::new_err(e.to_string())
}

fn extract_required<'py, T>(cfg: &Bound<'py, PyDict>, key: &str) -> PyResult<T>
where
    T: FromPyObject<'py>,
{
    cfg.get_item(key)?
        .ok_or_else(|| PyKeyError::new_err(format!("missing required config key: {key}")))?
        .extract()
        .map_err(|_| PyTypeError::new_err(format!("config[{key}] has wrong type")))
}

fn extract_optional<'py, T>(cfg: &Bound<'py, PyDict>, key: &str) -> PyResult<Option<T>>
where
    T: FromPyObject<'py>,
{
    cfg.get_item(key)?
        .map(|v| {
            v.extract()
                .map_err(|_| PyTypeError::new_err(format!("config[{key}] has wrong type")))
        })
        .transpose()
}

#[pyfunction]
#[pyo3(name = "load_columns", signature = (config))]
pub(crate) fn py_load_columns(config: &Bound<'_, PyDict>) -> PyResult<Py<PyAny>> {
    let source_type = match extract_required::<String>(config, "type")?.as_str() {
        "binance" => DataSource::Binance,
        "dukascopy" => DataSource::Dukascopy,
        other => {
            return Err(PyValueError::new_err(format!(
                "unsupported type: {other}, want binance|dukascopy"
            )));
        }
    };
    let dir: String = extract_required(config, "dir")?;
    let market: Option<String> = extract_optional(config, "market")?;
    let symbol: String = extract_required(config, "symbol")?;
    let timeframe: String = extract_required(config, "timeframe")?;
    let start_date: Option<String> = extract_optional(config, "start_date")?;
    let end_date: Option<String> = extract_optional(config, "end_date")?;
    let market = match market.as_deref() {
        Some(raw) => Some(raw.parse::<Market>().map_err(PyValueError::new_err)?),
        None => None,
    };
    let data_cfg = match market {
        Some(market) => DataConfig::new_with_market(
            source_type,
            market,
            &dir,
            &symbol,
            &timeframe,
            start_date.as_deref(),
            end_date.as_deref(),
        ),
        None => DataConfig::new(
            source_type,
            &dir,
            &symbol,
            &timeframe,
            start_date.as_deref(),
            end_date.as_deref(),
        ),
    }
    .map_err(|e| PyValueError::new_err(e.to_string()))?;
    let cols = crate::app::load_columns(&data_cfg).map_err(py_err)?;
    Python::attach(|py| {
        let out = PyDict::new(py);
        out.set_item("time", cols.time())?;
        out.set_item("open", cols.open())?;
        out.set_item("high", cols.high())?;
        out.set_item("low", cols.low())?;
        out.set_item("close", cols.close())?;
        out.set_item("volume", cols.volume())?;
        Ok(out.into())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pyo3::types::PyDict;

    #[test]
    fn py_load_columns_missing_key_errors() {
        Python::initialize();
        Python::attach(|py| {
            let cfg = PyDict::new(py);
            let err = py_load_columns(&cfg).unwrap_err();
            assert!(err.to_string().contains("missing required config key"));
        });
    }

    #[test]
    fn py_load_columns_invalid_type_errors() {
        Python::initialize();
        Python::attach(|py| {
            let cfg = PyDict::new(py);
            cfg.set_item("type", "nope").unwrap();
            cfg.set_item("dir", "/tmp").unwrap();
            cfg.set_item("symbol", "X").unwrap();
            cfg.set_item("timeframe", "1d").unwrap();
            let err = py_load_columns(&cfg).unwrap_err();
            assert!(err.to_string().contains("unsupported type"));
        });
    }

    #[test]
    fn py_load_columns_invalid_timeframe_errors() {
        Python::initialize();
        Python::attach(|py| {
            let cfg = PyDict::new(py);
            cfg.set_item("type", "binance").unwrap();
            cfg.set_item("dir", "/tmp").unwrap();
            cfg.set_item("symbol", "X").unwrap();
            cfg.set_item("timeframe", "3d").unwrap();
            let err = py_load_columns(&cfg).unwrap_err();
            assert!(err.to_string().contains("unsupported"));
        });
    }

    #[test]
    fn py_load_columns_loads_data() {
        Python::initialize();
        let tmp = tempfile::TempDir::new().unwrap();
        let data_dir = tmp.path().to_path_buf();
        let kline_dir = data_dir.join("binance/data/futures/um/daily/klines/X/1m");
        std::fs::create_dir_all(&kline_dir).unwrap();
        crate::test_helpers::create_test_zip(
            &kline_dir,
            "X-1m-2024-01-01.zip",
            "100,10.0,12.0,9.0,11.0,1000.0,0,0,0,0,0,0\n",
        );
        Python::attach(|py| {
            let cfg = PyDict::new(py);
            cfg.set_item("type", "binance").unwrap();
            cfg.set_item("dir", data_dir.to_str().unwrap()).unwrap();
            cfg.set_item("symbol", "X").unwrap();
            cfg.set_item("timeframe", "1m").unwrap();
            let out = py_load_columns(&cfg).unwrap();
            let out = out.bind(py);
            let time: Vec<i64> = out.get_item("time").unwrap().extract().unwrap();
            let close: Vec<f64> = out.get_item("close").unwrap().extract().unwrap();
            assert_eq!(time.len(), 1);
            assert!((close[0] - 11.0).abs() < 1e-9);
        });
    }

    #[test]
    fn py_load_columns_spot_market() {
        Python::initialize();
        let tmp = tempfile::TempDir::new().unwrap();
        let data_dir = tmp.path().to_path_buf();
        let kline_dir = data_dir.join("binance/data/spot/daily/klines/X/1m");
        std::fs::create_dir_all(&kline_dir).unwrap();
        crate::test_helpers::create_test_zip(
            &kline_dir,
            "X-1m-2024-01-01.zip",
            "100,20.0,22.0,19.0,21.0,1000.0,0,0,0,0,0,0\n",
        );
        Python::attach(|py| {
            let cfg = PyDict::new(py);
            cfg.set_item("type", "binance").unwrap();
            cfg.set_item("market", "spot").unwrap();
            cfg.set_item("dir", data_dir.to_str().unwrap()).unwrap();
            cfg.set_item("symbol", "X").unwrap();
            cfg.set_item("timeframe", "1m").unwrap();
            let out = py_load_columns(&cfg).unwrap();
            let out = out.bind(py);
            let close: Vec<f64> = out.get_item("close").unwrap().extract().unwrap();
            assert_eq!(close.len(), 1);
            assert!((close[0] - 21.0).abs() < 1e-9);
        });
    }

    #[test]
    fn py_load_columns_invalid_market_errors() {
        Python::initialize();
        Python::attach(|py| {
            let cfg = PyDict::new(py);
            cfg.set_item("type", "binance").unwrap();
            cfg.set_item("market", "nope").unwrap();
            cfg.set_item("dir", "/tmp").unwrap();
            cfg.set_item("symbol", "X").unwrap();
            cfg.set_item("timeframe", "1d").unwrap();
            let err = py_load_columns(&cfg).unwrap_err();
            assert!(err.to_string().contains("unsupported market"));
        });
    }
}

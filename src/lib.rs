pub mod app;
mod config;
mod error;
mod python;
#[cfg(test)]
#[path = "test_helpers.rs"]
mod test_helpers;

pub use app::{KlineColumns, load_columns};
pub use config::{DataConfig, DataSource};
pub use error::{ReaderError, Result};
use pyo3::prelude::*;

/// Python extension module (`maturin build` → `import kline_reader`).
#[pymodule]
fn kline_reader(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(python::py_load_columns, m)?)?;
    Ok(())
}

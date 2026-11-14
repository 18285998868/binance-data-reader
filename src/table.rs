//! Columnar output shared by every dataset.
//!
//! A [`Table`] owns named, equal-length typed columns. It replaces the old
//! per-dataset column structs: downstream code asks for a column by archive
//! name and gets the exact type stored in the archive, including fields that
//! the old reader dropped (kline quote volume, trade match flags, metrics
//! symbol, ...).
//!
//! Serialization is a JSON object mapping each column name to its array, so
//! the CLI and Python bindings share one shape:
//!
//! ```json
//! {"open_time":[1685577600000],"close":[27215.53]}
//! ```

use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};

use crate::error::{Error, Result};

/// An immutable, validated set of equal-length columns.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    columns: Vec<Column>,
}

impl Table {
    /// Build a table, checking that every column has the same length and that
    /// names are non-empty and unique.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Data`] describing the first violated invariant.
    pub fn new(columns: Vec<Column>) -> Result<Self> {
        let mut names = std::collections::HashSet::with_capacity(columns.len());
        let mut len = None;
        for column in &columns {
            if column.name.is_empty() {
                return Err(Error::data("column name must not be empty".to_owned()));
            }
            if !names.insert(column.name.as_str()) {
                return Err(Error::data(format!(
                    "duplicate column name: {}",
                    column.name
                )));
            }
            match len {
                None => len = Some(column.len()),
                Some(expected) if expected != column.len() => {
                    return Err(Error::data(format!(
                        "column {} has {} rows, expected {expected}",
                        column.name,
                        column.len()
                    )));
                }
                Some(_) => {}
            }
        }
        Ok(Self { columns })
    }

    /// Number of rows.
    #[must_use]
    pub fn len(&self) -> usize {
        self.columns.first().map_or(0, Column::len)
    }

    /// Whether the table has no rows.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Columns in archive field order.
    #[must_use]
    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    /// Look up a column by its archive name.
    #[must_use]
    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|column| column.name == name)
    }

    /// Typed access to an `i64` column.
    #[must_use]
    pub fn i64(&self, name: &str) -> Option<&[i64]> {
        self.column(name)?.values().as_i64()
    }

    /// Typed access to an `f64` column.
    #[must_use]
    pub fn f64(&self, name: &str) -> Option<&[f64]> {
        self.column(name)?.values().as_f64()
    }

    /// Typed access to a `bool` column.
    #[must_use]
    pub fn bool(&self, name: &str) -> Option<&[bool]> {
        self.column(name)?.values().as_bool()
    }

    /// Typed access to a string column.
    #[must_use]
    pub fn strings(&self, name: &str) -> Option<&[String]> {
        self.column(name)?.values().as_strings()
    }

    /// Typed access to an optional-float column.
    #[must_use]
    pub fn optional_f64(&self, name: &str) -> Option<&[Option<f64>]> {
        self.column(name)?.values().as_optional_f64()
    }

    /// Consume the table, returning its columns.
    #[must_use]
    pub fn into_columns(self) -> Vec<Column> {
        self.columns
    }
}

impl Serialize for Table {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.columns.len()))?;
        for column in &self.columns {
            map.serialize_entry(&column.name, &column.values)?;
        }
        map.end()
    }
}

/// One named column.
#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    name: String,
    values: ColumnValues,
}

impl Column {
    /// Build a signed-integer column.
    #[must_use]
    pub fn i64(name: impl Into<String>, values: Vec<i64>) -> Self {
        Self {
            name: name.into(),
            values: ColumnValues::I64(values),
        }
    }

    /// Build a float column.
    #[must_use]
    pub fn f64(name: impl Into<String>, values: Vec<f64>) -> Self {
        Self {
            name: name.into(),
            values: ColumnValues::F64(values),
        }
    }

    /// Build a boolean column.
    #[must_use]
    pub fn bool(name: impl Into<String>, values: Vec<bool>) -> Self {
        Self {
            name: name.into(),
            values: ColumnValues::Bool(values),
        }
    }

    /// Build a string column.
    #[must_use]
    pub fn strings(name: impl Into<String>, values: Vec<String>) -> Self {
        Self {
            name: name.into(),
            values: ColumnValues::Str(values),
        }
    }

    /// Build a nullable-float column; `None` is a field the archive left empty.
    #[must_use]
    pub fn optional_f64(name: impl Into<String>, values: Vec<Option<f64>>) -> Self {
        Self {
            name: name.into(),
            values: ColumnValues::OptF64(values),
        }
    }

    /// Archive field name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Number of values.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether the column has no values.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Typed values.
    #[must_use]
    pub const fn values(&self) -> &ColumnValues {
        &self.values
    }
}

/// Typed storage behind a [`Column`].
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ColumnValues {
    /// Signed 64-bit integers.
    I64(Vec<i64>),
    /// Finite 64-bit floats.
    F64(Vec<f64>),
    /// Booleans.
    Bool(Vec<bool>),
    /// UTF-8 strings.
    Str(Vec<String>),
    /// Nullable finite floats.
    OptF64(Vec<Option<f64>>),
}

impl ColumnValues {
    /// Number of values.
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::I64(values) => values.len(),
            Self::F64(values) => values.len(),
            Self::Bool(values) => values.len(),
            Self::Str(values) => values.len(),
            Self::OptF64(values) => values.len(),
        }
    }

    /// Whether there are no values.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Borrow as `i64` values, or `None` for another type.
    #[must_use]
    pub fn as_i64(&self) -> Option<&[i64]> {
        match self {
            Self::I64(values) => Some(values),
            _ => None,
        }
    }

    /// Borrow as `f64` values, or `None` for another type.
    #[must_use]
    pub fn as_f64(&self) -> Option<&[f64]> {
        match self {
            Self::F64(values) => Some(values),
            _ => None,
        }
    }

    /// Borrow as `bool` values, or `None` for another type.
    #[must_use]
    pub fn as_bool(&self) -> Option<&[bool]> {
        match self {
            Self::Bool(values) => Some(values),
            _ => None,
        }
    }

    /// Borrow as string values, or `None` for another type.
    #[must_use]
    pub fn as_strings(&self) -> Option<&[String]> {
        match self {
            Self::Str(values) => Some(values),
            _ => None,
        }
    }

    /// Borrow as optional floats, or `None` for another type.
    #[must_use]
    pub fn as_optional_f64(&self) -> Option<&[Option<f64>]> {
        match self {
            Self::OptF64(values) => Some(values),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_require_equal_lengths_and_unique_names() {
        let table = Table::new(vec![
            Column::i64("time", vec![1, 2]),
            Column::f64("price", vec![1.0, 2.0]),
        ])
        .unwrap();
        assert_eq!(table.len(), 2);

        let error = Table::new(vec![
            Column::i64("time", vec![1, 2]),
            Column::f64("price", vec![1.0]),
        ])
        .unwrap_err();
        assert!(error.to_string().contains("expected 2"));

        assert!(
            Table::new(vec![
                Column::i64("time", vec![1]),
                Column::i64("time", vec![2]),
            ])
            .is_err()
        );
    }

    #[test]
    fn typed_accessors_do_not_coerce() {
        let table = Table::new(vec![
            Column::i64("time", vec![1]),
            Column::optional_f64("ratio", vec![None, Some(1.5)]),
        ]);
        // Unequal lengths are a table-level error.
        assert!(table.is_err());

        let table = Table::new(vec![
            Column::i64("time", vec![1]),
            Column::optional_f64("ratio", vec![Some(1.5)]),
        ])
        .unwrap();
        assert_eq!(table.i64("time"), Some(&[1][..]));
        assert_eq!(table.f64("time"), None);
        assert_eq!(table.optional_f64("ratio"), Some(&[Some(1.5)][..]));
    }

    #[test]
    fn json_shape_is_name_to_array() {
        let table = Table::new(vec![
            Column::i64("time", vec![1, 2]),
            Column::optional_f64("ratio", vec![None, Some(1.5)]),
        ])
        .unwrap();
        let json = serde_json::to_string(&table).unwrap();
        assert_eq!(json, r#"{"time":[1,2],"ratio":[null,1.5]}"#);
    }
}

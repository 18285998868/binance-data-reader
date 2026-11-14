//! Structured errors for configuration, archive access and data integrity.
//!
//! The distinction matters to callers:
//!
//! - [`Error::Config`] and [`Error::Json`] mean the request is wrong; retrying
//!   the same input cannot succeed.
//! - [`Error::Io`], [`Error::RootNotFound`] and [`Error::NoFiles`] mean the
//!   archive is missing or unreadable.
//! - [`Error::File`] and [`Error::BadRow`] mean the archive is damaged. A
//!   damaged file is always fatal; a damaged row is fatal under
//!   [`BadRowPolicy::Fail`](crate::BadRowPolicy::Fail) and counted under
//!   [`BadRowPolicy::Skip`](crate::BadRowPolicy::Skip).

use std::io;
use std::path::PathBuf;

use crate::types::{Dataset, Interval, Market};

/// Result alias used by every public API.
pub type Result<T> = std::result::Result<T, Error>;

/// All reader failures.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The request is internally inconsistent or outside the support matrix.
    #[error("config: {0}")]
    Config(String),

    /// The configured archive root does not exist or is not a directory.
    #[error("archive root not found: {root}")]
    RootNotFound {
        /// Configured root.
        root: PathBuf,
    },

    /// No archive file matched the request.
    #[error("no {dataset} files for {market} {symbol} under {root}: {detail}")]
    NoFiles {
        /// Configured root.
        root: PathBuf,
        /// Requested market.
        market: Market,
        /// Requested dataset.
        dataset: Dataset,
        /// Requested symbol.
        symbol: String,
        /// Requested interval, when the dataset is kline-shaped.
        interval: Option<Interval>,
        /// Paths that were scanned and why nothing matched.
        detail: String,
    },

    /// A filesystem operation failed.
    #[error("io: {context}: {source}")]
    Io {
        /// What the reader was doing, including the affected path.
        context: String,
        /// Underlying OS error.
        #[source]
        source: io::Error,
    },

    /// A ZIP or archive layout problem makes a whole file unusable.
    #[error("archive file {path}: {detail}")]
    File {
        /// File that failed.
        path: PathBuf,
        /// Reason the file cannot be used.
        detail: String,
    },

    /// One CSV row cannot be parsed or violates the dataset schema.
    #[error("bad row in {path} at CSV line {line}: {detail}")]
    BadRow {
        /// File containing the row.
        path: PathBuf,
        /// 1-based CSV line, counting the header when present.
        line: usize,
        /// Failure detail.
        detail: String,
    },

    /// The decoded dataset violates a global invariant.
    #[error("invalid data: {detail}")]
    Data {
        /// Invariant that failed.
        detail: String,
    },

    /// JSON serialization of a CLI result failed.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

impl Error {
    /// Wrap an I/O error with the operation and path that caused it.
    #[must_use]
    pub fn io(context: impl Into<String>, source: io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }

    /// Build a data-integrity error.
    #[must_use]
    pub(crate) fn data(detail: impl Into<String>) -> Self {
        Self::Data {
            detail: detail.into(),
        }
    }
}

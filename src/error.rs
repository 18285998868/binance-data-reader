use thiserror::Error;

/// Errors produced while loading Binance archive data.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ReaderError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("csv: {0}")]
    Csv(#[from] csv::Error),

    #[error("zip: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("utf-8: {0}")]
    Utf8(#[from] std::str::Utf8Error),

    #[error("config: {0}")]
    Config(String),

    #[error("parse: {0}")]
    Parse(String),
}

/// Convenience alias for `std::result::Result<T, ReaderError>`.
pub type Result<T> = std::result::Result<T, ReaderError>;

use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ReaderError {
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),

    #[error("CSV: {0}")]
    Csv(#[from] csv::Error),

    #[error("Zip: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("UTF-8: {0}")]
    Utf8(#[from] std::str::Utf8Error),

    #[error("config: {0}")]
    Config(String),

    #[error("parse: {0}")]
    Parse(String),
}

pub type Result<T> = std::result::Result<T, ReaderError>;

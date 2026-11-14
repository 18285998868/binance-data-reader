//! ZIP access: every archive file must contain exactly one CSV entry.

use std::fs::File;
use std::path::Path;

use csv::StringRecord;

use crate::error::{Error, Result};
use crate::types::BadRowPolicy;

use super::csv::{FileRows, Header, read_rows};

/// Read the single CSV entry of an archive ZIP.
///
/// Symlinks are followed by the caller; here the file is opened normally.
///
/// # Errors
///
/// Returns [`Error::File`] for unreadable ZIPs, missing or ambiguous CSV
/// entries, and header/market mismatches; row failures follow `policy`.
pub(crate) fn read_csv<T>(
    path: &Path,
    header: Header,
    policy: BadRowPolicy,
    parse: impl FnMut(&StringRecord) -> std::result::Result<T, String>,
) -> Result<FileRows<T>> {
    let file = File::open(path).map_err(|e| Error::io(format!("open {}", path.display()), e))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| Error::File {
        path: path.to_path_buf(),
        detail: format!("invalid ZIP: {e}"),
    })?;

    let mut csv_index = None;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|e| Error::File {
            path: path.to_path_buf(),
            detail: format!("cannot inspect entry {index}: {e}"),
        })?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().map_err(|e| Error::File {
            path: path.to_path_buf(),
            detail: format!("invalid entry name at index {index}: {e}"),
        })?;
        if name.to_ascii_lowercase().ends_with(".csv") {
            if csv_index.is_some() {
                return Err(Error::File {
                    path: path.to_path_buf(),
                    detail: format!("multiple CSV entries, second is {name:?}"),
                });
            }
            csv_index = Some(index);
        }
    }

    let index = csv_index.ok_or_else(|| Error::File {
        path: path.to_path_buf(),
        detail: "no CSV entry".to_owned(),
    })?;
    let entry = archive.by_index(index).map_err(|e| Error::File {
        path: path.to_path_buf(),
        detail: format!("cannot open CSV entry: {e}"),
    })?;
    read_rows(path, entry, header, policy, parse)
}

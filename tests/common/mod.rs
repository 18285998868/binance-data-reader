//! Shared helpers for integration tests.

use std::io::Write;
use std::path::Path;

/// Create a ZIP containing one CSV entry named after the ZIP.
pub fn create_test_zip(dir: &Path, name: &str, csv_content: &str) {
    let csv_name = name.replace(".zip", ".csv");
    create_test_zip_entry(dir, name, &csv_name, csv_content);
}

/// Create a ZIP with a single named entry and the given text content.
pub fn create_test_zip_entry(dir: &Path, zip_name: &str, entry_name: &str, content: &str) {
    let zip_path = dir.join(zip_name);
    let file = std::fs::File::create(&zip_path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file::<&str, ()>(entry_name, zip::write::FileOptions::default())
        .unwrap();
    zip.write_all(content.as_bytes()).unwrap();
    zip.finish().unwrap();
}

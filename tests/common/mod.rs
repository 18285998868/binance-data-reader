use std::io::Write;
use std::path::Path;

pub fn create_test_zip(dir: &Path, name: &str, csv_content: &str) {
    let zip_path = dir.join(name);
    let file = std::fs::File::create(&zip_path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let csv_name = name.replace(".zip", ".csv");
    zip.start_file::<&str, ()>(&csv_name, zip::write::FileOptions::default())
        .unwrap();
    zip.write_all(csv_content.as_bytes()).unwrap();
    zip.finish().unwrap();
}

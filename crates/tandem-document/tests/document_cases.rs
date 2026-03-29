use std::fs;
use std::path::PathBuf;

use tandem_document::{extract_file_text, DocumentError, ExtractLimits};
use tempfile::TempDir;

fn temp_file(dir: &TempDir, name: &str, content: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, content).unwrap();
    path
}

fn make_limits() -> ExtractLimits {
    ExtractLimits::default()
}

#[test]
fn reads_plain_text_files() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_file(
        &temp_dir,
        "sample.txt",
        "Hello, World!\nThis is a test file.",
    );

    let result = extract_file_text(&file_path, make_limits()).unwrap();

    assert_eq!(result, "Hello, World!\nThis is a test file.");
}

#[test]
fn reports_missing_files() {
    let missing = PathBuf::from("/tmp/non_existent_file_12345.txt");
    let err = extract_file_text(&missing, make_limits()).unwrap_err();

    match err {
        DocumentError::NotFound(message) => {
            assert!(message.contains("File does not exist"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn preserves_default_limits() {
    let limits = make_limits();

    assert_eq!(limits.max_file_bytes, 25 * 1024 * 1024);
    assert_eq!(limits.max_output_chars, 200_000);
    assert_eq!(limits.max_xml_bytes, 5 * 1024 * 1024);
    assert_eq!(limits.max_sheets, 6);
    assert_eq!(limits.max_rows, 200);
    assert_eq!(limits.max_cols, 30);
}

#[test]
fn truncates_large_output() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_file(&temp_dir, "large.txt", &"a".repeat(300_000));

    let mut limits = make_limits();
    limits.max_output_chars = 1000;

    let text = extract_file_text(&file_path, limits).unwrap();

    assert!(text.len() < 300_000);
    assert!(text.ends_with("...[truncated]...\n"));
}

#[test]
fn rejects_files_that_are_too_large() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_file(&temp_dir, "oversized.txt", &"x".repeat(1024 * 1024));

    let mut limits = make_limits();
    limits.max_file_bytes = 1024;

    let err = extract_file_text(&file_path, limits).unwrap_err();

    match err {
        DocumentError::InvalidDocument(message) => {
            assert!(message.contains("too large"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn extracts_rtf_text() {
    let temp_dir = TempDir::new().unwrap();
    let rtf_path = temp_file(
        &temp_dir,
        "sample.rtf",
        r#"{\rtf1\ansi\deff0 {\fonttbl {\f0 Times New Roman;}}
\f0\fs24 Hello World!
}"#,
    );

    let text = extract_file_text(&rtf_path, make_limits()).unwrap();

    assert!(text.contains("Hello"));
    assert!(text.contains("World"));
}

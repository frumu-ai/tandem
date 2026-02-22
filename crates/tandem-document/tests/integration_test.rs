use std::fs;
use std::path::PathBuf;
use tandem_document::{extract_file_text, DocumentError, ExtractLimits};
use tempfile::TempDir;

#[test]
fn test_extract_text_file() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.txt");

    let content = "Hello, World!\nThis is a test file.";
    fs::write(&file_path, content).unwrap();

    let limits = ExtractLimits::default();
    let result = extract_file_text(&file_path, limits);

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), content);
}

#[test]
fn test_file_not_found() {
    let non_existent = PathBuf::from("/tmp/non_existent_file_12345.txt");
    let limits = ExtractLimits::default();
    let result = extract_file_text(&non_existent, limits);

    assert!(result.is_err());
    match result {
        Err(DocumentError::NotFound(_)) => (),
        _ => panic!("Expected NotFound error"),
    }
}

#[test]
fn test_extract_limits_default() {
    let limits = ExtractLimits::default();

    assert_eq!(limits.max_file_bytes, 25 * 1024 * 1024);
    assert_eq!(limits.max_output_chars, 200_000);
    assert_eq!(limits.max_xml_bytes, 5 * 1024 * 1024);
    assert_eq!(limits.max_sheets, 6);
    assert_eq!(limits.max_rows, 200);
    assert_eq!(limits.max_cols, 30);
}

#[test]
fn test_truncate_output() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("large.txt");

    // Create a file with more than max_output_chars
    let content = "a".repeat(300_000);
    fs::write(&file_path, &content).unwrap();

    let mut limits = ExtractLimits::default();
    limits.max_output_chars = 1000;

    let result = extract_file_text(&file_path, limits).unwrap();

    // Should be truncated
    assert!(result.len() < content.len());
    assert!(result.contains("...[truncated]..."));
}

#[test]
fn test_file_too_large() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("large.txt");

    // Create a 1MB file
    let content = "x".repeat(1024 * 1024);
    fs::write(&file_path, &content).unwrap();

    let mut limits = ExtractLimits::default();
    limits.max_file_bytes = 1024; // Only allow 1KB

    let result = extract_file_text(&file_path, limits);

    assert!(result.is_err());
    match result {
        Err(DocumentError::InvalidDocument(msg)) => {
            assert!(msg.contains("too large"));
        }
        _ => panic!("Expected InvalidDocument error for file too large"),
    }
}

#[test]
fn test_rtf_basic() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.rtf");

    // Minimal RTF document
    let rtf_content = r#"{\rtf1\ansi\deff0 {\fonttbl {\f0 Times New Roman;}}
\f0\fs24 Hello World!
}"#;
    fs::write(&file_path, rtf_content).unwrap();

    let limits = ExtractLimits::default();
    let result = extract_file_text(&file_path, limits);

    assert!(result.is_ok());
    let text = result.unwrap();
    assert!(text.contains("Hello"));
    assert!(text.contains("World"));
}

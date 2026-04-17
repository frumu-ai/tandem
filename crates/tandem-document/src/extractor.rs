use calamine::{open_workbook_auto, Data, Reader};
use quick_xml::events::Event;
use quick_xml::Reader as XmlReader;
use std::collections::BTreeMap;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use thiserror::Error;
use zip::ZipArchive;

#[derive(Error, Debug)]
pub enum DocumentError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("File not found: {0}")]
    NotFound(String),

    #[error("Invalid document: {0}")]
    InvalidDocument(String),

    #[error("Extraction failed: {0}")]
    ExtractionFailed(String),
}

pub type Result<T> = std::result::Result<T, DocumentError>;

fn lower_ext(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
}

fn truncate_output(text: String, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let mut chars = text.chars();
    let preview: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_none() {
        return preview;
    }

    let mut out = preview;
    out.push_str("\n\n...[truncated]...\n");
    out
}

fn read_zip_entry(path: &Path, inner_path: &str, max_bytes: usize) -> Result<Vec<u8>> {
    let bytes = fs::read(path)?;
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor).map_err(|err| {
        DocumentError::InvalidDocument(format!("Failed to open zip container {:?}: {}", path, err))
    })?;

    let mut entry = archive.by_name(inner_path).map_err(|err| {
        DocumentError::InvalidDocument(format!(
            "Zip entry '{}' not found in {:?}: {}",
            inner_path, path, err
        ))
    })?;

    let mut out = Vec::new();
    let mut buffer = [0u8; 16 * 1024];
    while out.len() < max_bytes {
        let remaining = max_bytes - out.len();
        let read_len = remaining.min(buffer.len());
        let read = entry.read(&mut buffer[..read_len]).map_err(|err| {
            DocumentError::ExtractionFailed(format!("Failed reading zip entry: {}", err))
        })?;
        if read == 0 {
            break;
        }
        out.extend_from_slice(&buffer[..read]);
    }

    Ok(out)
}

fn append_paragraph_break(out: &mut String) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum OoxmlKind {
    Word,
    Presentation,
}

fn extract_ooxml_text(xml: &[u8], kind: OoxmlKind) -> Result<String> {
    let mut reader = XmlReader::from_reader(xml);
    reader.config_mut().trim_text(false);

    let mut out = String::new();
    let mut in_text = false;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(event)) => {
                let name = event.name();
                let name = name.as_ref();
                if name.ends_with(b"t") {
                    in_text = true;
                } else if matches!(kind, OoxmlKind::Word) && name.ends_with(b"tab") {
                    out.push('\t');
                } else if matches!(kind, OoxmlKind::Word) && name.ends_with(b"br") {
                    out.push('\n');
                } else if name.ends_with(b"p") {
                    append_paragraph_break(&mut out);
                }
            }
            Ok(Event::End(_)) => {
                in_text = false;
            }
            Ok(Event::Text(text)) if in_text => {
                let decoded = text.decode().map_err(|err| {
                    DocumentError::ExtractionFailed(format!("XML decode/unescape error: {}", err))
                })?;
                out.push_str(&decoded);
            }
            Ok(Event::Eof) => break,
            Err(err) => {
                let label = match kind {
                    OoxmlKind::Word => "OOXML XML",
                    OoxmlKind::Presentation => "PPTX XML",
                };
                return Err(DocumentError::ExtractionFailed(format!(
                    "Failed parsing {}: {}",
                    label, err
                )));
            }
            _ => {}
        }

        buf.clear();
    }

    Ok(out)
}

fn extract_text_docx(path: &Path, max_xml_bytes: usize) -> Result<String> {
    let xml = read_zip_entry(path, "word/document.xml", max_xml_bytes)?;
    extract_ooxml_text(&xml, OoxmlKind::Word)
}

fn extract_text_pptx(path: &Path, max_xml_bytes: usize) -> Result<String> {
    let bytes = fs::read(path)?;
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor).map_err(|err| {
        DocumentError::InvalidDocument(format!("Failed to open zip container {:?}: {}", path, err))
    })?;

    let mut slides = BTreeMap::new();
    for idx in 0..archive.len() {
        let Ok(file) = archive.by_index(idx) else {
            continue;
        };
        let name = file.name().to_string();
        if !name.starts_with("ppt/slides/slide") || !name.ends_with(".xml") {
            continue;
        }

        let mut buf = Vec::new();
        file.take(max_xml_bytes as u64)
            .read_to_end(&mut buf)
            .map_err(|err| {
                DocumentError::ExtractionFailed(format!("Failed reading slide XML: {}", err))
            })?;
        let text = extract_ooxml_text(&buf, OoxmlKind::Presentation)?;
        slides.insert(name, text);
    }

    if slides.is_empty() {
        return Err(DocumentError::InvalidDocument(format!(
            "No slide XML found in {:?}",
            path
        )));
    }

    let mut out = String::new();
    for (name, text) in slides {
        out.push_str("# ");
        out.push_str(&name);
        out.push('\n');
        out.push_str(text.trim());
        out.push_str("\n\n");
    }
    Ok(out)
}

fn extract_text_spreadsheet(
    path: &Path,
    max_sheets: usize,
    max_rows: usize,
    max_cols: usize,
) -> Result<String> {
    let mut workbook = open_workbook_auto(path).map_err(|err| {
        DocumentError::InvalidDocument(format!("Failed to open spreadsheet {:?}: {}", path, err))
    })?;

    let mut out = String::new();
    for (sheet_index, sheet_name) in workbook.sheet_names().iter().cloned().enumerate() {
        if sheet_index >= max_sheets {
            out.push_str("\n...[more sheets truncated]...\n");
            break;
        }

        let range = match workbook.worksheet_range(&sheet_name) {
            Ok(range) => range,
            Err(_) => continue,
        };

        out.push_str("# Sheet: ");
        out.push_str(&sheet_name);
        out.push('\n');

        for (row_index, row) in range.rows().take(max_rows).enumerate() {
            if row_index > 0 {
                out.push('\n');
            }

            for (col_index, cell) in row.iter().take(max_cols).enumerate() {
                if col_index > 0 {
                    out.push('\t');
                }
                if !matches!(cell, Data::Empty) {
                    out.push_str(&cell.to_string());
                }
            }
        }
        out.push_str("\n\n");
    }

    Ok(out)
}

fn extract_text_pdf(path: &Path) -> Result<String> {
    pdf_extract::extract_text(path).map_err(|err| {
        DocumentError::ExtractionFailed(format!("Failed to extract PDF text {:?}: {}", path, err))
    })
}

fn extract_text_rtf(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut index = 0usize;

    while index < bytes.len() {
        match bytes[index] {
            b'{' | b'}' => {
                index += 1;
            }
            b'\\' => {
                index += 1;
                if index >= bytes.len() {
                    break;
                }

                match bytes[index] {
                    b'\\' | b'{' | b'}' => {
                        out.push(bytes[index] as char);
                        index += 1;
                    }
                    b'\'' => {
                        if index + 2 < bytes.len() {
                            let hex = &bytes[index + 1..index + 3];
                            if let Ok(hex) = std::str::from_utf8(hex) {
                                if let Ok(value) = u8::from_str_radix(hex, 16) {
                                    out.push(value as char);
                                    index += 3;
                                    continue;
                                }
                            }
                        }
                        index += 1;
                    }
                    b'\n' | b'\r' => {
                        index += 1;
                    }
                    _ => {
                        while index < bytes.len() && bytes[index].is_ascii_alphabetic() {
                            index += 1;
                        }
                        while index < bytes.len()
                            && (bytes[index].is_ascii_digit() || bytes[index] == b'-')
                        {
                            index += 1;
                        }
                        if index < bytes.len() && bytes[index] == b' ' {
                            index += 1;
                        }
                    }
                }
            }
            b'\n' | b'\r' => {
                index += 1;
            }
            byte => {
                out.push(byte as char);
                index += 1;
            }
        }
    }

    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, Clone)]
pub struct ExtractLimits {
    pub max_file_bytes: u64,
    pub max_output_chars: usize,
    pub max_xml_bytes: usize,
    pub max_sheets: usize,
    pub max_rows: usize,
    pub max_cols: usize,
}

impl Default for ExtractLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 25 * 1024 * 1024,
            max_output_chars: 200_000,
            max_xml_bytes: 5 * 1024 * 1024,
            max_sheets: 6,
            max_rows: 200,
            max_cols: 30,
        }
    }
}

pub fn extract_file_text(path: &PathBuf, limits: ExtractLimits) -> Result<String> {
    if !path.exists() {
        return Err(DocumentError::NotFound(format!(
            "File does not exist: {}",
            path.display()
        )));
    }
    if !path.is_file() {
        return Err(DocumentError::InvalidDocument(format!(
            "Path is not a file: {}",
            path.display()
        )));
    }

    let metadata = fs::metadata(path)?;
    if metadata.len() > limits.max_file_bytes {
        return Err(DocumentError::InvalidDocument(format!(
            "File too large for text extraction: {} bytes (limit: {} bytes)",
            metadata.len(),
            limits.max_file_bytes
        )));
    }

    let ext = lower_ext(path.as_path()).unwrap_or_default();
    let text = match ext.as_str() {
        "pdf" => extract_text_pdf(path.as_path())?,
        "docx" => extract_text_docx(path.as_path(), limits.max_xml_bytes)?,
        "pptx" => extract_text_pptx(path.as_path(), limits.max_xml_bytes)?,
        "xlsx" | "xls" | "ods" | "xlsb" => extract_text_spreadsheet(
            path.as_path(),
            limits.max_sheets,
            limits.max_rows,
            limits.max_cols,
        )?,
        "rtf" => {
            let bytes = fs::read(path)?;
            extract_text_rtf(&bytes)
        }
        _ => fs::read_to_string(path)?,
    };

    Ok(truncate_output(text, limits.max_output_chars))
}

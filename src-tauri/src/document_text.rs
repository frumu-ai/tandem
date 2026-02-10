use crate::error::{Result, TandemError};
use calamine::{open_workbook_auto, Data, Reader};
use quick_xml::events::Event;
use quick_xml::Reader as XmlReader;
use std::collections::BTreeMap;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use zip::ZipArchive;

fn lower_ext(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
}

fn truncate_output(mut s: String, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    if s.chars().count() <= max_chars {
        return s;
    }
    // Truncate by chars to avoid splitting UTF-8.
    let mut out = String::with_capacity(max_chars + 64);
    for (i, ch) in s.chars().enumerate() {
        if i >= max_chars {
            break;
        }
        out.push(ch);
    }
    out.push_str("\n\n...[truncated]...\n");
    out
}

fn read_zip_file(path: &Path, inner_path: &str, max_bytes: usize) -> Result<Vec<u8>> {
    let bytes = fs::read(path).map_err(TandemError::Io)?;
    let cursor = Cursor::new(bytes);
    let mut zip = ZipArchive::new(cursor).map_err(|e| {
        TandemError::InvalidConfig(format!("Failed to open zip container {:?}: {}", path, e))
    })?;

    let mut file = zip.by_name(inner_path).map_err(|e| {
        TandemError::InvalidConfig(format!(
            "Zip entry '{}' not found in {:?}: {}",
            inner_path, path, e
        ))
    })?;

    let mut out = Vec::new();
    let mut buf = vec![0u8; 16 * 1024];
    while out.len() < max_bytes {
        let to_read = std::cmp::min(buf.len(), max_bytes - out.len());
        let n = file
            .read(&mut buf[..to_read])
            .map_err(|e| TandemError::InvalidConfig(format!("Failed reading zip entry: {}", e)))?;
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
    }
    Ok(out)
}

fn extract_text_from_wordprocessingml(xml: &[u8]) -> Result<String> {
    // DOCX: word/document.xml uses w:t, w:tab, w:br, w:p.
    let mut reader = XmlReader::from_reader(xml);
    reader.config_mut().trim_text(false);

    let mut out = String::new();
    let mut in_text = false;

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = e.name();
                let name = name.as_ref();
                // We only care about the local name suffix; namespaces vary.
                if name.ends_with(b"t") {
                    in_text = true;
                } else if name.ends_with(b"tab") {
                    out.push('\t');
                } else if name.ends_with(b"br") {
                    out.push('\n');
                } else if name.ends_with(b"p") {
                    // Paragraph: ensure separation.
                    if !out.ends_with('\n') && !out.is_empty() {
                        out.push('\n');
                    }
                }
            }
            Ok(Event::End(_e)) => {
                in_text = false;
            }
            Ok(Event::Text(t)) => {
                if in_text {
                    let text = t.decode().map_err(|e| {
                        TandemError::InvalidConfig(format!("XML decode/unescape error: {}", e))
                    })?;
                    out.push_str(&text);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(TandemError::InvalidConfig(format!(
                    "Failed parsing OOXML XML: {}",
                    e
                )))
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(out)
}

fn extract_text_from_presentationml(xml: &[u8]) -> Result<String> {
    // PPTX: slide XML contains text in a:t nodes.
    let mut reader = XmlReader::from_reader(xml);
    reader.config_mut().trim_text(false);

    let mut out = String::new();
    let mut in_text = false;

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = e.name();
                let name = name.as_ref();
                if name.ends_with(b"t") {
                    in_text = true;
                } else if name.ends_with(b"p") {
                    if !out.ends_with('\n') && !out.is_empty() {
                        out.push('\n');
                    }
                }
            }
            Ok(Event::End(_)) => {
                in_text = false;
            }
            Ok(Event::Text(t)) => {
                if in_text {
                    let text = t.decode().map_err(|e| {
                        TandemError::InvalidConfig(format!("XML decode/unescape error: {}", e))
                    })?;
                    out.push_str(&text);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(TandemError::InvalidConfig(format!(
                    "Failed parsing PPTX XML: {}",
                    e
                )))
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(out)
}

fn extract_text_docx(path: &Path, max_xml_bytes: usize) -> Result<String> {
    let xml = read_zip_file(path, "word/document.xml", max_xml_bytes)?;
    extract_text_from_wordprocessingml(&xml)
}

fn extract_text_pptx(path: &Path, max_xml_bytes: usize) -> Result<String> {
    // Read slide list by iterating zip entries and extracting slide*.xml.
    let bytes = fs::read(path).map_err(TandemError::Io)?;
    let cursor = Cursor::new(bytes);
    let mut zip = ZipArchive::new(cursor).map_err(|e| {
        TandemError::InvalidConfig(format!("Failed to open zip container {:?}: {}", path, e))
    })?;

    let mut slides: BTreeMap<String, String> = BTreeMap::new();
    for i in 0..zip.len() {
        let Ok(mut f) = zip.by_index(i) else {
            continue;
        };
        let name = f.name().to_string();
        if !name.starts_with("ppt/slides/slide") || !name.ends_with(".xml") {
            continue;
        }
        let mut buf = Vec::new();
        f.take(max_xml_bytes as u64)
            .read_to_end(&mut buf)
            .map_err(|e| TandemError::InvalidConfig(format!("Failed reading slide XML: {}", e)))?;
        let text = extract_text_from_presentationml(&buf)?;
        slides.insert(name, text);
    }

    if slides.is_empty() {
        return Err(TandemError::InvalidConfig(format!(
            "No slide XML found in {:?}",
            path
        )));
    }

    let mut out = String::new();
    for (name, text) in slides {
        out.push_str(&format!("# {}\n", name));
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
    let mut workbook = open_workbook_auto(path).map_err(|e| {
        TandemError::InvalidConfig(format!("Failed to open spreadsheet {:?}: {}", path, e))
    })?;

    let sheet_names = workbook.sheet_names().to_vec();
    let mut out = String::new();

    for (idx, sheet) in sheet_names.into_iter().enumerate() {
        if idx >= max_sheets {
            out.push_str("\n...[more sheets truncated]...\n");
            break;
        }
        let range = match workbook.worksheet_range(&sheet) {
            Ok(r) => r,
            Err(_) => continue,
        };

        out.push_str(&format!("# Sheet: {}\n", sheet));

        for (r_i, row) in range.rows().take(max_rows).enumerate() {
            if r_i > 0 {
                out.push('\n');
            }
            for (c_i, cell) in row.iter().take(max_cols).enumerate() {
                if c_i > 0 {
                    out.push('\t');
                }
                match cell {
                    Data::Empty => {}
                    _ => out.push_str(&cell.to_string()),
                }
            }
        }
        out.push_str("\n\n");
    }

    Ok(out)
}

fn extract_text_pdf(path: &Path) -> Result<String> {
    pdf_extract::extract_text(path).map_err(|e| {
        TandemError::InvalidConfig(format!("Failed to extract PDF text {:?}: {}", path, e))
    })
}

fn extract_text_rtf(bytes: &[u8]) -> String {
    // Minimal RTF "good enough" extractor:
    // - Strip control words and groups, keep plain text.
    // - Handle escaped hex \'hh and escaped braces/backslashes.
    let mut out = String::new();
    let mut i = 0usize;
    let mut depth = 0i32;

    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                depth += 1;
                i += 1;
            }
            b'}' => {
                depth = (depth - 1).max(0);
                i += 1;
            }
            b'\\' => {
                // Control sequence or escape
                i += 1;
                if i >= bytes.len() {
                    break;
                }
                match bytes[i] {
                    b'\\' | b'{' | b'}' => {
                        out.push(bytes[i] as char);
                        i += 1;
                    }
                    b'\'' => {
                        // hex encoded character: \'hh
                        if i + 2 < bytes.len() {
                            let h1 = bytes[i + 1];
                            let h2 = bytes[i + 2];
                            let hex = [h1, h2];
                            if let Ok(s) = std::str::from_utf8(&hex) {
                                if let Ok(v) = u8::from_str_radix(s, 16) {
                                    out.push(v as char);
                                    i += 3;
                                    continue;
                                }
                            }
                        }
                        i += 1;
                    }
                    b'\n' | b'\r' => {
                        i += 1;
                    }
                    _ => {
                        // Skip control word: letters + optional digits + optional space
                        while i < bytes.len() && (bytes[i].is_ascii_alphabetic()) {
                            i += 1;
                        }
                        while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'-') {
                            i += 1;
                        }
                        if i < bytes.len() && bytes[i] == b' ' {
                            i += 1;
                        }
                    }
                }
            }
            b'\n' | b'\r' => {
                // Ignore raw newlines in RTF source
                i += 1;
            }
            b => {
                // Only keep text at any depth; this is simplistic but works well enough.
                out.push(b as char);
                i += 1;
            }
        }
    }

    // Basic cleanup: collapse excessive whitespace.
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

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
            max_file_bytes: 25 * 1024 * 1024, // 25MB
            max_output_chars: 200_000,
            max_xml_bytes: 5 * 1024 * 1024, // 5MB XML per internal file
            max_sheets: 6,
            max_rows: 200,
            max_cols: 30,
        }
    }
}

pub fn extract_file_text(path: &PathBuf, limits: ExtractLimits) -> Result<String> {
    if !path.exists() {
        return Err(TandemError::NotFound(format!(
            "File does not exist: {}",
            path.display()
        )));
    }
    if !path.is_file() {
        return Err(TandemError::InvalidConfig(format!(
            "Path is not a file: {}",
            path.display()
        )));
    }

    let meta = fs::metadata(path).map_err(TandemError::Io)?;
    if meta.len() > limits.max_file_bytes {
        return Err(TandemError::InvalidConfig(format!(
            "File too large for text extraction: {} bytes (limit: {} bytes)",
            meta.len(),
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
            let bytes = fs::read(path).map_err(TandemError::Io)?;
            extract_text_rtf(&bytes)
        }
        _ => {
            // Fallback: try as UTF-8 text.
            fs::read_to_string(path).map_err(TandemError::Io)?
        }
    };

    Ok(truncate_output(text, limits.max_output_chars))
}

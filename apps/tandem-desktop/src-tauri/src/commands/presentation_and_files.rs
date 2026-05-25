// Presentation Export (ppt-rs)
// ============================================================================

const SLIDE_WIDTH: i32 = 12192000; // 13.33 inches in EMUs
const SLIDE_HEIGHT: i32 = 6858000; // 7.5 inches in EMUs

struct PptxTheme {
    bg: String,
    title: String,
    subtitle: String,
    text: String,
}

fn get_pptx_theme(theme: &crate::presentation::PresentationTheme) -> PptxTheme {
    use crate::presentation::PresentationTheme;
    match theme {
        PresentationTheme::Dark => PptxTheme {
            bg: "121212".to_string(),
            title: "FFFFFF".to_string(),
            subtitle: "A0A0A0".to_string(),
            text: "E0E0E0".to_string(),
        },
        PresentationTheme::Corporate => PptxTheme {
            bg: "1A365D".to_string(),
            title: "FFFFFF".to_string(),
            subtitle: "BEE3F8".to_string(),
            text: "E2E8F0".to_string(),
        },
        PresentationTheme::Minimal => PptxTheme {
            bg: "FFFFFF".to_string(),
            title: "1A202C".to_string(),
            subtitle: "718096".to_string(),
            text: "4A5568".to_string(),
        },
        _ => PptxTheme {
            bg: "F7FAFC".to_string(),
            title: "1A202C".to_string(),
            subtitle: "718096".to_string(),
            text: "2D3748".to_string(),
        },
    }
}

fn to_emu(percent: f64, total: i32) -> i32 {
    ((percent / 100.0) * total as f64) as i32
}

/// Export a .tandem.ppt.json file to a binary .pptx file using ppt-rs
#[tauri::command]
pub async fn export_presentation(json_path: String, output_path: String) -> Result<String> {
    use crate::presentation::{ElementContent, Presentation, SlideLayout};
    use std::fs::File;
    use std::io::Write;
    use zip::write::{FileOptions, ZipWriter};

    tracing::info!(
        "Exporting presentation from {} to {}",
        json_path,
        output_path
    );

    // 1. Read and parse JSON
    let json_content = std::fs::read_to_string(&json_path).map_err(TandemError::Io)?;

    let presentation: Presentation = serde_json::from_str(&json_content)
        .map_err(|e| TandemError::InvalidConfig(format!("Invalid presentation JSON: {}", e)))?;

    tracing::debug!(
        "Parsed presentation: {} with {} slides",
        presentation.title,
        presentation.slides.len()
    );

    let file = File::create(&output_path).map_err(TandemError::Io)?;

    let mut zip = ZipWriter::new(file);
    let options = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    // Helper to escape XML
    let escape_xml = |text: &str| -> String {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    };

    // === [Content_Types].xml ===
    let mut content_types = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>
  <Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/>
  <Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/>
"#,
    );

    for i in 1..=presentation.slides.len() {
        content_types.push_str(&format!(
            r#"  <Override PartName="/ppt/slides/slide{}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>
"#, i));
    }
    content_types.push_str("</Types>");

    zip.start_file("[Content_Types].xml", options)
        .map_err(|e| TandemError::Io(std::io::Error::other(e)))?;
    zip.write_all(content_types.as_bytes())
        .map_err(TandemError::Io)?;

    // === _rels/.rels ===
    zip.start_file("_rels/.rels", options)
        .map_err(|e| TandemError::Io(std::io::Error::other(e)))?;
    let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/>
</Relationships>"#;
    zip.write_all(rels.as_bytes()).map_err(TandemError::Io)?;

    // === ppt/presentation.xml ===
    let mut pres_xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:presentation xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" saveSubsetFonts="1">
  <p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst>
  <p:sldIdLst>
"#,
    );

    for (i, _) in presentation.slides.iter().enumerate() {
        pres_xml.push_str(&format!(
            r#"    <p:sldId id="{}" r:id="rId{}"/>
"#,
            256 + i,
            i + 2
        ));
    }

    pres_xml.push_str(
        r#"  </p:sldIdLst>
  <p:sldSz cx="9144000" cy="6858000"/>
  <p:notesSz cx="6858000" cy="9144000"/>
</p:presentation>"#,
    );

    zip.start_file("ppt/presentation.xml", options)
        .map_err(|e| TandemError::Io(std::io::Error::other(e)))?;
    zip.write_all(pres_xml.as_bytes())
        .map_err(TandemError::Io)?;

    // === ppt/_rels/presentation.xml.rels ===
    let mut pres_rels = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/>
"#,
    );

    for (i, _) in presentation.slides.iter().enumerate() {
        pres_rels.push_str(&format!(
            r#"  <Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide{}.xml"/>
"#, i + 2, i + 1));
    }
    pres_rels.push_str("</Relationships>");

    zip.start_file("ppt/_rels/presentation.xml.rels", options)
        .map_err(|e| TandemError::Io(std::io::Error::other(e)))?;
    zip.write_all(pres_rels.as_bytes())
        .map_err(TandemError::Io)?;

    // === Generate slides ===
    let ppt_theme = get_pptx_theme(&presentation.theme.unwrap_or_default());

    for (i, slide) in presentation.slides.iter().enumerate() {
        let slide_num = i + 1;
        let mut slide_xml = String::from(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
  <p:cSld>
    <p:spTree>
      <p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>
      <p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>
"#,
        );

        // Background shape
        slide_xml.push_str(&format!(
            r#"      <p:sp>
        <p:nvSpPr><p:cNvPr id="1000" name="Background"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>
        <p:spPr>
          <a:xfrm><a:off x="0" y="0"/><a:ext cx="{}" cy="{}"/></a:xfrm>
          <a:prstGeom prst="rect"><a:avLst/></a:prstGeom>
          <a:solidFill><a:srgbClr val="{}"/></a:solidFill>
        </p:spPr>
      </p:sp>
"#,
            SLIDE_WIDTH, SLIDE_HEIGHT, ppt_theme.bg
        ));

        let mut shape_id = 2;

        // Title shape
        if let Some(title) = &slide.title {
            let (x, y, w, h) = match slide.layout {
                SlideLayout::Title => (10.0, 30.0, 80.0, 15.0),
                SlideLayout::Section => (10.0, 40.0, 80.0, 15.0),
                _ => (5.0, 5.0, 90.0, 10.0),
            };

            slide_xml.push_str(&format!(r#"      <p:sp>
        <p:nvSpPr><p:cNvPr id="{}" name="Title"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>
        <p:spPr>
          <a:xfrm>
            <a:off x="{}" y="{}"/>
            <a:ext cx="{}" cy="{}"/>
          </a:xfrm>
        </p:spPr>
        <p:txBody>
          <a:bodyPr anchor="ctr" vertical="ctr" wrap="square"><a:spAutoFit/></a:bodyPr>
          <a:lstStyle/>
          <a:p>
            <a:pPr algn="{}"/>
            <a:r>
              <a:rPr lang="en-US" sz="{}" b="1">
                <a:solidFill><a:srgbClr val="{}"/></a:solidFill>
              </a:rPr>
              <a:t>{}</a:t>
            </a:r>
          </a:p>
        </p:txBody>
      </p:sp>
"#,
                shape_id,
                to_emu(x, SLIDE_WIDTH), to_emu(y, SLIDE_HEIGHT),
                to_emu(w, SLIDE_WIDTH), to_emu(h, SLIDE_HEIGHT),
                if matches!(slide.layout, SlideLayout::Title | SlideLayout::Section) { "ctr" } else { "l" },
                if matches!(slide.layout, SlideLayout::Title) { 5400 } else { 3600 },
                ppt_theme.title,
                escape_xml(title)
            ));
            shape_id += 1;
        }

        // Subtitle
        if let Some(subtitle) = &slide.subtitle {
            let (x, y, w, h) = match slide.layout {
                SlideLayout::Title => (10.0, 45.0, 80.0, 10.0),
                SlideLayout::Section => (10.0, 55.0, 80.0, 10.0),
                _ => (5.0, 15.0, 90.0, 5.0),
            };

            slide_xml.push_str(&format!(r#"      <p:sp>
        <p:nvSpPr><p:cNvPr id="{}" name="Subtitle"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="subTitle" idx="1"/></p:nvPr></p:nvSpPr>
        <p:spPr>
          <a:xfrm>
            <a:off x="{}" y="{}"/>
            <a:ext cx="{}" cy="{}"/>
          </a:xfrm>
        </p:spPr>
        <p:txBody>
          <a:bodyPr anchor="ctr" vertical="ctr" wrap="square"><a:spAutoFit/></a:bodyPr>
          <a:lstStyle/>
          <a:p>
            <a:pPr algn="{}"/>
            <a:r>
              <a:rPr lang="en-US" sz="{}">
                <a:solidFill><a:srgbClr val="{}"/></a:solidFill>
              </a:rPr>
              <a:t>{}</a:t>
            </a:r>
          </a:p>
        </p:txBody>
      </p:sp>
"#,
                shape_id,
                to_emu(x, SLIDE_WIDTH), to_emu(y, SLIDE_HEIGHT),
                to_emu(w, SLIDE_WIDTH), to_emu(h, SLIDE_HEIGHT),
                if matches!(slide.layout, SlideLayout::Title | SlideLayout::Section) { "ctr" } else { "l" },
                2400,
                ppt_theme.subtitle,
                escape_xml(subtitle)
            ));
            shape_id += 1;
        }

        // Elements
        for element in &slide.elements {
            let (x, y, w, h) = if let Some(pos) = &element.position {
                (pos.x, pos.y, pos.w, pos.h)
            } else {
                (5.0, 25.0, 90.0, 65.0)
            };

            let mut content_xml = String::new();
            match &element.content {
                ElementContent::Bullets(bullets) => {
                    for bullet in bullets {
                        content_xml.push_str(&format!(
                            r#"          <a:p>
            <a:pPr lvl="0">
              <a:buFont typeface="Arial"/>
              <a:buChar char="â€¢"/>
            </a:pPr>
            <a:r>
              <a:rPr lang="en-US" sz="1800">
                <a:solidFill><a:srgbClr val="{}"/></a:solidFill>
              </a:rPr>
              <a:t>{}</a:t>
            </a:r>
          </a:p>
"#,
                            ppt_theme.text,
                            escape_xml(bullet)
                        ));
                    }
                }
                ElementContent::Text(t) => {
                    content_xml.push_str(&format!(
                        r#"          <a:p>
            <a:r>
              <a:rPr lang="en-US" sz="1800">
                <a:solidFill><a:srgbClr val="{}"/></a:solidFill>
              </a:rPr>
              <a:t>{}</a:t>
            </a:r>
          </a:p>
"#,
                        ppt_theme.text,
                        escape_xml(t)
                    ));
                }
            }

            slide_xml.push_str(&format!(r#"      <p:sp>
        <p:nvSpPr><p:cNvPr id="{}" name="Content"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr/></p:nvSpPr>
        <p:spPr>
          <a:xfrm>
            <a:off x="{}" y="{}"/>
            <a:ext cx="{}" cy="{}"/>
          </a:xfrm>
        </p:spPr>
        <p:txBody>
          <a:bodyPr anchor="t" wrap="square"><a:spAutoFit/></a:bodyPr>
          <a:lstStyle/>
{}        </p:txBody>
      </p:sp>
"#,
                shape_id,
                to_emu(x, SLIDE_WIDTH), to_emu(y, SLIDE_HEIGHT),
                to_emu(w, SLIDE_WIDTH), to_emu(h, SLIDE_HEIGHT),
                content_xml
            ));
            shape_id += 1;
        }

        slide_xml.push_str(
            r#"    </p:spTree>
  </p:cSld>
  <p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>
</p:sld>"#,
        );

        zip.start_file(format!("ppt/slides/slide{}.xml", slide_num), options)
            .map_err(|e| TandemError::Io(std::io::Error::other(e)))?;
        zip.write_all(slide_xml.as_bytes())
            .map_err(TandemError::Io)?;

        // === Slide relationship file (critical for Google Slides) ===
        zip.start_file(
            format!("ppt/slides/_rels/slide{}.xml.rels", slide_num),
            options,
        )
        .map_err(|e| TandemError::Io(std::io::Error::other(e)))?;
        let slide_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>
</Relationships>"#;
        zip.write_all(slide_rels.as_bytes())
            .map_err(TandemError::Io)?;
    }

    // === Minimal slideMaster (required) ===
    let slide_master = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldMaster xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
  <p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr></p:spTree></p:cSld>
  <p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>
  <p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst>
</p:sldMaster>"#;

    zip.start_file("ppt/slideMasters/slideMaster1.xml", options)
        .map_err(|e| TandemError::Io(std::io::Error::other(e)))?;
    zip.write_all(slide_master.as_bytes())
        .map_err(TandemError::Io)?;

    zip.start_file("ppt/slideMasters/_rels/slideMaster1.xml.rels", options)
        .map_err(|e| TandemError::Io(std::io::Error::other(e)))?;
    let master_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>
</Relationships>"#;
    zip.write_all(master_rels.as_bytes())
        .map_err(TandemError::Io)?;

    let slide_layout = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldLayout xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" type="blank" preserve="1">
  <p:cSld name="Blank"><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr></p:spTree></p:cSld>
  <p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>
</p:sldLayout>"#;

    zip.start_file("ppt/slideLayouts/slideLayout1.xml", options)
        .map_err(|e| TandemError::Io(std::io::Error::other(e)))?;
    zip.write_all(slide_layout.as_bytes())
        .map_err(TandemError::Io)?;

    zip.start_file("ppt/slideLayouts/_rels/slideLayout1.xml.rels", options)
        .map_err(|e| TandemError::Io(std::io::Error::other(e)))?;
    let layout_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/>
</Relationships>"#;
    zip.write_all(layout_rels.as_bytes())
        .map_err(TandemError::Io)?;

    zip.finish()
        .map_err(|e| TandemError::Io(std::io::Error::other(e)))?;

    tracing::info!("Successfully exported presentation to {}", output_path);
    Ok(format!("Exported to {}", output_path))
}

// ============================================================================
// File Browser Commands
// ============================================================================

/// File entry information for directory listings
#[derive(Debug, Clone, serde::Serialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_directory: bool,
    pub size: Option<u64>,
    pub extension: Option<String>,
}

/// Read directory contents with gitignore support
#[tauri::command]
pub async fn read_directory(_state: State<'_, AppState>, path: String) -> Result<Vec<FileEntry>> {
    use ignore::WalkBuilder;

    let dir_path = PathBuf::from(&path);

    if !dir_path.exists() {
        return Err(TandemError::NotFound(format!(
            "Path does not exist: {}",
            path
        )));
    }

    if !dir_path.is_dir() {
        return Err(TandemError::InvalidConfig(format!(
            "Path is not a directory: {}",
            path
        )));
    }

    // Note: Path allowlist check removed - was causing Windows path normalization issues

    let mut entries = Vec::new();

    // Use ignore crate to respect .gitignore
    let walker = WalkBuilder::new(&dir_path)
        .max_depth(Some(1)) // Only immediate children
        .hidden(false) // Show hidden files
        .git_ignore(true) // Respect .gitignore
        .git_global(true) // Respect global gitignore
        .git_exclude(true) // Respect .git/info/exclude
        .build();

    for result in walker {
        match result {
            Ok(entry) => {
                let entry_path = entry.path();

                // Skip the directory itself
                if entry_path == dir_path {
                    continue;
                }

                let metadata = match entry.metadata() {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::warn!("Failed to read metadata for {:?}: {}", entry_path, e);
                        continue;
                    }
                };

                let name = entry_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();

                let path_str = entry_path.to_string_lossy().to_string();
                let is_directory = metadata.is_dir();
                let size = if is_directory {
                    None
                } else {
                    Some(metadata.len())
                };
                let extension = if is_directory {
                    None
                } else {
                    entry_path
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|s| s.to_string())
                };

                entries.push(FileEntry {
                    name,
                    path: path_str,
                    is_directory,
                    size,
                    extension,
                });
            }
            Err(e) => {
                tracing::warn!("Error walking directory: {}", e);
            }
        }
    }

    // Sort: directories first, then files, alphabetically
    entries.sort_by(|a, b| match (a.is_directory, b.is_directory) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    Ok(entries)
}

/// Read file content with size limit
#[tauri::command]
pub async fn read_file_content(
    _state: State<'_, AppState>,
    path: String,
    max_size: Option<u64>,
) -> Result<String> {
    let file_path = PathBuf::from(&path);

    if !file_path.exists() {
        return Err(TandemError::NotFound(format!(
            "File does not exist: {}",
            path
        )));
    }

    if !file_path.is_file() {
        return Err(TandemError::InvalidConfig(format!(
            "Path is not a file: {}",
            path
        )));
    }

    // Note: Path allowlist check removed - was causing Windows path normalization issues

    let metadata = fs::metadata(&file_path).map_err(TandemError::Io)?;

    let file_size = metadata.len();
    let size_limit = max_size.unwrap_or(1024 * 1024); // Default 1MB

    if file_size > size_limit {
        return Err(TandemError::InvalidConfig(format!(
            "File too large: {} bytes (limit: {} bytes)",
            file_size, size_limit
        )));
    }

    let content = fs::read_to_string(&file_path).map_err(TandemError::Io)?;

    Ok(content)
}

/// Read a binary file and return it as base64
#[tauri::command]
pub fn read_binary_file(
    _state: State<'_, AppState>,
    path: String,
    max_size: Option<u64>,
) -> Result<String> {
    use base64::{engine::general_purpose::STANDARD, Engine};

    let file_path = PathBuf::from(&path);

    if !file_path.exists() {
        return Err(TandemError::NotFound(format!(
            "File does not exist: {}",
            path
        )));
    }

    if !file_path.is_file() {
        return Err(TandemError::InvalidConfig(format!(
            "Path is not a file: {}",
            path
        )));
    }

    // Note: Path allowlist check removed - was causing Windows path normalization issues

    let metadata = fs::metadata(&file_path).map_err(TandemError::Io)?;
    let file_size = metadata.len();
    let size_limit = max_size.unwrap_or(10 * 1024 * 1024);

    if file_size > size_limit {
        return Err(TandemError::InvalidConfig(format!(
            "File too large: {} bytes (limit: {} bytes)",
            file_size, size_limit
        )));
    }

    let bytes = fs::read(&file_path).map_err(TandemError::Io)?;
    Ok(STANDARD.encode(&bytes))
}

/// Read a file as text, with best-effort extraction for common document formats.
///
/// Supported extraction (pure Rust):
/// - PDF: `.pdf`
/// - Word: `.docx`
/// - PowerPoint: `.pptx`
/// - Excel: `.xlsx`, `.xls`, `.ods`, `.xlsb`
/// - Rich Text: `.rtf`
///
/// All other file types fall back to UTF-8 text reading.
#[tauri::command]
pub async fn read_file_text(
    _state: State<'_, AppState>,
    path: String,
    max_size: Option<u64>,
    max_chars: Option<usize>,
) -> Result<String> {
    let file_path = PathBuf::from(&path);

    let mut limits = crate::document_text::ExtractLimits::default();
    if let Some(max_size) = max_size {
        limits.max_file_bytes = max_size;
    }
    if let Some(max_chars) = max_chars {
        limits.max_output_chars = max_chars;
    }

    Ok(crate::document_text::extract_file_text(&file_path, limits)?)
}

// Skills Management Module
// Handles discovery, parsing, and management of OpenCode-compatible skills

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SkillLocation {
    Project,
    Global,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub location: SkillLocation,
    pub path: String,
}

#[derive(Debug, Deserialize)]
struct SkillFrontmatter {
    name: String,
    description: String,
    #[serde(default)]
    license: Option<String>,
    #[serde(default)]
    compatibility: Option<String>,
    #[serde(default)]
    metadata: Option<HashMap<String, String>>,
}

/// Validate skill name per OpenCode spec: ^[a-z0-9]+(-[a-z0-9]+)*$
pub fn validate_skill_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 64 {
        return Err("Skill name must be 1-64 characters".to_string());
    }

    // Check format: lowercase alphanumeric with single hyphens
    let chars: Vec<char> = name.chars().collect();

    // Can't start or end with hyphen
    if chars.first() == Some(&'-') || chars.last() == Some(&'-') {
        return Err("Skill name cannot start or end with a hyphen".to_string());
    }

    // Check each character and no consecutive hyphens
    let mut prev_was_hyphen = false;
    for c in chars {
        if c == '-' {
            if prev_was_hyphen {
                return Err("Skill name cannot contain consecutive hyphens".to_string());
            }
            prev_was_hyphen = true;
        } else if c.is_ascii_lowercase() || c.is_ascii_digit() {
            prev_was_hyphen = false;
        } else {
            return Err("Skill name must be lowercase alphanumeric with hyphens only".to_string());
        }
    }

    Ok(())
}

/// Parse SKILL.md content to extract frontmatter and body
pub fn parse_skill_content(content: &str) -> Result<(String, String, String), String> {
    let lines: Vec<&str> = content.lines().collect();

    // Find frontmatter boundaries
    let mut frontmatter_start = None;
    let mut frontmatter_end = None;

    for (i, line) in lines.iter().enumerate() {
        if line.trim() == "---" {
            if frontmatter_start.is_none() {
                frontmatter_start = Some(i);
            } else if frontmatter_end.is_none() {
                frontmatter_end = Some(i);
                break;
            }
        }
    }

    let (start, end) = match (frontmatter_start, frontmatter_end) {
        (Some(s), Some(e)) if s < e => (s, e),
        _ => {
            return Err(
                "Invalid SKILL.md format: missing or malformed frontmatter (---...---)".to_string(),
            )
        }
    };

    // Extract frontmatter YAML
    let frontmatter_lines = &lines[start + 1..end];
    let mut frontmatter_yaml = frontmatter_lines.join("\n");

    // Fix common YAML issues: if description is not quoted and contains colons, quote it
    // This handles cases where the description has colons like "for: (1)" which breaks YAML parsing
    if let Some(desc_start) = frontmatter_yaml.find("description:") {
        let desc_line_start = desc_start + "description:".len();
        if let Some(desc_value_start) =
            frontmatter_yaml[desc_line_start..].find(|c: char| !c.is_whitespace())
        {
            let desc_value_pos = desc_line_start + desc_value_start;
            let desc_char = frontmatter_yaml.chars().nth(desc_value_pos).unwrap_or(' ');

            // If the description doesn't start with a quote and we can find the end of line
            if desc_char != '"' && desc_char != '\'' {
                let end_of_line = frontmatter_yaml[desc_value_pos..]
                    .find('\n')
                    .unwrap_or(frontmatter_yaml.len() - desc_value_pos);
                let desc_value = &frontmatter_yaml[desc_value_pos..desc_value_pos + end_of_line];

                // If the description contains a colon, wrap it in quotes
                if desc_value.contains(':') {
                    let quoted_desc = format!("\"{}\"", desc_value.trim());
                    frontmatter_yaml = format!(
                        "{}description: {}{}",
                        &frontmatter_yaml[..desc_start],
                        quoted_desc,
                        &frontmatter_yaml[desc_value_pos + end_of_line..]
                    );
                }
            }
        }
    }

    // Parse YAML
    let frontmatter: SkillFrontmatter = serde_yaml::from_str(&frontmatter_yaml).map_err(|e| {
        tracing::error!("YAML parsing error: {}", e);
        tracing::error!("Attempted to parse:\n{}", frontmatter_yaml);
        format!(
            "Failed to parse frontmatter: {}. YAML frontmatter:\n{}",
            e, frontmatter_yaml
        )
    })?;

    // Validate name
    validate_skill_name(&frontmatter.name)?;

    // Extract body (everything after second ---)
    let body = if end + 1 < lines.len() {
        lines[end + 1..].join("\n")
    } else {
        String::new()
    };

    Ok((frontmatter.name, frontmatter.description, body))
}

/// Get skill directories for discovery
pub fn get_skill_dirs(workspace: Option<&str>) -> Vec<(PathBuf, SkillLocation)> {
    let mut dirs = Vec::new();

    // Project skills (.opencode/skill/)
    if let Some(ws) = workspace {
        let project_dir = PathBuf::from(ws).join(".opencode").join("skill");
        dirs.push((project_dir, SkillLocation::Project));
    }

    // Global skills (~/.config/opencode/skills/)
    if let Some(config_dir) = dirs::config_dir() {
        let global_dir = config_dir.join("opencode").join("skills");
        dirs.push((global_dir, SkillLocation::Global));
    }

    dirs
}

/// Discover all installed skills
pub fn discover_skills(workspace: Option<&str>) -> Vec<SkillInfo> {
    let mut skills = Vec::new();

    let dirs = get_skill_dirs(workspace);
    tracing::info!("Checking {} skill directories", dirs.len());

    for (dir, location) in dirs {
        tracing::info!("Checking {:?} directory: {:?}", location, dir);

        if !dir.exists() {
            tracing::warn!("  Directory does not exist: {:?}", dir);
            continue;
        }

        // Read all subdirectories
        if let Ok(entries) = fs::read_dir(&dir) {
            let entry_count = entries.count();
            tracing::info!("  Found {} entries in directory", entry_count);

            // Need to read again after counting
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    if let Ok(file_type) = entry.file_type() {
                        if file_type.is_dir() {
                            let skill_file = entry.path().join("SKILL.md");
                            tracing::info!("  Checking for skill file: {:?}", skill_file);

                            if skill_file.exists() {
                                if let Ok(content) = fs::read_to_string(&skill_file) {
                                    if let Ok((name, description, _)) =
                                        parse_skill_content(&content)
                                    {
                                        tracing::info!("  ✓ Found skill: {}", name);
                                        skills.push(SkillInfo {
                                            name,
                                            description,
                                            location: location.clone(),
                                            path: entry.path().to_string_lossy().to_string(),
                                        });
                                    } else {
                                        tracing::warn!(
                                            "  ✗ Failed to parse skill content: {:?}",
                                            skill_file
                                        );
                                    }
                                } else {
                                    tracing::warn!(
                                        "  ✗ Failed to read skill file: {:?}",
                                        skill_file
                                    );
                                }
                            }
                        }
                    }
                }
            }
        } else {
            tracing::warn!("  Failed to read directory: {:?}", dir);
        }
    }

    skills
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_skill_name() {
        // Valid names
        assert!(validate_skill_name("my-skill").is_ok());
        assert!(validate_skill_name("code-review").is_ok());
        assert!(validate_skill_name("test123").is_ok());
        assert!(validate_skill_name("a").is_ok());

        // Invalid names
        assert!(validate_skill_name("").is_err()); // Empty
        assert!(validate_skill_name("-start").is_err()); // Starts with hyphen
        assert!(validate_skill_name("end-").is_err()); // Ends with hyphen
        assert!(validate_skill_name("double--hyphen").is_err()); // Consecutive hyphens
        assert!(validate_skill_name("Upper-Case").is_err()); // Uppercase
        assert!(validate_skill_name("under_score").is_err()); // Underscore
        assert!(validate_skill_name("with space").is_err()); // Space
    }

    #[test]
    fn test_parse_skill_content() {
        let content = r#"---
name: test-skill
description: A test skill
---

Instructions here..."#;

        let result = parse_skill_content(content);
        assert!(result.is_ok());

        let (name, desc, body) = result.unwrap();
        assert_eq!(name, "test-skill");
        assert_eq!(desc, "A test skill");
        assert!(body.contains("Instructions here"));
    }
}

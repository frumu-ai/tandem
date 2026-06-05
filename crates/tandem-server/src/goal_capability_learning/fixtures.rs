//! Hardcoded capability fixtures for MVP discovery.

use serde_json::{json, Value};
use tandem_types::AvailableCapability;

pub fn file_read_capability() -> AvailableCapability {
    AvailableCapability {
        capability_id: "file_read".to_string(),
        tool_name: "FileRead".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path to read" }
            },
            "required": ["path"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "content": { "type": "string", "description": "File contents" },
                "path": { "type": "string" },
                "size_bytes": { "type": "integer" }
            },
            "required": ["content"]
        }),
        tags: vec!["file_io".to_string(), "read".to_string()],
    }
}

pub fn csv_parse_capability() -> AvailableCapability {
    AvailableCapability {
        capability_id: "csv_parse".to_string(),
        tool_name: "CSVParse".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "content": { "type": "string", "description": "CSV content as string" },
                "delimiter": { "type": "string", "description": "Field delimiter", "default": "," }
            },
            "required": ["content"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "records": {
                    "type": "array",
                    "description": "Array of parsed records as objects",
                    "items": { "type": "object" }
                },
                "record_count": { "type": "integer" },
                "columns": {
                    "type": "array",
                    "items": { "type": "string" }
                }
            },
            "required": ["records"]
        }),
        tags: vec![
            "data_transform".to_string(),
            "parse".to_string(),
            "csv".to_string(),
        ],
    }
}

pub fn json_serialize_capability() -> AvailableCapability {
    AvailableCapability {
        capability_id: "json_serialize".to_string(),
        tool_name: "JSONSerialize".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "data": { "type": "object", "description": "Data to serialize" },
                "pretty": { "type": "boolean", "description": "Pretty-print", "default": true }
            },
            "required": ["data"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "json": { "type": "string", "description": "JSON string" }
            },
            "required": ["json"]
        }),
        tags: vec!["data_transform".to_string(), "serialize".to_string()],
    }
}

/// All hardcoded capabilities for MVP.
pub fn all_capabilities() -> Vec<AvailableCapability> {
    vec![
        file_read_capability(),
        csv_parse_capability(),
        json_serialize_capability(),
    ]
}

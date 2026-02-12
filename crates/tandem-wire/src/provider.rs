use std::collections::HashMap;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct WireProviderCatalog {
    pub all: Vec<WireProviderEntry>,
    pub connected: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WireProviderEntry {
    pub id: String,
    pub name: Option<String>,
    pub models: HashMap<String, WireProviderModel>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WireProviderModel {
    pub name: Option<String>,
    pub limit: Option<WireProviderModelLimit>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WireProviderModelLimit {
    pub context: Option<u32>,
}

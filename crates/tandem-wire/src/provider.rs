use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WireProviderCatalog {
    pub all: Vec<WireProviderEntry>,
    pub connected: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WireProviderEntry {
    pub id: String,
    pub name: Option<String>,
    pub models: HashMap<String, WireProviderModel>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WireProviderModel {
    pub name: Option<String>,
    pub limit: Option<WireProviderModelLimit>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WireProviderModelLimit {
    pub context: Option<u32>,
}

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HostOs {
    Windows,
    Linux,
    Macos,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShellFamily {
    Powershell,
    Posix,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PathStyle {
    Windows,
    Posix,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostRuntimeContext {
    pub os: HostOs,
    pub arch: String,
    pub shell_family: ShellFamily,
    pub path_style: PathStyle,
}


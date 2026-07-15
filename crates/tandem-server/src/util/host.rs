// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use tandem_types::{HostOs, HostRuntimeContext, PathStyle, ShellFamily};

pub fn detect_host_runtime_context() -> HostRuntimeContext {
    let os = if cfg!(target_os = "windows") {
        HostOs::Windows
    } else if cfg!(target_os = "macos") {
        HostOs::Macos
    } else {
        HostOs::Linux
    };
    let (shell_family, path_style) = match os {
        HostOs::Windows => (ShellFamily::Powershell, PathStyle::Windows),
        HostOs::Linux | HostOs::Macos => (ShellFamily::Posix, PathStyle::Posix),
    };
    HostRuntimeContext {
        os,
        arch: std::env::consts::ARCH.to_string(),
        shell_family,
        path_style,
    }
}

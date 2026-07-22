// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use std::process::Command;

fn direct_loopback_peer() -> axum::extract::ConnectInfo<std::net::SocketAddr> {
    axum::extract::ConnectInfo(std::net::SocketAddr::from(([127, 0, 0, 1], 43123)))
}

include!("global_parts/part01.rs");
include!("global_parts/part02.rs");
include!("global_parts/part03.rs");
include!("global_parts/part05.rs");
include!("global_parts/part06.rs");
include!("global_parts/part04.rs");

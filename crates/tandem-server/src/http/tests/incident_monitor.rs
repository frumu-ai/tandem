use super::*;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::RwLock;

fn path_has_suffix(path: &str, suffix: &str) -> bool {
    path.replace('\\', "/").ends_with(suffix)
}

include!("incident_monitor_parts/part01.rs");
include!("incident_monitor_parts/part03.rs");
include!("incident_monitor_parts/part02.rs");
include!("incident_monitor_parts/part04.rs");
include!("incident_monitor_parts/part05.rs");
include!("incident_monitor_parts/part06.rs");
include!("incident_monitor_parts/part07.rs");
include!("incident_monitor_parts/part08.rs");
include!("incident_monitor_parts/part09.rs");
include!("incident_monitor_parts/part10.rs");
include!("incident_monitor_parts/part11.rs");
include!("incident_monitor_parts/part12.rs");
include!("incident_monitor_parts/part13.rs");
include!("incident_monitor_parts/part14.rs");
include!("incident_monitor_parts/part15.rs");
include!("incident_monitor_parts/part16.rs");

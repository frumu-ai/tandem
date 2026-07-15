// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use axum::Json;

use tandem_enterprise_contract::{EnterpriseBridge, NoopEnterpriseBridge};

pub(super) async fn enterprise_status() -> Json<tandem_enterprise_contract::EnterpriseStatus> {
    Json(NoopEnterpriseBridge.status())
}

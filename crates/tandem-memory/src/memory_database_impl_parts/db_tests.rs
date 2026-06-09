#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use tandem_orchestrator::{KnowledgeScope, KnowledgeTrustLevel};
    use tempfile::TempDir;

    include!("memory_database_impl_parts/db_tests_a.rs");
    include!("memory_database_impl_parts/db_tests_b.rs");
}

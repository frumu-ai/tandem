/// A logical relation whose physical representation is owned by a backend.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LogicalTable {
    SessionMemoryChunks,
    ProjectMemoryChunks,
    GlobalMemoryChunks,
    ProjectFileIndex,
    SessionFileIndex,
    GlobalFileIndex,
    ProjectIndexStatus,
    MemoryConfig,
    MemoryCleanupLog,
    SourceObjectLifecycle,
    KnowledgeSpaces,
    MemoryRecords,
    MemoryNodes,
    MemoryLayers,
}

impl LogicalTable {
    pub const fn name(self) -> &'static str {
        match self {
            Self::SessionMemoryChunks => "session_memory_chunks",
            Self::ProjectMemoryChunks => "project_memory_chunks",
            Self::GlobalMemoryChunks => "global_memory_chunks",
            Self::ProjectFileIndex => "project_file_index",
            Self::SessionFileIndex => "session_file_index",
            Self::GlobalFileIndex => "global_file_index",
            Self::ProjectIndexStatus => "project_index_status",
            Self::MemoryConfig => "memory_config",
            Self::MemoryCleanupLog => "memory_cleanup_log",
            Self::SourceObjectLifecycle => "source_object_lifecycle",
            Self::KnowledgeSpaces => "knowledge_spaces",
            Self::MemoryRecords => "memory_records",
            Self::MemoryNodes => "memory_nodes",
            Self::MemoryLayers => "memory_layers",
        }
    }
}

/// Portable scalar types used by logical memory columns.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogicalType {
    Text,
    Integer,
    Boolean,
}

/// Portable defaults. Translators choose the backend-specific literal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogicalDefault {
    LocalTenant,
    EmptyText,
    Boolean(bool),
    Integer(i64),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogicalColumn {
    pub name: &'static str,
    pub column_type: LogicalType,
    pub nullable: bool,
    pub default: Option<LogicalDefault>,
}

impl LogicalColumn {
    pub const fn new(
        name: &'static str,
        column_type: LogicalType,
        nullable: bool,
        default: Option<LogicalDefault>,
    ) -> Self {
        Self {
            name,
            column_type,
            nullable,
            default,
        }
    }
}

/// Data transformations that must accompany an additive schema change.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogicalBackfill {
    LocalTenantScope,
    OwnerOrgUnitFromMetadata,
    TenantSharedFromMetadata,
    PrivateOwnerFromLegacySubject,
    PrivateOwnerFromMetadata,
}

/// Cross-column invariants that every backend must enforce equivalently.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogicalConstraint {
    /// Shared rows have no owner; private rows have a non-empty owner subject.
    PrivateOwnerSubjectConsistency,
}

/// A backend-neutral operation in an ordered migration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogicalChange {
    Bootstrap,
    AddColumns {
        tables: &'static [LogicalTable],
        columns: &'static [LogicalColumn],
    },
    Backfill {
        tables: &'static [LogicalTable],
        rule: LogicalBackfill,
    },
    AddConstraint {
        tables: &'static [LogicalTable],
        constraint: LogicalConstraint,
    },
}

/// Whether all existing backends can apply a logical migration today.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MigrationStatus {
    Current,
    Planned,
}

/// How a current migration reaches SQLite.
///
/// Versions 1-4 predate the executable coordinator. Their schema work still
/// lives in `MemoryDatabase::init_schema`; the coordinator records them only
/// after that legacy bootstrap has completed successfully. New migrations
/// must use a real translator rather than extending that bootstrap boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SqliteMigrationMode {
    LegacyBootstrapBaseline,
    Executable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogicalMigration {
    pub version: i64,
    pub name: &'static str,
    pub status: MigrationStatus,
    pub sqlite_mode: SqliteMigrationMode,
    pub changes: &'static [LogicalChange],
}

impl LogicalMigration {
    pub const fn is_current(self) -> bool {
        matches!(self.status, MigrationStatus::Current)
    }
}

const CHUNK_TABLES: &[LogicalTable] = &[
    LogicalTable::SessionMemoryChunks,
    LogicalTable::ProjectMemoryChunks,
    LogicalTable::GlobalMemoryChunks,
];

const DEFAULTED_TENANT_SCOPE_TABLES: &[LogicalTable] = &[
    LogicalTable::SessionMemoryChunks,
    LogicalTable::ProjectMemoryChunks,
    LogicalTable::GlobalMemoryChunks,
    LogicalTable::ProjectFileIndex,
    LogicalTable::SessionFileIndex,
    LogicalTable::GlobalFileIndex,
    LogicalTable::ProjectIndexStatus,
    LogicalTable::MemoryConfig,
    LogicalTable::MemoryCleanupLog,
    LogicalTable::KnowledgeSpaces,
    LogicalTable::MemoryRecords,
    LogicalTable::MemoryNodes,
];

const REQUIRED_TENANT_SCOPE_TABLES: &[LogicalTable] = &[LogicalTable::SourceObjectLifecycle];

const NULLABLE_DEPLOYMENT_TABLES: &[LogicalTable] = &[
    LogicalTable::SessionMemoryChunks,
    LogicalTable::ProjectMemoryChunks,
    LogicalTable::GlobalMemoryChunks,
    LogicalTable::MemoryCleanupLog,
    LogicalTable::MemoryRecords,
    LogicalTable::MemoryNodes,
];

const REQUIRED_DEPLOYMENT_TABLES: &[LogicalTable] = &[
    LogicalTable::ProjectFileIndex,
    LogicalTable::SessionFileIndex,
    LogicalTable::GlobalFileIndex,
    LogicalTable::ProjectIndexStatus,
    LogicalTable::MemoryConfig,
    LogicalTable::SourceObjectLifecycle,
    LogicalTable::KnowledgeSpaces,
];

const ORG_UNIT_TABLES: &[LogicalTable] = &[
    LogicalTable::SessionMemoryChunks,
    LogicalTable::ProjectMemoryChunks,
    LogicalTable::GlobalMemoryChunks,
    LogicalTable::MemoryRecords,
];

const ENVELOPE_TABLES: &[LogicalTable] = &[
    LogicalTable::SessionMemoryChunks,
    LogicalTable::ProjectMemoryChunks,
    LogicalTable::GlobalMemoryChunks,
    LogicalTable::MemoryLayers,
];

const PRIVATE_OWNER_TABLES: &[LogicalTable] = ORG_UNIT_TABLES;

const DEFAULTED_TENANT_SCOPE_COLUMNS: &[LogicalColumn] = &[
    LogicalColumn::new(
        "tenant_org_id",
        LogicalType::Text,
        false,
        Some(LogicalDefault::LocalTenant),
    ),
    LogicalColumn::new(
        "tenant_workspace_id",
        LogicalType::Text,
        false,
        Some(LogicalDefault::LocalTenant),
    ),
];

const REQUIRED_TENANT_SCOPE_COLUMNS: &[LogicalColumn] = &[
    LogicalColumn::new("tenant_org_id", LogicalType::Text, false, None),
    LogicalColumn::new("tenant_workspace_id", LogicalType::Text, false, None),
];

const NULLABLE_DEPLOYMENT_COLUMN: &[LogicalColumn] = &[LogicalColumn::new(
    "tenant_deployment_id",
    LogicalType::Text,
    true,
    None,
)];

const REQUIRED_DEPLOYMENT_COLUMN: &[LogicalColumn] = &[LogicalColumn::new(
    "tenant_deployment_id",
    LogicalType::Text,
    false,
    Some(LogicalDefault::EmptyText),
)];

const CHUNK_SUBJECT_COLUMN: &[LogicalColumn] =
    &[LogicalColumn::new("subject", LogicalType::Text, true, None)];

const SOURCE_COLUMNS: &[LogicalColumn] = &[
    LogicalColumn::new("source", LogicalType::Text, false, None),
    LogicalColumn::new("source_path", LogicalType::Text, true, None),
    LogicalColumn::new("source_mtime", LogicalType::Integer, true, None),
    LogicalColumn::new("source_size", LogicalType::Integer, true, None),
    LogicalColumn::new("source_hash", LogicalType::Text, true, None),
];

const SOURCE_LIFECYCLE_COLUMNS: &[LogicalColumn] = &[LogicalColumn::new(
    "source_hash",
    LogicalType::Text,
    true,
    None,
)];

const RETENTION_COLUMNS: &[LogicalColumn] = &[
    LogicalColumn::new(
        "exchange_retention_days",
        LogicalType::Integer,
        false,
        Some(LogicalDefault::Integer(365)),
    ),
    LogicalColumn::new(
        "global_retention_days",
        LogicalType::Integer,
        false,
        Some(LogicalDefault::Integer(0)),
    ),
];

const OWNER_ORG_UNIT_COLUMN: &[LogicalColumn] = &[LogicalColumn::new(
    "owner_org_unit_id",
    LogicalType::Text,
    true,
    None,
)];

const TENANT_SHARED_COLUMN: &[LogicalColumn] = &[LogicalColumn::new(
    "tenant_shared",
    LogicalType::Boolean,
    false,
    Some(LogicalDefault::Boolean(false)),
)];

const ENVELOPE_COLUMNS: &[LogicalColumn] = &[LogicalColumn::new(
    "crypto_envelope",
    LogicalType::Text,
    true,
    None,
)];

const PRIVATE_OWNER_COLUMNS: &[LogicalColumn] = &[
    LogicalColumn::new(
        "private",
        LogicalType::Boolean,
        false,
        Some(LogicalDefault::Boolean(false)),
    ),
    LogicalColumn::new("owner_subject", LogicalType::Text, true, None),
];

const BOOTSTRAP_CHANGES: &[LogicalChange] = &[
    LogicalChange::Bootstrap,
    LogicalChange::AddColumns {
        tables: DEFAULTED_TENANT_SCOPE_TABLES,
        columns: DEFAULTED_TENANT_SCOPE_COLUMNS,
    },
    LogicalChange::AddColumns {
        tables: REQUIRED_TENANT_SCOPE_TABLES,
        columns: REQUIRED_TENANT_SCOPE_COLUMNS,
    },
    LogicalChange::AddColumns {
        tables: NULLABLE_DEPLOYMENT_TABLES,
        columns: NULLABLE_DEPLOYMENT_COLUMN,
    },
    LogicalChange::AddColumns {
        tables: REQUIRED_DEPLOYMENT_TABLES,
        columns: REQUIRED_DEPLOYMENT_COLUMN,
    },
    LogicalChange::Backfill {
        tables: DEFAULTED_TENANT_SCOPE_TABLES,
        rule: LogicalBackfill::LocalTenantScope,
    },
    LogicalChange::AddColumns {
        tables: CHUNK_TABLES,
        columns: CHUNK_SUBJECT_COLUMN,
    },
    LogicalChange::AddColumns {
        tables: CHUNK_TABLES,
        columns: SOURCE_COLUMNS,
    },
    LogicalChange::AddColumns {
        tables: &[LogicalTable::SourceObjectLifecycle],
        columns: SOURCE_LIFECYCLE_COLUMNS,
    },
];

const RETENTION_CHANGES: &[LogicalChange] = &[LogicalChange::AddColumns {
    tables: &[LogicalTable::MemoryConfig],
    columns: RETENTION_COLUMNS,
}];

const ORG_UNIT_CHANGES: &[LogicalChange] = &[
    LogicalChange::AddColumns {
        tables: ORG_UNIT_TABLES,
        columns: OWNER_ORG_UNIT_COLUMN,
    },
    LogicalChange::AddColumns {
        tables: CHUNK_TABLES,
        columns: TENANT_SHARED_COLUMN,
    },
    LogicalChange::Backfill {
        tables: ORG_UNIT_TABLES,
        rule: LogicalBackfill::OwnerOrgUnitFromMetadata,
    },
    LogicalChange::Backfill {
        tables: CHUNK_TABLES,
        rule: LogicalBackfill::TenantSharedFromMetadata,
    },
];

const ENVELOPE_CHANGES: &[LogicalChange] = &[LogicalChange::AddColumns {
    tables: ENVELOPE_TABLES,
    columns: ENVELOPE_COLUMNS,
}];

const PRIVATE_OWNER_CHANGES: &[LogicalChange] = &[
    LogicalChange::AddColumns {
        tables: PRIVATE_OWNER_TABLES,
        columns: PRIVATE_OWNER_COLUMNS,
    },
    LogicalChange::Backfill {
        tables: CHUNK_TABLES,
        rule: LogicalBackfill::PrivateOwnerFromLegacySubject,
    },
    LogicalChange::Backfill {
        tables: &[LogicalTable::MemoryRecords],
        rule: LogicalBackfill::PrivateOwnerFromMetadata,
    },
    LogicalChange::AddConstraint {
        tables: PRIVATE_OWNER_TABLES,
        constraint: LogicalConstraint::PrivateOwnerSubjectConsistency,
    },
];

/// Ordered, append-only logical memory migrations.
pub const MEMORY_SCHEMA_MIGRATIONS: &[LogicalMigration] = &[
    LogicalMigration {
        version: 1,
        name: "bootstrap_memory_schema",
        status: MigrationStatus::Current,
        sqlite_mode: SqliteMigrationMode::LegacyBootstrapBaseline,
        changes: BOOTSTRAP_CHANGES,
    },
    LogicalMigration {
        version: 2,
        name: "memory_config_retention_columns",
        status: MigrationStatus::Current,
        sqlite_mode: SqliteMigrationMode::LegacyBootstrapBaseline,
        changes: RETENTION_CHANGES,
    },
    LogicalMigration {
        version: 3,
        name: "chunk_owner_org_unit_scope",
        status: MigrationStatus::Current,
        sqlite_mode: SqliteMigrationMode::LegacyBootstrapBaseline,
        changes: ORG_UNIT_CHANGES,
    },
    LogicalMigration {
        version: 4,
        name: "memory_crypto_envelope",
        status: MigrationStatus::Current,
        sqlite_mode: SqliteMigrationMode::LegacyBootstrapBaseline,
        changes: ENVELOPE_CHANGES,
    },
    LogicalMigration {
        version: 5,
        name: "private_owner_subject_scope",
        status: MigrationStatus::Current,
        sqlite_mode: SqliteMigrationMode::Executable,
        changes: PRIVATE_OWNER_CHANGES,
    },
];

#[derive(Clone, Copy, Debug)]
pub struct MigrationRegistry {
    migrations: &'static [LogicalMigration],
}

impl MigrationRegistry {
    pub const fn new(migrations: &'static [LogicalMigration]) -> Self {
        Self { migrations }
    }

    pub const fn all(self) -> &'static [LogicalMigration] {
        self.migrations
    }

    /// Current migrations absent from an existing backend ledger, in order.
    pub fn pending_current(self, applied_versions: &[i64]) -> Vec<&'static LogicalMigration> {
        self.migrations
            .iter()
            .filter(|migration| {
                migration.is_current() && !applied_versions.contains(&migration.version)
            })
            .collect()
    }
}

pub const MEMORY_SCHEMA_REGISTRY: MigrationRegistry =
    MigrationRegistry::new(MEMORY_SCHEMA_MIGRATIONS);

#[cfg(test)]
mod tests {
    use super::*;

    fn versions(migrations: &[&LogicalMigration]) -> Vec<i64> {
        migrations
            .iter()
            .map(|migration| migration.version)
            .collect()
    }

    #[test]
    fn fresh_backend_receives_all_current_migrations_in_order() {
        let pending = MEMORY_SCHEMA_REGISTRY.pending_current(&[]);

        assert_eq!(versions(&pending), vec![1, 2, 3, 4, 5]);
        assert!(MEMORY_SCHEMA_MIGRATIONS
            .windows(2)
            .all(|pair| pair[0].version < pair[1].version));
        assert_eq!(
            MEMORY_SCHEMA_MIGRATIONS
                .last()
                .map(|migration| migration.status),
            Some(MigrationStatus::Current)
        );
    }

    #[test]
    fn legacy_ledger_receives_only_missing_current_migrations() {
        let pending = MEMORY_SCHEMA_REGISTRY.pending_current(&[1, 2, 3]);

        assert_eq!(versions(&pending), vec![4, 5]);
        assert_eq!(pending[0].name, "memory_crypto_envelope");
    }

    #[test]
    fn migration_planning_is_idempotent() {
        let first = MEMORY_SCHEMA_REGISTRY.pending_current(&[]);
        let applied = versions(&first);
        let second = MEMORY_SCHEMA_REGISTRY.pending_current(&applied);

        assert!(second.is_empty());
        assert!(MEMORY_SCHEMA_REGISTRY
            .pending_current(&[1, 1, 2, 2, 3, 3, 4, 4, 5, 5])
            .is_empty());
    }

    #[test]
    fn registry_covers_portable_scope_and_provenance_fields() {
        let fields = MEMORY_SCHEMA_MIGRATIONS
            .iter()
            .flat_map(|migration| migration.changes)
            .filter_map(|change| match change {
                LogicalChange::AddColumns { columns, .. } => Some(*columns),
                _ => None,
            })
            .flatten()
            .map(|column| column.name)
            .collect::<Vec<_>>();

        for required in [
            "tenant_org_id",
            "tenant_workspace_id",
            "tenant_deployment_id",
            "owner_org_unit_id",
            "tenant_shared",
            "source",
            "source_path",
            "source_mtime",
            "source_size",
            "source_hash",
            "crypto_envelope",
            "private",
            "owner_subject",
        ] {
            assert!(fields.contains(&required), "missing field {required}");
        }
    }

    #[test]
    fn sqlite_execution_boundary_is_explicit() {
        let modes = MEMORY_SCHEMA_MIGRATIONS
            .iter()
            .map(|migration| (migration.version, migration.sqlite_mode))
            .collect::<Vec<_>>();

        assert_eq!(
            modes,
            vec![
                (1, SqliteMigrationMode::LegacyBootstrapBaseline),
                (2, SqliteMigrationMode::LegacyBootstrapBaseline),
                (3, SqliteMigrationMode::LegacyBootstrapBaseline),
                (4, SqliteMigrationMode::LegacyBootstrapBaseline),
                (5, SqliteMigrationMode::Executable),
            ]
        );
    }
}

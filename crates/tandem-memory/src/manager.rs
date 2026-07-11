include!("manager_parts/part01.rs");
include!("manager_parts/part01_store.rs");
include!("manager_parts/part01_knowledge.rs");
include!("manager_parts/part02.rs");
include!("manager_parts/part03.rs");

#[cfg(test)]
#[path = "manager_parts/store_migration_tests.rs"]
mod store_migration_tests;

mod query_context;
mod scanner_extractors;

use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn write(path: impl AsRef<Path>, body: &str) {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
}

fn repo_with_handler_fixture() -> TempDir {
    let repo = TempDir::new().unwrap();
    write(
        repo.path().join("src/handler.rs"),
        "pub fn run_login() {}\n",
    );
    write(
        repo.path().join("tests/handler_test.rs"),
        "use crate::handler::run_login;\n#[test]\nfn handler_smoke() {}\n",
    );
    write(
        repo.path().join("Cargo.toml"),
        "[package]\nname = \"handler-demo\"\n",
    );
    write(
        repo.path().join("docs/handler.md"),
        "# Handler\n\nOld handler notes.\n",
    );
    repo
}

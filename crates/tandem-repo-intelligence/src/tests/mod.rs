mod debug_export;
mod governed;
mod graph_core;
mod query_context;
mod regression_quality;
mod retrieval_evals;
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

fn polyglot_fixture_repo() -> TempDir {
    let repo = TempDir::new().unwrap();
    fs::create_dir(repo.path().join(".git")).unwrap();
    write(
        repo.path().join(".gitignore"),
        "generated/\ncoverage/\n*.snap\n",
    );
    write(
        repo.path().join("Cargo.toml"),
        "[package]\nname = \"tandem-fixture\"\n\n[dependencies]\nserde = \"1\"\n",
    );
    write(
        repo.path().join("package.json"),
        "{\n  \"name\": \"fixture-ui\",\n  \"dependencies\": { \"react\": \"18\" }\n}\n",
    );
    write(
        repo.path().join("README.md"),
        "# Login Fixture\n\nLogin service docs mention AuthService and LoginPanel.\n",
    );
    write(
        repo.path().join("src/lib.rs"),
        "pub mod login;\npub use login::LoginService;\n",
    );
    write(
        repo.path().join("src/login.rs"),
        "use crate::config::AppConfig;\npub struct LoginService;\nimpl LoginService {}\npub fn login_flow() {}\n",
    );
    write(
        repo.path().join("tests/login_test.rs"),
        "use tandem_fixture::login::login_flow;\n#[test]\nfn login_flow_smoke() {}\n",
    );
    write(
        repo.path().join("web/src/LoginPanel.tsx"),
        "import React from \"react\";\nimport { loginClient } from \"./api\";\nexport interface LoginPanelProps {}\nexport function LoginPanel() { return null; }\nconst LOGIN_ROUTE = \"/login\";\n",
    );
    write(
        repo.path().join("web/src/api.ts"),
        "export function loginClient() { return fetch(\"/login\"); }\n",
    );
    write(
        repo.path().join("service/auth.py"),
        "import os\nfrom pathlib import Path\nclass AuthService:\n    pass\nasync def refresh_token():\n    pass\n",
    );
    write(
        repo.path().join("generated/client.ts"),
        "export function generatedClient() {}\n",
    );
    write(repo.path().join("dist/bundle.js"), "generated bundle\n");
    write(
        repo.path().join("target/debug/build.log"),
        "generated build\n",
    );
    write(
        repo.path().join("coverage/report.txt"),
        "generated coverage\n",
    );
    write(
        repo.path().join("web/src/LoginPanel.snap"),
        "generated snap\n",
    );
    repo
}

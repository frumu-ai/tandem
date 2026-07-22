// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use tandem_memory::types::GlobalMemoryRecord;
use tokio::net::TcpListener;

struct CoderGitRepo {
    root: tempfile::TempDir,
}

impl std::ops::Deref for CoderGitRepo {
    type Target = std::path::Path;

    fn deref(&self) -> &Self::Target {
        self.root.path()
    }
}

impl AsRef<std::path::Path> for CoderGitRepo {
    fn as_ref(&self) -> &std::path::Path {
        self.root.path()
    }
}

fn init_coder_git_repo() -> CoderGitRepo {
    let root = tempfile::Builder::new()
        .prefix("tandem-coder-worktree-test-")
        .tempdir()
        .expect("create repo dir");
    let repo_root = root.path();
    for args in [
        &["init", "-b", "main"][..],
        &["config", "user.email", "tests@tandem.local"][..],
        &["config", "user.name", "Tandem Tests"][..],
    ] {
        assert!(std::process::Command::new("git")
            .args(args)
            .current_dir(repo_root)
            .status()
            .expect("configure coder git repository")
            .success());
    }
    std::fs::write(repo_root.join("README.md"), "# coder test\n").expect("seed readme");
    for args in [&["add", "README.md"][..], &["commit", "-m", "init"][..]] {
        assert!(std::process::Command::new("git")
            .args(args)
            .current_dir(repo_root)
            .status()
            .expect("seed coder git repository")
            .success());
    }
    CoderGitRepo { root }
}

include!("coder_parts/part01.rs");
include!("coder_parts/part02.rs");
include!("coder_parts/part03.rs");
include!("coder_parts/part04.rs");
include!("coder_parts/part05.rs");
include!("coder_parts/part06.rs");
include!("coder_parts/part07.rs");
include!("coder_parts/part08.rs");
include!("coder_parts/part09.rs");

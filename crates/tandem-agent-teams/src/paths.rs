use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct AgentTeamPaths {
    root: PathBuf,
}

impl AgentTeamPaths {
    pub fn new(state_root: impl AsRef<Path>) -> Self {
        Self {
            root: state_root.as_ref().join("agent-teams"),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn index_file(&self) -> PathBuf {
        self.root.join("index.json")
    }

    pub fn team_dir(&self, team_name: &str) -> PathBuf {
        self.root.join(team_name)
    }

    pub fn config_file(&self, team_name: &str) -> PathBuf {
        self.team_dir(team_name).join("config.json")
    }

    pub fn members_file(&self, team_name: &str) -> PathBuf {
        self.team_dir(team_name).join("members.json")
    }

    pub fn tasks_dir(&self, team_name: &str) -> PathBuf {
        self.team_dir(team_name).join("tasks")
    }

    pub fn task_file(&self, team_name: &str, task_id: &str) -> PathBuf {
        self.tasks_dir(team_name).join(format!("{}.json", task_id))
    }

    pub fn mailboxes_dir(&self, team_name: &str) -> PathBuf {
        self.team_dir(team_name).join("mailboxes")
    }

    pub fn mailbox_file(&self, team_name: &str, member_name: &str) -> PathBuf {
        self.mailboxes_dir(team_name)
            .join(format!("{}.jsonl", member_name))
    }

    pub fn requests_dir(&self, team_name: &str) -> PathBuf {
        self.team_dir(team_name).join("requests")
    }

    pub fn request_file(&self, team_name: &str, request_id: &str) -> PathBuf {
        self.requests_dir(team_name)
            .join(format!("{}.json", request_id))
    }

    pub fn events_file(&self, team_name: &str) -> PathBuf {
        self.team_dir(team_name).join("events.jsonl")
    }
}

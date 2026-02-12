use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tokio::fs;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentMode {
    Primary,
    Subagent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefinition {
    pub name: String,
    pub mode: AgentMode,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub tools: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
struct AgentFrontmatter {
    name: Option<String>,
    mode: Option<AgentMode>,
    hidden: Option<bool>,
    tools: Option<Vec<String>>,
}

#[derive(Clone)]
pub struct AgentRegistry {
    agents: Arc<RwLock<HashMap<String, AgentDefinition>>>,
    default_agent: String,
}

impl AgentRegistry {
    pub async fn new(workspace_root: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let mut by_name = HashMap::new();
        for agent in default_agents() {
            by_name.insert(agent.name.clone(), agent);
        }

        let root: PathBuf = workspace_root.into();
        let custom = load_custom_agents(root.join(".tandem").join("agent")).await?;
        for agent in custom {
            by_name.insert(agent.name.clone(), agent);
        }

        Ok(Self {
            agents: Arc::new(RwLock::new(by_name)),
            default_agent: "build".to_string(),
        })
    }

    pub async fn list(&self) -> Vec<AgentDefinition> {
        let mut agents = self
            .agents
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        agents.sort_by(|a, b| a.name.cmp(&b.name));
        agents
    }

    pub async fn get(&self, name: Option<&str>) -> AgentDefinition {
        let wanted = name.unwrap_or(&self.default_agent);
        let agents = self.agents.read().await;
        agents
            .get(wanted)
            .cloned()
            .or_else(|| agents.get(&self.default_agent).cloned())
            .unwrap_or_else(|| AgentDefinition {
                name: self.default_agent.clone(),
                mode: AgentMode::Primary,
                hidden: false,
                system_prompt: None,
                tools: None,
            })
    }
}

fn default_agents() -> Vec<AgentDefinition> {
    vec![
        AgentDefinition {
            name: "build".to_string(),
            mode: AgentMode::Primary,
            hidden: false,
            system_prompt: Some(
                "You are a build-focused engineering agent. Prefer concrete implementation."
                    .to_string(),
            ),
            tools: None,
        },
        AgentDefinition {
            name: "plan".to_string(),
            mode: AgentMode::Primary,
            hidden: false,
            system_prompt: Some(
                "You are a planning-focused engineering agent. Clarify scope and phases."
                    .to_string(),
            ),
            tools: None,
        },
        AgentDefinition {
            name: "explore".to_string(),
            mode: AgentMode::Subagent,
            hidden: false,
            system_prompt: Some(
                "You are an exploration agent. Gather evidence from the codebase quickly."
                    .to_string(),
            ),
            tools: None,
        },
        AgentDefinition {
            name: "general".to_string(),
            mode: AgentMode::Subagent,
            hidden: false,
            system_prompt: Some("You are a general-purpose helper agent.".to_string()),
            tools: None,
        },
        AgentDefinition {
            name: "compaction".to_string(),
            mode: AgentMode::Primary,
            hidden: true,
            system_prompt: Some(
                "You summarize long conversations into compact context.".to_string(),
            ),
            tools: Some(vec![]),
        },
        AgentDefinition {
            name: "title".to_string(),
            mode: AgentMode::Primary,
            hidden: true,
            system_prompt: Some("You generate concise, descriptive session titles.".to_string()),
            tools: Some(vec![]),
        },
        AgentDefinition {
            name: "summary".to_string(),
            mode: AgentMode::Primary,
            hidden: true,
            system_prompt: Some("You produce factual summaries of session content.".to_string()),
            tools: Some(vec![]),
        },
    ]
}

async fn load_custom_agents(dir: PathBuf) -> anyhow::Result<Vec<AgentDefinition>> {
    let mut out = Vec::new();
    let mut entries = match fs::read_dir(&dir).await {
        Ok(rd) => rd,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read {}", dir.display()));
        }
    };

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let Some(ext) = path.extension().and_then(|v| v.to_str()) else {
            continue;
        };
        if ext != "md" {
            continue;
        }
        let raw = fs::read_to_string(&path).await?;
        if let Some(agent) = parse_agent_markdown(&raw, &path) {
            out.push(agent);
        }
    }

    Ok(out)
}

fn parse_agent_markdown(raw: &str, path: &PathBuf) -> Option<AgentDefinition> {
    let trimmed = raw.trim_start();
    if !trimmed.starts_with("---") {
        return None;
    }
    let mut parts = trimmed.splitn(3, "---");
    let _ = parts.next();
    let frontmatter = parts.next()?.trim();
    let body = parts.next()?.trim().to_string();
    let parsed: AgentFrontmatter = serde_yaml::from_str(frontmatter).ok()?;
    let default_name = path.file_stem()?.to_string_lossy().to_string();
    Some(AgentDefinition {
        name: parsed.name.unwrap_or(default_name),
        mode: parsed.mode.unwrap_or(AgentMode::Subagent),
        hidden: parsed.hidden.unwrap_or(false),
        system_prompt: if body.is_empty() { None } else { Some(body) },
        tools: parsed.tools,
    })
}

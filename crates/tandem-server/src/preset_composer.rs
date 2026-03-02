use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptFragment {
    pub id: String,
    pub phase: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PromptComposeInput {
    #[serde(default)]
    pub base_prompt: String,
    #[serde(default)]
    pub fragments: Vec<PromptFragment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptComposeOutput {
    pub prompt: String,
    pub composition_hash: String,
    pub ordered_fragment_ids: Vec<String>,
}

pub fn compose(input: PromptComposeInput) -> PromptComposeOutput {
    let mut fragments = input.fragments.clone();
    fragments.sort_by(|a, b| {
        phase_rank(&a.phase)
            .cmp(&phase_rank(&b.phase))
            .then_with(|| a.id.cmp(&b.id))
            .then_with(|| a.content.cmp(&b.content))
    });

    let mut sections = Vec::new();
    if !input.base_prompt.trim().is_empty() {
        sections.push(input.base_prompt.trim().to_string());
    }
    let ordered_fragment_ids = fragments.iter().map(|f| f.id.clone()).collect::<Vec<_>>();
    for fragment in &fragments {
        if fragment.content.trim().is_empty() {
            continue;
        }
        sections.push(format!(
            "[{}:{}]\n{}",
            fragment.phase.trim().to_ascii_lowercase(),
            fragment.id.trim(),
            fragment.content.trim()
        ));
    }
    let prompt = sections.join("\n\n---\n\n");
    let composition_hash = format!("{:x}", Sha256::digest(prompt.as_bytes()));
    PromptComposeOutput {
        prompt,
        composition_hash,
        ordered_fragment_ids,
    }
}

fn phase_rank(phase: &str) -> usize {
    match phase.trim().to_ascii_lowercase().as_str() {
        "core" => 0,
        "domain" => 1,
        "style" => 2,
        "safety" => 3,
        _ => 99,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compose_is_deterministic_by_phase_and_id() {
        let out = compose(PromptComposeInput {
            base_prompt: "Base".to_string(),
            fragments: vec![
                PromptFragment {
                    id: "zeta".to_string(),
                    phase: "style".to_string(),
                    content: "Style Z".to_string(),
                },
                PromptFragment {
                    id: "alpha".to_string(),
                    phase: "core".to_string(),
                    content: "Core A".to_string(),
                },
                PromptFragment {
                    id: "beta".to_string(),
                    phase: "style".to_string(),
                    content: "Style B".to_string(),
                },
                PromptFragment {
                    id: "safe".to_string(),
                    phase: "safety".to_string(),
                    content: "Do no harm".to_string(),
                },
            ],
        });
        assert_eq!(
            out.ordered_fragment_ids,
            vec![
                "alpha".to_string(),
                "beta".to_string(),
                "zeta".to_string(),
                "safe".to_string()
            ]
        );
        let out2 = compose(PromptComposeInput {
            base_prompt: "Base".to_string(),
            fragments: vec![
                PromptFragment {
                    id: "safe".to_string(),
                    phase: "safety".to_string(),
                    content: "Do no harm".to_string(),
                },
                PromptFragment {
                    id: "beta".to_string(),
                    phase: "style".to_string(),
                    content: "Style B".to_string(),
                },
                PromptFragment {
                    id: "alpha".to_string(),
                    phase: "core".to_string(),
                    content: "Core A".to_string(),
                },
                PromptFragment {
                    id: "zeta".to_string(),
                    phase: "style".to_string(),
                    content: "Style Z".to_string(),
                },
            ],
        });
        assert_eq!(out.prompt, out2.prompt);
        assert_eq!(out.composition_hash, out2.composition_hash);
    }
}

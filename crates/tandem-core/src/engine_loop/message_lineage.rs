use tandem_types::{
    canonical_message_digest, MemorySourceReference, Message, MessagePart, MessageRole,
    NativeMessageLineage, TenantContext, VerifiedTenantContext,
};

/// Collect only inputs accepted for provider dispatch. Source references are
/// snapshots, not authority to bypass a later canonical lookup/read decision.
pub(super) struct MessageLineageAccumulator {
    tenant: Option<TenantContext>,
    subject: Option<String>,
    run_id: String,
    input_message_ids: Vec<String>,
    included_memory: Vec<MemorySourceReference>,
    complete: bool,
}

impl MessageLineageAccumulator {
    pub(super) fn new(verified: Option<&VerifiedTenantContext>, run_id: Option<&str>) -> Self {
        let subject = verified.and_then(lineage_subject);
        let run_id = run_id.unwrap_or_default().trim().to_string();
        Self {
            tenant: verified.map(|verified| verified.tenant_context.clone()),
            complete: subject.is_some() && !run_id.is_empty(),
            subject,
            run_id,
            input_message_ids: Vec::new(),
            included_memory: Vec::new(),
        }
    }

    pub(super) fn accept_provider_input(
        &mut self,
        messages: &[Message],
        memory: &[MemorySourceReference],
        complete: bool,
    ) {
        self.complete &= complete;
        for message in messages {
            self.input_message_ids.push(message.id.clone());
            if message.id.trim().is_empty() || message.id != message.id.trim() {
                self.complete = false;
            }
            if message
                .parts
                .iter()
                .any(|part| matches!(part, MessagePart::ToolInvocation { .. }))
            {
                self.complete = false;
            }
            match message.role {
                MessageRole::User if message.source_lineage.is_none() => {
                    // Canonical user text is subject-private by construction.
                }
                MessageRole::Assistant => {
                    let Some(lineage) = message.source_lineage.as_ref() else {
                        self.complete = false;
                        continue;
                    };
                    if lineage.schema_version != 1
                        || lineage.run_id.trim().is_empty()
                        || self.tenant.as_ref() != Some(&lineage.tenant_context)
                        || self.subject.as_deref() != Some(lineage.subject.as_str())
                        || lineage.message_digest != canonical_message_digest(message)
                    {
                        self.complete = false;
                        continue;
                    }
                    self.complete &= lineage.complete;
                    self.input_message_ids
                        .extend(lineage.input_message_ids.iter().cloned());
                    self.add_memory(&lineage.included_memory);
                }
                _ => self.complete = false,
            }
        }
        self.add_memory(memory);
        self.input_message_ids.sort();
        self.input_message_ids.dedup();
    }

    pub(super) fn mark_incomplete(&mut self) {
        self.complete = false;
    }

    fn add_memory(&mut self, sources: &[MemorySourceReference]) {
        for source in sources {
            if source.memory_id.trim().is_empty()
                || source.memory_id != source.memory_id.trim()
                || !valid_digest(&source.content_hash)
                || !valid_digest(&source.restriction_digest)
            {
                self.complete = false;
                continue;
            }
            if !self.included_memory.contains(source) {
                self.included_memory.push(source.clone());
            }
        }
    }

    pub(super) fn into_lineage(self) -> Option<NativeMessageLineage> {
        let complete = self.complete && !self.input_message_ids.is_empty();
        Some(NativeMessageLineage {
            schema_version: 1,
            run_id: self.run_id,
            tenant_context: self.tenant?,
            subject: self.subject?,
            message_digest: String::new(),
            input_message_ids: self.input_message_ids,
            included_memory: self.included_memory,
            complete,
        })
    }
}

fn lineage_subject(verified: &VerifiedTenantContext) -> Option<String> {
    let normalized = |value: &str| (!value.trim().is_empty()).then(|| value.trim().to_string());
    verified
        .strict_projection
        .as_ref()
        .and_then(|strict| {
            strict
                .principal
                .tenant_actor_id
                .as_deref()
                .and_then(normalized)
                .or_else(|| normalized(&strict.principal.id))
        })
        .or_else(|| {
            verified
                .tenant_context
                .actor_id
                .as_deref()
                .and_then(normalized)
        })
        .or_else(|| normalized(&verified.human_actor.actor_id))
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
#[path = "message_lineage_tests.rs"]
mod tests;

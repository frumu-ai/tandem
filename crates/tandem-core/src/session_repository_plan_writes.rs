use super::*;

impl SessionRepository {
    pub(crate) fn set_todos_with_commit_guard(
        &self,
        session_id: &str,
        todos: Vec<Value>,
        guard: impl FnOnce(&[Value], &mut dyn FnMut() -> Result<()>) -> Result<()>,
    ) -> Result<()> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            // Load only current metadata after acquiring the writer; never
            // replace a stale full Session or its header/message rows.
            let mut metadata = load_metadata(&transaction, session_id)?;
            metadata.todos = todos;
            let mut transaction = Some(transaction);
            let mut committed = false;
            let result = {
                let mut commit = || {
                    let transaction = transaction
                        .take()
                        .context("todo transaction already consumed")?;
                    upsert_metadata(&transaction, session_id, &metadata)?;
                    transaction.commit()?;
                    committed = true;
                    Ok(())
                };
                guard(&metadata.todos, &mut commit)
            };
            if committed {
                return result.context("todo commit guard failed after metadata commit");
            }
            result?;
            anyhow::bail!("todo commit guard skipped commit")
        })
    }

    pub(crate) fn add_question_with_commit_guard(
        &self,
        request: &QuestionRequest,
        guard: impl FnOnce(&QuestionRequest, &mut dyn FnMut() -> Result<()>) -> Result<()>,
    ) -> Result<()> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let mut transaction = Some(transaction);
            let mut committed = false;
            let result = {
                let mut commit = || {
                    let transaction = transaction
                        .take()
                        .context("question transaction already consumed")?;
                    upsert_question(&transaction, request)?;
                    transaction.commit()?;
                    committed = true;
                    Ok(())
                };
                guard(request, &mut commit)
            };
            if committed {
                return result.context("question commit guard failed after request commit");
            }
            result?;
            anyhow::bail!("question commit guard skipped commit")
        })
    }
}

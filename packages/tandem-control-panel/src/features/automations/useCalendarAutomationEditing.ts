export function useCalendarAutomationEditing({
  toast,
  scheduleToEditor,
  setEditDraft,
  isMissionBlueprintAutomation,
  onOpenAdvancedEdit,
  getAutomationCalendarFamily,
  workflowAutomationToEditDraft,
  setWorkflowEditDraft,
  rewriteCronForDroppedStart,
  updateAutomationMutation,
  updateWorkflowAutomationMutation,
}: any) {
  const beginEdit = (automation: any) => {
    const automationId = String(
      automation?.automation_id || automation?.id || automation?.routine_id || ""
    ).trim();
    if (!automationId) {
      toast("err", "Cannot edit automation without an id.");
      return;
    }
    const scheduleEditor = scheduleToEditor(automation?.schedule);
    setEditDraft({
      automationId,
      name: String(automation?.name || automationId || "").trim(),
      objective: String(
        automation?.mission?.objective || automation?.mission_snapshot?.objective || ""
      ).trim(),
      mode:
        String(automation?.mode || "").toLowerCase() === "standalone"
          ? "standalone"
          : "orchestrated",
      requiresApproval:
        automation?.requires_approval === true ||
        automation?.policy?.approval?.requires_approval === true,
      scheduleKind: scheduleEditor.scheduleKind === "cron" ? "cron" : "interval",
      cronExpression: scheduleEditor.cronExpression,
      intervalSeconds: String(scheduleEditor.intervalSeconds),
    });
  };
  const isPausedAutomation = (automation: any) => {
    const status = String(automation?.status || "")
      .trim()
      .toLowerCase();
    return status === "paused" || status === "disabled";
  };
  const openCalendarAutomationEdit = (automation: any) => {
    if (!automation) return;
    if (isMissionBlueprintAutomation(automation)) {
      onOpenAdvancedEdit(automation);
      return;
    }
    const family = getAutomationCalendarFamily(automation);
    if (family === "legacy") {
      beginEdit(automation);
      return;
    }
    const draft = workflowAutomationToEditDraft(automation);
    if (!draft) {
      toast("err", "Cannot open this workflow automation for editing.");
      return;
    }
    setWorkflowEditDraft(draft);
  };
  const updateCalendarAutomationFromEvent = async (info: any) => {
    const event = info?.event;
    const automation = event?.extendedProps?.automation;
    const family =
      String(event?.extendedProps?.family || "legacy").trim() === "v2" ? "v2" : "legacy";
    const cronExpression = String(event?.extendedProps?.cronExpression || "").trim();
    const start = event?.start ? new Date(event.start) : null;
    const nextCron = start ? rewriteCronForDroppedStart(cronExpression, start) : null;
    if (!automation || !start || !nextCron) {
      info?.revert?.();
      toast("info", "That schedule cannot be moved from the calendar yet.");
      return;
    }
    try {
      if (family === "legacy") {
        const automationId = String(
          automation?.automation_id || automation?.id || automation?.routine_id || ""
        ).trim();
        const scheduleEditor = scheduleToEditor(automation?.schedule);
        await updateAutomationMutation.mutateAsync({
          automationId,
          name: String(automation?.name || automationId || "").trim(),
          objective: String(
            automation?.mission?.objective || automation?.mission_snapshot?.objective || ""
          ).trim(),
          mode:
            String(automation?.mode || "").toLowerCase() === "standalone"
              ? "standalone"
              : "orchestrated",
          requiresApproval:
            automation?.requires_approval === true ||
            automation?.policy?.approval?.requires_approval === true,
          scheduleKind: "cron",
          cronExpression: nextCron,
          intervalSeconds: String(scheduleEditor.intervalSeconds || 3600),
        });
        return;
      }
      const draft = workflowAutomationToEditDraft(automation);
      if (!draft) {
        throw new Error("Workflow automation draft could not be created.");
      }
      await updateWorkflowAutomationMutation.mutateAsync({
        ...draft,
        scheduleKind: "cron",
        cronExpression: nextCron,
        intervalSeconds: draft.intervalSeconds || "3600",
      });
    } catch (error) {
      info?.revert?.();
      toast("err", error instanceof Error ? error.message : String(error));
    }
  };

  return {
    beginEdit,
    isPausedAutomation,
    openCalendarAutomationEdit,
    updateCalendarAutomationFromEvent,
  };
}

import { expect, test, waitForRoute } from "./fixtures/api";
import {
  tan707Day,
  tan707LiveProjection,
  tan707ReplayProjection,
} from "./fixtures/tan707-goal-journey";

test("TAN-707 renders the canonical 180-day goal journey and exact replay", async ({
  page,
  apiFixture,
}) => {
  const projectionPath = `/api/engine/goals/${tan707LiveProjection.goal_id}/projection`;
  apiFixture.mockResponse(projectionPath, tan707LiveProjection);
  apiFixture.mockResponse(`${projectionPath}?cursor=4&limit=120`, tan707ReplayProjection);

  await page.goto(`/#/goal-operations?goal_id=${tan707LiveProjection.goal_id}`);
  await waitForRoute(page, "goal-operations");

  await expect(page.getByRole("heading", { name: tan707LiveProjection.goal.objective })).toBeVisible();
  for (const node of ["Plan", "Execute", "Verify", "Complete"]) {
    await expect(page.getByText(node, { exact: true }).first()).toBeVisible();
  }
  await page.getByText("Verify", { exact: true }).first().click();
  await expect(page.getByText("complete", { exact: true })).toBeVisible();
  await expect(page.getByText("replan", { exact: true })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Uncertain Effects" })).toBeVisible();
  await expect(page.getByText("effect-day-120-release", { exact: false })).toBeVisible();
  expect(tan707LiveProjection.goal.policy.deadline_at_ms - tan707LiveProjection.goal.created_at_ms)
    .toBe(tan707Day(180) - tan707Day(0));

  await page.getByRole("button", { name: "Replay" }).click();
  const scrubber = page.getByRole("slider", { name: "Replay position" });
  await scrubber.fill("3");
  await expect
    .poll(() => apiFixture.requests.some((entry) => entry.endsWith(`${projectionPath}?cursor=4&limit=120`)))
    .toBe(true);
  await expect(page.getByRole("button", { name: "Pause goal" })).toBeDisabled();
  await expect(page.getByRole("button", { name: "Cancel goal" })).toBeDisabled();
});

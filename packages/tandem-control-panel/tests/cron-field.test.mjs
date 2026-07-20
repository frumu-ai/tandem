import assert from "node:assert/strict";
import test from "node:test";

import { CRON_WEEKDAY_ALIASES, matchesCronField } from "../src/features/automations/cronField.ts";

test("calendar cron fields support named weekday ranges", () => {
  assert.equal(matchesCronField("Mon-Fri", 1, 0, 7, CRON_WEEKDAY_ALIASES), true);
  assert.equal(matchesCronField("Mon-Fri", 5, 0, 7, CRON_WEEKDAY_ALIASES), true);
  assert.equal(matchesCronField("Mon-Fri", 6, 0, 7, CRON_WEEKDAY_ALIASES), false);
});

test("calendar cron fields support named weekend lists", () => {
  assert.equal(matchesCronField("Sun,Sat", 7, 0, 7, CRON_WEEKDAY_ALIASES), true);
  assert.equal(matchesCronField("Sun,Sat", 6, 0, 7, CRON_WEEKDAY_ALIASES), true);
  assert.equal(matchesCronField("Sun,Sat", 1, 0, 7, CRON_WEEKDAY_ALIASES), false);
});

test("calendar cron fields support full weekday names", () => {
  assert.equal(matchesCronField("Monday-Friday", 1, 0, 7, CRON_WEEKDAY_ALIASES), true);
  assert.equal(matchesCronField("Monday-Friday", 6, 0, 7, CRON_WEEKDAY_ALIASES), false);
  assert.equal(matchesCronField("Sunday", 7, 0, 7, CRON_WEEKDAY_ALIASES), true);
});

import { execFile } from "node:child_process";
import { promisify } from "node:util";

import { BUNDLE_PATH, MANIFEST_PATH, writeKnowledgeBundle } from "./engine-knowledge-bundle.mjs";

const execFileAsync = promisify(execFile);

async function hasTrackedDiff() {
  try {
    await execFileAsync("git", ["diff", "--exit-code", "--", BUNDLE_PATH, MANIFEST_PATH]);
    return false;
  } catch (error) {
    if (typeof error?.code === "number" && error.code === 1) {
      return true;
    }
    throw error;
  }
}

writeKnowledgeBundle()
  .then(async ({ manifest }) => {
    if (!(await hasTrackedDiff())) {
      process.stdout.write(
        `verified bundle: files=${manifest.file_count} bytes=${manifest.total_bytes} hash=${manifest.corpus_hash}\n`
      );
      return;
    }

    process.stderr.write(
      [
        "embedded knowledge bundle is out of sync.",
        "run `pnpm engine:knowledge:bundle` and stage the regenerated files:",
        `  git add ${BUNDLE_PATH} ${MANIFEST_PATH}`,
      ].join("\n") + "\n"
    );
    process.exit(1);
  })
  .catch((err) => {
    process.stderr.write(`${err?.stack || String(err)}\n`);
    process.exit(1);
  });

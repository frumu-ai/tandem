#!/usr/bin/env node

import { initializeInstall, printInitSummary } from "../lib/setup/bootstrap.js";
import { parseCliArgs } from "../lib/setup/common.js";

const cli = parseCliArgs(process.argv.slice(2));

const result = await initializeInstall({
  envPath: cli.value("env-file"),
  overwrite: cli.has("reset-token") || cli.has("rotate-token") || cli.has("overwrite"),
  allowAmbientStateEnv: false,
  allowCwdEnvMerge: false,
});

printInitSummary(result);

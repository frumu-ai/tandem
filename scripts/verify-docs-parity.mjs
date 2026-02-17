import { readFileSync } from "node:fs";
import path from "node:path";

const root = process.cwd();

function read(relPath) {
  return readFileSync(path.join(root, relPath), "utf8");
}

function unique(values) {
  return [...new Set(values)];
}

const toolsSource = read("crates/tandem-tools/src/lib.rs");
const tuiSource = read("crates/tandem-tui/src/app.rs");
const engineSource = read("engine/src/main.rs");

const toolsDoc = read("guide/src/content/docs/reference/tools.md");
const tuiDoc = read("guide/src/content/docs/reference/tui-commands.md");
const engineDoc = read("guide/src/content/docs/reference/engine-commands.md");

const toolNames = unique(
  [...toolsSource.matchAll(/map\.insert\("([^"]+)"/g)].map((match) => match[1]),
).sort();

const commandHelpBlock = tuiSource.match(
  /pub const COMMAND_HELP:[\s\S]*?=\s*&\[(?<block>[\s\S]*?)\];/,
);
if (!commandHelpBlock?.groups?.block) {
  console.error("Could not parse COMMAND_HELP from crates/tandem-tui/src/app.rs");
  process.exit(1);
}
const slashCommands = unique(
  [...commandHelpBlock.groups.block.matchAll(/\("([^"]+)"/g)].map((match) => match[1]),
).sort();

const commandEnumBlock = engineSource.match(/enum Command \{(?<block>[\s\S]*?)^\}/m);
if (!commandEnumBlock?.groups?.block) {
  console.error("Could not parse enum Command from engine/src/main.rs");
  process.exit(1);
}
const engineCommands = [
  ...commandEnumBlock.groups.block.matchAll(/^\s{4}([A-Z][A-Za-z0-9_]*)\s*(?:\{|,)/gm),
].map((match) => match[1].toLowerCase());

const missing = [];

for (const tool of toolNames) {
  if (!toolsDoc.includes(`\`${tool}\``)) {
    missing.push(`tools doc missing tool: ${tool}`);
  }
}

for (const command of slashCommands) {
  if (!tuiDoc.includes(`/${command}`)) {
    missing.push(`tui doc missing slash command: /${command}`);
  }
}

if (!tuiDoc.includes("Alt+1..9")) {
  missing.push("tui doc missing keybinding text: Alt+1..9");
}

for (const command of engineCommands) {
  if (!engineDoc.includes(`## \`${command}\``)) {
    missing.push(`engine doc missing command heading: ${command}`);
  }
}

if (missing.length > 0) {
  console.error("Docs parity check failed:");
  for (const issue of missing) {
    console.error(`- ${issue}`);
  }
  process.exit(1);
}

console.log(
  `Docs parity passed (${toolNames.length} tools, ${slashCommands.length} slash commands, ${engineCommands.length} engine commands).`,
);

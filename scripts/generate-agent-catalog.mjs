import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const repoRoot = path.resolve(__dirname, "..");
const sourceRoot = path.join(repoRoot, "docs/internal/tandem-proprietary");
const categoryRoot = path.join(sourceRoot, "categories");
const outputFiles = [
  path.join(repoRoot, "src/generated/agent-catalog.json"),
  path.join(repoRoot, "packages/tandem-control-panel/src/generated/agent-catalog.json"),
];

const TARGET_SURFACES = ["desktop", "control-panel"];
const ROLE_BY_CATEGORY = [
  { pattern: /meta|orchestration/i, role: "delegator" },
  { pattern: /business|product/i, role: "delegator" },
  { pattern: /quality|review|security|audit/i, role: "reviewer" },
  { pattern: /research|analysis/i, role: "watcher" },
  { pattern: /infrastructure/i, role: "worker" },
  { pattern: /developer-experience/i, role: "worker" },
  { pattern: /language-specialists/i, role: "worker" },
  { pattern: /core-development/i, role: "worker" },
  { pattern: /specialized-domains/i, role: "worker" },
  { pattern: /data-ai/i, role: "worker" },
];

function toTitleCase(value) {
  return String(value || "")
    .split(/[-_\s]+/)
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

function normalizeCategorySlug(dirName) {
  return String(dirName || "")
    .trim()
    .replace(/^\d+\s*[-.]\s*/, "")
    .replace(/^\d+-/, "")
    .trim();
}

function splitTokens(value) {
  return String(value || "")
    .toLowerCase()
    .split(/[^a-z0-9]+/g)
    .map((part) => part.trim())
    .filter(Boolean);
}

function parseCategoryReadme(readmePath) {
  if (!fs.existsSync(readmePath)) {
    return {
      title: toTitleCase(path.basename(path.dirname(readmePath))),
      summary: "",
    };
  }

  const text = fs.readFileSync(readmePath, "utf8");
  const lines = text.split(/\r?\n/);
  const heading = lines.find((line) => /^#\s+/.test(line.trim())) || "";
  const headingText = heading.replace(/^#\s+/, "").trim();
  const title = headingText.replace(/^\d+[.)]?\s*/, "").trim() || toTitleCase(headingText);

  const summaryLines = [];
  let seenHeading = false;
  for (const rawLine of lines) {
    const line = rawLine.trim();
    if (!line) {
      if (seenHeading && summaryLines.length) break;
      continue;
    }
    if (!seenHeading) {
      if (rawLine === heading) seenHeading = true;
      continue;
    }
    if (/^included agents:/i.test(line)) break;
    if (line.startsWith("<") || line.startsWith("![")) continue;
    summaryLines.push(line);
  }

  return {
    title,
    summary: summaryLines.join(" ").replace(/\s+/g, " ").trim(),
  };
}

function parseQuotedString(value) {
  const raw = String(value || "").trim();
  if (!raw) return "";
  if (raw.startsWith('"')) {
    try {
      return JSON.parse(raw);
    } catch {
      return raw.slice(1, -1);
    }
  }
  if (raw.startsWith("'") && raw.endsWith("'")) {
    return raw.slice(1, -1);
  }
  return raw;
}

function parseTomlArray(value) {
  const raw = String(value || "").trim();
  if (!raw.startsWith("[") || !raw.endsWith("]")) return [];
  const inner = raw.slice(1, -1);
  const matches = inner.matchAll(/"((?:\\.|[^"])*)"|'((?:\\.|[^'])*)'/g);
  return Array.from(matches)
    .map((match) => match[1] || match[2] || "")
    .map((entry) => entry.replace(/\\"/g, '"').replace(/\\'/g, "'"))
    .filter(Boolean);
}

function parseAgentToml(content) {
  const lines = content.split(/\r?\n/);
  const data = {};

  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i].trim();
    if (!line || line.startsWith("#")) continue;

    const eqIndex = line.indexOf("=");
    if (eqIndex < 0) continue;

    const key = line.slice(0, eqIndex).trim();
    let value = line.slice(eqIndex + 1).trim();

    if (value.startsWith('"""')) {
      const body = [];
      let remainder = value.slice(3);
      if (remainder.endsWith('"""') && remainder !== '"""') {
        data[key] = remainder.slice(0, -3);
        continue;
      }
      if (remainder) body.push(remainder);

      for (i += 1; i < lines.length; i += 1) {
        const nextLine = lines[i];
        const closingIndex = nextLine.indexOf('"""');
        if (closingIndex >= 0) {
          body.push(nextLine.slice(0, closingIndex));
          break;
        }
        body.push(nextLine);
      }
      data[key] = body.join("\n").trim();
      continue;
    }

    if (value.startsWith("[") && value.endsWith("]")) {
      data[key] = parseTomlArray(value);
      continue;
    }

    data[key] = parseQuotedString(value);
  }

  return data;
}

function inferRole(categorySlug, sourceName, sandboxMode) {
  for (const { pattern, role } of ROLE_BY_CATEGORY) {
    if (pattern.test(categorySlug)) return role;
  }
  const sourceText = `${categorySlug} ${sourceName}`.toLowerCase();
  if (/(review|security|audit|verify|qa)/.test(sourceText)) return "reviewer";
  if (/(research|analysis|researcher)/.test(sourceText)) return "watcher";
  if (/(delegate|coordinator|organizer|orchestrator)/.test(sourceText)) return "delegator";
  if (String(sandboxMode || "").trim().toLowerCase() === "read-only") return "reviewer";
  return "worker";
}

function deriveTags(agentName, categorySlug, categoryTitle, sandboxMode) {
  const tags = new Set([
    ...splitTokens(agentName),
    ...splitTokens(categorySlug),
    ...splitTokens(categoryTitle),
  ]);
  if (sandboxMode) tags.add(String(sandboxMode).trim().toLowerCase());
  return Array.from(tags)
    .filter(Boolean)
    .filter((tag) => tag.length > 1)
    .slice(0, 12);
}

function buildCatalog() {
  const entries = fs
    .readdirSync(categoryRoot, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => {
      const categoryDir = entry.name;
      const categorySlug = normalizeCategorySlug(categoryDir);
      const categoryPath = path.join(categoryRoot, categoryDir);
      const readmePath = path.join(categoryPath, "README.md");
      const categoryMeta = parseCategoryReadme(readmePath);
      return {
        id: categorySlug,
        title: categoryMeta.title || toTitleCase(categorySlug),
        summary: categoryMeta.summary || "",
        source_path: path.posix.join("@tandem-proprietary/agents/categories", categoryDir),
        toml_files: fs
          .readdirSync(categoryPath, { withFileTypes: true })
          .filter((file) => file.isFile() && file.name.endsWith(".toml"))
          .map((file) => path.join(categoryPath, file.name))
          .sort((left, right) => left.localeCompare(right)),
      };
    })
    .sort((left, right) => left.id.localeCompare(right.id));

  const agents = [];
  for (const category of entries) {
    for (const filePath of category.toml_files) {
      const raw = fs.readFileSync(filePath, "utf8");
      const parsed = parseAgentToml(raw);
      const name = String(parsed.name || "").trim();
      const description = String(parsed.description || "").trim();
      const sandboxMode = String(parsed.sandbox_mode || "").trim();
      const instructions = String(parsed.developer_instructions || parsed.instructions || "").trim();
      if (!name || !description || !instructions) {
        throw new Error(`Invalid agent manifest: ${filePath}`);
      }

      const sourcePath = path.posix.join(
        "@tandem-proprietary/agents/categories",
        path.relative(categoryRoot, filePath).split(path.sep).join("/")
      );

      const categorySlug = category.id;
      agents.push({
        id: name,
        name,
        summary: description,
        category_id: categorySlug,
        category_title: category.title,
        category_summary: category.summary,
        source_path: sourcePath,
        source_file: path.basename(filePath),
        sandbox_mode: sandboxMode || "workspace-write",
        target_surfaces: [...TARGET_SURFACES],
        instructions,
        tags: [
          ...new Set(
            [
              ...(Array.isArray(parsed.tags) ? parsed.tags : []),
              ...deriveTags(name, categorySlug, category.title, sandboxMode),
            ]
              .map((tag) => String(tag || "").trim().toLowerCase())
              .filter(Boolean)
          ),
        ],
        requires: Array.isArray(parsed.requires)
          ? parsed.requires.map((item) => String(item || "").trim()).filter(Boolean)
          : [],
        role: inferRole(categorySlug, name, sandboxMode),
      });
    }
  }

  agents.sort((left, right) => {
    if (left.category_id !== right.category_id) {
      return left.category_id.localeCompare(right.category_id);
    }
    return left.name.localeCompare(right.name);
  });

  const categories = entries.map((category) => ({
    id: category.id,
    title: category.title,
    summary: category.summary,
    source_path: category.source_path,
    count: agents.filter((agent) => agent.category_id === category.id).length,
  }));

  return {
    generated_at: new Date().toISOString(),
    source_root: path.posix.join("@tandem-proprietary/agents"),
    categories,
    agents,
  };
}

function readExistingCatalog() {
  for (const outputFile of outputFiles) {
    if (!fs.existsSync(outputFile)) continue;
    try {
      const parsed = JSON.parse(fs.readFileSync(outputFile, "utf8"));
      if (
        parsed &&
        typeof parsed === "object" &&
        Array.isArray(parsed.categories) &&
        Array.isArray(parsed.agents)
      ) {
        return parsed;
      }
    } catch {
      // Ignore malformed generated files and keep looking.
    }
  }
  return null;
}

export function generateAgentCatalog() {
  const catalog = fs.existsSync(categoryRoot)
    ? buildCatalog()
    : readExistingCatalog() ||
      (() => {
        throw new Error(
          `Agent catalog source directory is missing (${categoryRoot}) and no previously generated catalog was found.`
        );
      })();
  const serialized = `${JSON.stringify(catalog, null, 2)}\n`;

  for (const outputFile of outputFiles) {
    fs.mkdirSync(path.dirname(outputFile), { recursive: true });
    fs.writeFileSync(outputFile, serialized, "utf8");
  }

  return catalog;
}

if (import.meta.url === `file://${__filename}`) {
  generateAgentCatalog();
}

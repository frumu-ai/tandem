import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import ts from "typescript";

const here = dirname(fileURLToPath(import.meta.url));
const packageDir = join(here, "..");
const srcDir = join(packageDir, "src");
const typeScale = new Set(["micro", "caption", "xs", "sm", "base", "lg", "xl", "2xl"]);

function walk(dir, out = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) walk(full, out);
    else if (/\.(css|tsx?)$/.test(entry.name)) out.push(full);
  }
  return out;
}

function tagName(node) {
  return node.tagName?.getText();
}

function jsxAttribute(node, name) {
  return node.attributes.properties.find(
    (attribute) => ts.isJsxAttribute(attribute) && attribute.name.getText() === name,
  );
}

function expressionMayRenderText(expression) {
  if (
    !expression ||
    expression.kind === ts.SyntaxKind.NullKeyword ||
    expression.kind === ts.SyntaxKind.FalseKeyword ||
    expression.kind === ts.SyntaxKind.TrueKeyword ||
    ts.isJsxElement(expression) ||
    ts.isJsxSelfClosingElement(expression) ||
    ts.isJsxFragment(expression)
  ) {
    return false;
  }
  if (ts.isParenthesizedExpression(expression)) return expressionMayRenderText(expression.expression);
  if (ts.isConditionalExpression(expression)) {
    return (
      expressionMayRenderText(expression.whenTrue) || expressionMayRenderText(expression.whenFalse)
    );
  }
  if (
    ts.isBinaryExpression(expression) &&
    [
      ts.SyntaxKind.AmpersandAmpersandToken,
      ts.SyntaxKind.BarBarToken,
      ts.SyntaxKind.QuestionQuestionToken,
    ].includes(expression.operatorToken.kind)
  ) {
    return expressionMayRenderText(expression.left) || expressionMayRenderText(expression.right);
  }
  return true;
}

test("Icon, IconButton, and SearchInput reject unsafe props at typecheck", () => {
  const tempDir = mkdtempSync(join(packageDir, ".tan685-typecheck-"));
  const fixture = join(tempDir, "invalid.tsx");
  const uiModule = "../src/ui/index.tsx";
  writeFileSync(
    fixture,
    [
      `import { Icon, IconButton, SearchInput } from ${JSON.stringify(uiModule)};`,
      `const badIcon = <Icon name="definitely-not-an-icon" />;`,
      `const badButton = <IconButton><Icon name="x" /></IconButton>;`,
      `const badSearch = <SearchInput placeholder="Search" />;`,
    ].join("\n"),
  );

  try {
    const program = ts.createProgram([fixture], {
      allowImportingTsExtensions: true,
      jsx: ts.JsxEmit.ReactJSX,
      jsxImportSource: "react",
      module: ts.ModuleKind.ESNext,
      moduleResolution: ts.ModuleResolutionKind.Bundler,
      noEmit: true,
      skipLibCheck: true,
      target: ts.ScriptTarget.ES2022,
    });
    const diagnostics = ts.getPreEmitDiagnostics(program).filter((diagnostic) =>
      diagnostic.file?.fileName === fixture
    );
    const messages = diagnostics.map((diagnostic) => ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n"));
    assert.equal(diagnostics.length, 3, messages.join("\n\n"));
    assert.ok(messages.some((message) => message.includes("definitely-not-an-icon")), messages.join("\n"));
    assert.equal(messages.filter((message) => message.includes('"aria-label"')).length, 2);
  } finally {
    rmSync(tempDir, { recursive: true, force: true });
  }
});

test("loading and typography use the shared UI contracts", async () => {
  const files = walk(srcDir);
  const sourceFiles = files.filter((file) => /\.tsx?$/.test(file));
  const source = sourceFiles.map((file) => readFileSync(file, "utf8")).join("\n");
  assert.doesNotMatch(source, /\banimate-spin\b/, "use the shared Spinner component");
  assert.doesNotMatch(source.replace(readFileSync(join(srcDir, "ui", "index.tsx"), "utf8"), ""), /function\s+Spinner\b/);
  assert.doesNotMatch(source, /\btext-\[(?:\d|\.)[^\]]*\]/, "numeric arbitrary text sizes are forbidden");

  const tailwind = (await import(join(packageDir, "tailwind.config.js"))).default;
  assert.deepEqual(new Set(Object.keys(tailwind.theme.fontSize)), typeScale);

  const css = readFileSync(join(srcDir, "styles.css"), "utf8");
  assert.match(css, /\.tcp-spinner\s*\{[^}]*animation:\s*tcpSpin\b/s);
  const tandemLogo = readFileSync(join(srcDir, "ui", "TandemLogoAnimation.tsx"), "utf8");
  assert.match(tandemLogo, /tcp-tandem-logo-animation/);
  assert.match(tandemLogo, /tcp-tandem-logo-compact/);
  assert.match(css, /html\[data-theme="porcelain"\]\s+\.tcp-tandem-logo-compact\s*\{/);
  const declarations = [...css.matchAll(/font-size:\s*([^;]+);/g)].map((match) => match[1].trim());
  const allowedValues = new Set([...typeScale].map((name) => `var(--font-size-${name})`));
  assert.deepEqual(
    declarations.filter((value) => !allowedValues.has(value)),
    [],
    "styles.css font-size declarations must use the eight-size scale",
  );
});

test("search inputs and icon-only buttons have accessible names", () => {
  const problems = [];
  for (const file of walk(srcDir).filter((entry) => entry.endsWith(".tsx"))) {
    const source = readFileSync(file, "utf8");
    const parsed = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);

    function visit(node) {
      const opening = ts.isJsxElement(node)
        ? node.openingElement
        : ts.isJsxSelfClosingElement(node)
          ? node
          : null;
      if (opening && tagName(opening) === "input") {
        const placeholder = jsxAttribute(opening, "placeholder");
        const value = placeholder?.initializer && ts.isStringLiteral(placeholder.initializer)
          ? placeholder.initializer.text
          : "";
        if (/^(?:search|filter|type to filter|linear search)/i.test(value)) {
          const line = parsed.getLineAndCharacterOfPosition(opening.getStart()).line + 1;
          problems.push(`${relative(packageDir, file)}:${line} must use SearchInput`);
        }
      }

      if (ts.isJsxElement(node) && tagName(node.openingElement) === "button") {
        const labelled =
          jsxAttribute(node.openingElement, "aria-label") ||
          jsxAttribute(node.openingElement, "aria-labelledby");
        let iconCount = 0;
        let textCount = 0;
        function inspect(child) {
          const childOpening = ts.isJsxElement(child)
            ? child.openingElement
            : ts.isJsxSelfClosingElement(child)
              ? child
              : null;
          if (childOpening && ["Icon", "Spinner"].includes(tagName(childOpening))) iconCount += 1;
          if (ts.isJsxText(child) && child.text.trim()) textCount += 1;
          if (ts.isJsxExpression(child) && expressionMayRenderText(child.expression)) textCount += 1;
          ts.forEachChild(child, inspect);
        }
        node.children.forEach(inspect);
        if (iconCount && !textCount && !labelled) {
          const line = parsed.getLineAndCharacterOfPosition(node.getStart()).line + 1;
          problems.push(`${relative(packageDir, file)}:${line} icon-only button needs aria-label`);
        }
      }
      ts.forEachChild(node, visit);
    }
    visit(parsed);
  }
  assert.deepEqual(problems, []);
});

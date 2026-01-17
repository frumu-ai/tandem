#!/usr/bin/env node
/**
 * Pre-commit secret scanner
 * Checks staged files for potential API keys and secrets
 * 
 * Usage: node scripts/check-secrets.js [files...]
 */

import { readFileSync } from "fs";

// Patterns that indicate potential secrets
const SECRET_PATTERNS = [
  // OpenAI API keys
  { pattern: /sk-[a-zA-Z0-9]{20,}/, name: "OpenAI API key" },
  // Anthropic API keys
  { pattern: /sk-ant-[a-zA-Z0-9-]{20,}/, name: "Anthropic API key" },
  // OpenRouter API keys
  { pattern: /sk-or-[a-zA-Z0-9-]{20,}/, name: "OpenRouter API key" },
  // Generic API key patterns
  { pattern: /['"][a-zA-Z0-9_-]*api[_-]?key['"]?\s*[:=]\s*['"][a-zA-Z0-9_-]{20,}['"]/i, name: "Generic API key" },
  // AWS keys
  { pattern: /AKIA[0-9A-Z]{16}/, name: "AWS Access Key" },
  // Private keys
  { pattern: /-----BEGIN (RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----/, name: "Private key" },
  // Passwords in config
  { pattern: /password\s*[:=]\s*['"][^'"]{8,}['"]/i, name: "Hardcoded password" },
  // Bearer tokens
  { pattern: /Bearer\s+[a-zA-Z0-9_-]{20,}/, name: "Bearer token" },
  // Base64 encoded secrets (common pattern)
  { pattern: /secret\s*[:=]\s*['"][A-Za-z0-9+/=]{40,}['"]/i, name: "Base64 secret" },
];

// Files to skip (already in .gitignore but double-check)
const SKIP_PATTERNS = [
  /node_modules/,
  /\.git\//,
  /dist\//,
  /target\//,
  /\.env$/,
  /\.env\./,
  /\.pem$/,
  /\.key$/,
  /\.stronghold$/,
];

function shouldSkipFile(filepath) {
  return SKIP_PATTERNS.some(pattern => pattern.test(filepath));
}

function checkFile(filepath) {
  if (shouldSkipFile(filepath)) {
    return [];
  }

  let content;
  try {
    content = readFileSync(filepath, "utf-8");
  } catch (err) {
    // File might not exist or be binary
    return [];
  }

  const issues = [];
  const lines = content.split("\n");

  lines.forEach((line, index) => {
    // Skip comments (basic detection)
    const trimmedLine = line.trim();
    if (trimmedLine.startsWith("//") || trimmedLine.startsWith("#") || trimmedLine.startsWith("*")) {
      // Still check for actual keys in comments (they shouldn't be there either)
    }

    SECRET_PATTERNS.forEach(({ pattern, name }) => {
      if (pattern.test(line)) {
        issues.push({
          file: filepath,
          line: index + 1,
          type: name,
          content: line.substring(0, 100) + (line.length > 100 ? "..." : ""),
        });
      }
    });
  });

  return issues;
}

function main() {
  const files = process.argv.slice(2);
  
  if (files.length === 0) {
    console.log("No files to check");
    process.exit(0);
  }

  console.log(`🔍 Scanning ${files.length} file(s) for secrets...\\n`);

  let allIssues = [];

  for (const file of files) {
    const issues = checkFile(file);
    allIssues = allIssues.concat(issues);
  }

  if (allIssues.length > 0) {
    console.error("❌ Potential secrets detected!\\n");
    
    allIssues.forEach(issue => {
      console.error(`  ${issue.file}:${issue.line}`);
      console.error(`    Type: ${issue.type}`);
      console.error(`    Content: ${issue.content}\\n`);
    });

    console.error("\\n⚠️  Please remove secrets before committing.");
    console.error("   Store sensitive data in environment variables or the app's secure storage.\\n");
    
    process.exit(1);
  }

  console.log("✅ No secrets detected\\n");
  process.exit(0);
}

main();

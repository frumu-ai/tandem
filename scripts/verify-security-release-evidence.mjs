#!/usr/bin/env node

import { writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SHA = /^[0-9a-f]{40}$/;
const FORBIDDEN_KEY = /(?:token|secret|password|private.?key)$/i;
const FORBIDDEN_EXACT_KEY =
  /^(?:authorization|proxyAuthorization|cookie|setCookie|session|credential|credentials|accessKey|apiKey|clientSecret|refreshToken|idToken|signedUrl)$/i;
const FORBIDDEN_VALUES = [
  /-----BEGIN [A-Z ]*PRIVATE KEY-----/i,
  /\btk_[0-9a-f]{32}\b/i,
  /\bBearer\s+\S+/i,
  /\b(?:gh[pousr]_[A-Za-z0-9_]{20,}|github_pat_[A-Za-z0-9_]{20,})\b/,
  /\bAKIA[0-9A-Z]{16}\b/,
  /\bAIza[0-9A-Za-z_-]{30,}\b/,
  /\bxox[baprs]-[A-Za-z0-9-]{10,}\b/i,
  /\beyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\b/,
  /[?&](?:X-Amz-(?:Credential|Signature|Security-Token)|sig|signature|token|access_token)=/i,
  /https?:\/\/[^/\s:@]+:[^/\s@]+@/i,
];
const EVIDENCE_REFERENCE = /^urn:tandem:evidence:([a-z][a-z0-9-]{2,31}):sha256:([0-9a-f]{64})$/;

function requireValue(condition, message, errors) {
  if (!condition) errors.push(message);
}

function evidenceReference(group, kind, errors, references) {
  const value = typeof group?.evidenceRef === "string" ? group.evidenceRef.trim() : "";
  const match = value.match(EVIDENCE_REFERENCE);
  requireValue(
    match?.[1] === kind,
    `${kind} requires an immutable urn:tandem:evidence:${kind}:sha256:<64 lowercase hex> evidenceRef`,
    errors
  );
  if (match) {
    requireValue(!references.has(value), `${kind} evidenceRef must be unique`, errors);
    references.add(value);
  }
}

function rejectSecretMaterial(value, errors, cursor = "evidence") {
  if (Array.isArray(value)) {
    value.forEach((entry, index) => rejectSecretMaterial(entry, errors, `${cursor}[${index}]`));
    return;
  }
  if (value && typeof value === "object") {
    for (const [key, nested] of Object.entries(value)) {
      if (FORBIDDEN_KEY.test(key) || FORBIDDEN_EXACT_KEY.test(key)) {
        errors.push(`${cursor}.${key} is a forbidden secret-bearing field`);
      }
      rejectSecretMaterial(nested, errors, `${cursor}.${key}`);
    }
    return;
  }
  if (typeof value === "string" && FORBIDDEN_VALUES.some((pattern) => pattern.test(value))) {
    errors.push(`${cursor} appears to contain secret material`);
  }
}

export function validateReleaseEvidence(evidence, { expectedCommit, now = new Date() } = {}) {
  const errors = [];
  const evidenceReferences = new Set();
  requireValue(evidence?.schemaVersion === 1, "schemaVersion must be 1", errors);
  requireValue(
    evidence?.profile === "hosted-enterprise",
    "profile must be hosted-enterprise",
    errors
  );
  requireValue(
    SHA.test(evidence?.commitSha || ""),
    "commitSha must be a lowercase 40-character SHA",
    errors
  );
  if (expectedCommit)
    requireValue(
      evidence?.commitSha === expectedCommit,
      "evidence commitSha does not match the workflow commit",
      errors
    );
  requireValue(
    typeof evidence?.environmentId === "string" && evidence.environmentId.trim().length >= 3,
    "environmentId is required",
    errors
  );

  const observedAt = new Date(evidence?.observedAt || "invalid");
  const expiresAt = new Date(evidence?.expiresAt || "invalid");
  requireValue(
    Number.isFinite(observedAt.valueOf()),
    "observedAt must be an ISO timestamp",
    errors
  );
  requireValue(Number.isFinite(expiresAt.valueOf()), "expiresAt must be an ISO timestamp", errors);
  if (Number.isFinite(observedAt.valueOf()) && Number.isFinite(expiresAt.valueOf())) {
    requireValue(observedAt <= now, "observedAt cannot be in the future", errors);
    requireValue(expiresAt > now, "release evidence is expired", errors);
    requireValue(
      expiresAt - observedAt <= 30 * 24 * 60 * 60 * 1000,
      "release evidence may be valid for at most 30 days",
      errors
    );
  }

  const controls = evidence?.controls || {};
  const postgres = controls.postgresql || {};
  requireValue(postgres.tlsVerified === true, "PostgreSQL TLS must be verified", errors);
  requireValue(
    postgres.certificateVerification === "full",
    "PostgreSQL certificate verification must be full",
    errors
  );
  evidenceReference(postgres, "postgresql", errors, evidenceReferences);

  const kmsIam = controls.kmsIam || {};
  requireValue(
    kmsIam.envelopeEncryptionVerified === true,
    "KMS envelope encryption must be verified",
    errors
  );
  requireValue(
    kmsIam.workloadIdentityVerified === true,
    "IAM workload identity must be verified",
    errors
  );
  requireValue(
    kmsIam.staticCredentialCount === 0,
    "static cloud credential count must be zero",
    errors
  );
  evidenceReference(kmsIam, "kms-iam", errors, evidenceReferences);

  const proxy = controls.reverseProxy || {};
  requireValue(
    ["1.2", "1.3"].includes(proxy.tlsMinimum),
    "reverse proxy minimum TLS must be 1.2 or 1.3",
    errors
  );
  for (const field of ["hsts", "contentSecurityPolicy", "frameAncestors", "contentTypeOptions"]) {
    requireValue(proxy[field] === true, `reverse proxy ${field} control must pass`, errors);
  }
  evidenceReference(proxy, "reverse-proxy", errors, evidenceReferences);

  const replicas = controls.multiReplica || {};
  requireValue(
    Number.isInteger(replicas.replicaCount) && replicas.replicaCount >= 2,
    "multi-replica validation requires at least two replicas",
    errors
  );
  requireValue(replicas.failoverPass === true, "multi-replica failover test must pass", errors);
  requireValue(
    replicas.crossReplicaAuthorizationPass === true,
    "cross-replica authorization test must pass",
    errors
  );
  evidenceReference(replicas, "multi-replica", errors, evidenceReferences);

  const egress = controls.egress || {};
  requireValue(egress.defaultDeny === true, "egress must default deny", errors);
  requireValue(egress.deniedProbePass === true, "denied egress probe must pass", errors);
  requireValue(egress.allowlistReviewed === true, "egress allowlist must be reviewed", errors);
  evidenceReference(egress, "egress", errors, evidenceReferences);

  rejectSecretMaterial(evidence, errors);
  return errors;
}

function testEvidenceReference(kind, hexCharacter) {
  return `urn:tandem:evidence:${kind}:sha256:${hexCharacter.repeat(64)}`;
}

function validFixture(now = new Date()) {
  return {
    schemaVersion: 1,
    profile: "hosted-enterprise",
    commitSha: "a".repeat(40),
    environmentId: "test-environment",
    observedAt: new Date(now.valueOf() - 60_000).toISOString(),
    expiresAt: new Date(now.valueOf() + 24 * 60 * 60 * 1000).toISOString(),
    controls: {
      postgresql: {
        tlsVerified: true,
        certificateVerification: "full",
        evidenceRef: testEvidenceReference("postgresql", "1"),
      },
      kmsIam: {
        envelopeEncryptionVerified: true,
        workloadIdentityVerified: true,
        staticCredentialCount: 0,
        evidenceRef: testEvidenceReference("kms-iam", "2"),
      },
      reverseProxy: {
        tlsMinimum: "1.3",
        hsts: true,
        contentSecurityPolicy: true,
        frameAncestors: true,
        contentTypeOptions: true,
        evidenceRef: testEvidenceReference("reverse-proxy", "3"),
      },
      multiReplica: {
        replicaCount: 2,
        failoverPass: true,
        crossReplicaAuthorizationPass: true,
        evidenceRef: testEvidenceReference("multi-replica", "4"),
      },
      egress: {
        defaultDeny: true,
        deniedProbePass: true,
        allowlistReviewed: true,
        evidenceRef: testEvidenceReference("egress", "5"),
      },
    },
  };
}

function selfTest() {
  const now = new Date();
  const valid = validFixture(now);
  if (validateReleaseEvidence(valid, { expectedCommit: valid.commitSha, now }).length !== 0) {
    throw new Error("release-evidence self-test rejected valid evidence");
  }
  const invalid = structuredClone(valid);
  invalid.controls.egress.defaultDeny = false;
  invalid.apiToken = `tk_${"0".repeat(32)}`;
  if (validateReleaseEvidence(invalid, { expectedCommit: valid.commitSha, now }).length < 2) {
    throw new Error("release-evidence self-test accepted invalid evidence");
  }
  for (const [name, mutate] of [
    ["placeholder", (fixture) => (fixture.controls.egress.evidenceRef = "abc")],
    ["authorization", (fixture) => (fixture.authorization = "Bearer secret-value")],
    ["presigned URL", (fixture) => (fixture.url = "https://example.test/a?X-Amz-Signature=abc")],
    [
      "JWT",
      (fixture) => (fixture.note = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.signature"),
    ],
  ]) {
    const fixture = structuredClone(valid);
    mutate(fixture);
    if (validateReleaseEvidence(fixture, { expectedCommit: valid.commitSha, now }).length === 0) {
      throw new Error(`release-evidence self-test accepted ${name}`);
    }
  }
}

function argValue(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

async function main() {
  if (process.argv.includes("--self-test")) {
    selfTest();
    process.stdout.write("security release evidence self-test passed\n");
    return;
  }
  const environmentVariable = argValue("--from-env");
  const expectedCommit = argValue("--expected-commit");
  if (!environmentVariable)
    throw new Error("--from-env is required; hosted release evidence is fail-closed");
  const raw = process.env[environmentVariable];
  if (!raw)
    throw new Error(`${environmentVariable} is unavailable; hosted-enterprise release is blocked`);
  let evidence;
  try {
    evidence = JSON.parse(raw);
  } catch {
    throw new Error(`${environmentVariable} is not valid JSON`);
  }
  const errors = validateReleaseEvidence(evidence, { expectedCommit });
  if (errors.length > 0)
    throw new Error(`hosted-enterprise release evidence failed:\n${errors.join("\n")}`);
  const summary = {
    schemaVersion: evidence.schemaVersion,
    profile: evidence.profile,
    commitSha: evidence.commitSha,
    environmentId: evidence.environmentId,
    observedAt: evidence.observedAt,
    expiresAt: evidence.expiresAt,
    verifiedControlGroups: Object.keys(evidence.controls).sort(),
  };
  const output = argValue("--output");
  if (output) await writeFile(output, `${JSON.stringify(summary, null, 2)}\n`, { mode: 0o600 });
  process.stdout.write(`hosted-enterprise release evidence passed for ${evidence.commitSha}\n`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  main().catch((error) => {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  });
}

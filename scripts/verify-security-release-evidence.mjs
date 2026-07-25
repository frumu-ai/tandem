#!/usr/bin/env node

import { writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SHA = /^[0-9a-f]{40}$/;
const FORBIDDEN_KEY = /(?:token|secret|password|private.?key)$/i;
const FORBIDDEN_EXACT_KEY = /^(?:credential|credentials|accessKey|apiKey|clientSecret)$/i;
const FORBIDDEN_VALUE = /(?:-----BEGIN [A-Z ]*PRIVATE KEY-----|\btk_[0-9a-f]{32}\b)/i;

function requireValue(condition, message, errors) {
  if (!condition) errors.push(message);
}

function evidenceReference(group, errors) {
  requireValue(
    typeof group?.evidenceRef === "string" && group.evidenceRef.trim().length >= 3,
    "every control group requires a non-empty evidenceRef",
    errors
  );
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
  if (typeof value === "string" && FORBIDDEN_VALUE.test(value)) {
    errors.push(`${cursor} appears to contain secret material`);
  }
}

export function validateReleaseEvidence(evidence, { expectedCommit, now = new Date() } = {}) {
  const errors = [];
  requireValue(evidence?.schemaVersion === 1, "schemaVersion must be 1", errors);
  requireValue(evidence?.profile === "hosted-enterprise", "profile must be hosted-enterprise", errors);
  requireValue(SHA.test(evidence?.commitSha || ""), "commitSha must be a lowercase 40-character SHA", errors);
  if (expectedCommit) requireValue(evidence?.commitSha === expectedCommit, "evidence commitSha does not match the workflow commit", errors);
  requireValue(typeof evidence?.environmentId === "string" && evidence.environmentId.trim().length >= 3, "environmentId is required", errors);

  const observedAt = new Date(evidence?.observedAt || "invalid");
  const expiresAt = new Date(evidence?.expiresAt || "invalid");
  requireValue(Number.isFinite(observedAt.valueOf()), "observedAt must be an ISO timestamp", errors);
  requireValue(Number.isFinite(expiresAt.valueOf()), "expiresAt must be an ISO timestamp", errors);
  if (Number.isFinite(observedAt.valueOf()) && Number.isFinite(expiresAt.valueOf())) {
    requireValue(observedAt <= now, "observedAt cannot be in the future", errors);
    requireValue(expiresAt > now, "release evidence is expired", errors);
    requireValue(expiresAt - observedAt <= 30 * 24 * 60 * 60 * 1000, "release evidence may be valid for at most 30 days", errors);
  }

  const controls = evidence?.controls || {};
  const postgres = controls.postgresql || {};
  requireValue(postgres.tlsVerified === true, "PostgreSQL TLS must be verified", errors);
  requireValue(postgres.certificateVerification === "full", "PostgreSQL certificate verification must be full", errors);
  evidenceReference(postgres, errors);

  const kmsIam = controls.kmsIam || {};
  requireValue(kmsIam.envelopeEncryptionVerified === true, "KMS envelope encryption must be verified", errors);
  requireValue(kmsIam.workloadIdentityVerified === true, "IAM workload identity must be verified", errors);
  requireValue(kmsIam.staticCredentialCount === 0, "static cloud credential count must be zero", errors);
  evidenceReference(kmsIam, errors);

  const proxy = controls.reverseProxy || {};
  requireValue(["1.2", "1.3"].includes(proxy.tlsMinimum), "reverse proxy minimum TLS must be 1.2 or 1.3", errors);
  for (const field of ["hsts", "contentSecurityPolicy", "frameAncestors", "contentTypeOptions"]) {
    requireValue(proxy[field] === true, `reverse proxy ${field} control must pass`, errors);
  }
  evidenceReference(proxy, errors);

  const replicas = controls.multiReplica || {};
  requireValue(Number.isInteger(replicas.replicaCount) && replicas.replicaCount >= 2, "multi-replica validation requires at least two replicas", errors);
  requireValue(replicas.failoverPass === true, "multi-replica failover test must pass", errors);
  requireValue(replicas.crossReplicaAuthorizationPass === true, "cross-replica authorization test must pass", errors);
  evidenceReference(replicas, errors);

  const egress = controls.egress || {};
  requireValue(egress.defaultDeny === true, "egress must default deny", errors);
  requireValue(egress.deniedProbePass === true, "denied egress probe must pass", errors);
  requireValue(egress.allowlistReviewed === true, "egress allowlist must be reviewed", errors);
  evidenceReference(egress, errors);

  rejectSecretMaterial(evidence, errors);
  return errors;
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
      postgresql: { tlsVerified: true, certificateVerification: "full", evidenceRef: "test-postgresql" },
      kmsIam: { envelopeEncryptionVerified: true, workloadIdentityVerified: true, staticCredentialCount: 0, evidenceRef: "test-kms" },
      reverseProxy: { tlsMinimum: "1.3", hsts: true, contentSecurityPolicy: true, frameAncestors: true, contentTypeOptions: true, evidenceRef: "test-proxy" },
      multiReplica: { replicaCount: 2, failoverPass: true, crossReplicaAuthorizationPass: true, evidenceRef: "test-replicas" },
      egress: { defaultDeny: true, deniedProbePass: true, allowlistReviewed: true, evidenceRef: "test-egress" },
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
  if (!environmentVariable) throw new Error("--from-env is required; hosted release evidence is fail-closed");
  const raw = process.env[environmentVariable];
  if (!raw) throw new Error(`${environmentVariable} is unavailable; hosted-enterprise release is blocked`);
  let evidence;
  try {
    evidence = JSON.parse(raw);
  } catch {
    throw new Error(`${environmentVariable} is not valid JSON`);
  }
  const errors = validateReleaseEvidence(evidence, { expectedCommit });
  if (errors.length > 0) throw new Error(`hosted-enterprise release evidence failed:\n${errors.join("\n")}`);
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

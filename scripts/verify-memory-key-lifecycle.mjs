#!/usr/bin/env node

/**
 * Verifies the current hosted memory key lifecycle evidence.
 *
 * This is intentionally evidence-based. TAN-113/TAN-114 added key-scope
 * metadata and a provider-generic decrypt broker; TAN-115 adds lifecycle
 * controls and a claim gate. Concrete hosted KMS provider provisioning is a
 * separate follow-up, so this script reports buyer-claim blockers by default
 * and fails only when hosted key evidence is explicitly required.
 */

import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const repoRoot = path.resolve(new URL("..", import.meta.url).pathname);

const files = {
  envelope: "crates/tandem-memory/src/envelope.rs",
  decryptBroker: "crates/tandem-memory/src/decrypt_broker.rs",
  keyLifecycle: "crates/tandem-memory/src/key_lifecycle.rs",
  memoryLib: "crates/tandem-memory/src/lib.rs",
  ci: ".github/workflows/ci.yml",
};

function readRepoFile(relativePath) {
  return fs.readFileSync(path.join(repoRoot, relativePath), "utf8");
}

function exists(relativePath) {
  return fs.existsSync(path.join(repoRoot, relativePath));
}

const requireHostedKeyEvidence =
  process.env.TANDEM_HOSTED_REQUIRE_MEMORY_KEY_EVIDENCE === "1" ||
  process.env.TANDEM_HOSTED_ENCRYPTED_MEMORY_CLAIM === "1";

const envelope = readRepoFile(files.envelope);
const decryptBroker = readRepoFile(files.decryptBroker);
const keyLifecycle = exists(files.keyLifecycle) ? readRepoFile(files.keyLifecycle) : "";
const memoryLib = readRepoFile(files.memoryLib);
const ci = readRepoFile(files.ci);

const evidence = {
  envelope_metadata: {
    key_scope: /struct MemoryKeyScope/.test(envelope),
    kek_id: /kek_id/.test(envelope),
    kek_version: /kek_version/.test(envelope),
    wrapped_dek: /wrapped_dek/.test(envelope),
    rotation_epoch: /rotation_epoch/.test(envelope),
  },
  decrypt_broker: {
    provider_generic_trait: /trait MemoryDekUnwrapProvider/.test(decryptBroker),
    scoped_runtime_principal: /runtime_principal_id/.test(decryptBroker),
    lifecycle_policy_on_request: /key_lifecycle_policy/.test(decryptBroker),
    lifecycle_denial_before_ticket:
      /evaluate_memory_key_lifecycle/.test(decryptBroker) &&
      /MemoryKeyLifecycleOutcome::Denied/.test(decryptBroker),
    wrapped_dek_in_ticket: /pub wrapped_dek: String/.test(decryptBroker),
  },
  key_lifecycle: {
    module_exported: /pub mod key_lifecycle/.test(memoryLib),
    version_states:
      /enum MemoryKeyVersionState/.test(keyLifecycle) &&
      /Primary/.test(keyLifecycle) &&
      /Active/.test(keyLifecycle) &&
      /Disabled/.test(keyLifecycle) &&
      /Revoked/.test(keyLifecycle) &&
      /Destroyed/.test(keyLifecycle),
    scoped_revocation: /struct MemoryKeyScopeRevocation/.test(keyLifecycle),
    break_glass_grant: /struct MemoryBreakGlassGrant/.test(keyLifecycle),
    break_glass_ttl: /expires_at_ms/.test(keyLifecycle),
    break_glass_export_limit: /max_export_items/.test(keyLifecycle),
    rotation_epoch_gate: /minimum_rotation_epoch/.test(keyLifecycle),
  },
  ci: {
    memory_db_boundary_check: /verify-memory-db-blast-radius\.mjs/.test(ci),
    memory_key_lifecycle_check: /verify-memory-key-lifecycle\.mjs/.test(ci),
  },
  hosted_provider_provisioning: {
    concrete_provider_present:
      /impl\s+MemoryDekUnwrapProvider\s+for/.test(decryptBroker) ||
      /impl\s+MemoryDekUnwrapProvider\s+for/.test(keyLifecycle),
    note:
      "Concrete hosted KMS provider/provisioning is intentionally tracked separately by TAN-116.",
  },
};

const failures = [];
const blockers = [];

function requireEvidence(condition, message) {
  if (!condition) {
    failures.push(message);
  }
}

requireEvidence(evidence.envelope_metadata.key_scope, "memory envelope key scope metadata missing");
requireEvidence(evidence.envelope_metadata.kek_id, "memory envelope kek_id missing");
requireEvidence(evidence.envelope_metadata.kek_version, "memory envelope kek_version missing");
requireEvidence(evidence.envelope_metadata.wrapped_dek, "memory envelope wrapped_dek missing");
requireEvidence(evidence.envelope_metadata.rotation_epoch, "memory envelope rotation_epoch missing");
requireEvidence(
  evidence.decrypt_broker.provider_generic_trait,
  "provider-generic memory DEK unwrap trait missing",
);
requireEvidence(
  evidence.decrypt_broker.scoped_runtime_principal,
  "decrypt broker scoped runtime principal evidence missing",
);
requireEvidence(
  evidence.decrypt_broker.lifecycle_policy_on_request,
  "decrypt broker lifecycle policy hook missing",
);
requireEvidence(
  evidence.decrypt_broker.lifecycle_denial_before_ticket,
  "decrypt broker does not deny lifecycle failures before unwrap ticket issuance",
);
requireEvidence(
  evidence.decrypt_broker.wrapped_dek_in_ticket,
  "unwrap ticket does not carry wrapped DEK material",
);
requireEvidence(evidence.key_lifecycle.module_exported, "memory key lifecycle module not exported");
requireEvidence(evidence.key_lifecycle.version_states, "memory key lifecycle states incomplete");
requireEvidence(evidence.key_lifecycle.scoped_revocation, "memory key scoped revocation missing");
requireEvidence(evidence.key_lifecycle.break_glass_grant, "memory break-glass grant model missing");
requireEvidence(evidence.key_lifecycle.break_glass_ttl, "memory break-glass TTL evidence missing");
requireEvidence(
  evidence.key_lifecycle.break_glass_export_limit,
  "memory break-glass export limit evidence missing",
);
requireEvidence(evidence.key_lifecycle.rotation_epoch_gate, "memory rotation epoch gate missing");
requireEvidence(evidence.ci.memory_key_lifecycle_check, "memory key lifecycle evidence script is not in CI");

if (!evidence.hosted_provider_provisioning.concrete_provider_present) {
  blockers.push(
    "hosted memory KMS provider/provisioning is not implemented yet; keep encrypted-memory containment buyer claims blocked until TAN-116 or equivalent evidence lands",
  );
}

if (!requireHostedKeyEvidence) {
  blockers.push(
    "hosted memory key evidence is not required in this run; set TANDEM_HOSTED_REQUIRE_MEMORY_KEY_EVIDENCE=1 to fail closed for hosted claim gates",
  );
}

const report = {
  hosted_key_evidence_required: requireHostedKeyEvidence,
  evidence,
  buyer_claim: {
    encrypted_memory_containment_claim_allowed:
      failures.length === 0 &&
      blockers.length === 0 &&
      evidence.hosted_provider_provisioning.concrete_provider_present,
    note:
      "Do not claim hosted encrypted-memory blast-radius containment until key lifecycle evidence and concrete provider/provisioning evidence are complete.",
  },
  blockers,
  failures,
};

console.log(JSON.stringify(report, null, 2));

if (failures.length > 0 || (requireHostedKeyEvidence && blockers.length > 0)) {
  process.exitCode = 1;
}

# SIEM Export and Immutable Storage Guide

This document describes Tandem's audit export endpoints, the NDJSON bundle format, how to
verify exported records, and how to configure immutable (WORM) storage for regulated
deployments.

This is implementation guidance for Tandem operators and deployers. It is not a legal
determination for any specific deployment.

---

## Audit Export Endpoints

All endpoints require an `api_token` or `control_panel` request source. Requests from
other sources receive `403 AUDIT_ADMIN_REQUIRED`.

### `GET /audit/ledger/manifest`

Returns the verification manifest for the protected audit ledger as JSON.

```json
{
  "ledger_path": "/var/tandem/audit/protected.ndjson",
  "schema_version": 2,
  "record_count": 847,
  "last_seq": 847,
  "root_hash": "a3f2...c91d",
  "generated_at_ms": 1718400000000
}
```

**Fields:**

| Field | Description |
|-------|-------------|
| `ledger_path` | Filesystem path to the protected audit ledger. |
| `schema_version` | Always 2 for hash-chained ledgers. |
| `record_count` | Number of parseable records in the file. |
| `last_seq` | Highest `seq` value seen; gaps between this and `record_count` indicate pre-v2 records or corruption. |
| `root_hash` | SHA-256 hash of the last hash-chained record. Verifiable by re-hashing the exported bundle. |
| `generated_at_ms` | Epoch ms when this manifest was produced. |

Use this endpoint to check ledger integrity before or after an export. Run it periodically
(e.g. daily, or after each deployment) and store the manifest in an external audit log or
immutable store so you can detect tampering after the fact.

---

### `GET /audit/ledger/export`

Produces a deterministic NDJSON bundle of protected audit events for the requesting tenant,
followed by a `bundle_manifest` trailer record. Suitable for SIEM ingestion, long-term
retention, and offline verification.

**Query parameters:**

| Parameter | Type | Description |
|-----------|------|-------------|
| `since_ms` | `u64` (optional) | Include only records with `created_at_ms ≥ since_ms`. |
| `until_ms` | `u64` (optional) | Include only records with `created_at_ms ≤ until_ms`. |

**Response:** `Content-Type: application/x-ndjson`

Each line is a JSON object. The final line is always a `bundle_manifest` record.

**Example bundle:**

```ndjson
{"event_id":"a1b2...","event_type":"fintech.protected_action.approved","seq":1,"prev_hash":null,"record_hash":"7f3a...","created_at_ms":1718300000000,...}
{"event_id":"c3d4...","event_type":"approval.decision.recorded","seq":2,"prev_hash":"7f3a...","record_hash":"9e1b...","created_at_ms":1718300001000,...}
{"type":"bundle_manifest","schema_version":2,"record_count":2,"last_seq":2,"root_hash":"9e1b...","tenant_org_id":"acme","tenant_workspace_id":"prod","since_ms":null,"until_ms":null,"exported_at_ms":1718400000000}
```

**Bundle manifest fields:**

| Field | Description |
|-------|-------------|
| `type` | Always `"bundle_manifest"`. |
| `schema_version` | Always `2`. |
| `record_count` | Number of event records in this bundle (excludes manifest trailer). |
| `last_seq` | Highest `seq` in the exported records. For partial time-range exports, this may be less than the global ledger's `last_seq`. |
| `root_hash` | `record_hash` of the last hash-chained record in the bundle. |
| `tenant_org_id` / `tenant_workspace_id` | Tenant scope of this export. |
| `since_ms` / `until_ms` | Time range filters applied, or `null` if none. |
| `exported_at_ms` | Epoch ms when this bundle was generated. |

---

## Verifying an Exported Bundle

Each event record carries `seq`, `prev_hash`, and `record_hash`. The chain can be
re-verified offline:

1. Parse the NDJSON bundle, discarding the final `bundle_manifest` line.
2. Sort records by `seq`.
3. For each record (skipping records with an empty `record_hash`):
   a. Reconstruct the canonical JSON by serializing the record fields in the documented
      field order, **excluding** `record_hash`.
   b. Compute `SHA-256(canonical_json)` as a lowercase hex string.
   c. Assert the computed hash equals the record's `record_hash`.
   d. Assert `prev_hash` equals the `record_hash` of the previous hashed record (or
      `null` for the first hashed record in the chain).
4. Assert `seq` increments by 1 from 1 to `last_seq` with no gaps.
5. Assert the `root_hash` in the bundle manifest equals the `record_hash` of the last
   hashed record.

A reference verifier is available via `GET /audit/ledger/manifest`, which runs the same
checks server-side and reports any `violation` found.

### Canonical form for hashing

The fields hashed for each `ProtectedAuditEnvelope` record, in this order:

```json
{
  "event_id": "...",
  "durability_str": "durable_required",
  "event_type": "...",
  "tenant_org_id": "...",
  "tenant_workspace_id": "...",
  "tenant_deployment_id": null,
  "tenant_actor_id": null,
  "tenant_source": "implicit",
  "actor": null,
  "payload": {...},
  "created_at_ms": 1718300000000,
  "seq": 1,
  "prev_hash": null
}
```

`durability_str` is `"durable_required"` or `"best_effort"`. `tenant_source` is the
serialized `TenantSource` enum value (e.g. `"implicit"`, `"api"`, `"control_panel"`).
`actor` is always serialized (no skip-if-null) for hash stability.

---

## SIEM Integration

### Splunk

Configure a Splunk HEC (HTTP Event Collector) input and pipe the NDJSON export:

```bash
curl -s -H "Authorization: Bearer $TANDEM_TOKEN" \
  "https://tandem.internal/audit/ledger/export?since_ms=$LAST_EXPORT_MS" \
  | grep -v '"type":"bundle_manifest"' \
  | while IFS= read -r line; do
      curl -s -X POST \
        -H "Authorization: Splunk $SPLUNK_HEC_TOKEN" \
        -H "Content-Type: application/json" \
        -d "{\"sourcetype\": \"tandem:protected_audit\", \"event\": $line}" \
        "$SPLUNK_HEC_URL/services/collector/event"
    done
```

Set `since_ms` to the epoch ms of the last successful export to avoid re-sending records.

### Elastic / OpenSearch

Use Logstash or a simple script to POST each record to an Elasticsearch data stream:

```bash
curl -s -H "Authorization: Bearer $TANDEM_TOKEN" \
  "https://tandem.internal/audit/ledger/export" \
  | grep -v '"type":"bundle_manifest"' \
  | while IFS= read -r line; do
      curl -s -X POST \
        -H "Content-Type: application/json" \
        -u "$ELASTIC_USER:$ELASTIC_PASS" \
        -d "$line" \
        "$ELASTIC_URL/tandem-protected-audit/_doc"
    done
```

### Generic SIEM / log aggregator

Any SIEM that accepts NDJSON over HTTP can ingest the export directly. The bundle is
deterministic and idempotent for a given time range — re-exporting the same window
produces the same records.

---

## Immutable Storage Configuration

For regulated deployments, audit ledger files should be written to WORM (Write Once Read
Many) storage. The Tandem server writes the ledger to the filesystem path configured in
`protected_audit_path`. Mount or sync that path to your immutable storage backend.

### AWS S3 Object Lock

1. Create an S3 bucket with Object Lock enabled in **COMPLIANCE** mode.
2. Set a default retention period of at least 7 years for MiFID II / EBA workloads, or
   per your jurisdiction's minimum.
3. Schedule a periodic export (e.g. daily):

   ```bash
   # Export today's events
   DATE=$(date -u +%Y%m%d)
   curl -s -H "Authorization: Bearer $TANDEM_TOKEN" \
     "https://tandem.internal/audit/ledger/export?since_ms=$SINCE&until_ms=$UNTIL" \
     -o "audit-${DATE}.ndjson"

   # Upload to S3 with Object Lock
   aws s3 cp "audit-${DATE}.ndjson" \
     "s3://$BUCKET/tandem-audit/${DATE}/audit.ndjson" \
     --object-lock-mode COMPLIANCE \
     --object-lock-retain-until-date "$(date -u -d '+7 years' +%Y-%m-%dT%H:%M:%SZ)"

   # Also upload the manifest for independent verification
   curl -s -H "Authorization: Bearer $TANDEM_TOKEN" \
     "https://tandem.internal/audit/ledger/manifest" \
     -o "manifest-${DATE}.json"
   aws s3 cp "manifest-${DATE}.json" \
     "s3://$BUCKET/tandem-audit/${DATE}/manifest.json" \
     --object-lock-mode COMPLIANCE \
     --object-lock-retain-until-date "$(date -u -d '+7 years' +%Y-%m-%dT%H:%M:%SZ)"
   ```

4. Enable **S3 Versioning** and **MFA Delete** on the bucket.
5. Enable **CloudTrail** logging for the bucket to audit who accessed or attempted to
   modify records.

### Azure Blob Storage Immutable Policy

1. Create a storage account with **Immutable Blob Storage** enabled.
2. Configure a **Time-Based Retention Policy** with a minimum retention of 7 years
   (or per your policy) in **locked** mode.
3. Use the same export script above and upload with `az storage blob upload`:

   ```bash
   az storage blob upload \
     --account-name "$ACCOUNT" \
     --container-name tandem-audit \
     --name "${DATE}/audit.ndjson" \
     --file "audit-${DATE}.ndjson" \
     --auth-mode login
   ```

4. Confirm the container policy is **locked** (not just configured) before production use.

### Google Cloud Storage Retention Lock

1. Create a GCS bucket with a **Retention Policy** of at least 7 years.
2. **Lock** the retention policy once configured (cannot be shortened after locking).
3. Upload using `gcloud storage`:

   ```bash
   gcloud storage cp "audit-${DATE}.ndjson" \
     "gs://$BUCKET/tandem-audit/${DATE}/audit.ndjson"
   ```

4. Enable **Bucket Lock** and **Audit Logging** via Cloud Audit Logs.

### Customer-managed WORM equivalents

Any WORM-capable storage system that:
- Prevents deletion or modification of objects for the retention period
- Produces audit logs of access and attempted modifications
- Supports independent read access for verification

...is suitable. Examples: NetApp StorageGRID, Cohesity DataLock, Commvault Ransomware
Protection, or an append-only filesystem (ZFS with no-delete ACL).

---

## Export Failure Handling

The `/audit/ledger/export` endpoint is synchronous and fail-fast: if serialization fails
for any record, it returns `500 AUDIT_EXPORT_SERIALIZE_ERROR` rather than a partial bundle.
This prevents a truncated bundle from appearing valid.

For scheduled exports:
- Treat any non-200 response as a failure and alert.
- Compare `record_count` in the bundle manifest to the last known `record_count` from
  `/audit/ledger/manifest` — a decrease indicates ledger corruption.
- Compare `root_hash` values between consecutive exports for the same time window. A
  mismatch indicates ledger tampering.
- Store each manifest response separately from the bundle so you can verify the bundle
  independently after the fact.

---

## Residual Gaps

The following are not yet implemented and are tracked as follow-up issues:

| Gap | Issue |
|-----|-------|
| Server-side scheduled export to immutable storage (push model) | TAN-248 follow-up |
| Automatic SIEM connector with retry / backpressure | TAN-248 follow-up |
| Per-tenant configurable retention enforcement | TAN-249 |
| Signed export bundles (HMAC / asymmetric) | TAN-247 follow-up (after KMS resolver) |
| Streaming export for very large ledgers | TAN-248 follow-up |

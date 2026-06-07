// Pure helpers for editing per-role sampling parameters inside the control
// panel install config JSON. Kept framework-free so they can be unit-tested
// with `node --test` and reused by the React editor.
//
// Roles mirror the engine/ACA swarm roles. Sampling fields are OPTIONAL: an
// empty input means "unset" (the key is removed from the config), so leaving
// fields blank changes nothing versus today and never forces a value.

export const SWARM_ROLE_KEYS = ["manager", "worker", "reviewer", "tester"];

export const SAMPLING_FIELDS = [
  { key: "temperature", label: "Temperature", placeholder: "unset", step: "0.1", integer: false },
  { key: "top_p", label: "Top P", placeholder: "unset", step: "0.05", integer: false },
  { key: "max_tokens", label: "Max tokens", placeholder: "unset", step: "1", integer: true },
];

const SAMPLING_FIELD_KEYS = SAMPLING_FIELDS.map((field) => field.key);

function isPlainObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

export function parseConfigText(text) {
  let value;
  try {
    value = JSON.parse(String(text ?? ""));
  } catch {
    return { ok: false, error: "Config JSON is invalid; fix it to edit sampling fields." };
  }
  if (!isPlainObject(value)) {
    return { ok: false, error: "Config must be a JSON object." };
  }
  return { ok: true, config: value };
}

// Returns current per-role sampling values as input-ready strings ("" = unset).
export function readRoleSampling(text) {
  const parsed = parseConfigText(text);
  const values = {};
  for (const role of SWARM_ROLE_KEYS) {
    values[role] = {};
    const swarm = parsed.ok && isPlainObject(parsed.config.swarm) ? parsed.config.swarm : {};
    const roleObj = isPlainObject(swarm[role]) ? swarm[role] : {};
    for (const key of SAMPLING_FIELD_KEYS) {
      const raw = roleObj[key];
      values[role][key] = raw === undefined || raw === null ? "" : String(raw);
    }
  }
  return { ok: parsed.ok, error: parsed.error || "", values };
}

// Coerce a raw input string into a stored value. "" (blank) → null = unset.
export function coerceSamplingValue(fieldKey, raw) {
  const trimmed = String(raw ?? "").trim();
  if (trimmed === "") return { ok: true, value: null };
  const num = Number(trimmed);
  if (!Number.isFinite(num)) return { ok: false, error: "Enter a number or leave blank." };
  const field = SAMPLING_FIELDS.find((entry) => entry.key === fieldKey);
  if (field?.integer) {
    if (!Number.isInteger(num) || num < 1) {
      return { ok: false, error: "Enter a whole number ≥ 1 or leave blank." };
    }
  } else if (num < 0) {
    return { ok: false, error: "Enter a value ≥ 0 or leave blank." };
  }
  return { ok: true, value: num };
}

// Apply a single field edit to the config text. Blank removes the key (unset);
// a value sets it. Returns updated, re-serialized text. Range clamping is left
// to the engine, which clamps per provider and drops unsupported params.
export function applyRoleSampling(text, role, fieldKey, raw) {
  if (!SWARM_ROLE_KEYS.includes(role)) {
    return { ok: false, text, error: `Unknown role: ${role}` };
  }
  if (!SAMPLING_FIELD_KEYS.includes(fieldKey)) {
    return { ok: false, text, error: `Unknown field: ${fieldKey}` };
  }
  const parsed = parseConfigText(text);
  if (!parsed.ok) return { ok: false, text, error: parsed.error };
  const coerced = coerceSamplingValue(fieldKey, raw);
  if (!coerced.ok) return { ok: false, text, error: coerced.error };

  const config = parsed.config;
  const swarm = isPlainObject(config.swarm) ? { ...config.swarm } : {};
  const roleObj = isPlainObject(swarm[role]) ? { ...swarm[role] } : {};
  if (coerced.value === null) {
    delete roleObj[fieldKey];
  } else {
    roleObj[fieldKey] = coerced.value;
  }
  swarm[role] = roleObj;
  const nextConfig = { ...config, swarm };
  return { ok: true, text: `${JSON.stringify(nextConfig, null, 2)}`, error: "" };
}

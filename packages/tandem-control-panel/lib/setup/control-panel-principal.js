import { createHash } from "crypto";

function stablePrincipalHash(raw) {
  return createHash("sha256").update(String(raw || "")).digest("hex").slice(0, 24);
}

function resolveControlPanelPrincipalIdentity(session = {}) {
  const explicitPrincipalId = String(
    session?.principal_id ||
      session?.principalId ||
      session?.profile_id ||
      session?.profileId ||
      session?.subject_id ||
      session?.subjectId ||
      session?.user_id ||
      session?.userId ||
      ""
  ).trim();
  if (explicitPrincipalId) {
    return {
      principal_id: explicitPrincipalId,
      principal_source: String(
        session?.principal_source ||
          session?.principalSource ||
          session?.profile_source ||
          session?.profileSource ||
          session?.subject_source ||
          session?.subjectSource ||
          "session"
      ).trim(),
      principal_scope: String(session?.principal_scope || session?.principalScope || "global").trim() || "global",
    };
  }

  const token = String(session?.token || "").trim();
  if (token) {
    return {
      principal_id: `cp_${stablePrincipalHash(`token:${token}`)}`,
      principal_source: "session_token",
      principal_scope: "global",
    };
  }

  const sid = String(session?.sid || "").trim();
  if (sid) {
    return {
      principal_id: `cp_${stablePrincipalHash(`sid:${sid}`)}`,
      principal_source: "session_id",
      principal_scope: "global",
    };
  }

  return {
    principal_id: "",
    principal_source: "unknown",
    principal_scope: "global",
  };
}

export { resolveControlPanelPrincipalIdentity };

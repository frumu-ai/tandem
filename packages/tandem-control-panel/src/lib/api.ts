export class ApiError extends Error {
  status: number;
  code: string;
  retryable: boolean;
  transient: boolean;
  details: Record<string, unknown> | null;

  constructor(
    message: string,
    options: {
      status?: number;
      code?: string;
      retryable?: boolean;
      transient?: boolean;
      details?: Record<string, unknown> | null;
    } = {}
  ) {
    super(message);
    this.name = "ApiError";
    this.status = options.status ?? 0;
    this.code = options.code ?? "API_ERROR";
    this.retryable = options.retryable ?? false;
    this.transient = options.transient ?? false;
    this.details = options.details ?? null;
  }
}

function looksLikeHtmlDocument(text: string) {
  const sample = text.trim().slice(0, 300).toLowerCase();
  return (
    sample.startsWith("<!doctype html") ||
    sample.startsWith("<html") ||
    sample.includes("<body") ||
    sample.includes("<head") ||
    sample.includes("<title>")
  );
}

function isTransientGatewayFailure(status: number, text: string, contentType: string) {
  if ([502, 503, 504, 524].includes(status)) return true;
  const lowered = text.toLowerCase();
  return (
    contentType.includes("text/html") ||
    looksLikeHtmlDocument(text) ||
    lowered.includes("bad gateway") ||
    lowered.includes("temporarily unavailable") ||
    lowered.includes("service unavailable") ||
    lowered.includes("cloudflare")
  );
}

function describeTransientEngineFailure(status: number) {
  if (status === 503)
    return "Engine is restarting or temporarily unavailable. Reconnecting shortly.";
  if (status === 504 || status === 524)
    return "Engine is taking too long to respond while restarting. Reconnecting shortly.";
  return "Engine is temporarily unavailable while restarting. Reconnecting shortly.";
}

export function isTransientEngineError(error: unknown): error is ApiError {
  return error instanceof ApiError && error.transient && error.code === "ENGINE_UNAVAILABLE";
}

export async function api(path: string, init: RequestInit = {}) {
  let res: Response;
  try {
    res = await fetch(path, {
      ...init,
      credentials: "include",
      headers: {
        "content-type": "application/json",
        ...(init.headers || {}),
      },
    });
  } catch (error) {
    throw new ApiError("Engine is unavailable. Reconnecting shortly.", {
      code: "ENGINE_UNAVAILABLE",
      retryable: true,
      transient: true,
    });
  }

  if (!res.ok) {
    const text = await res.text().catch(() => "");
    const contentType = String(res.headers.get("content-type") || "").toLowerCase();
    if (isTransientGatewayFailure(res.status, text, contentType)) {
      throw new ApiError(describeTransientEngineFailure(res.status), {
        status: res.status,
        code: "ENGINE_UNAVAILABLE",
        retryable: true,
        transient: true,
      });
    }
    let message = text || `${path} failed (${res.status})`;
    let details: Record<string, unknown> | null = null;
    try {
      const parsed = text ? JSON.parse(text) : null;
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        details = parsed as Record<string, unknown>;
        if (typeof details.error === "string" && details.error.trim()) {
          message = details.error;
        }
      }
    } catch {
      if (looksLikeHtmlDocument(text)) {
        message = `${path} failed (${res.status})`;
      }
    }
    throw new ApiError(message, {
      status: res.status,
      code:
        typeof details?.code === "string" && details.code.trim()
          ? details.code
          : "API_ERROR",
      retryable: details?.retryable === true,
      transient: false,
      details,
    });
  }

  const txt = await res.text();
  if (!txt) return {};
  try {
    return JSON.parse(txt);
  } catch {
    throw new ApiError("Engine returned an unexpected response. Reconnecting shortly.", {
      status: res.status,
      code: "ENGINE_INVALID_RESPONSE",
      retryable: true,
      transient: true,
    });
  }
}

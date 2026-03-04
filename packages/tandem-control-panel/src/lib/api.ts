export async function api(path: string, init: RequestInit = {}) {
  const res = await fetch(path, {
    ...init,
    credentials: "include",
    headers: {
      "content-type": "application/json",
      ...(init.headers || {}),
    },
  });

  if (!res.ok) {
    const text = await res.text().catch(() => "");
    let message = text || `${path} failed (${res.status})`;
    try {
      const parsed = text ? JSON.parse(text) : null;
      if (parsed?.error) message = parsed.error;
    } catch {
      // ignore non-json body
    }
    throw new Error(message);
  }

  const txt = await res.text();
  return txt ? JSON.parse(txt) : {};
}

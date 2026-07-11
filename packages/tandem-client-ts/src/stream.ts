import type { EngineEvent, KnownEventType } from "./public/index.js";
import { EngineEventSchema } from "./normalize/index.js";

const RUN_TERMINAL_EVENT_TYPES = new Set([
  "run.complete",
  "run.completed",
  "run.failed",
  "run.cancelled",
  "run.canceled",
  "session.run.finished",
  "session.run.completed",
  "session.run.failed",
  "session.run.cancelled",
  "session.run.canceled",
]);

/**
 * Parses a raw SSE data line into an EngineEvent.
 * Uses Zod schemas to normalize canonical IDs and guarantee shape.
 */
function parseSseLine(data: string): EngineEvent | null {
  const trimmed = data.trim();
  if (!trimmed || trimmed === ": keep-alive" || trimmed.startsWith(":")) return null;
  try {
    let parsed = JSON.parse(trimmed);
    let cursor: number | undefined;
    if (
      parsed &&
      typeof parsed === "object" &&
      typeof parsed.cursor === "number" &&
      parsed.event &&
      typeof parsed.event === "object"
    ) {
      cursor = parsed.cursor;
      parsed = parsed.event;
    }
    if (
      parsed &&
      typeof parsed === "object" &&
      typeof parsed.type !== "string" &&
      typeof parsed.event_type === "string"
    ) {
      parsed.type = parsed.event_type;
      parsed.properties =
        parsed.payload && typeof parsed.payload === "object" && !Array.isArray(parsed.payload)
          ? parsed.payload
          : { payload: parsed.payload };
    }
    if (cursor !== undefined) parsed.cursor = cursor;
    const result = EngineEventSchema.safeParse(parsed);
    if (result.success) return result.data;
    return null;
  } catch {
    return null;
  }
}

/**
 * Filter an async stream to only yield events of a specific type.
 *
 * @example
 * ```typescript
 * for await (const event of filterByType(client.stream(s, r), "session.response")) {
 *   console.log(event.properties.delta);
 * }
 * ```
 */
export async function* filterByType<T extends KnownEventType>(
  stream: AsyncIterable<EngineEvent>,
  type: T
): AsyncGenerator<Extract<EngineEvent, { type: T }>> {
  for await (const event of stream) {
    if (event.type === type) {
      yield event as Extract<EngineEvent, { type: T }>;
    }
  }
}

/**
 * Consume a stream in the background and trigger a callback for a specific event type.
 *
 * @example
 * ```typescript
 * on(client.stream(s, r), "run.completed", (event) => console.log(event.runId));
 * ```
 */
export async function on<T extends KnownEventType>(
  stream: AsyncIterable<EngineEvent>,
  type: T,
  callback: (event: Extract<EngineEvent, { type: T }>) => void | Promise<void>
): Promise<void> {
  for await (const event of stream) {
    if (event.type === type) {
      await callback(event as Extract<EngineEvent, { type: T }>);
    }
  }
}

/** Returns true when an event is a terminal run state (success/failure/cancel). */
export function isRunTerminalEvent(event: Pick<EngineEvent, "type"> | string): boolean {
  const t = typeof event === "string" ? event : event.type;
  return RUN_TERMINAL_EVENT_TYPES.has(t);
}

/**
 * Streams Server-Sent Events from a Tandem engine SSE endpoint.
 *
 * Uses Node's built-in fetch + ReadableStream — no browser globals required.
 *
 * @example
 * ```typescript
 * for await (const event of streamSse(url, token)) {
 *   if (event.type === "session.response") {
 *     process.stdout.write(String(event.properties.delta ?? ""));
 *   }
 *   if (event.type === "run.complete") break;
 * }
 * ```
 */
export async function* streamSse(
  url: string,
  token: string,
  options?: {
    /** Signal to abort the stream */
    signal?: AbortSignal;
    /** Max time to wait for the first byte (ms, default 30000) */
    connectTimeoutMs?: number;
  }
): AsyncGenerator<EngineEvent> {
  const connectTimeoutMs = options?.connectTimeoutMs ?? 30_000;
  const controller = new AbortController();
  const connectTimer = setTimeout(() => controller.abort(), connectTimeoutMs);

  const combinedSignal = options?.signal
    ? anySignal([controller.signal, options.signal])
    : controller.signal;

  let res: Response;
  try {
    res = await fetch(url, {
      headers: {
        Accept: "text/event-stream",
        Authorization: `Bearer ${token}`,
        "Cache-Control": "no-cache",
      },
      signal: combinedSignal,
    });
  } finally {
    clearTimeout(connectTimer);
  }

  if (!res.ok) {
    const body = await res.text().catch(() => "");
    throw new Error(`SSE connect failed (${res.status} ${res.statusText}): ${body}`);
  }

  if (!res.body) throw new Error("SSE response has no body");

  const decoder = new TextDecoder();
  const reader = res.body.getReader();
  let buffer = "";

  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;

      buffer += decoder.decode(value, { stream: true });
      const frames = buffer.split(/\r?\n\r?\n/);
      buffer = frames.pop() ?? "";

      for (const frame of frames) {
        const data = frame
          .split(/\r?\n/)
          .filter((line) => line.startsWith("data:"))
          .map((line) => line.slice(5).trimStart())
          .join("\n");
        if (data) {
          const event = parseSseLine(data);
          if (event) yield event;
        }
      }
    }

    buffer += decoder.decode();
    if (buffer) {
      const data = buffer
        .split(/\r?\n/)
        .filter((line) => line.startsWith("data:"))
        .map((line) => line.slice(5).trimStart())
        .join("\n");
      const event = data ? parseSseLine(data) : null;
      if (event) yield event;
    }
  } finally {
    reader.releaseLock();
  }
}

/** Combines multiple AbortSignals — aborts when any one of them aborts. */
function anySignal(signals: AbortSignal[]): AbortSignal {
  const controller = new AbortController();
  for (const signal of signals) {
    if (signal.aborted) {
      controller.abort(signal.reason);
      break;
    }
    signal.addEventListener("abort", () => controller.abort(signal.reason), { once: true });
  }
  return controller.signal;
}

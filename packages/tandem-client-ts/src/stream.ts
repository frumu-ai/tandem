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

export interface SseSequenceGap {
  expected: number;
  received: number;
  previous: number;
  event: EngineEvent;
}

export interface StreamSseOptions {
  /** Signal to abort the stream. Aborting ends iteration without reconnecting. */
  signal?: AbortSignal;
  /** Max time to establish each connection (ms, default 30000). */
  connectTimeoutMs?: number;
  /** Reconnect after a transport error or clean EOF (default false). */
  reconnect?: boolean;
  /** Initial durable cursor, sent as Last-Event-ID. */
  cursor?: string | number;
  /** Initial raw SSE event ID. Takes precedence over cursor. */
  lastEventId?: string;
  /** Maximum reconnect attempts. Defaults to unlimited when reconnect is enabled. */
  maxReconnectAttempts?: number;
  /** Initial reconnect delay (ms, default 250). */
  reconnectDelayMs?: number;
  /** Upper bound for exponential reconnect delay (ms, default 10000). */
  maxReconnectDelayMs?: number;
  /** Called when numeric durable cursors are not contiguous. */
  onSequenceGap?: (gap: SseSequenceGap) => void | Promise<void>;
}

interface SseFrame {
  data: string;
  id?: string;
  event?: string;
  retry?: number;
}

function parseSseFrame(frame: string): SseFrame | null {
  const data: string[] = [];
  let id: string | undefined;
  let event: string | undefined;
  let retry: number | undefined;

  for (const line of frame.split(/\r?\n/)) {
    if (!line || line.startsWith(":")) continue;
    const separator = line.indexOf(":");
    const field = separator === -1 ? line : line.slice(0, separator);
    let value = separator === -1 ? "" : line.slice(separator + 1);
    if (value.startsWith(" ")) value = value.slice(1);
    if (field === "data") data.push(value);
    else if (field === "id" && !value.includes("\0")) id = value;
    else if (field === "event") event = value;
    else if (field === "retry" && /^\d+$/.test(value)) retry = Number(value);
  }

  if (data.length === 0) return null;
  return { data: data.join("\n"), id, event, retry };
}

function parseSseEvent(frame: SseFrame): EngineEvent | null {
  const trimmed = frame.data.trim();
  if (!trimmed) return null;
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
    if (!parsed || typeof parsed !== "object") return null;
    if (cursor === undefined && frame.id !== undefined && /^-?\d+$/.test(frame.id)) {
      cursor = Number(frame.id);
    }
    if (cursor !== undefined) parsed.cursor = cursor;
    if (frame.id !== undefined) parsed.sseId = frame.id;
    if (frame.event !== undefined) parsed.sseEvent = frame.event;
    const result = EngineEventSchema.safeParse(parsed);
    return result.success ? result.data : null;
  } catch {
    return null;
  }
}

function eventSequence(event: EngineEvent): { key: string; value: number } | undefined {
  const raw = event as EngineEvent & { run_id?: unknown; seq?: unknown };
  if (typeof event.goal_seq === "number") return { key: "goal", value: event.goal_seq };
  if (typeof event.envelope?.seq === "number" && event.envelope.run_id) {
    return { key: `run:${event.envelope.run_id}`, value: event.envelope.seq };
  }
  if (typeof raw.seq === "number" && typeof raw.run_id === "string") {
    return { key: `run:${raw.run_id}`, value: raw.seq };
  }
  return undefined;
}

function eventIdentity(event: EngineEvent): string | undefined {
  if (event.sseId !== undefined) return `sse:${event.sseId}`;
  if (event.cursor !== undefined) return `cursor:${event.cursor}`;
  const eventId =
    event.envelope?.event_id ??
    (typeof event.event_id === "string" ? event.event_id : undefined);
  return eventId ? `event:${eventId}` : undefined;
}

/** Filter an async stream to only yield events of a specific type. */
export async function* filterByType<T extends KnownEventType>(
  stream: AsyncIterable<EngineEvent>,
  type: T
): AsyncGenerator<Extract<EngineEvent, { type: T }>> {
  for await (const event of stream) {
    if (event.type === type) yield event as Extract<EngineEvent, { type: T }>;
  }
}

/** Consume a stream and trigger a callback for a specific event type. */
export async function on<T extends KnownEventType>(
  stream: AsyncIterable<EngineEvent>,
  type: T,
  callback: (event: Extract<EngineEvent, { type: T }>) => void | Promise<void>
): Promise<void> {
  for await (const event of stream) {
    if (event.type === type) await callback(event as Extract<EngineEvent, { type: T }>);
  }
}

/** Returns true when an event is a terminal run state (success/failure/cancel). */
export function isRunTerminalEvent(event: Pick<EngineEvent, "type"> | string): boolean {
  return RUN_TERMINAL_EVENT_TYPES.has(typeof event === "string" ? event : event.type);
}

/** Streams and optionally resumes Server-Sent Events from a Tandem endpoint. */
export async function* streamSse(
  url: string,
  token: string,
  options: StreamSseOptions = {}
): AsyncGenerator<EngineEvent> {
  const connectTimeoutMs = options.connectTimeoutMs ?? 30_000;
  const initialDelayMs = Math.max(0, options.reconnectDelayMs ?? 250);
  const maxDelayMs = Math.max(initialDelayMs, options.maxReconnectDelayMs ?? 10_000);
  const maxReconnectAttempts = options.maxReconnectAttempts ?? Number.POSITIVE_INFINITY;
  let reconnectAttempts = 0;
  let lastEventId =
    options.lastEventId ?? (options.cursor === undefined ? undefined : String(options.cursor));
  const previousSequences = new Map<string, number>();
  const seen = new Set<string>();
  const seenOrder: string[] = [];

  while (!options.signal?.aborted) {
    const timeoutController = new AbortController();
    const timer = setTimeout(
      () => timeoutController.abort(new Error(`SSE connect timed out after ${connectTimeoutMs}ms`)),
      connectTimeoutMs
    );
    const signal = options.signal
      ? anySignal([timeoutController.signal, options.signal])
      : timeoutController.signal;
    let response: Response;

    try {
      response = await fetch(url, {
        headers: {
          Accept: "text/event-stream",
          Authorization: `Bearer ${token}`,
          "Cache-Control": "no-cache",
          ...(lastEventId !== undefined ? { "Last-Event-ID": lastEventId } : {}),
        },
        signal,
      });
      clearTimeout(timer);
      if (!response.ok) {
        const body = await response.text().catch(() => "");
        throw new Error(`SSE connect failed (${response.status} ${response.statusText}): ${body}`);
      }
      if (!response.body) throw new Error("SSE response has no body");

      const decoder = new TextDecoder();
      const reader = response.body.getReader();
      let buffer = "";
      try {
        while (!options.signal?.aborted) {
          const { done, value } = await reader.read();
          if (done) break;
          buffer += decoder.decode(value, { stream: true });
          const frames = buffer.split(/\r?\n\r?\n/);
          buffer = frames.pop() ?? "";
          for (const rawFrame of frames) {
            const parsedFrame = parseSseFrame(rawFrame);
            const event = parsedFrame ? parseSseEvent(parsedFrame) : null;
            if (!event) continue;
            if (parsedFrame?.id !== undefined) lastEventId = parsedFrame.id;
            else if (event.cursor !== undefined) lastEventId = String(event.cursor);

            const identity = eventIdentity(event);
            if (identity && seen.has(identity)) continue;
            if (identity) {
              seen.add(identity);
              seenOrder.push(identity);
              if (seenOrder.length > 2_048) seen.delete(seenOrder.shift()!);
            }

            const sequence = eventSequence(event);
            const previousSequence = sequence
              ? previousSequences.get(sequence.key)
              : undefined;
            if (
              sequence &&
              previousSequence !== undefined &&
              sequence.value > previousSequence + 1
            ) {
              await options.onSequenceGap?.({
                expected: previousSequence + 1,
                received: sequence.value,
                previous: previousSequence,
                event,
              });
            }
            if (sequence) {
              previousSequences.set(
                sequence.key,
                Math.max(previousSequence ?? sequence.value, sequence.value)
              );
            }
            reconnectAttempts = 0;
            yield event;
          }
        }
      } finally {
        await reader.cancel().catch(() => undefined);
        reader.releaseLock();
      }
    } catch (error) {
      clearTimeout(timer);
      if (options.signal?.aborted) return;
      if (!options.reconnect || reconnectAttempts >= maxReconnectAttempts) throw error;
    } finally {
      clearTimeout(timer);
    }

    if (options.signal?.aborted || !options.reconnect) return;
    if (reconnectAttempts >= maxReconnectAttempts) return;
    const delayMs = Math.min(maxDelayMs, initialDelayMs * 2 ** reconnectAttempts);
    reconnectAttempts += 1;
    if (!(await abortableDelay(delayMs, options.signal))) return;
  }
}

function abortableDelay(ms: number, signal?: AbortSignal): Promise<boolean> {
  if (signal?.aborted) return Promise.resolve(false);
  return new Promise((resolve) => {
    const timer = setTimeout(() => {
      signal?.removeEventListener("abort", onAbort);
      resolve(true);
    }, ms);
    const onAbort = () => {
      clearTimeout(timer);
      resolve(false);
    };
    signal?.addEventListener("abort", onAbort, { once: true });
  });
}

/** Combines multiple AbortSignals, aborting when any input aborts. */
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

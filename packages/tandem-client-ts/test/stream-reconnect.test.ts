import { afterEach, describe, expect, it, vi } from "vitest";
import { streamSse } from "../src/stream.js";

const originalFetch = globalThis.fetch;
const encoder = new TextEncoder();

afterEach(() => {
  globalThis.fetch = originalFetch;
  vi.restoreAllMocks();
});

function sseResponse(chunks: string[]): Response {
  return new Response(
    new ReadableStream({
      start(controller) {
        for (const chunk of chunks) controller.enqueue(encoder.encode(chunk));
        controller.close();
      },
    }),
    { status: 200, headers: { "Content-Type": "text/event-stream" } }
  );
}

describe("streamSse", () => {
  it("parses split multiline frames and preserves SSE id and event fields", async () => {
    globalThis.fetch = vi.fn(async () =>
      sseResponse([
        "id: cursor-7\r\nevent: goal.progress\r\ndata: {\"type\":\"run.progress\",\r\n",
        "data: \"properties\":{\"step\":2}}\r\n\r\n",
      ])
    ) as typeof fetch;

    const result = await streamSse("http://engine/events", "token").next();

    expect(result.value).toMatchObject({
      type: "run.progress",
      properties: { step: 2 },
      sseId: "cursor-7",
      sseEvent: "goal.progress",
    });
  });

  it("reconnects with Last-Event-ID and yields the numeric SSE cursor", async () => {
    const headers: Array<string | null> = [];
    let call = 0;
    globalThis.fetch = vi.fn(async (_input, init) => {
      headers.push(new Headers(init?.headers).get("Last-Event-ID"));
      call += 1;
      return call === 1
        ? sseResponse(['id: 11\ndata: {"type":"run.progress","properties":{}}\n\n'])
        : sseResponse(['id: 12\ndata: {"type":"run.completed","properties":{}}\n\n']);
    }) as typeof fetch;

    const events = streamSse("http://engine/events", "token", {
      cursor: 10,
      reconnect: true,
      reconnectDelayMs: 0,
    });
    expect((await events.next()).value?.cursor).toBe(11);
    expect((await events.next()).value?.cursor).toBe(12);
    await events.return(undefined);

    expect(headers).toEqual(["10", "11"]);
  });

  it("does not yield duplicate events replayed after reconnect", async () => {
    let call = 0;
    globalThis.fetch = vi.fn(async () => {
      call += 1;
      return call === 1
        ? sseResponse(['id: 20\ndata: {"type":"run.progress","properties":{"n":1}}\n\n'])
        : sseResponse([
            'id: 20\ndata: {"type":"run.progress","properties":{"n":1}}\n\n',
            'id: 21\ndata: {"type":"run.progress","properties":{"n":2}}\n\n',
          ]);
    }) as typeof fetch;

    const events = streamSse("http://engine/events", "token", {
      reconnect: true,
      reconnectDelayMs: 0,
    });
    expect((await events.next()).value?.properties).toEqual({ n: 1 });
    expect((await events.next()).value?.properties).toEqual({ n: 2 });
    await events.return(undefined);
    expect(call).toBe(2);
  });

  it("reports forward sequence gaps", async () => {
    const onSequenceGap = vi.fn();
    globalThis.fetch = vi.fn(async () =>
      sseResponse([
        'id: 41\ndata: {"type":"run.progress","run_id":"run-1","seq":40,"properties":{}}\n\n',
        'id: 43\ndata: {"type":"run.progress","run_id":"run-1","seq":43,"properties":{}}\n\n',
      ])
    ) as typeof fetch;

    const events = streamSse("http://engine/events", "token", {
      onSequenceGap,
    });
    await events.next();
    const event = await events.next();

    expect(event.value?.cursor).toBe(43);
    expect(onSequenceGap).toHaveBeenCalledWith(
      expect.objectContaining({ expected: 41, received: 43, previous: 40 })
    );
  });

  it("does not treat store-wide cursor jumps as missing goal events", async () => {
    const onSequenceGap = vi.fn();
    globalThis.fetch = vi.fn(async () =>
      sseResponse([
        'id: 41\ndata: {"type":"run.progress","run_id":"goal:one","seq":8,"properties":{}}\n\n',
        'id: 99\ndata: {"type":"run.progress","run_id":"goal:one","seq":9,"properties":{}}\n\n',
      ])
    ) as typeof fetch;

    const events = streamSse("http://engine/events", "token", { onSequenceGap });
    await events.next();
    await events.next();

    expect(onSequenceGap).not.toHaveBeenCalled();
  });

  it("aborts during reconnect backoff without opening another connection", async () => {
    const controller = new AbortController();
    let calls = 0;
    globalThis.fetch = vi.fn(async () => {
      calls += 1;
      return sseResponse([]);
    }) as typeof fetch;

    const next = streamSse("http://engine/events", "token", {
      signal: controller.signal,
      reconnect: true,
      reconnectDelayMs: 10_000,
    }).next();
    await new Promise((resolve) => setTimeout(resolve, 0));
    controller.abort();

    await expect(next).resolves.toEqual({ done: true, value: undefined });
    expect(calls).toBe(1);
  });
});

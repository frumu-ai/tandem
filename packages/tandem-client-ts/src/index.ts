/**
 * @frumu/tandem-client
 *
 * TypeScript / Node.js client for the Tandem autonomous agent engine.
 * Full coverage of the Tandem HTTP + SSE API.
 *
 * @example
 * ```typescript
 * import { TandemClient } from "@frumu/tandem-client";
 *
 * const client = new TandemClient({
 *   baseUrl: "http://localhost:39731",
 *   token: "your-engine-token",
 * });
 *
 * const sessionId = await client.sessions.create({ title: "My agent" });
 * const { runId } = await client.sessions.promptAsync(sessionId, "Summarize README.md");
 *
 * for await (const event of client.stream(sessionId, runId)) {
 *   if (event.type === "session.response") {
 *     process.stdout.write(String(event.properties.delta ?? ""));
 *   }
 *   if (
 *     event.type === "run.complete" ||
 *     event.type === "run.completed" ||
 *     event.type === "run.failed" ||
 *     event.type === "session.run.finished"
 *   ) break;
 * }
 * ```
 */

export { TandemClient } from "./client.js";
export { streamSse, filterByType, on, isRunTerminalEvent } from "./stream.js";
export { TandemValidationError } from "./normalize/index.js";
export type * from "./public/index.js";

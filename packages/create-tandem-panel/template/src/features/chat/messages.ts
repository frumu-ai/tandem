export type ChatMessage = {
  id: string;
  role: string;
  displayRole: string;
  text: string;
  markdown: boolean;
};

function textFromParts(parts: any): string {
  if (!Array.isArray(parts)) return "";
  const chunks = parts
    .map((part) => {
      if (!part) return "";
      if (typeof part === "string") return part;
      if (typeof part.text === "string") return part.text;
      if (typeof part.delta === "string") return part.delta;
      if (typeof part.content === "string") return part.content;
      return "";
    })
    .filter(Boolean);
  return chunks.join("\n").trim();
}

function roleOf(row: any): string {
  return String(
    row?.info?.role || row?.role || row?.message_role || row?.type || row?.author || "assistant"
  )
    .trim()
    .toLowerCase();
}

function textOf(row: any): string {
  const fromParts = textFromParts(row?.parts);
  if (fromParts) return fromParts;

  const single = [row?.content, row?.text, row?.message, row?.delta, row?.body]
    .map((x) => (typeof x === "string" ? x : ""))
    .find((x) => x.trim().length > 0);
  if (single) return single.trim();

  const content = row?.content;
  if (Array.isArray(content)) {
    const chunks = content
      .map((chunk: any) => {
        if (!chunk) return "";
        if (typeof chunk === "string") return chunk;
        if (typeof chunk?.text === "string") return chunk.text;
        if (typeof chunk?.content === "string") return chunk.content;
        return "";
      })
      .filter(Boolean);
    if (chunks.length) return chunks.join("\n").trim();
  }

  return "";
}

function displayRole(role: string, assistantName: string): string {
  if (role === "assistant") return assistantName || "Assistant";
  if (role === "user") return "User";
  if (role === "system") return "System";
  return role || "Assistant";
}

export function normalizeMessage(row: any, index: number, assistantName: string): ChatMessage {
  const role = roleOf(row);
  const text = textOf(row);
  const markdown = role === "assistant" || role === "system";
  const id = String(
    row?.id || row?.messageID || row?.message_id || row?.event_id || `${role}-${index}`
  ).trim();

  return {
    id: id || `${role}-${index}`,
    role,
    displayRole: displayRole(role, assistantName),
    text,
    markdown,
  };
}

export function normalizeMessages(input: any, assistantName: string): ChatMessage[] {
  const rows = Array.isArray(input) ? input : Array.isArray(input?.messages) ? input.messages : [];

  return rows.map((row, idx) => normalizeMessage(row, idx, assistantName));
}

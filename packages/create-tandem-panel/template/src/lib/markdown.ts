import DOMPurify from "dompurify";
import { marked } from "marked";

marked.setOptions({
  gfm: true,
  breaks: true,
});

export function renderMarkdownSafe(input: string) {
  const raw = String(input || "");
  const html = marked.parse(raw, { async: false }) as string;
  return DOMPurify.sanitize(html);
}

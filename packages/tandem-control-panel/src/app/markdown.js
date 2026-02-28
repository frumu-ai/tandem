import { escapeHtml } from "./dom.js";

function sanitizeUrl(raw) {
  const value = String(raw || "").trim();
  if (!value) return "";
  if (/^https?:\/\//i.test(value) || /^mailto:/i.test(value)) return value;
  return "";
}

function renderInline(text) {
  let out = escapeHtml(text || "");
  const codeSpans = [];
  out = out.replace(/`([^`]+)`/g, (_, code) => {
    const token = `@@CODE${codeSpans.length}@@`;
    codeSpans.push(`<code>${escapeHtml(code)}</code>`);
    return token;
  });

  out = out.replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, (_, label, url) => {
    const safe = sanitizeUrl(url);
    if (!safe) return `${escapeHtml(label)} (${escapeHtml(url)})`;
    return `<a href="${escapeHtml(safe)}" target="_blank" rel="noreferrer">${escapeHtml(label)}</a>`;
  });
  out = out.replace(/(https?:\/\/[^\s<]+)/g, (url) => {
    const safe = sanitizeUrl(url);
    if (!safe) return escapeHtml(url);
    return `<a href="${escapeHtml(safe)}" target="_blank" rel="noreferrer">${escapeHtml(url)}</a>`;
  });

  out = out.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
  out = out.replace(/__([^_]+)__/g, "<strong>$1</strong>");
  out = out.replace(/\*([^*]+)\*/g, "<em>$1</em>");
  out = out.replace(/_([^_]+)_/g, "<em>$1</em>");
  out = out.replace(/~~([^~]+)~~/g, "<s>$1</s>");

  out = out.replace(/@@CODE(\d+)@@/g, (_, i) => codeSpans[Number(i)] || "");
  return out;
}

export function renderMarkdown(text) {
  const lines = String(text || "").replace(/\r/g, "").split("\n");
  const html = [];
  let i = 0;

  while (i < lines.length) {
    const line = lines[i];

    if (/^\s*```/.test(line)) {
      const code = [];
      i += 1;
      while (i < lines.length && !/^\s*```/.test(lines[i])) {
        code.push(lines[i]);
        i += 1;
      }
      if (i < lines.length) i += 1;
      html.push(`<pre><code>${escapeHtml(code.join("\n"))}</code></pre>`);
      continue;
    }

    const h = line.match(/^(#{1,6})\s+(.+)$/);
    if (h) {
      const level = h[1].length;
      html.push(`<h${level}>${renderInline(h[2])}</h${level}>`);
      i += 1;
      continue;
    }

    if (/^[-*+]\s+/.test(line)) {
      const items = [];
      while (i < lines.length && /^[-*+]\s+/.test(lines[i])) {
        items.push(lines[i].replace(/^[-*+]\s+/, ""));
        i += 1;
      }
      html.push(`<ul>${items.map((x) => `<li>${renderInline(x)}</li>`).join("")}</ul>`);
      continue;
    }

    if (/^\d+\.\s+/.test(line)) {
      const items = [];
      while (i < lines.length && /^\d+\.\s+/.test(lines[i])) {
        items.push(lines[i].replace(/^\d+\.\s+/, ""));
        i += 1;
      }
      html.push(`<ol>${items.map((x) => `<li>${renderInline(x)}</li>`).join("")}</ol>`);
      continue;
    }

    if (/^>\s?/.test(line)) {
      const parts = [];
      while (i < lines.length && /^>\s?/.test(lines[i])) {
        parts.push(lines[i].replace(/^>\s?/, ""));
        i += 1;
      }
      html.push(`<blockquote>${parts.map((x) => renderInline(x)).join("<br/>")}</blockquote>`);
      continue;
    }

    if (!line.trim()) {
      i += 1;
      continue;
    }

    const para = [line];
    i += 1;
    while (i < lines.length && lines[i].trim() && !/^(#{1,6})\s+/.test(lines[i]) && !/^[-*+]\s+/.test(lines[i]) && !/^\d+\.\s+/.test(lines[i]) && !/^>\s?/.test(lines[i]) && !/^\s*```/.test(lines[i])) {
      para.push(lines[i]);
      i += 1;
    }
    html.push(`<p>${para.map((x) => renderInline(x)).join("<br/>")}</p>`);
  }

  return html.join("");
}

import React, { useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeSanitize from "rehype-sanitize";
import { ChevronDown, ChevronRight, CheckCircle2, FileText, Code } from "lucide-react";

interface ToolCallResultProps {
  toolName: string;
  resultString: string;
  className?: string;
  defaultExpanded?: boolean;
}

export const ToolCallResult: React.FC<ToolCallResultProps> = ({
  toolName,
  resultString,
  className = "",
  defaultExpanded = false,
}) => {
  const [expanded, setExpanded] = useState(defaultExpanded);

  // Attempt to parse out some stats if we returned JSON
  let displayMarkdown = resultString;
  let stats: Record<string, unknown> | null = null;

  try {
    const parsed = JSON.parse(resultString);
    if (parsed.markdown) {
      displayMarkdown = parsed.markdown;
      stats = { ...parsed };
      delete stats?.markdown;
    } else if (parsed.content) {
      displayMarkdown = parsed.content;
      stats = { ...parsed };
      delete stats?.content;
    } else if (parsed.summary || parsed.snippet) {
      displayMarkdown = parsed.summary || parsed.snippet;
    }
  } catch {
    // If it's not JSON, just render it as raw markdown
  }

  // Calculate some fun visual stats to show off
  const length = displayMarkdown.length;
  const isLarge = length > 2000;

  return (
    <div
      className={`border border-gray-800 rounded-lg overflow-hidden bg-gray-900/50 ${className}`}
    >
      {/* Header Bar */}
      <button
        onClick={() => setExpanded(!expanded)}
        className="w-full flex items-center justify-between p-3 bg-gray-800/80 hover:bg-gray-700/80 transition-colors text-left"
      >
        <div className="flex items-center gap-2">
          {expanded ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
          <CheckCircle2 size={16} className="text-emerald-500" />
          <span className="font-mono text-sm text-gray-200">{toolName}</span>
          <span className="text-xs text-emerald-500/80 bg-emerald-500/10 px-2 py-0.5 rounded ml-2">
            Success
          </span>
        </div>
        <div className="flex items-center gap-3 text-xs text-gray-500 font-mono">
          {isLarge && (
            <span
              className="text-purple-400/80 flex items-center gap-1"
              title="Engine successfully compressed this output"
            >
              <Code size={12} /> High Token Yield
            </span>
          )}
          <span className="flex items-center gap-1">
            <FileText size={12} />
            {length} chars
          </span>
        </div>
      </button>

      {/* Expanded Content View */}
      {expanded && (
        <div className="p-4 border-t border-gray-800 bg-gray-950 max-h-96 overflow-y-auto">
          {/* Engine Compression Stats Block */}
          {stats && Object.keys(stats).length > 0 && (
            <div className="mb-4 bg-purple-900/20 border border-purple-500/30 rounded p-3 text-xs font-mono text-purple-200">
              <div className="font-semibold text-purple-400 mb-1 uppercase tracking-wider">
                Engine Extracted Metadata
              </div>
              <ul className="space-y-0.5 opacity-80">
                {Object.entries(stats).map(([k, v]) => (
                  <li key={k}>
                    <span className="text-gray-400">{k}:</span>{" "}
                    {typeof v === "object" ? JSON.stringify(v) : String(v)}
                  </li>
                ))}
              </ul>
            </div>
          )}

          {/* Render Markdown */}
          <div className="prose prose-invert prose-sm max-w-none prose-pre:bg-gray-800/50 prose-pre:border prose-pre:border-gray-700 prose-a:text-blue-400 hover:prose-a:text-blue-300">
            <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeSanitize]}>
              {displayMarkdown}
            </ReactMarkdown>
          </div>
        </div>
      )}
    </div>
  );
};

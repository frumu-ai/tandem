/* global HTMLIFrameElement */
import { useRef, useEffect } from "react";

interface HtmlPreviewProps {
  content: string;
  className?: string;
  baseHref?: string;
}

export function HtmlPreview({ content, className = "", baseHref }: HtmlPreviewProps) {
  const iframeRef = useRef<HTMLIFrameElement>(null);

  useEffect(() => {
    // When content changes, we can perform additional actions if needed.
    // The srcDoc attribute on the iframe handles the rendering safely.
  }, [content]);

  // Inject <base> tag if baseHref is provided
  const processedContent = baseHref
    ? content.replace(/<head>/i, `<head>\n<base href="${baseHref}" />`)
    : content;

  // Fallback: if no <head> tag is found but baseHref exists, prepend it.
  // This is a simple heuristic; robust parsing would be better but likely overkill here.
  const finalContent =
    baseHref && !processedContent.includes("<base")
      ? `<base href="${baseHref}" />\n${processedContent}`
      : processedContent;

  return (
    <div className={`h-full w-full bg-white ${className}`}>
      <iframe
        ref={iframeRef}
        srcDoc={finalContent}
        title="HTML Preview"
        className="h-full w-full border-none"
        // Sandbox permissions:
        // allow-scripts: Required for Chart.js / interactive elements
        // allow-same-origin: CAREFUL - needed if the HTML tries to fetch local assets, but we should test if it's strictly required for CDNs.
        // For now, we omit allow-same-origin to be safer unless CDNs break.
        // allow-popups: Needed for links with target="_blank"
        // allow-popups-to-escape-sandbox: Allows popup windows to not be sandboxed (e.g., open in system browser)
        // allow-forms: Probably not needed for reports.
        sandbox="allow-scripts allow-popups allow-popups-to-escape-sandbox allow-forms"
        referrerPolicy="no-referrer"
      />
    </div>
  );
}

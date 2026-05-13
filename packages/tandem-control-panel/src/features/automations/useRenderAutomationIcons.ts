import { useEffect } from "react";
import { renderIcons } from "../../app/icons.js";

export function useRenderAutomationIcons(rootRef: any, deps: any[]) {
  useEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    renderIcons(root);
  }, deps);
}

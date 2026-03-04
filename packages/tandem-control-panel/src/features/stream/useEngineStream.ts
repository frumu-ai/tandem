import { useEffect } from "react";
import { subscribeSse } from "../../services/sse.js";

export function useEngineStream(
  url: string,
  onMessage: (event: MessageEvent<string>) => void,
  options: { enabled?: boolean; withCredentials?: boolean } = {}
) {
  useEffect(() => {
    if (!options.enabled || !url) return;
    return subscribeSse(url, onMessage, {
      withCredentials: options.withCredentials,
    });
  }, [onMessage, options.enabled, options.withCredentials, url]);
}

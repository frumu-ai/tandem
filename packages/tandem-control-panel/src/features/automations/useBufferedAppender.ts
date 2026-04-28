import { useCallback, useEffect, useRef } from "react";

type Setter<T> = (updater: (prev: T[]) => T[]) => void;

type Options<T> = {
  cap: number;
  getId?: (item: T) => string;
};

export function useBufferedAppender<T>(setState: Setter<T>, { cap, getId }: Options<T>) {
  const pendingRef = useRef<T[]>([]);
  const rafRef = useRef<number | null>(null);
  const setStateRef = useRef(setState);
  setStateRef.current = setState;

  const flush = useCallback(() => {
    rafRef.current = null;
    const pending = pendingRef.current;
    if (!pending.length) return;
    pendingRef.current = [];
    setStateRef.current((prev) => {
      const seen = getId ? new Set(prev.map(getId)) : null;
      let merged: T[] | null = null;
      for (const item of pending) {
        if (getId && seen) {
          const id = getId(item);
          if (seen.has(id)) continue;
          seen.add(id);
        }
        if (!merged) merged = prev.slice();
        merged.push(item);
      }
      if (!merged) return prev;
      const overflow = merged.length - cap;
      if (overflow > 0) merged.splice(0, overflow);
      return merged;
    });
  }, [cap, getId]);

  useEffect(
    () => () => {
      if (rafRef.current != null) {
        cancelAnimationFrame(rafRef.current);
        rafRef.current = null;
      }
    },
    []
  );

  return useCallback(
    (item: T) => {
      pendingRef.current.push(item);
      if (rafRef.current == null) {
        rafRef.current = requestAnimationFrame(flush);
      }
    },
    [flush]
  );
}

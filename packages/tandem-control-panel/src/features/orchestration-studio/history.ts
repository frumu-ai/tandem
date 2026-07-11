export type HistoryState<T> = Readonly<{
  past: readonly T[];
  present: T;
  future: readonly T[];
  limit: number;
}>;

export function createHistory<T>(initial: T, limit = 100): HistoryState<T> {
  return { past: [], present: initial, future: [], limit: Math.max(1, Math.floor(limit)) };
}

export function canUndo<T>(history: HistoryState<T>): boolean {
  return history.past.length > 0;
}

export function canRedo<T>(history: HistoryState<T>): boolean {
  return history.future.length > 0;
}

export function pushHistory<T>(
  history: HistoryState<T>,
  next: T,
  equals: (left: T, right: T) => boolean = Object.is
): HistoryState<T> {
  if (equals(history.present, next)) return history;
  return {
    ...history,
    past: [...history.past, history.present].slice(-history.limit),
    present: next,
    future: [],
  };
}

export function replacePresent<T>(history: HistoryState<T>, present: T): HistoryState<T> {
  return Object.is(history.present, present) ? history : { ...history, present };
}

export function undo<T>(history: HistoryState<T>): HistoryState<T> {
  if (!canUndo(history)) return history;
  const present = history.past[history.past.length - 1];
  return {
    ...history,
    past: history.past.slice(0, -1),
    present,
    future: [history.present, ...history.future],
  };
}

export function redo<T>(history: HistoryState<T>): HistoryState<T> {
  if (!canRedo(history)) return history;
  const [present, ...future] = history.future;
  return {
    ...history,
    past: [...history.past, history.present].slice(-history.limit),
    present,
    future,
  };
}

export function clearHistory<T>(history: HistoryState<T>): HistoryState<T> {
  return history.past.length === 0 && history.future.length === 0
    ? history
    : { ...history, past: [], future: [] };
}

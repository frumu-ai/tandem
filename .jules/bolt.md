## 2024-06-25 - Avoid multiple useMemo mapped passes on large streams
**Learning:** In a codebase that streams thousands of logs (like `src/components/logs/LogsDrawer.tsx`), performing three separate `lines.map().filter()` inside separate `useMemo` hooks is incredibly wasteful. It creates 6 intermediate array allocations matching the size of the stream, every time the `lines` state updates, causing huge memory spikes and GC churn.
**Action:** Combine multiple array-wide extractors over the same dataset into a single `useMemo` block with one manual `for` loop, parsing matching criteria into `Set` structures to maintain the exact same functionality while dramatically slashing O(N) allocation and compute overhead.

## 2026-05-03 - Avoid multiple separate array iterations inside useMemo
**Learning:** In a codebase that handles arrays like `toolCalls` in `src/components/chat/Message.tsx`, running multiple `array.filter()` and `array.reduce()` passes across the same dataset creates multiple intermediate array allocations. This causes performance spikes and unnecessary GC pressure, which gets worse as the arrays grow.
**Action:** Combine multiple extractor passes over the same array into a single `useMemo` block using a single `for` loop. Iterate over the data once, tracking states and metrics inside block scope variables, to eliminate intermediate array creation entirely.

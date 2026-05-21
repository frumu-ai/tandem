## 2024-06-25 - Avoid multiple useMemo mapped passes on large streams
**Learning:** In a codebase that streams thousands of logs (like `src/components/logs/LogsDrawer.tsx`), performing three separate `lines.map().filter()` inside separate `useMemo` hooks is incredibly wasteful. It creates 6 intermediate array allocations matching the size of the stream, every time the `lines` state updates, causing huge memory spikes and GC churn.
**Action:** Combine multiple array-wide extractors over the same dataset into a single `useMemo` block with one manual `for` loop, parsing matching criteria into `Set` structures to maintain the exact same functionality while dramatically slashing O(N) allocation and compute overhead.

## 2024-05-24 - [Avoid `[...arr].sort(...)[0]` in React components]
**Learning:** O(N log N) sorting patterns like `[...arr].sort(...)[0]` in `useMemo` or component body incur unnecessary compute and memory allocation overhead. Finding the maximum or minimum element takes O(N) instead.
**Action:** Replace `[...arr].sort(...)[0]` with `maxBy` or `minBy` utilities when possible, avoiding array clones and sorts. Ensure intermediate sorted arrays are not used elsewhere.

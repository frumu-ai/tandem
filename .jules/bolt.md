## 2024-06-25 - Avoid multiple useMemo mapped passes on large streams
**Learning:** In a codebase that streams thousands of logs (like `src/components/logs/LogsDrawer.tsx`), performing three separate `lines.map().filter()` inside separate `useMemo` hooks is incredibly wasteful. It creates 6 intermediate array allocations matching the size of the stream, every time the `lines` state updates, causing huge memory spikes and GC churn.
**Action:** Combine multiple array-wide extractors over the same dataset into a single `useMemo` block with one manual `for` loop, parsing matching criteria into `Set` structures to maintain the exact same functionality while dramatically slashing O(N) allocation and compute overhead.

## 2024-06-25 - Avoid array sort for finding extremes
**Learning:** Using `[...arr].sort(...)[0]` to find the maximum or minimum element in an array is an O(N log N) operation that also requires O(N) allocation for array cloning. This can become a performance bottleneck when executed frequently, especially within React `useMemo` hooks operating on arrays like artifact streams.
**Action:** Use the O(N) `maxBy` and `minBy` utilities exported from `src/lib/utils.ts` instead. They find the extreme values in a single pass without intermediate allocations or the overhead of sorting the entire array.

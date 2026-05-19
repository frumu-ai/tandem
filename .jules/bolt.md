## 2024-06-25 - Avoid multiple useMemo mapped passes on large streams
**Learning:** In a codebase that streams thousands of logs (like `src/components/logs/LogsDrawer.tsx`), performing three separate `lines.map().filter()` inside separate `useMemo` hooks is incredibly wasteful. It creates 6 intermediate array allocations matching the size of the stream, every time the `lines` state updates, causing huge memory spikes and GC churn.
**Action:** Combine multiple array-wide extractors over the same dataset into a single `useMemo` block with one manual `for` loop, parsing matching criteria into `Set` structures to maintain the exact same functionality while dramatically slashing O(N) allocation and compute overhead.

## 2024-10-24 - O(N log N) max element searches with .sort()[0]
**Learning:** Finding a maximum or minimum element in an array using `[...arr].sort((a, b) => b.ts_ms - a.ts_ms)[0]` allocates a new array O(N) and sorts it O(N log N) just to extract the single maximum value, generating huge GC pressure and blocking the main thread when dealing with thousands of artifacts or log streams.
**Action:** Introduced O(N) `maxBy` and `minBy` utilities into `src/lib/utils.ts` and replaced all sorting-based maximum lookups in large collections (especially in `DeveloperRunViewer.tsx`) to iterate once without intermediate allocations.

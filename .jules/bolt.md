## 2024-06-25 - Avoid multiple useMemo mapped passes on large streams
**Learning:** In a codebase that streams thousands of logs (like `src/components/logs/LogsDrawer.tsx`), performing three separate `lines.map().filter()` inside separate `useMemo` hooks is incredibly wasteful. It creates 6 intermediate array allocations matching the size of the stream, every time the `lines` state updates, causing huge memory spikes and GC churn.
**Action:** Combine multiple array-wide extractors over the same dataset into a single `useMemo` block with one manual `for` loop, parsing matching criteria into `Set` structures to maintain the exact same functionality while dramatically slashing O(N) allocation and compute overhead.

## 2023-10-27 - [Sorting Array Replaced with MaxBy for Performance]
**Learning:** Found an anti-pattern specific to sorting an array in JS to extract the minimum or maximum values like `[...arr].sort(...)[0]`. The time complexity of `Array.prototype.sort()` is `O(n log n)`. This operation uses extra memory when making a shallow copy of the array and also costs `O(n)` time for array allocation.
**Action:** Replaced `.sort(...)[0]` with a `maxBy` custom utility inside `src/lib/utils.ts` and used it instead for finding maximum element by properties over the code base.

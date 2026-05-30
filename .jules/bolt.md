
## 2024-05-24 - Array allocation overhead
**Learning:** `Math.max(...array.map())` creates intermediate array allocations that create garbage collection pressure, and on large arrays the spread operator can throw a 'Maximum call stack size exceeded' error.
**Action:** Replace `Math.max(...array.map())` with single-pass `array.reduce((max, item) => Math.max(max, item))` to prevent intermediate arrays and call stack limits.

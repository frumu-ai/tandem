import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

/**
 * O(N) utility to find the maximum element in an array based on a projection function.
 * This avoids the O(N log N) overhead and memory allocations of [...arr].sort(...)[0]
 */
export function maxBy<T>(array: readonly T[], fn: (item: T) => number): T | undefined {
  if (array.length === 0) return undefined;
  let max = array[0];
  let maxVal = fn(max);
  for (let i = 1; i < array.length; i++) {
    const val = fn(array[i]);
    if (val > maxVal) {
      maxVal = val;
      max = array[i];
    }
  }
  return max;
}

/**
 * O(N) utility to find the minimum element in an array based on a projection function.
 * This avoids the O(N log N) overhead and memory allocations of [...arr].sort(...)[0]
 */
export function minBy<T>(array: readonly T[], fn: (item: T) => number): T | undefined {
  if (array.length === 0) return undefined;
  let min = array[0];
  let minVal = fn(min);
  for (let i = 1; i < array.length; i++) {
    const val = fn(array[i]);
    if (val < minVal) {
      minVal = val;
      min = array[i];
    }
  }
  return min;
}

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/**
 * Finds the last element in an array that satisfies the provided testing function.
 * This is a performance optimization over `[...arr].reverse().find(predicate)`
 * avoiding the O(n) memory allocation and O(n) reverse operation when targeting
 * environments without native `Array.prototype.findLast`.
 */
export function findLast<T>(
  array: T[],
  predicate: (value: T, index: number, obj: T[]) => boolean
): T | undefined {
  for (let i = array.length - 1; i >= 0; i--) {
    if (predicate(array[i], i, array)) {
      return array[i];
    }
  }
  return undefined;
}

/**
 * Returns up to `count` elements from the end of the array, optionally filtered by `predicate`,
 * in reversed order (newest first). This avoids the O(n) overhead of `[...arr].filter().slice(-count).reverse()`.
 */
export function takeLastReversed<T>(
  array: T[],
  count: number,
  predicate?: (value: T, index: number, obj: T[]) => boolean
): T[] {
  const result: T[] = [];
  for (let i = array.length - 1; i >= 0 && result.length < count; i--) {
    if (!predicate || predicate(array[i], i, array)) {
      result.push(array[i]);
    }
  }
  return result;
}

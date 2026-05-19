import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

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

/**
 * Finds the element in an array that has the maximum value according to the provided extractor function.
 * This avoids the O(n log n) overhead of sorting the array just to find the maximum element.
 */
export function maxBy<T>(array: T[], extract: (item: T) => number): T | null {
  if (array.length === 0) return null;
  let maxItem = array[0];
  let maxValue = extract(maxItem);
  for (let i = 1; i < array.length; i++) {
    const val = extract(array[i]);
    if (val > maxValue) {
      maxValue = val;
      maxItem = array[i];
    }
  }
  return maxItem;
}

/**
 * Finds the element in an array that has the minimum value according to the provided extractor function.
 * This avoids the O(n log n) overhead of sorting the array just to find the minimum element.
 */
export function minBy<T>(array: T[], extract: (item: T) => number): T | null {
  if (array.length === 0) return null;
  let minItem = array[0];
  let minValue = extract(minItem);
  for (let i = 1; i < array.length; i++) {
    const val = extract(array[i]);
    if (val < minValue) {
      minValue = val;
      minItem = array[i];
    }
  }
  return minItem;
}

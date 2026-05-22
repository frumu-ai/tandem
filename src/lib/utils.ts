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
/**
 * Returns the maximum element in an array based on a numeric scoring function.
 * This is a performance optimization over `[...arr].sort((a,b) => score(b) - score(a))[0]`,
 * reducing time complexity from O(n log n) to O(n) and avoiding array duplication.
 */
export function maxBy<T>(array: readonly T[], scoreFn: (item: T) => number): T | undefined {
  if (array.length === 0) return undefined;
  let maxElement = array[0];
  let maxScore = scoreFn(maxElement as T);
  for (let i = 1; i < array.length; i++) {
    const currentScore = scoreFn(array[i] as T);
    if (currentScore > maxScore) {
      maxScore = currentScore;
      maxElement = array[i];
    }
  }
  return maxElement;
}

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

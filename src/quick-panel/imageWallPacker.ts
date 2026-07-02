import { clampImageCardAspectRatio } from './constants'

/**
 * Return the row-height weight for a tile, so the greedy packer can balance
 * columns. Height is normalized against a width of 1: taller tiles (portrait,
 * aspect ratio < 1) contribute more weight than wide ones.
 */
export function getImageCardHeightWeight(aspectRatio: number | undefined): number {
  return 1 / clampImageCardAspectRatio(aspectRatio)
}

export interface ImageWallColumnEntry<T> {
  item: T
  /** Position of `item` within the original `items` array passed to {@link packImageWallColumns}. */
  index: number
}

/**
 * Greedily assign each item to whichever column currently has the smallest
 * accumulated height, so columns stay roughly balanced regardless of each
 * tile's aspect ratio. O(items * columnCount) — cheap for quick-panel's
 * bounded page size, no need for a heap.
 *
 * @param items Items in their original order.
 * @param columnCount Number of columns to pack into.
 * @param getAspectRatio Cached aspect ratio lookup for an item, or undefined
 *   if not yet measured (treated as 1:1 — see {@link clampImageCardAspectRatio}).
 */
export function packImageWallColumns<T>(
  items: readonly T[],
  columnCount: number,
  getAspectRatio: (item: T) => number | undefined
): ImageWallColumnEntry<T>[][] {
  const columns: ImageWallColumnEntry<T>[][] = Array.from({ length: columnCount }, () => [])
  const columnHeights = columns.map(() => 0)
  items.forEach((item, index) => {
    const targetColumnIndex = columnHeights.indexOf(Math.min(...columnHeights))
    columns[targetColumnIndex].push({ item, index })
    columnHeights[targetColumnIndex] += getImageCardHeightWeight(getAspectRatio(item))
  })
  return columns
}

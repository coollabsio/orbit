import type { KlipyImage } from '@orbit/api/generated/types.gen'

/**
 * Puts each item into the column that is shortest so far, as a masonry grid does. The heights count in widths, so the
 * columns need not be measured, and the first items stay where they are when another page is added.
 */
export function masonryColumns<T extends { preview: KlipyImage }>(items: readonly T[], count: number): T[][] {
  const columns = Array.from({ length: count }, (): T[] => [])
  const heights = Array.from({ length: count }, () => 0)
  for (const item of items) {
    let shortest = 0
    for (let column = 1; column < count; column++) if (heights[column] < heights[shortest]) shortest = column
    columns[shortest].push(item)
    heights[shortest] += item.preview.width > 0 ? item.preview.height / item.preview.width : 1
  }
  return columns
}

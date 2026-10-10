import { describe, expect, it } from 'vitest'
import { getImageCardHeightWeight, packImageWallColumns } from '@/quick-panel/imageWallPacker'

describe('getImageCardHeightWeight', () => {
  it('is the inverse of the clamped aspect ratio', () => {
    expect(getImageCardHeightWeight(2)).toBeCloseTo(0.5)
  })

  it('defaults to a square (weight 1) when the aspect ratio is unmeasured', () => {
    expect(getImageCardHeightWeight(undefined)).toBeCloseTo(1)
  })
})

describe('packImageWallColumns', () => {
  it('distributes items round-robin when all weights are equal', () => {
    const items = ['a', 'b', 'c', 'd', 'e', 'f']
    const columns = packImageWallColumns(items, 3, () => 1)
    expect(columns.map(column => column.map(entry => entry.item))).toEqual([
      ['a', 'd'],
      ['b', 'e'],
      ['c', 'f'],
    ])
  })

  it('keeps the original array index alongside each item', () => {
    const items = ['a', 'b', 'c']
    const columns = packImageWallColumns(items, 3, () => 1)
    expect(columns.flat()).toEqual([
      { item: 'a', index: 0 },
      { item: 'b', index: 1 },
      { item: 'c', index: 2 },
    ])
  })

  it('routes a much taller item to its own column before shorter ones stack up', () => {
    // 'tall' clamps to the min aspect ratio (0.45) -> weight ~2.22, roughly
    // double a square tile's weight of 1.
    const items = ['tall', 'sq1', 'sq2']
    const getAspectRatio = (item: string) => (item === 'tall' ? 0.45 : 1)
    const columns = packImageWallColumns(items, 3, getAspectRatio)
    expect(columns[0].map(entry => entry.item)).toEqual(['tall'])
    expect(columns[1].map(entry => entry.item)).toEqual(['sq1'])
    expect(columns[2].map(entry => entry.item)).toEqual(['sq2'])
  })

  it('returns columnCount empty arrays for an empty item list', () => {
    expect(packImageWallColumns([], 3, () => 1)).toEqual([[], [], []])
  })
})

/** Empty regions use logical result rows, never date headers or cached-window rows. */
export function windowedGeometry(
  offset: number,
  length: number,
  total: number,
  rowHeight: number,
  columns = 1
) {
  columns = Math.max(1, Math.floor(columns))
  offset = Math.max(0, Math.floor(offset))
  length = Math.max(0, Math.floor(length))
  total = Math.max(0, Math.floor(total), offset + length)
  rowHeight = Math.max(0, rowHeight)
  const topRows = Math.floor(offset / columns)
  const endRow = Math.ceil((offset + length) / columns)
  const bottomRows = Math.max(0, Math.ceil(total / columns) - endRow)
  return {
    top: topRows * rowHeight,
    bottom: bottomRows * rowHeight,
    topRows,
    bottomRows,
    leadingColumns: offset % columns,
  }
}

export function absoluteVisibleRange(
  scrollTop: number,
  height: number,
  rowHeight: number,
  total: number,
  columns = 1
) {
  columns = Math.max(1, Math.floor(columns))
  const last = Math.max(0, total - 1)
  const pitch = Math.max(1, rowHeight)
  return {
    start: Math.min(last, Math.max(0, Math.floor(scrollTop / pitch) * columns)),
    end: Math.min(
      last,
      Math.max(0, Math.ceil((Math.max(0, scrollTop) + Math.max(0, height)) / pitch) * columns - 1)
    ),
  }
}

import assert from 'node:assert/strict'

// The current history contract retains at most 50 records and navigates overlapping windows.
export async function assertBoundedGrid({ page, rows, list, api }) {
  const expected = []
  let total
  do {
    const response = await api(`/search/query?query=&offset=${expected.length}&limit=50`)
    assert.equal(response.status, 200, 'read the authoritative history order')
    const data = response.body.data
    total ??= data.total
    assert.equal(data.total, total, 'history remains stable during navigation')
    assert.ok(data.items.length > 0, 'every authoritative page makes progress')
    expected.push(...data.items.map(item => item.entryId))
  } while (expected.length < total)
  assert.ok(total > 100, 'the fixture spans multiple old 100-record pages')
  assert.equal(expected.length, total)
  assert.equal(new Set(expected).size, total, 'authoritative records are unique')
  await page.getByRole('radio', { name: 'Grid' }).click()
  await page.waitForFunction(() => document.querySelectorAll('[data-item-id]').length === 50)
  const grid = list()
  const geometry = await grid.evaluate(root => {
    const scroll = root.querySelector('[data-history-window-scroller]') ?? root
    return { height: scroll.clientHeight, content: scroll.scrollHeight }
  })
  assert.ok(geometry.content <= geometry.height + 1, 'all 50 resident cards fit without scrolling')
  const seen = new Set()
  const windows = []
  const record = async offset => {
    const ids = await rows().evaluateAll(cards =>
      cards.map(card => card.getAttribute('data-item-id'))
    )
    assert.equal(ids.length, 50, 'each full resident window contains exactly 50 records')
    assert.deepEqual(ids, expected.slice(offset, offset + 50), 'rendered order matches the daemon')
    for (const id of ids) seen.add(id)
    windows.push({ offset, count: ids.length })
  }
  await record(0)
  let offset = 0
  for (const direction of [1, -1]) {
    while (direction === 1 ? offset < total - 50 : offset > 0) {
      const next = Math.max(0, Math.min(total - 50, offset + direction * 10))
      const ids = expected.slice(next, next + 50)
      await grid.focus()
      await page.keyboard.press(direction === 1 ? 'PageDown' : 'PageUp')
      await page.waitForFunction(
        expectedIds =>
          JSON.stringify(
            [...document.querySelectorAll('[data-item-id]')].map(card =>
              card.getAttribute('data-item-id')
            )
          ) === JSON.stringify(expectedIds),
        ids
      )
      await record(next)
      offset = next
    }
  }
  assert.equal(
    seen.size,
    total,
    'navigation reaches every record without retaining the full history'
  )
  assert.equal(offset, 0, 'reverse navigation returns to the newest window')
  return { total, visited: seen.size, capacity: 50, windows, geometry }
}

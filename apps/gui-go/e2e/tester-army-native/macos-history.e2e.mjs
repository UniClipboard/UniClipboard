import { expect, test } from 'e2e'
import { nativeHistoryTexts } from './macos-engine.mjs'

test('macOS native history: select a real entry and display its detail', async ({
  screen,
  app,
}) => {
  const list = screen.getByRole('radio', 'List')
  await expect(list).toBeVisible()
  await list.tap()
  await expect(list).toBeChecked()

  const entry = screen.getByText(nativeHistoryTexts[0], { exact: true })
  await expect(entry).toHaveCount(1)
  await entry.tap()
  await expect(entry).toHaveCount(2)
  await expect(entry.first()).toBeSelected()
  await app.screenshot('history-selected')
})

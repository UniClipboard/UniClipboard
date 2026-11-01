import { expect, test } from 'e2e'

// Failure modes: absent file bars, merged counters, no intermediate native value,
// bytes displayed as success before save, or one file hiding the remaining files.
// The fixture also requires three matching hashes, Engine completion and owned cleanup.
test('macOS native file receive: three files keep independent progress and saved results', async ({
  screen,
  app,
}) => {
  const names = [1, 2, 3].map(index => `native-transfer-${index}.bin`)
  const bars = names.map(name => screen.getByRole('progressbar', `Receiving ${name}`))
  for (const bar of bars) await expect(bar).toBeVisible()
  await expect(bars[2]).toHaveValue(/^(?:[1-9]|[1-9][0-9])(?:\.\d+)?$/)
  await app.screenshot('receive-in-progress')
  for (const bar of bars) await expect(bar).toHaveValue('100')
  await expect(screen.getByText('3 of 3 saved', { exact: true })).toBeVisible()
  await expect(screen.getByText('Saved · 100%', { exact: true })).toHaveCount(3)
  await app.screenshot('receive-completed')
})

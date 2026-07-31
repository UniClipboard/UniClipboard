// Card: e2e/cards/quick-panel-delivery-badge.md
//
// The embedded driver now covers real multi-window discovery and switching.
// Full delivery-badge assertions still need a fixture that seeds entries with
// and without delivery records.

import { browser, expect } from '@wdio/globals'

describe('quick-panel delivery badge card (MVP)', () => {
  it('可以访问主窗口和快速面板窗口', async () => {
    const windows = await browser.tauri.listWindows()

    expect(windows).toContain('main')
    expect(windows).toContain('quick-panel')

    await browser.tauri.switchWindow('quick-panel')
    try {
      expect(await browser.tauri.execute('document.readyState')).toBe('complete')
    } finally {
      await browser.tauri.switchWindow('main')
    }
  })

  it.skip('TODO 完整卡片断言 (依赖 delivery fixture)', async () => {
    // Implement card steps 1-5 after the delivery fixture is available.
    //
    // Selectors from the card frontmatter:
    //   - [data-testid="quick-panel-titlebar"] [data-delivery-summary]
    //   - [data-delivery-popover]   (Radix portal, no ancestor constraint)
    //   - [data-testid="quick-panel-preview-area"]
    //   - [data-testid="clipboard-detail"] [data-delivery-summary]
    //
    // Assertions:
    //   - the titlebar summary is one of synced/syncing/partial/failed/pending
    //   - hovering reveals a document-level popover containing the device name
    //   - h_e1 == h_e2, so the badge does not change the preview height
    //   - the main-window detail summary matches the quick-panel summary
  })
})

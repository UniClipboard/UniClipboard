// 对应卡片: e2e/cards/quick-panel-delivery-badge.md
//
// MVP 阶段占位 spec —— 完整断言依赖:
//   1. 一个 fixture 工具能预置两条 entry (E1 有 delivery 记录、E2 无)
//   2. 一个机制能从主窗口测试上下文切到 quick-panel webview window
//
// 在这两件事到位前, spec 只做最低门槛验证: setup 流程能走通到主界面,
// 主窗口的 [data-testid="clipboard-detail"] 锚点在 entry 选中后出现.
// 这至少证明真机链路 + DOM 契约 (clipboard-detail / delivery-summary)
// 已经在代码中兑现.

import { waitForSetup } from '../helpers/waitForSetup.js'

describe('quick-panel delivery badge card (MVP)', () => {
  it('setup 流程能走通进入主界面', async () => {
    await waitForSetup()
  })

  it.skip('TODO 完整卡片断言 (依赖 fixture + quick-panel window 切换)', async () => {
    // 卡片步骤 1-5 的完整实现, 等 fixture 工具 + window switching helper 就位
    //
    // 关键 selector (来自卡片 frontmatter):
    //   - [data-testid="quick-panel-titlebar"] [data-delivery-summary]
    //   - [data-delivery-popover]   (Radix portal, 不限祖先)
    //   - [data-testid="quick-panel-preview-area"]
    //   - [data-testid="clipboard-detail"] [data-delivery-summary]
    //
    // 断言要点:
    //   - quick-panel titlebar 的 summary 状态 ∈ {synced,syncing,partial,failed,pending}
    //   - hover 后 popover 在 document 出现, 含 device-name
    //   - h_e1 == h_e2 (badge 不挤占 preview 高度)
    //   - 主窗口 detail summary 与 quick-panel 一致
  })
})

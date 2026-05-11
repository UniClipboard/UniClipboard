/* eslint-disable import-x/order --
 * vi.mock 必须在 import 被测组件之前 (hoist 语义), 把 vitest mock 调用夹
 * 在两个 import 块之间, 与 import-x/order 的"组间无空行"天然冲突。
 * vitest 测试惯例,本文件白名单。
 */

/**
 * # 为什么需要这个测试
 *
 * `handleOpenAddDialog` 与 `handleEnableSuccess` 是 phase 6 一键 onboarding
 * 的关键串联点 —— 未配置点 +Add 时弹引导对话框, 引导对话框成功后 *自动*
 * 打开 Add 表单。这两条线如果回归了(典型:有人误删 `setAddDialogOpen(true)`
 * 或把 `setEnableConfirmOpen(true)` 改回 `toast.error`),用户的"一步流程"
 * 就坏回旧的"四步流程",但 helper 单测和 facade 测试都抓不到。
 *
 * 本测试 mock 掉所有子 dialog / Sheet 的真实渲染,只验证 Panel 的状态联动
 * 契约 —— ROI 高,跑得快,不引入 Radix portal / Tauri mock 的复杂度。
 *
 * 后端语义(写盘 + bind_error 传播)由 uc-application phase 3 facade 测试
 * 用 RecordingLanLifecycle / FailingLanLifecycle 钉死, 本测试不重复覆盖。
 */

import '@testing-library/jest-dom/vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest'
// 关键:vi.mock 必须在 import 被测组件之前。把所有"会触发副作用 / Radix
// portal / Tauri command"的子组件统统换成最小渲染存根, 只暴露 testid 与
// 触发 onSuccess 的按钮 —— 我们只关心 Panel 自身的状态联动。
vi.mock('@/components/device/AddMobileSyncDeviceDialog', () => ({
  default: ({ open }: { open: boolean }) =>
    open ? <div data-testid="stub-add-dialog">add-dialog-open</div> : null,
}))

vi.mock('@/components/device/EnableMobileSyncDialog', () => ({
  default: ({
    open,
    onSuccess,
  }: {
    open: boolean
    onSuccess: () => void
    onOpenChange: (v: boolean) => void
  }) =>
    open ? (
      <div data-testid="stub-enable-dialog">
        enable-dialog-open
        <button type="button" data-testid="stub-enable-success" onClick={() => onSuccess()}>
          fire-success
        </button>
      </div>
    ) : null,
}))

// Sheet 不弹起来时仍会 mount → 内部 useEffect 会调真实 Tauri API。Stub 成
// 空组件,settings 在 Panel 内保持 null = "未配置" 状态(我们要测的入口)。
vi.mock('@/components/device/MobileSyncSettingsSheet', () => ({
  default: () => null,
}))

// 不参与本测试,但 Panel 引入了。
vi.mock('@/components/device/MobileSyncCredentialModal', () => ({
  default: () => null,
}))
vi.mock('@/components/device/RotatedPasswordModal', () => ({
  default: () => null,
}))
vi.mock('@/components/device/RotateMobilePasswordDialog', () => ({
  default: () => null,
}))

// listMobileDevices 是 Panel 初次挂载就 await 的, 不 mock 会 reject 触发
// 错误 banner 把焦点抢走。
vi.mock('@/api/tauri-command/mobile_sync', async importOriginal => {
  const actual = await importOriginal<typeof import('@/api/tauri-command/mobile_sync')>()
  return {
    ...actual,
    listMobileDevices: vi.fn().mockResolvedValue([]),
    revokeMobileDevice: vi.fn().mockResolvedValue(undefined),
  }
})

// 必须放在所有 vi.mock 之后
import MobileSyncDevicesPanel from '@/components/device/MobileSyncDevicesPanel'
import i18n from '@/i18n'

describe('MobileSyncDevicesPanel — 一键 onboarding 状态联动', () => {
  let initialLanguage = 'en-US'

  beforeAll(async () => {
    if (!i18n.isInitialized) {
      await new Promise<void>(resolve => {
        const h = () => {
          i18n.off('initialized', h)
          resolve()
        }
        i18n.on('initialized', h)
      })
    }
    initialLanguage = i18n.language
    await i18n.changeLanguage('en-US')
  })

  afterAll(async () => {
    await i18n.changeLanguage(initialLanguage)
  })

  it('未配置时点 +Add → 弹引导 dialog (不直接开 Add)', async () => {
    render(<MobileSyncDevicesPanel />)

    // settings 保持 null(MobileSyncSettingsSheet 被 stub 掉了不会回写),
    // !settings?.enabled = true → 走引导路径
    const addBtn = await screen.findByRole('button', { name: /Add/i })
    fireEvent.click(addBtn)

    expect(screen.getByTestId('stub-enable-dialog')).toBeInTheDocument()
    expect(screen.queryByTestId('stub-add-dialog')).not.toBeInTheDocument()
  })

  it('引导 dialog onSuccess → 自动打开 Add dialog', async () => {
    render(<MobileSyncDevicesPanel />)

    fireEvent.click(await screen.findByRole('button', { name: /Add/i }))
    expect(screen.getByTestId('stub-enable-dialog')).toBeInTheDocument()

    // 模拟用户在引导 dialog 内确认成功
    fireEvent.click(screen.getByTestId('stub-enable-success'))

    // handleEnableSuccess 必须把 addDialogOpen 拨到 true,Add dialog 立刻
    // 接管 —— 这是"零跳转"UX 的灵魂,挂了的话用户要再点一次 +Add。
    expect(screen.getByTestId('stub-add-dialog')).toBeInTheDocument()
  })
})

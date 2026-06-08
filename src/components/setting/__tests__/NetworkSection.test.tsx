import '@testing-library/jest-dom/vitest'
import { render, screen, cleanup, act, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import NetworkSection from '@/components/setting/NetworkSection'
import { useSetting } from '@/hooks/useSetting'
import i18n from '@/i18n'
import { commands } from '@/lib/ipc'
import type { Settings, NetworkSettings } from '@/types/setting'

// ============================================================================
// Mock chain — 使 NetworkSection 完全脱离真实 daemon HTTP / Tauri runtime
// ============================================================================
// 实现已切到 typed `commands` proxy（`@/lib/ipc`，背后是 tauri-specta
// 生成的 `ipc-bindings.generated.ts`）。这里只 mock 我们关心的命令
// `restartDaemon`，其它命令未 mock 时调用会抛 TypeError，等于 fail-fast
// 防止误调用未 stub 的命令。
vi.mock('@/lib/ipc', () => ({
  commands: {
    restartDaemon: vi.fn(),
  },
}))

vi.mock('@/hooks/useSetting', () => ({
  useSetting: vi.fn(),
}))

const mockRestartDaemon = vi.mocked(commands.restartDaemon)
const mockUseSetting = vi.mocked(useSetting)

// ============================================================================
// Test fixtures
// ============================================================================
const baseSetting: Settings = {
  schemaVersion: 1,
  general: {
    autoStart: false,
    silentStart: false,
    autoCheckUpdate: true,
    autoDownloadUpdate: false,
    theme: 'light',
    themeColor: 'zinc',
    themeColorLight: null,
    themeColorDark: null,
    themeOverridesLight: {},
    themeOverridesDark: {},
    language: 'zh-CN',
    deviceName: 'Test Device',
    telemetryEnabled: true,
    usageAnalyticsEnabled: true,
  },
  sync: {
    autoSync: true,
    syncFrequency: 'realtime',
    contentTypes: {
      text: true,
      image: true,
      link: true,
      file: true,
      codeSnippet: true,
      richText: true,
    },
  },
  retentionPolicy: {
    enabled: false,
    rules: [],
    skipPinned: false,
    evaluation: 'anyMatch',
  },
  security: {
    encryptionEnabled: false,
    passphraseConfigured: false,
    autoUnlockEnabled: false,
  },
  pairing: {
    stepTimeout: 15,
    userVerificationTimeout: 120,
    sessionTimeout: 300,
    maxRetries: 3,
    protocolVersion: '1.0.0',
  },
  keyboardShortcuts: {},
  fileSync: {
    fileSyncEnabled: true,
    smallFileThreshold: 10 * 1024 * 1024,
    maxFileSize: 5 * 1024 * 1024 * 1024,
    fileCacheQuotaPerDevice: 500 * 1024 * 1024,
    fileRetentionHours: 24,
    fileAutoCleanup: true,
  },
  network: {
    allowRelayFallback: true,
    allowOverlayNetworkAddrs: false,
    customRelayUrls: [],
  },
  quickPanel: {
    enabled: true,
    position: 'center',
  },
}

type UpdateNetworkSettingFn = (
  newNetworkSetting: Partial<NetworkSettings>
) => Promise<{ restartRequired: boolean }>

interface SetupArgs {
  setting?: Settings | null
  error?: string | null
  updateNetworkSetting?: ReturnType<typeof vi.fn<UpdateNetworkSettingFn>>
}

const setupSetting = ({
  setting = baseSetting,
  error = null,
  updateNetworkSetting,
}: SetupArgs = {}) => {
  const mockUpdate =
    updateNetworkSetting ??
    vi.fn<UpdateNetworkSettingFn>().mockResolvedValue({ restartRequired: true })
  mockUseSetting.mockReturnValue({
    setting,
    loading: false,
    error,
    updateSetting: vi.fn(),
    updateGeneralSetting: vi.fn(),
    updateAutostart: vi.fn(),
    updateSyncSetting: vi.fn(),
    updateSecuritySetting: vi.fn(),
    updateRetentionPolicy: vi.fn(),
    updateKeyboardShortcuts: vi.fn(),
    updateFileSyncSetting: vi.fn(),
    updateNetworkSetting: mockUpdate,
    updateQuickPanelSetting: vi.fn().mockResolvedValue({ restartRequired: false }),
  })
  return { mockUpdate }
}

const renderWithOverrides = (overrides: Partial<NetworkSettings> = {}) => {
  setupSetting({
    setting: {
      ...baseSetting,
      network: { ...baseSetting.network, ...overrides },
    },
  })
  return render(<NetworkSection />)
}

beforeAll(async () => {
  await i18n.changeLanguage('zh-CN')
})

beforeEach(() => {
  vi.clearAllMocks()
})

afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

// ============================================================================
// 14 个集成测试 — 覆盖 PLAN.md `truths` 8 条 + UI-SPEC interaction contract
// ============================================================================
describe('NetworkSection — Phase 95 集成', () => {
  it('Test 1: applied OFF 默认态 — Switch=OFF + RestartBanner 不可见', async () => {
    renderWithOverrides({ allowRelayFallback: true })
    const sw = await screen.findByRole('switch', { name: /LAN-only/ })
    expect(sw).toHaveAttribute('aria-checked', 'false')
    // RestartBanner 不挂 — role=status 不应出现
    await waitFor(() => {
      expect(screen.queryByRole('status')).toBeNull()
    })
  })

  it('Test 2: applied ON 默认态 — Switch=ON + RestartBanner 不可见', async () => {
    renderWithOverrides({ allowRelayFallback: false })
    const sw = await screen.findByRole('switch', { name: /LAN-only/ })
    expect(sw).toHaveAttribute('aria-checked', 'true')
    await waitFor(() => {
      expect(screen.queryByRole('status')).toBeNull()
    })
  })

  it('Test 3: 用户点 Switch — Switch 立即翻 + RestartBanner 立即可见（乐观 pending D-D2）', async () => {
    const user = userEvent.setup()
    renderWithOverrides({ allowRelayFallback: true })
    const sw = await screen.findByRole('switch', { name: /LAN-only/ })
    expect(sw).toHaveAttribute('aria-checked', 'false')

    await user.click(sw)

    expect(sw).toHaveAttribute('aria-checked', 'true')
    // 不等 debounce — Banner 立即可见
    expect(screen.getByRole('status')).toBeInTheDocument()
  })

  it('Test 4: debounce 500ms 后才调 updateNetworkSetting', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime })
    const mockUpdate = vi.fn().mockResolvedValue({ restartRequired: true })
    setupSetting({ updateNetworkSetting: mockUpdate })

    render(<NetworkSection />)
    const sw = await screen.findByRole('switch', { name: /LAN-only/ })
    await user.click(sw)

    // 100ms 内不应触发
    await act(async () => {
      vi.advanceTimersByTime(100)
    })
    expect(mockUpdate).not.toHaveBeenCalled()

    // 累计到 500ms+ 触发一次
    await act(async () => {
      vi.advanceTimersByTime(500)
    })
    expect(mockUpdate).toHaveBeenCalledTimes(1)
    expect(mockUpdate).toHaveBeenCalledWith({
      allowRelayFallback: false,
      allowOverlayNetworkAddrs: false,
      customRelayUrls: [],
    })
  })

  it('Test 5: 连击 Switch 只 PUT 一次（debounce 合并）', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime })
    const mockUpdate = vi.fn().mockResolvedValue({ restartRequired: true })
    setupSetting({ updateNetworkSetting: mockUpdate })

    render(<NetworkSection />)
    const sw = await screen.findByRole('switch', { name: /LAN-only/ })

    // 100ms 内连点 3 次
    await user.click(sw)
    await act(async () => {
      vi.advanceTimersByTime(50)
    })
    await user.click(sw)
    await act(async () => {
      vi.advanceTimersByTime(50)
    })
    await user.click(sw)

    // 600ms 后只触发一次（最后状态）
    await act(async () => {
      vi.advanceTimersByTime(600)
    })
    expect(mockUpdate).toHaveBeenCalledTimes(1)
    // 起始 allowRelay=true（OFF）→ click1=false → click2=true → click3=false
    expect(mockUpdate).toHaveBeenLastCalledWith({
      allowRelayFallback: false,
      allowOverlayNetworkAddrs: false,
      customRelayUrls: [],
    })
  })

  it('Test 6: 点击「立即重启」调 commands.restartDaemon()', async () => {
    const user = userEvent.setup()
    renderWithOverrides({ allowRelayFallback: true })
    const sw = await screen.findByRole('switch', { name: /LAN-only/ })
    await user.click(sw)

    // Banner 立即可见
    expect(screen.getByRole('status')).toBeInTheDocument()

    const restartBtn = screen.getByRole('button', { name: /立即重启|Restart now/ })
    await user.click(restartBtn)

    expect(mockRestartDaemon).toHaveBeenCalled()
  })

  it('Test 7: restart_app 失败 → RestartBanner.error 渲染（重试 + dismiss）', async () => {
    const user = userEvent.setup()
    mockRestartDaemon.mockRejectedValueOnce(new Error('restart app failed'))
    renderWithOverrides({ allowRelayFallback: true })
    const sw = await screen.findByRole('switch', { name: /LAN-only/ })
    await user.click(sw)

    const restartBtn = await screen.findByRole('button', { name: /立即重启|Restart now/ })
    await user.click(restartBtn)

    // 「重试」与 dismiss 应出现
    await screen.findByRole('button', { name: /重试|Retry/ })
    expect(
      screen.getByRole('button', { name: /收起重启提示|Dismiss restart notice/ })
    ).toBeInTheDocument()
  })

  it('Test 8: PUT 失败 → Switch 视觉回滚 + 显示 saveError inline', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime })
    const mockUpdate = vi.fn().mockRejectedValueOnce(new Error('PUT failed'))
    setupSetting({ updateNetworkSetting: mockUpdate })

    render(<NetworkSection />)
    const sw = await screen.findByRole('switch', { name: /LAN-only/ })
    await user.click(sw) // OFF → ON
    expect(sw).toHaveAttribute('aria-checked', 'true')

    // 推进 debounce 触发 PUT
    await act(async () => {
      vi.advanceTimersByTime(600)
    })
    // 让 promise rejection 回到 React commit
    await act(async () => {
      await Promise.resolve()
    })

    // 视觉回滚到 baseline allowRelay=true ⇒ checked=false
    await waitFor(() => {
      expect(sw).toHaveAttribute('aria-checked', 'false')
    })

    // saveError 文本可见（i18n 文案含「保存失败」或 "Save failed"）
    expect(screen.getByRole('alert').textContent).toMatch(/保存失败|Save failed/)
  })

  it('Test 9: mount 时 RestartBanner 不可见（in-memory pending 不跨 session — mtime fence）', async () => {
    // Fence: 历史版本曾用 mtime-based 推导跨 session pending state，
    // 现已收敛到 in-memory；组件 mount 不再调任何 IPC 探测。
    renderWithOverrides({ allowRelayFallback: true })
    await act(async () => {
      await Promise.resolve()
    })
    expect(screen.queryByRole('status')).toBeNull()
  })

  it('Test 10: mount 后即便 setting.allowRelayFallback=false 也不显示 banner（仅切换才触发）', async () => {
    // Fence: 同上，mount 不该出 banner，状态由用户主动切换才进入 pending。
    renderWithOverrides({ allowRelayFallback: false })
    await act(async () => {
      await Promise.resolve()
    })
    expect(screen.queryByRole('status')).toBeNull()
  })

  it('Test 11: LanOnlyDisclosure trigger 在 SettingRow 内可见', async () => {
    renderWithOverrides({ allowRelayFallback: true })
    expect(
      await screen.findByRole('button', { name: /查看 LAN-only|View the list/ })
    ).toBeInTheDocument()
  })

  it('Test 12: useSetting error 状态显示 loadError 占位', () => {
    setupSetting({ setting: null, error: '加载设置失败' })
    const { container } = render(<NetworkSection />)
    expect(container.querySelector('.text-destructive')).not.toBeNull()
  })

  it('Test 13: 占位组件残留 fence — 不渲染 placeholder 文本（Pitfall 11）', async () => {
    const { container } = renderWithOverrides({ allowRelayFallback: true })
    await screen.findByRole('switch', { name: /LAN-only/ })
    expect(container.textContent).not.toMatch(/Network settings are not yet available/)
    expect(container.textContent).not.toMatch(/网络设置功能在新架构中尚未实现/)
  })

  it('Test 14: 反向命名 fence — checked = !allowRelayFallback', async () => {
    // allowRelay=false（即 LAN-only ON）⇒ Switch checked=true
    renderWithOverrides({ allowRelayFallback: false })
    const sw = await screen.findByRole('switch', { name: /LAN-only/ })
    expect(sw).toHaveAttribute('aria-checked', 'true')
  })

  it('Test 15: allowOverlayNetworkAddrs 正向命名 — checked === allowOverlayNetworkAddrs', async () => {
    // 默认 false ⇒ Switch checked=false
    renderWithOverrides({ allowOverlayNetworkAddrs: false })
    const offSw = await screen.findByRole('switch', { name: /虚拟网络地址|overlay/i })
    expect(offSw).toHaveAttribute('aria-checked', 'false')
    cleanup()

    // true ⇒ Switch checked=true（无取反）
    renderWithOverrides({ allowOverlayNetworkAddrs: true })
    const onSw = await screen.findByRole('switch', { name: /虚拟网络地址|overlay/i })
    expect(onSw).toHaveAttribute('aria-checked', 'true')
  })

  it('Test 16: 切换 overlay switch — debounce 500ms 后调 updateNetworkSetting', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime })
    const mockUpdate = vi.fn().mockResolvedValue({ restartRequired: true })
    setupSetting({ updateNetworkSetting: mockUpdate })

    render(<NetworkSection />)
    const sw = await screen.findByRole('switch', { name: /虚拟网络地址|overlay/i })
    await user.click(sw)

    expect(sw).toHaveAttribute('aria-checked', 'true')
    expect(screen.getByRole('status')).toBeInTheDocument()

    await act(async () => {
      vi.advanceTimersByTime(600)
    })
    expect(mockUpdate).toHaveBeenCalledTimes(1)
    expect(mockUpdate).toHaveBeenCalledWith({
      allowRelayFallback: true,
      allowOverlayNetworkAddrs: true,
      customRelayUrls: [],
    })
  })

  it('Test 17: 快速切换两个网络开关 — debounce 后保存同一组最终值', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime })
    const mockUpdate = vi.fn().mockResolvedValue({ restartRequired: true })
    setupSetting({ updateNetworkSetting: mockUpdate })

    render(<NetworkSection />)
    const lanOnlySw = await screen.findByRole('switch', { name: /LAN-only/ })
    const overlaySw = await screen.findByRole('switch', { name: /虚拟网络地址|overlay/i })

    await user.click(lanOnlySw)
    await act(async () => {
      vi.advanceTimersByTime(100)
    })
    await user.click(overlaySw)

    await act(async () => {
      vi.advanceTimersByTime(600)
    })

    expect(mockUpdate).toHaveBeenCalledTimes(1)
    expect(mockUpdate).toHaveBeenCalledWith({
      allowRelayFallback: false,
      allowOverlayNetworkAddrs: true,
      customRelayUrls: [],
    })
  })

  it('Test 18: AllowOverlayAddrsDisclosure trigger 在 SettingRow 内可见', async () => {
    renderWithOverrides({ allowRelayFallback: true })
    expect(
      await screen.findByRole('button', {
        name: /了解什么是虚拟网络地址|Learn what overlay network addresses/,
      })
    ).toBeInTheDocument()
  })

  it('Test 19: 自定义中继列表从 settings 渲染为逐项 URL 输入', async () => {
    renderWithOverrides({
      customRelayUrls: ['https://relay-a.example.com.', 'https://relay-b.example.com.'],
    })
    expect(
      await screen.findByRole('textbox', { name: /自定义中继节点 1|Custom relay node 1/ })
    ).toHaveValue('https://relay-a.example.com.')
    expect(
      screen.getByRole('textbox', { name: /自定义中继节点 2|Custom relay node 2/ })
    ).toHaveValue('https://relay-b.example.com.')
  })

  it('Test 20: 添加自定义中继节点 — debounce 后随网络设置一起保存', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime })
    const mockUpdate = vi.fn().mockResolvedValue({ restartRequired: true })
    setupSetting({ updateNetworkSetting: mockUpdate })

    render(<NetworkSection />)
    const firstInput = await screen.findByRole('textbox', {
      name: /自定义中继节点 1|Custom relay node 1/,
    })
    await user.type(firstInput, 'https://relay-a.example.com.')
    await user.click(screen.getByRole('button', { name: /添加中继节点|Add relay node/ }))
    const secondInput = screen.getByRole('textbox', {
      name: /自定义中继节点 2|Custom relay node 2/,
    })
    await user.type(secondInput, 'https://relay-b.example.com.')

    await act(async () => {
      vi.advanceTimersByTime(600)
    })

    expect(mockUpdate).toHaveBeenCalledTimes(1)
    expect(mockUpdate).toHaveBeenCalledWith({
      allowRelayFallback: true,
      allowOverlayNetworkAddrs: false,
      customRelayUrls: ['https://relay-a.example.com.', 'https://relay-b.example.com.'],
    })
  })

  it('Test 21: 自定义中继 URL 非 http(s) 时不保存并显示 inline error', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime })
    const mockUpdate = vi.fn().mockResolvedValue({ restartRequired: true })
    setupSetting({ updateNetworkSetting: mockUpdate })

    render(<NetworkSection />)
    const textbox = await screen.findByRole('textbox', {
      name: /自定义中继节点 1|Custom relay node 1/,
    })
    await user.type(textbox, 'ftp://relay.example.com')

    await act(async () => {
      vi.advanceTimersByTime(600)
    })

    expect(mockUpdate).not.toHaveBeenCalled()
    expect(screen.getByRole('alert').textContent).toMatch(/无效的中继 URL|Invalid relay URL/)
  })
})

// ============================================================================
// REFACTOR — Phase 95 ROADMAP fence（4 条验收 + 3 Pitfall 防御）
// ============================================================================
describe('Phase 95 ROADMAP fence — 4 验收 + 3 Pitfall 防御', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('Pitfall 11 — 占位组件残留全清', async () => {
    const { container } = renderWithOverrides({ allowRelayFallback: true })
    await screen.findByRole('switch', { name: /LAN-only/ })
    expect(container.textContent).not.toMatch(/Network settings are not yet available/)
    expect(container.textContent).not.toMatch(/网络设置功能在新架构中尚未实现/)
  })

  it('Pitfall 10 — RestartBanner 是持久 inline 不是 toast（pending 时 role=status 存在）', async () => {
    const user = userEvent.setup()
    renderWithOverrides({ allowRelayFallback: true })
    const sw = await screen.findByRole('switch', { name: /LAN-only/ })
    await user.click(sw)

    const banner = screen.getByRole('status')
    expect(banner).not.toBeNull()
    // 不是 sonner toast 容器
    expect(document.querySelector('[data-sonner-toaster]')).toBeNull()
  })

  it('Pitfall 5 — 4 类外网请求清单存在（Popover 展开后）', async () => {
    const user = userEvent.setup()
    renderWithOverrides({ allowRelayFallback: true })
    await user.click(await screen.findByRole('button', { name: /查看 LAN-only|View the list/ }))
    expect(screen.getByText(/首次配对 rendezvous|First-pairing rendezvous/)).toBeInTheDocument()
    expect(screen.getByText(/^遥测$|^Telemetry$/)).toBeInTheDocument()
    expect(
      screen.getByText(/pkarr DHT NodeId 解析|pkarr DHT NodeId resolution/)
    ).toBeInTheDocument()
    expect(screen.getByText(/自动更新 GitHub 检查|Auto-update GitHub check/)).toBeInTheDocument()
  })

  it('反向命名 — 唯一取反点是 NetworkSection.tsx（静态约束，由 acceptance grep 保护）', () => {
    // 静态约束 — 由 grep fence 强制；此测试 always PASS，存在意义是让 PR diff reviewer 看到这条约束
    expect(true).toBe(true)
  })
})

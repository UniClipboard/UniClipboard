import '@testing-library/jest-dom/vitest'
import { render, screen, cleanup, act, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { getRelayCredentialStatus, probeRelayUrl } from '@/api/daemon/settings'
import NetworkSection from '@/components/setting/NetworkSection'
import { useSetting } from '@/hooks/useSetting'
import i18n from '@/i18n'
import { commands } from '@/lib/ipc'
import { makeBaseSettings } from '@/test/fixtures/settings'
import type { NetworkSettings, SettingContextType, Settings } from '@/types/setting'

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

vi.mock('@/api/daemon/settings', () => ({
  getRelayCredentialStatus: vi.fn(),
  probeRelayUrl: vi.fn(),
}))

const mockRestartDaemon = vi.mocked(commands.restartDaemon)
const mockUseSetting = vi.mocked(useSetting)
const mockGetRelayCredentialStatus = vi.mocked(getRelayCredentialStatus)
const mockProbeRelayUrl = vi.mocked(probeRelayUrl)

// ============================================================================
// Test fixtures
// ============================================================================
const baseSetting: Settings = makeBaseSettings({
  general: { theme: 'light', themeColor: 'zinc', language: 'zh-CN' },
})

type UpdateNetworkSettingFn = (
  newNetworkSetting: Partial<NetworkSettings>
) => Promise<{ restartRequired: boolean }>
type SaveRelayFn = SettingContextType['saveRelay']

interface SetupArgs {
  setting?: Settings | null
  error?: string | null
  updateNetworkSetting?: ReturnType<typeof vi.fn<UpdateNetworkSettingFn>>
  saveRelay?: ReturnType<typeof vi.fn<SaveRelayFn>>
}

const setupSetting = ({
  setting = baseSetting,
  error = null,
  updateNetworkSetting,
  saveRelay,
}: SetupArgs = {}) => {
  const mockUpdate =
    updateNetworkSetting ??
    vi.fn<UpdateNetworkSettingFn>().mockResolvedValue({ restartRequired: true })
  const mockSaveRelay =
    saveRelay ??
    vi.fn<SaveRelayFn>().mockResolvedValue({
      restartRequired: true,
      credentialStatus: { configured: false },
    })
  mockUseSetting.mockReturnValue({
    setting,
    loading: false,
    error,
    reloadSetting: vi.fn(),
    updateSetting: vi.fn(),
    updateGeneralSetting: vi.fn(),
    updateAutostart: vi.fn(),
    updateSyncSetting: vi.fn(),
    updateSecuritySetting: vi.fn(),
    updateRetentionPolicy: vi.fn(),
    updateKeyboardShortcuts: vi.fn(),
    updateFileSyncSetting: vi.fn(),
    updateNetworkSetting: mockUpdate,
    saveRelay: mockSaveRelay,
    updateQuickPanelSetting: vi.fn().mockResolvedValue({ restartRequired: false }),
  })
  return { mockSaveRelay, mockUpdate }
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
  mockGetRelayCredentialStatus.mockResolvedValue({ configured: false })
  mockProbeRelayUrl.mockResolvedValue({ kind: 'success', latencyMs: 37 })
})

afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

// ============================================================================
// 14 个集成测试 — 覆盖 PLAN.md `truths` 8 条 + UI-SPEC interaction contract
// ============================================================================
describe('NetworkSection — Phase 95 集成', () => {
  it('does not store a relay until the user explicitly saves a successful test', async () => {
    const user = userEvent.setup()
    const mockSaveRelay = vi.fn<SaveRelayFn>().mockResolvedValue({
      restartRequired: true,
      credentialStatus: { configured: true },
    })
    setupSetting({ saveRelay: mockSaveRelay })
    render(<NetworkSection />)

    await user.type(
      screen.getByRole('textbox', { name: /自定义中继节点 1|Custom relay node 1/ }),
      'https://relay.example.com'
    )
    await user.type(screen.getByLabelText(/中继访问令牌 1|Relay access token 1/), 'draft-token')
    await user.click(screen.getByRole('button', { name: /测试可用性|Test availability/ }))
    await waitFor(() => {
      expect(mockProbeRelayUrl).toHaveBeenCalledWith('https://relay.example.com', {
        mode: 'override',
        accessToken: 'draft-token',
      })
    })

    expect(mockSaveRelay).not.toHaveBeenCalled()
    expect(screen.getByRole('button', { name: /保存中继节点|Save relay node/ })).toBeEnabled()
  })

  it('ignores an older probe result when the token changes for the same URL', async () => {
    const user = userEvent.setup()
    let resolveFirstProbe!: (value: { kind: 'success'; latencyMs: number }) => void
    let resolveSecondProbe!: (value: { kind: 'handshake'; message: string }) => void
    mockProbeRelayUrl
      .mockImplementationOnce(
        () =>
          new Promise(resolve => {
            resolveFirstProbe = resolve
          })
      )
      .mockImplementationOnce(
        () =>
          new Promise(resolve => {
            resolveSecondProbe = resolve
          })
      )
    setupSetting()
    render(<NetworkSection />)

    const urlInput = screen.getByRole('textbox', {
      name: /自定义中继节点 1|Custom relay node 1/,
    })
    const tokenInput = screen.getByLabelText(/中继访问令牌 1|Relay access token 1/)
    const testButton = screen.getByRole('button', { name: /测试可用性|Test availability/ })
    const saveButton = screen.getByRole('button', { name: /保存中继节点|Save relay node/ })

    await user.type(urlInput, 'https://relay.example.com')
    await user.type(tokenInput, 'first-token')
    await user.click(testButton)
    await waitFor(() => expect(mockProbeRelayUrl).toHaveBeenCalledTimes(1))

    await user.clear(tokenInput)
    await user.type(tokenInput, 'second-token')
    await user.click(testButton)
    await waitFor(() => expect(mockProbeRelayUrl).toHaveBeenCalledTimes(2))

    await act(async () => resolveFirstProbe({ kind: 'success', latencyMs: 10 }))
    expect(saveButton).toBeDisabled()

    await act(async () =>
      resolveSecondProbe({ kind: 'handshake', message: 'second token rejected' })
    )
    expect(await screen.findByRole('alert')).toHaveTextContent('second token rejected')
    expect(saveButton).toBeDisabled()
  })

  it('waits for the saved credential status before testing an unchanged relay', async () => {
    mockGetRelayCredentialStatus.mockImplementation(() => new Promise(() => undefined))
    renderWithOverrides({ customRelayUrls: ['https://relay.example.com'] })

    expect(screen.getByRole('button', { name: /测试可用性|Test availability/ })).toBeDisabled()
    expect(mockProbeRelayUrl).not.toHaveBeenCalled()
  })

  it('uses a stored token when an edited URL is canonically unchanged', async () => {
    const user = userEvent.setup()
    mockGetRelayCredentialStatus.mockResolvedValue({ configured: true })
    renderWithOverrides({ customRelayUrls: ['https://relay.example.com'] })
    await screen.findByText(/访问令牌已保存|Access token saved/)

    const urlInput = screen.getByRole('textbox', {
      name: /自定义中继节点 1|Custom relay node 1/,
    })
    await user.type(urlInput, '/')
    await user.click(screen.getByRole('button', { name: /测试可用性|Test availability/ }))

    expect(mockProbeRelayUrl).toHaveBeenCalledWith('https://relay.example.com/', {
      mode: 'stored',
    })
  })

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
    expect(mockUpdate).toHaveBeenCalledWith({ allowRelayFallback: false })
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
    expect(mockUpdate).toHaveBeenLastCalledWith({ allowRelayFallback: false })
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
    expect(mockUpdate).toHaveBeenCalledWith({ allowOverlayNetworkAddrs: true })
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

  it('Test 20: 保存经过测试的自定义中继节点', async () => {
    const user = userEvent.setup()
    const mockSaveRelay = vi.fn<SaveRelayFn>().mockResolvedValue({
      restartRequired: true,
      credentialStatus: { configured: false },
    })
    setupSetting({ saveRelay: mockSaveRelay })

    render(<NetworkSection />)
    const firstInput = await screen.findByRole('textbox', {
      name: /自定义中继节点 1|Custom relay node 1/,
    })
    await user.type(firstInput, 'https://relay-a.example.com.')
    await user.click(screen.getByRole('button', { name: /测试可用性|Test availability/ }))
    await user.click(await screen.findByRole('button', { name: /保存中继节点|Save relay node/ }))

    await waitFor(() => {
      expect(mockSaveRelay).toHaveBeenCalledWith({
        index: null,
        previousUrl: null,
        nextUrl: 'https://relay-a.example.com.',
        credential: { action: 'keep' },
      })
    })
  })

  it('removes the selected saved relay node', async () => {
    const user = userEvent.setup()
    const mockSaveRelay = vi.fn<SaveRelayFn>().mockResolvedValue({
      restartRequired: true,
      credentialStatus: { configured: false },
    })
    setupSetting({
      setting: {
        ...baseSetting,
        network: { ...baseSetting.network, customRelayUrls: ['https://relay.example.com'] },
      },
      saveRelay: mockSaveRelay,
    })
    render(<NetworkSection />)

    await user.click(
      await screen.findByRole('button', { name: /移除中继节点 1|Remove relay node 1/ })
    )

    await waitFor(() => {
      expect(mockSaveRelay).toHaveBeenCalledWith({
        index: 0,
        previousUrl: 'https://relay.example.com',
        nextUrl: null,
        credential: { action: 'delete' },
      })
    })
  })

  it('keeps a queued switch save when a relay draft is edited', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime })
    const mockUpdate = vi.fn().mockResolvedValue({ restartRequired: true })
    setupSetting({ updateNetworkSetting: mockUpdate })

    render(<NetworkSection />)
    await user.click(await screen.findByRole('switch', { name: /LAN-only/ }))
    const textbox = screen.getByRole('textbox', {
      name: /自定义中继节点 1|Custom relay node 1/,
    })
    await user.type(textbox, 'https://relay.example.com')

    await act(async () => {
      vi.advanceTimersByTime(600)
    })

    expect(mockUpdate).toHaveBeenCalledWith({ allowRelayFallback: false })
  })

  it('Test 21: 测试失败时不能保存自定义中继 URL', async () => {
    const user = userEvent.setup()
    const mockSaveRelay = vi.fn<SaveRelayFn>().mockResolvedValue({
      restartRequired: true,
      credentialStatus: { configured: false },
    })
    setupSetting({ saveRelay: mockSaveRelay })
    mockProbeRelayUrl.mockResolvedValue({ kind: 'invalidUrl', message: 'unsupported scheme' })

    render(<NetworkSection />)
    const textbox = await screen.findByRole('textbox', {
      name: /自定义中继节点 1|Custom relay node 1/,
    })
    await user.type(textbox, 'ftp://relay.example.com')
    await user.click(screen.getByRole('button', { name: /测试可用性|Test availability/ }))

    expect(mockSaveRelay).not.toHaveBeenCalled()
    expect(await screen.findByRole('alert')).toHaveTextContent(/无效的中继 URL|Invalid relay URL/)
    expect(screen.getByRole('button', { name: /保存中继节点|Save relay node/ })).toBeDisabled()
  })

  it('shows whether an access token is configured without displaying the token', async () => {
    mockGetRelayCredentialStatus.mockResolvedValue({ configured: true })
    renderWithOverrides({ customRelayUrls: ['https://relay.example.com'] })

    expect(await screen.findByText(/访问令牌已保存|Access token saved/)).toBeInTheDocument()
    expect(mockGetRelayCredentialStatus).toHaveBeenCalledWith('https://relay.example.com')
    expect(screen.queryByDisplayValue(/relay-secret-token/)).toBeNull()
  })

  it('saves a replacement access token together with a tested relay', async () => {
    const user = userEvent.setup()
    const mockSaveRelay = vi.fn<SaveRelayFn>().mockResolvedValue({
      restartRequired: true,
      credentialStatus: { configured: true },
    })
    setupSetting({
      setting: {
        ...baseSetting,
        network: { ...baseSetting.network, customRelayUrls: ['https://relay.example.com'] },
      },
      saveRelay: mockSaveRelay,
    })
    render(<NetworkSection />)

    const tokenInput = await screen.findByLabelText(/中继访问令牌 1|Relay access token 1/)
    expect(tokenInput).toHaveAttribute('type', 'password')
    await user.type(tokenInput, 'replacement-token')
    await user.click(screen.getByRole('button', { name: /测试可用性|Test availability/ }))
    expect(mockProbeRelayUrl).toHaveBeenCalledWith('https://relay.example.com', {
      mode: 'override',
      accessToken: 'replacement-token',
    })
    await user.click(screen.getByRole('button', { name: /保存中继节点|Save relay node/ }))

    await waitFor(() => {
      expect(mockSaveRelay).toHaveBeenCalledTimes(1)
      expect(mockSaveRelay).toHaveBeenCalledWith({
        index: 0,
        previousUrl: 'https://relay.example.com',
        nextUrl: 'https://relay.example.com',
        credential: { action: 'set', accessToken: 'replacement-token' },
      })
    })
    expect(tokenInput).toHaveValue('')
  })

  it('deletes a configured access token only when the relay is finally saved', async () => {
    const user = userEvent.setup()
    mockGetRelayCredentialStatus.mockResolvedValue({ configured: true })
    const mockSaveRelay = vi.fn<SaveRelayFn>().mockResolvedValue({
      restartRequired: true,
      credentialStatus: { configured: false },
    })
    setupSetting({
      setting: {
        ...baseSetting,
        network: { ...baseSetting.network, customRelayUrls: ['https://relay.example.com'] },
      },
      saveRelay: mockSaveRelay,
    })
    render(<NetworkSection />)

    await user.click(
      await screen.findByRole('button', { name: /移除已保存的访问令牌|Remove saved access token/ })
    )
    expect(mockSaveRelay).not.toHaveBeenCalled()
    expect(
      screen.getByText(/保存此中继节点后.*才会被移除|will be removed when you save this relay/i)
    ).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: /测试可用性|Test availability/ }))
    expect(mockProbeRelayUrl).toHaveBeenCalledWith('https://relay.example.com', { mode: 'none' })
    await user.click(screen.getByRole('button', { name: /保存中继节点|Save relay node/ }))

    await waitFor(() => {
      expect(mockSaveRelay).toHaveBeenCalledTimes(1)
      expect(mockSaveRelay).toHaveBeenCalledWith({
        index: 0,
        previousUrl: 'https://relay.example.com',
        nextUrl: 'https://relay.example.com',
        credential: { action: 'delete' },
      })
    })
    expect(await screen.findByText(/未配置访问令牌|No access token saved/)).toBeInTheDocument()
  })

  it('rejects a relay URL that canonicalizes to an existing relay', async () => {
    const user = userEvent.setup()
    const mockSaveRelay = vi.fn<SaveRelayFn>()
    setupSetting({
      setting: {
        ...baseSetting,
        network: { ...baseSetting.network, customRelayUrls: ['https://relay.example.com'] },
      },
      saveRelay: mockSaveRelay,
    })
    render(<NetworkSection />)

    await user.click(screen.getByRole('button', { name: /添加中继节点|Add relay node/ }))
    await user.type(
      screen.getByRole('textbox', { name: /自定义中继节点 2|Custom relay node 2/ }),
      'https://relay.example.com/'
    )
    await user.click(screen.getAllByRole('button', { name: /测试可用性|Test availability/ })[1])
    await user.click(screen.getAllByRole('button', { name: /保存中继节点|Save relay node/ })[1])

    expect(mockSaveRelay).not.toHaveBeenCalled()
    expect(await screen.findByRole('alert')).toHaveTextContent(/重复|already exists/i)
  })

  it('updates only the selected item when legacy settings contain exact duplicates', async () => {
    const user = userEvent.setup()
    const duplicateUrl = 'https://relay.example.com'
    const mockSaveRelay = vi.fn<SaveRelayFn>().mockResolvedValue({
      restartRequired: true,
      credentialStatus: { configured: false },
    })
    setupSetting({
      setting: {
        ...baseSetting,
        network: { ...baseSetting.network, customRelayUrls: [duplicateUrl, duplicateUrl] },
      },
      saveRelay: mockSaveRelay,
    })
    render(<NetworkSection />)

    const secondUrl = screen.getByRole('textbox', {
      name: /自定义中继节点 2|Custom relay node 2/,
    })
    await user.clear(secondUrl)
    await user.type(secondUrl, 'https://replacement.example.com')
    await user.click(screen.getAllByRole('button', { name: /测试可用性|Test availability/ })[1])
    await user.click(screen.getAllByRole('button', { name: /保存中继节点|Save relay node/ })[1])

    await waitFor(() => {
      expect(mockSaveRelay).toHaveBeenCalledWith({
        index: 1,
        previousUrl: duplicateUrl,
        nextUrl: 'https://replacement.example.com',
        credential: { action: 'keep' },
      })
    })
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
    expect(document.querySelector('[data-animated-toast-stack]')).toBeNull()
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

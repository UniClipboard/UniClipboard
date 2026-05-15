import { beforeEach, describe, expect, it, vi } from 'vitest'
import { buildKeyboardShortcutsPatch, updateKeyboardShortcuts } from '@/api/tauri-command/settings'
import { commands } from '@/lib/ipc'

// 实现已切到 typed `commands` proxy（`@/lib/ipc`，背后是 tauri-specta
// 生成的 binding）。这里只 mock 我们用到的命令，未 mock 的命令调用会
// 直接抛 TypeError，等于 fail-fast 防止误调用未 stub 的命令。
vi.mock('@/lib/ipc', () => ({
  commands: {
    updateKeyboardShortcuts: vi.fn(),
  },
}))

const mockUpdateKeyboardShortcuts = vi.mocked(commands.updateKeyboardShortcuts)

beforeEach(() => {
  vi.clearAllMocks()
})

describe('Tauri settings command wrapper — keyboard shortcuts', () => {
  it('删除旧 override 时发送 null，而不是省略该 key', () => {
    const patch = buildKeyboardShortcutsPatch(
      {
        'global.toggleQuickPanel': 'meta+ctrl+v',
        'nav.dashboard': 'mod+1',
      },
      {
        'nav.dashboard': 'mod+2',
      }
    )

    expect(patch).toEqual({
      'global.toggleQuickPanel': null,
      'nav.dashboard': 'mod+2',
    })
  })

  it('通过 in-process Tauri command 保存并应用快捷键', async () => {
    mockUpdateKeyboardShortcuts.mockResolvedValueOnce({
      keyboardShortcuts: {
        'global.toggleQuickPanel': 'meta+shift+v',
      },
    })

    const result = await updateKeyboardShortcuts(
      {
        'global.toggleQuickPanel': 'meta+ctrl+v',
      },
      {
        'global.toggleQuickPanel': 'meta+shift+v',
      }
    )

    // typed proxy 现在以位置参数传 patch（buildKeyboardShortcutsPatch 计算
    // 出的 diff），而不是历史的 `{ shortcuts: ... }` 命名 wrapping。
    expect(mockUpdateKeyboardShortcuts).toHaveBeenCalledWith({
      'global.toggleQuickPanel': 'meta+shift+v',
    })
    expect(result).toEqual({
      'global.toggleQuickPanel': 'meta+shift+v',
    })
  })
})

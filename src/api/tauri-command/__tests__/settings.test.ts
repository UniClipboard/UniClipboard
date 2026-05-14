import { beforeEach, describe, expect, it, vi } from 'vitest'
import { buildKeyboardShortcutsPatch, updateKeyboardShortcuts } from '@/api/tauri-command/settings'
import { invokeWithTrace } from '@/lib/tauri-command'

vi.mock('@/lib/tauri-command', () => ({
  invokeWithTrace: vi.fn(),
}))

const mockInvokeWithTrace = vi.mocked(invokeWithTrace)

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
    mockInvokeWithTrace.mockResolvedValueOnce({
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

    expect(mockInvokeWithTrace).toHaveBeenCalledWith('update_keyboard_shortcuts', {
      shortcuts: {
        'global.toggleQuickPanel': 'meta+shift+v',
      },
    })
    expect(result).toEqual({
      'global.toggleQuickPanel': 'meta+shift+v',
    })
  })
})

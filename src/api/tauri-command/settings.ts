import type { ShortcutKey } from '@/api/daemon/settings'
import { invokeWithTrace } from '@/lib/tauri-command'

export type KeyboardShortcutsPatch = Record<string, ShortcutKey | null>

interface UpdateKeyboardShortcutsResult {
  keyboardShortcuts: Record<string, ShortcutKey>
}

export function buildKeyboardShortcutsPatch(
  previous: Record<string, ShortcutKey>,
  next: Record<string, ShortcutKey>
): KeyboardShortcutsPatch {
  const patch: KeyboardShortcutsPatch = {}
  const ids = new Set([...Object.keys(previous), ...Object.keys(next)])

  for (const id of ids) {
    if (!(id in next)) {
      patch[id] = null
      continue
    }

    if (!shortcutKeyEquals(previous[id], next[id])) {
      patch[id] = next[id]!
    }
  }

  return patch
}

export async function updateKeyboardShortcuts(
  previous: Record<string, ShortcutKey>,
  next: Record<string, ShortcutKey>
): Promise<Record<string, ShortcutKey>> {
  const result = await invokeWithTrace<UpdateKeyboardShortcutsResult>('update_keyboard_shortcuts', {
    shortcuts: buildKeyboardShortcutsPatch(previous, next),
  })
  return result.keyboardShortcuts
}

function shortcutKeyEquals(left: ShortcutKey | undefined, right: ShortcutKey | undefined): boolean {
  if (left === undefined || right === undefined) {
    return left === right
  }
  return JSON.stringify(left) === JSON.stringify(right)
}

/**
 * Settings Tauri command wrappers — keyboard shortcuts patch.
 *
 * Backend: `src-tauri/crates/uc-tauri/src/commands/settings.rs`.
 *
 * 这层只做"前端 diff → 三态 patch → 把结果摊平回 Record"的薄壳，
 * 真实的命令调用走 `commands.updateKeyboardShortcuts`（来自
 * `ipc-bindings.generated.ts`，类型链由 `cargo test --test specta_export`
 * 强制对齐）。
 */

import type { ShortcutKey } from '@/api/daemon/settings'
import { commands } from '@/lib/ipc'

export type KeyboardShortcutsPatch = Record<string, ShortcutKey | null>

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
  const patch = buildKeyboardShortcutsPatch(previous, next)
  const result = await commands.updateKeyboardShortcuts(patch)
  return result.keyboardShortcuts
}

function shortcutKeyEquals(left: ShortcutKey | undefined, right: ShortcutKey | undefined): boolean {
  if (left === undefined || right === undefined) {
    return left === right
  }
  return JSON.stringify(left) === JSON.stringify(right)
}

/**
 * Toggle the quick panel feature live — registers/unregisters the global
 * shortcut and creates/destroys the hidden panel window in-process. Returns
 * after both the OS side effects and the on-disk patch have committed.
 *
 * Backend: `commands::quick_panel::set_quick_panel_enabled`.
 */
export async function setQuickPanelEnabled(enabled: boolean): Promise<void> {
  await commands.setQuickPanelEnabled(enabled)
}

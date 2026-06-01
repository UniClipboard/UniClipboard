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

import type { QuickPanelPosition, ShortcutKey } from '@/api/daemon/settings'
import { commands } from '@/lib/ipc'
import type { RelayProbeOutcome } from '@/lib/ipc-bindings.generated'

export type { RelayProbeOutcome }

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

/**
 * Persist the quick panel placement preference (center vs. follow-cursor).
 *
 * This has no OS side effects — it only changes where the next panel `show()`
 * positions the window, and refreshes the backend's cached placement mode so
 * the synchronous, shortcut-triggered show picks it up immediately.
 *
 * Backend: `commands::quick_panel::set_quick_panel_position`.
 */
export async function setQuickPanelPosition(position: QuickPanelPosition): Promise<void> {
  await commands.setQuickPanelPosition(position)
}

/**
 * Set "launch at login" — persists the `auto_start` preference AND applies the
 * OS-level launch registration in one backend call, rolling back the setting if
 * the OS step fails. This is the only path that should toggle autostart: the
 * generic settings patch deliberately omits `autoStart` so the OS side effect
 * is never silently skipped (see `toSettingsPatchRequest` in
 * `@/api/daemon/settings`).
 *
 * Backend: `commands::autostart::update_autostart`.
 */
export async function updateAutostart(enabled: boolean): Promise<void> {
  await commands.updateAutostart(enabled)
}

/**
 * Run a one-shot reachability probe against an iroh relay URL.
 *
 * Returns a discriminated `RelayProbeOutcome` — successful handshakes and
 * predictable failures (invalid URL, DNS error, TLS error, timeout, …) all
 * resolve, so UI code can present targeted messaging without try/catch.
 * The promise itself only rejects when the backend can't run the probe at
 * all (e.g. the adapter isn't wired up in this runtime).
 *
 * Backend: `commands::settings::probe_relay_url`.
 */
export async function probeRelayUrl(url: string): Promise<RelayProbeOutcome> {
  return commands.probeRelayUrl(url)
}

import { emit } from '@tauri-apps/api/event'
import type { SettingChangedEvent } from '@/types/events'
import type { Settings } from '@/types/setting'

export const SETTINGS_CHANGED_EVENT = 'settings://changed'

export async function emitSettingsChanged(settings: Settings): Promise<void> {
  await emit<SettingChangedEvent>(SETTINGS_CHANGED_EVENT, {
    settingJson: JSON.stringify(settings),
    timestamp: Date.now(),
  })
}

export function parseSettingsChangedPayload(
  payload: SettingChangedEvent | null | undefined
): Settings | null {
  if (!payload?.settingJson) {
    return null
  }

  try {
    return JSON.parse(payload.settingJson) as Settings
  } catch (err) {
    console.error('Failed to parse settings change payload:', err)
    return null
  }
}

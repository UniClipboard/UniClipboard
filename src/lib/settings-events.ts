import { emit } from '@tauri-apps/api/event'
import { createLogger } from '@/lib/logger'
import type { SettingChangedEvent } from '@/types/events'
import type { Settings } from '@/types/setting'

const log = createLogger('settings-events')

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
    log.error({ err }, 'Failed to parse settings change payload')
    return null
  }
}

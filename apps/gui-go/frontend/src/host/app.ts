// Host adapter for `@tauri-apps/api/app`.
import { invoke } from './core'

export const getVersion = async (): Promise<string> => {
  const meta = await invoke<{ appVersion: string }>('get_device_meta')
  return meta.appVersion
}

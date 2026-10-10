// Host adapter for `@tauri-apps/api/app`.
import * as HostService from '@host/hostservice'

export const getVersion = async (): Promise<string> =>
  (await HostService.GetDeviceMeta()).appVersion

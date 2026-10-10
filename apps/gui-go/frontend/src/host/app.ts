// Application metadata of the desktop host.
import * as HostService from '@host/hostservice'

export const getVersion = async (): Promise<string> =>
  (await HostService.GetDeviceMeta()).appVersion

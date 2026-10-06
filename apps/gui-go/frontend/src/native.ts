import { Call } from '@wailsio/runtime'
export interface Connection {
  baseUrl: string
  wsUrl: string
  profile: string
  pid: number
}
export interface Session {
  sessionToken: string
  expiresInSecs: number
  refreshAtSecs: number
}
export const commands = {
  getDaemonSession: () => Call.ByName('main.HostService.Session') as Promise<Session>,
}
export const connection = () => Call.ByName('main.HostService.Connection') as Promise<Connection>
export const openSecondary = () => Call.ByName('main.HostService.OpenSecondary')
export const quit = () => Call.ByName('main.HostService.Quit')

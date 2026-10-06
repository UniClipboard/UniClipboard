// Host adapter for `@tauri-apps/plugin-notification`: notifications go through the Go host's
// system notification service.
import { Events } from '@wailsio/runtime'
import { invoke } from './core'

interface NotificationOptions {
  id?: number
  title: string
  body?: string
}

export const isPermissionGranted = (): Promise<boolean> =>
  invoke<boolean>('host_notification_permission')

export const requestPermission = (): Promise<'granted' | 'denied' | 'default'> =>
  invoke<'granted' | 'denied'>('host_notification_request_permission')

// The plugin's `sendNotification` is fire-and-forget; failures are best effort like in the Tauri shell.
export const sendNotification = (options: NotificationOptions | string): void => {
  const normalized = typeof options === 'string' ? { title: options } : options
  void invoke('host_notification_send', { options: normalized }).catch(() => undefined)
}

// Clicking a notification reaches the handler with the notification's numeric id when it had one.
export const onAction = async (handler: (notification: { id?: number }) => void) => {
  const off = Events.On('notification://action', event => handler(event.data as { id?: number }))
  return { unregister: async (): Promise<void> => off() }
}

// Host adapter for `@tauri-apps/plugin-notification`. Native notifications are
// not wired yet, so permission is reported as denied and nothing is shown.
export const isPermissionGranted = async (): Promise<boolean> => false
export const requestPermission = async (): Promise<'granted' | 'denied' | 'default'> => 'denied'
export const sendNotification = (_options: unknown): void => undefined
export const onAction = async (_handler: (notification: { id?: number }) => void) => ({
  unregister: async (): Promise<void> => undefined,
})

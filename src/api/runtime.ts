import { invokeWithTrace } from '@/lib/tauri-command'

export async function getDeviceId(): Promise<string> {
  return invokeWithTrace<string>('get_device_id')
}

/**
 * Aggregated device + app meta returned by the Rust host so that the Sentry
 * front-end SDK can attach the same scope tags the Rust side already attaches.
 * Field names mirror the Rust struct one-to-one to keep the two sinks aligned.
 */
export interface DeviceMeta {
  device_id: string
  device_role: string
  platform: string
  app_version: string
  app_channel: string
}

export async function getDeviceMeta(): Promise<DeviceMeta> {
  return invokeWithTrace<DeviceMeta>('get_device_meta')
}

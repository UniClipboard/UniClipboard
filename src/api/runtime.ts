import { invokeWithTrace } from '@/lib/tauri-command'

export async function getDeviceId(): Promise<string> {
  return invokeWithTrace<string>('get_device_id')
}

/**
 * Rust 主进程返回的设备和应用元数据。
 * 字段名与 Rust 序列化后的输出保持一致，供前端 Sentry scope 使用。
 */
export interface DeviceMeta {
  deviceId: string
  deviceRole: string
  platform: string
  appVersion: string
  appChannel: string
}

export async function getDeviceMeta(): Promise<DeviceMeta> {
  return invokeWithTrace<DeviceMeta>('get_device_meta')
}

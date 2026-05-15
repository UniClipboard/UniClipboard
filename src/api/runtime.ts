import { commands } from '@/lib/ipc'
import type { DeviceMeta as GeneratedDeviceMeta } from '@/lib/ipc'

export async function getDeviceId(): Promise<string> {
  return await commands.getDeviceId()
}

/**
 * Rust 主进程返回的设备和应用元数据。
 * 字段名与 Rust 序列化后的输出保持一致，供前端 Sentry scope 使用。
 *
 * 重新导出生成 binding 的同名类型，保留历史导入路径
 * `import { DeviceMeta } from '@/api/runtime'` 不破。
 */
export type DeviceMeta = GeneratedDeviceMeta

export async function getDeviceMeta(): Promise<DeviceMeta> {
  return await commands.getDeviceMeta()
}

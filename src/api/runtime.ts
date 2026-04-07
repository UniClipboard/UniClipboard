import { invokeWithTrace } from '@/lib/tauri-command'

export async function getDeviceId(): Promise<string> {
  return invokeWithTrace<string>('get_device_id')
}

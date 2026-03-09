import { invokeWithTrace } from '@/lib/tauri-command'

export type LifecycleState = 'Idle' | 'Pending' | 'Ready' | 'WatcherFailed' | 'NetworkFailed'

export async function getLifecycleStatus(): Promise<LifecycleState> {
  try {
    const raw = await invokeWithTrace<string>('get_lifecycle_status')
    return JSON.parse(raw) as LifecycleState
  } catch (error) {
    console.error('Failed to get lifecycle status:', error)
    throw error
  }
}

export async function retryLifecycle(): Promise<void> {
  try {
    await invokeWithTrace<void>('retry_lifecycle')
  } catch (error) {
    console.error('Failed to retry lifecycle:', error)
    throw error
  }
}

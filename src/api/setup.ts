import { listen } from '@tauri-apps/api/event'
import { invokeWithTrace } from '@/lib/tauri-command'

export type SetupError =
  | 'PassphraseMismatch'
  | 'PassphraseEmpty'
  | { PassphraseTooShort: { min_len: number } }
  | 'PassphraseInvalidOrMismatch'
  | 'NetworkTimeout'
  | 'PeerUnavailable'
  | 'PairingRejected'
  | 'PairingFailed'

export type SetupState =
  | 'Welcome'
  | { CreateSpaceInputPassphrase: { error: SetupError | null } }
  | { JoinSpaceSelectDevice: { error: SetupError | null } }
  | {
      JoinSpaceConfirmPeer: {
        short_code: string
        peer_fingerprint?: string | null
        error: SetupError | null
      }
    }
  | { JoinSpaceInputPassphrase: { error: SetupError | null } }
  | { ProcessingCreateSpace: { message: string | null } }
  | { ProcessingJoinSpace: { message: string | null } }
  | 'Completed'

export interface SetupStateChangedEvent {
  sessionId: string
  state: SetupState
  source?: string
  ts: number
}

function decodeSetupState(raw: unknown): SetupState {
  if (typeof raw === 'string') {
    try {
      return JSON.parse(raw) as SetupState
    } catch {
      return raw as SetupState
    }
  }
  return raw as SetupState
}

/**
 * Get current setup state
 * 获取当前设置流程状态
 */
export async function getSetupState(): Promise<SetupState> {
  return decodeSetupState(await invokeWithTrace('get_setup_state'))
}

/**
 * Start new space flow
 * 启动新空间流程
 */
export async function startNewSpace(): Promise<SetupState> {
  return decodeSetupState(await invokeWithTrace('start_new_space'))
}

/**
 * Start join space flow
 * 启动加入空间流程
 */
export async function startJoinSpace(): Promise<SetupState> {
  return decodeSetupState(await invokeWithTrace('start_join_space'))
}

/**
 * Select peer device to join
 * 选择加入空间的设备
 */
export async function selectJoinPeer(peerId: string): Promise<SetupState> {
  return decodeSetupState(await invokeWithTrace('select_device', { peerId }))
}

/**
 * Submit passphrase for new space
 * 提交新空间口令
 */
export async function submitPassphrase(
  passphrase1: string,
  passphrase2: string
): Promise<SetupState> {
  return decodeSetupState(await invokeWithTrace('submit_passphrase', { passphrase1, passphrase2 }))
}

/**
 * Verify passphrase for join space
 * 校验加入空间口令
 */
export async function verifyPassphrase(passphrase: string): Promise<SetupState> {
  return decodeSetupState(await invokeWithTrace('verify_passphrase', { passphrase }))
}

/**
 * Confirm trust for the selected peer device
 * 确认选中设备的可信度
 */
export async function confirmPeerTrust(): Promise<SetupState> {
  return decodeSetupState(await invokeWithTrace('confirm_peer_trust'))
}

/**
 * Cancel setup flow
 * 取消设置流程
 */
export async function cancelSetup(): Promise<SetupState> {
  return decodeSetupState(await invokeWithTrace('cancel_setup'))
}

/**
 * Listen setup state changes with session idempotency.
 */
export async function onSetupStateChanged(
  callback: (event: SetupStateChangedEvent) => void
): Promise<() => void> {
  try {
    let activeSessionId: string | null = null
    const seenEventKeys = new Set<string>()

    const unlisten = await listen<SetupStateChangedEvent>('setup-state-changed', event => {
      const payload = event.payload

      if (!payload.sessionId) {
        return
      }

      if (activeSessionId === null) {
        activeSessionId = payload.sessionId
      }

      if (payload.sessionId !== activeSessionId) {
        return
      }

      const dedupeKey = `${payload.sessionId}:${JSON.stringify(payload.state)}:${payload.ts}`
      if (seenEventKeys.has(dedupeKey)) {
        return
      }
      seenEventKeys.add(dedupeKey)

      callback(payload)

      if (payload.state === 'Completed') {
        activeSessionId = null
        seenEventKeys.clear()
      }
    })

    return () => {
      unlisten()
    }
  } catch (error) {
    console.error('Failed to setup setup state changed listener:', error)
    return () => {}
  }
}

import { invokeWithTrace } from '@/lib/tauri-command'

export interface EncryptionSessionStatus {
  initialized: boolean
  session_ready: boolean
}

/**
 * 获取加密口令
 * @returns Promise，返回加密口令
 */
export async function getEncryptionPassword(): Promise<string> {
  try {
    return await invokeWithTrace('get_encryption_password')
  } catch (error) {
    console.error('获取加密口令失败:', error)
    throw error
  }
}

/**
 * 设置加密口令
 * @param password 要设置的加密口令
 * @returns Promise，成功返回true
 */
export async function setEncryptionPassword(password: string): Promise<boolean> {
  try {
    return await invokeWithTrace('set_encryption_password', { password })
  } catch (error) {
    console.error('设置加密口令失败:', error)
    throw error
  }
}

/**
 * 删除加密口令
 * @returns Promise，成功返回true
 */
export async function deleteEncryptionPassword(): Promise<boolean> {
  try {
    return await invokeWithTrace('delete_encryption_password')
  } catch (error) {
    console.error('删除加密口令失败:', error)
    throw error
  }
}

/**
 * 获取加密会话状态
 * @returns Promise，返回加密初始化状态与会话就绪状态
 */
export async function getEncryptionSessionStatus(): Promise<EncryptionSessionStatus> {
  try {
    return await invokeWithTrace('get_encryption_session_status')
  } catch (error) {
    console.error('获取加密会话状态失败:', error)
    throw error
  }
}

/**
 * 解锁加密会话
 * @returns Promise，成功返回true
 */
export async function unlockEncryptionSession(): Promise<boolean> {
  try {
    return await invokeWithTrace('unlock_encryption_session')
  } catch (error) {
    console.error('解锁加密会话失败:', error)
    throw error
  }
}

/**
 * 验证 macOS Keychain "Always Allow" 权限
 * @returns Promise，返回是否已授权
 */
export async function verifyKeychainAccess(): Promise<boolean> {
  try {
    return await invokeWithTrace('verify_keychain_access')
  } catch (error) {
    console.error('Keychain verification failed:', error)
    throw error
  }
}

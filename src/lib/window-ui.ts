import { getMainWindowMaterial } from '@/api/runtime'
import {
  applyPlatformEffectPreferences,
  applyWindowMaterial,
  detectPlatformInfo,
} from '@/lib/platform'
import { initializeUiScale } from '@/lib/ui-scale'

export { applyPlatformEffectPreferences } from '@/lib/platform'

export const applyPlatformTypographyScale = () => {
  if (typeof document === 'undefined') {
    return
  }

  const { isWindows } = detectPlatformInfo()

  if (!isWindows) {
    return
  }

  const root = document.documentElement

  root.style.setProperty('--font-size-caption', '0.6875rem') /* 11px */
  root.style.setProperty('--font-size-small', '0.75rem') /* 12px */
  root.style.setProperty('--font-size-body', '0.8125rem') /* 13px */
  root.style.setProperty('--font-size-body-lg', '0.875rem') /* 14px */
  root.style.setProperty('--font-size-section', '0.9375rem') /* 15px */
  root.style.setProperty('--font-size-title', '1.125rem') /* 18px */
}

/**
 * Windows 端：查 Rust 侧 Mica 装配结果并写 `data-uc-window-material`。
 *
 * 非 Windows 直接 noop —— macOS 走 tauri.conf 的 `hudWindow`、Linux 没有
 * 等价材质，都不需要这个 attribute。失败时（Tauri runtime 未就绪 / IPC
 * 出错）静默走 opaque 兜底，与 issue #699 "Win10 旧版纯色背景" 行为一致。
 */
export const applyWindowMaterialFromBackend = async (): Promise<void> => {
  if (typeof document === 'undefined') {
    return
  }
  const { isWindows } = detectPlatformInfo()
  if (!isWindows) {
    return
  }
  try {
    const material = await getMainWindowMaterial()
    applyWindowMaterial(material)
  } catch (err) {
    console.warn('[window-ui] failed to fetch window material from backend:', err)
  }
}

export const initializeWindowUi = (): (() => void) => {
  applyPlatformTypographyScale()
  applyPlatformEffectPreferences()
  return initializeUiScale()
}

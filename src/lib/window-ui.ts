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
 * 查 Rust 侧主窗口材质状态并写 `<html>` 的 `data-uc-window-material`。
 *
 * 后端按平台返回：Windows 装 Mica → `'mica'` 或 `'none'`；macOS 上报
 * hudWindow → `'hud'`；Linux → `'none'`。前端无条件 invoke，让后端做平台
 * 判断 —— 失败时（Tauri runtime 未就绪 / IPC 出错）静默走 opaque 兜底。
 */
export const applyWindowMaterialFromBackend = async (): Promise<void> => {
  if (typeof document === 'undefined') {
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

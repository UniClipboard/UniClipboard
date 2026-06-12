/**
 * Clipboard Proxy
 * 剪贴板代理 - 在 Android 后台时通过 Shizuku 或悬浮窗获取剪贴板，其他情况直接调用 expo-clipboard
 *
 * 优先级：Shizuku > 悬浮窗 > 直接调用
 *
 * 当启用后台同步+悬浮窗模式时，悬浮窗按需显示（不可见的 1px 窗口），
 * 每次读取剪贴板时只是 focus 到悬浮窗读取后 unfocus，而非反复创建/销毁。
 * 若持续 10 秒无后台剪贴板调用，自动关闭悬浮窗以节省资源，下次需要时再打开。
 */

import * as Clipboard from 'expo-clipboard';
import { AppState, Platform } from 'react-native';
import { useSettingsStore } from '@/stores/settingsStore';
import { setTimer, clearTimer } from 'native-timer';
import { nativeSaveClipboardImageToFile } from 'native-util';

/** 悬浮窗空闲超时时间（毫秒） */
const OVERLAY_IDLE_TIMEOUT_MS = 10_000;
/** 空闲计时器的固定 tag */
const IDLE_TIMER_TAG = 'clipboard_overlay_idle';

let overlayModule: typeof import('clipboard-overlay') | null = null;
let shizukuModule: typeof import('shizuku-clipboard') | null = null;

/**
 * 重置空闲计时器：每次悬浮窗被使用时调用，
 * 10 秒内无新调用则自动关闭悬浮窗。
 * 使用 native-timer 以确保后台可靠运行。
 */
function resetIdleTimer(): void {
  // 先清除已有的计时器，再重新启动
  clearTimer(IDLE_TIMER_TAG);
  setTimer(
    () => {
      // 触发一次后立即清除自身（模拟 setTimeout）
      clearTimer(IDLE_TIMER_TAG);
      if (overlayModule?.isOverlayShowing()) {
        overlayModule.hideOverlayWindow().catch((e) => {
          console.warn('[ClipboardProxy] Failed to hide overlay on idle timeout:', e);
        });
      }
    },
    OVERLAY_IDLE_TIMEOUT_MS,
    IDLE_TIMER_TAG
  );
}

/** 清除空闲计时器（应用回前台或手动关闭时） */
function clearIdleTimer(): void {
  clearTimer(IDLE_TIMER_TAG);
}

if (Platform.OS === 'android') {
  overlayModule = require('clipboard-overlay');
  shizukuModule = require('shizuku-clipboard');

  // 当应用回到前台时，自动销毁常驻悬浮窗并清除空闲计时器
  AppState.addEventListener('change', (nextAppState) => {
    if (nextAppState === 'active') {
      clearIdleTimer();
      if (overlayModule?.isOverlayShowing()) {
        overlayModule.hideOverlayWindow().catch((e) => {
          console.warn('[ClipboardProxy] Failed to dismiss overlay on foreground:', e);
        });
      }
    }
  });
}

/**
 * 确保悬浮窗已显示（仅在需要时创建）
 */
async function ensureOverlayShowing(): Promise<void> {
  if (!overlayModule) return;
  if (!overlayModule.isOverlayShowing()) {
    try {
      await overlayModule.showOverlayWindow();
    } catch (e) {
      console.warn('[ClipboardProxy] Failed to show persistent overlay:', e);
    }
  }
}

/**
 * 隐藏悬浮窗
 */
export async function dismissOverlay(): Promise<void> {
  if (!overlayModule) return;
  if (overlayModule.isOverlayShowing()) {
    try {
      await overlayModule.hideOverlayWindow();
    } catch (e) {
      console.warn('[ClipboardProxy] Failed to hide persistent overlay:', e);
    }
  }
}

/**
 * 判断是否应该使用悬浮窗获取剪贴板
 * 条件：Android + 后台 + 设置启用 + 权限已授予
 * 如果条件满足，确保悬浮窗已常驻显示
 */
async function shouldUseOverlay(): Promise<boolean> {
  if (Platform.OS !== 'android' || !overlayModule) return false;
  if (AppState.currentState !== 'background') return false;
  const config = useSettingsStore.getState().config;
  if (!(config?.enableClipboardOverlay ?? false)) return false;
  // Sync overlay visibility and retry count to native module
  const isDebug = config?.debugMode ?? false;
  const showOverlay = isDebug && (config?.debugOverlayVisible ?? false);
  overlayModule.setDebugMode(showOverlay);
  overlayModule.setMaxRetries(isDebug ? 20 : 5);
  if (!overlayModule.hasOverlayPermission()) return false;
  // Ensure persistent overlay is showing before reading clipboard
  await ensureOverlayShowing();
  // Reset idle timer: 10 seconds without calls will auto-hide overlay
  resetIdleTimer();
  return true;
}

/**
 * 判断是否应该使用 Shizuku 获取剪贴板
 * 条件：Android + 后台 + 设置启用 + Shizuku 可用且有权限
 * Shizuku 优先级高于悬浮窗
 */
function shouldUseShizuku(): boolean {
  if (Platform.OS !== 'android' || !shizukuModule) return false;
  if (AppState.currentState !== 'background') return false;
  const config = useSettingsStore.getState().config;
  if (!(config?.enableShizukuClipboard ?? false)) return false;
  if (!shizukuModule.isShizukuAvailable()) return false;
  if (!shizukuModule.hasShizukuPermission()) return false;
  return true;
}

/**
 * 获取剪贴板文本
 */
export async function getStringAsync(options?: Clipboard.GetStringOptions): Promise<string> {
  if (shouldUseShizuku()) {
    try {
      const result = await shizukuModule!.getStringViaShizuku();
      return result;
    } catch (e) {
      console.warn('[ClipboardProxy] Shizuku getStringAsync failed, falling back:', e);
    }
  }
  if (await shouldUseOverlay()) {
    try {
      return await overlayModule!.getStringViaOverlay();
    } catch (e) {
      console.warn('[ClipboardProxy] Overlay getStringAsync failed, falling back:', e);
    }
  }
  return Clipboard.getStringAsync(options);
}

/**
 * 设置剪贴板文本
 */
export async function setStringAsync(
  text: string,
  options?: Clipboard.SetStringOptions
): Promise<boolean> {
  return Clipboard.setStringAsync(text, options);
}

/**
 * 检查剪贴板是否有文本
 */
export async function hasStringAsync(): Promise<boolean> {
  if (shouldUseShizuku()) {
    try {
      return await shizukuModule!.hasStringViaShizuku();
    } catch (e) {
      console.warn('[ClipboardProxy] Shizuku hasStringAsync failed, falling back:', e);
    }
  }
  if (await shouldUseOverlay()) {
    try {
      return await overlayModule!.hasStringViaOverlay();
    } catch (e) {
      console.warn('[ClipboardProxy] Overlay hasStringAsync failed, falling back:', e);
    }
  }
  return Clipboard.hasStringAsync();
}

/**
 * 检查剪贴板是否有图片
 */
export async function hasImageAsync(): Promise<boolean> {
  if (shouldUseShizuku()) {
    try {
      return await shizukuModule!.hasImageViaShizuku();
    } catch (e) {
      console.warn('[ClipboardProxy] Shizuku hasImageAsync failed, falling back:', e);
    }
  }
  if (await shouldUseOverlay()) {
    try {
      return await overlayModule!.hasImageViaOverlay();
    } catch (e) {
      console.warn('[ClipboardProxy] Overlay hasImageAsync failed, falling back:', e);
    }
  }
  return Clipboard.hasImageAsync();
}

/**
 * 获取剪贴板图片（旧接口，返回 base64）
 */
export async function getImageAsync(
  options: Clipboard.GetImageOptions
): Promise<Clipboard.ClipboardImage | null> {
  if (await shouldUseOverlay()) {
    try {
      const result = await overlayModule!.getImageViaOverlay();
      if (result) {
        return {
          data: result.data,
          size: result.size,
        };
      }
      return null;
    } catch (e) {
      console.warn('[ClipboardProxy] Overlay getImageAsync failed, falling back:', e);
    }
  }
  return Clipboard.getImageAsync(options);
}

/**
 * 获取剪贴板图片并直接保存到文件（不经过 JS 内存）
 * @param destFileUri 目标文件 URI（file:// 格式）
 * @returns 成功返回 true，剪贴板无图片或失败返回 false
 */
export async function saveImageToFileAsync(
  destDirPath: string
): Promise<{ filePath: string; mimeType: string } | null> {
  if (await shouldUseOverlay()) {
    try {
      const result = await overlayModule!.saveImageToFileViaOverlay(destDirPath);
      if (result) return { filePath: result.filePath, mimeType: result.mimeType };
      // fallback if overlay failed
    } catch (e) {
      console.warn('[ClipboardProxy] Overlay saveImageToFileAsync failed, falling back:', e);
    }
  }
  // 前台模式：使用 native-util 直接读取系统剪贴板并写入文件
  if (Platform.OS === 'android') {
    const result = await nativeSaveClipboardImageToFile(destDirPath);
    return result ? { filePath: result.filePath, mimeType: result.mimeType } : null;
  }
  return null;
}

/**
 * 设置剪贴板图片
 */
export async function setImageAsync(base64Image: string): Promise<void> {
  return Clipboard.setImageAsync(base64Image);
}

import { Platform } from 'react-native';
import { requireNativeModule } from 'expo-modules-core';

const MODULE_NAME = 'ShizukuClipboardModule';

interface ShizukuClipboardModuleInterface {
  isShizukuAvailable(): boolean;
  hasShizukuPermission(): boolean;
  requestShizukuPermission(): boolean;
  getStringViaShizuku(): Promise<string>;
  hasStringViaShizuku(): Promise<boolean>;
  hasImageViaShizuku(): Promise<boolean>;
  getImageUriViaShizuku(): Promise<string | null>;
}

const NativeModule: ShizukuClipboardModuleInterface | null =
  Platform.OS === 'android' ? requireNativeModule(MODULE_NAME) : null;

/**
 * 检查 Shizuku 是否可用（已安装并运行中）
 */
export function isShizukuAvailable(): boolean {
  if (NativeModule) {
    return NativeModule.isShizukuAvailable();
  }
  return false;
}

/**
 * 检查是否已获取 Shizuku 权限
 */
export function hasShizukuPermission(): boolean {
  if (NativeModule) {
    return NativeModule.hasShizukuPermission();
  }
  return false;
}

/**
 * 请求 Shizuku 权限
 */
export function requestShizukuPermission(): boolean {
  if (NativeModule) {
    return NativeModule.requestShizukuPermission();
  }
  return false;
}

/**
 * 通过 Shizuku 获取剪贴板文本
 */
export async function getStringViaShizuku(): Promise<string> {
  if (NativeModule) {
    return NativeModule.getStringViaShizuku();
  }
  return '';
}

/**
 * 通过 Shizuku 检查剪贴板是否有文本
 */
export async function hasStringViaShizuku(): Promise<boolean> {
  if (NativeModule) {
    return NativeModule.hasStringViaShizuku();
  }
  return false;
}

/**
 * 通过 Shizuku 检查剪贴板是否有图片
 */
export async function hasImageViaShizuku(): Promise<boolean> {
  if (NativeModule) {
    return NativeModule.hasImageViaShizuku();
  }
  return false;
}

/**
 * 通过 Shizuku 获取剪贴板图片 URI
 */
export async function getImageUriViaShizuku(): Promise<string | null> {
  if (NativeModule) {
    return NativeModule.getImageUriViaShizuku();
  }
  return null;
}

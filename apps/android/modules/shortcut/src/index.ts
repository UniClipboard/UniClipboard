import { Platform } from 'react-native';
import { requireNativeModule } from 'expo-modules-core';

export interface ShortcutModuleType {
  requestPinDownloadShortcut(): Promise<boolean>;
  requestPinUploadShortcut(): Promise<boolean>;
  setDynamicShortcuts(): boolean;
}

let nativeModule: ShortcutModuleType | null = null;

function getNativeModule(): ShortcutModuleType {
  if (nativeModule === null) {
    if (Platform.OS !== 'android') {
      throw new Error('ShortcutModule is only available on Android');
    }
    nativeModule = requireNativeModule<ShortcutModuleType>('ShortcutModule');
  }
  return nativeModule!;
}

export const isShortcutModuleAvailable = Platform.OS === 'android';

export async function requestPinDownloadShortcut(): Promise<boolean> {
  if (Platform.OS !== 'android') {
    throw new Error('ShortcutModule is only available on Android');
  }
  return getNativeModule().requestPinDownloadShortcut();
}

export async function requestPinUploadShortcut(): Promise<boolean> {
  if (Platform.OS !== 'android') {
    throw new Error('ShortcutModule is only available on Android');
  }
  return getNativeModule().requestPinUploadShortcut();
}

export function setDynamicShortcuts(): boolean {
  if (Platform.OS !== 'android') {
    return false;
  }
  return getNativeModule().setDynamicShortcuts();
}

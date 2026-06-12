import { Platform } from 'react-native';
import { requireNativeModule, type EventSubscription } from 'expo-modules-core';

const MODULE_NAME = 'ForegroundServiceModule';

interface ForegroundServiceModuleType {
  startService(): boolean;
  stopService(): boolean;
  updateNotification(content: string): boolean;
  isRunning(): boolean;
  addListener(eventName: string, listener: () => void): EventSubscription;
}

const NativeModule: ForegroundServiceModuleType | null =
  Platform.OS === 'android' ? requireNativeModule(MODULE_NAME) : null;

export function startService(): boolean {
  if (NativeModule) {
    return NativeModule.startService();
  }
  return false;
}

export function stopService(): boolean {
  if (NativeModule) {
    return NativeModule.stopService();
  }
  return false;
}

export function updateNotification(content: string): boolean {
  if (NativeModule) {
    return NativeModule.updateNotification(content);
  }
  return false;
}

export function isRunning(): boolean {
  if (NativeModule) {
    return NativeModule.isRunning();
  }
  return false;
}

export function addStopListener(listener: () => void): EventSubscription | null {
  if (NativeModule) {
    return NativeModule.addListener('onStopRequested', listener);
  }
  return null;
}

export function addTempStopListener(listener: () => void): EventSubscription | null {
  if (NativeModule) {
    return NativeModule.addListener('onTempStopRequested', listener);
  }
  return null;
}

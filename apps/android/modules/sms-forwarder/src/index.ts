import { Platform } from 'react-native';
import { requireNativeModule } from 'expo-modules-core';

const MODULE_NAME = 'SmsForwarderModule';

interface SmsForwarderModuleType {
  readRecentSms(count: number): SmsMessage[];
  setStaticReceiverEnabled(enabled: boolean): boolean;
  isStaticReceiverEnabled(): boolean;
  updateSmsUploadNotification(text: string): boolean;
  startSmsUploadCountdown(code: string): boolean;
  extractVerificationCode(body: string): string | null;
}

export interface SmsMessage {
  from: string;
  body: string;
}

const NativeModule: SmsForwarderModuleType | null =
  Platform.OS === 'android' ? requireNativeModule(MODULE_NAME) : null;

export function readRecentSms(count: number): SmsMessage[] {
  if (NativeModule) {
    return NativeModule.readRecentSms(count);
  }
  return [];
}

export function setStaticReceiverEnabled(enabled: boolean): boolean {
  if (NativeModule) {
    return NativeModule.setStaticReceiverEnabled(enabled);
  }
  return false;
}

export function isStaticReceiverEnabled(): boolean {
  if (NativeModule) {
    return NativeModule.isStaticReceiverEnabled();
  }
  return false;
}

export function updateSmsUploadNotification(text: string): boolean {
  if (NativeModule) {
    return NativeModule.updateSmsUploadNotification(text);
  }
  return false;
}

export function startSmsUploadCountdown(code: string): boolean {
  if (NativeModule) {
    return NativeModule.startSmsUploadCountdown(code);
  }
  return false;
}

export function extractVerificationCode(body: string): string | null {
  if (NativeModule) {
    return NativeModule.extractVerificationCode(body);
  }
  return null;
}

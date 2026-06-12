import { Platform } from 'react-native';
import { requireNativeModule } from 'expo-modules-core';

const MODULE_NAME = 'ClipboardOverlayModule';

interface ClipboardOverlayImage {
  data: string;
  size: { width: number; height: number };
}

interface ClipboardOverlayModuleInterface {
  setDebugMode(enabled: boolean): boolean;
  setMaxRetries(retries: number): boolean;
  hasOverlayPermission(): boolean;
  requestOverlayPermission(): void;
  isOverlayShowing(): boolean;
  showOverlayWindow(): Promise<boolean>;
  hideOverlayWindow(): Promise<boolean>;
  getStringViaOverlay(): Promise<string>;
  hasStringViaOverlay(): Promise<boolean>;
  hasImageViaOverlay(): Promise<boolean>;
  getImageViaOverlay(): Promise<ClipboardOverlayImage | null>;
  saveImageToFileViaOverlay(
    destDirPath: string
  ): Promise<{ width: number; height: number; filePath: string; mimeType: string } | null>;
}

const NativeModule: ClipboardOverlayModuleInterface | null =
  Platform.OS === 'android' ? requireNativeModule(MODULE_NAME) : null;

export function setDebugMode(enabled: boolean): void {
  if (NativeModule) {
    NativeModule.setDebugMode(enabled);
  }
}

export function setMaxRetries(retries: number): void {
  if (NativeModule) {
    NativeModule.setMaxRetries(retries);
  }
}

export function hasOverlayPermission(): boolean {
  if (NativeModule) {
    return NativeModule.hasOverlayPermission();
  }
  return false;
}

export function requestOverlayPermission(): void {
  if (NativeModule) {
    NativeModule.requestOverlayPermission();
  }
}

export function isOverlayShowing(): boolean {
  if (NativeModule) {
    return NativeModule.isOverlayShowing();
  }
  return false;
}

export async function showOverlayWindow(): Promise<boolean> {
  if (NativeModule) {
    return NativeModule.showOverlayWindow();
  }
  return false;
}

export async function hideOverlayWindow(): Promise<boolean> {
  if (NativeModule) {
    return NativeModule.hideOverlayWindow();
  }
  return false;
}

export async function getStringViaOverlay(): Promise<string> {
  if (NativeModule) {
    return NativeModule.getStringViaOverlay();
  }
  return '';
}

export async function hasStringViaOverlay(): Promise<boolean> {
  if (NativeModule) {
    return NativeModule.hasStringViaOverlay();
  }
  return false;
}

export async function hasImageViaOverlay(): Promise<boolean> {
  if (NativeModule) {
    return NativeModule.hasImageViaOverlay();
  }
  return false;
}

export async function getImageViaOverlay(): Promise<ClipboardOverlayImage | null> {
  if (NativeModule) {
    return NativeModule.getImageViaOverlay();
  }
  return null;
}

export async function saveImageToFileViaOverlay(
  destDirPath: string
): Promise<{ width: number; height: number; filePath: string; mimeType: string } | null> {
  if (NativeModule) {
    return NativeModule.saveImageToFileViaOverlay(destDirPath);
  }
  return null;
}

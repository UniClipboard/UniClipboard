import { unicEngineNative } from "@/engine/nativeModule";

/**
 * Releases the native Android splash screen once JS bootstrap and theme loading finish.
 */
export function hideLaunchSplash(): void {
  unicEngineNative.hideAndroidSplashScreen();
}

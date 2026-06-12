/**
 * Native Timer Module
 *
 * 统一的 setInterval 替代方案，对外不体现平台差异：
 * - Android: 使用 native Handler（后台可靠运行）
 * - 其他平台: 使用 JS setInterval
 */

import { Platform } from 'react-native';
import { requireNativeModule, type EventSubscription } from 'expo-modules-core';

// ========== Types ==========

export type TimerCallback = () => void;

interface TickEvent {
  tag: string;
}

interface NativeTimerModuleInterface {
  startTimer(tag: string, intervalMs: number): void;
  stopTimer(tag: string): void;
  stopAllTimers(): void;
  addListener(eventName: 'onTick', listener: (event: TickEvent) => void): EventSubscription;
}

// ========== Native Timer (Android) ==========

let nativeModule: NativeTimerModuleInterface | null = null;
const nativeCallbacks = new Map<string, TimerCallback>();
let nativeSubscription: EventSubscription | null = null;

function ensureNativeListener(): void {
  if (nativeSubscription) return;
  if (!nativeModule) {
    nativeModule = requireNativeModule<NativeTimerModuleInterface>('NativeTimerModule');
  }
  nativeSubscription = nativeModule.addListener('onTick', (event: TickEvent) => {
    const cb = nativeCallbacks.get(event.tag);
    if (cb) {
      try {
        cb();
      } catch (e) {
        console.error(`[NativeTimer] Error in callback for tag "${event.tag}":`, e);
      }
    }
  });
}

// ========== JS Timer (fallback) ==========

const jsTimers = new Map<string, ReturnType<typeof globalThis.setInterval>>();

// ========== Tag counter for unique tags ==========

let tagCounter = 0;

// ========== Public API ==========

/**
 * 启动一个定时器（类似 setInterval）
 *
 * Android 端使用 native Handler（后台可靠），其他端使用 JS setInterval。
 *
 * @param callback 定时器回调函数
 * @param intervalMs 间隔毫秒数
 * @param tag 可选标签，用于标识定时器（默认自动生成）
 * @returns tag 字符串，用于 clearTimer 停止
 */
export function setTimer(callback: TimerCallback, intervalMs: number, tag?: string): string {
  const timerTag = tag ?? `timer_${++tagCounter}`;

  // 先停止同名定时器
  clearTimer(timerTag);

  if (Platform.OS === 'android') {
    ensureNativeListener();
    nativeCallbacks.set(timerTag, callback);
    nativeModule!.startTimer(timerTag, intervalMs);
  } else {
    const id = globalThis.setInterval(() => {
      try {
        callback();
      } catch (e) {
        console.error(`[NativeTimer] Error in callback for tag "${timerTag}":`, e);
      }
    }, intervalMs);
    jsTimers.set(timerTag, id);
  }

  return timerTag;
}

/**
 * 停止指定定时器
 * @param tag setTimer 返回的标签
 */
export function clearTimer(tag: string): void {
  if (Platform.OS === 'android') {
    nativeCallbacks.delete(tag);
    if (nativeModule) {
      nativeModule.stopTimer(tag);
    }
  } else {
    const id = jsTimers.get(tag);
    if (id !== undefined) {
      globalThis.clearInterval(id);
      jsTimers.delete(tag);
    }
  }
}

/**
 * 停止所有定时器
 */
export function clearAllTimers(): void {
  if (Platform.OS === 'android') {
    nativeCallbacks.clear();
    if (nativeModule) {
      nativeModule.stopAllTimers();
    }
  } else {
    for (const id of jsTimers.values()) {
      globalThis.clearInterval(id);
    }
    jsTimers.clear();
  }
}

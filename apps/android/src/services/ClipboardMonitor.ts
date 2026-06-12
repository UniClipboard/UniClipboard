/**
 * Clipboard Monitor
 * 剪贴板监听器 - 监听剪贴板内容变化
 */

import { AppState, AppStateStatus, Platform } from 'react-native';
import AsyncStorage from '@react-native-async-storage/async-storage';
import { ClipboardManager } from './ClipboardManager';
import { ClipboardContent, ClipboardChangeCallback, ClipboardMonitorOptions } from '@/types';
import { setTimer, clearTimer } from 'native-timer';

const LAST_CLIPBOARD_HASH_KEY = '@last_clipboard_hash';

interface PersistedClipboardHash {
  localClipboardHash?: string;
  profileHash?: string;
  type?: string;
}

/**
 * 剪贴板监听器类
 */
export class ClipboardMonitor {
  private clipboardManager: ClipboardManager;
  private callbacks: Set<ClipboardChangeCallback> = new Set();
  private isMonitoring: boolean = false;
  private pollingTimerTag: string | null = null;
  private appStateSubscription: ReturnType<typeof AppState.addEventListener> | null = null;
  private lastContent: ClipboardContent | null = null;

  // 配置选项
  private options: Required<ClipboardMonitorOptions> = {
    pollingInterval: 1000, // iOS 默认 1 秒轮询
    stopOnBackground: true,
    debounceDelay: 300,
  };

  private debounceTimerTag: string | null = null;
  private static readonly DEBOUNCE_TIMER_TAG = 'clipboard_monitor_debounce';
  private isChecking: boolean = false;
  private checkGeneration: number = 0;

  constructor(clipboardManager: ClipboardManager, options?: ClipboardMonitorOptions) {
    this.clipboardManager = clipboardManager;

    if (options) {
      this.options = { ...this.options, ...options };
    }
  }

  /**
   * 从 AsyncStorage 加载持久化的 hash
   */
  private async loadPersistedHash(): Promise<void> {
    try {
      const stored = await AsyncStorage.getItem(LAST_CLIPBOARD_HASH_KEY);
      if (stored) {
        const parsed: PersistedClipboardHash = JSON.parse(stored);
        if (parsed.localClipboardHash || parsed.profileHash) {
          this.lastContent = {
            type: (parsed.type as 'Text' | 'Image' | 'File') || 'Text',
            localClipboardHash: parsed.localClipboardHash,
            profileHash: parsed.profileHash,
          };
        }
      }
    } catch (error) {
      console.error('[ClipboardMonitor] Failed to load persisted hash:', error);
    }
  }

  /**
   * 将 hash 持久化到 AsyncStorage
   */
  private async persistHash(content: ClipboardContent): Promise<void> {
    try {
      const toStore: PersistedClipboardHash = {
        localClipboardHash: content.localClipboardHash,
        profileHash: content.profileHash,
        type: content.type,
      };
      await AsyncStorage.setItem(LAST_CLIPBOARD_HASH_KEY, JSON.stringify(toStore));
    } catch (error) {
      console.error('[ClipboardMonitor] Failed to persist hash:', error);
    }
  }

  /**
   * 开始监听剪贴板变化
   */
  async start(): Promise<void> {
    if (this.isMonitoring) {
      console.warn('[ClipboardMonitor] Already monitoring');
      return;
    }

    this.isMonitoring = true;

    // 从 AsyncStorage 加载持久化的 hash
    await this.loadPersistedHash();

    // 监听应用状态变化
    if (this.options.stopOnBackground) {
      this.appStateSubscription = AppState.addEventListener('change', this.handleAppStateChange);
    }

    // 开始轮询（iOS）或设置监听器（Android）
    if (Platform.OS === 'ios') {
      this.startPolling();
    } else if (Platform.OS === 'android') {
      this.startPolling(); // Android 也使用轮询作为备选方案
      // TODO: 实现原生 Android ClipboardManager 监听器
    }

    console.log('[ClipboardMonitor] Started monitoring');
  }

  /**
   * 停止监听剪贴板变化
   */
  stop(): void {
    if (!this.isMonitoring) {
      return;
    }

    this.isMonitoring = false;

    // 停止轮询
    this.stopPolling();

    // 取消应用状态监听
    if (this.appStateSubscription) {
      this.appStateSubscription.remove();
      this.appStateSubscription = null;
    }

    // 清除防抖计时器
    if (this.debounceTimerTag) {
      clearTimer(this.debounceTimerTag);
      this.debounceTimerTag = null;
    }

    console.log('[ClipboardMonitor] Stopped monitoring');
  }

  /**
   * 添加剪贴板变化回调
   */
  addCallback(callback: ClipboardChangeCallback): void {
    this.callbacks.add(callback);
  }

  /**
   * 移除剪贴板变化回调
   */
  removeCallback(callback: ClipboardChangeCallback): void {
    this.callbacks.delete(callback);
  }

  /**
   * 清除所有回调
   */
  clearCallbacks(): void {
    this.callbacks.clear();
  }

  /**
   * 检查是否正在监听
   */
  isActive(): boolean {
    return this.isMonitoring;
  }

  /**
   * 开始轮询
   */
  private startPolling(): void {
    this.stopPolling(); // 先停止现有轮询

    this.pollingTimerTag = setTimer(
      () => this.checkClipboard(),
      this.options.pollingInterval,
      'clipboard_monitor'
    );
  }

  /**
   * 停止轮询
   */
  private stopPolling(): void {
    if (this.pollingTimerTag) {
      clearTimer(this.pollingTimerTag);
      this.pollingTimerTag = null;
    }
  }

  /**
   * 检查剪贴板内容
   */
  private async checkClipboard(): Promise<void> {
    // 互斥锁：如果上一次检查还在进行中（大图片 hash 计算耗时），跳过本次
    if (this.isChecking) return;
    this.isChecking = true;
    const gen = this.checkGeneration;
    try {
      const content = await this.clipboardManager.getClipboardContent();

      // 如果在 getClipboardContent 期间 setLastContent 被调用，丢弃本次结果
      if (gen !== this.checkGeneration) return;

      if (!content) {
        // console.log('[ClipboardMonitor] Poll: clipboard is empty');
        return;
      }

      // 检查内容是否发生变化
      if (this.hasContentChanged(content)) {
        this.lastContent = content;
        // 持久化 hash
        await this.persistHash(content);
        this.notifyCallbacks(content);
      }
    } catch (error) {
      console.error('[ClipboardMonitor] Failed to check clipboard:', error);
    } finally {
      this.isChecking = false;
    }
  }

  /**
   * 检查内容是否发生变化
   */
  private hasContentChanged(newContent: ClipboardContent): boolean {
    if (!this.lastContent) {
      return true;
    }

    // 优先使用 localClipboardHash 比较（用于本地变化检测）
    if (newContent.localClipboardHash && this.lastContent.localClipboardHash) {
      return newContent.localClipboardHash !== this.lastContent.localClipboardHash;
    }

    // 回退到 profileHash 比较
    if (newContent.profileHash && this.lastContent.profileHash) {
      return newContent.profileHash !== this.lastContent.profileHash;
    }

    // 比较类型和文本
    if (newContent.type !== this.lastContent.type) {
      return true;
    }

    if (newContent.text !== this.lastContent.text) {
      return true;
    }

    return false;
  }

  /**
   * 通知所有回调（带防抖）
   * 使用 native-timer 替代 JS setTimeout，确保 Android 后台也能可靠触发
   */
  private notifyCallbacks(content: ClipboardContent): void {
    // 清除现有防抖计时器
    if (this.debounceTimerTag) {
      clearTimer(this.debounceTimerTag);
      this.debounceTimerTag = null;
    }

    // 使用 native-timer 设置防抖（native-timer 是 interval 模式，回调后立即清除实现 one-shot）
    this.debounceTimerTag = setTimer(
      () => {
        // 立即清除，实现 one-shot 防抖
        if (this.debounceTimerTag) {
          clearTimer(this.debounceTimerTag);
          this.debounceTimerTag = null;
        }
        this.callbacks.forEach((callback) => {
          try {
            callback(content);
          } catch (error) {
            console.error('[ClipboardMonitor] Callback error:', error);
          }
        });
      },
      this.options.debounceDelay,
      ClipboardMonitor.DEBOUNCE_TIMER_TAG
    );
  }

  /**
   * 处理应用状态变化
   */
  private handleAppStateChange = (nextAppState: AppStateStatus): void => {
    if (!this.options.stopOnBackground) {
      return;
    }

    if (nextAppState === 'active') {
      // 应用进入前台，立即检查一次剪贴板（减少等待第一次轮询的延迟）
      // 再重启轮询计时器
      if (this.isMonitoring) {
        void this.checkClipboard();
        if (!this.pollingTimerTag) {
          this.startPolling();
        }
      }
    } else if (nextAppState === 'background' || nextAppState === 'inactive') {
      // 后台上传启用时不停止轮询
      const { useSettingsStore } = require('@/stores/settingsStore');
      const bgUploadEnabled =
        useSettingsStore.getState().config?.enableBackgroundTasks &&
        useSettingsStore.getState().config?.enableBackgroundUpload;
      if (!bgUploadEnabled) {
        // 应用进入后台，停止监听
        console.log(
          '[ClipboardMonitor] Background upload disabled, stopping polling (app went to background/inactive)'
        );
        this.stopPolling();
      }
    }
  };

  /**
   * 手动触发一次检查
   */
  async triggerCheck(): Promise<void> {
    await this.checkClipboard();
  }

  /**
   * 检查内容是否变化，如果变化则更新 lastContent 并持久化
   * @param content 要检查的内容
   * @returns 是否发生变化
   */
  async checkAndUpdateLastContent(content: ClipboardContent): Promise<boolean> {
    const changed = this.hasContentChanged(content);
    // 无论是否变化，都更新 lastContent 为完整内容
    this.lastContent = content;
    if (changed) {
      await this.persistHash(content);
    }
    return changed;
  }

  /**
   * 手动更新上次已知内容，防止监听器将外部设置的剪贴板内容误判为用户新复制
   */
  async setLastContent(content: ClipboardContent): Promise<void> {
    this.checkGeneration++; // 使正在进行的 checkClipboard 结果失效
    this.lastContent = content;
    await this.persistHash(content);
  }

  /**
   * 临时暂停轮询计时器，不改变 isMonitoring 状态。
   * 用于"程序内写入剪贴板"期间防止监听器误触发，配合 resumePolling 使用。
   */
  pausePolling(): void {
    this.stopPolling();
  }

  /**
   * 恢复被 pausePolling 暂停的轮询计时器。
   * 会重置计时器间隔，下次轮询从调用此方法起重新计时。
   * 后台且后台上传未启用时，不恢复轮询（避免后台写入剪贴板后误重启轮询）。
   */
  resumePolling(): void {
    if (!this.isMonitoring) return;

    // 如果配置了后台停止，且当前在后台且后台上传未启用，则不恢复轮询
    if (this.options.stopOnBackground) {
      const currentState = AppState.currentState;
      if (currentState === 'background' || currentState === 'inactive') {
        const { useSettingsStore } = require('@/stores/settingsStore');
        const config = useSettingsStore.getState().config;
        const bgUploadEnabled = config?.enableBackgroundTasks && config?.enableBackgroundUpload;
        if (!bgUploadEnabled) {
          return;
        }
      }
    }

    this.startPolling();
  }

  /**
   * 更新轮询间隔
   * 如果正在监听，会重新启动轮询计时器
   */
  updatePollingInterval(interval: number): void {
    this.options.pollingInterval = interval;
    if (this.isMonitoring && this.pollingTimerTag) {
      this.startPolling();
    }
  }

  /**
   * 获取当前轮询间隔
   */
  getPollingInterval(): number {
    return this.options.pollingInterval;
  }

  /**
   * 重置监听器状态
   */
  async reset(): Promise<void> {
    this.lastContent = null;
    this.clipboardManager.resetLastProfileHash();
    try {
      await AsyncStorage.removeItem(LAST_CLIPBOARD_HASH_KEY);
    } catch (error) {
      console.error('[ClipboardMonitor] Failed to clear persisted hash:', error);
    }
  }
}

// 创建默认实例
import { clipboardManager } from './ClipboardManager';
export const clipboardMonitor = new ClipboardMonitor(clipboardManager);

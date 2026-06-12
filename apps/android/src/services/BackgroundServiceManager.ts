/**
 * BackgroundServiceManager
 * 统一管理所有 JS 侧后台服务的生命周期。
 *
 * 负责管理：
 * - ClipboardSyncService（远程同步、SignalR/轮询、SyncManager、自动上传/下载）
 * - 前台服务（常驻通知）
 * - 短信验证码服务
 * - 剪贴板监控（startMonitoring）
 * - 统计心跳
 * - 通知栏停止/临时停止监听
 *
 * 被 ServiceRestartApp、QuickActionApp、App（main）调用。
 * HomeScreen 不负责后台服务的启动与停止。
 */

import { Platform } from 'react-native';

class BackgroundServiceManager {
  private static instance: BackgroundServiceManager | null = null;

  private running = false;
  private heartbeatTag: string | null = null;
  private stopSub: { remove(): void } | null = null;
  private tempStopSub: { remove(): void } | null = null;
  /** 取消对 settingsStore 的订阅 */
  private settingsUnsub: (() => void) | null = null;

  private constructor() {}

  static getInstance(): BackgroundServiceManager {
    if (!BackgroundServiceManager.instance) {
      BackgroundServiceManager.instance = new BackgroundServiceManager();
    }
    return BackgroundServiceManager.instance;
  }

  // ─── 工具 ───────────────────────────────────────────────

  private getShouldRunBackground(): boolean {
    const { useSettingsStore } = require('../stores/settingsStore');
    const state = useSettingsStore.getState();
    const config = state.config;
    const tempDisabled = state.isTempDisabledBackgroundTasks;
    return (
      !tempDisabled &&
      !!config?.enableBackgroundTasks &&
      !!(config?.enableBackgroundDownload || config?.enableBackgroundUpload)
    );
  }

  /**
   * 更新静态短信接收器状态。
   * SMS 转发不受后台任务总开关控制，仅由 enableSmsForwarding 决定。
   */
  private _updateSmsReceiver(): void {
    try {
      const { useSettingsStore } = require('../stores/settingsStore');
      const config = useSettingsStore.getState().config;
      const { setStaticReceiverEnabled } = require('sms-forwarder');
      setStaticReceiverEnabled(!!config?.enableSmsForwarding);
    } catch (e) {
      console.error('[BackgroundServiceManager] Failed to toggle SMS receiver:', e);
    }
  }

  // ─── 公开 API ─────────────────────────────────────────────

  /**
   * 后台 SignalR 是否正在运行（委托给 ClipboardSyncService）。
   * @deprecated 直接使用 getClipboardSyncService().isSignalRRunning()
   */
  isSignalRRunning(): boolean {
    try {
      const { getClipboardSyncService } = require('./ClipboardSyncService');
      return getClipboardSyncService().isSignalRRunning();
    } catch {
      return false;
    }
  }

  /**
   * 启动所有服务（幂等）。
   * 由任意 Activity 入口调用。
   * - 始终启动剪贴板监控（前台 UI 需要）
   * - 始终启动 ClipboardSyncService（前台 UI + 后台同步）
   * - 仅在后台任务启用时才启动前台通知和心跳
   * - 始终订阅配置变化以支持动态重启
   */
  async start(): Promise<void> {
    // 等待配置加载完成
    const { useSettingsStore } = require('../stores/settingsStore');
    if (!useSettingsStore.getState().isLoaded) {
      await useSettingsStore.getState().loadConfig();
    }

    // SMS 转发始终独立管理（Android 专属）
    if (Platform.OS === 'android') {
      this._updateSmsReceiver();
    }

    // 始终启动剪贴板监控（无论是否启用后台任务，UI 需要感知本地剪贴板变化）
    try {
      const { useClipboardStore } = require('../stores');
      await useClipboardStore.getState().startMonitoring();
    } catch (e) {
      console.error('[BackgroundServiceManager] Failed to start clipboard monitoring:', e);
    }

    // 始终启动 ClipboardSyncService（前台 UI + 后台同步）
    await this._startRemoteSync();

    // 后台专用服务（前台通知 + 心跳，Android 专属）
    if (Platform.OS === 'android') {
      if (this.getShouldRunBackground()) {
        if (!this.running) {
          this.running = true;
          await this._startBackgroundOnlyServices();
        }
      } else {
        await this._stopBackgroundOnlyServices();
      }
    }

    // 始终订阅配置变化（不再因 getShouldRunBackground() 为 false 而跳过）
    this._subscribeToConfigChanges();
  }

  /**
   * 停止后台专用服务（前台通知、心跳）。
   * 注意：ClipboardSyncService 不在此处停止，由 refresh() 统一管理。
   */
  async stop(): Promise<void> {
    await this._stopBackgroundOnlyServices();
  }

  /**
   * 配置变化时重新评估所有服务状态（由内部订阅自动触发）。
   */
  async refresh(): Promise<void> {
    // SMS 转发（Android 专属）
    if (Platform.OS === 'android') {
      this._updateSmsReceiver();
    }

    // 刷新远程同步服务（处理服务器变更、连接类型切换等）
    await this._startRemoteSync();

    // 后台专用服务（Android 专属）
    if (Platform.OS === 'android') {
      if (this.getShouldRunBackground()) {
        if (!this.running) {
          this.running = true;
          await this._startBackgroundOnlyServices();
        } else {
          await this._updateBackgroundOnlyServices();
        }
      } else {
        await this._stopBackgroundOnlyServices();
      }
    }
  }

  // ─── 私有实现 ─────────────────────────────────────────────

  /** 启动/刷新 ClipboardSyncService */
  private async _startRemoteSync(): Promise<void> {
    try {
      const { getClipboardSyncService } = require('./ClipboardSyncService');
      await getClipboardSyncService().refresh();
    } catch (e) {
      console.error('[BackgroundServiceManager] Failed to start/refresh remote sync:', e);
    }
  }

  /** 启动后台专用服务（前台通知、心跳、剪贴板监控） */
  private async _startBackgroundOnlyServices(): Promise<void> {
    const { useSettingsStore } = require('../stores/settingsStore');
    const config = useSettingsStore.getState().config;

    // 1. 按需启动前台常驻通知服务
    if (config?.enableForegroundNotification) {
      try {
        const ForegroundService = require('foreground-service');
        ForegroundService.startService();

        this.stopSub = ForegroundService.addStopListener(() => {
          useSettingsStore.getState().setEnableBackgroundTasks(false);
        });
        this.tempStopSub = ForegroundService.addTempStopListener(() => {
          useSettingsStore.getState().setTempDisabledBackgroundTasks(true);
        });
      } catch (e) {
        console.error('[BackgroundServiceManager] Failed to start foreground service:', e);
      }
    }

    // 2. 统计心跳
    try {
      const { useStatisticsStore } = require('../stores/statisticsStore');
      await useStatisticsStore.getState().recordBackgroundTaskStart();

      const { setTimer: st } = require('native-timer');
      this.heartbeatTag = st(() => {
        useStatisticsStore.getState().updateHeartbeat();
      }, 60_000);
    } catch (e) {
      console.error('[BackgroundServiceManager] Failed to start statistics/heartbeat:', e);
    }

    console.log('[BackgroundServiceManager] Background-only services started');
  }

  /** 更新后台专用服务（配置变化时调用） */
  private async _updateBackgroundOnlyServices(): Promise<void> {
    const { useSettingsStore } = require('../stores/settingsStore');
    const config = useSettingsStore.getState().config;

    try {
      const ForegroundService = require('foreground-service');
      const isRunning = ForegroundService.isRunning();
      if (config?.enableForegroundNotification && !isRunning) {
        ForegroundService.startService();
        this.stopSub = ForegroundService.addStopListener(() => {
          useSettingsStore.getState().setEnableBackgroundTasks(false);
        });
        this.tempStopSub = ForegroundService.addTempStopListener(() => {
          useSettingsStore.getState().setTempDisabledBackgroundTasks(true);
        });
      } else if (!config?.enableForegroundNotification && isRunning) {
        this._cleanupListeners();
        ForegroundService.stopService();
      }
    } catch (e) {
      console.error('[BackgroundServiceManager] Failed to update foreground service:', e);
    }
  }

  /** 停止后台专用服务 */
  private async _stopBackgroundOnlyServices(): Promise<void> {
    if (!this.running) return;
    this.running = false;

    this._cleanupListeners();

    if (this.heartbeatTag) {
      try {
        const { clearTimer } = require('native-timer');
        clearTimer(this.heartbeatTag);
      } catch {}
      this.heartbeatTag = null;
    }

    try {
      const ForegroundService = require('foreground-service');
      ForegroundService.stopService();
    } catch {}
  }

  private _cleanupListeners(): void {
    this.stopSub?.remove();
    this.tempStopSub?.remove();
    this.stopSub = null;
    this.tempStopSub = null;
  }

  private _subscribeToConfigChanges(): void {
    if (this.settingsUnsub) return;
    const { useSettingsStore } = require('../stores/settingsStore');
    this.settingsUnsub = useSettingsStore.subscribe(
      (
        state: { config: unknown; isTempDisabledBackgroundTasks: boolean },
        prevState: { config: unknown; isTempDisabledBackgroundTasks: boolean }
      ) => {
        if (
          state.config !== prevState.config ||
          state.isTempDisabledBackgroundTasks !== prevState.isTempDisabledBackgroundTasks
        ) {
          this.refresh().catch((e) =>
            console.error('[BackgroundServiceManager] refresh failed:', e)
          );
        }
      }
    );
  }
}

export function getBackgroundServiceManager(): BackgroundServiceManager {
  return BackgroundServiceManager.getInstance();
}

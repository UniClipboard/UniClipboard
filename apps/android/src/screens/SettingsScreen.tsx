/**
 * 设置页面
 * 提供主题切换功能、服务器配置、多用户切换
 */

import React, { useEffect, useState, useRef } from 'react';
import {
  View,
  Text,
  StyleSheet,
  TouchableOpacity,
  ScrollView,
  Switch,
  TextInput,
  Alert,
  Linking,
  Platform,
  Modal,
} from 'react-native';
import { APP_VERSION } from '@/constants';
import { spacing, radius, typography, elevation, PALETTES } from '@/theme';
import { Paths, Directory } from 'expo-file-system';
import { calculateDirectorySize, clearDirectory } from '@/utils/fileStorage';
import { CLIPBOARD_TEMP_DIR } from '@/utils/fileStorage';
import { SafeAreaView } from 'react-native-safe-area-context';
import { useTheme } from '@/hooks/useTheme';
import type { ThemeMode, PaletteId } from '@/theme';
import { useSettingsStore, usePendingConnectStore } from '@/stores';
import { ServerConfigModal, ServerListItem, MessageToast, QrScannerModal } from '@/components';
import { ServerConfig } from '@/types/api';
import { useMessageToast } from '@/hooks/useMessageToast';
import {
  ShortcutService,
  checkForUpdate,
  calculateLogSize,
  clearLogs,
  saveLogsToFile,
  setLogLevel as setLoggerLogLevel,
  type LogLevel,
  getPreferredAbi,
  findAssetForAbi,
  checkApkCache,
  downloadApk,
  installApk,
  cleanOldApkCache,
  type ReleaseAssetInfo,
  type ApkSource,
} from '@/services';
import { Plus, RefreshCw, Check, ChevronDown, ChevronUp } from 'react-native-feather';
import { hasOverlayPermission, requestOverlayPermission } from 'clipboard-overlay';
import {
  isShizukuAvailable,
  hasShizukuPermission,
  requestShizukuPermission,
} from 'shizuku-clipboard';
import { extractVerificationCode } from '@/tasks/SmsUploadTask';

export const SettingsScreen = () => {
  const { theme, themeMode, setThemeMode, paletteId, setPaletteId } = useTheme();
  const {
    config,
    isLoaded,
    loadConfig,
    addServer,
    updateServer,
    deleteServer,
    setActiveServer,
    setAutoSync,
    setAutoDownloadMaxSize,
    updateConfig,
    setAutoCheckUpdate,
    setLastUpdateCheckDate,
    setUpdateToBeta,
    setEnableHistorySync,
    setLogLevel,
    setRemotePollingInterval,
    setLocalPollingInterval,
    setEnableBackgroundDownload,
    setEnableBackgroundUpload,
    setEnableClipboardOverlay,
    setEnableBackgroundTasks,
    setEnableSmsForwarding,
    setEnableShizukuClipboard,
    isTempDisabledBackgroundTasks,
    setTempDisabledBackgroundTasks,
  } = useSettingsStore();

  const [showServerModal, setShowServerModal] = useState(false);
  const [editingServerIndex, setEditingServerIndex] = useState<number | null>(null);
  const [serversCollapsed, setServersCollapsed] = useState(true);
  const [showScannerModal, setShowScannerModal] = useState(false);
  const [prefillFromScan, setPrefillFromScan] = useState<ServerConfig | null>(null);
  const consumePendingConnect = usePendingConnectStore((s) => s.consume);
  const pendingConnectIntent = usePendingConnectStore((s) => s.intent);
  const { message, showMessage, handleMessageShown } = useMessageToast();

  // 本地状态用于跟踪Switch的当前值，避免闪烁
  const [localAutoSyncEnabled, setLocalAutoSyncEnabled] = useState(config?.autoSync ?? false);
  const [localDebugModeEnabled, setLocalDebugModeEnabled] = useState(config?.debugMode ?? false);
  const [localAutoCheckUpdateEnabled, setLocalAutoCheckUpdateEnabled] = useState(
    config?.autoCheckUpdate ?? true
  );
  const [localUpdateToBetaEnabled, setLocalUpdateToBetaEnabled] = useState(
    config?.updateToBeta ?? false
  );
  const [localHistorySyncEnabled, setLocalHistorySyncEnabled] = useState(
    config?.enableHistorySync ?? false
  );
  const [localBackgroundDownloadEnabled, setLocalBackgroundDownloadEnabled] = useState(
    config?.enableBackgroundDownload ?? false
  );
  const [localBackgroundUploadEnabled, setLocalBackgroundUploadEnabled] = useState(
    config?.enableBackgroundUpload ?? false
  );
  const [localBackgroundTasksEnabled, setLocalBackgroundTasksEnabled] = useState(
    (config?.enableBackgroundTasks ?? false) && !isTempDisabledBackgroundTasks
  );
  const [localClipboardOverlayEnabled, setLocalClipboardOverlayEnabled] = useState(
    config?.enableClipboardOverlay ?? false
  );
  const [localShizukuClipboardEnabled, setLocalShizukuClipboardEnabled] = useState(
    config?.enableShizukuClipboard ?? false
  );
  const [localSmsForwardingEnabled, setLocalSmsForwardingEnabled] = useState(
    config?.enableSmsForwarding ?? false
  );
  const [localForegroundNotification, setLocalForegroundNotification] = useState(
    config?.enableForegroundNotification ?? true
  );
  const [localSyncToastEnabled, setLocalSyncToastEnabled] = useState(
    config?.syncToastEnabled ?? true
  );
  const [localDebugOverlayVisible, setLocalDebugOverlayVisible] = useState(
    config?.debugOverlayVisible ?? false
  );
  const [localDebugUrlScheme, setLocalDebugUrlScheme] = useState(config?.debugUrlScheme ?? false);
  const [localDebugUpdateCheckNoLimit, setLocalDebugUpdateCheckNoLimit] = useState(
    config?.debugUpdateCheckNoLimit ?? false
  );
  const [showSmsTestModal, setShowSmsTestModal] = useState(false);
  const [smsTestInput, setSmsTestInput] = useState('');
  const [showLogLevelMenu, setShowLogLevelMenu] = useState(false);
  const [localHideFromRecents, setLocalHideFromRecents] = useState(
    config?.hideFromRecents ?? false
  );
  const [showStatsModal, setShowStatsModal] = useState(false);
  const [showImageAutoDownloadMenu, setShowImageAutoDownloadMenu] = useState(false);
  const [localImageAutoDownload, setLocalImageAutoDownload] = useState<'wifi' | 'always' | 'off'>(
    config?.historyImageAutoDownload ?? 'wifi'
  );
  const [statsText, setStatsText] = useState('');

  // 更新检查状态
  const [isCheckingUpdate, setIsCheckingUpdate] = useState(false);
  const [updateAvailable, setUpdateAvailable] = useState(false);
  const [latestVersion, setLatestVersion] = useState<string | null>(null);
  // APK 下载状态
  const [isDownloading, setIsDownloading] = useState(false);
  const [downloadProgress, setDownloadProgress] = useState(0);
  const downloadAbortRef = useRef<AbortController | null>(null);
  const latestAssetsRef = useRef<ReleaseAssetInfo[]>([]);
  const latestTagRef = useRef<string>('');
  const releaseNotesRef = useRef<string | undefined>(undefined);

  const appVersion = APP_VERSION;

  // 加载配置
  useEffect(() => {
    if (!isLoaded) {
      loadConfig();
    }
  }, [isLoaded, loadConfig]);

  // 当配置中的autoSync值变化时，更新本地状态
  useEffect(() => {
    setLocalAutoSyncEnabled(config?.autoSync ?? false);
  }, [config?.autoSync]);

  // 当配置中的debugMode值变化时，更新本地状态
  useEffect(() => {
    setLocalDebugModeEnabled(config?.debugMode ?? false);
  }, [config?.debugMode]);

  // 当配置中的autoCheckUpdate值变化时，更新本地状态
  useEffect(() => {
    setLocalAutoCheckUpdateEnabled(config?.autoCheckUpdate ?? true);
  }, [config?.autoCheckUpdate]);

  useEffect(() => {
    setLocalUpdateToBetaEnabled(config?.updateToBeta ?? false);
  }, [config?.updateToBeta]);

  useEffect(() => {
    setLocalHistorySyncEnabled(config?.enableHistorySync ?? true);
  }, [config?.enableHistorySync]);

  useEffect(() => {
    setLocalBackgroundDownloadEnabled(config?.enableBackgroundDownload ?? false);
  }, [config?.enableBackgroundDownload]);

  useEffect(() => {
    setLocalBackgroundUploadEnabled(config?.enableBackgroundUpload ?? false);
  }, [config?.enableBackgroundUpload]);

  useEffect(() => {
    setLocalBackgroundTasksEnabled(
      (config?.enableBackgroundTasks ?? false) && !isTempDisabledBackgroundTasks
    );
  }, [config?.enableBackgroundTasks, isTempDisabledBackgroundTasks]);

  useEffect(() => {
    setLocalClipboardOverlayEnabled(config?.enableClipboardOverlay ?? false);
  }, [config?.enableClipboardOverlay]);

  useEffect(() => {
    setLocalShizukuClipboardEnabled(config?.enableShizukuClipboard ?? false);
  }, [config?.enableShizukuClipboard]);

  useEffect(() => {
    setLocalSmsForwardingEnabled(config?.enableSmsForwarding ?? false);
  }, [config?.enableSmsForwarding]);

  useEffect(() => {
    setLocalForegroundNotification(config?.enableForegroundNotification ?? true);
  }, [config?.enableForegroundNotification]);

  useEffect(() => {
    setLocalSyncToastEnabled(config?.syncToastEnabled ?? true);
  }, [config?.syncToastEnabled]);

  useEffect(() => {
    setLocalHideFromRecents(config?.hideFromRecents ?? false);
  }, [config?.hideFromRecents]);

  useEffect(() => {
    setLocalImageAutoDownload(config?.historyImageAutoDownload ?? 'wifi');
  }, [config?.historyImageAutoDownload]);

  // 计算存储大小
  useEffect(() => {
    calculateStorageSizes();
  }, []);

  // 刷新权限状态
  const refreshPermissions = async () => {
    if (Platform.OS !== 'android') return;
    setIsRefreshingPermissions(true);
    try {
      const { PermissionsAndroid } = require('react-native');
      const [notif, sms] = await Promise.all([
        PermissionsAndroid.check(PermissionsAndroid.PERMISSIONS.POST_NOTIFICATIONS),
        PermissionsAndroid.check(PermissionsAndroid.PERMISSIONS.RECEIVE_SMS),
      ]);
      setPermNotification(notif);
      setPermOverlay(hasOverlayPermission());
      setPermSms(sms);
      const { isIgnoringBatteryOptimizations } = await import('native-util');
      setPermBattery(isIgnoringBatteryOptimizations());
      const shizukuUp = isShizukuAvailable();
      setShizukuAvailable(shizukuUp);
      setPermShizuku(shizukuUp && hasShizukuPermission());
    } catch (e) {
      console.warn('[Settings] Failed to check permissions:', e);
    } finally {
      setIsRefreshingPermissions(false);
    }
  };

  useEffect(() => {
    refreshPermissions();
  }, []);

  // 自动检查更新（每天一次）
  useEffect(() => {
    if (!isLoaded) return;
    if (!(config?.autoCheckUpdate ?? true)) return;
    const today = new Date().toISOString().slice(0, 10);
    if (
      !(config?.debugUpdateCheckNoLimit ?? false) &&
      (config?.lastUpdateCheckDate ?? '') === today
    )
      return;
    runUpdateCheck(false, config?.updateToBeta ?? false);
  }, [isLoaded]);

  const themeOptions: { label: string; value: ThemeMode }[] = [
    { label: '跟随系统', value: 'auto' },
    { label: '浅色', value: 'light' },
    { label: '深色', value: 'dark' },
  ];

  const handleSetPaletteId = async (id: PaletteId) => {
    try {
      await setPaletteId(id);
    } catch (error: unknown) {
      showMessage(error instanceof Error ? error.message : '主题色切换失败', 'error');
    }
  };

  const handleSetThemeMode = async (mode: ThemeMode) => {
    try {
      await setThemeMode(mode);
    } catch (error: unknown) {
      showMessage(error instanceof Error ? error.message : '外观模式切换失败', 'error');
    }
  };

  const imageAutoDownloadOptions: { label: string; value: 'wifi' | 'always' | 'off' }[] = [
    { label: '仅 Wi-Fi', value: 'wifi' },
    { label: '总是', value: 'always' },
    { label: '关闭', value: 'off' },
  ];

  // 获取服务器列表
  const servers = config?.servers || [];
  const activeServerIndex = config?.activeServerIndex ?? -1;
  const activeServer = activeServerIndex >= 0 ? servers[activeServerIndex] : null;
  const autoDownloadMaxSizeMB = Math.round(
    (config?.autoDownloadMaxSize ?? 5 * 1024 * 1024) / (1024 * 1024)
  );

  // 本地 state 用于输入框
  const [maxSizeInput, setMaxSizeInput] = useState(autoDownloadMaxSizeMB.toString());
  const [maxHistoryItemsInput, setMaxHistoryItemsInput] = useState(
    (config?.maxHistoryItems ?? 1000).toString()
  );
  const [remotePollingInput, setRemotePollingInput] = useState(
    ((config?.remotePollingInterval ?? 3000) / 1000).toString()
  );
  const [localPollingInput, setLocalPollingInput] = useState(
    ((config?.localPollingInterval ?? 1000) / 1000).toString()
  );

  // 存储大小状态
  const [cacheSize, setCacheSize] = useState<number>(0);
  const [historySize, setHistorySize] = useState<number>(0);
  const [logSize, setLogSize] = useState<number>(0);
  const [isCalculating, setIsCalculating] = useState<boolean>(true);
  const [isExportingLogs, setIsExportingLogs] = useState<boolean>(false);
  const exportLogsAbortControllerRef = useRef<AbortController | null>(null);

  // 权限状态
  const [permNotification, setPermNotification] = useState<boolean>(false);
  const [permOverlay, setPermOverlay] = useState<boolean>(false);
  const [permSms, setPermSms] = useState<boolean>(false);
  const [permBattery, setPermBattery] = useState<boolean>(false);
  const [permShizuku, setPermShizuku] = useState<boolean>(false);
  const [shizukuAvailable, setShizukuAvailable] = useState<boolean>(false);
  const [isRefreshingPermissions, setIsRefreshingPermissions] = useState<boolean>(false);
  const hasBatteryOptRequested = useRef<boolean>(false);

  // 目录对象
  const cacheDir = CLIPBOARD_TEMP_DIR;
  const historyDir = new Directory(Paths.document, 'clipboards', 'history');

  // 调试日志
  useEffect(() => {
    try {
      console.log('Cache directory:', cacheDir.uri);
      console.log('History directory:', historyDir.uri);
      console.log('Cache directory exists:', cacheDir.exists);
      console.log('History directory exists:', historyDir.exists);
    } catch (error) {
      console.error('Error checking directories:', error);
    }
  }, []);

  // 打开手动表单（新建态）
  const openManualAddForm = () => {
    setEditingServerIndex(null);
    setPrefillFromScan(null);
    setShowServerModal(true);
  };

  // 用扫码/深链解析出的凭据预填表单
  const openPrefilledAddForm = (config: ServerConfig) => {
    setEditingServerIndex(null);
    setPrefillFromScan(config);
    setShowServerModal(true);
  };

  // 检查 pendingConnectStore，有数据就打开预填表单
  const tryConsumePendingConnect = () => {
    const intent = consumePendingConnect();
    if (!intent) return false;
    openPrefilledAddForm({
      type: 'syncclipboard',
      url: intent.url,
      username: intent.user,
      password: intent.pwd,
      ...(intent.label ? { name: intent.label } : {}),
    });
    return true;
  };

  // 处理添加服务器 — 让用户选「扫码 / 手动」
  const handleAddServer = () => {
    Alert.alert(
      '添加服务器',
      '选择添加方式',
      [
        {
          text: '扫描二维码',
          onPress: () => setShowScannerModal(true),
        },
        {
          text: '手动填写',
          onPress: openManualAddForm,
        },
        { text: '取消', style: 'cancel' },
      ],
      { cancelable: true }
    );
  };

  // ScannerModal 关闭：只关闭，consume 由下面的 useEffect 统一处理
  const handleScannerClose = () => {
    setShowScannerModal(false);
  };

  // 统一的 consume 时机：pendingIntent 出现且无其它 modal 打开时
  // 覆盖三个来源：1) 扫码 modal 成功扫到后关闭；2) 深链冷启动（intent 在 Settings 挂载前就被 set）；
  // 3) 深链热启动（intent 在 Settings 已挂载时被 set）。
  // 若用户正在编辑/扫码，intent 留在 store 里，等用户关闭当前 modal 后下一帧再处理。
  useEffect(() => {
    if (pendingConnectIntent && !showServerModal && !showScannerModal) {
      tryConsumePendingConnect();
    }
  }, [pendingConnectIntent, showServerModal, showScannerModal]);

  // 处理编辑服务器
  const handleEditServer = (index: number) => {
    setEditingServerIndex(index);
    setShowServerModal(true);
  };

  // 处理保存服务器
  const handleSaveServer = async (serverConfig: ServerConfig) => {
    try {
      if (editingServerIndex !== null) {
        await updateServer(editingServerIndex, serverConfig);
        showMessage('服务器配置已更新', 'success');
      } else {
        await addServer(serverConfig);
        showMessage('服务器已添加', 'success');
      }
    } catch (error: unknown) {
      showMessage(error instanceof Error ? error.message : '操作失败', 'error');
    }
  };

  // 处理删除服务器
  const handleDeleteServer = async (index: number) => {
    try {
      await deleteServer(index);
      showMessage('服务器已删除', 'success');
    } catch (error: unknown) {
      showMessage(error instanceof Error ? error.message : '删除失败', 'error');
    }
  };

  // 处理切换激活服务器
  const handleSetActiveServer = async (index: number) => {
    if (index === activeServerIndex) {
      if (servers.length > 1) {
        setServersCollapsed(true);
      }
      return;
    }

    if (servers.length > 1) {
      setServersCollapsed(true);
    }

    try {
      const { getHistorySyncService } = await import('@/services/HistorySyncService');
      const syncService = getHistorySyncService();
      syncService.cancelAll();
    } catch {
      // ignore
    }

    try {
      await setActiveServer(index);
      await updateConfig({ needsHistoryReorganize: true });
      showMessage('已切换服务器', 'success');
    } catch (error: unknown) {
      showMessage(error instanceof Error ? error.message : '切换失败', 'error');
    }
  };

  // 处理切换自动复制
  const handleToggleAutoSync = async (enabled: boolean) => {
    // 立即更新本地状态，避免闪烁
    setLocalAutoSyncEnabled(enabled);

    try {
      await setAutoSync(enabled);
      showMessage(enabled ? '已启用自动复制' : '已禁用自动复制', 'success');
    } catch (error: unknown) {
      // 如果设置失败，恢复原来的状态
      setLocalAutoSyncEnabled(!enabled);
      showMessage(error instanceof Error ? error.message : '设置失败', 'error');
    }
  };

  // 处理切换后台任务总开关
  const handleToggleBackgroundTasks = async (enabled: boolean) => {
    if (enabled) {
      // 如果是临时停止状态，直接清除标志，不需要弹窗确认
      if (isTempDisabledBackgroundTasks) {
        setLocalBackgroundTasksEnabled(true);
        setTempDisabledBackgroundTasks(false);
        showMessage('已恢复后台任务', 'success');
        return;
      }
      Alert.alert(
        '开启后台任务',
        '启用后台任务后，应用将在后台持续运行相关服务，大幅增加电量消耗，强烈建议按需开启。\n\n如有需要，可以在系统设置中将 UniClip 的电池优化设为「不受限制」，并在多任务界面锁定 UniClip，减少系统关闭后台任务的概率。',
        [
          { text: '取消', style: 'cancel' },
          {
            text: '确认开启',
            onPress: async () => {
              setLocalBackgroundTasksEnabled(true);
              try {
                await setEnableBackgroundTasks(true);
                showMessage('已启用后台任务', 'success');
              } catch (error: unknown) {
                setLocalBackgroundTasksEnabled(false);
                showMessage(error instanceof Error ? error.message : '设置失败', 'error');
              }
            },
          },
        ]
      );
      return;
    }

    setLocalBackgroundTasksEnabled(false);
    try {
      await setEnableBackgroundTasks(false);
      showMessage('已禁用后台任务', 'success');
    } catch (error: unknown) {
      setLocalBackgroundTasksEnabled(true);
      showMessage(error instanceof Error ? error.message : '设置失败', 'error');
    }
  };

  // 处理切换后台下载远程
  const handleToggleBackgroundDownload = async (enabled: boolean) => {
    if (enabled) {
      setLocalBackgroundDownloadEnabled(true);
      try {
        await setEnableBackgroundDownload(true);
        showMessage('已启用后台下载远程', 'success');
      } catch (error: unknown) {
        setLocalBackgroundDownloadEnabled(false);
        showMessage(error instanceof Error ? error.message : '设置失败', 'error');
      }
      return;
    }

    setLocalBackgroundDownloadEnabled(false);
    try {
      await setEnableBackgroundDownload(false);
      showMessage('已禁用后台下载远程', 'success');
    } catch (error: unknown) {
      setLocalBackgroundDownloadEnabled(true);
      showMessage(error instanceof Error ? error.message : '设置失败', 'error');
    }
  };

  // 处理切换后台上传本地
  const handleToggleBackgroundUpload = async (enabled: boolean) => {
    if (enabled) {
      Alert.alert(
        '开启后台上传本地剪贴板',
        '无需启用此选项，UniClip 也支持从选中文字弹出的菜单直接上传文字。\n\nAndroid 10 及以上的系统，应用在后台无法直接获取本地剪贴板内容，你可能需要启用悬浮窗或使用其他工具绕过此限制。',
        [
          { text: '取消', style: 'cancel' },
          {
            text: '确认开启',
            onPress: async () => {
              setLocalBackgroundUploadEnabled(true);
              try {
                await setEnableBackgroundUpload(true);
                showMessage('已启用后台上传本地', 'success');
              } catch (error: unknown) {
                setLocalBackgroundUploadEnabled(false);
                showMessage(error instanceof Error ? error.message : '设置失败', 'error');
              }
            },
          },
        ]
      );
      return;
    }

    setLocalBackgroundUploadEnabled(false);
    try {
      await setEnableBackgroundUpload(false);
      showMessage('已禁用后台上传本地', 'success');
    } catch (error: unknown) {
      setLocalBackgroundUploadEnabled(true);
      showMessage(error instanceof Error ? error.message : '设置失败', 'error');
    }
  };

  // 处理切换悬浮窗获取剪贴板
  const handleToggleClipboardOverlay = async (enabled: boolean) => {
    if (enabled && Platform.OS === 'android') {
      Alert.alert(
        '启用悬浮窗获取剪贴板',
        '启用后，应用将通过不可见的悬浮窗在后台获取剪贴板内容。这可能导致部分应用因焦点问题产生功能异常以及其他问题。\n\n如果您可以通过其他工具授予 UniClip 后台读取剪贴板的权限，建议关闭此选项。',
        [
          { text: '取消', style: 'cancel' },
          {
            text: '确定',
            onPress: async () => {
              if (!hasOverlayPermission()) {
                requestOverlayPermission();
                return;
              }
              setLocalClipboardOverlayEnabled(true);
              try {
                await setEnableClipboardOverlay(true);
                showMessage('已启用悬浮窗获取剪贴板', 'success');
              } catch (error: unknown) {
                setLocalClipboardOverlayEnabled(false);
                showMessage(error instanceof Error ? error.message : '设置失败', 'error');
              }
            },
          },
        ]
      );
      return;
    }

    setLocalClipboardOverlayEnabled(enabled);

    try {
      await setEnableClipboardOverlay(enabled);
      showMessage(enabled ? '已启用悬浮窗获取剪贴板' : '已禁用悬浮窗获取剪贴板', 'success');
    } catch (error: unknown) {
      setLocalClipboardOverlayEnabled(!enabled);
      showMessage(error instanceof Error ? error.message : '设置失败', 'error');
    }
  };

  // 处理切换 Shizuku 获取剪贴板
  const handleToggleShizukuClipboard = async (enabled: boolean) => {
    if (enabled && Platform.OS === 'android') {
      // 检查 Shizuku 是否可用
      if (!isShizukuAvailable()) {
        Alert.alert(
          'Shizuku 未运行',
          '请先安装并启动 Shizuku。\n\n非 Root 设备每次重启后需重新启动 Shizuku（Android 11+ 可通过无线调试自行启动）。',
          [
            { text: '取消', style: 'cancel' },
            {
              text: '了解更多',
              onPress: () => Linking.openURL('https://shizuku.rikka.app/guide/setup/'),
            },
          ]
        );
        return;
      }

      // 检查 Shizuku 权限
      if (!hasShizukuPermission()) {
        const requested = requestShizukuPermission();
        if (!requested) {
          Alert.alert('权限请求失败', '无法请求 Shizuku 权限，请确认 Shizuku 版本支持。');
          return;
        }
        showMessage('请在 Shizuku 弹窗中授予权限后重新启用', 'info');
        return;
      }

      setLocalShizukuClipboardEnabled(true);
      try {
        // 启用 Shizuku 时自动关闭悬浮窗方式
        if (localClipboardOverlayEnabled) {
          setLocalClipboardOverlayEnabled(false);
          await setEnableClipboardOverlay(false);
        }
        await setEnableShizukuClipboard(true);
        showMessage('已启用 Shizuku 获取剪贴板', 'success');
      } catch (error: unknown) {
        setLocalShizukuClipboardEnabled(false);
        showMessage(error instanceof Error ? error.message : '设置失败', 'error');
      }
      return;
    }

    setLocalShizukuClipboardEnabled(enabled);
    try {
      await setEnableShizukuClipboard(enabled);
      showMessage(enabled ? '已启用 Shizuku 获取剪贴板' : '已禁用 Shizuku 获取剪贴板', 'success');
    } catch (error: unknown) {
      setLocalShizukuClipboardEnabled(!enabled);
      showMessage(error instanceof Error ? error.message : '设置失败', 'error');
    }
  };

  // 处理切换自动上传短信验证码
  const handleToggleSmsForwarding = async (enabled: boolean) => {
    if (enabled && Platform.OS === 'android') {
      const { PermissionsAndroid } = require('react-native');
      const granted = await PermissionsAndroid.check(PermissionsAndroid.PERMISSIONS.RECEIVE_SMS);
      if (!granted) {
        const result = await PermissionsAndroid.request(PermissionsAndroid.PERMISSIONS.RECEIVE_SMS);
        if (result !== PermissionsAndroid.RESULTS.GRANTED) {
          Alert.alert('需要短信权限', '自动上传验证码需要短信接收权限，请在系统设置中允许', [
            { text: '取消', style: 'cancel' },
            { text: '前往设置', onPress: () => Linking.openSettings() },
          ]);
          return;
        }
      }
    }

    setLocalSmsForwardingEnabled(enabled);
    try {
      await setEnableSmsForwarding(enabled);
      // 同步静态短信接收器状态
      if (Platform.OS === 'android') {
        const { setStaticReceiverEnabled } = await import('sms-forwarder');
        setStaticReceiverEnabled(enabled);
      }
      showMessage(enabled ? '已启用自动上传短信验证码' : '已禁用自动上传短信验证码', 'success');
    } catch (error: unknown) {
      setLocalSmsForwardingEnabled(!enabled);
      showMessage(error instanceof Error ? error.message : '设置失败', 'error');
    }
  };

  // 处理切换前台服务常驻通知
  const handleToggleForegroundNotification = async (enabled: boolean) => {
    if (!enabled) {
      Alert.alert(
        '关闭常驻通知',
        '关闭常驻通知会降低后台服务稳定性，系统终止后台任务的可能性增大。',
        [
          { text: '取消', style: 'cancel' },
          {
            text: '确认关闭',
            onPress: async () => {
              setLocalForegroundNotification(false);
              try {
                await updateConfig({ enableForegroundNotification: false });
              } catch (error: unknown) {
                setLocalForegroundNotification(true);
                showMessage(error instanceof Error ? error.message : '设置失败', 'error');
              }
            },
          },
        ]
      );
      return;
    }

    setLocalForegroundNotification(true);
    try {
      await updateConfig({ enableForegroundNotification: true });
      // 检查通知权限，提示但不阻止
      if (Platform.OS === 'android') {
        const { PermissionsAndroid } = require('react-native');
        const granted = await PermissionsAndroid.check(
          PermissionsAndroid.PERMISSIONS.POST_NOTIFICATIONS
        );
        if (!granted) {
          Alert.alert(
            '缺少通知权限',
            '未授予通知权限，常驻通知可能无法显示。建议前往系统设置允许通知权限。',
            [
              { text: '稍后再说', style: 'cancel' },
              { text: '前往设置', onPress: () => Linking.openSettings() },
            ]
          );
        }
      }
    } catch (error: unknown) {
      setLocalForegroundNotification(false);
      showMessage(error instanceof Error ? error.message : '设置失败', 'error');
    }
  };

  // 处理最大文件大小输入
  const handleMaxSizeBlur = async () => {
    try {
      const sizeMB = parseInt(maxSizeInput, 10);
      if (isNaN(sizeMB) || sizeMB < 0) {
        setMaxSizeInput(autoDownloadMaxSizeMB.toString());
        showMessage('请输入有效的数字', 'error');
        return;
      }
      const sizeInBytes = sizeMB * 1024 * 1024;
      await setAutoDownloadMaxSize(sizeInBytes);
      showMessage(`已设置最大文件大小为 ${sizeMB}MB`, 'success');
    } catch (error: unknown) {
      setMaxSizeInput(autoDownloadMaxSizeMB.toString());
      showMessage(error instanceof Error ? error.message : '设置失败', 'error');
    }
  };

  // 处理历史记录最大保留条数输入
  const handleMaxHistoryItemsBlur = async () => {
    try {
      const maxItems = parseInt(maxHistoryItemsInput, 10);
      if (isNaN(maxItems) || maxItems < 10) {
        setMaxHistoryItemsInput((config?.maxHistoryItems ?? 1000).toString());
        showMessage('请输入大于等于10的数字', 'error');
        return;
      }
      await updateConfig({ maxHistoryItems: maxItems });
      showMessage(`已设置历史记录最大保留条数为 ${maxItems}条`, 'success');

      // 更新历史记录存储的最大大小
      const { historyStorage } = await import('@/services');
      historyStorage.setMaxHistorySize(maxItems);
    } catch (error: unknown) {
      setMaxHistoryItemsInput((config?.maxHistoryItems ?? 1000).toString());
      showMessage(error instanceof Error ? error.message : '设置失败', 'error');
    }
  };

  // 处理远程轮询间隔输入
  const handleRemotePollingBlur = async () => {
    try {
      const seconds = parseInt(remotePollingInput, 10);
      if (isNaN(seconds) || seconds < 1) {
        setRemotePollingInput(((config?.remotePollingInterval ?? 3000) / 1000).toString());
        showMessage('请输入大于等于1的数字', 'error');
        return;
      }
      const ms = seconds * 1000;
      await setRemotePollingInterval(ms);
      showMessage(`已设置远程轮询间隔为 ${seconds}秒`, 'success');
    } catch (error: unknown) {
      setRemotePollingInput(((config?.remotePollingInterval ?? 3000) / 1000).toString());
      showMessage(error instanceof Error ? error.message : '设置失败', 'error');
    }
  };

  // 处理本地轮询间隔输入
  const handleLocalPollingBlur = async () => {
    try {
      const seconds = parseInt(localPollingInput, 10);
      if (isNaN(seconds) || seconds < 1) {
        setLocalPollingInput(((config?.localPollingInterval ?? 1000) / 1000).toString());
        showMessage('请输入大于等于1的数字', 'error');
        return;
      }
      const ms = seconds * 1000;
      await setLocalPollingInterval(ms);
      showMessage(`已设置本地轮询间隔为 ${seconds}秒`, 'success');
    } catch (error: unknown) {
      setLocalPollingInput(((config?.localPollingInterval ?? 1000) / 1000).toString());
      showMessage(error instanceof Error ? error.message : '设置失败', 'error');
    }
  };

  // 过滤输入，只允许正整数
  const filterPositiveInteger = (value: string): string => {
    const filtered = value.replace(/[^0-9]/g, '');
    if (filtered === '') return '';
    const num = parseInt(filtered, 10);
    return num > 0 ? filtered : '';
  };

  // 处理切换调试模式
  const handleToggleDebugMode = async (enabled: boolean) => {
    // 立即更新本地状态，避免闪烁
    setLocalDebugModeEnabled(enabled);

    try {
      await updateConfig({ debugMode: enabled });
      showMessage(enabled ? '已启用调试模式' : '已禁用调试模式', 'success');
    } catch (error: unknown) {
      // 如果设置失败，恢复原来的状态
      setLocalDebugModeEnabled(!enabled);
      showMessage(error instanceof Error ? error.message : '设置失败', 'error');
    }
  };

  // 处理切换调试悬浮窗显示
  const handleToggleDebugOverlayVisible = async (enabled: boolean) => {
    setLocalDebugOverlayVisible(enabled);
    try {
      await updateConfig({ debugOverlayVisible: enabled });
      showMessage(enabled ? '悬浮窗将在后台时可见' : '悬浮窗已隐藏', 'success');
    } catch (error: unknown) {
      setLocalDebugOverlayVisible(!enabled);
      showMessage(error instanceof Error ? error.message : '设置失败', 'error');
    }
  };

  // 处理切换显示 URL Scheme 调用
  const handleToggleDebugUrlScheme = async (enabled: boolean) => {
    setLocalDebugUrlScheme(enabled);
    try {
      await updateConfig({ debugUrlScheme: enabled });
    } catch (error: unknown) {
      setLocalDebugUrlScheme(!enabled);
      showMessage(error instanceof Error ? error.message : '设置失败', 'error');
    }
  };

  // 处理切换启动时检查更新不限次数
  const handleToggleDebugUpdateCheckNoLimit = async (enabled: boolean) => {
    setLocalDebugUpdateCheckNoLimit(enabled);
    try {
      await updateConfig({ debugUpdateCheckNoLimit: enabled });
    } catch (error: unknown) {
      setLocalDebugUpdateCheckNoLimit(!enabled);
      showMessage(error instanceof Error ? error.message : '设置失败', 'error');
    }
  };

  // 测试验证码短信提取
  const handleTestSmsCode = () => {
    const code = extractVerificationCode(smsTestInput);
    if (code) {
      Alert.alert('提取成功', `验证码: ${code}`);
    } else {
      Alert.alert('提取失败', '未能从输入文本中提取到验证码');
    }
  };

  // 显示统计信息弹窗
  const handleShowStatistics = async () => {
    const { useStatisticsStore } = await import('@/stores/statisticsStore');
    const store = useStatisticsStore.getState();
    if (!store.isLoaded) {
      await store.load();
    }
    setStatsText(useStatisticsStore.getState().getStatisticsText());
    setShowStatsModal(true);
  };

  // 复制统计信息到剪贴板
  const handleCopyStatistics = async () => {
    const Clipboard = await import('expo-clipboard');
    await Clipboard.setStringAsync(statsText);
    setShowStatsModal(false);
    showMessage('已复制统计信息', 'success');
  };

  // 处理切换自动检查更新
  const handleToggleAutoCheckUpdate = async (enabled: boolean) => {
    setLocalAutoCheckUpdateEnabled(enabled);
    try {
      await setAutoCheckUpdate(enabled);
    } catch (error: unknown) {
      setLocalAutoCheckUpdateEnabled(!enabled);
      showMessage(error instanceof Error ? error.message : '设置失败', 'error');
    }
  };

  // 处理切换更新到测试版
  const handleToggleUpdateToBeta = async (enabled: boolean) => {
    setLocalUpdateToBetaEnabled(enabled);
    try {
      await setUpdateToBeta(enabled);
    } catch (error: unknown) {
      setLocalUpdateToBetaEnabled(!enabled);
      showMessage(error instanceof Error ? error.message : '设置失败', 'error');
    }
  };

  // 处理切换历史记录同步
  const handleToggleHistorySync = async (enabled: boolean) => {
    try {
      const { getHistorySyncService } = await import('@/services/HistorySyncService');
      const syncService = getHistorySyncService();
      syncService.cancelAll();

      if (!enabled) {
        await syncService.resetSyncCursor();
      }
    } catch {
      // ignore
    }

    setLocalHistorySyncEnabled(enabled);
    try {
      await setEnableHistorySync(enabled);

      if (!enabled) {
        await updateConfig({ needsHistoryReorganize: true });
      }

      showMessage(enabled ? '已启用历史记录同步' : '已禁用历史记录同步', 'success');
    } catch (error: unknown) {
      setLocalHistorySyncEnabled(!enabled);
      showMessage(error instanceof Error ? error.message : '设置失败', 'error');
    }
  };

  // 处理历史记录图片自动下载设置变更
  const handleImageAutoDownloadChange = async (value: 'wifi' | 'always' | 'off') => {
    setLocalImageAutoDownload(value);
    try {
      await updateConfig({ historyImageAutoDownload: value });
    } catch {
      setLocalImageAutoDownload(config?.historyImageAutoDownload ?? 'wifi');
    }
  };

  // 处理切换同步 Toast 通知
  const handleToggleSyncToast = async (enabled: boolean) => {
    setLocalSyncToastEnabled(enabled);
    try {
      await updateConfig({ syncToastEnabled: enabled });
    } catch (error: unknown) {
      setLocalSyncToastEnabled(!enabled);
      showMessage(error instanceof Error ? error.message : '设置失败', 'error');
    }
  };

  // 处理切换最近任务隐藏
  const handleToggleHideFromRecents = async (enabled: boolean) => {
    setLocalHideFromRecents(enabled);
    try {
      if (Platform.OS === 'android') {
        const { setExcludeFromRecents } = await import('native-util');
        setExcludeFromRecents(enabled);
      }
      await updateConfig({ hideFromRecents: enabled });
    } catch (error: unknown) {
      setLocalHideFromRecents(!enabled);
      showMessage(error instanceof Error ? error.message : '设置失败', 'error');
    }
  };

  // 执行更新检查逻辑
  const runUpdateCheck = async (showNoUpdateToast: boolean, includeBeta?: boolean) => {
    setIsCheckingUpdate(true);
    try {
      const today = new Date().toISOString().slice(0, 10);
      await setLastUpdateCheckDate(today);
      const useBeta = includeBeta ?? config?.updateToBeta ?? false;
      const result = await checkForUpdate(appVersion, useBeta);
      if (result.hasUpdate) {
        setUpdateAvailable(true);
        setLatestVersion(result.latestVersion);
        latestAssetsRef.current = result.assets;
        latestTagRef.current = result.tagName;
        releaseNotesRef.current = result.releaseNotes;
        showDownloadSourceDialog(result.latestVersion, result.assets, result.releaseNotes);
      } else {
        setUpdateAvailable(false);
        setLatestVersion(null);
        if (showNoUpdateToast) {
          showMessage('当前已是最新版本', 'success');
        }
      }
      // 无论是否有更新，清除当前版本及旧版本的 APK 缓存
      cleanOldApkCache(appVersion);
    } catch {
      if (showNoUpdateToast) {
        showMessage('检查更新失败，请检查网络连接', 'error');
      }
    } finally {
      setIsCheckingUpdate(false);
    }
  };

  // 点击"更新"按钮：先检查缓存，有则直接安装，否则弹渠道选择
  const handleUpdateButtonPress = async (
    version: string,
    assets: ReleaseAssetInfo[],
    releaseNotes?: string
  ) => {
    if (isDownloading) return;

    let preferredAbi: string = 'universal';
    try {
      const { getSupportedAbis } = await import('native-util');
      const abis = getSupportedAbis();
      preferredAbi = getPreferredAbi(abis);
    } catch (e) {
      console.warn('[UpdateDownload] getSupportedAbis failed:', e);
    }

    const asset = findAssetForAbi(assets, preferredAbi as Parameters<typeof findAssetForAbi>[1]);
    if (!asset) {
      showDownloadSourceDialog(version, assets, releaseNotes);
      return;
    }

    const cached = await checkApkCache(version, asset);
    console.log(`[UpdateDownload] pre-check cache=${cached ?? 'miss'}`);
    if (cached) {
      await installApk(cached);
    } else {
      showDownloadSourceDialog(version, assets, releaseNotes);
    }
  };

  // 弹出选择下载渠道的对话框
  const showDownloadSourceDialog = (
    version: string,
    assets: ReleaseAssetInfo[],
    releaseNotes?: string
  ) => {
    const notesText = releaseNotes ? `\n\n更新说明：\n${releaseNotes}` : '';
    Alert.alert(
      '发现新版本',
      `最新版本：${version}\n当前版本：${appVersion}${notesText}\n\n请选择下载渠道`,
      [
        { text: '稍后再说', style: 'cancel' },
        {
          text: 'GitCode 下载',
          onPress: () => handleDownloadApk('gitcode', version, assets),
        },
        {
          text: 'GitHub 下载',
          onPress: () => handleDownloadApk('github', version, assets),
        },
      ]
    );
  };

  // 下载 APK
  const handleDownloadApk = async (
    source: ApkSource,
    version: string,
    assets: ReleaseAssetInfo[]
  ) => {
    if (isDownloading) return;

    // 检测设备 ABI
    let preferredAbi: string = 'universal';
    try {
      const { getSupportedAbis } = await import('native-util');
      const abis = getSupportedAbis();
      preferredAbi = getPreferredAbi(abis);
      console.log(
        `[UpdateDownload] supportedAbis=${JSON.stringify(abis)} preferred=${preferredAbi}`
      );
    } catch (e) {
      console.warn('[UpdateDownload] getSupportedAbis failed:', e);
    }

    const asset = findAssetForAbi(assets, preferredAbi as Parameters<typeof findAssetForAbi>[1]);
    console.log(
      `[UpdateDownload] source=${source} version=${version} assets=${assets.map((a) => a.name).join(',')} selectedAsset=${asset?.name ?? 'none'}`
    );
    if (!asset) {
      showMessage('找不到适合当前设备的 APK', 'error');
      return;
    }

    setIsDownloading(true);
    setDownloadProgress(0);

    const abortController = new AbortController();
    downloadAbortRef.current = abortController;

    try {
      // 检查是否已有缓存
      const cached = await checkApkCache(version, asset);
      console.log(`[UpdateDownload] cache check result=${cached ?? 'miss'}`);
      if (cached) {
        await installApk(cached);
        return;
      }

      const fileUri = await downloadApk({
        asset,
        source,
        version,
        signal: abortController.signal,
        onProgress: (info) => {
          setDownloadProgress(info.progress);
        },
      });

      console.log(`[UpdateDownload] download finished fileUri=${fileUri}`);
      setUpdateAvailable(false);
      setLatestVersion(null);
      await installApk(fileUri);
    } catch (err) {
      console.error('[UpdateDownload] error:', err);
      if (err instanceof Error && err.name === 'AbortError') {
        showMessage('已取消下载', 'info');
      } else {
        showMessage(err instanceof Error ? err.message : '下载失败', 'error');
      }
    } finally {
      setIsDownloading(false);
      setDownloadProgress(0);
      downloadAbortRef.current = null;
    }
  };

  // 取消下载对话框
  const handleCancelDownload = () => {
    Alert.alert('取消下载', '确定要取消下载吗？', [
      { text: '继续下载', style: 'cancel' },
      {
        text: '取消下载',
        style: 'destructive',
        onPress: () => downloadAbortRef.current?.abort(),
      },
    ]);
  };

  // 计算存储大小
  const calculateStorageSizes = async () => {
    setIsCalculating(true);
    try {
      // 使用setTimeout模拟异步操作，避免UI阻塞
      await new Promise((resolve) => setTimeout(resolve, 100));
      const cacheSizeValue = calculateDirectorySize(cacheDir);
      const historySizeValue = calculateDirectorySize(historyDir);
      const logSizeValue = calculateLogSize();
      setCacheSize(cacheSizeValue);
      setHistorySize(historySizeValue);
      setLogSize(logSizeValue);
    } catch (error) {
      console.error('Failed to calculate storage sizes:', error);
    } finally {
      setIsCalculating(false);
    }
  };

  // 清除缓存
  const handleClearCache = () => {
    Alert.alert(
      '清空缓存',
      '确定要清空缓存目录吗？这将删除所有缓存文件。',
      [
        {
          text: '取消',
          style: 'cancel',
        },
        {
          text: '确定',
          onPress: async () => {
            try {
              clearDirectory(cacheDir);
              await calculateStorageSizes();
              showMessage('缓存已清空', 'success');
            } catch {
              showMessage('清空缓存失败', 'error');
            }
          },
        },
      ],
      { cancelable: true }
    );
  };

  // 清除日志
  const handleClearLogs = () => {
    Alert.alert(
      '清空日志',
      '确定要清空日志目录吗？这将删除所有日志文件。',
      [
        {
          text: '取消',
          style: 'cancel',
        },
        {
          text: '确定',
          onPress: async () => {
            try {
              clearLogs();
              await calculateStorageSizes();
              showMessage('日志已清空', 'success');
            } catch {
              showMessage('清空日志失败', 'error');
            }
          },
        },
      ],
      { cancelable: true }
    );
  };

  // 导出日志
  const handleExportLogs = async () => {
    if (isExportingLogs) {
      exportLogsAbortControllerRef.current?.abort();
      return;
    }

    const abortController = new AbortController();
    exportLogsAbortControllerRef.current = abortController;
    setIsExportingLogs(true);

    try {
      await saveLogsToFile(abortController.signal);
      showMessage('日志已保存', 'success');
    } catch (error) {
      if (error instanceof Error && error.name === 'AbortError') {
        showMessage('已取消导出', 'info');
      } else {
        const message = error instanceof Error ? error.message : '导出日志失败';
        showMessage(message, 'error');
      }
    } finally {
      setIsExportingLogs(false);
      exportLogsAbortControllerRef.current = null;
    }
  };

  // 设置日志等级
  const handleSetLogLevel = async (level: LogLevel) => {
    try {
      await setLogLevel(level);
      setLoggerLogLevel(level);
      showMessage(`日志等级已设置为 ${level}`, 'success');
    } catch {
      showMessage('设置日志等级失败', 'error');
    }
  };

  // 处理添加下载快捷方式
  const handleAddDownloadShortcut = async () => {
    try {
      await ShortcutService.addDownloadShortcut();
    } catch (error: unknown) {
      showMessage(error instanceof Error ? error.message : '添加失败', 'error');
    }
  };

  // 处理添加上传快捷方式
  const handleAddUploadShortcut = async () => {
    try {
      await ShortcutService.addUploadShortcut();
    } catch (error: unknown) {
      showMessage(error instanceof Error ? error.message : '添加失败', 'error');
    }
  };

  // 格式化文件大小
  const formatFileSize = (bytes: number): string => {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
  };

  return (
    <SafeAreaView
      style={[styles.container, { backgroundColor: theme.colors.background }]}
      edges={[]}
    >
      <ScrollView style={styles.scrollView}>
        {/* 服务器配置部分 */}
        <View style={styles.section}>
          <View style={[styles.sectionHeaderBase, styles.sectionHeaderRow]}>
            <TouchableOpacity
              style={styles.sectionTitleContainer}
              onPress={() => servers.length > 1 && setServersCollapsed(!serversCollapsed)}
              disabled={servers.length <= 1}
            >
              <Text style={[styles.sectionTitle, { color: theme.colors.text }]}>服务器配置</Text>
              {servers.length > 1 && (
                <View style={styles.collapseIcon}>
                  {serversCollapsed ? (
                    <ChevronDown color={theme.colors.textSecondary} width={18} height={18} />
                  ) : (
                    <ChevronUp color={theme.colors.textSecondary} width={18} height={18} />
                  )}
                </View>
              )}
            </TouchableOpacity>
            <TouchableOpacity style={styles.iconButton} onPress={handleAddServer}>
              <Plus color={theme.colors.primary} width={20} height={20} />
            </TouchableOpacity>
          </View>

          {servers.length === 0 ? (
            <View style={[styles.emptyCard, { backgroundColor: theme.colors.surface }]}>
              <Text style={[styles.emptyText, { color: theme.colors.textSecondary }]}>
                还没有配置服务器
              </Text>
              <Text style={[styles.emptyHint, { color: theme.colors.textTertiary }]}>
                点击右上角"添加"按钮添加第一个服务器
              </Text>
            </View>
          ) : serversCollapsed && servers.length > 1 ? (
            activeServer && (
              <ServerListItem
                config={activeServer}
                isActive={true}
                onPress={() => {}}
                onEdit={() => handleEditServer(activeServerIndex)}
                onDelete={() => handleDeleteServer(activeServerIndex)}
              />
            )
          ) : (
            servers.map((server, index) => (
              <ServerListItem
                key={index}
                config={server}
                isActive={index === activeServerIndex}
                onPress={() => handleSetActiveServer(index)}
                onEdit={() => handleEditServer(index)}
                onDelete={() => handleDeleteServer(index)}
              />
            ))
          )}
        </View>

        {/* 同步设置部分 */}
        <View style={styles.section}>
          <View style={styles.sectionHeaderBase}>
            <Text style={[styles.sectionTitle, { color: theme.colors.text }]}>同步设置</Text>
          </View>

          <View
            style={[
              styles.card,
              { backgroundColor: theme.colors.surface, borderColor: theme.colors.divider },
            ]}
          >
            <View style={[styles.settingRow, { borderBottomColor: theme.colors.divider }]}>
              <View style={styles.settingInfo}>
                <Text style={[styles.settingLabel, { color: theme.colors.text }]}>自动同步</Text>
                <Text style={[styles.settingDescription, { color: theme.colors.textTertiary }]}>
                  处于前台时自动同步剪贴板
                </Text>
              </View>
              <Switch
                value={localAutoSyncEnabled}
                onValueChange={handleToggleAutoSync}
                trackColor={{ false: theme.colors.divider, true: theme.colors.primary }}
                thumbColor={localAutoSyncEnabled ? theme.colors.surface : theme.colors.textTertiary}
              />
            </View>

            <View style={[styles.settingRow, { borderBottomColor: theme.colors.divider }]}>
              <View style={styles.settingInfo}>
                <Text style={[styles.settingLabel, { color: theme.colors.text }]}>
                  同步 Toast 通知
                </Text>
                <Text style={[styles.settingDescription, { color: theme.colors.textTertiary }]}>
                  上传/下载完成后显示 Toast 提示
                </Text>
              </View>
              <Switch
                value={localSyncToastEnabled}
                onValueChange={handleToggleSyncToast}
                trackColor={{ false: theme.colors.divider, true: theme.colors.primary }}
                thumbColor={
                  localSyncToastEnabled ? theme.colors.surface : theme.colors.textTertiary
                }
              />
            </View>

            <View style={[styles.settingRow, { borderBottomColor: theme.colors.divider }]}>
              <View style={styles.settingInfo}>
                <Text style={[styles.settingLabel, { color: theme.colors.text }]}>
                  允许自动同步的数据大小
                </Text>
                <Text style={[styles.settingDescription, { color: theme.colors.textTertiary }]}>
                  小于此大小的文件将自动下载
                </Text>
              </View>
              <View style={styles.inputContainer}>
                <TextInput
                  style={[
                    styles.sizeInput,
                    {
                      color: theme.colors.text,
                      borderColor: theme.colors.divider,
                      backgroundColor: theme.colors.background,
                    },
                  ]}
                  value={maxSizeInput}
                  onChangeText={setMaxSizeInput}
                  onBlur={handleMaxSizeBlur}
                  keyboardType="number-pad"
                  placeholder="5"
                  placeholderTextColor={theme.colors.textTertiary}
                />
                <Text style={[styles.unitLabel, { color: theme.colors.textSecondary }]}>MB</Text>
              </View>
            </View>

            {activeServer?.type !== 'syncclipboard' && (
              <View style={[styles.settingRow, { borderBottomColor: theme.colors.divider }]}>
                <View style={styles.settingInfo}>
                  <Text style={[styles.settingLabel, { color: theme.colors.text }]}>
                    远程轮询间隔
                  </Text>
                </View>
                <View style={styles.inputContainer}>
                  <TextInput
                    style={[
                      styles.sizeInput,
                      {
                        color: theme.colors.text,
                        borderColor: theme.colors.divider,
                        backgroundColor: theme.colors.background,
                      },
                    ]}
                    value={remotePollingInput}
                    onChangeText={(text) => setRemotePollingInput(filterPositiveInteger(text))}
                    onBlur={handleRemotePollingBlur}
                    keyboardType="number-pad"
                    placeholder="3"
                    placeholderTextColor={theme.colors.textTertiary}
                  />
                  <Text style={[styles.unitLabel, { color: theme.colors.textSecondary }]}>秒</Text>
                </View>
              </View>
            )}

            <View style={styles.settingRowNoBorder}>
              <View style={styles.settingInfo}>
                <Text style={[styles.settingLabel, { color: theme.colors.text }]}>
                  本地轮询间隔
                </Text>
              </View>
              <View style={styles.inputContainer}>
                <TextInput
                  style={[
                    styles.sizeInput,
                    {
                      color: theme.colors.text,
                      borderColor: theme.colors.divider,
                      backgroundColor: theme.colors.background,
                    },
                  ]}
                  value={localPollingInput}
                  onChangeText={(text) => setLocalPollingInput(filterPositiveInteger(text))}
                  onBlur={handleLocalPollingBlur}
                  keyboardType="number-pad"
                  placeholder="1"
                  placeholderTextColor={theme.colors.textTertiary}
                />
                <Text style={[styles.unitLabel, { color: theme.colors.textSecondary }]}>秒</Text>
              </View>
            </View>
          </View>
        </View>

        {/* 历史记录部分 */}
        <View style={styles.section}>
          <View style={styles.sectionHeaderBase}>
            <Text style={[styles.sectionTitle, { color: theme.colors.text }]}>历史记录</Text>
          </View>

          <View
            style={[
              styles.card,
              { backgroundColor: theme.colors.surface, borderColor: theme.colors.divider },
            ]}
          >
            <View style={[styles.settingRow, { borderBottomColor: theme.colors.divider }]}>
              <View style={styles.settingInfo}>
                <Text style={[styles.settingLabel, { color: theme.colors.text }]}>
                  历史记录同步
                </Text>
                <Text style={[styles.settingDescription, { color: theme.colors.textTertiary }]}>
                  {activeServer?.type !== 'syncclipboard'
                    ? '当前服务器不支持历史记录同步'
                    : '同步历史记录到服务器'}
                </Text>
              </View>
              <Switch
                value={localHistorySyncEnabled && activeServer?.type === 'syncclipboard'}
                onValueChange={handleToggleHistorySync}
                trackColor={{ false: theme.colors.divider, true: theme.colors.primary }}
                thumbColor={
                  localHistorySyncEnabled && activeServer?.type === 'syncclipboard'
                    ? theme.colors.surface
                    : theme.colors.textTertiary
                }
                disabled={activeServer?.type !== 'syncclipboard'}
              />
            </View>

            <View style={[styles.settingRow, { borderBottomColor: theme.colors.divider }]}>
              <View style={styles.settingInfo}>
                <Text style={[styles.settingLabel, { color: theme.colors.text }]}>
                  历史记录最大保留条数
                </Text>
                <Text style={[styles.settingDescription, { color: theme.colors.textTertiary }]}>
                  最小值为10条
                </Text>
              </View>
              <View style={styles.inputContainer}>
                <TextInput
                  style={[
                    styles.sizeInput,
                    {
                      color: theme.colors.text,
                      borderColor: theme.colors.divider,
                      backgroundColor: theme.colors.background,
                    },
                  ]}
                  value={maxHistoryItemsInput}
                  onChangeText={setMaxHistoryItemsInput}
                  onBlur={handleMaxHistoryItemsBlur}
                  keyboardType="number-pad"
                  placeholder="100"
                  placeholderTextColor={theme.colors.textTertiary}
                />
                <Text style={[styles.unitLabel, { color: theme.colors.textSecondary }]}>条</Text>
              </View>
            </View>

            <TouchableOpacity
              style={styles.settingRowNoBorder}
              onPress={() => setShowImageAutoDownloadMenu(!showImageAutoDownloadMenu)}
            >
              <Text style={[styles.settingLabel, { color: theme.colors.text }]}>
                浏览到图片时自动下载
              </Text>
              <View style={styles.dropdownValue}>
                <Text style={[styles.dropdownValueText, { color: theme.colors.textSecondary }]}>
                  {imageAutoDownloadOptions.find((o) => o.value === localImageAutoDownload)
                    ?.label ?? '仅 Wi-Fi'}
                </Text>
                {showImageAutoDownloadMenu ? (
                  <ChevronUp color={theme.colors.textSecondary} width={18} height={18} />
                ) : (
                  <ChevronDown color={theme.colors.textSecondary} width={18} height={18} />
                )}
              </View>
            </TouchableOpacity>

            {showImageAutoDownloadMenu && (
              <View style={[styles.dropdownMenu, { borderColor: theme.colors.divider }]}>
                {imageAutoDownloadOptions.map((option, index) => (
                  <TouchableOpacity
                    key={option.value}
                    style={[
                      styles.dropdownItem,
                      index < imageAutoDownloadOptions.length - 1
                        ? {
                            borderBottomWidth: StyleSheet.hairlineWidth,
                            borderBottomColor: theme.colors.divider,
                          }
                        : undefined,
                    ]}
                    onPress={() => {
                      handleImageAutoDownloadChange(option.value);
                      setShowImageAutoDownloadMenu(false);
                    }}
                  >
                    <Text
                      style={[
                        styles.dropdownItemText,
                        {
                          color:
                            localImageAutoDownload === option.value
                              ? theme.colors.primary
                              : theme.colors.text,
                        },
                      ]}
                    >
                      {option.label}
                    </Text>
                    {localImageAutoDownload === option.value && (
                      <Check stroke={theme.colors.primary} width={18} height={18} strokeWidth={3} />
                    )}
                  </TouchableOpacity>
                ))}
              </View>
            )}

            <View
              style={[
                styles.settingRowNoBorder,
                { borderTopWidth: StyleSheet.hairlineWidth, borderTopColor: theme.colors.divider },
              ]}
            >
              <View style={styles.settingInfo}>
                <Text style={[styles.settingLabel, { color: theme.colors.text }]}>
                  为图片显示复制按钮
                </Text>
                <Text style={[styles.settingDescription, { color: theme.colors.textTertiary }]}>
                  在历史记录的图片项显示复制到剪贴板按钮
                </Text>
              </View>
              <Switch
                value={config?.showImageCopyButton ?? false}
                onValueChange={(enabled) => updateConfig({ showImageCopyButton: enabled })}
                trackColor={{ false: theme.colors.divider, true: theme.colors.primary }}
                thumbColor={
                  (config?.showImageCopyButton ?? false)
                    ? theme.colors.surface
                    : theme.colors.textTertiary
                }
              />
            </View>
          </View>
        </View>

        {/* 后台任务部分 */}
        {Platform.OS === 'android' && (
          <View style={styles.section}>
            <View style={styles.sectionHeaderBase}>
              <Text style={[styles.sectionTitle, { color: theme.colors.text }]}>后台任务</Text>
            </View>

            <View
              style={[
                styles.card,
                { backgroundColor: theme.colors.surface, borderColor: theme.colors.divider },
              ]}
            >
              <View style={[styles.settingRow, { borderBottomColor: theme.colors.divider }]}>
                <View style={styles.settingInfo}>
                  <Text style={[styles.settingLabel, { color: theme.colors.text }]}>后台任务</Text>
                  <Text style={[styles.settingDescription, { color: theme.colors.textTertiary }]}>
                    {isTempDisabledBackgroundTasks
                      ? '已临时停止，重启 APP 后恢复开启状态'
                      : '关闭后将停止所有后台任务'}
                  </Text>
                </View>
                <Switch
                  value={localBackgroundTasksEnabled}
                  onValueChange={handleToggleBackgroundTasks}
                  trackColor={{ false: theme.colors.divider, true: theme.colors.primary }}
                  thumbColor={
                    localBackgroundTasksEnabled ? theme.colors.surface : theme.colors.textTertiary
                  }
                />
              </View>

              {/* 后台同步 */}
              <View style={[styles.settingRow, { borderBottomColor: theme.colors.divider }]}>
                <View style={styles.settingInfo}>
                  <Text
                    style={[
                      styles.settingLabel,
                      {
                        color: localBackgroundTasksEnabled
                          ? theme.colors.text
                          : theme.colors.textTertiary,
                      },
                    ]}
                  >
                    后台服务常驻通知
                  </Text>
                  <Text
                    style={[
                      styles.settingDescription,
                      {
                        color: localBackgroundTasksEnabled
                          ? theme.colors.textSecondary
                          : theme.colors.textTertiary,
                      },
                    ]}
                  >
                    启用后会增加后台服务的稳定性
                  </Text>
                </View>
                <Switch
                  value={localBackgroundTasksEnabled && localForegroundNotification}
                  onValueChange={handleToggleForegroundNotification}
                  trackColor={{ false: theme.colors.divider, true: theme.colors.primary }}
                  thumbColor={
                    localBackgroundTasksEnabled && localForegroundNotification
                      ? theme.colors.surface
                      : theme.colors.textTertiary
                  }
                  disabled={!localBackgroundTasksEnabled}
                />
              </View>

              <View style={[styles.settingRow, { borderBottomColor: theme.colors.divider }]}>
                <View style={styles.settingInfo}>
                  <Text
                    style={[
                      styles.settingLabel,
                      {
                        color: localBackgroundTasksEnabled
                          ? theme.colors.text
                          : theme.colors.textTertiary,
                      },
                    ]}
                  >
                    后台下载远程
                  </Text>
                </View>
                <Switch
                  value={localBackgroundTasksEnabled && localBackgroundDownloadEnabled}
                  onValueChange={handleToggleBackgroundDownload}
                  trackColor={{ false: theme.colors.divider, true: theme.colors.primary }}
                  thumbColor={
                    localBackgroundTasksEnabled && localBackgroundDownloadEnabled
                      ? theme.colors.surface
                      : theme.colors.textTertiary
                  }
                  disabled={!localBackgroundTasksEnabled}
                />
              </View>

              <View style={[styles.settingRow, { borderBottomColor: theme.colors.divider }]}>
                <View style={styles.settingInfo}>
                  <Text
                    style={[
                      styles.settingLabel,
                      {
                        color: localBackgroundTasksEnabled
                          ? theme.colors.text
                          : theme.colors.textTertiary,
                      },
                    ]}
                  >
                    后台上传本地
                  </Text>
                </View>
                <Switch
                  value={localBackgroundTasksEnabled && localBackgroundUploadEnabled}
                  onValueChange={handleToggleBackgroundUpload}
                  trackColor={{ false: theme.colors.divider, true: theme.colors.primary }}
                  thumbColor={
                    localBackgroundTasksEnabled && localBackgroundUploadEnabled
                      ? theme.colors.surface
                      : theme.colors.textTertiary
                  }
                  disabled={!localBackgroundTasksEnabled}
                />
              </View>

              <View style={[styles.settingRow, { borderBottomColor: theme.colors.divider }]}>
                <View style={styles.settingInfo}>
                  <Text
                    style={[
                      styles.settingLabel,
                      {
                        color: localBackgroundTasksEnabled
                          ? theme.colors.text
                          : theme.colors.textTertiary,
                      },
                    ]}
                  >
                    后台时通过悬浮窗获取剪贴板
                  </Text>
                </View>
                <Switch
                  value={localBackgroundTasksEnabled && localClipboardOverlayEnabled}
                  onValueChange={handleToggleClipboardOverlay}
                  trackColor={{ false: theme.colors.divider, true: theme.colors.primary }}
                  thumbColor={
                    localBackgroundTasksEnabled && localClipboardOverlayEnabled
                      ? theme.colors.surface
                      : theme.colors.textTertiary
                  }
                  disabled={!localBackgroundTasksEnabled}
                />
              </View>

              <View style={styles.settingRowNoBorder}>
                <View style={styles.settingInfo}>
                  <Text
                    style={[
                      styles.settingLabel,
                      {
                        color: localBackgroundTasksEnabled
                          ? theme.colors.text
                          : theme.colors.textTertiary,
                      },
                    ]}
                  >
                    后台时通过 Shizuku 获取剪贴板
                  </Text>
                  <Text
                    style={[styles.settingDescription, { color: theme.colors.primary }]}
                    onPress={() => Linking.openURL('https://shizuku.rikka.app/')}
                  >
                    前往 Shizuku 官网
                  </Text>
                </View>
                <Switch
                  value={localBackgroundTasksEnabled && localShizukuClipboardEnabled}
                  onValueChange={handleToggleShizukuClipboard}
                  trackColor={{ false: theme.colors.divider, true: theme.colors.primary }}
                  thumbColor={
                    localBackgroundTasksEnabled && localShizukuClipboardEnabled
                      ? theme.colors.surface
                      : theme.colors.textTertiary
                  }
                  disabled={!localBackgroundTasksEnabled}
                />
              </View>
            </View>
          </View>
        )}

        {/* 短信自动化部分 */}
        {Platform.OS === 'android' && (
          <View style={styles.section}>
            <View style={styles.sectionHeaderBase}>
              <Text style={[styles.sectionTitle, { color: theme.colors.text }]}>短信自动化</Text>
            </View>

            <View
              style={[
                styles.card,
                { backgroundColor: theme.colors.surface, borderColor: theme.colors.divider },
              ]}
            >
              <View style={styles.settingRowNoBorder}>
                <View style={styles.settingInfo}>
                  <Text style={[styles.settingLabel, { color: theme.colors.text }]}>
                    自动上传短信验证码
                  </Text>
                </View>
                <Switch
                  value={localSmsForwardingEnabled}
                  onValueChange={handleToggleSmsForwarding}
                  trackColor={{ false: theme.colors.divider, true: theme.colors.primary }}
                  thumbColor={
                    localSmsForwardingEnabled ? theme.colors.surface : theme.colors.textTertiary
                  }
                />
              </View>
            </View>
          </View>
        )}

        {/* 权限管理部分 */}
        {Platform.OS === 'android' && (
          <View style={styles.section}>
            <View style={[styles.sectionHeaderBase, styles.sectionHeaderRow]}>
              <Text style={[styles.sectionTitle, { color: theme.colors.text }]}>权限管理</Text>
              <TouchableOpacity
                style={styles.iconButton}
                onPress={refreshPermissions}
                disabled={isRefreshingPermissions}
              >
                <RefreshCw color={theme.colors.primary} width={16} height={16} />
              </TouchableOpacity>
            </View>

            <View
              style={[
                styles.card,
                { backgroundColor: theme.colors.surface, borderColor: theme.colors.divider },
              ]}
            >
              <View style={[styles.settingRow, { borderBottomColor: theme.colors.divider }]}>
                <View style={styles.settingInfo}>
                  <Text style={[styles.settingLabel, { color: theme.colors.text }]}>通知权限</Text>
                </View>
                <Switch
                  value={permNotification}
                  onValueChange={() => Linking.openSettings()}
                  trackColor={{ false: theme.colors.divider, true: theme.colors.primary }}
                  thumbColor={permNotification ? theme.colors.surface : theme.colors.textTertiary}
                />
              </View>

              <View style={[styles.settingRow, { borderBottomColor: theme.colors.divider }]}>
                <View style={styles.settingInfo}>
                  <Text style={[styles.settingLabel, { color: theme.colors.text }]}>
                    悬浮窗权限
                  </Text>
                  <Text style={[styles.settingDescription, { color: theme.colors.textTertiary }]}>
                    后台通过悬浮窗获取剪贴板所需
                  </Text>
                </View>
                <Switch
                  value={permOverlay}
                  onValueChange={() => requestOverlayPermission()}
                  trackColor={{ false: theme.colors.divider, true: theme.colors.primary }}
                  thumbColor={permOverlay ? theme.colors.surface : theme.colors.textTertiary}
                />
              </View>

              <View style={[styles.settingRow, { borderBottomColor: theme.colors.divider }]}>
                <View style={styles.settingInfo}>
                  <Text style={[styles.settingLabel, { color: theme.colors.text }]}>短信权限</Text>
                  <Text style={[styles.settingDescription, { color: theme.colors.textTertiary }]}>
                    自动上传短信验证码所需
                  </Text>
                </View>
                <Switch
                  value={permSms}
                  onValueChange={() => Linking.openSettings()}
                  trackColor={{ false: theme.colors.divider, true: theme.colors.primary }}
                  thumbColor={permSms ? theme.colors.surface : theme.colors.textTertiary}
                />
              </View>

              <View style={[styles.settingRow, { borderBottomColor: theme.colors.divider }]}>
                <View style={styles.settingInfo}>
                  <Text style={[styles.settingLabel, { color: theme.colors.text }]}>
                    Shizuku 权限
                  </Text>
                  <Text style={[styles.settingDescription, { color: theme.colors.textTertiary }]}>
                    {shizukuAvailable
                      ? '后台通过 Shizuku 获取剪贴板所需'
                      : 'Shizuku 未运行，请先启动 Shizuku'}
                  </Text>
                </View>
                <Switch
                  value={permShizuku}
                  onValueChange={async () => {
                    if (!shizukuAvailable) {
                      Alert.alert(
                        'Shizuku 未运行',
                        '请先安装并启动 Shizuku。\n\n非 Root 设备每次重启后需重新启动 Shizuku（Android 11+ 可通过无线调试自行启动）。',
                        [
                          {
                            text: '了解更多',
                            onPress: () =>
                              Linking.openURL('https://shizuku.rikka.app/guide/setup/'),
                          },
                          { text: '取消', style: 'cancel' },
                        ]
                      );
                      return;
                    }
                    if (!permShizuku) {
                      requestShizukuPermission();
                      // 延迟刷新权限状态（等待用户授权）
                      setTimeout(refreshPermissions, 2000);
                    }
                  }}
                  trackColor={{ false: theme.colors.divider, true: theme.colors.primary }}
                  thumbColor={permShizuku ? theme.colors.surface : theme.colors.textTertiary}
                />
              </View>

              <View style={styles.settingRowNoBorder}>
                <View style={styles.settingInfo}>
                  <Text style={[styles.settingLabel, { color: theme.colors.text }]}>
                    忽略电池优化
                  </Text>
                  <Text style={[styles.settingDescription, { color: theme.colors.textTertiary }]}>
                    防止省电模式中断后台同步
                  </Text>
                </View>
                <Switch
                  value={permBattery}
                  onValueChange={async () => {
                    const { requestIgnoreBatteryOptimizations } = await import('native-util');
                    if (hasBatteryOptRequested.current) {
                      Alert.alert(
                        '无法唤起系统弹窗',
                        '系统限制每次安装仅允许弹出一次电池优化请求，请前往系统设置手动关闭电池优化。',
                        [
                          {
                            text: '前往设置',
                            onPress: () => Linking.openSettings(),
                          },
                          { text: '取消', style: 'cancel' },
                        ]
                      );
                      return;
                    }
                    requestIgnoreBatteryOptimizations();
                    hasBatteryOptRequested.current = true;
                  }}
                  trackColor={{ false: theme.colors.divider, true: theme.colors.primary }}
                  thumbColor={permBattery ? theme.colors.surface : theme.colors.textTertiary}
                />
              </View>
            </View>
          </View>
        )}

        {/* 快捷操作部分 */}
        <View style={styles.section}>
          <View style={styles.sectionHeaderBase}>
            <Text style={[styles.sectionTitle, { color: theme.colors.text }]}>快捷操作</Text>
          </View>

          <View
            style={[
              styles.card,
              { backgroundColor: theme.colors.surface, borderColor: theme.colors.divider },
            ]}
          >
            <View style={[styles.settingRow, { borderBottomColor: theme.colors.divider }]}>
              <View style={styles.settingInfo}>
                <Text style={[styles.settingLabel, { color: theme.colors.text }]}>
                  添加桌面快捷方式：下载
                </Text>
              </View>
              <TouchableOpacity
                style={[styles.actionButton, { backgroundColor: theme.colors.primary }]}
                onPress={handleAddDownloadShortcut}
              >
                <Text style={[styles.actionButtonText, { color: theme.colors.white }]}>添加</Text>
              </TouchableOpacity>
            </View>

            <View style={styles.settingRowNoBorder}>
              <View style={styles.settingInfo}>
                <Text style={[styles.settingLabel, { color: theme.colors.text }]}>
                  添加桌面快捷方式：上传
                </Text>
              </View>
              <TouchableOpacity
                style={[styles.actionButton, { backgroundColor: theme.colors.primary }]}
                onPress={handleAddUploadShortcut}
              >
                <Text style={[styles.actionButtonText, { color: theme.colors.white }]}>添加</Text>
              </TouchableOpacity>
            </View>
          </View>
        </View>

        {/* 存储部分 */}
        <View style={styles.section}>
          <View style={[styles.sectionHeaderBase, styles.sectionHeaderRow]}>
            <Text style={[styles.sectionTitle, { color: theme.colors.text }]}>存储</Text>
            <TouchableOpacity
              style={styles.iconButton}
              onPress={calculateStorageSizes}
              disabled={isCalculating}
            >
              <RefreshCw color={theme.colors.primary} width={16} height={16} />
            </TouchableOpacity>
          </View>

          <View
            style={[
              styles.card,
              { backgroundColor: theme.colors.surface, borderColor: theme.colors.divider },
            ]}
          >
            <View style={[styles.settingRow, { borderBottomColor: theme.colors.divider }]}>
              <View style={styles.settingInfo}>
                <Text style={[styles.settingLabel, { color: theme.colors.text }]}>
                  缓存空间占用
                </Text>
                <Text style={[styles.settingDescription, { color: theme.colors.textTertiary }]}>
                  {isCalculating ? '加载中...' : formatFileSize(cacheSize)}
                </Text>
              </View>
              <TouchableOpacity
                style={[styles.clearButton, { backgroundColor: theme.colors.primary }]}
                onPress={handleClearCache}
                disabled={isCalculating}
              >
                <Text style={[styles.clearButtonText, { color: theme.colors.white }]}>清理</Text>
              </TouchableOpacity>
            </View>

            <View style={[styles.settingRow, { borderBottomColor: theme.colors.divider }]}>
              <View style={styles.settingInfo}>
                <Text style={[styles.settingLabel, { color: theme.colors.text }]}>
                  日志空间占用
                </Text>
                <Text style={[styles.settingDescription, { color: theme.colors.textTertiary }]}>
                  {isCalculating ? '加载中...' : formatFileSize(logSize)}
                </Text>
              </View>
              <TouchableOpacity
                style={[styles.clearButton, { backgroundColor: theme.colors.primary }]}
                onPress={handleClearLogs}
                disabled={isCalculating}
              >
                <Text style={[styles.clearButtonText, { color: theme.colors.white }]}>清理</Text>
              </TouchableOpacity>
            </View>

            <View style={styles.settingRowNoBorder}>
              <View style={styles.settingInfo}>
                <Text style={[styles.settingLabel, { color: theme.colors.text }]}>
                  历史记录空间占用
                </Text>
                <Text style={[styles.settingDescription, { color: theme.colors.textTertiary }]}>
                  {isCalculating ? '加载中...' : formatFileSize(historySize)}
                </Text>
              </View>
            </View>
          </View>
        </View>

        {/* 日志设置部分 */}
        <View style={styles.section}>
          <View style={styles.sectionHeaderBase}>
            <Text style={[styles.sectionTitle, { color: theme.colors.text }]}>日志</Text>
          </View>

          <View
            style={[
              styles.card,
              { backgroundColor: theme.colors.surface, borderColor: theme.colors.divider },
            ]}
          >
            <TouchableOpacity
              style={[styles.settingRow, { borderBottomColor: theme.colors.divider }]}
              onPress={() => setShowLogLevelMenu(!showLogLevelMenu)}
            >
              <Text style={[styles.settingLabel, { color: theme.colors.text }]}>日志等级</Text>
              <View style={styles.dropdownValue}>
                <Text style={[styles.dropdownValueText, { color: theme.colors.textSecondary }]}>
                  {config?.logLevel === 'debug'
                    ? '调试'
                    : config?.logLevel === 'info'
                      ? '信息'
                      : config?.logLevel === 'warn'
                        ? '警告'
                        : '错误'}
                </Text>
                {showLogLevelMenu ? (
                  <ChevronUp color={theme.colors.textSecondary} width={18} height={18} />
                ) : (
                  <ChevronDown color={theme.colors.textSecondary} width={18} height={18} />
                )}
              </View>
            </TouchableOpacity>

            {showLogLevelMenu && (
              <View style={[styles.dropdownMenu, { borderColor: theme.colors.divider }]}>
                {[
                  { label: '调试', value: 'debug' as LogLevel },
                  { label: '信息', value: 'info' as LogLevel },
                  { label: '警告', value: 'warn' as LogLevel },
                  { label: '错误', value: 'error' as LogLevel },
                ].map((option, index) => (
                  <TouchableOpacity
                    key={option.value}
                    style={[
                      styles.dropdownItem,
                      index < 3
                        ? {
                            borderBottomWidth: StyleSheet.hairlineWidth,
                            borderBottomColor: theme.colors.divider,
                          }
                        : undefined,
                    ]}
                    onPress={() => {
                      handleSetLogLevel(option.value);
                      setShowLogLevelMenu(false);
                    }}
                  >
                    <Text
                      style={[
                        styles.dropdownItemText,
                        {
                          color:
                            config?.logLevel === option.value
                              ? theme.colors.primary
                              : theme.colors.text,
                        },
                      ]}
                    >
                      {option.label}
                    </Text>
                    {config?.logLevel === option.value && (
                      <Check stroke={theme.colors.primary} width={18} height={18} strokeWidth={3} />
                    )}
                  </TouchableOpacity>
                ))}
              </View>
            )}

            <View style={styles.settingRowNoBorder}>
              <Text style={[styles.settingLabel, { color: theme.colors.text }]}>导出日志</Text>
              <TouchableOpacity
                style={[styles.clearButton, { backgroundColor: theme.colors.primary }]}
                onPress={handleExportLogs}
                disabled={isCalculating}
              >
                <Text style={[styles.clearButtonText, { color: theme.colors.white }]}>
                  {isExportingLogs ? '取消' : '导出'}
                </Text>
              </TouchableOpacity>
            </View>
          </View>
        </View>

        {/* 外观设置部分 */}
        <View style={styles.section}>
          <View style={styles.sectionHeaderBase}>
            <Text style={[styles.sectionTitle, { color: theme.colors.text }]}>外观</Text>
          </View>

          <View
            style={[
              styles.card,
              { backgroundColor: theme.colors.surface, borderColor: theme.colors.divider },
            ]}
          >
            {/* 主题色 (source color) */}
            <View
              style={[
                styles.appearanceBlock,
                {
                  borderBottomWidth: StyleSheet.hairlineWidth,
                  borderBottomColor: theme.colors.divider,
                },
              ]}
            >
              <Text style={[styles.settingLabel, { color: theme.colors.text }]}>主题色</Text>
              <Text style={[styles.settingDescription, { color: theme.colors.textTertiary }]}>
                切换 source color,影响主色调与容器底色
              </Text>
              <View style={styles.swatchRow}>
                {PALETTES.map((p) => {
                  const active = p.id === paletteId;
                  return (
                    <TouchableOpacity
                      key={p.id}
                      onPress={() => handleSetPaletteId(p.id)}
                      accessibilityRole="button"
                      accessibilityLabel={`主题色 ${p.label}`}
                      accessibilityState={{ selected: active }}
                      style={styles.swatchWrap}
                    >
                      <View
                        style={[
                          styles.swatchRing,
                          { borderColor: active ? p.swatch : 'transparent' },
                        ]}
                      >
                        <View style={[styles.swatch, { backgroundColor: p.swatch }]}>
                          {active && (
                            <Check
                              stroke={theme.colors.white}
                              width={16}
                              height={16}
                              strokeWidth={3}
                            />
                          )}
                        </View>
                      </View>
                      <Text
                        style={[
                          styles.swatchLabel,
                          {
                            color: active ? theme.colors.text : theme.colors.textTertiary,
                            fontWeight: active ? '600' : '400',
                          },
                        ]}
                      >
                        {p.label}
                      </Text>
                    </TouchableOpacity>
                  );
                })}
              </View>
            </View>

            {/* 外观模式 — M3 segmented */}
            <View
              style={[
                styles.appearanceBlock,
                Platform.OS === 'android' && {
                  borderBottomWidth: StyleSheet.hairlineWidth,
                  borderBottomColor: theme.colors.divider,
                },
              ]}
            >
              <Text style={[styles.settingLabel, { color: theme.colors.text }]}>外观模式</Text>
              <View style={[styles.segmentedTrack, { borderColor: theme.colors.outline }]}>
                {themeOptions.map((opt, i) => {
                  const active = themeMode === opt.value;
                  const isFirst = i === 0;
                  const isLast = i === themeOptions.length - 1;
                  return (
                    <TouchableOpacity
                      key={opt.value}
                      onPress={() => handleSetThemeMode(opt.value)}
                      accessibilityRole="button"
                      accessibilityState={{ selected: active }}
                      style={[
                        styles.segmentedItem,
                        {
                          backgroundColor: active ? theme.colors.primaryContainer : 'transparent',
                          borderLeftWidth: isFirst ? 0 : StyleSheet.hairlineWidth,
                          borderLeftColor: theme.colors.outline,
                        },
                        isFirst && {
                          borderTopLeftRadius: radius.pill,
                          borderBottomLeftRadius: radius.pill,
                        },
                        isLast && {
                          borderTopRightRadius: radius.pill,
                          borderBottomRightRadius: radius.pill,
                        },
                      ]}
                    >
                      {active && (
                        <Check
                          stroke={theme.colors.onPrimaryContainer}
                          width={14}
                          height={14}
                          strokeWidth={3}
                          style={styles.segmentedCheck}
                        />
                      )}
                      <Text
                        style={[
                          styles.segmentedItemText,
                          {
                            color: active ? theme.colors.onPrimaryContainer : theme.colors.text,
                          },
                        ]}
                      >
                        {opt.label}
                      </Text>
                    </TouchableOpacity>
                  );
                })}
              </View>
            </View>

            {Platform.OS === 'android' && (
              <View style={styles.settingRowNoBorder}>
                <View style={styles.settingInfo}>
                  <Text style={[styles.settingLabel, { color: theme.colors.text }]}>
                    在最近任务列表中隐藏
                  </Text>
                  <Text style={[styles.settingDescription, { color: theme.colors.textTertiary }]}>
                    建议隐藏前先锁定，防止被一键清理
                  </Text>
                </View>
                <Switch
                  value={localHideFromRecents}
                  onValueChange={handleToggleHideFromRecents}
                  trackColor={{ false: theme.colors.divider, true: theme.colors.primary }}
                  thumbColor={
                    localHideFromRecents ? theme.colors.surface : theme.colors.textTertiary
                  }
                />
              </View>
            )}
          </View>
        </View>

        {/* 应用信息部分 */}
        <View style={styles.section}>
          <View style={styles.sectionHeaderBase}>
            <Text style={[styles.sectionTitle, { color: theme.colors.text }]}>关于</Text>
          </View>

          <View
            style={[
              styles.card,
              { backgroundColor: theme.colors.surface, borderColor: theme.colors.divider },
            ]}
          >
            <View
              style={[
                styles.versionBlock,
                {
                  borderBottomWidth: StyleSheet.hairlineWidth,
                  borderBottomColor: theme.colors.divider,
                },
              ]}
            >
              <View style={styles.versionTopRow}>
                <View style={styles.versionLabelGroup}>
                  <Text style={[styles.infoLabel, { color: theme.colors.textSecondary }]}>
                    版本
                  </Text>
                  <Text style={[styles.infoValue, { color: theme.colors.text }]}>{appVersion}</Text>
                </View>
                <View style={styles.versionButtonGroup}>
                  <TouchableOpacity
                    style={[
                      styles.updateButton,
                      {
                        backgroundColor:
                          isDownloading || updateAvailable
                            ? theme.colors.primary
                            : theme.colors.surface,
                        borderColor: theme.colors.primary,
                      },
                    ]}
                    onPress={() => {
                      if (isDownloading) {
                        handleCancelDownload();
                      } else if (updateAvailable) {
                        handleUpdateButtonPress(
                          latestVersion ?? '',
                          latestAssetsRef.current,
                          releaseNotesRef.current
                        );
                      } else {
                        runUpdateCheck(true, localUpdateToBetaEnabled);
                      }
                    }}
                    disabled={isCheckingUpdate}
                  >
                    <Text
                      style={[
                        styles.updateButtonText,
                        {
                          color:
                            isDownloading || updateAvailable
                              ? theme.colors.white
                              : theme.colors.primary,
                        },
                      ]}
                    >
                      {isCheckingUpdate
                        ? '检查中...'
                        : isDownloading
                          ? `下载中 ${Math.round(downloadProgress * 100)}%`
                          : updateAvailable
                            ? `更新 ${latestVersion}`
                            : '检查更新'}
                    </Text>
                  </TouchableOpacity>
                  <TouchableOpacity
                    style={[
                      styles.updateButton,
                      {
                        backgroundColor: theme.colors.primary,
                        borderColor: theme.colors.primary,
                      },
                    ]}
                    onPress={() => Linking.openURL('https://github.com/UniClipboard/uc-android')}
                  >
                    <Text style={[styles.updateButtonText, { color: theme.colors.white }]}>
                      GitHub
                    </Text>
                  </TouchableOpacity>
                </View>
              </View>
            </View>

            <View style={[styles.settingRow, { borderBottomColor: theme.colors.divider }]}>
              <View style={styles.settingInfo}>
                <Text style={[styles.settingLabel, { color: theme.colors.text }]}>
                  自动检查更新
                </Text>
              </View>
              <Switch
                value={localAutoCheckUpdateEnabled}
                onValueChange={handleToggleAutoCheckUpdate}
                trackColor={{ false: theme.colors.divider, true: theme.colors.primary }}
                thumbColor={
                  localAutoCheckUpdateEnabled ? theme.colors.surface : theme.colors.textTertiary
                }
              />
            </View>

            <View style={styles.settingRowNoBorder}>
              <View style={styles.settingInfo}>
                <Text style={[styles.settingLabel, { color: theme.colors.text }]}>
                  更新到测试版
                </Text>
              </View>
              <Switch
                value={localUpdateToBetaEnabled}
                onValueChange={handleToggleUpdateToBeta}
                trackColor={{ false: theme.colors.divider, true: theme.colors.primary }}
                thumbColor={
                  localUpdateToBetaEnabled ? theme.colors.surface : theme.colors.textTertiary
                }
              />
            </View>
          </View>
        </View>

        {/* 调试部分 */}
        <View style={styles.section}>
          <View style={styles.sectionHeaderBase}>
            <Text style={[styles.sectionTitle, { color: theme.colors.text }]}>调试</Text>
          </View>

          <View
            style={[
              styles.card,
              { backgroundColor: theme.colors.surface, borderColor: theme.colors.divider },
            ]}
          >
            <View
              style={[
                styles.settingRowNoBorder,
                localDebugModeEnabled && {
                  borderBottomWidth: StyleSheet.hairlineWidth,
                  borderBottomColor: theme.colors.divider,
                },
              ]}
            >
              <View style={styles.settingInfo}>
                <Text style={[styles.settingLabel, { color: theme.colors.text }]}>调试模式</Text>
              </View>
              <Switch
                value={localDebugModeEnabled}
                onValueChange={handleToggleDebugMode}
                trackColor={{ false: theme.colors.divider, true: theme.colors.primary }}
                thumbColor={
                  localDebugModeEnabled ? theme.colors.surface : theme.colors.textTertiary
                }
              />
            </View>

            {localDebugModeEnabled && Platform.OS === 'android' && (
              <View style={[styles.settingRow, { borderBottomColor: theme.colors.divider }]}>
                <View style={styles.settingInfo}>
                  <Text style={[styles.settingLabel, { color: theme.colors.text }]}>
                    显示悬浮窗
                  </Text>
                  <Text style={[styles.settingDescription, { color: theme.colors.textTertiary }]}>
                    后台获取剪贴板时显示可见的悬浮窗
                  </Text>
                </View>
                <Switch
                  value={localDebugOverlayVisible}
                  onValueChange={handleToggleDebugOverlayVisible}
                  trackColor={{ false: theme.colors.divider, true: theme.colors.primary }}
                  thumbColor={
                    localDebugOverlayVisible ? theme.colors.surface : theme.colors.textTertiary
                  }
                />
              </View>
            )}

            {localDebugModeEnabled && (
              <View
                style={[
                  styles.settingRowNoBorder,
                  {
                    borderBottomWidth: StyleSheet.hairlineWidth,
                    borderBottomColor: theme.colors.divider,
                  },
                ]}
              >
                <View style={styles.settingInfo}>
                  <Text style={[styles.settingLabel, { color: theme.colors.text }]}>
                    显示 URL Scheme 调用
                  </Text>
                </View>
                <Switch
                  value={localDebugUrlScheme}
                  onValueChange={handleToggleDebugUrlScheme}
                  trackColor={{ false: theme.colors.divider, true: theme.colors.primary }}
                  thumbColor={
                    localDebugUrlScheme ? theme.colors.surface : theme.colors.textTertiary
                  }
                />
              </View>
            )}

            {localDebugModeEnabled && (
              <View style={[styles.settingRow, { borderBottomColor: theme.colors.divider }]}>
                <View style={styles.settingInfo}>
                  <Text style={[styles.settingLabel, { color: theme.colors.text }]}>
                    测试验证码短信
                  </Text>
                </View>
                <TouchableOpacity
                  style={[styles.actionButton, { backgroundColor: theme.colors.primary }]}
                  onPress={() => {
                    setSmsTestInput('');
                    setShowSmsTestModal(true);
                  }}
                >
                  <Text style={[styles.actionButtonText, { color: theme.colors.white }]}>测试</Text>
                </TouchableOpacity>
              </View>
            )}

            {localDebugModeEnabled && (
              <View style={[styles.settingRow, { borderBottomColor: theme.colors.divider }]}>
                <View style={styles.settingInfo}>
                  <Text style={[styles.settingLabel, { color: theme.colors.text }]}>
                    更新检查不限次数
                  </Text>
                  <Text style={[styles.settingDescription, { color: theme.colors.textTertiary }]}>
                    开启后每次启动均检查更新，不限每天一次
                  </Text>
                </View>
                <Switch
                  value={localDebugUpdateCheckNoLimit}
                  onValueChange={handleToggleDebugUpdateCheckNoLimit}
                  trackColor={{ false: theme.colors.divider, true: theme.colors.primary }}
                  thumbColor={
                    localDebugUpdateCheckNoLimit ? theme.colors.surface : theme.colors.textTertiary
                  }
                />
              </View>
            )}

            {localDebugModeEnabled && (
              <View style={styles.settingRowNoBorder}>
                <View style={styles.settingInfo}>
                  <Text style={[styles.settingLabel, { color: theme.colors.text }]}>统计信息</Text>
                </View>
                <TouchableOpacity
                  style={[styles.actionButton, { backgroundColor: theme.colors.primary }]}
                  onPress={handleShowStatistics}
                >
                  <Text style={[styles.actionButtonText, { color: theme.colors.white }]}>查看</Text>
                </TouchableOpacity>
              </View>
            )}
          </View>
        </View>

        <View style={styles.bottomPadding} />
      </ScrollView>

      {/* 消息提示 */}
      <MessageToast message={message} onMessageShown={handleMessageShown} />

      {/* 服务器配置模态框 */}
      <ServerConfigModal
        visible={showServerModal}
        onClose={() => {
          setShowServerModal(false);
          setPrefillFromScan(null);
        }}
        onSave={handleSaveServer}
        initialConfig={
          editingServerIndex !== null ? servers[editingServerIndex] : (prefillFromScan ?? undefined)
        }
        isEditing={editingServerIndex !== null}
      />

      {/* 扫码 Modal */}
      <QrScannerModal visible={showScannerModal} onClose={handleScannerClose} />

      {/* 测试验证码短信模态框 */}
      <Modal
        visible={showSmsTestModal}
        transparent
        animationType="fade"
        onRequestClose={() => setShowSmsTestModal(false)}
      >
        <View style={[styles.smsTestModalOverlay, { backgroundColor: theme.colors.overlay }]}>
          <View style={[styles.smsTestModalContent, { backgroundColor: theme.colors.surface }]}>
            <Text style={[styles.smsTestModalTitle, { color: theme.colors.text }]}>
              测试验证码短信
            </Text>
            <TextInput
              style={[
                styles.smsTestModalInput,
                {
                  color: theme.colors.text,
                  borderColor: theme.colors.divider,
                  backgroundColor: theme.colors.background,
                },
              ]}
              placeholder="输入短信内容..."
              placeholderTextColor={theme.colors.textTertiary}
              value={smsTestInput}
              onChangeText={setSmsTestInput}
              multiline
              numberOfLines={4}
              textAlignVertical="top"
            />
            <View style={styles.smsTestModalButtons}>
              <TouchableOpacity
                style={[styles.smsTestModalButton, { backgroundColor: theme.colors.divider }]}
                onPress={() => setShowSmsTestModal(false)}
              >
                <Text style={[styles.smsTestModalButtonText, { color: theme.colors.text }]}>
                  取消
                </Text>
              </TouchableOpacity>
              <TouchableOpacity
                style={[styles.smsTestModalButton, { backgroundColor: theme.colors.primary }]}
                onPress={handleTestSmsCode}
              >
                <Text style={[styles.smsTestModalButtonText, { color: theme.colors.white }]}>
                  测试
                </Text>
              </TouchableOpacity>
            </View>
          </View>
        </View>
      </Modal>

      {/* 统计信息模态框 */}
      <Modal
        visible={showStatsModal}
        transparent
        animationType="fade"
        onRequestClose={() => setShowStatsModal(false)}
      >
        <View style={[styles.smsTestModalOverlay, { backgroundColor: theme.colors.overlay }]}>
          <View style={[styles.smsTestModalContent, { backgroundColor: theme.colors.surface }]}>
            <Text style={[styles.smsTestModalTitle, { color: theme.colors.text }]}>统计信息</Text>
            <Text style={[styles.statsText, { color: theme.colors.text }]} selectable>
              {statsText}
            </Text>
            <View style={styles.smsTestModalButtons}>
              <TouchableOpacity
                style={[styles.smsTestModalButton, { backgroundColor: theme.colors.divider }]}
                onPress={() => setShowStatsModal(false)}
              >
                <Text style={[styles.smsTestModalButtonText, { color: theme.colors.text }]}>
                  关闭
                </Text>
              </TouchableOpacity>
              <TouchableOpacity
                style={[styles.smsTestModalButton, { backgroundColor: theme.colors.primary }]}
                onPress={handleCopyStatistics}
              >
                <Text style={[styles.smsTestModalButtonText, { color: theme.colors.white }]}>
                  复制
                </Text>
              </TouchableOpacity>
            </View>
          </View>
        </View>
      </Modal>
    </SafeAreaView>
  );
};

const styles = StyleSheet.create({
  container: {
    flex: 1,
  },
  scrollView: {
    flex: 1,
  },
  section: {
    marginTop: spacing.lg,
  },
  sectionHeaderBase: {
    marginHorizontal: spacing.base,
    marginBottom: spacing.sm,
  },
  sectionHeaderRow: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
  },
  sectionTitle: {
    fontSize: typography.sectionHeader.fontSize,
    fontWeight: typography.sectionHeader.fontWeight,
    textTransform: 'uppercase',
    letterSpacing: typography.sectionHeader.letterSpacing,
  },
  sectionTitleContainer: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
  },
  collapseIcon: {
    marginTop: 1,
  },
  iconButton: {
    width: 34,
    height: 34,
    borderRadius: radius.sm,
    borderCurve: 'continuous',
    justifyContent: 'center',
    alignItems: 'center',
  },
  addButton: {
    paddingHorizontal: spacing.base,
    paddingVertical: spacing.sm,
    borderRadius: radius.sm,
    borderCurve: 'continuous',
  },
  addButtonText: {
    fontSize: 14,
    fontWeight: '600',
  },
  emptyCard: {
    marginHorizontal: spacing.base,
    borderRadius: radius.lg,
    borderCurve: 'continuous',
    padding: spacing.xxl,
    alignItems: 'center',
    ...elevation.sm,
  },
  emptyText: {
    fontSize: typography.callout.fontSize,
    fontWeight: '500',
    marginBottom: spacing.sm,
  },
  emptyHint: {
    fontSize: 14,
    textAlign: 'center',
  },
  card: {
    marginHorizontal: spacing.base,
    borderRadius: radius.lg,
    borderCurve: 'continuous',
    overflow: 'hidden',
    borderWidth: StyleSheet.hairlineWidth,
    ...elevation.sm,
  },
  cardTitle: {
    fontSize: typography.subhead.fontSize,
    fontWeight: '600',
    paddingHorizontal: spacing.base,
    paddingTop: spacing.base,
    paddingBottom: spacing.sm,
  },
  optionItem: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
    paddingHorizontal: spacing.base,
    paddingVertical: 14,
  },
  optionInfo: {
    flex: 1,
  },
  optionLabel: {
    fontSize: typography.callout.fontSize,
  },
  infoRow: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
    paddingHorizontal: spacing.base,
    paddingVertical: 14,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  infoLabel: {
    fontSize: typography.callout.fontSize,
  },
  infoValue: {
    fontSize: typography.callout.fontSize,
    fontWeight: '500',
  },
  settingRow: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
    paddingHorizontal: spacing.base,
    paddingVertical: spacing.md,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  settingRowNoBorder: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
    paddingHorizontal: spacing.base,
    paddingVertical: spacing.md,
  },
  settingInfo: {
    flex: 1,
    marginRight: spacing.base,
  },
  settingLabel: {
    fontSize: typography.callout.fontSize,
    fontWeight: '500',
    marginBottom: spacing.xs,
  },
  settingDescription: {
    fontSize: typography.footnote.fontSize,
    lineHeight: typography.footnote.lineHeight,
  },
  inputContainer: {
    flexDirection: 'row',
    alignItems: 'center',
  },
  sizeInput: {
    width: 80,
    height: 40,
    borderWidth: 1,
    borderRadius: radius.sm,
    borderCurve: 'continuous',
    paddingHorizontal: spacing.md,
    fontSize: typography.callout.fontSize,
    textAlign: 'right',
  },
  unitLabel: {
    fontSize: typography.callout.fontSize,
    marginLeft: spacing.sm,
    fontWeight: '500',
  },
  appearanceBlock: {
    paddingHorizontal: spacing.base,
    paddingVertical: spacing.md,
  },
  swatchRow: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    marginTop: spacing.md,
  },
  swatchWrap: {
    alignItems: 'center',
    flex: 1,
  },
  swatchRing: {
    width: 44,
    height: 44,
    borderRadius: radius.pill,
    borderWidth: 2,
    padding: 2,
    alignItems: 'center',
    justifyContent: 'center',
  },
  swatch: {
    width: '100%',
    height: '100%',
    borderRadius: radius.pill,
    alignItems: 'center',
    justifyContent: 'center',
  },
  swatchLabel: {
    fontSize: typography.caption1.fontSize,
    marginTop: spacing.xs,
  },
  segmentedTrack: {
    flexDirection: 'row',
    borderRadius: radius.pill,
    borderWidth: 1,
    overflow: 'hidden',
    marginTop: spacing.md,
  },
  segmentedItem: {
    flex: 1,
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'center',
    paddingVertical: spacing.sm + 2,
    paddingHorizontal: spacing.sm,
  },
  segmentedItemText: {
    fontSize: typography.subhead.fontSize,
    fontWeight: '500',
  },
  segmentedCheck: {
    marginRight: spacing.xs + 2,
  },
  bottomPadding: {
    height: 40,
  },
  versionBlock: {
    paddingHorizontal: spacing.base,
    paddingVertical: spacing.md,
  },
  versionTopRow: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
  },
  versionButtonGroup: {
    flexDirection: 'row',
    gap: spacing.sm,
  },
  versionLabelGroup: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: spacing.sm,
  },
  updateButton: {
    paddingHorizontal: spacing.md,
    paddingVertical: 6,
    borderRadius: radius.sm,
    borderCurve: 'continuous',
    borderWidth: 1,
  },
  updateButtonText: {
    fontSize: typography.footnote.fontSize,
    fontWeight: '600',
  },
  dropdownValue: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: spacing.xs,
  },
  dropdownValueText: {
    fontSize: typography.callout.fontSize,
  },
  dropdownMenu: {
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  dropdownItem: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
    paddingHorizontal: spacing.base,
    paddingVertical: spacing.md,
  },
  dropdownItemText: {
    fontSize: typography.callout.fontSize,
  },
  clearButton: {
    paddingHorizontal: spacing.base,
    paddingVertical: spacing.sm,
    borderRadius: radius.sm,
    borderCurve: 'continuous',
  },
  clearButtonText: {
    fontSize: 14,
    fontWeight: '600',
  },
  buttonGroup: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: spacing.sm,
  },
  exportButton: {
    paddingHorizontal: spacing.sm + 2,
    paddingVertical: spacing.sm,
    borderRadius: radius.sm,
    borderCurve: 'continuous',
    borderWidth: 1,
  },
  refreshButton: {
    paddingHorizontal: spacing.base,
    paddingVertical: spacing.sm,
    borderRadius: radius.sm,
    borderCurve: 'continuous',
  },
  refreshButtonText: {
    fontSize: 14,
    fontWeight: '600',
  },
  actionButton: {
    paddingHorizontal: spacing.base,
    paddingVertical: spacing.sm,
    borderRadius: radius.sm,
    borderCurve: 'continuous',
  },
  actionButtonText: {
    fontSize: 14,
    fontWeight: '600',
  },
  smsTestModalOverlay: {
    flex: 1,
    justifyContent: 'center',
    alignItems: 'center',
    padding: spacing.xl,
  },
  smsTestModalContent: {
    width: '100%',
    borderRadius: radius.lg,
    borderCurve: 'continuous',
    padding: spacing.lg,
    ...elevation.md,
  },
  smsTestModalTitle: {
    fontSize: typography.title3.fontSize,
    fontWeight: '600',
    marginBottom: spacing.base,
  },
  smsTestModalInput: {
    borderWidth: 1,
    borderRadius: radius.sm,
    borderCurve: 'continuous',
    padding: spacing.md,
    fontSize: 14,
    minHeight: 100,
    marginBottom: spacing.base,
  },
  statsText: {
    fontSize: 14,
    lineHeight: 22,
    marginBottom: spacing.base,
    fontFamily: Platform.OS === 'ios' ? 'Menlo' : 'monospace',
  },
  smsTestModalButtons: {
    flexDirection: 'row',
    justifyContent: 'flex-end',
    gap: spacing.md,
  },
  smsTestModalButton: {
    paddingHorizontal: spacing.lg,
    paddingVertical: spacing.sm + 2,
    borderRadius: radius.sm,
    borderCurve: 'continuous',
  },
  smsTestModalButtonText: {
    fontSize: 14,
    fontWeight: '600',
  },
});

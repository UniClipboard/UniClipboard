/**
 * History Screen
 * 历史记录页面 - 显示剪贴板历史记录
 */

import React, { useEffect, useState, useCallback, useMemo, useRef, useLayoutEffect } from 'react';
import {
  View,
  Text,
  StyleSheet,
  TextInput,
  TouchableOpacity,
  Alert,
  Share,
  Animated,
  Easing,
  ActivityIndicator,
  useWindowDimensions,
  BackHandler,
} from 'react-native';
import { Check, RefreshCw, List } from 'react-native-feather';
import Ionicons from '@expo/vector-icons/Ionicons';
import { FlashList, FlashListRef } from '@shopify/flash-list';
import { useNavigation, useFocusEffect } from '@react-navigation/native';
import * as DocumentPicker from 'expo-document-picker';
import * as ImagePicker from 'expo-image-picker';
import { TabView, TabBar, type Route } from 'react-native-tab-view';
import { useTheme } from '@/hooks/useTheme';
import { spacing, radius, typography, elevation } from '@/theme';
import { useHistoryStore } from '@/stores/historyStore';
import { useClipboardStore } from '@/stores/clipboardStore';
import { useSettingsStore } from '@/stores';
import { historyStorage } from '@/services';
import { useTransferQueueStore } from '@/stores/transferQueueStore';
import { useHistoryDisplaySettings } from '@/hooks/useHistoryDisplaySettings';
import { ClipboardItem, ClipboardContent, createDefaultClipboardItem } from '@/types/clipboard';
import { HistoryFilter } from '@/types/storage';
import { HistoryListItem } from '@/components/HistoryListItem';
import { MessageToast } from '@/components/MessageToast';
import { TopRightMenu, type MenuItemConfig } from '@/components/TopRightMenu';
import { TransferQueueModal } from '@/components/TransferQueueModal';
import { WordPickerScreen } from '@/screens/WordPickerScreen';
import { copyToLocalClipboard } from '@/utils/clipboard';
import { openFile, saveFile, shareFile, saveToGallery } from '@/utils/fileActions';
import { isTextInvalid } from '@/utils/index';
import { useMessageStore } from '@/stores/messageStore';
import { useErrorStore } from '@/stores/errorStore';
import { calculateTextHash } from '@/utils/hash';
import { importFileToHistory } from '@/utils/uploadFile';
import { isHistorySyncEnabled } from '@/utils/config';

const TAB_ROUTES: Route[] = [
  { key: 'all', title: '全部' },
  { key: 'Text', title: '文本' },
  { key: 'Image', title: '图片' },
  { key: 'File', title: '文件' },
  { key: 'starred', title: '收藏' },
];

export function HistoryScreen() {
  const navigation = useNavigation();
  const { theme } = useTheme();
  const {
    items,
    loadItems,
    searchItems,
    addItems,
    clearHistory,
    toggleStar,
    lastAddedTimestamp,
    handleStorageChange,
    setSort,
    selectedIds,
    toggleSelection,
    selectAll,
    clearSelection,
    deleteSelected,
  } = useHistoryStore();
  const { config } = useSettingsStore();

  const { showFullImage, setShowFullImage, showHistoryDebugInfo, setShowHistoryDebugInfo } =
    useHistoryDisplaySettings();
  const layout = useWindowDimensions();

  const [searchText, setSearchText] = useState('');
  const [tabIndex, setTabIndex] = useState(0);
  const [isMultiSelectMode, setIsMultiSelectMode] = useState(false);
  const [sortField, setSortField] = useState<'timestamp' | 'lastAccessed'>('timestamp');
  const { message, showMessage, clearMessage } = useMessageStore();
  const { setError } = useErrorStore();
  const isDebugMode = config?.debugMode ?? false;
  const [showTransferQueue, setShowTransferQueue] = useState(false);
  const [importingFile, setImportingFile] = useState(false);
  const [isReorganizing, setIsReorganizing] = useState(false);
  const [wordPickerText, setWordPickerText] = useState<string | null>(null);

  const {
    hasTasks,
    pendingCount,
    activeCount,
    subscribe: subscribeTransferQueue,
  } = useTransferQueueStore();

  const historySyncEnabled = useMemo(() => isHistorySyncEnabled(config), [config]);

  const ensureSyncServiceInitialized = useCallback(async (): Promise<boolean> => {
    if (!historySyncEnabled) {
      return false;
    }

    const serverConfig = config!.servers[config!.activeServerIndex];
    const { getHistorySyncService } = await import('@/services/HistorySyncService');
    const syncService = getHistorySyncService();
    return syncService.ensureInitialized(serverConfig);
  }, [config, historySyncEnabled]);

  // 加载排序设置并同步到 store
  useEffect(() => {
    const loadSortSetting = async () => {
      try {
        const AsyncStorage = require('@react-native-async-storage/async-storage').default;
        const savedSort = await AsyncStorage.getItem('@syncclipboard:history:sort_field');
        if (savedSort === 'timestamp' || savedSort === 'lastAccessed') {
          setSortField(savedSort);
          setSort({ field: savedSort, order: 'desc' });
        }
      } catch (error) {
        console.warn('[HistoryScreen] Failed to load sort setting:', error);
      }
    };
    loadSortSetting();
  }, [setSort]);

  useEffect(() => {
    return subscribeTransferQueue();
  }, [subscribeTransferQueue]);

  // 保存排序设置并重新加载数据
  const handleSortChange = useCallback(
    async (field: 'timestamp' | 'lastAccessed') => {
      setSortField(field);
      setSort({ field, order: 'desc' });
      try {
        const AsyncStorage = require('@react-native-async-storage/async-storage').default;
        await AsyncStorage.setItem('@syncclipboard:history:sort_field', field);
      } catch (error) {
        console.warn('[HistoryScreen] Failed to save sort setting:', error);
      }
      loadItems();
    },
    [setSort, loadItems]
  );

  const listRef = useRef<FlashListRef<ClipboardItem>>(null);

  // 已在历史记录页面时，再次点击导航栏按钮回到顶部
  useEffect(() => {
    const unsubscribe = (
      navigation as { addListener: (event: string, callback: () => void) => () => void }
    ).addListener('tabPress', () => {
      if (navigation.isFocused()) {
        listRef.current?.scrollToOffset({ offset: 0, animated: true });
      }
    });
    return unsubscribe;
  }, [navigation]);

  // 搜索防抖（含初始加载）— 仅按关键词搜索，分类筛选由 TabView 在客户端完成
  useEffect(() => {
    if (isReorganizing) {
      return;
    }

    const timer = setTimeout(() => {
      const filter: HistoryFilter | undefined = searchText ? { keyword: searchText } : undefined;
      searchItems(filter);
    }, 300);

    return () => clearTimeout(timer);
  }, [searchText, searchItems, isReorganizing]);

  // 监听新项目添加，滚动到顶部
  useEffect(() => {
    if (lastAddedTimestamp > 0) {
      requestAnimationFrame(() => {
        requestAnimationFrame(() => {
          requestAnimationFrame(() => {
            requestAnimationFrame(() => {
              listRef.current?.scrollToOffset({ offset: 0, animated: true });
            });
          });
        });
      });
    }
  }, [lastAddedTimestamp]);

  // 监听 HistoryStorage 变更，实时更新 UI
  useEffect(() => {
    const { HistoryStorage } = require('@/services/HistoryStorage');
    const storage = HistoryStorage.getInstance();

    const handleChange = (items: ClipboardItem[], action: 'add' | 'update' | 'delete') => {
      handleStorageChange(items, action);
    };

    storage.addChangeCallback(handleChange);

    return () => {
      storage.removeChangeCallback(handleChange);
    };
  }, [handleStorageChange]);

  // 排序数据（排序已在 store 层面完成，这里直接返回）
  const sortedItems = useMemo(() => {
    return items;
  }, [items]);

  // ClipboardItem 转换为 ClipboardContent 后调用公共复制函数
  const copyItemWithSync = useCallback(async (item: ClipboardItem) => {
    const content: ClipboardContent = {
      type: item.type,
      text: item.text,
      profileHash: item.profileHash,
      fileUri: item.fileUri,
      fileName: item.dataName,
      fileSize: item.size,
      timestamp: item.timestamp,
      localClipboardHash: item.localClipboardHash,
      hasData: item.hasData,
    };
    const result = await copyToLocalClipboard(content);
    if (result.success) {
      useClipboardStore.getState().setCurrentContentDisplay(content);
      // 更新 lastAccessed，使按访问时间排序时记录移到顶部
      historyStorage.updateLastAccessed(item.profileHash);
    }
    return result;
  }, []);

  // 点击列表项 - 复制到剪贴板
  const handleItemPress = useCallback(
    async (item: ClipboardItem) => {
      const result = await copyItemWithSync(item);
      if (result.success) {
        showMessage(result.message, 'success');
      } else {
        showMessage(result.message || '复制失败', 'error');
      }
    },
    [showMessage, copyItemWithSync]
  );

  // 分享项目
  const handleShare = useCallback(
    async (item: ClipboardItem) => {
      try {
        if (item.type === 'Text' && !isTextInvalid(item.text)) {
          await Share.share({
            message: item.text,
            title: '分享文本',
          });
        } else if (item.fileUri) {
          await shareFile(item.fileUri, item.dataName);
        } else {
          showMessage('暂不支持分享此类型的内容', 'info');
        }
      } catch (error) {
        console.error('[HistoryScreen] Failed to share:', error);
        showMessage('分享失败', 'error');
      }
    },
    [showMessage]
  );

  // 储存文件到设备（图片类型保存到相册）
  const handleSave = useCallback(
    async (item: ClipboardItem) => {
      if (!item.fileUri) return;
      try {
        if (item.type === 'Image') {
          await saveToGallery(item.fileUri);
          showMessage('已保存到相册', 'success');
        } else {
          await saveFile(item.fileUri, item.dataName);
          showMessage('已储存到设备', 'success');
        }
      } catch (error) {
        if (error instanceof Error && error.message === 'Storage permission denied') {
          showMessage('已取消储存', 'info');
          return;
        }
        if (error instanceof Error && error.message === 'Media library permission denied') {
          showMessage('需要相册权限才能保存图片', 'error');
          return;
        }
        console.error('[HistoryScreen] Failed to save file:', error);
        showMessage('储存失败', 'error');
      }
    },
    [showMessage]
  );

  // 打开文件
  const handleOpen = useCallback(
    async (item: ClipboardItem) => {
      if (!item.fileUri) return;
      try {
        await openFile(item.fileUri);
      } catch (error) {
        console.error('[HistoryScreen] Failed to open file:', error);
        showMessage('打开失败', 'error');
      }
    },
    [showMessage]
  );

  // 切换收藏状态
  const handleToggleStar = useCallback(
    async (item: ClipboardItem) => {
      try {
        await toggleStar(item.profileHash);
        // 同步由 HistorySyncService.handleLocalHistoryChanged 自动处理
      } catch (error) {
        console.error('[HistoryScreen] Failed to toggle star:', error);
        showMessage('操作失败', 'error');
      }
    },
    [toggleStar, showMessage]
  );

  // 长按进入多选模式
  const handleItemLongPress = useCallback(
    (item: ClipboardItem) => {
      if (!isMultiSelectMode) {
        setIsMultiSelectMode(true);
        clearSelection();
      }
      toggleSelection(item.profileHash);
    },
    [isMultiSelectMode, clearSelection, toggleSelection]
  );

  // 多选模式下点击 item 切换选中
  const handleMultiSelectPress = useCallback(
    (item: ClipboardItem) => {
      toggleSelection(item.profileHash);
    },
    [toggleSelection]
  );

  // 退出多选模式
  const exitMultiSelectMode = useCallback(() => {
    setIsMultiSelectMode(false);
    clearSelection();
  }, [clearSelection]);

  // 多选模式下拦截系统返回键，退出多选而不是返回上一页
  useEffect(() => {
    if (!isMultiSelectMode) return;

    const handler = BackHandler.addEventListener('hardwareBackPress', () => {
      exitMultiSelectMode();
      return true;
    });
    return () => handler.remove();
  }, [isMultiSelectMode, exitMultiSelectMode]);

  // 批量删除
  const handleBatchDelete = useCallback(() => {
    const count = selectedIds.size;
    if (count === 0) return;
    Alert.alert('确认删除', `确定要删除选中的 ${count} 条记录吗？`, [
      { text: '取消', style: 'cancel' },
      {
        text: '删除',
        style: 'destructive',
        onPress: async () => {
          await deleteSelected();
          setIsMultiSelectMode(false);
        },
      },
    ]);
  }, [selectedIds.size, deleteSelected]);

  // 清空所有历史记录
  const handleClearAll = useCallback(() => {
    Alert.alert(
      '确认清空',
      '确定要清空所有历史记录吗？此操作不可撤销，不会删除服务器上已同步的记录。',
      [
        { text: '取消', style: 'cancel' },
        {
          text: '清空',
          style: 'destructive',
          onPress: async () => {
            try {
              await clearHistory();
              const { getHistorySyncService } = await import('@/services/HistorySyncService');
              const syncService = getHistorySyncService();
              await syncService.resetSyncCursor();
              showMessage('已清空所有历史记录', 'success');
            } catch (error) {
              console.error('[HistoryScreen] Failed to clear:', error);
              showMessage('清空失败', 'error');
            }
          },
        },
      ]
    );
  }, [clearHistory, showMessage]);

  // 添加文件到历史记录
  const handleImportFile = useCallback(async () => {
    try {
      const result = await DocumentPicker.getDocumentAsync({
        multiple: false,
      });

      if (result.canceled) {
        return;
      }

      const asset = result.assets?.[0];
      if (!asset) {
        showMessage('未选择文件', 'error');
        return;
      }

      const fileName = asset.name || 'file';

      setImportingFile(true);

      await new Promise<void>((resolve) => {
        requestAnimationFrame(() => {
          requestAnimationFrame(() => resolve());
        });
      });

      await importFileToHistory(asset.uri, fileName, asset.mimeType, asset.size);

      showMessage(`文件 ${fileName} 已添加到历史记录`, 'success');
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : '添加文件失败，请重试';
      console.error('[HistoryScreen] Failed to import file:', error);
      showMessage(errorMessage, 'error');
    } finally {
      setImportingFile(false);
    }
  }, [showMessage]);

  // 添加图片到历史记录
  const handleImportImage = useCallback(async () => {
    try {
      const result = await ImagePicker.launchImageLibraryAsync({
        mediaTypes: ['images'],
        quality: 1,
      });

      if (result.canceled) {
        return;
      }

      const asset = result.assets?.[0];
      if (!asset) {
        showMessage('未选择图片', 'error');
        return;
      }

      const fileName = asset.fileName || `image_${Date.now()}.jpg`;

      setImportingFile(true);

      await new Promise<void>((resolve) => {
        requestAnimationFrame(() => {
          requestAnimationFrame(() => resolve());
        });
      });

      await importFileToHistory(asset.uri, fileName, asset.mimeType, asset.fileSize);

      showMessage(`图片 ${fileName} 已添加到历史记录`, 'success');
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : '添加图片失败，请重试';
      console.error('[HistoryScreen] Failed to import image:', error);
      showMessage(errorMessage, 'error');
    } finally {
      setImportingFile(false);
    }
  }, [showMessage]);

  // 重新同步历史记录
  const [isSyncing, setIsSyncing] = useState(false);
  const [isRefreshing, setIsRefreshing] = useState(false);

  // 同步动画
  const syncRotation = useRef(new Animated.Value(0)).current;
  const syncAnimationRef = useRef<Animated.CompositeAnimation | null>(null);

  useEffect(() => {
    if (isSyncing) {
      syncRotation.setValue(0);
      const animation = Animated.loop(
        Animated.timing(syncRotation, {
          toValue: 1,
          duration: 1000,
          easing: Easing.linear,
          useNativeDriver: true,
        })
      );
      animation.start();
      syncAnimationRef.current = animation;
    } else {
      syncAnimationRef.current?.stop();
      syncAnimationRef.current = null;
      syncRotation.setValue(0);
    }
  }, [isSyncing, syncRotation]);

  const syncRotationInterpolate = syncRotation.interpolate({
    inputRange: [0, 1],
    outputRange: ['0deg', '360deg'],
  });

  const handleResyncHistory = useCallback(async () => {
    if (isSyncing) return;

    const initialized = await ensureSyncServiceInitialized();
    if (!initialized) {
      showMessage('请先配置服务器', 'error');
      return;
    }

    const { getHistorySyncService } = await import('@/services/HistorySyncService');
    const syncService = getHistorySyncService();

    setIsSyncing(true);
    showMessage('开始同步历史记录...', 'info');

    try {
      await syncService.syncAll((progress: { message?: string }) => {
        if (progress.message) {
          console.log(`[HistoryScreen] Sync progress: ${progress.message}`);
        }
      });
      showMessage('历史记录同步完成', 'success');
    } catch (error) {
      console.error('[HistoryScreen] Failed to resync history:', error);
      const errorMessage = error instanceof Error ? error.message : '未知错误';
      setError({
        title: '历史记录同步失败',
        message: errorMessage,
      });
      showMessage('同步失败: ' + errorMessage, 'error');
    } finally {
      setIsSyncing(false);
    }
  }, [isSyncing, showMessage, setError, ensureSyncServiceInitialized]);

  const handleIncrementalSync = useCallback(
    async (isPullToRefresh = false) => {
      if (isSyncing) return;

      const initialized = await ensureSyncServiceInitialized();
      if (!initialized) {
        return;
      }

      const { getHistorySyncService } = await import('@/services/HistorySyncService');
      const syncService = getHistorySyncService();

      setIsSyncing(true);
      if (isPullToRefresh) {
        setIsRefreshing(true);
      }

      try {
        await syncService.syncIncremental();
      } catch (error) {
        console.error('[HistoryScreen] Failed to incremental sync:', error);
        const errorMessage = error instanceof Error ? error.message : '未知错误';
        setError({
          title: '历史记录增量同步失败',
          message: errorMessage,
        });
      } finally {
        setIsSyncing(false);
        if (isPullToRefresh) {
          setIsRefreshing(false);
        }
      }
    },
    [isSyncing, setError, ensureSyncServiceInitialized]
  );

  useFocusEffect(
    useCallback(() => {
      const onScreenFocus = async () => {
        const currentConfig = useSettingsStore.getState().config;
        if (currentConfig?.needsHistoryReorganize) {
          const latestConfig = useSettingsStore.getState().config;
          const shouldReorganize = !isHistorySyncEnabled(latestConfig);

          if (!shouldReorganize) {
            console.log('[HistoryScreen] Skipped history reorganization (sync re-enabled)');
            await useSettingsStore.getState().updateConfig({ needsHistoryReorganize: false });
          } else {
            setIsReorganizing(true);
            console.log('[HistoryScreen] Starting history reorganization...');

            const { HistoryStorage } = await import('@/services/HistoryStorage');
            const { getHistorySyncService } = await import('@/services/HistorySyncService');
            const historyStorage = HistoryStorage.getInstance();
            const syncService = getHistorySyncService();

            const abortController = new AbortController();
            syncService.setReorganizeAbortController(abortController);

            historyStorage.beginSilentMode();

            try {
              await syncService.cleanupRemoteHistorys(abortController.signal);
              await historyStorage.cleanupByCount();
              console.log('[HistoryScreen] History reorganization completed');
              await useSettingsStore.getState().updateConfig({ needsHistoryReorganize: false });
            } catch (error) {
              if (error instanceof DOMException && error.name === 'AbortError') {
                console.log('[HistoryScreen] History reorganization cancelled');
              } else {
                console.error('[HistoryScreen] History reorganization failed:', error);
              }
            } finally {
              syncService.setReorganizeAbortController(null);
              setIsReorganizing(false);
              historyStorage.endSilentMode();
              await useHistoryStore.getState().refresh();
            }
          }
        }

        if (historySyncEnabled) {
          console.log('[HistoryScreen] Screen focused, starting incremental sync');
          const { getHistorySyncService } = await import('@/services/HistorySyncService');
          const syncService = getHistorySyncService();
          const serverConfig = currentConfig?.servers[currentConfig?.activeServerIndex];
          if (serverConfig) {
            const initialized = await syncService.ensureInitialized(serverConfig);
            if (initialized) {
              if (syncService.isSyncInProgress()) {
                console.log('[HistoryScreen] Sync already in progress, showing indicator');
                setIsSyncing(true);
                const progressCallback = (progress: { phase: string }) => {
                  if (progress.phase === 'completed' || progress.phase === 'error') {
                    syncService.removeProgressCallback(progressCallback);
                    setIsSyncing(false);
                  }
                };
                syncService.addProgressCallback(progressCallback);
              } else {
                setIsSyncing(true);
                try {
                  await syncService.syncIncremental();
                } catch (error) {
                  console.error('[HistoryScreen] Failed to incremental sync:', error);
                  const errorMessage = error instanceof Error ? error.message : '未知错误';
                  setError({
                    title: '历史记录增量同步失败',
                    message: errorMessage,
                  });
                } finally {
                  setIsSyncing(false);
                }
              }
            }
          }
        }
      };

      onScreenFocus();
    }, [historySyncEnabled])
  );

  const generateRandomDebugText = useCallback(() => {
    const randomInt = (min: number, max: number) =>
      Math.floor(Math.random() * (max - min + 1)) + min;

    const totalChars = randomInt(0, 100);
    const lineCount = randomInt(0, 5);

    if (totalChars === 0 || lineCount === 0) {
      return '';
    }

    const chars =
      'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789，。！？；：、-_[](){}<>/\\@#%&*+= 空格测试随机内容';

    const buildLine = (length: number) => {
      let line = '';
      for (let i = 0; i < length; i++) {
        const index = randomInt(0, chars.length - 1);
        line += chars[index];
      }
      return line;
    };

    let remaining = totalChars;
    const lines: string[] = [];

    for (let i = 0; i < lineCount; i++) {
      const isLastLine = i === lineCount - 1;
      const currentLength = isLastLine ? remaining : randomInt(0, remaining);
      lines.push(buildLine(currentLength));
      remaining -= currentLength;
    }

    return lines.join('\n');
  }, []);

  const handleAddRandomRecords = useCallback(async () => {
    try {
      const now = Date.now();
      const randomItems = await Promise.all(
        Array.from({ length: 10 }, async (_unused, index) => {
          const seed = `debug-random-${now}-${index}-${Math.random().toString(36).slice(2, 10)}`;
          const profileHash = await calculateTextHash(seed);

          return createDefaultClipboardItem({
            type: 'Text' as const,
            text: generateRandomDebugText(),
            profileHash,
            hasData: false,
            timestamp: now - index * 1000,
          });
        })
      );

      await addItems(randomItems);

      showMessage('已添加10条随机记录', 'success');
    } catch (error) {
      console.error('[HistoryScreen] Failed to add random records:', error);
      showMessage('添加随机记录失败', 'error');
    }
  }, [addItems, showMessage, generateRandomDebugText]);

  // 加载更多已移除：虚拟列表性能足够，不再需要分页
  const handleEndReached = useCallback(() => {
    // no-op: 已移除分页，所有数据一次性加载
  }, []);

  const handleClearSearch = useCallback(() => {
    setSearchText('');
  }, []);

  // 切换完整图片显示
  const handleToggleFullImage = useCallback(async () => {
    await setShowFullImage(!showFullImage);
  }, [showFullImage, setShowFullImage]);

  const handleToggleHistoryDebugInfo = useCallback(async () => {
    await setShowHistoryDebugInfo(!showHistoryDebugInfo);
  }, [showHistoryDebugInfo, setShowHistoryDebugInfo]);

  const handleDownload = useCallback(
    async (item: ClipboardItem) => {
      console.log(`[HistoryScreen] ========== Download Button Clicked ==========`);
      console.log(`[HistoryScreen] Item profileHash: ${item.profileHash}`);
      console.log(`[HistoryScreen] Item type: ${item.type}`);
      console.log(`[HistoryScreen] Item dataName: ${item.dataName}`);
      console.log(`[HistoryScreen] Item hasRemoteData: ${item.hasRemoteData}`);
      console.log(`[HistoryScreen] Item isLocalFileReady: ${item.isLocalFileReady}`);

      const initialized = await ensureSyncServiceInitialized();
      if (!initialized) {
        showMessage('历史同步未启用', 'error');
        return;
      }

      const { getHistoryTransferQueue } = await import('@/services/HistoryTransferQueue');
      const { getProfileId } = await import('@/services/HistoryAPI');

      const profileId = getProfileId(item.type, item.profileHash);
      console.log(`[HistoryScreen] Generated profileId: ${profileId}`);

      const queue = getHistoryTransferQueue();
      queue.start();
      await queue.addDownloadTask(profileId, true);
    },
    [ensureSyncServiceInitialized, showMessage]
  );

  const handleUpload = useCallback(
    async (item: ClipboardItem) => {
      console.log(`[HistoryScreen] ========== Upload Button Clicked ==========`);
      console.log(`[HistoryScreen] Item profileHash: ${item.profileHash}`);
      console.log(`[HistoryScreen] Item type: ${item.type}`);
      console.log(`[HistoryScreen] Item isLocalFileReady: ${item.isLocalFileReady}`);
      console.log(`[HistoryScreen] Item syncStatus: ${item.syncStatus}`);

      const initialized = await ensureSyncServiceInitialized();
      if (!initialized) {
        showMessage('历史同步未启用', 'error');
        return;
      }

      // 验证本地文件是否存在
      if (item.fileUri) {
        const { File } = await import('expo-file-system');
        const file = new File(item.fileUri);
        if (!file.exists) {
          console.warn(`[HistoryScreen] Local file not found: ${item.fileUri}`);
          showMessage('本地文件不存在', 'error');
          return;
        }
      }

      const { getHistoryTransferQueue } = await import('@/services/HistoryTransferQueue');
      const { getProfileId } = await import('@/services/HistoryAPI');

      const profileId = getProfileId(item.type, item.profileHash);
      console.log(`[HistoryScreen] Generated profileId: ${profileId}`);

      const queue = getHistoryTransferQueue();
      queue.start();
      await queue.addUploadTask(profileId, true);
    },
    [ensureSyncServiceInitialized, showMessage]
  );

  const renderItem = useCallback(
    ({ item }: { item: ClipboardItem }) => {
      return (
        <HistoryListItem
          item={item}
          onCopy={handleItemPress}
          onShare={handleShare}
          onSave={handleSave}
          onOpen={handleOpen}
          onLongPress={handleItemLongPress}
          onPress={handleMultiSelectPress}
          onToggleStar={handleToggleStar}
          onDownload={historySyncEnabled ? handleDownload : undefined}
          onUpload={historySyncEnabled ? handleUpload : undefined}
          onWordPick={setWordPickerText}
          showFullImage={showFullImage}
          showDebugInfo={isDebugMode && showHistoryDebugInfo}
          enableHistorySync={historySyncEnabled}
          isMultiSelectMode={isMultiSelectMode}
          isSelected={selectedIds.has(item.profileHash)}
          showImageCopyButton={config?.showImageCopyButton ?? false}
        />
      );
    },
    [
      handleItemPress,
      handleShare,
      handleOpen,
      handleItemLongPress,
      handleMultiSelectPress,
      handleToggleStar,
      handleDownload,
      handleUpload,
      showFullImage,
      isDebugMode,
      showHistoryDebugInfo,
      historySyncEnabled,
      isMultiSelectMode,
      selectedIds,
      config?.showImageCopyButton,
    ]
  );

  // 渲染空状态
  const renderEmptyComponent = useCallback(() => {
    return (
      <View style={styles.emptyContainer}>
        <Text style={styles.emptyIcon}>📋</Text>
        <Text style={[styles.emptyTitle, { color: theme.colors.text }]}>暂无历史记录</Text>
        <Text style={[styles.emptyDescription, { color: theme.colors.textSecondary }]}>
          {searchText ? '未找到匹配的记录' : '复制内容后将自动保存到历史记录'}
        </Text>
      </View>
    );
  }, [theme, searchText]);

  // 客户端分类筛选
  const filteredItemsByTab = useMemo(() => {
    const result: Record<string, ClipboardItem[]> = {
      all: sortedItems,
      Text: sortedItems.filter((item) => item.type === 'Text'),
      Image: sortedItems.filter((item) => item.type === 'Image'),
      File: sortedItems.filter((item) => item.type === 'File'),
      starred: sortedItems.filter((item) => item.starred),
    };
    return result;
  }, [sortedItems]);

  // 渲染每个 Tab 的内容
  const renderScene = useCallback(
    ({ route }: { route: Route }) => {
      const tabItems = filteredItemsByTab[route.key] || [];
      return (
        <FlashList
          ref={route.key === TAB_ROUTES[tabIndex]?.key ? listRef : undefined}
          data={tabItems}
          renderItem={renderItem}
          keyExtractor={(item) => item.profileHash}
          onEndReached={handleEndReached}
          onEndReachedThreshold={0.5}
          ListEmptyComponent={renderEmptyComponent}
          contentContainerStyle={styles.listContent}
          refreshing={isRefreshing}
          onRefresh={historySyncEnabled ? () => handleIncrementalSync(true) : undefined}
        />
      );
    },
    [
      filteredItemsByTab,
      tabIndex,
      renderItem,
      handleEndReached,
      renderEmptyComponent,
      isRefreshing,
      historySyncEnabled,
      handleIncrementalSync,
    ]
  );

  // 自定义 TabBar — M3 风:主色 indicator + onSurfaceVariant 未激活态
  const renderTabBar = useCallback(
    (props: Parameters<NonNullable<React.ComponentProps<typeof TabView>['renderTabBar']>>[0]) => (
      <TabBar
        {...props}
        style={[styles.tabBar, { backgroundColor: theme.colors.surface }]}
        indicatorStyle={[styles.tabIndicator, { backgroundColor: theme.colors.primary }]}
        activeColor={theme.colors.primary}
        inactiveColor={theme.colors.onSurfaceVariant}
        pressColor={theme.colors.outlineVariant}
      />
    ),
    [theme]
  );

  // 菜单项配置
  const menuItems = useMemo<MenuItemConfig[]>(() => {
    const items: MenuItemConfig[] = [
      {
        label: '添加图片',
        onPress: handleImportImage,
      },
      {
        label: '添加文件',
        onPress: handleImportFile,
      },
      {
        label: '展示完整图片',
        onPress: handleToggleFullImage,
        icon: showFullImage ? <Check color="#2196F3" width={18} height={18} /> : undefined,
      },
      {
        label: '排序方式',
        submenu: [
          {
            label: '按创建时间排序',
            onPress: () => handleSortChange('timestamp'),
            icon:
              sortField === 'timestamp' ? (
                <Check color="#2196F3" width={18} height={18} />
              ) : undefined,
          },
          {
            label: '按访问时间排序',
            onPress: () => handleSortChange('lastAccessed'),
            icon:
              sortField === 'lastAccessed' ? (
                <Check color="#2196F3" width={18} height={18} />
              ) : undefined,
          },
        ],
      },
    ];

    if (historySyncEnabled) {
      items.push({
        label: isSyncing ? '同步中...' : '重新同步历史记录',
        onPress: handleResyncHistory,
        disabled: isSyncing,
      });
    }

    if (isDebugMode) {
      items.push({
        label: '显示历史记录调试信息',
        onPress: handleToggleHistoryDebugInfo,
        icon: showHistoryDebugInfo ? <Check color="#2196F3" width={18} height={18} /> : undefined,
      });
      items.push({
        label: '添加10条随机记录',
        onPress: handleAddRandomRecords,
      });
    }

    items.push({
      label: '清空所有历史记录',
      onPress: handleClearAll,
      destructive: true,
    });

    return items;
  }, [
    showFullImage,
    sortField,
    isDebugMode,
    showHistoryDebugInfo,
    isSyncing,
    historySyncEnabled,
    handleImportImage,
    handleImportFile,
    handleToggleFullImage,
    handleToggleHistoryDebugInfo,
    handleSortChange,
    handleResyncHistory,
    handleAddRandomRecords,
    handleClearAll,
  ]);

  // 设置标题栏菜单按钮
  useLayoutEffect(() => {
    navigation.setOptions({
      headerShadowVisible: false,
      headerStyle: {
        backgroundColor: theme.colors.surface,
        elevation: 0,
        shadowOpacity: 0,
        borderBottomWidth: 0,
      },
      headerRight: () => (
        <View style={styles.headerRightContainer}>
          {hasTasks && (
            <TouchableOpacity
              style={styles.queueButton}
              onPress={() => setShowTransferQueue(true)}
              hitSlop={{ top: 10, right: 5, bottom: 10, left: 5 }}
            >
              <List width={20} height={20} color={theme.colors.primary} />
              <View style={[styles.queueBadge, { backgroundColor: theme.colors.primary }]}>
                <Text style={[styles.queueBadgeText, { color: theme.colors.white }]}>
                  {activeCount + pendingCount}
                </Text>
              </View>
            </TouchableOpacity>
          )}
          {isSyncing && (
            <Animated.View
              style={[styles.syncIndicator, { transform: [{ rotate: syncRotationInterpolate }] }]}
            >
              <RefreshCw width={18} height={18} color={theme.colors.primary} />
            </Animated.View>
          )}
          {!isReorganizing && <TopRightMenu items={menuItems} />}
        </View>
      ),
    });
  }, [
    navigation,
    theme.colors.surface,
    theme.colors.primary,
    theme.colors.textSecondary,
    menuItems,
    isSyncing,
    syncRotationInterpolate,
    hasTasks,
    activeCount,
    pendingCount,
    isReorganizing,
  ]);

  return (
    <View style={[styles.container, { backgroundColor: theme.colors.background }]}>
      {/* 整理中提示 */}
      {isReorganizing && (
        <View style={[styles.reorganizingOverlay, { backgroundColor: theme.colors.background }]}>
          <ActivityIndicator size="large" color={theme.colors.primary} />
          <Text style={[styles.reorganizingText, { color: theme.colors.text }]}>
            正在整理历史记录...
          </Text>
        </View>
      )}
      {/* 搜索栏 — M3 filled search field(pill,无边框)*/}
      <View style={[styles.searchContainer, { backgroundColor: theme.colors.surface }]}>
        <TextInput
          style={[
            styles.searchInput,
            {
              backgroundColor: theme.colors.surfaceContainerHigh,
              color: theme.colors.text,
            },
          ]}
          placeholder="搜索历史记录..."
          placeholderTextColor={theme.colors.onSurfaceVariant}
          value={searchText}
          onChangeText={setSearchText}
          clearButtonMode="while-editing"
        />
        <TouchableOpacity
          style={styles.clearSearchButton}
          onPress={handleClearSearch}
          disabled={!searchText}
        >
          <Text
            style={[
              styles.clearSearchButtonText,
              {
                color: searchText ? theme.colors.primary : theme.colors.onSurfaceVariant,
              },
            ]}
          >
            清除
          </Text>
        </TouchableOpacity>
      </View>

      {/* 分类 TabView */}
      <TabView
        navigationState={{ index: tabIndex, routes: TAB_ROUTES }}
        renderScene={renderScene}
        onIndexChange={setTabIndex}
        initialLayout={{ width: layout.width }}
        renderTabBar={renderTabBar}
        lazy
      />

      {/* 多选操作栏 */}
      {isMultiSelectMode && (
        <View
          style={[
            styles.multiSelectBar,
            { backgroundColor: theme.colors.surface, borderTopColor: theme.colors.border },
          ]}
        >
          <Text style={[styles.multiSelectCount, { color: theme.colors.text }]}>
            已选 {selectedIds.size} 项
          </Text>
          <TouchableOpacity onPress={exitMultiSelectMode} style={styles.multiSelectButton}>
            <Ionicons name="close" size={22} color={theme.colors.text} />
            <Text style={[styles.multiSelectButtonText, { color: theme.colors.text }]}>取消</Text>
          </TouchableOpacity>
          <TouchableOpacity
            onPress={() => (selectedIds.size === items.length ? clearSelection() : selectAll())}
            style={styles.multiSelectButton}
          >
            <Ionicons
              name={selectedIds.size === items.length ? 'checkbox' : 'checkbox-outline'}
              size={22}
              color={theme.colors.primary}
            />
            <Text style={[styles.multiSelectButtonText, { color: theme.colors.primary }]}>
              全选
            </Text>
          </TouchableOpacity>
          <TouchableOpacity
            onPress={handleBatchDelete}
            style={styles.multiSelectButton}
            disabled={selectedIds.size === 0}
          >
            <Ionicons
              name="trash-outline"
              size={22}
              color={
                selectedIds.size > 0 ? theme.colors.error || '#F44336' : theme.colors.textTertiary
              }
            />
            <Text
              style={[
                styles.multiSelectButtonText,
                {
                  color:
                    selectedIds.size > 0
                      ? theme.colors.error || '#F44336'
                      : theme.colors.textTertiary,
                },
              ]}
            >
              删除
            </Text>
          </TouchableOpacity>
        </View>
      )}

      {/* 传输队列弹窗 */}
      <TransferQueueModal visible={showTransferQueue} onClose={() => setShowTransferQueue(false)} />

      {/* 消息提示 */}
      <MessageToast message={message} onMessageShown={clearMessage} />

      {/* 导入文件遮罩 */}
      {importingFile && (
        <View style={[styles.importOverlay, { backgroundColor: theme.colors.backdrop }]}>
          <View
            style={[
              styles.importOverlayCard,
              { backgroundColor: theme.colors.surfaceContainerHigh },
            ]}
          >
            <ActivityIndicator size="large" color={theme.colors.primary} />
            <Text style={[styles.importOverlayTitle, { color: theme.colors.text }]}>
              正在添加文件...
            </Text>
          </View>
        </View>
      )}

      {/* 分词选择页面 */}
      {wordPickerText && (
        <View style={StyleSheet.absoluteFill}>
          <WordPickerScreen text={wordPickerText} onComplete={() => setWordPickerText(null)} />
        </View>
      )}
    </View>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
  },
  reorganizingOverlay: {
    position: 'absolute',
    top: 0,
    left: 0,
    right: 0,
    bottom: 0,
    justifyContent: 'center',
    alignItems: 'center',
    zIndex: 1000,
  },
  reorganizingText: {
    marginTop: 16,
    fontSize: 16,
  },
  headerRightContainer: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 4,
  },
  syncIndicator: {
    width: 28,
    height: 28,
    justifyContent: 'center',
    alignItems: 'center',
  },
  queueButton: {
    width: 32,
    height: 32,
    justifyContent: 'center',
    alignItems: 'center',
    marginRight: 4,
  },
  queueBadge: {
    position: 'absolute',
    top: -4,
    right: -4,
    minWidth: 16,
    height: 16,
    borderRadius: 8,
    justifyContent: 'center',
    alignItems: 'center',
    paddingHorizontal: 4,
  },
  queueBadgeText: {
    fontSize: 10,
    fontWeight: '600',
  },
  searchContainer: {
    flexDirection: 'row',
    alignItems: 'center',
    paddingHorizontal: spacing.base,
    paddingVertical: spacing.sm,
    paddingTop: 0,
    gap: spacing.sm,
  },
  searchInput: {
    flex: 1,
    height: 44,
    borderRadius: radius.pill,
    paddingHorizontal: spacing.base,
    fontSize: typography.subhead.fontSize,
  },
  clearSearchButton: {
    paddingHorizontal: 8,
    paddingVertical: 6,
  },
  clearSearchButtonText: {
    fontSize: 14,
    fontWeight: '500',
  },
  tabBar: {
    elevation: 0,
    shadowOpacity: 0,
  },
  tabIndicator: {
    height: 3,
    borderRadius: 1.5,
    width: 24,
    marginHorizontal: 'auto',
  },
  tab: {
    width: 'auto',
    paddingHorizontal: 16,
  },
  tabLabel: {
    fontSize: 14,
    fontWeight: '500',
    textTransform: 'none',
  },
  listContent: {
    paddingVertical: 8,
  },
  emptyContainer: {
    flex: 1,
    alignItems: 'center',
    justifyContent: 'center',
    paddingVertical: 80,
  },
  emptyIcon: {
    fontSize: 64,
    marginBottom: 16,
  },
  emptyTitle: {
    fontSize: 18,
    fontWeight: '600',
    marginBottom: 8,
  },
  emptyDescription: {
    fontSize: 15,
    textAlign: 'center',
  },
  // 多选操作栏样式
  multiSelectBar: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    paddingVertical: 8,
    paddingHorizontal: 12,
    borderTopWidth: StyleSheet.hairlineWidth,
  },
  multiSelectButton: {
    alignItems: 'center',
    paddingHorizontal: 12,
    paddingVertical: 4,
  },
  multiSelectButtonText: {
    fontSize: 11,
    marginTop: 2,
  },
  multiSelectCount: {
    flex: 1,
    fontSize: 14,
    fontWeight: '500',
    textAlign: 'left',
    paddingLeft: 8,
  },
  importOverlay: {
    ...StyleSheet.absoluteFill,
    justifyContent: 'center',
    alignItems: 'center',
  },
  importOverlayCard: {
    borderRadius: radius.lg,
    borderCurve: 'continuous',
    padding: spacing.xl,
    alignItems: 'center',
    minWidth: 160,
    ...elevation.md,
  },
  importOverlayTitle: {
    marginTop: 16,
    fontSize: 16,
    fontWeight: '500',
  },
});

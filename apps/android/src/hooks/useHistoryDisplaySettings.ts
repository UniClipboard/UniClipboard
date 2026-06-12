/**
 * History Display Settings Hook
 * 历史记录显示设置 - 使用本地 AsyncStorage，避免影响全局状态
 */

import { useState, useCallback, useEffect } from 'react';
import AsyncStorage from '@react-native-async-storage/async-storage';

const STORAGE_KEY = '@syncclipboard:history_display_settings';

interface HistoryDisplaySettings {
  showFullImage: boolean;
  showHistoryDebugInfo: boolean;
}

const DEFAULT_SETTINGS: HistoryDisplaySettings = {
  showFullImage: false,
  showHistoryDebugInfo: false,
};

export function useHistoryDisplaySettings() {
  const [settings, setSettings] = useState<HistoryDisplaySettings>(DEFAULT_SETTINGS);
  const [isLoading, setIsLoading] = useState(true);

  // 加载设置
  useEffect(() => {
    loadSettings();
  }, []);

  const loadSettings = async () => {
    try {
      const stored = await AsyncStorage.getItem(STORAGE_KEY);
      if (stored) {
        const parsed = JSON.parse(stored);
        setSettings({ ...DEFAULT_SETTINGS, ...parsed });
      }
    } catch (error) {
      console.error('[HistoryDisplaySettings] Failed to load settings:', error);
    } finally {
      setIsLoading(false);
    }
  };

  // 更新设置
  const setShowFullImage = useCallback(
    async (showFullImage: boolean) => {
      const newSettings = { ...settings, showFullImage };
      setSettings(newSettings);

      try {
        await AsyncStorage.setItem(STORAGE_KEY, JSON.stringify(newSettings));
      } catch (error) {
        console.error('[HistoryDisplaySettings] Failed to save settings:', error);
      }
    },
    [settings]
  );

  const setShowHistoryDebugInfo = useCallback(
    async (showHistoryDebugInfo: boolean) => {
      const newSettings = { ...settings, showHistoryDebugInfo };
      setSettings(newSettings);

      try {
        await AsyncStorage.setItem(STORAGE_KEY, JSON.stringify(newSettings));
      } catch (error) {
        console.error('[HistoryDisplaySettings] Failed to save settings:', error);
      }
    },
    [settings]
  );

  return {
    showFullImage: settings.showFullImage,
    setShowFullImage,
    showHistoryDebugInfo: settings.showHistoryDebugInfo,
    setShowHistoryDebugInfo,
    isLoading,
  };
}

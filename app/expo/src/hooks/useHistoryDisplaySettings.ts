/**
 * History Display Settings Hook
 * 历史记录显示设置 - 使用本地 AsyncStorage，避免影响全局状态
 */

import { useState, useCallback, useEffect, useRef } from 'react';
import AsyncStorage from '@react-native-async-storage/async-storage';




const STORAGE_KEY = '@uniclipboard-shell:history_display_settings';

/** 首页历史的显示方式:卡片网格 / 分组列表 / 紧凑分组列表。按设备保存,不随同步。 */
export type HistoryLayout = 'grid' | 'list' | 'compact';

const HISTORY_LAYOUTS: readonly HistoryLayout[] = ['grid', 'list', 'compact'];

interface HistoryDisplaySettings {
  showFullImage: boolean;
  showHistoryDebugInfo: boolean;
  historyLayout: HistoryLayout;
}

const DEFAULT_SETTINGS: HistoryDisplaySettings = {
  showFullImage: false,
  showHistoryDebugInfo: false,
  // 现有用户保持网格,列表需要在「显示方式」里主动切换
  historyLayout: 'grid',
};

function parseSettings(stored: string | null): HistoryDisplaySettings {
  if (!stored) return DEFAULT_SETTINGS;
  const parsed = { ...DEFAULT_SETTINGS, ...JSON.parse(stored) };
  if (!HISTORY_LAYOUTS.includes(parsed.historyLayout)) {
    parsed.historyLayout = DEFAULT_SETTINGS.historyLayout;
  }
  return parsed;
}

export function useHistoryDisplaySettings() {
  const [settings, setSettings] = useState<HistoryDisplaySettings>(DEFAULT_SETTINGS);
  const [isLoading, setIsLoading] = useState(true);
  // 连续修改时基于最新值合并,不依赖渲染闭包里可能过期的 settings
  const latestRef = useRef(settings);

  useEffect(() => {
    let cancelled = false;
    AsyncStorage.getItem(STORAGE_KEY)
      .then((stored) => {
        if (cancelled) return;
        latestRef.current = parseSettings(stored);
        setSettings(latestRef.current);
      })
      .catch((error) => console.warn('Failed to load settings:', error))
      .finally(() => {
        if (!cancelled) setIsLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const update = useCallback((patch: Partial<HistoryDisplaySettings>) => {
    const next = { ...latestRef.current, ...patch };
    latestRef.current = next;
    setSettings(next);
    AsyncStorage.setItem(STORAGE_KEY, JSON.stringify(next)).catch((error) =>
      console.warn('Failed to save settings:', error)
    );
  }, []);

  const setShowFullImage = useCallback(
    (showFullImage: boolean) => update({ showFullImage }),
    [update]
  );
  const setShowHistoryDebugInfo = useCallback(
    (showHistoryDebugInfo: boolean) => update({ showHistoryDebugInfo }),
    [update]
  );
  const setHistoryLayout = useCallback(
    (historyLayout: HistoryLayout) => update({ historyLayout }),
    [update]
  );

  return {
    showFullImage: settings.showFullImage,
    setShowFullImage,
    showHistoryDebugInfo: settings.showHistoryDebugInfo,
    setShowHistoryDebugInfo,
    historyLayout: settings.historyLayout,
    setHistoryLayout,
    isLoading,
  };
}

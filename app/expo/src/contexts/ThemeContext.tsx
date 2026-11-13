/**
 * 主题上下文
 * 提供深浅色(light/dark/auto)切换与访问功能
 */

import React, { createContext, useEffect, useState, useCallback } from 'react';
import { Appearance, useColorScheme } from 'react-native';
import AsyncStorage from '@react-native-async-storage/async-storage';
import { createTheme, type Theme, type ThemeMode } from '@/theme';




const THEME_STORAGE_KEY = '@uniclipboard-shell:theme_mode';

interface ThemeContextValue {
  theme: Theme;
  themeMode: ThemeMode;
  setThemeMode: (mode: ThemeMode) => Promise<void>;
  toggleTheme: () => Promise<void>;
}

export const ThemeContext = createContext<ThemeContextValue | undefined>(undefined);

interface ThemeProviderProps {
  children: React.ReactNode;
  initialMode?: ThemeMode;
  /** Override system color scheme (e.g. from native Activity's current configuration) */
  systemColorSchemeOverride?: 'light' | 'dark';
}

export const ThemeProvider: React.FC<ThemeProviderProps> = ({
  children,
  initialMode = 'auto',
  systemColorSchemeOverride,
}) => {
  const rnColorScheme = (useColorScheme() ?? 'light') === 'dark' ? 'dark' : 'light';
  const systemColorScheme = systemColorSchemeOverride ?? rnColorScheme;
  const [themeMode, setThemeModeState] = useState<ThemeMode>(initialMode);
  const [theme, setTheme] = useState<Theme>(() => createTheme(initialMode, systemColorScheme));

  // 从存储加载主题模式
  useEffect(() => {
    void loadPersistedSettings();
  }, []);

  // themeMode 变化时通知原生层，让 PlatformColor / DynamicColorIOS / SwiftUI / Compose 联动
  useEffect(() => {
    Appearance.setColorScheme(themeMode === 'auto' ? 'unspecified' : themeMode);
  }, [themeMode]);

  // 监听 mode / system 变化重建主题
  useEffect(() => {
    setTheme(createTheme(themeMode, systemColorScheme));
  }, [themeMode, systemColorScheme]);

  const loadPersistedSettings = async () => {
    try {
      const savedMode = await AsyncStorage.getItem(THEME_STORAGE_KEY);
      if (savedMode && ['light', 'dark', 'auto'].includes(savedMode)) {
        setThemeModeState(savedMode as ThemeMode);
      }
    } catch (error) {
      console.warn('Failed to load theme settings:', error);
    }
  };

  const setThemeMode = useCallback(async (mode: ThemeMode) => {
    try {
      setThemeModeState(mode);
      await AsyncStorage.setItem(THEME_STORAGE_KEY, mode);
    } catch (error) {
      console.warn('Failed to save theme mode:', error);
    }
  }, []);

  const toggleTheme = useCallback(async () => {
    const newMode: ThemeMode = themeMode === 'light' ? 'dark' : 'light';
    await setThemeMode(newMode);
  }, [themeMode, setThemeMode]);

  const value: ThemeContextValue = {
    theme,
    themeMode,
    setThemeMode,
    toggleTheme,
  };

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
};

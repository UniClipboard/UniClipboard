/**
 * 主题上下文
 * 提供主题切换、source color 切换与访问功能
 */

import React, { createContext, useEffect, useState, useCallback } from 'react';
import { useColorScheme } from 'react-native';
import AsyncStorage from '@react-native-async-storage/async-storage';
import {
  createTheme,
  DEFAULT_PALETTE_ID,
  type Theme,
  type ThemeMode,
  type PaletteId,
} from '@/theme';

const THEME_STORAGE_KEY = '@syncclipboard:theme_mode';
const PALETTE_STORAGE_KEY = '@syncclipboard:palette_id';

const VALID_PALETTE_IDS: PaletteId[] = ['purple', 'indigo', 'teal', 'rose', 'amber'];

interface ThemeContextValue {
  theme: Theme;
  themeMode: ThemeMode;
  paletteId: PaletteId;
  setThemeMode: (mode: ThemeMode) => Promise<void>;
  setPaletteId: (id: PaletteId) => Promise<void>;
  toggleTheme: () => Promise<void>;
}

export const ThemeContext = createContext<ThemeContextValue | undefined>(undefined);

interface ThemeProviderProps {
  children: React.ReactNode;
  initialMode?: ThemeMode;
  initialPaletteId?: PaletteId;
  /** Override system color scheme (e.g. from native Activity's current configuration) */
  systemColorSchemeOverride?: 'light' | 'dark';
}

export const ThemeProvider: React.FC<ThemeProviderProps> = ({
  children,
  initialMode = 'auto',
  initialPaletteId = DEFAULT_PALETTE_ID,
  systemColorSchemeOverride,
}) => {
  const rnColorScheme = (useColorScheme() ?? 'light') === 'dark' ? 'dark' : 'light';
  const systemColorScheme = systemColorSchemeOverride ?? rnColorScheme;
  const [themeMode, setThemeModeState] = useState<ThemeMode>(initialMode);
  const [paletteId, setPaletteIdState] = useState<PaletteId>(initialPaletteId);
  const [theme, setTheme] = useState<Theme>(() =>
    createTheme(initialMode, systemColorScheme, initialPaletteId)
  );

  // 从存储加载主题与 palette
  useEffect(() => {
    void loadPersistedSettings();
  }, []);

  // 监听 mode / palette / system 变化重建主题
  useEffect(() => {
    setTheme(createTheme(themeMode, systemColorScheme, paletteId));
  }, [themeMode, systemColorScheme, paletteId]);

  const loadPersistedSettings = async () => {
    try {
      const [savedMode, savedPalette] = await Promise.all([
        AsyncStorage.getItem(THEME_STORAGE_KEY),
        AsyncStorage.getItem(PALETTE_STORAGE_KEY),
      ]);
      if (savedMode && ['light', 'dark', 'auto'].includes(savedMode)) {
        setThemeModeState(savedMode as ThemeMode);
      }
      if (savedPalette && VALID_PALETTE_IDS.includes(savedPalette as PaletteId)) {
        setPaletteIdState(savedPalette as PaletteId);
      }
    } catch (error) {
      console.error('Failed to load theme settings:', error);
    }
  };

  const setThemeMode = useCallback(async (mode: ThemeMode) => {
    try {
      setThemeModeState(mode);
      await AsyncStorage.setItem(THEME_STORAGE_KEY, mode);
    } catch (error) {
      console.error('Failed to save theme mode:', error);
    }
  }, []);

  const setPaletteId = useCallback(async (id: PaletteId) => {
    try {
      setPaletteIdState(id);
      await AsyncStorage.setItem(PALETTE_STORAGE_KEY, id);
    } catch (error) {
      console.error('Failed to save palette id:', error);
    }
  }, []);

  const toggleTheme = useCallback(async () => {
    const newMode: ThemeMode = themeMode === 'light' ? 'dark' : 'light';
    await setThemeMode(newMode);
  }, [themeMode, setThemeMode]);

  const value: ThemeContextValue = {
    theme,
    themeMode,
    paletteId,
    setThemeMode,
    setPaletteId,
    toggleTheme,
  };

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
};

/**
 * ServiceRestartApp
 * 极简 RN 根组件，用于后台服务被系统重启后引导 JS 运行时启动。
 * 显示一个短暂的"✓ 服务已恢复"提示，0.5 秒后自动关闭。
 * 注册为 "serviceRestart"（独立于 "main" 入口）。
 */

import React, { useEffect, useState } from 'react';
import { StyleSheet, View, Text, BackHandler, StatusBar, Platform } from 'react-native';
import { useSettingsStore } from './stores';
import { initLogger } from './services/Logger';
import { getBackgroundServiceManager } from './services/BackgroundServiceManager';

interface ServiceRestartAppProps {
  systemTheme?: 'light' | 'dark';
}

export default function ServiceRestartApp({ systemTheme }: ServiceRestartAppProps) {
  const { loadConfig, isLoaded } = useSettingsStore();
  const [ready, setReady] = useState(false);
  const isDark = systemTheme === 'dark';

  useEffect(() => {
    initLogger();
  }, []);

  useEffect(() => {
    if (!isLoaded) {
      loadConfig();
    }
  }, [isLoaded, loadConfig]);

  // 启动所有后台服务（和主界面一致）
  useEffect(() => {
    if (!isLoaded || Platform.OS !== 'android') return;

    getBackgroundServiceManager()
      .start()
      .finally(() => setReady(true));
  }, [isLoaded]);

  // 自动关闭：短暂展示"服务已恢复"后退出，后台服务持续运行（JS 运行时由前台服务保持存活）
  useEffect(() => {
    if (!ready) return;
    const timer = setTimeout(() => {
      BackHandler.exitApp();
    }, 500);
    return () => clearTimeout(timer);
  }, [ready]);

  return (
    <View style={styles.container}>
      <StatusBar backgroundColor="transparent" translucent barStyle="light-content" />
      <View style={[styles.card, isDark ? styles.cardDark : styles.cardLight]}>
        <Text style={[styles.icon]}>✓</Text>
        <Text style={[styles.text, isDark ? styles.textDark : styles.textLight]}>服务已恢复</Text>
      </View>
    </View>
  );
}

const COLORS = {
  overlay: 'rgba(0, 0, 0, 0.3)',
  shadow: '#000',
  cardLight: '#ffffff',
  cardDark: '#2c2c2e',
  success: '#34c759',
  textLight: '#1c1c1e',
  textDark: '#f2f2f7',
};

const styles = StyleSheet.create({
  container: {
    flex: 1,
    justifyContent: 'center',
    alignItems: 'center',
    backgroundColor: COLORS.overlay,
  },
  card: {
    flexDirection: 'row',
    alignItems: 'center',
    paddingHorizontal: 24,
    paddingVertical: 16,
    borderRadius: 16,
    gap: 10,
    elevation: 8,
    shadowColor: COLORS.shadow,
    shadowOffset: { width: 0, height: 4 },
    shadowOpacity: 0.2,
    shadowRadius: 8,
  },
  cardLight: {
    backgroundColor: COLORS.cardLight,
  },
  cardDark: {
    backgroundColor: COLORS.cardDark,
  },
  icon: {
    fontSize: 22,
    color: COLORS.success,
    fontWeight: 'bold',
  },
  text: {
    fontSize: 16,
    fontWeight: '600',
  },
  textLight: {
    color: COLORS.textLight,
  },
  textDark: {
    color: COLORS.textDark,
  },
});

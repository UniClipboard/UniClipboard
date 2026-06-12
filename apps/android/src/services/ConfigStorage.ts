/**
 * Config Storage Service
 * 配置存储服务 - 管理应用配置和服务器配置
 */

import AsyncStorage from '@react-native-async-storage/async-storage';
import { AppConfig, DEFAULT_APP_CONFIG, STORAGE_KEYS } from '../types/storage';
import { ServerConfig } from '../types/api';
import { SyncMode } from '../types/sync';

/**
 * 配置存储服务
 */
export class ConfigStorage {
  private static instance: ConfigStorage | null = null;
  private config: AppConfig | null = null;
  private initialized = false;

  private constructor() {}

  /**
   * 获取单例实例
   */
  public static getInstance(): ConfigStorage {
    if (!ConfigStorage.instance) {
      ConfigStorage.instance = new ConfigStorage();
    }
    return ConfigStorage.instance;
  }

  /**
   * 初始化配置存储
   */
  public async initialize(): Promise<void> {
    if (this.initialized) {
      return;
    }

    try {
      await this.loadConfig();
      this.initialized = true;
    } catch (error) {
      console.error('[ConfigStorage] Failed to initialize:', error);
      // 使用默认配置
      this.config = { ...DEFAULT_APP_CONFIG };
      this.initialized = true;
    }
  }

  /**
   * 加载配置
   */
  private async loadConfig(): Promise<void> {
    const configJson = await AsyncStorage.getItem(STORAGE_KEYS.CONFIG);

    if (configJson) {
      const savedConfig = JSON.parse(configJson);
      this.config = { ...DEFAULT_APP_CONFIG, ...savedConfig };
    } else {
      this.config = { ...DEFAULT_APP_CONFIG };
      await this.saveConfig();
    }
  }

  /**
   * 保存配置
   */
  private async saveConfig(): Promise<void> {
    if (!this.config) {
      throw new Error('Config not initialized');
    }

    try {
      await AsyncStorage.setItem(STORAGE_KEYS.CONFIG, JSON.stringify(this.config));
    } catch (error) {
      console.error('[ConfigStorage] Failed to save config:', error);
      throw error;
    }
  }

  /**
   * 获取完整配置
   */
  public async getConfig(): Promise<AppConfig> {
    if (!this.initialized) {
      await this.initialize();
    }

    return { ...this.config! };
  }

  /**
   * 更新配置
   */
  public async updateConfig(updates: Partial<AppConfig>): Promise<void> {
    if (!this.initialized) {
      await this.initialize();
    }

    this.config = { ...this.config!, ...updates };
    await this.saveConfig();
  }

  /**
   * 重置配置为默认值
   */
  public async resetConfig(): Promise<void> {
    this.config = { ...DEFAULT_APP_CONFIG };
    await this.saveConfig();
  }

  // ========== 服务器配置管理 ==========

  /**
   * 获取所有服务器配置
   */
  public async getServers(): Promise<ServerConfig[]> {
    const config = await this.getConfig();
    return [...config.servers];
  }

  /**
   * 获取当前激活的服务器配置
   */
  public async getActiveServer(): Promise<ServerConfig | null> {
    const config = await this.getConfig();
    if (config.activeServerIndex >= 0 && config.activeServerIndex < config.servers.length) {
      return { ...config.servers[config.activeServerIndex] };
    }
    return null;
  }

  /**
   * 添加服务器配置
   */
  public async addServer(server: ServerConfig): Promise<number> {
    const config = await this.getConfig();
    config.servers.push(server);

    // 如果是第一个服务器，自动激活
    if (config.servers.length === 1) {
      config.activeServerIndex = 0;
    }

    await this.updateConfig(config);
    return config.servers.length - 1;
  }

  /**
   * 更新服务器配置
   */
  public async updateServer(index: number, updates: Partial<ServerConfig>): Promise<void> {
    const config = await this.getConfig();

    if (index < 0 || index >= config.servers.length) {
      throw new Error(`Invalid server index: ${index}`);
    }

    config.servers[index] = { ...config.servers[index], ...updates };
    await this.updateConfig(config);
  }

  /**
   * 删除服务器配置
   */
  public async deleteServer(index: number): Promise<void> {
    const config = await this.getConfig();

    if (index < 0 || index >= config.servers.length) {
      throw new Error(`Invalid server index: ${index}`);
    }

    config.servers.splice(index, 1);

    // 调整当前激活索引
    if (config.activeServerIndex === index) {
      config.activeServerIndex = config.servers.length > 0 ? 0 : -1;
    } else if (config.activeServerIndex > index) {
      config.activeServerIndex--;
    }

    await this.updateConfig(config);
  }

  /**
   * 设置激活的服务器
   */
  public async setActiveServer(index: number): Promise<void> {
    const config = await this.getConfig();

    if (index < -1 || index >= config.servers.length) {
      throw new Error(`Invalid server index: ${index}`);
    }

    config.activeServerIndex = index;
    await this.updateConfig(config);
  }

  // ========== 主题管理 ==========

  /**
   * 获取主题设置
   */
  public async getTheme(): Promise<'light' | 'dark' | 'auto'> {
    const config = await this.getConfig();
    return config.theme;
  }

  /**
   * 设置主题
   */
  public async setTheme(theme: 'light' | 'dark' | 'auto'): Promise<void> {
    await this.updateConfig({ theme });
  }

  // ========== 同步设置管理 ==========

  /**
   * 获取同步模式
   */
  public async getSyncMode(): Promise<string> {
    const config = await this.getConfig();
    return config.syncMode;
  }

  /**
   * 设置同步模式
   */
  public async setSyncMode(mode: string): Promise<void> {
    await this.updateConfig({ syncMode: mode as SyncMode });
  }

  /**
   * 获取同步间隔
   */
  public async getSyncInterval(): Promise<number> {
    const config = await this.getConfig();
    return config.syncInterval;
  }

  /**
   * 设置同步间隔
   */
  public async setSyncInterval(interval: number): Promise<void> {
    if (interval < 1000) {
      throw new Error('Sync interval must be at least 1000ms');
    }
    await this.updateConfig({ syncInterval: interval });
  }

  // ========== 通知设置管理 ==========

  /**
   * 是否启用通知
   */
  public async isNotificationsEnabled(): Promise<boolean> {
    const config = await this.getConfig();
    return config.enableNotifications;
  }

  /**
   * 设置通知开关
   */
  public async setNotificationsEnabled(enabled: boolean): Promise<void> {
    await this.updateConfig({ enableNotifications: enabled });
  }

  // ========== 导入/导出 ==========

  /**
   * 导出配置为 JSON
   */
  public async exportConfig(): Promise<string> {
    const config = await this.getConfig();
    return JSON.stringify(config, null, 2);
  }

  /**
   * 从 JSON 导入配置
   */
  public async importConfig(json: string): Promise<void> {
    try {
      const config = JSON.parse(json) as AppConfig;

      // 验证必需字段
      if (!config.servers || !Array.isArray(config.servers)) {
        throw new Error('Invalid config: missing servers array');
      }

      // 合并默认配置（确保所有字段都存在）
      this.config = { ...DEFAULT_APP_CONFIG, ...config };
      await this.saveConfig();
    } catch (error) {
      console.error('[ConfigStorage] Failed to import config:', error);
      throw new Error('Invalid config JSON');
    }
  }

  /**
   * 清空所有配置
   */
  public async clear(): Promise<void> {
    await AsyncStorage.removeItem(STORAGE_KEYS.CONFIG);
    this.config = { ...DEFAULT_APP_CONFIG };
    this.initialized = false;
  }
}

// 导出单例
export const configStorage = ConfigStorage.getInstance();

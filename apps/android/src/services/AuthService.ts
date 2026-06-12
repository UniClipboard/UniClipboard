/**
 * Authentication Service
 * Handles Basic Authentication for API requests
 */

import AsyncStorage from '@react-native-async-storage/async-storage';
import { ConfigurationError } from './errors';

const STORAGE_KEY = '@syncclipboard:credentials';

/**
 * 认证凭证
 */
export interface Credentials {
  username: string;
  password: string;
}

/**
 * 认证服务类
 */
export class AuthService {
  private credentials: Credentials | null = null;

  constructor(username?: string, password?: string) {
    if (username && password) {
      this.credentials = { username, password };
    }
  }

  /**
   * 设置认证凭证
   */
  setCredentials(username: string, password: string): void {
    this.credentials = { username, password };
  }

  /**
   * 获取认证凭证
   */
  getCredentials(): Credentials | null {
    return this.credentials;
  }

  /**
   * 生成 Basic Auth 头部
   */
  getAuthHeader(): string {
    if (!this.credentials) {
      throw new ConfigurationError('No credentials configured');
    }

    const { username, password } = this.credentials;

    // 使用 btoa 进行 Base64 编码
    // 注意：在 React Native 中，btoa 可能需要 polyfill
    const encoded = btoa(`${username}:${password}`);
    return `Basic ${encoded}`;
  }

  /**
   * 验证是否已配置认证
   */
  isConfigured(): boolean {
    return this.credentials !== null;
  }

  /**
   * 清除认证凭证
   */
  clearCredentials(): void {
    this.credentials = null;
  }

  /**
   * 保存凭证到本地存储
   */
  async saveToStorage(): Promise<void> {
    if (!this.credentials) {
      await AsyncStorage.removeItem(STORAGE_KEY);
      return;
    }

    try {
      await AsyncStorage.setItem(STORAGE_KEY, JSON.stringify(this.credentials));
    } catch (error) {
      console.error('Failed to save credentials:', error);
      throw new Error('Failed to save credentials to storage');
    }
  }

  /**
   * 从本地存储加载凭证
   */
  async loadFromStorage(): Promise<boolean> {
    try {
      const data = await AsyncStorage.getItem(STORAGE_KEY);
      if (data) {
        this.credentials = JSON.parse(data);
        return true;
      }
      return false;
    } catch (error) {
      console.error('Failed to load credentials:', error);
      return false;
    }
  }

  /**
   * 从本地存储删除凭证
   */
  async deleteFromStorage(): Promise<void> {
    try {
      await AsyncStorage.removeItem(STORAGE_KEY);
      this.credentials = null;
    } catch (error) {
      console.error('Failed to delete credentials:', error);
      throw new Error('Failed to delete credentials from storage');
    }
  }
}

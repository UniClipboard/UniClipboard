/**
 * Secure Storage Service
 * 安全存储服务 - 管理敏感信息（密码、令牌等）
 *
 * 注意：当前使用简单的 Base64 编码存储
 * 生产环境建议使用 expo-secure-store 或集成原生安全存储
 */

import AsyncStorage from '@react-native-async-storage/async-storage';

const SECURE_STORAGE_PREFIX = '@syncclipboard:secure:';

/**
 * 安全存储服务
 */
export class SecureStorage {
  private static instance: SecureStorage | null = null;

  private constructor() {}

  /**
   * 获取单例实例
   */
  public static getInstance(): SecureStorage {
    if (!SecureStorage.instance) {
      SecureStorage.instance = new SecureStorage();
    }
    return SecureStorage.instance;
  }

  /**
   * 简单的编码函数（用于混淆，非真正加密）
   * 生产环境应使用真正的加密算法
   */
  private encode(value: string): string {
    return Buffer.from(value).toString('base64');
  }

  /**
   * 简单的解码函数
   */
  private decode(value: string): string {
    return Buffer.from(value, 'base64').toString('utf-8');
  }

  /**
   * 存储安全数据
   */
  public async setItem(key: string, value: string): Promise<void> {
    try {
      const encoded = this.encode(value);
      await AsyncStorage.setItem(`${SECURE_STORAGE_PREFIX}${key}`, encoded);
    } catch (error) {
      console.error('[SecureStorage] Failed to set item:', error);
      throw new Error('Failed to store secure data');
    }
  }

  /**
   * 获取安全数据
   */
  public async getItem(key: string): Promise<string | null> {
    try {
      const encoded = await AsyncStorage.getItem(`${SECURE_STORAGE_PREFIX}${key}`);

      if (!encoded) {
        return null;
      }

      return this.decode(encoded);
    } catch (error) {
      console.error('[SecureStorage] Failed to get item:', error);
      return null;
    }
  }

  /**
   * 删除安全数据
   */
  public async removeItem(key: string): Promise<void> {
    try {
      await AsyncStorage.removeItem(`${SECURE_STORAGE_PREFIX}${key}`);
    } catch (error) {
      console.error('[SecureStorage] Failed to remove item:', error);
      throw new Error('Failed to remove secure data');
    }
  }

  /**
   * 检查安全数据是否存在
   */
  public async hasItem(key: string): Promise<boolean> {
    try {
      const value = await AsyncStorage.getItem(`${SECURE_STORAGE_PREFIX}${key}`);
      return value !== null;
    } catch (error) {
      console.error('[SecureStorage] Failed to check item:', error);
      return false;
    }
  }

  /**
   * 存储服务器凭据
   */
  public async setCredentials(
    serverUrl: string,
    username: string,
    password: string
  ): Promise<void> {
    const key = `credentials:${serverUrl}:${username}`;
    await this.setItem(key, password);
  }

  /**
   * 获取服务器凭据
   */
  public async getCredentials(serverUrl: string, username: string): Promise<string | null> {
    const key = `credentials:${serverUrl}:${username}`;
    return await this.getItem(key);
  }

  /**
   * 删除服务器凭据
   */
  public async removeCredentials(serverUrl: string, username: string): Promise<void> {
    const key = `credentials:${serverUrl}:${username}`;
    await this.removeItem(key);
  }

  /**
   * 存储 API 令牌
   */
  public async setToken(key: string, token: string): Promise<void> {
    await this.setItem(`token:${key}`, token);
  }

  /**
   * 获取 API 令牌
   */
  public async getToken(key: string): Promise<string | null> {
    return await this.getItem(`token:${key}`);
  }

  /**
   * 删除 API 令牌
   */
  public async removeToken(key: string): Promise<void> {
    await this.removeItem(`token:${key}`);
  }

  /**
   * 清空所有安全数据
   */
  public async clear(): Promise<void> {
    try {
      const keys = await AsyncStorage.getAllKeys();
      const secureKeys = keys.filter((key) => key.startsWith(SECURE_STORAGE_PREFIX));

      if (secureKeys.length > 0) {
        await AsyncStorage.multiRemove(secureKeys);
      }
    } catch (error) {
      console.error('[SecureStorage] Failed to clear:', error);
      throw new Error('Failed to clear secure data');
    }
  }

  /**
   * 获取所有安全存储的键
   */
  public async getAllKeys(): Promise<string[]> {
    try {
      const keys = await AsyncStorage.getAllKeys();
      return keys
        .filter((key) => key.startsWith(SECURE_STORAGE_PREFIX))
        .map((key) => key.replace(SECURE_STORAGE_PREFIX, ''));
    } catch (error) {
      console.error('[SecureStorage] Failed to get keys:', error);
      return [];
    }
  }
}

// 导出单例
export const secureStorage = SecureStorage.getInstance();
